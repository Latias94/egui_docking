//! Exact native-binding cleanup, retirement, and token quarantine.

use std::collections::{BTreeMap, BTreeSet};

use crate::effect::{EffectId, EffectPhase, EffectUnsupportedReason, PlatformEffect};
use crate::platform::{
    PlatformSnapshot, WindowCloseObservation, WindowCloseObservationStream, WindowCloseState,
    WindowInputObservationStream,
};
use crate::platform_provider::PlatformObservationLease;
use crate::presentation_observation::NativeStagingResourceId;
use crate::retention::BindingRetentionManifest;
use crate::surface_recovery::SurfaceRecoveryObligationId;
use crate::viewport::{ViewportBinding, ViewportRole, WindowToken};
use crate::viewport_registry::ViewportOwnership;

/// Why an exact binding left the current logical surface roster.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BindingRetirementOrigin {
    NativeCreateAborted {
        create: EffectId,
    },
    RecoveryReplacementFailed {
        replacement: EffectId,
        failed: EffectId,
    },
    RecoveryReplacementProviderLost {
        replacement: EffectId,
        last_effect: EffectId,
    },
    RecoveryReplacementCleanup {
        replacement: EffectId,
    },
    RecoveryReplacementCompensationTransferred {
        replacement: EffectId,
        cleanup: EffectId,
    },
    WorkspaceReplaced,
    SurfaceVacated,
}

/// Queryable state retained for an exact binding whose platform lifecycle is still owned.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BindingRetirementStatus {
    AwaitingInputRestore,
    AwaitingAppearance,
    /// An externally owned binding remains quarantined until the provider reports
    /// destruction for this exact binding incarnation.
    AwaitingExactDestruction,
    CleanupRequested {
        effect: EffectId,
    },
    CleanupIndeterminate {
        effect: EffectId,
    },
    /// The observation-only continuation failed; the destructive predecessor remains unknown.
    CleanupObservationFailed {
        effect: EffectId,
    },
    CleanupFailed {
        effect: EffectId,
    },
    CleanupBlocked {
        effect: EffectId,
        reason: EffectUnsupportedReason,
    },
}

/// Why one binding-scoped destructive cleanup remains authoritative.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum BindingCleanupPurpose {
    /// The binding has left the live registry and remains quarantined until its
    /// exact platform lifecycle is terminal.
    Retired,
    /// A recovery replacement is still registered but cannot acquire graph
    /// ownership until this cleanup obligation settles.
    RecoveryReplacement {
        destroyed_binding: ViewportBinding,
        recovery_obligation: SurfaceRecoveryObligationId,
        requirement: RecoveryReplacementCleanupRequirement,
    },
}

/// Whether a recovery cleanup may be revoked by one exact native-close clear.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum RecoveryReplacementCleanupRequirement {
    ConditionalClose {
        requested: WindowCloseObservation,
        clear: Option<WindowCloseObservation>,
    },
    Mandatory,
}

/// Recovery work released by one exact cleanup terminal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum RecoveryReplacementCleanupCompletion {
    ResetLostReplacement {
        destroyed_binding: ViewportBinding,
        recovery_obligation: SurfaceRecoveryObligationId,
    },
    CompleteRecovery {
        destroyed_binding: ViewportBinding,
        recovery_obligation: SurfaceRecoveryObligationId,
    },
}

/// Result of reducing one registry-authoritative close request into recovery cleanup ownership.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum RecoveryCleanupCloseRequestOutcome {
    NotOwned,
    Duplicate,
    Rearmed,
    Mandatory,
}

impl RecoveryCleanupCloseRequestOutcome {
    pub(super) const fn was_consumed(self) -> bool {
        !matches!(self, Self::NotOwned)
    }
}

impl BindingRetirementStatus {
    pub(super) const fn cleanup_effect(self) -> Option<EffectId> {
        match self {
            Self::CleanupRequested { effect }
            | Self::CleanupIndeterminate { effect }
            | Self::CleanupObservationFailed { effect }
            | Self::CleanupFailed { effect }
            | Self::CleanupBlocked { effect, .. } => Some(effect),
            Self::AwaitingInputRestore
            | Self::AwaitingAppearance
            | Self::AwaitingExactDestruction => None,
        }
    }
}

/// Exact binding isolated from the current logical surface roster.
#[derive(Debug, Clone, PartialEq)]
pub struct BindingRetirement {
    binding: ViewportBinding,
    role: ViewportRole,
    ownership: ViewportOwnership,
    origin: BindingRetirementOrigin,
    status: BindingRetirementStatus,
    cleanup_predecessor: Option<EffectId>,
    observed: bool,
    input_observations: WindowInputObservationStream,
    close_observations: WindowCloseObservationStream,
    may_reappear: bool,
    cleanup: BindingCleanupAction,
    retained_staging_resource: Option<NativeStagingResourceId>,
    purpose: BindingCleanupPurpose,
}

impl BindingRetirement {
    #[must_use]
    pub const fn binding(&self) -> ViewportBinding {
        self.binding
    }

    #[must_use]
    pub const fn role(&self) -> ViewportRole {
        self.role
    }

    #[must_use]
    pub const fn ownership(&self) -> ViewportOwnership {
        self.ownership
    }

    #[must_use]
    pub const fn origin(&self) -> BindingRetirementOrigin {
        self.origin
    }

    #[must_use]
    pub const fn status(&self) -> BindingRetirementStatus {
        self.status
    }

