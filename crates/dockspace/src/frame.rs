//! Atomic platform-fact coordinator used by the docking engine.

mod binding_retirement;
mod native_bringup;
mod native_create;
mod native_staging_resource;
mod pointer_passthrough;
mod recovery_cleanup;
mod recovery_replacement;

use std::collections::{BTreeMap, BTreeSet};

use thiserror::Error;

pub use self::binding_retirement::{
    BindingRetirement, BindingRetirementOrigin, BindingRetirementStatus,
};
use self::binding_retirement::{
    BindingRetirementCleanup, BindingRetirementCleanupRequest, BindingRetirementDirective,
    BindingRetirementLifecycle, BindingRetirementLifecycleError, BindingRetirementRequest,
    BindingRetirementTerminal,
};
pub use self::native_bringup::{NativeVisibilityProof, NativeVisibleProof};
use self::native_create::NativeCreateState;
pub use self::native_create::{NativeCreatePhase, NativeCreateRequest, NativeCreateSaga};
use self::native_staging_resource::{
    NativeStagingResourceConservationError, NativeStagingResourceLedger,
    NativeStagingResourceLedgerError, NativeStagingResourceOwner,
};
use self::pointer_passthrough::{
    PointerPassthroughEffectRequest, PointerPassthroughEvidence, PointerPassthroughLifecycle,
    PointerPassthroughLifecycleError, PointerPassthroughRecovery,
};
use self::recovery_cleanup::AbandonedReplacementRetirementMode;
pub use self::recovery_replacement::{RecoveryPending, RecoveryPendingStatus};
use self::recovery_replacement::{
    RecoveryPendingRequest, RecoveryReplacementLifecycle, RecoveryReplacementLifecycleError,
    StagingCloseAbort, StagingCloseAbortOwner, StagingCloseAbortRequest,
};

use crate::backend_ingress::BackendIngressDrainReceipt;
use crate::close_plan::NativeCloseEdge;
use crate::coordinates::{
    CoordinateUnavailable, RecoveryCoordinateSnapshot, ViewportPlacementProof,
};
#[cfg(test)]
use crate::effect::EffectRequest;
use crate::effect::{
    EffectDispatchResult, EffectId, EffectInvalidation, EffectLedger, EffectLedgerError,
    EffectPhase, EffectRecord, EffectResult, EffectTransition, PlatformEffect,
    PlatformEffectEmission,
};
use crate::geometry::LogicalRect;
pub use crate::ids::NativeCreateSagaId;
use crate::ids::{EngineAuthorityDomainId, ItemId, SurfaceId, WorkspaceEpoch};
use crate::intent::{NativePlacementProof, PointerId};
use crate::interaction::PreparedNativeTearOff;
use crate::platform::{
    CapabilityRosterObservationStream, ObservedWorkArea, PlatformCapabilities, PlatformCapability,
    PlatformSnapshot, WindowCloseObservation, WindowCloseState, WindowInputObservation,
    WindowInventoryObservationStream, WindowPresentationObservation, WindowPresentationState,
    WorkAreaRosterObservationStream,
};
use crate::platform_provider::{
    PlatformObservationAuthority, PlatformObservationAuthorityError, PlatformObservationLease,
    PlatformProviderAuthorityFrontier, PlatformProviderReplacementTicket,
};
use crate::presentation_observation::{NativeStagingOwner, NativeStagingResourceId};
use crate::retention::{BindingRetentionManifest, EffectRetentionManifest};
use crate::surface_recovery::SurfaceRecoveryObligationId;
#[cfg(test)]
use crate::viewport::InventoryGeneration;
use crate::viewport::{
    CapabilityGeneration, InputObservationGeneration, InventoryObservationGeneration,
    PlatformSnapshotGeneration, ViewportBinding, ViewportRole, WindowToken, WorkAreaGeneration,
    WorkAreaObservationGeneration, WorkAreaToken,
};
use crate::viewport_registry::{
    DetachedViewportFacts, NativeCloseEdgeDisposition, RegistryEvent, ViewportAdmission,
    ViewportLifecycle, ViewportOwnership, ViewportRecord, ViewportRegistry, ViewportRegistryError,
};

/// Exact panel-focus disposition owned by the core focus coordinator.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PanelFocus {
    /// Activate and focus this exact source-roster item after merge-back.
    Item(ItemId),
    /// Explicitly leave the merged content without a focused panel.
    None,
}

/// Summary of the exact native bindings reconciled by a successful workspace restore.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ViewportReconciliation {
    rebound: Vec<(ViewportBinding, ViewportBinding)>,
    retired: Vec<ViewportBinding>,
    cleanup_effects: Vec<EffectId>,
    unbound_surfaces: Vec<SurfaceId>,
}

impl ViewportReconciliation {
    #[must_use]
    pub fn rebound(&self) -> &[(ViewportBinding, ViewportBinding)] {
        &self.rebound
    }

    #[must_use]
    pub fn retired(&self) -> &[ViewportBinding] {
        &self.retired
    }

    #[must_use]
    pub fn cleanup_effects(&self) -> &[EffectId] {
        &self.cleanup_effects
    }

    #[must_use]
    pub fn unbound_surfaces(&self) -> &[SurfaceId] {
        &self.unbound_surfaces
    }
}

/// Whether a destroyed binding must be forwarded to graph-level surface recovery.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DestroyedBindingDisposition {
    ForwardToEngine,
    Handled {
        /// A recovery replacement disappeared before admission. Suppress only this snapshot's
        /// retry so the exact staging destruction cannot mutate the source graph.
        suppress_recovery_retry: Option<SurfaceId>,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum ViewportLifecycleAction {
    TransferNativeCreate {
        saga: NativeCreateSagaId,
        prepared: Box<PreparedNativeTearOff>,
        proof: NativeVisibleProof,
    },
    SurfaceDestroyed {
        observation: WindowCloseObservation,
        source_coordinates: Option<RecoveryCoordinateSnapshot>,
    },
    /// A retained recovery's replacement completed post-show staging and is
    /// ready for an exact first-live presentation barrier.
    ///
    /// The coordinator has already transferred any retained staging resource
    /// to this replacement's first-live owner before emitting the action.
    RecoveryReplacementReady {
        destroyed_binding: ViewportBinding,
        replacement_binding: ViewportBinding,
        recovery_obligation: SurfaceRecoveryObligationId,
    },
    /// A replacement disappeared while its first-live presentation barrier
    /// was still pending. The coordinator has already returned any retained
    /// staging resource to the destroyed surface recovery owner.
    RecoveryReplacementLost {
        destroyed_binding: ViewportBinding,
        replacement_binding: ViewportBinding,
        recovery_obligation: SurfaceRecoveryObligationId,
    },
    RetryRecovery {
        destroyed_binding: ViewportBinding,
        recovery_obligation: SurfaceRecoveryObligationId,
    },
}

/// Complete publication result of one platform fact snapshot.
#[derive(Debug, Clone, PartialEq)]
pub struct ViewportFrameTransition {
    capability_generation: CapabilityGeneration,
    work_area_generation: WorkAreaGeneration,
    capabilities_changed: bool,
    work_areas_changed: bool,
    registry_events: Vec<RegistryEvent>,
    consumed_staging_close_requests: Vec<WindowCloseObservation>,
    actions: Vec<ViewportLifecycleAction>,
}

impl ViewportFrameTransition {
    #[must_use]
    pub const fn capability_generation(&self) -> CapabilityGeneration {
        self.capability_generation
    }

    #[must_use]
    pub const fn work_area_generation(&self) -> WorkAreaGeneration {
        self.work_area_generation
    }

    #[must_use]
    pub const fn capabilities_changed(&self) -> bool {
        self.capabilities_changed
    }

    #[must_use]
    pub const fn work_areas_changed(&self) -> bool {
        self.work_areas_changed
    }

    #[must_use]
    pub fn registry_events(&self) -> &[RegistryEvent] {
        &self.registry_events
    }

    /// Returns whether the coordinator consumed this exact pre-admission close request.
    ///
    /// The registry event remains in the transition for lifecycle accounting, but it must never
    /// be reclassified as an application-owned native surface close by the engine.
    pub(crate) fn staging_close_was_consumed(&self, requested: WindowCloseObservation) -> bool {
        self.consumed_staging_close_requests.contains(&requested)
    }

    pub(crate) fn actions(&self) -> &[ViewportLifecycleAction] {
        &self.actions
    }
}

/// Deep core module owning capabilities, bindings, coordinate facts, and effects.
#[derive(Debug, Clone, PartialEq)]
pub struct ViewportCoordinator {
    authority_domain: EngineAuthorityDomainId,
    workspace_epoch: WorkspaceEpoch,
    platform_provider: PlatformObservationAuthority,
    provider_reconciliation_pending: bool,
    capability_observations: CapabilityRosterObservationStream,
    capabilities: PlatformCapabilities,
    capability_generation: CapabilityGeneration,
    inventory_observations: WindowInventoryObservationStream,
    last_platform_snapshot: Option<PlatformSnapshot>,
    work_area_observations: WorkAreaRosterObservationStream,
    work_areas: BTreeMap<WorkAreaToken, ObservedWorkArea>,
    work_area_generation: WorkAreaGeneration,
    registry: ViewportRegistry,
    effects: EffectLedger,
    focus_effect_lane_tail: Option<EffectId>,
    pointer_passthrough: PointerPassthroughLifecycle,
    native_creates: NativeCreateState,
    native_staging_resources: NativeStagingResourceLedger,
    binding_retirement: BindingRetirementLifecycle,
    recovery_replacements: RecoveryReplacementLifecycle,
}

#[cfg(test)]
impl Default for ViewportCoordinator {
    fn default() -> Self {
        let mut coordinator = Self::new(EngineAuthorityDomainId::new_for_test(1));
        coordinator
            .create_platform_provider()
            .expect("the test coordinator provider must allocate");
        coordinator
    }
}

/// Exact viewport facts frozen for one tick-start logical surface.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SurfaceVacancyAuthority {
    surface: SurfaceId,
    binding: Option<FrozenSurfaceBinding>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct FrozenSurfaceBinding {
    binding: ViewportBinding,
    role: ViewportRole,
    ownership: ViewportOwnership,
}

impl SurfaceVacancyAuthority {
    fn capture(surface: SurfaceId, record: Option<&ViewportRecord>) -> Self {
        Self {
            surface,
            binding: record.map(|record| FrozenSurfaceBinding {
                binding: record.binding(),
                role: record.role(),
                ownership: record.ownership(),
            }),
        }
    }

    pub(crate) const fn surface(self) -> SurfaceId {
        self.surface
    }

    pub(crate) const fn has_binding(self) -> bool {
        self.binding.is_some()
    }

    pub(crate) const fn binding(self) -> Option<ViewportBinding> {
        match self.binding {
            Some(binding) => Some(binding.binding),
            None => None,
        }
    }
}

/// Exact bindings and effects settled by one tick-final vacancy reconciliation.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct SurfaceVacancySettlement {
    logical_vacated_bindings: Vec<ViewportBinding>,
    effects: Vec<EffectId>,
    retired_native_creates: Vec<(NativeCreateSagaId, ViewportBinding)>,
}

impl SurfaceVacancySettlement {
    pub(crate) fn logical_vacated_bindings(&self) -> &[ViewportBinding] {
        &self.logical_vacated_bindings
    }

    pub(crate) fn effects(&self) -> &[EffectId] {
        &self.effects
    }

    pub(crate) fn retired_native_creates(&self) -> &[(NativeCreateSagaId, ViewportBinding)] {
        &self.retired_native_creates
    }

    pub(crate) fn extend(&mut self, other: Self) {
        self.logical_vacated_bindings
            .extend(other.logical_vacated_bindings);
        self.effects.extend(other.effects);
        self.retired_native_creates
            .extend(other.retired_native_creates);
    }
}

#[derive(Debug, Clone, Copy)]
struct RestoreStagingBinding {
    creation_effect: Option<EffectId>,
    cleanup: Option<BindingRetirementCleanup>,
    origin: BindingRetirementOrigin,
    retained_resource: Option<NativeStagingResourceId>,
    resource_owner: Option<NativeStagingResourceOwner>,
}

struct RestoreAnalysis {
    staging_by_binding: BTreeMap<ViewportBinding, RestoreStagingBinding>,
    cleanup_by_binding: BTreeMap<ViewportBinding, EffectId>,
    retained_bindings: BTreeSet<ViewportBinding>,
    passthrough_recoveries: BTreeMap<ViewportBinding, PointerPassthroughRecovery>,
}

#[derive(Default)]
struct RestoreAccumulation {
    retired: Vec<ViewportBinding>,
    cleanup_effects: Vec<EffectId>,
    unbound_surfaces: Vec<SurfaceId>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BindingRetirementCleanupRestoreAction {
    Redispatch {
        binding: ViewportBinding,
    },
    ContinueObservation {
        binding: ViewportBinding,
        predecessor: EffectId,
        after: Option<EffectId>,
    },
}

impl ViewportCoordinator {
    fn validated_compensating_close(
        &self,
        effect: EffectId,
        binding: ViewportBinding,
        predecessor: EffectId,
    ) -> Result<&EffectRecord, ViewportCoordinatorError> {
        let record = self
            .effects
            .record(effect)
            .ok_or(ViewportCoordinatorError::MissingCleanupEffect { effect })?;
        if !matches!(
            record.request().effect(),
            PlatformEffect::CompensatingClose {
                binding: actual,
                compensates,
            } if *actual == binding && *compensates == predecessor
        ) {
            return Err(ViewportCoordinatorError::InvalidCleanupContinuation {
                effect,
                predecessor,
            });
        }
        Ok(record)
    }

    pub(crate) fn new(authority_domain: EngineAuthorityDomainId) -> Self {
        Self {
            authority_domain,
            workspace_epoch: WorkspaceEpoch::new(0),
            platform_provider: PlatformObservationAuthority::new(authority_domain),
            provider_reconciliation_pending: false,
            capability_observations: CapabilityRosterObservationStream::default(),
            capabilities: PlatformCapabilities::default(),
            capability_generation: CapabilityGeneration::new(0),
            inventory_observations: WindowInventoryObservationStream::default(),
            last_platform_snapshot: None,
            work_area_observations: WorkAreaRosterObservationStream::default(),
            work_areas: BTreeMap::new(),
            work_area_generation: WorkAreaGeneration::new(0),
            registry: ViewportRegistry::new(authority_domain),
            effects: EffectLedger::default(),
            focus_effect_lane_tail: None,
            pointer_passthrough: PointerPassthroughLifecycle::default(),
            native_creates: NativeCreateState::default(),
            native_staging_resources: NativeStagingResourceLedger::default(),
            binding_retirement: BindingRetirementLifecycle::default(),
            recovery_replacements: RecoveryReplacementLifecycle::default(),
        }
    }

    /// Enrolls the sole platform observation provider for this coordinator.
    pub(crate) fn create_platform_provider(
        &mut self,
    ) -> Result<PlatformObservationLease, ViewportCoordinatorError> {
        let mut candidate = self.clone();
        let provider = candidate
            .platform_provider
            .create()
            .map_err(ViewportCoordinatorError::PlatformProvider)?;
        candidate.reconcile_after_provider_enrollment()?;
        *self = candidate;
        Ok(provider)
    }

    /// Returns the current platform observation provider, when enrolled.
    pub(crate) const fn platform_provider(&self) -> Option<PlatformObservationLease> {
        self.platform_provider.active()
    }

    /// Returns the monotonic platform-provider authority frontier.
    pub(crate) const fn platform_provider_frontier(&self) -> PlatformProviderAuthorityFrontier {
        self.platform_provider.frontier()
    }

    fn active_platform_provider(
        &self,
    ) -> Result<PlatformObservationLease, ViewportCoordinatorError> {
        self.platform_provider
            .active()
            .ok_or(ViewportCoordinatorError::PlatformProviderUnavailable)
    }

    fn revoke_recovery_bringups_for_provider_replacement(
        &mut self,
        provider: PlatformObservationLease,
    ) -> Result<(), ViewportCoordinatorError> {
        for (_, binding, phase) in self.recovery_replacements.active_bringups() {
            let replacement = phase.create_effect();
            let replacement_may_have_executed = self.effect_may_have_executed(replacement);
            let invalidation = EffectInvalidation::PlatformProviderReplaced { provider };
            let _ = self.effects.invalidate_unemitted(replacement, invalidation);
            if let Some(show) = phase.show_effect() {
                let _ = self.effects.invalidate_unemitted(show, invalidation);
            }
            let binding_was_discarded =
                !replacement_may_have_executed && self.registry.discard_unobserved(binding).is_ok();
            let surface = self
                .recovery_replacements
                .mark_provider_lost(binding, phase, binding_was_discarded)
                .map_err(recovery_replacement_error)?;
            if !binding_was_discarded {
                let _ = self.retire_abandoned_recovery_replacement_in_place(
                    surface,
                    binding,
                    AbandonedReplacementRetirementMode::AwaitSuccessorObservation,
                )?;
            }
        }
        let compensations = self
            .recovery_replacements
            .values()
            .filter_map(|pending| {
                let RecoveryPendingStatus::CompensatingReplacement {
                    replacement,
                    cleanup,
                } = pending.status()
                else {
                    return None;
                };
                Some((
                    pending.destroyed_binding().surface(),
                    pending.replacement_binding()?,
                    replacement,
                    cleanup,
                ))
            })
            .collect::<Vec<_>>();
        for (surface, binding, replacement, cleanup) in compensations {
            self.retire_recovery_compensation_for_provider_replacement(
                surface,
                binding,
                replacement,
                cleanup,
                provider,
            )?;
        }
        Ok(())
    }

