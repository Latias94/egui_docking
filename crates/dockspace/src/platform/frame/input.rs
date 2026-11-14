//! Pointer passthrough and cross-window drag routing coordination.

use crate::platform::frame::*;

impl ViewportCoordinator {
    pub(super) fn reduce_pointer_passthrough_dispatch_result(
        &mut self,
        effect: EffectId,
        result: EffectDispatchResult,
    ) -> Result<(), ViewportCoordinatorError> {
        let Some(binding) =
            self.effects
                .record(effect)
                .and_then(|record| match record.request().effect() {
                    PlatformEffect::SetPointerPassthrough { binding, .. } => Some(*binding),
                    _ => None,
                })
        else {
            return Ok(());
        };
        let evidence = self.pointer_passthrough_evidence(binding);
        let matched = self
            .pointer_passthrough
            .record_dispatch_result(binding, effect, result, evidence);
        if matched {
            let _ = self.reconcile_pointer_passthrough_saga(binding)?;
        }
        Ok(())
    }

    pub(super) fn observe_pointer_passthrough_snapshot(
        &mut self,
        provider: PlatformObservationLease,
    ) {
        for binding in self.pointer_passthrough.tracked_bindings() {
            let Some(observation) = self.pointer_passthrough_evidence(binding).observation() else {
                continue;
            };
            let Some(effect) = self.pointer_passthrough.causal_effect(binding, observation) else {
                continue;
            };
            let transition = self.effects.mark_observed_applied(
                provider,
                effect,
                binding,
                self.registry.inventory_generation(),
            );
            let accepted = matches!(
                transition,
                EffectTransition::Applied | EffectTransition::Duplicate
            ) && self.effects.record(effect).is_some_and(|record| {
                matches!(record.phase(), EffectPhase::ObservedApplied { .. })
            });
            if accepted {
                self.pointer_passthrough
                    .accept_observed_effect(binding, effect);
            }
        }
    }

    pub(super) fn reconcile_pointer_passthrough_sagas(
        &mut self,
    ) -> Result<(), ViewportCoordinatorError> {
        for binding in self.pointer_passthrough.tracked_bindings() {
            let _ = self.reconcile_pointer_passthrough_saga(binding)?;
        }
        Ok(())
    }

    pub(super) fn reconcile_pointer_passthrough_saga(
        &mut self,
        binding: ViewportBinding,
    ) -> Result<Option<EffectId>, ViewportCoordinatorError> {
        let evidence = self.pointer_passthrough_evidence(binding);
        let attempts = self.pointer_passthrough.effect_attempts(binding);
        let enable_phase = attempts.enable().and_then(|effect| {
            self.effects
                .record(effect)
                .map(crate::platform::effect::EffectRecord::phase)
        });
        let restore_phase = attempts.restore().and_then(|effect| {
            self.effects
                .record(effect)
                .map(crate::platform::effect::EffectRecord::phase)
        });
        let request = self
            .pointer_passthrough
            .reconcile(binding, evidence, enable_phase, restore_phase)
            .map_err(pointer_passthrough_error)?;
        request
            .map(|request| self.apply_pointer_passthrough_request_in(self.workspace_epoch, request))
            .transpose()
    }

    pub(super) fn request_pointer_input_restore_in(
        &mut self,
        issuance_epoch: WorkspaceEpoch,
        binding: ViewportBinding,
    ) -> Result<EffectId, ViewportCoordinatorError> {
        let evidence = self.pointer_passthrough_evidence(binding);
        let request = self
            .pointer_passthrough
            .restore_request(binding, evidence)
            .map_err(pointer_passthrough_error)?;
        self.apply_pointer_passthrough_request_in(issuance_epoch, request)
    }

