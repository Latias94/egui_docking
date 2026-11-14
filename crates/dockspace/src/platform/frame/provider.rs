//! Platform provider enrollment, replacement and authority checks.

use crate::platform::frame::*;

impl ViewportCoordinator {
    /// Enrolls the sole platform observation provider for this coordinator.
    pub(crate) fn create_platform_provider(
        &mut self,
    ) -> Result<PlatformObservationLease, ViewportCoordinatorError> {
        let mut candidate = self.clone();
        let provider = candidate
            .platform_provider
            .create()
            .map_err(ViewportCoordinatorError::PlatformProvider)?;
        candidate.reconcile_after_provider_enrollment()?;
        *self = candidate;
        Ok(provider)
    }

    /// Returns the current platform observation provider, when enrolled.
    pub(crate) const fn platform_provider(&self) -> Option<PlatformObservationLease> {
        self.platform_provider.active()
    }

    /// Returns the monotonic platform-provider authority frontier.
    pub(crate) const fn platform_provider_frontier(&self) -> PlatformProviderAuthorityFrontier {
        self.platform_provider.frontier()
    }

    pub(super) fn active_platform_provider(
        &self,
    ) -> Result<PlatformObservationLease, ViewportCoordinatorError> {
        self.platform_provider
            .active()
            .ok_or(ViewportCoordinatorError::PlatformProviderUnavailable)
    }

    #[cfg(test)]
    pub(super) fn revoke_recovery_bringups_for_provider_replacement(
        &mut self,
        provider: PlatformObservationLease,
    ) -> Result<(), ViewportCoordinatorError> {
        for (_, binding, phase) in self.recovery_replacements.active_bringups() {
            let replacement = phase.create_effect();
            let replacement_may_have_executed = self.effect_may_have_executed(replacement);
            let invalidation = EffectInvalidation::PlatformProviderReplaced { provider };
            let _ = self.effects.invalidate_unemitted(replacement, invalidation);
            let binding_was_discarded =
                !replacement_may_have_executed && self.registry.discard_unobserved(binding).is_ok();
            let surface = self
                .recovery_replacements
                .mark_provider_lost(binding, phase, binding_was_discarded)
                .map_err(recovery_replacement_error)?;
            if !binding_was_discarded {
                let _ = self.retire_abandoned_recovery_replacement_in_place(
                    surface,
                    binding,
                    AbandonedReplacementRetirementMode::AwaitSuccessorObservation,
                )?;
            }
        }
        let compensations = self
            .recovery_replacements
            .values()
            .filter_map(|pending| {
                let RecoveryPendingStatus::AwaitingReplacementCleanup { replacement } =
                    pending.status()
                else {
                    return None;
                };
                Some((
                    pending.source_binding().surface(),
                    pending.replacement_binding()?,
                    replacement,
                ))
            })
            .collect::<Vec<_>>();
        for (surface, binding, replacement) in compensations {
            self.retire_recovery_cleanup_for_provider_replacement(
                surface,
                binding,
                replacement,
                provider,
            )?;
        }
        Ok(())
    }

    /// Requires the exact currently enrolled platform provider.
    pub(crate) fn require_platform_provider(
        &self,
        provider: PlatformObservationLease,
    ) -> Result<(), PlatformObservationAuthorityError> {
        self.platform_provider.require_active(provider)
    }

    /// Returns the core-owned platform handoff which is awaiting dispatch
    /// quiescence, when one exists.
    #[cfg(test)]
    pub(crate) const fn pending_platform_provider_replacement(
        &self,
    ) -> Option<PlatformProviderReservation> {
        self.platform_provider.pending_replacement()
    }