    #[must_use]
    pub const fn observed(&self) -> bool {
        self.observed
    }

    #[must_use]
    pub const fn may_reappear(&self) -> bool {
        self.may_reappear
    }

    pub(super) const fn input_observations(&self) -> &WindowInputObservationStream {
        &self.input_observations
    }

    pub(super) const fn retained_staging_resource(&self) -> Option<NativeStagingResourceId> {
        self.retained_staging_resource
    }

    pub(super) const fn is_retired(&self) -> bool {
        matches!(self.purpose, BindingCleanupPurpose::Retired)
    }

    pub(super) const fn recovery_cleanup_requirement(
        &self,
    ) -> Option<RecoveryReplacementCleanupRequirement> {
        match self.purpose {
            BindingCleanupPurpose::Retired => None,
            BindingCleanupPurpose::RecoveryReplacement { requirement, .. } => Some(requirement),
        }
    }

    pub(super) const fn recovery_cleanup_completion(
        &self,
    ) -> Option<RecoveryReplacementCleanupCompletion> {
        match self.purpose {
            BindingCleanupPurpose::Retired => None,
            BindingCleanupPurpose::RecoveryReplacement {
                destroyed_binding,
                recovery_obligation,
                requirement: RecoveryReplacementCleanupRequirement::ConditionalClose { .. },
            } => Some(RecoveryReplacementCleanupCompletion::ResetLostReplacement {
                destroyed_binding,
                recovery_obligation,
            }),
            BindingCleanupPurpose::RecoveryReplacement {
                destroyed_binding,
                recovery_obligation,
                requirement: RecoveryReplacementCleanupRequirement::Mandatory,
            } => Some(RecoveryReplacementCleanupCompletion::CompleteRecovery {
                destroyed_binding,
                recovery_obligation,
            }),
        }
    }

    pub(super) const fn recovery_cleanup_owner(
        &self,
    ) -> Option<(ViewportBinding, SurfaceRecoveryObligationId)> {
        match self.purpose {
            BindingCleanupPurpose::Retired => None,
            BindingCleanupPurpose::RecoveryReplacement {
                destroyed_binding,
                recovery_obligation,
                ..
            } => Some((destroyed_binding, recovery_obligation)),
        }
    }

    /// Returns whether this cleanup retains an independent create-correlation proof.
    ///
    /// A recovery replacement may need compensation even when the replacement never appeared in
    /// a platform inventory snapshot: the correlated create effect is the authority that the
    /// exact runtime-owned binding may still exist. Ordinary retirement cleanup has no such
    /// proof and therefore still requires an observed window before a destructive retry.
    pub(super) const fn allows_unobserved_destructive_retry(&self) -> bool {
        matches!(
            self.purpose,
            BindingCleanupPurpose::RecoveryReplacement { .. }
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum BindingCleanupAction {
    CompensateCreate { create: EffectId },
    ReleaseOwnedWindow,
    KeepExternal,
}

/// Typed input used to begin ownership of one exact retired binding.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct BindingCleanupRequest {
    pub(super) binding: ViewportBinding,
    pub(super) role: ViewportRole,
    pub(super) ownership: ViewportOwnership,
    pub(super) origin: BindingRetirementOrigin,
    pub(super) status: BindingRetirementStatus,
    pub(super) observed: bool,
    pub(super) input_observations: WindowInputObservationStream,
    pub(super) close_observations: WindowCloseObservationStream,
    pub(super) may_reappear: bool,
    pub(super) cleanup: BindingCleanupAction,
    pub(super) retained_staging_resource: Option<NativeStagingResourceId>,
    pub(super) purpose: BindingCleanupPurpose,
}

impl From<BindingCleanupRequest> for BindingRetirement {
    fn from(request: BindingCleanupRequest) -> Self {
        Self {
            binding: request.binding,
            role: request.role,
            ownership: request.ownership,
            origin: request.origin,
            status: request.status,
            cleanup_predecessor: request.status.cleanup_effect(),
            observed: request.observed,
            input_observations: request.input_observations,
            close_observations: request.close_observations,
            may_reappear: request.may_reappear,
            cleanup: request.cleanup,
            retained_staging_resource: request.retained_staging_resource,
            purpose: request.purpose,
        }
    }
}

/// Effect request emitted by the lifecycle without mutating the effect ledger.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum BindingCleanupEffectRequest {
    CompensatingClose {
        binding: ViewportBinding,
        create: EffectId,
    },
    RequestRootClose {
        binding: ViewportBinding,
    },
    ReleaseChild {
        binding: ViewportBinding,
    },
}

impl BindingCleanupEffectRequest {
    pub(super) const fn binding(self) -> ViewportBinding {
        match self {
            Self::CompensatingClose { binding, .. }
            | Self::RequestRootClose { binding }
            | Self::ReleaseChild { binding } => binding,
        }
    }

    pub(super) const fn platform_effect(self) -> PlatformEffect {
        match self {
            Self::CompensatingClose { binding, create } => PlatformEffect::CompensatingClose {
                binding,
                compensates: create,
            },
            Self::RequestRootClose { binding } => PlatformEffect::RequestRootClose { binding },
            Self::ReleaseChild { binding } => PlatformEffect::ReleaseChild { binding },
        }
    }
}

/// Retired binding removed from lifecycle ownership after a terminal fact.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct BindingCleanupTerminal {
    retirement: BindingRetirement,
}

impl BindingCleanupTerminal {
    pub(super) const fn binding(&self) -> ViewportBinding {
        self.retirement.binding
    }