    /// Requires the exact currently enrolled platform provider.
    pub(crate) fn require_platform_provider(
        &self,
        provider: PlatformObservationLease,
    ) -> Result<(), PlatformObservationAuthorityError> {
        self.platform_provider.require_active(provider)
    }

    /// Returns the core-owned platform handoff which is awaiting dispatch
    /// quiescence, when one exists.
    pub(crate) const fn pending_platform_provider_replacement(
        &self,
    ) -> Option<PlatformProviderReplacementTicket> {
        self.platform_provider.pending_replacement()
    }

    /// Revokes one provider and freezes a typed handoff ticket for its successor.
    pub(crate) fn begin_platform_provider_replacement(
        &mut self,
        provider: PlatformObservationLease,
    ) -> Result<PlatformProviderReplacementTicket, ViewportCoordinatorError> {
        let mut candidate = self.clone();
        candidate
            .platform_provider
            .require_active(provider)
            .map_err(ViewportCoordinatorError::PlatformProvider)?;

        candidate.end_all_drag_routing_in_place()?;
        let create_sagas = candidate.abortable_native_create_sagas();
        for saga in create_sagas {
            let _ = candidate.abort_native_create(saga)?;
        }
        candidate.revoke_recovery_bringups_for_provider_replacement(provider)?;

        let _ = candidate
            .effects
            .invalidate_unemitted_causal_successors_for_provider_replacement(provider);
        let provider_fact_requests = candidate
            .effects
            .records()
            .filter_map(|(effect, record)| {
                (!record.was_emitted()
                    && matches!(record.phase(), EffectPhase::Requested)
                    && matches!(
                        record.request().effect(),
                        PlatformEffect::ShowWindow { .. }
                            | PlatformEffect::SetPointerPassthrough { .. }
                            | PlatformEffect::RequestFocus { .. }
                            | PlatformEffect::ResolveNativeClose { .. }
                    ))
                .then_some(effect)
            })
            .collect::<Vec<_>>();
        for effect in provider_fact_requests {
            let _ = candidate.effects.invalidate_unemitted(
                effect,
                EffectInvalidation::PlatformProviderReplaced { provider },
            );
        }

        candidate
            .pointer_passthrough
            .recover_for_provider_replacement();
        candidate.focus_effect_lane_tail = None;
        candidate.effects.revoke_provider_authority(provider);

        candidate
            .capability_observations
            .reset_for_provider_replacement();
        if candidate.capabilities != PlatformCapabilities::default() {
            candidate.capability_generation = candidate
                .capability_generation
                .checked_next()
                .ok_or(ViewportCoordinatorError::CapabilityGenerationExhausted)?;
            candidate.capabilities = PlatformCapabilities::default();
        }
        candidate
            .inventory_observations
            .reset_for_provider_replacement();
        candidate.last_platform_snapshot = None;
        candidate
            .work_area_observations
            .reset_for_provider_replacement();
        if !candidate.work_areas.is_empty() {
            candidate.work_area_generation = candidate
                .work_area_generation
                .checked_next()
                .ok_or(ViewportCoordinatorError::WorkAreaGenerationExhausted)?;
            candidate.work_areas.clear();
        }
        candidate
            .registry
            .revoke_live_provider_authority()
            .map_err(ViewportCoordinatorError::Registry)?;
        candidate
            .binding_retirement
            .reset_for_provider_replacement();
        let ticket = candidate
            .platform_provider
            .begin_replacement(provider)
            .map_err(ViewportCoordinatorError::PlatformProvider)?;
        candidate.provider_reconciliation_pending = true;
        *self = candidate;
        Ok(ticket)
    }

    /// Activates the exact successor reserved by a completed provider handoff.
    pub(crate) fn finish_platform_provider_replacement(
        &mut self,
        ticket: PlatformProviderReplacementTicket,
    ) -> Result<PlatformObservationLease, ViewportCoordinatorError> {
        let mut candidate = self.clone();
        let provider = candidate
            .platform_provider
            .finish_replacement(ticket)
            .map_err(ViewportCoordinatorError::PlatformProvider)?;
        candidate.reconcile_after_provider_enrollment()?;
        *self = candidate;
        Ok(provider)
    }

    /// Abandons the reserved successor while preserving fail-closed platform state.
    pub(crate) fn abort_platform_provider_replacement(
        &mut self,
        ticket: PlatformProviderReplacementTicket,
    ) -> Result<(), ViewportCoordinatorError> {
        let mut candidate = self.clone();
        candidate
            .platform_provider
            .abort_replacement(ticket)
            .map_err(ViewportCoordinatorError::PlatformProvider)?;
        *self = candidate;
        Ok(())
    }

    /// Compacts destroyed-binding guards after the exact observation producer has quiesced.
    pub(crate) fn compact_quiesced_destroyed_binding_guards(
        &mut self,
        provider: PlatformObservationLease,
    ) -> usize {
        self.binding_retirement
            .compact_destroyed_tombstones_from(provider)
    }

    /// Compacts one exact binding guard after its joined producer lane proves quiescence.
    pub(crate) fn compact_quiesced_destroyed_binding_guard(
        &mut self,
        binding: ViewportBinding,
        provider: PlatformObservationLease,
    ) -> Result<(), ViewportCoordinatorError> {
        self.binding_retirement
            .compact_destroyed_tombstone(binding, provider)
            .map_err(binding_retirement_error)
    }

    #[cfg(test)]
    pub(crate) fn record_destroyed_binding_guard_for_test(
        &mut self,
        binding: ViewportBinding,
        provider: PlatformObservationLease,
    ) {
        self.binding_retirement
            .record_destroyed_tombstone(binding, provider);
    }

    /// Releases the exact revoked effect-provider guard proven quiescent by backend drain.
    pub(crate) fn compact_quiesced_backend_effect_provider(
        &mut self,
        receipt: &BackendIngressDrainReceipt,
    ) -> bool {
        self.effects.compact_quiesced_backend_provider(receipt)
    }

    /// Registers an existing adapter window without storing its OS handle.
    pub(crate) fn register_existing(
        &mut self,
        epoch: WorkspaceEpoch,
        surface: SurfaceId,
        token: WindowToken,
        role: ViewportRole,
        recovery_obligation: Option<SurfaceRecoveryObligationId>,
    ) -> Result<ViewportBinding, ViewportCoordinatorError> {
        if self.registry.records().next().is_some() && epoch != self.workspace_epoch {
            return Err(ViewportCoordinatorError::WorkspaceEpochMismatch {
                expected: self.workspace_epoch,
                actual: epoch,
            });
        }
        if self.binding_retirement.token_is_reserved(token) {
            return Err(ViewportCoordinatorError::RetiredTokenReserved { token });
        }
        if role == ViewportRole::Child && recovery_obligation.is_none() {
            return Err(ViewportCoordinatorError::ChildRecoveryRequired { surface });
        }
        if self.recovery_replacements.pending(surface).is_some() {
            return Err(ViewportCoordinatorError::PendingRecoveryRegistrationMismatch { surface });
        }
        let binding = self
            .registry
            .register_existing(epoch, surface, token, role)
            .map_err(ViewportCoordinatorError::Registry)?;
        self.workspace_epoch = epoch;
        Ok(binding)
    }

    /// Atomically invalidates old-epoch platform state and reconciles safe current bindings.
    pub(crate) fn reconcile_workspace_epoch(
        &mut self,
        new_epoch: WorkspaceEpoch,
        desired_surfaces: &BTreeSet<SurfaceId>,
    ) -> Result<ViewportReconciliation, ViewportCoordinatorError> {
        let mut candidate = self.clone();
        let analysis = candidate.analyze_restore(desired_surfaces)?;
        let registry = candidate
            .registry
            .reconcile_restore(new_epoch, &analysis.retained_bindings)
            .map_err(ViewportCoordinatorError::Registry)?;
        let rebound_map: BTreeMap<ViewportBinding, ViewportBinding> =
            registry.retained().iter().copied().collect();

        candidate
            .effects
            .invalidate_unemitted_for_workspace_replacement(new_epoch);
        candidate.workspace_epoch = new_epoch;
        candidate.pointer_passthrough.clear();
        let mut accumulated = RestoreAccumulation::default();
        candidate
            .migrate_retirement_cleanup_obligations(new_epoch, &mut accumulated.cleanup_effects)?;
        candidate.request_retirement_restore_cleanup(
            new_epoch,
            &analysis,
            &mut accumulated.cleanup_effects,
        )?;
        candidate.request_rebound_restore_cleanup(
            &analysis,
            &rebound_map,
            &mut accumulated.cleanup_effects,
        )?;
        candidate.retire_restored_bindings(registry.retired(), &analysis, &mut accumulated)?;
        candidate.finish_restore(desired_surfaces, &mut accumulated.unbound_surfaces)?;

        let reconciliation = ViewportReconciliation {
            rebound: registry.retained().to_vec(),
            retired: accumulated.retired,
            cleanup_effects: accumulated.cleanup_effects,
            unbound_surfaces: accumulated.unbound_surfaces,
        };
        *self = candidate;
        Ok(reconciliation)
    }

    fn analyze_restore(
        &mut self,
        desired_surfaces: &BTreeSet<SurfaceId>,
    ) -> Result<RestoreAnalysis, ViewportCoordinatorError> {
        let create_by_binding = self.native_create_snapshot_by_binding();
        let mut staging_by_binding: BTreeMap<_, _> = create_by_binding
            .iter()
            .map(|(binding, saga)| {
                (
                    *binding,
                    RestoreStagingBinding {
                        creation_effect: Some(saga.create),
                        cleanup: Some(BindingRetirementCleanup::CompensateCreate {
                            create: saga.create,
                        }),
                        origin: BindingRetirementOrigin::NativeCreateAborted {
                            create: saga.create,
                        },
                        retained_resource: Some(saga.resource()),
                        resource_owner: Some(
                            if matches!(
                                saga.phase(),
                                NativeCreatePhase::AwaitingFirstLivePresentation { .. }
                            ) {
                                NativeStagingResourceOwner::NativeCreateFirstLive(*binding)
                            } else {
                                NativeStagingResourceOwner::NativeCreateSaga(saga.id())
                            },
                        ),
                    },
                )
            })
            .collect();
        let mut cleanup_by_binding = BTreeMap::new();
        for (effect, record) in self.effects.records() {
            if let PlatformEffect::ResolveNativeClose {
                edge,
                resolution: crate::effect::NativeCloseResolution::Accept,
                ..
            } = record.request().effect()
                && record.was_emitted()
                && !matches!(
                    record.phase(),
                    EffectPhase::DispatchFailed(_)
                        | EffectPhase::Unsupported(_)
                        | EffectPhase::Invalidated { .. }
                )
            {
                cleanup_by_binding.insert(edge.binding(), effect);
            }
        }
        let pending_recoveries = self
            .recovery_replacements
            .values()
            .cloned()
            .collect::<Vec<_>>();
        for pending in &pending_recoveries {
            if let Some(binding) = pending.replacement_binding() {
                let staging_close = self
                    .recovery_replacements
                    .take_staging_close_for_retirement(
                        pending.destroyed_binding().surface(),
                        binding,
                    )
                    .map_err(recovery_replacement_error)?;
                let active_staging_cleanup =
                    staging_close.and_then(|transfer| transfer.into_active_cleanup());
                let cleanup = pending
                    .replacement_effect()
                    .map(|create| BindingRetirementCleanup::CompensateCreate { create });
                staging_by_binding.insert(
                    binding,
                    RestoreStagingBinding {
                        creation_effect: pending.replacement_effect(),
                        cleanup,
                        origin: BindingRetirementOrigin::WorkspaceReplaced,
                        retained_resource: pending.retained_staging_resource(),
                        resource_owner: pending.retained_staging_resource().map(|_| {
                            if matches!(
                                pending.status(),
                                RecoveryPendingStatus::AwaitingFirstLivePresentation
                            ) {
                                NativeStagingResourceOwner::RecoveryReplacementFirstLive {
                                    obligation: pending.recovery_obligation(),
                                    binding,
                                }
                            } else {
                                NativeStagingResourceOwner::SurfaceRecovery {
                                    obligation: pending.recovery_obligation(),
                                    binding: pending.destroyed_binding(),
                                }
                            }
                        }),
                    },
                );
                if let RecoveryPendingStatus::CompensatingReplacement { cleanup, .. } =
                    pending.status()
                {
                    cleanup_by_binding.insert(binding, cleanup);
                } else if let Some(effect) = active_staging_cleanup {
                    let replacement = pending.replacement_effect().ok_or(
                        ViewportCoordinatorError::MissingReplacementEffect {
                            surface: pending.destroyed_binding().surface(),
                        },
                    )?;
                    let _ = self.validated_compensating_close(effect, binding, replacement)?;
                    cleanup_by_binding.insert(binding, effect);
                }
            }
        }
        if let Some(binding) = self.recovery_replacements.first_staging_close_binding() {
            return Err(
                ViewportCoordinatorError::PendingRecoveryRegistrationMismatch {
                    surface: binding.surface(),
                },
            );
        }
        let replacement_bindings: BTreeSet<_> = self
            .recovery_replacements
            .values()
            .filter_map(RecoveryPending::replacement_binding)
            .collect();
        let retained_bindings = self
            .registry
            .records()
            .filter_map(|(surface, record)| {
                Self::restore_binding_is_safe(
                    surface,
                    record,
                    desired_surfaces,
                    &create_by_binding,
                    &replacement_bindings,
                )
                .then_some(record.binding())
            })
            .collect();
        Ok(RestoreAnalysis {
            staging_by_binding,
            cleanup_by_binding,
            retained_bindings,
            passthrough_recoveries: self.pointer_passthrough.recovery_snapshots(),
        })
    }

    fn restore_binding_is_safe(
        surface: SurfaceId,
        record: &ViewportRecord,
        desired_surfaces: &BTreeSet<SurfaceId>,
        creates: &BTreeMap<ViewportBinding, NativeCreateSaga>,
        replacements: &BTreeSet<ViewportBinding>,
    ) -> bool {
        let binding = record.binding();
        // A surface identity alone cannot prove that a replacement workspace preserved the
        // child's exact root, recovery host, floating identities, ownership, and geometry
        // contract. Until restore publishes that semantic manifest, child bindings fail closed
        // and require a new explicit registration carrying a freshly validated recovery plan.
        desired_surfaces.contains(&surface)
            && record.role() != ViewportRole::Child
            && record.admission() == ViewportAdmission::Admitted
            && !creates.contains_key(&binding)
            && !replacements.contains(&binding)
            && !matches!(
                record.lifecycle(),
                ViewportLifecycle::CloseRequested
                    | ViewportLifecycle::AwaitingDestroyed
                    | ViewportLifecycle::Missing
                    | ViewportLifecycle::Destroyed
            )
    }

    fn migrate_retirement_cleanup_obligations(
        &mut self,
        new_epoch: WorkspaceEpoch,
        cleanup_effects: &mut Vec<EffectId>,
    ) -> Result<(), ViewportCoordinatorError> {
        let actions: Vec<_> = self
            .binding_retirement
            .values()
            .map(|retirement| self.retirement_cleanup_restore_action(retirement))
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .flatten()
            .collect();

        self.apply_retirement_cleanup_actions(new_epoch, actions, cleanup_effects)
    }

    fn migrate_retirement_cleanup_obligations_after_provider_replacement(
        &mut self,
    ) -> Result<(), ViewportCoordinatorError> {
        let actions = self
            .binding_retirement
            .values()
            .map(|retirement| self.retirement_cleanup_provider_replacement_action(retirement))
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .flatten()
            .collect();
        self.apply_retirement_cleanup_actions(self.workspace_epoch, actions, &mut Vec::new())
    }

    fn reconcile_after_provider_enrollment(&mut self) -> Result<(), ViewportCoordinatorError> {
        if !self.provider_reconciliation_pending {
            return Ok(());
        }
        self.migrate_retirement_cleanup_obligations_after_provider_replacement()?;
        let retirements = self.binding_retirement.keys().copied().collect::<Vec<_>>();
        for binding in retirements {
            let _ = self.drive_binding_retirement(binding)?;
        }
        self.provider_reconciliation_pending = false;
        Ok(())
    }

