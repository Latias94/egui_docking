//! Atomic platform-fact coordinator used by the docking engine.

mod binding_cleanup;
mod error;
mod input;
mod native_bringup;
mod native_create;
mod native_staging_resource;
mod pointer_passthrough;
mod provider;
mod recovery_cleanup;
mod recovery_replacement;
mod snapshot;
mod workspace_restore;

pub use error::ViewportCoordinatorError;

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

#[cfg(test)]
use crate::backend_ingress::BackendIngressDrainReceipt;
use crate::close_plan::{CloseRequestId, NativeCloseEdge};
#[cfg(test)]
use crate::coordinates::CoordinateUnavailable;
use crate::coordinates::{RecoveryCoordinateSnapshot, ViewportPlacementProof};
#[cfg(test)]
use crate::effect::EffectRequest;
use crate::effect::{
    EffectDispatchResult, EffectId, EffectInvalidation, EffectLedger, EffectLedgerError,
    EffectPhase, EffectRecord, EffectResult, EffectTransition, NativeCloseResolution,
    PlatformEffect, PlatformEffectEmission,
};
#[cfg(test)]
use crate::geometry::LogicalRect;
use crate::geometry::PhysicalRect;
pub use crate::ids::NativeCreateSagaId;
use crate::ids::{EngineAuthorityDomainId, ItemId, SurfaceId, WorkspaceEpoch};
use crate::intent::{NativePlacementProof, PointerId};
use crate::platform::{
    CapabilityRosterObservationStream, NativeDragSourceInput, ObservedWorkArea,
    PlatformCapabilities, PlatformCapability, PlatformSnapshot, WindowCloseObservation,
    WindowCloseState, WindowInputObservation, WindowInputState, WindowInventoryObservationStream,
    WindowPresentationObservation, WindowPresentationState, WorkAreaRosterObservationStream,
};
#[cfg(test)]
use crate::platform_provider::PlatformProviderReservation;
use crate::platform_provider::{
    PlatformObservationAuthority, PlatformObservationAuthorityError, PlatformObservationLease,
    PlatformProviderAuthorityFrontier,
};
use crate::presentation_observation::NativeStagingResourceId;
#[cfg(test)]
use crate::retention::{BindingRetentionManifest, EffectRetentionManifest};
use crate::surface_recovery::SurfaceRecoveryObligationId;
#[cfg(test)]
use crate::viewport::InventoryGeneration;
#[cfg(test)]
use crate::viewport::WorkAreaObservationGeneration;
use crate::viewport::{
    CapabilityGeneration, InputObservationGeneration, InventoryObservationGeneration,
    NativeSurfaceEstablishmentOrigin, PlatformSnapshotGeneration, ViewportBinding, ViewportRole,
    WindowToken, WorkAreaGeneration, WorkAreaToken,
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
    #[cfg(test)]
    pub fn cleanup_effects(&self) -> &[EffectId] {
        &self.cleanup_effects
    }

    #[must_use]
    #[cfg(test)]
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
        binding: ViewportBinding,
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
    #[cfg(test)]
    pub const fn capability_generation(&self) -> CapabilityGeneration {
        self.capability_generation
    }

    #[must_use]
    #[cfg(test)]
    pub const fn work_area_generation(&self) -> WorkAreaGeneration {
        self.work_area_generation
    }

    #[must_use]
    #[cfg(test)]
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

    /// Compacts terminal-binding guards after the exact observation producer has quiesced.
    #[cfg(test)]
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
        self.binding_cleanup
            .keys()
            .any(|cleanup| cleanup.window() == binding.window())
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

    pub(crate) fn has_terminal_native_binding_guard(&self, binding: ViewportBinding) -> bool {
        self.binding_cleanup.is_terminal(binding) && !self.binding_cleanup.contains_key(&binding)
    }

    pub(crate) fn settle_unpublished_placement_vacancies(
        &mut self,
        vacancies: &[ViewportBinding],
        published: &Self,
    ) -> Result<(), ViewportCoordinatorError> {
        for &binding in vacancies {
            if !published.recognizes_input_binding(binding)
                && (self.registry.window(binding.window()).is_some()
                    || self.retains_window_cleanup(binding))
                && self.has_terminal_native_binding_guard(binding)
            {
                // A shared placement created and vacated inside this candidate
                // never owned a published producer or physical callback lane.
                // The surviving window or its cleanup owns physical obligations.
                let provider = self.active_platform_provider()?;
                self.compact_quiesced_terminal_binding_guard(binding, provider)?;
            }
        }
        Ok(())
    }

    pub(crate) fn recognizes_retired_native_window(
        &self,
        binding: crate::viewport::WindowBinding,
    ) -> bool {
        self.registry
            .placements_in_window(binding)
            .any(|record| record.admission() == ViewportAdmission::Retiring)
            || self
                .binding_cleanup
                .keys()
                .any(|retired| retired.window() == binding)
            || self
                .binding_cleanup
                .terminal_bindings()
                .iter()
                .any(|retired| retired.window() == binding)
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
    #[cfg(test)]
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

    pub(crate) fn set_surface_placements(
        &mut self,
        window: crate::viewport::WindowBinding,
        placements: &[crate::coordinates::NativeSurfacePlacement],
    ) -> Result<Vec<SurfaceId>, ViewportCoordinatorError> {
        self.registry
            .set_surface_placements(window, placements, |binding| {
                self.binding_cleanup.contains_key(&binding)
                    || self.binding_cleanup.is_terminal(binding)
            })
            .map_err(ViewportCoordinatorError::Registry)
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
                (record.request().effect().binding().window() == binding.window())
                    .then_some((effect, record.request().effect().binding()))
            })
            .collect::<Vec<_>>();
        for (effect, exact) in effects {
            let _ =
                self.effects
                    .mark_destroyed(effect, exact, self.registry.inventory_generation());
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
        let bindings = self
            .registry
            .bindings_for_window(observation.binding().window())
            .collect::<Vec<_>>();
        if bindings.is_empty() {
            actions.push(ViewportLifecycleAction::SurfaceDestroyed {
                binding: observation.binding(),
                observation,
                source_coordinates,
            });
        } else {
            for binding in bindings {
                actions.push(ViewportLifecycleAction::SurfaceDestroyed {
                    binding,
                    observation,
                    source_coordinates: self
                        .registry
                        .placement_record(binding)
                        .and_then(ViewportRecord::recovery_coordinates)
                        .or(source_coordinates),
                });
            }
        }
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

    #[cfg(test)]
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
        self.binding_cleanup
            .terminal_bindings()
            .iter()
            .any(|terminal| terminal.window() == binding.window())
            || self
                .binding_cleanup
                .keys()
                .any(|retired| retired.window() == binding.window())
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
            if candidate
                .registry
                .bindings_for_window(expected.binding.window())
                .count()
                > 1
            {
                // Placement vacancy does not retire a real window with another
                // hosted member. The final member owns any native cleanup.
                candidate
                    .registry
                    .detach(expected.binding)
                    .map_err(ViewportCoordinatorError::Registry)?;
                let provider = candidate.active_platform_provider()?;
                candidate
                    .binding_cleanup
                    .record_terminal_binding_guard(expected.binding, provider);
                continue;
            }
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
            .is_some_and(|bindings| {
                bindings
                    .iter()
                    .any(|observed| observed.window() == binding.window())
            })
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
    #[cfg(test)]
    pub const fn capability_generation(&self) -> CapabilityGeneration {
        self.capability_generation
    }

    #[must_use]
    pub const fn work_area_generation(&self) -> WorkAreaGeneration {
        self.work_area_generation
    }

    /// Returns the latest accepted provider generation, including tombstones.
    #[must_use]
    #[cfg(test)]
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

    #[cfg(test)]
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

    #[cfg(test)]
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
    #[cfg(test)]
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
            .admit_window(
                &candidate
                    .registry
                    .bindings_for_window(binding.window())
                    .collect::<Vec<_>>(),
            )
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

#[cfg(test)]
mod tests;
