//! Exact native-binding retirement lifecycle and token quarantine.

use std::collections::{BTreeMap, BTreeSet};

use crate::effect::{EffectId, EffectPhase, EffectUnsupportedReason, PlatformEffect};
use crate::platform::{
    PlatformSnapshot, WindowCloseObservationStream, WindowCloseState, WindowInputObservationStream,
};
use crate::platform_provider::PlatformObservationLease;
use crate::presentation_observation::NativeStagingResourceId;
use crate::retention::BindingRetentionManifest;
use crate::viewport::{ViewportBinding, ViewportRole, WindowToken};
use crate::viewport_registry::ViewportOwnership;

/// Why an exact binding left the current logical surface roster.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BindingRetirementOrigin {
    NativeCreateAborted { create: EffectId },
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
    observed: bool,
    input_observations: WindowInputObservationStream,
    close_observations: WindowCloseObservationStream,
    may_reappear: bool,
    cleanup: BindingRetirementCleanup,
    retained_staging_resource: Option<NativeStagingResourceId>,
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
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum BindingRetirementCleanup {
    CompensateCreate { create: EffectId },
    ReleaseOwnedWindow,
    KeepExternal,
}

/// Typed input used to begin ownership of one exact retired binding.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct BindingRetirementRequest {
    pub(super) binding: ViewportBinding,
    pub(super) role: ViewportRole,
    pub(super) ownership: ViewportOwnership,
    pub(super) origin: BindingRetirementOrigin,
    pub(super) status: BindingRetirementStatus,
    pub(super) observed: bool,
    pub(super) input_observations: WindowInputObservationStream,
    pub(super) close_observations: WindowCloseObservationStream,
    pub(super) may_reappear: bool,
    pub(super) cleanup: BindingRetirementCleanup,
    pub(super) retained_staging_resource: Option<NativeStagingResourceId>,
}

impl From<BindingRetirementRequest> for BindingRetirement {
    fn from(request: BindingRetirementRequest) -> Self {
        Self {
            binding: request.binding,
            role: request.role,
            ownership: request.ownership,
            origin: request.origin,
            status: request.status,
            observed: request.observed,
            input_observations: request.input_observations,
            close_observations: request.close_observations,
            may_reappear: request.may_reappear,
            cleanup: request.cleanup,
            retained_staging_resource: request.retained_staging_resource,
        }
    }
}

/// Effect request emitted by the lifecycle without mutating the effect ledger.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum BindingRetirementCleanupRequest {
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

impl BindingRetirementCleanupRequest {
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
pub(super) struct BindingRetirementTerminal {
    retirement: BindingRetirement,
}

impl BindingRetirementTerminal {
    pub(super) const fn binding(&self) -> ViewportBinding {
        self.retirement.binding
    }

    pub(super) const fn cleanup_effect(&self) -> Option<EffectId> {
        self.retirement.status.cleanup_effect()
    }