    pub(super) const fn retained_staging_resource(&self) -> Option<NativeStagingResourceId> {
        self.retirement.retained_staging_resource
    }

    pub(super) const fn recovery_cleanup_completion(
        &self,
    ) -> Option<RecoveryReplacementCleanupCompletion> {
        self.retirement.recovery_cleanup_completion()
    }
}

/// Side effects that the lifecycle delegates back to the platform coordinator.
#[derive(Debug, Clone, PartialEq)]
pub(super) enum BindingCleanupDirective {
    Drive { binding: ViewportBinding },
    RequestCleanup(BindingCleanupEffectRequest),
    Finish(BindingCleanupTerminal),
    ExactDestroyed(BindingCleanupTerminal),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum BindingCleanupError {
    AlreadyExists {
        binding: ViewportBinding,
    },
    Missing {
        binding: ViewportBinding,
    },
    TokenReserved {
        token: WindowToken,
    },
    CleanupOwnershipMismatch {
        binding: ViewportBinding,
        ownership: ViewportOwnership,
    },
    CleanupEffectOwned {
        effect: EffectId,
        owner: ViewportBinding,
    },
    CleanupObservationMismatch {
        continuation: EffectId,
        predecessor: EffectId,
    },
    CleanupWindowNotObserved {
        effect: EffectId,
    },
    CleanupPurposeMismatch {
        binding: ViewportBinding,
    },
    DestroyedTombstoneMissing {
        binding: ViewportBinding,
    },
    DestroyedTombstoneProviderMismatch {
        binding: ViewportBinding,
        expected: PlatformObservationLease,
        submitted: PlatformObservationLease,
    },
}

/// Sole owner of exact binding cleanup, effect lineage, token quarantine, and tombstones.
#[derive(Debug, Clone, Default, PartialEq)]
pub(super) struct BindingCleanupLifecycle {
    entries: BTreeMap<ViewportBinding, BindingRetirement>,
    by_token: BTreeMap<WindowToken, ViewportBinding>,
    by_cleanup_effect: BTreeMap<EffectId, ViewportBinding>,
    destroyed_tombstones: BTreeSet<ViewportBinding>,
    destroyed_tombstone_providers: BTreeMap<ViewportBinding, PlatformObservationLease>,
}

impl BindingCleanupLifecycle {
    /// Revokes provider-local observation generations for every non-terminal cleanup obligation.
    ///
    /// Stable binding ownership and cleanup lineage remain intact, but a
    /// successor provider must establish fresh input and close facts before an
    /// old `observed` bit can authorize new cleanup work.
    pub(super) fn reset_for_provider_replacement(&mut self) {
        for retirement in self.entries.values_mut() {
            retirement.may_reappear |= retirement.observed;
            retirement.observed = false;
            retirement
                .input_observations
                .reset_for_provider_replacement();
            retirement
                .close_observations
                .reset_for_provider_replacement();
        }
    }

    pub(super) fn extend_referenced_effects(&self, effects: &mut BTreeSet<EffectId>) {
        for retirement in self.entries.values() {
            if let BindingRetirementOrigin::NativeCreateAborted { create } = retirement.origin {
                effects.insert(create);
            }
            if let BindingRetirementOrigin::RecoveryReplacementFailed {
                replacement,
                failed,
            } = retirement.origin
            {
                effects.insert(replacement);
                effects.insert(failed);
            }
            if let BindingRetirementOrigin::RecoveryReplacementProviderLost {
                replacement,
                last_effect,
            } = retirement.origin
            {
                effects.insert(replacement);
                effects.insert(last_effect);
            }
            if let BindingRetirementOrigin::RecoveryReplacementCleanup { replacement } =
                retirement.origin
            {
                effects.insert(replacement);
            }
            if let BindingRetirementOrigin::RecoveryReplacementCompensationTransferred {
                replacement,
                cleanup,
            } = retirement.origin
            {
                effects.insert(replacement);
                effects.insert(cleanup);
            }
            if let Some(effect) = retirement.status.cleanup_effect() {
                effects.insert(effect);
            }
            if let BindingCleanupAction::CompensateCreate { create } = retirement.cleanup {
                effects.insert(create);
            }
        }
        effects.extend(self.by_cleanup_effect.keys().copied());
    }

    /// Accounts for every active cleanup obligation, quarantine index, lineage, and tombstone.
    ///
    /// Destroyed bindings remain guards until the platform observation ingress proves that no
    /// delayed snapshot can name the exact incarnation. A newer wall-clock frame or a reused
    /// window token is not such a proof.
    pub(super) fn retention_manifest(&self) -> BindingRetentionManifest {
        debug_assert_eq!(
            self.destroyed_tombstones.len(),
            self.destroyed_tombstone_providers.len(),
            "every destroyed binding guard retains its exact observation producer"
        );
        BindingRetentionManifest::new(
            self.entries.len(),
            self.by_token.len(),
            self.by_cleanup_effect.len(),
            self.destroyed_tombstones.len(),
        )
    }

    pub(super) fn contains_key(&self, binding: &ViewportBinding) -> bool {
        self.entries.contains_key(binding)
    }

    pub(super) fn get(&self, binding: &ViewportBinding) -> Option<&BindingRetirement> {
        self.entries.get(binding)
    }

    pub(super) fn iter(&self) -> impl Iterator<Item = (&ViewportBinding, &BindingRetirement)> {
        self.entries.iter()
    }

