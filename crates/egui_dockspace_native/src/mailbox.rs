//! Ordered callback mailbox for the fork-backed native host.

use std::sync::{Arc, Mutex, MutexGuard, OnceLock, PoisonError};
use winit::window::WindowId;

use crate::coordinator::{
    NoFrameInvalidationReadiness, NoFrameTerminal, PendingNoFrameInvalidation,
    PreparedNoFrameInvalidations, no_frame_invalidation_readiness,
    output_terminal_requires_adapter_abandonment,
};
use dockspace::model::SurfaceId;
use dockspace::runtime::{
    DockspaceHostFrame, DockspaceSession, HostFrameReport, NativeCleanupObservation,
    NativeCloseEffectAcknowledgement, NativeCreateEffectAcknowledgement, NativeEffectResult,
    NativeInputBudget, NativeMonotonicClock, NativeSurfaceBinding, NativeWindowBinding,
    NativeWindowFacts, PreparedDockspaceHostFrameCommit,
};
#[cfg(test)]
use eframe::NativeViewportCreateFailureKind;
use eframe::{
    NativeDeviceRemoval, NativeGlobalFocusObservation, NativeHostAttachment, NativeHostHandler,
    NativeHostWake, NativeOutputAdmission, NativeOutputPassFinalization, NativeOutputPassFinalizer,
    NativeOutputResult, NativeOutputToken, NativePhysicalRect, NativeViewportCloseRequest,
    NativeViewportCreateAdmission, NativeViewportCreateAttempt, NativeViewportCreateFailure,
    NativeViewportFocusResult, NativeViewportPointerPassthroughResult, NativeViewportRoster,
    NativeViewportVisibilityResult, NativeWindowEvent, NativeWindowEventDisposition,
    NativeWindowSnapshot, egui::ViewportId,
};

use crate::close_control::{
    NativeCloseCallbackAdmission, NativeCloseCaptureAdmission, NativeCloseControl,
    NativeViewportCloseCancellationRecord,
};
use crate::error::{NativeHostProtocolError, NativeRuntimeError, NativeViewportBindingError};
use crate::event::{NativeDeviceRemovalRecord, NativeWindowEventClass, NativeWindowEventRecord};
use crate::focus_control::{NativeGlobalFocusRecord, NativeViewportFocusRecord};
use crate::input_control::NativePointerPassthroughRecord;
use crate::lifecycle_progress::NativeOutputBacklog;
use crate::pointer_capture::{NativePointerCaptureKey, NativePointerCaptureRecord};
use crate::pointer_event::NativePointerTranslator;
use crate::pointer_observation::{NativePointerObservationKey, NativePointerObservationRecord};
use crate::retirement::{
    CleanupResultRetentionError, CommittedRetirement, DestroyedObservation, NativeRetirementState,
};
use crate::viewport_callback::{NativeViewportCreateFailureRecord, NativeViewportVisibilityRecord};
use crate::viewport_map::{CreateAdmission, CreateTerminal, NativeViewportMap};
#[cfg(test)]
use crate::window_snapshot::CompiledWindowObservation;

mod admission;
#[cfg(test)]
mod admission_tests;
use admission::NativeAdmission;
pub(crate) use admission::NativeDispatch;
mod callback_ingress;
mod capacity;
mod framework_callbacks;
mod lifecycle_receipts;
mod ordered_input;
mod output;
mod placement;
mod route_retirement;
pub(crate) use callback_ingress::NativeCallbackHead;
mod command_callback;
#[cfg(test)]
mod command_ingress_tests;
pub(crate) mod journal;
#[cfg(test)]
mod lifecycle_ingress_tests;
mod output_resource;
#[cfg(test)]
mod pointer_observation_tests;
mod preparation_output;
#[cfg(all(test, target_os = "linux"))]
mod preparation_output_tests;
mod roster;
#[cfg(test)]
mod shared_command_tests;
mod terminal_roster;
mod window_resource;
#[cfg(test)]
mod workspace_replacement_tests;
use command_callback::CommandCallbackAdmission;
use journal::{HostIngressJournal, ReservedHostRecord};
use output_resource::NativeOutputResource;
pub(crate) use output_resource::PendingNativeOutput;
use roster::NativeViewportRosterKey;
pub(crate) use roster::{NativeViewportRosterEnvelope, NativeViewportRosterRecord};
use terminal_roster::TerminalRosterState;
use window_resource::NativeWindowResources;
pub(crate) use window_resource::{
    NativePreparationObservation, NativePreparationPaint, NativePreparationPaintPhase,
};

#[derive(Debug, Clone)]
pub(crate) enum HostRecord {
    WindowEvent(NativeWindowEventRecord),
    DeviceRemoval(NativeDeviceRemovalRecord),
    GlobalFocus(NativeGlobalFocusRecord),
    ViewportFocus(NativeViewportFocusRecord),
    ViewportPointerPassthrough(NativePointerPassthroughRecord),
    PointerObservation(NativePointerObservationKey),
    PointerCapture(NativePointerCaptureKey),
    ViewportCloseCancelled(NativeViewportCloseCancellationRecord),
    ViewportRoster(NativeViewportRosterEnvelope),
    ViewportCreateFailed(NativeViewportCreateFailureRecord),
    ViewportVisibility(NativeViewportVisibilityRecord),
    ViewportCreated(NativeViewportCreatedRecord),
    Output { token: NativeOutputToken },
}

