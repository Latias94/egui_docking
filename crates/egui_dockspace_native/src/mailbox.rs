//! Ordered callback mailbox for the fork-backed native host.

use std::collections::{BTreeMap, VecDeque};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use dockspace::runtime::NativeSurfaceBinding;
use eframe::{
    NativeHostHandler, NativeHostWake, NativeOutputResult, NativeOutputToken, NativePhysicalRect,
    NativeViewportCreateFailure, NativeWindowEvent, NativeWindowSnapshot, egui::ViewportId,
};

use crate::event::NativeWindowEventRecord;
use crate::viewport_map::NativeViewportMap;

#[derive(Debug, Clone)]
pub(crate) enum HostRecord {
    WindowEvent(NativeWindowEventRecord),
    ViewportCreateFailed(NativeViewportCreateFailureRecord),
    Output {
        result: NativeOutputResult,
        submitted: bool,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct NativeViewportCreateFailureRecord {
    viewport: eframe::egui::ViewportId,
    binding: NativeSurfaceBinding,
}

impl NativeViewportCreateFailureRecord {
    pub(crate) const fn new(
        viewport: eframe::egui::ViewportId,
        binding: NativeSurfaceBinding,
    ) -> Self {
        Self { viewport, binding }
    }

    pub(crate) const fn viewport(self) -> eframe::egui::ViewportId {
        self.viewport
    }

    pub(crate) const fn binding(self) -> NativeSurfaceBinding {
        self.binding
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

    fn abandon(&mut self) {
        self.abandoned = true;
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
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OutputRecordDisposition {
    Recorded,
    Ignored,
    ProtocolViolation,
    Inactive,
}

#[derive(Debug)]
struct HostRecords {
    active: bool,
    journal: VecDeque<HostRecord>,
    output_reservations: BTreeMap<NativeOutputToken, OutputReservation>,
    create_reservations: BTreeMap<ViewportId, NativeCreateReservation>,
    output_context: Option<NativeOutputToken>,
    last_output_ordinal: u64,
    output_order_invalid: bool,
    event_boundary_pending: bool,
}

impl HostRecords {
    fn active() -> Self {
        Self {
            active: true,
            journal: VecDeque::new(),
            output_reservations: BTreeMap::new(),
            create_reservations: BTreeMap::new(),
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
        if !self.active || self.output_reservations.contains_key(&token) {
            return false;
        }
        self.output_reservations.insert(
            token,
            binding.map_or_else(
                || OutputReservation::unbound(window),
                |binding| OutputReservation::bound(binding, window),
            ),
        );
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
        if !self.active {
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
        if !self.active {
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

    fn deactivate(&mut self) {
        self.active = false;
        self.journal.clear();
        self.output_reservations.clear();
        self.create_reservations.clear();
        self.output_context = None;
        self.last_output_ordinal = 0;
        self.output_order_invalid = false;
        self.event_boundary_pending = false;
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
            Some(HostRecord::ViewportCreateFailed(_) | HostRecord::Output { .. }) | None => None,
        }
    }

    pub(crate) fn front_viewport_create_failure(
        &self,
    ) -> Option<NativeViewportCreateFailureRecord> {
        match self.lock().journal.front() {
            Some(HostRecord::ViewportCreateFailed(failure)) => Some(*failure),
            Some(HostRecord::WindowEvent(_) | HostRecord::Output { .. }) | None => None,
        }
    }

    pub(crate) fn has_pending_input(&self) -> bool {
        matches!(
            self.lock().journal.front(),
            Some(HostRecord::WindowEvent(_) | HostRecord::ViewportCreateFailed(_))
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
                HostRecord::WindowEvent(_) | HostRecord::ViewportCreateFailed(_) => None,
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

    /// Reserves the exact binding which owns one deferred create callback.
    ///
    /// The fork callback reports only a viewport id on creation failure. The
    /// binding therefore has to be frozen before eframe starts the deferred
    /// create operation; looking it up after a replacement would be a
    /// time-of-check/time-of-use guess.
    pub(crate) fn reserve_create(
        &self,
        viewport: ViewportId,
        binding: NativeSurfaceBinding,
        rect: NativePhysicalRect,
    ) -> bool {
        let mut records = self.lock();
        match records.create_reservations.get(&viewport) {
            Some(current) => current.binding == binding && current.rect == rect,
            None => {
                records
                    .create_reservations
                    .insert(viewport, NativeCreateReservation { binding, rect });
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

    pub(crate) fn create_binding(&self, viewport: ViewportId) -> Option<NativeSurfaceBinding> {
        self.lock()
            .create_reservations
            .get(&viewport)
            .copied()
            .map(|reservation| reservation.binding)
    }

    pub(crate) fn create_rect(&self, viewport: ViewportId) -> Option<NativePhysicalRect> {
        self.lock()
            .create_reservations
            .get(&viewport)
            .map(|reservation| reservation.rect)
    }

    pub(crate) fn reserve_output(
        &self,
        token: NativeOutputToken,
        window: NativeWindowSnapshot,
    ) -> bool {
        let binding = self
            .lock_viewports()
            .binding_for_output(token.viewport_id(), token.window_id());
        self.lock().reserve_output(token, binding, window)
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
        let mut records = self.lock();
        if records.journal.iter().any(|record| {
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
        let previous_len = records.journal.len();
        records.journal.retain(|record| {
            !matches!(record, HostRecord::Output { result, .. } if result.token() == token)
        });
        if records.journal.len() != previous_len {
            records.output_reservations.remove(&token);
            return true;
        }
        let Some(reservation) = records.output_reservations.get_mut(&token) else {
            return false;
        };
        reservation.abandon();
        true
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

    pub(crate) fn deactivate(&self) {
        self.lock().deactivate();
    }

    fn record_viewport_create_failure(&self, viewport: eframe::egui::ViewportId) -> NativeHostWake {
        let mut records = self.lock();
        let Some(reservation) = records.create_reservations.get(&viewport).copied() else {
            return NativeHostWake::Wait;
        };
        if !records.record_viewport_create_failure(NativeViewportCreateFailureRecord::new(
            viewport,
            reservation.binding,
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
        self.record_viewport_create_failure(viewport)
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

    #[cfg(test)]
    pub(crate) fn event_boundary_pending(&self) -> bool {
        self.lock().event_boundary_pending
    }
}

impl NativeHostHandler for NativeHostBridge {
    fn deferred_undecorated_outer_rect(
        &self,
        viewport_id: ViewportId,
    ) -> Option<NativePhysicalRect> {
        self.create_rect(viewport_id)
    }

    fn on_window_event(&self, event: NativeWindowEvent<'_>) {
        let record = NativeWindowEventRecord::from_eframe(event, &self.lock_viewports());
        let mut records = self.lock();
        if records.active {
            records.journal.push_back(HostRecord::WindowEvent(record));
        }
    }

    fn on_output_begin(&self, token: NativeOutputToken, window: NativeWindowSnapshot) {
        self.reserve_output(token, window);
    }

    fn on_output(&self, result: NativeOutputResult) -> NativeHostWake {
        match self.lock().record_output(result) {
            OutputRecordDisposition::Recorded | OutputRecordDisposition::ProtocolViolation => {
                NativeHostWake::RepaintRoot
            }
            OutputRecordDisposition::Ignored | OutputRecordDisposition::Inactive => {
                NativeHostWake::Wait
            }
        }
    }

    fn on_viewport_create_failed(&self, failure: NativeViewportCreateFailure) -> NativeHostWake {
        self.record_viewport_create_failure(failure.viewport_id())
    }
}