    pub(super) fn retired_iter(
        &self,
    ) -> impl Iterator<Item = (&ViewportBinding, &BindingRetirement)> {
        self.entries
            .iter()
            .filter(|(_, retirement)| retirement.is_retired())
    }

    pub(super) fn keys(&self) -> impl Iterator<Item = &ViewportBinding> {
        self.entries.keys()
    }

    pub(super) fn values(&self) -> impl Iterator<Item = &BindingRetirement> {
        self.entries.values()
    }

    pub(super) fn len(&self) -> usize {
        self.entries.len()
    }

    pub(super) fn token_is_reserved(&self, token: WindowToken) -> bool {
        self.by_token.contains_key(&token)
    }

    pub(super) fn begin(
        &mut self,
        request: BindingCleanupRequest,
    ) -> Result<(), BindingCleanupError> {
        let binding = request.binding;
        let token = binding.token();
        if self.entries.contains_key(&binding) {
            return Err(BindingCleanupError::AlreadyExists { binding });
        }
        if self.by_token.contains_key(&token) {
            return Err(BindingCleanupError::TokenReserved { token });
        }
        if matches!(request.cleanup, BindingCleanupAction::KeepExternal)
            != (request.ownership == ViewportOwnership::External)
        {
            return Err(BindingCleanupError::CleanupOwnershipMismatch {
                binding,
                ownership: request.ownership,
            });
        }
        if let BindingCleanupPurpose::RecoveryReplacement {
            destroyed_binding,
            requirement,
            ..
        } = request.purpose
        {
            let conditional_request_matches = match requirement {
                RecoveryReplacementCleanupRequirement::ConditionalClose { requested, .. } => {
                    requested.binding() == binding
                        && requested.known_state() == Some(WindowCloseState::LiveRequested)
                }
                RecoveryReplacementCleanupRequirement::Mandatory => true,
            };
            if destroyed_binding.surface() != binding.surface()
                || !conditional_request_matches
                || request.ownership != ViewportOwnership::RuntimeOwned
                || !matches!(
                    request.cleanup,
                    BindingCleanupAction::CompensateCreate { .. }
                )
                || request.status.cleanup_effect().is_none()
            {
                return Err(BindingCleanupError::CleanupPurposeMismatch { binding });
            }
        }
        if let Some(effect) = request.status.cleanup_effect()
            && let Some(owner) = self.by_cleanup_effect.get(&effect).copied()
        {
            return Err(BindingCleanupError::CleanupEffectOwned { effect, owner });
        }

        if let Some(effect) = request.status.cleanup_effect() {
            self.by_cleanup_effect.insert(effect, binding);
        }
        self.by_token.insert(token, binding);
        self.entries.insert(binding, request.into());
        Ok(())
    }

    /// Updates binding-scoped provider streams and returns coordinator work without executing it.
    pub(super) fn observe_snapshot(
        &mut self,
        provider: PlatformObservationLease,
        snapshot: &PlatformSnapshot,
    ) -> Vec<BindingCleanupDirective> {
        let observations = snapshot
            .window_observations()
            .iter()
            .map(|window| (window.binding(), window))
            .collect::<BTreeMap<_, _>>();
        let close_observations = snapshot
            .close_observations()
            .iter()
            .copied()
            .map(|observation| (observation.binding(), observation))
            .collect::<BTreeMap<_, _>>();
        let bindings = self.entries.keys().copied().collect::<Vec<_>>();
        let mut directives = Vec::with_capacity(bindings.len());

        for binding in bindings {
            let destroyed = self.entries.get_mut(&binding).is_some_and(|retirement| {
                retirement
                    .close_observations
                    .observe(binding, close_observations.get(&binding).copied());
                retirement.is_retired()
                    && retirement
                        .close_observations
                        .current()
                        .is_some_and(|observation| {
                            observation.known_state() == Some(WindowCloseState::Destroyed)
                        })
            });
            if destroyed {
                if let Some(terminal) = self.finish_destroyed(binding, provider) {
                    directives.push(BindingCleanupDirective::ExactDestroyed(terminal));
                }
                continue;
            }
            if let Some(observation) = observations.get(&binding)
                && let Some(retirement) = self.entries.get_mut(&binding)
            {
                retirement.observed = true;
                retirement.may_reappear = false;
                retirement
                    .input_observations
                    .observe(binding, observation.input_observation());
            }
            directives.push(BindingCleanupDirective::Drive { binding });
        }

        directives
    }

    pub(super) fn recovery_cleanup(&self, binding: ViewportBinding) -> Option<&BindingRetirement> {
        self.entries
            .get(&binding)
            .filter(|retirement| !retirement.is_retired())
    }

    /// Reduces a newly accepted close edge into the cleanup which already owns this binding.
    ///
    /// The registry has already validated provider ordering before emitting this edge. A distinct
    /// request therefore rearms conditional cleanup to the newest exact edge and revokes any clear
    /// that belonged to the previous generation. Mandatory cleanup consumes later close requests
    /// without changing its terminal obligation.
    pub(super) fn reduce_recovery_cleanup_close_request(
        &mut self,
        requested: WindowCloseObservation,
    ) -> RecoveryCleanupCloseRequestOutcome {
        let Some(cleanup) = self.entries.get_mut(&requested.binding()) else {
            return RecoveryCleanupCloseRequestOutcome::NotOwned;
        };
        let BindingCleanupPurpose::RecoveryReplacement { requirement, .. } = &mut cleanup.purpose
        else {
            return RecoveryCleanupCloseRequestOutcome::NotOwned;
        };
        match requirement {
            RecoveryReplacementCleanupRequirement::Mandatory => {
                RecoveryCleanupCloseRequestOutcome::Mandatory
            }
            RecoveryReplacementCleanupRequirement::ConditionalClose {
                requested: owned,
                clear,
            } if *owned == requested => RecoveryCleanupCloseRequestOutcome::Duplicate,
            RecoveryReplacementCleanupRequirement::ConditionalClose {
                requested: owned,
                clear,
            } => {
                *owned = requested;
                *clear = None;
                RecoveryCleanupCloseRequestOutcome::Rearmed
            }
        }
    }

