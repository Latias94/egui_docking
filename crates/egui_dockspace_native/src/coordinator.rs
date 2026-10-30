//! Thin coordinator between eframe callbacks and the renderer-neutral session.

use std::collections::VecDeque;
use std::sync::Arc;

use dockspace::geometry::PhysicalRect;
use dockspace::model::SurfaceId;
use dockspace::runtime::{
    DockspacePresentationTransition, DockspaceSession, HostFrameReport, HostInputOutcome,
    HostWindowToken, NativeCloseDisposition, NativeCloseEffectAcknowledgement, NativeCloseState,
    NativeCreateEffectAcknowledgement, NativeCreateStrategy, NativeDispatchFailure,
    NativeEffectAcknowledgement, NativeEffectOperation, NativeEffectRequest, NativeEffectResult,
    NativeEffectSubmissionError, NativeGlobalFocus, NativeHostErrorKind, NativeIndeterminateReason,
    NativeInputBudget, NativeInputStorageLimit, NativePointerInput, NativePointerRoster,
    NativePresentationEffectAcknowledgement, NativeReceiverAnswer, NativeReceiverQuery,
    NativeSurfaceBinding, NativeUnsupportedReason, NativeWindowFacts, NativeWorkAreaBinding,
    NativeWorkAreaRoster, PaintedSurfaceOutput, SurfacePresentationResult,
    SurfaceUnavailableReason,
};
use eframe::{
    NativeHostHandler, NativeHostWake, NativeOutputStatus, NativeOutputToken, NativePhysicalRect,
    NativeViewportCreateAttempt, NativeViewportCreateFailureKind, NativeViewportFocusStatus,
    NativeViewportPointerPassthroughCommandToken, NativeViewportPointerPassthroughStatus,
    NativeViewportVisibilityStatus,
    egui::{ViewportCommand, ViewportId},
};
use winit::window::WindowId;

use crate::NativeRuntimeError;
use crate::capabilities::capabilities_for_backend;
use crate::deferred_viewport::{DeferredViewportDriver, DeferredViewportSpec, viewport_id_for};
use crate::effect_coordinator::{
    NativeEffectCoordinator, NativeViewportCommand, NativeViewportEffectKind,
    NativeViewportEffectPlan,
};
use crate::error::{
    NativeHostProtocolError, NativeOutputBindingError, NativeOutputBindingErrorKind,
    NativeViewportBindingError,
};
use crate::event::{NativeDeviceRemovalRecord, NativeWindowEventClass, NativeWindowEventRecord};
use crate::focus_control::{
    NativeFocusControl, NativeFocusResultDisposition, NativeFocusTermination,
};
use crate::host_frame::NativeHostFrame;
use crate::input_control::{
    NativeInputControl, NativeInputResultDisposition, NativePointerPassthroughCommand,
};
use crate::lifecycle_progress::{
    NativeCloseProgress, NativeLifecycleProgress, NativeOutputBacklog,
};
use crate::mailbox::{
    DeferredViewportPaint, NativeDispatch, NativeHostBridge, NativeOutputPrefix,
    NativeStagingPaintRecord, NativeViewportRosterEnvelope, NativeViewportRosterRecord,
    OutputAbandonment, PendingNativeOutput, PreparedRouteRetirements,
};
#[cfg(test)]
use crate::mailbox::{HostRecord, OutputReservation};
use crate::pointer_event::{NativePointerTranslation, NativePointerTranslator};
use crate::receiver::NativeReceiverStore;
use crate::retirement::{CleanupResultRetentionError, DestroyedObservation};
use crate::viewport_callback::{NativeViewportCreateFailureRecord, NativeViewportVisibilityRecord};
use crate::viewport_map::NativeViewportMap;
use crate::window_snapshot::{CompiledWindowObservation, compile_window_observation};
use crate::work_area::{FrozenWorkAreaRoster, NativeWorkAreaState, PreparedWorkAreaRoster};

mod close_driver;
mod effect_dispatch;
use effect_dispatch::ViewportFailureOrigin;
mod shutdown;
mod storage;
pub use storage::NativeStorageLimit;
pub(crate) use storage::{
    NativeCombinedStorageLimit, NativeOwnerStorageLimit, NativeSnapshotStorageLimit,
};

pub(crate) use shutdown::{NativeShutdownAdvance, NativeShutdownRegistration};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StagingPaintFailureDisposition {
    Obsolete,
    Fatal,
}

const fn staging_paint_failure_disposition(
    kind: Option<NativeHostErrorKind>,
) -> StagingPaintFailureDisposition {
    match kind {
        Some(NativeHostErrorKind::StaleBinding) => StagingPaintFailureDisposition::Obsolete,
        _ => StagingPaintFailureDisposition::Fatal,
    }
}

/// Sole native coordinator for one renderer-neutral docking session.
///
/// Native callbacks only append immutable records. Application code reduces
/// those records at the next root update, acknowledges each accepted window
/// event, renders one affine host frame, and explicitly binds every painted
/// output to the eframe output token that produced it.
pub(crate) struct NativeCoordinator {
    session: DockspaceSession,
    bridge: Arc<NativeHostBridge>,
    queued_snapshot: Option<QueuedNativeSnapshot>,
    snapshot_storage: Option<NativeSnapshotStorageLimit>,
    owner_storage: Option<NativeOwnerStorageLimit>,
    combined_storage_limit: Option<usize>,
    close_observation_settlement: (bool, bool),
    close_decision_scratch: Vec<dockspace::close::CloseRequestId>,
    deferred_viewports: DeferredViewportDriver,
    receivers: NativeReceiverStore,
    effects: NativeEffectCoordinator,
    pending_effect_results: VecDeque<PendingNativeEffectResult>,
    pending_presentation_transitions: Vec<DockspacePresentationTransition>,
    callback_errors: VecDeque<NativeRuntimeError>,
    pointer_translator: NativePointerTranslator,
    work_areas: NativeWorkAreaState,
    focus_control: NativeFocusControl,
    input_control: NativeInputControl,
    lifecycle_progress: NativeLifecycleProgress,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PendingNoFrameInvalidation {
    pub(crate) terminal: NoFrameTerminal,
    pub(crate) viewport: ViewportId,
    pub(crate) window: WindowId,
    pub(crate) create_attempt: Option<NativeViewportCreateAttempt>,
    pub(crate) binding: NativeSurfaceBinding,
}

impl PendingNoFrameInvalidation {
    fn awaiting_terminal(token: NativeOutputToken, binding: NativeSurfaceBinding) -> Self {
        Self {
            terminal: NoFrameTerminal::Awaiting(token),
            viewport: token.viewport_id(),
            window: token.window_id(),
            create_attempt: token.create_attempt(),
            binding,
        }
    }

    pub(crate) const fn surface(self) -> SurfaceId {
        self.binding.surface()
    }

    pub(crate) const fn viewport(self) -> ViewportId {
        self.viewport
    }

