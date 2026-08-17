//! Thin coordinator between eframe callbacks and the renderer-neutral session.

use std::collections::{BTreeMap, VecDeque};
use std::sync::{Arc, Mutex, PoisonError};

use dockspace::geometry::PhysicalRect;
use dockspace::model::SurfaceId;
use dockspace::runtime::{
    DockspaceSession, HostInputOutcome, HostWindowToken, NativeCloseDisposition,
    NativeCloseEffectAcknowledgement, NativeCloseState, NativeDispatchFailure,
    NativeEffectAcknowledgement, NativeEffectOperation, NativeEffectRequest, NativeEffectResult,
    NativeEffectSubmissionError, NativeGlobalFocus, NativeHostErrorKind, NativeIndeterminateReason,
    NativePointerInput, NativePointerRoster, NativePresentationEffectAcknowledgement,
    NativeReceiverAnswer, NativeReceiverQuery, NativeSurfaceBinding, NativeUnsupportedReason,
    NativeWindowFacts, NativeWorkAreaBinding, NativeWorkAreaRoster, PaintedNativeStagingOutput,
    PaintedSurfaceOutput, SurfacePresentationResult, SurfaceUnavailableReason,
};
use eframe::{
    NativeHostHandler, NativeHostWake, NativeOutputStatus, NativeOutputToken, NativePhysicalRect,
    NativeViewportCreateFailureKind, NativeViewportFocusStatus,
    NativeViewportPointerPassthroughCommandToken, NativeViewportPointerPassthroughStatus,
    NativeViewportVisibilityStatus,
    egui::{ViewportCommand, ViewportId},
};
use winit::window::WindowId;

use crate::NativeRuntimeError;
use crate::close_control::NativeCloseControl;
use crate::deferred_viewport::{DeferredViewportDriver, DeferredViewportSpec, viewport_id_for};
use crate::effect_coordinator::{
    NativeEffectCoordinator, NativeViewportEffectKind, NativeViewportEffectPlan,
};
use crate::error::{
    NativeHostProtocolError, NativeOutputBindingError, NativeOutputBindingErrorKind,
    NativeViewportBindingError,
};
use crate::event::NativeWindowEventRecord;
use crate::focus_control::{
    NativeFocusControl, NativeFocusResultDisposition, NativeFocusTermination,
};
use crate::host_frame::NativeHostFrame;
use crate::input_control::{
    NativeInputControl, NativeInputResultDisposition, NativePointerPassthroughCommand,
};
use crate::lifecycle_progress::NativeLifecycleProgress;
use crate::mailbox::{
    DeferredViewportPaint, NativeHostBridge, NativeViewportRosterEnvelope,
    NativeViewportRosterRecord, PreparedRouteRetirements,
};
#[cfg(test)]
use crate::mailbox::{HostRecord, OutputReservation};
use crate::pointer_event::{NativePointerTranslation, NativePointerTranslator};
use crate::receiver::NativeReceiverStore;
use crate::retirement::{
    CleanupResultRetentionError, CommittedRetirement, DestroyedObservation, NativeRetirementState,
};
use crate::viewport_callback::{NativeViewportCreateFailureRecord, NativeViewportVisibilityRecord};
use crate::viewport_map::NativeViewportMap;
use crate::window_snapshot::{CompiledWindowObservation, compile_window_observation};
use crate::work_area::{FrozenWorkAreaRoster, NativeWorkAreaState, PreparedWorkAreaRoster};

mod close_driver;
mod effect_dispatch;
mod shutdown;

pub(crate) use shutdown::{NativeShutdownAdvance, NativeShutdownRegistration};

/// Sole native coordinator for one renderer-neutral docking session.
///
/// Native callbacks only append immutable records. Application code reduces
/// those records at the next root update, acknowledges each accepted window
/// event, renders one affine host frame, and explicitly binds every painted
/// output to the eframe output token that produced it.
pub(crate) struct NativeCoordinator {
    session: DockspaceSession,
    bridge: Arc<NativeHostBridge>,
    viewports: Arc<Mutex<NativeViewportMap>>,
    pending_outputs: BTreeMap<NativeOutputToken, PendingNativeOutput>,
    pending_presentation_acknowledgements:
        BTreeMap<NativeSurfaceBinding, NativePresentationEffectAcknowledgement>,
    queued_snapshot: Option<QueuedNativeSnapshot>,
    deferred_viewports: DeferredViewportDriver,
    receivers: NativeReceiverStore,
    effects: NativeEffectCoordinator,
    pending_effect_results: VecDeque<PendingNativeEffectResult>,
    callback_errors: VecDeque<NativeRuntimeError>,
    retirements: NativeRetirementState,
    pointer_translator: NativePointerTranslator,
    work_areas: NativeWorkAreaState,
    close_control: NativeCloseControl,
    focus_control: NativeFocusControl,
    input_control: NativeInputControl,
    lifecycle_progress: NativeLifecycleProgress,
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

#[derive(Debug)]
struct QueuedNativeSnapshot {
    roster: NativeViewportRosterEnvelope,
    presentation_acknowledgements: Vec<NativeSurfaceBinding>,
    input_acknowledgement: Option<NativeSurfaceBinding>,
    focus_reportable: Vec<NativeSurfaceBinding>,
    work_areas: PreparedWorkAreaRoster,
}

pub(crate) struct PreparedNativeRetirements {
    committed: Vec<CommittedRetirement>,
    routes: PreparedRouteRetirements,
}

enum PendingNativeOutput {
    Surface(PaintedSurfaceOutput),
    Staging(PaintedNativeStagingOutput),
}

impl std::fmt::Debug for PendingNativeOutput {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Surface(output) => formatter
                .debug_tuple("Surface")
                .field(&output.surface())
                .finish(),
            Self::Staging(output) => formatter
                .debug_tuple("Staging")
                .field(&output.surface())
                .field(&output.phase())
                .finish(),
        }
    }
}