    fn apply_retirement_cleanup_actions(
        &mut self,
        issuance_epoch: WorkspaceEpoch,
        actions: Vec<BindingRetirementCleanupRestoreAction>,
        cleanup_effects: &mut Vec<EffectId>,
    ) -> Result<(), ViewportCoordinatorError> {
        for action in actions {
            match action {
                BindingRetirementCleanupRestoreAction::Redispatch { binding } => {
                    let request = self
                        .binding_retirement
                        .plan_cleanup_redispatch(binding)
                        .map_err(binding_retirement_error)?;
                    if let Some(request) = request {
                        let effect = self.request_retirement_cleanup(request)?;
                        self.binding_retirement
                            .accept_cleanup_effect(binding, effect)
                            .map_err(binding_retirement_error)?;
                        cleanup_effects.push(effect);
                    }
                }
                BindingRetirementCleanupRestoreAction::ContinueObservation {
                    binding,
                    predecessor,
                    after,
                } => {
                    let superseded = self
                        .binding_retirement
                        .get(&binding)
                        .and_then(|retirement| retirement.status().cleanup_effect())
                        .filter(|effect| *effect != predecessor);
                    let successor = self
                        .effects
                        .request_in(
                            issuance_epoch,
                            PlatformEffect::ContinueCleanup {
                                binding,
                                predecessor,
                                after,
                            },
                        )
                        .map_err(ViewportCoordinatorError::Effect)?;
                    self.binding_retirement
                        .accept_cleanup_observation_effect(binding, successor, predecessor)
                        .map_err(binding_retirement_error)?;
                    if let Some(effect) = superseded
                        && !matches!(
                            self.effects.supersede_cleanup_observation(
                                effect,
                                successor,
                                predecessor,
                                binding,
                            ),
                            EffectTransition::Applied | EffectTransition::Duplicate
                        )
                    {
                        return Err(ViewportCoordinatorError::InvalidCleanupContinuation {
                            effect: successor,
                            predecessor,
                        });
                    }
                    cleanup_effects.push(successor);
                }
            }
        }
        Ok(())
    }

    fn retirement_cleanup_provider_replacement_action(
        &self,
        retirement: &BindingRetirement,
    ) -> Result<Option<BindingRetirementCleanupRestoreAction>, ViewportCoordinatorError> {
        let Some(effect) = retirement.status().cleanup_effect() else {
            return Ok(None);
        };
        let record = self
            .effects
            .record(effect)
            .ok_or(ViewportCoordinatorError::MissingCleanupEffect { effect })?;
        let observation_failed = matches!(
            record.phase(),
            EffectPhase::ObservationDispatchFailed(_) | EffectPhase::ObservationUnsupported(_)
        );
        let pending_delivery = matches!(
            retirement.status(),
            BindingRetirementStatus::CleanupRequested { .. }
                | BindingRetirementStatus::CleanupIndeterminate { .. }
        );
        if !pending_delivery
            && !(observation_failed
                && matches!(
                    record.request().effect(),
                    PlatformEffect::ContinueCleanup { .. }
                ))
        {
            return Ok(None);
        }
        match record.request().effect() {
            PlatformEffect::ContinueCleanup {
                binding,
                predecessor,
                ..
            } => {
                let predecessor_record = self.effects.record(*predecessor).ok_or(
                    ViewportCoordinatorError::InvalidCleanupContinuation {
                        effect,
                        predecessor: *predecessor,
                    },
                )?;
                if *binding != retirement.binding()
                    || predecessor_record.request().effect().binding() != retirement.binding()
                    || !predecessor_record.was_emitted()
                    || !is_destructive_cleanup(predecessor_record.request().effect())
                {
                    return Err(ViewportCoordinatorError::InvalidCleanupContinuation {
                        effect,
                        predecessor: *predecessor,
                    });
                }
                Ok((record.was_emitted()
                    || matches!(record.phase(), EffectPhase::Invalidated { .. }))
                .then_some(BindingRetirementCleanupRestoreAction::ContinueObservation {
                    binding: retirement.binding(),
                    predecessor: *predecessor,
                    after: None,
                }))
            }
            platform_effect if is_destructive_cleanup(platform_effect) => {
                if platform_effect.binding() != retirement.binding() {
                    return Err(ViewportCoordinatorError::InvalidCleanupContinuation {
                        effect,
                        predecessor: effect,
                    });
                }
                Ok(record.was_emitted().then_some(
                    BindingRetirementCleanupRestoreAction::ContinueObservation {
                        binding: retirement.binding(),
                        predecessor: effect,
                        after: None,
                    },
                ))
            }
            _ => Err(ViewportCoordinatorError::InvalidCleanupContinuation {
                effect,
                predecessor: effect,
            }),
        }
    }

    fn retirement_cleanup_restore_action(
        &self,
        retirement: &BindingRetirement,
    ) -> Result<Option<BindingRetirementCleanupRestoreAction>, ViewportCoordinatorError> {
        let Some(effect) = retirement.status().cleanup_effect() else {
            return Ok(None);
        };
        let record = self
            .effects
            .record(effect)
            .ok_or(ViewportCoordinatorError::MissingCleanupEffect { effect })?;
        match record.request().effect() {
            PlatformEffect::ContinueCleanup {
                binding,
                predecessor,
                ..
            } => {
                let predecessor_record = self.effects.record(*predecessor).ok_or(
                    ViewportCoordinatorError::InvalidCleanupContinuation {
                        effect,
                        predecessor: *predecessor,
                    },
                )?;
                if *binding != retirement.binding()
                    || predecessor_record.request().effect().binding() != retirement.binding()
                    || !predecessor_record.was_emitted()
                    || !is_destructive_cleanup(predecessor_record.request().effect())
                {
                    return Err(ViewportCoordinatorError::InvalidCleanupContinuation {
                        effect,
                        predecessor: *predecessor,
                    });
                }
                if !matches!(
                    predecessor_record.phase(),
                    EffectPhase::Requested | EffectPhase::Indeterminate(_)
                ) {
                    return Ok(None);
                }
                Ok(Some(
                    BindingRetirementCleanupRestoreAction::ContinueObservation {
                        binding: retirement.binding(),
                        predecessor: *predecessor,
                        after: self.cleanup_observation_lane_predecessor(effect)?,
                    },
                ))
            }
            platform_effect if is_destructive_cleanup(platform_effect) => {
                if platform_effect.binding() != retirement.binding() {
                    return Err(ViewportCoordinatorError::InvalidCleanupContinuation {
                        effect,
                        predecessor: effect,
                    });
                }
                if record.was_emitted()
                    && matches!(
                        record.phase(),
                        EffectPhase::Requested | EffectPhase::Indeterminate(_)
                    )
                {
                    return Ok(Some(
                        BindingRetirementCleanupRestoreAction::ContinueObservation {
                            binding: retirement.binding(),
                            predecessor: effect,
                            after: None,
                        },
                    ));
                }
                Ok((!record.was_emitted()
                    && matches!(record.phase(), EffectPhase::Invalidated { .. }))
                .then_some(BindingRetirementCleanupRestoreAction::Redispatch {
                    binding: retirement.binding(),
                }))
            }
            _ => Err(ViewportCoordinatorError::InvalidCleanupContinuation {
                effect,
                predecessor: effect,
            }),
        }
    }

    fn cleanup_observation_lane_predecessor(
        &self,
        tail: EffectId,
    ) -> Result<Option<EffectId>, ViewportCoordinatorError> {
        let mut current = Some(tail);
        while let Some(effect) = current {
            let record = self
                .effects
                .record(effect)
                .ok_or(ViewportCoordinatorError::MissingCleanupEffect { effect })?;
            let invalidated_unemitted =
                !record.was_emitted() && matches!(record.phase(), EffectPhase::Invalidated { .. });
            if !invalidated_unemitted {
                return Ok(Some(effect));
            }
            current = match record.request().effect() {
                PlatformEffect::ContinueCleanup { after, .. }
                    if after.is_none_or(|predecessor| predecessor < effect) =>
                {
                    *after
                }
                _ => {
                    return Err(ViewportCoordinatorError::InvalidCleanupContinuation {
                        effect,
                        predecessor: effect,
                    });
                }
            };
        }
        Ok(None)
    }

    fn request_retirement_restore_cleanup(
        &mut self,
        new_epoch: WorkspaceEpoch,
        analysis: &RestoreAnalysis,
        cleanup_effects: &mut Vec<EffectId>,
    ) -> Result<(), ViewportCoordinatorError> {
        for (binding, old_saga) in &analysis.passthrough_recoveries {
            if !self.binding_retirement.contains_key(binding) {
                continue;
            }
            self.pointer_passthrough
                .install_epoch_recovery(*binding, old_saga);
            let effect = self.request_pointer_input_restore_in(new_epoch, *binding)?;
            cleanup_effects.push(effect);
        }
        Ok(())
    }

    fn request_rebound_restore_cleanup(
        &mut self,
        analysis: &RestoreAnalysis,
        rebound: &BTreeMap<ViewportBinding, ViewportBinding>,
        cleanup_effects: &mut Vec<EffectId>,
    ) -> Result<(), ViewportCoordinatorError> {
        for (old_binding, old_saga) in &analysis.passthrough_recoveries {
            let Some(binding) = rebound.get(old_binding).copied() else {
                continue;
            };
            self.pointer_passthrough
                .install_epoch_recovery(binding, old_saga);
            if let Some(effect) = self.reconcile_pointer_passthrough_saga(binding)? {
                cleanup_effects.push(effect);
            }
        }
        Ok(())
    }

    fn retire_restored_bindings(
        &mut self,
        retired: &[DetachedViewportFacts],
        analysis: &RestoreAnalysis,
        accumulated: &mut RestoreAccumulation,
    ) -> Result<(), ViewportCoordinatorError> {
        for facts in retired {
            let binding = facts.binding();
            accumulated.retired.push(binding);
            let staging = analysis.staging_by_binding.get(&binding).copied();
            let cleanup =
                staging
                    .and_then(|staging| staging.cleanup)
                    .unwrap_or_else(|| match facts.ownership() {
                        ViewportOwnership::External => BindingRetirementCleanup::KeepExternal,
                        ViewportOwnership::RuntimeOwned => {
                            BindingRetirementCleanup::ReleaseOwnedWindow
                        }
                    });
            let origin = staging.map_or(BindingRetirementOrigin::WorkspaceReplaced, |staging| {
                staging.origin
            });
            let existing_effect = match analysis.cleanup_by_binding.get(&binding).copied() {
                Some(effect) => {
                    let record = self
                        .effects
                        .record(effect)
                        .ok_or(ViewportCoordinatorError::MissingCleanupEffect { effect })?;
                    (record.was_emitted()
                        && !matches!(record.phase(), EffectPhase::Invalidated { .. }))
                    .then_some(effect)
                }
                None => None,
            };
            let creation_may_reappear = staging
                .and_then(|staging| staging.creation_effect)
                .is_some_and(|effect| {
                    self.effects.record(effect).is_some_and(|record| {
                        record.was_emitted()
                            && matches!(
                                record.phase(),
                                EffectPhase::Requested
                                    | EffectPhase::Indeterminate(_)
                                    | EffectPhase::ObservedApplied { .. }
                            )
                    })
                });
            let observed =
                facts.ever_observed() && !matches!(facts.lifecycle(), ViewportLifecycle::Missing);
            let may_reappear = creation_may_reappear
                || (staging
                    .and_then(|staging| staging.creation_effect)
                    .is_none()
                    && facts.ever_observed()
                    && !matches!(facts.lifecycle(), ViewportLifecycle::Destroyed));
            // An external binding has no cleanup effect that can prove destruction.
            // Its token remains quarantined until this exact incarnation is observed
            // as destroyed, including when no inventory snapshot ever saw it live.
            let external_requires_exact_destruction = facts.ownership()
                == ViewportOwnership::External
                && !matches!(facts.lifecycle(), ViewportLifecycle::Destroyed);
            if !external_requires_exact_destruction && !observed && !may_reappear {
                if let Some((resource, owner)) = staging
                    .and_then(|staging| staging.retained_resource.zip(staging.resource_owner))
                {
                    self.release_native_staging_resource(resource, owner)?;
                }
                continue;
            }
            let status = existing_effect
                .map_or(BindingRetirementStatus::AwaitingAppearance, |effect| {
                    self.retirement_status_for_effect(effect)
                });
            let retained_staging_resource = staging.and_then(|staging| staging.retained_resource);
            if let Some((resource, owner)) =
                staging.and_then(|staging| staging.retained_resource.zip(staging.resource_owner))
            {
                self.transition_native_staging_resource(
                    resource,
                    owner,
                    NativeStagingResourceOwner::BindingRetirement(binding),
                )?;
            }
            self.begin_binding_retirement(BindingRetirementRequest {
                binding,
                role: facts.role(),
                ownership: facts.ownership(),
                origin,
                status,
                observed,
                input_observations: facts.input_observations(),
                close_observations: facts.close_observations(),
                may_reappear,
                cleanup,
                retained_staging_resource,
            })?;
            if let Some(old_saga) = analysis.passthrough_recoveries.get(&binding) {
                self.pointer_passthrough
                    .install_retired_recovery(binding, old_saga);
                if let Some(effect) = self.reconcile_pointer_passthrough_saga(binding)? {
                    accumulated.cleanup_effects.push(effect);
                }
            }
            if existing_effect.is_some() {
                let retirement = self
                    .binding_retirement
                    .get(&binding)
                    .ok_or(ViewportCoordinatorError::MissingBindingRetirement { binding })?;
                let active_record = existing_effect.and_then(|effect| self.effects.record(effect));
                let can_migrate = active_record.is_some_and(|record| {
                    is_destructive_cleanup(record.request().effect())
                        || matches!(
                            record.request().effect(),
                            PlatformEffect::ContinueCleanup { .. }
                        )
                });
                if can_migrate
                    && let Some(action) = self.retirement_cleanup_restore_action(retirement)?
                {
                    self.apply_retirement_cleanup_actions(
                        self.workspace_epoch,
                        vec![action],
                        &mut accumulated.cleanup_effects,
                    )?;
                }
            } else if let Some(effect) = self.drive_binding_retirement(binding)? {
                accumulated.cleanup_effects.push(effect);
            }
        }
        Ok(())
    }

    fn retirement_status_for_effect(&self, effect: EffectId) -> BindingRetirementStatus {
        match self
            .effects
            .record(effect)
            .map(crate::effect::EffectRecord::phase)
        {
            Some(EffectPhase::Indeterminate(_)) => {
                BindingRetirementStatus::CleanupIndeterminate { effect }
            }
            Some(EffectPhase::ObservationDispatchFailed(_)) => {
                BindingRetirementStatus::CleanupObservationFailed { effect }
            }
            Some(EffectPhase::DispatchFailed(_)) => {
                BindingRetirementStatus::CleanupFailed { effect }
            }
            Some(
                EffectPhase::Unsupported(reason) | EffectPhase::ObservationUnsupported(reason),
            ) => BindingRetirementStatus::CleanupBlocked { effect, reason },
            _ => BindingRetirementStatus::CleanupRequested { effect },
        }
    }

    fn begin_binding_retirement(
        &mut self,
        request: BindingRetirementRequest,
    ) -> Result<(), ViewportCoordinatorError> {
        self.binding_retirement
            .begin(request)
            .map_err(binding_retirement_error)
    }

    fn finish_binding_retirement(
        &mut self,
        terminal: BindingRetirementTerminal,
    ) -> Result<(), ViewportCoordinatorError> {
        if let Some(resource) = terminal.retained_staging_resource() {
            self.release_native_staging_resource(
                resource,
                NativeStagingResourceOwner::BindingRetirement(terminal.binding()),
            )?;
        }
        Ok(())
    }

    fn drive_binding_retirement(
        &mut self,
        binding: ViewportBinding,
    ) -> Result<Option<EffectId>, ViewportCoordinatorError> {
        if !self.binding_retirement.contains_key(&binding) {
            return Ok(None);
        }
        let pointer_restore_pending = self.pointer_passthrough.contains_binding(binding);
        let directive = self
            .binding_retirement
            .plan_drive(binding, pointer_restore_pending)
            .map_err(binding_retirement_error)?;
        directive.map_or(Ok(None), |directive| {
            self.apply_binding_retirement_directive(directive)
        })
    }

    fn apply_binding_retirement_directive(
        &mut self,
        directive: BindingRetirementDirective,
    ) -> Result<Option<EffectId>, ViewportCoordinatorError> {
        match directive {
            BindingRetirementDirective::Drive { binding } => self.drive_binding_retirement(binding),
            BindingRetirementDirective::RequestCleanup(request) => {
                let binding = request.binding();
                let effect = self.request_retirement_cleanup(request)?;
                self.binding_retirement
                    .accept_cleanup_effect(binding, effect)
                    .map_err(binding_retirement_error)?;
                Ok(Some(effect))
            }
            BindingRetirementDirective::Finish(terminal) => {
                self.finish_binding_retirement(terminal)?;
                Ok(None)
            }
            BindingRetirementDirective::ExactDestroyed(terminal) => {
                self.mark_binding_effects_destroyed(terminal.binding());
                self.pointer_passthrough
                    .terminate_binding(terminal.binding());
                self.finish_binding_retirement(terminal)?;
                Ok(None)
            }
        }
    }

    fn finish_restore(
        &mut self,
        desired_surfaces: &BTreeSet<SurfaceId>,
        unbound_surfaces: &mut Vec<SurfaceId>,
    ) -> Result<(), ViewportCoordinatorError> {
        let pending_resources = self
            .recovery_replacements
            .values()
            .filter_map(RecoveryPending::retained_staging_resource)
            .collect::<Vec<_>>();
        for resource in pending_resources {
            let owner = self
                .native_staging_resource_owner(resource)
                .ok_or(NativeStagingResourceLedgerError::MissingResource { resource })?;
            if !matches!(owner, NativeStagingResourceOwner::BindingRetirement(_)) {
                self.release_native_staging_resource(resource, owner)?;
            }
        }
        self.clear_active_native_creates();
        let _ = self.recovery_replacements.clear();
        self.capabilities = PlatformCapabilities::default();
        self.work_area_observations = WorkAreaRosterObservationStream::default();
        self.work_areas.clear();
        let bound_surfaces: BTreeSet<_> = self
            .registry
            .records()
            .map(|(surface, _)| surface)
            .collect();
        let additionally_unbound: Vec<_> = desired_surfaces
            .iter()
            .filter(|surface| {
                !bound_surfaces.contains(surface) && !unbound_surfaces.contains(surface)
            })
            .copied()
            .collect();
        unbound_surfaces.extend(additionally_unbound);
        unbound_surfaces.sort_unstable();
        Ok(())
    }

