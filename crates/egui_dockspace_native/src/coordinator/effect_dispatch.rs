//! Native effect dispatch, acknowledgement retention, and retirement handoff.

use super::*;

pub(super) enum ViewportFailureOrigin {
    CapturedCallback,
    RetirementDispatch,
}

impl NativeCoordinator {
    fn resolve_exact_close(
        &self,
        context: &eframe::egui::Context,
        close: dockspace::runtime::NativeSurfaceCloseRequest,
        decision: eframe::NativeViewportCloseDecision,
    ) -> Option<ViewportId> {
        let request = self
            .bridge
            .with_close_control(|control| control.held_request(close))?;
        let route = self.exact_window_route_for_binding(close.binding())?;
        if route != (request.viewport_id(), request.window_id()) {
            return None;
        }
        let veto = decision == eframe::NativeViewportCloseDecision::Veto;
        if veto && !self.bridge.reserve_close_callback(close.binding(), request) {
            return None;
        }
        if eframe::resolve_native_viewport_close(context, request, decision) {
            Some(route.0)
        } else {
            if veto {
                self.bridge
                    .cancel_close_callback_reservation(close.binding(), request);
            }
            None
        }
    }

    fn dispatch_close_cancellation(
        &mut self,
        context: &eframe::egui::Context,
        request: NativeEffectRequest,
        close: dockspace::runtime::NativeSurfaceCloseRequest,
    ) -> Result<(), NativeRuntimeError> {
        let dispatch = request.begin_dispatch();
        let Some(viewport) =
            self.resolve_exact_close(context, close, eframe::NativeViewportCloseDecision::Veto)
        else {
            return self.report_derived_effect_result(
                close.binding(),
                dispatch.not_applied(NativeDispatchFailure::AdapterRejected),
            );
        };
        let Some(NativeEffectAcknowledgement::Close(acknowledgement)) = dispatch.accepted() else {
            return Err(NativeHostProtocolError::UnexpectedViewportEffectAcknowledgement.into());
        };
        if self
            .bridge
            .with_close_control(|control| control.begin_cancellation(close, acknowledgement))
            != Some(viewport)
        {
            return Err(NativeHostProtocolError::NativeCloseCorrelationChanged.into());
        }
        Ok(())
    }

    fn exact_viewport_for_binding(&self, binding: NativeSurfaceBinding) -> Option<ViewportId> {
        self.bridge
            .with_viewports(|viewports| viewports.viewport_for_window(binding.window()))
    }

    pub(super) fn exact_window_route_for_binding(
        &self,
        binding: NativeSurfaceBinding,
    ) -> Option<(ViewportId, WindowId)> {
        self.bridge.with_viewports(|viewports| {
            let viewport = viewports.viewport_for_window(binding.window())?;
            Some((viewport, viewports.window(viewport)?))
        })
    }

    pub(super) fn current_window_route_for_binding(
        &self,
        binding: NativeSurfaceBinding,
    ) -> Option<(ViewportId, WindowId)> {
        if !self
            .session
            .native_window_bindings()
            .any(|window| window == binding.window())
        {
            return None;
        }
        self.exact_window_route_for_binding(binding)
    }

    pub(super) fn fail_pending_viewport_effect(
        &mut self,
        viewport: ViewportId,
        binding: NativeSurfaceBinding,
        origin: ViewportFailureOrigin,
    ) -> Result<(), NativeRuntimeError> {
        let failure = NativeViewportCreateFailureRecord::new(
            viewport,
            binding,
            NativeViewportCreateFailureKind::WindowUnavailable,
        );
        let Some(result) = self.effects.take_failure_result(failure) else {
            return Ok(());
        };
        if matches!(origin, ViewportFailureOrigin::RetirementDispatch) {
            return self.report_derived_effect_result(binding, result);
        }
        if let Err(error) = self.session.report_native_effect_result(result) {
            return self.retain_or_return_effect_result(binding, error);
        }
        Ok(())
    }

