//! Thin coordinator between eframe callbacks and the renderer-neutral session.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, PoisonError};

use dockspace::geometry::PhysicalRect;
use dockspace::model::SurfaceId;
use dockspace::runtime::{
    DockspaceSession, HostInputOutcome, HostWindowToken, NativeCloseEffectAcknowledgement,
    NativeCloseState, NativeDispatchFailure, NativeEffectAcknowledgement, NativeEffectOperation,
    NativeEffectRequest, NativeEffectResult, NativeEffectSubmissionError, NativeHostErrorKind,
    NativePointerInput, NativePointerRoster, NativePresentationEffectAcknowledgement,
    NativeReceiverAnswer, NativeReceiverQuery, NativeSurfaceBinding, NativeUnsupportedReason,
    NativeWindowFacts, NativeWorkAreaBinding, NativeWorkAreaRoster, PaintedNativeStagingOutput,
    PaintedSurfaceOutput, SurfacePresentationResult, SurfaceUnavailableReason,
};
use eframe::{
    NativeHostHandler, NativeHostWake, NativeOutputStatus, NativeOutputToken, NativePhysicalRect,
    NativeViewportCreateFailureKind, NativeViewportVisibilityStatus, egui::ViewportId,
};
use winit::window::WindowId;

use crate::deferred_viewport::{DeferredViewportDriver, DeferredViewportSpec, viewport_id_for};
use crate::effect_coordinator::{
    NativeEffectCoordinator, NativeViewportEffectKind, NativeViewportEffectPlan, PendingShowEffect,
};
use crate::error::{
    NativeHostProtocolError, NativeOutputBindingError, NativeOutputBindingErrorKind,
};
use crate::host_frame::NativeHostFrame;
use crate::mailbox::{
    DeferredViewportPaint, NativeHostBridge, NativeViewportRosterRecord, PreparedRouteRetirements,
};
#[cfg(test)]
use crate::mailbox::{HostRecord, OutputReservation};
use crate::pointer_event::{NativePointerTranslation, NativePointerTranslator};
use crate::receiver::NativeReceiverStore;
use crate::retirement::{CleanupResultRetentionError, CommittedRetirement, NativeRetirementState};
use crate::viewport_callback::{NativeViewportCreateFailureRecord, NativeViewportVisibilityRecord};
use crate::viewport_map::NativeViewportMap;
use crate::window_snapshot::{CompiledWindowObservation, compile_window_observation};
use crate::work_area::{FrozenWorkAreaRoster, NativeWorkAreaState, PreparedWorkAreaRoster};
use crate::{NativeRuntimeError, NativeViewportBindingError, NativeWindowEventRecord};

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
    retirements: NativeRetirementState,
    pointer_translator: NativePointerTranslator,
    work_areas: NativeWorkAreaState,
}