    /// Atomically publishes one complete platform snapshot.
    pub(crate) fn publish_snapshot(
        &mut self,
        snapshot: &PlatformSnapshot,
    ) -> Result<ViewportFrameTransition, ViewportCoordinatorError> {
        let mut candidate = self.clone();
        let provider = candidate.active_platform_provider()?;
        if let Some(current) = candidate
            .inventory_observations
            .stale_against(snapshot.inventory_observation())
        {
            return Err(ViewportCoordinatorError::StaleInventoryObservation {
                submitted: snapshot.inventory_observation().generation(),
                current,
            });
        }
        if candidate
            .inventory_observations
            .conflicts_with(snapshot.inventory_observation())
        {
            return Err(ViewportCoordinatorError::ConflictingInventoryObservation {
                generation: snapshot.inventory_observation().generation(),
            });
        }
        if let Some(previous) = candidate
            .inventory_observations
            .generation_gap_before(snapshot.inventory_observation())
        {
            return Err(
                ViewportCoordinatorError::InventoryObservationGenerationGap {
                    previous,
                    submitted: snapshot.inventory_observation().generation(),
                },
            );
        }
        if let Some(previous) = candidate.last_platform_snapshot.as_ref() {
            if snapshot.generation() < previous.generation() {
                return Err(ViewportCoordinatorError::StalePlatformSnapshot {
                    submitted: snapshot.generation(),
                    current: previous.generation(),
                });
            }
            if snapshot.generation() == previous.generation() {
                if snapshot != previous {
                    return Err(ViewportCoordinatorError::ConflictingPlatformSnapshot {
                        generation: snapshot.generation(),
                    });
                }
                return Ok(ViewportFrameTransition {
                    capability_generation: candidate.capability_generation,
                    work_area_generation: candidate.work_area_generation,
                    capabilities_changed: false,
                    work_areas_changed: false,
                    registry_events: Vec::new(),
                    consumed_staging_close_requests: Vec::new(),
                    actions: Vec::new(),
                });
            }
        }
        candidate
            .capability_observations
            .observe(Some(snapshot.capability_observation().clone()));
        let capabilities = candidate
            .capability_observations
            .current()
            .and_then(|observation| observation.known_roster())
            .cloned()
            .unwrap_or_default();
        let capabilities_changed = candidate.capabilities != capabilities;
        if capabilities_changed {
            candidate.capability_generation = candidate
                .capability_generation
                .checked_next()
                .ok_or(ViewportCoordinatorError::CapabilityGenerationExhausted)?;
            candidate.capabilities = capabilities;
        }
        let capability_generation = candidate.capability_generation;
        candidate
            .inventory_observations
            .observe(Some(snapshot.inventory_observation().clone()));
        if !candidate
            .capabilities
            .authoritative_inventory()
            .is_supported()
        {
            // Retain the provider watermark, but revoke semantic authority. A
            // later capability recovery cannot resurrect inventory observed
            // while its prerequisite authority was unavailable.
            candidate.inventory_observations.observe(None);
        }
        let inventory = candidate
            .inventory_observations
            .current()
            .and_then(|observation| observation.known_roster());
        candidate
            .work_area_observations
            .observe(Some(snapshot.work_area_observation().clone()));
        if !candidate.capabilities.work_area().is_supported() {
            // Work-area geometry is meaningful only while the accepted
            // capability roster proves that this provider owns that fact.
            candidate.work_area_observations.observe(None);
        }
        let work_areas: BTreeMap<_, _> = candidate
            .work_area_observations
            .current()
            .and_then(|observation| observation.known_roster())
            .unwrap_or_default()
            .iter()
            .copied()
            .map(|work_area| (work_area.token(), work_area))
            .collect();
        let work_areas_changed = candidate.work_areas != work_areas;
        if work_areas_changed {
            candidate.work_area_generation = candidate
                .work_area_generation
                .checked_next()
                .ok_or(ViewportCoordinatorError::WorkAreaGenerationExhausted)?;
            candidate.work_areas = work_areas;
        }
        let recovery_coordinates: BTreeMap<_, _> = candidate
            .registry
            .records()
            .map(|(_, record)| (record.binding(), record.recovery_coordinates()))
            .collect();
        let retired_bindings = candidate
            .binding_retirement
            .keys()
            .copied()
            .collect::<BTreeSet<_>>();
        let registry = candidate
            .registry
            .apply_snapshot_with_retired_bindings(
                snapshot,
                inventory,
                &retired_bindings,
                candidate.binding_retirement.destroyed_bindings(),
            )
            .map_err(ViewportCoordinatorError::Registry)?;
        for binding in registry.events().iter().filter_map(|event| match event {
            RegistryEvent::Destroyed { observation } => Some(observation.binding()),
            RegistryEvent::Ready { .. }
            | RegistryEvent::PresentationChanged { .. }
            | RegistryEvent::FactsUnavailable { .. }
            | RegistryEvent::BindingMissing { .. }
            | RegistryEvent::CloseRequested { .. }
            | RegistryEvent::CloseRequestCleared { .. } => None,
        }) {
            candidate
                .binding_retirement
                .record_destroyed_tombstone(binding, provider);
            candidate.terminate_pointer_passthrough_binding(binding);
        }
        candidate.reconcile_binding_retirements(provider, snapshot)?;
        candidate.observe_pointer_passthrough_snapshot(provider);
        candidate.reconcile_pointer_passthrough_sagas()?;
        let retirement_bindings = candidate
            .binding_retirement
            .keys()
            .copied()
            .collect::<Vec<_>>();
        for binding in retirement_bindings {
            let _ = candidate.drive_binding_retirement(binding)?;
        }
        let registry_events = registry.events().to_vec();
        // A pre-admission replacement close belongs to the coordinator, not the
        // graph-level close/recovery reducer. Establish that ownership before
        // deriving lifecycle actions so this same snapshot cannot also enqueue
        // `RetryRecovery` and create a second compensating close.
        let mut consumed_staging_close_requests = Vec::new();
        for requested in registry_events.iter().filter_map(|event| match event {
            RegistryEvent::CloseRequested { observation } => Some(*observation),
            RegistryEvent::Ready { .. }
            | RegistryEvent::PresentationChanged { .. }
            | RegistryEvent::FactsUnavailable { .. }
            | RegistryEvent::BindingMissing { .. }
            | RegistryEvent::CloseRequestCleared { .. }
            | RegistryEvent::Destroyed { .. } => None,
        }) {
            if candidate.abort_staging_close(requested)? {
                consumed_staging_close_requests.push(requested);
            }
        }
        let actions = candidate.reduce_registry_events(&registry_events, &recovery_coordinates)?;
        let transition = ViewportFrameTransition {
            capability_generation,
            work_area_generation: candidate.work_area_generation,
            capabilities_changed,
            work_areas_changed,
            registry_events,
            consumed_staging_close_requests,
            actions,
        };
        candidate.last_platform_snapshot = Some(snapshot.clone());
        *self = candidate;
        Ok(transition)
    }

    /// Publishes one exact native-close fact without replacing the complete
    /// platform snapshot retained for every other authority lane.
    pub(crate) fn publish_close_observation(
        &mut self,
        observation: WindowCloseObservation,
    ) -> Result<ViewportFrameTransition, ViewportCoordinatorError> {
        let mut candidate = self.clone();
        candidate.active_platform_provider()?;
        let recovery_coordinates = candidate
            .registry
            .records()
            .map(|(_, record)| (record.binding(), record.recovery_coordinates()))
            .collect::<BTreeMap<_, _>>();
        let registry = candidate
            .registry
            .apply_close_observation(observation)
            .map_err(ViewportCoordinatorError::Registry)?;
        let registry_events = registry.events().to_vec();
        let mut consumed_staging_close_requests = Vec::new();
        for requested in registry_events.iter().filter_map(|event| match event {
            RegistryEvent::CloseRequested { observation } => Some(*observation),
            RegistryEvent::Ready { .. }
            | RegistryEvent::PresentationChanged { .. }
            | RegistryEvent::FactsUnavailable { .. }
            | RegistryEvent::BindingMissing { .. }
            | RegistryEvent::CloseRequestCleared { .. }
            | RegistryEvent::Destroyed { .. } => None,
        }) {
            if candidate.abort_staging_close(requested)? {
                consumed_staging_close_requests.push(requested);
            }
        }
        let actions = candidate.reduce_registry_events(&registry_events, &recovery_coordinates)?;
        let transition = ViewportFrameTransition {
            capability_generation: candidate.capability_generation,
            work_area_generation: candidate.work_area_generation,
            capabilities_changed: false,
            work_areas_changed: false,
            registry_events,
            consumed_staging_close_requests,
            actions,
        };
        *self = candidate;
        Ok(transition)
    }

    /// Returns whether this binding has not yet acquired graph ownership.
    ///
    /// This intentionally includes retired native-create bindings: a late caller must not turn
    /// an old close edge into a new surface-close plan while its cleanup remains outstanding.
    pub(crate) fn is_staging_binding(&self, binding: ViewportBinding) -> bool {
        self.native_create_is_staging_binding(binding)
            || self.recovery_replacements.contains_replacement(binding)
            || self.recovery_replacements.contains_staging_close(binding)
            || self.binding_retirement.contains_key(&binding)
    }

    /// Returns whether a retained recovery is exclusively owned by a pending
    /// staging-close abort rather than by normal recovery retry.
    ///
    /// The binding is intentionally matched together with the exact destroyed
    /// binding and obligation. This makes an old abort unable to suppress a
    /// later recovery incarnation on the same logical surface.
    pub(crate) fn recovery_retry_is_suppressed(
        &self,
        destroyed_binding: ViewportBinding,
        recovery_obligation: SurfaceRecoveryObligationId,
    ) -> bool {
        self.recovery_replacements
            .recovery_retry_is_suppressed(destroyed_binding, recovery_obligation)
    }

    /// Consumes one native close request which targets a pre-admission binding.
    ///
    /// The caller must invoke this before exposing a close edge to the application. A staging
    /// window owns no workspace topology, so it is aborted or compensated internally instead of
    /// entering the ordinary `ClosePlan` protocol.
    fn abort_staging_close(
        &mut self,
        requested: WindowCloseObservation,
    ) -> Result<bool, ViewportCoordinatorError> {
        if requested.known_state() != Some(WindowCloseState::LiveRequested) {
            return Ok(false);
        }
        let binding = requested.binding();
        if self
            .recovery_replacements
            .staging_close_matches_retiring_request(requested)
        {
            return Ok(true);
        }
        if let Some(saga) = self.abortable_native_create_for_binding(binding) {
            let _ = self.abort_native_create(saga)?;
            return Ok(true);
        }

        if self.recovery_replacements.contains_replacement(binding) {
            let mut candidate = self.clone();
            candidate.abort_recovery_replacement(requested)?;
            *self = candidate;
            return Ok(true);
        }

        Ok(self
            .recovery_replacements
            .staging_close(binding)
            .is_some_and(|abort| abort.requested() == requested))
    }

    fn abort_recovery_replacement(
        &mut self,
        requested: WindowCloseObservation,
    ) -> Result<(), ViewportCoordinatorError> {
        let binding = requested.binding();
        if self
            .recovery_replacements
            .staging_close_matches_retiring_request(requested)
        {
            return Ok(());
        }
        let surface = self
            .recovery_replacements
            .replacement_surface(binding)
            .ok_or(ViewportCoordinatorError::MissingRecoveryPending {
                surface: binding.surface(),
            })?;
        let compensates = self
            .recovery_replacements
            .pending(surface)
            .and_then(RecoveryPending::replacement_effect);
        self.begin_staging_replacement_abort(
            requested,
            StagingCloseAbortOwner::RecoveryReplacement { surface },
            compensates,
        )
    }

    fn begin_staging_replacement_abort(
        &mut self,
        requested: WindowCloseObservation,
        owner: StagingCloseAbortOwner,
        compensates: Option<EffectId>,
    ) -> Result<(), ViewportCoordinatorError> {
        let binding = requested.binding();
        let pre_close_admission = match self.registry.record(binding.surface()) {
            Some(record) if record.binding() == binding => record.admission(),
            Some(_) => {
                return Err(ViewportCoordinatorError::Registry(
                    ViewportRegistryError::StaleBinding { binding },
                ));
            }
            None => {
                return Err(ViewportCoordinatorError::Registry(
                    ViewportRegistryError::MissingSurface {
                        surface: binding.surface(),
                    },
                ));
            }
        };
        if pre_close_admission != ViewportAdmission::Pending {
            return Err(
                ViewportCoordinatorError::StagingBindingUnexpectedAdmission {
                    binding,
                    admission: pre_close_admission,
                },
            );
        }
        self.registry
            .mark_awaiting_destroyed(binding)
            .map_err(ViewportCoordinatorError::Registry)?;
        self.pointer_passthrough.terminate_binding(binding);
        let cleanup = compensates
            .map(|compensates| {
                self.effects
                    .request(PlatformEffect::CompensatingClose {
                        binding,
                        compensates,
                    })
                    .map_err(ViewportCoordinatorError::Effect)
            })
            .transpose()?;
        self.recovery_replacements
            .begin_staging_close(StagingCloseAbortRequest {
                binding,
                owner,
                cleanup,
                requested,
                pre_close_admission,
            })
            .map_err(recovery_replacement_error)?;
        Ok(())
    }

    fn reduce_staging_close_clear(
        &mut self,
        requested: WindowCloseObservation,
        observation: WindowCloseObservation,
    ) -> Result<(), ViewportCoordinatorError> {
        let binding = observation.binding();
        if requested.binding() != binding
            || requested.known_state() != Some(WindowCloseState::LiveRequested)
            || observation.known_state() != Some(WindowCloseState::LiveClear)
        {
            return Ok(());
        }
        if !self
            .recovery_replacements
            .record_staging_close_clear(requested, observation)
        {
            return Ok(());
        }
        self.try_resume_staging_close_after_clear(binding)
    }

    fn reduce_staging_close_abort_result(
        &mut self,
        effect: EffectId,
    ) -> Result<(), ViewportCoordinatorError> {
        let bindings = self
            .recovery_replacements
            .staging_close_bindings_for_cleanup(effect);
        for binding in bindings {
            self.try_resume_staging_close_after_clear(binding)?;
        }
        Ok(())
    }

    fn try_resume_staging_close_after_clear(
        &mut self,
        binding: ViewportBinding,
    ) -> Result<(), ViewportCoordinatorError> {
        let Some(resume) = self.recovery_replacements.staging_close_resume(binding) else {
            return Ok(());
        };
        let pre_close_admission = resume.pre_close_admission;
        let cleanup = resume.cleanup;
        let clear = resume.clear;
        if pre_close_admission != ViewportAdmission::Pending {
            return Ok(());
        }
        self.invalidate_unemitted_staging_close_cleanup(cleanup);
        if !self.staging_close_cleanup_is_proven_safe(cleanup, clear) {
            return Ok(());
        }
        if !self
            .registry
            .resume_pending_after_staging_close_clear(binding)
            .map_err(ViewportCoordinatorError::Registry)?
        {
            return Ok(());
        }
        self.recovery_replacements
            .mark_staging_close_resumed(binding, clear.inventory_generation())
            .map_err(recovery_replacement_error)?;
        Ok(())
    }

    /// Drops a private abort cleanup before restoring its replacement to pending admission.
    ///
    /// The cleanup is queued while reducing `LiveRequested`, but platform effects are extracted
    /// only after the entire host frame settles. An exact `LiveClear` in that interval revokes
    /// the reason for the close; leaving the request pending would let it close a replacement
    /// after the coordinator has already restored it.
    fn invalidate_unemitted_staging_close_cleanup(&mut self, cleanup: Option<EffectId>) {
        let Some(cleanup) = cleanup else {
            return;
        };
        let should_invalidate = self.effects.record(cleanup).is_some_and(|record| {
            !record.was_emitted() && matches!(record.phase(), EffectPhase::Requested)
        });
        if should_invalidate {
            let _ = self
                .effects
                .invalidate_unemitted(cleanup, EffectInvalidation::StagingCloseCleared);
        }
    }

    fn staging_close_cleanup_is_proven_safe(
        &self,
        cleanup: Option<EffectId>,
        clear: WindowCloseObservation,
    ) -> bool {
        let Some(cleanup) = cleanup else {
            return true;
        };
        if clear.acknowledges(cleanup) {
            return true;
        }
        let Some(record) = self.effects.record(cleanup) else {
            return false;
        };
        !record.was_emitted()
            || matches!(
                record.phase(),
                EffectPhase::DispatchFailed(_) | EffectPhase::Unsupported(_)
            )
    }

    fn staging_close_abort_allows_replacement_admission(
        &self,
        binding: ViewportBinding,
        presentation: WindowPresentationObservation,
    ) -> bool {
        self.recovery_replacements
            .staging_close_allows_admission(binding, presentation)
    }