impl HostRecord {
    fn requires_terminal_roster(&self) -> bool {
        match self {
            Self::ViewportPointerPassthrough(_)
            | Self::ViewportVisibility(_)
            | Self::ViewportCreated(_) => true,
            Self::WindowEvent(event) => {
                matches!(event.event(), winit::event::WindowEvent::Destroyed)
            }
            Self::DeviceRemoval(_)
            | Self::PointerObservation(_)
            | Self::PointerCapture(_)
            | Self::GlobalFocus(_)
            | Self::ViewportFocus(_)
            | Self::ViewportCloseCancelled(_)
            | Self::ViewportRoster(_)
            | Self::ViewportCreateFailed(_)
            | Self::Output { .. } => false,
        }
    }
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
pub(crate) enum DeferredViewportPaint {
    Created,
    Staging(NativePreparationPaint),
    Semantic(NativeSurfaceBinding),
    Retain(NativeSurfaceBinding),
    Waiting,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct OutputReservation {
    binding: Option<NativeSurfaceBinding>,
    terminal_recorded: bool,
    abandoned: bool,
    wake_on_terminal: bool,
    wake_after_current: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum OutputAbandonment {
    NewlyAbandoned,
    AlreadyAbandonedAwaitingTerminal,
    MissingOrTerminal,
}

impl OutputReservation {
    pub(crate) const fn unbound() -> Self {
        Self {
            binding: None,
            terminal_recorded: false,
            abandoned: false,
            wake_on_terminal: true,
            wake_after_current: false,
        }
    }

    pub(crate) const fn bound(binding: NativeSurfaceBinding) -> Self {
        Self {
            binding: Some(binding),
            terminal_recorded: false,
            abandoned: false,
            wake_on_terminal: true,
            wake_after_current: false,
        }
    }

    const fn diagnostic(binding: Option<NativeSurfaceBinding>) -> Self {
        Self {
            binding,
            terminal_recorded: false,
            abandoned: true,
            wake_on_terminal: false,
            wake_after_current: false,
        }
    }

    const fn terminal_create(binding: NativeSurfaceBinding) -> Self {
        Self {
            binding: Some(binding),
            terminal_recorded: false,
            abandoned: true,
            wake_on_terminal: true,
            wake_after_current: true,
        }
    }

    pub(crate) fn attach(&mut self, binding: NativeSurfaceBinding) -> bool {
        if let Some(existing) = self.binding {
            // The token owns a real-window callback, while each semantic
            // contribution validates its own exact surface placement.
            return existing.window() == binding.window();
        }
        self.binding = Some(binding);
        true
    }

    pub(crate) const fn binding(self) -> Option<NativeSurfaceBinding> {
        self.binding
    }

    pub(crate) const fn is_abandoned(self) -> bool {
        self.abandoned
    }

    #[cfg(test)]
    pub(crate) const fn unbound_for_test() -> Self {
        Self::unbound()
    }

    #[cfg(test)]
    const fn bound_for_test(binding: NativeSurfaceBinding) -> Self {
        Self::bound(binding)
    }

    fn abandon(&mut self) -> bool {
        self.abandon_with_terminal_wake(true)
    }

    fn abandon_silently(&mut self) -> bool {
        self.abandon_with_terminal_wake(false)
    }

    fn abandon_with_terminal_wake(&mut self, wake_on_terminal: bool) -> bool {
        if self.abandoned {
            return false;
        }
        self.abandoned = true;
        self.wake_on_terminal = wake_on_terminal;
        true
    }

    fn settle_abandonment(&mut self, wake_on_terminal: bool) -> OutputAbandonment {
        if self.terminal_recorded {
            return OutputAbandonment::MissingOrTerminal;
        }
        if self.abandoned {
            self.wake_on_terminal |= wake_on_terminal;
            return OutputAbandonment::AlreadyAbandonedAwaitingTerminal;
        }
        self.abandoned = true;
        self.wake_on_terminal = wake_on_terminal;
        OutputAbandonment::NewlyAbandoned
    }

    const fn wakes_on_terminal(self) -> bool {
        self.wake_on_terminal
    }

    const fn wakes_after_current(self) -> bool {
        self.wake_after_current
    }

    fn require_wake_after_current(&mut self) -> bool {
        if self.terminal_recorded {
            return self.wake_after_current;
        }
        self.wake_after_current = true;
        true
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct NativeCreateReservation {
    binding: NativeSurfaceBinding,
    rect: NativePhysicalRect,
}

#[cfg(test)]
mod tests;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OutputRecordDisposition {
    Recorded,
    RecordedService,
    RecordedAfterCurrent,
    Ignored,
    IgnoredAndWake,
    IgnoredAndWakeAfterCurrent,
    ProtocolViolation,
    Inactive,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HostIngressMode {
    Active,
    Quarantined,
    Frozen,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HostAttachmentState {
    Available,
    Attached,
    Retired,
}

impl HostAttachmentState {
    fn try_attach(&mut self) -> bool {
        if !matches!(self, Self::Available) {
            return false;
        }
        *self = Self::Attached;
        true
    }

    fn detach(&mut self) {
        if matches!(self, Self::Attached) {
            *self = Self::Retired;
        }
    }
}

impl HostIngressMode {
    const fn accepts_new_work(self) -> bool {
        matches!(self, Self::Active)
    }

    const fn accepts_terminal_callbacks(self) -> bool {
        !matches!(self, Self::Frozen)
    }

    fn accepts_window_event(self, event: &winit::event::WindowEvent) -> bool {
        match self {
            Self::Active => NativeWindowEventClass::classify(event).is_dockspace_ingress(),
            Self::Quarantined => matches!(
                event,
                winit::event::WindowEvent::CloseRequested | winit::event::WindowEvent::Destroyed
            ),
            Self::Frozen => false,
        }
    }

    const fn accepts_device_removal(self) -> bool {
        self.accepts_terminal_callbacks()
    }
}

#[derive(Debug)]
struct NativeHostState {
    viewports: NativeViewportMap,
    attachment: HostAttachmentState,
    mode: HostIngressMode,
    admission: NativeAdmission,
    journal: HostIngressJournal,
    output_resources: output_resource::NativeOutputResources,
    input_storage_allowance: Option<capacity::NativeHostInputAllowance>,
    pointer_translation_reserved_bytes: usize,
    snapshot_storage_reserved_bytes: usize,
    window_resource_staging_reserved_bytes: usize,
    host_owner_external_reserved_bytes: usize,
    root_placement_capacity_bytes: usize,
    window_resources: NativeWindowResources,
    retirements: NativeRetirementState,
    close_control: NativeCloseControl,
    roster_context: u64,
    committed_viewport_roster: Option<NativeViewportRosterKey>,
    in_flight_viewport_roster: Option<NativeViewportRosterKey>,
    output_context: Option<NativeOutputToken>,
    last_output_ordinal: u64,
    submitted_output_prefix: usize,
    output_order_invalid: bool,
    event_boundary_pending: bool,
    global_focus_refresh_pending: bool,
    terminal_roster: TerminalRosterState,
}

/// Exclusive claim over an immutable same-route cursor prefix.
///
/// The mailbox lock keeps callbacks and retirement from rewriting membership
/// while translation is admitted by the semantic recorder. Dropping a failed
/// claim leaves its raw records untouched. After retention only the recorder
/// owns the payload; the mailbox keeps the existing commit boundary.
pub(crate) struct CursorPrefixClaim<'records> {
    records: MutexGuard<'records, NativeHostState>,
    len: std::num::NonZeroUsize,
}

impl CursorPrefixClaim<'_> {
    #[cfg(test)]
    pub(crate) const fn record_count(&self) -> std::num::NonZeroUsize {
        self.len
    }

    #[cfg(test)]
    pub(crate) fn events(&self) -> impl Iterator<Item = &NativeWindowEventRecord> {
        self.captured_events().map(|(event, _)| event)
    }

    pub(crate) fn captured_events(
        &self,
    ) -> impl Iterator<Item = (&NativeWindowEventRecord, std::time::Duration)> {
        self.records
            .journal
            .reserved_iter()
            .take(self.len.get())
            .filter_map(|record| {
                if let HostRecord::WindowEvent(event) = record.record() {
                    Some((event, record.captured_at()))
                } else {
                    None
                }
            })
    }

    #[cfg(test)]
    pub(crate) fn retain_prefix(mut self, count: std::num::NonZeroUsize) -> bool {
        if count > self.len {
            return false;
        }
        self.records.journal.transfer_front(count.get());
        self.records.event_boundary_pending = true;
        true
    }

    pub(crate) fn retain_translated_prefix(
        mut self,
        count: std::num::NonZeroUsize,
        translator: &mut NativePointerTranslator,
        candidate: NativePointerTranslator,
    ) -> bool {
        if count > self.len {
            return false;
        }
        // Drop the old backing before returning its overlap promise to callbacks.
        *translator = candidate;
        self.records.pointer_translation_reserved_bytes =
            translator.reserved_translation_capacity_bytes();
        self.records.journal.transfer_front(count.get());
        self.records.event_boundary_pending = true;
        true
    }
}

impl NativeHostState {
    #[cfg(test)]
    fn active() -> Self {
        Self::with_clock(NativeViewportMap::default(), NativeMonotonicClock::system())
    }

    #[cfg(test)]
    fn with_clock(viewports: NativeViewportMap, clock: NativeMonotonicClock) -> Self {
        Self::try_with_clock(viewports, clock).expect("native test host storage is available")
    }

    #[cfg(test)]
    fn try_with_clock(
        viewports: NativeViewportMap,
        clock: NativeMonotonicClock,
    ) -> Result<Self, NativeHostProtocolError> {
        Self::try_with_clock_and_capacity(
            viewports,
            clock,
            terminal_roster::DEFAULT_WINDOW_CAPACITY,
            terminal_roster::DEFAULT_WORK_AREA_CAPACITY,
        )
    }

    fn try_with_clock_and_capacity(
        viewports: NativeViewportMap,
        clock: NativeMonotonicClock,
        windows: usize,
        work_areas: usize,
    ) -> Result<Self, NativeHostProtocolError> {
        NativeHostBridge::initial_storage_bytes(windows, work_areas)
            .ok_or(NativeHostProtocolError::NativeReportStorageUnavailable)?;
        let journal = HostIngressJournal::try_new(clock)?;
        let output_resources = output_resource::NativeOutputResources::try_new()?;
        let terminal_roster = TerminalRosterState::try_with_capacity(windows, work_areas)?;
        Ok(Self {
            viewports,
            attachment: HostAttachmentState::Available,
            mode: HostIngressMode::Active,
            admission: NativeAdmission::default(),
            journal,
            output_resources,
            input_storage_allowance: None,
            pointer_translation_reserved_bytes: NativePointerTranslator::default()
                .reserved_translation_capacity_bytes(),
            snapshot_storage_reserved_bytes: 0,
            window_resource_staging_reserved_bytes: 0,
            host_owner_external_reserved_bytes: 0,
            root_placement_capacity_bytes: 0,
            window_resources: NativeWindowResources::default(),
            retirements: NativeRetirementState::default(),
            close_control: NativeCloseControl::default(),
            roster_context: 0,
            committed_viewport_roster: None,
            in_flight_viewport_roster: None,
            output_context: None,
            last_output_ordinal: 0,
            submitted_output_prefix: 0,
            output_order_invalid: false,
            event_boundary_pending: false,
            global_focus_refresh_pending: false,
            terminal_roster,
        })
    }

    fn freeze(&mut self) {
        self.mode = HostIngressMode::Frozen;
    }

    fn enter_quarantine(&mut self) {
        if !matches!(self.mode, HostIngressMode::Frozen) {
            self.mode = HostIngressMode::Quarantined;
        }
        // Quarantine cannot accept another staging or semantic paint. Release
        // adapter-owned wake obligations here; exact output reservations and
        // binding retirement continue to carry terminal lifetime authority.
        if self.window_resources.clear_staging() {
            // Only an actual preparation cancellation changes this roster's
            // context. Callback-only or undeclared routes owe no new snapshot.
            self.invalidate_viewport_roster();
        }
        self.viewports.clear_semantic_wakes();
        if self
            .journal
            .iter()
            .chain(self.owner_ingress().map(ReservedHostRecord::record))
            .any(HostRecord::requires_terminal_roster)
        {
            self.terminal_roster.require();
        }
    }

    fn require_terminal_roster(&mut self) {
        self.terminal_roster.require();
    }

    fn accepts_window_event(&self, event: &winit::event::WindowEvent) -> bool {
        self.mode.accepts_window_event(event)
    }

    fn block_semantic_viewport(&mut self, viewport: ViewportId) {
        self.viewports.block_semantic_viewport(viewport);
    }

    fn release_semantic_viewports_if_ready(&mut self) {
        if self.semantic_prelude_ready() {
            self.viewports.release_blocked_semantic_viewports();
        }
    }

    #[cfg(test)]
    fn take_ready_semantic_viewports(&mut self) -> Vec<ViewportId> {
        std::iter::from_fn(|| self.viewports.claim_ready_semantic_viewport()).collect()
    }

    fn invalidate_viewport_roster(&mut self) {
        self.roster_context = self
            .roster_context
            .checked_add(1)
            .expect("native viewport roster context exhausted");
    }

    fn viewport_roster_known_borrowed(&self, roster: NativeViewportRoster<'_>) -> bool {
        let known = |key: &NativeViewportRosterKey| {
            key.context == self.roster_context
                && key.roster.matches_borrowed(
                    roster,
                    &self.viewports,
                    self.terminal_roster.work_area_capacity(),
                )
        };
        self.committed_viewport_roster.as_ref().is_some_and(known)
            || self.in_flight_viewport_roster.as_ref().is_some_and(known)
            || self
                .journal
                .iter()
                .chain(self.owner_ingress().map(ReservedHostRecord::record))
                .any(|record| matches!(record, HostRecord::ViewportRoster(envelope) if known(&envelope.key)))
    }

    fn defer_ordinary_roster_capture(&mut self) {
        if self.terminal_roster.window_capacity_exceeded() {
            self.freeze_ordinary();
        } else if !self.terminal_roster.has_pending() {
            // A baseline and an accepted snapshot can borrow both backings.
            // Drain that snapshot and recapture fresh framework facts through
            // the existing protected receipt without cancelling the gesture.
            self.terminal_roster.require();
        }
    }
}

fn deferred_semantic_prelude_ready<T>(mut records: impl Iterator<Item = T>) -> bool {
    records.next().is_none()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum IdleCursorPrefixEntry {
    Cursor,
    Output,
    Boundary,
}

fn idle_cursor_prefix_latest_index(
    entries: impl Iterator<Item = IdleCursorPrefixEntry>,
) -> Option<usize> {
    let mut latest_cursor_index = None;
    for (index, entry) in entries.enumerate() {
        match entry {
            IdleCursorPrefixEntry::Cursor => latest_cursor_index = Some(index),
            IdleCursorPrefixEntry::Output | IdleCursorPrefixEntry::Boundary => break,
        }
    }
    latest_cursor_index.filter(|index| *index != 0)
}

const fn is_next_output_ordinal(previous: u64, current: u64) -> bool {
    matches!(previous.checked_add(1), Some(expected) if expected == current)
}

pub(crate) struct NativeHostBridge {
    records: Mutex<NativeHostState>,
    output_pass_finalizer: OnceLock<Arc<dyn NativeOutputPassFinalizer>>,
    clock: NativeMonotonicClock,
}

const OUTPUT_PREFIX_CAPACITY: usize = 256;

/// A fixed range of the existing capture journal, without owning its records.
/// Reading advances before callers mutate or remove an exact output owner.
pub(crate) struct NativeOutputPrefix {
    outputs: [Option<(u64, NativeOutputToken)>; OUTPUT_PREFIX_CAPACITY],
    next: usize,
    through: u64,
    remaining: usize,
}

impl Default for NativeOutputPrefix {
    fn default() -> Self {
        Self {
            outputs: [None; OUTPUT_PREFIX_CAPACITY],
            next: 0,
            through: 0,
            remaining: 0,
        }
    }
}

pub(crate) struct NativeOutputPrefixEntry {
    pub(crate) result: NativeOutputResult,
    pub(crate) submitted: bool,
    pub(crate) attached: bool,
    pub(crate) binding: Option<NativeSurfaceBinding>,
}

pub(crate) struct NativeDispatchBudget<'bridge> {
    bridge: &'bridge NativeHostBridge,
    token: NativeOutputToken,
    budget: Option<NativeInputBudget>,
}

impl std::ops::Deref for NativeDispatchBudget<'_> {
    type Target = NativeInputBudget;

    fn deref(&self) -> &Self::Target {
        self.budget
            .as_ref()
            .expect("a checked-out budget remains owned")
    }
}

impl std::ops::DerefMut for NativeDispatchBudget<'_> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.budget
            .as_mut()
            .expect("a checked-out budget remains owned")
    }
}

impl Drop for NativeDispatchBudget<'_> {
    fn drop(&mut self) {
        let mut records = self.bridge.lock();
        if let (Some(resource), Some(budget)) = (
            records.output_resources.get_mut(&self.token),
            self.budget.take(),
        ) {
            resource.return_budget(budget);
        }
    }
}

#[must_use = "prepared route retirements must be committed or dropped"]
pub(crate) struct PreparedRouteRetirements {
    bridge: Arc<NativeHostBridge>,
    routes: Vec<CommittedRetirement>,
    abandoned: Vec<NativeOutputToken>,
    active: bool,
}

impl PreparedRouteRetirements {
    #[cfg(test)]
    pub(crate) fn commit(self) -> Vec<NativeOutputToken> {
        self.commit_with_routes().1
    }