    pub(super) fn record_recovery_cleanup_clear(
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
        let Some(cleanup) = self.entries.get_mut(&observation.binding()) else {
            return false;
        };
        let BindingCleanupPurpose::RecoveryReplacement {
            requirement:
                RecoveryReplacementCleanupRequirement::ConditionalClose {
                    requested: owned,
                    clear,
                },
            ..
        } = &mut cleanup.purpose
        else {
            return false;
        };
        if *owned != requested {
            return false;
        }
        *clear = Some(observation);
        true
    }

    pub(super) fn recovery_cleanup_bindings_for_effect(
        &self,
        effect: EffectId,
    ) -> Vec<ViewportBinding> {
        self.by_cleanup_effect
            .get(&effect)
            .copied()
            .filter(|binding| {
                self.recovery_cleanup(*binding).is_some_and(|cleanup| {
                    matches!(
                        cleanup.recovery_cleanup_requirement(),
                        Some(RecoveryReplacementCleanupRequirement::ConditionalClose {
                            clear: Some(_),
                            ..
                        })
                    )
                })
            })
            .into_iter()
            .collect()
    }

    pub(super) fn recovery_cleanup_resume(
        &self,
        binding: ViewportBinding,
    ) -> Option<(EffectId, WindowCloseObservation)> {
        let cleanup = self.recovery_cleanup(binding)?;
        let RecoveryReplacementCleanupRequirement::ConditionalClose {
            clear: Some(clear), ..
        } = cleanup.recovery_cleanup_requirement()?
        else {
            return None;
        };
        Some((cleanup.status.cleanup_effect()?, clear))
    }

    pub(super) fn finish_recovery_cleanup_resume(
        &mut self,
        binding: ViewportBinding,
    ) -> Result<BindingCleanupTerminal, BindingCleanupError> {
        let cleanup = self
            .recovery_cleanup(binding)
            .ok_or(BindingCleanupError::CleanupPurposeMismatch { binding })?;
        if !matches!(
            cleanup.recovery_cleanup_requirement(),
            Some(RecoveryReplacementCleanupRequirement::ConditionalClose { clear: Some(_), .. })
        ) {
            return Err(BindingCleanupError::CleanupPurposeMismatch { binding });
        }
        self.finish(binding)
            .ok_or(BindingCleanupError::Missing { binding })
    }

    pub(super) fn promote_recovery_cleanup_to_mandatory(
        &mut self,
        binding: ViewportBinding,
    ) -> Result<EffectId, BindingCleanupError> {
        let cleanup = self
            .entries
            .get_mut(&binding)
            .ok_or(BindingCleanupError::Missing { binding })?;
        let BindingCleanupPurpose::RecoveryReplacement { requirement, .. } = &mut cleanup.purpose
        else {
            return Err(BindingCleanupError::CleanupPurposeMismatch { binding });
        };
        *requirement = RecoveryReplacementCleanupRequirement::Mandatory;
        cleanup
            .status
            .cleanup_effect()
            .ok_or(BindingCleanupError::CleanupPurposeMismatch { binding })
    }

    pub(super) fn promote_recovery_cleanup_to_retirement(
        &mut self,
        request: BindingCleanupRequest,
    ) -> Result<(), BindingCleanupError> {
        let binding = request.binding;
        if request.purpose != BindingCleanupPurpose::Retired {
            return Err(BindingCleanupError::CleanupPurposeMismatch { binding });
        }
        self.set_status(binding, request.status)?;
        let retirement = self
            .entries
            .get_mut(&binding)
            .ok_or(BindingCleanupError::Missing { binding })?;
        if retirement.is_retired() {
            return Err(BindingCleanupError::CleanupPurposeMismatch { binding });
        }
        retirement.role = request.role;
        retirement.ownership = request.ownership;
        retirement.origin = request.origin;
        retirement.observed = request.observed;
        retirement.input_observations = request.input_observations;
        retirement.close_observations = request.close_observations;
        retirement.may_reappear = request.may_reappear;
        retirement.cleanup = request.cleanup;
        retirement.retained_staging_resource = request.retained_staging_resource;
        retirement.purpose = BindingCleanupPurpose::Retired;
        Ok(())
    }

