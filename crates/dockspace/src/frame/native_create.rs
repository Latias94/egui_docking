use std::collections::BTreeMap;

use crate::effect::{
    EffectId, EffectInvalidation, EffectPhase, EffectRecord, EffectTransition, PlatformEffect,
};
use crate::ids::NativeCreateSagaId;
use crate::interaction::PreparedNativeTearOff;
use crate::platform::{WindowPresentationObservation, WindowPresentationState};
use crate::platform_provider::PlatformObservationLease;
use crate::presentation_observation::{
    NativeStagingBasis, NativeStagingPresentation, NativeStagingPresentationPhase,
    NativeStagingResourceDescriptor, NativeStagingResourceId, PresentedNativeStagingPresentation,
};
use crate::viewport::{
    CoordinateGeneration, CoordinateObservationGeneration, InventoryGeneration,
    PresentationObservationGeneration, ViewportBinding, ViewportRole,
};
use crate::viewport_registry::{ViewportLifecycle, ViewportRecord};

use super::binding_retirement::BindingRetirementRequest;
use super::native_staging_resource::NativeStagingResourceOwner;
use super::{
    BindingRetirementCleanup, BindingRetirementOrigin, BindingRetirementStatus,
    RecoveryPendingStatus, ViewportCoordinator, ViewportCoordinatorError, ViewportLifecycleAction,
};

/// Active phase of one native create saga.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NativeCreatePhase {
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
    /// Both staging outputs were presented and ownership may be transferred.
    AwaitingOwnershipTransfer {
        proof: NativeVisibleProof,
    },
    /// Ownership moved, but the exact first live docking output is still pending.
    AwaitingFirstLivePresentation {
        proof: NativeVisibleProof,
    },
}

impl NativeCreatePhase {
    pub(crate) fn show_effect(self) -> Option<EffectId> {
        match self {
            Self::AwaitingShowAcknowledgement { show, .. } | Self::AwaitingVisible { show, .. } => {
                Some(show)
            }
            Self::AwaitingPostShowPresentation { visibility, .. } => Some(visibility.show),
            Self::AwaitingOwnershipTransfer { proof }
            | Self::AwaitingFirstLivePresentation { proof } => Some(proof.show()),
            Self::AwaitingHidden { .. } | Self::AwaitingPreShowPresentation { .. } => None,
        }
    }