    fn reconcile_binding_retirements(
        &mut self,
        provider: PlatformObservationLease,
        snapshot: &PlatformSnapshot,
    ) -> Result<(), ViewportCoordinatorError> {
        let directives = self.binding_retirement.observe_snapshot(provider, snapshot);
        for directive in directives {
            let _ = self.apply_binding_retirement_directive(directive)?;
        }
        Ok(())
    }

    fn request_retirement_cleanup(
        &mut self,
        request: BindingRetirementCleanupRequest,
    ) -> Result<EffectId, ViewportCoordinatorError> {
        self.effects
            .request_in(self.workspace_epoch, request.platform_effect())
            .map_err(ViewportCoordinatorError::Effect)
    }

    fn reduce_registry_events(
        &mut self,
        events: &[RegistryEvent],
        recovery_coordinates: &BTreeMap<ViewportBinding, Option<RecoveryCoordinateSnapshot>>,
    ) -> Result<Vec<ViewportLifecycleAction>, ViewportCoordinatorError> {
        let mut actions = Vec::new();
        let mut presentation_reduced = BTreeSet::new();
        let mut suppress_recovery_retry = BTreeSet::new();
        for event in events {
            match *event {
                RegistryEvent::Ready { binding } => {
                    if presentation_reduced.insert(binding) {
                        self.reduce_ready_binding(binding)?;
                    }
                }
                RegistryEvent::PresentationChanged { binding } => {
                    if presentation_reduced.insert(binding) {
                        self.reduce_ready_binding(binding)?;
                    }
                }
                RegistryEvent::CloseRequested { observation } => {
                    if let Some(surface) =
                        self.recovery_surface_owned_by_staging_close(observation.binding())
                    {
                        suppress_recovery_retry.insert(surface);
                    }
                }
                RegistryEvent::CloseRequestCleared {
                    requested,
                    observation,
                } => {
                    self.reduce_staging_close_clear(requested, observation)?;
                }
                RegistryEvent::Destroyed { observation } => {
                    let binding = observation.binding();
                    let disposition = self.reduce_destroyed_binding(
                        observation,
                        recovery_coordinates.get(&binding).copied().flatten(),
                        &mut actions,
                    )?;
                    if let DestroyedBindingDisposition::Handled {
                        suppress_recovery_retry: Some(surface),
                    } = disposition
                    {
                        suppress_recovery_retry.insert(surface);
                    }
                }
                RegistryEvent::FactsUnavailable { .. } | RegistryEvent::BindingMissing { .. } => {}
            }
        }

        // A registry event can arrive in the same reducer batch that creates an
        // effect.  In that case the observation is deliberately rejected by
        // the ledger's emission fence.  Revisit the durable facts at every
        // snapshot boundary so a causal barrier cannot strand a replacement or
        // close request after its one-shot edge event has been consumed.
        let ready_bindings: Vec<ViewportBinding> = self
            .registry
            .records()
            .filter_map(|(_, record)| record.is_ready().then_some(record.binding()))
            .collect();
        for binding in ready_bindings {
            if presentation_reduced.insert(binding) {
                self.reduce_ready_binding(binding)?;
            }
        }
        let mut queued = BTreeSet::new();
        for pending in self.recovery_replacements.values() {
            if matches!(
                pending.status(),
                RecoveryPendingStatus::AwaitingRecoveryHost
                    | RecoveryPendingStatus::ReplacementRequested { .. }
                    | RecoveryPendingStatus::ReplacementIndeterminate { .. }
                    | RecoveryPendingStatus::ReplacementFailed { .. }
                    | RecoveryPendingStatus::ReplacementProviderLost { .. }
            ) && !suppress_recovery_retry.contains(&pending.destroyed_binding().surface())
                && !self.recovery_retry_is_suppressed(
                    pending.destroyed_binding(),
                    pending.recovery_obligation(),
                )
                && queued.insert(pending.destroyed_binding().surface())
            {
                actions.push(ViewportLifecycleAction::RetryRecovery {
                    destroyed_binding: pending.destroyed_binding(),
                    recovery_obligation: pending.recovery_obligation(),
                });
            }
        }
        Ok(actions)
    }

    fn recovery_surface_owned_by_staging_close(
        &self,
        binding: ViewportBinding,
    ) -> Option<SurfaceId> {
        self.recovery_replacements
            .staging_close_owner_surface(binding)
    }

    fn reduce_ready_binding(
        &mut self,
        binding: ViewportBinding,
    ) -> Result<(), ViewportCoordinatorError> {
        let provider = self.active_platform_provider()?;
        self.reduce_ready_recovery_replacement(provider, binding)?;
        self.reduce_ready_native_create(provider, binding)
    }

    fn reduce_ready_recovery_replacement(
        &mut self,
        provider: PlatformObservationLease,
        binding: ViewportBinding,
    ) -> Result<(), ViewportCoordinatorError> {
        let Some((surface, phase)) = self.recovery_replacements.bringup(binding) else {
            return Ok(());
        };
        let pending = self
            .recovery_replacements
            .pending(surface)
            .ok_or(ViewportCoordinatorError::MissingRecoveryPending { surface })?;
        let owner = NativeStagingOwner::recovery_replacement(
            pending.destroyed_binding(),
            pending.recovery_obligation(),
            pending.retained_staging_resource(),
        )
        .ok_or(ViewportCoordinatorError::InvalidNativeStagingPresentation { binding })?;
        if let Some(next) = self.reduce_ready_native_bringup(provider, binding, owner, phase)? {
            self.recovery_replacements
                .update_bringup(binding, phase, next)
                .map_err(recovery_replacement_error)?;
        }
        Ok(())
    }

    fn replacement_is_admissible(&self, binding: ViewportBinding) -> bool {
        self.registry
            .record(binding.surface())
            .is_some_and(|record| {
                record.binding() == binding
                    && record.admission() != ViewportAdmission::Retiring
                    && record.is_ready()
                    && record
                        .presentation_observation()
                        .is_some_and(|observation| {
                            observation.known_state() == Some(WindowPresentationState::Visible)
                                && self.staging_close_abort_allows_replacement_admission(
                                    binding,
                                    observation,
                                )
                        })
            })
    }

    fn reduce_pointer_passthrough_dispatch_result(
        &mut self,
        effect: EffectId,
        result: EffectDispatchResult,
    ) -> Result<(), ViewportCoordinatorError> {
        let Some(binding) =
            self.effects
                .record(effect)
                .and_then(|record| match record.request().effect() {
                    PlatformEffect::SetPointerPassthrough { binding, .. } => Some(*binding),
                    _ => None,
                })
        else {
            return Ok(());
        };
        let evidence = self.pointer_passthrough_evidence(binding);
        let matched = self
            .pointer_passthrough
            .record_dispatch_result(binding, effect, result, evidence);
        if matched {
            let _ = self.reconcile_pointer_passthrough_saga(binding)?;
        }
        Ok(())
    }

    fn observe_pointer_passthrough_snapshot(&mut self, provider: PlatformObservationLease) {
        for binding in self.pointer_passthrough.tracked_bindings() {
            let Some(observation) = self.pointer_passthrough_evidence(binding).observation() else {
                continue;
            };
            let Some(effect) = self.pointer_passthrough.causal_effect(binding, observation) else {
                continue;
            };
            let transition = self.effects.mark_observed_applied(
                provider,
                effect,
                binding,
                self.registry.inventory_generation(),
            );
            let accepted = matches!(
                transition,
                EffectTransition::Applied | EffectTransition::Duplicate
            ) && self.effects.record(effect).is_some_and(|record| {
                matches!(record.phase(), EffectPhase::ObservedApplied { .. })
            });
            if accepted {
                self.pointer_passthrough
                    .accept_observed_effect(binding, effect);
            }
        }
    }

    fn reconcile_pointer_passthrough_sagas(&mut self) -> Result<(), ViewportCoordinatorError> {
        for binding in self.pointer_passthrough.tracked_bindings() {
            let _ = self.reconcile_pointer_passthrough_saga(binding)?;
        }
        Ok(())
    }

    fn reconcile_pointer_passthrough_saga(
        &mut self,
        binding: ViewportBinding,
    ) -> Result<Option<EffectId>, ViewportCoordinatorError> {
        let evidence = self.pointer_passthrough_evidence(binding);
        let attempts = self.pointer_passthrough.effect_attempts(binding);
        let enable_phase = attempts.enable().and_then(|effect| {
            self.effects
                .record(effect)
                .map(crate::effect::EffectRecord::phase)
        });
        let restore_phase = attempts.restore().and_then(|effect| {
            self.effects
                .record(effect)
                .map(crate::effect::EffectRecord::phase)
        });
        let request = self
            .pointer_passthrough
            .reconcile(binding, evidence, enable_phase, restore_phase)
            .map_err(pointer_passthrough_error)?;
        request
            .map(|request| self.apply_pointer_passthrough_request_in(self.workspace_epoch, request))
            .transpose()
    }

    fn request_pointer_input_restore_in(
        &mut self,
        issuance_epoch: WorkspaceEpoch,
        binding: ViewportBinding,
    ) -> Result<EffectId, ViewportCoordinatorError> {
        let evidence = self.pointer_passthrough_evidence(binding);
        let request = self
            .pointer_passthrough
            .restore_request(binding, evidence)
            .map_err(pointer_passthrough_error)?;
        self.apply_pointer_passthrough_request_in(issuance_epoch, request)
    }

    fn apply_pointer_passthrough_request_in(
        &mut self,
        issuance_epoch: WorkspaceEpoch,
        request: PointerPassthroughEffectRequest,
    ) -> Result<EffectId, ViewportCoordinatorError> {
        let binding = request.binding();
        let after = self.pointer_input_lane_predecessor(request.lane_tail());
        let effect = self
            .effects
            .request_in(
                issuance_epoch,
                PlatformEffect::SetPointerPassthrough {
                    binding,
                    enabled: request.enabled(),
                    after,
                },
            )
            .map_err(ViewportCoordinatorError::Effect)?;
        self.pointer_passthrough
            .accept_effect_request(request, effect)
            .map_err(pointer_passthrough_error)?;
        Ok(effect)
    }

    fn pointer_input_lane_predecessor(
        &self,
        mut predecessor: Option<EffectId>,
    ) -> Option<EffectId> {
        while let Some(effect) = predecessor {
            let record = self.effects.record(effect)?;
            let invalidated_unemitted =
                !record.was_emitted() && matches!(record.phase(), EffectPhase::Invalidated { .. });
            if !invalidated_unemitted {
                return Some(effect);
            }
            predecessor = match record.request().effect() {
                PlatformEffect::SetPointerPassthrough { after, .. } => *after,
                _ => None,
            };
        }
        None
    }

    fn pointer_passthrough_evidence(&self, binding: ViewportBinding) -> PointerPassthroughEvidence {
        let (observation, generation_watermark, window_observed, routeable) =
            self.pointer_input_facts(binding);
        PointerPassthroughEvidence::new(
            observation,
            generation_watermark,
            window_observed,
            routeable,
            self.capabilities.pointer_hit_test_observation(),
            self.capabilities.pointer_hit_test_control(),
        )
    }

    fn pointer_input_facts(
        &self,
        binding: ViewportBinding,
    ) -> (
        Option<WindowInputObservation>,
        Option<InputObservationGeneration>,
        bool,
        bool,
    ) {
        if let Some(record) = self
            .registry
            .record(binding.surface())
            .filter(|record| record.binding() == binding)
        {
            return (
                record.input_observation(),
                record.input_observation_generation_watermark(),
                record.is_observed(),
                record.is_routeable(),
            );
        }
        self.binding_retirement
            .get(&binding)
            .map_or((None, None, false, false), |retired| {
                (
                    retired.input_observations().current(),
                    retired.input_observations().generation_watermark(),
                    retired.observed(),
                    false,
                )
            })
    }

    fn terminate_pointer_passthrough_binding(&mut self, binding: ViewportBinding) {
        self.pointer_passthrough.terminate_binding(binding);
        let effects: Vec<EffectId> = self
            .effects
            .records()
            .filter_map(|(effect, record)| {
                matches!(
                    record.request().effect(),
                    PlatformEffect::SetPointerPassthrough { binding: target, .. }
                        if *target == binding
                )
                .then_some(effect)
            })
            .collect();
        for effect in effects {
            let _ =
                self.effects
                    .mark_destroyed(effect, binding, self.registry.inventory_generation());
        }
    }

    fn prepare_pointer_passthrough_for_vacancy(
        &mut self,
        binding: ViewportBinding,
    ) -> Result<bool, ViewportCoordinatorError> {
        self.pointer_passthrough
            .prepare_binding_for_vacancy(binding);
        let _ = self.reconcile_pointer_passthrough_saga(binding)?;
        Ok(!self.pointer_passthrough.contains_binding(binding))
    }

    fn reduce_destroyed_binding(
        &mut self,
        observation: WindowCloseObservation,
        source_coordinates: Option<RecoveryCoordinateSnapshot>,
        actions: &mut Vec<ViewportLifecycleAction>,
    ) -> Result<DestroyedBindingDisposition, ViewportCoordinatorError> {
        let binding = observation.binding();
        self.mark_binding_effects_destroyed(binding);
        if let Some(abort) = self.recovery_replacements.take_staging_close(binding) {
            return self.reduce_staging_close_abort_destroyed(binding, abort, actions);
        }

        if let Some(disposition) = self.reduce_replacement_destroyed(binding, actions)? {
            return Ok(disposition);
        }
        let Some(ownership_transferred) = self.settle_destroyed_native_create(binding)? else {
            self.push_direct_destruction(observation, source_coordinates, actions);
            return Ok(DestroyedBindingDisposition::ForwardToEngine);
        };
        if ownership_transferred {
            self.push_direct_destruction(observation, source_coordinates, actions);
            return Ok(DestroyedBindingDisposition::ForwardToEngine);
        }
        let _ = self.registry.remove_destroyed(binding);
        Ok(DestroyedBindingDisposition::Handled {
            suppress_recovery_retry: None,
        })
    }

    fn mark_binding_effects_destroyed(&mut self, binding: ViewportBinding) {
        let effects = self
            .effects
            .records()
            .filter_map(|(effect, record)| {
                (record.request().effect().binding() == binding).then_some(effect)
            })
            .collect::<Vec<_>>();
        for effect in effects {
            let _ =
                self.effects
                    .mark_destroyed(effect, binding, self.registry.inventory_generation());
        }
    }

    fn reduce_staging_close_abort_destroyed(
        &mut self,
        binding: ViewportBinding,
        abort: StagingCloseAbort,
        actions: &mut Vec<ViewportLifecycleAction>,
    ) -> Result<DestroyedBindingDisposition, ViewportCoordinatorError> {
        if let Some(cleanup) = abort.cleanup() {
            let _ =
                self.effects
                    .mark_destroyed(cleanup, binding, self.registry.inventory_generation());
        }
        match abort.owner() {
            StagingCloseAbortOwner::RecoveryReplacement { surface } => {
                let pending = self
                    .recovery_replacements
                    .pending(surface)
                    .filter(|pending| pending.replacement_binding() == Some(binding))
                    .cloned()
                    .ok_or(
                        ViewportCoordinatorError::PendingRecoveryRegistrationMismatch { surface },
                    )?;
                if let Some(effect) = pending.replacement_effect() {
                    let _ = self.effects.mark_destroyed(
                        effect,
                        binding,
                        self.registry.inventory_generation(),
                    );
                }
                let was_awaiting_first_live = matches!(
                    pending.status(),
                    RecoveryPendingStatus::AwaitingFirstLivePresentation
                );
                self.return_recovery_replacement_resource_to_surface(&pending, binding)?;
                self.registry
                    .remove_destroyed(binding)
                    .map_err(ViewportCoordinatorError::Registry)?;
                let pending = self
                    .recovery_replacements
                    .reset_lost_replacement(surface, binding)
                    .map_err(recovery_replacement_error)?;
                if was_awaiting_first_live {
                    actions.push(ViewportLifecycleAction::RecoveryReplacementLost {
                        destroyed_binding: pending.destroyed_binding(),
                        replacement_binding: binding,
                        recovery_obligation: pending.recovery_obligation(),
                    });
                }
                Ok(DestroyedBindingDisposition::Handled {
                    suppress_recovery_retry: Some(surface),
                })
            }
        }
    }

