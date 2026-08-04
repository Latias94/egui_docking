//! Independent cleanup for failed native recovery replacements.

use super::binding_retirement::{
    BindingRetirementCleanup, BindingRetirementOrigin, BindingRetirementRequest,
    BindingRetirementStatus,
};
use super::recovery_replacement::AbandonedReplacementCause;
use super::{ViewportCoordinator, ViewportCoordinatorError, recovery_replacement_error};
use crate::effect::{EffectId, EffectInvalidation, EffectPhase};
use crate::ids::SurfaceId;
use crate::platform_provider::PlatformObservationLease;
use crate::viewport::ViewportBinding;
use crate::viewport_registry::ViewportLifecycle;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum AbandonedReplacementRetirementMode {
    DriveImmediately,
    AwaitSuccessorObservation,
}

impl ViewportCoordinator {
    pub(super) fn effect_may_have_executed(&self, effect: EffectId) -> bool {
        self.effects.record(effect).is_some_and(|record| {
            record.was_emitted()
                && !matches!(
                    record.phase(),
                    EffectPhase::DispatchFailed(_)
                        | EffectPhase::Unsupported(_)
                        | EffectPhase::Invalidated { .. }
                )
        })
    }

    pub(super) fn retire_abandoned_recovery_replacement(
        &mut self,
        surface: SurfaceId,
        binding: ViewportBinding,
        mode: AbandonedReplacementRetirementMode,
    ) -> Result<Option<EffectId>, ViewportCoordinatorError> {
        let mut candidate = self.clone();
        let cleanup =
            candidate.retire_abandoned_recovery_replacement_in_place(surface, binding, mode)?;
        *self = candidate;
        Ok(cleanup)
    }

    pub(super) fn retire_recovery_compensation_for_provider_replacement(
        &mut self,
        surface: SurfaceId,
        binding: ViewportBinding,
        replacement: EffectId,
        cleanup: EffectId,
        provider: PlatformObservationLease,
    ) -> Result<(), ViewportCoordinatorError> {
        let cleanup_record = self.validated_compensating_close(cleanup, binding, replacement)?;
        let cleanup_was_emitted = cleanup_record.was_emitted();
        if !cleanup_was_emitted {
            let _ = self.effects.invalidate_unemitted(
                cleanup,
                EffectInvalidation::PlatformProviderReplaced { provider },
            );
        }
        let facts = self
            .registry
            .detach(binding)
            .map_err(ViewportCoordinatorError::Registry)?;
        let (owned_replacement, owned_cleanup) = self
            .recovery_replacements
            .transfer_compensation_for_provider_replacement(surface, binding)
            .map_err(recovery_replacement_error)?;
        if owned_replacement != replacement || owned_cleanup != cleanup {
            return Err(ViewportCoordinatorError::PendingRecoveryRegistrationMismatch { surface });
        }
        let observed = facts.ever_observed()
            && !matches!(
                facts.lifecycle(),
                ViewportLifecycle::Missing | ViewportLifecycle::Destroyed
            );
        let may_reappear = !matches!(facts.lifecycle(), ViewportLifecycle::Destroyed)
            && (facts.ever_observed() || self.effect_may_have_executed(replacement));
        let status = if cleanup_was_emitted {
            self.retirement_status_for_effect(cleanup)
        } else {
            BindingRetirementStatus::AwaitingAppearance
        };
        self.begin_binding_retirement(BindingRetirementRequest {
            binding,
            role: facts.role(),
            ownership: facts.ownership(),
            origin: BindingRetirementOrigin::RecoveryReplacementCompensationTransferred {
                replacement,
                cleanup,
            },
            status,
            observed,
            input_observations: facts.input_observations(),
            close_observations: facts.close_observations(),
            may_reappear,
            cleanup: BindingRetirementCleanup::CompensateCreate {
                create: replacement,
            },
            retained_staging_resource: None,
        })?;
        let _ = self.prepare_pointer_passthrough_for_vacancy(binding)?;
        Ok(())
    }

    pub(super) fn retire_abandoned_recovery_replacement_in_place(
        &mut self,
        surface: SurfaceId,
        binding: ViewportBinding,
        mode: AbandonedReplacementRetirementMode,
    ) -> Result<Option<EffectId>, ViewportCoordinatorError> {
        let detached = self
            .recovery_replacements
            .detach_abandoned_replacement(surface, binding)
            .map_err(recovery_replacement_error)?;
        let (replacement, cause, staging_close) = detached.into_parts();
        let origin = match cause {
            AbandonedReplacementCause::DispatchFailed { failed_effect } => {
                BindingRetirementOrigin::RecoveryReplacementFailed {
                    replacement,
                    failed: failed_effect,
                }
            }
            AbandonedReplacementCause::ProviderLost { last_effect } => {
                BindingRetirementOrigin::RecoveryReplacementProviderLost {
                    replacement,
                    last_effect,
                }
            }
        };
        let existing_cleanup =
            match staging_close.and_then(|transfer| transfer.into_active_cleanup()) {
                Some(effect) => {
                    let record = self.validated_compensating_close(effect, binding, replacement)?;
                    (!matches!(record.phase(), EffectPhase::Invalidated { .. })).then_some(effect)
                }
                None => None,
            };
        let status = match existing_cleanup {
            Some(effect) => self.retirement_status_for_effect(effect),
            None => BindingRetirementStatus::AwaitingInputRestore,
        };
        let facts = self
            .registry
            .detach(binding)
            .map_err(ViewportCoordinatorError::Registry)?;
        let observed = facts.ever_observed()
            && !matches!(
                facts.lifecycle(),
                ViewportLifecycle::Missing | ViewportLifecycle::Destroyed
            );
        let may_reappear = !matches!(facts.lifecycle(), ViewportLifecycle::Destroyed)
            && (facts.ever_observed() || self.effect_may_have_executed(replacement));
        self.begin_binding_retirement(BindingRetirementRequest {
            binding,
            role: facts.role(),
            ownership: facts.ownership(),
            origin,
            status,
            observed,
            input_observations: facts.input_observations(),
            close_observations: facts.close_observations(),
            may_reappear,
            cleanup: BindingRetirementCleanup::CompensateCreate {
                create: replacement,
            },
            retained_staging_resource: None,
        })?;
        let _ = self.prepare_pointer_passthrough_for_vacancy(binding)?;
        let cleanup = (mode == AbandonedReplacementRetirementMode::DriveImmediately
            && existing_cleanup.is_none())
        .then(|| self.drive_binding_retirement(binding))
        .transpose()?
        .flatten();
        Ok(cleanup)
    }
}
