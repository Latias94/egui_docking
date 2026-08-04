//! Pre-admission native-close ownership for recovery replacements.

use crate::effect::EffectId;
use crate::ids::SurfaceId;
use crate::platform::{WindowCloseObservation, WindowCloseState, WindowPresentationObservation};
use crate::surface_recovery::SurfaceRecoveryObligationId;
use crate::viewport::{InventoryGeneration, ViewportBinding};
use crate::viewport_registry::ViewportAdmission;

use super::{RecoveryReplacementLifecycle, RecoveryReplacementLifecycleError};

/// Core-private owner of a native close which arrived before replacement admission.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::frame) enum StagingCloseAbortOwner {
    RecoveryReplacement { surface: SurfaceId },
}

/// One internal compensation lane for a staging replacement close.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::frame) struct StagingCloseAbort {
    owner: StagingCloseAbortOwner,
    cleanup: Option<EffectId>,
    requested: WindowCloseObservation,
    pre_close_admission: ViewportAdmission,
    phase: StagingCloseAbortPhase,
}

impl StagingCloseAbort {
    pub(in crate::frame) const fn owner(&self) -> StagingCloseAbortOwner {
        self.owner
    }

    pub(in crate::frame) const fn cleanup(&self) -> Option<EffectId> {
        self.cleanup
    }

    pub(in crate::frame) const fn requested(&self) -> WindowCloseObservation {
        self.requested
    }

    pub(super) fn into_retirement_transfer(self) -> StagingCloseRetirementTransfer {
        let active_cleanup = matches!(self.phase, StagingCloseAbortPhase::Retiring { .. })
            .then_some(self.cleanup)
            .flatten();
        StagingCloseRetirementTransfer { active_cleanup }
    }
}

/// Affine transfer of a pre-admission close lane into exact binding retirement.
#[derive(Debug, PartialEq, Eq)]
pub(in crate::frame) struct StagingCloseRetirementTransfer {
    active_cleanup: Option<EffectId>,
}

impl StagingCloseRetirementTransfer {
    pub(in crate::frame) const fn into_active_cleanup(self) -> Option<EffectId> {
        self.active_cleanup
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StagingCloseAbortPhase {
    Retiring {
        cleared: Option<WindowCloseObservation>,
    },
    ResumedPending {
        cleared_inventory_generation: InventoryGeneration,
    },
}

/// Typed request which starts one replacement-owned staging close lane.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::frame) struct StagingCloseAbortRequest {
    pub(in crate::frame) binding: ViewportBinding,
    pub(in crate::frame) owner: StagingCloseAbortOwner,
    pub(in crate::frame) cleanup: Option<EffectId>,
    pub(in crate::frame) requested: WindowCloseObservation,
    pub(in crate::frame) pre_close_admission: ViewportAdmission,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::frame) struct StagingCloseResume {
    pub(in crate::frame) pre_close_admission: ViewportAdmission,
    pub(in crate::frame) cleanup: Option<EffectId>,
    pub(in crate::frame) clear: WindowCloseObservation,
}

impl RecoveryReplacementLifecycle {
    pub(in crate::frame) fn take_staging_close_for_retirement(
        &mut self,
        surface: SurfaceId,
        binding: ViewportBinding,
    ) -> Result<Option<StagingCloseRetirementTransfer>, RecoveryReplacementLifecycleError> {
        let Some(abort) = self.staging_close_aborts.get(&binding) else {
            return Ok(None);
        };
        if abort.owner != (StagingCloseAbortOwner::RecoveryReplacement { surface }) {
            return Err(RecoveryReplacementLifecycleError::StagingAbortOwnerMismatch { binding });
        }
        Ok(self
            .staging_close_aborts
            .remove(&binding)
            .map(StagingCloseAbort::into_retirement_transfer))
    }

    pub(in crate::frame) fn first_staging_close_binding(&self) -> Option<ViewportBinding> {
        self.staging_close_aborts.keys().next().copied()
    }

    pub(in crate::frame) fn begin_staging_close(
        &mut self,
        request: StagingCloseAbortRequest,
    ) -> Result<(), RecoveryReplacementLifecycleError> {
        let surface = match request.owner {
            StagingCloseAbortOwner::RecoveryReplacement { surface } => surface,
        };
        if request.requested.binding() != request.binding
            || request.requested.known_state() != Some(WindowCloseState::LiveRequested)
            || request.pre_close_admission != ViewportAdmission::Pending
        {
            return Err(
                RecoveryReplacementLifecycleError::StagingAbortOwnerMismatch {
                    binding: request.binding,
                },
            );
        }
        self.ensure_replacement(surface, request.binding)?;
        if self.staging_close_aborts.contains_key(&request.binding) {
            return Err(
                RecoveryReplacementLifecycleError::StagingAbortAlreadyExists {
                    binding: request.binding,
                },
            );
        }
        self.staging_close_aborts.insert(
            request.binding,
            StagingCloseAbort {
                owner: request.owner,
                cleanup: request.cleanup,
                requested: request.requested,
                pre_close_admission: request.pre_close_admission,
                phase: StagingCloseAbortPhase::Retiring { cleared: None },
            },
        );
        Ok(())
    }