    fn reduce_replacement_destroyed(
        &mut self,
        binding: ViewportBinding,
        actions: &mut Vec<ViewportLifecycleAction>,
    ) -> Result<Option<DestroyedBindingDisposition>, ViewportCoordinatorError> {
        let surface = self.recovery_replacements.replacement_surface(binding);
        let Some(surface) = surface else {
            return Ok(None);
        };
        let pending = self
            .recovery_replacements
            .pending(surface)
            .cloned()
            .ok_or(ViewportCoordinatorError::MissingRecoveryPending { surface })?;
        if let RecoveryPendingStatus::CompensatingReplacement {
            cleanup: effect, ..
        } = pending.status()
        {
            let _ =
                self.effects
                    .mark_destroyed(effect, binding, self.registry.inventory_generation());
            if let Some(resource) = pending.retained_staging_resource() {
                self.release_native_staging_resource(
                    resource,
                    NativeStagingResourceOwner::SurfaceRecovery {
                        obligation: pending.recovery_obligation(),
                        binding: pending.destroyed_binding(),
                    },
                )?;
            }
            self.registry
                .remove_destroyed(binding)
                .map_err(ViewportCoordinatorError::Registry)?;
            let _ = self
                .recovery_replacements
                .take_for_vacancy(surface, binding)
                .map_err(recovery_replacement_error)?;
            return Ok(Some(DestroyedBindingDisposition::Handled {
                suppress_recovery_retry: None,
            }));
        }
        if let Some(effect) = pending.replacement_effect() {
            let _ =
                self.effects
                    .mark_destroyed(effect, binding, self.registry.inventory_generation());
        }
        let destroyed_binding = pending.destroyed_binding();
        let recovery_obligation = pending.recovery_obligation();
        let was_awaiting_first_live = matches!(
            pending.status(),
            RecoveryPendingStatus::AwaitingFirstLivePresentation
        );
        self.return_recovery_replacement_resource_to_surface(&pending, binding)?;
        self.registry
            .remove_destroyed(binding)
            .map_err(ViewportCoordinatorError::Registry)?;
        let _ = self
            .recovery_replacements
            .reset_lost_replacement(surface, binding)
            .map_err(recovery_replacement_error)?;
        if was_awaiting_first_live {
            actions.push(ViewportLifecycleAction::RecoveryReplacementLost {
                destroyed_binding,
                replacement_binding: binding,
                recovery_obligation,
            });
        }
        Ok(Some(DestroyedBindingDisposition::Handled {
            suppress_recovery_retry: Some(surface),
        }))
    }

    fn return_recovery_replacement_resource_to_surface(
        &mut self,
        pending: &RecoveryPending,
        replacement_binding: ViewportBinding,
    ) -> Result<(), ViewportCoordinatorError> {
        if !matches!(
            pending.status(),
            RecoveryPendingStatus::AwaitingFirstLivePresentation
        ) {
            return Ok(());
        }
        if pending.replacement_binding() != Some(replacement_binding) {
            return Err(
                ViewportCoordinatorError::PendingRecoveryRegistrationMismatch {
                    surface: pending.destroyed_binding().surface(),
                },
            );
        }
        if let Some(resource) = pending.retained_staging_resource() {
            self.transition_native_staging_resource(
                resource,
                NativeStagingResourceOwner::RecoveryReplacementFirstLive {
                    obligation: pending.recovery_obligation(),
                    binding: replacement_binding,
                },
                NativeStagingResourceOwner::SurfaceRecovery {
                    obligation: pending.recovery_obligation(),
                    binding: pending.destroyed_binding(),
                },
            )?;
        }
        Ok(())
    }

    fn push_direct_destruction(
        &self,
        observation: WindowCloseObservation,
        source_coordinates: Option<RecoveryCoordinateSnapshot>,
        actions: &mut Vec<ViewportLifecycleAction>,
    ) {
        actions.push(ViewportLifecycleAction::SurfaceDestroyed {
            observation,
            source_coordinates,
        });
    }

    pub(crate) fn retry_cleanup(
        &mut self,
        failed_effect: EffectId,
    ) -> Result<EffectId, ViewportCoordinatorError> {
        let phase = self
            .effects
            .record(failed_effect)
            .map(crate::effect::EffectRecord::phase)
            .ok_or(ViewportCoordinatorError::MissingCleanupEffect {
                effect: failed_effect,
            })?;
        let retry_observation = matches!(
            phase,
            EffectPhase::ObservationDispatchFailed(_) | EffectPhase::ObservationUnsupported(_)
        );
        if !matches!(
            phase,
            EffectPhase::DispatchFailed(_) | EffectPhase::Unsupported(_)
        ) && !retry_observation
        {
            return Err(ViewportCoordinatorError::CleanupEffectNotRetryable {
                effect: failed_effect,
                phase,
            });
        }

        let mut candidate = self.clone();
        let retry = if retry_observation {
            candidate
                .retry_retirement_cleanup_observation(failed_effect)?
                .ok_or(ViewportCoordinatorError::MissingCleanupEffect {
                    effect: failed_effect,
                })?
        } else if let Some(retry) = candidate.retry_retirement_destructive_cleanup(failed_effect)? {
            retry
        } else if let Some(retry) = candidate.retry_replacement_cleanup(failed_effect)? {
            retry
        } else {
            return Err(ViewportCoordinatorError::MissingCleanupEffect {
                effect: failed_effect,
            });
        };
        *self = candidate;
        Ok(retry)
    }

    fn retry_retirement_destructive_cleanup(
        &mut self,
        failed_effect: EffectId,
    ) -> Result<Option<EffectId>, ViewportCoordinatorError> {
        let Some(request) = self
            .binding_retirement
            .plan_destructive_retry(failed_effect)
            .map_err(binding_retirement_error)?
        else {
            return Ok(None);
        };
        let binding = request.binding();
        let retry = self.request_retirement_cleanup(request)?;
        self.binding_retirement
            .accept_cleanup_effect(binding, retry)
            .map_err(binding_retirement_error)?;
        Ok(Some(retry))
    }

    fn retry_retirement_cleanup_observation(
        &mut self,
        failed_effect: EffectId,
    ) -> Result<Option<EffectId>, ViewportCoordinatorError> {
        let Some(retirement_binding) = self
            .binding_retirement
            .observation_retry_binding(failed_effect)
        else {
            return Ok(None);
        };
        let retirement = self.binding_retirement.get(&retirement_binding).ok_or(
            ViewportCoordinatorError::MissingBindingRetirement {
                binding: retirement_binding,
            },
        )?;
        let failed_record = self.effects.record(failed_effect).ok_or(
            ViewportCoordinatorError::MissingCleanupEffect {
                effect: failed_effect,
            },
        )?;
        let PlatformEffect::ContinueCleanup {
            binding,
            predecessor,
            ..
        } = failed_record.request().effect()
        else {
            return Err(ViewportCoordinatorError::InvalidCleanupContinuation {
                effect: failed_effect,
                predecessor: failed_effect,
            });
        };
        let predecessor_record = self.effects.record(*predecessor).ok_or(
            ViewportCoordinatorError::InvalidCleanupContinuation {
                effect: failed_effect,
                predecessor: *predecessor,
            },
        )?;
        if !failed_record.was_emitted()
            || *binding != retirement.binding()
            || predecessor_record.request().effect().binding() != retirement.binding()
            || !predecessor_record.was_emitted()
            || !is_destructive_cleanup(predecessor_record.request().effect())
            || !matches!(
                predecessor_record.phase(),
                EffectPhase::Requested | EffectPhase::Indeterminate(_)
            )
        {
            return Err(ViewportCoordinatorError::InvalidCleanupContinuation {
                effect: failed_effect,
                predecessor: *predecessor,
            });
        }
        let binding = *binding;
        let predecessor = *predecessor;
        let after = self.cleanup_observation_lane_predecessor(failed_effect)?;
        let retry = self
            .effects
            .request_in(
                self.workspace_epoch,
                PlatformEffect::ContinueCleanup {
                    binding,
                    predecessor,
                    after,
                },
            )
            .map_err(ViewportCoordinatorError::Effect)?;
        self.binding_retirement
            .accept_cleanup_observation_effect(retirement_binding, retry, predecessor)
            .map_err(binding_retirement_error)?;
        if !matches!(
            self.effects.supersede_cleanup_observation(
                failed_effect,
                retry,
                predecessor,
                retirement_binding,
            ),
            EffectTransition::Applied | EffectTransition::Duplicate
        ) {
            return Err(ViewportCoordinatorError::InvalidCleanupContinuation {
                effect: retry,
                predecessor,
            });
        }
        Ok(Some(retry))
    }

    fn retry_replacement_cleanup(
        &mut self,
        failed_effect: EffectId,
    ) -> Result<Option<EffectId>, ViewportCoordinatorError> {
        let Some(surface) = self
            .recovery_replacements
            .compensation_surface(failed_effect)
        else {
            return Ok(None);
        };
        let (binding, compensates) = self
            .recovery_replacements
            .pending(surface)
            .and_then(|pending| {
                Some((
                    pending.replacement_binding()?,
                    pending.replacement_effect()?,
                ))
            })
            .ok_or(ViewportCoordinatorError::MissingReplacementEffect { surface })?;
        let retry = self
            .effects
            .request(PlatformEffect::CompensatingClose {
                binding,
                compensates,
            })
            .map_err(ViewportCoordinatorError::Effect)?;
        self.registry
            .mark_awaiting_destroyed(binding)
            .map_err(ViewportCoordinatorError::Registry)?;
        self.recovery_replacements
            .retry_compensation(failed_effect, retry)
            .map_err(recovery_replacement_error)?;
        Ok(Some(retry))
    }

    pub(crate) fn report_effect_from(
        &mut self,
        provider: PlatformObservationLease,
        current_epoch: WorkspaceEpoch,
        result: EffectResult,
    ) -> Result<EffectTransition, ViewportCoordinatorError> {
        let effect = result.effect();
        if let Some(observation) = result.cleanup_observation() {
            let mut candidate = self.clone();
            let transition =
                candidate
                    .effects
                    .report_cleanup_observation(provider, current_epoch, result);
            if transition != EffectTransition::Applied {
                return Ok(transition);
            }
            let predecessor_phase = candidate
                .effects
                .record(effect)
                .map(EffectRecord::phase)
                .ok_or(ViewportCoordinatorError::MissingCleanupEffect { effect })?;
            let reduced = candidate
                .binding_retirement
                .reduce_cleanup_observation(
                    observation.continuation(),
                    observation.predecessor(),
                    predecessor_phase,
                )
                .map_err(binding_retirement_error)?;
            if reduced != Some(observation.binding()) {
                return Err(ViewportCoordinatorError::InvalidCleanupContinuation {
                    effect: observation.continuation(),
                    predecessor: observation.predecessor(),
                });
            }
            let retirement_bindings = candidate
                .binding_retirement
                .keys()
                .copied()
                .collect::<Vec<_>>();
            for binding in retirement_bindings {
                let _ = candidate.drive_binding_retirement(binding)?;
            }
            *self = candidate;
            return Ok(transition);
        }
        let transition = self.effects.report(provider, current_epoch, result);
        if transition != EffectTransition::Applied {
            return Ok(transition);
        }
        self.reduce_pointer_passthrough_dispatch_result(effect, result.result())?;
        let create_saga = self.native_create_for_effect(effect);
        if let Some(saga_id) = create_saga
            && matches!(
                result.result(),
                EffectDispatchResult::DispatchFailed(_) | EffectDispatchResult::Unsupported(_)
            )
        {
            let _ = self.abort_native_create(saga_id)?;
        }
        let _ = self.reduce_retirement_cleanup_result(effect)?;
        let retirement_bindings = self.binding_retirement.keys().copied().collect::<Vec<_>>();
        for binding in retirement_bindings {
            let _ = self.drive_binding_retirement(binding)?;
        }
        self.reduce_pending_recovery_result(effect, result.result())?;
        self.reduce_staging_close_abort_result(effect)?;
        Ok(transition)
    }

    #[cfg(test)]
    pub(crate) fn report_effect(
        &mut self,
        current_epoch: WorkspaceEpoch,
        result: EffectResult,
    ) -> Result<EffectTransition, ViewportCoordinatorError> {
        let provider = self.active_platform_provider()?;
        self.report_effect_from(provider, current_epoch, result)
    }

    fn reduce_retirement_cleanup_result(
        &mut self,
        effect: EffectId,
    ) -> Result<Option<ViewportBinding>, ViewportCoordinatorError> {
        let Some(retirement_phase) = self
            .effects
            .record(effect)
            .map(crate::effect::EffectRecord::phase)
        else {
            return Ok(None);
        };
        self.binding_retirement
            .reduce_effect(effect, retirement_phase)
            .map_err(binding_retirement_error)
    }

    fn reduce_pending_recovery_result(
        &mut self,
        effect: EffectId,
        result: EffectDispatchResult,
    ) -> Result<(), ViewportCoordinatorError> {
        let Some((surface, replacement)) =
            self.recovery_replacements.replacement_for_effect(effect)
        else {
            return Ok(());
        };
        let indeterminate = matches!(result, EffectDispatchResult::Indeterminate(_));
        let replacement_was_discarded = !indeterminate
            && replacement.is_some_and(|binding| self.registry.discard_unobserved(binding).is_ok());
        let observed = self.recovery_replacements.observe_replacement_dispatch(
            surface,
            effect,
            indeterminate,
            replacement_was_discarded,
        );
        if !indeterminate
            && !replacement_was_discarded
            && observed
            && let Some(binding) = replacement
        {
            let _ = self.retire_abandoned_recovery_replacement(
                surface,
                binding,
                AbandonedReplacementRetirementMode::DriveImmediately,
            )?;
        }
        Ok(())
    }

    pub(crate) fn request_native_close_resolution(
        &mut self,
        request: crate::close_plan::CloseRequestId,
        edge: NativeCloseEdge,
        resolution: crate::effect::NativeCloseResolution,
        after_effect: Option<EffectId>,
    ) -> Result<EffectId, ViewportCoordinatorError> {
        self.effects
            .request_native_close(
                self.workspace_epoch,
                request,
                edge,
                resolution,
                after_effect,
            )
            .map_err(ViewportCoordinatorError::Effect)
    }

    pub(crate) fn mark_native_close_awaiting_destroyed(
        &mut self,
        binding: ViewportBinding,
    ) -> Result<(), ViewportCoordinatorError> {
        self.registry
            .mark_awaiting_destroyed(binding)
            .map_err(ViewportCoordinatorError::Registry)
    }

    pub(crate) fn resume_after_failed_native_close(
        &mut self,
        binding: ViewportBinding,
    ) -> Result<(), ViewportCoordinatorError> {
        self.registry
            .resume_after_failed_close(binding)
            .map_err(ViewportCoordinatorError::Registry)
    }

    pub(crate) fn try_take_new_effects(
        &mut self,
    ) -> Result<Vec<PlatformEffectEmission>, ViewportCoordinatorError> {
        let Some(provider) = self.platform_provider.active() else {
            return Ok(Vec::new());
        };
        let inventory_generation = self.registry.inventory_generation();
        let registry = &self.registry;
        self.effects
            .take_new_requests(provider, inventory_generation, |edge| {
                matches!(
                    registry.native_close_edge_disposition(edge),
                    NativeCloseEdgeDisposition::ExactPending
                )
            })
            .map_err(ViewportCoordinatorError::Effect)
    }

    #[cfg(test)]
    pub(crate) fn take_new_effects(&mut self) -> Vec<EffectRequest> {
        self.try_take_new_effects()
            .expect("test effect extraction must preserve causal authority")
            .into_iter()
            .map(|emission| emission.request().clone())
            .collect()
    }

    pub(crate) fn latest_effect_id(&self) -> EffectId {
        self.effects.latest_id()
    }

    pub(crate) fn try_take_new_effects_after(
        &mut self,
        boundary: EffectId,
    ) -> Result<Vec<PlatformEffectEmission>, ViewportCoordinatorError> {
        let Some(provider) = self.platform_provider.active() else {
            return Ok(Vec::new());
        };
        let inventory_generation = self.registry.inventory_generation();
        let registry = &self.registry;
        self.effects
            .take_new_requests_after(boundary, provider, inventory_generation, |edge| {
                matches!(
                    registry.native_close_edge_disposition(edge),
                    NativeCloseEdgeDisposition::ExactPending
                )
            })
            .map_err(ViewportCoordinatorError::Effect)
    }

    /// Returns current typed platform facts for a frozen native close edge.
    #[must_use]
    pub(crate) fn native_close_edge_disposition(
        &self,
        edge: NativeCloseEdge,
    ) -> NativeCloseEdgeDisposition {
        self.registry.native_close_edge_disposition(edge)
    }

    /// Captures the exact current binding authority for one tick-start logical surface.
    pub(crate) fn capture_surface_vacancy_authority(
        &self,
        surface: SurfaceId,
    ) -> SurfaceVacancyAuthority {
        SurfaceVacancyAuthority::capture(surface, self.registry.record(surface))
    }

    fn vacancy_binding_has_terminal_owner(&self, binding: ViewportBinding) -> bool {
        self.binding_retirement.was_destroyed(binding)
            || self.binding_retirement.contains_key(&binding)
    }