    pub(crate) fn commit_with_routes(
        mut self,
    ) -> (Vec<CommittedRetirement>, Vec<NativeOutputToken>) {
        let mut records = self.bridge.lock();
        for retirement in &self.routes {
            records.retire_deferred_binding_into(
                retirement.viewport(),
                retirement.binding(),
                |token| {
                    assert!(self.abandoned.len() < self.abandoned.capacity());
                    self.abandoned.push(token);
                },
            );
        }
        records.viewports.commit_retirements(&self.routes);
        records.refresh_window_resource_references();
        self.active = false;
        (
            std::mem::take(&mut self.routes),
            std::mem::take(&mut self.abandoned),
        )
    }
}

impl Drop for PreparedRouteRetirements {
    fn drop(&mut self) {
        if !self.active {
            return;
        }
        self.bridge.lock().viewports.abort_retirements(&self.routes);
    }
}

impl NativeHostBridge {
    pub(crate) fn observation_time(&self) -> std::time::Duration {
        self.clock.now()
    }

    #[cfg(test)]
    pub(crate) fn enable_observation_attachment_for_test(&self) {
        assert!(self.lock().attachment.try_attach());
    }

    #[cfg(test)]
    pub(crate) fn record_pointer_observation_for_test(
        &self,
        record: NativePointerObservationRecord,
    ) -> bool {
        let mut records = self.lock();
        let NativeHostState {
            window_resources,
            journal,
            ..
        } = &mut *records;
        window_resources.receive_observation_record(record, |record| journal.reserve(record))
            == CommandCallbackAdmission::First
    }

