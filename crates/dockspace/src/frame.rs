//! Atomic platform-fact coordinator used by the docking engine.

mod binding_cleanup;
mod native_bringup;
mod native_create;
mod native_staging_resource;
mod pointer_passthrough;
mod recovery_cleanup;
mod recovery_replacement;

use std::collections::{BTreeMap, BTreeSet};

use thiserror::Error;

use self::binding_cleanup::{
    BindingCleanupAction, BindingCleanupDirective, BindingCleanupError, BindingCleanupLifecycle,
    BindingCleanupPurpose, BindingCleanupRequest, BindingCleanupTerminal,
    RecoveryReplacementCleanupCompletion, RecoveryReplacementCleanupRequirement,
};
pub use self::binding_cleanup::{
    BindingRetirement, BindingRetirementOrigin, BindingRetirementStatus,
};
pub(crate) use self::native_bringup::{NativeReadyBasis, NativeReadyProof};
use self::native_create::NativeCreateState;
pub use self::native_create::{NativeCreatePhase, NativeCreateRequest, NativeCreateSaga};
pub(crate) use self::native_create::{NativeCreateProposal, PreparedNativeCreate};
use self::native_staging_resource::{
    NativeStagingResourceConservationError, NativeStagingResourceLedger,
    NativeStagingResourceLedgerError, NativeStagingResourceOwner,
};
use self::pointer_passthrough::{
    PointerPassthroughEffectRequest, PointerPassthroughEvidence, PointerPassthroughLifecycle,
    PointerPassthroughLifecycleError, PointerPassthroughRecovery,
};
use self::recovery_cleanup::AbandonedReplacementRetirementMode;
#[cfg(feature = "serde")]
pub(crate) use self::recovery_replacement::NativeSurfaceInitialRequest;
pub(crate) use self::recovery_replacement::{
    NativeSurfaceRestorePlan, QueuedInitialSurface, QueuedInitialSurfaceStatus,
};
pub use self::recovery_replacement::{RecoveryPending, RecoveryPendingStatus};
use self::recovery_replacement::{
    RecoveryPendingRequest, RecoveryReplacementLifecycle, RecoveryReplacementLifecycleError,
};