    pub(super) fn apply_pointer_passthrough_request_in(
        &mut self,
        issuance_epoch: WorkspaceEpoch,
        request: PointerPassthroughEffectRequest,
    ) -> Result<EffectId, ViewportCoordinatorError> {
        let binding = request.binding();
        let after = self.pointer_input_lane_predecessor(request.lane_tail());
        let effect = self
            .effects
            .request_in(
                issuance_epoch,
                PlatformEffect::SetPointerPassthrough {
                    binding,
                    enabled: request.enabled(),
                    after,
                },
            )
            .map_err(ViewportCoordinatorError::Effect)?;
        self.pointer_passthrough
            .accept_effect_request(request, effect)
            .map_err(pointer_passthrough_error)?;
        Ok(effect)
    }

    pub(super) fn pointer_input_lane_predecessor(
        &self,
        mut predecessor: Option<EffectId>,
    ) -> Option<EffectId> {
        while let Some(effect) = predecessor {
            let record = self.effects.record(effect)?;
            let invalidated_unemitted =
                !record.was_emitted() && matches!(record.phase(), EffectPhase::Invalidated { .. });
            if !invalidated_unemitted {
                return Some(effect);
            }
            predecessor = match record.request().effect() {
                PlatformEffect::SetPointerPassthrough { after, .. } => *after,
                _ => None,
            };
        }
        None
    }

    pub(super) fn pointer_passthrough_evidence(
        &self,
        binding: ViewportBinding,
    ) -> PointerPassthroughEvidence {
        let (observation, generation_watermark, window_observed, routeable) =
            self.pointer_input_facts(binding);
        PointerPassthroughEvidence::new(
            observation,
            generation_watermark,
            window_observed,
            routeable,
            self.capabilities.pointer_hit_test_observation(),
            self.capabilities.pointer_hit_test_control(),
        )
    }

    pub(super) fn pointer_input_facts(
        &self,
        binding: ViewportBinding,
    ) -> (
        Option<WindowInputObservation>,
        Option<InputObservationGeneration>,
        bool,
        bool,
    ) {
        if let Some(record) = self.registry.window(binding.window()) {
            return (
                record.input_observation(),
                record.input_observation_generation_watermark(),
                record.is_observed(),
                record.is_routeable(),
            );
        }
        self.binding_cleanup
            .get(&binding)
            .map_or((None, None, false, false), |retired| {
                (
                    retired.input_observations().current(),
                    retired.input_observations().generation_watermark(),
                    retired.observed(),
                    false,
                )
            })
    }

    pub(super) fn terminate_pointer_passthrough_binding(&mut self, binding: ViewportBinding) {
        self.pointer_passthrough.terminate_binding(binding);
        let effects: Vec<EffectId> = self
            .effects
            .records()
            .filter_map(|(effect, record)| {
                matches!(
                    record.request().effect(),
                    PlatformEffect::SetPointerPassthrough { binding: target, .. }
                        if *target == binding
                )
                .then_some(effect)
            })
            .collect();
        for effect in effects {
            let _ =
                self.effects
                    .mark_destroyed(effect, binding, self.registry.inventory_generation());
        }
    }

    pub(super) fn prepare_pointer_passthrough_for_vacancy(
        &mut self,
        binding: ViewportBinding,
    ) -> Result<bool, ViewportCoordinatorError> {
        self.pointer_passthrough
            .prepare_binding_for_vacancy(binding);
        let _ = self.reconcile_pointer_passthrough_saga(binding)?;
        Ok(!self.pointer_passthrough.contains_binding(binding))
    }
}

impl ViewportCoordinator {
    pub(crate) fn begin_drag_routing(
        &mut self,
        pointer: PointerId,
        surface: SurfaceId,
        input: NativeDragSourceInput,
    ) -> Result<Option<EffectId>, ViewportCoordinatorError> {
        let Some(record) = self.registry.record(surface) else {
            return Ok(None);
        };
        let binding = record.binding();
        let mut candidate = self.clone();
        let Some(change) = candidate
            .pointer_passthrough
            .begin_routing(pointer, binding, input)
            .map_err(pointer_passthrough_error)?
        else {
            return Ok(None);
        };
        let restored = change
            .previous()
            .map(|previous| candidate.reconcile_pointer_passthrough_saga(previous))
            .transpose()?
            .flatten();
        let effect = candidate.reconcile_pointer_passthrough_saga(change.current())?;
        *self = candidate;
        Ok(effect.or(restored))
    }