    pub(crate) fn reserve_viewport_route(
        &self,
        viewport: ViewportId,
        binding: NativeSurfaceBinding,
    ) -> Result<(), NativeViewportBindingError> {
        let mut records = self.lock();
        if !records.can_admit_binding(binding) {
            return Err(NativeViewportBindingError::CapacityExceeded {
                limit: records.terminal_roster.window_capacity(),
            });
        }
        records.prepare_window_resource_reference(viewport, binding)?;
        records.viewports.reserve(viewport, binding)?;
        records
            .window_resources
            .commit_host_reference(viewport, binding);
        Ok(())
    }

    pub(crate) fn bind_viewport_route(
        &self,
        viewport: ViewportId,
        window: WindowId,
        binding: NativeSurfaceBinding,
    ) -> Result<(), NativeViewportBindingError> {
        let mut records = self.lock();
        if !records.can_admit_binding(binding) {
            return Err(NativeViewportBindingError::CapacityExceeded {
                limit: records.terminal_roster.window_capacity(),
            });
        }
        if !records.retirements.can_retain_window(binding, window) {
            return Err(NativeViewportBindingError::BindingNotCurrent {
                viewport,
                surface: binding.surface(),
            });
        }
        records.prepare_window_resource_reference(viewport, binding)?;
        records.viewports.bind(viewport, window, binding)?;
        records
            .window_resources
            .commit_host_reference(viewport, binding);
        Ok(())
    }

