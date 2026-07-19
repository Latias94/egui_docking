//! Atomic platform-fact coordinator used by the docking engine.

use std::collections::{BTreeMap, BTreeSet};

use thiserror::Error;

use crate::command::WorkspaceCommand;
use crate::coordinates::{
    CoordinateUnavailable, TearOffPlacementProof, TearOffPlacementRequest,
    TearOffPlacementUnavailable, ViewportPlacementProof,
};
use crate::effect::{
    EffectDispatchResult, EffectId, EffectLedger, EffectLedgerError, EffectPhase, EffectRequest,
    EffectResult, EffectTransition, PlatformEffect,
};
use crate::geometry::LogicalRect;
use crate::ids::{SurfaceId, WorkspaceEpoch};
use crate::intent::{ContainedRecoveryPlan, NativePlacementProof, PointerId};
use crate::interaction::PreparedNativeTearOff;
use crate::platform::{
    ObservedWorkArea, PlatformCapabilities, PlatformCapability, PlatformSnapshot, WindowInputState,
};
use crate::viewport::{
    CapabilityGeneration, RouteGeneration, ViewportBinding, ViewportRole, WindowToken,
    WorkAreaGeneration, WorkAreaToken,
};
use crate::viewport_registry::{
    RegistryEvent, RetiredViewportFacts, ViewportLifecycle, ViewportRecord, ViewportRegistry,
    ViewportRegistryError,
};
use crate::viewport_route::{ViewportRouteError, ViewportRouteProof, ViewportRouteState};

macro_rules! lifecycle_id {
    ($name:ident, $description:literal) => {
        #[doc = $description]
        #[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
        #[repr(transparent)]
        pub struct $name(u64);

        impl $name {
            #[must_use]
            pub const fn new(value: u64) -> Self {
                Self(value)
            }

            #[must_use]
            pub const fn get(self) -> u64 {
                self.0
            }

            #[must_use]
            pub const fn checked_next(self) -> Option<Self> {
                match self.0.checked_add(1) {
                    Some(value) => Some(Self(value)),
                    None => None,
                }
            }
        }
    };
}

lifecycle_id!(
    NativeCreateSagaId,
    "Monotonic identity of one native tear-off creation saga."
);
lifecycle_id!(
    ViewportCloseRequestId,
    "Monotonic identity of one edge-triggered platform close request."
);

/// Public lifecycle phase of one native create saga.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NativeCreateStatus {
    Requested,
    Indeterminate,
    GeometryReadyUncommitted,
    CommittedAwaitingVisibility {
        show: EffectId,
    },
    Committed {
        show: Option<EffectId>,
        focus: Option<EffectId>,
    },
    Cancelled,
    Compensating {
        effect: EffectId,
    },
}

impl NativeCreateStatus {
    const fn topology_committed(self) -> bool {
        matches!(
            self,
            Self::CommittedAwaitingVisibility { .. } | Self::Committed { .. }
        )
    }
}

/// Queryable native create saga. Source ownership remains unchanged until commit.
#[derive(Debug, Clone, PartialEq)]
pub struct NativeCreateSaga {
    id: NativeCreateSagaId,
    binding: ViewportBinding,
    effect: EffectId,
    prepared: PreparedNativeTearOff,
    status: NativeCreateStatus,
}

/// Identity returned when one native create effect enters the ledger.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct NativeCreateRequest {
    saga: NativeCreateSagaId,
    effect: EffectId,
    binding: ViewportBinding,
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
    pub const fn effect(&self) -> EffectId {
        self.effect
    }

    #[must_use]
    pub const fn prepared(&self) -> &PreparedNativeTearOff {
        &self.prepared
    }

    #[must_use]
    pub const fn status(&self) -> NativeCreateStatus {
        self.status
    }
}

/// Workspace mutation frozen when an application accepts one viewport close.
///
/// `primary` is attempted only after the matching native binding is observed
/// destroyed. `recovery` retains the complete root when the primary command is
/// no longer valid at that point.
#[derive(Debug, Clone, PartialEq)]
pub struct ViewportClosePlan {
    primary: Option<WorkspaceCommand>,
    recovery: ContainedRecoveryPlan,
}

impl ViewportClosePlan {
    #[must_use]
    pub fn new(
        primary: Option<WorkspaceCommand>,
        recovery: impl Into<ContainedRecoveryPlan>,
    ) -> Self {
        Self {
            primary,
            recovery: recovery.into(),
        }
    }

    #[must_use]
    pub const fn primary(&self) -> Option<&WorkspaceCommand> {
        self.primary.as_ref()
    }

    #[must_use]
    pub const fn recovery(&self) -> ContainedRecoveryPlan {
        self.recovery
    }
}

/// Application decision for one exact platform close-request edge.
#[derive(Debug, Clone, PartialEq)]
pub enum ViewportCloseDecision {
    Veto,
    Accept(ViewportClosePlan),
}

/// Queryable phase of one viewport close saga.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ViewportCloseStatus {
    AwaitingDecision,
    Vetoed { effect: EffectId },
    AwaitingDestroyed { effect: Option<EffectId> },
    EffectFailed { effect: EffectId },
    Indeterminate { effect: EffectId },
    Cleared,
    Destroyed,
}

/// Frozen record of one edge-triggered platform close request.
#[derive(Debug, Clone, PartialEq)]
pub struct ViewportCloseRequest {
    id: ViewportCloseRequestId,
    binding: ViewportBinding,
    role: ViewportRole,
    recovery: Option<ContainedRecoveryPlan>,
    status: ViewportCloseStatus,
    plan: Option<ViewportClosePlan>,
    effect: Option<EffectId>,
}

/// Summary of the exact native bindings reconciled by a successful workspace restore.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ViewportReconciliation {
    rebound: Vec<(ViewportBinding, ViewportBinding)>,
    retired: Vec<ViewportBinding>,
    cleanup_effects: Vec<EffectId>,
    replacements: Vec<ViewportBinding>,
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
    pub fn replacements(&self) -> &[ViewportBinding] {
        &self.replacements
    }

    #[must_use]
    pub fn unbound_surfaces(&self) -> &[SurfaceId] {
        &self.unbound_surfaces
    }
}

/// Queryable state retained for an old binding which may still appear or disappear late.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RetiredViewportStatus {
    AwaitingAppearance,
    CleanupRequested { effect: EffectId },
    CleanupIndeterminate { effect: EffectId },
    CleanupFailed { effect: EffectId },
}

/// Old-epoch binding isolated from the current logical surface roster.
#[derive(Debug, Clone, PartialEq)]
pub struct RetiredViewport {
    binding: ViewportBinding,
    role: ViewportRole,
    status: RetiredViewportStatus,
    observed: bool,
    may_appear_late: bool,
    cleanup: RetiredCleanup,
}

impl RetiredViewport {
    #[must_use]
    pub const fn binding(&self) -> ViewportBinding {
        self.binding
    }

    #[must_use]
    pub const fn role(&self) -> ViewportRole {
        self.role
    }

    #[must_use]
    pub const fn status(&self) -> RetiredViewportStatus {
        self.status
    }

    #[must_use]
    pub const fn observed(&self) -> bool {
        self.observed
    }

    #[must_use]
    pub const fn may_appear_late(&self) -> bool {
        self.may_appear_late
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RetiredCleanup {
    CompensateCreate { effect: EffectId },
    ReleaseWindow,
}

/// Queryable recovery phase after a native surface was authoritatively destroyed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RecoveryPendingStatus {
    AwaitingRecoveryHost,
    ReplacementRequested { effect: EffectId },
    ReplacementIndeterminate { effect: EffectId },
    ReplacementFailed { effect: EffectId },
    ReplacementReady,
    RecoveryCommitted,
    CompensatingReplacement { effect: EffectId },
}

/// Whole-root recovery retained while neither a contained host nor replacement is ready.
#[derive(Debug, Clone, PartialEq)]
pub struct RecoveryPending {
    destroyed_binding: ViewportBinding,
    recovery: ContainedRecoveryPlan,
    replacement_binding: Option<ViewportBinding>,
    replacement_effect: Option<EffectId>,
    status: RecoveryPendingStatus,
}

/// Status of a restore-time replacement for a retained logical surface.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RestoreReplacementStatus {
    Requested { effect: EffectId },
    Indeterminate { effect: EffectId },
    Ready,
    Failed { effect: EffectId },
}

/// New current binding created because an old binding could not be safely rebound.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RestoreReplacement {
    binding: ViewportBinding,
    effect: EffectId,
    status: RestoreReplacementStatus,
}

impl RestoreReplacement {
    #[must_use]
    pub const fn binding(self) -> ViewportBinding {
        self.binding
    }

    #[must_use]
    pub const fn effect(self) -> EffectId {
        self.effect
    }

    #[must_use]
    pub const fn status(self) -> RestoreReplacementStatus {
        self.status
    }
}

impl RecoveryPending {
    #[must_use]
    pub const fn destroyed_binding(&self) -> ViewportBinding {
        self.destroyed_binding
    }

    #[must_use]
    pub const fn recovery(&self) -> ContainedRecoveryPlan {
        self.recovery
    }

    #[must_use]
    pub const fn replacement_binding(&self) -> Option<ViewportBinding> {
        self.replacement_binding
    }

    #[must_use]
    pub const fn replacement_effect(&self) -> Option<EffectId> {
        self.replacement_effect
    }

    #[must_use]
    pub const fn status(&self) -> RecoveryPendingStatus {
        self.status
    }
}

impl ViewportCloseRequest {
    #[must_use]
    pub const fn id(&self) -> ViewportCloseRequestId {
        self.id
    }

    #[must_use]
    pub const fn binding(&self) -> ViewportBinding {
        self.binding
    }

    #[must_use]
    pub const fn role(&self) -> ViewportRole {
        self.role
    }

    /// Returns the recovery captured when the request edge was observed.
    #[must_use]
    pub const fn recovery(&self) -> Option<ContainedRecoveryPlan> {
        self.recovery
    }

    #[must_use]
    pub const fn status(&self) -> ViewportCloseStatus {
        self.status
    }

    /// Returns the accepted immutable plan, if this request was accepted.
    #[must_use]
    pub const fn plan(&self) -> Option<&ViewportClosePlan> {
        self.plan.as_ref()
    }