    /// Reconciles native resources against exact tick-start binding authority.
    pub(crate) fn settle_surface_vacancies(
        &mut self,
        authorities: &[SurfaceVacancyAuthority],
        live_surfaces: &BTreeSet<SurfaceId>,
    ) -> Result<SurfaceVacancySettlement, ViewportCoordinatorError> {
        let mut candidate = self.clone();
        let mut settlement = SurfaceVacancySettlement::default();
        for authority in authorities {
            let surface = authority.surface();
            if live_surfaces.contains(&surface) {
                continue;
            }
            let actual = candidate.registry.record(surface);
            let actual_binding = actual.map(ViewportRecord::binding);
            let Some(expected) = authority.binding else {
                if actual_binding.is_some() {
                    return Err(ViewportCoordinatorError::SurfaceVacancyAuthorityChanged {
                        surface,
                        expected: None,
                        actual: actual_binding,
                    });
                }
                continue;
            };
            let Some(actual) = actual else {
                if candidate.vacancy_binding_has_terminal_owner(expected.binding) {
                    settlement.logical_vacated_bindings.push(expected.binding);
                    continue;
                }
                return Err(ViewportCoordinatorError::SurfaceVacancyAuthorityChanged {
                    surface,
                    expected: Some(expected.binding),
                    actual: None,
                });
            };
            if actual.binding() != expected.binding {
                return Err(ViewportCoordinatorError::SurfaceVacancyAuthorityChanged {
                    surface,
                    expected: Some(expected.binding),
                    actual: Some(actual.binding()),
                });
            }
            if actual.role() != expected.role || actual.ownership() != expected.ownership {
                return Err(
                    ViewportCoordinatorError::SurfaceVacancyBindingFactsChanged {
                        binding: expected.binding,
                        expected_role: expected.role,
                        actual_role: actual.role(),
                        expected_ownership: expected.ownership,
                        actual_ownership: actual.ownership(),
                    },
                );
            }
            settlement.logical_vacated_bindings.push(expected.binding);
            if actual.admission() == ViewportAdmission::Retiring {
                continue;
            }
            let mut retained_staging_resource = None;
            if let Some((saga, resource)) =
                candidate.retire_first_live_native_create_for_vacancy(expected.binding)
            {
                settlement
                    .retired_native_creates
                    .push((saga, expected.binding));
                retained_staging_resource = Some((
                    resource,
                    NativeStagingResourceOwner::NativeCreateFirstLive(expected.binding),
                ));
            } else if let Some(pending) = candidate
                .recovery_replacements
                .pending(surface)
                .filter(|pending| pending.replacement_binding() == Some(expected.binding))
                .cloned()
            {
                let _ = candidate
                    .recovery_replacements
                    .take_for_vacancy(surface, expected.binding)
                    .map_err(recovery_replacement_error)?;
                retained_staging_resource = pending.retained_staging_resource().map(|resource| {
                    let owner = if matches!(
                        pending.status(),
                        RecoveryPendingStatus::AwaitingFirstLivePresentation
                    ) {
                        NativeStagingResourceOwner::RecoveryReplacementFirstLive {
                            obligation: pending.recovery_obligation(),
                            binding: expected.binding,
                        }
                    } else {
                        NativeStagingResourceOwner::SurfaceRecovery {
                            obligation: pending.recovery_obligation(),
                            binding: pending.destroyed_binding(),
                        }
                    };
                    (resource, owner)
                });
            }
            let facts = candidate
                .registry
                .detach(expected.binding)
                .map_err(ViewportCoordinatorError::Registry)?;
            let binding = facts.binding();
            let observed = facts.ever_observed()
                && !matches!(
                    facts.lifecycle(),
                    ViewportLifecycle::Missing | ViewportLifecycle::Destroyed
                );
            let may_reappear =
                facts.ever_observed() && !matches!(facts.lifecycle(), ViewportLifecycle::Destroyed);
            let cleanup = match facts.ownership() {
                ViewportOwnership::External => BindingRetirementCleanup::KeepExternal,
                ViewportOwnership::RuntimeOwned => BindingRetirementCleanup::ReleaseOwnedWindow,
            };
            // Inventory absence cannot establish that an external window was destroyed.
            // Keep its token owned by this exact binding until a Destroyed observation
            // arrives, even if it was never observed in an inventory snapshot.
            let external_requires_exact_destruction = facts.ownership()
                == ViewportOwnership::External
                && !matches!(facts.lifecycle(), ViewportLifecycle::Destroyed);
            let needs_retirement = external_requires_exact_destruction
                || facts.ownership() == ViewportOwnership::RuntimeOwned
                || observed
                || may_reappear
                || candidate.pointer_passthrough.contains_binding(binding);
            if needs_retirement {
                if let Some((resource, owner)) = retained_staging_resource {
                    candidate.transition_native_staging_resource(
                        resource,
                        owner,
                        NativeStagingResourceOwner::BindingRetirement(binding),
                    )?;
                }
                candidate.begin_binding_retirement(BindingRetirementRequest {
                    binding,
                    role: facts.role(),
                    ownership: facts.ownership(),
                    origin: BindingRetirementOrigin::SurfaceVacated,
                    status: BindingRetirementStatus::AwaitingInputRestore,
                    observed,
                    input_observations: facts.input_observations(),
                    close_observations: facts.close_observations(),
                    may_reappear,
                    cleanup,
                    retained_staging_resource: retained_staging_resource
                        .map(|(resource, _)| resource),
                })?;
                let _ = candidate.prepare_pointer_passthrough_for_vacancy(binding)?;
                if let Some(effect) = candidate.drive_binding_retirement(binding)? {
                    settlement.effects.push(effect);
                }
            } else if let Some((resource, owner)) = retained_staging_resource {
                candidate.release_native_staging_resource(resource, owner)?;
            }
        }
        *self = candidate;
        Ok(settlement)
    }

    #[must_use]
    pub const fn capabilities(&self) -> &PlatformCapabilities {
        &self.capabilities
    }

    #[must_use]
    pub fn native_tear_off_capability(&self) -> PlatformCapability {
        self.capabilities.native_tear_off()
    }

    #[must_use]
    pub const fn capability_generation(&self) -> CapabilityGeneration {
        self.capability_generation
    }

    #[must_use]
    pub const fn work_area_generation(&self) -> WorkAreaGeneration {
        self.work_area_generation
    }

    /// Returns the latest accepted provider generation, including tombstones.
    #[must_use]
    pub const fn work_area_observation_generation(&self) -> Option<WorkAreaObservationGeneration> {
        self.work_area_observations.generation_watermark()
    }

    #[must_use]
    pub fn work_area(&self, token: WorkAreaToken) -> Option<ObservedWorkArea> {
        self.work_areas.get(&token).copied()
    }

    pub fn work_areas(&self) -> impl Iterator<Item = (WorkAreaToken, ObservedWorkArea)> + '_ {
        self.work_areas
            .iter()
            .map(|(token, work_area)| (*token, *work_area))
    }

    #[must_use]
    pub const fn registry(&self) -> &ViewportRegistry {
        &self.registry
    }

    #[must_use]
    pub const fn effects(&self) -> &EffectLedger {
        &self.effects
    }

    pub(crate) fn effect_retention_manifest(&self) -> EffectRetentionManifest {
        self.effects.retention_manifest()
    }

    pub(crate) fn extend_referenced_effects(&self, effects: &mut BTreeSet<EffectId>) {
        effects.extend(self.focus_effect_lane_tail);
        self.pointer_passthrough.extend_referenced_effects(effects);
        self.native_creates.extend_referenced_effects(effects);
        self.binding_retirement.extend_referenced_effects(effects);
        self.recovery_replacements
            .extend_referenced_effects(effects);
    }

    pub(crate) fn compact_published_terminal_effects(
        &mut self,
        retained_by_owner: &BTreeSet<EffectId>,
    ) -> usize {
        self.effects.compact_published_terminal(retained_by_owner)
    }

    pub(crate) fn mark_effect_boundary_published(&mut self) {
        self.effects.mark_boundary_published();
    }

    pub(crate) fn binding_retention_manifest(&self) -> BindingRetentionManifest {
        self.binding_retirement.retention_manifest()
    }

    pub fn binding_retirements(
        &self,
    ) -> impl Iterator<Item = (ViewportBinding, &BindingRetirement)> {
        self.binding_retirement
            .iter()
            .map(|(binding, retirement)| (*binding, retirement))
    }

    #[must_use]
    pub fn recovery_pending(&self, surface: SurfaceId) -> Option<&RecoveryPending> {
        self.recovery_replacements.pending(surface)
    }

    pub fn pending_recoveries(&self) -> impl Iterator<Item = (SurfaceId, &RecoveryPending)> {
        self.recovery_replacements
            .iter()
            .map(|(surface, pending)| (*surface, pending))
    }

    #[must_use]
    pub fn viewport(&self, surface: SurfaceId) -> Option<&ViewportRecord> {
        self.registry.record(surface)
    }

    pub(crate) fn surface_coordinate_authority(
        &self,
        surface: SurfaceId,
    ) -> crate::viewport::CoordinateGeneration {
        self.registry.surface_authority_generation(surface)
    }

    /// Produces an opaque placement proof from current acknowledged facts.
    ///
    /// # Errors
    ///
    /// Returns a typed coordinate error when the surface is not ready or its
    /// placement facts are incomplete.
    pub fn placement(
        &self,
        surface: SurfaceId,
        logical_rect: LogicalRect,
        work_area: WorkAreaToken,
    ) -> Result<ViewportPlacementProof, CoordinateUnavailable> {
        let work_area_facts = self
            .work_areas
            .get(&work_area)
            .copied()
            .ok_or(CoordinateUnavailable::UnknownWorkArea { token: work_area })?;
        self.registry.placement(
            surface,
            logical_rect,
            work_area_facts,
            self.work_area_generation,
        )
    }

    #[must_use]
    pub(crate) fn placement_is_current(&self, proof: &ViewportPlacementProof) -> bool {
        self.work_areas.contains_key(&proof.work_area())
            && self
                .registry
                .proof_is_current(proof, self.work_area_generation)
    }

    pub(crate) fn native_placement_is_current(&self, proof: &NativePlacementProof) -> bool {
        match proof {
            NativePlacementProof::Surface(proof) => self.placement_is_current(proof),
            NativePlacementProof::TearOff(proof) => {
                self.platform_provider() == Some(proof.platform_provider())
                    && self.work_area_generation == proof.work_area_generation()
                    && self.work_areas.contains_key(&proof.work_area())
                    && self.capabilities.native_tear_off().is_supported()
            }
        }
    }

    pub(crate) fn begin_drag_routing(
        &mut self,
        pointer: PointerId,
        surface: SurfaceId,
    ) -> Result<Option<EffectId>, ViewportCoordinatorError> {
        let Some(record) = self.registry.record(surface) else {
            return Ok(None);
        };
        let binding = record.binding();
        let mut candidate = self.clone();
        let Some(change) = candidate
            .pointer_passthrough
            .begin_routing(pointer, binding)
            .map_err(pointer_passthrough_error)?
        else {
            return Ok(None);
        };
        let restored = change
            .previous()
            .map(|previous| candidate.reconcile_pointer_passthrough_saga(previous))
            .transpose()?
            .flatten();
        let effect = candidate.reconcile_pointer_passthrough_saga(change.current())?;
        *self = candidate;
        Ok(effect.or(restored))
    }

    #[must_use]
    pub(crate) fn drag_source(&self, pointer: PointerId) -> Option<ViewportBinding> {
        self.pointer_passthrough.drag_source(pointer)
    }

    pub(crate) fn end_drag_routing(
        &mut self,
        pointer: PointerId,
    ) -> Result<Option<EffectId>, ViewportCoordinatorError> {
        if self.pointer_passthrough.drag_source(pointer).is_none() {
            return Ok(None);
        }
        let mut candidate = self.clone();
        let effect = candidate.end_drag_routing_in_place(pointer)?;
        *self = candidate;
        Ok(effect)
    }

    pub(crate) fn end_all_drag_routing(&mut self) -> Result<(), ViewportCoordinatorError> {
        let mut candidate = self.clone();
        let _ = candidate.end_all_drag_routing_in_place()?;
        *self = candidate;
        Ok(())
    }

    fn end_all_drag_routing_in_place(&mut self) -> Result<bool, ViewportCoordinatorError> {
        let pointers = self.pointer_passthrough.active_pointers();
        for pointer in &pointers {
            let _ = self.end_drag_routing_in_place(*pointer)?;
        }
        Ok(!pointers.is_empty())
    }

    fn end_drag_routing_in_place(
        &mut self,
        pointer: PointerId,
    ) -> Result<Option<EffectId>, ViewportCoordinatorError> {
        let Some(binding) = self
            .pointer_passthrough
            .end_routing(pointer)
            .map_err(pointer_passthrough_error)?
        else {
            return Ok(None);
        };
        let effect = self.reconcile_pointer_passthrough_saga(binding)?;
        Ok(effect)
    }

    pub(crate) fn request_focus_binding(
        &mut self,
        binding: ViewportBinding,
    ) -> Result<EffectId, ViewportCoordinatorError> {
        let after = self.focus_effect_lane_predecessor();
        let effect = self
            .effects
            .request(PlatformEffect::RequestFocus { binding, after })
            .map_err(ViewportCoordinatorError::Effect)?;
        self.focus_effect_lane_tail = Some(effect);
        Ok(effect)
    }

    fn focus_effect_lane_predecessor(&self) -> Option<EffectId> {
        let mut predecessor = self.focus_effect_lane_tail;
        while let Some(effect) = predecessor {
            let record = self.effects.record(effect)?;
            let invalidated_unemitted =
                !record.was_emitted() && matches!(record.phase(), EffectPhase::Invalidated { .. });
            if !invalidated_unemitted {
                return Some(effect);
            }
            predecessor = match record.request().effect() {
                PlatformEffect::RequestFocus { after, .. } => *after,
                _ => None,
            };
        }
        None
    }

    pub(crate) fn observe_focus_effect(
        &mut self,
        provider: PlatformObservationLease,
        effect: EffectId,
        binding: ViewportBinding,
    ) -> EffectTransition {
        self.effects.mark_observed_applied(
            provider,
            effect,
            binding,
            self.registry.inventory_generation(),
        )
    }

    pub(crate) fn observe_effect_applied(
        &mut self,
        provider: PlatformObservationLease,
        effect: EffectId,
        binding: ViewportBinding,
        inventory_generation: crate::viewport::InventoryGeneration,
    ) -> EffectTransition {
        self.effects
            .mark_observed_applied(provider, effect, binding, inventory_generation)
    }

    pub(crate) fn focus_effect_observation_matches(
        &self,
        provider: PlatformObservationLease,
        effect: EffectId,
        binding: ViewportBinding,
    ) -> bool {
        self.effects.observation_matches_delivery(
            provider,
            effect,
            binding,
            self.registry.inventory_generation(),
        )
    }

    pub(crate) fn complete_destroyed_surface(
        &mut self,
        binding: ViewportBinding,
    ) -> Result<(), ViewportCoordinatorError> {
        self.registry
            .remove_destroyed(binding)
            .map_err(ViewportCoordinatorError::Registry)?;
        Ok(())
    }

    pub(crate) fn defer_destroyed_surface_recovery(
        &mut self,
        binding: ViewportBinding,
        recovery_obligation: SurfaceRecoveryObligationId,
        retained_staging_resource: Option<NativeStagingResourceId>,
    ) -> Result<(), ViewportCoordinatorError> {
        if let Some(pending) = self.recovery_replacements.pending(binding.surface()) {
            if pending.destroyed_binding() == binding
                && pending.recovery_obligation() == recovery_obligation
                && pending.retained_staging_resource() == retained_staging_resource
            {
                return Ok(());
            }
            return Err(
                ViewportCoordinatorError::PendingRecoveryObligationMismatch {
                    surface: binding.surface(),
                },
            );
        }
        let record = self
            .registry
            .record(binding.surface())
            .filter(|record| record.binding() == binding)
            .ok_or(ViewportCoordinatorError::Registry(
                ViewportRegistryError::MissingSurface {
                    surface: binding.surface(),
                },
            ))?;
        if !matches!(record.lifecycle(), ViewportLifecycle::Destroyed) {
            return Err(ViewportCoordinatorError::DestroyedSurfaceStillObserved { binding });
        }
        let role = record.role();
        let placement = record.coordinates().map(|coordinates| {
            coordinates
                .outer_bounds()
                .unwrap_or_else(|| coordinates.content_bounds())
        });
        let mut candidate = self.clone();
        if let Some(resource) = retained_staging_resource {
            candidate.transition_native_staging_resource(
                resource,
                NativeStagingResourceOwner::NativeCreateFirstLive(binding),
                NativeStagingResourceOwner::SurfaceRecovery {
                    obligation: recovery_obligation,
                    binding,
                },
            )?;
        }
        candidate
            .registry
            .remove_destroyed(binding)
            .map_err(ViewportCoordinatorError::Registry)?;
        let request = if let Some(placement) = placement
            && candidate
                .capabilities
                .native_window_lifecycle()
                .is_supported()
            && candidate
                .capabilities
                .authoritative_inventory()
                .is_supported()
        {
            let replacement = candidate
                .registry
                .reserve(candidate.workspace_epoch, binding.surface(), role)
                .map_err(ViewportCoordinatorError::Registry)?;
            let effect = candidate
                .effects
                .request(PlatformEffect::RequestReplacement {
                    binding: replacement,
                    placement,
                    role,
                })
                .map_err(ViewportCoordinatorError::Effect)?;
            RecoveryPendingRequest::replacement(
                binding,
                role,
                recovery_obligation,
                retained_staging_resource,
                replacement,
                effect,
            )
        } else {
            RecoveryPendingRequest::awaiting_host(
                binding,
                role,
                recovery_obligation,
                retained_staging_resource,
            )
        };
        candidate
            .recovery_replacements
            .begin(request)
            .map_err(recovery_replacement_error)?;
        *self = candidate;
        Ok(())
    }