    /// Moves a displaced physical window into its existing retirement owner.
    pub(crate) fn reserve_retired_viewport_replacement(
        &self,
        viewport: ViewportId,
        expected: NativeSurfaceBinding,
        successor: NativeSurfaceBinding,
    ) -> Result<(), NativeViewportBindingError> {
        let mut records = self.lock();
        if !records.can_admit_binding(successor) {
            return Err(NativeViewportBindingError::CapacityExceeded {
                limit: records.terminal_roster.window_capacity(),
            });
        }
        if records.retirements.has_quiescence_owner(successor) {
            return Err(NativeViewportBindingError::BindingNotCurrent {
                viewport,
                surface: successor.surface(),
            });
        }
        if records.viewports.has_window_receipt(viewport, expected)
            && records.viewports.window(viewport).is_some_and(|window| {
                !records
                    .retirements
                    .can_retain_window_receipt(expected, window)
            })
        {
            return Err(NativeViewportBindingError::BindingNotCurrent {
                viewport,
                surface: expected.surface(),
            });
        }
        records.prepare_window_resource_reference(viewport, successor)?;
        let displaced = records
            .viewports
            .reserve_retired_replacement(viewport, expected, successor)?;
        if let Some(receipt) = displaced {
            records
                .retirements
                .retain_window(receipt)
                .expect("preflighted replacement retains its physical owner");
        }
        if expected != successor {
            records.window_resources.retire(viewport, expected);
        }
        records
            .window_resources
            .commit_host_reference(viewport, successor);
        records.refresh_window_resource_references();
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn new(viewports: NativeViewportMap) -> Self {
        Self::with_clock(viewports, NativeMonotonicClock::system())
    }

    pub(crate) fn try_new(viewports: NativeViewportMap) -> Result<Self, NativeHostProtocolError> {
        Self::try_new_with_clock(viewports, NativeMonotonicClock::system())
    }

    pub(crate) fn try_new_with_clock(
        viewports: NativeViewportMap,
        clock: NativeMonotonicClock,
    ) -> Result<Self, NativeHostProtocolError> {
        Self::try_new_with_capacity_and_clock(
            viewports,
            terminal_roster::DEFAULT_WINDOW_CAPACITY,
            terminal_roster::DEFAULT_WORK_AREA_CAPACITY,
            clock,
        )
    }

    #[cfg(test)]
    pub(crate) fn try_new_with_capacity(
        viewports: NativeViewportMap,
        windows: usize,
        work_areas: usize,
    ) -> Result<Self, NativeHostProtocolError> {
        Self::try_new_with_capacity_and_clock(
            viewports,
            windows,
            work_areas,
            NativeMonotonicClock::system(),
        )
    }

    pub(crate) fn try_new_with_capacity_and_clock(
        viewports: NativeViewportMap,
        windows: usize,
        work_areas: usize,
        clock: NativeMonotonicClock,
    ) -> Result<Self, NativeHostProtocolError> {
        Ok(Self {
            records: Mutex::new(NativeHostState::try_with_clock_and_capacity(
                viewports,
                clock.clone(),
                windows,
                work_areas,
            )?),
            output_pass_finalizer: OnceLock::new(),
            clock,
        })
    }

    #[cfg(test)]
    fn with_clock(viewports: NativeViewportMap, clock: NativeMonotonicClock) -> Self {
        Self {
            records: Mutex::new(NativeHostState::with_clock(viewports, clock.clone())),
            output_pass_finalizer: OnceLock::new(),
            clock,
        }
    }

    pub(crate) fn dispatch_budget(
        &self,
        token: NativeOutputToken,
    ) -> Option<NativeDispatchBudget<'_>> {
        let budget = self
            .lock()
            .output_resources
            .get_mut(&token)?
            .take_budget()?;
        Some(NativeDispatchBudget {
            bridge: self,
            token,
            budget: Some(budget),
        })
    }