    /// Returns the lifecycle effect an ordinary presentation observation must acknowledge.
    #[must_use]
    pub const fn acknowledged_effect(self) -> EffectId {
        match self {
            Self::AwaitingHidden { create } | Self::AwaitingPreShowPresentation { create, .. } => {
                create
            }
            Self::AwaitingShowAcknowledgement { show, .. } | Self::AwaitingVisible { show, .. } => {
                show
            }
            Self::AwaitingPostShowPresentation { visibility, .. } => visibility.show,
            Self::AwaitingOwnershipTransfer { proof }
            | Self::AwaitingFirstLivePresentation { proof } => proof.show(),
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

/// Exact presentation proof which authorizes one native ownership transfer.
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

    /// Returns the retained staging resource proven by both staging outputs.
    #[must_use]
    pub const fn resource(self) -> NativeStagingResourceId {
        self.post_show.resource()
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

/// Queryable native create saga.
///
/// Source ownership remains unchanged through
/// [`NativeCreatePhase::AwaitingPostShowPresentation`]. After transfer, the
/// saga remains queryable until the exact first live target output admits
/// routing, focus, accessibility, and source-resource retirement.
#[derive(Debug, Clone, PartialEq)]
pub struct NativeCreateSaga {
    pub(super) id: NativeCreateSagaId,
    pub(super) binding: ViewportBinding,
    pub(super) create: EffectId,
    pub(super) resource: NativeStagingResourceId,
    pub(super) prepared: PreparedNativeTearOff,
    pub(super) phase: NativeCreatePhase,
}

/// Identity returned when one native create effect enters the ledger.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct NativeCreateRequest {
    pub(super) saga: NativeCreateSagaId,
    pub(super) effect: EffectId,
    pub(super) binding: ViewportBinding,
}

#[derive(Debug, Clone, PartialEq)]
pub(super) struct NativeCreateState {
    last_saga: NativeCreateSagaId,
    sagas: BTreeMap<NativeCreateSagaId, NativeCreateSaga>,
}

impl Default for NativeCreateState {
    fn default() -> Self {
        Self {
            last_saga: NativeCreateSagaId::new(0),
            sagas: BTreeMap::new(),
        }
    }
}

impl NativeCreateState {
    pub(super) fn extend_referenced_effects(
        &self,
        effects: &mut std::collections::BTreeSet<EffectId>,
    ) {
        for saga in self.sagas.values() {
            effects.insert(saga.create);
            effects.extend(saga.phase.show_effect());
        }
    }
}

impl NativeCreateRequest {
    #[must_use]
    pub const fn saga(self) -> NativeCreateSagaId {
        self.saga
    }

    #[must_use]
    pub const fn effect(self) -> EffectId {
        self.effect
    }

    #[must_use]
    pub const fn binding(self) -> ViewportBinding {
        self.binding
    }
}

impl NativeCreateSaga {
    #[must_use]
    pub const fn id(&self) -> NativeCreateSagaId {
        self.id
    }

    #[must_use]
    pub const fn binding(&self) -> ViewportBinding {
        self.binding
    }

    #[must_use]
    pub const fn create(&self) -> EffectId {
        self.create
    }

    /// Returns the retained staging resource owned by this saga.
    #[must_use]
    pub const fn resource(&self) -> NativeStagingResourceId {
        self.resource
    }

    #[must_use]
    pub const fn prepared(&self) -> &PreparedNativeTearOff {
        &self.prepared
    }

    #[must_use]
    pub const fn phase(&self) -> NativeCreatePhase {
        self.phase
    }
}

impl ViewportCoordinator {
    /// Starts a non-idempotent native create saga without moving source content.
    pub(crate) fn start_native_create(
        &mut self,
        prepared: PreparedNativeTearOff,
    ) -> Result<NativeCreateRequest, ViewportCoordinatorError> {
        let capability = self.native_tear_off_capability();
        if !capability.is_supported() {
            return Err(ViewportCoordinatorError::NativeCapabilityUnavailable { capability });
        }
        let proposal = prepared.proposal();
        if !self.native_placement_is_current(proposal.placement()) {
            return Err(ViewportCoordinatorError::StalePlacementProof);
        }
        if proposal.converted_main().source_root() != proposal.root() {
            return Err(ViewportCoordinatorError::RecoveryRootMismatch);
        }

        let mut candidate = self.clone();
        let saga = candidate
            .native_creates
            .last_saga
            .checked_next()
            .ok_or(ViewportCoordinatorError::NativeCreateSagaExhausted)?;
        let resource = NativeStagingResourceId::mint(candidate.authority_domain, saga);
        let descriptor = NativeStagingResourceDescriptor::mint(
            resource,
            prepared.source_presentation(),
            prepared.payload().clone(),
        )
        .filter(|descriptor| {
            descriptor.source_presentation().surface() == prepared.source_surface()
        })
        .ok_or(ViewportCoordinatorError::InvalidNativeStagingResource { saga })?;
        let binding = candidate
            .registry
            .reserve(
                prepared.source_version().epoch(),
                proposal.surface(),
                ViewportRole::Child,
            )
            .map_err(ViewportCoordinatorError::Registry)?;
        let effect = candidate
            .effects
            .request(PlatformEffect::CreateWindow {
                binding,
                placement: proposal.physical_placement(),
                role: ViewportRole::Child,
            })
            .map_err(ViewportCoordinatorError::Effect)?;
        candidate.native_creates.last_saga = saga;
        candidate.insert_native_staging_resource(
            descriptor,
            NativeStagingResourceOwner::NativeCreateSaga(saga),
        )?;
        candidate.native_creates.sagas.insert(
            saga,
            NativeCreateSaga {
                id: saga,
                binding,
                create: effect,
                resource,
                prepared,
                phase: NativeCreatePhase::AwaitingHidden { create: effect },
            },
        );
        *self = candidate;
        Ok(NativeCreateRequest {
            saga,
            effect,
            binding,
        })
    }

    /// Returns every exact native staging pass currently requested by core.
    pub(crate) fn native_staging_presentations(
        &self,
    ) -> impl Iterator<Item = NativeStagingPresentation> + '_ {
        self.native_creates
            .sagas
            .values()
            .filter_map(|saga| match saga.phase {
                NativeCreatePhase::AwaitingPreShowPresentation { presentation, .. }
                | NativeCreatePhase::AwaitingPostShowPresentation { presentation, .. } => {
                    Some(presentation)
                }
                NativeCreatePhase::AwaitingHidden { .. }
                | NativeCreatePhase::AwaitingShowAcknowledgement { .. }
                | NativeCreatePhase::AwaitingVisible { .. }
                | NativeCreatePhase::AwaitingOwnershipTransfer { .. }
                | NativeCreatePhase::AwaitingFirstLivePresentation { .. } => None,
            })
            .filter(|presentation| self.native_staging_presentation_is_current(*presentation))
    }

    /// Returns every retained source resource still owned by native lifecycle state.
    pub(crate) fn retained_native_staging_resources(
        &self,
    ) -> impl ExactSizeIterator<Item = &NativeStagingResourceDescriptor> {
        self.native_staging_resources.iter()
    }

    fn mint_native_staging_presentation(
        &self,
        saga: NativeCreateSagaId,
        binding: ViewportBinding,
        phase: NativeStagingPresentationPhase,
        provider: PlatformObservationLease,
        presentation_generation: PresentationObservationGeneration,
        coordinate_generation: CoordinateGeneration,
    ) -> Result<NativeStagingPresentation, ViewportCoordinatorError> {
        let resource = self
            .native_creates
            .sagas
            .get(&saga)
            .map(|create| create.resource)
            .filter(|resource| self.native_staging_resources.get(*resource).is_some())
            .ok_or(ViewportCoordinatorError::InvalidNativeStagingResource { saga })?;
        let basis = NativeStagingBasis::new(
            provider,
            presentation_generation,
            coordinate_generation,
            resource,
        )
        .ok_or(ViewportCoordinatorError::InvalidNativeStagingResource { saga })?;
        NativeStagingPresentation::mint(binding, phase, basis)
            .ok_or(ViewportCoordinatorError::InvalidNativeStagingResource { saga })
    }

    fn native_staging_presentation_is_current(
        &self,
        presentation: NativeStagingPresentation,
    ) -> bool {
        let basis = presentation.basis();
        let Some(saga) = self.native_creates.sagas.get(&presentation.saga()) else {
            return false;
        };
        let phase_matches = match saga.phase {
            NativeCreatePhase::AwaitingPreShowPresentation {
                presentation: expected,
                ..
            }
            | NativeCreatePhase::AwaitingPostShowPresentation {
                presentation: expected,
                ..
            } => expected == presentation,
            NativeCreatePhase::AwaitingHidden { .. }
            | NativeCreatePhase::AwaitingShowAcknowledgement { .. }
            | NativeCreatePhase::AwaitingVisible { .. }
            | NativeCreatePhase::AwaitingOwnershipTransfer { .. }
            | NativeCreatePhase::AwaitingFirstLivePresentation { .. } => false,
        };
        if !phase_matches
            || saga.binding != presentation.binding()
            || saga.resource != basis.resource()
        {
            return false;
        }
        let expected_state = match presentation.phase() {
            NativeStagingPresentationPhase::PreShow => WindowPresentationState::Hidden,
            NativeStagingPresentationPhase::PostShow => WindowPresentationState::Visible,
        };
        self.native_staging_basis_matches_current(presentation, expected_state)
    }

    fn native_staging_basis_matches_current(
        &self,
        presentation: NativeStagingPresentation,
        expected_state: WindowPresentationState,
    ) -> bool {
        let basis = presentation.basis();
        if self.platform_provider() != Some(basis.platform_provider())
            || self
                .native_staging_resources
                .get(basis.resource())
                .is_none()
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
        observation.known_state() == Some(expected_state)
            && observation.generation() == basis.presentation_observation_generation()
            && record.coordinate_generation() == basis.coordinate_generation()
    }

    pub(super) fn reduce_ready_native_create(
        &mut self,
        provider: PlatformObservationLease,
        binding: ViewportBinding,
    ) -> Result<(), ViewportCoordinatorError> {
        let Some(saga_id) = self
            .native_creates
            .sagas
            .iter()
            .find_map(|(id, saga)| (saga.binding == binding).then_some(*id))
        else {
            return Ok(());
        };
        let phase = (|| {
            let saga = self
                .native_creates
                .sagas
                .get(&saga_id)
                .ok_or(ViewportCoordinatorError::MissingCreateSaga { saga: saga_id })?;
            Ok::<_, ViewportCoordinatorError>(saga.phase)
        })()?;
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
            return Ok(());
        };
        match phase {
            NativeCreatePhase::AwaitingHidden { create }
                if observation.known_state() == Some(WindowPresentationState::Hidden)
                    && self.presentation_observation_settles(observation, create) =>
            {
                let Some(create_emitted_inventory_generation) =
                    self.effect_emission_fence(create, binding)
                else {
                    return Ok(());
                };
                if self.effects.mark_observed_applied(
                    provider,
                    create,
                    binding,
                    observation.inventory_generation(),
                ) != EffectTransition::Applied
                {
                    return Ok(());
                }
                let presentation = self.mint_native_staging_presentation(
                    saga_id,
                    binding,
                    NativeStagingPresentationPhase::PreShow,
                    provider,
                    observation.generation(),
                    coordinate_generation,
                )?;
                self.native_creates
                    .sagas
                    .get_mut(&saga_id)
                    .ok_or(ViewportCoordinatorError::MissingCreateSaga { saga: saga_id })?
                    .phase = NativeCreatePhase::AwaitingPreShowPresentation {
                    create,
                    hidden_generation: observation.generation(),
                    hidden_inventory_generation: observation.inventory_generation(),
                    hidden_coordinate_generation: coordinate_generation,
                    hidden_coordinate_observation_generation: coordinate_observation_generation,
                    create_emitted_inventory_generation,
                    presentation,
                };
            }
            NativeCreatePhase::AwaitingPreShowPresentation {
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
                    saga_id,
                    binding,
                    NativeStagingPresentationPhase::PreShow,
                    provider,
                    observation.generation(),
                    coordinate_generation,
                )?;
                self.native_creates
                    .sagas
                    .get_mut(&saga_id)
                    .ok_or(ViewportCoordinatorError::MissingCreateSaga { saga: saga_id })?
                    .phase = NativeCreatePhase::AwaitingPreShowPresentation {
                    create,
                    hidden_generation: observation.generation(),
                    hidden_inventory_generation: observation.inventory_generation(),
                    hidden_coordinate_generation: coordinate_generation,
                    hidden_coordinate_observation_generation: coordinate_observation_generation,
                    create_emitted_inventory_generation,
                    presentation,
                };
            }
            NativeCreatePhase::AwaitingShowAcknowledgement {
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
                    return Ok(());
                };
                if self.effects.mark_observed_applied(
                    provider,
                    show,
                    binding,
                    observation.inventory_generation(),
                ) != EffectTransition::Applied
                {
                    return Ok(());
                }
                self.native_creates
                    .sagas
                    .get_mut(&saga_id)
                    .ok_or(ViewportCoordinatorError::MissingCreateSaga { saga: saga_id })?
                    .phase = NativeCreatePhase::AwaitingVisible {
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
                };
            }
            NativeCreatePhase::AwaitingVisible {
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
                    saga_id,
                    binding,
                    NativeStagingPresentationPhase::PostShow,
                    provider,
                    observation.generation(),
                    coordinate_generation,
                )?;
                self.native_creates
                    .sagas
                    .get_mut(&saga_id)
                    .ok_or(ViewportCoordinatorError::MissingCreateSaga { saga: saga_id })?
                    .phase = NativeCreatePhase::AwaitingPostShowPresentation {
                    visibility,
                    presentation,
                };
            }
            NativeCreatePhase::AwaitingPostShowPresentation {
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
                    saga_id,
                    binding,
                    NativeStagingPresentationPhase::PostShow,
                    provider,
                    observation.generation(),
                    coordinate_generation,
                )?;
                self.native_creates
                    .sagas
                    .get_mut(&saga_id)
                    .ok_or(ViewportCoordinatorError::MissingCreateSaga { saga: saga_id })?
                    .phase = NativeCreatePhase::AwaitingPostShowPresentation {
                    visibility,
                    presentation,
                };
            }
            NativeCreatePhase::AwaitingHidden { .. }
            | NativeCreatePhase::AwaitingPreShowPresentation { .. }
            | NativeCreatePhase::AwaitingShowAcknowledgement { .. }
            | NativeCreatePhase::AwaitingVisible { .. }
            | NativeCreatePhase::AwaitingPostShowPresentation { .. }
            | NativeCreatePhase::AwaitingOwnershipTransfer { .. }
            | NativeCreatePhase::AwaitingFirstLivePresentation { .. } => {}
        }
        Ok(())
    }

    /// Applies one exact final-presentation proof for a native staging pass.
    pub(crate) fn observe_native_staging_presentation(
        &mut self,
        presented: PresentedNativeStagingPresentation,
    ) -> Result<Option<ViewportLifecycleAction>, ViewportCoordinatorError> {
        let request = presented.request();
        if request.authority_domain() != self.authority_domain {
            return Ok(None);
        }
        let saga_id = request.saga();
        let Some(saga) = self.native_creates.sagas.get(&saga_id) else {
            return Ok(None);
        };
        if saga.binding != request.binding()
            || saga.resource != request.resource()
            || !self.native_staging_presentation_is_current(request)
        {
            return Ok(None);
        }
        let phase = saga.phase;
        let prepared = saga.prepared.clone();
        let binding = saga.binding;
        let current = self
            .registry
            .record(binding.surface())
            .filter(|record| record.binding() == binding);

        match phase {
            NativeCreatePhase::AwaitingPreShowPresentation {
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
                    return Ok(None);
                };
                let Some(observation) = record.presentation_observation() else {
                    return Ok(None);
                };
                if observation.known_state() != Some(WindowPresentationState::Hidden)
                    || observation.generation() != hidden_generation
                    || observation.inventory_generation() < hidden_inventory_generation
                    || record.coordinate_generation() != hidden_coordinate_generation
                    || record.coordinate_observation_generation()
                        < Some(hidden_coordinate_observation_generation)
                {
                    return Ok(None);
                }
                let show = self
                    .effects
                    .request(PlatformEffect::ShowWindow {
                        binding,
                        after_hidden: hidden_generation,
                        after_pre_show: presented,
                    })
                    .map_err(ViewportCoordinatorError::Effect)?;
                self.native_creates
                    .sagas
                    .get_mut(&saga_id)
                    .ok_or(ViewportCoordinatorError::MissingCreateSaga { saga: saga_id })?
                    .phase = NativeCreatePhase::AwaitingShowAcknowledgement {
                    create,
                    show,
                    hidden_generation,
                    hidden_inventory_generation,
                    hidden_coordinate_generation,
                    hidden_coordinate_observation_generation,
                    create_emitted_inventory_generation,
                    pre_show: presented,
                };
                Ok(None)
            }
            NativeCreatePhase::AwaitingPostShowPresentation {
                visibility,
                presentation,
            } if presentation == request
                && request.phase() == NativeStagingPresentationPhase::PostShow
                && visibility.pre_show.resource() == presented.resource() =>
            {
                let Some(record) = current else {
                    return Ok(None);
                };
                let Some(observation) = record.presentation_observation() else {
                    return Ok(None);
                };
                if observation.known_state() != Some(WindowPresentationState::Visible)
                    || observation.generation() != visibility.visible_generation
                    || observation.inventory_generation() < visibility.visible_inventory_generation
                    || record.coordinate_generation() != visibility.visible_coordinate_generation
                    || record.coordinate_observation_generation()
                        < Some(visibility.visible_coordinate_observation_generation)
                {
                    return Ok(None);
                }
                let proof = NativeVisibleProof {
                    visibility,
                    post_show: presented,
                };
                self.native_creates
                    .sagas
                    .get_mut(&saga_id)
                    .ok_or(ViewportCoordinatorError::MissingCreateSaga { saga: saga_id })?
                    .phase = NativeCreatePhase::AwaitingOwnershipTransfer { proof };
                Ok(Some(ViewportLifecycleAction::TransferNativeCreate {
                    saga: saga_id,
                    prepared: Box::new(prepared),
                    proof,
                }))
            }
            NativeCreatePhase::AwaitingHidden { .. }
            | NativeCreatePhase::AwaitingPreShowPresentation { .. }
            | NativeCreatePhase::AwaitingShowAcknowledgement { .. }
            | NativeCreatePhase::AwaitingVisible { .. }
            | NativeCreatePhase::AwaitingPostShowPresentation { .. }
            | NativeCreatePhase::AwaitingOwnershipTransfer { .. }
            | NativeCreatePhase::AwaitingFirstLivePresentation { .. } => Ok(None),
        }
    }

    fn presentation_observation_settles(
        &self,
        observation: WindowPresentationObservation,
        effect: EffectId,
    ) -> bool {
        observation.acknowledges(effect)
            && self.presentation_observation_is_after_emission(observation, effect)
    }

    fn presentation_observation_is_after_emission(
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

    fn effect_emission_fence(
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

    pub(crate) fn transfer_native_create(
        &mut self,
        saga_id: NativeCreateSagaId,
        proof: NativeVisibleProof,
    ) -> Result<ViewportBinding, ViewportCoordinatorError> {
        let saga = self
            .native_creates
            .sagas
            .get(&saga_id)
            .ok_or(ViewportCoordinatorError::MissingCreateSaga { saga: saga_id })?;
        let NativeCreatePhase::AwaitingOwnershipTransfer { proof: expected } = saga.phase else {
            return Err(ViewportCoordinatorError::CreateSagaNotReady { saga: saga_id });
        };
        let visibility = proof.visibility;
        if proof != expected
            || visibility.binding != saga.binding
            || proof.resource() != saga.resource
            || visibility.pre_show.resource() != saga.resource
            || visibility.visible_generation <= visibility.acknowledged_generation
            || visibility.acknowledged_generation <= visibility.hidden_generation
            // Coordinate generations are geometry-authority generations, not
            // per-observation ticks. Equal values are valid when the native
            // rectangle stayed unchanged; a later stage may never regress to
            // an older geometry authority.
            || visibility.acknowledged_coordinate_generation
                < visibility.hidden_coordinate_generation
            || visibility.visible_coordinate_generation
                < visibility.acknowledged_coordinate_generation
            || visibility.acknowledged_coordinate_observation_generation
                < visibility.hidden_coordinate_observation_generation
            || visibility.visible_coordinate_observation_generation
                < visibility.acknowledged_coordinate_observation_generation
            || visibility.visible_inventory_generation
                <= visibility.acknowledged_inventory_generation
            || visibility.acknowledged_inventory_generation
                <= visibility.hidden_inventory_generation
            || visibility.visible_inventory_generation
                <= visibility.show_emitted_inventory_generation
            || visibility.hidden_inventory_generation
                <= visibility.create_emitted_inventory_generation
            || visibility.acknowledged_inventory_generation
                <= visibility.show_emitted_inventory_generation
        {
            return Err(ViewportCoordinatorError::CreateSagaNotReady { saga: saga_id });
        }
        if !self.native_staging_basis_matches_current(
            proof.post_show.request(),
            WindowPresentationState::Visible,
        ) {
            return Err(ViewportCoordinatorError::CreateSagaNotReady { saga: saga_id });
        }
        let binding = saga.binding;
        let surface = binding.surface();
        let current_presentation = self
            .registry
            .record(surface)
            .filter(|record| record.binding() == binding)
            .and_then(ViewportRecord::presentation_observation);
        if !current_presentation.is_some_and(|observation| {
            observation.binding() == binding
                && observation.generation() == visibility.visible_generation
                && observation.inventory_generation() >= visibility.visible_inventory_generation
                && observation.known_state() == Some(WindowPresentationState::Visible)
        }) {
            return Err(ViewportCoordinatorError::CreateSagaNotReady { saga: saga_id });
        }
        let Some(record) = self.registry.record(surface) else {
            return Err(ViewportCoordinatorError::CreateSagaNotReady { saga: saga_id });
        };
        if record.coordinate_generation() != visibility.visible_coordinate_generation {
            return Err(ViewportCoordinatorError::CreateSagaNotReady { saga: saga_id });
        }
        if record.coordinate_observation_generation()
            < Some(visibility.visible_coordinate_observation_generation)
        {
            return Err(ViewportCoordinatorError::CreateSagaNotReady { saga: saga_id });
        }
        if self.effect_emission_fence(visibility.create, binding)
            != Some(visibility.create_emitted_inventory_generation)
            || self.effect_emission_fence(visibility.show, binding)
                != Some(visibility.show_emitted_inventory_generation)
        {
            return Err(ViewportCoordinatorError::CreateSagaNotReady { saga: saga_id });
        }
        let create_phase = self
            .effects
            .record(visibility.create)
            .map(EffectRecord::phase);
        let show_phase = self
            .effects
            .record(visibility.show)
            .map(EffectRecord::phase);
        let show_request_matches_pre_show =
            self.effects.record(visibility.show).is_some_and(|record| {
                matches!(
                    record.request().effect(),
                    PlatformEffect::ShowWindow {
                        binding: effect_binding,
                        after_pre_show,
                        ..
                    } if *effect_binding == binding && *after_pre_show == visibility.pre_show
                )
            });
        if !matches!(
            create_phase,
            Some(EffectPhase::ObservedApplied {
                inventory_generation
            }) if inventory_generation == visibility.hidden_inventory_generation
        ) || !matches!(
            show_phase,
            Some(EffectPhase::ObservedApplied {
                inventory_generation
            }) if inventory_generation == visibility.acknowledged_inventory_generation
        ) || !show_request_matches_pre_show
        {
            return Err(ViewportCoordinatorError::CreateSagaNotReady { saga: saga_id });
        }
        let mut candidate = self.clone();
        candidate.transition_native_staging_resource(
            saga.resource,
            NativeStagingResourceOwner::NativeCreateSaga(saga_id),
            NativeStagingResourceOwner::NativeCreateFirstLive(binding),
        )?;
        candidate
            .native_creates
            .sagas
            .get_mut(&saga_id)
            .ok_or(ViewportCoordinatorError::MissingCreateSaga { saga: saga_id })?
            .phase = NativeCreatePhase::AwaitingFirstLivePresentation { proof };
        *self = candidate;
        Ok(binding)
    }

    /// Admits a transferred native binding after the engine has accepted an
    /// exact presentation observation for its first live docking output.
    pub(crate) fn admit_native_create_first_live(
        &mut self,
        saga_id: NativeCreateSagaId,
        binding: ViewportBinding,
    ) -> Result<(), ViewportCoordinatorError> {
        let saga = self
            .native_creates
            .sagas
            .get(&saga_id)
            .ok_or(ViewportCoordinatorError::MissingCreateSaga { saga: saga_id })?;
        if saga.binding != binding
            || !matches!(
                saga.phase,
                NativeCreatePhase::AwaitingFirstLivePresentation { .. }
            )
        {
            return Err(ViewportCoordinatorError::CreateSagaNotReady { saga: saga_id });
        }

        let mut candidate = self.clone();
        candidate
            .registry
            .admit(binding)
            .map_err(ViewportCoordinatorError::Registry)?;
        let saga = candidate
            .native_creates
            .sagas
            .remove(&saga_id)
            .expect("the native create saga was validated above");
        candidate.release_native_staging_resource(
            saga.resource,
            NativeStagingResourceOwner::NativeCreateFirstLive(binding),
        )?;
        *self = candidate;
        Ok(())
    }

    pub(crate) fn reject_native_create(
        &mut self,
        saga_id: NativeCreateSagaId,
    ) -> Result<Option<EffectId>, ViewportCoordinatorError> {
        self.abort_native_create(saga_id)
    }

    pub(crate) fn cancel_native_create(
        &mut self,
        saga_id: NativeCreateSagaId,
    ) -> Result<Option<EffectId>, ViewportCoordinatorError> {
        self.abort_native_create(saga_id)
    }

    pub(super) fn abort_native_create(
        &mut self,
        saga_id: NativeCreateSagaId,
    ) -> Result<Option<EffectId>, ViewportCoordinatorError> {
        if self.native_creates.sagas.get(&saga_id).is_some_and(|saga| {
            matches!(
                saga.phase,
                NativeCreatePhase::AwaitingFirstLivePresentation { .. }
            )
        }) {
            return Err(ViewportCoordinatorError::CreateSagaAlreadyTransferred { saga: saga_id });
        }
        let saga = self
            .native_creates
            .sagas
            .remove(&saga_id)
            .ok_or(ViewportCoordinatorError::MissingCreateSaga { saga: saga_id })?;
        let binding = saga.binding;
        let create = saga.create;
        let resource = saga.resource;
        let create_may_have_executed = self.effects.record(create).is_some_and(|record| {
            record.was_emitted()
                && !matches!(
                    record.phase(),
                    EffectPhase::DispatchFailed(_)
                        | EffectPhase::Unsupported(_)
                        | EffectPhase::Invalidated { .. }
                )
        });
        if !create_may_have_executed {
            let _ = self
                .effects
                .invalidate_unemitted(create, EffectInvalidation::NativeCreateAborted);
        }
        if let Some(show) = saga.phase.show_effect() {
            let _ = self
                .effects
                .invalidate_unemitted(show, EffectInvalidation::NativeCreateAborted);
        }
        let facts = self
            .registry
            .detach(binding)
            .map_err(ViewportCoordinatorError::Registry)?;
        self.pointer_passthrough
            .prepare_binding_for_vacancy(binding);
        let observed =
            facts.ever_observed() && !matches!(facts.lifecycle(), ViewportLifecycle::Missing);
        let may_reappear = create_may_have_executed && !observed;
        if !observed && !may_reappear {
            self.terminate_pointer_passthrough_binding(binding);
            self.release_native_staging_resource(
                resource,
                NativeStagingResourceOwner::NativeCreateSaga(saga_id),
            )?;
            return Ok(None);
        }
        self.transition_native_staging_resource(
            resource,
            NativeStagingResourceOwner::NativeCreateSaga(saga_id),
            NativeStagingResourceOwner::BindingRetirement(binding),
        )?;
        self.begin_binding_retirement(BindingRetirementRequest {
            binding,
            role: facts.role(),
            ownership: facts.ownership(),
            origin: BindingRetirementOrigin::NativeCreateAborted { create },
            status: BindingRetirementStatus::AwaitingAppearance,
            observed,
            input_observations: facts.input_observations(),
            close_observations: facts.close_observations(),
            may_reappear,
            cleanup: BindingRetirementCleanup::CompensateCreate { create },
            retained_staging_resource: Some(resource),
        })?;
        let _ = self.reconcile_pointer_passthrough_saga(binding)?;
        self.drive_binding_retirement(binding)
    }

    #[must_use]
    pub fn native_create_saga(&self, saga: NativeCreateSagaId) -> Option<&NativeCreateSaga> {
        self.native_creates.sagas.get(&saga)
    }

    pub fn native_create_sagas(
        &self,
    ) -> impl Iterator<Item = (NativeCreateSagaId, &NativeCreateSaga)> {
        self.native_creates
            .sagas
            .iter()
            .map(|(id, saga)| (*id, saga))
    }

    pub(super) fn abortable_native_create_sagas(&self) -> Vec<NativeCreateSagaId> {
        self.native_creates
            .sagas
            .iter()
            .filter_map(|(id, saga)| {
                (!matches!(
                    saga.phase,
                    NativeCreatePhase::AwaitingFirstLivePresentation { .. }
                ))
                .then_some(*id)
            })
            .collect()
    }

    pub(super) fn native_create_snapshot_by_binding(
        &self,
    ) -> BTreeMap<ViewportBinding, NativeCreateSaga> {
        self.native_creates
            .sagas
            .values()
            .map(|saga| (saga.binding, saga.clone()))
            .collect()
    }

    pub(super) fn clear_active_native_creates(&mut self) {
        self.native_creates.sagas.clear();
    }

    pub(super) fn native_create_is_staging_binding(&self, binding: ViewportBinding) -> bool {
        self.native_creates.sagas.values().any(|saga| {
            saga.binding == binding
                && !matches!(
                    saga.phase,
                    NativeCreatePhase::AwaitingFirstLivePresentation { .. }
                )
        })
    }

    pub(super) fn abortable_native_create_for_binding(
        &self,
        binding: ViewportBinding,
    ) -> Option<NativeCreateSagaId> {
        self.native_creates.sagas.iter().find_map(|(id, saga)| {
            (saga.binding == binding
                && !matches!(
                    saga.phase,
                    NativeCreatePhase::AwaitingFirstLivePresentation { .. }
                ))
            .then_some(*id)
        })
    }

    pub(super) fn native_create_for_effect(&self, effect: EffectId) -> Option<NativeCreateSagaId> {
        self.native_creates.sagas.iter().find_map(|(id, saga)| {
            (saga.create == effect || saga.phase.show_effect() == Some(effect)).then_some(*id)
        })
    }

    pub(super) fn settle_destroyed_native_create(
        &mut self,
        binding: ViewportBinding,
    ) -> Result<Option<bool>, ViewportCoordinatorError> {
        let saga_id = self
            .native_creates
            .sagas
            .iter()
            .find_map(|(id, saga)| (saga.binding == binding).then_some(*id));
        let Some(saga_id) = saga_id else {
            return Ok(None);
        };
        let saga = self
            .native_creates
            .sagas
            .remove(&saga_id)
            .expect("the matching native create saga was just observed");
        let ownership_transferred = matches!(
            saga.phase,
            NativeCreatePhase::AwaitingFirstLivePresentation { .. }
        );
        let inventory_generation = self.registry.inventory_generation();
        let _ = self
            .effects
            .mark_destroyed(saga.create, binding, inventory_generation);
        if let Some(show) = saga.phase.show_effect() {
            let _ = self
                .effects
                .mark_destroyed(show, binding, inventory_generation);
        }
        if !ownership_transferred {
            self.release_native_staging_resource(
                saga.resource,
                NativeStagingResourceOwner::NativeCreateSaga(saga_id),
            )?;
        }
        Ok(Some(ownership_transferred))
    }

    pub(super) fn retire_first_live_native_create_for_vacancy(
        &mut self,
        binding: ViewportBinding,
    ) -> Option<(NativeCreateSagaId, NativeStagingResourceId)> {
        let saga = self
            .native_creates
            .sagas
            .iter()
            .find_map(|(saga, create)| {
                (create.binding == binding
                    && matches!(
                        create.phase,
                        NativeCreatePhase::AwaitingFirstLivePresentation { .. }
                    ))
                .then_some(*saga)
            })?;
        let create = self.native_creates.sagas.remove(&saga)?;
        Some((saga, create.resource))
    }

    pub(super) fn transition_native_staging_resource(
        &mut self,
        resource: NativeStagingResourceId,
        expected: NativeStagingResourceOwner,
        next: NativeStagingResourceOwner,
    ) -> Result<(), ViewportCoordinatorError> {
        self.native_staging_resources
            .transition(resource, expected, next)
            .map_err(ViewportCoordinatorError::from)
    }

    fn insert_native_staging_resource(
        &mut self,
        descriptor: NativeStagingResourceDescriptor,
        owner: NativeStagingResourceOwner,
    ) -> Result<(), ViewportCoordinatorError> {
        self.native_staging_resources
            .insert(descriptor, owner)
            .map_err(ViewportCoordinatorError::from)
    }

    pub(super) fn release_native_staging_resource(
        &mut self,
        resource: NativeStagingResourceId,
        expected: NativeStagingResourceOwner,
    ) -> Result<(), ViewportCoordinatorError> {
        self.native_staging_resources
            .release(resource, expected)
            .map(|_| ())
            .map_err(ViewportCoordinatorError::from)
    }

    #[must_use]
    pub(super) fn native_staging_resource_owner(
        &self,
        resource: NativeStagingResourceId,
    ) -> Option<NativeStagingResourceOwner> {
        self.native_staging_resources.owner(resource)
    }

    /// Releases a transferred resource only for its exact pre-admission binding.
    pub(crate) fn release_transferred_native_staging_resource(
        &mut self,
        resource: NativeStagingResourceId,
        binding: ViewportBinding,
    ) -> Result<(), ViewportCoordinatorError> {
        self.release_native_staging_resource(
            resource,
            NativeStagingResourceOwner::NativeCreateFirstLive(binding),
        )
    }

    /// Settles a resource whose logical surface vanished at the tick boundary.
    ///
    /// Binding retirement remains the sole owner until its exact native cleanup
    /// terminates. Every other lifecycle must match the bound recovery before
    /// the resource can be released.
    pub(crate) fn settle_vacated_native_staging_resource(
        &mut self,
        resource: NativeStagingResourceId,
        binding: ViewportBinding,
        recovery_obligation: crate::surface_recovery::SurfaceRecoveryObligationId,
    ) -> Result<(), ViewportCoordinatorError> {
        let owner = self.native_staging_resource_owner(resource).ok_or(
            super::native_staging_resource::NativeStagingResourceLedgerError::MissingResource {
                resource,
            },
        )?;
        let release = match owner {
            NativeStagingResourceOwner::NativeCreateFirstLive(owner_binding) => {
                owner_binding == binding
            }
            NativeStagingResourceOwner::SurfaceRecovery {
                obligation,
                binding: owner_binding,
            } => obligation == recovery_obligation && owner_binding == binding,
            NativeStagingResourceOwner::RecoveryReplacementFirstLive {
                obligation,
                binding: owner_binding,
            } => obligation == recovery_obligation && owner_binding.surface() == binding.surface(),
            NativeStagingResourceOwner::BindingRetirement(_) => return Ok(()),
            NativeStagingResourceOwner::NativeCreateSaga(_) => false,
        };
        if !release {
            return Err(ViewportCoordinatorError::NativeStagingResourceOwnerMismatch { resource });
        }
        self.release_native_staging_resource(resource, owner)
    }

    /// Validates retained-resource conservation after a complete lifecycle reduction.
    ///
    /// Callers must not invoke this between publishing frame actions and reducing
    /// those actions into the engine. A transferred binding may be moving from an
    /// active create saga into a pending recovery during that interval.
    pub(crate) fn validate_native_staging_resource_conservation(
        &self,
    ) -> Result<(), ViewportCoordinatorError> {
        let mut references = Vec::with_capacity(
            self.native_creates.sagas.len()
                + self.recovery_replacements.len()
                + self.binding_retirement.len(),
        );
        references.extend(self.native_creates.sagas.iter().map(|(saga, create)| {
            let owner = if matches!(
                create.phase,
                NativeCreatePhase::AwaitingFirstLivePresentation { .. }
            ) {
                NativeStagingResourceOwner::NativeCreateFirstLive(create.binding)
            } else {
                NativeStagingResourceOwner::NativeCreateSaga(*saga)
            };
            (create.resource, owner)
        }));
        for (surface, pending) in self.recovery_replacements.iter() {
            let Some(resource) = pending.retained_staging_resource() else {
                continue;
            };
            let owner = if matches!(
                pending.status(),
                RecoveryPendingStatus::AwaitingFirstLivePresentation
            ) {
                let binding = pending.replacement_binding().ok_or(
                    ViewportCoordinatorError::PendingRecoveryRegistrationMismatch {
                        surface: *surface,
                    },
                )?;
                NativeStagingResourceOwner::RecoveryReplacementFirstLive {
                    obligation: pending.recovery_obligation(),
                    binding,
                }
            } else {
                NativeStagingResourceOwner::SurfaceRecovery {
                    obligation: pending.recovery_obligation(),
                    binding: pending.destroyed_binding(),
                }
            };
            references.push((resource, owner));
        }
        references.extend(
            self.binding_retirement
                .iter()
                .filter_map(|(binding, retirement)| {
                    retirement.retained_staging_resource().map(|resource| {
                        (
                            resource,
                            NativeStagingResourceOwner::BindingRetirement(*binding),
                        )
                    })
                }),
        );

        self.native_staging_resources
            .validate_conservation(references)
            .map_err(ViewportCoordinatorError::from)
    }

    #[cfg(test)]
    pub(super) fn insert_native_staging_resource_for_test(
        &mut self,
        descriptor: NativeStagingResourceDescriptor,
        owner: NativeStagingResourceOwner,
    ) -> Result<(), ViewportCoordinatorError> {
        self.insert_native_staging_resource(descriptor, owner)
    }
}
