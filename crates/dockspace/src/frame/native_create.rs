use std::collections::BTreeMap;

use crate::command::{MovePayload, WorkspaceCommand};
use crate::effect::{EffectId, EffectInvalidation, EffectPhase, PlatformEffect};
use crate::geometry::PhysicalRect;
use crate::ids::{NativeCreateSagaId, RootId, SurfaceId, WorkspaceEpoch};
use crate::platform_provider::PlatformObservationLease;
use crate::presentation_observation::{
    NativeStagingOwner, NativeStagingResourceDescriptor, NativeStagingResourceId,
    PresentedSurfaceAuthority,
};
use crate::surface_recovery::{ConvertedMainRecovery, SurfaceRecoveryObligation};
use crate::viewport::{ViewportBinding, ViewportRole};
use crate::viewport_focus::FocusCausalStamp;
use crate::viewport_registry::{ViewportLifecycle, ViewportRecord};

use super::binding_cleanup::{BindingCleanupPurpose, BindingCleanupRequest};
use super::native_bringup::{NativePreparation, NativePreparationOutcome};
use super::native_staging_resource::NativeStagingResourceOwner;
use super::{
    BindingCleanupAction, BindingRetirementOrigin, BindingRetirementStatus, NativeReadyProof,
    ViewportCoordinator, ViewportCoordinatorError, ViewportLifecycleAction,
};

/// Placement and recovery identities shared by every native create entry point.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct NativeCreateProposal {
    surface: SurfaceId,
    physical_placement: PhysicalRect,
    converted_main: ConvertedMainRecovery,
}

impl NativeCreateProposal {
    pub(crate) const fn new(
        surface: SurfaceId,
        physical_placement: PhysicalRect,
        converted_main: ConvertedMainRecovery,
    ) -> Self {
        Self {
            surface,
            physical_placement,
            converted_main,
        }
    }

    pub(crate) fn from_tear_off(proposal: &crate::intent::NativeTearOffProposal) -> Self {
        Self::new(
            proposal.surface(),
            proposal.physical_placement(),
            proposal.converted_main(),
        )
    }

    pub(crate) const fn surface(&self) -> SurfaceId {
        self.surface
    }

    pub(crate) const fn root(&self) -> RootId {
        self.converted_main.source_root()
    }

    pub(crate) const fn physical_placement(&self) -> PhysicalRect {
        self.physical_placement
    }

    pub(crate) const fn converted_main(&self) -> ConvertedMainRecovery {
        self.converted_main
    }
}

/// Frozen source, mutation, recovery, and focus facts for one native create saga.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct PreparedNativeCreate {
    source_presentation: PresentedSurfaceAuthority,
    payload: MovePayload,
    source_epoch: WorkspaceEpoch,
    command: WorkspaceCommand,
    proposal: NativeCreateProposal,
    recovery_obligation: SurfaceRecoveryObligation,
    focus_causal: FocusCausalStamp,
}

impl PreparedNativeCreate {
    pub(crate) const fn new(
        source_presentation: PresentedSurfaceAuthority,
        payload: MovePayload,
        source_epoch: WorkspaceEpoch,
        command: WorkspaceCommand,
        proposal: NativeCreateProposal,
        recovery_obligation: SurfaceRecoveryObligation,
        focus_causal: FocusCausalStamp,
    ) -> Self {
        Self {
            source_presentation,
            payload,
            source_epoch,
            command,
            proposal,
            recovery_obligation,
            focus_causal,
        }
    }

    pub(crate) const fn source_surface(&self) -> SurfaceId {
        self.source_presentation.surface()
    }

    pub(crate) const fn source_presentation(&self) -> PresentedSurfaceAuthority {
        self.source_presentation
    }

    pub(crate) const fn payload(&self) -> &MovePayload {
        &self.payload
    }

    pub(crate) const fn source_epoch(&self) -> WorkspaceEpoch {
        self.source_epoch
    }

    pub(crate) const fn command(&self) -> &WorkspaceCommand {
        &self.command
    }

    pub(crate) const fn proposal(&self) -> &NativeCreateProposal {
        &self.proposal
    }

    pub(crate) const fn recovery_obligation(&self) -> &SurfaceRecoveryObligation {
        &self.recovery_obligation
    }

    pub(crate) const fn focus_causal(&self) -> FocusCausalStamp {
        self.focus_causal
    }
}