    pub(crate) fn service_budget(&self) -> NativeInputBudget {
        NativeInputBudget::new(&self.clock)
    }

    /// The borrow cannot escape this closure. Callers release the mailbox lock
    /// before invoking core, eframe, or another bridge operation.
    pub(crate) fn with_close_control<R>(
        &self,
        action: impl FnOnce(&mut NativeCloseControl) -> R,
    ) -> R {
        action(&mut self.lock().close_control)
    }

    pub(crate) fn can_retain_create_ack(
        &self,
        viewport: ViewportId,
        binding: NativeSurfaceBinding,
    ) -> bool {
        self.lock()
            .window_resources
            .can_retain_create_ack(viewport, binding)
    }

    pub(crate) fn retain_create_ack(
        &self,
        viewport: ViewportId,
        binding: NativeSurfaceBinding,
        acknowledgement: NativeCreateEffectAcknowledgement,
    ) -> Result<(), NativeCreateEffectAcknowledgement> {
        self.lock()
            .window_resources
            .retain_create_ack(viewport, binding, acknowledgement)
    }

    pub(crate) fn clear_create_ack(&self, binding: NativeSurfaceBinding) -> bool {
        self.lock().window_resources.clear_create_ack(binding)
    }

    pub(crate) fn create_ack_count(&self) -> usize {
        self.lock().window_resources.create_ack_count()
    }

    pub(crate) fn pending_no_frame_count(&self) -> usize {
        self.lock().window_resources.pending_no_frame().count()
    }

    #[cfg(test)]
    pub(crate) fn retain_no_frame(
        &self,
        viewport: ViewportId,
        binding: NativeSurfaceBinding,
        invalidation: PendingNoFrameInvalidation,
    ) -> Result<(), PendingNoFrameInvalidation> {
        self.lock()
            .window_resources
            .retain_no_frame(viewport, binding, invalidation)
    }

    #[cfg(test)]
    pub(crate) fn no_frame_for_binding(
        &self,
        binding: NativeSurfaceBinding,
    ) -> Option<PendingNoFrameInvalidation> {
        self.lock()
            .window_resources
            .pending_no_frame()
            .find(|record| record.binding == binding)
            .copied()
    }

    pub(crate) fn has_pending_no_frame_invalidation(&self, window: NativeWindowBinding) -> bool {
        self.lock()
            .window_resources
            .pending_no_frame()
            .any(|record| record.binding.window() == window)
    }

