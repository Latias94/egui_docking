//! Ordered callback mailbox for the fork-backed native host.

use std::collections::{BTreeMap, VecDeque};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use dockspace::runtime::{NativeStagingPaintRequest, NativeSurfaceBinding};
#[cfg(test)]
use eframe::NativeViewportCreateFailureKind;
use eframe::{
    NativeHostHandler, NativeHostWake, NativeOutputResult, NativeOutputToken, NativePhysicalRect,
    NativeViewportCreateAdmission, NativeViewportCreateAttempt, NativeViewportCreateFailure,
    NativeViewportRoster, NativeViewportVisibilityResult, NativeWindowEvent, NativeWindowSnapshot,
    egui::ViewportId,
};

use crate::error::NativeHostProtocolError;
use crate::event::NativeWindowEventRecord;
use crate::retirement::CommittedRetirement;
use crate::viewport_callback::{NativeViewportCreateFailureRecord, NativeViewportVisibilityRecord};
use crate::viewport_map::NativeViewportMap;
#[cfg(test)]
use crate::window_snapshot::CompiledWindowObservation;
use crate::work_area::FrozenWorkAreaRoster;

#[derive(Debug, Clone)]
pub(crate) enum HostRecord {
    WindowEvent(NativeWindowEventRecord),
    ViewportRoster(NativeViewportRosterRecord),
    ViewportCreateFailed(NativeViewportCreateFailureRecord),
    ViewportVisibility(NativeViewportVisibilityRecord),
    ViewportCreated(NativeViewportCreatedRecord),
    StagingPainted(NativeStagingPaintRecord),
    Output {
        result: NativeOutputResult,
        submitted: bool,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct NativeViewportCreatedRecord {
    token: NativeOutputToken,
    binding: NativeSurfaceBinding,
}

impl NativeViewportCreatedRecord {
    const fn new(token: NativeOutputToken, binding: NativeSurfaceBinding) -> Self {
        Self { token, binding }
    }

    pub(crate) const fn token(self) -> NativeOutputToken {
        self.token
    }

    pub(crate) const fn binding(self) -> NativeSurfaceBinding {
        self.binding
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct NativeStagingPaintRecord {
    token: NativeOutputToken,
    request: NativeStagingPaintRequest,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DeferredViewportPaint {
    Created,
    Staging(NativeStagingPaintRequest),
    Semantic(NativeSurfaceBinding),
    Waiting,
}

impl NativeStagingPaintRecord {
    const fn new(token: NativeOutputToken, request: NativeStagingPaintRequest) -> Self {
        Self { token, request }
    }

    pub(crate) const fn token(self) -> NativeOutputToken {
        self.token
    }

    pub(crate) const fn request(self) -> NativeStagingPaintRequest {
        self.request
    }
}

/// Frozen dockspace observations captured from one complete root-window roster.
#[derive(Debug, Clone)]
pub(crate) struct NativeViewportRosterRecord {
    observations: Vec<FrozenWindowObservation>,
    work_areas: Result<FrozenWorkAreaRoster, NativeHostProtocolError>,
    #[cfg(test)]
    compiled_override: Option<Result<Vec<CompiledWindowObservation>, NativeHostProtocolError>>,
}

/// One callback-time window snapshot paired with its exact dockspace binding.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct FrozenWindowObservation {
    binding: NativeSurfaceBinding,
    snapshot: NativeWindowSnapshot,
}

impl FrozenWindowObservation {
    const fn new(binding: NativeSurfaceBinding, snapshot: NativeWindowSnapshot) -> Self {
        Self { binding, snapshot }
    }

    pub(crate) const fn binding(self) -> NativeSurfaceBinding {
        self.binding
    }

    pub(crate) const fn snapshot(self) -> NativeWindowSnapshot {
        self.snapshot
    }
}

impl NativeViewportRosterRecord {
    fn capture(roster: NativeViewportRoster<'_>, viewports: &NativeViewportMap) -> Self {
        let mut observations = Vec::new();
        for record in roster.records() {
            let Some(binding) =
                viewports.binding_for_roster(record.viewport_id(), record.window_id())
            else {
                continue;
            };
            observations.push(FrozenWindowObservation::new(binding, record.window()));
        }
        Self {
            observations,
            work_areas: FrozenWorkAreaRoster::capture(roster.work_areas()),
            #[cfg(test)]
            compiled_override: None,
        }
    }

    pub(crate) fn observations(&self) -> &[FrozenWindowObservation] {
        &self.observations
    }

    pub(crate) fn work_areas(&self) -> Result<FrozenWorkAreaRoster, NativeHostProtocolError> {
        self.work_areas.clone()
    }

    #[cfg(test)]
    pub(crate) fn compiled_override(
        &self,
    ) -> Option<Result<Vec<CompiledWindowObservation>, NativeHostProtocolError>> {
        self.compiled_override.clone()
    }

    #[cfg(test)]
    pub(crate) fn for_test(
        observations: impl IntoIterator<Item = CompiledWindowObservation>,
        facts_valid: bool,
    ) -> Self {
        Self {
            observations: Vec::new(),
            work_areas: Ok(FrozenWorkAreaRoster::Unknown),
            compiled_override: Some(if facts_valid {
                Ok(observations.into_iter().collect())
            } else {
                Err(NativeHostProtocolError::InvalidWindowSnapshot)
            }),
        }
    }

    #[cfg(test)]
    pub(crate) fn for_test_with_work_areas(
        observations: impl IntoIterator<Item = CompiledWindowObservation>,
        work_areas: FrozenWorkAreaRoster,
    ) -> Self {
        Self {
            observations: Vec::new(),
            work_areas: Ok(work_areas),
            compiled_override: Some(Ok(observations.into_iter().collect())),
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct OutputReservation {
    binding: Option<NativeSurfaceBinding>,
    window: Option<NativeWindowSnapshot>,
    terminal_recorded: bool,
    abandoned: bool,
}

impl OutputReservation {
    pub(crate) const fn unbound(window: NativeWindowSnapshot) -> Self {
        Self {
            binding: None,
            window: Some(window),
            terminal_recorded: false,
            abandoned: false,
        }
    }

    pub(crate) const fn bound(binding: NativeSurfaceBinding, window: NativeWindowSnapshot) -> Self {
        Self {
            binding: Some(binding),
            window: Some(window),
            terminal_recorded: false,
            abandoned: false,
        }
    }

    pub(crate) fn attach(&mut self, binding: NativeSurfaceBinding) -> bool {
        if let Some(existing) = self.binding {
            return existing == binding;
        }
        self.binding = Some(binding);
        true
    }

    pub(crate) const fn binding(self) -> Option<NativeSurfaceBinding> {
        self.binding
    }

    pub(crate) const fn window(self) -> Option<NativeWindowSnapshot> {
        self.window
    }

    #[cfg(test)]
    pub(crate) const fn unbound_for_test() -> Self {
        Self {
            binding: None,
            window: None,
            terminal_recorded: false,
            abandoned: false,
        }
    }

    #[cfg(test)]
    const fn bound_for_test(binding: NativeSurfaceBinding) -> Self {
        Self {
            binding: Some(binding),
            window: None,
            terminal_recorded: false,
            abandoned: false,
        }
    }

    fn abandon(&mut self) -> bool {
        if self.abandoned {
            return false;
        }
        self.abandoned = true;
        true
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct NativeCreateReservation {
    binding: NativeSurfaceBinding,
    rect: NativePhysicalRect,
}

#[cfg(test)]
mod tests {
    use dockspace::model::{
        DockspaceLayout, DockspaceNode, DockspaceRootLayout, DockspaceSurfaceLayout, ItemId,
        RootId, SurfaceId,
    };
    use dockspace::policy::DockPolicy;
    use dockspace::runtime::DockspaceSession;
    use winit::event::WindowEvent;
    use winit::window::WindowId;

    use super::*;

    fn bindings() -> (NativeSurfaceBinding, NativeSurfaceBinding) {
        let layout = DockspaceLayout::new([
            DockspaceSurfaceLayout::new(
                SurfaceId::new(1),
                DockspaceRootLayout::new(
                    RootId::new(1),
                    DockspaceNode::central_tabs([ItemId::new(1)]),
                ),
            ),
            DockspaceSurfaceLayout::new(
                SurfaceId::new(2),
                DockspaceRootLayout::new(
                    RootId::new(2),
                    DockspaceNode::central_tabs([ItemId::new(2)]),
                ),
            ),
        ])
        .expect("test layout validates");
        let mut session = DockspaceSession::from_layout(layout, DockPolicy::default())
            .expect("test session initializes");
        session
            .enable_managed_native_host(dockspace::runtime::NativePointerRoster::Exact(Vec::new()))
            .expect("native host enrolls");
        session
            .register_native_root(
                SurfaceId::new(1),
                dockspace::runtime::HostWindowToken::new(1),
            )
            .expect("first root registers");
        session
            .register_native_root(
                SurfaceId::new(2),
                dockspace::runtime::HostWindowToken::new(2),
            )
            .expect("second root registers");
        let mut frame = session
            .begin_host_frame()
            .expect("registration frame begins");
        frame
            .complete_unpainted_surfaces(dockspace::runtime::SurfaceUnavailableReason::Deferred)
            .expect("registration surfaces settle");
        let report = frame.commit().expect("registration frame commits");
        let mut found = report.inputs().iter().filter_map(|input| match input {
            dockspace::runtime::HostInputOutcome::NativeSurfaceRegistered { binding } => {
                Some(*binding)
            }
            _ => None,
        });
        (
            found.next().expect("first binding exists"),
            found.next().expect("second binding exists"),
        )
    }

    #[test]
    fn output_reservation_keeps_the_binding_seen_at_begin() {
        let (first, second) = bindings();
        let mut reservation = OutputReservation::bound_for_test(first);

        assert!(!reservation.attach(second));
        assert_eq!(reservation.binding(), Some(first));
        assert!(reservation.window().is_none());
    }

    #[test]
    fn output_ordinals_must_form_one_contiguous_sequence() {
        assert!(is_next_output_ordinal(0, 1));
        assert!(is_next_output_ordinal(1, 2));
        assert!(!is_next_output_ordinal(1, 1));
        assert!(!is_next_output_ordinal(1, 3));
        assert!(!is_next_output_ordinal(u64::MAX, 1));
    }

    #[test]
    fn freeze_preserves_owned_mailbox_prefix_and_rejects_new_callbacks() {
        let (binding, _) = bindings();
        let viewport = ViewportId::ROOT;
        let rect = NativePhysicalRect::new(10, 20, 800, 600);
        let pending = NativeWindowEventRecord::for_test(
            1,
            WindowId::from(11),
            Some(viewport),
            Some(binding),
            WindowEvent::Focused(true),
        );
        let mut records = HostRecords::active();
        records
            .journal
            .push_back(HostRecord::WindowEvent(pending.clone()));
        records
            .create_reservations
            .insert(viewport, NativeCreateReservation { binding, rect });
        records.hidden_render_bindings.insert(viewport, binding);

        records.freeze();

        assert_eq!(records.mode, HostIngressMode::Frozen);
        assert!(matches!(
            records.journal.front(),
            Some(HostRecord::WindowEvent(event)) if *event == pending
        ));
        assert_eq!(
            records.create_reservations.get(&viewport),
            Some(&NativeCreateReservation { binding, rect })
        );
        assert_eq!(
            records.hidden_render_bindings.get(&viewport),
            Some(&binding)
        );
        let accepted =
            records.record_viewport_create_failure(NativeViewportCreateFailureRecord::new(
                viewport,
                binding,
                NativeViewportCreateFailureKind::WindowUnavailable,
            ));
        assert!(!accepted);
    }

    #[test]
    fn quarantine_accepts_only_terminal_callbacks() {
        let (binding, _) = bindings();
        let viewport = ViewportId::ROOT;
        let window = WindowId::from(11);
        let mut records = HostRecords::active();

        records.quarantine_after_fatal(None);

        assert_eq!(records.mode, HostIngressMode::Quarantined);
        assert!(records.accepts_window_event(&WindowEvent::Destroyed));
        assert!(!records.accepts_window_event(&WindowEvent::Focused(true)));
        assert!(
            records.record_viewport_create_failure(NativeViewportCreateFailureRecord::new(
                viewport,
                binding,
                NativeViewportCreateFailureKind::WindowUnavailable,
            ))
        );
        assert!(
            records.record_viewport_visibility(NativeViewportVisibilityRecord::for_test(
                viewport,
                window,
                binding,
                false,
                eframe::NativeViewportVisibilityStatus::Dispatched,
            ))
        );

        records.freeze();

        assert_eq!(records.mode, HostIngressMode::Frozen);
        assert!(!records.accepts_window_event(&WindowEvent::Destroyed));
        assert!(
            !records.record_viewport_create_failure(NativeViewportCreateFailureRecord::new(
                viewport,
                binding,
                NativeViewportCreateFailureKind::WindowUnavailable,
            ))
        );
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OutputRecordDisposition {
    Recorded,
    Ignored,
    ProtocolViolation,
    Inactive,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HostIngressMode {
    Active,
    Quarantined,
    Frozen,
}

impl HostIngressMode {
    const fn accepts_new_work(self) -> bool {
        matches!(self, Self::Active)
    }

    const fn accepts_terminal_callbacks(self) -> bool {
        !matches!(self, Self::Frozen)
    }

    fn accepts_window_event(self, event: &winit::event::WindowEvent) -> bool {
        matches!(self, Self::Active)
            || matches!(self, Self::Quarantined)
                && matches!(event, winit::event::WindowEvent::Destroyed)
    }
}

#[derive(Debug)]
struct HostRecords {
    mode: HostIngressMode,
    journal: VecDeque<HostRecord>,
    output_reservations: BTreeMap<NativeOutputToken, OutputReservation>,
    create_reservations: BTreeMap<ViewportId, NativeCreateReservation>,
    hidden_render_bindings: BTreeMap<ViewportId, NativeSurfaceBinding>,
    staging_requests: BTreeMap<ViewportId, NativeStagingPaintRequest>,
    output_context: Option<NativeOutputToken>,
    last_output_ordinal: u64,
    output_order_invalid: bool,
    event_boundary_pending: bool,
}

impl HostRecords {
    fn active() -> Self {
        Self {
            mode: HostIngressMode::Active,
            journal: VecDeque::new(),
            output_reservations: BTreeMap::new(),
            create_reservations: BTreeMap::new(),
            hidden_render_bindings: BTreeMap::new(),
            staging_requests: BTreeMap::new(),
            output_context: None,
            last_output_ordinal: 0,
            output_order_invalid: false,
            event_boundary_pending: false,
        }
    }

    fn reserve_output(
        &mut self,
        token: NativeOutputToken,
        binding: Option<NativeSurfaceBinding>,
        window: NativeWindowSnapshot,
    ) -> bool {
        if matches!(self.mode, HostIngressMode::Frozen)
            || self.output_reservations.contains_key(&token)
        {
            return false;
        }
        let mut reservation = binding.map_or_else(
            || OutputReservation::unbound(window),
            |binding| OutputReservation::bound(binding, window),
        );
        if matches!(self.mode, HostIngressMode::Quarantined) {
            reservation.abandon();
        }
        self.output_reservations.insert(token, reservation);
        true
    }

    fn attach_output_binding(
        &mut self,
        token: NativeOutputToken,
        binding: NativeSurfaceBinding,
    ) -> bool {
        let Some(reservation) = self.output_reservations.get_mut(&token) else {
            return false;
        };
        reservation.attach(binding)
    }

    fn record_output(&mut self, result: NativeOutputResult) -> OutputRecordDisposition {
        if !self.mode.accepts_terminal_callbacks() {
            return OutputRecordDisposition::Inactive;
        }
        let Some(reservation) = self.output_reservations.get(&result.token()) else {
            self.output_order_invalid = true;
            return OutputRecordDisposition::ProtocolViolation;
        };
        if reservation.terminal_recorded || !self.accept_output_order(result) {
            self.output_order_invalid = true;
            return OutputRecordDisposition::ProtocolViolation;
        }
        let reservation = self
            .output_reservations
            .get_mut(&result.token())
            .expect("validated output reservation remains present");
        reservation.terminal_recorded = true;
        if reservation.abandoned {
            self.output_reservations.remove(&result.token());
            return OutputRecordDisposition::Ignored;
        }
        self.journal.push_back(HostRecord::Output {
            result,
            submitted: false,
        });
        OutputRecordDisposition::Recorded
    }

    fn accept_output_order(&mut self, result: NativeOutputResult) -> bool {
        match self.output_context {
            Some(context) if !context.same_context(result.token()) => return false,
            Some(_) => {}
            None => self.output_context = Some(result.token()),
        }
        let ordinal = result.ordinal().get();
        if !is_next_output_ordinal(self.last_output_ordinal, ordinal) {
            return false;
        }
        self.last_output_ordinal = ordinal;
        true
    }

    fn record_viewport_create_failure(
        &mut self,
        failure: NativeViewportCreateFailureRecord,
    ) -> bool {
        if !self.mode.accepts_terminal_callbacks() {
            return false;
        }
        if self.journal.iter().any(
            |record| matches!(record, HostRecord::ViewportCreateFailed(existing) if *existing == failure),
        ) {
            return true;
        }
        self.journal
            .push_back(HostRecord::ViewportCreateFailed(failure));
        true
    }

    fn record_viewport_visibility(&mut self, record: NativeViewportVisibilityRecord) -> bool {
        if !self.mode.accepts_terminal_callbacks() {
            return false;
        }
        self.journal
            .push_back(HostRecord::ViewportVisibility(record));
        true
    }

    fn record_viewport_created(&mut self, record: NativeViewportCreatedRecord) -> bool {
        if !self.mode.accepts_new_work() {
            return false;
        }
        if self.journal.iter().any(|queued| {
            matches!(
                queued,
                HostRecord::ViewportCreated(existing)
                    if existing.binding == record.binding
            )
        }) {
            return false;
        }
        self.journal.push_back(HostRecord::ViewportCreated(record));
        true
    }

    fn record_staging_painted(&mut self, record: NativeStagingPaintRecord) -> bool {
        if !self.mode.accepts_new_work()
            || self.staging_requests.get(&record.token.viewport_id()) != Some(&record.request)
        {
            return false;
        }
        let Some(reservation) = self.output_reservations.get(&record.token) else {
            return false;
        };
        if reservation.binding() != Some(record.request.binding()) {
            return false;
        }
        self.staging_requests.remove(&record.token.viewport_id());
        self.journal.push_back(HostRecord::StagingPainted(record));
        true
    }

    fn abandon_output(&mut self, token: NativeOutputToken) -> bool {
        if self.journal.iter().any(|record| {
            matches!(
                record,
                HostRecord::Output {
                    result,
                    submitted: true,
                } if result.token() == token
            )
        }) {
            return false;
        }
        let previous_len = self.journal.len();
        self.journal.retain(|record| {
            !matches!(record, HostRecord::Output { result, .. } if result.token() == token)
        });
        if self.journal.len() != previous_len {
            self.output_reservations.remove(&token);
            return true;
        }
        let Some(reservation) = self.output_reservations.get_mut(&token) else {
            return false;
        };
        reservation.abandon()
    }

    fn freeze(&mut self) {
        self.mode = HostIngressMode::Frozen;
    }

    fn quarantine_after_fatal(&mut self, token: Option<NativeOutputToken>) -> bool {
        if !matches!(self.mode, HostIngressMode::Frozen) {
            self.mode = HostIngressMode::Quarantined;
        }
        token.is_some_and(|token| self.abandon_output(token))
    }

    fn accepts_window_event(&self, event: &winit::event::WindowEvent) -> bool {
        self.mode.accepts_window_event(event)
    }

    fn retire_deferred_binding(
        &mut self,
        viewport: ViewportId,
        binding: NativeSurfaceBinding,
    ) -> Vec<NativeOutputToken> {
        if self
            .create_reservations
            .get(&viewport)
            .is_some_and(|reservation| reservation.binding == binding)
        {
            self.create_reservations.remove(&viewport);
        }
        if self.hidden_render_bindings.get(&viewport) == Some(&binding) {
            self.hidden_render_bindings.remove(&viewport);
        }
        if self
            .staging_requests
            .get(&viewport)
            .is_some_and(|request| request.binding() == binding)
        {
            self.staging_requests.remove(&viewport);
        }
        let mut abandoned = Vec::new();
        for (token, reservation) in &mut self.output_reservations {
            if reservation.binding() == Some(binding)
                && !reservation.terminal_recorded
                && reservation.abandon()
            {
                abandoned.push(*token);
            }
        }
        abandoned
    }

    fn references_binding(&self, binding: NativeSurfaceBinding) -> bool {
        self.journal.iter().any(|record| match record {
            HostRecord::WindowEvent(event) => event.references_binding(binding),
            HostRecord::ViewportRoster(roster) => roster
                .observations()
                .iter()
                .any(|observation| observation.binding() == binding),
            HostRecord::ViewportCreateFailed(failure) => failure.binding() == binding,
            HostRecord::ViewportVisibility(visibility) => visibility.binding() == binding,
            HostRecord::ViewportCreated(created) => created.binding() == binding,
            HostRecord::StagingPainted(staging) => staging.request().binding() == binding,
            HostRecord::Output { .. } => false,
        }) || self
            .output_reservations
            .values()
            .any(|reservation| reservation.binding() == Some(binding))
            || self
                .create_reservations
                .values()
                .any(|reservation| reservation.binding == binding)
            || self
                .hidden_render_bindings
                .values()
                .any(|current| *current == binding)
            || self
                .staging_requests
                .values()
                .any(|request| request.binding() == binding)
    }
}

const fn is_next_output_ordinal(previous: u64, current: u64) -> bool {
    matches!(previous.checked_add(1), Some(expected) if expected == current)
}

#[derive(Debug)]
pub(crate) struct NativeHostBridge {
    records: Mutex<HostRecords>,
    viewports: Arc<Mutex<NativeViewportMap>>,
}

#[must_use = "prepared route retirements must be committed or dropped"]
pub(crate) struct PreparedRouteRetirements {
    bridge: Arc<NativeHostBridge>,
    routes: Vec<CommittedRetirement>,
    active: bool,
}

impl PreparedRouteRetirements {
    pub(crate) fn commit(mut self) -> Vec<NativeOutputToken> {
        let mut viewports = self.bridge.lock_viewports();
        let mut records = self.bridge.lock();
        let mut abandoned = Vec::new();
        for retirement in &self.routes {
            abandoned.extend(
                records.retire_deferred_binding(retirement.viewport(), retirement.binding()),
            );
        }
        viewports.commit_retirements(&self.routes);
        self.active = false;
        abandoned
    }
}

impl Drop for PreparedRouteRetirements {
    fn drop(&mut self) {
        if !self.active {
            return;
        }
        self.bridge.lock_viewports().abort_retirements(&self.routes);
    }
}

impl NativeHostBridge {
    pub(crate) fn new(viewports: Arc<Mutex<NativeViewportMap>>) -> Self {
        Self {
            records: Mutex::new(HostRecords::active()),
            viewports,
        }
    }

    fn lock(&self) -> MutexGuard<'_, HostRecords> {
        self.records.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn lock_viewports(&self) -> MutexGuard<'_, NativeViewportMap> {
        self.viewports
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }

    pub(crate) fn front_event(&self) -> Option<NativeWindowEventRecord> {
        match self.lock().journal.front() {
            Some(HostRecord::WindowEvent(event)) => Some(event.clone()),
            Some(
                HostRecord::ViewportRoster(_)
                | HostRecord::ViewportCreateFailed(_)
                | HostRecord::ViewportVisibility(_)
                | HostRecord::ViewportCreated(_)
                | HostRecord::StagingPainted(_)
                | HostRecord::Output { .. },
            )
            | None => None,
        }
    }

    pub(crate) fn front_viewport_roster(&self) -> Option<NativeViewportRosterRecord> {
        match self.lock().journal.front() {
            Some(HostRecord::ViewportRoster(roster)) => Some(roster.clone()),
            Some(
                HostRecord::WindowEvent(_)
                | HostRecord::ViewportCreated(_)
                | HostRecord::StagingPainted(_)
                | HostRecord::ViewportCreateFailed(_)
                | HostRecord::ViewportVisibility(_)
                | HostRecord::Output { .. },
            )
            | None => None,
        }
    }

    pub(crate) fn front_viewport_created(&self) -> Option<NativeViewportCreatedRecord> {
        match self.lock().journal.front() {
            Some(HostRecord::ViewportCreated(created)) => Some(*created),
            Some(
                HostRecord::WindowEvent(_)
                | HostRecord::ViewportRoster(_)
                | HostRecord::ViewportCreateFailed(_)
                | HostRecord::ViewportVisibility(_)
                | HostRecord::StagingPainted(_)
                | HostRecord::Output { .. },
            )
            | None => None,
        }
    }

    pub(crate) fn front_staging_painted(&self) -> Option<NativeStagingPaintRecord> {
        match self.lock().journal.front() {
            Some(HostRecord::StagingPainted(record)) => Some(*record),
            Some(
                HostRecord::WindowEvent(_)
                | HostRecord::ViewportRoster(_)
                | HostRecord::ViewportCreateFailed(_)
                | HostRecord::ViewportVisibility(_)
                | HostRecord::ViewportCreated(_)
                | HostRecord::Output { .. },
            )
            | None => None,
        }
    }

    pub(crate) fn front_viewport_create_failure(
        &self,
    ) -> Option<NativeViewportCreateFailureRecord> {
        match self.lock().journal.front() {
            Some(HostRecord::ViewportCreateFailed(failure)) => Some(*failure),
            Some(
                HostRecord::WindowEvent(_)
                | HostRecord::ViewportRoster(_)
                | HostRecord::ViewportCreated(_)
                | HostRecord::ViewportVisibility(_)
                | HostRecord::StagingPainted(_)
                | HostRecord::Output { .. },
            )
            | None => None,
        }
    }

    pub(crate) fn has_pending_input(&self) -> bool {
        matches!(
            self.lock().journal.front(),
            Some(
                HostRecord::WindowEvent(_)
                    | HostRecord::ViewportRoster(_)
                    | HostRecord::ViewportCreateFailed(_)
                    | HostRecord::ViewportVisibility(_)
                    | HostRecord::ViewportCreated(_)
                    | HostRecord::StagingPainted(_)
            )
        )
    }

    pub(crate) fn acknowledge_event(&self, ordinal: u64) -> bool {
        let mut records = self.lock();
        let matches = matches!(
            records.journal.front(),
            Some(HostRecord::WindowEvent(event)) if event.ordinal() == ordinal
        );
        if matches {
            records.journal.pop_front();
            records.event_boundary_pending = true;
        }
        matches
    }

    pub(crate) fn acknowledge_viewport_create_failure(
        &self,
        expected: NativeViewportCreateFailureRecord,
    ) -> bool {
        let mut records = self.lock();
        let matches = matches!(
            records.journal.front(),
            Some(HostRecord::ViewportCreateFailed(failure)) if *failure == expected
        );
        if matches {
            records.journal.pop_front();
            records.event_boundary_pending = true;
        }
        matches
    }

    pub(crate) fn front_viewport_visibility(&self) -> Option<NativeViewportVisibilityRecord> {
        match self.lock().journal.front() {
            Some(HostRecord::ViewportVisibility(record)) => Some(*record),
            Some(
                HostRecord::WindowEvent(_)
                | HostRecord::ViewportRoster(_)
                | HostRecord::ViewportCreateFailed(_)
                | HostRecord::ViewportCreated(_)
                | HostRecord::StagingPainted(_)
                | HostRecord::Output { .. },
            )
            | None => None,
        }
    }

    pub(crate) fn acknowledge_viewport_visibility(
        &self,
        expected: NativeViewportVisibilityRecord,
    ) -> bool {
        let mut records = self.lock();
        let matches = matches!(
            records.journal.front(),
            Some(HostRecord::ViewportVisibility(record)) if *record == expected
        );
        if matches {
            records.journal.pop_front();
            records.event_boundary_pending = true;
        }
        matches
    }

    pub(crate) fn acknowledge_viewport_roster(&self) -> bool {
        let mut records = self.lock();
        if !matches!(records.journal.front(), Some(HostRecord::ViewportRoster(_))) {
            return false;
        }
        records.journal.pop_front();
        records.event_boundary_pending = true;
        true
    }

    pub(crate) fn acknowledge_viewport_created(
        &self,
        expected: NativeViewportCreatedRecord,
    ) -> bool {
        let mut records = self.lock();
        let matches = matches!(
            records.journal.front(),
            Some(HostRecord::ViewportCreated(created)) if *created == expected
        );
        if matches {
            records.journal.pop_front();
            records.event_boundary_pending = true;
        }
        matches
    }

    pub(crate) fn acknowledge_staging_painted(&self, expected: NativeStagingPaintRecord) -> bool {
        let mut records = self.lock();
        let matches = matches!(
            records.journal.front(),
            Some(HostRecord::StagingPainted(record)) if *record == expected
        );
        if matches {
            records.journal.pop_front();
            records.event_boundary_pending = true;
        }
        matches
    }

    pub(crate) fn output_prefix(&self) -> Vec<(NativeOutputResult, bool)> {
        let records = self.lock();
        if records.event_boundary_pending {
            return Vec::new();
        }
        let outputs: Vec<_> = records
            .journal
            .iter()
            .map_while(|record| match record {
                HostRecord::Output { result, submitted } => Some((*result, *submitted)),
                HostRecord::WindowEvent(_)
                | HostRecord::ViewportRoster(_)
                | HostRecord::ViewportCreateFailed(_)
                | HostRecord::ViewportVisibility(_)
                | HostRecord::ViewportCreated(_)
                | HostRecord::StagingPainted(_) => None,
            })
            .collect();

        // Preserve the single callback journal order. Output ordinals are
        // context-local generation identities, not a cross-viewport clock;
        // comparing them here would let one viewport's result overtake an
        // older event or another context's output.
        outputs
    }

    pub(crate) fn output_order_is_valid(&self) -> bool {
        !self.lock().output_order_invalid
    }

    /// Reserves the exact binding which may admit one deferred create attempt.
    pub(crate) fn reserve_create(
        &self,
        viewport: ViewportId,
        binding: NativeSurfaceBinding,
        rect: NativePhysicalRect,
    ) -> bool {
        let mut records = self.lock();
        if !records.mode.accepts_new_work() {
            return false;
        }
        match records.create_reservations.get(&viewport) {
            Some(current) => {
                current.binding == binding
                    && current.rect == rect
                    && records.hidden_render_bindings.get(&viewport) == Some(&binding)
            }
            None => {
                if records
                    .hidden_render_bindings
                    .get(&viewport)
                    .is_some_and(|current| *current != binding)
                {
                    return false;
                }
                records
                    .create_reservations
                    .insert(viewport, NativeCreateReservation { binding, rect });
                records.hidden_render_bindings.insert(viewport, binding);
                true
            }
        }
    }

    pub(crate) fn clear_create(&self, viewport: ViewportId, binding: NativeSurfaceBinding) -> bool {
        let mut records = self.lock();
        if records
            .create_reservations
            .get(&viewport)
            .map(|reservation| reservation.binding)
            != Some(binding)
        {
            return false;
        }
        records.create_reservations.remove(&viewport);
        true
    }

    pub(crate) fn clear_hidden_render(
        &self,
        viewport: ViewportId,
        binding: NativeSurfaceBinding,
    ) -> bool {
        let mut records = self.lock();
        if records.hidden_render_bindings.get(&viewport) != Some(&binding) {
            return false;
        }
        records.hidden_render_bindings.remove(&viewport);
        true
    }

    pub(crate) fn retire_deferred_binding(
        &self,
        viewport: ViewportId,
        binding: NativeSurfaceBinding,
    ) -> Vec<NativeOutputToken> {
        self.lock().retire_deferred_binding(viewport, binding)
    }

    pub(crate) fn prepare_route_retirements(
        self: &Arc<Self>,
        retirements: &[CommittedRetirement],
    ) -> Result<PreparedRouteRetirements, NativeSurfaceBinding> {
        self.lock_viewports().prepare_retirements(retirements)?;
        Ok(PreparedRouteRetirements {
            bridge: self.clone(),
            routes: retirements.to_vec(),
            active: true,
        })
    }

    pub(crate) fn references_binding(&self, binding: NativeSurfaceBinding) -> bool {
        self.lock().references_binding(binding)
    }

    fn hidden_render_enabled(&self, viewport: ViewportId) -> bool {
        let records = self.lock();
        records.mode.accepts_new_work() && records.hidden_render_bindings.contains_key(&viewport)
    }

    pub(crate) fn create_binding(&self, viewport: ViewportId) -> Option<NativeSurfaceBinding> {
        self.lock()
            .create_reservations
            .get(&viewport)
            .copied()
            .map(|reservation| reservation.binding)
    }

    pub(crate) fn replace_staging_requests(
        &self,
        requests: BTreeMap<ViewportId, NativeStagingPaintRequest>,
    ) {
        self.lock().staging_requests = requests;
    }

    pub(crate) fn record_deferred_viewport_paint(
        &self,
        token: NativeOutputToken,
    ) -> DeferredViewportPaint {
        let mut records = self.lock();
        let Some(reservation) = records.output_reservations.get(&token).copied() else {
            return DeferredViewportPaint::Waiting;
        };
        if let Some(create) = records
            .create_reservations
            .get(&token.viewport_id())
            .copied()
            && reservation.binding() == Some(create.binding)
        {
            let created = NativeViewportCreatedRecord::new(token, create.binding);
            let recorded = records.record_viewport_created(created);
            records
                .output_reservations
                .get_mut(&token)
                .expect("the deferred output reservation remains present")
                .abandon();
            return if recorded {
                DeferredViewportPaint::Created
            } else {
                DeferredViewportPaint::Waiting
            };
        }
        let Some(request) = records.staging_requests.get(&token.viewport_id()).copied() else {
            let input_pending = matches!(
                records.journal.front(),
                Some(
                    HostRecord::WindowEvent(_)
                        | HostRecord::ViewportRoster(_)
                        | HostRecord::ViewportCreateFailed(_)
                        | HostRecord::ViewportVisibility(_)
                        | HostRecord::ViewportCreated(_)
                        | HostRecord::StagingPainted(_)
                )
            );
            if !input_pending && let Some(binding) = reservation.binding() {
                return DeferredViewportPaint::Semantic(binding);
            }
            records
                .output_reservations
                .get_mut(&token)
                .expect("the deferred output reservation remains present")
                .abandon();
            return DeferredViewportPaint::Waiting;
        };
        let record = NativeStagingPaintRecord::new(token, request);
        if records.record_staging_painted(record) {
            DeferredViewportPaint::Staging(request)
        } else {
            records
                .output_reservations
                .get_mut(&token)
                .expect("the deferred output reservation remains present")
                .abandon();
            DeferredViewportPaint::Waiting
        }
    }

    pub(crate) fn reserve_output(
        &self,
        token: NativeOutputToken,
        window: NativeWindowSnapshot,
        root_roster: Option<NativeViewportRoster<'_>>,
    ) -> bool {
        // Keep the route lock until the reservation is recorded. Retirement
        // takes the same route-then-records lock order, so it cannot report
        // quiescence between observing this callback and publishing its
        // exact binding reference.
        let viewports = self.lock_viewports();
        let binding = viewports.binding_for_output(
            token.viewport_id(),
            token.window_id(),
            token.create_attempt(),
        );
        let roster =
            root_roster.map(|roster| NativeViewportRosterRecord::capture(roster, &viewports));
        let mut records = self.lock();
        if !records.reserve_output(token, binding, window) {
            return false;
        }
        if let Some(roster) = roster {
            records
                .journal
                .push_back(HostRecord::ViewportRoster(roster));
        }
        true
    }

    pub(crate) fn output_reservation(&self, token: NativeOutputToken) -> Option<OutputReservation> {
        self.lock().output_reservations.get(&token).copied()
    }

    pub(crate) fn attach_output_binding(
        &self,
        token: NativeOutputToken,
        binding: NativeSurfaceBinding,
    ) -> bool {
        self.lock().attach_output_binding(token, binding)
    }

    pub(crate) fn mark_output_submitted(&self, token: NativeOutputToken) -> bool {
        let mut records = self.lock();
        let Some(HostRecord::Output { result, submitted }) = records.journal.iter_mut().find(
            |record| matches!(record, HostRecord::Output { result, .. } if result.token() == token),
        ) else {
            return false;
        };
        if result.token() != token {
            return false;
        }
        *submitted = true;
        true
    }

    pub(crate) fn abandon_output(&self, token: NativeOutputToken) -> bool {
        self.lock().abandon_output(token)
    }

    pub(crate) fn commit_frame_boundary(&self) {
        let mut records = self.lock();
        records.event_boundary_pending = false;
        while matches!(
            records.journal.front(),
            Some(HostRecord::Output {
                submitted: true,
                ..
            })
        ) {
            let Some(HostRecord::Output { result, .. }) = records.journal.pop_front() else {
                unreachable!("matched output record must still be at the journal head");
            };
            records.output_reservations.remove(&result.token());
        }
    }

    pub(crate) fn freeze(&self) {
        self.lock().freeze();
    }

    pub(crate) fn quarantine_after_fatal(&self, token: Option<NativeOutputToken>) -> bool {
        self.lock().quarantine_after_fatal(token)
    }

    fn admit_viewport_create_attempt(
        &self,
        attempt: NativeViewportCreateAttempt,
    ) -> NativeViewportCreateAdmission {
        let viewport = attempt.viewport_id();
        let mut viewports = self.lock_viewports();
        let records = self.lock();
        let Some(reservation) = records.create_reservations.get(&viewport).copied() else {
            return NativeViewportCreateAdmission::Defer;
        };
        if !records.mode.accepts_new_work()
            || records.hidden_render_bindings.get(&viewport) != Some(&reservation.binding)
            || !viewports.admit_create_attempt(viewport, reservation.binding, attempt)
        {
            return NativeViewportCreateAdmission::Defer;
        }
        NativeViewportCreateAdmission::Proceed {
            undecorated_outer_rect: Some(reservation.rect),
        }
    }

    fn record_eframe_viewport_create_failure(
        &self,
        failure: NativeViewportCreateFailure,
    ) -> NativeHostWake {
        let attempt = failure.create_attempt();
        let viewport = attempt.viewport_id();
        let viewports = self.lock_viewports();
        let Some(binding) = viewports.binding_for_create_attempt(attempt) else {
            return NativeHostWake::Wait;
        };
        let mut records = self.lock();
        if records
            .create_reservations
            .get(&viewport)
            .is_none_or(|reservation| reservation.binding != binding)
        {
            return NativeHostWake::Wait;
        }
        if !records.record_viewport_create_failure(NativeViewportCreateFailureRecord::new(
            viewport,
            binding,
            failure.kind(),
        )) {
            return NativeHostWake::Wait;
        }
        NativeHostWake::RepaintRoot
    }

    #[cfg(test)]
    fn record_reserved_viewport_create_failure_for_test(
        &self,
        viewport: ViewportId,
        kind: NativeViewportCreateFailureKind,
    ) -> NativeHostWake {
        let mut records = self.lock();
        let Some(reservation) = records.create_reservations.get(&viewport).copied() else {
            return NativeHostWake::Wait;
        };
        if !records.record_viewport_create_failure(NativeViewportCreateFailureRecord::new(
            viewport,
            reservation.binding,
            kind,
        )) {
            return NativeHostWake::Wait;
        }
        NativeHostWake::RepaintRoot
    }

    #[cfg(test)]
    pub(crate) fn record_viewport_create_failure_for_test(
        &self,
        viewport: eframe::egui::ViewportId,
    ) -> NativeHostWake {
        self.record_reserved_viewport_create_failure_for_test(
            viewport,
            NativeViewportCreateFailureKind::WindowUnavailable,
        )
    }

    #[cfg(test)]
    pub(crate) fn reserve_create_for_test(
        &self,
        viewport: eframe::egui::ViewportId,
        binding: NativeSurfaceBinding,
        rect: NativePhysicalRect,
    ) -> bool {
        self.reserve_create(viewport, binding, rect)
    }

    #[cfg(test)]
    pub(crate) fn push_record(&self, record: HostRecord) {
        self.lock().journal.push_back(record);
    }

    pub(crate) fn callback_boundary_pending(&self) -> bool {
        self.lock().event_boundary_pending
    }
}

impl NativeHostHandler for NativeHostBridge {
    fn begin_deferred_viewport_create(
        &self,
        attempt: NativeViewportCreateAttempt,
    ) -> NativeViewportCreateAdmission {
        self.admit_viewport_create_attempt(attempt)
    }

    fn render_hidden_deferred_viewport(&self, viewport_id: ViewportId) -> bool {
        self.hidden_render_enabled(viewport_id)
    }

    fn on_window_event(&self, event: NativeWindowEvent<'_>) {
        // Freeze the route and publish the event atomically with respect to
        // route retirement. Otherwise retirement could report quiescence
        // after route lookup but before this event became visible.
        let mut viewports = self.lock_viewports();
        let mut records = self.lock();
        if !records.accepts_window_event(event.event()) {
            return;
        }
        let record = NativeWindowEventRecord::from_eframe(event, &mut viewports);
        records.journal.push_back(HostRecord::WindowEvent(record));
    }

    fn on_output_begin(
        &self,
        token: NativeOutputToken,
        window: NativeWindowSnapshot,
        root_roster: Option<NativeViewportRoster<'_>>,
    ) {
        self.reserve_output(token, window, root_roster);
    }

    fn on_output(&self, result: NativeOutputResult) -> NativeHostWake {
        match self.lock().record_output(result) {
            OutputRecordDisposition::Recorded
            | OutputRecordDisposition::Ignored
            | OutputRecordDisposition::ProtocolViolation => NativeHostWake::RepaintRoot,
            OutputRecordDisposition::Inactive => NativeHostWake::Wait,
        }
    }

    fn on_viewport_create_failed(&self, failure: NativeViewportCreateFailure) -> NativeHostWake {
        self.record_eframe_viewport_create_failure(failure)
    }

    fn on_viewport_visibility(&self, result: NativeViewportVisibilityResult) -> NativeHostWake {
        let viewports = self.lock_viewports();
        let Some(binding) =
            viewports.binding_for_event(result.window_id(), Some(result.viewport_id()))
        else {
            return NativeHostWake::Wait;
        };
        let mut records = self.lock();
        if records.record_viewport_visibility(NativeViewportVisibilityRecord::from_eframe(
            result, binding,
        )) {
            NativeHostWake::RepaintRoot
        } else {
            NativeHostWake::Wait
        }
    }
}
