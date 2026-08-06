use std::collections::BTreeMap;

use crate::effect::{EffectId, EffectInvalidation, EffectPhase, EffectRecord, PlatformEffect};
use crate::ids::{NativeCreateSagaId, SurfaceId};
use crate::interaction::PreparedNativeTearOff;
use crate::platform::WindowPresentationState;
use crate::platform_provider::PlatformObservationLease;
use crate::presentation_observation::{
    NativeStagingOwner, NativeStagingPresentation, NativeStagingResourceDescriptor,
    NativeStagingResourceId, PresentedNativeStagingPresentation,
};
use crate::viewport::{
    CoordinateGeneration, CoordinateObservationGeneration, InventoryGeneration,
    PresentationObservationGeneration, ViewportBinding, ViewportRole,
};
use crate::viewport_registry::{ViewportLifecycle, ViewportRecord};

use super::binding_cleanup::{BindingCleanupPurpose, BindingCleanupRequest};
use super::native_bringup::{NativeBringupPhase, NativeBringupPresentationOutcome};
use super::native_staging_resource::NativeStagingResourceOwner;
use super::{
    BindingCleanupAction, BindingRetirementOrigin, BindingRetirementStatus, NativeVisibilityProof,
    NativeVisibleProof, RecoveryPendingStatus, ViewportCoordinator, ViewportCoordinatorError,
    ViewportLifecycleAction,
};

/// Queryable owner-specific phase of one native create saga.
///
/// The shared hidden-to-post-show proof is converted into this public shape,
/// while ownership transfer and first-live admission remain native-create-only
/// states. Recovery replacement cannot represent either terminal phase.
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
    /// Both staging outputs were presented and graph ownership may advance.
    AwaitingOwnershipTransfer {
        proof: NativeVisibleProof,
    },
    /// Graph ownership moved, but the exact first live output is still pending.
    AwaitingFirstLivePresentation {
        proof: NativeVisibleProof,
    },
}

impl NativeCreatePhase {
    fn bringup(self) -> Option<NativeBringupPhase> {
        match self {
            Self::AwaitingHidden { create } => Some(NativeBringupPhase::AwaitingHidden { create }),
            Self::AwaitingPreShowPresentation {
                create,
                hidden_generation,
                hidden_inventory_generation,
                hidden_coordinate_generation,
                hidden_coordinate_observation_generation,
                create_emitted_inventory_generation,
                presentation,
            } => Some(NativeBringupPhase::AwaitingPreShowPresentation {
                create,
                hidden_generation,
                hidden_inventory_generation,
                hidden_coordinate_generation,
                hidden_coordinate_observation_generation,
                create_emitted_inventory_generation,
                presentation,
            }),
            Self::AwaitingShowAcknowledgement {
                create,
                show,
                hidden_generation,
                hidden_inventory_generation,
                hidden_coordinate_generation,
                hidden_coordinate_observation_generation,
                create_emitted_inventory_generation,
                pre_show,
            } => Some(NativeBringupPhase::AwaitingShowAcknowledgement {
                create,
                show,
                hidden_generation,
                hidden_inventory_generation,
                hidden_coordinate_generation,
                hidden_coordinate_observation_generation,
                create_emitted_inventory_generation,
                pre_show,
            }),
            Self::AwaitingVisible {
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
            } => Some(NativeBringupPhase::AwaitingVisible {
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
            }),
            Self::AwaitingPostShowPresentation {
                visibility,
                presentation,
            } => Some(NativeBringupPhase::AwaitingPostShowPresentation {
                visibility,
                presentation,
            }),
            Self::AwaitingOwnershipTransfer { .. } | Self::AwaitingFirstLivePresentation { .. } => {
                None
            }
        }
    }

