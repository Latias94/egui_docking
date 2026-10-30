//! Shared native-window and renderer preparation before exact first-live publication.

use crate::effect::{EffectId, EffectPhase, EffectTransition, PlatformEffect};
use crate::platform::{
    NativeCreateReadiness, NativeCreateStrategy, WindowPresentationObservation,
    WindowPresentationState,
};
use crate::platform_provider::PlatformObservationLease;
use crate::presentation_observation::{
    NativeStagingBasis, NativeStagingOwner, NativeStagingPresentation,
    NativeStagingPresentationPhase, NativeStagingResourceId, PresentedNativeStagingPresentation,
};
use crate::viewport::{
    CoordinateGeneration, CoordinateObservationGeneration, InventoryGeneration,
    PresentationObservationGeneration, ViewportBinding,
};

use super::{ViewportCoordinator, ViewportCoordinatorError};

/// Shared platform and presentation phase of one native bringup.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum NativeBringupPhase {
    AwaitingPrepared {
        create: EffectId,
    },
    AwaitingHidden {
        create: EffectId,
    },
    AwaitingPreShowPresentation {
        create: EffectId,
        hidden_generation: PresentationObservationGeneration,
        hidden_inventory_generation: InventoryGeneration,
        hidden_coordinate_generation: CoordinateGeneration,
        hidden_coordinate_observation_generation: CoordinateObservationGeneration,
        create_emitted_inventory_generation: InventoryGeneration,
        presentation: NativeStagingPresentation,
    },
    AwaitingShowAcknowledgement {
        create: EffectId,
        show: EffectId,
        hidden_generation: PresentationObservationGeneration,
        hidden_inventory_generation: InventoryGeneration,
        hidden_coordinate_generation: CoordinateGeneration,
        hidden_coordinate_observation_generation: CoordinateObservationGeneration,
        create_emitted_inventory_generation: InventoryGeneration,
        pre_show: PresentedNativeStagingPresentation,
    },
    AwaitingVisible {
        create: EffectId,
        show: EffectId,
        hidden_generation: PresentationObservationGeneration,
        hidden_inventory_generation: InventoryGeneration,
        hidden_coordinate_generation: CoordinateGeneration,
        hidden_coordinate_observation_generation: CoordinateObservationGeneration,
        acknowledged_generation: PresentationObservationGeneration,
        acknowledged_inventory_generation: InventoryGeneration,
        acknowledged_coordinate_generation: CoordinateGeneration,
        acknowledged_coordinate_observation_generation: CoordinateObservationGeneration,
        create_emitted_inventory_generation: InventoryGeneration,
        show_emitted_inventory_generation: InventoryGeneration,
        pre_show: PresentedNativeStagingPresentation,
    },
    AwaitingPostShowPresentation {
        visibility: NativeVisibilityProof,
        presentation: NativeStagingPresentation,
    },
}

impl NativeBringupPhase {
    pub(crate) const fn initial(create: EffectId, strategy: NativeCreateStrategy) -> Self {
        match strategy {
            NativeCreateStrategy::DirectPrepared => Self::AwaitingPrepared { create },
            NativeCreateStrategy::Staged => Self::AwaitingHidden { create },
        }
    }

    pub(crate) const fn create_effect(self) -> EffectId {
        match self {
            Self::AwaitingPrepared { create }
            | Self::AwaitingHidden { create }
            | Self::AwaitingPreShowPresentation { create, .. }
            | Self::AwaitingShowAcknowledgement { create, .. }
            | Self::AwaitingVisible { create, .. } => create,
            Self::AwaitingPostShowPresentation { visibility, .. } => visibility.create,
        }
    }

    pub(crate) fn show_effect(self) -> Option<EffectId> {
        match self {
            Self::AwaitingShowAcknowledgement { show, .. } | Self::AwaitingVisible { show, .. } => {
                Some(show)
            }
            Self::AwaitingPostShowPresentation { visibility, .. } => Some(visibility.show),
            Self::AwaitingPrepared { .. }
            | Self::AwaitingHidden { .. }
            | Self::AwaitingPreShowPresentation { .. } => None,
        }
    }