    #[must_use]
    pub(crate) fn drag_source(&self, pointer: PointerId) -> Option<ViewportBinding> {
        self.pointer_passthrough.drag_source(pointer)
    }

    #[must_use]
    pub(crate) fn confirmed_drag_source(&self, pointer: PointerId) -> Option<ViewportBinding> {
        let binding = self.pointer_passthrough.drag_source(pointer)?;
        let evidence = self.pointer_passthrough_evidence(binding);
        if self.pointer_passthrough.drag_source_input(pointer)
            == Some(NativeDragSourceInput::Preserve)
        {
            return (!self.pointer_passthrough.contains_binding(binding)
                && self
                    .registry
                    .record(binding.surface())
                    .is_some_and(|record| record.binding() == binding && record.is_routeable())
                && evidence.authoritative_state() == Some(WindowInputState::ReceivesInput))
            .then_some(binding);
        }
        if evidence.authoritative_state() != Some(WindowInputState::PassThrough) {
            return None;
        }
        let attempts = self.pointer_passthrough.effect_attempts(binding);
        if attempts.restore().is_some() {
            return None;
        }
        attempts
            .enable()
            .is_some_and(|effect| {
                self.effects.record(effect).is_some_and(|record| {
                    matches!(record.phase(), EffectPhase::ObservedApplied { .. })
                })
            })
            .then_some(binding)
    }

    #[must_use]
    pub(crate) fn drag_route_effect(&self, pointer: PointerId) -> Option<EffectId> {
        if self.pointer_passthrough.drag_source_input(pointer)
            != Some(NativeDragSourceInput::PassThrough)
        {
            return None;
        }
        let binding = self.pointer_passthrough.drag_source(pointer)?;
        self.pointer_passthrough.effect_attempts(binding).enable()
    }

    pub(crate) fn drag_source_input(&self, pointer: PointerId) -> Option<NativeDragSourceInput> {
        self.pointer_passthrough.drag_source_input(pointer)
    }

    pub(crate) fn end_drag_routing(
        &mut self,
        pointer: PointerId,
    ) -> Result<Option<EffectId>, ViewportCoordinatorError> {
        if self.pointer_passthrough.drag_source(pointer).is_none() {
            return Ok(None);
        }
        let mut candidate = self.clone();
        let effect = candidate.end_drag_routing_in_place(pointer)?;
        *self = candidate;
        Ok(effect)
    }

    pub(crate) fn end_all_drag_routing(&mut self) -> Result<(), ViewportCoordinatorError> {
        let mut candidate = self.clone();
        let _ = candidate.end_all_drag_routing_in_place()?;
        *self = candidate;
        Ok(())
    }

    pub(super) fn end_all_drag_routing_in_place(
        &mut self,
    ) -> Result<bool, ViewportCoordinatorError> {
        let pointers = self.pointer_passthrough.active_pointers();
        for pointer in &pointers {
            let _ = self.end_drag_routing_in_place(*pointer)?;
        }
        Ok(!pointers.is_empty())
    }

    pub(super) fn end_drag_routing_in_place(
        &mut self,
        pointer: PointerId,
    ) -> Result<Option<EffectId>, ViewportCoordinatorError> {
        let Some(binding) = self
            .pointer_passthrough
            .end_routing(pointer)
            .map_err(pointer_passthrough_error)?
        else {
            return Ok(None);
        };
        let effect = self.reconcile_pointer_passthrough_saga(binding)?;
        Ok(effect)
    }
}
