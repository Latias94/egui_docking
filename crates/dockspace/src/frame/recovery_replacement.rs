//! Native surface recovery and replacement ownership lifecycle.

use std::collections::{BTreeMap, BTreeSet};

use crate::effect::EffectId;
use crate::ids::SurfaceId;
use crate::presentation_observation::NativeStagingResourceId;
use crate::surface_recovery::SurfaceRecoveryObligationId;
use crate::viewport::{ViewportBinding, ViewportRole};

use super::native_bringup::{NativeBringupPhase, NativeVisibleProof};

mod abandoned;
mod status;
pub(super) use self::abandoned::{AbandonedReplacementCause, AbandonedReplacementDetach};
use self::status::RecoveryReplacementPhase;
pub use self::status::{RecoveryPending, RecoveryPendingStatus};

/// Typed request which starts ownership of one destroyed surface recovery.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct RecoveryPendingRequest {
    pub(super) destroyed_binding: ViewportBinding,
    pub(super) role: ViewportRole,
    pub(super) recovery_obligation: SurfaceRecoveryObligationId,
    pub(super) retained_staging_resource: Option<NativeStagingResourceId>,
    phase: RecoveryReplacementPhase,
}

impl RecoveryPendingRequest {
    pub(super) const fn awaiting_host(
        destroyed_binding: ViewportBinding,
        role: ViewportRole,
        recovery_obligation: SurfaceRecoveryObligationId,
        retained_staging_resource: Option<NativeStagingResourceId>,
    ) -> Self {
        Self {
            destroyed_binding,
            role,
            recovery_obligation,
            retained_staging_resource,
            phase: RecoveryReplacementPhase::AwaitingRecoveryHost,
        }
    }

    pub(super) const fn replacement(
        destroyed_binding: ViewportBinding,
        role: ViewportRole,
        recovery_obligation: SurfaceRecoveryObligationId,
        retained_staging_resource: Option<NativeStagingResourceId>,
        binding: ViewportBinding,
        effect: EffectId,
    ) -> Self {
        Self {
            destroyed_binding,
            role,
            recovery_obligation,
            retained_staging_resource,
            phase: RecoveryReplacementPhase::ReplacementRequested {
                binding,
                create: effect,
            },
        }
    }
}