    pub(crate) fn complete_pending_recovery(
        &mut self,
        destroyed_surface: SurfaceId,
    ) -> Result<(), ViewportCoordinatorError> {
        let pending = self
            .recovery_replacements
            .pending(destroyed_surface)
            .cloned()
            .ok_or(ViewportCoordinatorError::MissingRecoveryPending {
                surface: destroyed_surface,
            })?;
        let mut candidate = self.clone();
        if let Some(resource) = pending.retained_staging_resource() {
            let owner = if matches!(
                pending.status(),
                RecoveryPendingStatus::AwaitingFirstLivePresentation
            ) {
                let replacement = pending.replacement_binding().ok_or(
                    ViewportCoordinatorError::PendingRecoveryRegistrationMismatch {
                        surface: destroyed_surface,
                    },
                )?;
                NativeStagingResourceOwner::RecoveryReplacementFirstLive {
                    obligation: pending.recovery_obligation(),
                    binding: replacement,
                }
            } else {
                NativeStagingResourceOwner::SurfaceRecovery {
                    obligation: pending.recovery_obligation(),
                    binding: pending.destroyed_binding(),
                }
            };
            candidate.release_native_staging_resource(resource, owner)?;
        }
        let Some(replacement) = pending.replacement_binding() else {
            let _ = candidate
                .recovery_replacements
                .complete_without_replacement(destroyed_surface)
                .map_err(recovery_replacement_error)?;
            *self = candidate;
            return Ok(());
        };
        let replacement_effect = pending.replacement_effect().ok_or(
            ViewportCoordinatorError::MissingReplacementEffect {
                surface: destroyed_surface,
            },
        )?;
        let effect = candidate
            .effects
            .request(PlatformEffect::CompensatingClose {
                binding: replacement,
                compensates: replacement_effect,
            })
            .map_err(ViewportCoordinatorError::Effect)?;
        candidate
            .registry
            .mark_awaiting_destroyed(replacement)
            .map_err(ViewportCoordinatorError::Registry)?;
        candidate
            .recovery_replacements
            .begin_compensation(destroyed_surface, effect)
            .map_err(recovery_replacement_error)?;
        *self = candidate;
        Ok(())
    }

    /// Admits a recovery replacement only after its exact first live docking
    /// output has been accepted by the engine.
    pub(crate) fn admit_recovery_replacement_first_live(
        &mut self,
        destroyed_binding: ViewportBinding,
        replacement_binding: ViewportBinding,
        recovery_obligation: SurfaceRecoveryObligationId,
    ) -> Result<(), ViewportCoordinatorError> {
        let surface = destroyed_binding.surface();
        if !self.replacement_is_admissible(replacement_binding)
            || !self.registry.record(surface).is_some_and(|record| {
                record.binding() == replacement_binding
                    && record.admission() == ViewportAdmission::Pending
            })
            || !self
                .recovery_replacements
                .pending(surface)
                .and_then(RecoveryPending::first_live_proof)
                .is_some_and(|proof| proof.binding() == replacement_binding)
        {
            return Err(ViewportCoordinatorError::PendingRecoveryRegistrationMismatch { surface });
        }

        let mut candidate = self.clone();
        let pending = candidate
            .recovery_replacements
            .complete_first_live(destroyed_binding, replacement_binding, recovery_obligation)
            .map_err(recovery_replacement_error)?;
        candidate
            .registry
            .admit(replacement_binding)
            .map_err(ViewportCoordinatorError::Registry)?;
        if let Some(resource) = pending.retained_staging_resource() {
            candidate.release_native_staging_resource(
                resource,
                NativeStagingResourceOwner::RecoveryReplacementFirstLive {
                    obligation: recovery_obligation,
                    binding: replacement_binding,
                },
            )?;
        }
        *self = candidate;
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn exhaust_capability_generation(&mut self) {
        self.capability_generation = CapabilityGeneration::new(u64::MAX);
    }

    #[cfg(test)]
    pub(crate) fn exhaust_work_area_generation(&mut self) {
        self.work_area_generation = WorkAreaGeneration::new(u64::MAX);
    }
}

fn binding_retirement_error(error: BindingRetirementLifecycleError) -> ViewportCoordinatorError {
    match error {
        BindingRetirementLifecycleError::AlreadyExists { binding } => {
            ViewportCoordinatorError::RetiredTokenReserved {
                token: binding.token(),
            }
        }
        BindingRetirementLifecycleError::TokenReserved { token } => {
            ViewportCoordinatorError::RetiredTokenReserved { token }
        }
        BindingRetirementLifecycleError::Missing { binding } => {
            ViewportCoordinatorError::MissingBindingRetirement { binding }
        }
        BindingRetirementLifecycleError::CleanupOwnershipMismatch { binding, ownership } => {
            ViewportCoordinatorError::BindingRetirementCleanupOwnershipMismatch {
                binding,
                ownership,
            }
        }
        BindingRetirementLifecycleError::CleanupEffectOwned { effect, .. } => {
            ViewportCoordinatorError::InvalidCleanupContinuation {
                effect,
                predecessor: effect,
            }
        }
        BindingRetirementLifecycleError::CleanupObservationMismatch {
            continuation,
            predecessor,
        } => ViewportCoordinatorError::InvalidCleanupContinuation {
            effect: continuation,
            predecessor,
        },
        BindingRetirementLifecycleError::CleanupWindowNotObserved { effect } => {
            ViewportCoordinatorError::CleanupWindowNotObserved { effect }
        }
        BindingRetirementLifecycleError::DestroyedTombstoneMissing { binding } => {
            ViewportCoordinatorError::DestroyedBindingGuardMissing { binding }
        }
        BindingRetirementLifecycleError::DestroyedTombstoneProviderMismatch {
            binding,
            expected,
            submitted,
        } => ViewportCoordinatorError::DestroyedBindingGuardProviderMismatch {
            binding,
            expected,
            submitted,
        },
    }
}

const fn recovery_replacement_error(
    error: RecoveryReplacementLifecycleError,
) -> ViewportCoordinatorError {
    match error {
        RecoveryReplacementLifecycleError::AlreadyPending { surface }
        | RecoveryReplacementLifecycleError::PendingIdentityMismatch { surface } => {
            ViewportCoordinatorError::PendingRecoveryObligationMismatch { surface }
        }
        RecoveryReplacementLifecycleError::DuplicateRetainedResource { resource } => {
            ViewportCoordinatorError::DuplicateNativeStagingResourceReference { resource }
        }
        RecoveryReplacementLifecycleError::MissingPending { surface } => {
            ViewportCoordinatorError::MissingRecoveryPending { surface }
        }
        RecoveryReplacementLifecycleError::ReplacementAlreadyOwned { binding, .. }
        | RecoveryReplacementLifecycleError::ReplacementMismatch { binding, .. }
        | RecoveryReplacementLifecycleError::StagingAbortAlreadyExists { binding }
        | RecoveryReplacementLifecycleError::MissingStagingAbort { binding }
        | RecoveryReplacementLifecycleError::StagingAbortOwnerMismatch { binding } => {
            ViewportCoordinatorError::PendingRecoveryRegistrationMismatch {
                surface: binding.surface(),
            }
        }
        RecoveryReplacementLifecycleError::ReplacementEffectMissing { surface } => {
            ViewportCoordinatorError::MissingReplacementEffect { surface }
        }
        RecoveryReplacementLifecycleError::CompensationEffectMissing { effect } => {
            ViewportCoordinatorError::MissingCleanupEffect { effect }
        }
        RecoveryReplacementLifecycleError::InvalidStatus { surface } => {
            ViewportCoordinatorError::PendingRecoveryRegistrationMismatch { surface }
        }
    }
}

const fn pointer_passthrough_error(
    error: PointerPassthroughLifecycleError,
) -> ViewportCoordinatorError {
    match error {
        PointerPassthroughLifecycleError::SagaMissing { binding } => {
            ViewportCoordinatorError::PointerPassthroughSagaMissing { binding }
        }
        PointerPassthroughLifecycleError::RestoreObligationMissing { binding } => {
            ViewportCoordinatorError::PointerInputRestoreObligationMissing { binding }
        }
    }
}

const fn is_destructive_cleanup(effect: &PlatformEffect) -> bool {
    effect.is_destructive_cleanup()
}

/// Fatal platform coordinator transition failure.
#[derive(Debug, Clone, PartialEq, Error)]
pub enum ViewportCoordinatorError {
    #[error(transparent)]
    PlatformProvider(PlatformObservationAuthorityError),
    #[error("no platform observation provider is active")]
    PlatformProviderUnavailable,
    #[error("platform capability generation is exhausted")]
    CapabilityGenerationExhausted,
    #[error("platform work-area generation is exhausted")]
    WorkAreaGenerationExhausted,
    /// A delayed complete platform batch cannot mutate any newer authority
    /// lane, including the transitional pointer roster.
    #[error("platform snapshot is stale: submitted {submitted:?}, current {current:?}")]
    StalePlatformSnapshot {
        /// Delayed provider batch generation.
        submitted: PlatformSnapshotGeneration,
        /// Last accepted provider batch generation.
        current: PlatformSnapshotGeneration,
    },
    /// A provider reused one complete-batch generation for different facts.
    #[error("platform snapshot generation {generation:?} conflicts with its accepted envelope")]
    ConflictingPlatformSnapshot {
        /// Reused provider batch generation.
        generation: PlatformSnapshotGeneration,
    },
    #[error(
        "platform inventory observation is stale: submitted {submitted:?}, current {current:?}"
    )]
    StaleInventoryObservation {
        submitted: InventoryObservationGeneration,
        current: InventoryObservationGeneration,
    },
    /// One provider reused an accepted inventory generation for a different
    /// complete roster. The whole platform snapshot is rejected before any
    /// binding-scoped fact can publish.
    #[error("platform inventory generation {generation:?} conflicts with its accepted envelope")]
    ConflictingInventoryObservation {
        /// Reused provider generation.
        generation: InventoryObservationGeneration,
    },
    /// A provider omitted one or more complete inventory envelopes. The whole
    /// snapshot is rejected so its binding-scoped facts cannot publish under a
    /// discontinuous roster.
    #[error("platform inventory generation skipped after {previous:?}: submitted {submitted:?}")]
    InventoryObservationGenerationGap {
        /// Last accepted provider generation.
        previous: InventoryObservationGeneration,
        /// Discontinuous submitted generation.
        submitted: InventoryObservationGeneration,
    },
    #[error(transparent)]
    Registry(ViewportRegistryError),
    #[error(transparent)]
    Effect(EffectLedgerError),
    #[error("drag source binding is stale or not ready: {binding:?}")]
    StaleDragSource { binding: ViewportBinding },
    #[error("pointer pass-through saga is missing for drag source {binding:?}")]
    PointerPassthroughSagaMissing { binding: ViewportBinding },
    #[error("pointer-input restore obligation is missing for {binding:?}")]
    PointerInputRestoreObligationMissing { binding: ViewportBinding },
    #[error("authoritative pointer-input observation is unavailable for {binding:?}")]
    PointerInputObservationMissing { binding: ViewportBinding },
    #[error("workspace does not contain logical surface {surface:?}")]
    MissingWorkspaceSurface { surface: SurfaceId },
    #[error("docking-owned child surface {surface:?} requires an explicit whole-root recovery")]
    ChildRecoveryRequired { surface: SurfaceId },
    #[error(
        "surface vacancy authority changed for {surface:?}: expected {expected:?}, got {actual:?}"
    )]
    SurfaceVacancyAuthorityChanged {
        surface: SurfaceId,
        expected: Option<ViewportBinding>,
        actual: Option<ViewportBinding>,
    },
    #[error("surface vacancy binding facts changed for {binding:?}")]
    SurfaceVacancyBindingFactsChanged {
        binding: ViewportBinding,
        expected_role: ViewportRole,
        actual_role: ViewportRole,
        expected_ownership: ViewportOwnership,
        actual_ownership: ViewportOwnership,
    },
    #[error("viewport coordinator epoch mismatch: expected {expected:?}, got {actual:?}")]
    WorkspaceEpochMismatch {
        expected: WorkspaceEpoch,
        actual: WorkspaceEpoch,
    },
    #[error("binding retirement is missing: {binding:?}")]
    MissingBindingRetirement { binding: ViewportBinding },
    #[error("retired viewport token remains reserved until exact retirement completion: {token:?}")]
    RetiredTokenReserved { token: WindowToken },
    #[error("retired cleanup ownership mismatch for {binding:?}: {ownership:?}")]
    BindingRetirementCleanupOwnershipMismatch {
        binding: ViewportBinding,
        ownership: ViewportOwnership,
    },
    #[error("destroyed viewport binding is still observed: {binding:?}")]
    DestroyedSurfaceStillObserved { binding: ViewportBinding },
    #[error("destroyed binding retention guard is missing for {binding:?}")]
    DestroyedBindingGuardMissing { binding: ViewportBinding },
    #[error(
        "destroyed binding guard {binding:?} belongs to provider {expected:?}, not {submitted:?}"
    )]
    DestroyedBindingGuardProviderMismatch {
        binding: ViewportBinding,
        expected: PlatformObservationLease,
        submitted: PlatformObservationLease,
    },
    #[error("native recovery is not pending for surface {surface:?}")]
    MissingRecoveryPending { surface: SurfaceId },
    #[error("staging binding {binding:?} did not retain pending admission: {admission:?}")]
    StagingBindingUnexpectedAdmission {
        binding: ViewportBinding,
        admission: ViewportAdmission,
    },
    #[error("surface {surface} pending recovery belongs to another binding or obligation")]
    PendingRecoveryObligationMismatch { surface: SurfaceId },
    #[error("registered replacement does not match pending recovery for surface {surface:?}")]
    PendingRecoveryRegistrationMismatch { surface: SurfaceId },
    #[error("native replacement effect is missing for surface {surface:?}")]
    MissingReplacementEffect { surface: SurfaceId },
    #[error("native create saga identity is exhausted")]
    NativeCreateSagaExhausted,
    #[error("native create capability is unavailable: {capability:?}")]
    NativeCapabilityUnavailable { capability: PlatformCapability },
    #[error("native placement proof is stale")]
    StalePlacementProof,
    #[error("native recovery root does not match the created root")]
    RecoveryRootMismatch,
    #[error("native create saga {saga:?} does not exist")]
    MissingCreateSaga { saga: NativeCreateSagaId },
    #[error("native create saga {saga:?} is not ready to commit")]
    CreateSagaNotReady { saga: NativeCreateSagaId },
    #[error("native create saga {saga:?} already transferred workspace ownership")]
    CreateSagaAlreadyTransferred { saga: NativeCreateSagaId },
    #[error("native create saga {saga:?} cannot mint a valid retained staging resource")]
    InvalidNativeStagingResource { saga: NativeCreateSagaId },
    #[error("native binding {binding:?} cannot mint a valid staging presentation")]
    InvalidNativeStagingPresentation { binding: ViewportBinding },
    #[error("retained native staging resource {resource:?} already exists")]
    DuplicateNativeStagingResource { resource: NativeStagingResourceId },
    #[error("retained native staging resource {resource:?} does not exist")]
    MissingNativeStagingResource { resource: NativeStagingResourceId },
    #[error("retained native staging resource {resource:?} has another lifecycle owner")]
    NativeStagingResourceOwnerMismatch { resource: NativeStagingResourceId },
    #[error("retained native staging resource {resource:?} has multiple durable references")]
    DuplicateNativeStagingResourceReference { resource: NativeStagingResourceId },
    #[error("retained native staging resource {resource:?} has no durable lifecycle reference")]
    UnreferencedNativeStagingResource { resource: NativeStagingResourceId },
    #[error("platform effect {effect:?} does not belong to a retryable cleanup")]
    MissingCleanupEffect { effect: EffectId },
    #[error(
        "cleanup continuation {effect:?} does not name a valid emitted predecessor {predecessor:?}"
    )]
    InvalidCleanupContinuation {
        effect: EffectId,
        predecessor: EffectId,
    },
    #[error("cleanup effect {effect:?} cannot be retried from phase {phase:?}")]
    CleanupEffectNotRetryable {
        effect: EffectId,
        phase: EffectPhase,
    },
    #[error("cleanup effect {effect:?} has no authoritatively observed window")]
    CleanupWindowNotObserved { effect: EffectId },
}

impl From<NativeStagingResourceLedgerError> for ViewportCoordinatorError {
    fn from(source: NativeStagingResourceLedgerError) -> Self {
        match source {
            NativeStagingResourceLedgerError::DuplicateResource { resource } => {
                Self::DuplicateNativeStagingResource { resource }
            }
            NativeStagingResourceLedgerError::MissingResource { resource } => {
                Self::MissingNativeStagingResource { resource }
            }
            NativeStagingResourceLedgerError::OwnerMismatch { resource, .. } => {
                Self::NativeStagingResourceOwnerMismatch { resource }
            }
        }
    }
}

impl From<NativeStagingResourceConservationError> for ViewportCoordinatorError {
    fn from(source: NativeStagingResourceConservationError) -> Self {
        match source {
            NativeStagingResourceConservationError::DuplicateReference { resource, .. } => {
                Self::DuplicateNativeStagingResourceReference { resource }
            }
            NativeStagingResourceConservationError::ReferencedResourceMissing {
                resource, ..
            } => Self::MissingNativeStagingResource { resource },
            NativeStagingResourceConservationError::ReferenceOwnerMismatch { resource, .. } => {
                Self::NativeStagingResourceOwnerMismatch { resource }
            }
            NativeStagingResourceConservationError::UnreferencedResource { resource, .. } => {
                Self::UnreferencedNativeStagingResource { resource }
            }
        }
    }
}

#[cfg(test)]
mod tests;