use crate::backend_ingress::BackendIngressDrainReceipt;
use crate::close_plan::{CloseRequestId, NativeCloseEdge};
use crate::coordinates::{
    CoordinateUnavailable, RecoveryCoordinateSnapshot, ViewportPlacementProof,
};
#[cfg(test)]
use crate::effect::EffectRequest;
use crate::effect::{
    EffectDispatchResult, EffectId, EffectInvalidation, EffectLedger, EffectLedgerError,
    EffectPhase, EffectRecord, EffectResult, EffectTransition, NativeCloseResolution,
    PlatformEffect, PlatformEffectEmission,
};
use crate::geometry::{LogicalRect, PhysicalRect};
pub use crate::ids::NativeCreateSagaId;
use crate::ids::{EngineAuthorityDomainId, ItemId, SurfaceId, WorkspaceEpoch};
use crate::intent::{NativePlacementProof, PointerId};
use crate::platform::{
    CapabilityRosterObservationStream, NativeDragSourceInput, ObservedWorkArea,
    PlatformCapabilities, PlatformCapability, PlatformSnapshot, WindowCloseObservation,
    WindowCloseState, WindowInputObservation, WindowInputState, WindowInventoryObservationStream,
    WindowPresentationObservation, WindowPresentationState, WorkAreaRosterObservationStream,
};
use crate::platform_provider::{
    PlatformObservationAuthority, PlatformObservationAuthorityError, PlatformObservationLease,
    PlatformProviderAuthorityFrontier, PlatformProviderReservation,
};
use crate::presentation_observation::NativeStagingResourceId;
use crate::retention::{BindingRetentionManifest, EffectRetentionManifest};
use crate::surface_recovery::SurfaceRecoveryObligationId;
#[cfg(test)]
use crate::viewport::InventoryGeneration;
use crate::viewport::{
    CapabilityGeneration, InputObservationGeneration, InventoryObservationGeneration,
    NativeSurfaceEstablishmentOrigin, PlatformSnapshotGeneration, ViewportBinding, ViewportRole,
    WindowToken, WorkAreaGeneration, WorkAreaObservationGeneration, WorkAreaToken,
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

/// Opaque preparation to retain one exact external root window on a restored surface.
///
/// Only the coordinator can prepare this value. Workspace reconciliation revalidates its
/// predecessor and target against current facts before publishing any changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PreparedNativeRootContinuation {
    predecessor: ViewportBinding,
    target: SurfaceId,
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
    PrepareNativeCreateFirstLive {
        saga: NativeCreateSagaId,
        prepared: Box<native_create::PreparedNativeCreate>,
        proof: NativeReadyProof,
    },
    SurfaceDestroyed {
        observation: WindowCloseObservation,
        source_coordinates: Option<RecoveryCoordinateSnapshot>,
    },
    /// An existing logical surface acquired current native renderer readiness.
    /// Its establishment owner awaits the exact semantic first-live output.
    NativeSurfaceReady {
        origin: NativeSurfaceEstablishmentOrigin,
        binding: ViewportBinding,
        recovery_obligation: SurfaceRecoveryObligationId,
    },
    /// A pending replacement lost readiness or disappeared. Revoke its old
    /// first-live barrier before a later preparation can establish a new one.
    NativeSurfaceLost {
        origin: NativeSurfaceEstablishmentOrigin,
        binding: ViewportBinding,
        recovery_obligation: SurfaceRecoveryObligationId,
    },
    RetryRecovery {
        source_binding: ViewportBinding,
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
    consumed_pre_admission_close_requests: Vec<WindowCloseObservation>,
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
    pub(crate) fn pre_admission_close_was_consumed(
        &self,
        requested: WindowCloseObservation,
    ) -> bool {
        self.consumed_pre_admission_close_requests
            .contains(&requested)
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
    binding_cleanup: BindingCleanupLifecycle,
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
    pub(crate) fn capture(surface: SurfaceId, record: Option<ViewportRecord<'_>>) -> Self {
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
}

impl SurfaceVacancySettlement {
    pub(crate) fn logical_vacated_bindings(&self) -> &[ViewportBinding] {
        &self.logical_vacated_bindings
    }

    pub(crate) fn effects(&self) -> &[EffectId] {
        &self.effects
    }
}

#[derive(Debug, Clone, Copy)]
struct RestoreStagingBinding {
    creation_effect: Option<EffectId>,
    cleanup: Option<BindingCleanupAction>,
    origin: BindingRetirementOrigin,
    retained_resource: Option<NativeStagingResourceId>,
    resource_owner: Option<NativeStagingResourceOwner>,
}

struct RestoreAnalysis {
    staging_by_binding: BTreeMap<ViewportBinding, RestoreStagingBinding>,
    cleanup_by_binding: BTreeMap<ViewportBinding, EffectId>,
    retained_bindings: BTreeMap<ViewportBinding, SurfaceId>,
    passthrough_recoveries: BTreeMap<ViewportBinding, PointerPassthroughRecovery>,
}

#[derive(Default)]
struct RestoreAccumulation {
    retired: Vec<ViewportBinding>,
    cleanup_effects: Vec<EffectId>,
    unbound_surfaces: Vec<SurfaceId>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BindingCleanupRestoreAction {
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
            binding_cleanup: BindingCleanupLifecycle::default(),
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
                let RecoveryPendingStatus::AwaitingReplacementCleanup { replacement } =
                    pending.status()
                else {
                    return None;
                };
                Some((
                    pending.source_binding().surface(),
                    pending.replacement_binding()?,
                    replacement,
                ))
            })
            .collect::<Vec<_>>();
        for (surface, binding, replacement) in compensations {
            self.retire_recovery_cleanup_for_provider_replacement(
                surface,
                binding,
                replacement,
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
    ) -> Option<PlatformProviderReservation> {
        self.platform_provider.pending_replacement()
    }

    /// Revokes one provider and freezes a typed handoff ticket for its successor.
    pub(crate) fn begin_platform_provider_replacement(
        &mut self,
        provider: PlatformObservationLease,
    ) -> Result<PlatformProviderReservation, ViewportCoordinatorError> {
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
                        PlatformEffect::SetPointerPassthrough { .. }
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
        candidate.binding_cleanup.reset_for_provider_replacement();
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
        ticket: PlatformProviderReservation,
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
        ticket: PlatformProviderReservation,
    ) -> Result<(), ViewportCoordinatorError> {
        let mut candidate = self.clone();
        candidate
            .platform_provider
            .abort_replacement(ticket)
            .map_err(ViewportCoordinatorError::PlatformProvider)?;
        *self = candidate;
        Ok(())
    }

    /// Compacts terminal-binding guards after the exact observation producer has quiesced.
    pub(crate) fn compact_quiesced_terminal_binding_guards(
        &mut self,
        provider: PlatformObservationLease,
    ) -> usize {
        self.binding_cleanup
            .compact_terminal_binding_guards_from(provider)
    }

    /// Compacts one exact binding guard after its joined producer lane proves quiescence.
    pub(crate) fn compact_quiesced_terminal_binding_guard(
        &mut self,
        binding: ViewportBinding,
        provider: PlatformObservationLease,
    ) -> Result<(), ViewportCoordinatorError> {
        self.binding_cleanup
            .compact_terminal_binding_guard(binding, provider)
            .map_err(binding_cleanup_error)
    }

    /// Input terminal facts may name a current or still-retained exact incarnation.
    pub(crate) fn recognizes_input_binding(&self, binding: ViewportBinding) -> bool {
        self.registry
            .record(binding.surface())
            .is_some_and(|record| record.binding() == binding)
            || self.binding_cleanup.contains_key(&binding)
            || self.binding_cleanup.is_terminal(binding)
    }

    pub(crate) fn retains_window_cleanup(&self, binding: ViewportBinding) -> bool {
        self.binding_cleanup.contains_key(&binding)
    }

    pub(crate) fn recognizes_retired_native_binding(&self, binding: ViewportBinding) -> bool {
        self.registry
            .record(binding.surface())
            .is_some_and(|record| {
                record.binding() == binding && record.admission() == ViewportAdmission::Retiring
            })
            || self.binding_cleanup.contains_key(&binding)
            || self.binding_cleanup.is_terminal(binding)
    }

    #[cfg(test)]
    pub(crate) fn record_terminal_binding_guard_for_test(
        &mut self,
        binding: ViewportBinding,
        provider: PlatformObservationLease,
    ) {
        self.binding_cleanup
            .record_terminal_binding_guard(binding, provider);
    }

    /// Releases the exact revoked effect-provider guard proven quiescent by backend drain.
    pub(crate) fn compact_quiesced_backend_effect_provider(
        &mut self,
        receipt: &BackendIngressDrainReceipt,
    ) -> bool {
        self.effects.compact_quiesced_backend_provider(receipt)
    }

    /// Installs a candidate-local allowance for new native binding lifetimes.
    pub(crate) fn set_native_binding_allocation_budget(&mut self, remaining: Option<usize>) {
        self.registry
            .set_native_binding_allocation_budget(remaining);
    }

    pub(crate) const fn native_binding_allocation_available(&self) -> bool {
        self.registry.native_binding_allocation_available()
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
        if self.binding_cleanup.token_is_reserved(token) {
            return Err(ViewportCoordinatorError::RetiredTokenReserved { token });
        }
        if role == ViewportRole::Child && recovery_obligation.is_none() {
            return Err(ViewportCoordinatorError::ChildRecoveryRequired { surface });
        }
        if self.recovery_replacements.contains_surface(surface) {
            return Err(ViewportCoordinatorError::PendingRecoveryRegistrationMismatch { surface });
        }
        let binding = self
            .registry
            .register_existing(epoch, surface, token, role)
            .map_err(ViewportCoordinatorError::Registry)?;
        self.workspace_epoch = epoch;
        Ok(binding)
    }

    /// Registers an existing native child whose destruction is owned by dockspace.
    pub(crate) fn register_owned_child(
        &mut self,
        epoch: WorkspaceEpoch,
        surface: SurfaceId,
        token: WindowToken,
    ) -> Result<ViewportBinding, ViewportCoordinatorError> {
        if self.registry.records().next().is_some() && epoch != self.workspace_epoch {
            return Err(ViewportCoordinatorError::WorkspaceEpochMismatch {
                expected: self.workspace_epoch,
                actual: epoch,
            });
        }
        if self.binding_cleanup.token_is_reserved(token) {
            return Err(ViewportCoordinatorError::RetiredTokenReserved { token });
        }
        if self.recovery_replacements.contains_surface(surface) {
            return Err(ViewportCoordinatorError::PendingRecoveryRegistrationMismatch { surface });
        }
        let binding = self
            .registry
            .register_owned_child(epoch, surface, token)
            .map_err(ViewportCoordinatorError::Registry)?;
        self.workspace_epoch = epoch;
        Ok(binding)
    }

    /// Prepares retention of an exact admitted external root on a replacement surface.
    ///
    /// The source surface may be absent from the replacement workspace. The target must
    /// belong to that workspace and cannot already belong to another native window.
    pub(crate) fn prepare_native_root_continuation(
        &self,
        predecessor: ViewportBinding,
        target: SurfaceId,
        desired_surfaces: &BTreeSet<SurfaceId>,
    ) -> Result<PreparedNativeRootContinuation, ViewportCoordinatorError> {
        let continuation = PreparedNativeRootContinuation {
            predecessor,
            target,
        };
        self.analyze_restore(desired_surfaces, std::slice::from_ref(&continuation))?;
        Ok(continuation)
    }

    /// Prepares a complete set of external Root continuations in one transaction.
    ///
    /// Participating windows may exchange logical surfaces. Duplicate sources,
    /// duplicate targets and targets occupied by a nonparticipating window fail
    /// before any registry or cleanup state changes.
    pub(crate) fn prepare_native_root_continuations(
        &self,
        mappings: &[(ViewportBinding, SurfaceId)],
        desired_surfaces: &BTreeSet<SurfaceId>,
    ) -> Result<Vec<PreparedNativeRootContinuation>, ViewportCoordinatorError> {
        let continuations = mappings
            .iter()
            .map(|&(predecessor, target)| PreparedNativeRootContinuation {
                predecessor,
                target,
            })
            .collect::<Vec<_>>();
        self.analyze_restore(desired_surfaces, &continuations)?;
        Ok(continuations)
    }

    /// Atomically invalidates old-epoch platform state and reconciles safe current bindings.
    ///
    /// An explicit continuation overrides only its predecessor's default same-surface target.
    /// Its current eligibility is revalidated before any candidate state can publish.
    pub(crate) fn reconcile_workspace_epoch(
        &mut self,
        new_epoch: WorkspaceEpoch,
        desired_surfaces: &BTreeSet<SurfaceId>,
        continuations: &[PreparedNativeRootContinuation],
    ) -> Result<ViewportReconciliation, ViewportCoordinatorError> {
        let mut candidate = self.clone();
        let analysis = candidate.analyze_restore(desired_surfaces, continuations)?;
        let registry = candidate
            .registry
            .reconcile_restore(new_epoch, &analysis.retained_bindings)
            .map_err(ViewportCoordinatorError::Registry)?;
        let rebound_map: BTreeMap<ViewportBinding, ViewportBinding> =
            registry.retained().iter().copied().collect();
        // A retained window's old incarnation still owns its producer lane until
        // exact ingress quiescence, even though no physical destruction occurs.
        if let Some(provider) = candidate.platform_provider() {
            for &(predecessor, _) in registry.retained() {
                candidate
                    .binding_cleanup
                    .record_terminal_binding_guard(predecessor, provider);
            }
        }

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
        candidate.finish_restore(desired_surfaces, &mut accumulated.unbound_surfaces);

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
        &self,
        desired_surfaces: &BTreeSet<SurfaceId>,
        continuations: &[PreparedNativeRootContinuation],
    ) -> Result<RestoreAnalysis, ViewportCoordinatorError> {
        let create_by_binding = self.native_create_snapshot_by_binding();
        let mut staging_by_binding: BTreeMap<_, _> = create_by_binding
            .iter()
            .map(|(binding, saga)| {
                (
                    *binding,
                    RestoreStagingBinding {
                        creation_effect: Some(saga.create),
                        cleanup: Some(BindingCleanupAction::CompensateCreate {
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
                let cleanup = pending
                    .replacement_effect()
                    .map(|create| BindingCleanupAction::CompensateCreate { create });
                staging_by_binding.insert(
                    binding,
                    RestoreStagingBinding {
                        creation_effect: pending.replacement_effect(),
                        cleanup,
                        origin: BindingRetirementOrigin::WorkspaceReplaced,
                        retained_resource: None,
                        resource_owner: None,
                    },
                );
                if let Some(cleanup) = self.binding_cleanup.recovery_cleanup(binding) {
                    if cleanup.recovery_cleanup_owner()
                        != Some((pending.source_binding(), pending.recovery_obligation()))
                    {
                        return Err(
                            ViewportCoordinatorError::PendingRecoveryRegistrationMismatch {
                                surface: pending.source_binding().surface(),
                            },
                        );
                    }
                    if let Some(effect) = cleanup.status().cleanup_effect() {
                        let replacement = pending.replacement_effect().ok_or(
                            ViewportCoordinatorError::MissingReplacementEffect {
                                surface: pending.source_binding().surface(),
                            },
                        )?;
                        let _ = self.validated_compensating_close(effect, binding, replacement)?;
                        cleanup_by_binding.insert(binding, effect);
                    }
                }
            }
        }
        let replacement_bindings: BTreeSet<_> = self
            .recovery_replacements
            .values()
            .filter_map(RecoveryPending::replacement_binding)
            .collect();
        let mut targets = BTreeSet::new();
        let mut continuing = BTreeMap::new();
        for &PreparedNativeRootContinuation {
            predecessor,
            target,
        } in continuations
        {
            if continuing.insert(predecessor, target).is_some() {
                return Err(
                    ViewportCoordinatorError::NativeRootContinuationDuplicatePredecessor {
                        predecessor,
                    },
                );
            }
            if !targets.insert(target) {
                return Err(
                    ViewportCoordinatorError::NativeRootContinuationDuplicateTarget { target },
                );
            }
        }
        for (&predecessor, &target) in &continuing {
            if !desired_surfaces.contains(&target) {
                return Err(
                    ViewportCoordinatorError::NativeRootContinuationTargetMissing { target },
                );
            }
            let record = self
                .registry
                .record(predecessor.surface())
                .filter(|record| record.binding() == predecessor)
                .ok_or(
                    ViewportCoordinatorError::NativeRootContinuationPredecessorUnavailable {
                        predecessor,
                    },
                )?;
            if record.role() != ViewportRole::Root
                || record.ownership() != ViewportOwnership::External
                || self.binding_cleanup.contains_key(&predecessor)
                || self.binding_cleanup.is_terminal(predecessor)
                || cleanup_by_binding.contains_key(&predecessor)
                || !Self::restore_binding_is_safe(
                    target,
                    record,
                    desired_surfaces,
                    &create_by_binding,
                    &replacement_bindings,
                )
            {
                return Err(
                    ViewportCoordinatorError::NativeRootContinuationPredecessorUnavailable {
                        predecessor,
                    },
                );
            }
            if let Some(occupied) = self.registry.record(target)
                && occupied.binding() != predecessor
                && !continuing.contains_key(&occupied.binding())
            {
                return Err(
                    ViewportCoordinatorError::NativeRootContinuationTargetOccupied {
                        target,
                        binding: occupied.binding(),
                    },
                );
            }
        }
        let retained_bindings = self
            .registry
            .records()
            .filter_map(|(surface, record)| {
                let target = continuing
                    .get(&record.binding())
                    .copied()
                    .unwrap_or(surface);
                Self::restore_binding_is_safe(
                    target,
                    record,
                    desired_surfaces,
                    &create_by_binding,
                    &replacement_bindings,
                )
                .then_some((record.binding(), target))
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
        target: SurfaceId,
        record: ViewportRecord<'_>,
        desired_surfaces: &BTreeSet<SurfaceId>,
        creates: &BTreeMap<ViewportBinding, NativeCreateSaga>,
        replacements: &BTreeSet<ViewportBinding>,
    ) -> bool {
        let binding = record.binding();
        // A surface identity alone cannot prove that a replacement workspace preserved the
        // child's exact root, recovery host, floating identities, ownership, and geometry
        // contract. Until restore publishes that semantic manifest, child bindings fail closed
        // and require a new explicit registration carrying a freshly validated recovery plan.
        desired_surfaces.contains(&target)
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
            .binding_cleanup
            .retired_iter()
            .map(|(_, retirement)| self.retirement_cleanup_restore_action(retirement))
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
            .binding_cleanup
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
        let retirements = self.binding_cleanup.keys().copied().collect::<Vec<_>>();
        for binding in retirements {
            let _ = self.drive_binding_cleanup(binding)?;
        }
        self.provider_reconciliation_pending = false;
        Ok(())
    }

    fn apply_retirement_cleanup_actions(
        &mut self,
        issuance_epoch: WorkspaceEpoch,
        actions: Vec<BindingCleanupRestoreAction>,
        cleanup_effects: &mut Vec<EffectId>,
    ) -> Result<(), ViewportCoordinatorError> {
        for action in actions {
            match action {
                BindingCleanupRestoreAction::Redispatch { binding } => {
                    let request = self
                        .binding_cleanup
                        .plan_cleanup_redispatch(binding)
                        .map_err(binding_cleanup_error)?;
                    if let Some(request) = request {
                        let effect = self.request_retirement_cleanup(request)?;
                        self.binding_cleanup
                            .accept_cleanup_effect(binding, effect)
                            .map_err(binding_cleanup_error)?;
                        cleanup_effects.push(effect);
                    }
                }
                BindingCleanupRestoreAction::ContinueObservation {
                    binding,
                    predecessor,
                    after,
                } => {
                    let superseded = self
                        .binding_cleanup
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
                    self.binding_cleanup
                        .accept_cleanup_observation_effect(binding, successor, predecessor)
                        .map_err(binding_cleanup_error)?;
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
    ) -> Result<Option<BindingCleanupRestoreAction>, ViewportCoordinatorError> {
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
                .then_some(BindingCleanupRestoreAction::ContinueObservation {
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
                    BindingCleanupRestoreAction::ContinueObservation {
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
    ) -> Result<Option<BindingCleanupRestoreAction>, ViewportCoordinatorError> {
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
                Ok(Some(BindingCleanupRestoreAction::ContinueObservation {
                    binding: retirement.binding(),
                    predecessor: *predecessor,
                    after: self.cleanup_observation_lane_predecessor(effect)?,
                }))
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
                    return Ok(Some(BindingCleanupRestoreAction::ContinueObservation {
                        binding: retirement.binding(),
                        predecessor: effect,
                        after: None,
                    }));
                }
                Ok((!record.was_emitted()
                    && matches!(record.phase(), EffectPhase::Invalidated { .. }))
                .then_some(BindingCleanupRestoreAction::Redispatch {
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
            if !self.binding_cleanup.contains_key(binding) {
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
            let recovery_cleanup_exists = self.binding_cleanup.recovery_cleanup(binding).is_some();
            accumulated.retired.push(binding);
            let staging = analysis.staging_by_binding.get(&binding).copied();
            let cleanup =
                staging
                    .and_then(|staging| staging.cleanup)
                    .unwrap_or_else(|| match facts.ownership() {
                        ViewportOwnership::External => BindingCleanupAction::KeepExternal,
                        ViewportOwnership::RuntimeOwned => BindingCleanupAction::ReleaseChild,
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
                if let Some(create) = staging.and_then(|staging| staging.creation_effect) {
                    self.retain_uncreated_binding_guard(binding, create);
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
                    NativeStagingResourceOwner::BindingCleanup(binding),
                )?;
            }
            let request = BindingCleanupRequest {
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
                purpose: BindingCleanupPurpose::Retired,
            };
            if recovery_cleanup_exists {
                self.binding_cleanup
                    .promote_recovery_cleanup_to_retirement(request)
                    .map_err(binding_cleanup_error)?;
            } else {
                self.begin_binding_cleanup(request)?;
            }
            if let Some(old_saga) = analysis.passthrough_recoveries.get(&binding) {
                self.pointer_passthrough
                    .install_retired_recovery(binding, old_saga);
                if let Some(effect) = self.reconcile_pointer_passthrough_saga(binding)? {
                    accumulated.cleanup_effects.push(effect);
                }
            }
            if existing_effect.is_some() {
                let retirement = self
                    .binding_cleanup
                    .get(&binding)
                    .ok_or(ViewportCoordinatorError::MissingBindingCleanup { binding })?;
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
            } else if let Some(effect) = self.drive_binding_cleanup(binding)? {
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

    fn begin_binding_cleanup(
        &mut self,
        request: BindingCleanupRequest,
    ) -> Result<(), ViewportCoordinatorError> {
        self.binding_cleanup
            .begin(request)
            .map_err(binding_cleanup_error)
    }

    fn finish_binding_cleanup(
        &mut self,
        terminal: BindingCleanupTerminal,
    ) -> Result<(), ViewportCoordinatorError> {
        if let Some(resource) = terminal.retained_staging_resource() {
            self.release_native_staging_resource(
                resource,
                NativeStagingResourceOwner::BindingCleanup(terminal.binding()),
            )?;
        }
        Ok(())
    }

    fn drive_binding_cleanup(
        &mut self,
        binding: ViewportBinding,
    ) -> Result<Option<EffectId>, ViewportCoordinatorError> {
        if !self.binding_cleanup.contains_key(&binding) {
            return Ok(None);
        }
        let pointer_restore_pending = self.pointer_passthrough.contains_binding(binding);
        if !pointer_restore_pending
            && self
                .binding_cleanup
                .get(&binding)
                .is_some_and(BindingRetirement::is_retired)
            && let Some(record) = self.registry.record(binding.surface())
            && record.binding() == binding
        {
            if record.admission() != ViewportAdmission::Retiring {
                return Err(
                    ViewportCoordinatorError::RetiringBindingUnexpectedAdmission {
                        binding,
                        admission: record.admission(),
                    },
                );
            }
            let _ = self
                .registry
                .detach(binding)
                .map_err(ViewportCoordinatorError::Registry)?;
        }
        let directive = self
            .binding_cleanup
            .plan_drive(binding, pointer_restore_pending)
            .map_err(binding_cleanup_error)?;
        directive.map_or(Ok(None), |directive| {
            self.apply_binding_cleanup_directive(directive)
        })
    }

    fn apply_binding_cleanup_directive(
        &mut self,
        directive: BindingCleanupDirective,
    ) -> Result<Option<EffectId>, ViewportCoordinatorError> {
        match directive {
            BindingCleanupDirective::Drive { binding } => self.drive_binding_cleanup(binding),
            BindingCleanupDirective::RequestCleanup(request) => {
                let binding = request.binding();
                let effect = self.request_retirement_cleanup(request)?;
                self.binding_cleanup
                    .accept_cleanup_effect(binding, effect)
                    .map_err(binding_cleanup_error)?;
                Ok(Some(effect))
            }
            BindingCleanupDirective::Finish(terminal) => {
                if let Some(create) = terminal.uncreated_effect() {
                    self.retain_uncreated_binding_guard(terminal.binding(), create);
                }
                self.finish_binding_cleanup(terminal)?;
                Ok(None)
            }
            BindingCleanupDirective::ExactDestroyed(terminal) => {
                self.mark_binding_effects_destroyed(terminal.binding());
                self.pointer_passthrough
                    .terminate_binding(terminal.binding());
                self.finish_binding_cleanup(terminal)?;
                Ok(None)
            }
        }
    }

    fn finish_restore(
        &mut self,
        desired_surfaces: &BTreeSet<SurfaceId>,
        unbound_surfaces: &mut Vec<SurfaceId>,
    ) {
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
                    consumed_pre_admission_close_requests: Vec::new(),
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
            .binding_cleanup
            .retired_iter()
            .map(|(binding, _)| *binding)
            .collect::<BTreeSet<_>>();
        let registry = candidate
            .registry
            .apply_snapshot_with_retired_bindings(
                snapshot,
                inventory,
                &retired_bindings,
                candidate.binding_cleanup.terminal_bindings(),
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
                .binding_cleanup
                .record_terminal_binding_guard(binding, provider);
            candidate.terminate_pointer_passthrough_binding(binding);
        }
        candidate.reconcile_binding_cleanups(provider, snapshot)?;
        candidate.observe_pointer_passthrough_snapshot(provider);
        candidate.reconcile_pointer_passthrough_sagas()?;
        let retirement_bindings = candidate
            .binding_cleanup
            .keys()
            .copied()
            .collect::<Vec<_>>();
        for binding in retirement_bindings {
            let _ = candidate.drive_binding_cleanup(binding)?;
        }
        let registry_events = registry.events().to_vec();
        // A pre-admission replacement close belongs to the coordinator, not the
        // graph-level close/recovery reducer. Establish that ownership before
        // deriving lifecycle actions so this same snapshot cannot also enqueue
        // `RetryRecovery` and create a second compensating close.
        let mut consumed_pre_admission_close_requests = Vec::new();
        for requested in registry_events.iter().filter_map(|event| match event {
            RegistryEvent::CloseRequested { observation } => Some(*observation),
            RegistryEvent::Ready { .. }
            | RegistryEvent::PresentationChanged { .. }
            | RegistryEvent::FactsUnavailable { .. }
            | RegistryEvent::BindingMissing { .. }
            | RegistryEvent::CloseRequestCleared { .. }
            | RegistryEvent::Destroyed { .. } => None,
        }) {
            if candidate.consume_pre_admission_close(requested)? {
                consumed_pre_admission_close_requests.push(requested);
            }
        }
        let actions = candidate.reduce_registry_events(&registry_events, &recovery_coordinates)?;
        let transition = ViewportFrameTransition {
            capability_generation,
            work_area_generation: candidate.work_area_generation,
            capabilities_changed,
            work_areas_changed,
            registry_events,
            consumed_pre_admission_close_requests,
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
        let mut consumed_pre_admission_close_requests = Vec::new();
        for requested in registry_events.iter().filter_map(|event| match event {
            RegistryEvent::CloseRequested { observation } => Some(*observation),
            RegistryEvent::Ready { .. }
            | RegistryEvent::PresentationChanged { .. }
            | RegistryEvent::FactsUnavailable { .. }
            | RegistryEvent::BindingMissing { .. }
            | RegistryEvent::CloseRequestCleared { .. }
            | RegistryEvent::Destroyed { .. } => None,
        }) {
            if candidate.consume_pre_admission_close(requested)? {
                consumed_pre_admission_close_requests.push(requested);
            }
        }
        let actions = candidate.reduce_registry_events(&registry_events, &recovery_coordinates)?;
        let transition = ViewportFrameTransition {
            capability_generation: candidate.capability_generation,
            work_area_generation: candidate.work_area_generation,
            capabilities_changed: false,
            work_areas_changed: false,
            registry_events,
            consumed_pre_admission_close_requests,
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
            || self.binding_cleanup.contains_key(&binding)
    }

    /// Returns whether a retained recovery is exclusively owned by a pending
    /// pre-admission cleanup rather than by normal recovery retry.
    ///
    /// The binding is intentionally matched together with the exact source
    /// binding and obligation. This makes an old abort unable to suppress a
    /// later recovery incarnation on the same logical surface.
    pub(crate) fn recovery_retry_is_suppressed(
        &self,
        source_binding: ViewportBinding,
        recovery_obligation: SurfaceRecoveryObligationId,
    ) -> bool {
        let Some(pending) = self
            .recovery_replacements
            .pending(source_binding.surface())
            .filter(|pending| {
                pending.source_binding() == source_binding
                    && pending.recovery_obligation() == recovery_obligation
            })
        else {
            return false;
        };
        let Some(replacement) = pending.replacement_binding() else {
            return false;
        };
        self.binding_cleanup
            .recovery_cleanup(replacement)
            .and_then(BindingRetirement::recovery_cleanup_owner)
            == Some((source_binding, recovery_obligation))
    }

    /// Consumes one native close request which targets a pre-admission binding.
    ///
    /// The caller must invoke this before exposing a close edge to the application. A staging
    /// window owns no workspace topology, so it is aborted or compensated internally instead of
    /// entering the ordinary `ClosePlan` protocol.
    fn consume_pre_admission_close(
        &mut self,
        requested: WindowCloseObservation,
    ) -> Result<bool, ViewportCoordinatorError> {
        if requested.known_state() != Some(WindowCloseState::LiveRequested) {
            return Ok(false);
        }
        let binding = requested.binding();
        if self
            .binding_cleanup
            .reduce_recovery_cleanup_close_request(requested)
            .was_consumed()
        {
            return Ok(true);
        }
        if let Some(saga) = self.abortable_native_create_for_binding(binding) {
            let _ = self.abort_native_create(saga)?;
            return Ok(true);
        }

        if self.recovery_replacements.contains_replacement(binding) {
            let mut candidate = self.clone();
            candidate.begin_pre_admission_recovery_cleanup(requested)?;
            *self = candidate;
            return Ok(true);
        }

        Ok(false)
    }

    fn begin_pre_admission_recovery_cleanup(
        &mut self,
        requested: WindowCloseObservation,
    ) -> Result<(), ViewportCoordinatorError> {
        let binding = requested.binding();
        if self.binding_cleanup.recovery_cleanup(binding).is_some() {
            return Ok(());
        }
        let surface = self
            .recovery_replacements
            .replacement_surface(binding)
            .ok_or(ViewportCoordinatorError::MissingRecoveryPending {
                surface: binding.surface(),
            })?;
        let pending = self
            .recovery_replacements
            .pending(surface)
            .cloned()
            .ok_or(ViewportCoordinatorError::MissingRecoveryPending { surface })?;
        let replacement = pending
            .replacement_effect()
            .ok_or(ViewportCoordinatorError::MissingReplacementEffect { surface })?;
        let (admission, role, ownership, input_observations, close_observations) =
            match self.registry.record(binding.surface()) {
                Some(record) if record.binding() == binding => (
                    record.admission(),
                    record.role(),
                    record.ownership(),
                    record.input_observations(),
                    record.close_observations(),
                ),
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
        if admission != ViewportAdmission::Pending {
            return Err(
                ViewportCoordinatorError::StagingBindingUnexpectedAdmission { binding, admission },
            );
        }
        let cleanup = self
            .effects
            .request(PlatformEffect::CompensatingClose {
                binding,
                compensates: replacement,
            })
            .map_err(ViewportCoordinatorError::Effect)?;
        self.registry
            .mark_awaiting_destroyed(binding)
            .map_err(ViewportCoordinatorError::Registry)?;
        self.pointer_passthrough.terminate_binding(binding);
        self.begin_binding_cleanup(BindingCleanupRequest {
            binding,
            role,
            ownership,
            origin: BindingRetirementOrigin::RecoveryReplacementCleanup { replacement },
            status: BindingRetirementStatus::CleanupRequested { effect: cleanup },
            observed: true,
            input_observations,
            close_observations,
            may_reappear: true,
            cleanup: BindingCleanupAction::CompensateCreate {
                create: replacement,
            },
            retained_staging_resource: None,
            purpose: BindingCleanupPurpose::RecoveryReplacement {
                destroyed_binding: pending.source_binding(),
                recovery_obligation: pending.recovery_obligation(),
                requirement: RecoveryReplacementCleanupRequirement::ConditionalClose {
                    requested,
                    clear: None,
                },
            },
        })?;
        Ok(())
    }

    fn reduce_pre_admission_close_clear(
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
            .binding_cleanup
            .record_recovery_cleanup_clear(requested, observation)
        {
            return Ok(());
        }
        self.try_resume_pre_admission_cleanup(binding)
    }

    fn reduce_pre_admission_cleanup_result(
        &mut self,
        effect: EffectId,
    ) -> Result<(), ViewportCoordinatorError> {
        let bindings = self
            .binding_cleanup
            .recovery_cleanup_bindings_for_effect(effect);
        for binding in bindings {
            self.try_resume_pre_admission_cleanup(binding)?;
        }
        Ok(())
    }

    fn try_resume_pre_admission_cleanup(
        &mut self,
        binding: ViewportBinding,
    ) -> Result<(), ViewportCoordinatorError> {
        let Some((cleanup, clear)) = self.binding_cleanup.recovery_cleanup_resume(binding) else {
            return Ok(());
        };
        self.invalidate_unemitted_pre_admission_cleanup(cleanup);
        if !self.pre_admission_cleanup_is_proven_safe(cleanup, clear) {
            return Ok(());
        }
        if !self
            .registry
            .resume_pending_after_pre_admission_close_clear(binding)
            .map_err(ViewportCoordinatorError::Registry)?
        {
            return Ok(());
        }
        self.recovery_replacements
            .mark_admission_fence(binding, clear.inventory_generation())
            .map_err(recovery_replacement_error)?;
        let _ = self
            .binding_cleanup
            .finish_recovery_cleanup_resume(binding)
            .map_err(binding_cleanup_error)?;
        Ok(())
    }

    /// Drops a private abort cleanup before restoring its replacement to pending admission.
    ///
    /// The cleanup is queued while reducing `LiveRequested`, but platform effects are extracted
    /// only after the entire host frame settles. An exact `LiveClear` in that interval revokes
    /// the reason for the close; leaving the request pending would let it close a replacement
    /// after the coordinator has already restored it.
    fn invalidate_unemitted_pre_admission_cleanup(&mut self, cleanup: EffectId) {
        let should_invalidate = self.effects.record(cleanup).is_some_and(|record| {
            !record.was_emitted() && matches!(record.phase(), EffectPhase::Requested)
        });
        if should_invalidate {
            let _ = self
                .effects
                .invalidate_unemitted(cleanup, EffectInvalidation::PreAdmissionCloseCleared);
        }
    }

    fn pre_admission_cleanup_is_proven_safe(
        &self,
        cleanup: EffectId,
        clear: WindowCloseObservation,
    ) -> bool {
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

    fn pre_admission_cleanup_allows_replacement_admission(
        &self,
        binding: ViewportBinding,
        presentation: WindowPresentationObservation,
    ) -> bool {
        self.recovery_replacements
            .admission_allows(binding, presentation.inventory_generation())
    }

    fn reconcile_binding_cleanups(
        &mut self,
        provider: PlatformObservationLease,
        snapshot: &PlatformSnapshot,
    ) -> Result<(), ViewportCoordinatorError> {
        let directives = self.binding_cleanup.observe_snapshot(provider, snapshot);
        for directive in directives {
            let _ = self.apply_binding_cleanup_directive(directive)?;
        }
        Ok(())
    }

    fn request_retirement_cleanup(
        &mut self,
        effect: PlatformEffect,
    ) -> Result<EffectId, ViewportCoordinatorError> {
        self.effects
            .request_in(self.workspace_epoch, effect)
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
                        actions.extend(self.reduce_ready_binding(binding)?);
                    }
                }
                RegistryEvent::PresentationChanged { binding } => {
                    if presentation_reduced.insert(binding) {
                        actions.extend(self.reduce_ready_binding(binding)?);
                    }
                }
                RegistryEvent::CloseRequested { observation } => {
                    if let Some(surface) =
                        self.recovery_surface_owned_by_pre_admission_cleanup(observation.binding())
                    {
                        suppress_recovery_retry.insert(surface);
                    }
                }
                RegistryEvent::CloseRequestCleared {
                    requested,
                    observation,
                } => {
                    self.reduce_pre_admission_close_clear(requested, observation)?;
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
            .map(|(_, record)| record.binding())
            .collect();
        for binding in ready_bindings {
            if presentation_reduced.insert(binding) {
                actions.extend(self.reduce_ready_binding(binding)?);
            }
        }
        let mut queued = BTreeSet::new();
        for pending in self.recovery_replacements.values() {
            if pending.permits_recovery_retry()
                && !suppress_recovery_retry.contains(&pending.source_binding().surface())
                && !self.recovery_retry_is_suppressed(
                    pending.source_binding(),
                    pending.recovery_obligation(),
                )
                && queued.insert(pending.source_binding().surface())
            {
                actions.push(ViewportLifecycleAction::RetryRecovery {
                    source_binding: pending.source_binding(),
                    recovery_obligation: pending.recovery_obligation(),
                });
            }
        }
        Ok(actions)
    }

    fn recovery_surface_owned_by_pre_admission_cleanup(
        &self,
        binding: ViewportBinding,
    ) -> Option<SurfaceId> {
        self.binding_cleanup
            .recovery_cleanup(binding)
            .and_then(BindingRetirement::recovery_cleanup_owner)
            .map(|(source_binding, _)| source_binding.surface())
    }

    fn reduce_ready_binding(
        &mut self,
        binding: ViewportBinding,
    ) -> Result<Vec<ViewportLifecycleAction>, ViewportCoordinatorError> {
        let provider = self.active_platform_provider()?;
        let mut actions = Vec::new();
        actions.extend(self.reduce_ready_native_surface(provider, binding)?);
        actions.extend(self.reduce_ready_native_create(provider, binding)?);
        Ok(actions)
    }

    fn reduce_ready_native_surface(
        &mut self,
        provider: PlatformObservationLease,
        binding: ViewportBinding,
    ) -> Result<Vec<ViewportLifecycleAction>, ViewportCoordinatorError> {
        let mut actions = Vec::new();
        if let Some(surface) = self.recovery_replacements.replacement_surface(binding) {
            let proof = self
                .recovery_replacements
                .pending(surface)
                .and_then(|pending| pending.first_live_proof());
            if let Some(proof) = proof {
                if self.native_ready_proof_is_current(proof) {
                    return Ok(actions);
                }
                let pending = self
                    .recovery_replacements
                    .pending(surface)
                    .ok_or(ViewportCoordinatorError::MissingRecoveryPending { surface })?;
                actions.push(ViewportLifecycleAction::NativeSurfaceLost {
                    origin: pending.origin(),
                    binding,
                    recovery_obligation: pending.recovery_obligation(),
                });
                self.recovery_replacements
                    .resume_preparation(binding, proof)
                    .map_err(recovery_replacement_error)?;
            }
        }
        let Some((surface, phase)) = self.recovery_replacements.bringup(binding) else {
            return Ok(actions);
        };
        let pending = self
            .recovery_replacements
            .pending(surface)
            .ok_or(ViewportCoordinatorError::MissingRecoveryPending { surface })?;
        let origin = pending.origin();
        let recovery_obligation = pending.recovery_obligation();
        let owner = pending.staging_owner();
        let failed = self
            .registry
            .record(binding.surface())
            .and_then(ViewportRecord::create_preparation)
            .is_some_and(|preparation| {
                preparation.known_readiness()
                    == Some(crate::platform::NativeCreateReadiness::Failed)
                    && self
                        .effect_emission_fence(phase.create_effect(), binding)
                        .is_some_and(|emitted| preparation.inventory_generation() > emitted)
            });
        if failed {
            self.recovery_replacements
                .fail_preparation(binding, phase)
                .map_err(recovery_replacement_error)?;
            self.retire_abandoned_recovery_replacement_in_place(
                surface,
                binding,
                AbandonedReplacementRetirementMode::DriveImmediately,
            )?;
            return Ok(actions);
        }
        match self.reduce_ready_native_preparation(provider, binding, owner, phase)? {
            native_bringup::NativePreparationOutcome::Ignored => Ok(actions),
            native_bringup::NativePreparationOutcome::Ready(proof) => {
                if !self.replacement_is_admissible(binding) {
                    return Ok(actions);
                }
                self.recovery_replacements
                    .mark_awaiting_first_live(binding, phase, proof)
                    .map_err(recovery_replacement_error)?;
                actions.push(ViewportLifecycleAction::NativeSurfaceReady {
                    origin,
                    binding,
                    recovery_obligation,
                });
                Ok(actions)
            }
        }
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
                                && self.pre_admission_cleanup_allows_replacement_admission(
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
        self.binding_cleanup
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
        if self.binding_cleanup.recovery_cleanup(binding).is_some() {
            let provider = self.active_platform_provider()?;
            let terminal = self
                .binding_cleanup
                .finish_destroyed(binding, provider)
                .ok_or(ViewportCoordinatorError::MissingBindingCleanup { binding })?;
            return self.reduce_recovery_cleanup_destroyed(binding, terminal, actions);
        }

        if let Some(disposition) = self.reduce_replacement_destroyed(binding, actions)? {
            return Ok(disposition);
        }
        if !self.settle_destroyed_native_create(binding)? {
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

    fn reduce_recovery_cleanup_destroyed(
        &mut self,
        binding: ViewportBinding,
        terminal: BindingCleanupTerminal,
        actions: &mut Vec<ViewportLifecycleAction>,
    ) -> Result<DestroyedBindingDisposition, ViewportCoordinatorError> {
        let completion = terminal.recovery_cleanup_completion().ok_or(
            ViewportCoordinatorError::PendingRecoveryRegistrationMismatch {
                surface: binding.surface(),
            },
        )?;
        let (destroyed_binding, recovery_obligation, complete_recovery) = match completion {
            RecoveryReplacementCleanupCompletion::ResetLostReplacement {
                destroyed_binding,
                recovery_obligation,
            } => (destroyed_binding, recovery_obligation, false),
            RecoveryReplacementCleanupCompletion::CompleteRecovery {
                destroyed_binding,
                recovery_obligation,
            } => (destroyed_binding, recovery_obligation, true),
        };
        let surface = destroyed_binding.surface();
        let pending = self
            .recovery_replacements
            .pending(surface)
            .filter(|pending| {
                pending.source_binding() == destroyed_binding
                    && pending.recovery_obligation() == recovery_obligation
                    && pending.replacement_binding() == Some(binding)
            })
            .cloned()
            .ok_or(ViewportCoordinatorError::PendingRecoveryRegistrationMismatch { surface })?;

        if let Some(effect) = pending.replacement_effect() {
            let _ =
                self.effects
                    .mark_destroyed(effect, binding, self.registry.inventory_generation());
        }
        if complete_recovery {
            self.registry
                .remove_destroyed(binding)
                .map_err(ViewportCoordinatorError::Registry)?;
            let _ = self
                .recovery_replacements
                .take_for_vacancy(surface, binding)
                .map_err(recovery_replacement_error)?;
            return Ok(DestroyedBindingDisposition::Handled {
                suppress_recovery_retry: None,
            });
        }

        let was_awaiting_first_live = matches!(
            pending.status(),
            RecoveryPendingStatus::AwaitingFirstLivePresentation
        );
        self.registry
            .remove_destroyed(binding)
            .map_err(ViewportCoordinatorError::Registry)?;
        let pending = self
            .recovery_replacements
            .reset_lost_replacement(surface, binding)
            .map_err(recovery_replacement_error)?;
        if was_awaiting_first_live {
            actions.push(ViewportLifecycleAction::NativeSurfaceLost {
                origin: pending.origin(),
                binding,
                recovery_obligation: pending.recovery_obligation(),
            });
        }
        Ok(DestroyedBindingDisposition::Handled {
            suppress_recovery_retry: Some(surface),
        })
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
        if matches!(
            pending.status(),
            RecoveryPendingStatus::AwaitingReplacementCleanup { .. }
        ) {
            return Err(ViewportCoordinatorError::MissingBindingCleanup { binding });
        }
        if let Some(effect) = pending.replacement_effect() {
            let _ =
                self.effects
                    .mark_destroyed(effect, binding, self.registry.inventory_generation());
        }
        let origin = pending.origin();
        let recovery_obligation = pending.recovery_obligation();
        let was_awaiting_first_live = matches!(
            pending.status(),
            RecoveryPendingStatus::AwaitingFirstLivePresentation
        );
        self.registry
            .remove_destroyed(binding)
            .map_err(ViewportCoordinatorError::Registry)?;
        let _ = self
            .recovery_replacements
            .reset_lost_replacement(surface, binding)
            .map_err(recovery_replacement_error)?;
        if was_awaiting_first_live {
            actions.push(ViewportLifecycleAction::NativeSurfaceLost {
                origin,
                binding,
                recovery_obligation,
            });
        }
        Ok(Some(DestroyedBindingDisposition::Handled {
            suppress_recovery_retry: Some(surface),
        }))
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
            .binding_cleanup
            .plan_destructive_retry(failed_effect)
            .map_err(binding_cleanup_error)?
        else {
            return Ok(None);
        };
        let binding = request.binding();
        let retry = self.request_retirement_cleanup(request)?;
        self.binding_cleanup
            .accept_cleanup_effect(binding, retry)
            .map_err(binding_cleanup_error)?;
        Ok(Some(retry))
    }

    fn retry_retirement_cleanup_observation(
        &mut self,
        failed_effect: EffectId,
    ) -> Result<Option<EffectId>, ViewportCoordinatorError> {
        let Some(retirement_binding) = self
            .binding_cleanup
            .observation_retry_binding(failed_effect)
        else {
            return Ok(None);
        };
        let retirement = self.binding_cleanup.get(&retirement_binding).ok_or(
            ViewportCoordinatorError::MissingBindingCleanup {
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
        self.binding_cleanup
            .accept_cleanup_observation_effect(retirement_binding, retry, predecessor)
            .map_err(binding_cleanup_error)?;
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

    /// Only a definitive result of the original unobserved create can end an
    /// appearance wait after a workspace reset. Its immutable request epoch and
    /// delivered provider are still validated by the effect ledger.
    pub(crate) fn accepts_retired_create_failure(&self, result: EffectResult) -> bool {
        if result.is_cleanup_observation()
            || !matches!(
                result.result(),
                EffectDispatchResult::DispatchFailed(_) | EffectDispatchResult::Unsupported(_)
            )
        {
            return false;
        }
        let Some(binding) = self
            .binding_cleanup
            .unobserved_create_failure_target(result.effect())
        else {
            return false;
        };
        self.effects.record(result.effect()).is_some_and(|record| {
            matches!(record.request().effect(),
                PlatformEffect::CreateWindow { binding: target, .. }
                    | PlatformEffect::RequestReplacement { binding: target, .. }
                    if *target == binding)
        })
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
                .binding_cleanup
                .reduce_cleanup_observation(
                    observation.continuation(),
                    observation.predecessor(),
                    predecessor_phase,
                )
                .map_err(binding_cleanup_error)?;
            if reduced != Some(observation.binding()) {
                return Err(ViewportCoordinatorError::InvalidCleanupContinuation {
                    effect: observation.continuation(),
                    predecessor: observation.predecessor(),
                });
            }
            let retirement_bindings = candidate
                .binding_cleanup
                .keys()
                .copied()
                .collect::<Vec<_>>();
            for binding in retirement_bindings {
                let _ = candidate.drive_binding_cleanup(binding)?;
            }
            candidate.reduce_pre_admission_cleanup_result(observation.continuation())?;
            *self = candidate;
            return Ok(transition);
        }
        let transition =
            if result.epoch() != current_epoch && self.accepts_retired_create_failure(result) {
                self.effects.report_exact(provider, result)
            } else {
                self.effects.report(provider, current_epoch, result)
            };
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
        let retirement_bindings = self.binding_cleanup.keys().copied().collect::<Vec<_>>();
        for binding in retirement_bindings {
            let _ = self.drive_binding_cleanup(binding)?;
        }
        self.reduce_pending_recovery_result(effect, result.result())?;
        self.reduce_pre_admission_cleanup_result(effect)?;
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
        self.binding_cleanup
            .reduce_effect(effect, retirement_phase)
            .map_err(binding_cleanup_error)
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
        if replacement_was_discarded && let Some(binding) = replacement {
            self.retain_uncreated_binding_guard(binding, effect);
        }
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

    pub(crate) fn reconcile_unemitted_native_close_resolutions(
        &mut self,
        current: impl FnMut(CloseRequestId, NativeCloseEdge, NativeCloseResolution) -> bool,
    ) {
        self.effects
            .reconcile_unemitted_native_close_resolutions(current);
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
        self.try_take_new_effects_with_limit(None)
    }

    pub(crate) fn try_take_new_effects_with_limit(
        &mut self,
        limit: Option<usize>,
    ) -> Result<Vec<PlatformEffectEmission>, ViewportCoordinatorError> {
        let Some(provider) = self.platform_provider.active() else {
            return Ok(Vec::new());
        };
        let inventory_generation = self.registry.inventory_generation();
        let registry = &self.registry;
        self.effects
            .take_new_requests_with_limit(
                provider,
                inventory_generation,
                |edge| {
                    matches!(
                        registry.native_close_edge_disposition(edge),
                        NativeCloseEdgeDisposition::ExactPending
                    )
                },
                limit,
            )
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
        self.binding_cleanup.is_terminal(binding) || self.binding_cleanup.contains_key(&binding)
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
                if candidate.vacancy_binding_has_terminal_owner(expected.binding) {
                    // The predecessor already owns its exact terminal cleanup.
                    // A replacement at the same logical surface has independent
                    // vacancy authority and must remain untouched by this record.
                    settlement.logical_vacated_bindings.push(expected.binding);
                    continue;
                }
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
            let binding = actual.binding();
            let role = actual.role();
            let ownership = actual.ownership();
            let lifecycle = actual.lifecycle();
            let input_observations = actual.input_observations();
            let close_observations = actual.close_observations();
            let ever_observed = actual.ever_observed();
            if candidate
                .recovery_replacements
                .pending(surface)
                .is_some_and(|pending| pending.replacement_binding() == Some(expected.binding))
            {
                let _ = candidate
                    .recovery_replacements
                    .take_for_vacancy(surface, expected.binding)
                    .map_err(recovery_replacement_error)?;
            }
            let observed = ever_observed
                && !matches!(
                    lifecycle,
                    ViewportLifecycle::Missing | ViewportLifecycle::Destroyed
                );
            let may_reappear = ever_observed && !matches!(lifecycle, ViewportLifecycle::Destroyed);
            let cleanup = match ownership {
                ViewportOwnership::External => BindingCleanupAction::KeepExternal,
                ViewportOwnership::RuntimeOwned => BindingCleanupAction::ReleaseChild,
            };
            // Inventory absence cannot establish that an external window was destroyed.
            // Keep its token owned by this exact binding until a Destroyed observation
            // arrives, even if it was never observed in an inventory snapshot.
            let external_requires_exact_destruction = ownership == ViewportOwnership::External
                && !matches!(lifecycle, ViewportLifecycle::Destroyed);
            let pointer_restore_pending = candidate.pointer_passthrough.contains_binding(binding);
            let needs_retirement = external_requires_exact_destruction
                || ownership == ViewportOwnership::RuntimeOwned
                || observed
                || may_reappear
                || pointer_restore_pending;
            if pointer_restore_pending {
                candidate
                    .registry
                    .mark_awaiting_destroyed(binding)
                    .map_err(ViewportCoordinatorError::Registry)?;
            } else {
                let _ = candidate
                    .registry
                    .detach(binding)
                    .map_err(ViewportCoordinatorError::Registry)?;
            }
            if needs_retirement {
                candidate.begin_binding_cleanup(BindingCleanupRequest {
                    binding,
                    role,
                    ownership,
                    origin: BindingRetirementOrigin::SurfaceVacated,
                    status: BindingRetirementStatus::AwaitingInputRestore,
                    observed,
                    input_observations,
                    close_observations,
                    may_reappear,
                    cleanup,
                    retained_staging_resource: None,
                    purpose: BindingCleanupPurpose::Retired,
                })?;
                let _ = candidate.prepare_pointer_passthrough_for_vacancy(binding)?;
                if let Some(effect) = candidate.drive_binding_cleanup(binding)? {
                    settlement.effects.push(effect);
                }
            }
        }
        *self = candidate;
        Ok(settlement)
    }

    #[must_use]
    pub const fn capabilities(&self) -> &PlatformCapabilities {
        &self.capabilities
    }

    pub(crate) fn authoritative_inventory_contains(&self, binding: ViewportBinding) -> bool {
        self.inventory_observations
            .current()
            .and_then(|observation| observation.known_roster())
            .is_some_and(|bindings| bindings.binary_search(&binding).is_ok())
    }

    #[must_use]
    pub fn native_exact_placement_create_capability(&self) -> PlatformCapability {
        self.capabilities.native_exact_placement_create()
    }

    #[must_use]
    pub fn physical_cross_surface_drag_capability(
        &self,
        source_input: NativeDragSourceInput,
    ) -> PlatformCapability {
        self.capabilities.physical_cross_surface_drag(source_input)
    }

    #[must_use]
    pub fn physical_native_drag_capability(
        &self,
        source_input: NativeDragSourceInput,
    ) -> PlatformCapability {
        self.capabilities.physical_native_drag(source_input)
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
        self.binding_cleanup.extend_referenced_effects(effects);
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
        if let Some(provider) = self.platform_provider() {
            let registry = &self.registry;
            self.effects
                .mark_creation_bindings_published(provider, |binding| {
                    registry
                        .record(binding.surface())
                        .is_some_and(|record| record.binding() == binding)
                });
        }
    }

    pub(crate) fn binding_retention_manifest(&self) -> BindingRetentionManifest {
        self.binding_cleanup
            .retention_manifest()
            .with_surface_authority_generations(self.registry.surface_authority_generation_count())
    }

    pub(crate) fn compact_surface_coordinate_authority(
        &mut self,
        workspace_surfaces: impl IntoIterator<Item = SurfaceId>,
    ) -> usize {
        let mut retained = workspace_surfaces.into_iter().collect::<BTreeSet<_>>();
        retained.extend(self.registry.records().map(|(surface, _)| surface));
        retained.extend(self.binding_cleanup.keys().map(|binding| binding.surface()));
        retained.extend(
            self.binding_cleanup
                .terminal_bindings()
                .iter()
                .map(|binding| binding.surface()),
        );
        retained.extend(self.recovery_replacements.surfaces());
        self.registry
            .compact_surface_authority_generations(&retained)
    }

    pub fn binding_retirements(
        &self,
    ) -> impl Iterator<Item = (ViewportBinding, &BindingRetirement)> {
        self.binding_cleanup
            .retired_iter()
            .map(|(binding, retirement)| (*binding, retirement))
    }

    #[must_use]
    pub fn recovery_pending(&self, surface: SurfaceId) -> Option<&RecoveryPending> {
        self.recovery_replacements.pending(surface)
    }

    pub(crate) fn has_surface_establishment(&self, surface: SurfaceId) -> bool {
        self.recovery_replacements.contains_surface(surface)
    }

    pub(crate) fn queued_native_surface(
        &self,
        surface: SurfaceId,
    ) -> Option<&QueuedInitialSurface> {
        self.recovery_replacements.queued_initial(surface)
    }

    pub(crate) fn waiting_native_surfaces(&self) -> impl Iterator<Item = &QueuedInitialSurface> {
        self.recovery_replacements.waiting_initial()
    }

    pub(crate) fn queued_native_surface_can_advance(&self, initial: &QueuedInitialSurface) -> bool {
        self.registry
            .record(initial.request.recovery_host)
            .is_some_and(|root| {
                root.role() == ViewportRole::Root
                    && root.ownership() == ViewportOwnership::External
                    && root.admission() == ViewportAdmission::Admitted
                    && root.has_coordinate_authority()
            })
            && !matches!(
                self.native_exact_placement_create_capability(),
                PlatformCapability::Unknown(_)
            )
            && initial.dependencies.iter().all(|binding| {
                !self.binding_cleanup.contains_key(binding)
                    && !self.binding_cleanup.is_terminal(*binding)
            })
            && (self.native_binding_allocation_available()
                || matches!(
                    self.native_exact_placement_create_capability(),
                    PlatformCapability::Unsupported(_)
                ))
    }

    pub(crate) fn enqueue_native_surface_restore(
        &mut self,
        plan: &NativeSurfaceRestorePlan,
    ) -> Result<(), ViewportCoordinatorError> {
        let dependencies = std::sync::Arc::new(
            self.binding_cleanup
                .keys()
                .copied()
                .chain(self.binding_cleanup.terminal_bindings().iter().copied())
                .collect(),
        );
        for &request in &plan.children {
            self.recovery_replacements
                .enqueue_initial(request, std::sync::Arc::clone(&dependencies))
                .map_err(recovery_replacement_error)?;
        }
        Ok(())
    }

    pub(crate) fn stop_queued_native_surface(
        &mut self,
        surface: SurfaceId,
        status: QueuedInitialSurfaceStatus,
    ) -> Result<bool, ViewportCoordinatorError> {
        self.recovery_replacements
            .stop_initial(surface, status)
            .map_err(recovery_replacement_error)
    }

    pub fn pending_recoveries(&self) -> impl Iterator<Item = (SurfaceId, &RecoveryPending)> {
        self.recovery_replacements
            .iter()
            .map(|(surface, pending)| (*surface, pending))
    }

    #[must_use]
    pub fn viewport(&self, surface: SurfaceId) -> Option<ViewportRecord<'_>> {
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
                    && self
                        .capabilities
                        .physical_native_drag(proof.source_input())
                        .is_supported()
            }
        }
    }

    pub(crate) fn begin_drag_routing(
        &mut self,
        pointer: PointerId,
        surface: SurfaceId,
        input: NativeDragSourceInput,
    ) -> Result<Option<EffectId>, ViewportCoordinatorError> {
        let Some(record) = self.registry.record(surface) else {
            return Ok(None);
        };
        let binding = record.binding();
        let mut candidate = self.clone();
        let Some(change) = candidate
            .pointer_passthrough
            .begin_routing(pointer, binding, input)
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

    #[must_use]
    pub(crate) fn confirmed_drag_source(&self, pointer: PointerId) -> Option<ViewportBinding> {
        let binding = self.pointer_passthrough.drag_source(pointer)?;
        let evidence = self.pointer_passthrough_evidence(binding);
        if self.pointer_passthrough.drag_source_input(pointer)
            == Some(NativeDragSourceInput::Preserve)
        {
            return (!self.pointer_passthrough.contains_binding(binding)
                && self
                    .registry
                    .record(binding.surface())
                    .is_some_and(|record| record.binding() == binding && record.is_routeable())
                && evidence.authoritative_state() == Some(WindowInputState::ReceivesInput))
            .then_some(binding);
        }
        if evidence.authoritative_state() != Some(WindowInputState::PassThrough) {
            return None;
        }
        let attempts = self.pointer_passthrough.effect_attempts(binding);
        if attempts.restore().is_some() {
            return None;
        }
        attempts
            .enable()
            .is_some_and(|effect| {
                self.effects.record(effect).is_some_and(|record| {
                    matches!(record.phase(), EffectPhase::ObservedApplied { .. })
                })
            })
            .then_some(binding)
    }

    #[must_use]
    pub(crate) fn drag_route_effect(&self, pointer: PointerId) -> Option<EffectId> {
        if self.pointer_passthrough.drag_source_input(pointer)
            != Some(NativeDragSourceInput::PassThrough)
        {
            return None;
        }
        let binding = self.pointer_passthrough.drag_source(pointer)?;
        self.pointer_passthrough.effect_attempts(binding).enable()
    }

    pub(crate) fn drag_source_input(&self, pointer: PointerId) -> Option<NativeDragSourceInput> {
        self.pointer_passthrough.drag_source_input(pointer)
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

    /// Establishes a native child for a logical surface already present in the workspace.
    /// A detached terminal initial attempt may retry under its existing recovery obligation.
    pub(crate) fn start_native_surface(
        &mut self,
        surface: SurfaceId,
        placement: PhysicalRect,
        recovery_obligation: SurfaceRecoveryObligationId,
    ) -> Result<ViewportBinding, ViewportCoordinatorError> {
        let mut candidate = self.clone();
        let capability = candidate.native_exact_placement_create_capability();
        if !capability.is_supported() {
            return Err(ViewportCoordinatorError::NativeCapabilityUnavailable { capability });
        }
        let retry_initial = if let Some(pending) = candidate.recovery_replacements.pending(surface)
        {
            if pending.recovery_obligation() != recovery_obligation {
                return Err(
                    ViewportCoordinatorError::PendingRecoveryObligationMismatch { surface },
                );
            }
            if !pending.permits_initial_retry() {
                return Err(
                    ViewportCoordinatorError::PendingRecoveryRegistrationMismatch { surface },
                );
            }
            true
        } else {
            false
        };
        let binding = candidate
            .registry
            .reserve(candidate.workspace_epoch, surface, ViewportRole::Child)
            .map_err(ViewportCoordinatorError::Registry)?;
        let create = candidate
            .effects
            .request(PlatformEffect::CreateWindow {
                binding,
                placement,
                role: ViewportRole::Child,
            })
            .map_err(ViewportCoordinatorError::Effect)?;
        if retry_initial {
            candidate
                .recovery_replacements
                .retry_initial(binding, recovery_obligation, create)
                .map_err(recovery_replacement_error)?;
        } else {
            candidate
                .recovery_replacements
                .begin_initial(RecoveryPendingRequest::initial(
                    binding,
                    ViewportRole::Child,
                    recovery_obligation,
                    create,
                ))
                .map_err(recovery_replacement_error)?;
        }
        *self = candidate;
        Ok(binding)
    }

    /// Cancels only the exact active initial establishment, retaining its logical recovery.
    pub(crate) fn cancel_initial_native_surface(
        &mut self,
        binding: ViewportBinding,
    ) -> Result<bool, ViewportCoordinatorError> {
        let Some(pending) = self
            .recovery_replacements
            .pending(binding.surface())
            .filter(|pending| {
                pending.origin() == (NativeSurfaceEstablishmentOrigin::Initial { binding })
                    && pending.replacement_binding() == Some(binding)
                    && !matches!(
                        pending.status(),
                        RecoveryPendingStatus::ReplacementCancelled { .. }
                    )
            })
        else {
            return Ok(false);
        };
        let create = pending.replacement_effect().ok_or(
            ViewportCoordinatorError::MissingReplacementEffect {
                surface: binding.surface(),
            },
        )?;
        let mut candidate = self.clone();
        let create_may_have_executed = candidate.effect_may_have_executed(create);
        let effects = candidate
            .effects
            .records()
            .filter_map(|(effect, _)| (effect == create).then_some(effect))
            .collect::<Vec<_>>();
        for effect in effects {
            let _ = candidate
                .effects
                .invalidate_unemitted(effect, EffectInvalidation::NativeSurfaceCancelled);
        }
        let binding_was_discarded = !create_may_have_executed
            && candidate
                .binding_cleanup
                .recovery_cleanup(binding)
                .is_none()
            && candidate.registry.discard_unobserved(binding).is_ok();
        let surface = candidate
            .recovery_replacements
            .mark_cancelled(binding, binding_was_discarded)
            .map_err(recovery_replacement_error)?;
        if binding_was_discarded {
            candidate.terminate_pointer_passthrough_binding(binding);
            candidate.retain_uncreated_binding_guard(binding, create);
        } else {
            let _ = candidate.retire_abandoned_recovery_replacement_in_place(
                surface,
                binding,
                AbandonedReplacementRetirementMode::DriveImmediately,
            )?;
        }
        *self = candidate;
        Ok(true)
    }

    pub(crate) fn defer_destroyed_surface_recovery(
        &mut self,
        binding: ViewportBinding,
        recovery_obligation: SurfaceRecoveryObligationId,
    ) -> Result<(), ViewportCoordinatorError> {
        if let Some(pending) = self.recovery_replacements.pending(binding.surface()) {
            if pending.origin() == (NativeSurfaceEstablishmentOrigin::Destroyed { binding })
                && pending.recovery_obligation() == recovery_obligation
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
        // Native replacement placement is an outer-window fact. Content bounds
        // are not an interchangeable fallback because system decorations make
        // the two rectangles observably different. Without an exact retained
        // outer anchor, recovery must remain host-owned and fail closed.
        let placement = record
            .coordinates()
            .and_then(|coordinates| coordinates.outer_bounds());
        let mut candidate = self.clone();
        candidate
            .registry
            .remove_destroyed(binding)
            .map_err(ViewportCoordinatorError::Registry)?;
        let request = if let Some(placement) = placement
            && candidate.registry.native_binding_allocation_available()
            && candidate
                .capabilities
                .native_exact_placement_create()
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
                replacement,
                effect,
            )
        } else {
            RecoveryPendingRequest::awaiting_host(binding, role, recovery_obligation)
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
        let owner = (pending.source_binding(), pending.recovery_obligation());
        if let Some(cleanup) = candidate.binding_cleanup.recovery_cleanup(replacement) {
            if cleanup.recovery_cleanup_owner() != Some(owner) {
                return Err(
                    ViewportCoordinatorError::PendingRecoveryRegistrationMismatch {
                        surface: destroyed_surface,
                    },
                );
            }
            let _ = candidate
                .binding_cleanup
                .promote_recovery_cleanup_to_mandatory(replacement)
                .map_err(binding_cleanup_error)?;
        } else {
            let (role, ownership, observed, input_observations, close_observations) = {
                let record = candidate
                    .registry
                    .record(replacement.surface())
                    .filter(|record| record.binding() == replacement)
                    .ok_or(ViewportCoordinatorError::Registry(
                        ViewportRegistryError::StaleBinding {
                            binding: replacement,
                        },
                    ))?;
                (
                    record.role(),
                    record.ownership(),
                    record.ever_observed(),
                    record.input_observations(),
                    record.close_observations(),
                )
            };
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
            candidate.begin_binding_cleanup(BindingCleanupRequest {
                binding: replacement,
                role,
                ownership,
                origin: BindingRetirementOrigin::RecoveryReplacementCleanup {
                    replacement: replacement_effect,
                },
                status: BindingRetirementStatus::CleanupRequested { effect },
                observed,
                input_observations,
                close_observations,
                may_reappear: true,
                cleanup: BindingCleanupAction::CompensateCreate {
                    create: replacement_effect,
                },
                retained_staging_resource: None,
                purpose: BindingCleanupPurpose::RecoveryReplacement {
                    destroyed_binding: pending.source_binding(),
                    recovery_obligation: pending.recovery_obligation(),
                    requirement: RecoveryReplacementCleanupRequirement::Mandatory,
                },
            })?;
        }
        candidate
            .recovery_replacements
            .begin_cleanup(destroyed_surface)
            .map_err(recovery_replacement_error)?;
        *self = candidate;
        Ok(())
    }

    /// Admits an established native surface only after its exact first live docking
    /// output has been accepted by the engine.
    pub(crate) fn admit_native_surface_first_live(
        &mut self,
        origin: NativeSurfaceEstablishmentOrigin,
        binding: ViewportBinding,
        recovery_obligation: SurfaceRecoveryObligationId,
    ) -> Result<(), ViewportCoordinatorError> {
        let surface = origin.binding().surface();
        if !self.replacement_is_admissible(binding)
            || !self.registry.record(surface).is_some_and(|record| {
                record.binding() == binding && record.admission() == ViewportAdmission::Pending
            })
            || !self
                .recovery_replacements
                .pending(surface)
                .and_then(RecoveryPending::first_live_proof)
                .is_some_and(|proof| proof.binding() == binding)
        {
            return Err(ViewportCoordinatorError::PendingRecoveryRegistrationMismatch { surface });
        }

        let mut candidate = self.clone();
        let _ = candidate
            .recovery_replacements
            .complete_first_live(origin, binding, recovery_obligation)
            .map_err(recovery_replacement_error)?;
        candidate
            .registry
            .admit(binding)
            .map_err(ViewportCoordinatorError::Registry)?;
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

fn binding_cleanup_error(error: BindingCleanupError) -> ViewportCoordinatorError {
    match error {
        BindingCleanupError::AlreadyExists { binding } => {
            ViewportCoordinatorError::RetiredTokenReserved {
                token: binding.token(),
            }
        }
        BindingCleanupError::TokenReserved { token } => {
            ViewportCoordinatorError::RetiredTokenReserved { token }
        }
        BindingCleanupError::Missing { binding } => {
            ViewportCoordinatorError::MissingBindingCleanup { binding }
        }
        BindingCleanupError::CleanupOwnershipMismatch { binding, ownership } => {
            ViewportCoordinatorError::BindingCleanupOwnershipMismatch { binding, ownership }
        }
        BindingCleanupError::CleanupPurposeMismatch { binding } => {
            ViewportCoordinatorError::PendingRecoveryRegistrationMismatch {
                surface: binding.surface(),
            }
        }
        BindingCleanupError::CleanupEffectOwned { effect, .. } => {
            ViewportCoordinatorError::InvalidCleanupContinuation {
                effect,
                predecessor: effect,
            }
        }
        BindingCleanupError::CleanupObservationMismatch {
            continuation,
            predecessor,
        } => ViewportCoordinatorError::InvalidCleanupContinuation {
            effect: continuation,
            predecessor,
        },
        BindingCleanupError::CleanupWindowNotObserved { effect } => {
            ViewportCoordinatorError::CleanupWindowNotObserved { effect }
        }
        BindingCleanupError::TerminalBindingGuardMissing { binding } => {
            ViewportCoordinatorError::TerminalBindingGuardMissing { binding }
        }
        BindingCleanupError::TerminalBindingGuardProviderMismatch {
            binding,
            expected,
            submitted,
        } => ViewportCoordinatorError::TerminalBindingGuardProviderMismatch {
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
        RecoveryReplacementLifecycleError::MissingPending { surface } => {
            ViewportCoordinatorError::MissingRecoveryPending { surface }
        }
        RecoveryReplacementLifecycleError::ReplacementAlreadyOwned { binding, .. }
        | RecoveryReplacementLifecycleError::ReplacementMismatch { binding, .. } => {
            ViewportCoordinatorError::PendingRecoveryRegistrationMismatch {
                surface: binding.surface(),
            }
        }
        RecoveryReplacementLifecycleError::ReplacementEffectMissing { surface } => {
            ViewportCoordinatorError::MissingReplacementEffect { surface }
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
    /// The predecessor is not an exact current, safe, admitted external root binding.
    #[error("native root continuation predecessor is unavailable: {predecessor:?}")]
    NativeRootContinuationPredecessorUnavailable { predecessor: ViewportBinding },
    /// One physical window cannot continue on two logical surfaces.
    #[error("native root continuation repeats predecessor {predecessor:?}")]
    NativeRootContinuationDuplicatePredecessor { predecessor: ViewportBinding },
    /// Two physical windows cannot acquire the same incoming logical surface.
    #[error("native root continuation repeats target {target:?}")]
    NativeRootContinuationDuplicateTarget { target: SurfaceId },
    /// The replacement workspace does not contain the requested continuation target.
    #[error("native root continuation target is absent from the workspace: {target:?}")]
    NativeRootContinuationTargetMissing { target: SurfaceId },
    /// Another native window already owns the requested continuation target.
    #[error("native root continuation target {target:?} is already bound to {binding:?}")]
    NativeRootContinuationTargetOccupied {
        target: SurfaceId,
        binding: ViewportBinding,
    },
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
    MissingBindingCleanup { binding: ViewportBinding },
    #[error("retired viewport token remains reserved until exact retirement completion: {token:?}")]
    RetiredTokenReserved { token: WindowToken },
    #[error("retired cleanup ownership mismatch for {binding:?}: {ownership:?}")]
    BindingCleanupOwnershipMismatch {
        binding: ViewportBinding,
        ownership: ViewportOwnership,
    },
    #[error("destroyed viewport binding is still observed: {binding:?}")]
    DestroyedSurfaceStillObserved { binding: ViewportBinding },
    #[error("terminal binding retention guard is missing for {binding:?}")]
    TerminalBindingGuardMissing { binding: ViewportBinding },
    #[error(
        "terminal binding guard {binding:?} belongs to provider {expected:?}, not {submitted:?}"
    )]
    TerminalBindingGuardProviderMismatch {
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
    #[error("retiring binding {binding:?} has unexpected admission {admission:?}")]
    RetiringBindingUnexpectedAdmission {
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
    #[error("native create saga {saga:?} cannot mint a valid retained staging resource")]
    InvalidNativeStagingResource { saga: NativeCreateSagaId },
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