    /// Advances one retirement using only the external pointer-restore obligation fact.
    pub(super) fn plan_drive(
        &mut self,
        binding: ViewportBinding,
        pointer_restore_pending: bool,
    ) -> Result<Option<BindingCleanupDirective>, BindingCleanupError> {
        let retirement = self
            .entries
            .get(&binding)
            .ok_or(BindingCleanupError::Missing { binding })?;
        let ownership = retirement.ownership;
        let observed = retirement.observed;
        let may_reappear = retirement.may_reappear;
        let status = retirement.status;

        if pointer_restore_pending {
            // A cleanup effect which already owns the binding cannot be replaced by a
            // pointer-restore gate. The restore saga remains an independent obligation;
            // retaining this status preserves the destructive effect's sole owner.
            if status.cleanup_effect().is_none() {
                self.set_status(binding, BindingRetirementStatus::AwaitingInputRestore)?;
            }
            return Ok(None);
        }
        if ownership == ViewportOwnership::External {
            self.set_status(binding, BindingRetirementStatus::AwaitingExactDestruction)?;
            return Ok(None);
        }
        // Once a destructive request or its observation continuation owns this binding, an
        // absent inventory sample cannot revoke that cleanup lineage. The effect itself may be
        // what causes the window to appear before it is destroyed, so replacing this status with
        // `AwaitingAppearance` would orphan the exact terminal acknowledgement and permit a
        // duplicate destructive request.
        if status.cleanup_effect().is_some() {
            return Ok(None);
        }
        if !observed {
            if may_reappear {
                self.set_status(binding, BindingRetirementStatus::AwaitingAppearance)?;
                return Ok(None);
            }
            return Ok(self.finish(binding).map(BindingCleanupDirective::Finish));
        }
        Ok(Some(BindingCleanupDirective::RequestCleanup(
            self.cleanup_request(binding)?,
        )))
    }

    /// Records a newly minted destructive cleanup effect as the active lineage predecessor.
    pub(super) fn accept_cleanup_effect(
        &mut self,
        binding: ViewportBinding,
        effect: EffectId,
    ) -> Result<(), BindingCleanupError> {
        self.validate_cleanup_effect_owner(binding, effect)?;
        self.set_cleanup_predecessor(binding, effect)?;
        self.set_status(
            binding,
            BindingRetirementStatus::CleanupRequested { effect },
        )
    }

    /// Records a successor observation effect without retaining superseded intermediates.
    pub(super) fn accept_cleanup_observation_effect(
        &mut self,
        binding: ViewportBinding,
        effect: EffectId,
        predecessor: EffectId,
    ) -> Result<(), BindingCleanupError> {
        self.validate_cleanup_effect_owner(binding, effect)?;
        self.validate_cleanup_effect_owner(binding, predecessor)?;
        self.set_cleanup_predecessor(binding, predecessor)?;
        self.set_status(
            binding,
            BindingRetirementStatus::CleanupRequested { effect },
        )
    }

    /// Invalidates the old cleanup identity and plans a restore-time redispatch.
    pub(super) fn plan_cleanup_redispatch(
        &mut self,
        binding: ViewportBinding,
    ) -> Result<Option<BindingCleanupEffectRequest>, BindingCleanupError> {
        let observed = self
            .entries
            .get(&binding)
            .ok_or(BindingCleanupError::Missing { binding })?
            .observed;
        let request = (observed && self.cleanup_request_is_available(binding))
            .then(|| self.cleanup_request(binding))
            .transpose()?;
        self.set_status(binding, BindingRetirementStatus::AwaitingAppearance)?;
        Ok(request)
    }

    /// Reduces the current phase of an exact cleanup effect through the indexed owner.
    pub(super) fn reduce_effect(
        &mut self,
        effect: EffectId,
        phase: EffectPhase,
    ) -> Result<Option<ViewportBinding>, BindingCleanupError> {
        let Some(binding) = self.by_cleanup_effect.get(&effect).copied() else {
            return Ok(None);
        };
        let retirement = self
            .entries
            .get(&binding)
            .ok_or(BindingCleanupError::Missing { binding })?;
        let active = retirement.status.cleanup_effect();
        if active != Some(effect) {
            if retirement.cleanup_predecessor != Some(effect) {
                return Ok(None);
            }
            return match phase {
                EffectPhase::DispatchFailed(_) => {
                    self.set_status(binding, BindingRetirementStatus::CleanupFailed { effect })?;
                    Ok(Some(binding))
                }
                EffectPhase::Unsupported(reason) => {
                    self.set_status(
                        binding,
                        BindingRetirementStatus::CleanupBlocked { effect, reason },
                    )?;
                    Ok(Some(binding))
                }
                EffectPhase::Requested
                | EffectPhase::Indeterminate(_)
                | EffectPhase::ObservationDispatchFailed(_)
                | EffectPhase::ObservedApplied { .. }
                | EffectPhase::CleanupObservationIndeterminate { .. }
                | EffectPhase::CleanupResultObserved { .. }
                | EffectPhase::CleanupObservationSuperseded { .. }
                | EffectPhase::ObservationUnsupported(_)
                | EffectPhase::Destroyed { .. }
                | EffectPhase::Invalidated { .. } => Ok(Some(binding)),
            };
        }
        let status = match phase {
            EffectPhase::Indeterminate(_) => {
                BindingRetirementStatus::CleanupIndeterminate { effect }
            }
            EffectPhase::ObservationDispatchFailed(_) => {
                BindingRetirementStatus::CleanupObservationFailed { effect }
            }
            EffectPhase::DispatchFailed(_) => BindingRetirementStatus::CleanupFailed { effect },
            EffectPhase::Unsupported(reason) | EffectPhase::ObservationUnsupported(reason) => {
                BindingRetirementStatus::CleanupBlocked { effect, reason }
            }
            EffectPhase::Requested
            | EffectPhase::ObservedApplied { .. }
            | EffectPhase::CleanupObservationIndeterminate { .. }
            | EffectPhase::CleanupResultObserved { .. }
            | EffectPhase::CleanupObservationSuperseded { .. }
            | EffectPhase::Destroyed { .. }
            | EffectPhase::Invalidated { .. } => return Ok(Some(binding)),
        };
        self.set_status(binding, status)?;
        Ok(Some(binding))
    }