/// Queryable lifecycle of an exact host-prepared native create.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NativeCreatePhase {
    AwaitingPrepared { create: EffectId },
    AwaitingFirstLivePreparation { correlation: EffectId },
    AwaitingFirstLivePresentation { correlation: EffectId },
}
impl NativeCreatePhase {
    #[must_use]
    pub const fn presentation_correlation_effect(self) -> EffectId {
        match self {
            Self::AwaitingPrepared { create } => create,
            Self::AwaitingFirstLivePreparation { correlation }
            | Self::AwaitingFirstLivePresentation { correlation } => correlation,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum NativeCreateLifecycle {
    Preparing(NativePreparation),
    AwaitingFirstLivePreparation { proof: NativeReadyProof },
    AwaitingFirstLivePresentation { proof: NativeReadyProof },
}
impl NativeCreateLifecycle {
    const fn preparation(self) -> Option<NativePreparation> {
        match self {
            Self::Preparing(preparation) => Some(preparation),
            Self::AwaitingFirstLivePreparation { .. }
            | Self::AwaitingFirstLivePresentation { .. } => None,
        }
    }
    const fn query(self) -> NativeCreatePhase {
        match self {
            Self::Preparing(preparation) => NativeCreatePhase::AwaitingPrepared {
                create: preparation.create_effect(),
            },
            Self::AwaitingFirstLivePreparation { proof } => {
                NativeCreatePhase::AwaitingFirstLivePreparation {
                    correlation: proof.create(),
                }
            }
            Self::AwaitingFirstLivePresentation { proof } => {
                NativeCreatePhase::AwaitingFirstLivePresentation {
                    correlation: proof.create(),
                }
            }
        }
    }
}

/// Queryable native create saga.
///
/// Source ownership remains unchanged for every pending phase. The exact first
/// live target output atomically transfers ownership, admits the binding,
/// releases its staging resource and completes this saga.
#[derive(Debug, Clone, PartialEq)]
pub struct NativeCreateSaga {
    pub(super) id: NativeCreateSagaId,
    pub(super) binding: ViewportBinding,
    pub(super) create: EffectId,
    pub(super) resource: NativeStagingResourceId,
    pub(super) prepared: PreparedNativeCreate,
    phase: NativeCreateLifecycle,
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
    pub(crate) const fn prepared(&self) -> &PreparedNativeCreate {
        &self.prepared
    }

    #[must_use]
    pub const fn phase(&self) -> NativeCreatePhase {
        self.phase.query()
    }
}

impl ViewportCoordinator {
    /// Starts a non-idempotent native create saga without moving source content.
    pub(crate) fn start_native_create(
        &mut self,
        prepared: PreparedNativeCreate,
    ) -> Result<NativeCreateRequest, ViewportCoordinatorError> {
        let capability = self.native_exact_placement_create_capability();
        if !capability.is_supported() {
            return Err(ViewportCoordinatorError::NativeCapabilityUnavailable { capability });
        }
        let proposal = prepared.proposal();
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
                prepared.source_epoch(),
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
                phase: NativeCreateLifecycle::Preparing(NativePreparation::initial(effect)),
            },
        );
        *self = candidate;
        Ok(NativeCreateRequest {
            saga,
            effect,
            binding,
        })
    }

    /// Returns recovery surfaces whose live presentation and interaction authority is suspended.
    pub(crate) fn suspended_native_presentation_surfaces(
        &self,
    ) -> impl Iterator<Item = SurfaceId> + '_ {
        self.recovery_replacements.suspended_surfaces()
    }

    /// Returns every retained source resource still owned by native lifecycle state.
    pub(crate) fn retained_native_staging_resources(
        &self,
    ) -> impl ExactSizeIterator<Item = &NativeStagingResourceDescriptor> {
        self.native_staging_resources.iter()
    }