    pub(crate) fn finish_no_frame_invalidations(&self, committed: &[PendingNoFrameInvalidation]) {
        let mut records = self.lock();
        for &invalidation in committed {
            records.window_resources.clear_no_frame_if(invalidation);
        }
        records.refresh_window_resource_references();
    }

    pub(crate) fn prepare_no_frame_invalidations(
        &self,
        session: &DockspaceSession,
    ) -> Result<PreparedNoFrameInvalidations, NativeRuntimeError> {
        let mut records = self.lock();
        let slots = records
            .window_resources
            .resource_slot_limit()
            .unwrap_or_else(|| records.window_resources.pending_no_frame().count());
        let mut ready = Vec::new();
        let mut repaint_viewports = Vec::new();
        ready
            .try_reserve_exact(slots)
            .map_err(|_| NativeHostProtocolError::NativeReportStorageUnavailable)?;
        repaint_viewports
            .try_reserve_exact(slots)
            .map_err(|_| NativeHostProtocolError::NativeReportStorageUnavailable)?;
        if ready.capacity() > slots || repaint_viewports.capacity() > slots {
            return Err(NativeHostProtocolError::NativeReportStorageUnavailable.into());
        }
        let NativeHostState {
            window_resources,
            output_resources,
            viewports,
            ..
        } = &mut *records;
        window_resources.retain_no_frame_records(|invalidation| {
            let (terminal_pending, binding) = match invalidation.terminal {
                NoFrameTerminal::Awaiting(token) => {
                    let pending = output_resources.contains_key(&token);
                    if !pending {
                        invalidation.terminal = NoFrameTerminal::Observed;
                    }
                    (
                        pending,
                        viewports.binding_for_output(
                            token.viewport_id(),
                            token.window_id(),
                            token.create_attempt(),
                        ),
                    )
                }
                NoFrameTerminal::Observed => (
                    false,
                    viewports.binding_for_output(
                        invalidation.viewport,
                        invalidation.window,
                        invalidation.create_attempt,
                    ),
                ),
            };
            match no_frame_invalidation_readiness(
                terminal_pending,
                binding.is_some_and(|binding| binding.window() == invalidation.binding.window()),
                session
                    .native_window_bindings()
                    .any(|window| window == invalidation.binding.window()),
            ) {
                NoFrameInvalidationReadiness::WaitingForTerminal => true,
                NoFrameInvalidationReadiness::Stale => false,
                NoFrameInvalidationReadiness::Ready => {
                    assert!(ready.len() < ready.capacity());
                    ready.push(*invalidation);
                    if !repaint_viewports.contains(&invalidation.viewport) {
                        assert!(repaint_viewports.len() < repaint_viewports.capacity());
                        repaint_viewports.push(invalidation.viewport);
                    }
                    true
                }
            }
        });
        Ok(PreparedNoFrameInvalidations {
            ready,
            repaint_viewports,
        })
    }

    pub(crate) fn settle_close_requests(&self, applied: bool) -> usize {
        self.lock().close_control.settle_clear(applied)
    }

    pub(crate) fn admission_dispatch(&self) -> NativeDispatch {
        let mut records = self.lock();
        let count = records.retained_record_count();
        let age = records.oldest_record_age();
        if matches!(records.admission, NativeAdmission::Running)
            && (!records.journal.can_push_raw()
                || !records.can_reserve_output_data()
                || count >= admission::RECORD_LIMIT
                || age.is_some_and(|age| age >= admission::AGE_LIMIT))
        {
            records.freeze_ordinary();
        }
        if records.can_reserve_output_data() {
            records.admission.try_resume(count, age);
        }
        records.dispatch()
    }

    pub(crate) fn permits_unscheduled_input(&self) -> bool {
        !matches!(self.lock().admission, NativeAdmission::Draining { .. })
    }

    pub(crate) fn accepts_new_work(&self) -> bool {
        self.lock().mode.accepts_new_work()
    }

    /// The raw owner remains queued while already accepted core input drains.
    pub(crate) fn defer_for_core_input_capacity(&self) {
        self.lock().event_boundary_pending = true;
    }

    pub(crate) fn finish_overload_cancellation(&self, boundary: u64) -> bool {
        self.lock().admission.finish_cancellation(boundary)
    }

    #[cfg(test)]
    pub(crate) fn freeze_ordinary_for_test(&self) -> u64 {
        self.lock().freeze_ordinary()
    }

    fn lock(&self) -> MutexGuard<'_, NativeHostState> {
        self.records.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Runs a route query under the single host-state lock.
    /// The closure must not invoke core, eframe, or another bridge operation.
    pub(crate) fn with_viewports<R>(&self, action: impl FnOnce(&NativeViewportMap) -> R) -> R {
        action(&self.lock().viewports)
    }

    /// Mutates routes under the single host-state lock.
    /// The closure must not invoke core, eframe, or another bridge operation.
    pub(crate) fn with_viewports_mut<R>(
        &self,
        action: impl FnOnce(&mut NativeViewportMap) -> R,
    ) -> R {
        let mut records = self.lock();
        let result = action(&mut records.viewports);
        records.refresh_window_resource_references();
        result
    }

    pub(crate) fn set_focus_reportable(&self, bindings: &[NativeSurfaceBinding]) {
        let mut records = self.lock();
        let newly_reportable = records.viewports.set_focus_reportable(bindings);
        records.global_focus_refresh_pending |= newly_reportable;
    }