    pub(super) const fn retained_staging_resource(&self) -> Option<NativeStagingResourceId> {
        self.retirement.retained_staging_resource
    }
}

/// Side effects that the lifecycle delegates back to the platform coordinator.
#[derive(Debug, Clone, PartialEq)]
pub(super) enum BindingRetirementDirective {
    Drive { binding: ViewportBinding },
    RequestCleanup(BindingRetirementCleanupRequest),
    Finish(BindingRetirementTerminal),
    ExactDestroyed(BindingRetirementTerminal),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum BindingRetirementLifecycleError {
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
    CleanupWindowNotObserved {
        effect: EffectId,
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

/// Sole owner of retired binding state, effect identity, token reservations, and tombstones.
#[derive(Debug, Clone, Default, PartialEq)]
pub(super) struct BindingRetirementLifecycle {
    retirements: BTreeMap<ViewportBinding, BindingRetirement>,
    by_token: BTreeMap<WindowToken, ViewportBinding>,
    by_cleanup_effect: BTreeMap<EffectId, ViewportBinding>,
    destroyed_tombstones: BTreeSet<ViewportBinding>,
    destroyed_tombstone_providers: BTreeMap<ViewportBinding, PlatformObservationLease>,
}

impl BindingRetirementLifecycle {
    pub(super) fn extend_referenced_effects(&self, effects: &mut BTreeSet<EffectId>) {
        for retirement in self.retirements.values() {
            if let BindingRetirementOrigin::NativeCreateAborted { create } = retirement.origin {
                effects.insert(create);
            }
            if let Some(effect) = retirement.status.cleanup_effect() {
                effects.insert(effect);
            }
            if let BindingRetirementCleanup::CompensateCreate { create } = retirement.cleanup {
                effects.insert(create);
            }
        }
        effects.extend(self.by_cleanup_effect.keys().copied());
    }

    /// Accounts for every active retirement, quarantine index, cleanup lineage, and tombstone.
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
            self.retirements.len(),
            self.by_token.len(),
            self.by_cleanup_effect.len(),
            self.destroyed_tombstones.len(),
        )
    }

    pub(super) fn contains_key(&self, binding: &ViewportBinding) -> bool {
        self.retirements.contains_key(binding)
    }

    pub(super) fn get(&self, binding: &ViewportBinding) -> Option<&BindingRetirement> {
        self.retirements.get(binding)
    }

    pub(super) fn iter(&self) -> impl Iterator<Item = (&ViewportBinding, &BindingRetirement)> {
        self.retirements.iter()
    }

    pub(super) fn keys(&self) -> impl Iterator<Item = &ViewportBinding> {
        self.retirements.keys()
    }

    pub(super) fn values(&self) -> impl Iterator<Item = &BindingRetirement> {
        self.retirements.values()
    }

    pub(super) fn len(&self) -> usize {
        self.retirements.len()
    }

    pub(super) fn token_is_reserved(&self, token: WindowToken) -> bool {
        self.by_token.contains_key(&token)
    }

    pub(super) fn begin(
        &mut self,
        request: BindingRetirementRequest,
    ) -> Result<(), BindingRetirementLifecycleError> {
        let binding = request.binding;
        let token = binding.token();
        if self.retirements.contains_key(&binding) {
            return Err(BindingRetirementLifecycleError::AlreadyExists { binding });
        }
        if self.by_token.contains_key(&token) {
            return Err(BindingRetirementLifecycleError::TokenReserved { token });
        }
        if matches!(request.cleanup, BindingRetirementCleanup::KeepExternal)
            != (request.ownership == ViewportOwnership::External)
        {
            return Err(BindingRetirementLifecycleError::CleanupOwnershipMismatch {
                binding,
                ownership: request.ownership,
            });
        }
        if let Some(effect) = request.status.cleanup_effect()
            && let Some(owner) = self.by_cleanup_effect.get(&effect).copied()
        {
            return Err(BindingRetirementLifecycleError::CleanupEffectOwned { effect, owner });
        }

        if let Some(effect) = request.status.cleanup_effect() {
            self.by_cleanup_effect.insert(effect, binding);
        }
        self.by_token.insert(token, binding);
        self.retirements.insert(binding, request.into());
        Ok(())
    }

    /// Updates binding-scoped provider streams and returns coordinator work without executing it.
    pub(super) fn observe_snapshot(
        &mut self,
        provider: PlatformObservationLease,
        snapshot: &PlatformSnapshot,
    ) -> Vec<BindingRetirementDirective> {
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
        let bindings = self.retirements.keys().copied().collect::<Vec<_>>();
        let mut directives = Vec::with_capacity(bindings.len());

        for binding in bindings {
            let destroyed = self
                .retirements
                .get_mut(&binding)
                .is_some_and(|retirement| {
                    retirement
                        .close_observations
                        .observe(binding, close_observations.get(&binding).copied());
                    retirement
                        .close_observations
                        .current()
                        .is_some_and(|observation| {
                            observation.known_state() == Some(WindowCloseState::Destroyed)
                        })
                });
            if destroyed {
                if let Some(terminal) = self.finish_destroyed(binding, provider) {
                    directives.push(BindingRetirementDirective::ExactDestroyed(terminal));
                }
                continue;
            }
            if let Some(observation) = observations.get(&binding)
                && let Some(retirement) = self.retirements.get_mut(&binding)
            {
                retirement.observed = true;
                retirement.may_reappear = false;
                retirement
                    .input_observations
                    .observe(binding, observation.input_observation());
            }
            directives.push(BindingRetirementDirective::Drive { binding });
        }

        directives
    }

    /// Advances one retirement using only the external pointer-restore obligation fact.
    pub(super) fn plan_drive(
        &mut self,
        binding: ViewportBinding,
        pointer_restore_pending: bool,
    ) -> Result<Option<BindingRetirementDirective>, BindingRetirementLifecycleError> {
        let retirement = self
            .retirements
            .get(&binding)
            .ok_or(BindingRetirementLifecycleError::Missing { binding })?;
        let ownership = retirement.ownership;
        let observed = retirement.observed;
        let may_reappear = retirement.may_reappear;
        let status = retirement.status;

        if pointer_restore_pending {
            self.set_status(binding, BindingRetirementStatus::AwaitingInputRestore)?;
            return Ok(None);
        }
        if ownership == ViewportOwnership::External {
            self.set_status(binding, BindingRetirementStatus::AwaitingExactDestruction)?;
            return Ok(None);
        }
        if !observed {
            if may_reappear {
                self.set_status(binding, BindingRetirementStatus::AwaitingAppearance)?;
                return Ok(None);
            }
            return Ok(self.finish(binding).map(BindingRetirementDirective::Finish));
        }
        if status.cleanup_effect().is_some() {
            return Ok(None);
        }
        Ok(Some(BindingRetirementDirective::RequestCleanup(
            self.cleanup_request(binding)?,
        )))
    }

    /// Records the exact effect minted from a prior cleanup directive.
    pub(super) fn accept_cleanup_effect(
        &mut self,
        binding: ViewportBinding,
        effect: EffectId,
    ) -> Result<(), BindingRetirementLifecycleError> {
        self.set_status(
            binding,
            BindingRetirementStatus::CleanupRequested { effect },
        )
    }

    /// Invalidates the old cleanup identity and plans a restore-time redispatch.
    pub(super) fn plan_cleanup_redispatch(
        &mut self,
        binding: ViewportBinding,
    ) -> Result<Option<BindingRetirementCleanupRequest>, BindingRetirementLifecycleError> {
        let observed = self
            .retirements
            .get(&binding)
            .ok_or(BindingRetirementLifecycleError::Missing { binding })?
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
    ) -> Result<Option<ViewportBinding>, BindingRetirementLifecycleError> {
        let Some(binding) = self.by_cleanup_effect.get(&effect).copied() else {
            return Ok(None);
        };
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
            | EffectPhase::Destroyed { .. }
            | EffectPhase::Invalidated { .. } => return Ok(Some(binding)),
        };
        self.set_status(binding, status)?;
        Ok(Some(binding))
    }

