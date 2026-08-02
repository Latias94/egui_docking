//! Native surface recovery and replacement ownership lifecycle.

use std::collections::{BTreeMap, BTreeSet};

use crate::effect::EffectId;
use crate::ids::SurfaceId;
use crate::platform::{WindowCloseObservation, WindowCloseState, WindowPresentationObservation};
use crate::presentation_observation::NativeStagingResourceId;
use crate::surface_recovery::SurfaceRecoveryObligationId;
use crate::viewport::{InventoryGeneration, ViewportBinding, ViewportRole};
use crate::viewport_registry::ViewportAdmission;

/// Queryable recovery phase after a native surface was authoritatively destroyed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RecoveryPendingStatus {
    AwaitingRecoveryHost,
    ReplacementRegistered,
    ReplacementRequested { effect: EffectId },
    ReplacementIndeterminate { effect: EffectId },
    ReplacementFailed { effect: EffectId },
    AwaitingFirstLivePresentation,
    RecoveryCommitted,
    CompensatingReplacement { effect: EffectId },
}

/// Whole-root recovery retained while neither a contained host nor replacement is ready.
#[derive(Debug, Clone, PartialEq)]
pub struct RecoveryPending {
    destroyed_binding: ViewportBinding,
    role: ViewportRole,
    recovery_obligation: SurfaceRecoveryObligationId,
    replacement_binding: Option<ViewportBinding>,
    replacement_effect: Option<EffectId>,
    retained_staging_resource: Option<NativeStagingResourceId>,
    status: RecoveryPendingStatus,
}

impl RecoveryPending {
    #[must_use]
    pub const fn destroyed_binding(&self) -> ViewportBinding {
        self.destroyed_binding
    }

    #[must_use]
    pub const fn role(&self) -> ViewportRole {
        self.role
    }

    pub(crate) const fn recovery_obligation(&self) -> SurfaceRecoveryObligationId {
        self.recovery_obligation
    }

    #[must_use]
    pub const fn replacement_binding(&self) -> Option<ViewportBinding> {
        self.replacement_binding
    }

    #[must_use]
    pub const fn replacement_effect(&self) -> Option<EffectId> {
        self.replacement_effect
    }

    /// Returns the retained source resource associated with a transferred native create.
    #[must_use]
    pub const fn retained_staging_resource(&self) -> Option<NativeStagingResourceId> {
        self.retained_staging_resource
    }

    #[must_use]
    pub const fn status(&self) -> RecoveryPendingStatus {
        self.status
    }
}

/// Typed request which starts ownership of one destroyed surface recovery.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct RecoveryPendingRequest {
    pub(super) destroyed_binding: ViewportBinding,
    pub(super) role: ViewportRole,
    pub(super) recovery_obligation: SurfaceRecoveryObligationId,
    pub(super) replacement_binding: Option<ViewportBinding>,
    pub(super) replacement_effect: Option<EffectId>,
    pub(super) retained_staging_resource: Option<NativeStagingResourceId>,
    pub(super) status: RecoveryPendingStatus,
}

impl From<RecoveryPendingRequest> for RecoveryPending {
    fn from(request: RecoveryPendingRequest) -> Self {
        Self {
            destroyed_binding: request.destroyed_binding,
            role: request.role,
            recovery_obligation: request.recovery_obligation,
            replacement_binding: request.replacement_binding,
            replacement_effect: request.replacement_effect,
            retained_staging_resource: request.retained_staging_resource,
            status: request.status,
        }
    }
}

/// Core-private owner of a native close which arrived before replacement admission.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum StagingCloseAbortOwner {
    RecoveryReplacement { surface: SurfaceId },
}

/// One internal compensation lane for a staging replacement close.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct StagingCloseAbort {
    owner: StagingCloseAbortOwner,
    cleanup: Option<EffectId>,
    requested: WindowCloseObservation,
    pre_close_admission: ViewportAdmission,
    phase: StagingCloseAbortPhase,
}