    #[cfg(test)]
    const fn terminal_observed_for_test(
        viewport: ViewportId,
        window: WindowId,
        binding: NativeSurfaceBinding,
    ) -> Self {
        Self {
            terminal: NoFrameTerminal::Observed,
            viewport,
            window,
            create_attempt: None,
            binding,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NoFrameTerminal {
    Awaiting(NativeOutputToken),
    Observed,
}

pub(crate) struct PreparedNoFrameInvalidations {
    pub(crate) ready: Vec<PendingNoFrameInvalidation>,
    pub(crate) repaint_viewports: Vec<ViewportId>,
}

impl PreparedNoFrameInvalidations {
    pub(crate) fn select_for_surfaces(&mut self, surfaces: &[SurfaceId], enabled: bool) {
        self.repaint_viewports.clear();
        self.ready.retain(|invalidation| {
            if !enabled {
                return false;
            }
            if surfaces.contains(&invalidation.surface()) {
                return true;
            }
            if !self.repaint_viewports.contains(&invalidation.viewport()) {
                assert!(self.repaint_viewports.len() < self.repaint_viewports.capacity());
                self.repaint_viewports.push(invalidation.viewport());
            }
            false
        });
    }
}

#[cfg(test)]
pub(crate) fn split_no_frame_invalidations_for_frame<T>(
    frame_surfaces: &[SurfaceId],
    ready: impl IntoIterator<Item = T>,
    surface: impl Fn(&T) -> SurfaceId,
    viewport: impl Fn(&T) -> ViewportId,
) -> (Vec<T>, Vec<ViewportId>) {
    let mut accepted = Vec::new();
    let mut repaint_viewports = Vec::new();
    for invalidation in ready {
        if frame_surfaces.contains(&surface(&invalidation)) {
            accepted.push(invalidation);
        } else if !repaint_viewports.contains(&viewport(&invalidation)) {
            repaint_viewports.push(viewport(&invalidation));
        }
    }
    (accepted, repaint_viewports)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NoFrameInvalidationReadiness {
    WaitingForTerminal,
    Ready,
    Stale,
}

pub(crate) const fn no_frame_invalidation_readiness(
    terminal_pending: bool,
    route_is_current: bool,
    binding_is_current: bool,
) -> NoFrameInvalidationReadiness {
    if terminal_pending {
        NoFrameInvalidationReadiness::WaitingForTerminal
    } else if route_is_current && binding_is_current {
        NoFrameInvalidationReadiness::Ready
    } else {
        NoFrameInvalidationReadiness::Stale
    }
}

fn resolve_deferred_binding_paint(
    disposition: DeferredViewportPaint,
    route_matches: bool,
    binding_is_current: bool,
    binding_is_recognized: bool,
) -> DeferredViewportPaint {
    if !route_matches || !binding_is_recognized {
        DeferredViewportPaint::Waiting
    } else if binding_is_current {
        // The bridge has already checked staging. A prospective child must
        // paint before first-live presentation can publish its durable surface.
        disposition
    } else {
        match disposition {
            DeferredViewportPaint::Semantic(binding) | DeferredViewportPaint::Retain(binding) => {
                DeferredViewportPaint::Retain(binding)
            }
            DeferredViewportPaint::Created
            | DeferredViewportPaint::Staging(_)
            | DeferredViewportPaint::Waiting => {
                unreachable!("only binding paint dispositions reach authority resolution")
            }
        }
    }
}

const fn should_record_no_frame_invalidation(
    accepts_invalidation: bool,
    binding_is_current: bool,
    surface_is_presented: bool,
) -> bool {
    accepts_invalidation && binding_is_current && surface_is_presented
}

/// One terminal result which still needs a causal boundary in the core.
///
/// A stale result for a retired window lifetime is allowed to be consumed by
/// the exact binding-quiescence boundary. All other rejected results remain
/// affine until a later retry succeeds.
#[derive(Debug)]
struct PendingNativeEffectResult {
    result: NativeEffectResult,
    last_rejection: Option<NativeHostErrorKind>,
}

impl PendingNativeEffectResult {
    fn blocks_quiescence(&self, binding: NativeSurfaceBinding) -> bool {
        self.result.binding().same_window_lifetime(binding)
            && self.last_rejection != Some(NativeHostErrorKind::StaleBinding)
    }

    fn can_be_consumed_by_quiescence(&self, binding: NativeSurfaceBinding) -> bool {
        self.result.binding().same_window_lifetime(binding)
            && self.last_rejection == Some(NativeHostErrorKind::StaleBinding)
    }
}

const fn native_effect_submission_rejection_is_recoverable(kind: NativeHostErrorKind) -> bool {
    // The affine result remains owned here until the exact retry or retirement
    // boundary, including while accepted input drains to release capacity.
    matches!(
        kind,
        NativeHostErrorKind::StaleBinding
            | NativeHostErrorKind::OperationConflict
            | NativeHostErrorKind::InputCapacity
    )
}

#[derive(Debug)]
struct QueuedNativeSnapshot {
    settlement: Option<bool>,
    roster: NativeViewportRosterEnvelope,
    acknowledgements: Vec<NativeSnapshotAcknowledgements>,
    input_acknowledgement: Option<NativeSurfaceBinding>,
    focus_reportable: Vec<NativeSurfaceBinding>,
    work_areas: PreparedWorkAreaRoster,
}

/// One existing snapshot sidecar settles both independent window properties.
#[derive(Debug, Clone, Copy)]
struct NativeSnapshotAcknowledgements {
    binding: NativeSurfaceBinding,
    create: bool,
    presentation: bool,
}

pub(crate) type PreparedNativeRetirements = PreparedRouteRetirements;

impl std::fmt::Debug for NativeCoordinator {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut state = formatter.debug_struct("NativeCoordinator");
        state.field("session", &self.session);
        self.bridge.with_viewports(|viewports| {
            state.field("viewports", viewports);
        });
        state
            .field("pending_outputs", &self.bridge.pending_output_count())
            .field(
                "pending_no_frame_invalidations",
                &self.bridge.pending_no_frame_count(),
            )
            .field(
                "pending_create_acknowledgements",
                &self.bridge.create_ack_count(),
            )
            .field(
                "pending_presentation_acknowledgements",
                &self.bridge.presentation_ack_count(),
            )
            .field(
                "queued_snapshot_window_acknowledgements",
                &self
                    .queued_snapshot
                    .as_ref()
                    .map_or(0, |queued| queued.acknowledgements.len()),
            )
            .field(
                "queued_snapshot_input_acknowledgement",
                &self
                    .queued_snapshot
                    .as_ref()
                    .is_some_and(|queued| queued.input_acknowledgement.is_some()),
            )
            .field(
                "queued_snapshot_focus_reportable",
                &self
                    .queued_snapshot
                    .as_ref()
                    .map_or(0, |queued| queued.focus_reportable.len()),
            )
            .field("deferred_viewports", &self.deferred_viewports)
            .field("receivers", &self.receivers)
            .field("effects", &self.effects)
            .field("pending_effect_results", &self.pending_effect_results.len())
            .field(
                "pending_presentation_transitions",
                &self.pending_presentation_transitions.len(),
            )
            .field("callback_errors", &self.callback_errors.len())
            .field("pointer_translator", &self.pointer_translator)
            .field("focus_control", &self.focus_control)
            .field("input_control", &self.input_control)
            .field("lifecycle_progress", &self.lifecycle_progress)
            .finish_non_exhaustive()
    }
}

impl NativeCoordinator {
    /// Enrolls the one managed desktop provider for a session.
    ///
    /// # Errors
    ///
    /// Returns an error when another native or pointer provider is active or
    /// the initial pointer roster is not a complete valid checkpoint.
    pub fn new(
        session: DockspaceSession,
        initial_pointer_roster: NativePointerRoster,
    ) -> Result<Self, NativeRuntimeError> {
        Self::new_with_storage_limits(session, initial_pointer_roster, None, None)
    }

    pub(crate) fn new_with_storage_limits(
        session: DockspaceSession,
        initial_pointer_roster: NativePointerRoster,
        storage_limit: Option<NativeInputStorageLimit>,
        owner_limit: Option<NativeOwnerStorageLimit>,
    ) -> Result<Self, NativeRuntimeError> {
        Self::new_with_storage_allowance(
            session,
            initial_pointer_roster,
            storage_limit,
            owner_limit,
            None,
            None,
        )
    }

    pub(crate) fn new_with_combined_storage_limit(
        session: DockspaceSession,
        initial_pointer_roster: NativePointerRoster,
        limit: NativeCombinedStorageLimit,
    ) -> Result<Self, NativeRuntimeError> {
        Self::new_with_combined_storage_limit_and_clock(
            session,
            initial_pointer_roster,
            limit,
            dockspace::runtime::NativeMonotonicClock::system(),
        )
    }

    pub(crate) fn new_with_combined_storage_limit_and_clock(
        session: DockspaceSession,
        initial_pointer_roster: NativePointerRoster,
        limit: NativeCombinedStorageLimit,
        clock: dockspace::runtime::NativeMonotonicClock,
    ) -> Result<Self, NativeRuntimeError> {
        limit.validate()?;
        Self::new_with_storage_allowance(
            session,
            initial_pointer_roster,
            Some(limit.input),
            Some(limit.owners),
            Some(limit),
            Some(clock),
        )
    }

    fn new_with_storage_allowance(
        session: DockspaceSession,
        initial_pointer_roster: NativePointerRoster,
        storage_limit: Option<NativeInputStorageLimit>,
        owner_limit: Option<NativeOwnerStorageLimit>,
        combined: Option<NativeCombinedStorageLimit>,
        clock: Option<dockspace::runtime::NativeMonotonicClock>,
    ) -> Result<Self, NativeRuntimeError> {
        let binding_slots = storage_limit
            .and_then(NativeInputStorageLimit::producer_storage_limit)
            .map(|limit| limit.binding_slots().get());
        let effect_slots = storage_limit
            .and_then(NativeInputStorageLimit::report_storage_limit)
            .map(|limit| limit.effect_slots().get());
        if let Some(owners) = owner_limit {
            owners.validate(binding_slots, effect_slots)?;
        }
        let input_control = NativeInputControl::with_storage_limits(binding_slots, effect_slots)?;
        let effects = NativeEffectCoordinator::with_storage_limits(
            effect_slots,
            owner_limit.map(|limit| limit.command_slots),
        )?;
        let deferred_viewports = match owner_limit {
            Some(_) => DeferredViewportDriver::with_capacity_limit(
                binding_slots.expect("owner storage validates the binding bound"),
            )?,
            None => DeferredViewportDriver::default(),
        };
        let bridge = Arc::new(match combined {
            Some(limit) => {
                let producer = limit
                    .input
                    .producer_storage_limit()
                    .expect("the combined profile validates producer bounds");
                NativeHostBridge::try_new_with_capacity_and_clock(
                    NativeViewportMap::default(),
                    producer.binding_slots().get(),
                    producer.work_area_slots().get(),
                    clock.expect("the combined constructor supplies its monotonic clock"),
                )?
            }
            None => match clock {
                Some(clock) => {
                    NativeHostBridge::try_new_with_clock(NativeViewportMap::default(), clock)?
                }
                None => NativeHostBridge::try_new(NativeViewportMap::default())?,
            },
        });
        if let Some(limit) = combined {
            if !bridge.configure_input_storage_allowance(
                limit.byte_limit,
                limit
                    .external_reserved_bytes()
                    .ok_or(NativeHostProtocolError::NativeReportStorageUnavailable)?,
            ) {
                return Err(NativeHostProtocolError::NativeReportStorageUnavailable.into());
            }
        }
        let mut close_decision_scratch = Vec::new();
        if let Some(owners) = owner_limit {
            let bindings = binding_slots.expect("owner storage validates the binding bound");
            bridge.configure_lifecycle_storage(bindings, owners.close_slots)?;
            close_decision_scratch
                .try_reserve_exact(owners.close_slots)
                .map_err(|_| NativeHostProtocolError::NativeReportStorageUnavailable)?;
            if close_decision_scratch.capacity() > owners.close_slots {
                return Err(NativeHostProtocolError::NativeReportStorageUnavailable.into());
            }
            bridge.configure_window_resource_storage(
                owners.resource_slots,
                owners
                    .external_reserved_bytes(bindings)
                    .ok_or(NativeHostProtocolError::NativeReportStorageUnavailable)?,
            )?;
        }
        let mut pending_effect_results = VecDeque::new();
        if let Some(reports) = storage_limit.and_then(NativeInputStorageLimit::report_storage_limit)
        {
            let slot_bytes = size_of::<PendingNativeEffectResult>();
            let reserved_slot_bytes =
                size_of::<(NativeEffectResult, Option<NativeHostErrorKind>)>();
            if slot_bytes > reserved_slot_bytes
                || reports
                    .effect_slots()
                    .get()
                    .checked_mul(slot_bytes)
                    .is_none_or(|bytes| bytes > reports.byte_limit())
            {
                return Err(NativeHostProtocolError::NativeReportStorageUnavailable.into());
            }
            pending_effect_results
                .try_reserve_exact(reports.effect_slots().get())
                .map_err(|_| NativeHostProtocolError::NativeReportStorageUnavailable)?;
            if pending_effect_results.capacity() > reports.effect_slots().get() {
                return Err(NativeHostProtocolError::NativeReportStorageUnavailable.into());
            }
        }
        let mut native = Self {
            session,
            bridge,
            queued_snapshot: None,
            snapshot_storage: None,
            owner_storage: owner_limit,
            combined_storage_limit: combined.map(|limit| limit.byte_limit),
            close_observation_settlement: (false, false),
            close_decision_scratch,
            deferred_viewports,
            receivers: NativeReceiverStore::default(),
            effects,
            pending_effect_results,
            pending_presentation_transitions: Vec::new(),
            callback_errors: VecDeque::new(),
            pointer_translator: NativePointerTranslator::default(),
            work_areas: NativeWorkAreaState::default(),
            focus_control: NativeFocusControl::default(),
            input_control,
            lifecycle_progress: NativeLifecycleProgress::default(),
        };
        if let Some(producer) =
            storage_limit.and_then(NativeInputStorageLimit::producer_storage_limit)
        {
            let window_slots = producer.binding_slots().get();
            let work_area_slots = producer.work_area_slots().get();
            let byte_limit =
                NativeSnapshotStorageLimit::required_bytes(window_slots, work_area_slots)
                    .ok_or(NativeHostProtocolError::NativeReportStorageUnavailable)?;
            native.configure_snapshot_storage_limit(NativeSnapshotStorageLimit {
                window_slots,
                work_area_slots,
                byte_limit,
            })?;
        }
        if let Some(limit) = storage_limit {
            native
                .session
                .enable_managed_native_host_with_input_storage_limit(
                    NativeCreateStrategy::Staged,
                    initial_pointer_roster,
                    limit,
                )?;
        } else {
            native
                .session
                .enable_managed_native_host(NativeCreateStrategy::Staged, initial_pointer_roster)?;
        }
        native.session.report_native_inventory_unknown()?;
        Ok(native)
    }

    /// Returns the eframe host handler to install in [`eframe::NativeOptions`].
    #[must_use]
    pub fn native_host_handler(&self) -> Arc<dyn NativeHostHandler> {
        self.bridge.clone()
    }

    pub(crate) fn dispatch_bridge(&self) -> Arc<NativeHostBridge> {
        Arc::clone(&self.bridge)
    }

    /// Returns the published product view owner.
    #[must_use]
    pub const fn session(&self) -> &DockspaceSession {
        &self.session
    }

    #[cfg(test)]
    pub(crate) const fn lifecycle_progress(&self) -> NativeLifecycleProgress {
        self.lifecycle_progress
    }

    pub(crate) fn close_progress(&self) -> NativeCloseProgress {
        self.bridge.with_close_control(|control| control.progress())
    }

    pub(crate) fn output_backlog(&self) -> NativeOutputBacklog {
        self.bridge.output_backlog()
    }

    pub(crate) fn is_quiescent(&self) -> bool {
        !self.session.has_pending_native_create()
            && !self.bridge.has_pending_coordinator_work()
            && !self.bridge.has_pending_lifecycle_output(&self.session)
            && !self.receivers.has_pending_commit()
            && self.queued_snapshot.is_none()
            && !self.deferred_viewports.has_transitional_viewport()
            && !self.effects.has_pending_work()
            && self.pending_effect_results.is_empty()
            && self.pending_presentation_transitions.is_empty()
            && self.callback_errors.is_empty()
            && !self.bridge.retirement_has_pending_work()
            && !self.focus_control.has_pending_work()
            && !self.input_control.has_pending_work()
    }

    fn retain_internal_presentation_transitions(&mut self, report: &HostFrameReport) {
        self.pending_presentation_transitions
            .extend_from_slice(report.presentation_transitions());
    }

    pub(crate) fn take_internal_presentation_transitions(
        &mut self,
    ) -> Vec<DockspacePresentationTransition> {
        std::mem::take(&mut self.pending_presentation_transitions)
    }

    /// Associates one eframe viewport with an exact current core binding.
    ///
    /// # Errors
    ///
    /// Returns an error when either identity is already associated with a
    /// different peer.
    pub fn bind_viewport(
        &mut self,
        viewport: ViewportId,
        window: WindowId,
        binding: NativeSurfaceBinding,
    ) -> Result<(), NativeViewportBindingError> {
        if !self.session.is_current_native_binding(binding) {
            return Err(NativeViewportBindingError::BindingNotCurrent {
                viewport,
                surface: binding.surface(),
            });
        }
        self.bridge.bind_viewport_route(viewport, window, binding)
    }

    /// Reserves one eframe viewport for an exact binding before eframe creates its window.
    ///
    /// # Errors
    ///
    /// Returns an error when the binding is stale or the viewport/surface is already reserved by
    /// another exact peer.
    pub fn reserve_viewport(
        &mut self,
        viewport: ViewportId,
        binding: NativeSurfaceBinding,
    ) -> Result<(), NativeViewportBindingError> {
        if !self.session.is_current_native_binding(binding) {
            return Err(NativeViewportBindingError::BindingNotCurrent {
                viewport,
                surface: binding.surface(),
            });
        }
        self.bridge.reserve_viewport_route(viewport, binding)
    }

    /// Attaches the native window created for an exact reserved viewport binding.
    ///
    /// # Errors
    ///
    /// Returns an error when the reservation changed or the window is already attached elsewhere.
    pub fn attach_viewport_window(
        &mut self,
        viewport: ViewportId,
        expected: NativeSurfaceBinding,
        window: WindowId,
    ) -> Result<(), NativeViewportBindingError> {
        if !self.session.is_current_native_binding(expected) {
            return Err(NativeViewportBindingError::BindingNotCurrent {
                viewport,
                surface: expected.surface(),
            });
        }
        self.bridge
            .attach_viewport_route(viewport, window, expected)
    }

    /// Replaces one viewport association only after proving its predecessor.
    ///
    /// This explicit compare-and-replace operation prevents a delayed A1
    /// callback from overwriting a recreated A2 native binding.
    ///
    /// # Errors
    ///
    /// Returns an error when the predecessor is stale, the successor changes
    /// the logical surface, or a peer identity is already bound elsewhere.
    pub fn replace_viewport(
        &mut self,
        viewport: ViewportId,
        expected: NativeSurfaceBinding,
        successor_window: WindowId,
        successor: NativeSurfaceBinding,
    ) -> Result<(), NativeViewportBindingError> {
        if !self.session.is_current_native_binding(successor) {
            return Err(NativeViewportBindingError::BindingNotCurrent {
                viewport,
                surface: successor.surface(),
            });
        }
        self.bridge.replace_viewport_route(
            viewport,
            expected,
            successor,
            crate::mailbox::NativeViewportReplacement::Attach(successor_window),
        )?;
        self.receivers.retire_binding(expected);
        Ok(())
    }

    /// Replaces one exact viewport reservation before the successor window exists.
    ///
    /// The predecessor window route is removed immediately, so any late callback from it becomes
    /// unknown rather than acquiring the successor's authority.
    ///
    /// # Errors
    ///
    /// Returns an error when the predecessor is stale or the successor changes logical surface.
    pub fn reserve_viewport_replacement(
        &mut self,
        viewport: ViewportId,
        expected: NativeSurfaceBinding,
        successor: NativeSurfaceBinding,
    ) -> Result<(), NativeViewportBindingError> {
        if !self.session.is_current_native_binding(successor) {
            return Err(NativeViewportBindingError::BindingNotCurrent {
                viewport,
                surface: successor.surface(),
            });
        }
        self.bridge.replace_viewport_route(
            viewport,
            expected,
            successor,
            crate::mailbox::NativeViewportReplacement::Reserve,
        )?;
        self.receivers.retire_binding(expected);
        Ok(())
    }

    /// Removes one viewport association after proving its exact incarnation.
    ///
    /// # Errors
    ///
    /// Returns an error when the viewport is absent or `expected` is stale.
    pub fn unbind_viewport(
        &mut self,
        viewport: ViewportId,
        expected: NativeSurfaceBinding,
    ) -> Result<NativeSurfaceBinding, NativeViewportBindingError> {
        let removed = self.bridge.unbind_viewport_route(viewport, expected)?;
        self.receivers.retire_binding(removed);
        Ok(removed)
    }

    /// Returns the current exact binding for one eframe viewport.
    #[must_use]
    pub fn viewport_binding(&self, viewport: ViewportId) -> Option<NativeSurfaceBinding> {
        self.bridge
            .with_viewports(|viewports| viewports.binding(viewport))
    }

    /// Returns the eframe viewport currently presenting one logical surface.
    #[must_use]
    pub fn surface_viewport(&self, surface: SurfaceId) -> Option<ViewportId> {
        self.bridge
            .with_viewports(|viewports| viewports.viewport(surface))
    }

    /// Returns every child viewport which the next root pass must re-declare.
    #[cfg(test)]
    pub(crate) fn deferred_viewport_specs(&self) -> Vec<DeferredViewportSpec> {
        self.deferred_viewports.retained_specs()
    }

    pub(crate) fn prepare_deferred_viewport_specs(
        &self,
    ) -> Result<Vec<DeferredViewportSpec>, NativeRuntimeError> {
        let count = self.deferred_viewports.specs().count();
        let mut specs = Vec::new();
        specs
            .try_reserve_exact(count)
            .map_err(|_| NativeHostProtocolError::NativeReportStorageUnavailable)?;
        if specs.capacity() > count {
            return Err(NativeHostProtocolError::NativeReportStorageUnavailable.into());
        }
        specs.extend(self.deferred_viewports.specs());
        Ok(specs)
    }

    fn dispatch_queued_focus(
        &mut self,
        context: &eframe::egui::Context,
    ) -> Result<bool, NativeRuntimeError> {
        if let Some((viewport, binding, window)) = self.focus_control.queued_route() {
            if self.exact_window_route_for_binding(binding) != Some((viewport, window)) {
                return Err(NativeHostProtocolError::NativeFocusCorrelationChanged.into());
            }
            let prepared = eframe::prepare_native_viewport_focus(viewport);
            let token = prepared.token();
            if !self
                .bridge
                .reserve_focus_callback(viewport, binding, window, token)
            {
                let Some(NativeFocusTermination::Queued { viewport, request }) =
                    self.focus_control.take_queued_terminal(binding)
                else {
                    return Err(NativeHostProtocolError::NativeFocusCorrelationChanged.into());
                };
                self.effects
                    .remove_command(viewport, binding, &ViewportCommand::Focus);
                self.queue_pending_effect_result(
                    request.dispatch_failed(NativeDispatchFailure::AdapterRejected),
                    None,
                );
            } else {
                if !self.focus_control.mark_dispatched(viewport, token) {
                    return Err(NativeHostProtocolError::NativeFocusCorrelationChanged.into());
                }
                prepared.enqueue(context);
                let removed =
                    self.effects
                        .remove_command(viewport, binding, &ViewportCommand::Focus);
                debug_assert!(
                    removed,
                    "the dispatched focus request owns its queued command"
                );
            }
            return Ok(true);
        }
        Ok(false)
    }

    pub(crate) fn dispatch_viewport_commands(
        &mut self,
        context: &eframe::egui::Context,
    ) -> Result<bool, NativeRuntimeError> {
        let mut progressed = self.dispatch_queued_focus(context)?;
        for (viewport, command) in self.effects.drain_commands() {
            context.send_viewport_cmd_to(viewport, command);
            progressed = true;
        }
        Ok(progressed)
    }

    #[cfg(test)]
    pub(crate) fn take_viewport_commands(
        &mut self,
        context: &eframe::egui::Context,
    ) -> Result<Vec<(ViewportId, ViewportCommand)>, NativeRuntimeError> {
        self.dispatch_queued_focus(context)?;
        Ok(self.effects.take_commands())
    }

    pub(crate) fn pointer_passthrough_command(&self) -> Option<NativePointerPassthroughCommand> {
        self.input_control.command_to_dispatch()
    }

    pub(crate) fn dispatch_pointer_passthrough(
        &mut self,
        context: &eframe::egui::Context,
        command: NativePointerPassthroughCommand,
    ) -> bool {
        let prepared = eframe::prepare_native_viewport_pointer_passthrough(
            command.viewport(),
            command.enabled(),
        );
        if !self.mark_pointer_passthrough_dispatched(command, prepared.token()) {
            return false;
        }
        prepared.enqueue(context);
        true
    }

    pub(crate) fn mark_pointer_passthrough_dispatched(
        &mut self,
        command: NativePointerPassthroughCommand,
        token: NativeViewportPointerPassthroughCommandToken,
    ) -> bool {
        if self.input_control.command_to_dispatch() != Some(command) {
            return false;
        }
        let Some((viewport, window)) = self.exact_window_route_for_binding(command.binding())
        else {
            return false;
        };
        if viewport != command.viewport()
            || !self.bridge.reserve_pointer_callback(
                viewport,
                command.binding(),
                window,
                token,
                command.enabled(),
            )
        {
            return false;
        }
        self.input_control.mark_dispatched(command, token)
    }

    pub(crate) fn fail_pointer_passthrough_dispatch(
        &mut self,
        command: NativePointerPassthroughCommand,
    ) -> bool {
        let Some(result) = self.input_control.fail_queued_dispatch(command) else {
            return false;
        };
        self.queue_pending_effect_result(result, None);
        true
    }

    /// Classifies one exact deferred callback without reducing core state.
    pub(crate) fn deferred_viewport_paint(
        &self,
        token: NativeOutputToken,
    ) -> DeferredViewportPaint {
        let disposition = self.bridge.record_deferred_viewport_paint(token);
        let binding = match disposition {
            DeferredViewportPaint::Semantic(binding) | DeferredViewportPaint::Retain(binding) => {
                binding
            }
            DeferredViewportPaint::Created
            | DeferredViewportPaint::Staging(_)
            | DeferredViewportPaint::Waiting => return disposition,
        };
        let current = self.bridge.with_viewports(|viewports| {
            viewports.binding_for_output(
                token.viewport_id(),
                token.window_id(),
                token.create_attempt(),
            )
        });
        let validated = resolve_deferred_binding_paint(
            disposition,
            current == Some(binding),
            self.session.is_current_native_binding(binding),
            self.session.recognizes_native_binding(binding),
        );
        if validated == DeferredViewportPaint::Waiting {
            self.bridge.abandon_output(token);
        }
        validated
    }

    /// Returns the viewport which currently owns one logical surface.
    #[must_use]
    pub(crate) fn repaint_viewport(&self, surface: SurfaceId) -> Option<ViewportId> {
        self.surface_viewport(surface)
    }

    pub(crate) fn prepare_committed_retirements(
        &self,
    ) -> Result<Option<PreparedNativeRetirements>, NativeRuntimeError> {
        self.bridge.prepare_committed_route_retirements()
    }

    #[must_use = "retired output tokens must be abandoned by every output-owned sidecar"]
    pub(crate) fn commit_retirements(
        &mut self,
        prepared: PreparedNativeRetirements,
    ) -> Vec<NativeOutputToken> {
        let (committed, abandoned) = prepared.commit_with_routes();
        for retirement in &committed {
            let binding = retirement.binding();
            self.bridge.clear_create_ack(binding);
            self.bridge.clear_presentation_ack(binding);
            self.effects.remove_show(binding);
            self.terminalize_focus(binding);
            self.effects.remove_commands(binding);
            self.bridge
                .with_close_control(|control| control.retire_binding(binding));
            self.retain_retired_input_results(binding);
            self.receivers.retire_binding(binding);
        }
        for &token in &abandoned {
            self.bridge.take_pending_output(token);
            self.receivers.abandon(token);
        }
        self.bridge.retirement_commit_routes(&committed);
        abandoned
    }

    #[cfg(test)]
    pub(crate) fn settle_host_frame_inputs(
        &mut self,
        inputs: &[HostInputOutcome],
    ) -> Result<bool, NativeRuntimeError> {
        self.settle_host_frame_inputs_with_pending(inputs, false)
    }

    pub(crate) fn settle_host_frame_report(
        &mut self,
        report: &HostFrameReport,
    ) -> Result<bool, NativeRuntimeError> {
        self.settle_host_frame_inputs_with_pending(report.inputs(), report.native_input_pending())
    }

    fn settle_host_frame_inputs_with_pending(
        &mut self,
        inputs: &[HostInputOutcome],
        input_pending: bool,
    ) -> Result<bool, NativeRuntimeError> {
        let Some(queued) = self.queued_snapshot.as_mut() else {
            return Ok(false);
        };
        // Provider enrollment may publish an earlier baseline snapshot in the
        // same host frame. The coordinator queues the last snapshot input, so
        // its matching terminal is the last snapshot outcome in input order.
        if let Some(applied) = inputs.iter().rev().find_map(|input| match input {
            HostInputOutcome::NativePlatformSnapshotApplied { .. } => Some(true),
            HostInputOutcome::NativePlatformSnapshotStale
            | HostInputOutcome::NativePlatformProviderRejected => Some(false),
            _ => None,
        }) {
            queued.settlement = Some(applied);
        }
        if input_pending {
            return Ok(false);
        }
        let snapshot_applied = queued
            .settlement
            .ok_or(NativeHostProtocolError::ViewportRosterAcknowledgementMismatch)?;
        if snapshot_applied
            && !self
                .input_control
                .snapshot_settlement_matches(queued.input_acknowledgement)
        {
            return Err(NativeHostProtocolError::NativeInputCorrelationChanged.into());
        }
        if !self
            .bridge
            .settle_viewport_roster(&queued.roster, snapshot_applied)
        {
            return Err(NativeHostProtocolError::ViewportRosterAcknowledgementMismatch.into());
        }
        let queued = self
            .queued_snapshot
            .take()
            .expect("the settled snapshot remains owned");
        self.bridge.retirement_settle_snapshot(snapshot_applied);
        if snapshot_applied {
            self.work_areas.commit(queued.work_areas, &self.session);
            self.bridge.with_viewports_mut(|viewports| {
                viewports.set_focus_reportable(queued.focus_reportable)
            });
            let window_acknowledged = !queued.acknowledgements.is_empty();
            for acknowledgement in queued.acknowledgements {
                if acknowledgement.create {
                    self.bridge.clear_create_ack(acknowledgement.binding);
                }
                if acknowledgement.presentation {
                    self.bridge.clear_presentation_ack(acknowledgement.binding);
                }
            }
            if window_acknowledged {
                self.bridge.invalidate_viewport_roster();
            }
            let input_settled = self
                .input_control
                .settle_snapshot(queued.input_acknowledgement);
            debug_assert_eq!(input_settled, queued.input_acknowledgement.is_some());
            if queued.roster.is_terminal() && self.requires_terminal_snapshot() {
                self.bridge.require_terminal_roster();
            }
            Ok(true)
        } else {
            Ok(queued.roster.is_terminal())
        }
    }

    #[cfg(test)]
    pub(crate) fn settle_close_control_inputs(
        &mut self,
        inputs: &[HostInputOutcome],
    ) -> Result<bool, NativeRuntimeError> {
        let (changed, errors) = self.settle_close_control_inputs_all(inputs, false);
        match errors.into_iter().next() {
            Some(error) => Err(error),
            None => Ok(changed),
        }
    }

    pub(crate) fn settle_close_control_report(
        &mut self,
        report: &HostFrameReport,
    ) -> Result<bool, NativeRuntimeError> {
        let (changed, errors) =
            self.settle_close_control_inputs_all(report.inputs(), report.native_input_pending());
        match errors.into_iter().next() {
            Some(error) => Err(error),
            None => Ok(changed),
        }
    }

    fn settle_close_control_inputs_all(
        &mut self,
        inputs: &[HostInputOutcome],
        input_pending: bool,
    ) -> (bool, Vec<NativeRuntimeError>) {
        let mut matched_request = false;
        let (mut close_observation_applied, mut close_observation_rejected) =
            self.close_observation_settlement;
        let mut errors = Vec::new();
        for input in inputs {
            match input {
                HostInputOutcome::NativeSurfaceCloseRequested { close, plan, .. } => {
                    self.bridge
                        .with_close_control(|control| control.retain_decision_plan(*close, plan));
                }
                HostInputOutcome::NativePlatformSnapshotApplied { close_requests }
                | HostInputOutcome::NativeCloseObservationApplied { close_requests } => {
                    close_observation_applied = true;
                    for request in close_requests {
                        match self
                            .bridge
                            .with_close_control(|control| control.bind_request(*request))
                        {
                            Ok(matched) => matched_request |= matched,
                            Err(()) => {
                                errors.push(
                                    NativeHostProtocolError::NativeCloseCorrelationChanged.into(),
                                );
                            }
                        }
                    }
                }
                HostInputOutcome::NativePlatformSnapshotStale
                | HostInputOutcome::NativePlatformProviderRejected => {
                    close_observation_rejected = true;
                }
                _ => {}
            }
        }
        self.close_observation_settlement = (close_observation_applied, close_observation_rejected);
        if input_pending {
            return (matched_request, errors);
        }
        self.close_observation_settlement = (false, false);
        let clear_applied = close_observation_applied && !close_observation_rejected;
        let clear_count = self.bridge.settle_close_requests(clear_applied);
        let clear_pending = clear_count != 0;
        if clear_pending && (!close_observation_applied || close_observation_rejected) {
            errors.push(NativeHostProtocolError::NativeCloseCancellationRejected.into());
        }
        (matched_request || clear_pending, errors)
    }

    pub(crate) fn settle_native_admissions(
        &mut self,
        admissions: &[NativeSurfaceBinding],
    ) -> Result<bool, NativeRuntimeError> {
        let (changed, errors) = self.settle_native_admissions_all(admissions);
        match errors.into_iter().next() {
            Some(error) => Err(error),
            None => Ok(changed),
        }
    }

    fn settle_native_admissions_all(
        &mut self,
        admissions: &[NativeSurfaceBinding],
    ) -> (bool, Vec<NativeRuntimeError>) {
        let mut changed = false;
        let mut errors = Vec::new();
        for binding in admissions {
            let Some(viewport) = self.surface_viewport(binding.surface()) else {
                errors.push(
                    NativeHostProtocolError::NativeAdmissionRouteChanged(binding.surface()).into(),
                );
                continue;
            };
            if self.viewport_binding(viewport) != Some(*binding)
                || !self.session.is_current_native_binding(*binding)
            {
                errors.push(
                    NativeHostProtocolError::NativeAdmissionRouteChanged(binding.surface()).into(),
                );
                continue;
            }
            if self.bridge.clear_hidden_render(viewport, *binding) {
                self.lifecycle_progress.record_first_live_admission();
                changed = true;
            }
        }
        (changed, errors)
    }

    pub(crate) fn try_report_retirement_quiescence(&mut self) -> Result<bool, NativeRuntimeError> {
        let mut first_error = None;
        let recorded = self.try_report_retirement_quiescence_into(|error| {
            first_error.get_or_insert(error);
        });
        match first_error {
            Some(error) => Err(error),
            None => Ok(recorded),
        }
    }

    fn try_report_retirement_quiescence_into(
        &mut self,
        mut on_error: impl FnMut(NativeRuntimeError),
    ) -> bool {
        self.bridge.admission_dispatch();
        if !self.bridge.permits_unscheduled_input() {
            return false;
        }
        let mut recorded = false;
        let mut after = None;
        while let Some(binding) = self.bridge.next_retirement_quiescence_candidate(after) {
            after = Some(binding);
            if self.binding_has_retained_host_references(binding) {
                continue;
            }
            if self.session.recognizes_native_binding(binding)
                && let Err(error) = self.session.report_native_binding_quiescence(binding)
            {
                on_error(error.into());
                continue;
            }
            let finished = self.bridge.retirement_finish_quiescence(binding);
            debug_assert!(finished, "quiescence candidate remains pending");
            if finished {
                self.pending_effect_results
                    .retain(|pending| !pending.can_be_consumed_by_quiescence(binding));
                self.lifecycle_progress.record_quiescent_retirement();
            }
            recorded = true;
        }
        recorded
    }

    fn binding_has_retained_host_references(&self, binding: NativeSurfaceBinding) -> bool {
        self.bridge.references_binding(binding)
            || self.bridge.retirement_blocks_quiescence(binding)
            || self
                .bridge
                .with_viewports(|routes| routes.bindings().any(|route| route == binding))
            || self.pointer_translator.references_binding(binding)
            || self.receivers.references_binding(binding)
            || self.deferred_viewports.viewport_for(binding).is_some()
            || self.effects.references_binding(binding)
            || self
                .pending_effect_results
                .iter()
                .any(|pending| pending.blocks_quiescence(binding))
            || self.bridge.retirement_references_cleanup(binding)
            || self.focus_control.references_binding(binding)
            || self.input_control.references_binding(binding)
    }

    pub(crate) fn try_report_pending_effect_results(&mut self) -> Result<bool, NativeRuntimeError> {
        let (reported, error) = self.report_pending_effect_results_until_blocked();
        match error {
            Some(error) => Err(error),
            None => Ok(reported),
        }
    }

    fn report_pending_effect_results_until_blocked(
        &mut self,
    ) -> (bool, Option<NativeRuntimeError>) {
        self.bridge.admission_dispatch();
        if !self.bridge.permits_unscheduled_input() {
            return (false, None);
        }
        let mut reported = false;
        let mut index = 0;
        while index < self.pending_effect_results.len() {
            let pending = &self.pending_effect_results[index];
            let binding = pending.result.binding();
            if pending.last_rejection == Some(NativeHostErrorKind::StaleBinding) {
                if !self.session.recognizes_native_binding(binding) {
                    let _ = self.pending_effect_results.remove(index);
                    continue;
                }
                if self.bridge.retirement_is_quiescence_candidate(binding) {
                    index += 1;
                    continue;
                }
            }
            let mut pending = self
                .pending_effect_results
                .remove(index)
                .expect("the indexed pending result remains owned until its attempt");
            if let Err(error) = self.session.report_native_effect_result(pending.result) {
                let (kind, result) = error.into_parts();
                pending.result = result;
                pending.last_rejection = Some(kind);
                if kind == NativeHostErrorKind::StaleBinding {
                    self.bridge.retirement_mark_create_failure_recorded(binding);
                    if !self.session.recognizes_native_binding(binding) {
                        continue;
                    }
                    if self.bridge.retirement_is_quiescence_candidate(binding) {
                        self.pending_effect_results.insert(index, pending);
                        index += 1;
                        continue;
                    }
                }
                // Restore into the same backing and original ordered position.
                self.pending_effect_results.insert(index, pending);
                let error = (!native_effect_submission_rejection_is_recoverable(kind))
                    .then(|| NativeHostProtocolError::NativeEffectResultRejected(kind).into());
                return (reported, error);
            }
            self.bridge.retirement_mark_create_failure_recorded(binding);
            reported = true;
        }
        (reported, None)
    }

    fn queue_pending_effect_result(
        &mut self,
        result: NativeEffectResult,
        last_rejection: Option<NativeHostErrorKind>,
    ) {
        self.pending_effect_results
            .push_back(PendingNativeEffectResult {
                result,
                last_rejection,
            });
    }

    fn retain_retired_input_results(&mut self, binding: NativeSurfaceBinding) {
        let results = &mut self.pending_effect_results;
        self.input_control.retire_binding_into(binding, |result| {
            results.push_back(PendingNativeEffectResult {
                result,
                last_rejection: None,
            });
        });
    }

    fn terminalize_focus(&mut self, binding: NativeSurfaceBinding) {
        let Some(termination) = self.focus_control.take_terminal(binding) else {
            return;
        };
        self.queue_focus_termination(binding, termination);
    }

    fn terminalize_queued_focus(&mut self, binding: NativeSurfaceBinding) {
        let Some(termination) = self.focus_control.take_queued_terminal(binding) else {
            return;
        };
        self.queue_focus_termination(binding, termination);
    }

    fn queue_focus_termination(
        &mut self,
        binding: NativeSurfaceBinding,
        termination: NativeFocusTermination,
    ) {
        let result = match termination {
            NativeFocusTermination::Queued { viewport, request } => {
                let removed =
                    self.effects
                        .remove_command(viewport, binding, &ViewportCommand::Focus);
                debug_assert!(
                    removed,
                    "a queued focus request retains its viewport command"
                );
                request.dispatch_failed(NativeDispatchFailure::ProviderStopped)
            }
            NativeFocusTermination::Dispatched(request) => {
                request.indeterminate(NativeIndeterminateReason::AcknowledgementLost)
            }
        };
        self.queue_pending_effect_result(result, None);
    }

    /// Reduces one exact deferred-viewport failure without losing the affine
    /// effect result across a retryable core rejection.
    pub(crate) fn reduce_next_viewport_create_failure(
        &mut self,
    ) -> Result<bool, NativeRuntimeError> {
        let Some(failure) = self.next_viewport_create_failure()? else {
            return Ok(false);
        };
        let Some(result) = self.effects.take_failure_result(failure) else {
            let failure_is_stale = self.viewport_binding(failure.viewport())
                != Some(failure.binding())
                || !self.session.is_current_native_binding(failure.binding());
            if failure_is_stale {
                self.acknowledge_viewport_create_failure(failure, true)?;
                return Ok(true);
            }
            return Err(NativeHostProtocolError::ViewportCreateFailureWithoutEffect.into());
        };
        let (result_recorded, callback_error) =
            match self.session.report_native_effect_result(result) {
                Ok(()) => (true, None),
                Err(error) => {
                    let (kind, result) = error.into_parts();
                    (
                        kind == NativeHostErrorKind::StaleBinding,
                        self.retain_rejected_effect_result(kind, result).err(),
                    )
                }
            };
        self.acknowledge_viewport_create_failure(failure, result_recorded)?;
        if let Some(error) = callback_error {
            self.callback_errors.push_back(error);
        }
        Ok(true)
    }

    /// Returns the next event which is safe to translate.
    ///
    /// A presentation result at the journal head is submitted first and forms
    /// a frame barrier. Conversely, an acknowledged event prefix must commit
    /// before a later presentation result can enter the core prelude. This
    /// prevents the runtime's presentation phase from overtaking older native
    /// input recorded in the backend ingress lane.
    ///
    /// # Errors
    ///
    /// Returns an error when a terminal output is still waiting for its affine
    /// painted output or core rejects its presentation result.
    #[cfg(test)]
    pub fn next_window_event(
        &mut self,
    ) -> Result<Option<NativeWindowEventRecord>, NativeRuntimeError> {
        Ok(self.bridge.front_event())
    }

    /// Acknowledges the exact event after its typed facts have been accepted.
    ///
    /// The event remains in the journal until this method succeeds, so a
    /// failed translation or dropped host frame can retry the same raw edge.
    ///
    /// # Errors
    ///
    /// Returns an error unless `ordinal` names the exact journal-head event.
    pub fn acknowledge_window_event(&self, ordinal: u64) -> Result<(), NativeRuntimeError> {
        if self.bridge.acknowledge_event(ordinal) {
            Ok(())
        } else {
            Err(NativeHostProtocolError::WindowEventAcknowledgementMismatch.into())
        }
    }

    fn next_device_removal(
        &mut self,
    ) -> Result<Option<NativeDeviceRemovalRecord>, NativeRuntimeError> {
        Ok(self.bridge.front_device_removal())
    }

    fn acknowledge_device_removal(
        &self,
        removal: NativeDeviceRemovalRecord,
    ) -> Result<(), NativeRuntimeError> {
        if self.bridge.acknowledge_device_removal(removal) {
            Ok(())
        } else {
            Err(NativeHostProtocolError::DeviceRemovalAcknowledgementMismatch.into())
        }
    }

    /// Returns the next exact deferred-viewport creation failure in callback order.
    ///
    /// The binding is frozen when eframe reports the failed OS attempt, so replacing the viewport
    /// before the next root update cannot retarget the failure to a newer incarnation.
    ///
    /// # Errors
    ///
    /// Returns an error when an earlier renderer settlement cannot be applied.
    fn next_viewport_create_failure(
        &mut self,
    ) -> Result<Option<NativeViewportCreateFailureRecord>, NativeRuntimeError> {
        Ok(self.bridge.front_viewport_create_failure())
    }

    fn next_viewport_visibility(
        &mut self,
    ) -> Result<Option<NativeViewportVisibilityRecord>, NativeRuntimeError> {
        Ok(self.bridge.front_viewport_visibility())
    }

    /// Acknowledges one exact viewport creation failure after the driver has converted it into the
    /// matching affine native-effect result.
    ///
    /// # Errors
    ///
    /// Returns an error unless `failure` is the callback journal head.
    fn acknowledge_viewport_create_failure(
        &mut self,
        failure: NativeViewportCreateFailureRecord,
        result_recorded: bool,
    ) -> Result<(), NativeRuntimeError> {
        if !self
            .bridge
            .acknowledge_viewport_create_failure(failure, result_recorded)
        {
            return Err(
                NativeHostProtocolError::ViewportCreateFailureAcknowledgementMismatch.into(),
            );
        }
        self.bridge
            .clear_create(failure.viewport(), failure.binding());
        self.bridge
            .clear_hidden_render(failure.viewport(), failure.binding());
        self.deferred_viewports.remove(failure.binding());
        self.receivers.retire_binding(failure.binding());
        Ok(())
    }

    fn acknowledge_viewport_visibility(
        &self,
        record: NativeViewportVisibilityRecord,
    ) -> Result<(), NativeRuntimeError> {
        if self.bridge.acknowledge_viewport_visibility(record) {
            Ok(())
        } else {
            Err(NativeHostProtocolError::ViewportVisibilityAcknowledgementMismatch.into())
        }
    }

    /// Retains one bounded callback prefix for an exact host-frame boundary.
    ///
    /// The mailbox remains the only callback-order authority. Pointer facts,
    /// close requests, exact destruction events, viewport rosters, and create
    /// failures are acknowledged only after their corresponding core input is
    /// retained. Same-route cursor motions share one prefix of at most 256
    /// semantic edges; other callbacks remain causal barriers. The driver must
    /// commit or retry that retained prefix before claiming its successor.
    pub(crate) fn reduce_callback_prefix(
        &mut self,
        context: &eframe::egui::Context,
        budget: &mut NativeInputBudget,
    ) -> Result<Option<std::num::NonZeroUsize>, NativeRuntimeError> {
        self.bridge.admission_dispatch();
        let recorded_before = self.session.native_input_recorded_count();
        let allowance_before = budget.remaining_records();
        let result = self
            .prepare_output_prefix(budget)
            .and_then(|()| self.reduce_callback_prefix_inner(context, budget));
        let result = match result {
            Err(error) if error.is_input_capacity() && self.session.has_pending_native_input() => {
                self.bridge.defer_for_core_input_capacity();
                Ok(None)
            }
            result => result,
        };
        if let Ok(Some(count)) = &result
            && self.session.native_input_recorded_count() == recorded_before
            && budget.remaining_records() == allowance_before
        {
            budget.complete_unsequenced_records(count.get());
        }
        result
    }

    fn reduce_callback_prefix_inner(
        &mut self,
        context: &eframe::egui::Context,
        budget: &mut NativeInputBudget,
    ) -> Result<Option<std::num::NonZeroUsize>, NativeRuntimeError> {
        if budget.is_exhausted() || self.bridge.callback_boundary_pending() {
            return Ok(None);
        }
        if !matches!(
            self.bridge.admission_dispatch(),
            NativeDispatch::Replay { .. }
        ) {
            return Ok(None);
        }
        if self.reduce_next_viewport_close_cancelled()? {
            return Ok(Some(std::num::NonZeroUsize::MIN));
        }
        if self.reduce_next_viewport_roster()? {
            return Ok(Some(std::num::NonZeroUsize::MIN));
        }
        if self.reduce_next_viewport_create_failure()? {
            return Ok(Some(std::num::NonZeroUsize::MIN));
        }
        if self.reduce_next_viewport_visibility()? {
            return Ok(Some(std::num::NonZeroUsize::MIN));
        }
        if self.reduce_next_viewport_created()? {
            return Ok(Some(std::num::NonZeroUsize::MIN));
        }
        if self.reduce_next_staging_painted(context, budget)? {
            return Ok(Some(std::num::NonZeroUsize::MIN));
        }
        if self.reduce_next_viewport_pointer_passthrough()? {
            return Ok(Some(std::num::NonZeroUsize::MIN));
        }
        if self.reduce_next_global_focus()? {
            return Ok(Some(std::num::NonZeroUsize::MIN));
        }
        if self.reduce_next_viewport_focus()? {
            return Ok(Some(std::num::NonZeroUsize::MIN));
        }
        if let Some(removal) = self.next_device_removal()? {
            self.session.report_native_input_device_removal()?;
            self.pointer_translator.forget_removed_devices();
            self.bridge
                .release_pointer_translation_capacity(&self.pointer_translator);
            self.acknowledge_device_removal(removal)?;
            return Ok(Some(std::num::NonZeroUsize::MIN));
        }
        if !self.session.has_active_gesture()
            && self.pointer_translator.can_coalesce_idle_cursor_moves()
        {
            self.bridge.coalesce_idle_cursor_prefix(budget);
        }
        if budget.is_exhausted() {
            return Ok(None);
        }
        let bridge = Arc::clone(&self.bridge);
        if let Some(prefix) = bridge.claim_cursor_prefix(budget.remaining_records()) {
            let pointer_capacity = self.session.prepare_native_pointer_capacity()?;
            if pointer_capacity == Some(0) {
                drop(prefix);
                self.bridge.defer_for_core_input_capacity();
                return Ok(None);
            }
            let prefix_limit = pointer_capacity.map_or(budget.remaining_records(), |capacity| {
                capacity.min(budget.remaining_records())
            });
            // Raw admission prepaid both current state and candidate growth.
            let mut candidate = self.pointer_translator.clone();
            let mut consumed = 0;
            let mut ignored = 0;
            {
                let mut events = prefix.captured_events().take(prefix_limit);
                let inputs = std::iter::from_fn(|| {
                    loop {
                        if budget.is_exhausted() {
                            return None;
                        }
                        let (record, captured_at) = events.next()?;
                        consumed += 1;
                        match candidate.translate_captured(record, captured_at, |point| {
                            self.work_areas.binding_at(point)
                        }) {
                            NativePointerTranslation::Input(input) => return Some(input),
                            NativePointerTranslation::Ignored => ignored += 1,
                            NativePointerTranslation::NotPointer => unreachable!(
                                "a cursor prefix claim contains only immutable cursor-moved records"
                            ),
                        }
                    }
                });
                self.session.record_native_pointer_sequence(inputs)?;
            }
            let Some(count) = std::num::NonZeroUsize::new(consumed) else {
                return Ok(None);
            };
            if !prefix.retain_translated_prefix(count, &mut self.pointer_translator, candidate) {
                return Err(NativeHostProtocolError::WindowEventAcknowledgementMismatch.into());
            }
            budget.complete_unsequenced_records(ignored);
            return Ok(Some(count));
        }
        let Some((record, captured_at)) = self.bridge.front_captured_event() else {
            return Ok(None);
        };
        if matches!(record.event(), winit::event::WindowEvent::Destroyed)
            && let Some(binding) = record.binding()
        {
            let bridge = Arc::clone(&self.bridge);
            bridge.retire_destroyed_input(&record, || {
                self.session.report_native_input_binding_retired(binding)?;
                // Install local suppression and receipt progress before any
                // close correlation, effect settlement, or retirement can fail.
                self.pointer_translator.retire_binding(binding);
                Ok(())
            })?;
            bridge.release_pointer_translation_capacity(&self.pointer_translator);
        }
        if NativeWindowEventClass::classify(record.event()) != NativeWindowEventClass::Pointer {
            self.reduce_non_pointer_event(&record)?;
            self.acknowledge_window_event(record.ordinal())?;
            return Ok(Some(std::num::NonZeroUsize::MIN));
        }
        // Keep the original state until core retention and exact source transfer.
        // The accepted raw callback already owns the overlapping storage promise.
        let mut candidate = self.pointer_translator.clone();
        let work_areas = &self.work_areas;
        match candidate
            .translate_captured(&record, captured_at, |point| work_areas.binding_at(point))
        {
            NativePointerTranslation::NotPointer => {
                unreachable!("pointer translation follows the same immutable event classification")
            }
            NativePointerTranslation::Ignored => budget.complete_unsequenced_records(1),
            NativePointerTranslation::Input(input) => self.record_pointer(input)?,
        }
        if !self.bridge.acknowledge_translated_event(
            record.ordinal(),
            &mut self.pointer_translator,
            candidate,
        ) {
            return Err(NativeHostProtocolError::WindowEventAcknowledgementMismatch.into());
        }
        Ok(Some(std::num::NonZeroUsize::MIN))
    }

    pub(crate) fn has_pending_output_prefix(&self) -> bool {
        self.bridge.has_pending_output_prefix()
    }

    /// Returns exact renderer terminals whose egui pass never reached the
    /// host-owned finalizer and therefore has no core output attachment.
    ///
    /// An unwind produces `NotPresented` for the reserved token. The adapter
    /// must retire its staged pass before ordinary output-prefix reduction;
    /// otherwise the coordinator would correctly reject the missing affine
    /// output as `OutputAwaitingAttachment`.
    pub(crate) fn unfinalized_output_prefix(&self) -> NativeOutputPrefix {
        self.bridge.output_prefix(256)
    }

    pub(crate) fn next_unfinalized_not_presented_output(
        &self,
        prefix: &mut NativeOutputPrefix,
    ) -> Result<Option<NativeOutputToken>, NativeRuntimeError> {
        while let Some(entry) = self.bridge.next_output_prefix(prefix)? {
            if output_terminal_requires_adapter_abandonment(
                entry.result.status(),
                entry.submitted,
                entry.attached,
            ) {
                return Ok(Some(entry.result.token()));
            }
        }
        Ok(None)
    }

    pub(crate) fn has_pending_callback_records(&self) -> bool {
        self.bridge.has_pending_callback_records()
    }

    pub(crate) fn require_output_wake_after_current(&self, token: NativeOutputToken) -> bool {
        self.bridge.require_output_wake_after_current(token)
    }

    fn reduce_next_viewport_pointer_passthrough(&mut self) -> Result<bool, NativeRuntimeError> {
        let Some(record) = self.bridge.front_viewport_pointer_passthrough() else {
            return Ok(false);
        };
        let disposition = self.input_control.classify(record);
        let mut callback_error = None;
        match disposition {
            NativeInputResultDisposition::Unrelated => {
                if record.status() == NativeViewportPointerPassthroughStatus::Applied
                    && let Some(binding) = record.binding()
                {
                    let route = self.exact_window_route_for_binding(binding);
                    if route == Some((record.viewport(), record.window()))
                        && self.session.is_current_native_binding(binding)
                        && self.input_control.observe_unowned_applied(record)?
                    {
                        self.bridge.invalidate_viewport_roster();
                    }
                }
            }
            NativeInputResultDisposition::Current { binding, status }
                if self.session.is_current_native_binding(binding)
                    && self.exact_window_route_for_binding(binding)
                        == Some((record.viewport(), record.window())) =>
            {
                match status {
                    NativeViewportPointerPassthroughStatus::Applied => {
                        if !self.input_control.accept_applied(record)? {
                            return Err(
                                NativeHostProtocolError::NativeInputCorrelationChanged.into()
                            );
                        }
                        self.bridge.invalidate_viewport_roster();
                    }
                    NativeViewportPointerPassthroughStatus::Unsupported => {
                        let result = self
                            .input_control
                            .take_unsupported_result(record)
                            .ok_or(NativeHostProtocolError::NativeInputCorrelationChanged)?;
                        callback_error = self.report_callback_terminal_result(result);
                    }
                    NativeViewportPointerPassthroughStatus::Failed
                    | NativeViewportPointerPassthroughStatus::Retired => {
                        let result = self
                            .input_control
                            .take_dispatch_failure_result(
                                record,
                                NativeDispatchFailure::AdapterRejected,
                            )
                            .ok_or(NativeHostProtocolError::NativeInputCorrelationChanged)?;
                        callback_error = self.report_callback_terminal_result(result);
                    }
                }
            }
            NativeInputResultDisposition::Current { binding, .. }
            | NativeInputResultDisposition::Stale { binding } => {
                let result = self
                    .input_control
                    .take_dispatch_failure_result(record, NativeDispatchFailure::AdapterRejected)
                    .ok_or(NativeHostProtocolError::NativeInputCorrelationChanged)?;
                callback_error = self.report_callback_terminal_result(result);
                debug_assert!(
                    !self.session.is_current_native_binding(binding)
                        || self.exact_window_route_for_binding(binding)
                            != Some((record.viewport(), record.window())),
                    "a current input callback must take the exact route branch"
                );
            }
        }
        if !self.bridge.acknowledge_viewport_pointer_passthrough(record) {
            return Err(
                NativeHostProtocolError::ViewportPointerPassthroughAcknowledgementMismatch.into(),
            );
        }
        if let Some(error) = callback_error {
            self.callback_errors.push_back(error);
        }
        Ok(true)
    }

    fn report_callback_terminal_result(
        &mut self,
        result: NativeEffectResult,
    ) -> Option<NativeRuntimeError> {
        if let Err(error) = self.session.report_native_effect_result(result) {
            let (kind, result) = error.into_parts();
            self.retain_rejected_effect_result(kind, result).err()
        } else {
            None
        }
    }

    pub(crate) fn take_callback_error(&mut self) -> Option<NativeRuntimeError> {
        self.callback_errors.pop_front()
    }

    fn take_callback_errors(&mut self) -> Vec<NativeRuntimeError> {
        self.callback_errors.drain(..).collect()
    }

    fn reduce_next_global_focus(&mut self) -> Result<bool, NativeRuntimeError> {
        let Some(record) = self.bridge.front_global_focus() else {
            return Ok(false);
        };
        debug_assert_ne!(record.event(), 0, "native event ordinals are non-zero");
        self.session.report_native_global_focus(record.focus())?;
        if !self.bridge.acknowledge_global_focus(record) {
            return Err(NativeHostProtocolError::GlobalFocusAcknowledgementMismatch.into());
        }
        if let NativeGlobalFocus::Dock(binding) = record.focus()
            && let Some(request) = self.focus_control.take_observed(binding)
            && request.accepted().is_some()
        {
            return Err(NativeHostProtocolError::UnexpectedViewportEffectAcknowledgement.into());
        }
        Ok(true)
    }

    fn reduce_next_viewport_focus(&mut self) -> Result<bool, NativeRuntimeError> {
        let Some(record) = self.bridge.front_viewport_focus() else {
            return Ok(false);
        };
        let disposition = self.focus_control.classify(record);
        let mut callback_error = None;
        match disposition {
            NativeFocusResultDisposition::Unrelated => {}
            NativeFocusResultDisposition::Current { binding, status }
                if self.session.is_current_native_binding(binding)
                    && self.bridge.with_viewports(|viewports| {
                        viewports.focus_binding_for_event(record.window(), Some(record.viewport()))
                    }) == Some(binding) =>
            {
                match status {
                    NativeViewportFocusStatus::AlreadyFocused => {
                        self.session
                            .report_native_global_focus(NativeGlobalFocus::Dock(binding))?;
                        let request = self
                            .focus_control
                            .take_callback(record)
                            .ok_or(NativeHostProtocolError::NativeFocusCorrelationChanged)?;
                        if request.accepted().is_some() {
                            return Err(
                                NativeHostProtocolError::UnexpectedViewportEffectAcknowledgement
                                    .into(),
                            );
                        }
                    }
                    NativeViewportFocusStatus::Requested => {
                        if !self.focus_control.mark_requested(record) {
                            return Err(
                                NativeHostProtocolError::NativeFocusCorrelationChanged.into()
                            );
                        }
                    }
                    NativeViewportFocusStatus::Retired => {
                        let request = self
                            .focus_control
                            .take_callback(record)
                            .ok_or(NativeHostProtocolError::NativeFocusCorrelationChanged)?;
                        callback_error = self.report_callback_terminal_result(
                            request.dispatch_failed(NativeDispatchFailure::AdapterRejected),
                        );
                    }
                }
            }
            NativeFocusResultDisposition::Current { binding, .. }
            | NativeFocusResultDisposition::Stale { binding } => {
                let request = self
                    .focus_control
                    .take_callback(record)
                    .ok_or(NativeHostProtocolError::NativeFocusCorrelationChanged)?;
                let result = request.dispatch_failed(NativeDispatchFailure::AdapterRejected);
                callback_error = self.report_callback_terminal_result(result);
                debug_assert!(
                    !self.session.is_current_native_binding(binding)
                        || self.bridge.with_viewports(|viewports| {
                            viewports
                                .focus_binding_for_event(record.window(), Some(record.viewport()))
                        }) != Some(binding),
                    "a current focus callback must take the exact route branch"
                );
            }
        }
        if !self.bridge.acknowledge_viewport_focus(record) {
            return Err(NativeHostProtocolError::ViewportFocusAcknowledgementMismatch.into());
        }
        if let Some(error) = callback_error {
            self.callback_errors.push_back(error);
        }
        Ok(true)
    }

    fn reduce_next_viewport_close_cancelled(&mut self) -> Result<bool, NativeRuntimeError> {
        let Some(record) = self.bridge.front_viewport_close_cancelled() else {
            return Ok(false);
        };
        let Some((binding, acknowledgement)) = self
            .bridge
            .with_close_control(|control| control.cancellation_observation(record))
        else {
            if !self.bridge.acknowledge_viewport_close_cancelled(record) {
                return Err(NativeHostProtocolError::NativeCloseCorrelationChanged.into());
            }
            return Ok(true);
        };
        self.publish_close(binding, NativeCloseState::Clear, Some(acknowledgement))?;
        if !self.bridge.acknowledge_viewport_close_cancelled(record)
            || !self
                .bridge
                .with_close_control(|control| control.mark_clear_recorded(binding))
        {
            return Err(NativeHostProtocolError::NativeCloseCorrelationChanged.into());
        }
        Ok(true)
    }

    fn reduce_next_viewport_visibility(&mut self) -> Result<bool, NativeRuntimeError> {
        let Some(record) = self.next_viewport_visibility()? else {
            return Ok(false);
        };
        self.bridge.invalidate_viewport_roster();
        let current = self.bridge.with_viewports(|viewports| {
            viewports.binding_for_event(record.window(), Some(record.viewport()))
        });
        if current != Some(record.binding())
            || !self.session.is_current_native_binding(record.binding())
        {
            let callback_error = self.effects.take_show(record).and_then(|request| {
                self.report_callback_terminal_result(
                    request.dispatch_failed(NativeDispatchFailure::AdapterRejected),
                )
            });
            self.acknowledge_viewport_visibility(record)?;
            if let Some(error) = callback_error {
                self.callback_errors.push_back(error);
            }
            return Ok(true);
        }
        if !record.visible() {
            self.acknowledge_viewport_visibility(record)?;
            return Ok(true);
        }
        if record.status() == NativeViewportVisibilityStatus::Dispatched
            && self.effects.matches_show(record)
            && !self
                .bridge
                .can_retain_presentation_ack(record.viewport(), record.binding())
        {
            return Err(NativeHostProtocolError::PresentationAcknowledgementAlreadyPending.into());
        }
        let Some(request) = self.effects.take_show(record) else {
            self.acknowledge_viewport_visibility(record)?;
            return Ok(true);
        };
        let callback_error = match record.status() {
            NativeViewportVisibilityStatus::Dispatched => {
                let Some(NativeEffectAcknowledgement::Presentation(acknowledgement)) =
                    request.accepted()
                else {
                    return Err(
                        NativeHostProtocolError::UnexpectedViewportEffectAcknowledgement.into(),
                    );
                };
                self.bridge
                    .retain_presentation_ack(record.viewport(), record.binding(), acknowledgement)
                    .expect("the exact visibility callback retained its prepaid presentation slot");
                self.bridge.invalidate_viewport_roster();
                None
            }
            NativeViewportVisibilityStatus::Unsupported => {
                let result = request.unsupported(NativeUnsupportedReason::BackendUnsupported);
                self.report_callback_terminal_result(result)
            }
            NativeViewportVisibilityStatus::Retired => self.report_callback_terminal_result(
                request.dispatch_failed(NativeDispatchFailure::AdapterRejected),
            ),
        };
        self.acknowledge_viewport_visibility(record)?;
        if let Some(error) = callback_error {
            self.callback_errors.push_back(error);
        }
        Ok(true)
    }

    fn has_pending_presentation_effect(&self, binding: NativeSurfaceBinding) -> bool {
        self.bridge.presentation_ack(binding).is_some() || self.effects.has_pending_show(binding)
    }

    fn reduce_next_viewport_created(&mut self) -> Result<bool, NativeRuntimeError> {
        let Some(created) = self.bridge.front_viewport_created() else {
            return Ok(false);
        };
        if !self
            .bridge
            .can_retain_create_ack(created.token().viewport_id(), created.binding())
        {
            return Err(NativeHostProtocolError::CreateAcknowledgementAlreadyPending.into());
        }
        let attach = self.bridge.with_viewports_mut(|viewports| {
            viewports.attach_output(
                created.token().viewport_id(),
                created.binding(),
                created.token().window_id(),
                created.token().create_attempt(),
            )
        });
        if attach.is_err() {
            return Err(NativeHostProtocolError::OutputRouteAttachmentFailed.into());
        }
        let request = self
            .effects
            .take_for_output(created.token().viewport_id(), created.binding())
            .ok_or(NativeHostProtocolError::UnexpectedViewportEffectAcknowledgement)?;
        let Some(NativeEffectAcknowledgement::Create(acknowledgement)) = request.accepted() else {
            return Err(NativeHostProtocolError::UnexpectedViewportEffectAcknowledgement.into());
        };
        self.bridge
            .retain_create_ack(
                created.token().viewport_id(),
                created.binding(),
                acknowledgement,
            )
            .expect("the exact create callback retained its prepaid creation observation");
        self.bridge.invalidate_viewport_roster();
        if !self.bridge.acknowledge_viewport_created(created) {
            return Err(NativeHostProtocolError::OutputRouteAttachmentFailed.into());
        }
        assert!(
            self.bridge
                .clear_create(created.token().viewport_id(), created.binding())
        );
        Ok(true)
    }

    fn reduce_next_staging_painted(
        &mut self,
        context: &eframe::egui::Context,
        budget: &mut NativeInputBudget,
    ) -> Result<bool, NativeRuntimeError> {
        let Some(staging) = self.bridge.front_staging_painted() else {
            return Ok(false);
        };
        if self.bridge.has_pending_output(staging.token()) {
            return Err(NativeHostProtocolError::OutputAwaitingAttachment.into());
        }
        let Some(mut frame) = self
            .session
            .poll_native_host_frame(budget, |_| NativeReceiverAnswer::Unknown)?
        else {
            self.bridge.hold_native_boundary();
            return Ok(false);
        };
        // Replay and the staging callback are distinct records. Spending the
        // final credit on its predecessors must leave staging at the raw head.
        if frame.native_input_pending() || budget.is_exhausted() {
            let mut frame =
                NativeHostFrame::new(frame, self.bridge.clone(), &mut self.receivers, None);
            frame.complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)?;
            let report = frame.commit()?;
            self.finish_internal_report(context, report)?;
            return Ok(false);
        }
        if !self.bridge.accepts_staging_paint(staging.token()) {
            drop(frame);
            self.discard_obsolete_staging_paint(staging)?;
            budget.complete_unsequenced_records(1);
            return Ok(true);
        }
        if let Err(error) = frame.confirm_native_staging_painted(staging.request()) {
            if staging_paint_failure_disposition(error.native_kind())
                == StagingPaintFailureDisposition::Fatal
            {
                return Err(error.into());
            }
            drop(frame);
            self.discard_obsolete_staging_paint(staging)?;
            budget.complete_unsequenced_records(1);
            return Ok(true);
        }
        frame.complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)?;
        let mut report = frame.commit()?;
        self.receivers
            .commit_presentations(report.surface_presentation_commits());
        self.retain_internal_presentation_transitions(&report);
        self.settle_host_frame_report(&report)?;
        self.settle_close_control_report(&report)?;
        self.settle_native_admissions(report.native_admissions())?;
        let mut outputs = report.take_painted_native_staging_outputs();
        if outputs.len() != 1 {
            let actual = outputs.len();
            drop(outputs);
            return Err(NativeHostProtocolError::PaintedStagingOutputCountMismatch {
                expected: 1,
                actual,
            }
            .into());
        }
        let output = outputs.pop().expect("one staging output was checked");
        if output.binding() != staging.request().binding()
            || !self
                .bridge
                .attach_output_binding(staging.token(), output.binding())
            || !self
                .bridge
                .require_output_wake_after_current(staging.token())
        {
            drop(output);
            return Err(NativeHostProtocolError::OutputRouteAttachmentFailed.into());
        }
        if let Err(output) = self.bridge.bind_staging_output(staging.token(), output) {
            drop(output);
            return Err(NativeHostProtocolError::OutputRouteAttachmentFailed.into());
        }
        if !self.bridge.acknowledge_staging_painted(staging) {
            return Err(NativeHostProtocolError::OutputRouteAttachmentFailed.into());
        }
        self.accept_native_effects(context, report.take_native_effects())?;
        self.bridge
            .commit_frame_boundary(report.native_input_pending());
        budget.complete_unsequenced_records(1);
        Ok(true)
    }

    fn discard_obsolete_staging_paint(
        &mut self,
        staging: NativeStagingPaintRecord,
    ) -> Result<(), NativeRuntimeError> {
        if !self.bridge.acknowledge_staging_painted(staging) {
            return Err(NativeHostProtocolError::OutputRouteAttachmentFailed.into());
        }
        // The terminal may have already retired an abandoned output. Its exact
        // staging ingress still transfers and commits without reopening paint.
        let _ = self.abandon_output_token(staging.token());
        Ok(())
    }

    fn reduce_non_pointer_event(
        &mut self,
        record: &NativeWindowEventRecord,
    ) -> Result<(), NativeRuntimeError> {
        let Some(binding) = record.binding() else {
            return Ok(());
        };
        match record.event() {
            winit::event::WindowEvent::CloseRequested => {
                if self
                    .bridge
                    .with_close_control(|control| control.needs_request_recording(record, binding))
                {
                    self.publish_close(binding, NativeCloseState::Requested, None)?;
                    if !self.bridge.with_close_control(|control| {
                        control.mark_request_recorded(record, binding)
                    }) {
                        return Err(NativeHostProtocolError::NativeCloseCorrelationChanged.into());
                    }
                }
            }
            winit::event::WindowEvent::Destroyed => {
                let accepted_close = self.bridge.with_close_control(|control| {
                    control
                        .accepted_destroyed_matches(record, binding)
                        .map_err(|()| NativeHostProtocolError::NativeCloseCorrelationChanged)
                })?;
                if let Some(viewport) = record.viewport_id() {
                    self.fail_pending_viewport_effect(
                        viewport,
                        binding,
                        ViewportFailureOrigin::CapturedCallback,
                    )?;
                    match self.bridge.retirement_observe_destroyed(
                        viewport,
                        binding,
                        record.window_id(),
                        record.ordinal(),
                    ) {
                        DestroyedObservation::Recorded => {
                            if !self.session.recognizes_native_binding(binding) {
                                self.bridge
                                    .retirement_retire_unrecognized_destroyed(binding);
                            }
                            self.bridge.invalidate_viewport_roster();
                            if accepted_close
                                && !self.bridge.with_close_control(|control| {
                                    control.settle_accepted_destroyed(record, binding)
                                })
                            {
                                return Err(
                                    NativeHostProtocolError::NativeCloseCorrelationChanged.into()
                                );
                            }
                            self.lifecycle_progress.record_destroyed_observation();
                            self.retire_deferred_sidecars(viewport, binding);
                        }
                        DestroyedObservation::Duplicate => {
                            if accepted_close {
                                return Err(
                                    NativeHostProtocolError::NativeCloseCorrelationChanged.into()
                                );
                            }
                        }
                        DestroyedObservation::Rejected => {
                            if accepted_close {
                                return Err(
                                    NativeHostProtocolError::NativeCloseCorrelationChanged.into()
                                );
                            }
                        }
                    }
                } else if accepted_close {
                    return Err(NativeHostProtocolError::NativeCloseCorrelationChanged.into());
                }
            }
            _ => {}
        }
        Ok(())
    }

    /// Reduces one same-callback root viewport roster into a complete platform fact.
    ///
    /// Eframe may expose live windows that are not dockspace surfaces, so
    /// unmapped records are ignored. Core still performs the exact-set check
    /// against every current dockspace binding. Any incomplete, stale, or
    /// malformed candidate revokes inventory authority as `Unknown`; it never
    /// infers destruction from absence.
    pub(crate) fn reduce_next_viewport_roster(&mut self) -> Result<bool, NativeRuntimeError> {
        let Some(roster) = self.bridge.front_viewport_roster() else {
            return Ok(false);
        };
        self.record_viewport_roster(roster.roster(), roster.clone())?;
        if !self.bridge.acknowledge_viewport_roster(&roster) {
            return Err(NativeHostProtocolError::ViewportRosterAcknowledgementMismatch.into());
        }
        Ok(true)
    }

    fn record_viewport_roster(
        &mut self,
        roster: &NativeViewportRosterRecord,
        envelope: NativeViewportRosterEnvelope,
    ) -> Result<(), NativeRuntimeError> {
        let compiled = self.bridge.with_retirement_destroyed_observations(
            |destroyed_count, destroyed, created_ack, presentation_ack| {
                self.preflight_snapshot_storage(roster, destroyed_count)?;
                let mut observations = self.compile_viewport_roster(
                    roster,
                    destroyed_count,
                    created_ack,
                    presentation_ack,
                )?;
                observations.extend(
                    destroyed
                        .map(|(binding, facts)| CompiledWindowObservation::new(binding, facts)),
                );
                Ok::<_, NativeRuntimeError>((observations, destroyed_count != 0))
            },
        );
        let compiled = match compiled {
            Err(error) if error.is_input_capacity() => return Err(error),
            result => result,
        };
        self.session
            .configure_managed_native_capabilities(capabilities_for_backend(roster.backend()))?;
        if let (Ok((observations, has_destroyed)), Ok(frozen_work_areas)) =
            (compiled, roster.work_areas())
        {
            let (core_work_areas, prepared_work_areas) =
                self.work_areas.prepare(frozen_work_areas)?;
            let mut acknowledgements = Vec::new();
            let mut focus_reportable = Vec::new();
            acknowledgements
                .try_reserve_exact(observations.len())
                .map_err(|_| NativeHostProtocolError::NativeReportStorageUnavailable)?;
            focus_reportable
                .try_reserve_exact(observations.len())
                .map_err(|_| NativeHostProtocolError::NativeReportStorageUnavailable)?;
            if self.snapshot_storage.is_some_and(|limit| {
                acknowledgements.capacity() > 2 * limit.window_slots
                    || focus_reportable.capacity() > 2 * limit.window_slots
            }) {
                return Err(NativeHostProtocolError::NativeReportStorageUnavailable.into());
            }
            let mut input_acknowledgement = None;
            for observation in &observations {
                let binding = observation.binding();
                if observation.create_acknowledged() || observation.presentation_acknowledged() {
                    acknowledgements.push(NativeSnapshotAcknowledgements {
                        binding,
                        create: observation.create_acknowledged(),
                        presentation: observation.presentation_acknowledged(),
                    });
                }
                if observation.input_acknowledged() {
                    let previous = input_acknowledgement.replace(binding);
                    debug_assert!(
                        previous.is_none(),
                        "one globally ordered input lane acknowledges at most one binding"
                    );
                }
                if observation.focus_reportable() {
                    focus_reportable.push(binding);
                }
            }
            match self.session.report_managed_native_snapshot(
                observations
                    .iter()
                    .map(|observation| (observation.binding(), observation.facts())),
                core_work_areas,
            ) {
                Ok(()) => {
                    debug_assert!(self.queued_snapshot.is_none());
                    self.queued_snapshot = Some(QueuedNativeSnapshot {
                        settlement: None,
                        roster: envelope,
                        acknowledgements,
                        input_acknowledgement,
                        focus_reportable,
                        work_areas: prepared_work_areas,
                    });
                    if has_destroyed {
                        self.bridge.retirement_mark_snapshot_queued();
                    }
                    return Ok(());
                }
                Err(error)
                    if matches!(
                        error.native_kind(),
                        Some(
                            NativeHostErrorKind::StaleBinding
                                | NativeHostErrorKind::InvalidFacts
                                | NativeHostErrorKind::OperationConflict
                        )
                    ) => {}
                Err(error) => return Err(error.into()),
            }
        }
        let (_, prepared_work_areas) = self.work_areas.prepare(&FrozenWorkAreaRoster::Unknown)?;
        self.session.report_native_inventory_unknown()?;
        debug_assert!(self.queued_snapshot.is_none());
        self.queued_snapshot = Some(QueuedNativeSnapshot {
            settlement: None,
            roster: envelope,
            acknowledgements: Vec::new(),
            input_acknowledgement: None,
            focus_reportable: Vec::new(),
            work_areas: prepared_work_areas,
        });
        Ok(())
    }

    fn compile_viewport_roster(
        &self,
        roster: &NativeViewportRosterRecord,
        destroyed_count: usize,
        created_ack: &dyn Fn(
            NativeSurfaceBinding,
        ) -> Option<Option<NativeCreateEffectAcknowledgement>>,
        presentation_ack: &dyn Fn(
            NativeSurfaceBinding,
        ) -> Option<NativePresentationEffectAcknowledgement>,
    ) -> Result<Vec<CompiledWindowObservation>, NativeRuntimeError> {
        let capacity = roster
            .observation_count()
            .checked_add(destroyed_count)
            .ok_or(NativeHostProtocolError::NativeReportStorageUnavailable)?;
        let mut observations = Vec::new();
        observations
            .try_reserve_exact(capacity)
            .map_err(|_| NativeHostProtocolError::NativeReportStorageUnavailable)?;
        if self
            .snapshot_storage
            .is_some_and(|limit| observations.capacity() > 2 * limit.window_slots)
        {
            return Err(NativeHostProtocolError::NativeReportStorageUnavailable.into());
        }
        #[cfg(test)]
        if let Some(compiled) = roster.compiled_override() {
            observations.extend(
                compiled?
                    .iter()
                    .copied()
                    .map(|observation| self.with_native_input_observation(observation)),
            );
            return Ok(observations);
        }
        for observation in roster.observations().iter().copied() {
            let compiled = compile_window_observation(
                observation.binding(),
                observation.snapshot(),
                presentation_ack(observation.binding()),
            )?;
            let compiled = created_ack(observation.binding()).map_or(compiled, |acknowledgement| {
                compiled.with_created(acknowledgement)
            });
            observations.push(self.with_native_input_observation(compiled));
        }
        Ok(observations)
    }

    fn with_native_input_observation(
        &self,
        observation: CompiledWindowObservation,
    ) -> CompiledWindowObservation {
        self.input_control
            .snapshot_fact(observation.binding())
            .map_or(observation, |(state, acknowledgement)| {
                observation.with_input(state, acknowledgement)
            })
    }

    /// Registers one application root window with the core-owned native roster.
    ///
    /// # Errors
    ///
    /// Returns an error when the surface is absent or already has an active
    /// native registration.
    pub fn register_native_root(
        &mut self,
        surface: SurfaceId,
        window: HostWindowToken,
    ) -> Result<(), NativeRuntimeError> {
        self.session.register_native_root(surface, window)?;
        Ok(())
    }

    /// Submits one complete managed-desktop platform snapshot.
    ///
    /// # Errors
    ///
    /// Returns an error when the roster is incomplete, stale, or internally
    /// contradictory.
    pub fn report_snapshot(
        &mut self,
        windows: impl IntoIterator<Item = (NativeSurfaceBinding, NativeWindowFacts)>,
        work_areas: NativeWorkAreaRoster,
        capabilities: dockspace::runtime::NativeHostCapabilities,
    ) -> Result<(), NativeRuntimeError> {
        self.session
            .configure_managed_native_capabilities(capabilities)?;
        self.session
            .report_managed_native_snapshot(windows, work_areas)?;
        Ok(())
    }

    /// Explicitly revokes authoritative native inventory for the next frame.
    ///
    /// # Errors
    ///
    /// Returns an error when no managed native provider is active.
    pub fn report_inventory_unknown(&mut self) -> Result<(), NativeRuntimeError> {
        self.session.report_native_inventory_unknown()?;
        Ok(())
    }

    /// Returns a current exact work-area binding for outside-all routing.
    #[must_use]
    pub fn native_work_area(
        &self,
        token: dockspace::runtime::HostWorkAreaToken,
    ) -> Option<NativeWorkAreaBinding> {
        self.session.native_work_area(token)
    }

    #[cfg(test)]
    fn work_area_for_point(
        &self,
        point: dockspace::geometry::PhysicalPoint,
    ) -> Option<NativeWorkAreaBinding> {
        self.work_areas.binding_at(point)
    }

    /// Records one complete event-time pointer fact.
    ///
    /// # Errors
    ///
    /// Returns an error when the edge names stale authority or cannot follow
    /// the active provider sequence.
    pub fn record_pointer(&mut self, input: NativePointerInput) -> Result<(), NativeRuntimeError> {
        self.session.record_native_pointer(input)?;
        Ok(())
    }

    /// Publishes one exact native close edge.
    ///
    /// # Errors
    ///
    /// Returns an error when the close edge is stale or belongs to another provider.
    pub fn publish_close(
        &mut self,
        binding: NativeSurfaceBinding,
        state: NativeCloseState,
        acknowledgement: Option<NativeCloseEffectAcknowledgement>,
    ) -> Result<(), NativeRuntimeError> {
        self.session
            .publish_native_close(binding, state, acknowledgement)?;
        Ok(())
    }

    /// Records one negative or indeterminate result for an emitted native effect.
    ///
    /// Successful dispatch acknowledgements are attached to the subsequent
    /// exact property observation instead of being reported through this lane.
    ///
    /// # Errors
    ///
    /// Returns an error only when a retained rejection is not retryable.
    /// Stale-binding and operation-conflict results remain pending for an exact
    /// retry or binding retirement boundary.
    pub fn report_effect_result(
        &mut self,
        result: NativeEffectResult,
    ) -> Result<(), NativeRuntimeError> {
        let binding = result.binding();
        if let Err(error) = self.session.report_native_effect_result(result) {
            return self.retain_or_return_effect_result(binding, error);
        }
        Ok(())
    }

    /// Records permanent host quiescence for one retired binding.
    ///
    /// # Errors
    ///
    /// Returns an error while any callback, route, or sidecar may still name
    /// the binding, or when the binding remains live.
    pub fn report_binding_quiescence(
        &mut self,
        binding: NativeSurfaceBinding,
    ) -> Result<(), NativeRuntimeError> {
        if self.binding_has_retained_host_references(binding) {
            return Err(
                NativeHostProtocolError::NativeBindingNotQuiescent(binding.surface()).into(),
            );
        }
        self.session.report_native_binding_quiescence(binding)?;
        Ok(())
    }

    /// Begins one native host frame after applying queued renderer settlements.
    ///
    /// # Errors
    ///
    /// Returns an error when a renderer result is stale or the core rejects the
    /// frozen native input prefix.
    #[cfg(test)]
    pub fn begin_host_frame(
        &mut self,
        resolve: impl FnMut(NativeReceiverQuery) -> NativeReceiverAnswer,
    ) -> Result<NativeHostFrame<'_>, NativeRuntimeError> {
        self.bridge.admission_dispatch();
        self.prepare_output_prefix(&mut NativeInputBudget::new(
            &dockspace::runtime::NativeMonotonicClock::new(|| std::time::Duration::ZERO),
        ))?;
        if self.bridge.has_pending_input() && !self.bridge.callback_boundary_pending() {
            return Err(NativeHostProtocolError::CallbackRecordPending.into());
        }
        let dispatch = self.bridge.admission_dispatch();
        let mut frame = NativeHostFrame::new(
            self.session.begin_native_host_frame(resolve)?,
            self.bridge.clone(),
            &mut self.receivers,
            None,
        );
        if let NativeDispatch::Cancel { boundary } = dispatch {
            frame.cancel_for_overload(boundary, &mut self.pointer_translator)?;
        }
        Ok(frame)
    }