    /// Returns the lifecycle effect which correlates presentation facts for this phase.
    #[must_use]
    pub const fn presentation_correlation_effect(self) -> EffectId {
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

    pub(super) const fn show_effect(self) -> Option<EffectId> {
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

    const fn staging_presentation(self) -> Option<NativeStagingPresentation> {
        match self {
            Self::AwaitingPreShowPresentation { presentation, .. }
            | Self::AwaitingPostShowPresentation { presentation, .. } => Some(presentation),
            Self::AwaitingHidden { .. }
            | Self::AwaitingShowAcknowledgement { .. }
            | Self::AwaitingVisible { .. }
            | Self::AwaitingOwnershipTransfer { .. }
            | Self::AwaitingFirstLivePresentation { .. } => None,
        }
    }
}

impl From<NativeBringupPhase> for NativeCreatePhase {
    fn from(phase: NativeBringupPhase) -> Self {
        match phase {
            NativeBringupPhase::AwaitingHidden { create } => Self::AwaitingHidden { create },
            NativeBringupPhase::AwaitingPreShowPresentation {
                create,
                hidden_generation,
                hidden_inventory_generation,
                hidden_coordinate_generation,
                hidden_coordinate_observation_generation,
                create_emitted_inventory_generation,
                presentation,
            } => Self::AwaitingPreShowPresentation {
                create,
                hidden_generation,
                hidden_inventory_generation,
                hidden_coordinate_generation,
                hidden_coordinate_observation_generation,
                create_emitted_inventory_generation,
                presentation,
            },
            NativeBringupPhase::AwaitingShowAcknowledgement {
                create,
                show,
                hidden_generation,
                hidden_inventory_generation,
                hidden_coordinate_generation,
                hidden_coordinate_observation_generation,
                create_emitted_inventory_generation,
                pre_show,
            } => Self::AwaitingShowAcknowledgement {
                create,
                show,
                hidden_generation,
                hidden_inventory_generation,
                hidden_coordinate_generation,
                hidden_coordinate_observation_generation,
                create_emitted_inventory_generation,
                pre_show,
            },
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
            } => Self::AwaitingVisible {
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
            },
            NativeBringupPhase::AwaitingPostShowPresentation {
                visibility,
                presentation,
            } => Self::AwaitingPostShowPresentation {
                visibility,
                presentation,
            },
        }
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
        let native_create = self
            .native_creates
            .sagas
            .values()
            .filter_map(|saga| saga.phase.staging_presentation());
        let recovery = self
            .recovery_replacements
            .values()
            .filter_map(|pending| pending.bringup_phase()?.staging_presentation());
        native_create
            .chain(recovery)
            .filter(|presentation| self.native_staging_presentation_is_current(*presentation))
    }

    /// Returns recovery surfaces whose live presentation and interaction authority is suspended.
    pub(crate) fn suspended_native_presentation_surfaces(
        &self,
    ) -> impl Iterator<Item = SurfaceId> + '_ {
        self.recovery_replacements
            .values()
            .filter(|pending| pending.suspends_live_presentation())
            .map(|pending| pending.destroyed_binding().surface())
    }

    /// Returns every retained source resource still owned by native lifecycle state.
    pub(crate) fn retained_native_staging_resources(
        &self,
    ) -> impl ExactSizeIterator<Item = &NativeStagingResourceDescriptor> {
        self.native_staging_resources.iter()
    }