impl StagingCloseAbort {
    pub(super) const fn owner(self) -> StagingCloseAbortOwner {
        self.owner
    }

    pub(super) const fn cleanup(self) -> Option<EffectId> {
        self.cleanup
    }

    pub(super) const fn requested(self) -> WindowCloseObservation {
        self.requested
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum StagingCloseAbortPhase {
    Retiring {
        cleared: Option<WindowCloseObservation>,
    },
    ResumedPending {
        cleared_inventory_generation: InventoryGeneration,
    },
}

/// Typed request which starts one replacement-owned staging close lane.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct StagingCloseAbortRequest {
    pub(super) binding: ViewportBinding,
    pub(super) owner: StagingCloseAbortOwner,
    pub(super) cleanup: Option<EffectId>,
    pub(super) requested: WindowCloseObservation,
    pub(super) pre_close_admission: ViewportAdmission,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct StagingCloseResume {
    pub(super) pre_close_admission: ViewportAdmission,
    pub(super) cleanup: Option<EffectId>,
    pub(super) clear: WindowCloseObservation,
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
    CompensationEffectMissing {
        effect: EffectId,
    },
    DuplicateRetainedResource {
        resource: NativeStagingResourceId,
    },
    InvalidStatus {
        surface: SurfaceId,
    },
    StagingAbortAlreadyExists {
        binding: ViewportBinding,
    },
    MissingStagingAbort {
        binding: ViewportBinding,
    },
    StagingAbortOwnerMismatch {
        binding: ViewportBinding,
    },
}

/// Sole owner of recovery, replacement-incarnation, and pre-admission close state.
#[derive(Debug, Clone, Default, PartialEq)]
pub(super) struct RecoveryReplacementLifecycle {
    pending: BTreeMap<SurfaceId, RecoveryPending>,
    by_replacement: BTreeMap<ViewportBinding, SurfaceId>,
    staging_close_aborts: BTreeMap<ViewportBinding, StagingCloseAbort>,
}

impl RecoveryReplacementLifecycle {
    pub(super) fn extend_referenced_effects(&self, effects: &mut BTreeSet<EffectId>) {
        for pending in self.pending.values() {
            effects.extend(pending.replacement_effect);
            let status_effect = match pending.status {
                RecoveryPendingStatus::ReplacementRequested { effect }
                | RecoveryPendingStatus::ReplacementIndeterminate { effect }
                | RecoveryPendingStatus::ReplacementFailed { effect }
                | RecoveryPendingStatus::CompensatingReplacement { effect } => Some(effect),
                RecoveryPendingStatus::AwaitingRecoveryHost
                | RecoveryPendingStatus::ReplacementRegistered
                | RecoveryPendingStatus::AwaitingFirstLivePresentation
                | RecoveryPendingStatus::RecoveryCommitted => None,
            };
            effects.extend(status_effect);
        }
        effects.extend(
            self.staging_close_aborts
                .values()
                .filter_map(|abort| abort.cleanup),
        );
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

    pub(super) fn contains_staging_close(&self, binding: ViewportBinding) -> bool {
        self.staging_close_aborts.contains_key(&binding)
    }

    pub(super) fn staging_close(&self, binding: ViewportBinding) -> Option<StagingCloseAbort> {
        self.staging_close_aborts.get(&binding).copied()
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
            .replacement_binding
            .is_some_and(|binding| binding.surface() != surface)
        {
            return Err(RecoveryReplacementLifecycleError::PendingIdentityMismatch { surface });
        }
        self.validate_status(&request)?;
        if let Some(resource) = request.retained_staging_resource
            && self
                .pending
                .values()
                .any(|pending| pending.retained_staging_resource == Some(resource))
        {
            return Err(RecoveryReplacementLifecycleError::DuplicateRetainedResource { resource });
        }
        if let Some(binding) = request.replacement_binding {
            self.reserve_replacement(surface, binding)?;
        }
        self.pending.insert(surface, request.into());
        Ok(())
    }

    pub(super) fn may_adopt_existing(
        &self,
        surface: SurfaceId,
        role: ViewportRole,
        recovery_obligation: Option<SurfaceRecoveryObligationId>,
    ) -> Result<bool, RecoveryReplacementLifecycleError> {
        let Some(pending) = self.pending.get(&surface) else {
            return Ok(false);
        };
        let may_adopt = pending.replacement_binding.is_none()
            && matches!(
                pending.status,
                RecoveryPendingStatus::AwaitingRecoveryHost
                    | RecoveryPendingStatus::ReplacementFailed { .. }
            )
            && pending.role == role
            && recovery_obligation == Some(pending.recovery_obligation);
        if !may_adopt {
            return Err(RecoveryReplacementLifecycleError::PendingIdentityMismatch { surface });
        }
        Ok(true)
    }

    pub(super) fn adopt_existing(
        &mut self,
        surface: SurfaceId,
        binding: ViewportBinding,
    ) -> Result<(), RecoveryReplacementLifecycleError> {
        let pending = self
            .pending
            .get(&surface)
            .ok_or(RecoveryReplacementLifecycleError::MissingPending { surface })?;
        if pending.replacement_binding.is_some()
            || binding.surface() != surface
            || !matches!(
                pending.status,
                RecoveryPendingStatus::AwaitingRecoveryHost
                    | RecoveryPendingStatus::ReplacementFailed { .. }
            )
        {
            return Err(RecoveryReplacementLifecycleError::PendingIdentityMismatch { surface });
        }
        self.reserve_replacement(surface, binding)?;
        let pending = self
            .pending
            .get_mut(&surface)
            .ok_or(RecoveryReplacementLifecycleError::MissingPending { surface })?;
        pending.replacement_binding = Some(binding);
        pending.replacement_effect = None;
        pending.status = RecoveryPendingStatus::ReplacementRegistered;
        Ok(())
    }

    pub(super) fn mark_awaiting_first_live(
        &mut self,
        binding: ViewportBinding,
    ) -> Result<RecoveryPending, RecoveryReplacementLifecycleError> {
        let surface = self.replacement_owner(binding)?;
        let pending = self
            .pending
            .get_mut(&surface)
            .ok_or(RecoveryReplacementLifecycleError::MissingPending { surface })?;
        if !matches!(
            pending.status,
            RecoveryPendingStatus::ReplacementRegistered
                | RecoveryPendingStatus::ReplacementRequested { .. }
                | RecoveryPendingStatus::ReplacementIndeterminate { .. }
                | RecoveryPendingStatus::ReplacementFailed { .. }
        ) {
            return Err(RecoveryReplacementLifecycleError::InvalidStatus { surface });
        }
        pending.status = RecoveryPendingStatus::AwaitingFirstLivePresentation;
        Ok(pending.clone())
    }

    pub(super) fn observe_replacement_dispatch(
        &mut self,
        effect: EffectId,
        indeterminate: bool,
        replacement_was_discarded: bool,
    ) -> Option<SurfaceId> {
        let surface = self.pending.iter().find_map(|(surface, pending)| {
            (pending.replacement_effect == Some(effect)).then_some(*surface)
        })?;
        let pending = self.pending.get_mut(&surface)?;
        let owns_effect = matches!(
            pending.status,
            RecoveryPendingStatus::ReplacementRequested { effect: owned }
                | RecoveryPendingStatus::ReplacementIndeterminate { effect: owned }
                | RecoveryPendingStatus::ReplacementFailed { effect: owned }
                if owned == effect
        );
        if !owns_effect {
            return None;
        }
        if indeterminate {
            pending.status = RecoveryPendingStatus::ReplacementIndeterminate { effect };
            return Some(surface);
        }
        if replacement_was_discarded && let Some(binding) = pending.replacement_binding.take() {
            self.by_replacement.remove(&binding);
            self.staging_close_aborts.remove(&binding);
        }
        pending.status = RecoveryPendingStatus::ReplacementFailed { effect };
        Some(surface)
    }

    pub(super) fn compensation_surface(&self, effect: EffectId) -> Option<SurfaceId> {
        self.pending.iter().find_map(|(surface, pending)| {
            matches!(
                pending.status,
                RecoveryPendingStatus::CompensatingReplacement { effect: owned }
                    if owned == effect
            )
            .then_some(*surface)
        })
    }

    pub(super) fn replacement_for_effect(
        &self,
        effect: EffectId,
    ) -> Option<(SurfaceId, Option<ViewportBinding>)> {
        self.pending.iter().find_map(|(surface, pending)| {
            let owns_dispatch = matches!(
                pending.status,
                RecoveryPendingStatus::ReplacementRequested { effect: owned }
                    | RecoveryPendingStatus::ReplacementIndeterminate { effect: owned }
                    | RecoveryPendingStatus::ReplacementFailed { effect: owned }
                    if owned == effect
            );
            (pending.replacement_effect == Some(effect) && owns_dispatch)
                .then_some((*surface, pending.replacement_binding))
        })
    }

    pub(super) fn begin_compensation(
        &mut self,
        surface: SurfaceId,
        effect: EffectId,
    ) -> Result<(), RecoveryReplacementLifecycleError> {
        let pending = self
            .pending
            .get_mut(&surface)
            .ok_or(RecoveryReplacementLifecycleError::MissingPending { surface })?;
        if pending.replacement_binding.is_none() || pending.replacement_effect.is_none() {
            return Err(RecoveryReplacementLifecycleError::ReplacementEffectMissing { surface });
        }
        pending.retained_staging_resource = None;
        pending.status = RecoveryPendingStatus::CompensatingReplacement { effect };
        Ok(())
    }

    pub(super) fn retry_compensation(
        &mut self,
        failed_effect: EffectId,
        retry_effect: EffectId,
    ) -> Result<SurfaceId, RecoveryReplacementLifecycleError> {
        let surface = self.compensation_surface(failed_effect).ok_or(
            RecoveryReplacementLifecycleError::CompensationEffectMissing {
                effect: failed_effect,
            },
        )?;
        self.pending
            .get_mut(&surface)
            .ok_or(RecoveryReplacementLifecycleError::MissingPending { surface })?
            .status = RecoveryPendingStatus::CompensatingReplacement {
            effect: retry_effect,
        };
        Ok(surface)
    }

    pub(super) fn reset_lost_replacement(
        &mut self,
        surface: SurfaceId,
        binding: ViewportBinding,
    ) -> Result<RecoveryPending, RecoveryReplacementLifecycleError> {
        self.ensure_replacement(surface, binding)?;
        self.by_replacement.remove(&binding);
        self.staging_close_aborts.remove(&binding);
        let pending = self
            .pending
            .get_mut(&surface)
            .ok_or(RecoveryReplacementLifecycleError::MissingPending { surface })?;
        pending.replacement_binding = None;
        pending.replacement_effect = None;
        pending.status = RecoveryPendingStatus::AwaitingRecoveryHost;
        Ok(pending.clone())
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
        if pending.replacement_binding.is_some() {
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
            || pending.replacement_binding != Some(replacement_binding)
            || pending.recovery_obligation != recovery_obligation
            || !matches!(
                pending.status,
                RecoveryPendingStatus::AwaitingFirstLivePresentation
            )
        {
            return Err(RecoveryReplacementLifecycleError::PendingIdentityMismatch { surface });
        }
        self.ensure_replacement(surface, replacement_binding)?;
        self.take(surface)
    }

    pub(super) fn clear(&mut self) -> Vec<RecoveryPending> {
        self.by_replacement.clear();
        self.staging_close_aborts.clear();
        let pending = std::mem::take(&mut self.pending);
        pending.into_values().collect()
    }

    pub(super) fn clear_staging_close_aborts(&mut self) {
        self.staging_close_aborts.clear();
    }

    pub(super) fn begin_staging_close(
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

    pub(super) fn staging_close_matches_retiring_request(
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

    pub(super) fn record_staging_close_clear(
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

    pub(super) fn staging_close_bindings_for_cleanup(
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

    pub(super) fn staging_close_resume(
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

    pub(super) fn mark_staging_close_resumed(
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

    pub(super) fn take_staging_close(
        &mut self,
        binding: ViewportBinding,
    ) -> Option<StagingCloseAbort> {
        self.staging_close_aborts.remove(&binding)
    }

    pub(super) fn staging_close_owner_surface(
        &self,
        binding: ViewportBinding,
    ) -> Option<SurfaceId> {
        match self.staging_close_aborts.get(&binding)?.owner {
            StagingCloseAbortOwner::RecoveryReplacement { surface } => Some(surface),
        }
    }

    pub(super) fn recovery_retry_is_suppressed(
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
        let Some(replacement) = pending.replacement_binding else {
            return false;
        };
        self.staging_close_owner_surface(replacement) == Some(destroyed_binding.surface())
    }

    pub(super) fn staging_close_allows_admission(
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

    fn take(
        &mut self,
        surface: SurfaceId,
    ) -> Result<RecoveryPending, RecoveryReplacementLifecycleError> {
        let pending = self
            .pending
            .get(&surface)
            .ok_or(RecoveryReplacementLifecycleError::MissingPending { surface })?;
        if let Some(binding) = pending.replacement_binding {
            self.ensure_replacement(surface, binding)?;
        }
        let pending = self
            .pending
            .remove(&surface)
            .expect("pending recovery was validated before terminal removal");
        if let Some(binding) = pending.replacement_binding {
            self.by_replacement.remove(&binding);
            self.staging_close_aborts.remove(&binding);
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
                .and_then(|pending| pending.replacement_binding)
                != Some(binding)
        {
            return Err(RecoveryReplacementLifecycleError::ReplacementMismatch {
                surface,
                binding,
            });
        }
        Ok(())
    }

    fn validate_status(
        &self,
        request: &RecoveryPendingRequest,
    ) -> Result<(), RecoveryReplacementLifecycleError> {
        let surface = request.destroyed_binding.surface();
        let has_replacement = request.replacement_binding.is_some();
        let valid = match request.status {
            RecoveryPendingStatus::AwaitingRecoveryHost => {
                !has_replacement && request.replacement_effect.is_none()
            }
            RecoveryPendingStatus::ReplacementRegistered => {
                has_replacement && request.replacement_effect.is_none()
            }
            RecoveryPendingStatus::ReplacementRequested { effect }
            | RecoveryPendingStatus::ReplacementIndeterminate { effect } => {
                has_replacement && request.replacement_effect == Some(effect)
            }
            RecoveryPendingStatus::ReplacementFailed { effect } => {
                request.replacement_effect == Some(effect)
            }
            RecoveryPendingStatus::AwaitingFirstLivePresentation => has_replacement,
            RecoveryPendingStatus::RecoveryCommitted => false,
            RecoveryPendingStatus::CompensatingReplacement { .. } => {
                has_replacement && request.replacement_effect.is_some()
            }
        };
        if valid {
            Ok(())
        } else {
            Err(RecoveryReplacementLifecycleError::InvalidStatus { surface })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ids::{EngineAuthorityDomainId, NativeCreateSagaId, WorkspaceEpoch};
    use crate::viewport::{WindowIncarnation, WindowToken};

    fn binding(surface: u64, incarnation: u64) -> ViewportBinding {
        ViewportBinding::new(
            EngineAuthorityDomainId::new_for_test(1),
            WorkspaceEpoch::new(1),
            SurfaceId::new(surface),
            WindowToken::new(10),
            WindowIncarnation::new(incarnation),
        )
    }

    fn request(destroyed: ViewportBinding, replacement: ViewportBinding) -> RecoveryPendingRequest {
        RecoveryPendingRequest {
            destroyed_binding: destroyed,
            role: ViewportRole::Child,
            recovery_obligation: SurfaceRecoveryObligationId::new_for_test(1),
            replacement_binding: Some(replacement),
            replacement_effect: None,
            retained_staging_resource: None,
            status: RecoveryPendingStatus::ReplacementRegistered,
        }
    }

    #[test]
    fn replacement_lookup_is_exact_across_binding_incarnations() {
        let destroyed = binding(1, 1);
        let replacement = binding(1, 2);
        let stale = binding(1, 3);
        let mut lifecycle = RecoveryReplacementLifecycle::default();
        lifecycle
            .begin(request(destroyed, replacement))
            .expect("replacement must register");

        assert_eq!(
            lifecycle.replacement_surface(replacement),
            Some(SurfaceId::new(1))
        );
        assert_eq!(lifecycle.replacement_surface(stale), None);
        assert!(matches!(
            lifecycle.reset_lost_replacement(SurfaceId::new(1), stale),
            Err(RecoveryReplacementLifecycleError::ReplacementMismatch { .. })
        ));
        assert_eq!(
            lifecycle
                .pending(SurfaceId::new(1))
                .and_then(RecoveryPending::replacement_binding),
            Some(replacement)
        );
    }

    #[test]
    fn first_live_completion_requires_exact_replacement_and_obligation() {
        let destroyed = binding(1, 1);
        let replacement = binding(1, 2);
        let stale = binding(1, 3);
        let obligation = SurfaceRecoveryObligationId::new_for_test(1);
        let mut lifecycle = RecoveryReplacementLifecycle::default();
        lifecycle
            .begin(request(destroyed, replacement))
            .expect("replacement must register");
        lifecycle
            .mark_awaiting_first_live(replacement)
            .expect("replacement must enter first-live");

        assert!(matches!(
            lifecycle.complete_first_live(destroyed, stale, obligation),
            Err(RecoveryReplacementLifecycleError::PendingIdentityMismatch { .. })
        ));
        lifecycle
            .complete_first_live(destroyed, replacement, obligation)
            .expect("exact first-live proof must complete recovery");
        assert!(lifecycle.is_empty());
    }

    #[test]
    fn late_replacement_result_cannot_regress_first_live_state() {
        let destroyed = binding(1, 1);
        let replacement = binding(1, 2);
        let effect = EffectId::new(7);
        let mut pending = request(destroyed, replacement);
        pending.replacement_effect = Some(effect);
        pending.status = RecoveryPendingStatus::ReplacementRequested { effect };
        let mut lifecycle = RecoveryReplacementLifecycle::default();
        lifecycle
            .begin(pending)
            .expect("requested replacement must register");
        lifecycle
            .mark_awaiting_first_live(replacement)
            .expect("replacement must enter first-live");

        assert_eq!(
            lifecycle.observe_replacement_dispatch(effect, false, true),
            None
        );
        assert_eq!(lifecycle.replacement_for_effect(effect), None);
        assert_eq!(
            lifecycle
                .pending(SurfaceId::new(1))
                .map(RecoveryPending::status),
            Some(RecoveryPendingStatus::AwaitingFirstLivePresentation)
        );
        assert_eq!(
            lifecycle.replacement_surface(replacement),
            Some(SurfaceId::new(1))
        );
    }

    #[test]
    fn one_retained_resource_cannot_back_two_pending_recoveries() {
        let resource = NativeStagingResourceId::mint(
            EngineAuthorityDomainId::new_for_test(1),
            NativeCreateSagaId::new(9),
        );
        let mut first = request(binding(1, 1), binding(1, 2));
        first.retained_staging_resource = Some(resource);
        let mut second = request(binding(2, 1), binding(2, 2));
        second.retained_staging_resource = Some(resource);
        let mut lifecycle = RecoveryReplacementLifecycle::default();
        lifecycle
            .begin(first)
            .expect("first resource owner must register");

        assert_eq!(
            lifecycle.begin(second),
            Err(RecoveryReplacementLifecycleError::DuplicateRetainedResource { resource })
        );
        assert_eq!(lifecycle.len(), 1);
    }
}
