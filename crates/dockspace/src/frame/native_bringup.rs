//! Shared hidden-to-live admission protocol for native windows.

use crate::effect::{EffectId, EffectTransition, PlatformEffect};
use crate::platform::{WindowPresentationObservation, WindowPresentationState};
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
    pub(crate) const fn create_effect(self) -> EffectId {
        match self {
            Self::AwaitingHidden { create }
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
            Self::AwaitingHidden { .. } | Self::AwaitingPreShowPresentation { .. } => None,
        }
    }

    pub(crate) const fn staging_presentation(self) -> Option<NativeStagingPresentation> {
        match self {
            Self::AwaitingPreShowPresentation { presentation, .. }
            | Self::AwaitingPostShowPresentation { presentation, .. } => Some(presentation),
            Self::AwaitingHidden { .. }
            | Self::AwaitingShowAcknowledgement { .. }
            | Self::AwaitingVisible { .. } => None,
        }
    }
}

/// Exact platform visibility proof retained while post-show staging is pending.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct NativeVisibilityProof {
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

/// Exact presentation proof which authorizes domain-specific native admission.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct NativeVisibleProof {
    pub(super) visibility: NativeVisibilityProof,
    pub(super) post_show: PresentedNativeStagingPresentation,
}

impl NativeVisibleProof {
    #[must_use]
    pub const fn binding(self) -> ViewportBinding {
        self.visibility.binding
    }

    #[must_use]
    pub const fn create(self) -> EffectId {
        self.visibility.create
    }

    #[must_use]
    pub const fn show(self) -> EffectId {
        self.visibility.show
    }

    /// Returns the retained staging resource proven by both outputs, if any.
    #[must_use]
    pub const fn retained_resource(self) -> Option<NativeStagingResourceId> {
        self.post_show.retained_resource()
    }

    #[must_use]
    pub const fn hidden_generation(self) -> PresentationObservationGeneration {
        self.visibility.hidden_generation
    }

    #[must_use]
    pub const fn acknowledged_generation(self) -> PresentationObservationGeneration {
        self.visibility.acknowledged_generation
    }

    #[must_use]
    pub const fn hidden_inventory_generation(self) -> InventoryGeneration {
        self.visibility.hidden_inventory_generation
    }

    #[must_use]
    pub const fn acknowledged_inventory_generation(self) -> InventoryGeneration {
        self.visibility.acknowledged_inventory_generation
    }

    #[must_use]
    pub const fn visible_generation(self) -> PresentationObservationGeneration {
        self.visibility.visible_generation
    }

    #[must_use]
    pub const fn visible_inventory_generation(self) -> InventoryGeneration {
        self.visibility.visible_inventory_generation
    }

    #[must_use]
    pub const fn inventory_generation(self) -> InventoryGeneration {
        self.visibility.visible_inventory_generation
    }

    #[must_use]
    pub const fn hidden_coordinate_generation(self) -> CoordinateGeneration {
        self.visibility.hidden_coordinate_generation
    }

    #[must_use]
    pub const fn hidden_coordinate_observation_generation(self) -> CoordinateObservationGeneration {
        self.visibility.hidden_coordinate_observation_generation
    }

    #[must_use]
    pub const fn acknowledged_coordinate_generation(self) -> CoordinateGeneration {
        self.visibility.acknowledged_coordinate_generation
    }

    #[must_use]
    pub const fn acknowledged_coordinate_observation_generation(
        self,
    ) -> CoordinateObservationGeneration {
        self.visibility
            .acknowledged_coordinate_observation_generation
    }

    #[must_use]
    pub const fn visible_coordinate_generation(self) -> CoordinateGeneration {
        self.visibility.visible_coordinate_generation
    }

    #[must_use]
    pub const fn visible_coordinate_observation_generation(
        self,
    ) -> CoordinateObservationGeneration {
        self.visibility.visible_coordinate_observation_generation
    }

    #[must_use]
    pub const fn create_emitted_inventory_generation(self) -> InventoryGeneration {
        self.visibility.create_emitted_inventory_generation
    }

    #[must_use]
    pub const fn show_emitted_inventory_generation(self) -> InventoryGeneration {
        self.visibility.show_emitted_inventory_generation
    }
}

pub(super) enum NativeBringupPresentationOutcome {
    Ignored,
    Advanced(NativeBringupPhase),
    Ready(NativeVisibleProof),
}

