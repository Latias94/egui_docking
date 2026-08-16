//! Native effect dispatch, acknowledgement retention, and retirement handoff.

use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NativeRetirementDispatch {
    OmitDeferredViewport,
    CloseViewport,
}

impl NativeCoordinator {
    fn exact_viewport_for_binding(&self, binding: NativeSurfaceBinding) -> Option<ViewportId> {
        let viewports = self
            .viewports
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let viewport = viewports.viewport(binding.surface())?;
        (viewports.binding(viewport) == Some(binding)).then_some(viewport)
    }

    pub(super) fn exact_window_route_for_binding(
        &self,
        binding: NativeSurfaceBinding,
    ) -> Option<(ViewportId, WindowId)> {
        let viewports = self
            .viewports
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let viewport = viewports.viewport(binding.surface())?;
        if viewports.binding(viewport) != Some(binding) {
            return None;
        }
        Some((viewport, viewports.window(viewport)?))
    }

    pub(super) fn fail_pending_viewport_effect(
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

    pub(super) fn retire_deferred_sidecars(
        &mut self,
        viewport: ViewportId,
        binding: NativeSurfaceBinding,
    ) {
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
        self.effects.remove_commands(binding);
        self.focus_control.retire_binding(binding);
        self.input_control.retire_binding(binding);
        self.receivers.retire_binding(binding);
        for token in self.bridge.retire_deferred_binding(viewport, binding) {
            self.pending_outputs.remove(&token);
            self.receivers.abandon(token);
        }
    }

    fn accept_retirement_effect(
        &mut self,
        request: NativeEffectRequest,
        binding: NativeSurfaceBinding,
        dispatch: NativeRetirementDispatch,
    ) -> Result<(), NativeRuntimeError> {
        let Some(viewport) = self.exact_viewport_for_binding(binding) else {
            if self.session.is_current_native_binding(binding) {
                return self.submit_unsupported_effect(request);
            }
            return Err(
                NativeHostProtocolError::RetiredViewportRouteChanged(binding.surface()).into(),
            );
        };
        if !self.session.recognizes_native_binding(binding)
            || !self.retirements.can_begin_release(viewport, binding)
        {
            return self.submit_unsupported_effect(request);
        }
        self.fail_pending_viewport_effect(viewport, binding)?;
        let Some(NativeEffectAcknowledgement::Close(acknowledgement)) = request.accepted() else {
            return Err(NativeHostProtocolError::UnexpectedViewportEffectAcknowledgement.into());
        };
        self.retire_deferred_sidecars(viewport, binding);
        let inserted = self
            .retirements
            .begin_release(viewport, binding, acknowledgement);
        assert!(inserted, "preflighted native retirement must insert");
        if dispatch == NativeRetirementDispatch::CloseViewport {
            self.effects
                .queue_command(viewport, binding, ViewportCommand::Close);
        }
        Ok(())
    }

    /// Accepts the minimal platform effects implemented by this vertical slice.
    ///
    /// New and replacement child windows retain their affine request until
    /// eframe reports the first exact callback or a typed create failure.
    /// `ShowWindow` is accepted only for an existing exact retained viewport.
    /// `ReleaseChild` stops re-declaring the child but retains its route until
    /// an exact destruction callback commits. `RequestRootClose` uses the same
    /// retirement lane and additionally queues one exact eframe close command.
    /// Unrelated platform operations remain explicitly unsupported.
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
                    self.accept_retirement_effect(
                        request,
                        binding,
                        NativeRetirementDispatch::OmitDeferredViewport,
                    )?;
                }
                NativeEffectOperation::RequestRootClose { binding } => {
                    let binding = *binding;
                    self.accept_retirement_effect(
                        request,
                        binding,
                        NativeRetirementDispatch::CloseViewport,
                    )?;
                }
                NativeEffectOperation::ResolveNativeClose { close, resolution } => {
                    let close = *close;
                    match resolution {
                        NativeCloseDisposition::Accept => {
                            if !self.close_control.recognizes_request(close) {
                                self.submit_failed_effect(
                                    request,
                                    NativeDispatchFailure::AdapterRejected,
                                )?;
                                continue;
                            }
                            self.accept_retirement_effect(
                                request,
                                close.binding(),
                                NativeRetirementDispatch::OmitDeferredViewport,
                            )?;
                            if !self.close_control.accept_close(close) {
                                return Err(
                                    NativeHostProtocolError::NativeCloseCorrelationChanged.into()
                                );
                            }
                        }
                        NativeCloseDisposition::Cancel => {
                            let Some(viewport) = self.close_control.cancellation_viewport(close)
                            else {
                                self.submit_failed_effect(
                                    request,
                                    NativeDispatchFailure::AdapterRejected,
                                )?;
                                continue;
                            };
                            let Some(NativeEffectAcknowledgement::Close(acknowledgement)) =
                                request.accepted()
                            else {
                                return Err(
                                    NativeHostProtocolError::UnexpectedViewportEffectAcknowledgement
                                        .into(),
                                );
                            };
                            if self
                                .close_control
                                .begin_cancellation(close, acknowledgement)
                                != Some(viewport)
                            {
                                return Err(
                                    NativeHostProtocolError::NativeCloseCorrelationChanged.into()
                                );
                            }
                            self.effects.queue_command(
                                viewport,
                                close.binding(),
                                ViewportCommand::CancelClose,
                            );
                        }
                    }
                }
                NativeEffectOperation::CancelRootClose { binding } => {
                    let binding = *binding;
                    let Some(close) = self.close_control.request_for_binding(binding) else {
                        self.submit_failed_effect(request, NativeDispatchFailure::AdapterRejected)?;
                        continue;
                    };
                    let Some(viewport) = self.close_control.cancellation_viewport(close) else {
                        self.submit_failed_effect(request, NativeDispatchFailure::AdapterRejected)?;
                        continue;
                    };
                    let Some(NativeEffectAcknowledgement::Close(acknowledgement)) =
                        request.accepted()
                    else {
                        return Err(
                            NativeHostProtocolError::UnexpectedViewportEffectAcknowledgement.into(),
                        );
                    };
                    if self
                        .close_control
                        .begin_cancellation(close, acknowledgement)
                        != Some(viewport)
                    {
                        return Err(NativeHostProtocolError::NativeCloseCorrelationChanged.into());
                    }
                    self.effects
                        .queue_command(viewport, binding, ViewportCommand::CancelClose);
                }
                NativeEffectOperation::RequestFocus { binding } => {
                    let binding = *binding;
                    let Some((viewport, window)) = self.exact_window_route_for_binding(binding)
                    else {
                        self.submit_failed_effect(
                            request,
                            NativeDispatchFailure::WindowUnavailable,
                        )?;
                        continue;
                    };
                    match self
                        .focus_control
                        .retain(viewport, window, binding, request)
                    {
                        Ok(()) => {
                            self.effects
                                .queue_command(viewport, binding, ViewportCommand::Focus);
                        }
                        Err(request) => {
                            self.submit_failed_effect(
                                request,
                                NativeDispatchFailure::AdapterRejected,
                            )?;
                        }
                    }
                }
                NativeEffectOperation::SetPointerPassthrough { binding, enabled } => {
                    let binding = *binding;
                    let enabled = *enabled;
                    let Some((viewport, window)) = self.exact_window_route_for_binding(binding)
                    else {
                        self.submit_failed_effect(
                            request,
                            NativeDispatchFailure::WindowUnavailable,
                        )?;
                        continue;
                    };
                    if !self.session.is_current_native_binding(binding) {
                        self.submit_failed_effect(
                            request,
                            NativeDispatchFailure::WindowUnavailable,
                        )?;
                        continue;
                    }
                    self.input_control
                        .retain(viewport, window, binding, enabled, request);
                }
                NativeEffectOperation::RetainChild { binding } => {
                    let binding = *binding;
                    if !self.session.is_current_native_binding(binding)
                        || self.exact_viewport_for_binding(binding).is_none()
                    {
                        self.submit_unsupported_effect(request)?;
                        continue;
                    }
                    if request.accepted().is_some() {
                        return Err(
                            NativeHostProtocolError::UnexpectedViewportEffectAcknowledgement.into(),
                        );
                    }
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

    pub(super) fn submit_unsupported_effect(
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

    pub(super) fn submit_failed_effect(
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

    pub(super) fn retain_or_return_effect_result(
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
                (NativeViewportEffectKind::Replacement, Some(predecessor)) => {
                    viewports.reserve_retired_replacement(viewport, predecessor, plan.binding())
                }
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
}