    pub(super) fn native_staging_presentation_is_current(
        &self,
        presentation: NativeStagingPresentation,
    ) -> bool {
        let owner_is_current = match presentation.owner() {
            NativeStagingOwner::NativeCreate { resource } => {
                let saga = resource.saga();
                self.native_creates.sagas.get(&saga).is_some_and(|create| {
                    create.binding == presentation.binding()
                        && create.resource == resource
                        && create.phase.staging_presentation() == Some(presentation)
                })
            }
            NativeStagingOwner::RecoveryReplacement {
                destroyed_binding,
                recovery_obligation,
                retained_resource,
            } => self
                .recovery_replacements
                .pending(destroyed_binding.surface())
                .is_some_and(|pending| {
                    pending.destroyed_binding() == destroyed_binding
                        && pending.recovery_obligation() == recovery_obligation
                        && pending.retained_staging_resource() == retained_resource
                        && pending.replacement_binding() == Some(presentation.binding())
                        && pending
                            .bringup_phase()
                            .and_then(NativeBringupPhase::staging_presentation)
                            == Some(presentation)
                }),
        };
        owner_is_current && self.native_staging_basis_matches_current(presentation)
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
        let saga = self
            .native_creates
            .sagas
            .get(&saga_id)
            .ok_or(ViewportCoordinatorError::MissingCreateSaga { saga: saga_id })?;
        let Some(phase) = saga.phase.bringup() else {
            return Ok(());
        };
        let owner = NativeStagingOwner::native_create(saga_id, saga.resource)
            .ok_or(ViewportCoordinatorError::InvalidNativeStagingResource { saga: saga_id })?;
        if let Some(next) = self.reduce_ready_native_bringup(provider, binding, owner, phase)? {
            self.native_creates
                .sagas
                .get_mut(&saga_id)
                .ok_or(ViewportCoordinatorError::MissingCreateSaga { saga: saga_id })?
                .phase = next.into();
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
        match request.owner() {
            NativeStagingOwner::NativeCreate { .. } => {
                self.observe_native_create_staging_presentation(presented)
            }
            NativeStagingOwner::RecoveryReplacement { .. } => {
                self.observe_recovery_staging_presentation(presented)
            }
        }
    }

    fn observe_native_create_staging_presentation(
        &mut self,
        presented: PresentedNativeStagingPresentation,
    ) -> Result<Option<ViewportLifecycleAction>, ViewportCoordinatorError> {
        let request = presented.request();
        if !self.native_staging_presentation_is_current(request) {
            return Ok(None);
        }
        let NativeStagingOwner::NativeCreate { resource } = request.owner() else {
            return Ok(None);
        };
        let saga_id = resource.saga();
        let Some(saga) = self.native_creates.sagas.get(&saga_id) else {
            return Ok(None);
        };
        let Some(phase) = saga.phase.bringup() else {
            return Ok(None);
        };
        let prepared = saga.prepared.clone();
        let binding = saga.binding;
        let owner = request.owner();
        match self.observe_native_bringup_presentation(binding, owner, phase, presented)? {
            NativeBringupPresentationOutcome::Ignored => Ok(None),
            NativeBringupPresentationOutcome::Advanced(next) => {
                self.native_creates
                    .sagas
                    .get_mut(&saga_id)
                    .ok_or(ViewportCoordinatorError::MissingCreateSaga { saga: saga_id })?
                    .phase = next.into();
                Ok(None)
            }
            NativeBringupPresentationOutcome::Ready(proof) => {
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
        }
    }

    fn observe_recovery_staging_presentation(
        &mut self,
        presented: PresentedNativeStagingPresentation,
    ) -> Result<Option<ViewportLifecycleAction>, ViewportCoordinatorError> {
        let request = presented.request();
        if !self.native_staging_presentation_is_current(request) {
            return Ok(None);
        }
        let NativeStagingOwner::RecoveryReplacement {
            destroyed_binding,
            recovery_obligation,
            retained_resource,
        } = request.owner()
        else {
            return Ok(None);
        };
        let surface = destroyed_binding.surface();
        let Some(pending) = self.recovery_replacements.pending(surface) else {
            return Ok(None);
        };
        let Some(phase) = pending.bringup_phase() else {
            return Ok(None);
        };
        let binding = request.binding();
        match self.observe_native_bringup_presentation(
            binding,
            request.owner(),
            phase,
            presented,
        )? {
            NativeBringupPresentationOutcome::Ignored => Ok(None),
            NativeBringupPresentationOutcome::Advanced(next) => {
                self.recovery_replacements
                    .update_bringup(binding, phase, next)
                    .map_err(super::recovery_replacement_error)?;
                Ok(None)
            }
            NativeBringupPresentationOutcome::Ready(proof) => {
                if !self.replacement_is_admissible(binding) {
                    return Ok(None);
                }
                let mut staging_resources = self.native_staging_resources.clone();
                let mut recovery_replacements = self.recovery_replacements.clone();
                if let Some(resource) = retained_resource {
                    staging_resources
                        .transition(
                            resource,
                            NativeStagingResourceOwner::SurfaceRecovery {
                                obligation: recovery_obligation,
                                binding: destroyed_binding,
                            },
                            NativeStagingResourceOwner::RecoveryReplacementFirstLive {
                                obligation: recovery_obligation,
                                binding,
                            },
                        )
                        .map_err(ViewportCoordinatorError::from)?;
                }
                recovery_replacements
                    .mark_awaiting_first_live(binding, phase, proof)
                    .map_err(super::recovery_replacement_error)?;
                self.native_staging_resources = staging_resources;
                self.recovery_replacements = recovery_replacements;
                Ok(Some(ViewportLifecycleAction::RecoveryReplacementReady {
                    destroyed_binding,
                    replacement_binding: binding,
                    recovery_obligation,
                }))
            }
        }
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
            || proof.retained_resource() != Some(saga.resource)
            || visibility.pre_show.retained_resource() != Some(saga.resource)
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
        if !self.native_staging_basis_matches_current(proof.post_show.request()) {
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
            NativeStagingResourceOwner::BindingCleanup(binding),
        )?;
        self.begin_binding_cleanup(BindingCleanupRequest {
            binding,
            role: facts.role(),
            ownership: facts.ownership(),
            origin: BindingRetirementOrigin::NativeCreateAborted { create },
            status: BindingRetirementStatus::AwaitingAppearance,
            observed,
            input_observations: facts.input_observations(),
            close_observations: facts.close_observations(),
            may_reappear,
            cleanup: BindingCleanupAction::CompensateCreate { create },
            retained_staging_resource: Some(resource),
            purpose: BindingCleanupPurpose::Retired,
        })?;
        let _ = self.reconcile_pointer_passthrough_saga(binding)?;
        self.drive_binding_cleanup(binding)
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
            NativeStagingResourceOwner::BindingCleanup(_) => return Ok(()),
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
                + self.binding_cleanup.len(),
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
            self.binding_cleanup
                .iter()
                .filter_map(|(binding, retirement)| {
                    retirement.retained_staging_resource().map(|resource| {
                        (
                            resource,
                            NativeStagingResourceOwner::BindingCleanup(*binding),
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