    pub(super) fn reduce_ready_native_create(
        &mut self,
        provider: PlatformObservationLease,
        binding: ViewportBinding,
    ) -> Result<Option<ViewportLifecycleAction>, ViewportCoordinatorError> {
        let Some(saga_id) = self
            .native_creates
            .sagas
            .iter()
            .find_map(|(id, saga)| (saga.binding == binding).then_some(*id))
        else {
            return Ok(None);
        };
        let saga = self
            .native_creates
            .sagas
            .get(&saga_id)
            .ok_or(ViewportCoordinatorError::MissingCreateSaga { saga: saga_id })?;
        let resource = saga.resource;
        let create = saga.create;
        let mut phase = saga.phase;
        let owner = NativeStagingOwner::native_create(saga_id, resource)
            .ok_or(ViewportCoordinatorError::InvalidNativeStagingResource { saga: saga_id })?;
        let preparation_failed = self
            .registry
            .record(binding.surface())
            .and_then(ViewportRecord::create_preparation)
            .is_some_and(|observation| {
                observation.known_readiness()
                    == Some(crate::platform::NativeCreateReadiness::Failed)
                    && self
                        .effect_emission_fence(create, binding)
                        .is_some_and(|emitted| observation.inventory_generation() > emitted)
            });
        if preparation_failed {
            self.abort_native_create(saga_id)?;
            return Ok(None);
        }
        if let NativeCreateLifecycle::AwaitingFirstLivePreparation { proof }
        | NativeCreateLifecycle::AwaitingFirstLivePresentation { proof } = phase
        {
            if self.native_ready_proof_is_current(proof) {
                return Ok(None);
            }
            if matches!(
                phase,
                NativeCreateLifecycle::AwaitingFirstLivePresentation { .. }
            ) {
                self.transition_native_staging_resource(
                    resource,
                    NativeStagingResourceOwner::NativeCreateFirstLive(binding),
                    NativeStagingResourceOwner::NativeCreateSaga(saga_id),
                )?;
            }
            phase = NativeCreateLifecycle::Preparing(proof.resume_preparation());
            self.native_creates
                .sagas
                .get_mut(&saga_id)
                .expect("the saga remains owned")
                .phase = phase;
        }
        let Some(preparation) = phase.preparation() else {
            return Ok(None);
        };
        match self.reduce_ready_native_preparation(provider, binding, owner, preparation)? {
            NativePreparationOutcome::Ignored => Ok(None),
            NativePreparationOutcome::Ready(proof) => self
                .complete_native_create_preparation(saga_id, proof)
                .map(Some),
        }
    }

    fn complete_native_create_preparation(
        &mut self,
        saga_id: NativeCreateSagaId,
        proof: NativeReadyProof,
    ) -> Result<ViewportLifecycleAction, ViewportCoordinatorError> {
        let saga = self
            .native_creates
            .sagas
            .get_mut(&saga_id)
            .ok_or(ViewportCoordinatorError::MissingCreateSaga { saga: saga_id })?;
        if proof.binding() != saga.binding || proof.retained_resource() != Some(saga.resource) {
            return Err(ViewportCoordinatorError::CreateSagaNotReady { saga: saga_id });
        }
        saga.phase = NativeCreateLifecycle::AwaitingFirstLivePreparation { proof };
        Ok(ViewportLifecycleAction::PrepareNativeCreateFirstLive {
            saga: saga_id,
            prepared: Box::new(saga.prepared.clone()),
            proof,
        })
    }

    pub(crate) fn native_ready_basis(
        &self,
        binding: ViewportBinding,
    ) -> Option<super::NativeReadyBasis> {
        let create_proof = self
            .native_creates
            .sagas
            .values()
            .find(|saga| saga.binding == binding)
            .and_then(|saga| match saga.phase {
                NativeCreateLifecycle::AwaitingFirstLivePreparation { proof }
                | NativeCreateLifecycle::AwaitingFirstLivePresentation { proof } => Some(proof),
                NativeCreateLifecycle::Preparing(_) => None,
            });
        let proof = create_proof.or_else(|| {
            self.recovery_replacements
                .pending(binding.surface())
                .and_then(|pending| pending.first_live_proof())
        })?;
        (proof.binding() == binding && self.native_ready_proof_is_current(proof))
            .then_some(proof.basis())
    }

    pub(crate) fn prepare_native_create_first_live(
        &mut self,
        saga_id: NativeCreateSagaId,
        proof: NativeReadyProof,
    ) -> Result<ViewportBinding, ViewportCoordinatorError> {
        let saga = self
            .native_creates
            .sagas
            .get(&saga_id)
            .ok_or(ViewportCoordinatorError::MissingCreateSaga { saga: saga_id })?;
        let NativeCreateLifecycle::AwaitingFirstLivePreparation { proof: expected } = saga.phase
        else {
            return Err(ViewportCoordinatorError::CreateSagaNotReady { saga: saga_id });
        };
        if proof != expected
            || proof.binding() != saga.binding
            || proof.retained_resource() != Some(saga.resource)
            || !self.native_ready_proof_is_current(proof)
        {
            return Err(ViewportCoordinatorError::CreateSagaNotReady { saga: saga_id });
        }
        let binding = saga.binding;
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
            .phase = NativeCreateLifecycle::AwaitingFirstLivePresentation { proof };
        *self = candidate;
        Ok(binding)
    }