    /// Rebinds one active observation continuation to the destructive
    /// predecessor whose delayed result it authoritatively observed.
    pub(super) fn reduce_cleanup_observation(
        &mut self,
        continuation: EffectId,
        predecessor: EffectId,
        predecessor_phase: EffectPhase,
    ) -> Result<Option<ViewportBinding>, BindingCleanupError> {
        let Some(binding) = self.by_cleanup_effect.get(&continuation).copied() else {
            return Ok(None);
        };
        let active = self
            .entries
            .get(&binding)
            .ok_or(BindingCleanupError::Missing { binding })?
            .status
            .cleanup_effect();
        if active != Some(continuation) {
            return Err(BindingCleanupError::CleanupObservationMismatch {
                continuation,
                predecessor,
            });
        }
        let status = match predecessor_phase {
            EffectPhase::Indeterminate(_) => BindingRetirementStatus::CleanupIndeterminate {
                effect: continuation,
            },
            EffectPhase::DispatchFailed(_) => BindingRetirementStatus::CleanupFailed {
                effect: predecessor,
            },
            EffectPhase::Unsupported(reason) => BindingRetirementStatus::CleanupBlocked {
                effect: predecessor,
                reason,
            },
            EffectPhase::Requested
            | EffectPhase::ObservationDispatchFailed(_)
            | EffectPhase::ObservedApplied { .. }
            | EffectPhase::CleanupObservationIndeterminate { .. }
            | EffectPhase::CleanupResultObserved { .. }
            | EffectPhase::CleanupObservationSuperseded { .. }
            | EffectPhase::ObservationUnsupported(_)
            | EffectPhase::Destroyed { .. }
            | EffectPhase::Invalidated { .. } => {
                return Err(BindingCleanupError::CleanupObservationMismatch {
                    continuation,
                    predecessor,
                });
            }
        };
        self.set_status(binding, status)?;
        Ok(Some(binding))
    }

    pub(super) fn plan_destructive_retry(
        &self,
        failed_effect: EffectId,
    ) -> Result<Option<BindingCleanupEffectRequest>, BindingCleanupError> {
        let Some(binding) = self.by_cleanup_effect.get(&failed_effect).copied() else {
            return Ok(None);
        };
        let retirement = self
            .entries
            .get(&binding)
            .ok_or(BindingCleanupError::Missing { binding })?;
        if !matches!(
            retirement.status,
            BindingRetirementStatus::CleanupFailed { effect }
                | BindingRetirementStatus::CleanupBlocked { effect, .. }
                if effect == failed_effect
        ) {
            return Ok(None);
        }
        if !retirement.observed && !retirement.allows_unobserved_destructive_retry() {
            return Err(BindingCleanupError::CleanupWindowNotObserved {
                effect: failed_effect,
            });
        }
        self.cleanup_request(binding).map(Some)
    }

    pub(super) fn observation_retry_binding(
        &self,
        failed_effect: EffectId,
    ) -> Option<ViewportBinding> {
        let binding = self.by_cleanup_effect.get(&failed_effect).copied()?;
        let retirement = self.entries.get(&binding)?;
        matches!(
            retirement.status,
            BindingRetirementStatus::CleanupObservationFailed { effect }
                | BindingRetirementStatus::CleanupBlocked { effect, .. }
                if effect == failed_effect
        )
        .then_some(binding)
    }

    pub(super) fn finish_destroyed(
        &mut self,
        binding: ViewportBinding,
        provider: PlatformObservationLease,
    ) -> Option<BindingCleanupTerminal> {
        self.record_destroyed_tombstone(binding, provider);
        self.finish(binding)
    }

    pub(super) fn record_destroyed_tombstone(
        &mut self,
        binding: ViewportBinding,
        provider: PlatformObservationLease,
    ) {
        self.destroyed_tombstones.insert(binding);
        self.destroyed_tombstone_providers.insert(binding, provider);
    }

    /// Releases guards owned by one producer after its observation lane has quiesced.
    pub(super) fn compact_destroyed_tombstones_from(
        &mut self,
        provider: PlatformObservationLease,
    ) -> usize {
        let compacted = self
            .destroyed_tombstone_providers
            .iter()
            .filter_map(|(binding, owner)| (*owner == provider).then_some(*binding))
            .collect::<Vec<_>>();
        for binding in &compacted {
            self.destroyed_tombstone_providers.remove(binding);
            self.destroyed_tombstones.remove(binding);
        }
        compacted.len()
    }

    /// Releases one exact guard after its producer-bound ingress lane quiesces.
    pub(super) fn compact_destroyed_tombstone(
        &mut self,
        binding: ViewportBinding,
        provider: PlatformObservationLease,
    ) -> Result<(), BindingCleanupError> {
        let expected = self
            .destroyed_tombstone_providers
            .get(&binding)
            .copied()
            .ok_or(BindingCleanupError::DestroyedTombstoneMissing { binding })?;
        if expected != provider {
            return Err(BindingCleanupError::DestroyedTombstoneProviderMismatch {
                binding,
                expected,
                submitted: provider,
            });
        }
        self.destroyed_tombstone_providers.remove(&binding);
        self.destroyed_tombstones.remove(&binding);
        Ok(())
    }