    pub(crate) const fn staging_presentation(self) -> Option<NativeStagingPresentation> {
        match self {
            Self::AwaitingPreShowPresentation { presentation, .. }
            | Self::AwaitingPostShowPresentation { presentation, .. } => Some(presentation),
            Self::AwaitingPrepared { .. }
            | Self::AwaitingHidden { .. }
            | Self::AwaitingShowAcknowledgement { .. }
            | Self::AwaitingVisible { .. } => None,
        }
    }
}

/// Exact platform visibility proof retained while post-show staging is pending.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct NativeVisibilityProof {
    pub(super) binding: ViewportBinding,
    pub(super) create: EffectId,
    pub(super) show: EffectId,
    pub(super) hidden_generation: PresentationObservationGeneration,
    pub(super) hidden_inventory_generation: InventoryGeneration,
    pub(super) hidden_coordinate_generation: CoordinateGeneration,
    pub(super) hidden_coordinate_observation_generation: CoordinateObservationGeneration,
    pub(super) acknowledged_generation: PresentationObservationGeneration,
    pub(super) acknowledged_inventory_generation: InventoryGeneration,
    pub(super) acknowledged_coordinate_generation: CoordinateGeneration,
    pub(super) acknowledged_coordinate_observation_generation: CoordinateObservationGeneration,
    pub(super) visible_generation: PresentationObservationGeneration,
    pub(super) visible_inventory_generation: InventoryGeneration,
    pub(super) visible_coordinate_generation: CoordinateGeneration,
    pub(super) visible_coordinate_observation_generation: CoordinateObservationGeneration,
    pub(super) create_emitted_inventory_generation: InventoryGeneration,
    pub(super) show_emitted_inventory_generation: InventoryGeneration,
    pub(super) pre_show: PresentedNativeStagingPresentation,
}

/// Exact staged renderer preparation, retained only by the lifecycle owner.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct NativeStagedReadyProof {
    visibility: NativeVisibilityProof,
    post_show: PresentedNativeStagingPresentation,
}

/// Compact provenance retained with a compiled prospective child scene.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct NativeReadyBasis {
    coordinate_generation: CoordinateGeneration,
    evidence: NativeReadyBasisEvidence,
}