#[derive(Debug)]
struct QueuedNativeSnapshot {
    acknowledgements: Vec<NativeSurfaceBinding>,
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
                "queued_snapshot_acknowledgements",
                &self
                    .queued_snapshot
                    .as_ref()
                    .map_or(0, |queued| queued.acknowledgements.len()),
            )
            .field("deferred_viewports", &self.deferred_viewports)
            .field("receivers", &self.receivers)
            .field("effects", &self.effects)
            .field("retirements", &self.retirements)
            .field("pointer_translator", &self.pointer_translator)
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
            retirements: NativeRetirementState::default(),
            pointer_translator: NativePointerTranslator::default(),
            work_areas: NativeWorkAreaState::default(),
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
            .binding_for_output(token.viewport_id(), token.window_id());
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
            self.pending_presentation_acknowledgements
                .remove(&retirement.binding());
            self.effects.remove_show(retirement.binding());
            self.receivers.retire_binding(retirement.binding());
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
        if applied && !rejected {
            self.work_areas.commit(queued.work_areas, &self.session);
            for binding in queued.acknowledgements {
                self.pending_presentation_acknowledgements.remove(&binding);
            }
            true
        } else {
            false
        }
    }

    pub(crate) fn settle_native_admissions(
        &mut self,
        admissions: &[NativeSurfaceBinding],
    ) -> Result<bool, NativeRuntimeError> {
        let mut changed = false;
        for binding in admissions {
            let viewport = self.surface_viewport(binding.surface()).ok_or(
                NativeHostProtocolError::NativeAdmissionRouteChanged(binding.surface()),
            )?;
            if self.viewport_binding(viewport) != Some(*binding)
                || !self.session.is_current_native_binding(*binding)
            {
                return Err(NativeHostProtocolError::NativeAdmissionRouteChanged(
                    binding.surface(),
                )
                .into());
            }
            changed |= self.bridge.clear_hidden_render(viewport, *binding);
        }
        Ok(changed)
    }

    pub(crate) fn try_report_retirement_quiescence(&mut self) -> Result<bool, NativeRuntimeError> {
        let candidates = self.retirements.quiescence_candidates().collect::<Vec<_>>();
        let mut recorded = false;
        for binding in candidates {
            if self.bridge.references_binding(binding)
                || self.receivers.references_binding(binding)
                || self.deferred_viewports.viewport_for(binding).is_some()
                || self.effects.references_binding(binding)
                || self.retirements.references_cleanup(binding)
                || self
                    .pending_presentation_acknowledgements
                    .contains_key(&binding)
            {
                continue;
            }
            self.session.report_native_binding_quiescence(binding)?;
            let finished = self.retirements.finish_quiescence(binding);
            debug_assert!(finished, "quiescence candidate remains pending");
            recorded = true;
        }
        Ok(recorded)
    }

    fn exact_viewport_for_binding(&self, binding: NativeSurfaceBinding) -> Option<ViewportId> {
        let viewports = self
            .viewports
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let viewport = viewports.viewport(binding.surface())?;
        (viewports.binding(viewport) == Some(binding)).then_some(viewport)
    }

    fn fail_pending_viewport_effect(
        &mut self,
        viewport: ViewportId,
        binding: NativeSurfaceBinding,
    ) -> Result<(), NativeRuntimeError> {
        let failure = NativeViewportCreateFailureRecord::new(
            viewport,
            binding,
            NativeViewportCreateFailureKind::WindowUnavailable,
        );
        if !self.effects.prepare_failure(failure) {
            return Ok(());
        }
        if let Some(result) = self.effects.take_failure_result(failure)
            && let Err(error) = self.session.report_native_effect_result(result)
        {
            let (kind, result) = error.into_parts();
            self.effects
                .restore_failure_result(failure, result)
                .unwrap_or_else(|_| {
                    panic!("destroyed native effect lost its retryable terminal result")
                });
            return Err(NativeHostProtocolError::NativeEffectResultRejected(kind).into());
        }
        if self.effects.failure_reported(failure) {
            debug_assert!(self.effects.finish_failure(failure));
        }
        Ok(())
    }

    fn retire_deferred_sidecars(&mut self, viewport: ViewportId, binding: NativeSurfaceBinding) {
        let suppressed = self
            .viewports
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .suppress_pointer_route(viewport, binding);
        debug_assert!(
            suppressed,
            "retired sidecars retain one exact viewport route"
        );
        self.deferred_viewports.remove(binding);
        self.effects.remove_show(binding);
        self.receivers.retire_binding(binding);
        for token in self.bridge.retire_deferred_binding(viewport, binding) {
            self.pending_outputs.remove(&token);
            self.receivers.abandon(token);
        }
    }

    /// Accepts the minimal platform effects implemented by this vertical slice.
    ///
    /// New and replacement child windows retain their affine request until
    /// eframe reports the first exact callback or a typed create failure.
    /// `ShowWindow` is accepted only for an existing exact retained viewport.
    /// `ReleaseChild` stops re-declaring the child but retains its route until
    /// an exact destruction callback commits. Unrelated platform operations
    /// remain explicitly unsupported.
    pub(crate) fn accept_native_effects(
        &mut self,
        requests: Vec<NativeEffectRequest>,
    ) -> Result<(), NativeRuntimeError> {
        for request in requests {
            match request.operation() {
                NativeEffectOperation::CreateWindow {
                    binding,
                    placement,
                    role,
                }
                | NativeEffectOperation::RequestReplacement {
                    binding,
                    placement,
                    role,
                } => {
                    let binding = *binding;
                    let placement = *placement;
                    let role = *role;
                    let kind = match request.operation() {
                        NativeEffectOperation::CreateWindow { .. } => {
                            NativeViewportEffectKind::Create
                        }
                        NativeEffectOperation::RequestReplacement { .. } => {
                            NativeViewportEffectKind::Replacement
                        }
                        _ => unreachable!("matched one deferred viewport operation"),
                    };
                    let viewport = match kind {
                        NativeViewportEffectKind::Create => viewport_id_for(binding),
                        NativeViewportEffectKind::Replacement => {
                            let Some(viewport) = self.surface_viewport(binding.surface()) else {
                                self.submit_unsupported_effect(request)?;
                                continue;
                            };
                            viewport
                        }
                    };
                    let predecessor = match kind {
                        NativeViewportEffectKind::Create => None,
                        NativeViewportEffectKind::Replacement => self.viewport_binding(viewport),
                    };
                    if !self
                        .deferred_viewports
                        .can_insert(viewport, binding, placement, role)
                        || predecessor.is_some_and(|predecessor| {
                            !self
                                .retirements
                                .can_retire_committed_route_for_replacement(viewport, predecessor)
                        })
                    {
                        self.submit_unsupported_effect(request)?;
                        continue;
                    }
                    let plan = match self.retain_viewport_effect(viewport, request) {
                        Ok(plan) => plan,
                        Err(request) => {
                            self.submit_unsupported_effect(request)?;
                            continue;
                        }
                    };
                    debug_assert_eq!(plan.binding(), binding);
                    debug_assert_eq!(plan.placement(), placement);
                    debug_assert_eq!(plan.role(), role);
                    debug_assert_eq!(plan.kind(), kind);
                    let inserted = self
                        .deferred_viewports
                        .insert(viewport, binding, placement, role);
                    assert!(inserted, "preflighted deferred viewport must insert");
                    if let Some(predecessor) = predecessor {
                        assert!(
                            self.retirements
                                .retire_committed_route_for_replacement(viewport, predecessor),
                            "preflighted predecessor route must retire through replacement"
                        );
                        self.receivers.retire_binding(predecessor);
                    }
                }
                NativeEffectOperation::ShowWindow { binding } => {
                    let binding = *binding;
                    let Some(viewport) = self.deferred_viewports.viewport_for(binding) else {
                        self.submit_unsupported_effect(request)?;
                        continue;
                    };
                    let current = self
                        .viewports
                        .lock()
                        .unwrap_or_else(PoisonError::into_inner)
                        .binding(viewport);
                    if current != Some(binding) || self.has_pending_presentation_effect(binding) {
                        self.submit_unsupported_effect(request)?;
                        continue;
                    }
                    let shown = self.deferred_viewports.set_visible(binding);
                    assert_eq!(shown, Some(viewport));
                    self.effects.retain_show(binding, request).map_err(|_| {
                        NativeHostProtocolError::UnexpectedViewportEffectAcknowledgement
                    })?;
                }
                NativeEffectOperation::ReleaseChild { binding }
                | NativeEffectOperation::CompensatingClose { binding } => {
                    let binding = *binding;
                    let Some(viewport) = self.exact_viewport_for_binding(binding) else {
                        if self.session.is_current_native_binding(binding) {
                            self.submit_unsupported_effect(request)?;
                            continue;
                        }
                        return Err(NativeHostProtocolError::RetiredViewportRouteChanged(
                            binding.surface(),
                        )
                        .into());
                    };
                    if !self.session.recognizes_native_binding(binding)
                        || !self.retirements.can_begin_release(viewport, binding)
                    {
                        self.submit_unsupported_effect(request)?;
                        continue;
                    }
                    self.fail_pending_viewport_effect(viewport, binding)?;
                    let Some(NativeEffectAcknowledgement::Close(acknowledgement)) =
                        request.accepted()
                    else {
                        return Err(
                            NativeHostProtocolError::UnexpectedViewportEffectAcknowledgement.into(),
                        );
                    };
                    self.retire_deferred_sidecars(viewport, binding);
                    let inserted =
                        self.retirements
                            .begin_release(viewport, binding, acknowledgement);
                    assert!(inserted, "preflighted native retirement must insert");
                }
                NativeEffectOperation::AwaitCleanup { binding } => {
                    let binding = *binding;
                    if !self.session.recognizes_native_binding(binding)
                        || !self.retirements.can_accept_cleanup_observation(binding)
                    {
                        self.submit_failed_effect(request, NativeDispatchFailure::AdapterRejected)?;
                        continue;
                    }
                    let Some(NativeEffectAcknowledgement::Cleanup(observation)) =
                        request.accepted()
                    else {
                        return Err(
                            NativeHostProtocolError::UnexpectedViewportEffectAcknowledgement.into(),
                        );
                    };
                    let correlated = self
                        .retirements
                        .accept_cleanup_observation(binding, observation)
                        .map_err(|()| NativeHostProtocolError::CleanupRelayConflict)?;
                    if let Some(result) = correlated {
                        self.report_cleanup_result(binding, result)?;
                    }
                }
                _ => {
                    self.submit_unsupported_effect(request)?;
                }
            }
        }
        Ok(())
    }

    fn submit_unsupported_effect(
        &mut self,
        request: NativeEffectRequest,
    ) -> Result<(), NativeRuntimeError> {
        let binding = request.operation().binding();
        let result = request.unsupported(NativeUnsupportedReason::BackendUnsupported);
        if let Err(error) = self.session.report_native_effect_result(result) {
            return self.retain_or_return_effect_result(binding, error);
        }
        Ok(())
    }

    fn submit_failed_effect(
        &mut self,
        request: NativeEffectRequest,
        reason: NativeDispatchFailure,
    ) -> Result<(), NativeRuntimeError> {
        let binding = request.operation().binding();
        let result = request.dispatch_failed(reason);
        if let Err(error) = self.session.report_native_effect_result(result) {
            return self.retain_or_return_effect_result(binding, error);
        }
        Ok(())
    }

    fn report_cleanup_result(
        &mut self,
        binding: NativeSurfaceBinding,
        result: NativeEffectResult,
    ) -> Result<(), NativeRuntimeError> {
        if let Err(error) = self.session.report_native_effect_result(result) {
            return self.retain_or_return_effect_result(binding, error);
        }
        Ok(())
    }

    fn retain_or_return_effect_result(
        &mut self,
        binding: NativeSurfaceBinding,
        error: NativeEffectSubmissionError,
    ) -> Result<(), NativeRuntimeError> {
        let (kind, result) = error.into_parts();
        if !result.binding().same_window_lifetime(binding)
            || !result.can_be_correlated_by_cleanup()
            || !matches!(kind, NativeHostErrorKind::StaleBinding)
        {
            return Err(Self::fatal_effect_submission(kind, result));
        }
        if self.retirements.cleanup_is_terminal(binding) {
            drop(result);
            return Ok(());
        }
        if !self.retirements.can_accept_cleanup_result(binding) {
            return Err(Self::fatal_effect_submission(kind, result));
        }
        match self.retirements.retain_cleanup_result(binding, result) {
            Ok(Some(correlated)) => self.report_cleanup_result(binding, correlated),
            Ok(None) => Ok(()),
            Err(CleanupResultRetentionError::CorrelationMismatch) => {
                Err(NativeHostProtocolError::CleanupRelayConflict.into())
            }
            Err(CleanupResultRetentionError::Occupied(result)) => {
                Err(Self::fatal_effect_submission(kind, result))
            }
        }
    }

    fn fatal_effect_submission(
        kind: NativeHostErrorKind,
        result: NativeEffectResult,
    ) -> NativeRuntimeError {
        // The application treats host-protocol errors as terminal and drops
        // the whole coordinator/session. Do not expose a misleading recovery
        // capability whose owning session can no longer make progress.
        drop(result);
        NativeHostProtocolError::NativeEffectResultRejected(kind).into()
    }

    /// Retains one core-emitted deferred viewport effect until eframe reports
    /// an exact success or failure callback.
    ///
    /// The caller must invoke this only after the core host frame which emitted
    /// `request` committed. A rejected request is returned unchanged so the
    /// driver can convert it into an explicit typed dispatch result.
    pub(crate) fn retain_viewport_effect(
        &mut self,
        viewport: ViewportId,
        request: dockspace::runtime::NativeEffectRequest,
    ) -> Result<NativeViewportEffectPlan, dockspace::runtime::NativeEffectRequest> {
        let Some(plan) = self.effects.plan(viewport, &request) else {
            return Err(request);
        };
        if !self.session.is_current_native_binding(plan.binding()) {
            return Err(request);
        }
        let predecessor = {
            let viewports = self
                .viewports
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            viewports.binding(viewport)
        };
        if matches!(plan.kind(), NativeViewportEffectKind::Replacement) && predecessor.is_none() {
            return Err(request);
        }
        let Some(requested_rect) = exact_native_rect(plan.placement()) else {
            return Err(request);
        };
        if !self
            .bridge
            .reserve_create(viewport, plan.binding(), requested_rect)
        {
            return Err(request);
        }
        if let Err(request) = self.effects.insert(plan, request) {
            debug_assert!(self.bridge.clear_create(viewport, plan.binding()));
            return Err(request);
        }

        let reserve_result = {
            let mut viewports = self
                .viewports
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            match (plan.kind(), predecessor) {
                (NativeViewportEffectKind::Create, None) => {
                    viewports.reserve(viewport, plan.binding())
                }
                (NativeViewportEffectKind::Replacement, Some(predecessor)) => viewports
                    .reserve_retired_replacement(viewport, predecessor, plan.binding()),
                (NativeViewportEffectKind::Create, Some(_)) => {
                    Err(NativeViewportBindingError::ViewportAlreadyBound {
                        viewport,
                        existing: viewports
                            .binding(viewport)
                            .expect("matched viewport binding remains present")
                            .surface(),
                    })
                }
                (NativeViewportEffectKind::Replacement, None) => {
                    unreachable!("replacement predecessor was checked before retention")
                }
            }
        };
        if reserve_result.is_err() {
            debug_assert!(self.bridge.clear_create(viewport, plan.binding()));
            return Err(self
                .effects
                .remove_unstarted(plan)
                .expect("failed viewport reservation retains its affine request"));
        }
        Ok(plan)
    }

    /// Reduces one exact deferred-viewport failure without losing the affine
    /// effect result across a retryable core rejection.
    pub(crate) fn reduce_next_viewport_create_failure(
        &mut self,
    ) -> Result<bool, NativeRuntimeError> {
        let Some(failure) = self.next_viewport_create_failure()? else {
            return Ok(false);
        };
        if !self.effects.prepare_failure(failure) {
            return Err(NativeHostProtocolError::ViewportCreateFailureWithoutEffect.into());
        }
        if let Some(result) = self.effects.take_failure_result(failure)
            && let Err(error) = self.session.report_native_effect_result(result)
        {
            let (kind, result) = error.into_parts();
            self.effects
                .restore_failure_result(failure, result)
                .unwrap_or_else(|_| {
                    panic!("retryable native effect result lost its exact failure owner")
                });
            return Err(NativeHostProtocolError::NativeEffectResultRejected(kind).into());
        }
        debug_assert!(self.effects.failure_reported(failure));
        self.acknowledge_viewport_create_failure(failure)?;
        debug_assert!(self.effects.finish_failure(failure));
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

    /// Reduces the journal head when it is a pointer event.
    ///
    /// The translator is copied before reduction. Smooth-scroll sequence state
    /// is published only after the core accepts the corresponding input and the
    /// immutable callback record is acknowledged, so a rejected edge remains
    /// replayable without advancing the adapter-owned sequence state.
    pub(crate) fn reduce_next_pointer_event(&mut self) -> Result<bool, NativeRuntimeError> {
        let Some(record) = self.next_window_event()? else {
            return Ok(false);
        };
        let mut candidate = self.pointer_translator;
        let work_areas = &self.work_areas;
        match candidate.translate(&record, |point| work_areas.binding_at(point)) {
            NativePointerTranslation::NotPointer => Ok(false),
            NativePointerTranslation::Ignored => {
                self.acknowledge_window_event(record.ordinal())?;
                self.pointer_translator = candidate;
                Ok(true)
            }
            NativePointerTranslation::Input(input) => {
                self.record_pointer(input)?;
                self.acknowledge_window_event(record.ordinal())?;
                self.pointer_translator = candidate;
                Ok(true)
            }
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
        let Some(record) = self.next_window_event()? else {
            return Ok(false);
        };
        let mut candidate = self.pointer_translator;
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

    fn reduce_next_viewport_visibility(&mut self) -> Result<bool, NativeRuntimeError> {
        let Some(record) = self.next_viewport_visibility()? else {
            return Ok(false);
        };
        let current = self
            .viewports
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .binding_for_event(record.window(), Some(record.viewport()));
        if current != Some(record.binding())
            || !self.session.is_current_native_binding(record.binding())
        {
            self.acknowledge_viewport_visibility(record)?;
            return Ok(true);
        }
        if !record.visible() {
            self.acknowledge_viewport_visibility(record)?;
            return Ok(true);
        }
        let Some(pending) = self.effects.take_show(record.binding()) else {
            self.acknowledge_viewport_visibility(record)?;
            return Ok(true);
        };
        match (record.status(), pending) {
            (
                NativeViewportVisibilityStatus::Dispatched,
                PendingShowEffect::AwaitingDispatch(request),
            ) => {
                let Some(NativeEffectAcknowledgement::Presentation(acknowledgement)) =
                    request.accepted()
                else {
                    return Err(
                        NativeHostProtocolError::UnexpectedViewportEffectAcknowledgement.into(),
                    );
                };
                self.pending_presentation_acknowledgements
                    .insert(record.binding(), acknowledgement);
            }
            (
                NativeViewportVisibilityStatus::Unsupported,
                PendingShowEffect::AwaitingDispatch(request),
            ) => {
                let result = request.unsupported(NativeUnsupportedReason::BackendUnsupported);
                self.report_or_retain_show_result(record.binding(), result)?;
            }
            (_, PendingShowEffect::AwaitingUnsupportedReport(result)) => {
                self.report_or_retain_show_result(record.binding(), result)?;
            }
        }
        self.acknowledge_viewport_visibility(record)?;
        Ok(true)
    }

    fn report_or_retain_show_result(
        &mut self,
        binding: NativeSurfaceBinding,
        result: NativeEffectResult,
    ) -> Result<(), NativeRuntimeError> {
        if let Err(error) = self.session.report_native_effect_result(result) {
            let (kind, result) = error.into_parts();
            self.effects
                .restore_show_result(binding, result)
                .map_err(|_| NativeHostProtocolError::UnexpectedViewportEffectAcknowledgement)?;
            return Err(NativeHostProtocolError::NativeEffectResultRejected(kind).into());
        }
        Ok(())
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
            .attach(
                created.token().viewport_id(),
                created.binding(),
                created.token().window_id(),
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
                self.publish_close(binding, NativeCloseState::Requested, None)?;
            }
            winit::event::WindowEvent::Destroyed => {
                if self.session.recognizes_native_binding(binding)
                    && let Some(viewport) = record.viewport_id()
                {
                    self.fail_pending_viewport_effect(viewport, binding)?;
                    if self.retirements.observe_destroyed(viewport, binding) {
                        self.retire_deferred_sidecars(viewport, binding);
                    }
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
        self.record_viewport_roster(&roster)?;
        if !self.bridge.acknowledge_viewport_roster() {
            return Err(NativeHostProtocolError::ViewportRosterAcknowledgementMismatch.into());
        }
        Ok(true)
    }

    fn record_viewport_roster(
        &mut self,
        roster: &NativeViewportRosterRecord,
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
            let acknowledged_bindings = observations
                .iter()
                .filter(|observation| observation.presentation_acknowledged())
                .map(|observation| observation.binding())
                .collect::<Vec<_>>();
            match self.session.report_managed_native_snapshot(
                observations
                    .iter()
                    .map(|observation| (observation.binding(), observation.facts())),
                prepared_work_areas.core(),
            ) {
                Ok(()) => {
                    debug_assert!(self.queued_snapshot.is_none());
                    self.queued_snapshot = Some(QueuedNativeSnapshot {
                        acknowledgements: acknowledged_bindings,
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
            acknowledgements: Vec::new(),
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
            return compiled;
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
            })
            .collect()
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
        Ok(NativeHostFrame::new(frame, bridge, self.viewports.clone()))
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
            .binding_for_output(token.viewport_id(), token.window_id());
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
        self.bridge.deactivate();
    }
}

#[cfg(test)]
mod tests;