    pub(super) fn destroyed_bindings(&self) -> &BTreeSet<ViewportBinding> {
        &self.destroyed_tombstones
    }

    pub(super) fn was_destroyed(&self, binding: ViewportBinding) -> bool {
        self.destroyed_tombstones.contains(&binding)
    }

    #[cfg(test)]
    pub(super) fn corrupt_binding_identity_for_test(
        &mut self,
        key: ViewportBinding,
        replacement: ViewportBinding,
    ) {
        if let Some(retirement) = self.entries.get_mut(&key) {
            retirement.binding = replacement;
        }
    }

    fn cleanup_request(
        &self,
        binding: ViewportBinding,
    ) -> Result<BindingCleanupEffectRequest, BindingCleanupError> {
        let retirement = self
            .entries
            .get(&binding)
            .ok_or(BindingCleanupError::Missing { binding })?;
        if matches!(retirement.cleanup, BindingCleanupAction::KeepExternal)
            != (retirement.ownership == ViewportOwnership::External)
        {
            return Err(BindingCleanupError::CleanupOwnershipMismatch {
                binding,
                ownership: retirement.ownership,
            });
        }
        match retirement.cleanup {
            BindingCleanupAction::CompensateCreate { create } => {
                Ok(BindingCleanupEffectRequest::CompensatingClose { binding, create })
            }
            BindingCleanupAction::ReleaseOwnedWindow => match retirement.role {
                ViewportRole::Root => Ok(BindingCleanupEffectRequest::RequestRootClose { binding }),
                ViewportRole::Child => Ok(BindingCleanupEffectRequest::ReleaseChild { binding }),
            },
            BindingCleanupAction::KeepExternal => {
                Err(BindingCleanupError::CleanupOwnershipMismatch {
                    binding,
                    ownership: retirement.ownership,
                })
            }
        }
    }

    fn cleanup_request_is_available(&self, binding: ViewportBinding) -> bool {
        self.entries.get(&binding).is_some_and(|retirement| {
            !matches!(retirement.cleanup, BindingCleanupAction::KeepExternal)
        })
    }

    fn set_status(
        &mut self,
        binding: ViewportBinding,
        status: BindingRetirementStatus,
    ) -> Result<(), BindingCleanupError> {
        let retirement = self
            .entries
            .get(&binding)
            .ok_or(BindingCleanupError::Missing { binding })?;
        let old_effect = retirement.status.cleanup_effect();
        let cleanup_predecessor = retirement.cleanup_predecessor;
        let new_effect = status.cleanup_effect();
        if let Some(effect) = new_effect
            && let Some(owner) = self.by_cleanup_effect.get(&effect).copied()
            && owner != binding
        {
            return Err(BindingCleanupError::CleanupEffectOwned { effect, owner });
        }
        if old_effect != new_effect
            && let Some(effect) = new_effect
        {
            self.by_cleanup_effect.insert(effect, binding);
        }
        if old_effect != new_effect
            && let Some(effect) = old_effect
            && Some(effect) != cleanup_predecessor
            && self.by_cleanup_effect.get(&effect) == Some(&binding)
        {
            self.by_cleanup_effect.remove(&effect);
        }
        self.entries
            .get_mut(&binding)
            .ok_or(BindingCleanupError::Missing { binding })?
            .status = status;
        Ok(())
    }

    fn set_cleanup_predecessor(
        &mut self,
        binding: ViewportBinding,
        predecessor: EffectId,
    ) -> Result<(), BindingCleanupError> {
        let retirement = self
            .entries
            .get(&binding)
            .ok_or(BindingCleanupError::Missing { binding })?;
        let old_predecessor = retirement.cleanup_predecessor;
        let active_effect = retirement.status.cleanup_effect();
        if let Some(owner) = self.by_cleanup_effect.get(&predecessor).copied()
            && owner != binding
        {
            return Err(BindingCleanupError::CleanupEffectOwned {
                effect: predecessor,
                owner,
            });
        }
        if old_predecessor == Some(predecessor) {
            return Ok(());
        }
        self.by_cleanup_effect.insert(predecessor, binding);
        if let Some(old) = old_predecessor
            && Some(old) != active_effect
            && self.by_cleanup_effect.get(&old) == Some(&binding)
        {
            self.by_cleanup_effect.remove(&old);
        }
        self.entries
            .get_mut(&binding)
            .ok_or(BindingCleanupError::Missing { binding })?
            .cleanup_predecessor = Some(predecessor);
        Ok(())
    }

    fn validate_cleanup_effect_owner(
        &self,
        binding: ViewportBinding,
        effect: EffectId,
    ) -> Result<(), BindingCleanupError> {
        if !self.entries.contains_key(&binding) {
            return Err(BindingCleanupError::Missing { binding });
        }
        if let Some(owner) = self.by_cleanup_effect.get(&effect).copied()
            && owner != binding
        {
            return Err(BindingCleanupError::CleanupEffectOwned { effect, owner });
        }
        Ok(())
    }

    fn finish(&mut self, binding: ViewportBinding) -> Option<BindingCleanupTerminal> {
        let retirement = self.entries.remove(&binding)?;
        if self.by_token.get(&binding.token()) == Some(&binding) {
            self.by_token.remove(&binding.token());
        }
        self.by_cleanup_effect.retain(|_, owner| *owner != binding);
        Some(BindingCleanupTerminal { retirement })
    }
}

#[cfg(test)]
mod tests;