    pub(super) fn retire_deferred_sidecars(
        &mut self,
        viewport: ViewportId,
        binding: NativeSurfaceBinding,
    ) {
        self.bridge.with_viewports_mut(|viewports| {
            if let Some(current) = viewports.binding(viewport)
                && current.window() == binding.window()
            {
                viewports.suppress_pointer_route(viewport, current);
            }
        });
        self.deferred_viewports.remove(binding);
        if let Some(pending) = self
            .focus_control
            .pending_binding()
            .filter(|pending| pending.window() == binding.window())
        {
            self.terminalize_focus(pending);
        }
        self.retain_retired_input_results(binding);
        self.receivers.retire_window(binding.window());
        self.bridge
            .retire_deferred_binding_paints(viewport, binding);
    }

    pub(super) fn accept_retirement_effect(
        &mut self,
        request: NativeEffectRequest,
        binding: NativeSurfaceBinding,
    ) -> Result<(), NativeRuntimeError> {
        let Some(viewport) = self.exact_viewport_for_binding(binding) else {
            if self.session.is_current_native_binding(binding) {
                return self.submit_unsupported_effect(request);
            }
            return Err(
                NativeHostProtocolError::RetiredViewportRouteChanged(binding.surface()).into(),
            );
        };
        if !self
            .session
            .recognizes_native_window_binding(binding.window())
            || !self.bridge.retirement_can_begin_release(viewport, binding)
        {
            return self.submit_unsupported_effect(request);
        }
        self.fail_pending_viewport_effect(
            viewport,
            binding,
            ViewportFailureOrigin::RetirementDispatch,
        )?;
        let dispatch = request.begin_dispatch();
        self.retire_deferred_sidecars(viewport, binding);
        let Some(NativeEffectAcknowledgement::Close(acknowledgement)) = dispatch.accepted() else {
            return Err(NativeHostProtocolError::UnexpectedViewportEffectAcknowledgement.into());
        };
        let inserted = self
            .bridge
            .retirement_begin_release(viewport, binding, acknowledgement);
        assert!(inserted, "preflighted native retirement must insert");
        Ok(())
    }