    pub(in crate::frame) fn staging_close_matches_retiring_request(
        &self,
        requested: WindowCloseObservation,
    ) -> bool {
        self.staging_close_aborts
            .get(&requested.binding())
            .is_some_and(|abort| {
                abort.requested == requested
                    && matches!(abort.phase, StagingCloseAbortPhase::Retiring { .. })
            })
    }

    pub(in crate::frame) fn record_staging_close_clear(
        &mut self,
        requested: WindowCloseObservation,
        observation: WindowCloseObservation,
    ) -> bool {
        if requested.binding() != observation.binding()
            || requested.known_state() != Some(WindowCloseState::LiveRequested)
            || observation.known_state() != Some(WindowCloseState::LiveClear)
        {
            return false;
        }
        let Some(abort) = self.staging_close_aborts.get_mut(&observation.binding()) else {
            return false;
        };
        if abort.requested != requested {
            return false;
        }
        let StagingCloseAbortPhase::Retiring { cleared } = &mut abort.phase else {
            return false;
        };
        *cleared = Some(observation);
        true
    }

    pub(in crate::frame) fn staging_close_bindings_for_cleanup(
        &self,
        effect: EffectId,
    ) -> Vec<ViewportBinding> {
        self.staging_close_aborts
            .iter()
            .filter_map(|(binding, abort)| {
                (abort.cleanup == Some(effect)
                    && matches!(
                        abort.phase,
                        StagingCloseAbortPhase::Retiring { cleared: Some(_) }
                    ))
                .then_some(*binding)
            })
            .collect()
    }

    pub(in crate::frame) fn staging_close_resume(
        &self,
        binding: ViewportBinding,
    ) -> Option<StagingCloseResume> {
        self.staging_close_aborts
            .get(&binding)
            .and_then(|abort| match abort.phase {
                StagingCloseAbortPhase::Retiring {
                    cleared: Some(clear),
                } => Some(StagingCloseResume {
                    pre_close_admission: abort.pre_close_admission,
                    cleanup: abort.cleanup,
                    clear,
                }),
                StagingCloseAbortPhase::Retiring { cleared: None }
                | StagingCloseAbortPhase::ResumedPending { .. } => None,
            })
    }

    pub(in crate::frame) fn mark_staging_close_resumed(
        &mut self,
        binding: ViewportBinding,
        cleared_inventory_generation: InventoryGeneration,
    ) -> Result<(), RecoveryReplacementLifecycleError> {
        let abort = self
            .staging_close_aborts
            .get_mut(&binding)
            .ok_or(RecoveryReplacementLifecycleError::MissingStagingAbort { binding })?;
        if !matches!(abort.phase, StagingCloseAbortPhase::Retiring { .. }) {
            return Err(RecoveryReplacementLifecycleError::InvalidStatus {
                surface: binding.surface(),
            });
        }
        abort.phase = StagingCloseAbortPhase::ResumedPending {
            cleared_inventory_generation,
        };
        Ok(())
    }

    pub(in crate::frame) fn take_staging_close(
        &mut self,
        binding: ViewportBinding,
    ) -> Option<StagingCloseAbort> {
        self.staging_close_aborts.remove(&binding)
    }

    pub(in crate::frame) fn staging_close_owner_surface(
        &self,
        binding: ViewportBinding,
    ) -> Option<SurfaceId> {
        match self.staging_close_aborts.get(&binding)?.owner {
            StagingCloseAbortOwner::RecoveryReplacement { surface } => Some(surface),
        }
    }

    pub(in crate::frame) fn recovery_retry_is_suppressed(
        &self,
        destroyed_binding: ViewportBinding,
        recovery_obligation: SurfaceRecoveryObligationId,
    ) -> bool {
        let Some(pending) = self.pending.get(&destroyed_binding.surface()) else {
            return false;
        };
        if pending.destroyed_binding != destroyed_binding
            || pending.recovery_obligation != recovery_obligation
        {
            return false;
        }
        let Some(replacement) = pending.replacement_binding() else {
            return false;
        };
        self.staging_close_owner_surface(replacement) == Some(destroyed_binding.surface())
    }

    pub(in crate::frame) fn staging_close_allows_admission(
        &self,
        binding: ViewportBinding,
        presentation: WindowPresentationObservation,
    ) -> bool {
        self.staging_close_aborts.get(&binding).is_none_or(|abort| {
            matches!(
                abort.phase,
                StagingCloseAbortPhase::ResumedPending {
                    cleared_inventory_generation,
                } if presentation.inventory_generation() > cleared_inventory_generation
            )
        })
    }
}