    /// Admits the native binding as the engine atomically publishes ownership for an
    /// exact presentation observation of its first live docking output.
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
                NativeCreateLifecycle::AwaitingFirstLivePresentation { .. }
            )
        {
            return Err(ViewportCoordinatorError::CreateSagaNotReady { saga: saga_id });
        }

        let mut candidate = self.clone();
        let bindings = candidate
            .registry
            .bindings_for_window(binding.window())
            .collect::<Vec<_>>();
        let admission = candidate
            .registry
            .placement_record(binding)
            .map(|record| record.admission())
            .ok_or(ViewportCoordinatorError::CreateSagaNotReady { saga: saga_id })?;
        if admission == crate::viewport_registry::ViewportAdmission::Pending {
            candidate
                .registry
                .admit_window(&bindings)
                .map_err(ViewportCoordinatorError::Registry)?;
        } else if admission != crate::viewport_registry::ViewportAdmission::Admitted {
            return Err(ViewportCoordinatorError::CreateSagaNotReady { saga: saga_id });
        }
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
        let saga = self
            .native_creates
            .sagas
            .remove(&saga_id)
            .ok_or(ViewportCoordinatorError::MissingCreateSaga { saga: saga_id })?;
        let binding = saga.binding;
        let create = saga.create;
        let resource = saga.resource;
        let resource_owner = if matches!(
            saga.phase,
            NativeCreateLifecycle::AwaitingFirstLivePresentation { .. }
        ) {
            NativeStagingResourceOwner::NativeCreateFirstLive(binding)
        } else {
            NativeStagingResourceOwner::NativeCreateSaga(saga_id)
        };
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
            self.release_native_staging_resource(resource, resource_owner)?;
            self.retain_uncreated_binding_guard(binding, create);
            return Ok(None);
        }
        self.transition_native_staging_resource(
            resource,
            resource_owner,
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

    pub(super) fn retain_uncreated_binding_guard(
        &mut self,
        binding: ViewportBinding,
        create: EffectId,
    ) {
        if let Some(provider) = self
            .effects
            .record(create)
            .and_then(|record| record.provider().or(record.binding_publication_provider()))
        {
            // A proven never-created incarnation still needs its producer-bound
            // terminal guard until the joined ingress prefix is reclaimed.
            self.binding_cleanup
                .record_terminal_binding_guard(binding, provider);
        }
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
        self.native_creates.sagas.keys().copied().collect()
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
        self.native_creates
            .sagas
            .values()
            .any(|saga| saga.binding == binding)
    }

    pub(super) fn abortable_native_create_for_binding(
        &self,
        binding: ViewportBinding,
    ) -> Option<NativeCreateSagaId> {
        self.native_creates
            .sagas
            .iter()
            .find_map(|(id, saga)| (saga.binding == binding).then_some(*id))
    }

    pub(super) fn native_create_for_effect(&self, effect: EffectId) -> Option<NativeCreateSagaId> {
        self.native_creates
            .sagas
            .iter()
            .find_map(|(id, saga)| (saga.create == effect).then_some(*id))
    }

    pub(super) fn settle_destroyed_native_create(
        &mut self,
        binding: ViewportBinding,
    ) -> Result<bool, ViewportCoordinatorError> {
        let saga_id = self
            .native_creates
            .sagas
            .iter()
            .find_map(|(id, saga)| (saga.binding == binding).then_some(*id));
        let Some(saga_id) = saga_id else {
            return Ok(false);
        };
        let saga = self
            .native_creates
            .sagas
            .remove(&saga_id)
            .expect("the matching native create saga was just observed");
        let inventory_generation = self.registry.inventory_generation();
        let _ = self
            .effects
            .mark_destroyed(saga.create, binding, inventory_generation);
        let owner = if matches!(
            saga.phase,
            NativeCreateLifecycle::AwaitingFirstLivePresentation { .. }
        ) {
            NativeStagingResourceOwner::NativeCreateFirstLive(binding)
        } else {
            NativeStagingResourceOwner::NativeCreateSaga(saga_id)
        };
        self.release_native_staging_resource(saga.resource, owner)?;
        Ok(true)
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

    /// Validates retained-resource conservation after a complete lifecycle reduction.
    pub(crate) fn validate_native_staging_resource_conservation(
        &self,
    ) -> Result<(), ViewportCoordinatorError> {
        let mut references =
            Vec::with_capacity(self.native_creates.sagas.len() + self.binding_cleanup.len());
        references.extend(self.native_creates.sagas.iter().map(|(saga, create)| {
            let owner = if matches!(
                create.phase,
                NativeCreateLifecycle::AwaitingFirstLivePresentation { .. }
            ) {
                NativeStagingResourceOwner::NativeCreateFirstLive(create.binding)
            } else {
                NativeStagingResourceOwner::NativeCreateSaga(*saga)
            };
            (create.resource, owner)
        }));
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
}