    /// Begins one native frame using only final-pass egui receiver authority.
    pub(crate) fn poll_resolved_host_frame(
        &mut self,
        context: &eframe::egui::Context,
        budget: &mut NativeInputBudget,
    ) -> Result<Option<NativeHostFrame<'_>>, NativeRuntimeError> {
        self.bridge.admission_dispatch();
        self.prepare_output_prefix(budget)?;
        if self.bridge.has_pending_input() && !self.bridge.callback_boundary_pending() {
            if budget.is_exhausted() {
                return Ok(None);
            }
            return Err(NativeHostProtocolError::CallbackRecordPending.into());
        }
        let bridge = self.bridge.clone();
        let dispatch = bridge.admission_dispatch();
        let receivers = &self.receivers;
        let Some(frame) = self
            .session
            .poll_native_host_frame(budget, |query| receivers.resolve(query))?
        else {
            bridge.hold_native_boundary();
            return Ok(None);
        };
        let mut frame =
            NativeHostFrame::new(frame, bridge, &mut self.receivers, Some(context.clone()));
        if let NativeDispatch::Cancel { boundary } = dispatch {
            frame.cancel_for_overload(boundary, &mut self.pointer_translator)?;
        }
        Ok(Some(frame))
    }

    pub(crate) fn poll_host_frame(
        &mut self,
        budget: &mut NativeInputBudget,
        resolve: impl FnMut(NativeReceiverQuery) -> NativeReceiverAnswer,
    ) -> Result<Option<NativeHostFrame<'_>>, NativeRuntimeError> {
        self.bridge.admission_dispatch();
        self.prepare_output_prefix(budget)?;
        if self.bridge.has_pending_input() && !self.bridge.callback_boundary_pending() {
            if budget.is_exhausted() {
                return Ok(None);
            }
            return Err(NativeHostProtocolError::CallbackRecordPending.into());
        }
        let dispatch = self.bridge.admission_dispatch();
        let Some(frame) = self.session.poll_native_host_frame(budget, resolve)? else {
            self.bridge.hold_native_boundary();
            return Ok(None);
        };
        let mut frame = NativeHostFrame::new(frame, self.bridge.clone(), &mut self.receivers, None);
        if let NativeDispatch::Cancel { boundary } = dispatch {
            frame.cancel_for_overload(boundary, &mut self.pointer_translator)?;
        }
        Ok(Some(frame))
    }

    /// Binds one affine painted output and its final-pass receiver roster to the
    /// token active while that viewport UI ran.
    ///
    /// # Errors
    ///
    /// Returns an error carrying the output when the token was not announced,
    /// the exact native binding differs, or the token is already pending.
    pub fn bind_painted_surface(
        &mut self,
        context: &eframe::egui::Context,
        token: NativeOutputToken,
        output: PaintedSurfaceOutput,
        paint: &crate::host_frame::NativeSurfacePaint,
    ) -> Result<(), NativeOutputBindingError> {
        let Some(reservation) = self.bridge.output_reservation(token) else {
            return Err(NativeOutputBindingError::new(
                NativeOutputBindingErrorKind::TokenNotReserved,
                token,
                output,
            ));
        };
        if self.bridge.has_pending_output(token) {
            return Err(NativeOutputBindingError::new(
                NativeOutputBindingErrorKind::OutputAlreadyBound,
                token,
                output,
            ));
        }
        let current_binding = self.bridge.with_viewports(|viewports| {
            viewports.binding_for_output(
                token.viewport_id(),
                token.window_id(),
                token.create_attempt(),
            )
        });
        let binding = reservation.binding().or(current_binding);
        let Some(binding) = binding else {
            return Err(NativeOutputBindingError::new(
                NativeOutputBindingErrorKind::RouteUnavailable,
                token,
                output,
            ));
        };
        if reservation.binding().is_some() && current_binding != Some(binding) {
            return Err(NativeOutputBindingError::new(
                NativeOutputBindingErrorKind::BindingMismatch,
                token,
                output,
            ));
        }
        if !output.matches_native_binding(binding) {
            return Err(NativeOutputBindingError::new(
                NativeOutputBindingErrorKind::BindingMismatch,
                token,
                output,
            ));
        }
        let requires_lifecycle_wake = self.session.presented_surface(output.surface()).is_none();
        if self
            .receivers
            .stage(context, token, binding, &output, paint)
            .is_err()
        {
            return Err(NativeOutputBindingError::new(
                NativeOutputBindingErrorKind::ReceiverMismatch,
                token,
                output,
            ));
        }
        if !self.bridge.attach_output_binding(token, binding) {
            self.receivers.abandon(token);
            return Err(NativeOutputBindingError::new(
                NativeOutputBindingErrorKind::BindingMismatch,
                token,
                output,
            ));
        }
        if requires_lifecycle_wake && !self.bridge.require_output_wake_after_current(token) {
            self.receivers.abandon(token);
            return Err(NativeOutputBindingError::new(
                NativeOutputBindingErrorKind::BindingMismatch,
                token,
                output,
            ));
        }
        if let Err(output) = self.bridge.bind_surface_output(token, output) {
            self.receivers.abandon(token);
            return Err(NativeOutputBindingError::new(
                NativeOutputBindingErrorKind::OutputAlreadyBound,
                token,
                output,
            ));
        }
        Ok(())
    }

    /// Abandons one announced eframe output which produced no dockspace paint.
    ///
    /// Any attached affine output is dropped into the core's explicit
    /// not-presented queue. A later renderer callback for the token is ignored.
    /// This method must be used for mapped viewport callbacks which finish
    /// without a matching [`PaintedSurfaceOutput`].
    ///
    /// Returns [`NativeHostWake::Wait`] when the token is unknown or its
    /// presentation result has already entered a core host-frame boundary.
    /// A successful abandon requests another root update so the core can
    /// consume the affine output's explicit dropped terminal.
    #[must_use]
    pub fn abandon_output_token(&mut self, token: NativeOutputToken) -> NativeHostWake {
        if !self.bridge.abandon_output(token) {
            return NativeHostWake::Wait;
        }
        self.receivers.abandon(token);
        self.bridge.take_pending_output(token);
        NativeHostWake::RepaintRoot
    }

    pub(crate) fn settle_retained_output_token(
        &mut self,
        token: NativeOutputToken,
        retained: bool,
    ) -> Result<NativeHostWake, NativeOutputBindingErrorKind> {
        let reservation = self
            .bridge
            .output_reservation(token)
            .ok_or(NativeOutputBindingErrorKind::TokenNotReserved)?;
        if self.bridge.has_pending_output(token) {
            return Err(NativeOutputBindingErrorKind::OutputAlreadyBound);
        }
        let wake_on_terminal = !retained;
        let no_frame = reservation
            .binding()
            .filter(|binding| {
                !retained
                    && should_record_no_frame_invalidation(
                        self.bridge.accepts_no_frame_invalidation(),
                        self.session.is_current_native_binding(*binding),
                        self.session.presented_surface(binding.surface()).is_some(),
                    )
            })
            .map(|binding| PendingNoFrameInvalidation::awaiting_terminal(token, binding));
        if !matches!(
            self.bridge
                .settle_output_abandonment(token, wake_on_terminal, no_frame),
            OutputAbandonment::NewlyAbandoned | OutputAbandonment::AlreadyAbandonedAwaitingTerminal
        ) {
            return Err(NativeOutputBindingErrorKind::TokenNotReserved);
        }
        if !retained && let Some(binding) = reservation.binding() {
            self.receivers.retire_binding(binding);
        }
        // A retained output needs no follow-up. A NoFrame fallback is allowed
        // to wake only from its physical terminal callback, after the window
        // has actually replaced the old framebuffer.
        Ok(NativeHostWake::Wait)
    }

    pub(crate) fn prepare_no_frame_invalidations(
        &mut self,
    ) -> Result<PreparedNoFrameInvalidations, NativeRuntimeError> {
        self.bridge.prepare_no_frame_invalidations(&self.session)
    }

    pub(crate) fn has_pending_no_frame_invalidation(&self, surface: SurfaceId) -> bool {
        self.bridge.has_pending_no_frame_invalidation(surface)
    }

    pub(crate) fn finish_no_frame_invalidations(
        &mut self,
        committed: &[PendingNoFrameInvalidation],
    ) {
        self.bridge.finish_no_frame_invalidations(committed);
    }

    pub(crate) fn quarantine_after_fatal_into(
        &mut self,
        mut notify: impl FnMut(NativeOutputToken),
    ) {
        self.bridge.enter_quarantine();
        if self.requires_terminal_snapshot() {
            self.bridge.require_terminal_roster();
        }
        while let Some((token, paint)) = self.bridge.claim_quarantine_output() {
            self.receivers.abandon(token);
            drop(paint);
            notify(token);
        }
        self.cancel_undispatched_control_commands();
        self.cancel_unadmitted_viewport_effects();
    }

    #[cfg(test)]
    pub(crate) fn quarantine_after_fatal(&mut self) -> Vec<NativeOutputToken> {
        let mut abandoned = Vec::new();
        self.quarantine_after_fatal_into(|token| abandoned.push(token));
        abandoned
    }

    fn requires_terminal_snapshot(&self) -> bool {
        self.bridge.create_ack_count() != 0
            || self.bridge.presentation_ack_count() != 0
            || self.input_control.requires_snapshot()
            || self.bridge.retirement_requires_snapshot()
            || self.bridge.viewport_roster_dirty()
    }

    fn cancel_undispatched_control_commands(&mut self) {
        self.input_control.begin_quarantine();
        if let Some(binding) = self.focus_control.pending_binding() {
            self.terminalize_queued_focus(binding);
        }
    }

    fn cancel_unadmitted_viewport_effects(&mut self) {
        loop {
            let plan = self.bridge.with_viewports(|viewports| {
                self.effects.awaiting_viewport_callbacks().find(|plan| {
                    !viewports.has_create_lifetime_owner(plan.viewport(), plan.binding())
                })
            });
            let Some(plan) = plan else {
                break;
            };
            let Some(request) = self.effects.remove_unstarted(plan) else {
                unreachable!("the selected unadmitted request remains owned");
            };
            self.bridge
                .retire_deferred_binding_paints(plan.viewport(), plan.binding());
            self.deferred_viewports.remove(plan.binding());
            self.receivers.retire_binding(plan.binding());
            let removed = self.bridge.with_viewports_mut(|viewports| {
                viewports.remove_viewport(plan.viewport(), plan.binding())
            });
            debug_assert!(
                removed.is_ok(),
                "unadmitted viewport effect retains one exact route"
            );
            drop(request);
        }
    }

    fn prepare_output_prefix(
        &mut self,
        budget: &mut NativeInputBudget,
    ) -> Result<(), NativeRuntimeError> {
        if !self.bridge.can_reduce_output_prefix() {
            return Err(NativeHostProtocolError::OutputOrderViolation.into());
        }
        let mut prefix = self
            .bridge
            .output_prefix(budget.remaining_records().min(256));
        loop {
            let Some(entry) = self.bridge.next_output_prefix(&mut prefix)? else {
                break;
            };
            let result = entry.result;
            if budget.is_exhausted() {
                break;
            }
            if entry.submitted {
                continue;
            }
            let Some(output) = self.bridge.take_pending_output(result.token()) else {
                return Err(NativeHostProtocolError::OutputAwaitingAttachment.into());
            };
            let binding = entry.binding;
            let presentation = match result.status() {
                NativeOutputStatus::Presented => SurfacePresentationResult::Presented,
                NativeOutputStatus::NotPresented => SurfacePresentationResult::Dropped,
            };
            match output {
                PendingNativeOutput::Surface(output) => {
                    if let Err(error) = self
                        .session
                        .report_surface_presentation(output, presentation)
                    {
                        let abandoned = self.bridge.abandon_output(result.token());
                        debug_assert!(abandoned, "failed presentation was not submitted");
                        self.receivers.abandon(result.token());
                        return Err(error.into());
                    }
                    match (result.status(), binding) {
                        (NativeOutputStatus::Presented, Some(binding)) => {
                            self.receivers.renderer_presented(result.token(), binding);
                        }
                        (NativeOutputStatus::Presented, None)
                        | (NativeOutputStatus::NotPresented, _) => {
                            self.receivers.dropped(result.token());
                        }
                    }
                }
                PendingNativeOutput::Staging(output) => {
                    if let Err(error) = self
                        .session
                        .report_native_staging_presentation(output, presentation)
                    {
                        let abandoned = self.bridge.abandon_output(result.token());
                        debug_assert!(abandoned, "failed staging presentation was not submitted");
                        return Err(error.into());
                    }
                }
            }
            if !self.bridge.mark_output_submitted(result.token()) {
                return Err(NativeHostProtocolError::OutputOrderViolation.into());
            }
        }
        Ok(())
    }
}

fn exact_native_rect(rect: PhysicalRect) -> Option<NativePhysicalRect> {
    Some(NativePhysicalRect::new(
        exact_i32(rect.x())?,
        exact_i32(rect.y())?,
        exact_non_zero_u32(rect.width())?,
        exact_non_zero_u32(rect.height())?,
    ))
}

fn exact_i32(value: f64) -> Option<i32> {
    (value.fract() == 0.0 && value >= f64::from(i32::MIN) && value <= f64::from(i32::MAX))
        .then_some(value as i32)
}

fn exact_non_zero_u32(value: f64) -> Option<u32> {
    (value.fract() == 0.0 && value >= 1.0 && value <= f64::from(u32::MAX)).then_some(value as u32)
}

pub(crate) const fn output_terminal_requires_adapter_abandonment(
    status: NativeOutputStatus,
    submitted: bool,
    attached: bool,
) -> bool {
    !submitted && !attached && matches!(status, NativeOutputStatus::NotPresented)
}

impl Drop for NativeCoordinator {
    fn drop(&mut self) {
        self.bridge.freeze();
    }
}

#[cfg(test)]
mod tests;