    /// Retry only after ordinary ingress recovers; retain no old focus payload.
    pub(crate) fn request_pending_global_focus(&self, context: &eframe::egui::Context) {
        let request = {
            let records = self.lock();
            records.global_focus_refresh_pending
                && records.mode.accepts_new_work()
                && matches!(records.admission, NativeAdmission::Running)
        };
        if request {
            eframe::request_native_global_focus(context);
        }
    }

    /// Transfers an exact never-admitted route into its prepaid retirement row.
    /// Receipt admission and route removal share this lock, so a concurrent
    /// physical create cannot disappear between the ownership check and transfer.
    pub(crate) fn retire_unstarted_viewport(
        &self,
        viewport: ViewportId,
        binding: NativeSurfaceBinding,
    ) -> bool {
        let mut records = self.lock();
        if records.retirements.has_quiescence_owner(binding)
            || records
                .viewports
                .has_create_lifetime_owner(viewport, binding)
        {
            return false;
        }
        let Ok((_, receipt)) = records.viewports.remove_viewport(viewport, binding) else {
            return false;
        };
        debug_assert!(
            receipt.is_none(),
            "an unstarted route owns no physical receipt"
        );
        records.retirements.commit_detachment(binding);
        records.refresh_window_resource_references();
        true
    }

    pub(crate) fn has_pending_coordinator_work(&self) -> bool {
        let records = self.lock();
        records.viewports.has_pending_create_receipts() || records.has_pending_coordinator_work()
    }

    #[cfg(test)]
    pub(crate) fn captured_through_for_test(&self) -> u64 {
        self.lock().journal.captured_through()
    }

    #[cfg(test)]
    pub(crate) fn input_backlog_for_test(&self) -> (usize, Option<std::time::Duration>) {
        let records = self.lock();
        (records.retained_record_count(), records.oldest_record_age())
    }

    /// Reserves the exact binding which may admit one deferred create attempt.
    pub(crate) fn reserve_create(
        &self,
        viewport: ViewportId,
        binding: NativeSurfaceBinding,
        rect: NativePhysicalRect,
    ) -> bool {
        let mut records = self.lock();
        if !records.can_admit_binding(binding) || !records.admit_ordinary() {
            return false;
        }
        records
            .window_resources
            .reserve_create(viewport, binding, rect)
    }

    pub(crate) fn clear_create(&self, viewport: ViewportId, binding: NativeSurfaceBinding) -> bool {
        self.lock().window_resources.clear_create(viewport, binding)
    }

    pub(crate) fn clear_hidden_render(
        &self,
        viewport: ViewportId,
        binding: NativeSurfaceBinding,
    ) -> bool {
        self.lock().window_resources.clear_hidden(viewport, binding)
    }

    fn hidden_render_enabled(&self, viewport: ViewportId) -> bool {
        let records = self.lock();
        records.mode.accepts_new_work()
            && records.window_resources.hidden_binding(viewport).is_some()
    }

    pub(crate) fn create_binding(&self, viewport: ViewportId) -> Option<NativeSurfaceBinding> {
        self.lock()
            .window_resources
            .create(viewport)
            .map(|reservation| reservation.binding)
    }

    pub(crate) fn accepts_no_frame_invalidation(&self) -> bool {
        self.lock().accepts_no_frame_invalidation()
    }

    pub(crate) fn commit_frame_boundary(&self, input_pending: bool) {
        let mut records = self.lock();
        records.event_boundary_pending = input_pending;
        if input_pending {
            return;
        }
        records.journal.commit_transferred();
        records.viewports.commit_create_boundary();
        records.window_resources.commit_transferred_callbacks();
        records.close_control.commit_transferred_callbacks();
        records.retirements.commit_create_failures();
        records.retirements.commit_destroyed_callbacks();
        records.terminal_roster.commit_transferred();
        loop {
            let Some(HostRecord::Output { token }) = records.front_record() else {
                break;
            };
            let token = *token;
            if !records
                .output_resources
                .get(&token)
                .is_some_and(|resource| resource.submitted)
            {
                break;
            }
            records.output_resources.remove(&token);
            records.submitted_output_prefix -= 1;
        }
        records.release_semantic_viewports_if_ready();
        records.refresh_window_resource_references();
    }

    #[cfg(test)]
    pub(crate) fn take_ready_semantic_viewports(&self) -> Vec<ViewportId> {
        self.lock().take_ready_semantic_viewports()
    }

    pub(crate) fn claim_ready_semantic_viewport(&self) -> Option<ViewportId> {
        self.lock().viewports.claim_ready_semantic_viewport()
    }

    pub(crate) fn block_semantic_viewport(&self, viewport: ViewportId) {
        self.lock().block_semantic_viewport(viewport);
    }

    pub(crate) fn freeze(&self) {
        self.lock().freeze();
    }

    pub(crate) fn enter_quarantine(&self) {
        self.lock().enter_quarantine();
    }

    pub(crate) fn claim_quarantine_output(
        &self,
    ) -> Option<(NativeOutputToken, Option<PendingNativeOutput>)> {
        self.lock().claim_quarantine_output()
    }

    #[cfg(test)]
    pub(crate) fn quarantine_after_fatal(&self) -> Vec<NativeOutputToken> {
        self.enter_quarantine();
        std::iter::from_fn(|| self.claim_quarantine_output().map(|(token, _)| token)).collect()
    }

    pub(crate) fn require_terminal_roster(&self) {
        self.lock().require_terminal_roster();
    }
}