#[cfg(test)]
impl NativeReadyBasis {
    pub(crate) const fn for_test() -> Self {
        Self {
            coordinate_generation: CoordinateGeneration::new(1),
            evidence: NativeReadyBasisEvidence::Direct {
                prepared_since: CoordinateObservationGeneration::new(1),
            },
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum NativeReadyBasisEvidence {
    Direct {
        prepared_since: CoordinateObservationGeneration,
    },
    Staged {
        output: crate::presentation_observation::HostFrameKey,
    },
}

/// Both host strategies enter the same first-live publication pipeline.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum NativeReadyProof {
    Direct {
        binding: ViewportBinding,
        owner: NativeStagingOwner,
        provider: PlatformObservationLease,
        create: EffectId,
        create_emitted: InventoryGeneration,
        create_applied: InventoryGeneration,
        coordinate_generation: CoordinateGeneration,
        prepared_since: CoordinateObservationGeneration,
    },
    Staged(NativeStagedReadyProof),
}

impl NativeReadyProof {
    pub(crate) const fn binding(self) -> ViewportBinding {
        match self {
            Self::Direct { binding, .. } => binding,
            Self::Staged(proof) => proof.visibility.binding,
        }
    }
    pub(crate) const fn create(self) -> EffectId {
        match self {
            Self::Direct { create, .. } => create,
            Self::Staged(proof) => proof.visibility.create,
        }
    }
    pub(crate) const fn show(self) -> Option<EffectId> {
        match self {
            Self::Direct { .. } => None,
            Self::Staged(proof) => Some(proof.visibility.show),
        }
    }
    pub(crate) const fn correlation_effect(self) -> EffectId {
        match self.show() {
            Some(show) => show,
            None => self.create(),
        }
    }
    pub(crate) const fn owner(self) -> NativeStagingOwner {
        match self {
            Self::Direct { owner, .. } => owner,
            Self::Staged(proof) => proof.post_show.request().owner(),
        }
    }
    pub(crate) const fn retained_resource(self) -> Option<NativeStagingResourceId> {
        self.owner().retained_resource()
    }
    pub(crate) const fn basis(self) -> NativeReadyBasis {
        match self {
            Self::Direct {
                coordinate_generation,
                prepared_since,
                ..
            } => NativeReadyBasis {
                coordinate_generation,
                evidence: NativeReadyBasisEvidence::Direct { prepared_since },
            },
            Self::Staged(proof) => NativeReadyBasis {
                coordinate_generation: proof.visibility.visible_coordinate_generation,
                evidence: NativeReadyBasisEvidence::Staged {
                    output: proof.post_show.output_key(),
                },
            },
        }
    }
    pub(super) const fn resume_preparation(self) -> NativeBringupPhase {
        match self {
            Self::Direct { create, .. } => NativeBringupPhase::AwaitingPrepared { create },
            Self::Staged(proof) => NativeBringupPhase::AwaitingPostShowPresentation {
                visibility: proof.visibility,
                presentation: proof.post_show.request(),
            },
        }
    }
}

pub(super) enum NativePreparationOutcome {
    Ignored,
    Advanced(NativeBringupPhase),
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
        let binding = proof.binding();
        let Some(record) = self
            .registry
            .record(binding.surface())
            .filter(|record| record.binding() == binding)
        else {
            return false;
        };
        if !record.is_ready()
            || record.coordinates().is_none_or(|coordinates| {
                coordinates.content_size().width() <= 0.0
                    || coordinates.content_size().height() <= 0.0
            })
            || record
                .create_preparation()
                .and_then(|preparation| preparation.known_readiness())
                == Some(NativeCreateReadiness::Failed)
            || record.coordinate_generation() != proof.basis().coordinate_generation
            || record
                .presentation_observation()
                .and_then(WindowPresentationObservation::known_state)
                != Some(WindowPresentationState::Visible)
        {
            return false;
        }
        match proof {
            NativeReadyProof::Direct { provider, create, create_emitted, create_applied, prepared_since, .. } => {
                self.platform_provider() == Some(provider)
                    && record.renderer_prepared_since() == Some(prepared_since)
                    && record.create_preparation().and_then(|preparation| preparation.known_readiness()) == Some(NativeCreateReadiness::Prepared)
                    && self.effect_emission_fence(create, binding) == Some(create_emitted)
                    && create_applied > create_emitted
                    && self.effects.record(create).is_some_and(|effect| {
                        effect.provider() == Some(provider) && matches!(effect.phase(), EffectPhase::ObservedApplied { inventory_generation } if inventory_generation == create_applied)
                    })
            }
            NativeReadyProof::Staged(proof) => {
                let visibility = proof.visibility;
                let provider = proof.post_show.request().basis().platform_provider();
                self.platform_provider() == Some(provider)
                    && visibility.pre_show.request().basis().platform_provider() == provider
                    && proof.post_show.request().binding() == binding
                    && proof.post_show.request().owner() == visibility.pre_show.request().owner()
                    && visibility.visible_generation > visibility.acknowledged_generation
                    && visibility.acknowledged_generation > visibility.hidden_generation
                    && visibility.acknowledged_coordinate_generation >= visibility.hidden_coordinate_generation
                    && visibility.visible_coordinate_generation >= visibility.acknowledged_coordinate_generation
                    && visibility.acknowledged_coordinate_observation_generation >= visibility.hidden_coordinate_observation_generation
                    && visibility.visible_coordinate_observation_generation >= visibility.acknowledged_coordinate_observation_generation
                    && visibility.visible_inventory_generation > visibility.acknowledged_inventory_generation
                    && visibility.acknowledged_inventory_generation > visibility.hidden_inventory_generation
                    && visibility.visible_inventory_generation > visibility.show_emitted_inventory_generation
                    && visibility.hidden_inventory_generation > visibility.create_emitted_inventory_generation
                    && visibility.acknowledged_inventory_generation > visibility.show_emitted_inventory_generation
                    && record.presentation_observation().is_some_and(|observation| observation.generation() >= visibility.visible_generation && observation.inventory_generation() >= visibility.visible_inventory_generation)
                    && record.coordinate_observation_generation() >= Some(visibility.visible_coordinate_observation_generation)
                    && self.effect_emission_fence(visibility.create, binding) == Some(visibility.create_emitted_inventory_generation)
                    && self.effect_emission_fence(visibility.show, binding) == Some(visibility.show_emitted_inventory_generation)
                    && self.effects.record(visibility.create).is_some_and(|effect| effect.provider() == Some(provider) && matches!(effect.phase(), EffectPhase::ObservedApplied { inventory_generation } if inventory_generation <= visibility.hidden_inventory_generation))
                    && self.effects.record(visibility.show).is_some_and(|effect| {
                        effect.provider() == Some(provider) && matches!(effect.phase(), EffectPhase::ObservedApplied { inventory_generation } if inventory_generation == visibility.acknowledged_inventory_generation)
                            && matches!(effect.request().effect(), PlatformEffect::ShowWindow { binding: actual, after_pre_show, .. } if *actual == binding && *after_pre_show == visibility.pre_show)
                    })
            }
        }
    }

    pub(super) fn reduce_ready_native_bringup(
        &mut self,
        provider: PlatformObservationLease,
        binding: ViewportBinding,
        owner: NativeStagingOwner,
        phase: NativeBringupPhase,
    ) -> Result<NativePreparationOutcome, ViewportCoordinatorError> {
        let create_applied =
            self.settle_native_create_preparation(provider, binding, phase.create_effect());
        if let NativeBringupPhase::AwaitingPrepared { create } = phase {
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
            let Some(create_applied) = create_applied else {
                return Ok(NativePreparationOutcome::Ignored);
            };
            let Some(create_emitted) = self.effect_emission_fence(create, binding) else {
                return Ok(NativePreparationOutcome::Ignored);
            };
            let proof = NativeReadyProof::Direct {
                binding,
                owner,
                provider,
                create,
                create_emitted,
                create_applied,
                coordinate_generation: record.coordinate_generation(),
                prepared_since,
            };
            return Ok(if self.native_ready_proof_is_current(proof) {
                NativePreparationOutcome::Ready(proof)
            } else {
                NativePreparationOutcome::Ignored
            });
        }
        let Some((observation, coordinate_generation, coordinate_observation_generation)) = self
            .registry
            .record(binding.surface())
            .filter(|record| record.binding() == binding)
            .and_then(|record| {
                Some((
                    record.presentation_observation()?,
                    record.coordinate_generation(),
                    record.coordinate_observation_generation()?,
                ))
            })
        else {
            return Ok(NativePreparationOutcome::Ignored);
        };
        let next = match phase {
            NativeBringupPhase::AwaitingHidden { create }
                if observation.known_state() == Some(WindowPresentationState::Hidden)
                    && create_applied.is_some() =>
            {
                let Some(create_emitted_inventory_generation) =
                    self.effect_emission_fence(create, binding)
                else {
                    return Ok(NativePreparationOutcome::Ignored);
                };
                let presentation = self.mint_native_staging_presentation(
                    binding,
                    owner,
                    NativeStagingPresentationPhase::PreShow,
                    provider,
                    observation.generation(),
                    coordinate_generation,
                )?;
                Some(NativeBringupPhase::AwaitingPreShowPresentation {
                    create,
                    hidden_generation: observation.generation(),
                    hidden_inventory_generation: observation.inventory_generation(),
                    hidden_coordinate_generation: coordinate_generation,
                    hidden_coordinate_observation_generation: coordinate_observation_generation,
                    create_emitted_inventory_generation,
                    presentation,
                })
            }
            NativeBringupPhase::AwaitingPreShowPresentation {
                create,
                create_emitted_inventory_generation,
                presentation,
                ..
            } if observation.known_state() == Some(WindowPresentationState::Hidden)
                && self.presentation_observation_follows_applied_effect(
                    provider,
                    observation,
                    create,
                )
                && (presentation.basis().platform_provider() != provider
                    || presentation.basis().presentation_observation_generation()
                        != observation.generation()
                    || presentation.basis().coordinate_generation() != coordinate_generation) =>
            {
                let presentation = self.mint_native_staging_presentation(
                    binding,
                    owner,
                    NativeStagingPresentationPhase::PreShow,
                    provider,
                    observation.generation(),
                    coordinate_generation,
                )?;
                Some(NativeBringupPhase::AwaitingPreShowPresentation {
                    create,
                    hidden_generation: observation.generation(),
                    hidden_inventory_generation: observation.inventory_generation(),
                    hidden_coordinate_generation: coordinate_generation,
                    hidden_coordinate_observation_generation: coordinate_observation_generation,
                    create_emitted_inventory_generation,
                    presentation,
                })
            }
            NativeBringupPhase::AwaitingShowAcknowledgement {
                create,
                show,
                hidden_generation,
                hidden_inventory_generation,
                hidden_coordinate_generation,
                hidden_coordinate_observation_generation,
                create_emitted_inventory_generation,
                pre_show,
            } if observation.generation() > hidden_generation
                && observation.inventory_generation() > hidden_inventory_generation
                && self.presentation_observation_settles(observation, show) =>
            {
                let Some(show_emitted_inventory_generation) =
                    self.effect_emission_fence(show, binding)
                else {
                    return Ok(NativePreparationOutcome::Ignored);
                };
                if self.effects.mark_observed_applied(
                    provider,
                    show,
                    binding,
                    observation.inventory_generation(),
                ) != EffectTransition::Applied
                {
                    return Ok(NativePreparationOutcome::Ignored);
                }
                Some(NativeBringupPhase::AwaitingVisible {
                    create,
                    show,
                    hidden_generation,
                    hidden_inventory_generation,
                    hidden_coordinate_generation,
                    hidden_coordinate_observation_generation,
                    acknowledged_generation: observation.generation(),
                    acknowledged_inventory_generation: observation.inventory_generation(),
                    acknowledged_coordinate_generation: coordinate_generation,
                    acknowledged_coordinate_observation_generation:
                        coordinate_observation_generation,
                    create_emitted_inventory_generation,
                    show_emitted_inventory_generation,
                    pre_show,
                })
            }
            NativeBringupPhase::AwaitingVisible {
                create,
                show,
                hidden_generation,
                hidden_inventory_generation,
                hidden_coordinate_generation,
                hidden_coordinate_observation_generation,
                acknowledged_generation,
                acknowledged_inventory_generation,
                acknowledged_coordinate_generation,
                acknowledged_coordinate_observation_generation,
                create_emitted_inventory_generation,
                show_emitted_inventory_generation,
                pre_show,
            } if observation.generation() > acknowledged_generation
                && observation.inventory_generation() > acknowledged_inventory_generation
                && observation.known_state() == Some(WindowPresentationState::Visible)
                && self.presentation_observation_is_after_emission(observation, show) =>
            {
                let visibility = NativeVisibilityProof {
                    binding,
                    create,
                    show,
                    hidden_generation,
                    hidden_inventory_generation,
                    hidden_coordinate_generation,
                    hidden_coordinate_observation_generation,
                    acknowledged_generation,
                    acknowledged_inventory_generation,
                    acknowledged_coordinate_generation,
                    acknowledged_coordinate_observation_generation,
                    visible_generation: observation.generation(),
                    visible_inventory_generation: observation.inventory_generation(),
                    visible_coordinate_generation: coordinate_generation,
                    visible_coordinate_observation_generation: coordinate_observation_generation,
                    create_emitted_inventory_generation,
                    show_emitted_inventory_generation,
                    pre_show,
                };
                let presentation = self.mint_native_staging_presentation(
                    binding,
                    owner,
                    NativeStagingPresentationPhase::PostShow,
                    provider,
                    observation.generation(),
                    coordinate_generation,
                )?;
                Some(NativeBringupPhase::AwaitingPostShowPresentation {
                    visibility,
                    presentation,
                })
            }
            NativeBringupPhase::AwaitingPostShowPresentation {
                mut visibility,
                presentation,
            } if observation.known_state() == Some(WindowPresentationState::Visible)
                && self
                    .presentation_observation_is_after_emission(observation, visibility.show)
                && (presentation.basis().platform_provider() != provider
                    || presentation.basis().presentation_observation_generation()
                        != observation.generation()
                    || presentation.basis().coordinate_generation() != coordinate_generation) =>
            {
                visibility.visible_generation = observation.generation();
                visibility.visible_inventory_generation = observation.inventory_generation();
                visibility.visible_coordinate_generation = coordinate_generation;
                visibility.visible_coordinate_observation_generation =
                    coordinate_observation_generation;
                let presentation = self.mint_native_staging_presentation(
                    binding,
                    owner,
                    NativeStagingPresentationPhase::PostShow,
                    provider,
                    observation.generation(),
                    coordinate_generation,
                )?;
                Some(NativeBringupPhase::AwaitingPostShowPresentation {
                    visibility,
                    presentation,
                })
            }
            NativeBringupPhase::AwaitingPrepared { .. }
            | NativeBringupPhase::AwaitingHidden { .. }
            | NativeBringupPhase::AwaitingPreShowPresentation { .. }
            | NativeBringupPhase::AwaitingShowAcknowledgement { .. }
            | NativeBringupPhase::AwaitingVisible { .. }
            | NativeBringupPhase::AwaitingPostShowPresentation { .. } => None,
        };
        Ok(next.map_or(
            NativePreparationOutcome::Ignored,
            NativePreparationOutcome::Advanced,
        ))
    }

    pub(super) fn observe_native_bringup_presentation(
        &mut self,
        binding: ViewportBinding,
        owner: NativeStagingOwner,
        phase: NativeBringupPhase,
        presented: PresentedNativeStagingPresentation,
    ) -> Result<NativePreparationOutcome, ViewportCoordinatorError> {
        let request = presented.request();
        if request.authority_domain() != self.authority_domain
            || request.binding() != binding
            || request.owner() != owner
            || phase.staging_presentation() != Some(request)
            || !self.native_staging_basis_matches_current(request)
        {
            return Ok(NativePreparationOutcome::Ignored);
        }
        let current = self
            .registry
            .record(binding.surface())
            .filter(|record| record.binding() == binding);
        match phase {
            NativeBringupPhase::AwaitingPreShowPresentation {
                create,
                hidden_generation,
                hidden_inventory_generation,
                hidden_coordinate_generation,
                hidden_coordinate_observation_generation,
                create_emitted_inventory_generation,
                presentation,
            } if presentation == request
                && request.phase() == NativeStagingPresentationPhase::PreShow =>
            {
                let Some(record) = current else {
                    return Ok(NativePreparationOutcome::Ignored);
                };
                let Some(observation) = record.presentation_observation() else {
                    return Ok(NativePreparationOutcome::Ignored);
                };
                if observation.known_state() != Some(WindowPresentationState::Hidden)
                    || observation.generation() != hidden_generation
                    || observation.inventory_generation() < hidden_inventory_generation
                    || record.coordinate_generation() != hidden_coordinate_generation
                    || record.coordinate_observation_generation()
                        < Some(hidden_coordinate_observation_generation)
                {
                    return Ok(NativePreparationOutcome::Ignored);
                }
                let show = self
                    .effects
                    .request(PlatformEffect::ShowWindow {
                        binding,
                        after_hidden: hidden_generation,
                        after_pre_show: presented,
                    })
                    .map_err(ViewportCoordinatorError::Effect)?;
                Ok(NativePreparationOutcome::Advanced(
                    NativeBringupPhase::AwaitingShowAcknowledgement {
                        create,
                        show,
                        hidden_generation,
                        hidden_inventory_generation,
                        hidden_coordinate_generation,
                        hidden_coordinate_observation_generation,
                        create_emitted_inventory_generation,
                        pre_show: presented,
                    },
                ))
            }
            NativeBringupPhase::AwaitingPostShowPresentation {
                visibility,
                presentation,
            } if presentation == request
                && request.phase() == NativeStagingPresentationPhase::PostShow
                && visibility.pre_show.request().owner() == presented.request().owner() =>
            {
                let Some(record) = current else {
                    return Ok(NativePreparationOutcome::Ignored);
                };
                let Some(observation) = record.presentation_observation() else {
                    return Ok(NativePreparationOutcome::Ignored);
                };
                if observation.known_state() != Some(WindowPresentationState::Visible)
                    || observation.generation() != visibility.visible_generation
                    || observation.inventory_generation() < visibility.visible_inventory_generation
                    || record.coordinate_generation() != visibility.visible_coordinate_generation
                    || record.coordinate_observation_generation()
                        < Some(visibility.visible_coordinate_observation_generation)
                {
                    return Ok(NativePreparationOutcome::Ignored);
                }
                Ok(NativePreparationOutcome::Ready(NativeReadyProof::Staged(
                    NativeStagedReadyProof {
                        visibility,
                        post_show: presented,
                    },
                )))
            }
            NativeBringupPhase::AwaitingPrepared { .. }
            | NativeBringupPhase::AwaitingHidden { .. }
            | NativeBringupPhase::AwaitingPreShowPresentation { .. }
            | NativeBringupPhase::AwaitingShowAcknowledgement { .. }
            | NativeBringupPhase::AwaitingVisible { .. }
            | NativeBringupPhase::AwaitingPostShowPresentation { .. } => {
                Ok(NativePreparationOutcome::Ignored)
            }
        }
    }

    fn mint_native_staging_presentation(
        &self,
        binding: ViewportBinding,
        owner: NativeStagingOwner,
        phase: NativeStagingPresentationPhase,
        provider: PlatformObservationLease,
        presentation_generation: PresentationObservationGeneration,
        coordinate_generation: CoordinateGeneration,
    ) -> Result<NativeStagingPresentation, ViewportCoordinatorError> {
        if owner
            .retained_resource()
            .is_some_and(|resource| self.native_staging_resources.get(resource).is_none())
        {
            return Err(ViewportCoordinatorError::InvalidNativeStagingPresentation { binding });
        }
        let basis = NativeStagingBasis::new(
            provider,
            presentation_generation,
            coordinate_generation,
            owner,
        )
        .ok_or(ViewportCoordinatorError::InvalidNativeStagingPresentation { binding })?;
        NativeStagingPresentation::mint(binding, phase, basis)
            .ok_or(ViewportCoordinatorError::InvalidNativeStagingPresentation { binding })
    }

    pub(super) fn native_staging_basis_matches_current(
        &self,
        presentation: NativeStagingPresentation,
    ) -> bool {
        let basis = presentation.basis();
        if self.platform_provider() != Some(basis.platform_provider())
            || basis
                .retained_resource()
                .is_some_and(|resource| self.native_staging_resources.get(resource).is_none())
        {
            return false;
        }
        let Some(record) = self
            .registry
            .record(presentation.binding().surface())
            .filter(|record| record.binding() == presentation.binding())
        else {
            return false;
        };
        let Some(observation) = record.presentation_observation() else {
            return false;
        };
        let expected_state = match presentation.phase() {
            NativeStagingPresentationPhase::PreShow => WindowPresentationState::Hidden,
            NativeStagingPresentationPhase::PostShow => WindowPresentationState::Visible,
        };
        observation.known_state() == Some(expected_state)
            && observation.generation() == basis.presentation_observation_generation()
            && record.coordinate_generation() == basis.coordinate_generation()
    }

    pub(super) fn presentation_observation_settles(
        &self,
        observation: WindowPresentationObservation,
        effect: EffectId,
    ) -> bool {
        observation.acknowledges(effect)
            && self.presentation_observation_is_after_emission(observation, effect)
    }

    fn presentation_observation_follows_applied_effect(
        &self,
        provider: PlatformObservationLease,
        observation: WindowPresentationObservation,
        effect: EffectId,
    ) -> bool {
        self.presentation_observation_is_after_emission(observation, effect)
            && self.effects.record(effect).is_some_and(|record| {
                record.provider() == Some(provider)
                    && matches!(
                        record.phase(),
                        EffectPhase::ObservedApplied {
                            inventory_generation,
                        } if observation.inventory_generation() >= inventory_generation
                    )
            })
    }

    pub(super) fn presentation_observation_is_after_emission(
        &self,
        observation: WindowPresentationObservation,
        effect: EffectId,
    ) -> bool {
        self.effects.record(effect).is_some_and(|record| {
            record.request().effect().binding() == observation.binding()
                && record
                    .emitted_inventory_generation()
                    .is_some_and(|emitted| observation.inventory_generation() > emitted)
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