    /// Revokes one provider and freezes a typed handoff ticket for its successor.
    #[cfg(test)]
    pub(crate) fn begin_platform_provider_replacement(
        &mut self,
        provider: PlatformObservationLease,
    ) -> Result<PlatformProviderReservation, ViewportCoordinatorError> {
        let mut candidate = self.clone();
        candidate
            .platform_provider
            .require_active(provider)
            .map_err(ViewportCoordinatorError::PlatformProvider)?;

        candidate.end_all_drag_routing_in_place()?;
        let create_sagas = candidate.abortable_native_create_sagas();
        for saga in create_sagas {
            let _ = candidate.abort_native_create(saga)?;
        }
        candidate.revoke_recovery_bringups_for_provider_replacement(provider)?;

        let _ = candidate
            .effects
            .invalidate_unemitted_causal_successors_for_provider_replacement(provider);
        let provider_fact_requests = candidate
            .effects
            .records()
            .filter_map(|(effect, record)| {
                (!record.was_emitted()
                    && matches!(record.phase(), EffectPhase::Requested)
                    && matches!(
                        record.request().effect(),
                        PlatformEffect::SetPointerPassthrough { .. }
                            | PlatformEffect::RequestFocus { .. }
                            | PlatformEffect::ResolveNativeClose { .. }
                    ))
                .then_some(effect)
            })
            .collect::<Vec<_>>();
        for effect in provider_fact_requests {
            let _ = candidate.effects.invalidate_unemitted(
                effect,
                EffectInvalidation::PlatformProviderReplaced { provider },
            );
        }

        candidate
            .pointer_passthrough
            .recover_for_provider_replacement();
        candidate.focus_effect_lane_tail = None;
        candidate.effects.revoke_provider_authority(provider);

        candidate
            .capability_observations
            .reset_for_provider_replacement();
        if candidate.capabilities != PlatformCapabilities::default() {
            candidate.capability_generation = candidate
                .capability_generation
                .checked_next()
                .ok_or(ViewportCoordinatorError::CapabilityGenerationExhausted)?;
            candidate.capabilities = PlatformCapabilities::default();
        }
        candidate
            .inventory_observations
            .reset_for_provider_replacement();
        candidate.last_platform_snapshot = None;
        candidate
            .work_area_observations
            .reset_for_provider_replacement();
        if !candidate.work_areas.is_empty() {
            candidate.work_area_generation = candidate
                .work_area_generation
                .checked_next()
                .ok_or(ViewportCoordinatorError::WorkAreaGenerationExhausted)?;
            candidate.work_areas.clear();
        }
        candidate
            .registry
            .revoke_live_provider_authority()
            .map_err(ViewportCoordinatorError::Registry)?;
        candidate.binding_cleanup.reset_for_provider_replacement();
        let ticket = candidate
            .platform_provider
            .begin_replacement(provider)
            .map_err(ViewportCoordinatorError::PlatformProvider)?;
        candidate.provider_reconciliation_pending = true;
        *self = candidate;
        Ok(ticket)
    }

    /// Activates the exact successor reserved by a completed provider handoff.
    #[cfg(test)]
    pub(crate) fn finish_platform_provider_replacement(
        &mut self,
        ticket: PlatformProviderReservation,
    ) -> Result<PlatformObservationLease, ViewportCoordinatorError> {
        let mut candidate = self.clone();
        let provider = candidate
            .platform_provider
            .finish_replacement(ticket)
            .map_err(ViewportCoordinatorError::PlatformProvider)?;
        candidate.reconcile_after_provider_enrollment()?;
        *self = candidate;
        Ok(provider)
    }

    /// Abandons the reserved successor while preserving fail-closed platform state.
    #[cfg(test)]
    pub(crate) fn abort_platform_provider_replacement(
        &mut self,
        ticket: PlatformProviderReservation,
    ) -> Result<(), ViewportCoordinatorError> {
        let mut candidate = self.clone();
        candidate
            .platform_provider
            .abort_replacement(ticket)
            .map_err(ViewportCoordinatorError::PlatformProvider)?;
        *self = candidate;
        Ok(())
    }
}