    #[must_use]
    pub const fn effect(&self) -> Option<EffectId> {
        self.effect
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum ViewportLifecycleAction {
    CreateReady {
        saga: NativeCreateSagaId,
        prepared: Box<PreparedNativeTearOff>,
    },
    SurfaceDestroyed {
        binding: ViewportBinding,
        resolution: ViewportDestructionResolution,
    },
    RetryRecovery {
        destroyed_binding: ViewportBinding,
        recovery: ContainedRecoveryPlan,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum ViewportDestructionResolution {
    Accepted {
        request: ViewportCloseRequestId,
        plan: ViewportClosePlan,
    },
    Recover {
        recovery: ContainedRecoveryPlan,
    },
    Unplanned,
}

/// Complete publication result of one platform fact snapshot.
#[derive(Debug, Clone, PartialEq)]
pub struct ViewportFrameTransition {
    capability_generation: CapabilityGeneration,
    work_area_generation: WorkAreaGeneration,
    route_generation: RouteGeneration,
    capabilities_changed: bool,
    work_areas_changed: bool,
    registry_events: Vec<RegistryEvent>,
    close_requests: Vec<ViewportCloseRequestId>,
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
    pub const fn route_generation(&self) -> RouteGeneration {
        self.route_generation
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

    /// Returns close-request identities created by this exact snapshot edge.
    #[must_use]
    pub fn close_requests(&self) -> &[ViewportCloseRequestId] {
        &self.close_requests
    }

    pub(crate) fn actions(&self) -> &[ViewportLifecycleAction] {
        &self.actions
    }
}

/// Deep core module owning capabilities, bindings, coordinate facts, routes, and effects.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ViewportCoordinator {
    workspace_epoch: WorkspaceEpoch,
    capabilities: PlatformCapabilities,
    capability_generation: CapabilityGeneration,
    work_areas: BTreeMap<WorkAreaToken, ObservedWorkArea>,
    work_area_generation: WorkAreaGeneration,
    registry: ViewportRegistry,
    routes: ViewportRouteState,
    effects: EffectLedger,
    drag_sources: BTreeMap<PointerId, ViewportBinding>,
    pointer_hit_test_leases: BTreeMap<ViewportBinding, PointerHitTestLease>,
    last_create_saga: NativeCreateSagaId,
    create_sagas: BTreeMap<NativeCreateSagaId, NativeCreateSaga>,
    last_close_request: ViewportCloseRequestId,
    close_requests: BTreeMap<ViewportCloseRequestId, ViewportCloseRequest>,
    active_close_requests: BTreeMap<ViewportBinding, ViewportCloseRequestId>,
    recovery_plans: BTreeMap<SurfaceId, ContainedRecoveryPlan>,
    retired_viewports: BTreeMap<WindowToken, RetiredViewport>,
    pending_recoveries: BTreeMap<SurfaceId, RecoveryPending>,
    restore_replacements: BTreeMap<SurfaceId, RestoreReplacement>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PointerHitTestLease {
    original: WindowInputState,
    changed_by_us: bool,
    holders: BTreeSet<PointerId>,
}

struct RestoreAnalysis {
    close_by_binding: BTreeMap<ViewportBinding, ViewportCloseRequest>,
    creation_by_binding: BTreeMap<ViewportBinding, (EffectId, RetiredCleanup)>,
    cleanup_by_binding: BTreeMap<ViewportBinding, EffectId>,
    retained_bindings: BTreeSet<ViewportBinding>,
    passthrough_restores: BTreeSet<ViewportBinding>,
    replacement_supported: bool,
}

#[derive(Default)]
struct RestoreAccumulation {
    retired: Vec<ViewportBinding>,
    cleanup_effects: Vec<EffectId>,
    replacements: Vec<ViewportBinding>,
    unbound_surfaces: Vec<SurfaceId>,
}

impl ViewportCoordinator {
    /// Registers an existing adapter window without storing its OS handle.
    pub(crate) fn register_existing(
        &mut self,
        epoch: WorkspaceEpoch,
        surface: SurfaceId,
        token: WindowToken,
        role: ViewportRole,
        recovery: Option<ContainedRecoveryPlan>,
    ) -> Result<ViewportBinding, ViewportCoordinatorError> {
        if self.registry.records().next().is_some() && epoch != self.workspace_epoch {
            return Err(ViewportCoordinatorError::WorkspaceEpochMismatch {
                expected: self.workspace_epoch,
                actual: epoch,
            });
        }
        if self.retired_viewports.contains_key(&token) {
            return Err(ViewportCoordinatorError::RetiredTokenReserved { token });
        }
        if role == ViewportRole::Child && recovery.is_none() {
            return Err(ViewportCoordinatorError::ChildRecoveryRequired { surface });
        }
        let binding = self
            .registry
            .register_existing(epoch, surface, token, role)
            .map_err(ViewportCoordinatorError::Registry)?;
        if let Some(recovery) = recovery {
            self.recovery_plans.insert(surface, recovery);
        }
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
        let analysis = candidate.analyze_restore(desired_surfaces);
        let registry = candidate
            .registry
            .reconcile_restore(new_epoch, &analysis.retained_bindings)
            .map_err(ViewportCoordinatorError::Registry)?;
        let rebound_map: BTreeMap<ViewportBinding, ViewportBinding> =
            registry.retained().iter().copied().collect();

        candidate.effects.invalidate_unemitted_before(new_epoch);
        candidate.workspace_epoch = new_epoch;
        candidate.routes.clear();
        candidate.drag_sources.clear();
        candidate.pointer_hit_test_leases.clear();
        candidate.restore_replacements.clear();
        let mut accumulated = RestoreAccumulation::default();
        candidate.reissue_invalidated_retired_cleanup(&mut accumulated.cleanup_effects)?;
        candidate.request_rebound_restore_cleanup(
            new_epoch,
            &analysis,
            &rebound_map,
            &mut accumulated.cleanup_effects,
        )?;
        candidate.request_restore_replacements(
            new_epoch,
            desired_surfaces,
            registry.retired(),
            analysis.replacement_supported,
            &mut accumulated,
        )?;
        candidate.retire_restored_bindings(registry.retired(), &analysis, &mut accumulated)?;
        candidate.finish_restore(desired_surfaces, &mut accumulated.unbound_surfaces);

        let reconciliation = ViewportReconciliation {
            rebound: registry.retained().to_vec(),
            retired: accumulated.retired,
            cleanup_effects: accumulated.cleanup_effects,
            replacements: accumulated.replacements,
            unbound_surfaces: accumulated.unbound_surfaces,
        };
        *self = candidate;
        Ok(reconciliation)
    }

    fn analyze_restore(&self, desired_surfaces: &BTreeSet<SurfaceId>) -> RestoreAnalysis {
        let create_by_binding: BTreeMap<_, _> = self
            .create_sagas
            .values()
            .map(|saga| (saga.binding, saga.clone()))
            .collect();
        let close_by_binding: BTreeMap<_, _> = self
            .active_close_requests
            .iter()
            .filter_map(|(binding, request)| {
                self.close_requests
                    .get(request)
                    .cloned()
                    .map(|request| (*binding, request))
            })
            .collect();
        let mut creation_by_binding: BTreeMap<_, _> = create_by_binding
            .iter()
            .map(|(binding, saga)| {
                (
                    *binding,
                    (
                        saga.effect,
                        RetiredCleanup::CompensateCreate {
                            effect: saga.effect,
                        },
                    ),
                )
            })
            .collect();
        let mut cleanup_by_binding = BTreeMap::new();
        for (binding, saga) in &create_by_binding {
            if let NativeCreateStatus::Compensating { effect } = saga.status {
                cleanup_by_binding.insert(*binding, effect);
            }
        }
        for (binding, request) in &close_by_binding {
            if let Some(effect) = request.effect {
                cleanup_by_binding.insert(*binding, effect);
            }
        }
        for pending in self.pending_recoveries.values() {
            if let (Some(binding), Some(effect)) =
                (pending.replacement_binding, pending.replacement_effect)
            {
                creation_by_binding.insert(binding, (effect, RetiredCleanup::ReleaseWindow));
                if let RecoveryPendingStatus::CompensatingReplacement { effect } = pending.status {
                    cleanup_by_binding.insert(binding, effect);
                }
            }
        }
        for replacement in self.restore_replacements.values() {
            creation_by_binding.insert(
                replacement.binding,
                (replacement.effect, RetiredCleanup::ReleaseWindow),
            );
        }
        let replacement_bindings: BTreeSet<_> = self
            .pending_recoveries
            .values()
            .filter_map(|pending| pending.replacement_binding)
            .chain(
                self.restore_replacements
                    .values()
                    .filter_map(|replacement| {
                        (replacement.status != RestoreReplacementStatus::Ready)
                            .then_some(replacement.binding)
                    }),
            )
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
                    &close_by_binding,
                    &replacement_bindings,
                )
                .then_some(record.binding())
            })
            .collect();
        RestoreAnalysis {
            close_by_binding,
            creation_by_binding,
            cleanup_by_binding,
            retained_bindings,
            passthrough_restores: self
                .pointer_hit_test_leases
                .iter()
                .filter_map(|(binding, lease)| lease.changed_by_us.then_some(*binding))
                .collect(),
            replacement_supported: self.capabilities.native_window_lifecycle().is_supported()
                && self.capabilities.authoritative_inventory().is_supported(),
        }
    }

    fn restore_binding_is_safe(
        surface: SurfaceId,
        record: &ViewportRecord,
        desired_surfaces: &BTreeSet<SurfaceId>,
        creates: &BTreeMap<ViewportBinding, NativeCreateSaga>,
        closes: &BTreeMap<ViewportBinding, ViewportCloseRequest>,
        replacements: &BTreeSet<ViewportBinding>,
    ) -> bool {
        let binding = record.binding();
        let create_is_committed = creates
            .get(&binding)
            .is_none_or(|saga| saga.status.topology_committed());
        let destructive_close = closes.get(&binding).is_some_and(|request| {
            request.plan.is_some()
                && matches!(
                    request.status,
                    ViewportCloseStatus::AwaitingDestroyed { .. }
                        | ViewportCloseStatus::Indeterminate { .. }
                )
        });
        desired_surfaces.contains(&surface)
            && create_is_committed
            && !destructive_close
            && !replacements.contains(&binding)
            && !matches!(
                record.lifecycle(),
                ViewportLifecycle::AwaitingDestroyed | ViewportLifecycle::Missing
            )
    }

    fn reissue_invalidated_retired_cleanup(
        &mut self,
        cleanup_effects: &mut Vec<EffectId>,
    ) -> Result<(), ViewportCoordinatorError> {
        let invalidated: Vec<_> = self
            .retired_viewports
            .iter()
            .filter_map(|(token, retired)| {
                retired_effect(retired.status)
                    .filter(|effect| {
                        self.effects.record(*effect).is_some_and(|record| {
                            matches!(record.phase(), EffectPhase::InvalidatedByRestore { .. })
                        })
                    })
                    .map(|_| (*token, retired.binding, retired.cleanup, retired.observed))
            })
            .collect();
        for (token, binding, cleanup, observed) in invalidated {
            if let Some(retired) = self.retired_viewports.get_mut(&token) {
                retired.status = RetiredViewportStatus::AwaitingAppearance;
            }
            if observed {
                let effect = self.request_retired_cleanup(binding, cleanup)?;
                self.retired_viewports
                    .get_mut(&token)
                    .ok_or(ViewportCoordinatorError::MissingRetiredViewport { token })?
                    .status = RetiredViewportStatus::CleanupRequested { effect };
                cleanup_effects.push(effect);
            }
        }
        Ok(())
    }

    fn request_rebound_restore_cleanup(
        &mut self,
        new_epoch: WorkspaceEpoch,
        analysis: &RestoreAnalysis,
        rebound: &BTreeMap<ViewportBinding, ViewportBinding>,
        cleanup_effects: &mut Vec<EffectId>,
    ) -> Result<(), ViewportCoordinatorError> {
        for old_binding in &analysis.passthrough_restores {
            let Some(binding) = rebound.get(old_binding).copied() else {
                continue;
            };
            let effect = self
                .effects
                .request_in(
                    new_epoch,
                    PlatformEffect::SetPointerPassthrough {
                        binding,
                        enabled: false,
                    },
                )
                .map_err(ViewportCoordinatorError::Effect)?;
            cleanup_effects.push(effect);
        }
        for (old_binding, request) in &analysis.close_by_binding {
            let Some(binding) = rebound.get(old_binding).copied() else {
                continue;
            };
            let effect_kind = match request.role {
                ViewportRole::Root => PlatformEffect::CancelRootClose { binding },
                ViewportRole::Child => PlatformEffect::RetainChild { binding },
            };
            cleanup_effects.push(
                self.effects
                    .request_in(new_epoch, effect_kind)
                    .map_err(ViewportCoordinatorError::Effect)?,
            );
        }
        Ok(())
    }

    fn request_restore_replacements(
        &mut self,
        new_epoch: WorkspaceEpoch,
        desired_surfaces: &BTreeSet<SurfaceId>,
        retired: &[RetiredViewportFacts],
        replacement_supported: bool,
        accumulated: &mut RestoreAccumulation,
    ) -> Result<(), ViewportCoordinatorError> {
        for facts in retired {
            let surface = facts.binding().surface();
            if !desired_surfaces.contains(&surface) {
                continue;
            }
            let placement = facts.last_coordinates().map(|coordinates| {
                coordinates
                    .outer_bounds()
                    .unwrap_or_else(|| coordinates.content_bounds())
            });
            let Some(placement) = placement.filter(|_| replacement_supported) else {
                accumulated.unbound_surfaces.push(surface);
                continue;
            };
            let binding = self
                .registry
                .reserve(new_epoch, surface, facts.role())
                .map_err(ViewportCoordinatorError::Registry)?;
            let effect = self
                .effects
                .request_in(
                    new_epoch,
                    PlatformEffect::RequestReplacement {
                        binding,
                        placement,
                        role: facts.role(),
                    },
                )
                .map_err(ViewportCoordinatorError::Effect)?;
            self.restore_replacements.insert(
                surface,
                RestoreReplacement {
                    binding,
                    effect,
                    status: RestoreReplacementStatus::Requested { effect },
                },
            );
            accumulated.replacements.push(binding);
            accumulated.cleanup_effects.push(effect);
        }
        Ok(())
    }

    fn retire_restored_bindings(
        &mut self,
        retired: &[RetiredViewportFacts],
        analysis: &RestoreAnalysis,
        accumulated: &mut RestoreAccumulation,
    ) -> Result<(), ViewportCoordinatorError> {
        for facts in retired {
            let binding = facts.binding();
            accumulated.retired.push(binding);
            let cleanup = analysis
                .creation_by_binding
                .get(&binding)
                .map_or(RetiredCleanup::ReleaseWindow, |(_, cleanup)| *cleanup);
            let existing_effect =
                analysis
                    .cleanup_by_binding
                    .get(&binding)
                    .copied()
                    .filter(|effect| {
                        self.effects.record(*effect).is_some_and(|record| {
                            record.was_emitted()
                                && !matches!(
                                    record.phase(),
                                    EffectPhase::DispatchFailed(_)
                                        | EffectPhase::Unsupported(_)
                                        | EffectPhase::InvalidatedByRestore { .. }
                                )
                        })
                    });
            let creation_may_appear =
                analysis
                    .creation_by_binding
                    .get(&binding)
                    .is_some_and(|(effect, _)| {
                        self.effects.record(*effect).is_some_and(|record| {
                            record.was_emitted()
                                && matches!(
                                    record.phase(),
                                    EffectPhase::Requested | EffectPhase::Indeterminate(_)
                                )
                        })
                    });
            let may_appear_late = creation_may_appear
                || (!facts.ever_observed()
                    && !matches!(facts.lifecycle(), ViewportLifecycle::Missing));
            let observed =
                facts.ever_observed() && !matches!(facts.lifecycle(), ViewportLifecycle::Missing);
            if !observed && !may_appear_late {
                continue;
            }
            let status = existing_effect
                .map_or(RetiredViewportStatus::AwaitingAppearance, |effect| {
                    self.retired_status_for_effect(effect)
                });
            self.retired_viewports.insert(
                binding.token(),
                RetiredViewport {
                    binding,
                    role: facts.role(),
                    status,
                    observed,
                    may_appear_late,
                    cleanup,
                },
            );
            if observed && existing_effect.is_none() {
                let effect = self.request_retired_cleanup(binding, cleanup)?;
                self.retired_viewports
                    .get_mut(&binding.token())
                    .ok_or(ViewportCoordinatorError::MissingRetiredViewport {
                        token: binding.token(),
                    })?
                    .status = RetiredViewportStatus::CleanupRequested { effect };
                accumulated.cleanup_effects.push(effect);
            }
        }
        Ok(())
    }

    fn retired_status_for_effect(&self, effect: EffectId) -> RetiredViewportStatus {
        match self
            .effects
            .record(effect)
            .map(crate::effect::EffectRecord::phase)
        {
            Some(EffectPhase::Indeterminate(_)) => {
                RetiredViewportStatus::CleanupIndeterminate { effect }
            }
            Some(EffectPhase::DispatchFailed(_) | EffectPhase::Unsupported(_)) => {
                RetiredViewportStatus::CleanupFailed { effect }
            }
            _ => RetiredViewportStatus::CleanupRequested { effect },
        }
    }

    fn finish_restore(
        &mut self,
        desired_surfaces: &BTreeSet<SurfaceId>,
        unbound_surfaces: &mut Vec<SurfaceId>,
    ) {
        for request in self.close_requests.values_mut() {
            if !matches!(request.status, ViewportCloseStatus::Destroyed) {
                request.status = ViewportCloseStatus::Cleared;
                request.plan = None;
            }
        }
        self.active_close_requests.clear();
        self.create_sagas.clear();
        self.recovery_plans.clear();
        self.pending_recoveries.clear();
        self.capabilities = PlatformCapabilities::default();
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

    /// Freezes one application decision for an exact close-request identity.
    ///
    /// No workspace ownership changes here. An accepted plan is published to
    /// the engine only after authoritative inventory observes the same binding
    /// destroyed.
    pub(crate) fn decide_viewport_close(
        &mut self,
        request_id: ViewportCloseRequestId,
        decision: ViewportCloseDecision,
    ) -> Result<Option<EffectId>, ViewportCoordinatorError> {
        let mut candidate = self.clone();
        let request = candidate.close_requests.get(&request_id).cloned().ok_or(
            ViewportCoordinatorError::MissingCloseRequest {
                request: request_id,
            },
        )?;
        if !matches!(
            request.status,
            ViewportCloseStatus::AwaitingDecision | ViewportCloseStatus::EffectFailed { .. }
        ) {
            return Err(ViewportCoordinatorError::CloseRequestAlreadyDecided {
                request: request_id,
                status: request.status,
            });
        }

        let (effect, status, plan) = match decision {
            ViewportCloseDecision::Veto => {
                let effect_kind = match request.role {
                    ViewportRole::Root => PlatformEffect::CancelRootClose {
                        binding: request.binding,
                    },
                    ViewportRole::Child => PlatformEffect::RetainChild {
                        binding: request.binding,
                    },
                };
                let effect = candidate
                    .effects
                    .request(effect_kind)
                    .map_err(ViewportCoordinatorError::Effect)?;
                (Some(effect), ViewportCloseStatus::Vetoed { effect }, None)
            }
            ViewportCloseDecision::Accept(mut plan) => {
                if request
                    .recovery
                    .is_some_and(|recovery| recovery.root() != plan.recovery().root())
                {
                    return Err(ViewportCoordinatorError::CloseRecoveryRootMismatch {
                        request: request_id,
                    });
                }
                if let Some(projected) =
                    candidate.project_recovery_plan(request.binding.surface(), plan.recovery())
                {
                    plan.recovery = projected;
                }
                let effect_kind = match request.role {
                    ViewportRole::Root => PlatformEffect::RequestRootClose {
                        binding: request.binding,
                    },
                    ViewportRole::Child => PlatformEffect::ReleaseChild {
                        binding: request.binding,
                    },
                };
                let effect = Some(
                    candidate
                        .effects
                        .request(effect_kind)
                        .map_err(ViewportCoordinatorError::Effect)?,
                );
                candidate
                    .registry
                    .mark_awaiting_destroyed(request.binding)
                    .map_err(ViewportCoordinatorError::Registry)?;
                (
                    effect,
                    ViewportCloseStatus::AwaitingDestroyed { effect },
                    Some(plan),
                )
            }
        };

        let close_request = candidate.close_requests.get_mut(&request_id).ok_or(
            ViewportCoordinatorError::MissingCloseRequest {
                request: request_id,
            },
        )?;
        close_request.effect = effect;
        close_request.status = status;
        close_request.plan = plan;
        *self = candidate;
        Ok(effect)
    }

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
        if proposal.recovery().root() != proposal.root() {
            return Err(ViewportCoordinatorError::RecoveryRootMismatch);
        }

        let mut candidate = self.clone();
        let saga = candidate
            .last_create_saga
            .checked_next()
            .ok_or(ViewportCoordinatorError::NativeCreateSagaExhausted)?;
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
        candidate.last_create_saga = saga;
        candidate.create_sagas.insert(
            saga,
            NativeCreateSaga {
                id: saga,
                binding,
                effect,
                prepared,
                status: NativeCreateStatus::Requested,
            },
        );
        *self = candidate;
        Ok(NativeCreateRequest {
            saga,
            effect,
            binding,
        })
    }

    /// Atomically publishes a complete platform snapshot and all routes derived from it.
    pub(crate) fn publish_snapshot(
        &mut self,
        snapshot: &PlatformSnapshot,
    ) -> Result<ViewportFrameTransition, ViewportCoordinatorError> {
        let mut candidate = self.clone();
        let capability_generation = candidate
            .capability_generation
            .checked_next()
            .ok_or(ViewportCoordinatorError::CapabilityGenerationExhausted)?;
        let capabilities_changed = candidate.capabilities != *snapshot.capabilities();
        candidate.capabilities = snapshot.capabilities().clone();
        candidate.capability_generation = capability_generation;
        let work_areas: BTreeMap<_, _> = snapshot
            .work_areas()
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
        let registry = candidate
            .registry
            .apply_snapshot(snapshot)
            .map_err(ViewportCoordinatorError::Registry)?;
        candidate.refresh_recovery_geometry();
        candidate.reconcile_create_inventory(snapshot)?;
        candidate.reconcile_retired_inventory(snapshot)?;
        let route_generation = candidate
            .routes
            .publish(
                snapshot,
                &candidate.registry,
                capability_generation,
                &candidate.drag_sources,
            )
            .map_err(ViewportCoordinatorError::Route)?;
        let registry_events = registry.events().to_vec();
        let (actions, close_requests) = candidate.reduce_registry_events(&registry_events)?;
        let transition = ViewportFrameTransition {
            capability_generation,
            work_area_generation: candidate.work_area_generation,
            route_generation,
            capabilities_changed,
            work_areas_changed,
            registry_events,
            close_requests,
            actions,
        };
        *self = candidate;
        Ok(transition)
    }

    /// Reprojects each durable recovery intent from the latest native geometry.
    ///
    /// The child rectangle is authoritative desktop-physical data; only the
    /// designated recovery host's acknowledged origin and scale may convert it
    /// into host-local logical coordinates. Missing either fact leaves the last
    /// known intent unchanged.
    fn refresh_recovery_geometry(&mut self) {
        let updates: Vec<(SurfaceId, ContainedRecoveryPlan)> = self
            .recovery_plans
            .iter()
            .filter_map(|(child_surface, plan)| {
                self.project_recovery_plan(*child_surface, *plan)
                    .map(|plan| (*child_surface, plan))
            })
            .collect();
        for (child_surface, plan) in updates {
            self.recovery_plans.insert(child_surface, plan);
        }
    }

    fn project_recovery_plan(
        &self,
        child_surface: SurfaceId,
        plan: ContainedRecoveryPlan,
    ) -> Option<ContainedRecoveryPlan> {
        let child = self.registry.record(child_surface)?.coordinates()?;
        let host = self.registry.record(plan.surface())?.coordinates()?;
        let physical = child
            .outer_bounds()
            .unwrap_or_else(|| child.content_bounds());
        host.desktop_rect_to_surface(physical)
            .ok()
            .map(|logical| plan.with_requested_rect(logical))
    }

    fn reconcile_retired_inventory(
        &mut self,
        snapshot: &PlatformSnapshot,
    ) -> Result<(), ViewportCoordinatorError> {
        if !snapshot
            .capabilities()
            .authoritative_inventory()
            .is_supported()
        {
            return Ok(());
        }
        let present: BTreeSet<WindowToken> = snapshot
            .windows()
            .iter()
            .map(crate::platform::ObservedWindow::token)
            .collect();
        let tokens: Vec<WindowToken> = self.retired_viewports.keys().copied().collect();
        for token in tokens {
            let is_present = present.contains(&token);
            let (binding, cleanup, status, observed, may_appear_late) = {
                let retired = self
                    .retired_viewports
                    .get(&token)
                    .ok_or(ViewportCoordinatorError::MissingRetiredViewport { token })?;
                (
                    retired.binding,
                    retired.cleanup,
                    retired.status,
                    retired.observed,
                    retired.may_appear_late,
                )
            };
            if is_present {
                if let Some(retired) = self.retired_viewports.get_mut(&token) {
                    retired.observed = true;
                }
                if status == RetiredViewportStatus::AwaitingAppearance {
                    let effect = self.request_retired_cleanup(binding, cleanup)?;
                    if let Some(retired) = self.retired_viewports.get_mut(&token) {
                        retired.status = RetiredViewportStatus::CleanupRequested { effect };
                    }
                }
                continue;
            }
            if observed {
                if let Some(effect) = retired_effect(status) {
                    let _ = self.effects.mark_destroyed(
                        effect,
                        binding,
                        self.registry.inventory_generation(),
                    );
                }
                self.retired_viewports.remove(&token);
            } else if !may_appear_late {
                self.retired_viewports.remove(&token);
            }
        }
        Ok(())
    }

    fn request_retired_cleanup(
        &mut self,
        binding: ViewportBinding,
        cleanup: RetiredCleanup,
    ) -> Result<EffectId, ViewportCoordinatorError> {
        let effect = match cleanup {
            RetiredCleanup::CompensateCreate { effect } => PlatformEffect::CompensatingClose {
                binding,
                compensates: effect,
            },
            RetiredCleanup::ReleaseWindow => match self
                .retired_viewports
                .get(&binding.token())
                .map_or(ViewportRole::Child, |retired| retired.role)
            {
                ViewportRole::Root => PlatformEffect::RequestRootClose { binding },
                ViewportRole::Child => PlatformEffect::ReleaseChild { binding },
            },
        };
        self.effects
            .request_in(self.workspace_epoch, effect)
            .map_err(ViewportCoordinatorError::Effect)
    }

    fn reconcile_create_inventory(
        &mut self,
        snapshot: &PlatformSnapshot,
    ) -> Result<(), ViewportCoordinatorError> {
        if !snapshot
            .capabilities()
            .authoritative_inventory()
            .is_supported()
        {
            return Ok(());
        }
        let present_tokens: std::collections::BTreeSet<WindowToken> = snapshot
            .windows()
            .iter()
            .map(crate::platform::ObservedWindow::token)
            .collect();
        let sagas: Vec<(NativeCreateSagaId, NativeCreateStatus, ViewportBinding)> = self
            .create_sagas
            .iter()
            .map(|(id, saga)| (*id, saga.status, saga.binding))
            .collect();
        for (saga, status, binding) in sagas {
            let present = present_tokens.contains(&binding.token());
            if status == NativeCreateStatus::Cancelled && present {
                self.request_create_compensation(saga)?;
                continue;
            }
            if !present
                && matches!(
                    status,
                    NativeCreateStatus::Cancelled | NativeCreateStatus::Indeterminate
                )
                && self
                    .registry
                    .record(binding.surface())
                    .is_some_and(|record| {
                        record.binding() == binding
                            && matches!(
                                record.lifecycle(),
                                crate::viewport_registry::ViewportLifecycle::AwaitingObservation
                            )
                    })
            {
                self.retire_unobserved_create(saga, binding)?;
            }
        }
        Ok(())
    }

    fn retire_unobserved_create(
        &mut self,
        saga: NativeCreateSagaId,
        binding: ViewportBinding,
    ) -> Result<(), ViewportCoordinatorError> {
        let create = self
            .create_sagas
            .get(&saga)
            .ok_or(ViewportCoordinatorError::MissingCreateSaga { saga })?;
        let cleanup = RetiredCleanup::CompensateCreate {
            effect: create.effect,
        };
        self.registry
            .discard_unobserved(binding)
            .map_err(ViewportCoordinatorError::Registry)?;
        self.retired_viewports.insert(
            binding.token(),
            RetiredViewport {
                binding,
                role: ViewportRole::Child,
                status: RetiredViewportStatus::AwaitingAppearance,
                observed: false,
                may_appear_late: true,
                cleanup,
            },
        );
        self.create_sagas.remove(&saga);
        Ok(())
    }

    fn reduce_registry_events(
        &mut self,
        events: &[RegistryEvent],
    ) -> Result<(Vec<ViewportLifecycleAction>, Vec<ViewportCloseRequestId>), ViewportCoordinatorError>
    {
        let mut actions = Vec::new();
        let mut close_requests = Vec::new();
        for event in events {
            match *event {
                RegistryEvent::Ready { binding } => {
                    self.reduce_ready_binding(binding, &mut actions)?;
                }
                RegistryEvent::CloseRequested { binding } => {
                    if let Some(request) = self.reduce_close_requested(binding)? {
                        close_requests.push(request);
                    }
                }
                RegistryEvent::CloseRequestCleared { binding } => {
                    self.reduce_close_request_cleared(binding);
                }
                RegistryEvent::Destroyed { binding } => {
                    self.reduce_destroyed_binding(binding, &mut actions);
                }
                RegistryEvent::FactsUnavailable { .. } => {}
            }
        }
        let ready_surfaces: BTreeSet<SurfaceId> = self
            .registry
            .records()
            .filter_map(|(surface, record)| record.is_ready().then_some(surface))
            .collect();
        let mut queued = BTreeSet::new();
        for pending in self.pending_recoveries.values() {
            if ready_surfaces.contains(&pending.recovery.surface())
                && matches!(
                    pending.status,
                    RecoveryPendingStatus::AwaitingRecoveryHost
                        | RecoveryPendingStatus::ReplacementRequested { .. }
                        | RecoveryPendingStatus::ReplacementIndeterminate { .. }
                        | RecoveryPendingStatus::ReplacementFailed { .. }
                )
                && queued.insert(pending.destroyed_binding.surface())
            {
                actions.push(ViewportLifecycleAction::RetryRecovery {
                    destroyed_binding: pending.destroyed_binding,
                    recovery: pending.recovery,
                });
            }
        }
        Ok((actions, close_requests))
    }

    fn reduce_close_requested(
        &mut self,
        binding: ViewportBinding,
    ) -> Result<Option<ViewportCloseRequestId>, ViewportCoordinatorError> {
        if self.active_close_requests.contains_key(&binding) {
            return Ok(None);
        }
        let record =
            self.registry
                .record(binding.surface())
                .ok_or(ViewportCoordinatorError::Registry(
                    ViewportRegistryError::MissingSurface {
                        surface: binding.surface(),
                    },
                ))?;
        if record.binding() != binding {
            return Err(ViewportCoordinatorError::Registry(
                ViewportRegistryError::StaleBinding { binding },
            ));
        }
        let request = self
            .last_close_request
            .checked_next()
            .ok_or(ViewportCoordinatorError::CloseRequestIdExhausted)?;
        let close_request = ViewportCloseRequest {
            id: request,
            binding,
            role: record.role(),
            recovery: self.recovery_plans.get(&binding.surface()).copied(),
            status: ViewportCloseStatus::AwaitingDecision,
            plan: None,
            effect: None,
        };
        self.last_close_request = request;
        self.close_requests.insert(request, close_request);
        self.active_close_requests.insert(binding, request);
        Ok(Some(request))
    }

    fn reduce_close_request_cleared(&mut self, binding: ViewportBinding) {
        let Some(request_id) = self.active_close_requests.get(&binding).copied() else {
            return;
        };
        let Some(request) = self.close_requests.get_mut(&request_id) else {
            self.active_close_requests.remove(&binding);
            return;
        };
        if matches!(
            request.status,
            ViewportCloseStatus::AwaitingDestroyed { .. }
        ) {
            return;
        }
        if let Some(effect) = request.effect {
            let _ = self.effects.mark_observed_applied(
                effect,
                binding,
                self.registry.inventory_generation(),
            );
        }
        request.status = ViewportCloseStatus::Cleared;
        self.active_close_requests.remove(&binding);
    }

    fn reduce_ready_binding(
        &mut self,
        binding: ViewportBinding,
        actions: &mut Vec<ViewportLifecycleAction>,
    ) -> Result<(), ViewportCoordinatorError> {
        self.reduce_observed_effects(binding);
        if let Some(replacement) = self
            .restore_replacements
            .values_mut()
            .find(|replacement| replacement.binding == binding)
        {
            let _ = self.effects.mark_observed_applied(
                replacement.effect,
                binding,
                self.registry.inventory_generation(),
            );
            replacement.status = RestoreReplacementStatus::Ready;
        }
        let recovery_surface = self
            .pending_recoveries
            .iter()
            .find_map(|(surface, pending)| {
                (pending.replacement_binding == Some(binding)).then_some(*surface)
            });
        if let Some(surface) = recovery_surface {
            let (effect, status, recovery) = self
                .pending_recoveries
                .get(&surface)
                .map(|pending| (pending.replacement_effect, pending.status, pending.recovery))
                .ok_or(ViewportCoordinatorError::MissingRecoveryPending { surface })?;
            if matches!(
                status,
                RecoveryPendingStatus::ReplacementRequested { .. }
                    | RecoveryPendingStatus::ReplacementIndeterminate { .. }
                    | RecoveryPendingStatus::ReplacementFailed { .. }
            ) {
                if let Some(effect) = effect {
                    let _ = self.effects.mark_observed_applied(
                        effect,
                        binding,
                        self.registry.inventory_generation(),
                    );
                }
                self.recovery_plans.insert(surface, recovery);
                self.pending_recoveries.remove(&surface);
            }
        }
        let Some(saga_id) = self
            .create_sagas
            .iter()
            .find_map(|(id, saga)| (saga.binding == binding).then_some(*id))
        else {
            return Ok(());
        };
        let (status, effect, prepared) = {
            let saga = self
                .create_sagas
                .get(&saga_id)
                .ok_or(ViewportCoordinatorError::MissingCreateSaga { saga: saga_id })?;
            (saga.status, saga.effect, saga.prepared.clone())
        };
        match status {
            NativeCreateStatus::Requested | NativeCreateStatus::Indeterminate => {
                let _ = self.effects.mark_observed_applied(
                    effect,
                    binding,
                    self.registry.inventory_generation(),
                );
                self.create_sagas
                    .get_mut(&saga_id)
                    .ok_or(ViewportCoordinatorError::MissingCreateSaga { saga: saga_id })?
                    .status = NativeCreateStatus::GeometryReadyUncommitted;
                actions.push(ViewportLifecycleAction::CreateReady {
                    saga: saga_id,
                    prepared: Box::new(prepared),
                });
            }
            NativeCreateStatus::Cancelled => {
                self.request_create_compensation(saga_id)?;
            }
            NativeCreateStatus::GeometryReadyUncommitted
            | NativeCreateStatus::CommittedAwaitingVisibility { .. }
            | NativeCreateStatus::Committed { .. }
            | NativeCreateStatus::Compensating { .. } => {}
        }
        if matches!(
            status,
            NativeCreateStatus::CommittedAwaitingVisibility { .. }
        ) && self
            .registry
            .record(binding.surface())
            .is_some_and(|record| record.binding() == binding && record.is_routeable())
        {
            let show = match status {
                NativeCreateStatus::CommittedAwaitingVisibility { show } => show,
                _ => unreachable!("visibility transition checked above"),
            };
            let _ = self.effects.mark_observed_applied(
                show,
                binding,
                self.registry.inventory_generation(),
            );
            let focus = self.request_focus(binding.surface())?;
            self.create_sagas
                .get_mut(&saga_id)
                .ok_or(ViewportCoordinatorError::MissingCreateSaga { saga: saga_id })?
                .status = NativeCreateStatus::Committed {
                show: Some(show),
                focus,
            };
        }
        Ok(())
    }

    fn reduce_observed_effects(&mut self, binding: ViewportBinding) {
        let Some(coordinates) = self
            .registry
            .record(binding.surface())
            .filter(|record| record.binding() == binding)
            .and_then(ViewportRecord::coordinates)
        else {
            return;
        };
        let input_state = coordinates.input_state();
        let focused = coordinates.focused();
        let presentation = coordinates.presentation();
        let observed: Vec<EffectId> = self
            .effects
            .records()
            .filter_map(|(effect, record)| match record.request().effect() {
                PlatformEffect::SetPointerPassthrough {
                    binding: target,
                    enabled,
                } if *target == binding
                    && ((*enabled && input_state == Some(WindowInputState::PassThrough))
                        || (!*enabled && input_state == Some(WindowInputState::ReceivesInput))) =>
                {
                    Some(effect)
                }
                PlatformEffect::RequestFocus { binding: target }
                    if *target == binding && focused == Some(true) =>
                {
                    Some(effect)
                }
                PlatformEffect::ShowWindow { binding: target }
                    if *target == binding
                        && presentation
                            == Some(crate::platform::WindowPresentationState::Visible) =>
                {
                    Some(effect)
                }
                _ => None,
            })
            .collect();
        for effect in observed {
            let _ = self.effects.mark_observed_applied(
                effect,
                binding,
                self.registry.inventory_generation(),
            );
        }
    }

    fn reduce_destroyed_binding(
        &mut self,
        binding: ViewportBinding,
        actions: &mut Vec<ViewportLifecycleAction>,
    ) {
        if let Some(surface) = self
            .restore_replacements
            .iter()
            .find_map(|(surface, replacement)| (replacement.binding == binding).then_some(*surface))
        {
            if let Some(replacement) = self.restore_replacements.remove(&surface) {
                let _ = self.effects.mark_destroyed(
                    replacement.effect,
                    binding,
                    self.registry.inventory_generation(),
                );
            }
            self.push_direct_destruction(binding, actions);
            return;
        }
        if self.reduce_replacement_destroyed(binding) {
            return;
        }
        if self.reduce_close_destroyed_binding(binding, actions) {
            return;
        }
        let saga_id = self
            .create_sagas
            .iter()
            .find_map(|(id, saga)| (saga.binding == binding).then_some(*id));
        let Some(saga_id) = saga_id else {
            self.push_direct_destruction(binding, actions);
            return;
        };
        let status = self
            .create_sagas
            .get(&saga_id)
            .map_or(NativeCreateStatus::Cancelled, |saga| saga.status);
        match status {
            NativeCreateStatus::CommittedAwaitingVisibility { .. }
            | NativeCreateStatus::Committed { .. } => {
                self.push_direct_destruction(binding, actions);
            }
            NativeCreateStatus::Compensating { effect } => {
                let _ = self.effects.mark_destroyed(
                    effect,
                    binding,
                    self.registry.inventory_generation(),
                );
                if let Some(saga) = self.create_sagas.get_mut(&saga_id) {
                    saga.status = NativeCreateStatus::Cancelled;
                }
                let _ = self.registry.remove_missing(binding);
            }
            NativeCreateStatus::Requested
            | NativeCreateStatus::Indeterminate
            | NativeCreateStatus::GeometryReadyUncommitted
            | NativeCreateStatus::Cancelled => {
                if let Some(saga) = self.create_sagas.get_mut(&saga_id) {
                    saga.status = NativeCreateStatus::Cancelled;
                }
                let _ = self.registry.remove_missing(binding);
            }
        }
    }

    fn reduce_replacement_destroyed(&mut self, binding: ViewportBinding) -> bool {
        let surface = self
            .pending_recoveries
            .iter()
            .find_map(|(surface, pending)| {
                (pending.replacement_binding == Some(binding)).then_some(*surface)
            });
        let Some(surface) = surface else {
            return false;
        };
        let Some(pending) = self.pending_recoveries.get_mut(&surface) else {
            return true;
        };
        if let RecoveryPendingStatus::CompensatingReplacement { effect } = pending.status {
            let _ =
                self.effects
                    .mark_destroyed(effect, binding, self.registry.inventory_generation());
            let _ = self.registry.remove_missing(binding);
            self.pending_recoveries.remove(&surface);
            return true;
        }
        if let Some(effect) = pending.replacement_effect {
            let _ =
                self.effects
                    .mark_destroyed(effect, binding, self.registry.inventory_generation());
        }
        let _ = self.registry.remove_missing(binding);
        pending.replacement_binding = None;
        pending.replacement_effect = None;
        pending.status = RecoveryPendingStatus::AwaitingRecoveryHost;
        true
    }

    fn reduce_close_destroyed_binding(
        &mut self,
        binding: ViewportBinding,
        actions: &mut Vec<ViewportLifecycleAction>,
    ) -> bool {
        let Some(request_id) = self.active_close_requests.remove(&binding) else {
            return false;
        };
        let Some(request) = self.close_requests.get_mut(&request_id) else {
            self.push_direct_destruction(binding, actions);
            return true;
        };
        let plan = request.plan.clone();
        let recovery = request
            .recovery
            .or_else(|| self.recovery_plans.get(&binding.surface()).copied());
        let effect = request.effect;
        request.status = ViewportCloseStatus::Destroyed;

        if let Some(plan) = plan {
            if let Some(effect) = effect {
                let _ = self.effects.mark_destroyed(
                    effect,
                    binding,
                    self.registry.inventory_generation(),
                );
            }
            actions.push(ViewportLifecycleAction::SurfaceDestroyed {
                binding,
                resolution: ViewportDestructionResolution::Accepted {
                    request: request_id,
                    plan,
                },
            });
        } else if let Some(recovery) = recovery {
            actions.push(ViewportLifecycleAction::SurfaceDestroyed {
                binding,
                resolution: ViewportDestructionResolution::Recover { recovery },
            });
        } else {
            actions.push(ViewportLifecycleAction::SurfaceDestroyed {
                binding,
                resolution: ViewportDestructionResolution::Unplanned,
            });
        }
        true
    }

    fn push_direct_destruction(
        &self,
        binding: ViewportBinding,
        actions: &mut Vec<ViewportLifecycleAction>,
    ) {
        if let Some(recovery) = self.recovery_plans.get(&binding.surface()).copied() {
            actions.push(ViewportLifecycleAction::SurfaceDestroyed {
                binding,
                resolution: ViewportDestructionResolution::Recover { recovery },
            });
        } else {
            actions.push(ViewportLifecycleAction::SurfaceDestroyed {
                binding,
                resolution: ViewportDestructionResolution::Unplanned,
            });
        }
    }

    fn request_create_compensation(
        &mut self,
        saga_id: NativeCreateSagaId,
    ) -> Result<EffectId, ViewportCoordinatorError> {
        let (binding, create_effect, status) = {
            let saga = self
                .create_sagas
                .get(&saga_id)
                .ok_or(ViewportCoordinatorError::MissingCreateSaga { saga: saga_id })?;
            (saga.binding, saga.effect, saga.status)
        };
        if let NativeCreateStatus::Compensating { effect } = status {
            return Ok(effect);
        }
        let effect = self
            .effects
            .request(PlatformEffect::CompensatingClose {
                binding,
                compensates: create_effect,
            })
            .map_err(ViewportCoordinatorError::Effect)?;
        self.registry
            .mark_awaiting_destroyed(binding)
            .map_err(ViewportCoordinatorError::Registry)?;
        self.create_sagas
            .get_mut(&saga_id)
            .ok_or(ViewportCoordinatorError::MissingCreateSaga { saga: saga_id })?
            .status = NativeCreateStatus::Compensating { effect };
        Ok(effect)
    }

    pub(crate) fn complete_native_create(
        &mut self,
        saga_id: NativeCreateSagaId,
    ) -> Result<(), ViewportCoordinatorError> {
        let saga = self
            .create_sagas
            .get(&saga_id)
            .ok_or(ViewportCoordinatorError::MissingCreateSaga { saga: saga_id })?;
        if saga.status != NativeCreateStatus::GeometryReadyUncommitted {
            return Err(ViewportCoordinatorError::CreateSagaNotReady { saga: saga_id });
        }
        let binding = saga.binding;
        let surface = binding.surface();
        let recovery = ContainedRecoveryPlan::from_proposal(saga.prepared.proposal().recovery());
        let recovery = self
            .project_recovery_plan(surface, recovery)
            .unwrap_or(recovery);
        let routeable = self
            .registry
            .record(surface)
            .is_some_and(|record| record.binding() == binding && record.is_routeable());
        let (show, focus) = if routeable {
            (None, self.request_focus(surface)?)
        } else {
            let show = self
                .effects
                .request(PlatformEffect::ShowWindow { binding })
                .map_err(ViewportCoordinatorError::Effect)?;
            (Some(show), None)
        };
        self.create_sagas
            .get_mut(&saga_id)
            .ok_or(ViewportCoordinatorError::MissingCreateSaga { saga: saga_id })?
            .status = match show {
            Some(show) => NativeCreateStatus::CommittedAwaitingVisibility { show },
            None => NativeCreateStatus::Committed { show, focus },
        };
        self.recovery_plans.insert(surface, recovery);
        Ok(())
    }

    pub(crate) fn reject_native_create(
        &mut self,
        saga_id: NativeCreateSagaId,
    ) -> Result<EffectId, ViewportCoordinatorError> {
        self.request_create_compensation(saga_id)
    }

    pub(crate) fn cancel_native_create(
        &mut self,
        saga_id: NativeCreateSagaId,
    ) -> Result<Option<EffectId>, ViewportCoordinatorError> {
        let saga = self
            .create_sagas
            .get_mut(&saga_id)
            .ok_or(ViewportCoordinatorError::MissingCreateSaga { saga: saga_id })?;
        if saga.status.topology_committed() {
            return Err(ViewportCoordinatorError::CreateSagaAlreadyCommitted { saga: saga_id });
        }
        if matches!(
            saga.status,
            NativeCreateStatus::GeometryReadyUncommitted | NativeCreateStatus::Compensating { .. }
        ) {
            return self.request_create_compensation(saga_id).map(Some);
        }
        saga.status = NativeCreateStatus::Cancelled;
        Ok(None)
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
        if !matches!(phase, EffectPhase::DispatchFailed(_)) {
            return Err(ViewportCoordinatorError::CleanupEffectNotRetryable {
                effect: failed_effect,
                phase,
            });
        }

        let mut candidate = self.clone();
        let retry = if let Some(retry) = candidate.retry_create_cleanup(failed_effect)? {
            retry
        } else if let Some(retry) = candidate.retry_retired_cleanup(failed_effect)? {
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

    fn retry_create_cleanup(
        &mut self,
        failed_effect: EffectId,
    ) -> Result<Option<EffectId>, ViewportCoordinatorError> {
        let Some(saga_id) = self.create_sagas.iter().find_map(|(saga_id, saga)| {
            matches!(
                saga.status,
                NativeCreateStatus::Compensating { effect } if effect == failed_effect
            )
            .then_some(*saga_id)
        }) else {
            return Ok(None);
        };
        let (binding, compensates) = self
            .create_sagas
            .get(&saga_id)
            .map(|saga| (saga.binding, saga.effect))
            .ok_or(ViewportCoordinatorError::MissingCreateSaga { saga: saga_id })?;
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
        self.create_sagas
            .get_mut(&saga_id)
            .ok_or(ViewportCoordinatorError::MissingCreateSaga { saga: saga_id })?
            .status = NativeCreateStatus::Compensating { effect: retry };
        Ok(Some(retry))
    }

    fn retry_retired_cleanup(
        &mut self,
        failed_effect: EffectId,
    ) -> Result<Option<EffectId>, ViewportCoordinatorError> {
        let Some(token) = self.retired_viewports.iter().find_map(|(token, retired)| {
            matches!(
                retired.status,
                RetiredViewportStatus::CleanupFailed { effect } if effect == failed_effect
            )
            .then_some(*token)
        }) else {
            return Ok(None);
        };
        let (binding, cleanup, observed) = self
            .retired_viewports
            .get(&token)
            .map(|retired| (retired.binding, retired.cleanup, retired.observed))
            .ok_or(ViewportCoordinatorError::MissingRetiredViewport { token })?;
        if !observed {
            return Err(ViewportCoordinatorError::CleanupWindowNotObserved {
                effect: failed_effect,
            });
        }
        let retry = self.request_retired_cleanup(binding, cleanup)?;
        self.retired_viewports
            .get_mut(&token)
            .ok_or(ViewportCoordinatorError::MissingRetiredViewport { token })?
            .status = RetiredViewportStatus::CleanupRequested { effect: retry };
        Ok(Some(retry))
    }

    fn retry_replacement_cleanup(
        &mut self,
        failed_effect: EffectId,
    ) -> Result<Option<EffectId>, ViewportCoordinatorError> {
        let Some(surface) = self
            .pending_recoveries
            .iter()
            .find_map(|(surface, pending)| {
                matches!(
                    pending.status,
                    RecoveryPendingStatus::CompensatingReplacement { effect }
                        if effect == failed_effect
                )
                .then_some(*surface)
            })
        else {
            return Ok(None);
        };
        let (binding, compensates) = self
            .pending_recoveries
            .get(&surface)
            .and_then(|pending| Some((pending.replacement_binding?, pending.replacement_effect?)))
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
        self.pending_recoveries
            .get_mut(&surface)
            .ok_or(ViewportCoordinatorError::MissingRecoveryPending { surface })?
            .status = RecoveryPendingStatus::CompensatingReplacement { effect: retry };
        Ok(Some(retry))
    }

    pub(crate) fn report_effect(
        &mut self,
        current_epoch: WorkspaceEpoch,
        result: EffectResult,
    ) -> EffectTransition {
        let effect = result.effect();
        let transition = self.effects.report(current_epoch, result);
        if transition != EffectTransition::Applied {
            return transition;
        }
        let status = match result.result() {
            EffectDispatchResult::Indeterminate(_) => NativeCreateStatus::Indeterminate,
            EffectDispatchResult::DispatchFailed(_) | EffectDispatchResult::Unsupported(_) => {
                NativeCreateStatus::Cancelled
            }
        };
        if let Some(saga) = self
            .create_sagas
            .values_mut()
            .find(|saga| saga.effect == effect)
        {
            saga.status = status;
        }
        let close_request = self
            .close_requests
            .iter()
            .find_map(|(request, record)| (record.effect == Some(effect)).then_some(*request));
        if let Some(request_id) = close_request {
            let close_status = match result.result() {
                EffectDispatchResult::Indeterminate(_) => {
                    ViewportCloseStatus::Indeterminate { effect }
                }
                EffectDispatchResult::DispatchFailed(_) | EffectDispatchResult::Unsupported(_) => {
                    ViewportCloseStatus::EffectFailed { effect }
                }
            };
            if let Some(request) = self.close_requests.get_mut(&request_id) {
                let binding = request.binding;
                let accepted = request.plan.is_some();
                request.status = close_status;
                if accepted && matches!(close_status, ViewportCloseStatus::EffectFailed { .. }) {
                    let _ = self.registry.resume_after_failed_close(binding);
                }
            }
        }
        if let Some(retired) = self.retired_viewports.values_mut().find(|retired| {
            matches!(
                retired.status,
                RetiredViewportStatus::CleanupRequested { effect: current }
                    if current == effect
            )
        }) {
            retired.status = match result.result() {
                EffectDispatchResult::Indeterminate(_) => {
                    RetiredViewportStatus::CleanupIndeterminate { effect }
                }
                EffectDispatchResult::DispatchFailed(_) | EffectDispatchResult::Unsupported(_) => {
                    RetiredViewportStatus::CleanupFailed { effect }
                }
            };
        }
        let recovery_surface = self
            .pending_recoveries
            .iter()
            .find_map(|(surface, pending)| {
                (pending.replacement_effect == Some(effect)).then_some(*surface)
            });
        if let Some(surface) = recovery_surface
            && let Some(pending) = self.pending_recoveries.get_mut(&surface)
        {
            pending.status = match result.result() {
                EffectDispatchResult::Indeterminate(_) => {
                    RecoveryPendingStatus::ReplacementIndeterminate { effect }
                }
                EffectDispatchResult::DispatchFailed(_) | EffectDispatchResult::Unsupported(_) => {
                    if let Some(binding) = pending.replacement_binding
                        && self.registry.discard_unobserved(binding).is_ok()
                    {
                        pending.replacement_binding = None;
                    }
                    RecoveryPendingStatus::ReplacementFailed { effect }
                }
            };
        }
        if let Some(replacement) = self
            .restore_replacements
            .values_mut()
            .find(|replacement| replacement.effect == effect)
        {
            replacement.status = match result.result() {
                EffectDispatchResult::Indeterminate(_) => {
                    RestoreReplacementStatus::Indeterminate { effect }
                }
                EffectDispatchResult::DispatchFailed(_) | EffectDispatchResult::Unsupported(_) => {
                    let _ = self.registry.discard_unobserved(replacement.binding);
                    RestoreReplacementStatus::Failed { effect }
                }
            };
        }
        transition
    }

    pub(crate) fn take_new_effects(&mut self) -> Vec<EffectRequest> {
        self.effects.take_new_requests()
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

    #[must_use]
    pub fn native_create_saga(&self, saga: NativeCreateSagaId) -> Option<&NativeCreateSaga> {
        self.create_sagas.get(&saga)
    }

    pub fn native_create_sagas(
        &self,
    ) -> impl Iterator<Item = (NativeCreateSagaId, &NativeCreateSaga)> {
        self.create_sagas.iter().map(|(id, saga)| (*id, saga))
    }

    #[must_use]
    pub fn viewport_close_request(
        &self,
        request: ViewportCloseRequestId,
    ) -> Option<&ViewportCloseRequest> {
        self.close_requests.get(&request)
    }

    pub fn viewport_close_requests(
        &self,
    ) -> impl Iterator<Item = (ViewportCloseRequestId, &ViewportCloseRequest)> {
        self.close_requests
            .iter()
            .map(|(id, request)| (*id, request))
    }

    pub fn retired_viewports(&self) -> impl Iterator<Item = (WindowToken, &RetiredViewport)> {
        self.retired_viewports
            .iter()
            .map(|(token, viewport)| (*token, viewport))
    }

    #[must_use]
    pub fn recovery_pending(&self, surface: SurfaceId) -> Option<&RecoveryPending> {
        self.pending_recoveries.get(&surface)
    }

    pub fn pending_recoveries(&self) -> impl Iterator<Item = (SurfaceId, &RecoveryPending)> {
        self.pending_recoveries
            .iter()
            .map(|(surface, pending)| (*surface, pending))
    }

    #[must_use]
    pub fn restore_replacement(&self, surface: SurfaceId) -> Option<RestoreReplacement> {
        self.restore_replacements.get(&surface).copied()
    }

    #[must_use]
    pub fn viewport(&self, surface: SurfaceId) -> Option<&ViewportRecord> {
        self.registry.record(surface)
    }

    #[must_use]
    pub fn route(&self, pointer: PointerId) -> Option<&ViewportRouteProof> {
        self.routes.proof(pointer)
    }

    #[must_use]
    pub fn route_is_current(&self, proof: &ViewportRouteProof) -> bool {
        self.routes.is_current(proof)
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
                self.work_area_generation == proof.work_area_generation()
                    && self.work_areas.contains_key(&proof.work_area())
                    && proof.route().capability_generation() == self.capability_generation
                    && proof.route().inventory_generation() == self.registry.inventory_generation()
                    && proof.route().route_generation() == self.routes.generation()
            }
        }
    }

    pub(crate) fn tear_off_placement(
        &self,
        pointer: PointerId,
        request: TearOffPlacementRequest,
    ) -> Result<TearOffPlacementProof, ViewportCoordinatorError> {
        let route = self
            .routes
            .proof(pointer)
            .filter(|proof| self.routes.is_current(proof))
            .ok_or(ViewportCoordinatorError::StaleRoute { pointer })?;
        let work_area = self.work_areas.get(&request.work_area()).copied().ok_or(
            ViewportCoordinatorError::TearOffPlacement(
                TearOffPlacementUnavailable::UnknownWorkArea(request.work_area()),
            ),
        )?;
        crate::coordinates::solve_tear_off_placement(
            route,
            work_area,
            self.work_area_generation,
            request,
        )
        .map_err(ViewportCoordinatorError::TearOffPlacement)
    }

    pub(crate) fn begin_drag_routing(
        &mut self,
        pointer: PointerId,
        surface: SurfaceId,
    ) -> Result<Option<EffectId>, ViewportCoordinatorError> {
        if !self
            .capabilities
            .pointer_hit_test_observation()
            .is_supported()
        {
            return Ok(None);
        }
        let Some((binding, original)) = self
            .registry
            .record(surface)
            .filter(|record| record.is_routeable())
            .and_then(|record| {
                record
                    .coordinates()
                    .and_then(|coordinates| coordinates.input_state())
                    .map(|input| (record.binding(), input))
            })
        else {
            return Ok(None);
        };
        if self.drag_sources.get(&pointer) == Some(&binding) {
            return Ok(None);
        }
        let mut candidate = self.clone();
        let restored = candidate.end_drag_routing(pointer)?;
        if let Some(lease) = candidate.pointer_hit_test_leases.get_mut(&binding) {
            lease.holders.insert(pointer);
            candidate.drag_sources.insert(pointer, binding);
            candidate.routes.clear();
            *self = candidate;
            return Ok(restored);
        }

        let (changed_by_us, effect) = match original {
            WindowInputState::PassThrough => (false, None),
            WindowInputState::ReceivesInput => {
                if !candidate
                    .capabilities
                    .pointer_hit_test_control()
                    .is_supported()
                {
                    *self = candidate;
                    return Ok(restored);
                }
                let effect = candidate
                    .effects
                    .request(PlatformEffect::SetPointerPassthrough {
                        binding,
                        enabled: true,
                    })
                    .map_err(ViewportCoordinatorError::Effect)?;
                (true, Some(effect))
            }
        };
        candidate.pointer_hit_test_leases.insert(
            binding,
            PointerHitTestLease {
                original,
                changed_by_us,
                holders: BTreeSet::from([pointer]),
            },
        );
        candidate.drag_sources.insert(pointer, binding);
        candidate.routes.clear();
        *self = candidate;
        Ok(effect.or(restored))
    }

    #[must_use]
    pub(crate) fn drag_source(&self, pointer: PointerId) -> Option<ViewportBinding> {
        self.drag_sources.get(&pointer).copied()
    }

    pub(crate) fn end_drag_routing(
        &mut self,
        pointer: PointerId,
    ) -> Result<Option<EffectId>, ViewportCoordinatorError> {
        let Some(binding) = self.drag_sources.get(&pointer).copied() else {
            return Ok(None);
        };
        let mut candidate = self.clone();
        candidate.drag_sources.remove(&pointer);
        let Some(lease) = candidate.pointer_hit_test_leases.get_mut(&binding) else {
            return Err(ViewportCoordinatorError::PointerHitTestLeaseMissing { binding });
        };
        lease.holders.remove(&pointer);
        let last_holder = lease.holders.is_empty();
        let restore =
            last_holder && lease.changed_by_us && lease.original == WindowInputState::ReceivesInput;
        if last_holder {
            candidate.pointer_hit_test_leases.remove(&binding);
        }
        let effect = if restore {
            Some(
                candidate
                    .effects
                    .request(PlatformEffect::SetPointerPassthrough {
                        binding,
                        enabled: false,
                    })
                    .map_err(ViewportCoordinatorError::Effect)?,
            )
        } else {
            None
        };
        candidate.routes.clear();
        *self = candidate;
        Ok(effect)
    }

    pub(crate) fn end_all_drag_routing(&mut self) -> Result<(), ViewportCoordinatorError> {
        let pointers: Vec<PointerId> = self.drag_sources.keys().copied().collect();
        let mut candidate = self.clone();
        for pointer in pointers {
            let _ = candidate.end_drag_routing(pointer)?;
        }
        *self = candidate;
        Ok(())
    }

    pub(crate) fn request_focus(
        &mut self,
        surface: SurfaceId,
    ) -> Result<Option<EffectId>, ViewportCoordinatorError> {
        if !self.capabilities.window_focus().is_supported() {
            return Ok(None);
        }
        let Some(binding) = self
            .registry
            .record(surface)
            .filter(|record| record.is_routeable())
            .map(ViewportRecord::binding)
        else {
            return Ok(None);
        };
        self.effects
            .request(PlatformEffect::RequestFocus { binding })
            .map(Some)
            .map_err(ViewportCoordinatorError::Effect)
    }

    pub(crate) fn complete_destroyed_surface(
        &mut self,
        binding: ViewportBinding,
    ) -> Result<(), ViewportCoordinatorError> {
        self.registry
            .remove_missing(binding)
            .map_err(ViewportCoordinatorError::Registry)?;
        self.recovery_plans.remove(&binding.surface());
        Ok(())
    }

    pub(crate) fn defer_destroyed_surface_recovery(
        &mut self,
        binding: ViewportBinding,
        recovery: ContainedRecoveryPlan,
    ) -> Result<(), ViewportCoordinatorError> {
        if self.pending_recoveries.contains_key(&binding.surface()) {
            return Ok(());
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
        if !matches!(record.lifecycle(), ViewportLifecycle::Missing) {
            return Err(ViewportCoordinatorError::DestroyedSurfaceStillObserved { binding });
        }
        let role = record.role();
        let placement = record.coordinates().map(|coordinates| {
            coordinates
                .outer_bounds()
                .unwrap_or_else(|| coordinates.content_bounds())
        });
        let mut candidate = self.clone();
        candidate
            .registry
            .remove_missing(binding)
            .map_err(ViewportCoordinatorError::Registry)?;
        let mut pending = RecoveryPending {
            destroyed_binding: binding,
            recovery,
            replacement_binding: None,
            replacement_effect: None,
            status: RecoveryPendingStatus::AwaitingRecoveryHost,
        };
        if let Some(placement) = placement
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
            pending.replacement_binding = Some(replacement);
            pending.replacement_effect = Some(effect);
            pending.status = RecoveryPendingStatus::ReplacementRequested { effect };
        }
        candidate
            .pending_recoveries
            .insert(binding.surface(), pending);
        *self = candidate;
        Ok(())
    }

    pub(crate) fn complete_pending_recovery(
        &mut self,
        destroyed_surface: SurfaceId,
    ) -> Result<(), ViewportCoordinatorError> {
        let pending = self
            .pending_recoveries
            .get(&destroyed_surface)
            .cloned()
            .ok_or(ViewportCoordinatorError::MissingRecoveryPending {
                surface: destroyed_surface,
            })?;
        let mut candidate = self.clone();
        candidate.recovery_plans.remove(&destroyed_surface);
        let Some(replacement) = pending.replacement_binding else {
            candidate.pending_recoveries.remove(&destroyed_surface);
            *self = candidate;
            return Ok(());
        };
        let replacement_effect = pending.replacement_effect.ok_or(
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
            .pending_recoveries
            .get_mut(&destroyed_surface)
            .ok_or(ViewportCoordinatorError::MissingRecoveryPending {
                surface: destroyed_surface,
            })?
            .status = RecoveryPendingStatus::CompensatingReplacement { effect };
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

    #[cfg(test)]
    pub(crate) fn exhaust_close_request_ids(&mut self) {
        self.last_close_request = ViewportCloseRequestId::new(u64::MAX);
    }
}

const fn retired_effect(status: RetiredViewportStatus) -> Option<EffectId> {
    match status {
        RetiredViewportStatus::CleanupRequested { effect }
        | RetiredViewportStatus::CleanupIndeterminate { effect }
        | RetiredViewportStatus::CleanupFailed { effect } => Some(effect),
        RetiredViewportStatus::AwaitingAppearance => None,
    }
}

/// Fatal platform coordinator transition failure.
#[derive(Debug, Clone, PartialEq, Error)]
pub enum ViewportCoordinatorError {
    #[error("platform capability generation is exhausted")]
    CapabilityGenerationExhausted,
    #[error("platform work-area generation is exhausted")]
    WorkAreaGenerationExhausted,
    #[error(transparent)]
    Registry(ViewportRegistryError),
    #[error(transparent)]
    Route(ViewportRouteError),
    #[error(transparent)]
    Effect(EffectLedgerError),
    #[error("drag source binding is stale or not ready: {binding:?}")]
    StaleDragSource { binding: ViewportBinding },
    #[error("no current route exists for pointer {pointer:?}")]
    StaleRoute { pointer: PointerId },
    #[error(transparent)]
    TearOffPlacement(TearOffPlacementUnavailable),
    #[error("pointer hit-test lease is missing for drag source {binding:?}")]
    PointerHitTestLeaseMissing { binding: ViewportBinding },
    #[error("workspace does not contain logical surface {surface:?}")]
    MissingWorkspaceSurface { surface: SurfaceId },
    #[error("docking-owned child surface {surface:?} requires an explicit whole-root recovery")]
    ChildRecoveryRequired { surface: SurfaceId },
    #[error("viewport coordinator epoch mismatch: expected {expected:?}, got {actual:?}")]
    WorkspaceEpochMismatch {
        expected: WorkspaceEpoch,
        actual: WorkspaceEpoch,
    },
    #[error("retired viewport token is missing: {token:?}")]
    MissingRetiredViewport { token: WindowToken },
    #[error("retired viewport token remains reserved until authoritative absence: {token:?}")]
    RetiredTokenReserved { token: WindowToken },
    #[error("destroyed viewport binding is still observed: {binding:?}")]
    DestroyedSurfaceStillObserved { binding: ViewportBinding },
    #[error("native recovery is not pending for surface {surface:?}")]
    MissingRecoveryPending { surface: SurfaceId },
    #[error("native replacement effect is missing for surface {surface:?}")]
    MissingReplacementEffect { surface: SurfaceId },
    #[error("native create saga identity is exhausted")]
    NativeCreateSagaExhausted,
    #[error("viewport close-request identity is exhausted")]
    CloseRequestIdExhausted,
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
    #[error("native create saga {saga:?} has already committed")]
    CreateSagaAlreadyCommitted { saga: NativeCreateSagaId },
    #[error("platform effect {effect:?} does not belong to a retryable cleanup")]
    MissingCleanupEffect { effect: EffectId },
    #[error("cleanup effect {effect:?} cannot be retried from phase {phase:?}")]
    CleanupEffectNotRetryable {
        effect: EffectId,
        phase: EffectPhase,
    },
    #[error("cleanup effect {effect:?} has no authoritatively observed window")]
    CleanupWindowNotObserved { effect: EffectId },
    #[error("viewport close request {request:?} does not exist")]
    MissingCloseRequest { request: ViewportCloseRequestId },
    #[error("viewport close request {request:?} was already decided: {status:?}")]
    CloseRequestAlreadyDecided {
        request: ViewportCloseRequestId,
        status: ViewportCloseStatus,
    },
    #[error("viewport close request {request:?} changed its recovery root")]
    CloseRecoveryRootMismatch { request: ViewportCloseRequestId },
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::effect::EffectPhase;
    use crate::geometry::{LogicalSize, PhysicalRect, ScaleFactor};
    use crate::ids::{FloatingPresentationId, RootId};
    use crate::intent::{Authority, ContainedPlacementProof, ContainedTearOffProposal};
    use crate::platform::{ObservedWindow, WindowInputState, WindowPresentationState};
    use crate::scene::{SceneGeneration, SceneStamp};
    use crate::transition::WorkspaceVersion;

    fn observed_window(token: WindowToken, close_requested: bool) -> ObservedWindow {
        ObservedWindow::new(token)
            .with_content_bounds(Authority::Known(
                PhysicalRect::new(10.0, 20.0, 300.0, 200.0)
                    .expect("test content bounds must be valid"),
            ))
            .with_outer_bounds(Authority::Known(
                PhysicalRect::new(5.0, 0.0, 310.0, 225.0).expect("test outer bounds must be valid"),
            ))
            .with_scale_factor(Authority::Known(
                ScaleFactor::new(1.0).expect("test scale must be valid"),
            ))
            .with_input_state(Authority::Known(WindowInputState::ReceivesInput))
            .with_presentation(Authority::Known(WindowPresentationState::Visible))
            .with_close_requested(Authority::Known(close_requested))
    }

    fn snapshot(windows: Vec<ObservedWindow>) -> PlatformSnapshot {
        let mut capabilities = PlatformCapabilities::default();
        capabilities.set_authoritative_inventory(PlatformCapability::Supported);
        PlatformSnapshot::new(capabilities, windows, Vec::new(), Vec::new())
            .expect("test snapshot must be valid")
    }

    fn routing_coordinator(
        input: WindowInputState,
        presentation: WindowPresentationState,
        control: PlatformCapability,
    ) -> (ViewportCoordinator, ViewportBinding) {
        let token = WindowToken::new(41);
        let surface = SurfaceId::new(42);
        let mut coordinator = ViewportCoordinator::default();
        let binding = coordinator
            .register_existing(
                WorkspaceEpoch::default(),
                surface,
                token,
                ViewportRole::Root,
                None,
            )
            .expect("test viewport must register");
        let window = observed_window(token, false)
            .with_input_state(Authority::Known(input))
            .with_presentation(Authority::Known(presentation));
        let mut capabilities = PlatformCapabilities::default();
        capabilities.set_authoritative_inventory(PlatformCapability::Supported);
        capabilities.set_pointer_hit_test_observation(PlatformCapability::Supported);
        capabilities.set_pointer_hit_test_control(control);
        let facts = PlatformSnapshot::new(capabilities, vec![window], Vec::new(), Vec::new())
            .expect("test routing snapshot must be valid");
        coordinator
            .publish_snapshot(&facts)
            .expect("test routing facts must publish");
        (coordinator, binding)
    }

    fn recovery(root: u64) -> ContainedTearOffProposal {
        let surface = SurfaceId::new(99);
        let requested =
            LogicalRect::new(30.0, 40.0, 300.0, 200.0).expect("test recovery bounds must be valid");
        let bounds =
            LogicalRect::new(0.0, 0.0, 1_000.0, 800.0).expect("test surface bounds must be valid");
        let minimum = LogicalSize::new(0.0, 0.0).expect("test minimum size must be valid");
        let placement = ContainedPlacementProof::new(
            SceneStamp::new(WorkspaceVersion::default(), SceneGeneration::new(1)),
            surface,
            requested,
            minimum,
            bounds,
            requested,
        );
        ContainedTearOffProposal::new(
            RootId::new(root),
            FloatingPresentationId::new(root),
            placement,
            7,
        )
    }

    fn register(
        coordinator: &mut ViewportCoordinator,
        surface: u64,
        token: u64,
        role: ViewportRole,
    ) -> ViewportBinding {
        coordinator
            .register_existing(
                WorkspaceEpoch::new(1),
                SurfaceId::new(surface),
                WindowToken::new(token),
                role,
                (role == ViewportRole::Child).then(|| recovery(surface).into()),
            )
            .expect("test viewport must register")
    }

    #[test]
    fn capability_generation_exhaustion_rolls_back_the_complete_snapshot() {
        let mut coordinator = ViewportCoordinator::default();
        coordinator.exhaust_capability_generation();
        let before = coordinator.clone();
        let snapshot = PlatformSnapshot::new(
            PlatformCapabilities::default(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
        )
        .expect("empty snapshot must be valid");

        assert_eq!(
            coordinator.publish_snapshot(&snapshot),
            Err(ViewportCoordinatorError::CapabilityGenerationExhausted)
        );
        assert_eq!(coordinator, before);
    }

    #[test]
    fn work_area_generation_exhaustion_rolls_back_the_complete_snapshot() {
        let mut coordinator = ViewportCoordinator::default();
        coordinator.exhaust_work_area_generation();
        let before = coordinator.clone();
        let mut capabilities = PlatformCapabilities::default();
        capabilities.set_work_area(PlatformCapability::Supported);
        let snapshot = PlatformSnapshot::new(
            capabilities,
            Vec::new(),
            Vec::new(),
            vec![ObservedWorkArea::new(
                WorkAreaToken::new(1),
                PhysicalRect::new(-1920.0, 0.0, 1920.0, 1080.0)
                    .expect("test work area must be valid"),
                ScaleFactor::new(1.0).expect("test work-area scale must be valid"),
            )],
        )
        .expect("work-area snapshot must be valid");

        assert_eq!(
            coordinator.publish_snapshot(&snapshot),
            Err(ViewportCoordinatorError::WorkAreaGenerationExhausted)
        );
        assert_eq!(coordinator, before);
    }

    #[test]
    fn focus_request_is_applied_only_after_the_exact_binding_is_observed_focused() {
        let mut coordinator = ViewportCoordinator::default();
        let binding = register(&mut coordinator, 7, 70, ViewportRole::Child);
        let mut capabilities = PlatformCapabilities::default();
        capabilities.set_authoritative_inventory(PlatformCapability::Supported);
        capabilities.set_window_focus(PlatformCapability::Supported);
        let platform_snapshot = |focused| {
            PlatformSnapshot::new(
                capabilities.clone(),
                vec![
                    observed_window(binding.token(), false).with_focused(Authority::Known(focused)),
                ],
                Vec::new(),
                Vec::new(),
            )
            .expect("focus snapshot must be valid")
        };
        coordinator
            .publish_snapshot(&platform_snapshot(false))
            .expect("initial unfocused observation must publish");

        let effect = coordinator
            .request_focus(binding.surface())
            .expect("focus request must enter the ledger")
            .expect("supported focus must produce an effect");
        assert!(matches!(
            coordinator.take_new_effects().as_slice(),
            [request]
                if request.id() == effect
                    && matches!(
                        request.effect(),
                        PlatformEffect::RequestFocus { binding: actual }
                            if *actual == binding
                    )
        ));
        coordinator
            .publish_snapshot(&platform_snapshot(false))
            .expect("unfocused observation must remain pending");
        assert_eq!(
            coordinator
                .effects()
                .record(effect)
                .expect("focus effect must remain queryable")
                .phase(),
            EffectPhase::Requested
        );

        coordinator
            .publish_snapshot(&platform_snapshot(true))
            .expect("focused observation must publish");
        assert!(matches!(
            coordinator
                .effects()
                .record(effect)
                .expect("focus effect must remain queryable")
                .phase(),
            EffectPhase::ObservedApplied { .. }
        ));
    }

    #[test]
    fn root_and_child_veto_emit_distinct_effects_once() {
        for (role, expected_root_effect) in
            [(ViewportRole::Root, true), (ViewportRole::Child, false)]
        {
            let mut coordinator = ViewportCoordinator::default();
            let binding = register(&mut coordinator, 1, 10, role);
            let transition = coordinator
                .publish_snapshot(&snapshot(vec![observed_window(binding.token(), true)]))
                .expect("close edge must publish");
            let [request] = transition.close_requests() else {
                panic!("one close request must be created");
            };

            let effect = coordinator
                .decide_viewport_close(*request, ViewportCloseDecision::Veto)
                .expect("veto must enter the effect ledger")
                .expect("veto must have an effect");
            let emitted = coordinator.take_new_effects();
            assert_eq!(emitted.len(), 1);
            assert_eq!(emitted[0].id(), effect);
            assert!(match emitted[0].effect() {
                PlatformEffect::CancelRootClose { binding: actual } => {
                    expected_root_effect && *actual == binding
                }
                PlatformEffect::RetainChild { binding: actual } => {
                    !expected_root_effect && *actual == binding
                }
                _ => false,
            });
            assert_eq!(
                coordinator.decide_viewport_close(*request, ViewportCloseDecision::Veto),
                Err(ViewportCoordinatorError::CloseRequestAlreadyDecided {
                    request: *request,
                    status: ViewportCloseStatus::Vetoed { effect },
                })
            );
            assert!(coordinator.take_new_effects().is_empty());

            let repeated = coordinator
                .publish_snapshot(&snapshot(vec![observed_window(binding.token(), true)]))
                .expect("repeated close fact must publish");
            assert!(repeated.close_requests().is_empty());

            coordinator
                .publish_snapshot(&snapshot(vec![observed_window(binding.token(), false)]))
                .expect("cleared close fact must publish");
            let next_edge = coordinator
                .publish_snapshot(&snapshot(vec![observed_window(binding.token(), true)]))
                .expect("next close edge must publish");
            assert_eq!(
                next_edge.close_requests(),
                &[request
                    .checked_next()
                    .expect("test close identity must advance")]
            );
        }
    }

    #[test]
    fn child_accept_waits_for_matching_destruction_before_publishing_the_plan() {
        let mut coordinator = ViewportCoordinator::default();
        let binding = register(&mut coordinator, 2, 20, ViewportRole::Child);
        let recovery = recovery(2);
        coordinator
            .recovery_plans
            .insert(binding.surface(), recovery.into());
        let transition = coordinator
            .publish_snapshot(&snapshot(vec![observed_window(binding.token(), true)]))
            .expect("close edge must publish");
        let request = transition.close_requests()[0];
        let plan = ViewportClosePlan::new(None, recovery);
        let effect = coordinator
            .decide_viewport_close(request, ViewportCloseDecision::Accept(plan.clone()))
            .expect("accept must enter the release effect")
            .expect("child accept must have a release effect");
        let emitted = coordinator.take_new_effects();
        assert!(matches!(
            emitted.as_slice(),
            [effect_request]
                if effect_request.id() == effect
                    && matches!(
                        effect_request.effect(),
                        PlatformEffect::ReleaseChild { binding: actual }
                            if *actual == binding
                    )
        ));

        let still_present = coordinator
            .publish_snapshot(&snapshot(vec![observed_window(binding.token(), true)]))
            .expect("present child must remain topology-neutral");
        assert!(still_present.actions().is_empty());

        let destroyed = coordinator
            .publish_snapshot(&snapshot(Vec::new()))
            .expect("authoritative absence must publish");
        assert!(matches!(
            destroyed.actions(),
            [ViewportLifecycleAction::SurfaceDestroyed {
                binding: actual,
                resolution: ViewportDestructionResolution::Accepted {
                    request: actual_request,
                    plan: actual_plan,
                },
            }] if *actual == binding && *actual_request == request && *actual_plan == plan
        ));
        assert_eq!(
            coordinator
                .viewport_close_request(request)
                .expect("close history must remain queryable")
                .status(),
            ViewportCloseStatus::Destroyed
        );
        assert!(matches!(
            coordinator
                .effects()
                .record(effect)
                .expect("release effect must remain queryable")
                .phase(),
            EffectPhase::Destroyed { .. }
        ));
    }

    #[test]
    fn direct_destruction_uses_the_frozen_whole_root_recovery() {
        let mut coordinator = ViewportCoordinator::default();
        let binding = register(&mut coordinator, 3, 30, ViewportRole::Child);
        let recovery = recovery(3);
        coordinator
            .recovery_plans
            .insert(binding.surface(), recovery.into());
        coordinator
            .publish_snapshot(&snapshot(vec![observed_window(binding.token(), false)]))
            .expect("ready observation must publish");

        let destroyed = coordinator
            .publish_snapshot(&snapshot(Vec::new()))
            .expect("direct destruction must publish");
        assert!(matches!(
            destroyed.actions(),
            [ViewportLifecycleAction::SurfaceDestroyed {
                binding: actual,
                resolution: ViewportDestructionResolution::Recover {
                    recovery: actual_recovery,
                },
            }] if *actual == binding && *actual_recovery == recovery.into()
        ));
    }

    #[test]
    fn pointer_hit_test_lease_restores_only_after_the_last_holder() {
        let (mut coordinator, binding) = routing_coordinator(
            WindowInputState::ReceivesInput,
            WindowPresentationState::Visible,
            PlatformCapability::Supported,
        );
        let first = PointerId::new(1);
        let second = PointerId::new(2);

        assert!(
            coordinator
                .begin_drag_routing(first, binding.surface())
                .expect("first lease must begin")
                .is_some()
        );
        assert_eq!(
            coordinator
                .begin_drag_routing(second, binding.surface())
                .expect("second lease holder must begin"),
            None
        );
        let requested = coordinator.take_new_effects();
        assert!(matches!(
            requested.as_slice(),
            [request]
                if matches!(
                    request.effect(),
                    PlatformEffect::SetPointerPassthrough {
                        binding: actual,
                        enabled: true,
                    } if *actual == binding
                )
        ));

        assert_eq!(
            coordinator
                .end_drag_routing(first)
                .expect("first holder must release"),
            None
        );
        assert!(coordinator.take_new_effects().is_empty());
        assert!(
            coordinator
                .end_drag_routing(second)
                .expect("last holder must release")
                .is_some()
        );
        let restored = coordinator.take_new_effects();
        assert!(matches!(
            restored.as_slice(),
            [request]
                if matches!(
                    request.effect(),
                    PlatformEffect::SetPointerPassthrough {
                        binding: actual,
                        enabled: false,
                    } if *actual == binding
                )
        ));
    }

    #[test]
    fn preexisting_passthrough_needs_neither_control_capability_nor_restore() {
        let (mut coordinator, binding) = routing_coordinator(
            WindowInputState::PassThrough,
            WindowPresentationState::Visible,
            PlatformCapability::unsupported(
                crate::platform::PlatformRequirement::PointerHitTestControl,
                crate::platform::PlatformCapabilityReason::BackendUnsupported,
            ),
        );
        let pointer = PointerId::new(1);

        assert_eq!(
            coordinator
                .begin_drag_routing(pointer, binding.surface())
                .expect("existing passthrough must be leased"),
            None
        );
        assert_eq!(coordinator.drag_source(pointer), Some(binding));
        assert!(coordinator.take_new_effects().is_empty());
        assert_eq!(
            coordinator
                .end_drag_routing(pointer)
                .expect("existing passthrough lease must end"),
            None
        );
        assert!(coordinator.take_new_effects().is_empty());
    }

    #[test]
    fn receiving_source_without_hit_test_control_does_not_acquire_a_lease() {
        let (mut coordinator, binding) = routing_coordinator(
            WindowInputState::ReceivesInput,
            WindowPresentationState::Visible,
            PlatformCapability::unsupported(
                crate::platform::PlatformRequirement::PointerHitTestControl,
                crate::platform::PlatformCapabilityReason::BackendUnsupported,
            ),
        );
        let pointer = PointerId::new(1);

        assert_eq!(
            coordinator
                .begin_drag_routing(pointer, binding.surface())
                .expect("missing control is a supported no-op"),
            None
        );
        assert_eq!(coordinator.drag_source(pointer), None);
        assert!(coordinator.take_new_effects().is_empty());
    }

    #[test]
    fn hidden_and_minimized_windows_cannot_start_pointer_routing_or_focus() {
        for presentation in [
            WindowPresentationState::Hidden,
            WindowPresentationState::Minimized,
        ] {
            let (mut coordinator, binding) = routing_coordinator(
                WindowInputState::ReceivesInput,
                presentation,
                PlatformCapability::Supported,
            );
            coordinator
                .capabilities
                .set_window_focus(PlatformCapability::Supported);

            assert_eq!(
                coordinator
                    .begin_drag_routing(PointerId::new(1), binding.surface())
                    .expect("non-routeable presentation is a supported no-op"),
                None
            );
            assert_eq!(
                coordinator
                    .request_focus(binding.surface())
                    .expect("non-routeable presentation cannot receive focus"),
                None
            );
            assert!(coordinator.take_new_effects().is_empty());
        }
    }

    #[test]
    fn close_request_identity_exhaustion_rolls_back_the_snapshot() {
        let mut coordinator = ViewportCoordinator::default();
        let binding = register(&mut coordinator, 4, 40, ViewportRole::Root);
        coordinator
            .publish_snapshot(&snapshot(vec![observed_window(binding.token(), false)]))
            .expect("ready observation must publish");
        coordinator.exhaust_close_request_ids();
        let before = coordinator.clone();

        assert_eq!(
            coordinator.publish_snapshot(&snapshot(vec![observed_window(binding.token(), true)])),
            Err(ViewportCoordinatorError::CloseRequestIdExhausted)
        );
        assert_eq!(coordinator, before);
    }
}