impl ViewportCoordinator {
    pub(super) fn reduce_ready_native_bringup(
        &mut self,
        provider: PlatformObservationLease,
        binding: ViewportBinding,
        owner: NativeStagingOwner,
        phase: NativeBringupPhase,
    ) -> Result<Option<NativeBringupPhase>, ViewportCoordinatorError> {
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
            return Ok(None);
        };
        let next = match phase {
            NativeBringupPhase::AwaitingHidden { create }
                if observation.known_state() == Some(WindowPresentationState::Hidden)
                    && self.presentation_observation_settles(observation, create) =>
            {
                let Some(create_emitted_inventory_generation) =
                    self.effect_emission_fence(create, binding)
                else {
                    return Ok(None);
                };
                if self.effects.mark_observed_applied(
                    provider,
                    create,
                    binding,
                    observation.inventory_generation(),
                ) != EffectTransition::Applied
                {
                    return Ok(None);
                }
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
                && self.presentation_observation_settles(observation, create)
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
                    return Ok(None);
                };
                if self.effects.mark_observed_applied(
                    provider,
                    show,
                    binding,
                    observation.inventory_generation(),
                ) != EffectTransition::Applied
                {
                    return Ok(None);
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
            NativeBringupPhase::AwaitingHidden { .. }
            | NativeBringupPhase::AwaitingPreShowPresentation { .. }
            | NativeBringupPhase::AwaitingShowAcknowledgement { .. }
            | NativeBringupPhase::AwaitingVisible { .. }
            | NativeBringupPhase::AwaitingPostShowPresentation { .. } => None,
        };
        Ok(next)
    }

    pub(super) fn observe_native_bringup_presentation(
        &mut self,
        binding: ViewportBinding,
        owner: NativeStagingOwner,
        phase: NativeBringupPhase,
        presented: PresentedNativeStagingPresentation,
    ) -> Result<NativeBringupPresentationOutcome, ViewportCoordinatorError> {
        let request = presented.request();
        if request.authority_domain() != self.authority_domain
            || request.binding() != binding
            || request.owner() != owner
            || phase.staging_presentation() != Some(request)
            || !self.native_staging_basis_matches_current(request)
        {
            return Ok(NativeBringupPresentationOutcome::Ignored);
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
                    return Ok(NativeBringupPresentationOutcome::Ignored);
                };
                let Some(observation) = record.presentation_observation() else {
                    return Ok(NativeBringupPresentationOutcome::Ignored);
                };
                if observation.known_state() != Some(WindowPresentationState::Hidden)
                    || observation.generation() != hidden_generation
                    || observation.inventory_generation() < hidden_inventory_generation
                    || record.coordinate_generation() != hidden_coordinate_generation
                    || record.coordinate_observation_generation()
                        < Some(hidden_coordinate_observation_generation)
                {
                    return Ok(NativeBringupPresentationOutcome::Ignored);
                }
                let show = self
                    .effects
                    .request(PlatformEffect::ShowWindow {
                        binding,
                        after_hidden: hidden_generation,
                        after_pre_show: presented,
                    })
                    .map_err(ViewportCoordinatorError::Effect)?;
                Ok(NativeBringupPresentationOutcome::Advanced(
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
                    return Ok(NativeBringupPresentationOutcome::Ignored);
                };
                let Some(observation) = record.presentation_observation() else {
                    return Ok(NativeBringupPresentationOutcome::Ignored);
                };
                if observation.known_state() != Some(WindowPresentationState::Visible)
                    || observation.generation() != visibility.visible_generation
                    || observation.inventory_generation() < visibility.visible_inventory_generation
                    || record.coordinate_generation() != visibility.visible_coordinate_generation
                    || record.coordinate_observation_generation()
                        < Some(visibility.visible_coordinate_observation_generation)
                {
                    return Ok(NativeBringupPresentationOutcome::Ignored);
                }
                Ok(NativeBringupPresentationOutcome::Ready(
                    NativeVisibleProof {
                        visibility,
                        post_show: presented,
                    },
                ))
            }
            NativeBringupPhase::AwaitingHidden { .. }
            | NativeBringupPhase::AwaitingPreShowPresentation { .. }
            | NativeBringupPhase::AwaitingShowAcknowledgement { .. }
            | NativeBringupPhase::AwaitingVisible { .. }
            | NativeBringupPhase::AwaitingPostShowPresentation { .. } => {
                Ok(NativeBringupPresentationOutcome::Ignored)
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