    /// Accepts the minimal platform effects implemented by this vertical slice.
    ///
    /// New and replacement child windows retain their affine request until
    /// eframe reports the first exact callback or a typed create failure.
    /// `ReleaseChild` stops re-declaring the child but retains its route until
    /// an exact destruction callback commits.
    /// Unrelated platform operations remain explicitly unsupported.
    pub(crate) fn accept_native_effects(
        &mut self,
        context: &eframe::egui::Context,
        requests: Vec<NativeEffectRequest>,
    ) -> Result<(), NativeRuntimeError> {
        for request in requests {
            if !self.bridge.accepts_new_work()
                && matches!(
                    request.operation(),
                    NativeEffectOperation::CreateWindow { .. }
                        | NativeEffectOperation::RequestReplacement { .. }
                        | NativeEffectOperation::RequestFocus { .. }
                        | NativeEffectOperation::SetPointerPassthrough { enabled: true, .. }
                )
            {
                self.submit_failed_effect(request, NativeDispatchFailure::ProviderStopped)?;
                continue;
            }
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
                            let predecessor_viewport = self
                                .session
                                .native_replacement_predecessor(binding)
                                .and_then(|predecessor| {
                                    self.bridge.with_viewports(|viewports| {
                                        viewports.viewport_for_window(predecessor.window())
                                    })
                                });
                            let Some(viewport) = predecessor_viewport else {
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
                                .bridge
                                .retirement_can_retire_committed_route_for_replacement(
                                    viewport,
                                    predecessor,
                                )
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
                    let declared = self.effects.dispatch_viewport(plan, || {
                        let inserted = self
                            .deferred_viewports
                            .insert(viewport, binding, placement, role);
                        assert!(inserted, "preflighted deferred viewport must insert");
                        if let Some(predecessor) = predecessor {
                            assert!(
                                self.bridge
                                    .retirement_retire_committed_route_for_replacement(
                                        viewport,
                                        predecessor
                                    ),
                                "preflighted predecessor route must retire through replacement"
                            );
                            self.receivers.retire_binding(predecessor);
                        }
                    });
                    assert!(declared, "the admitted viewport retains its exact effect");
                }
                NativeEffectOperation::ReleaseChild { binding }
                | NativeEffectOperation::CompensatingClose { binding } => {
                    let binding = *binding;
                    self.accept_retirement_effect(request, binding)?;
                }
                NativeEffectOperation::ResolveNativeClose { close, resolution } => {
                    let close = *close;
                    match resolution {
                        NativeCloseDisposition::Accept => {
                            let can_retire = self
                                .exact_viewport_for_binding(close.binding())
                                .is_some_and(|viewport| {
                                    self.session
                                        .recognizes_native_window_binding(close.binding().window())
                                        && self
                                            .bridge
                                            .retirement_can_begin_release(viewport, close.binding())
                                });
                            if !can_retire {
                                self.submit_failed_effect(
                                    request,
                                    NativeDispatchFailure::AdapterRejected,
                                )?;
                                continue;
                            }
                            let dispatch = request.begin_dispatch();
                            if self
                                .resolve_exact_close(
                                    context,
                                    close,
                                    eframe::NativeViewportCloseDecision::Allow,
                                )
                                .is_none()
                            {
                                self.report_derived_effect_result(
                                    close.binding(),
                                    dispatch.not_applied(NativeDispatchFailure::AdapterRejected),
                                )?;
                                continue;
                            }
                            self.accept_retirement_effect(
                                dispatch.await_observation(),
                                close.binding(),
                            )?;
                            if !self
                                .bridge
                                .with_close_control(|control| control.accept_close(close))
                            {
                                return Err(
                                    NativeHostProtocolError::NativeCloseCorrelationChanged.into()
                                );
                            }
                        }
                        NativeCloseDisposition::Cancel => {
                            self.dispatch_close_cancellation(context, request, close)?;
                        }
                    }
                }
                NativeEffectOperation::CancelRootClose { binding } => {
                    let binding = *binding;
                    let Some(close) = self
                        .bridge
                        .with_close_control(|control| control.request_for_binding(binding))
                    else {
                        self.submit_failed_effect(request, NativeDispatchFailure::AdapterRejected)?;
                        continue;
                    };
                    self.dispatch_close_cancellation(context, request, close)?;
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
                    if let Err(request) = self
                        .focus_control
                        .retain(viewport, window, binding, request)
                    {
                        self.submit_failed_effect(request, NativeDispatchFailure::AdapterRejected)?;
                    }
                }
                NativeEffectOperation::SetPointerPassthrough { binding, enabled } => {
                    let binding = *binding;
                    let enabled = *enabled;
                    let Some((viewport, window)) = self.current_window_route_for_binding(binding)
                    else {
                        self.submit_failed_effect(
                            request,
                            NativeDispatchFailure::WindowUnavailable,
                        )?;
                        continue;
                    };
                    if let Err(request) = self
                        .input_control
                        .retain(viewport, window, binding, enabled, request)
                    {
                        self.submit_failed_effect(request, NativeDispatchFailure::AdapterRejected)?;
                    }
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
                        || !self
                            .bridge
                            .retirement_can_accept_cleanup_observation(binding)
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
                        .bridge
                        .retirement_accept_cleanup_observation(binding, observation)
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
        self.report_derived_effect_result(binding, result)
    }

    pub(super) fn submit_failed_effect(
        &mut self,
        request: NativeEffectRequest,
        reason: NativeDispatchFailure,
    ) -> Result<(), NativeRuntimeError> {
        let binding = request.operation().binding();
        let result = request.dispatch_failed(reason);
        self.report_derived_effect_result(binding, result)
    }

    pub(super) fn report_derived_effect_result(
        &mut self,
        binding: NativeSurfaceBinding,
        result: NativeEffectResult,
    ) -> Result<(), NativeRuntimeError> {
        self.bridge.admission_dispatch();
        if !self.bridge.permits_unscheduled_input() {
            self.queue_pending_effect_result(result, None);
            return Ok(());
        }
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
        self.report_derived_effect_result(binding, result)
    }

    pub(super) fn retain_or_return_effect_result(
        &mut self,
        binding: NativeSurfaceBinding,
        error: NativeEffectSubmissionError,
    ) -> Result<(), NativeRuntimeError> {
        let (kind, result) = error.into_parts();
        if kind == NativeHostErrorKind::StaleBinding
            && !self.session.recognizes_native_binding(result.binding())
        {
            // A superseded core lifetime cannot accept a later cleanup relay.
            return Ok(());
        }
        if !result.binding().same_window_lifetime(binding)
            || !result.can_be_correlated_by_cleanup()
            || !matches!(kind, NativeHostErrorKind::StaleBinding)
        {
            return self.retain_rejected_effect_result(kind, result);
        }
        if self.bridge.retirement_cleanup_is_terminal(binding) {
            drop(result);
            return Ok(());
        }
        if !self.bridge.retirement_can_accept_cleanup_result(binding) {
            return self.retain_rejected_effect_result(kind, result);
        }
        match self
            .bridge
            .retirement_retain_cleanup_result(binding, result)
        {
            Ok(Some(correlated)) => self.report_cleanup_result(binding, correlated),
            Ok(None) => Ok(()),
            Err(CleanupResultRetentionError::CorrelationMismatch) => {
                Err(NativeHostProtocolError::CleanupRelayConflict.into())
            }
            Err(CleanupResultRetentionError::Occupied(result)) => {
                self.retain_rejected_effect_result(kind, result)
            }
        }
    }

    pub(super) fn retain_rejected_effect_result(
        &mut self,
        kind: NativeHostErrorKind,
        result: NativeEffectResult,
    ) -> Result<(), NativeRuntimeError> {
        if kind == NativeHostErrorKind::StaleBinding
            && !self.session.recognizes_native_binding(result.binding())
        {
            // Core has already retired this exact lifetime. The callback-side
            // owner was consumed before submission, so only this stale result
            // retires; a successor binding for the same surface is untouched.
            drop(result);
            return Ok(());
        }
        self.queue_pending_effect_result(result, Some(kind));
        if native_effect_submission_rejection_is_recoverable(kind) {
            Ok(())
        } else {
            Err(NativeHostProtocolError::NativeEffectResultRejected(kind).into())
        }
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
        let predecessor = self
            .bridge
            .with_viewports(|viewports| viewports.binding(viewport));
        if matches!(plan.kind(), NativeViewportEffectKind::Replacement) && predecessor.is_none() {
            return Err(request);
        }
        let Some(requested_rect) = native_create_rect(plan.placement()) else {
            return Err(request);
        };
        if !self
            .bridge
            .reserve_create(viewport, plan.binding(), requested_rect)
        {
            return Err(request);
        }
        if let Err(request) = self.effects.insert(plan, request) {
            let cleared = self.bridge.clear_create(viewport, plan.binding());
            debug_assert!(cleared, "the rejected create retains its reservation");
            self.bridge.clear_hidden_render(viewport, plan.binding());
            return Err(request);
        }

        let reserve_result = match (plan.kind(), predecessor) {
            (NativeViewportEffectKind::Create, None) => {
                self.bridge.reserve_viewport_route(viewport, plan.binding())
            }
            (NativeViewportEffectKind::Replacement, Some(predecessor)) => self
                .bridge
                .reserve_retired_viewport_replacement(viewport, predecessor, plan.binding()),
            (NativeViewportEffectKind::Create, Some(_)) => {
                Err(NativeViewportBindingError::ViewportAlreadyBound {
                    viewport,
                    existing: self
                        .bridge
                        .with_viewports(|viewports| viewports.binding(viewport))
                        .expect("matched viewport binding remains present")
                        .surface(),
                })
            }
            (NativeViewportEffectKind::Replacement, None) => {
                unreachable!("replacement predecessor was checked before retention")
            }
        };
        if reserve_result.is_err() {
            let cleared = self.bridge.clear_create(viewport, plan.binding());
            debug_assert!(cleared, "the rejected create retains its reservation");
            self.bridge.clear_hidden_render(viewport, plan.binding());
            return Err(self
                .effects
                .remove_unstarted(plan)
                .expect("failed viewport reservation retains its affine request"));
        }
        Ok(plan)
    }
}