impl From<RecoveryPendingRequest> for RecoveryPending {
    fn from(request: RecoveryPendingRequest) -> Self {
        Self {
            destroyed_binding: request.destroyed_binding,
            role: request.role,
            recovery_obligation: request.recovery_obligation,
            retained_staging_resource: request.retained_staging_resource,
            phase: request.phase,
            admission_not_before: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum RecoveryReplacementLifecycleError {
    AlreadyPending {
        surface: SurfaceId,
    },
    MissingPending {
        surface: SurfaceId,
    },
    PendingIdentityMismatch {
        surface: SurfaceId,
    },
    ReplacementAlreadyOwned {
        binding: ViewportBinding,
        owner: SurfaceId,
    },
    ReplacementMismatch {
        surface: SurfaceId,
        binding: ViewportBinding,
    },
    ReplacementEffectMissing {
        surface: SurfaceId,
    },
    DuplicateRetainedResource {
        resource: NativeStagingResourceId,
    },
    InvalidStatus {
        surface: SurfaceId,
    },
}

/// Sole owner of recovery and replacement-incarnation state.
#[derive(Debug, Clone, Default, PartialEq)]
pub(super) struct RecoveryReplacementLifecycle {
    pending: BTreeMap<SurfaceId, RecoveryPending>,
    by_replacement: BTreeMap<ViewportBinding, SurfaceId>,
}

impl RecoveryReplacementLifecycle {
    pub(super) fn extend_referenced_effects(&self, effects: &mut BTreeSet<EffectId>) {
        for pending in self.pending.values() {
            effects.extend(pending.replacement_effect());
            match pending.phase {
                RecoveryReplacementPhase::BringingUp { phase, .. } => {
                    effects.extend(phase.show_effect());
                }
                RecoveryReplacementPhase::Failed { failed_effect, .. } => {
                    effects.insert(failed_effect);
                }
                RecoveryReplacementPhase::ProviderLost {
                    replacement_effect,
                    last_effect,
                    ..
                } => {
                    effects.insert(replacement_effect);
                    effects.insert(last_effect);
                }
                RecoveryReplacementPhase::AwaitingRecoveryHost
                | RecoveryReplacementPhase::ReplacementRequested { .. }
                | RecoveryReplacementPhase::ReplacementIndeterminate { .. }
                | RecoveryReplacementPhase::AwaitingFirstLive { .. }
                | RecoveryReplacementPhase::AwaitingCleanup { .. } => {}
            }
        }
    }

    pub(super) fn pending(&self, surface: SurfaceId) -> Option<&RecoveryPending> {
        self.pending.get(&surface)
    }

    pub(super) fn iter(&self) -> impl Iterator<Item = (&SurfaceId, &RecoveryPending)> {
        self.pending.iter()
    }

    pub(super) fn values(&self) -> impl Iterator<Item = &RecoveryPending> {
        self.pending.values()
    }

    pub(super) fn len(&self) -> usize {
        self.pending.len()
    }

    #[cfg(test)]
    pub(super) fn is_empty(&self) -> bool {
        self.pending.is_empty()
    }

    pub(super) fn replacement_surface(&self, binding: ViewportBinding) -> Option<SurfaceId> {
        self.by_replacement.get(&binding).copied()
    }

    pub(super) fn contains_replacement(&self, binding: ViewportBinding) -> bool {
        self.by_replacement.contains_key(&binding)
    }

    pub(super) fn begin(
        &mut self,
        request: RecoveryPendingRequest,
    ) -> Result<(), RecoveryReplacementLifecycleError> {
        let surface = request.destroyed_binding.surface();
        if self.pending.contains_key(&surface) {
            return Err(RecoveryReplacementLifecycleError::AlreadyPending { surface });
        }
        if request
            .phase
            .replacement_binding()
            .is_some_and(|binding| binding.surface() != surface)
        {
            return Err(RecoveryReplacementLifecycleError::PendingIdentityMismatch { surface });
        }
        if let Some(resource) = request.retained_staging_resource
            && self
                .pending
                .values()
                .any(|pending| pending.retained_staging_resource == Some(resource))
        {
            return Err(RecoveryReplacementLifecycleError::DuplicateRetainedResource { resource });
        }
        if let Some(binding) = request.phase.replacement_binding() {
            self.reserve_replacement(surface, binding)?;
        }
        self.pending.insert(surface, request.into());
        Ok(())
    }

    pub(super) fn bringup(
        &self,
        binding: ViewportBinding,
    ) -> Option<(SurfaceId, NativeBringupPhase)> {
        let surface = self.replacement_surface(binding)?;
        let pending = self.pending.get(&surface)?;
        match pending.phase {
            RecoveryReplacementPhase::ReplacementRequested {
                binding: owned,
                create,
            }
            | RecoveryReplacementPhase::ReplacementIndeterminate {
                binding: owned,
                create,
            } if owned == binding => Some((surface, NativeBringupPhase::AwaitingHidden { create })),
            RecoveryReplacementPhase::BringingUp {
                binding: owned,
                phase,
                ..
            } if owned == binding => Some((surface, phase)),
            RecoveryReplacementPhase::AwaitingRecoveryHost
            | RecoveryReplacementPhase::ReplacementRequested { .. }
            | RecoveryReplacementPhase::ReplacementIndeterminate { .. }
            | RecoveryReplacementPhase::BringingUp { .. }
            | RecoveryReplacementPhase::Failed { .. }
            | RecoveryReplacementPhase::ProviderLost { .. }
            | RecoveryReplacementPhase::AwaitingFirstLive { .. }
            | RecoveryReplacementPhase::AwaitingCleanup { .. } => None,
        }
    }

    pub(super) fn active_bringups(&self) -> Vec<(SurfaceId, ViewportBinding, NativeBringupPhase)> {
        self.pending
            .iter()
            .filter_map(|(surface, pending)| match pending.phase {
                RecoveryReplacementPhase::ReplacementRequested { binding, create }
                | RecoveryReplacementPhase::ReplacementIndeterminate { binding, create } => Some((
                    *surface,
                    binding,
                    NativeBringupPhase::AwaitingHidden { create },
                )),
                RecoveryReplacementPhase::BringingUp { binding, phase, .. } => {
                    Some((*surface, binding, phase))
                }
                RecoveryReplacementPhase::AwaitingRecoveryHost
                | RecoveryReplacementPhase::Failed { .. }
                | RecoveryReplacementPhase::ProviderLost { .. }
                | RecoveryReplacementPhase::AwaitingFirstLive { .. }
                | RecoveryReplacementPhase::AwaitingCleanup { .. } => None,
            })
            .collect()
    }

    pub(super) fn update_bringup(
        &mut self,
        binding: ViewportBinding,
        expected: NativeBringupPhase,
        next: NativeBringupPhase,
    ) -> Result<(), RecoveryReplacementLifecycleError> {
        let surface = self.replacement_owner(binding)?;
        let pending = self
            .pending
            .get_mut(&surface)
            .ok_or(RecoveryReplacementLifecycleError::MissingPending { surface })?;
        let matches_expected = match pending.phase {
            RecoveryReplacementPhase::ReplacementRequested {
                binding: owned,
                create,
            }
            | RecoveryReplacementPhase::ReplacementIndeterminate {
                binding: owned,
                create,
            } => owned == binding && expected == NativeBringupPhase::AwaitingHidden { create },
            RecoveryReplacementPhase::BringingUp {
                binding: owned,
                phase,
            } => owned == binding && phase == expected,
            RecoveryReplacementPhase::AwaitingRecoveryHost
            | RecoveryReplacementPhase::Failed { .. }
            | RecoveryReplacementPhase::ProviderLost { .. }
            | RecoveryReplacementPhase::AwaitingFirstLive { .. }
            | RecoveryReplacementPhase::AwaitingCleanup { .. } => false,
        };
        if !matches_expected || matches!(next, NativeBringupPhase::AwaitingHidden { .. }) {
            return Err(RecoveryReplacementLifecycleError::PendingIdentityMismatch { surface });
        }
        pending.phase = RecoveryReplacementPhase::BringingUp {
            binding,
            phase: next,
        };
        Ok(())
    }

    pub(super) fn mark_provider_lost(
        &mut self,
        binding: ViewportBinding,
        expected: NativeBringupPhase,
        binding_was_discarded: bool,
    ) -> Result<SurfaceId, RecoveryReplacementLifecycleError> {
        let surface = self.replacement_owner(binding)?;
        let pending = self
            .pending
            .get_mut(&surface)
            .ok_or(RecoveryReplacementLifecycleError::MissingPending { surface })?;
        let phase_matches = match pending.phase {
            RecoveryReplacementPhase::ReplacementRequested {
                binding: owned,
                create,
            }
            | RecoveryReplacementPhase::ReplacementIndeterminate {
                binding: owned,
                create,
            } => owned == binding && expected == NativeBringupPhase::AwaitingHidden { create },
            RecoveryReplacementPhase::BringingUp {
                binding: owned,
                phase,
            } => owned == binding && phase == expected,
            RecoveryReplacementPhase::AwaitingRecoveryHost
            | RecoveryReplacementPhase::Failed { .. }
            | RecoveryReplacementPhase::ProviderLost { .. }
            | RecoveryReplacementPhase::AwaitingFirstLive { .. }
            | RecoveryReplacementPhase::AwaitingCleanup { .. } => false,
        };
        if !phase_matches {
            return Err(RecoveryReplacementLifecycleError::PendingIdentityMismatch { surface });
        }
        if binding_was_discarded {
            self.by_replacement.remove(&binding);
        }
        pending.phase = RecoveryReplacementPhase::ProviderLost {
            binding: (!binding_was_discarded).then_some(binding),
            replacement_effect: expected.create_effect(),
            last_effect: expected
                .show_effect()
                .unwrap_or_else(|| expected.create_effect()),
        };
        Ok(surface)
    }

    pub(super) fn mark_awaiting_first_live(
        &mut self,
        binding: ViewportBinding,
        expected: NativeBringupPhase,
        proof: NativeVisibleProof,
    ) -> Result<RecoveryPending, RecoveryReplacementLifecycleError> {
        let surface = self.replacement_owner(binding)?;
        let pending = self
            .pending
            .get_mut(&surface)
            .ok_or(RecoveryReplacementLifecycleError::MissingPending { surface })?;
        if !matches!(
            pending.phase,
            RecoveryReplacementPhase::BringingUp {
                binding: owned,
                phase,
                ..
            } if owned == binding && phase == expected
        ) || proof.binding() != binding
        {
            return Err(RecoveryReplacementLifecycleError::InvalidStatus { surface });
        }
        pending.phase = RecoveryReplacementPhase::AwaitingFirstLive { proof };
        Ok(pending.clone())
    }

    pub(super) fn observe_replacement_dispatch(
        &mut self,
        surface: SurfaceId,
        effect: EffectId,
        indeterminate: bool,
        replacement_was_discarded: bool,
    ) -> bool {
        let Some(pending) = self.pending.get_mut(&surface) else {
            return false;
        };
        if !pending.phase.owns_dispatch_effect(effect) {
            return false;
        }
        if indeterminate {
            if let RecoveryReplacementPhase::ReplacementRequested { binding, create } =
                pending.phase
            {
                pending.phase =
                    RecoveryReplacementPhase::ReplacementIndeterminate { binding, create };
            }
            return true;
        }
        let Some(replacement_effect) = pending.replacement_effect() else {
            return false;
        };
        let mut binding = pending.replacement_binding();
        if replacement_was_discarded && let Some(discarded) = binding.take() {
            self.by_replacement.remove(&discarded);
        }
        pending.phase = RecoveryReplacementPhase::Failed {
            binding,
            replacement_effect,
            failed_effect: effect,
        };
        true
    }

    pub(super) fn detach_abandoned_replacement(
        &mut self,
        surface: SurfaceId,
        binding: ViewportBinding,
    ) -> Result<AbandonedReplacementDetach, RecoveryReplacementLifecycleError> {
        self.ensure_replacement(surface, binding)?;
        let pending = self
            .pending
            .get_mut(&surface)
            .ok_or(RecoveryReplacementLifecycleError::MissingPending { surface })?;
        let (owned, replacement_effect, cause) = match pending.phase {
            RecoveryReplacementPhase::Failed {
                binding: Some(owned),
                replacement_effect,
                failed_effect,
            } => (
                owned,
                replacement_effect,
                AbandonedReplacementCause::DispatchFailed { failed_effect },
            ),
            RecoveryReplacementPhase::ProviderLost {
                binding: Some(owned),
                replacement_effect,
                last_effect,
            } => (
                owned,
                replacement_effect,
                AbandonedReplacementCause::ProviderLost { last_effect },
            ),
            RecoveryReplacementPhase::AwaitingRecoveryHost
            | RecoveryReplacementPhase::ReplacementRequested { .. }
            | RecoveryReplacementPhase::ReplacementIndeterminate { .. }
            | RecoveryReplacementPhase::BringingUp { .. }
            | RecoveryReplacementPhase::Failed { binding: None, .. }
            | RecoveryReplacementPhase::ProviderLost { binding: None, .. }
            | RecoveryReplacementPhase::AwaitingFirstLive { .. }
            | RecoveryReplacementPhase::AwaitingCleanup { .. } => {
                return Err(RecoveryReplacementLifecycleError::InvalidStatus { surface });
            }
        };
        if owned != binding {
            return Err(RecoveryReplacementLifecycleError::PendingIdentityMismatch { surface });
        }
        self.by_replacement.remove(&binding);
        pending.phase = match cause {
            AbandonedReplacementCause::DispatchFailed { failed_effect } => {
                RecoveryReplacementPhase::Failed {
                    binding: None,
                    replacement_effect,
                    failed_effect,
                }
            }
            AbandonedReplacementCause::ProviderLost { last_effect } => {
                RecoveryReplacementPhase::ProviderLost {
                    binding: None,
                    replacement_effect,
                    last_effect,
                }
            }
        };
        Ok(AbandonedReplacementDetach {
            replacement_effect,
            cause,
        })
    }

    pub(super) fn replacement_for_effect(
        &self,
        effect: EffectId,
    ) -> Option<(SurfaceId, Option<ViewportBinding>)> {
        self.pending.iter().find_map(|(surface, pending)| {
            pending
                .phase
                .owns_dispatch_effect(effect)
                .then_some((*surface, pending.replacement_binding()))
        })
    }

    pub(super) fn begin_cleanup(
        &mut self,
        surface: SurfaceId,
    ) -> Result<(), RecoveryReplacementLifecycleError> {
        let pending = self
            .pending
            .get_mut(&surface)
            .ok_or(RecoveryReplacementLifecycleError::MissingPending { surface })?;
        let Some(binding) = pending.replacement_binding() else {
            return Err(RecoveryReplacementLifecycleError::ReplacementEffectMissing { surface });
        };
        let Some(replacement_effect) = pending.replacement_effect() else {
            return Err(RecoveryReplacementLifecycleError::ReplacementEffectMissing { surface });
        };
        pending.retained_staging_resource = None;
        pending.phase = RecoveryReplacementPhase::AwaitingCleanup {
            binding,
            replacement_effect,
        };
        Ok(())
    }

    pub(super) fn transfer_cleanup_for_provider_replacement(
        &mut self,
        surface: SurfaceId,
        binding: ViewportBinding,
    ) -> Result<EffectId, RecoveryReplacementLifecycleError> {
        self.ensure_replacement(surface, binding)?;
        let pending = self
            .pending
            .get(&surface)
            .ok_or(RecoveryReplacementLifecycleError::MissingPending { surface })?;
        let RecoveryReplacementPhase::AwaitingCleanup {
            binding: owned,
            replacement_effect,
        } = pending.phase
        else {
            return Err(RecoveryReplacementLifecycleError::InvalidStatus { surface });
        };
        if owned != binding {
            return Err(RecoveryReplacementLifecycleError::PendingIdentityMismatch { surface });
        }
        let _ = self.take(surface)?;
        Ok(replacement_effect)
    }

    pub(super) fn reset_lost_replacement(
        &mut self,
        surface: SurfaceId,
        binding: ViewportBinding,
    ) -> Result<RecoveryPending, RecoveryReplacementLifecycleError> {
        self.ensure_replacement(surface, binding)?;
        self.by_replacement.remove(&binding);
        let pending = self
            .pending
            .get_mut(&surface)
            .ok_or(RecoveryReplacementLifecycleError::MissingPending { surface })?;
        pending.phase = RecoveryReplacementPhase::AwaitingRecoveryHost;
        pending.admission_not_before = None;
        Ok(pending.clone())
    }

    pub(super) fn mark_admission_fence(
        &mut self,
        binding: ViewportBinding,
        generation: crate::viewport::InventoryGeneration,
    ) -> Result<(), RecoveryReplacementLifecycleError> {
        let surface = self.replacement_owner(binding)?;
        let pending = self
            .pending
            .get_mut(&surface)
            .ok_or(RecoveryReplacementLifecycleError::MissingPending { surface })?;
        if pending.replacement_binding() != Some(binding) {
            return Err(RecoveryReplacementLifecycleError::PendingIdentityMismatch { surface });
        }
        pending.admission_not_before = Some(generation);
        Ok(())
    }

    pub(super) fn admission_allows(
        &self,
        binding: ViewportBinding,
        generation: crate::viewport::InventoryGeneration,
    ) -> bool {
        let Some(surface) = self.replacement_surface(binding) else {
            return true;
        };
        self.pending.get(&surface).is_some_and(|pending| {
            pending.replacement_binding() == Some(binding)
                && pending
                    .admission_not_before()
                    .is_none_or(|minimum| generation > minimum)
        })
    }

    pub(super) fn take_for_vacancy(
        &mut self,
        surface: SurfaceId,
        binding: ViewportBinding,
    ) -> Result<RecoveryPending, RecoveryReplacementLifecycleError> {
        self.ensure_replacement(surface, binding)?;
        self.take(surface)
    }

    pub(super) fn complete_without_replacement(
        &mut self,
        surface: SurfaceId,
    ) -> Result<RecoveryPending, RecoveryReplacementLifecycleError> {
        let pending = self
            .pending
            .get(&surface)
            .ok_or(RecoveryReplacementLifecycleError::MissingPending { surface })?;
        if pending.replacement_binding().is_some() {
            return Err(RecoveryReplacementLifecycleError::InvalidStatus { surface });
        }
        self.take(surface)
    }

    pub(super) fn complete_first_live(
        &mut self,
        destroyed_binding: ViewportBinding,
        replacement_binding: ViewportBinding,
        recovery_obligation: SurfaceRecoveryObligationId,
    ) -> Result<RecoveryPending, RecoveryReplacementLifecycleError> {
        let surface = destroyed_binding.surface();
        let pending = self
            .pending
            .get(&surface)
            .ok_or(RecoveryReplacementLifecycleError::MissingPending { surface })?;
        if pending.destroyed_binding != destroyed_binding
            || pending.replacement_binding() != Some(replacement_binding)
            || pending.recovery_obligation != recovery_obligation
            || !matches!(
                pending.phase,
                RecoveryReplacementPhase::AwaitingFirstLive { .. }
            )
        {
            return Err(RecoveryReplacementLifecycleError::PendingIdentityMismatch { surface });
        }
        self.ensure_replacement(surface, replacement_binding)?;
        self.take(surface)
    }

    pub(super) fn clear(&mut self) -> Vec<RecoveryPending> {
        self.by_replacement.clear();
        let pending = std::mem::take(&mut self.pending);
        pending.into_values().collect()
    }

    fn take(
        &mut self,
        surface: SurfaceId,
    ) -> Result<RecoveryPending, RecoveryReplacementLifecycleError> {
        let pending = self
            .pending
            .get(&surface)
            .ok_or(RecoveryReplacementLifecycleError::MissingPending { surface })?;
        if let Some(binding) = pending.replacement_binding() {
            self.ensure_replacement(surface, binding)?;
        }
        let pending = self
            .pending
            .remove(&surface)
            .expect("pending recovery was validated before terminal removal");
        if let Some(binding) = pending.replacement_binding() {
            self.by_replacement.remove(&binding);
        }
        Ok(pending)
    }

    fn reserve_replacement(
        &mut self,
        surface: SurfaceId,
        binding: ViewportBinding,
    ) -> Result<(), RecoveryReplacementLifecycleError> {
        if let Some(owner) = self.by_replacement.get(&binding).copied() {
            return Err(RecoveryReplacementLifecycleError::ReplacementAlreadyOwned {
                binding,
                owner,
            });
        }
        self.by_replacement.insert(binding, surface);
        Ok(())
    }

    fn replacement_owner(
        &self,
        binding: ViewportBinding,
    ) -> Result<SurfaceId, RecoveryReplacementLifecycleError> {
        self.by_replacement.get(&binding).copied().ok_or(
            RecoveryReplacementLifecycleError::ReplacementMismatch {
                surface: binding.surface(),
                binding,
            },
        )
    }

    fn ensure_replacement(
        &self,
        surface: SurfaceId,
        binding: ViewportBinding,
    ) -> Result<(), RecoveryReplacementLifecycleError> {
        if self.by_replacement.get(&binding).copied() != Some(surface)
            || self
                .pending
                .get(&surface)
                .and_then(RecoveryPending::replacement_binding)
                != Some(binding)
        {
            return Err(RecoveryReplacementLifecycleError::ReplacementMismatch {
                surface,
                binding,
            });
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
