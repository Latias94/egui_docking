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
            || proof
                .retained_resource()
                .is_some_and(|resource| self.native_staging_resources.get(resource).is_none())
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
                    && self.native_staging_basis_matches_current(proof.post_show.request())
                    && visibility.pre_show.request().basis().platform_provider() == provider
                    && proof.post_show.request().binding() == binding
                    && proof.post_show.request().owner() == visibility.pre_show.request().owner()
                    && visibility.visible_generation > visibility.acknowledged_generation
                    && visibility.acknowledged_generation > visibility.hidden_generation
                    && visibility.visible_coordinate_generation >= visibility.hidden_coordinate_generation
                    && visibility.visible_coordinate_observation_generation >= visibility.hidden_coordinate_observation_generation
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
        if let NativeBringupPhase::AwaitingShowAcknowledgement {
            create,
            show,
            hidden_generation,
            hidden_inventory_generation,
            hidden_coordinate_generation,
            hidden_coordinate_observation_generation,
            create_emitted_inventory_generation,
            pre_show,
        } = phase
        {
            // Effect settlement is independent of coordinate and renderer readiness.
            // Consume the one-shot ACK before a later snapshot replaces its envelope.
            let observation = self
                .registry
                .record(binding.surface())
                .filter(|record| record.binding() == binding)
                .and_then(|record| record.presentation_observation());
            let Some(observation) = observation.filter(|observation| {
                observation.generation() > hidden_generation
                    && observation.inventory_generation() > hidden_inventory_generation
                    && self.presentation_observation_settles(*observation, show)
            }) else {
                return Ok(NativePreparationOutcome::Ignored);
            };
            let Some(show_emitted_inventory_generation) = self.effect_emission_fence(show, binding)
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
            return Ok(NativePreparationOutcome::Advanced(
                NativeBringupPhase::AwaitingVisible {
                    create,
                    show,
                    hidden_generation,
                    hidden_inventory_generation,
                    hidden_coordinate_generation,
                    hidden_coordinate_observation_generation,
                    acknowledged_generation: observation.generation(),
                    acknowledged_inventory_generation: observation.inventory_generation(),
                    create_emitted_inventory_generation,
                    show_emitted_inventory_generation,
                    pre_show,
                },
            ));
        }
        let Some((observation, coordinate_generation, coordinate_observation_generation)) = self
            .registry
            .record(binding.surface())
            .filter(|record| {
                record.binding() == binding
                    && record.is_ready()
                    && matches!(
                        record
                            .create_preparation()
                            .and_then(|preparation| preparation.known_readiness()),
                        Some(NativeCreateReadiness::Created | NativeCreateReadiness::Prepared)
                    )
            })
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
                    && create_applied.is_some()
                    && self.presentation_observation_follows_applied_effect(
                        provider,
                        observation,
                        create,
                    ) =>
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
                && !self.native_staging_basis_matches_current(presentation) =>
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
            NativeBringupPhase::AwaitingVisible {
                create,
                show,
                hidden_generation,
                hidden_inventory_generation,
                hidden_coordinate_generation,
                hidden_coordinate_observation_generation,
                acknowledged_generation,
                acknowledged_inventory_generation,

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
                && self.presentation_observation_follows_applied_effect(
                    provider,
                    observation,
                    visibility.show,
                )
                && !self.native_staging_basis_matches_current(presentation) =>
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
                    || observation.generation() < hidden_generation
                    || observation.inventory_generation() < hidden_inventory_generation
                    || record.coordinate_generation() != hidden_coordinate_generation
                    || record.coordinate_observation_generation()
                        < Some(hidden_coordinate_observation_generation)
                    || self.effect_emission_fence(create, binding)
                        != Some(create_emitted_inventory_generation)
                    || !self.presentation_observation_follows_applied_effect(
                        request.basis().platform_provider(),
                        observation,
                        create,
                    )
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
                    || observation.generation() < visibility.visible_generation
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
        // The basis names an observation inside the current uninterrupted
        // interval; later identical snapshots do not replace that request.
        record.is_ready()
            && record.coordinates().is_some_and(|coordinates| {
                coordinates.content_size().width() > 0.0
                    && coordinates.content_size().height() > 0.0
            })
            && matches!(
                record
                    .create_preparation()
                    .and_then(|preparation| preparation.known_readiness()),
                Some(NativeCreateReadiness::Created | NativeCreateReadiness::Prepared)
            )
            && observation.known_state() == Some(expected_state)
            && record.staging_presentation_is_continuous_since(
                basis.presentation_observation_generation(),
            )
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::{PhysicalRect, ScaleFactor};
    use crate::ids::{SurfaceId, WorkspaceEpoch};
    use crate::intent::{Authority, AuthorityUnavailableReason};
    use crate::platform::{
        CapabilityRosterObservation, ObservedWindow, PlatformCapabilities, PlatformCapability,
        PlatformSnapshot, PresentationEffectAcknowledgement, WindowCoordinateObservation,
        WindowCreatePreparationObservation, WindowInventoryObservation, WorkAreaRosterObservation,
    };
    use crate::surface_recovery::SurfaceRecoveryObligationId;
    use crate::viewport::{
        CapabilityObservationGeneration, InventoryObservationGeneration,
        PlatformSnapshotGeneration, ViewportRole, WorkAreaObservationGeneration,
    };
    use crate::viewport_focus::{FocusObservationGeneration, unknown_focus_observation};

    fn window(
        binding: ViewportBinding,
        generation: u64,
        state: WindowPresentationState,
        create_acknowledgement: Option<EffectId>,
    ) -> ObservedWindow {
        let bounds = PhysicalRect::new(10.0, 20.0, 300.0, 200.0).unwrap();
        let scale = ScaleFactor::new(1.0).unwrap();
        ObservedWindow::new(binding)
            .with_coordinate_observation(WindowCoordinateObservation::new(
                binding,
                CoordinateObservationGeneration::new(generation),
                Authority::Known(bounds),
                Authority::Known(bounds),
                Authority::Known(scale),
                Authority::Known(scale),
            ))
            .with_presentation_observation(WindowPresentationObservation::new(
                binding,
                PresentationObservationGeneration::new(generation),
                Authority::Known(state),
                PresentationEffectAcknowledgement::known(None),
            ))
            .with_create_preparation(WindowCreatePreparationObservation::new(
                binding,
                CoordinateObservationGeneration::new(generation),
                Authority::Known(NativeCreateReadiness::Created),
                create_acknowledgement,
            ))
    }

    fn publish(coordinator: &mut ViewportCoordinator, generation: u64, window: ObservedWindow) {
        let mut capabilities = PlatformCapabilities::default();
        capabilities.set_authoritative_inventory(PlatformCapability::Supported);
        let reason = AuthorityUnavailableReason::NotReported;
        let snapshot = PlatformSnapshot::new(
            PlatformSnapshotGeneration::new(generation),
            CapabilityRosterObservation::new(
                CapabilityObservationGeneration::new(generation),
                Authority::Known(capabilities),
            ),
            unknown_focus_observation(FocusObservationGeneration::new(generation), reason),
            WindowInventoryObservation::new(
                InventoryObservationGeneration::new(generation),
                Authority::Known(vec![window.binding()]),
            )
            .unwrap(),
            vec![window],
            Vec::new(),
            WorkAreaRosterObservation::unknown(
                WorkAreaObservationGeneration::new(generation),
                reason,
            ),
        )
        .unwrap();
        coordinator.publish_snapshot(&snapshot).unwrap();
    }

    fn advance(
        coordinator: &mut ViewportCoordinator,
        phase: NativeBringupPhase,
    ) -> NativeBringupPhase {
        let presentation = phase.staging_presentation().unwrap();
        let NativePreparationOutcome::Advanced(next) = coordinator
            .observe_native_bringup_presentation(
                presentation.binding(),
                presentation.owner(),
                phase,
                PresentedNativeStagingPresentation::mint_observed_for_test(presentation, 1),
            )
            .unwrap()
        else {
            panic!("pre-show presentation must advance to ShowWindow");
        };
        next
    }

    fn reduce(
        coordinator: &mut ViewportCoordinator,
        binding: ViewportBinding,
        owner: NativeStagingOwner,
        phase: NativeBringupPhase,
    ) -> NativeBringupPhase {
        match coordinator
            .reduce_ready_native_bringup(
                coordinator.platform_provider().unwrap(),
                binding,
                owner,
                phase,
            )
            .unwrap()
        {
            NativePreparationOutcome::Advanced(next) => next,
            NativePreparationOutcome::Ignored => phase,
            NativePreparationOutcome::Ready(_) => panic!("staging still requires Presented"),
        }
    }

    fn awaiting_staging(post_show: bool) -> (ViewportCoordinator, NativeBringupPhase, u64) {
        let mut coordinator = ViewportCoordinator::default();
        let binding = coordinator
            .registry
            .reserve(
                WorkspaceEpoch::default(),
                SurfaceId::new(710),
                ViewportRole::Child,
            )
            .unwrap();
        let owner = NativeStagingOwner::InitialSurface {
            binding,
            recovery_obligation: SurfaceRecoveryObligationId::new_for_test(712),
        };
        let create = coordinator
            .effects
            .request(PlatformEffect::RequestReplacement {
                binding,
                placement: PhysicalRect::new(10.0, 20.0, 300.0, 200.0).unwrap(),
                role: ViewportRole::Child,
                strategy: NativeCreateStrategy::Staged,
            })
            .unwrap();
        assert_eq!(coordinator.take_new_effects().len(), 1);
        publish(
            &mut coordinator,
            1,
            window(binding, 1, WindowPresentationState::Hidden, Some(create)),
        );
        let phase = reduce(
            &mut coordinator,
            binding,
            owner,
            NativeBringupPhase::AwaitingHidden { create },
        );
        assert!(matches!(
            phase,
            NativeBringupPhase::AwaitingPreShowPresentation { .. }
        ));
        if !post_show {
            return (coordinator, phase, 1);
        }
        let phase = advance(&mut coordinator, phase);
        let effects = coordinator.take_new_effects();
        let [show] = effects.as_slice() else {
            panic!("pre-show must emit one ShowWindow");
        };
        publish(
            &mut coordinator,
            2,
            window(binding, 2, WindowPresentationState::Hidden, None)
                .with_presentation_observation(WindowPresentationObservation::new(
                    binding,
                    PresentationObservationGeneration::new(2),
                    Authority::Known(WindowPresentationState::Hidden),
                    PresentationEffectAcknowledgement::known(Some(show.id())),
                )),
        );
        let phase = reduce(&mut coordinator, binding, owner, phase);
        assert!(matches!(phase, NativeBringupPhase::AwaitingVisible { .. }));
        publish(
            &mut coordinator,
            3,
            window(binding, 3, WindowPresentationState::Visible, None),
        );
        let phase = reduce(&mut coordinator, binding, owner, phase);
        assert!(matches!(
            phase,
            NativeBringupPhase::AwaitingPostShowPresentation { .. }
        ));
        (coordinator, phase, 3)
    }

    #[test]
    fn unchanged_snapshots_preserve_inflight_pre_show_and_post_show() {
        for post_show in [false, true] {
            let (mut coordinator, mut phase, generation) = awaiting_staging(post_show);
            let presentation = phase.staging_presentation().unwrap();
            let binding = presentation.binding();
            let state = if post_show {
                WindowPresentationState::Visible
            } else {
                WindowPresentationState::Hidden
            };
            for generation in generation + 1..=generation + 16 {
                publish(
                    &mut coordinator,
                    generation,
                    window(binding, generation, state, None),
                );
                phase = reduce(&mut coordinator, binding, presentation.owner(), phase);
                assert_eq!(phase.staging_presentation(), Some(presentation));
                assert!(coordinator.native_staging_basis_matches_current(presentation));
            }
            let outcome = coordinator
                .observe_native_bringup_presentation(
                    binding,
                    presentation.owner(),
                    phase,
                    PresentedNativeStagingPresentation::mint_observed_for_test(
                        presentation,
                        if post_show { 2 } else { 1 },
                    ),
                )
                .unwrap();
            if post_show {
                let NativePreparationOutcome::Ready(proof) = outcome else {
                    panic!("the original post-show output must establish readiness");
                };
                assert!(coordinator.native_ready_proof_is_current(proof));
                publish(
                    &mut coordinator,
                    generation + 17,
                    window(binding, generation + 17, state, None),
                );
                assert!(coordinator.native_ready_proof_is_current(proof));
                let revoked = window(binding, generation + 18, state, None)
                    .with_create_preparation(WindowCreatePreparationObservation::new(
                        binding,
                        CoordinateObservationGeneration::new(generation + 18),
                        Authority::Unknown(AuthorityUnavailableReason::NotReported),
                        None,
                    ));
                publish(&mut coordinator, generation + 18, revoked);
                assert!(!coordinator.native_ready_proof_is_current(proof));
                publish(
                    &mut coordinator,
                    generation + 19,
                    window(binding, generation + 19, state, None),
                );
                assert!(
                    !coordinator.native_ready_proof_is_current(proof),
                    "restored readiness requires a new post-show output"
                );
            } else {
                assert!(matches!(
                    outcome,
                    NativePreparationOutcome::Advanced(
                        NativeBringupPhase::AwaitingShowAcknowledgement { .. }
                    )
                ));
                assert!(
                    matches!(coordinator.take_new_effects().as_slice(), [request] if matches!(request.effect(), PlatformEffect::ShowWindow { binding: actual, .. } if *actual == binding))
                );
            }
        }
    }

    #[test]
    fn interrupted_staging_rejects_old_outputs_after_the_same_facts_return() {
        #[derive(Clone, Copy, Debug)]
        enum Revocation {
            PresentationUnknown,
            Visibility,
            ReadinessUnknown,
            ReadinessChanged,
            Failed,
            Bounds,
            OuterBounds,
            NativeScale,
            PresentationScale,
        }

        for post_show in [false, true] {
            for revocation in [
                Revocation::PresentationUnknown,
                Revocation::Visibility,
                Revocation::ReadinessUnknown,
                Revocation::ReadinessChanged,
                Revocation::Failed,
                Revocation::Bounds,
                Revocation::OuterBounds,
                Revocation::NativeScale,
                Revocation::PresentationScale,
            ] {
                let (mut coordinator, phase, generation) = awaiting_staging(post_show);
                let presentation = phase.staging_presentation().unwrap();
                let binding = presentation.binding();
                let state = if post_show {
                    WindowPresentationState::Visible
                } else {
                    WindowPresentationState::Hidden
                };
                let next = generation + 1;
                let mut revoked = window(binding, next, state, None);
                match revocation {
                    Revocation::PresentationUnknown | Revocation::Visibility => {
                        revoked = revoked.with_presentation_observation(
                            WindowPresentationObservation::new(
                                binding,
                                PresentationObservationGeneration::new(next),
                                if matches!(revocation, Revocation::PresentationUnknown) {
                                    Authority::Unknown(AuthorityUnavailableReason::NotReported)
                                } else {
                                    Authority::Known(WindowPresentationState::Minimized)
                                },
                                PresentationEffectAcknowledgement::known(None),
                            ),
                        );
                    }
                    Revocation::ReadinessUnknown
                    | Revocation::ReadinessChanged
                    | Revocation::Failed => {
                        revoked = revoked.with_create_preparation(
                            WindowCreatePreparationObservation::new(
                                binding,
                                CoordinateObservationGeneration::new(next),
                                match revocation {
                                    Revocation::ReadinessUnknown => {
                                        Authority::Unknown(AuthorityUnavailableReason::NotReported)
                                    }
                                    Revocation::ReadinessChanged => {
                                        Authority::Known(NativeCreateReadiness::Prepared)
                                    }
                                    _ => Authority::Known(NativeCreateReadiness::Failed),
                                },
                                None,
                            ),
                        );
                    }
                    Revocation::Bounds
                    | Revocation::OuterBounds
                    | Revocation::NativeScale
                    | Revocation::PresentationScale => {
                        let original = PhysicalRect::new(10.0, 20.0, 300.0, 200.0).unwrap();
                        let changed = PhysicalRect::new(11.0, 20.0, 300.0, 200.0).unwrap();
                        let scale = ScaleFactor::new(1.0).unwrap();
                        let changed_scale = ScaleFactor::new(2.0).unwrap();
                        revoked =
                            revoked.with_coordinate_observation(WindowCoordinateObservation::new(
                                binding,
                                CoordinateObservationGeneration::new(next),
                                Authority::Known(if matches!(revocation, Revocation::Bounds) {
                                    changed
                                } else {
                                    original
                                }),
                                Authority::Known(
                                    if matches!(revocation, Revocation::OuterBounds) {
                                        changed
                                    } else {
                                        original
                                    },
                                ),
                                Authority::Known(
                                    if matches!(revocation, Revocation::NativeScale) {
                                        changed_scale
                                    } else {
                                        scale
                                    },
                                ),
                                Authority::Known(
                                    if matches!(revocation, Revocation::PresentationScale) {
                                        changed_scale
                                    } else {
                                        scale
                                    },
                                ),
                            ));
                    }
                }
                publish(&mut coordinator, next, revoked);
                assert!(
                    !coordinator.native_staging_basis_matches_current(presentation),
                    "{revocation:?}"
                );
                publish(
                    &mut coordinator,
                    next + 1,
                    window(binding, next + 1, state, None),
                );
                assert!(
                    !coordinator.native_staging_basis_matches_current(presentation),
                    "{revocation:?}"
                );
                assert!(
                    matches!(
                        coordinator
                            .observe_native_bringup_presentation(
                                binding,
                                presentation.owner(),
                                phase,
                                PresentedNativeStagingPresentation::mint_observed_for_test(
                                    presentation,
                                    1
                                )
                            )
                            .unwrap(),
                        NativePreparationOutcome::Ignored
                    ),
                    "{revocation:?}"
                );
                if !matches!(revocation, Revocation::Failed) {
                    let refreshed = reduce(&mut coordinator, binding, presentation.owner(), phase);
                    let replacement = refreshed.staging_presentation().unwrap();
                    assert_ne!(replacement, presentation, "{revocation:?}");
                    assert!(
                        coordinator.native_staging_basis_matches_current(replacement),
                        "{revocation:?}"
                    );
                }
                assert!(
                    coordinator.take_new_effects().is_empty(),
                    "a stale output must not issue ShowWindow: {revocation:?}"
                );
            }
        }
    }
}