impl PendingNativeOutput {
    fn requires_lifecycle_settlement(&self, session: &DockspaceSession) -> bool {
        match self {
            Self::Staging(_) => true,
            Self::Surface(output) => session.presented_surface(output.surface()).is_none(),
        }
    }
}

impl std::fmt::Debug for NativeCoordinator {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("NativeCoordinator")
            .field("session", &self.session)
            .field(
                "viewports",
                &self
                    .viewports
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner),
            )
            .field("pending_outputs", &self.pending_outputs.len())
            .field(
                "pending_presentation_acknowledgements",
                &self.pending_presentation_acknowledgements.len(),
            )
            .field(
                "queued_snapshot_presentation_acknowledgements",
                &self
                    .queued_snapshot
                    .as_ref()
                    .map_or(0, |queued| queued.presentation_acknowledgements.len()),
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
            .field("callback_errors", &self.callback_errors.len())
            .field("retirements", &self.retirements)
            .field("pointer_translator", &self.pointer_translator)
            .field("close_control", &self.close_control)
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
        mut session: DockspaceSession,
        initial_pointer_roster: NativePointerRoster,
    ) -> Result<Self, NativeRuntimeError> {
        session.enable_managed_native_host(initial_pointer_roster)?;
        session.report_native_inventory_unknown()?;
        let viewports = Arc::new(Mutex::new(NativeViewportMap::default()));
        Ok(Self {
            session,
            bridge: Arc::new(NativeHostBridge::new(viewports.clone())),
            viewports,
            pending_outputs: BTreeMap::new(),
            pending_presentation_acknowledgements: BTreeMap::new(),
            queued_snapshot: None,
            deferred_viewports: DeferredViewportDriver::default(),
            receivers: NativeReceiverStore::default(),
            effects: NativeEffectCoordinator::default(),
            pending_effect_results: VecDeque::new(),
            callback_errors: VecDeque::new(),
            retirements: NativeRetirementState::default(),
            pointer_translator: NativePointerTranslator::default(),
            work_areas: NativeWorkAreaState::default(),
            close_control: NativeCloseControl::default(),
            focus_control: NativeFocusControl::default(),
            input_control: NativeInputControl::default(),
            lifecycle_progress: NativeLifecycleProgress::default(),
        })
    }

    /// Returns the eframe host handler to install in [`eframe::NativeOptions`].
    #[must_use]
    pub fn native_host_handler(&self) -> Arc<dyn NativeHostHandler> {
        self.bridge.clone()
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

    pub(crate) fn is_quiescent(&self) -> bool {
        !self.bridge.has_pending_coordinator_work()
            && self
                .pending_outputs
                .values()
                .all(|output| !output.requires_lifecycle_settlement(&self.session))
            && self.pending_presentation_acknowledgements.is_empty()
            && self.queued_snapshot.is_none()
            && !self.deferred_viewports.has_transitional_viewport()
            && !self.effects.has_pending_work()
            && self.pending_effect_results.is_empty()
            && self.callback_errors.is_empty()
            && !self.retirements.has_pending_work()
            && !self.pointer_translator.has_pending_provider_tail()
            && !self.close_control.has_pending_work()
            && !self.focus_control.has_pending_work()
            && !self.input_control.has_pending_work()
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
        self.viewports
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .bind(viewport, window, binding)
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
        self.viewports
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .reserve(viewport, binding)
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
        self.viewports
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .attach(viewport, expected, window)
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
        self.viewports
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .replace(viewport, expected, successor_window, successor)?;
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
        self.viewports
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .reserve_replacement(viewport, expected, successor)?;
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
        let removed = self
            .viewports
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove_viewport(viewport, expected)?;
        self.receivers.retire_binding(removed);
        Ok(removed)
    }

    /// Returns the current exact binding for one eframe viewport.
    #[must_use]
    pub fn viewport_binding(&self, viewport: ViewportId) -> Option<NativeSurfaceBinding> {
        self.viewports
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .binding(viewport)
    }

    /// Returns the eframe viewport currently presenting one logical surface.
    #[must_use]
    pub fn surface_viewport(&self, surface: SurfaceId) -> Option<ViewportId> {
        self.viewports
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .viewport(surface)
    }

    /// Returns every child viewport which the next root pass must re-declare.
    pub(crate) fn deferred_viewport_specs(&self) -> Vec<DeferredViewportSpec> {
        self.deferred_viewports.retained_specs()
    }

    pub(crate) fn take_viewport_commands(&mut self) -> Vec<(ViewportId, ViewportCommand)> {
        let commands = self.effects.take_commands();
        for (viewport, command) in &commands {
            if *command == ViewportCommand::Focus {
                let marked = self.focus_control.mark_dispatched(*viewport);
                debug_assert!(marked, "a queued focus command retains its affine request");
            }
        }
        commands
    }

    pub(crate) fn pointer_passthrough_command(&self) -> Option<NativePointerPassthroughCommand> {
        self.input_control.command_to_dispatch()
    }

    pub(crate) fn mark_pointer_passthrough_dispatched(
        &mut self,
        command: NativePointerPassthroughCommand,
        token: NativeViewportPointerPassthroughCommandToken,
    ) -> bool {
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
        let DeferredViewportPaint::Semantic(binding) = disposition else {
            return disposition;
        };
        let current = self
            .viewports
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .binding_for_output(
                token.viewport_id(),
                token.window_id(),
                token.create_attempt(),
            );
        if current == Some(binding) && self.session.is_current_native_binding(binding) {
            disposition
        } else {
            self.bridge.abandon_output(token);
            DeferredViewportPaint::Waiting
        }
    }

    /// Returns the viewport which currently owns one logical surface.
    #[must_use]
    pub(crate) fn repaint_viewport(&self, surface: SurfaceId) -> Option<ViewportId> {
        self.surface_viewport(surface)
    }

    pub(crate) fn prepare_committed_retirements(
        &self,
    ) -> Result<Option<PreparedNativeRetirements>, NativeRuntimeError> {
        let committed = self.retirements.committed_routes();
        if committed.is_empty() {
            return Ok(None);
        }
        let routes = self
            .bridge
            .prepare_route_retirements(&committed)
            .map_err(|binding| {
                NativeHostProtocolError::RetiredViewportRouteChanged(binding.surface())
            })?;
        Ok(Some(PreparedNativeRetirements { committed, routes }))
    }

    #[must_use = "retired output tokens must be abandoned by every output-owned sidecar"]
    pub(crate) fn commit_retirements(
        &mut self,
        prepared: PreparedNativeRetirements,
    ) -> Vec<NativeOutputToken> {
        let PreparedNativeRetirements { committed, routes } = prepared;
        let abandoned = routes.commit();
        for retirement in &committed {
            let binding = retirement.binding();
            self.pending_presentation_acknowledgements.remove(&binding);
            self.effects.remove_show(binding);
            self.terminalize_focus(binding);
            self.effects.remove_commands(binding);
            self.close_control.retire_binding(binding);
            self.retain_retired_input_results(binding);
            self.receivers.retire_binding(binding);
        }
        for &token in &abandoned {
            self.pending_outputs.remove(&token);
            self.receivers.abandon(token);
        }
        self.retirements.commit_routes(&committed);
        abandoned
    }

    pub(crate) fn settle_host_frame_inputs(&mut self, inputs: &[HostInputOutcome]) -> bool {
        let Some(queued) = self.queued_snapshot.take() else {
            return false;
        };
        let applied = inputs.iter().any(|input| {
            matches!(
                input,
                HostInputOutcome::NativePlatformSnapshotApplied { .. }
            )
        });
        let rejected = inputs.iter().any(|input| {
            matches!(
                input,
                HostInputOutcome::NativePlatformSnapshotStale
                    | HostInputOutcome::NativePlatformProviderRejected
            )
        });
        debug_assert_ne!(
            applied, rejected,
            "one queued native snapshot has one outcome"
        );
        self.retirements.settle_snapshot(applied && !rejected);
        let snapshot_applied = applied && !rejected;
        debug_assert!(
            self.bridge
                .settle_viewport_roster(&queued.roster, snapshot_applied),
            "one queued native snapshot settles its exact roster envelope"
        );
        if snapshot_applied {
            self.work_areas.commit(queued.work_areas, &self.session);
            self.viewports
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .set_focus_reportable(queued.focus_reportable);
            for binding in queued.presentation_acknowledgements {
                self.pending_presentation_acknowledgements.remove(&binding);
            }
            self.input_control
                .settle_snapshot(queued.input_acknowledgement);
            if queued.roster.is_terminal() && self.requires_terminal_snapshot() {
                self.bridge.require_terminal_roster();
            }
            true
        } else {
            queued.roster.is_terminal()
        }
    }

    pub(crate) fn settle_close_control_inputs(
        &mut self,
        inputs: &[HostInputOutcome],
    ) -> Result<bool, NativeRuntimeError> {
        let (changed, errors) = self.settle_close_control_inputs_all(inputs);
        match errors.into_iter().next() {
            Some(error) => Err(error),
            None => Ok(changed),
        }
    }

    fn settle_close_control_inputs_all(
        &mut self,
        inputs: &[HostInputOutcome],
    ) -> (bool, Vec<NativeRuntimeError>) {
        let mut matched_request = false;
        let mut close_observation_applied = false;
        let mut close_observation_rejected = false;
        let mut errors = Vec::new();
        for input in inputs {
            match input {
                HostInputOutcome::NativePlatformSnapshotApplied { close_requests }
                | HostInputOutcome::NativeCloseObservationApplied { close_requests } => {
                    close_observation_applied = true;
                    for request in close_requests {
                        match self.close_control.bind_request(*request) {
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
        let clear_pending = self
            .close_control
            .settle_clear(close_observation_applied && !close_observation_rejected);
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
        let (recorded, errors) = self.try_report_retirement_quiescence_all();
        match errors.into_iter().next() {
            Some(error) => Err(error),
            None => Ok(recorded),
        }
    }

    fn try_report_retirement_quiescence_all(&mut self) -> (bool, Vec<NativeRuntimeError>) {
        let candidates = self.retirements.quiescence_candidates().collect::<Vec<_>>();
        let mut recorded = false;
        let mut errors = Vec::new();
        for binding in candidates {
            if self.bridge.references_binding(binding)
                || self.pointer_translator.references_binding(binding)
                || self.receivers.references_binding(binding)
                || self.deferred_viewports.viewport_for(binding).is_some()
                || self.effects.references_binding(binding)
                || self
                    .pending_effect_results
                    .iter()
                    .any(|pending| pending.blocks_quiescence(binding))
                || self.retirements.references_cleanup(binding)
                || self.close_control.references_binding(binding)
                || self.focus_control.references_binding(binding)
                || self.input_control.references_binding(binding)
                || self
                    .pending_presentation_acknowledgements
                    .contains_key(&binding)
            {
                continue;
            }
            if let Err(error) = self.session.report_native_binding_quiescence(binding) {
                errors.push(error.into());
                continue;
            }
            let finished = self.retirements.finish_quiescence(binding);
            debug_assert!(finished, "quiescence candidate remains pending");
            if finished {
                self.pending_effect_results
                    .retain(|pending| !pending.can_be_consumed_by_quiescence(binding));
                self.lifecycle_progress.record_quiescent_retirement();
            }
            recorded = true;
        }
        (recorded, errors)
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
        let mut reported = false;
        let mut deferred_stale = VecDeque::new();
        let pending_count = self.pending_effect_results.len();
        for _ in 0..pending_count {
            let Some(mut pending) = self.pending_effect_results.pop_front() else {
                break;
            };
            let binding = pending.result.binding();
            if pending.last_rejection == Some(NativeHostErrorKind::StaleBinding)
                && self.retirements.is_quiescence_candidate(binding)
            {
                deferred_stale.push_back(pending);
                continue;
            }
            if let Err(error) = self.session.report_native_effect_result(pending.result) {
                let (kind, result) = error.into_parts();
                pending.result = result;
                pending.last_rejection = Some(kind);
                if kind == NativeHostErrorKind::StaleBinding
                    && self.retirements.is_quiescence_candidate(binding)
                {
                    deferred_stale.push_back(pending);
                    continue;
                }
                self.pending_effect_results.push_front(pending);
                while let Some(stale) = deferred_stale.pop_back() {
                    self.pending_effect_results.push_front(stale);
                }
                return (
                    reported,
                    Some(NativeHostProtocolError::NativeEffectResultRejected(kind).into()),
                );
            }
            reported = true;
        }
        while let Some(stale) = deferred_stale.pop_back() {
            self.pending_effect_results.push_front(stale);
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
        for result in self.input_control.retire_binding(binding) {
            self.queue_pending_effect_result(result, None);
        }
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
                self.acknowledge_viewport_create_failure(failure)?;
                return Ok(true);
            }
            return Err(NativeHostProtocolError::ViewportCreateFailureWithoutEffect.into());
        };
        let callback_error = self.report_callback_terminal_result(result);
        self.acknowledge_viewport_create_failure(failure)?;
        if let Some((kind, error)) = callback_error
            && !(kind == NativeHostErrorKind::StaleBinding
                && self.retirements.is_quiescence_candidate(failure.binding()))
        {
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
    pub fn next_window_event(
        &mut self,
    ) -> Result<Option<NativeWindowEventRecord>, NativeRuntimeError> {
        self.prepare_output_prefix()?;
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
        self.prepare_output_prefix()?;
        Ok(self.bridge.front_viewport_create_failure())
    }

    fn next_viewport_visibility(
        &mut self,
    ) -> Result<Option<NativeViewportVisibilityRecord>, NativeRuntimeError> {
        self.prepare_output_prefix()?;
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
    ) -> Result<(), NativeRuntimeError> {
        if !self.bridge.acknowledge_viewport_create_failure(failure) {
            return Err(
                NativeHostProtocolError::ViewportCreateFailureAcknowledgementMismatch.into(),
            );
        }
        self.bridge
            .clear_create(failure.viewport(), failure.binding());
        self.bridge
            .clear_hidden_render(failure.viewport(), failure.binding());
        self.deferred_viewports.remove(failure.binding());
        let mut viewports = self
            .viewports
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if viewports.binding(failure.viewport()) == Some(failure.binding()) {
            let removed = viewports.remove_viewport(failure.viewport(), failure.binding());
            debug_assert!(
                removed.is_ok(),
                "exact failed reservation must be removable"
            );
        }
        drop(viewports);
        self.receivers.retire_binding(failure.binding());
        let retained = self
            .retirements
            .begin_unmaterialized_quiescence(failure.binding());
        debug_assert!(
            retained || self.retirements.has_quiescence_owner(failure.binding()),
            "a failed native create retains one exact quiescence owner"
        );
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

    /// Reduces the next callback record into one pending host-frame boundary.
    ///
    /// The mailbox remains the only callback-order authority. Pointer facts,
    /// close requests, exact destruction events, viewport rosters, and create
    /// failures are acknowledged only after their corresponding core input is
    /// retained. A later callback cannot join that boundary: the driver must
    /// commit or retry the exact core frame before reducing another record.
    pub(crate) fn reduce_callback_head(&mut self) -> Result<bool, NativeRuntimeError> {
        if self.bridge.callback_boundary_pending() {
            return Ok(false);
        }
        if self.reduce_next_viewport_close_cancelled()? {
            return Ok(true);
        }
        if self.reduce_next_viewport_roster()? {
            return Ok(true);
        }
        if self.reduce_next_viewport_create_failure()? {
            return Ok(true);
        }
        if self.reduce_next_viewport_visibility()? {
            return Ok(true);
        }
        if self.reduce_next_viewport_created()? {
            return Ok(true);
        }
        if self.reduce_next_staging_painted()? {
            return Ok(true);
        }
        if self.reduce_next_viewport_pointer_passthrough()? {
            return Ok(true);
        }
        if self.reduce_next_global_focus()? {
            return Ok(true);
        }
        if self.reduce_next_viewport_focus()? {
            return Ok(true);
        }
        let Some(record) = self.next_window_event()? else {
            return Ok(false);
        };
        let mut candidate = self.pointer_translator.clone();
        if matches!(record.event(), winit::event::WindowEvent::Destroyed)
            && let Some(binding) = record.binding()
        {
            if let Some(cancel) = candidate.cancel_destroyed_binding(binding) {
                self.record_pointer(cancel)?;
                // Keep the raw callback at the mailbox head while this first
                // causal boundary commits. Core intentionally requires the
                // provider reset to arrive in a later pointer prefix.
                self.pointer_translator = candidate;
                if !self.bridge.hold_event_for_next_boundary(record.ordinal()) {
                    return Err(NativeHostProtocolError::WindowEventAcknowledgementMismatch.into());
                }
                return Ok(true);
            }
            if let Some(reset) = candidate.reset_destroyed_binding(binding) {
                self.record_pointer(reset)?;
                // The recorder now owns the replayable reset. Publish it before
                // later destroyed-sidecar work can fail, so retrying the raw
                // callback cannot enqueue the same edge twice.
                self.pointer_translator = candidate.clone();
            }
        }
        let work_areas = &self.work_areas;
        match candidate.translate(&record, |point| work_areas.binding_at(point)) {
            NativePointerTranslation::NotPointer => self.reduce_non_pointer_event(&record)?,
            NativePointerTranslation::Ignored => {}
            NativePointerTranslation::Input(input) => self.record_pointer(input)?,
        }
        self.acknowledge_window_event(record.ordinal())?;
        self.pointer_translator = candidate;
        Ok(true)
    }

    fn reduce_next_viewport_pointer_passthrough(&mut self) -> Result<bool, NativeRuntimeError> {
        self.prepare_output_prefix()?;
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
                        && self.input_control.observe_unowned_applied(record)
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
                        if !self.input_control.accept_applied(record) {
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
                    NativeViewportPointerPassthroughStatus::Failed => {
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
        if let Some((_, error)) = callback_error {
            self.callback_errors.push_back(error);
        }
        Ok(true)
    }

    fn report_callback_terminal_result(
        &mut self,
        result: NativeEffectResult,
    ) -> Option<(NativeHostErrorKind, NativeRuntimeError)> {
        if let Err(error) = self.session.report_native_effect_result(result) {
            let (kind, result) = error.into_parts();
            let error = self.retain_rejected_effect_result(kind, result);
            Some((kind, error))
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
        self.prepare_output_prefix()?;
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
        self.prepare_output_prefix()?;
        let Some(record) = self.bridge.front_viewport_focus() else {
            return Ok(false);
        };
        let disposition = self.focus_control.classify(record);
        let mut callback_error = None;
        match disposition {
            NativeFocusResultDisposition::Unrelated => {}
            NativeFocusResultDisposition::Current { binding, status }
                if self.session.is_current_native_binding(binding)
                    && self
                        .viewports
                        .lock()
                        .unwrap_or_else(PoisonError::into_inner)
                        .focus_binding_for_event(record.window(), Some(record.viewport()))
                        == Some(binding) =>
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
                        || self
                            .viewports
                            .lock()
                            .unwrap_or_else(PoisonError::into_inner)
                            .focus_binding_for_event(record.window(), Some(record.viewport()))
                            != Some(binding),
                    "a current focus callback must take the exact route branch"
                );
            }
        }
        if !self.bridge.acknowledge_viewport_focus(record) {
            return Err(NativeHostProtocolError::ViewportFocusAcknowledgementMismatch.into());
        }
        if let Some((_, error)) = callback_error {
            self.callback_errors.push_back(error);
        }
        Ok(true)
    }

    fn reduce_next_viewport_close_cancelled(&mut self) -> Result<bool, NativeRuntimeError> {
        self.prepare_output_prefix()?;
        let Some(record) = self.bridge.front_viewport_close_cancelled() else {
            return Ok(false);
        };
        let acknowledgement = self
            .close_control
            .cancellation_observation(record)
            .ok_or(NativeHostProtocolError::NativeCloseCorrelationChanged)?;
        self.publish_close(
            record.binding(),
            NativeCloseState::Clear,
            Some(acknowledgement),
        )?;
        if !self.bridge.acknowledge_viewport_close_cancelled(record)
            || !self.close_control.mark_clear_recorded(record.binding())
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
        let current = self
            .viewports
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .binding_for_event(record.window(), Some(record.viewport()));
        if current != Some(record.binding())
            || !self.session.is_current_native_binding(record.binding())
        {
            let callback_error = self
                .effects
                .take_show(record.binding())
                .and_then(|request| {
                    self.report_callback_terminal_result(
                        request.dispatch_failed(NativeDispatchFailure::AdapterRejected),
                    )
                });
            self.acknowledge_viewport_visibility(record)?;
            if let Some((_, error)) = callback_error {
                self.callback_errors.push_back(error);
            }
            return Ok(true);
        }
        if !record.visible() {
            self.acknowledge_viewport_visibility(record)?;
            return Ok(true);
        }
        let Some(request) = self.effects.take_show(record.binding()) else {
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
                self.pending_presentation_acknowledgements
                    .insert(record.binding(), acknowledgement);
                self.bridge.invalidate_viewport_roster();
                None
            }
            NativeViewportVisibilityStatus::Unsupported => {
                let result = request.unsupported(NativeUnsupportedReason::BackendUnsupported);
                self.report_callback_terminal_result(result)
            }
        };
        self.acknowledge_viewport_visibility(record)?;
        if let Some((_, error)) = callback_error {
            self.callback_errors.push_back(error);
        }
        Ok(true)
    }

    fn has_pending_presentation_effect(&self, binding: NativeSurfaceBinding) -> bool {
        self.pending_presentation_acknowledgements
            .contains_key(&binding)
            || self.effects.has_pending_show(binding)
    }

    fn reduce_next_viewport_created(&mut self) -> Result<bool, NativeRuntimeError> {
        self.prepare_output_prefix()?;
        let Some(created) = self.bridge.front_viewport_created() else {
            return Ok(false);
        };
        if self
            .pending_presentation_acknowledgements
            .contains_key(&created.binding())
        {
            return Err(NativeHostProtocolError::PresentationAcknowledgementAlreadyPending.into());
        }
        let attach = self
            .viewports
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .attach_output(
                created.token().viewport_id(),
                created.binding(),
                created.token().window_id(),
                created.token().create_attempt(),
            );
        if attach.is_err() {
            return Err(NativeHostProtocolError::OutputRouteAttachmentFailed.into());
        }
        let request = self
            .effects
            .take_for_output(created.token().viewport_id(), created.binding())
            .ok_or(NativeHostProtocolError::UnexpectedViewportEffectAcknowledgement)?;
        let Some(NativeEffectAcknowledgement::Presentation(acknowledgement)) = request.accepted()
        else {
            return Err(NativeHostProtocolError::UnexpectedViewportEffectAcknowledgement.into());
        };
        self.pending_presentation_acknowledgements
            .insert(created.binding(), acknowledgement);
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

    fn reduce_next_staging_painted(&mut self) -> Result<bool, NativeRuntimeError> {
        self.prepare_output_prefix()?;
        let Some(staging) = self.bridge.front_staging_painted() else {
            return Ok(false);
        };
        if self.pending_outputs.contains_key(&staging.token()) {
            return Err(NativeHostProtocolError::OutputAwaitingAttachment.into());
        }
        let mut frame = self
            .session
            .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)?;
        frame.confirm_native_staging_painted(staging.request())?;
        frame.complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)?;
        let mut report = frame.commit()?;
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
        {
            drop(output);
            return Err(NativeHostProtocolError::OutputRouteAttachmentFailed.into());
        }
        self.pending_outputs
            .insert(staging.token(), PendingNativeOutput::Staging(output));
        if !self.bridge.acknowledge_staging_painted(staging) {
            return Err(NativeHostProtocolError::OutputRouteAttachmentFailed.into());
        }
        self.accept_native_effects(report.take_native_effects())?;
        self.bridge.commit_frame_boundary();
        Ok(true)
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
                self.close_control.observe_request(record, binding);
                self.publish_close(binding, NativeCloseState::Requested, None)?;
            }
            winit::event::WindowEvent::Destroyed => {
                let accepted_close = self
                    .close_control
                    .accepted_destroyed_matches(record, binding)
                    .map_err(|()| NativeHostProtocolError::NativeCloseCorrelationChanged)?;
                if self.session.recognizes_native_binding(binding)
                    && let Some(viewport) = record.viewport_id()
                {
                    self.fail_pending_viewport_effect(viewport, binding)?;
                    match self.retirements.observe_destroyed(viewport, binding) {
                        DestroyedObservation::Recorded => {
                            self.bridge.invalidate_viewport_roster();
                            if accepted_close
                                && !self
                                    .close_control
                                    .settle_accepted_destroyed(record, binding)
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
        self.prepare_output_prefix()?;
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
        if let (Ok(mut observations), Ok(frozen_work_areas)) =
            (self.compile_viewport_roster(roster), roster.work_areas())
        {
            let prepared_work_areas = self.work_areas.prepare(&frozen_work_areas)?;
            let destroyed = self
                .retirements
                .destroyed_observations()
                .map(|(binding, facts)| CompiledWindowObservation::new(binding, facts))
                .collect::<Vec<_>>();
            observations.extend(destroyed.iter().copied());
            let mut presentation_acknowledgements = Vec::new();
            let mut input_acknowledgement = None;
            let mut focus_reportable = Vec::new();
            for observation in &observations {
                let binding = observation.binding();
                if observation.presentation_acknowledged() {
                    presentation_acknowledgements.push(binding);
                }
                if observation.input_acknowledged() {
                    debug_assert!(
                        input_acknowledgement.replace(binding).is_none(),
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
                prepared_work_areas.core(),
            ) {
                Ok(()) => {
                    debug_assert!(self.queued_snapshot.is_none());
                    self.queued_snapshot = Some(QueuedNativeSnapshot {
                        roster: envelope,
                        presentation_acknowledgements,
                        input_acknowledgement,
                        focus_reportable,
                        work_areas: prepared_work_areas,
                    });
                    if !destroyed.is_empty() {
                        self.retirements.mark_snapshot_queued();
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
        let prepared_work_areas = self.work_areas.prepare(&FrozenWorkAreaRoster::Unknown)?;
        self.session.report_native_inventory_unknown()?;
        debug_assert!(self.queued_snapshot.is_none());
        self.queued_snapshot = Some(QueuedNativeSnapshot {
            roster: envelope,
            presentation_acknowledgements: Vec::new(),
            input_acknowledgement: None,
            focus_reportable: Vec::new(),
            work_areas: prepared_work_areas,
        });
        Ok(())
    }

    fn compile_viewport_roster(
        &self,
        roster: &NativeViewportRosterRecord,
    ) -> Result<Vec<CompiledWindowObservation>, NativeHostProtocolError> {
        #[cfg(test)]
        if let Some(compiled) = roster.compiled_override() {
            return compiled.map(|observations| {
                observations
                    .into_iter()
                    .map(|observation| self.with_native_input_observation(observation))
                    .collect()
            });
        }

        roster
            .observations()
            .iter()
            .copied()
            .map(|observation| {
                compile_window_observation(
                    observation.binding(),
                    observation.snapshot(),
                    self.pending_presentation_acknowledgements
                        .get(&observation.binding())
                        .copied(),
                )
                .map(|compiled| self.with_native_input_observation(compiled))
            })
            .collect()
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
    ) -> Result<(), NativeRuntimeError> {
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
    /// Returns an error retaining the result when its provider or binding is stale.
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
        self.session.report_native_binding_quiescence(binding)?;
        Ok(())
    }

    /// Begins one native host frame after applying queued renderer settlements.
    ///
    /// # Errors
    ///
    /// Returns an error when a renderer result is stale or the core rejects the
    /// frozen native input prefix.
    pub fn begin_host_frame(
        &mut self,
        resolve: impl FnMut(NativeReceiverQuery) -> NativeReceiverAnswer,
    ) -> Result<NativeHostFrame<'_>, NativeRuntimeError> {
        self.prepare_output_prefix()?;
        if self.bridge.has_pending_input() && !self.bridge.callback_boundary_pending() {
            return Err(NativeHostProtocolError::CallbackRecordPending.into());
        }
        Ok(NativeHostFrame::new(
            self.session.begin_native_host_frame(resolve)?,
            self.bridge.clone(),
            self.viewports.clone(),
            None,
        ))
    }

    /// Begins one native frame using only final-pass egui receiver authority.
    pub(crate) fn begin_resolved_host_frame(
        &mut self,
        context: &eframe::egui::Context,
    ) -> Result<NativeHostFrame<'_>, NativeRuntimeError> {
        self.prepare_output_prefix()?;
        if self.bridge.has_pending_input() && !self.bridge.callback_boundary_pending() {
            return Err(NativeHostProtocolError::CallbackRecordPending.into());
        }
        let bridge = self.bridge.clone();
        let receivers = &self.receivers;
        let frame = self
            .session
            .begin_native_host_frame(|query| receivers.resolve(context, query))?;
        Ok(NativeHostFrame::new(
            frame,
            bridge,
            self.viewports.clone(),
            Some(context.clone()),
        ))
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
        token: NativeOutputToken,
        output: PaintedSurfaceOutput,
        paint: &egui_dockspace::native_support::NativeSurfacePaint,
    ) -> Result<(), NativeOutputBindingError> {
        let Some(reservation) = self.bridge.output_reservation(token) else {
            return Err(NativeOutputBindingError::new(
                NativeOutputBindingErrorKind::TokenNotReserved,
                token,
                output,
            ));
        };
        if self.pending_outputs.contains_key(&token) {
            return Err(NativeOutputBindingError::new(
                NativeOutputBindingErrorKind::OutputAlreadyBound,
                token,
                output,
            ));
        }
        let current_binding = self
            .viewports
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .binding_for_output(
                token.viewport_id(),
                token.window_id(),
                token.create_attempt(),
            );
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
        if self
            .receivers
            .stage(token, binding, &output, paint)
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
        self.pending_outputs
            .insert(token, PendingNativeOutput::Surface(output));
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
        self.pending_outputs.remove(&token);
        NativeHostWake::RepaintRoot
    }

    pub(crate) fn quarantine_after_fatal(&mut self) -> Vec<NativeOutputToken> {
        let mut abandoned = self.bridge.quarantine_after_fatal();
        if self.requires_terminal_snapshot() {
            self.bridge.require_terminal_roster();
        }
        for &token in &abandoned {
            self.receivers.abandon(token);
            self.pending_outputs.remove(&token);
        }
        self.cancel_undispatched_control_commands();
        abandoned.extend(self.cancel_unadmitted_viewport_effects());
        abandoned.sort_unstable();
        abandoned.dedup();
        abandoned
    }

    fn requires_terminal_snapshot(&self) -> bool {
        !self.pending_presentation_acknowledgements.is_empty()
            || self.input_control.requires_snapshot()
            || self.retirements.requires_snapshot()
            || self.bridge.viewport_roster_dirty()
    }

    fn cancel_undispatched_control_commands(&mut self) {
        self.input_control.begin_quarantine();
        if let Some(binding) = self.focus_control.pending_binding() {
            self.terminalize_queued_focus(binding);
        }
    }

    fn cancel_unadmitted_viewport_effects(&mut self) -> Vec<NativeOutputToken> {
        let plans = {
            let viewports = self
                .viewports
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            self.effects
                .awaiting_viewport_callbacks()
                .filter(|plan| {
                    !viewports.has_admitted_create_attempt(plan.viewport(), plan.binding())
                })
                .collect::<Vec<_>>()
        };
        let mut abandoned = Vec::new();
        for plan in plans {
            let Some(request) = self.effects.remove_unstarted(plan) else {
                continue;
            };
            abandoned.extend(
                self.bridge
                    .retire_deferred_binding(plan.viewport(), plan.binding()),
            );
            self.deferred_viewports.remove(plan.binding());
            self.receivers.retire_binding(plan.binding());
            let removed = self
                .viewports
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .remove_viewport(plan.viewport(), plan.binding());
            debug_assert!(
                removed.is_ok(),
                "unadmitted viewport effect retains one exact route"
            );
            drop(request);
        }
        abandoned
    }

    fn prepare_output_prefix(&mut self) -> Result<(), NativeRuntimeError> {
        if !self.bridge.output_order_is_valid() {
            return Err(NativeHostProtocolError::OutputOrderViolation.into());
        }
        for (result, submitted) in self.bridge.output_prefix() {
            if submitted {
                continue;
            }
            let Some(output) = self.pending_outputs.remove(&result.token()) else {
                return Err(NativeHostProtocolError::OutputAwaitingAttachment.into());
            };
            let binding = self
                .bridge
                .output_reservation(result.token())
                .and_then(|reservation| reservation.binding());
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
                            self.receivers.presented(result.token(), binding);
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
            debug_assert!(self.bridge.mark_output_submitted(result.token()));
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

impl Drop for NativeCoordinator {
    fn drop(&mut self) {
        self.bridge.freeze();
    }
}

#[cfg(test)]
mod tests;