    pub(super) fn plan_destructive_retry(
        &self,
        failed_effect: EffectId,
    ) -> Result<Option<BindingRetirementCleanupRequest>, BindingRetirementLifecycleError> {
        let Some(binding) = self.by_cleanup_effect.get(&failed_effect).copied() else {
            return Ok(None);
        };
        let retirement = self
            .retirements
            .get(&binding)
            .ok_or(BindingRetirementLifecycleError::Missing { binding })?;
        if !matches!(
            retirement.status,
            BindingRetirementStatus::CleanupFailed { effect }
                | BindingRetirementStatus::CleanupBlocked { effect, .. }
                if effect == failed_effect
        ) {
            return Ok(None);
        }
        if !retirement.observed {
            return Err(BindingRetirementLifecycleError::CleanupWindowNotObserved {
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
        let retirement = self.retirements.get(&binding)?;
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
    ) -> Option<BindingRetirementTerminal> {
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
    ) -> Result<(), BindingRetirementLifecycleError> {
        let expected = self
            .destroyed_tombstone_providers
            .get(&binding)
            .copied()
            .ok_or(BindingRetirementLifecycleError::DestroyedTombstoneMissing { binding })?;
        if expected != provider {
            return Err(
                BindingRetirementLifecycleError::DestroyedTombstoneProviderMismatch {
                    binding,
                    expected,
                    submitted: provider,
                },
            );
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
        if let Some(retirement) = self.retirements.get_mut(&key) {
            retirement.binding = replacement;
        }
    }

    fn cleanup_request(
        &self,
        binding: ViewportBinding,
    ) -> Result<BindingRetirementCleanupRequest, BindingRetirementLifecycleError> {
        let retirement = self
            .retirements
            .get(&binding)
            .ok_or(BindingRetirementLifecycleError::Missing { binding })?;
        if matches!(retirement.cleanup, BindingRetirementCleanup::KeepExternal)
            != (retirement.ownership == ViewportOwnership::External)
        {
            return Err(BindingRetirementLifecycleError::CleanupOwnershipMismatch {
                binding,
                ownership: retirement.ownership,
            });
        }
        match retirement.cleanup {
            BindingRetirementCleanup::CompensateCreate { create } => {
                Ok(BindingRetirementCleanupRequest::CompensatingClose { binding, create })
            }
            BindingRetirementCleanup::ReleaseOwnedWindow => match retirement.role {
                ViewportRole::Root => {
                    Ok(BindingRetirementCleanupRequest::RequestRootClose { binding })
                }
                ViewportRole::Child => {
                    Ok(BindingRetirementCleanupRequest::ReleaseChild { binding })
                }
            },
            BindingRetirementCleanup::KeepExternal => {
                Err(BindingRetirementLifecycleError::CleanupOwnershipMismatch {
                    binding,
                    ownership: retirement.ownership,
                })
            }
        }
    }

    fn cleanup_request_is_available(&self, binding: ViewportBinding) -> bool {
        self.retirements.get(&binding).is_some_and(|retirement| {
            !matches!(retirement.cleanup, BindingRetirementCleanup::KeepExternal)
        })
    }

    fn set_status(
        &mut self,
        binding: ViewportBinding,
        status: BindingRetirementStatus,
    ) -> Result<(), BindingRetirementLifecycleError> {
        let old_effect = self
            .retirements
            .get(&binding)
            .ok_or(BindingRetirementLifecycleError::Missing { binding })?
            .status
            .cleanup_effect();
        let new_effect = status.cleanup_effect();
        if let Some(effect) = new_effect
            && let Some(owner) = self.by_cleanup_effect.get(&effect).copied()
            && owner != binding
        {
            return Err(BindingRetirementLifecycleError::CleanupEffectOwned { effect, owner });
        }
        if old_effect != new_effect
            && let Some(effect) = new_effect
        {
            self.by_cleanup_effect.insert(effect, binding);
        }
        self.retirements
            .get_mut(&binding)
            .ok_or(BindingRetirementLifecycleError::Missing { binding })?
            .status = status;
        Ok(())
    }

    fn finish(&mut self, binding: ViewportBinding) -> Option<BindingRetirementTerminal> {
        let retirement = self.retirements.remove(&binding)?;
        if self.by_token.get(&binding.token()) == Some(&binding) {
            self.by_token.remove(&binding.token());
        }
        self.by_cleanup_effect.retain(|_, owner| *owner != binding);
        Some(BindingRetirementTerminal { retirement })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::effect::DispatchFailureReason;
    use crate::ids::{EngineAuthorityDomainId, SurfaceId, WorkspaceEpoch};
    use crate::platform_provider::PlatformObservationAuthority;
    use crate::viewport::WindowIncarnation;

    fn binding(surface: u64, token: u64) -> ViewportBinding {
        ViewportBinding::new(
            EngineAuthorityDomainId::new_for_test(1),
            WorkspaceEpoch::new(0),
            SurfaceId::new(surface),
            WindowToken::new(token),
            WindowIncarnation::new(1),
        )
    }

    fn runtime_request(
        binding: ViewportBinding,
        status: BindingRetirementStatus,
    ) -> BindingRetirementRequest {
        BindingRetirementRequest {
            binding,
            role: ViewportRole::Child,
            ownership: ViewportOwnership::RuntimeOwned,
            origin: BindingRetirementOrigin::SurfaceVacated,
            status,
            observed: true,
            input_observations: WindowInputObservationStream::default(),
            close_observations: WindowCloseObservationStream::default(),
            may_reappear: false,
            cleanup: BindingRetirementCleanup::ReleaseOwnedWindow,
            retained_staging_resource: None,
        }
    }

    fn provider_pair() -> (PlatformObservationLease, PlatformObservationLease) {
        let mut authority =
            PlatformObservationAuthority::new(EngineAuthorityDomainId::new_for_test(1));
        let first = authority.create().expect("first provider must enroll");
        let ticket = authority
            .begin_replacement(first)
            .expect("first provider replacement must begin");
        let second = authority
            .finish_replacement(ticket)
            .expect("successor provider must activate");
        (first, second)
    }

    #[test]
    fn quiesced_producer_compacts_only_its_destroyed_binding_guards() {
        let mut lifecycle = BindingRetirementLifecycle::default();
        let (first, second) = provider_pair();

        for token in 1..=10_000 {
            let provider = if token <= 6_000 { first } else { second };
            lifecycle.record_destroyed_tombstone(binding(1, token), provider);
        }

        let retention = lifecycle.retention_manifest();
        assert_eq!(retention.active_retirements(), 0);
        assert_eq!(retention.token_index_entries(), 0);
        assert_eq!(retention.cleanup_lineage_entries(), 0);
        assert_eq!(retention.destroyed_binding_guards(), 10_000);
        assert_eq!(retention.retained_structure_count(), 10_000);
        assert_eq!(
            retention.terminal_release_barrier(),
            Some(
                crate::retention::RuntimeRetentionReleaseBarrier::PlatformObservationIngressQuiesced
            ),
        );

        assert_eq!(lifecycle.compact_destroyed_tombstones_from(first), 6_000);
        assert_eq!(
            lifecycle.retention_manifest().destroyed_binding_guards(),
            4_000
        );
        assert!(lifecycle.was_destroyed(binding(1, 6_001)));
        assert!(!lifecycle.was_destroyed(binding(1, 6_000)));

        assert_eq!(lifecycle.compact_destroyed_tombstones_from(first), 0);
        assert_eq!(lifecycle.compact_destroyed_tombstones_from(second), 4_000);
        assert_eq!(lifecycle.retention_manifest().destroyed_binding_guards(), 0);
        assert_eq!(
            lifecycle.retention_manifest().terminal_release_barrier(),
            None
        );
    }

    #[test]
    fn exact_quiescence_compacts_same_provider_guards_without_cross_binding_fallthrough() {
        let mut lifecycle = BindingRetirementLifecycle::default();
        let (provider, foreign_provider) = provider_pair();
        for token in 1..=10_000 {
            lifecycle.record_destroyed_tombstone(binding(1, token), provider);
        }

        let first = binding(1, 1);
        assert_eq!(
            lifecycle
                .compact_destroyed_tombstone(first, foreign_provider)
                .expect_err("a foreign provider cannot compact the exact guard"),
            BindingRetirementLifecycleError::DestroyedTombstoneProviderMismatch {
                binding: first,
                expected: provider,
                submitted: foreign_provider,
            }
        );
        assert!(lifecycle.was_destroyed(first));

        for token in 1..=10_000 {
            lifecycle
                .compact_destroyed_tombstone(binding(1, token), provider)
                .expect("each exact producer proof must compact one guard");
        }
        assert_eq!(lifecycle.retention_manifest().destroyed_binding_guards(), 0);
        assert_eq!(
            lifecycle
                .compact_destroyed_tombstone(first, provider)
                .expect_err("one exact proof cannot compact twice"),
            BindingRetirementLifecycleError::DestroyedTombstoneMissing { binding: first },
        );
    }

    #[test]
    fn retention_accounts_for_active_quarantine_and_cleanup_indexes() {
        let binding = binding(1, 10);
        let mut lifecycle = BindingRetirementLifecycle::default();
        lifecycle
            .begin(runtime_request(
                binding,
                BindingRetirementStatus::CleanupRequested {
                    effect: EffectId::new(7),
                },
            ))
            .expect("retirement must begin");

        let retention = lifecycle.retention_manifest();
        assert_eq!(retention.active_retirements(), 1);
        assert_eq!(retention.token_index_entries(), 1);
        assert_eq!(retention.cleanup_lineage_entries(), 1);
        assert_eq!(retention.destroyed_binding_guards(), 0);
        assert_eq!(retention.retained_structure_count(), 3);
        assert_eq!(retention.terminal_release_barrier(), None);
    }

    #[test]
    fn cleanup_lineage_keeps_emitted_predecessors_indexed_until_terminal() {
        let binding = binding(1, 10);
        let predecessor = EffectId::new(1);
        let successor = EffectId::new(2);
        let mut lifecycle = BindingRetirementLifecycle::default();
        lifecycle
            .begin(runtime_request(
                binding,
                BindingRetirementStatus::CleanupRequested {
                    effect: predecessor,
                },
            ))
            .expect("retirement must begin");
        lifecycle
            .accept_cleanup_effect(binding, successor)
            .expect("cleanup continuation must become current");

        assert_eq!(
            lifecycle
                .reduce_effect(
                    predecessor,
                    EffectPhase::DispatchFailed(DispatchFailureReason::ProviderStopped),
                )
                .expect("late predecessor result must reduce"),
            Some(binding)
        );
        assert_eq!(
            lifecycle.get(&binding).map(BindingRetirement::status),
            Some(BindingRetirementStatus::CleanupFailed {
                effect: predecessor,
            })
        );
        assert_eq!(
            lifecycle
                .reduce_effect(
                    successor,
                    EffectPhase::DispatchFailed(DispatchFailureReason::ProviderStopped),
                )
                .expect("successor identity must remain in the same cleanup lineage"),
            Some(binding)
        );
    }

    #[test]
    fn terminal_destruction_releases_token_and_complete_cleanup_lineage() {
        let binding = binding(1, 10);
        let (provider, _) = provider_pair();
        let predecessor = EffectId::new(1);
        let successor = EffectId::new(2);
        let mut lifecycle = BindingRetirementLifecycle::default();
        lifecycle
            .begin(runtime_request(
                binding,
                BindingRetirementStatus::CleanupRequested {
                    effect: predecessor,
                },
            ))
            .expect("retirement must begin");
        lifecycle
            .accept_cleanup_effect(binding, successor)
            .expect("cleanup continuation must become current");

        let terminal = lifecycle
            .finish_destroyed(binding, provider)
            .expect("exact destruction must finish the retirement");
        assert_eq!(terminal.binding(), binding);
        assert!(lifecycle.was_destroyed(binding));
        assert!(!lifecycle.token_is_reserved(binding.token()));
        assert_eq!(
            lifecycle
                .reduce_effect(
                    predecessor,
                    EffectPhase::DispatchFailed(DispatchFailureReason::ProviderStopped),
                )
                .expect("terminal lineage lookup is infallible"),
            None
        );
        assert_eq!(
            lifecycle
                .reduce_effect(
                    successor,
                    EffectPhase::DispatchFailed(DispatchFailureReason::ProviderStopped),
                )
                .expect("terminal successor lookup is infallible"),
            None
        );
    }

    #[test]
    fn begin_rejects_token_and_cleanup_effect_aliases() {
        let first = binding(1, 10);
        let same_token = binding(2, 10);
        let other = binding(3, 11);
        let effect = EffectId::new(1);
        let status = BindingRetirementStatus::CleanupRequested { effect };
        let mut lifecycle = BindingRetirementLifecycle::default();
        lifecycle
            .begin(runtime_request(first, status))
            .expect("first retirement must begin");

        assert_eq!(
            lifecycle.begin(runtime_request(same_token, status)),
            Err(BindingRetirementLifecycleError::TokenReserved {
                token: first.token(),
            })
        );
        assert_eq!(
            lifecycle.begin(runtime_request(other, status)),
            Err(BindingRetirementLifecycleError::CleanupEffectOwned {
                effect,
                owner: first,
            })
        );
    }
}
