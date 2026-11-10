//! Exact revocable host readiness before semantic first-live publication.

use super::{ViewportCoordinator, ViewportCoordinatorError};
use crate::effect::{EffectId, EffectPhase, EffectTransition};
use crate::platform::{
    NativeCreateReadiness, WindowPresentationObservation, WindowPresentationState,
};
use crate::platform_provider::PlatformObservationLease;
use crate::presentation_observation::{NativeStagingOwner, NativeStagingResourceId};
use crate::viewport::{
    CoordinateGeneration, CoordinateObservationGeneration, InventoryGeneration, ViewportBinding,
};

/// The sole host preparation contract: a binding awaits an exact Prepared observation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct NativePreparation {
    pub(super) create: EffectId,
}

impl NativePreparation {
    pub(crate) const fn initial(create: EffectId) -> Self {
        Self { create }
    }
    pub(crate) const fn create_effect(self) -> EffectId {
        self.create
    }
}

/// Compact uninterrupted readiness provenance retained with prospective content.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct NativeReadyBasis {
    coordinate_generation: CoordinateGeneration,
    prepared_since: CoordinateObservationGeneration,
}

#[cfg(test)]
impl NativeReadyBasis {
    pub(crate) const fn for_test() -> Self {
        Self {
            coordinate_generation: CoordinateGeneration::new(1),
            prepared_since: CoordinateObservationGeneration::new(1),
        }
    }
}

/// Core verifies the host's readiness and create causality, never its preparation script.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct NativeReadyProof {
    binding: ViewportBinding,
    owner: NativeStagingOwner,
    provider: PlatformObservationLease,
    create: EffectId,
    create_emitted: InventoryGeneration,
    create_applied: InventoryGeneration,
    basis: NativeReadyBasis,
}

impl NativeReadyProof {
    #[cfg(test)]
    pub(crate) const fn for_test(
        binding: ViewportBinding,
        owner: NativeStagingOwner,
        provider: PlatformObservationLease,
    ) -> Self {
        Self {
            binding,
            owner,
            provider,
            create: EffectId::new(7),
            create_emitted: InventoryGeneration::new(1),
            create_applied: InventoryGeneration::new(2),
            basis: NativeReadyBasis::for_test(),
        }
    }

    pub(crate) const fn binding(self) -> ViewportBinding {
        self.binding
    }
    pub(crate) const fn create(self) -> EffectId {
        self.create
    }
    pub(crate) const fn owner(self) -> NativeStagingOwner {
        self.owner
    }
    pub(crate) const fn retained_resource(self) -> Option<NativeStagingResourceId> {
        self.owner.retained_resource()
    }
    pub(crate) const fn basis(self) -> NativeReadyBasis {
        self.basis
    }
    pub(super) const fn resume_preparation(self) -> NativePreparation {
        NativePreparation::initial(self.create)
    }
}

pub(super) enum NativePreparationOutcome {
    Ignored,
    Ready(NativeReadyProof),
}

impl ViewportCoordinator {
    /// Creation acknowledgement is independent of visibility and renderer preparation.
    fn settle_native_create_preparation(
        &mut self,
        provider: PlatformObservationLease,
        binding: ViewportBinding,
        create: EffectId,
    ) -> Option<InventoryGeneration> {
        if let Some(applied) = self
            .effects
            .record(create)
            .and_then(|record| match record.phase() {
                EffectPhase::ObservedApplied {
                    inventory_generation,
                } if record.provider() == Some(provider) => Some(inventory_generation),
                _ => None,
            })
        {
            return Some(applied);
        }
        let preparation = self
            .registry
            .record(binding.surface())
            .filter(|record| record.binding() == binding)?
            .create_preparation()?;
        let emitted = self.effect_emission_fence(create, binding)?;
        if preparation.known_readiness().is_none()
            || preparation.acknowledged_create() != Some(create)
            || preparation.inventory_generation() <= emitted
            || self.effects.mark_observed_applied(
                provider,
                create,
                binding,
                preparation.inventory_generation(),
            ) != EffectTransition::Applied
        {
            return None;
        }
        Some(preparation.inventory_generation())
    }

    pub(crate) fn native_ready_proof_is_current(&self, proof: NativeReadyProof) -> bool {
        let Some(record) = self
            .registry
            .record(proof.binding.surface())
            .filter(|record| record.binding() == proof.binding)
        else {
            return false;
        };
        record.is_ready()
            && record.coordinates().is_some_and(|coordinates| {
                coordinates.content_size().width() > 0.0 && coordinates.content_size().height() > 0.0
            })
            && record.coordinate_generation() == proof.basis.coordinate_generation
            && record.presentation_observation().and_then(WindowPresentationObservation::known_state)
                == Some(WindowPresentationState::Visible)
            && proof.retained_resource().is_none_or(|resource| self.native_staging_resources.get(resource).is_some())
            && self.platform_provider() == Some(proof.provider)
            && record.renderer_prepared_since() == Some(proof.basis.prepared_since)
            && record.create_preparation().and_then(|preparation| preparation.known_readiness())
                == Some(NativeCreateReadiness::Prepared)
            && self.effect_emission_fence(proof.create, proof.binding) == Some(proof.create_emitted)
            && proof.create_applied > proof.create_emitted
            && self.effects.record(proof.create).is_some_and(|effect| {
                effect.provider() == Some(proof.provider)
                    && matches!(effect.phase(), EffectPhase::ObservedApplied { inventory_generation }
                        if inventory_generation == proof.create_applied)
            })
    }

    pub(super) fn reduce_ready_native_preparation(
        &mut self,
        provider: PlatformObservationLease,
        binding: ViewportBinding,
        owner: NativeStagingOwner,
        preparation: NativePreparation,
    ) -> Result<NativePreparationOutcome, ViewportCoordinatorError> {
        let create = preparation.create;
        let Some(create_applied) = self.settle_native_create_preparation(provider, binding, create)
        else {
            return Ok(NativePreparationOutcome::Ignored);
        };
        let Some(create_emitted) = self.effect_emission_fence(create, binding) else {
            return Ok(NativePreparationOutcome::Ignored);
        };
        let Some(record) = self
            .registry
            .record(binding.surface())
            .filter(|record| record.binding() == binding)
        else {
            return Ok(NativePreparationOutcome::Ignored);
        };
        let Some(prepared_since) = record.renderer_prepared_since() else {
            return Ok(NativePreparationOutcome::Ignored);
        };
        let proof = NativeReadyProof {
            binding,
            owner,
            provider,
            create,
            create_emitted,
            create_applied,
            basis: NativeReadyBasis {
                coordinate_generation: record.coordinate_generation(),
                prepared_since,
            },
        };
        Ok(if self.native_ready_proof_is_current(proof) {
            NativePreparationOutcome::Ready(proof)
        } else {
            NativePreparationOutcome::Ignored
        })
    }

    pub(super) fn effect_emission_fence(
        &self,
        effect: EffectId,
        binding: ViewportBinding,
    ) -> Option<InventoryGeneration> {
        self.effects.record(effect).and_then(|record| {
            (record.request().effect().binding() == binding)
                .then(|| record.emitted_inventory_generation())
                .flatten()
        })
    }
}
