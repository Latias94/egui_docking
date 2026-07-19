//! Atomic platform-fact coordinator used by the docking engine.

use std::collections::{BTreeMap, BTreeSet};

use thiserror::Error;

use crate::command::TabTarget;
use crate::coordinates::{
    CoordinateUnavailable, TearOffPlacementProof, TearOffPlacementRequest,
    TearOffPlacementUnavailable, ViewportPlacementProof,
};
use crate::effect::{
    EffectDispatchResult, EffectId, EffectLedger, EffectLedgerError, EffectPhase, EffectRequest,
    EffectResult, EffectTransition, PlatformEffect,
};
use crate::geometry::LogicalRect;
use crate::ids::{ItemId, SurfaceId, WorkspaceEpoch};
use crate::intent::{
    ContainedPlacementUnavailable, ContainedRecoveryPlan, NativePlacementProof, PointerId,
};
use crate::interaction::PreparedNativeTearOff;
use crate::platform::{
    ObservedWorkArea, PlatformCapabilities, PlatformCapability, PlatformSnapshot,
    WindowInputObservation, WindowInputObservationStream, WindowInputState,
};
use crate::viewport::{
    CapabilityGeneration, InputObservationGeneration, RouteGeneration, ViewportBinding,
    ViewportRole, WindowToken, WorkAreaGeneration, WorkAreaToken,
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
    CommittedAwaitingVisibility { show: EffectId },
    Committed { show: Option<EffectId> },
    Cancelled,
    Compensating { effect: EffectId },
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

/// Typed logical disposition frozen when an application accepts one viewport close.
#[derive(Debug, Clone, PartialEq)]
pub enum ViewportClosePlan {
    /// Unbind the destroyed native window while preserving the complete logical surface.
    RetainLayout,
    /// Atomically merge the main tabs and complete contained forest into another surface.
    MergeBack(ViewportMergeBackPlan),
}

impl ViewportClosePlan {
    #[must_use]
    pub const fn retain_layout() -> Self {
        Self::RetainLayout
    }

    #[must_use]
    pub const fn merge_back(plan: ViewportMergeBackPlan) -> Self {
        Self::MergeBack(plan)
    }
}

/// Prevalidated semantic target for one full-surface merge-back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ViewportMergeBackPlan {
    target_surface: SurfaceId,
    target: TabTarget,
}

/// Exact panel-focus disposition owned by the core focus coordinator.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PanelFocus {
    /// Activate and focus this exact source-roster item after merge-back.
    Item(ItemId),
    /// Explicitly leave the merged content without a focused panel.
    None,
}

impl ViewportMergeBackPlan {
    #[must_use]
    pub const fn new(target_surface: SurfaceId, target: TabTarget) -> Self {
        Self {
            target_surface,
            target,
        }
    }

    #[must_use]
    pub const fn target_surface(&self) -> SurfaceId {
        self.target_surface
    }

    #[must_use]
    pub const fn target(&self) -> &TabTarget {
        &self.target
    }
}

/// Application decision for one exact platform close-request edge.
#[derive(Debug, Clone, PartialEq)]
pub enum ViewportCloseDecision {
    Prevent,
    Accept(ViewportClosePlan),
}

/// Why an accepted close was rejected and converted into an explicit hold.
#[derive(Debug, Clone, PartialEq)]
pub enum ViewportCloseDecisionRejection {
    /// Complete native-window absence cannot prove destruction.
    DestructionAuthorityUnavailable { capability: PlatformCapability },
    /// The recovery placement is no longer authorized by the current scene.
    RecoveryPlacementUnavailable(ContainedPlacementUnavailable),
    /// Merge-back cannot target the same logical surface whose native host is closing.
    MergeBackTargetsClosingSurface { surface: SurfaceId },
    /// Merge-back must target the child surface's registered recovery host.
    MergeBackRecoveryTargetMismatch {
        expected: SurfaceId,
        actual: SurfaceId,
    },
    /// Merge-back requires the source main root to be one complete tabs stack.
    MergeBackSourceNotTabs { root: crate::ids::RootId },
    /// The frozen merge-back tabs target is absent, stale, or outside the target surface.
    MergeBackTargetUnavailable { surface: SurfaceId },
    /// The accepted close cannot freeze authoritative source content geometry.
    SourceGeometryUnavailable,
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
    CleanupRequested {
        effect: EffectId,
    },
    CleanupIndeterminate {
        effect: EffectId,
    },
    /// The observation-only continuation failed; the destructive predecessor remains unknown.
    CleanupObservationFailed {
        effect: EffectId,
    },
    CleanupFailed {
        effect: EffectId,
    },
}

/// Old-epoch binding isolated from the current logical surface roster.
#[derive(Debug, Clone, PartialEq)]
pub struct RetiredViewport {
    binding: ViewportBinding,
    role: ViewportRole,
    status: RetiredViewportStatus,
    observed: bool,
    input_observations: WindowInputObservationStream,
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
    ReplacementRegistered,
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
    role: ViewportRole,
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
    pub const fn role(&self) -> ViewportRole {
        self.role
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
    CreateVisible {
        saga: NativeCreateSagaId,
        binding: ViewportBinding,
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
        recovery: Option<ContainedRecoveryPlan>,
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
    focus_effect_lane_tail: Option<EffectId>,
    drag_sources: BTreeMap<PointerId, ViewportBinding>,
    pointer_passthrough_sagas: BTreeMap<ViewportBinding, PointerPassthroughSaga>,
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
struct PointerPassthroughSaga {
    original: Option<PointerInputOriginal>,
    holders: BTreeSet<PointerId>,
    enable: Option<PointerInputEnableAttempt>,
    restore: Option<PointerInputRestoreObligation>,
    lane_tail: Option<EffectId>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct PointerInputOriginal {
    state: WindowInputState,
    generation: InputObservationGeneration,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct PointerInputEffectAttempt {
    effect: EffectId,
    retry: PointerPassthroughRetryFence,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct PointerInputEnableAttempt {
    effect: PointerInputEffectAttempt,
    issued_after: InputObservationGeneration,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct PointerInputRestoreAttempt {
    effect: PointerInputEffectAttempt,
    issued_after: Option<InputObservationGeneration>,
    terminal_reported_after: Option<InputObservationGeneration>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct PointerInputRestoreObligation {
    settlement: PointerInputRestoreSettlement,
    attempt: Option<PointerInputRestoreAttempt>,
    state_settled: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PointerInputRestoreSettlement {
    StateOrExactEffect,
    ExactEffect,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PointerPassthroughRetryFence {
    None,
    DispatchFailed {
        evidence: PointerPassthroughRetryEvidence,
        edge_seen: bool,
    },
    Unsupported {
        capabilities: PointerPassthroughCapabilities,
        edge_seen: bool,
    },
    Indeterminate,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PointerPassthroughAction {
    None,
    Remove,
    RequestEnable,
    RequestRestore,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct PointerPassthroughCapabilities {
    observation: PlatformCapability,
    control: PlatformCapability,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct PointerPassthroughEvidence {
    observation: Option<WindowInputObservation>,
    generation_watermark: Option<InputObservationGeneration>,
    window_observed: bool,
    routeable: bool,
    capabilities: PointerPassthroughCapabilities,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct PointerPassthroughRetryEvidence {
    input_state: Option<WindowInputState>,
    acknowledgement_known: bool,
    acknowledged_effect: Option<EffectId>,
    window_observed: bool,
    routeable: bool,
    capabilities: PointerPassthroughCapabilities,
}

impl PointerPassthroughEvidence {
    fn authoritative_observation(self) -> Option<WindowInputObservation> {
        self.observation
            .filter(|observation| observation.known_state().is_some())
    }

    fn authoritative_state(self) -> Option<WindowInputState> {
        self.authoritative_observation()
            .and_then(WindowInputObservation::known_state)
    }

    fn retry_evidence(self) -> PointerPassthroughRetryEvidence {
        let (acknowledgement_known, acknowledged_effect) =
            self.observation.map_or((false, None), |observation| {
                match observation.acknowledged_effect() {
                    crate::platform::InputEffectAcknowledgement::Known(effect) => (true, effect),
                    crate::platform::InputEffectAcknowledgement::Unknown(_) => (false, None),
                }
            });
        PointerPassthroughRetryEvidence {
            input_state: self
                .observation
                .and_then(WindowInputObservation::known_state),
            acknowledgement_known,
            acknowledged_effect,
            window_observed: self.window_observed,
            routeable: self.routeable,
            capabilities: self.capabilities,
        }
    }
}

impl PointerPassthroughSaga {
    fn new(holder: PointerId) -> Self {
        Self {
            original: None,
            holders: BTreeSet::from([holder]),
            enable: None,
            restore: None,
            lane_tail: None,
        }
    }

    fn epoch_recovery(previous: &Self) -> Self {
        Self {
            original: previous.original,
            holders: BTreeSet::new(),
            enable: None,
            restore: Some(PointerInputRestoreObligation {
                settlement: PointerInputRestoreSettlement::ExactEffect,
                attempt: None,
                state_settled: false,
            }),
            lane_tail: previous.lane_tail,
        }
    }

    fn retired_recovery(&self) -> Self {
        let mut recovery = self.clone();
        recovery.holders.clear();
        recovery
    }

    fn recovery_required(&self) -> bool {
        self.restore.is_some()
            || (self
                .original
                .is_some_and(|original| original.state == WindowInputState::ReceivesInput)
                && self.enable.is_some())
    }

    fn route_source(&self, binding: ViewportBinding) -> crate::viewport_route::ViewportRouteSource {
        if self.restore.is_some() {
            return crate::viewport_route::ViewportRouteSource::unavailable(binding);
        }
        if let Some(enable) = self.enable {
            return crate::viewport_route::ViewportRouteSource::core_effect(
                binding,
                enable.effect.effect,
                enable.issued_after,
            );
        }
        if self
            .original
            .is_some_and(|original| original.state == WindowInputState::PassThrough)
        {
            crate::viewport_route::ViewportRouteSource::preexisting(binding)
        } else {
            crate::viewport_route::ViewportRouteSource::unavailable(binding)
        }
    }

    fn causal_effect(&self, observation: WindowInputObservation) -> Option<EffectId> {
        if let Some(attempt) = self.restore.and_then(|restore| restore.attempt)
            && attempt
                .issued_after
                .is_none_or(|generation| observation.generation() > generation)
            && observation.known_state() == Some(WindowInputState::ReceivesInput)
            && observation.acknowledges(attempt.effect.effect)
        {
            return Some(attempt.effect.effect);
        }
        self.enable.and_then(|attempt| {
            (observation.generation() > attempt.issued_after
                && observation.known_state() == Some(WindowInputState::PassThrough)
                && observation.acknowledges(attempt.effect.effect))
            .then_some(attempt.effect.effect)
        })
    }

    fn accept_observed_effect(&mut self, effect: EffectId) {
        if self
            .restore
            .and_then(|restore| restore.attempt)
            .is_some_and(|attempt| attempt.effect.effect == effect)
        {
            self.restore = None;
            self.enable = None;
        }
    }

    fn record_dispatch_result(
        &mut self,
        effect: EffectId,
        result: EffectDispatchResult,
        evidence: PointerPassthroughEvidence,
    ) -> bool {
        if let Some(enable) = &mut self.enable
            && enable.effect.effect == effect
        {
            enable.effect.record_dispatch_result(result, evidence);
            return true;
        }
        if let Some(restore) = &mut self.restore
            && let Some(attempt) = &mut restore.attempt
            && attempt.effect.effect == effect
        {
            if matches!(
                result,
                EffectDispatchResult::DispatchFailed(_) | EffectDispatchResult::Unsupported(_)
            ) {
                attempt.terminal_reported_after =
                    attempt.issued_after.max(evidence.generation_watermark);
            }
            attempt.effect.record_dispatch_result(result, evidence);
            return true;
        }
        false
    }

    fn action(
        &mut self,
        evidence: PointerPassthroughEvidence,
        enable_phase: Option<EffectPhase>,
        restore_phase: Option<EffectPhase>,
    ) -> PointerPassthroughAction {
        self.capture_original_state(evidence);
        self.observe_retry_edges(evidence);
        self.settle_restore_from_state(evidence, enable_phase, restore_phase);

        if self.restore.is_some_and(|restore| restore.state_settled) {
            if self.holders.is_empty()
                && !matches!(
                    restore_phase,
                    Some(
                        EffectPhase::DispatchFailed(_)
                            | EffectPhase::ObservedApplied { .. }
                            | EffectPhase::Unsupported(_)
                            | EffectPhase::Destroyed { .. }
                            | EffectPhase::InvalidatedByRestore { .. }
                    )
                )
            {
                return PointerPassthroughAction::None;
            }
            self.restore = None;
        }

        if let Some(restore) = self.restore {
            let request_restore = match restore.attempt {
                None => true,
                Some(attempt) => {
                    evidence.capabilities.control.is_supported()
                        && (matches!(
                            restore_phase,
                            Some(EffectPhase::InvalidatedByRestore { .. })
                        ) || restore_phase.is_some_and(|phase| attempt.effect.can_retry(phase)))
                }
            };
            return if request_restore {
                PointerPassthroughAction::RequestRestore
            } else {
                PointerPassthroughAction::None
            };
        }

        if self.holders.is_empty() {
            if self
                .original
                .is_some_and(|original| original.state == WindowInputState::ReceivesInput)
                && self.enable.is_some()
            {
                self.restore = Some(PointerInputRestoreObligation {
                    settlement: PointerInputRestoreSettlement::StateOrExactEffect,
                    attempt: None,
                    state_settled: false,
                });
                return PointerPassthroughAction::RequestRestore;
            }
            return PointerPassthroughAction::Remove;
        }

        let Some(original) = self.original else {
            return PointerPassthroughAction::None;
        };
        let needs_enable = self.enable.map_or_else(
            || {
                original.state == WindowInputState::ReceivesInput
                    || evidence.authoritative_state() != Some(WindowInputState::PassThrough)
            },
            |attempt| {
                let state_proves_passthrough =
                    evidence
                        .authoritative_observation()
                        .is_some_and(|observation| {
                            observation.generation() > attempt.issued_after
                                && observation.known_state() == Some(WindowInputState::PassThrough)
                        });
                !state_proves_passthrough
                    && (matches!(enable_phase, Some(EffectPhase::InvalidatedByRestore { .. }))
                        || enable_phase.is_some_and(|phase| attempt.effect.can_retry(phase)))
            },
        );
        if needs_enable
            && evidence.authoritative_observation().is_some()
            && evidence.routeable
            && evidence.capabilities.observation.is_supported()
            && evidence.capabilities.control.is_supported()
        {
            PointerPassthroughAction::RequestEnable
        } else {
            PointerPassthroughAction::None
        }
    }

    fn capture_original_state(&mut self, evidence: PointerPassthroughEvidence) {
        if self.original.is_none() {
            self.original = evidence
                .authoritative_observation()
                .and_then(|observation| {
                    observation.known_state().map(|state| PointerInputOriginal {
                        state,
                        generation: observation.generation(),
                    })
                });
        }
    }

    fn observe_retry_edges(&mut self, evidence: PointerPassthroughEvidence) {
        if let Some(enable) = &mut self.enable {
            enable.effect.observe_retry_edge(evidence);
        }
        if let Some(restore) = &mut self.restore
            && let Some(attempt) = &mut restore.attempt
        {
            attempt.effect.observe_retry_edge(evidence);
        }
    }

    fn settle_restore_from_state(
        &mut self,
        evidence: PointerPassthroughEvidence,
        enable_phase: Option<EffectPhase>,
        restore_phase: Option<EffectPhase>,
    ) {
        let enable_cannot_apply_later = matches!(
            enable_phase,
            Some(
                EffectPhase::DispatchFailed(_)
                    | EffectPhase::ObservedApplied { .. }
                    | EffectPhase::Unsupported(_)
                    | EffectPhase::Destroyed { .. }
            )
        );
        let restore_reached_terminal_lane_position = matches!(
            restore_phase,
            Some(EffectPhase::DispatchFailed(_) | EffectPhase::Unsupported(_))
        );
        let state_can_settle_restore = self.restore.is_some_and(|restore| {
            restore.settlement == PointerInputRestoreSettlement::StateOrExactEffect
                && !restore.state_settled
                && restore.attempt.is_some_and(|attempt| {
                    evidence
                        .authoritative_observation()
                        .is_some_and(|observation| {
                            let barrier = if restore_reached_terminal_lane_position {
                                attempt.terminal_reported_after
                            } else if enable_cannot_apply_later {
                                attempt.issued_after
                            } else {
                                return false;
                            };
                            barrier.is_none_or(|generation| observation.generation() > generation)
                                && observation.known_state()
                                    == Some(WindowInputState::ReceivesInput)
                        })
                })
        });
        if state_can_settle_restore {
            if let Some(restore) = &mut self.restore {
                restore.state_settled = true;
            }
            self.enable = None;
        }
    }
}

impl PointerInputEffectAttempt {
    const fn new(effect: EffectId) -> Self {
        Self {
            effect,
            retry: PointerPassthroughRetryFence::None,
        }
    }

    fn record_dispatch_result(
        &mut self,
        result: EffectDispatchResult,
        evidence: PointerPassthroughEvidence,
    ) {
        self.retry = match result {
            EffectDispatchResult::DispatchFailed(_) => {
                PointerPassthroughRetryFence::DispatchFailed {
                    evidence: evidence.retry_evidence(),
                    edge_seen: false,
                }
            }
            EffectDispatchResult::Unsupported(_) => PointerPassthroughRetryFence::Unsupported {
                capabilities: evidence.capabilities,
                edge_seen: false,
            },
            EffectDispatchResult::Indeterminate(_) => PointerPassthroughRetryFence::Indeterminate,
        };
    }

    fn observe_retry_edge(&mut self, evidence: PointerPassthroughEvidence) {
        match &mut self.retry {
            PointerPassthroughRetryFence::DispatchFailed {
                evidence: blocked,
                edge_seen,
            } => *edge_seen |= *blocked != evidence.retry_evidence(),
            PointerPassthroughRetryFence::Unsupported {
                capabilities,
                edge_seen,
            } => *edge_seen |= *capabilities != evidence.capabilities,
            PointerPassthroughRetryFence::None | PointerPassthroughRetryFence::Indeterminate => {}
        }
    }

    const fn can_retry(self, phase: EffectPhase) -> bool {
        matches!(
            (phase, self.retry),
            (
                EffectPhase::DispatchFailed(_),
                PointerPassthroughRetryFence::DispatchFailed {
                    edge_seen: true,
                    ..
                }
            ) | (
                EffectPhase::Unsupported(_),
                PointerPassthroughRetryFence::Unsupported {
                    edge_seen: true,
                    ..
                }
            )
        )
    }
}

struct RestoreAnalysis {
    close_by_binding: BTreeMap<ViewportBinding, ViewportCloseRequest>,
    creation_by_binding: BTreeMap<ViewportBinding, (EffectId, RetiredCleanup)>,
    cleanup_by_binding: BTreeMap<ViewportBinding, EffectId>,
    retained_bindings: BTreeSet<ViewportBinding>,
    passthrough_recoveries: BTreeMap<ViewportBinding, PointerPassthroughSaga>,
    replacement_supported: bool,
}

#[derive(Default)]
struct RestoreAccumulation {
    retired: Vec<ViewportBinding>,
    cleanup_effects: Vec<EffectId>,
    replacements: Vec<ViewportBinding>,
    unbound_surfaces: Vec<SurfaceId>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RetiredCleanupRestoreAction {
    Redispatch {
        token: WindowToken,
        binding: ViewportBinding,
        cleanup: RetiredCleanup,
        observed: bool,
    },
    ContinueObservation {
        token: WindowToken,
        binding: ViewportBinding,
        predecessor: EffectId,
        after: Option<EffectId>,
    },
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
        let pending_recovery = self
            .pending_recoveries
            .get(&surface)
            .map(|pending| pending.recovery);
        let adopts_pending_recovery = if let Some(pending) = self.pending_recoveries.get(&surface) {
            let may_adopt = pending.replacement_binding.is_none()
                && matches!(
                    pending.status,
                    RecoveryPendingStatus::AwaitingRecoveryHost
                        | RecoveryPendingStatus::ReplacementFailed { .. }
                )
                && pending.role == role
                && recovery
                    .is_some_and(|candidate| pending.recovery.matches_registration(candidate));
            if !may_adopt {
                return Err(
                    ViewportCoordinatorError::PendingRecoveryRegistrationMismatch { surface },
                );
            }
            true
        } else {
            false
        };
        let binding = self
            .registry
            .register_existing(epoch, surface, token, role)
            .map_err(ViewportCoordinatorError::Registry)?;
        let recovery = if adopts_pending_recovery {
            pending_recovery
        } else {
            recovery
        };
        if let Some(recovery) = recovery {
            self.recovery_plans.insert(surface, recovery);
        }
        if adopts_pending_recovery {
            let pending = self
                .pending_recoveries
                .get_mut(&surface)
                .ok_or(ViewportCoordinatorError::MissingRecoveryPending { surface })?;
            pending.replacement_binding = Some(binding);
            pending.replacement_effect = None;
            pending.status = RecoveryPendingStatus::ReplacementRegistered;
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
        candidate.pointer_passthrough_sagas.clear();
        candidate.restore_replacements.clear();
        let mut accumulated = RestoreAccumulation::default();
        candidate
            .migrate_retired_cleanup_obligations(new_epoch, &mut accumulated.cleanup_effects)?;
        candidate.request_retired_restore_cleanup(
            new_epoch,
            &analysis,
            &mut accumulated.cleanup_effects,
        )?;
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
            passthrough_recoveries: self.passthrough_recovery_sagas(),
            replacement_supported: self.capabilities.native_window_lifecycle().is_supported()
                && self.capabilities.authoritative_inventory().is_supported(),
        }
    }

    fn passthrough_recovery_sagas(&self) -> BTreeMap<ViewportBinding, PointerPassthroughSaga> {
        self.pointer_passthrough_sagas
            .iter()
            .filter_map(|(binding, saga)| {
                saga.recovery_required().then_some((*binding, saga.clone()))
            })
            .collect()
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
        // A surface identity alone cannot prove that a replacement workspace preserved the
        // child's exact root, recovery host, floating identities, ownership, and geometry
        // contract. Until restore publishes that semantic manifest, child bindings fail closed
        // and require a new explicit registration carrying a freshly validated recovery plan.
        desired_surfaces.contains(&surface)
            && record.role() != ViewportRole::Child
            && create_is_committed
            && !destructive_close
            && !replacements.contains(&binding)
            && !matches!(
                record.lifecycle(),
                ViewportLifecycle::AwaitingDestroyed | ViewportLifecycle::Missing
            )
    }

    fn migrate_retired_cleanup_obligations(
        &mut self,
        new_epoch: WorkspaceEpoch,
        cleanup_effects: &mut Vec<EffectId>,
    ) -> Result<(), ViewportCoordinatorError> {
        let actions: Vec<_> = self
            .retired_viewports
            .iter()
            .map(|(token, retired)| self.retired_cleanup_restore_action(*token, retired))
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .flatten()
            .collect();

        for action in actions {
            match action {
                RetiredCleanupRestoreAction::Redispatch {
                    token,
                    binding,
                    cleanup,
                    observed,
                } => {
                    self.retired_viewports
                        .get_mut(&token)
                        .ok_or(ViewportCoordinatorError::MissingRetiredViewport { token })?
                        .status = RetiredViewportStatus::AwaitingAppearance;
                    if observed {
                        let effect = self.request_retired_cleanup(binding, cleanup)?;
                        self.retired_viewports
                            .get_mut(&token)
                            .ok_or(ViewportCoordinatorError::MissingRetiredViewport { token })?
                            .status = RetiredViewportStatus::CleanupRequested { effect };
                        cleanup_effects.push(effect);
                    }
                }
                RetiredCleanupRestoreAction::ContinueObservation {
                    token,
                    binding,
                    predecessor,
                    after,
                } => {
                    let successor = self
                        .effects
                        .request_in(
                            new_epoch,
                            PlatformEffect::ContinueCleanup {
                                binding,
                                predecessor,
                                after,
                            },
                        )
                        .map_err(ViewportCoordinatorError::Effect)?;
                    self.retired_viewports
                        .get_mut(&token)
                        .ok_or(ViewportCoordinatorError::MissingRetiredViewport { token })?
                        .status = RetiredViewportStatus::CleanupRequested { effect: successor };
                    cleanup_effects.push(successor);
                }
            }
        }
        Ok(())
    }

    fn retired_cleanup_restore_action(
        &self,
        token: WindowToken,
        retired: &RetiredViewport,
    ) -> Result<Option<RetiredCleanupRestoreAction>, ViewportCoordinatorError> {
        let Some(effect) = retired_effect(retired.status) else {
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
                if *binding != retired.binding
                    || predecessor_record.request().effect().binding() != retired.binding
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
                Ok(Some(RetiredCleanupRestoreAction::ContinueObservation {
                    token,
                    binding: retired.binding,
                    predecessor: *predecessor,
                    after: self.cleanup_observation_lane_predecessor(effect)?,
                }))
            }
            platform_effect if is_destructive_cleanup(platform_effect) => {
                if platform_effect.binding() != retired.binding {
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
                    return Ok(Some(RetiredCleanupRestoreAction::ContinueObservation {
                        token,
                        binding: retired.binding,
                        predecessor: effect,
                        after: None,
                    }));
                }
                Ok((!record.was_emitted()
                    && matches!(record.phase(), EffectPhase::InvalidatedByRestore { .. }))
                .then_some(RetiredCleanupRestoreAction::Redispatch {
                    token,
                    binding: retired.binding,
                    cleanup: retired.cleanup,
                    observed: retired.observed,
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
            let invalidated_unemitted = !record.was_emitted()
                && matches!(record.phase(), EffectPhase::InvalidatedByRestore { .. });
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

    fn request_retired_restore_cleanup(
        &mut self,
        new_epoch: WorkspaceEpoch,
        analysis: &RestoreAnalysis,
        cleanup_effects: &mut Vec<EffectId>,
    ) -> Result<(), ViewportCoordinatorError> {
        for (binding, old_saga) in &analysis.passthrough_recoveries {
            let is_existing_retired = self
                .retired_viewports
                .get(&binding.token())
                .is_some_and(|retired| retired.binding == *binding);
            if !is_existing_retired {
                continue;
            }
            self.pointer_passthrough_sagas
                .insert(*binding, PointerPassthroughSaga::epoch_recovery(old_saga));
            let effect = self.request_pointer_input_restore_in(new_epoch, *binding)?;
            cleanup_effects.push(effect);
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
        for (old_binding, old_saga) in &analysis.passthrough_recoveries {
            let Some(binding) = rebound.get(old_binding).copied() else {
                continue;
            };
            self.pointer_passthrough_sagas
                .insert(binding, PointerPassthroughSaga::epoch_recovery(old_saga));
            if let Some(effect) = self.reconcile_pointer_passthrough_saga(binding)? {
                cleanup_effects.push(effect);
            }
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
            if facts.role() == ViewportRole::Child {
                accumulated.unbound_surfaces.push(surface);
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
                    input_observations: facts.input_observations(),
                    may_appear_late,
                    cleanup,
                },
            );
            if let Some(old_saga) = analysis.passthrough_recoveries.get(&binding) {
                self.pointer_passthrough_sagas
                    .insert(binding, old_saga.retired_recovery());
                if let Some(effect) = self.reconcile_pointer_passthrough_saga(binding)? {
                    accumulated.cleanup_effects.push(effect);
                }
            }
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
            Some(
                EffectPhase::ObservationDispatchFailed(_) | EffectPhase::ObservationUnsupported(_),
            ) => RetiredViewportStatus::CleanupObservationFailed { effect },
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
    ) -> Result<EffectId, ViewportCoordinatorError> {
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
        if matches!(decision, ViewportCloseDecision::Accept(_)) {
            let capability = candidate.capabilities.authoritative_inventory();
            if !capability.is_supported() {
                return Err(
                    ViewportCoordinatorError::CloseDestructionAuthorityUnavailable { capability },
                );
            }
        }

        let (effect, status, plan) = match decision {
            ViewportCloseDecision::Prevent => {
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
                (effect, ViewportCloseStatus::Vetoed { effect }, None)
            }
            ViewportCloseDecision::Accept(plan) => {
                let effect_kind = match request.role {
                    ViewportRole::Root => PlatformEffect::RequestRootClose {
                        binding: request.binding,
                    },
                    ViewportRole::Child => PlatformEffect::ReleaseChild {
                        binding: request.binding,
                    },
                };
                let effect = candidate
                    .effects
                    .request(effect_kind)
                    .map_err(ViewportCoordinatorError::Effect)?;
                candidate
                    .registry
                    .mark_awaiting_destroyed(request.binding)
                    .map_err(ViewportCoordinatorError::Registry)?;
                (
                    effect,
                    ViewportCloseStatus::AwaitingDestroyed {
                        effect: Some(effect),
                    },
                    Some(plan),
                )
            }
        };

        let close_request = candidate.close_requests.get_mut(&request_id).ok_or(
            ViewportCoordinatorError::MissingCloseRequest {
                request: request_id,
            },
        )?;
        close_request.effect = Some(effect);
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
        for binding in registry.events().iter().filter_map(|event| match event {
            RegistryEvent::Destroyed { binding } => Some(*binding),
            RegistryEvent::Ready { .. }
            | RegistryEvent::FactsUnavailable { .. }
            | RegistryEvent::CloseRequested { .. }
            | RegistryEvent::CloseRequestCleared { .. } => None,
        }) {
            candidate.terminate_pointer_passthrough_binding(binding);
        }
        candidate.refresh_recovery_geometry();
        candidate.reconcile_create_inventory(snapshot)?;
        candidate.reconcile_retired_inventory(snapshot)?;
        candidate.observe_pointer_passthrough_snapshot();
        candidate.reconcile_pointer_passthrough_sagas()?;
        let route_sources: BTreeMap<_, _> = candidate
            .drag_sources
            .iter()
            .map(|(pointer, binding)| {
                let source = candidate
                    .pointer_passthrough_sagas
                    .get(binding)
                    .map_or_else(
                        || crate::viewport_route::ViewportRouteSource::unavailable(*binding),
                        |saga| saga.route_source(*binding),
                    );
                (*pointer, source)
            })
            .collect();
        let route_generation = candidate
            .routes
            .publish(
                snapshot,
                &candidate.registry,
                capability_generation,
                &route_sources,
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
        let authoritative = snapshot
            .capabilities()
            .authoritative_inventory()
            .is_supported();
        let observations: BTreeMap<WindowToken, _> = snapshot
            .windows()
            .iter()
            .map(|window| (window.token(), window))
            .collect();
        let tokens: Vec<WindowToken> = self.retired_viewports.keys().copied().collect();
        for token in tokens {
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
            if let Some(observation) = observations.get(&token) {
                if let Some(retired) = self.retired_viewports.get_mut(&token) {
                    retired.observed = true;
                    retired
                        .input_observations
                        .observe(binding, observation.input_observation());
                }
                if status == RetiredViewportStatus::AwaitingAppearance {
                    let effect = self.request_retired_cleanup(binding, cleanup)?;
                    if let Some(retired) = self.retired_viewports.get_mut(&token) {
                        retired.status = RetiredViewportStatus::CleanupRequested { effect };
                    }
                }
                continue;
            }
            if !authoritative {
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
                self.terminate_pointer_passthrough_binding(binding);
                self.retired_viewports.remove(&token);
            } else if !may_appear_late {
                self.terminate_pointer_passthrough_binding(binding);
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
                input_observations: WindowInputObservationStream::default(),
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
        self.reduce_ready_restore_replacement(binding);
        self.reduce_ready_recovery_replacement(binding)?;
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
        if let NativeCreateStatus::CommittedAwaitingVisibility { show } = status
            && self
                .registry
                .record(binding.surface())
                .is_some_and(|record| record.binding() == binding && record.is_routeable())
        {
            let _ = self.effects.mark_observed_applied(
                show,
                binding,
                self.registry.inventory_generation(),
            );
            self.create_sagas
                .get_mut(&saga_id)
                .ok_or(ViewportCoordinatorError::MissingCreateSaga { saga: saga_id })?
                .status = NativeCreateStatus::Committed { show: Some(show) };
            actions.push(ViewportLifecycleAction::CreateVisible {
                saga: saga_id,
                binding,
            });
        }
        Ok(())
    }

    fn reduce_ready_restore_replacement(&mut self, binding: ViewportBinding) {
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
    }

    fn reduce_ready_recovery_replacement(
        &mut self,
        binding: ViewportBinding,
    ) -> Result<(), ViewportCoordinatorError> {
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
                RecoveryPendingStatus::ReplacementRegistered
                    | RecoveryPendingStatus::ReplacementRequested { .. }
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
        let presentation = coordinates.presentation();
        let observed: Vec<EffectId> = self
            .effects
            .records()
            .filter_map(|(effect, record)| match record.request().effect() {
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
            .pointer_passthrough_sagas
            .get_mut(&binding)
            .is_some_and(|saga| saga.record_dispatch_result(effect, result, evidence));
        if matched {
            let _ = self.reconcile_pointer_passthrough_saga(binding)?;
        }
        Ok(())
    }

    fn observe_pointer_passthrough_snapshot(&mut self) {
        let bindings: Vec<ViewportBinding> =
            self.pointer_passthrough_sagas.keys().copied().collect();
        for binding in bindings {
            let Some(observation) = self.pointer_passthrough_evidence(binding).observation else {
                continue;
            };
            let Some(effect) = self
                .pointer_passthrough_sagas
                .get(&binding)
                .and_then(|saga| saga.causal_effect(observation))
            else {
                continue;
            };
            let transition = self.effects.mark_observed_applied(
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
            if accepted && let Some(saga) = self.pointer_passthrough_sagas.get_mut(&binding) {
                saga.accept_observed_effect(effect);
            }
        }
    }

    fn reconcile_pointer_passthrough_sagas(&mut self) -> Result<(), ViewportCoordinatorError> {
        let bindings: Vec<ViewportBinding> =
            self.pointer_passthrough_sagas.keys().copied().collect();
        for binding in bindings {
            let _ = self.reconcile_pointer_passthrough_saga(binding)?;
        }
        Ok(())
    }

    fn reconcile_pointer_passthrough_saga(
        &mut self,
        binding: ViewportBinding,
    ) -> Result<Option<EffectId>, ViewportCoordinatorError> {
        let evidence = self.pointer_passthrough_evidence(binding);
        let (enable, restore) =
            self.pointer_passthrough_sagas
                .get(&binding)
                .map_or((None, None), |saga| {
                    (
                        saga.enable.map(|attempt| attempt.effect.effect),
                        saga.restore
                            .and_then(|obligation| obligation.attempt)
                            .map(|attempt| attempt.effect.effect),
                    )
                });
        let enable_phase = enable.and_then(|effect| {
            self.effects
                .record(effect)
                .map(crate::effect::EffectRecord::phase)
        });
        let restore_phase = restore.and_then(|effect| {
            self.effects
                .record(effect)
                .map(crate::effect::EffectRecord::phase)
        });
        let action = self
            .pointer_passthrough_sagas
            .get_mut(&binding)
            .map_or(PointerPassthroughAction::None, |saga| {
                saga.action(evidence, enable_phase, restore_phase)
            });
        match action {
            PointerPassthroughAction::None => Ok(None),
            PointerPassthroughAction::Remove => {
                self.pointer_passthrough_sagas.remove(&binding);
                Ok(None)
            }
            PointerPassthroughAction::RequestEnable => {
                self.request_pointer_passthrough_enable(binding).map(Some)
            }
            PointerPassthroughAction::RequestRestore => self
                .request_pointer_input_restore_in(self.workspace_epoch, binding)
                .map(Some),
        }
    }

    fn request_pointer_passthrough_enable(
        &mut self,
        binding: ViewportBinding,
    ) -> Result<EffectId, ViewportCoordinatorError> {
        let observation = self
            .pointer_passthrough_evidence(binding)
            .authoritative_observation()
            .ok_or(ViewportCoordinatorError::PointerInputObservationMissing { binding })?;
        let after = self.pointer_input_lane_predecessor(binding);
        let effect = self
            .effects
            .request(PlatformEffect::SetPointerPassthrough {
                binding,
                enabled: true,
                after,
            })
            .map_err(ViewportCoordinatorError::Effect)?;
        let saga = self
            .pointer_passthrough_sagas
            .get_mut(&binding)
            .ok_or(ViewportCoordinatorError::PointerPassthroughSagaMissing { binding })?;
        saga.lane_tail = Some(effect);
        saga.enable = Some(PointerInputEnableAttempt {
            effect: PointerInputEffectAttempt::new(effect),
            issued_after: observation.generation(),
        });
        Ok(effect)
    }

    fn request_pointer_input_restore_in(
        &mut self,
        issuance_epoch: WorkspaceEpoch,
        binding: ViewportBinding,
    ) -> Result<EffectId, ViewportCoordinatorError> {
        let issued_after = self
            .pointer_passthrough_evidence(binding)
            .generation_watermark;
        let after = self.pointer_input_lane_predecessor(binding);
        let effect = self
            .effects
            .request_in(
                issuance_epoch,
                PlatformEffect::SetPointerPassthrough {
                    binding,
                    enabled: false,
                    after,
                },
            )
            .map_err(ViewportCoordinatorError::Effect)?;
        let saga = self
            .pointer_passthrough_sagas
            .get_mut(&binding)
            .ok_or(ViewportCoordinatorError::PointerPassthroughSagaMissing { binding })?;
        let restore = saga
            .restore
            .as_mut()
            .ok_or(ViewportCoordinatorError::PointerInputRestoreObligationMissing { binding })?;
        restore.attempt = Some(PointerInputRestoreAttempt {
            effect: PointerInputEffectAttempt::new(effect),
            issued_after,
            terminal_reported_after: None,
        });
        saga.lane_tail = Some(effect);
        Ok(effect)
    }

    fn pointer_input_lane_predecessor(&self, binding: ViewportBinding) -> Option<EffectId> {
        let mut predecessor = self
            .pointer_passthrough_sagas
            .get(&binding)
            .and_then(|saga| saga.lane_tail);
        while let Some(effect) = predecessor {
            let record = self.effects.record(effect)?;
            let invalidated_unemitted = !record.was_emitted()
                && matches!(record.phase(), EffectPhase::InvalidatedByRestore { .. });
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
        PointerPassthroughEvidence {
            observation,
            generation_watermark,
            window_observed,
            routeable,
            capabilities: PointerPassthroughCapabilities {
                observation: self.capabilities.pointer_hit_test_observation(),
                control: self.capabilities.pointer_hit_test_control(),
            },
        }
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
        self.retired_viewports
            .get(&binding.token())
            .filter(|retired| retired.binding == binding)
            .map_or((None, None, false, false), |retired| {
                (
                    retired.input_observations.current(),
                    retired.input_observations.generation_watermark(),
                    retired.observed,
                    false,
                )
            })
    }

    fn terminate_pointer_passthrough_binding(&mut self, binding: ViewportBinding) {
        self.pointer_passthrough_sagas.remove(&binding);
        let had_drag_source = self.drag_sources.values().any(|source| *source == binding);
        self.drag_sources.retain(|_, source| *source != binding);
        if had_drag_source {
            self.routes.clear();
        }
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
                    recovery,
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
        let show = if routeable {
            None
        } else {
            let show = self
                .effects
                .request(PlatformEffect::ShowWindow { binding })
                .map_err(ViewportCoordinatorError::Effect)?;
            Some(show)
        };
        self.create_sagas
            .get_mut(&saga_id)
            .ok_or(ViewportCoordinatorError::MissingCreateSaga { saga: saga_id })?
            .status = match show {
            Some(show) => NativeCreateStatus::CommittedAwaitingVisibility { show },
            None => NativeCreateStatus::Committed { show },
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
        let retry_observation = matches!(
            phase,
            EffectPhase::ObservationDispatchFailed(_) | EffectPhase::ObservationUnsupported(_)
        );
        if !matches!(phase, EffectPhase::DispatchFailed(_)) && !retry_observation {
            return Err(ViewportCoordinatorError::CleanupEffectNotRetryable {
                effect: failed_effect,
                phase,
            });
        }

        let mut candidate = self.clone();
        let retry = if retry_observation {
            candidate
                .retry_retired_cleanup_observation(failed_effect)?
                .ok_or(ViewportCoordinatorError::MissingCleanupEffect {
                    effect: failed_effect,
                })?
        } else if let Some(retry) = candidate.retry_create_cleanup(failed_effect)? {
            retry
        } else if let Some(retry) = candidate.retry_retired_destructive_cleanup(failed_effect)? {
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

    fn retry_retired_destructive_cleanup(
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

    fn retry_retired_cleanup_observation(
        &mut self,
        failed_effect: EffectId,
    ) -> Result<Option<EffectId>, ViewportCoordinatorError> {
        let Some(token) = self.retired_viewports.iter().find_map(|(token, retired)| {
            matches!(
                retired.status,
                RetiredViewportStatus::CleanupObservationFailed { effect }
                    if effect == failed_effect
            )
            .then_some(*token)
        }) else {
            return Ok(None);
        };
        let retired = self
            .retired_viewports
            .get(&token)
            .ok_or(ViewportCoordinatorError::MissingRetiredViewport { token })?;
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
            || *binding != retired.binding
            || predecessor_record.request().effect().binding() != retired.binding
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
    ) -> Result<EffectTransition, ViewportCoordinatorError> {
        let effect = result.effect();
        let transition = self.effects.report(current_epoch, result);
        if transition == EffectTransition::StaleEpoch
            && let Some(token) =
                self.active_cleanup_continuation_for(current_epoch, result.effect())
        {
            let exact = self.effects.report_exact(result);
            if exact == EffectTransition::Applied
                && matches!(
                    result.result(),
                    EffectDispatchResult::DispatchFailed(_) | EffectDispatchResult::Unsupported(_)
                )
            {
                self.retired_viewports
                    .get_mut(&token)
                    .ok_or(ViewportCoordinatorError::MissingRetiredViewport { token })?
                    .status = RetiredViewportStatus::CleanupFailed { effect };
            }
            return Ok(exact);
        }
        if transition != EffectTransition::Applied {
            return Ok(transition);
        }
        self.reduce_pointer_passthrough_dispatch_result(effect, result.result())?;
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
        self.reduce_retired_cleanup_result(effect);
        self.reduce_pending_recovery_result(effect, result.result());
        self.reduce_restore_replacement_result(effect, result.result());
        Ok(transition)
    }

    fn reduce_retired_cleanup_result(&mut self, effect: EffectId) {
        let retired_phase = self
            .effects
            .record(effect)
            .map(crate::effect::EffectRecord::phase);
        if let Some(retired) = self
            .retired_viewports
            .values_mut()
            .find(|retired| retired_effect(retired.status) == Some(effect))
        {
            retired.status = match retired_phase {
                Some(EffectPhase::Indeterminate(_)) => {
                    RetiredViewportStatus::CleanupIndeterminate { effect }
                }
                Some(
                    EffectPhase::ObservationDispatchFailed(_)
                    | EffectPhase::ObservationUnsupported(_),
                ) => RetiredViewportStatus::CleanupObservationFailed { effect },
                Some(EffectPhase::DispatchFailed(_) | EffectPhase::Unsupported(_)) => {
                    RetiredViewportStatus::CleanupFailed { effect }
                }
                _ => retired.status,
            };
        }
    }

    fn reduce_pending_recovery_result(&mut self, effect: EffectId, result: EffectDispatchResult) {
        let recovery_surface = self
            .pending_recoveries
            .iter()
            .find_map(|(surface, pending)| {
                (pending.replacement_effect == Some(effect)).then_some(*surface)
            });
        if let Some(surface) = recovery_surface
            && let Some(pending) = self.pending_recoveries.get_mut(&surface)
        {
            pending.status = match result {
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
    }

    fn reduce_restore_replacement_result(
        &mut self,
        effect: EffectId,
        result: EffectDispatchResult,
    ) {
        if let Some(replacement) = self
            .restore_replacements
            .values_mut()
            .find(|replacement| replacement.effect == effect)
        {
            replacement.status = match result {
                EffectDispatchResult::Indeterminate(_) => {
                    RestoreReplacementStatus::Indeterminate { effect }
                }
                EffectDispatchResult::DispatchFailed(_) | EffectDispatchResult::Unsupported(_) => {
                    let _ = self.registry.discard_unobserved(replacement.binding);
                    RestoreReplacementStatus::Failed { effect }
                }
            };
        }
    }

    fn active_cleanup_continuation_for(
        &self,
        current_epoch: WorkspaceEpoch,
        predecessor: EffectId,
    ) -> Option<WindowToken> {
        self.retired_viewports.iter().find_map(|(token, retired)| {
            let active = retired_effect(retired.status)?;
            let active_record = self.effects.record(active)?;
            if !active_record.was_emitted()
                || active_record.request().epoch() != current_epoch
                || matches!(
                    active_record.phase(),
                    EffectPhase::InvalidatedByRestore { .. }
                        | EffectPhase::ObservedApplied { .. }
                        | EffectPhase::Destroyed { .. }
                )
            {
                return None;
            }
            let PlatformEffect::ContinueCleanup {
                binding,
                predecessor: exact,
                ..
            } = active_record.request().effect()
            else {
                return None;
            };
            if *exact != predecessor {
                return None;
            }
            let predecessor_record = self.effects.record(predecessor)?;
            (*binding == retired.binding
                && predecessor_record.was_emitted()
                && predecessor_record.request().effect().binding() == retired.binding
                && is_destructive_cleanup(predecessor_record.request().effect()))
            .then_some(*token)
        })
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
        let Some(record) = self.registry.record(surface) else {
            return Ok(None);
        };
        let binding = record.binding();
        if self.drag_sources.get(&pointer) == Some(&binding) {
            return Ok(None);
        }
        let mut candidate = self.clone();
        let restored = candidate.end_drag_routing(pointer)?;
        if let Some(saga) = candidate.pointer_passthrough_sagas.get_mut(&binding) {
            saga.holders.insert(pointer);
        } else {
            candidate
                .pointer_passthrough_sagas
                .insert(binding, PointerPassthroughSaga::new(pointer));
        }
        candidate.drag_sources.insert(pointer, binding);
        candidate.routes.clear();
        let effect = candidate.reconcile_pointer_passthrough_saga(binding)?;
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
        candidate
            .pointer_passthrough_sagas
            .get_mut(&binding)
            .ok_or(ViewportCoordinatorError::PointerPassthroughSagaMissing { binding })?
            .holders
            .remove(&pointer);
        let effect = candidate.reconcile_pointer_passthrough_saga(binding)?;
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
            let invalidated_unemitted = !record.was_emitted()
                && matches!(record.phase(), EffectPhase::InvalidatedByRestore { .. });
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
        effect: EffectId,
        binding: ViewportBinding,
    ) -> EffectTransition {
        self.effects
            .mark_observed_applied(effect, binding, self.registry.inventory_generation())
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
            role,
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
        | RetiredViewportStatus::CleanupObservationFailed { effect }
        | RetiredViewportStatus::CleanupFailed { effect } => Some(effect),
        RetiredViewportStatus::AwaitingAppearance => None,
    }
}

const fn is_destructive_cleanup(effect: &PlatformEffect) -> bool {
    matches!(
        effect,
        PlatformEffect::CompensatingClose { .. }
            | PlatformEffect::ReleaseChild { .. }
            | PlatformEffect::RequestRootClose { .. }
    )
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
    #[error("registered replacement does not match pending recovery for surface {surface:?}")]
    PendingRecoveryRegistrationMismatch { surface: SurfaceId },
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
    #[error("viewport close request {request:?} does not exist")]
    MissingCloseRequest { request: ViewportCloseRequestId },
    #[error("viewport close request {request:?} was already decided: {status:?}")]
    CloseRequestAlreadyDecided {
        request: ViewportCloseRequestId,
        status: ViewportCloseStatus,
    },
    #[error("viewport close destruction authority is unavailable: {capability:?}")]
    CloseDestructionAuthorityUnavailable { capability: PlatformCapability },
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::effect::EffectPhase;
    use crate::geometry::{LogicalSize, PhysicalRect, ScaleFactor};
    use crate::ids::{FloatingPresentationId, RootId};
    use crate::intent::{
        Authority, AuthorityUnavailableReason, ContainedPlacementProof, ContainedTearOffProposal,
    };
    use crate::platform::{
        InputEffectAcknowledgement, ObservedWindow, WindowInputObservation, WindowInputState,
        WindowPresentationState,
    };
    use crate::scene::{SceneGeneration, SceneStamp};
    use crate::transition::WorkspaceVersion;
    use crate::viewport_focus::{FocusObservationGeneration, unknown_focus_observation};

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
        PlatformSnapshot::new(
            capabilities,
            unknown_focus_observation(
                FocusObservationGeneration::new(1),
                AuthorityUnavailableReason::NotReported,
            ),
            windows,
            Vec::new(),
            Vec::new(),
        )
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
        let facts = routing_snapshot(
            binding,
            1,
            input,
            presentation,
            control,
            InputEffectAcknowledgement::known(None),
        );
        coordinator
            .publish_snapshot(&facts)
            .expect("test routing facts must publish");
        (coordinator, binding)
    }

    fn routing_snapshot(
        binding: ViewportBinding,
        generation: u64,
        input: WindowInputState,
        presentation: WindowPresentationState,
        control: PlatformCapability,
        acknowledgement: InputEffectAcknowledgement,
    ) -> PlatformSnapshot {
        routing_snapshot_with_input_authority(
            binding,
            generation,
            Authority::Known(input),
            presentation,
            control,
            acknowledgement,
        )
    }

    fn routing_snapshot_with_input_authority(
        binding: ViewportBinding,
        generation: u64,
        input: Authority<WindowInputState>,
        presentation: WindowPresentationState,
        control: PlatformCapability,
        acknowledgement: InputEffectAcknowledgement,
    ) -> PlatformSnapshot {
        let window = observed_window(binding.token(), false)
            .with_input_observation(WindowInputObservation::new(
                binding,
                InputObservationGeneration::new(generation),
                input,
                acknowledgement,
            ))
            .with_presentation(Authority::Known(presentation));
        let mut capabilities = PlatformCapabilities::default();
        capabilities.set_authoritative_inventory(PlatformCapability::Supported);
        capabilities.set_pointer_hit_test_observation(PlatformCapability::Supported);
        capabilities.set_pointer_hit_test_control(control);
        PlatformSnapshot::new(
            capabilities,
            unknown_focus_observation(
                FocusObservationGeneration::new(generation),
                AuthorityUnavailableReason::NotReported,
            ),
            vec![window],
            Vec::new(),
            Vec::new(),
        )
        .expect("test routing snapshot must be valid")
    }

    fn released_unobserved_enable() -> (ViewportCoordinator, ViewportBinding, EffectId, EffectId) {
        let (mut coordinator, binding) = routing_coordinator(
            WindowInputState::ReceivesInput,
            WindowPresentationState::Visible,
            PlatformCapability::Supported,
        );
        let enable = coordinator
            .begin_drag_routing(PointerId::new(1), binding.surface())
            .expect("drag routing must begin")
            .expect("enable effect must exist");
        let _ = coordinator.take_new_effects();
        let restore = coordinator
            .end_drag_routing(PointerId::new(1))
            .expect("drag release must preserve restoration")
            .expect("release must queue restoration behind the enable attempt");
        let requests = coordinator.take_new_effects();
        assert!(matches!(
            requests.as_slice(),
            [request]
                if request.id() == restore
                    && matches!(
                        request.effect(),
                        PlatformEffect::SetPointerPassthrough {
                            binding: actual,
                            enabled: false,
                            after: Some(predecessor),
                        } if *actual == binding && *predecessor == enable
                    )
        ));
        (coordinator, binding, enable, restore)
    }

    fn take_single_pointer_restore(
        coordinator: &mut ViewportCoordinator,
        binding: ViewportBinding,
        expected_after: Option<EffectId>,
    ) -> EffectId {
        let restore_effects = coordinator.take_new_effects();
        let [restore_request] = restore_effects.as_slice() else {
            panic!("passthrough edge must issue exactly one restore: {restore_effects:?}");
        };
        assert_eq!(
            restore_request.effect(),
            &PlatformEffect::SetPointerPassthrough {
                binding,
                enabled: false,
                after: expected_after,
            }
        );
        restore_request.id()
    }

    fn report_effect_result(
        coordinator: &mut ViewportCoordinator,
        current_epoch: WorkspaceEpoch,
        effect: EffectId,
        issuance_epoch: WorkspaceEpoch,
        result: EffectDispatchResult,
    ) -> EffectTransition {
        coordinator
            .report_effect(
                current_epoch,
                EffectResult::new(effect, issuance_epoch, result),
            )
            .expect("effect result must reduce")
    }

    fn migrated_retired_effects(
        requests: &[EffectRequest],
        binding: ViewportBinding,
        cleanup: EffectId,
        old_restore: EffectId,
    ) -> (EffectId, EffectId) {
        let cleanup_successor = requests
            .iter()
            .find_map(|request| match request.effect() {
                PlatformEffect::ContinueCleanup {
                    binding: actual,
                    predecessor,
                    ..
                } if *actual == binding && *predecessor == cleanup => Some(request.id()),
                _ => None,
            })
            .expect("emitted cleanup must receive an observation-only successor");
        let restore_successor = requests
            .iter()
            .find_map(|request| match request.effect() {
                PlatformEffect::SetPointerPassthrough {
                    binding: actual,
                    enabled: false,
                    after: Some(predecessor),
                } if *actual == binding && *predecessor == old_restore => Some(request.id()),
                _ => None,
            })
            .expect("retired pointer restore must receive a causally ordered successor");
        assert!(requests.iter().all(|request| {
            !matches!(
                request.effect(),
                PlatformEffect::RequestRootClose { .. } | PlatformEffect::ReleaseChild { .. }
            )
        }));
        (cleanup_successor, restore_successor)
    }

    struct RepeatedRestoreBoundary {
        coordinator: ViewportCoordinator,
        binding: ViewportBinding,
        enable: EffectId,
        destructive: EffectId,
        fourth_epoch: WorkspaceEpoch,
        emitted_cleanup: EffectId,
        emitted_restore: EffectId,
        unemitted_cleanup: EffectId,
        unemitted_restore: EffectId,
        current: Vec<EffectRequest>,
    }

    fn repeated_restore_boundary() -> RepeatedRestoreBoundary {
        let (mut coordinator, binding, enable, old_restore) = released_unobserved_enable();
        coordinator
            .reconcile_workspace_epoch(WorkspaceEpoch::new(1), &BTreeSet::new())
            .expect("first restore must retire the source binding");
        let destructive = coordinator
            .take_new_effects()
            .into_iter()
            .find_map(|request| {
                matches!(
                    request.effect(),
                    PlatformEffect::RequestRootClose { binding: actual } if *actual == binding
                )
                .then_some(request.id())
            })
            .expect("first restore must emit one destructive cleanup");

        coordinator
            .reconcile_workspace_epoch(WorkspaceEpoch::new(2), &BTreeSet::new())
            .expect("second restore must establish emitted observation lanes");
        let emitted = coordinator.take_new_effects();
        let (emitted_cleanup, emitted_restore) =
            migrated_retired_effects(&emitted, binding, destructive, old_restore);

        let third = coordinator
            .reconcile_workspace_epoch(WorkspaceEpoch::new(3), &BTreeSet::new())
            .expect("third restore must queue observation successors");
        let unemitted_cleanup = third
            .cleanup_effects()
            .iter()
            .copied()
            .find(|effect| {
                matches!(
                    coordinator
                        .effects()
                        .record(*effect)
                        .map(|record| record.request().effect()),
                    Some(PlatformEffect::ContinueCleanup { .. })
                )
            })
            .expect("third restore must queue a cleanup observation successor");
        let unemitted_restore = third
            .cleanup_effects()
            .iter()
            .copied()
            .find(|effect| {
                matches!(
                    coordinator
                        .effects()
                        .record(*effect)
                        .map(|record| record.request().effect()),
                    Some(PlatformEffect::SetPointerPassthrough { enabled: false, .. })
                )
            })
            .expect("third restore must queue a pointer restore successor");

        let fourth_epoch = WorkspaceEpoch::new(4);
        coordinator
            .reconcile_workspace_epoch(fourth_epoch, &BTreeSet::new())
            .expect("fourth restore must migrate both observation lanes");
        let current = coordinator.take_new_effects();
        RepeatedRestoreBoundary {
            coordinator,
            binding,
            enable,
            destructive,
            fourth_epoch,
            emitted_cleanup,
            emitted_restore,
            unemitted_cleanup,
            unemitted_restore,
            current,
        }
    }

    struct MigratedRetiredObligations {
        coordinator: ViewportCoordinator,
        binding: ViewportBinding,
        old_restore: EffectId,
        cleanup: EffectId,
        cleanup_successor: EffectId,
        restore_successor: EffectId,
        first_epoch: WorkspaceEpoch,
        second_epoch: WorkspaceEpoch,
    }

    fn migrated_retired_obligations(restore_was_indeterminate: bool) -> MigratedRetiredObligations {
        let (mut coordinator, binding, _, old_restore) = released_unobserved_enable();
        if restore_was_indeterminate {
            assert_eq!(
                report_effect_result(
                    &mut coordinator,
                    binding.epoch(),
                    old_restore,
                    binding.epoch(),
                    EffectDispatchResult::Indeterminate(
                        crate::effect::EffectIndeterminateReason::AcknowledgementLost,
                    ),
                ),
                EffectTransition::Applied
            );
        }

        let first_epoch = WorkspaceEpoch::new(1);
        coordinator
            .reconcile_workspace_epoch(first_epoch, &BTreeSet::new())
            .expect("first workspace replacement must retire the binding");
        let cleanup = coordinator
            .take_new_effects()
            .iter()
            .find_map(|request| match request.effect() {
                PlatformEffect::RequestRootClose { binding: actual } if *actual == binding => {
                    Some(request.id())
                }
                _ => None,
            })
            .expect("first restore must emit one destructive cleanup");

        let second_epoch = WorkspaceEpoch::new(2);
        coordinator
            .reconcile_workspace_epoch(second_epoch, &BTreeSet::new())
            .expect("second workspace replacement must migrate retired obligations");
        let migrated = coordinator.take_new_effects();
        let (cleanup_successor, restore_successor) =
            migrated_retired_effects(&migrated, binding, cleanup, old_restore);
        MigratedRetiredObligations {
            coordinator,
            binding,
            old_restore,
            cleanup,
            cleanup_successor,
            restore_successor,
            first_epoch,
            second_epoch,
        }
    }

    fn assert_cleanup_observation_successor_is_retryable(fixture: &mut MigratedRetiredObligations) {
        assert_eq!(
            report_effect_result(
                &mut fixture.coordinator,
                fixture.second_epoch,
                fixture.old_restore,
                fixture.binding.epoch(),
                EffectDispatchResult::DispatchFailed(
                    crate::effect::DispatchFailureReason::ProviderStopped,
                ),
            ),
            EffectTransition::StaleEpoch
        );
        assert_eq!(
            report_effect_result(
                &mut fixture.coordinator,
                fixture.second_epoch,
                fixture.cleanup_successor,
                fixture.second_epoch,
                EffectDispatchResult::DispatchFailed(
                    crate::effect::DispatchFailureReason::ProviderStopped,
                ),
            ),
            EffectTransition::Applied
        );
        assert_eq!(
            fixture
                .coordinator
                .effects()
                .record(fixture.cleanup_successor)
                .map(crate::effect::EffectRecord::phase),
            Some(EffectPhase::ObservationDispatchFailed(
                crate::effect::DispatchFailureReason::ProviderStopped,
            ))
        );

        let observation_retry = fixture
            .coordinator
            .retry_cleanup(fixture.cleanup_successor)
            .expect("provider recovery must retry only cleanup observation");
        let retry_requests = fixture.coordinator.take_new_effects();
        assert!(matches!(
            retry_requests.as_slice(),
            [request]
                if request.id() == observation_retry
                    && matches!(
                        request.effect(),
                        PlatformEffect::ContinueCleanup {
                            binding: actual,
                            predecessor,
                            after: Some(after),
                        } if *actual == fixture.binding
                            && *predecessor == fixture.cleanup
                            && *after == fixture.cleanup_successor
                    )
        ));
    }

    fn assert_destructive_cleanup_result_requires_its_issuance_epoch(
        fixture: &mut MigratedRetiredObligations,
    ) {
        let failure = EffectDispatchResult::DispatchFailed(
            crate::effect::DispatchFailureReason::ProviderStopped,
        );
        assert_eq!(
            report_effect_result(
                &mut fixture.coordinator,
                fixture.second_epoch,
                fixture.cleanup,
                fixture.second_epoch,
                failure,
            ),
            EffectTransition::StaleEpoch
        );
        assert_eq!(
            report_effect_result(
                &mut fixture.coordinator,
                fixture.second_epoch,
                fixture.cleanup,
                fixture.first_epoch,
                failure,
            ),
            EffectTransition::Applied
        );
        assert_eq!(
            fixture
                .coordinator
                .effects()
                .record(fixture.cleanup)
                .map(crate::effect::EffectRecord::phase),
            Some(EffectPhase::DispatchFailed(
                crate::effect::DispatchFailureReason::ProviderStopped,
            ))
        );
        assert_eq!(
            fixture
                .coordinator
                .retired_viewports()
                .find_map(|(token, retired)| {
                    (token == fixture.binding.token()).then_some(retired.status())
                }),
            Some(RetiredViewportStatus::CleanupFailed {
                effect: fixture.cleanup,
            })
        );
    }

    fn settle_migrated_pointer_restore(fixture: &mut MigratedRetiredObligations) {
        fixture
            .coordinator
            .publish_snapshot(&routing_snapshot(
                fixture.binding,
                2,
                WindowInputState::ReceivesInput,
                WindowPresentationState::Visible,
                PlatformCapability::Supported,
                InputEffectAcknowledgement::known(Some(fixture.restore_successor)),
            ))
            .expect("exact current restore acknowledgement must settle the obligation");
        assert!(
            !fixture
                .coordinator
                .pointer_passthrough_sagas
                .contains_key(&fixture.binding)
        );
        assert!(matches!(
            fixture
                .coordinator
                .effects()
                .record(fixture.restore_successor)
                .expect("restore successor must remain auditable")
                .phase(),
            EffectPhase::ObservedApplied { .. }
        ));
    }

    fn failed_cleanup_observation() -> (
        ViewportCoordinator,
        ViewportBinding,
        EffectId,
        EffectId,
        WorkspaceEpoch,
        WorkspaceEpoch,
    ) {
        let (mut coordinator, binding) = routing_coordinator(
            WindowInputState::ReceivesInput,
            WindowPresentationState::Visible,
            PlatformCapability::Supported,
        );
        let destructive_epoch = WorkspaceEpoch::new(1);
        coordinator
            .reconcile_workspace_epoch(destructive_epoch, &BTreeSet::new())
            .expect("first restore must retire the binding");
        let destructive = coordinator
            .take_new_effects()
            .into_iter()
            .find_map(|request| {
                matches!(
                    request.effect(),
                    PlatformEffect::RequestRootClose { binding: actual } if *actual == binding
                )
                .then_some(request.id())
            })
            .expect("first restore must emit destructive cleanup");
        let observation_epoch = WorkspaceEpoch::new(2);
        coordinator
            .reconcile_workspace_epoch(observation_epoch, &BTreeSet::new())
            .expect("second restore must continue cleanup observation");
        let continuation = coordinator
            .take_new_effects()
            .into_iter()
            .find_map(|request| {
                matches!(
                    request.effect(),
                    PlatformEffect::ContinueCleanup {
                        binding: actual,
                        predecessor,
                        ..
                    } if *actual == binding && *predecessor == destructive
                )
                .then_some(request.id())
            })
            .expect("second restore must emit cleanup continuation");
        assert_eq!(
            report_effect_result(
                &mut coordinator,
                observation_epoch,
                continuation,
                observation_epoch,
                EffectDispatchResult::DispatchFailed(
                    crate::effect::DispatchFailureReason::ProviderStopped,
                ),
            ),
            EffectTransition::Applied
        );
        (
            coordinator,
            binding,
            destructive,
            continuation,
            destructive_epoch,
            observation_epoch,
        )
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
            unknown_focus_observation(
                FocusObservationGeneration::new(1),
                AuthorityUnavailableReason::NotReported,
            ),
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
            unknown_focus_observation(
                FocusObservationGeneration::new(1),
                AuthorityUnavailableReason::NotReported,
            ),
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
    fn focus_requests_form_one_provider_serialized_global_lane() {
        let mut coordinator = ViewportCoordinator::default();
        let first_binding = register(&mut coordinator, 7, 70, ViewportRole::Root);
        let second_binding = register(&mut coordinator, 8, 80, ViewportRole::Root);
        let first = coordinator
            .request_focus_binding(first_binding)
            .expect("first focus request must enter the ledger");
        let second = coordinator
            .request_focus_binding(second_binding)
            .expect("second focus request must enter the ledger");
        assert!(matches!(
            coordinator.take_new_effects().as_slice(),
            [first_request, second_request]
                if first_request.id() == first
                    && second_request.id() == second
                    && matches!(
                        first_request.effect(),
                        PlatformEffect::RequestFocus {
                            binding,
                            after: None,
                        } if *binding == first_binding
                    )
                    && matches!(
                        second_request.effect(),
                        PlatformEffect::RequestFocus {
                            binding,
                            after: Some(predecessor),
                        } if *binding == second_binding && *predecessor == first
                    )
        ));
    }

    #[test]
    fn restore_skips_only_a_focus_predecessor_the_provider_never_received() {
        for emit_old in [false, true] {
            let mut coordinator = ViewportCoordinator::default();
            let old_binding = register(&mut coordinator, 9, 90, ViewportRole::Root);
            let old = coordinator
                .request_focus_binding(old_binding)
                .expect("old focus request must allocate");
            if emit_old {
                let emitted = coordinator.take_new_effects();
                assert_eq!(emitted.len(), 1);
                assert_eq!(emitted[0].id(), old);
            }

            let desired = BTreeSet::from([old_binding.surface()]);
            coordinator
                .reconcile_workspace_epoch(WorkspaceEpoch::new(2), &desired)
                .expect("restore must reconcile focus history");
            let existing_binding = coordinator
                .registry()
                .record(old_binding.surface())
                .map(ViewportRecord::binding);
            let new_binding = existing_binding.unwrap_or_else(|| {
                coordinator
                    .register_existing(
                        WorkspaceEpoch::new(2),
                        old_binding.surface(),
                        WindowToken::new(91),
                        ViewportRole::Root,
                        None,
                    )
                    .expect("restored binding must register")
            });
            let new = coordinator
                .request_focus_binding(new_binding)
                .expect("new focus request must allocate");
            let request = coordinator
                .take_new_effects()
                .into_iter()
                .find(|request| request.id() == new)
                .expect("new focus request must be emitted");
            assert!(matches!(
                request.effect(),
                PlatformEffect::RequestFocus { after, .. }
                    if *after == emit_old.then_some(old)
            ));
        }
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
                .decide_viewport_close(*request, ViewportCloseDecision::Prevent)
                .expect("veto must enter the effect ledger");
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
                coordinator.decide_viewport_close(*request, ViewportCloseDecision::Prevent),
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
        let plan = ViewportClosePlan::retain_layout();
        let effect = coordinator
            .decide_viewport_close(request, ViewportCloseDecision::Accept(plan.clone()))
            .expect("accept must enter the release effect");
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
                    recovery: actual_recovery,
                },
            }] if *actual == binding
                && *actual_request == request
                && *actual_plan == plan
                && *actual_recovery == Some(recovery.into())
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
        let enable = requested[0].id();
        assert!(matches!(
            requested.as_slice(),
            [request]
                if matches!(
                    request.effect(),
                    PlatformEffect::SetPointerPassthrough {
                        binding: actual,
                        enabled: true,
                        ..
                    } if *actual == binding
                )
        ));
        coordinator
            .publish_snapshot(&routing_snapshot(
                binding,
                2,
                WindowInputState::PassThrough,
                WindowPresentationState::Visible,
                PlatformCapability::Supported,
                InputEffectAcknowledgement::known(Some(enable)),
            ))
            .expect("enable must be observed before restoration can be confirmed");

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
        let restore = restored[0].id();
        assert!(matches!(
            restored.as_slice(),
            [request]
                if matches!(
                    request.effect(),
                    PlatformEffect::SetPointerPassthrough {
                        binding: actual,
                        enabled: false,
                        ..
                    } if *actual == binding
                )
        ));
        assert!(coordinator.pointer_passthrough_sagas.contains_key(&binding));

        coordinator
            .publish_snapshot(&routing_snapshot(
                binding,
                3,
                WindowInputState::ReceivesInput,
                WindowPresentationState::Visible,
                PlatformCapability::Supported,
                InputEffectAcknowledgement::known(Some(restore)),
            ))
            .expect("restored input observation must publish");
        assert!(!coordinator.pointer_passthrough_sagas.contains_key(&binding));
    }

    #[test]
    fn wrong_effect_and_stale_capture_cannot_attribute_a_late_enable() {
        let (mut coordinator, binding) = routing_coordinator(
            WindowInputState::ReceivesInput,
            WindowPresentationState::Visible,
            PlatformCapability::Supported,
        );
        let pointer = PointerId::new(1);
        let enable = coordinator
            .begin_drag_routing(pointer, binding.surface())
            .expect("drag routing must begin")
            .expect("receiving input must request passthrough");
        let _ = coordinator.take_new_effects();
        let restore = coordinator
            .end_drag_routing(pointer)
            .expect("drag routing must end")
            .expect("release must queue restoration");
        assert_eq!(
            take_single_pointer_restore(&mut coordinator, binding, Some(enable)),
            restore
        );

        coordinator
            .publish_snapshot(&routing_snapshot(
                binding,
                3,
                WindowInputState::ReceivesInput,
                WindowPresentationState::Visible,
                PlatformCapability::Supported,
                InputEffectAcknowledgement::known(None),
            ))
            .expect("newer receiving observation must publish");
        coordinator
            .publish_snapshot(&routing_snapshot(
                binding,
                2,
                WindowInputState::PassThrough,
                WindowPresentationState::Visible,
                PlatformCapability::Supported,
                InputEffectAcknowledgement::known(Some(enable)),
            ))
            .expect("stale exact acknowledgement must be ignored");
        assert!(coordinator.take_new_effects().is_empty());

        coordinator
            .publish_snapshot(&routing_snapshot(
                binding,
                4,
                WindowInputState::PassThrough,
                WindowPresentationState::Visible,
                PlatformCapability::Supported,
                InputEffectAcknowledgement::known(Some(EffectId::new(900))),
            ))
            .expect("wrong effect acknowledgement must publish without attribution");
        assert!(coordinator.take_new_effects().is_empty());
        assert!(coordinator.pointer_passthrough_sagas.contains_key(&binding));

        coordinator
            .publish_snapshot(&routing_snapshot(
                binding,
                5,
                WindowInputState::PassThrough,
                WindowPresentationState::Visible,
                PlatformCapability::Supported,
                InputEffectAcknowledgement::known(Some(enable)),
            ))
            .expect("fresh exact enable acknowledgement must publish");
        assert!(coordinator.take_new_effects().is_empty());
        assert!(matches!(
            coordinator
                .effects()
                .record(restore)
                .expect("queued restore must remain")
                .phase(),
            EffectPhase::Requested
        ));

        coordinator
            .publish_snapshot(&routing_snapshot(
                binding,
                6,
                WindowInputState::ReceivesInput,
                WindowPresentationState::Visible,
                PlatformCapability::Supported,
                InputEffectAcknowledgement::known(Some(restore)),
            ))
            .expect("exact restore acknowledgement must settle the saga");
        assert!(!coordinator.pointer_passthrough_sagas.contains_key(&binding));
    }

    #[test]
    fn unknown_acknowledgement_does_not_erase_known_input_state_authority() {
        let (mut coordinator, binding) = routing_coordinator(
            WindowInputState::ReceivesInput,
            WindowPresentationState::Visible,
            PlatformCapability::Supported,
        );
        coordinator
            .publish_snapshot(&routing_snapshot(
                binding,
                2,
                WindowInputState::ReceivesInput,
                WindowPresentationState::Visible,
                PlatformCapability::Supported,
                InputEffectAcknowledgement::unknown(
                    crate::intent::AuthorityUnavailableReason::NotReported,
                ),
            ))
            .expect("unknown acknowledgement authority must publish");

        let enable = coordinator
            .begin_drag_routing(PointerId::new(1), binding.surface())
            .expect("drag routing must remain total")
            .expect("known receiving state must request passthrough");
        assert!(matches!(
            coordinator.take_new_effects().as_slice(),
            [request]
                if request.id() == enable
                    && matches!(
                        request.effect(),
                        PlatformEffect::SetPointerPassthrough {
                            binding: actual,
                            enabled: true,
                            ..
                        } if *actual == binding
                    )
        ));
    }

    #[test]
    fn newer_passthrough_state_with_unknown_ack_does_not_retry_failed_enable() {
        let (mut coordinator, binding) = routing_coordinator(
            WindowInputState::ReceivesInput,
            WindowPresentationState::Visible,
            PlatformCapability::Supported,
        );
        let pointer = PointerId::new(1);
        let enable = coordinator
            .begin_drag_routing(pointer, binding.surface())
            .expect("drag routing must begin")
            .expect("receiving input must request passthrough");
        let _ = coordinator.take_new_effects();
        assert_eq!(
            coordinator
                .report_effect(
                    binding.epoch(),
                    EffectResult::new(
                        enable,
                        binding.epoch(),
                        EffectDispatchResult::DispatchFailed(
                            crate::effect::DispatchFailureReason::AdapterRejected,
                        ),
                    ),
                )
                .expect("enable result must reduce"),
            EffectTransition::Applied
        );
        let unavailable = crate::intent::AuthorityUnavailableReason::NotReported;
        coordinator
            .publish_snapshot(&routing_snapshot(
                binding,
                2,
                WindowInputState::PassThrough,
                WindowPresentationState::Visible,
                PlatformCapability::Supported,
                InputEffectAcknowledgement::unknown(unavailable),
            ))
            .expect("newer passthrough state must publish without exact attribution");
        assert!(coordinator.take_new_effects().is_empty());

        let restore = coordinator
            .end_drag_routing(pointer)
            .expect("drag release must preserve restoration")
            .expect("known passthrough state must still be restored");
        assert_eq!(
            take_single_pointer_restore(&mut coordinator, binding, Some(enable)),
            restore
        );
    }

    #[test]
    fn definitive_enable_failure_after_release_preserves_queued_restoration() {
        for result in [
            EffectDispatchResult::DispatchFailed(
                crate::effect::DispatchFailureReason::AdapterRejected,
            ),
            EffectDispatchResult::Unsupported(
                crate::effect::EffectUnsupportedReason::BackendUnsupported,
            ),
        ] {
            let (mut coordinator, binding, enable, restore) = released_unobserved_enable();
            assert_eq!(
                coordinator
                    .report_effect(
                        binding.epoch(),
                        EffectResult::new(enable, binding.epoch(), result),
                    )
                    .expect("effect report must reduce"),
                EffectTransition::Applied
            );
            assert!(coordinator.pointer_passthrough_sagas.contains_key(&binding));
            assert!(coordinator.take_new_effects().is_empty());
            assert!(matches!(
                coordinator
                    .effects()
                    .record(restore)
                    .expect("queued restore must remain durable")
                    .phase(),
                EffectPhase::Requested
            ));
        }
    }

    #[test]
    fn late_enable_after_active_failure_still_restores_after_release() {
        let (mut coordinator, binding) = routing_coordinator(
            WindowInputState::ReceivesInput,
            WindowPresentationState::Visible,
            PlatformCapability::Supported,
        );
        let pointer = PointerId::new(1);
        let enable = coordinator
            .begin_drag_routing(pointer, binding.surface())
            .expect("drag routing must begin")
            .expect("receiving input must request passthrough");
        let _ = coordinator.take_new_effects();
        assert_eq!(
            coordinator
                .report_effect(
                    binding.epoch(),
                    EffectResult::new(
                        enable,
                        binding.epoch(),
                        EffectDispatchResult::DispatchFailed(
                            crate::effect::DispatchFailureReason::AdapterRejected,
                        ),
                    ),
                )
                .expect("effect report must reduce"),
            EffectTransition::Applied
        );

        coordinator
            .publish_snapshot(&routing_snapshot(
                binding,
                2,
                WindowInputState::PassThrough,
                WindowPresentationState::Visible,
                PlatformCapability::Supported,
                InputEffectAcknowledgement::known(Some(enable)),
            ))
            .expect("late enable observation must remain causal");
        assert!(matches!(
            coordinator
                .effects()
                .record(enable)
                .expect("enable effect must remain queryable")
                .phase(),
            EffectPhase::ObservedApplied { .. }
        ));

        let restore = coordinator
            .end_drag_routing(pointer)
            .expect("drag release must preserve restoration")
            .expect("late enable must be followed by restoration");
        assert_eq!(
            take_single_pointer_restore(&mut coordinator, binding, Some(enable)),
            restore
        );
    }

    #[test]
    fn lost_enable_acknowledgement_cannot_block_restore_queue() {
        let (mut coordinator, binding) = routing_coordinator(
            WindowInputState::ReceivesInput,
            WindowPresentationState::Visible,
            PlatformCapability::Supported,
        );
        let pointer = PointerId::new(1);
        let enable = coordinator
            .begin_drag_routing(pointer, binding.surface())
            .expect("drag routing must begin")
            .expect("receiving input must request passthrough");
        let _ = coordinator.take_new_effects();
        assert_eq!(
            coordinator
                .report_effect(
                    binding.epoch(),
                    EffectResult::new(
                        enable,
                        binding.epoch(),
                        EffectDispatchResult::Indeterminate(
                            crate::effect::EffectIndeterminateReason::AcknowledgementLost,
                        ),
                    ),
                )
                .expect("effect report must reduce"),
            EffectTransition::Applied
        );

        let restore = coordinator
            .end_drag_routing(pointer)
            .expect("drag release must preserve restoration")
            .expect("indeterminate enable must still queue restoration");
        assert_eq!(
            take_single_pointer_restore(&mut coordinator, binding, Some(enable)),
            restore
        );
        assert!(coordinator.pointer_passthrough_sagas.contains_key(&binding));
    }

    #[test]
    fn control_loss_between_enable_and_release_cannot_block_the_first_restore() {
        let (mut coordinator, binding) = routing_coordinator(
            WindowInputState::ReceivesInput,
            WindowPresentationState::Visible,
            PlatformCapability::Supported,
        );
        let pointer = PointerId::new(1);
        let enable = coordinator
            .begin_drag_routing(pointer, binding.surface())
            .expect("drag routing must begin")
            .expect("receiving input must request passthrough");
        let _ = coordinator.take_new_effects();
        let unavailable = PlatformCapability::unsupported(
            crate::platform::PlatformRequirement::PointerHitTestControl,
            crate::platform::PlatformCapabilityReason::BackendUnsupported,
        );
        coordinator
            .publish_snapshot(&routing_snapshot(
                binding,
                2,
                WindowInputState::ReceivesInput,
                WindowPresentationState::Visible,
                unavailable,
                InputEffectAcknowledgement::known(None),
            ))
            .expect("control loss must remain representable");
        let restore = coordinator
            .end_drag_routing(pointer)
            .expect("drag release must preserve the restore obligation")
            .expect("control loss must not block the first restore request");
        assert_eq!(
            take_single_pointer_restore(&mut coordinator, binding, Some(enable)),
            restore
        );
        assert_eq!(
            coordinator
                .report_effect(
                    binding.epoch(),
                    EffectResult::new(
                        restore,
                        binding.epoch(),
                        EffectDispatchResult::Unsupported(
                            crate::effect::EffectUnsupportedReason::CapabilityRevoked,
                        ),
                    ),
                )
                .expect("effect report must reduce"),
            EffectTransition::Applied
        );
        assert!(coordinator.take_new_effects().is_empty());

        coordinator
            .publish_snapshot(&routing_snapshot(
                binding,
                3,
                WindowInputState::PassThrough,
                WindowPresentationState::Visible,
                unavailable,
                InputEffectAcknowledgement::known(Some(enable)),
            ))
            .expect("late enable under control loss must preserve restoration");
        assert!(coordinator.take_new_effects().is_empty());

        coordinator
            .publish_snapshot(&routing_snapshot(
                binding,
                4,
                WindowInputState::PassThrough,
                WindowPresentationState::Visible,
                PlatformCapability::Supported,
                InputEffectAcknowledgement::known(Some(enable)),
            ))
            .expect("control recovery must unlock one restore retry");
        let retry = take_single_pointer_restore(&mut coordinator, binding, Some(restore));
        assert!(coordinator.pointer_passthrough_sagas.contains_key(&binding));
        assert_ne!(retry, restore);
    }

    #[test]
    fn versioned_gap_watermark_allows_newer_safe_state_to_settle_failed_restore() {
        let (mut coordinator, binding) = routing_coordinator(
            WindowInputState::ReceivesInput,
            WindowPresentationState::Visible,
            PlatformCapability::Supported,
        );
        let pointer = PointerId::new(1);
        let enable = coordinator
            .begin_drag_routing(pointer, binding.surface())
            .expect("drag routing must begin")
            .expect("receiving input must request passthrough");
        let _ = coordinator.take_new_effects();
        let unavailable = crate::intent::AuthorityUnavailableReason::NotReported;
        coordinator
            .publish_snapshot(&routing_snapshot_with_input_authority(
                binding,
                2,
                Authority::Unknown(unavailable),
                WindowPresentationState::Visible,
                PlatformCapability::Supported,
                InputEffectAcknowledgement::unknown(unavailable),
            ))
            .expect("versioned input tombstone must publish");
        assert_eq!(
            coordinator
                .registry()
                .record(binding.surface())
                .expect("source binding must remain registered")
                .input_observation(),
            None
        );

        let restore = coordinator
            .end_drag_routing(pointer)
            .expect("drag release must preserve restoration")
            .expect("versioned input gap must not block restoration");
        assert_eq!(
            take_single_pointer_restore(&mut coordinator, binding, Some(enable)),
            restore
        );
        assert_eq!(
            coordinator
                .pointer_passthrough_sagas
                .get(&binding)
                .and_then(|saga| saga.restore)
                .and_then(|obligation| obligation.attempt)
                .and_then(|attempt| attempt.issued_after),
            Some(InputObservationGeneration::new(2))
        );
        assert_eq!(
            coordinator
                .report_effect(
                    binding.epoch(),
                    EffectResult::new(
                        restore,
                        binding.epoch(),
                        EffectDispatchResult::DispatchFailed(
                            crate::effect::DispatchFailureReason::AdapterRejected,
                        ),
                    ),
                )
                .expect("restore result must reduce"),
            EffectTransition::Applied
        );
        assert!(coordinator.take_new_effects().is_empty());
        assert!(coordinator.pointer_passthrough_sagas.contains_key(&binding));

        coordinator
            .publish_snapshot(&routing_snapshot(
                binding,
                3,
                WindowInputState::ReceivesInput,
                WindowPresentationState::Visible,
                PlatformCapability::Supported,
                InputEffectAcknowledgement::unknown(unavailable),
            ))
            .expect("newer safe state must publish independently of effect acknowledgement");
        assert!(!coordinator.pointer_passthrough_sagas.contains_key(&binding));
        assert!(coordinator.take_new_effects().is_empty());
    }

    #[test]
    fn stale_pre_restore_receiving_state_cannot_settle_a_failed_restore() {
        let (mut coordinator, binding) = routing_coordinator(
            WindowInputState::ReceivesInput,
            WindowPresentationState::Visible,
            PlatformCapability::Supported,
        );
        let pointer = PointerId::new(1);
        let enable = coordinator
            .begin_drag_routing(pointer, binding.surface())
            .expect("drag routing must begin")
            .expect("receiving input must request passthrough");
        let _ = coordinator.take_new_effects();
        let restore = coordinator
            .end_drag_routing(pointer)
            .expect("drag release must preserve restoration")
            .expect("release must queue restoration");
        assert_eq!(
            take_single_pointer_restore(&mut coordinator, binding, Some(enable)),
            restore
        );

        assert_eq!(
            coordinator
                .report_effect(
                    binding.epoch(),
                    EffectResult::new(
                        restore,
                        binding.epoch(),
                        EffectDispatchResult::DispatchFailed(
                            crate::effect::DispatchFailureReason::AdapterRejected,
                        ),
                    ),
                )
                .expect("effect report must reduce"),
            EffectTransition::Applied
        );
        assert!(coordinator.pointer_passthrough_sagas.contains_key(&binding));

        coordinator
            .publish_snapshot(&routing_snapshot(
                binding,
                2,
                WindowInputState::PassThrough,
                WindowPresentationState::Visible,
                PlatformCapability::Supported,
                InputEffectAcknowledgement::known(Some(enable)),
            ))
            .expect("late enable must not erase the restore obligation");
        assert!(coordinator.pointer_passthrough_sagas.contains_key(&binding));
        let retry = take_single_pointer_restore(&mut coordinator, binding, Some(restore));

        coordinator
            .publish_snapshot(&routing_snapshot(
                binding,
                3,
                WindowInputState::ReceivesInput,
                WindowPresentationState::Visible,
                PlatformCapability::Supported,
                InputEffectAcknowledgement::known(Some(retry)),
            ))
            .expect("the retry acknowledgement must settle late enable restoration");
        assert!(!coordinator.pointer_passthrough_sagas.contains_key(&binding));
    }

    #[test]
    fn safe_state_keeps_requested_restore_as_the_next_drag_predecessor() {
        let (mut coordinator, binding) = routing_coordinator(
            WindowInputState::ReceivesInput,
            WindowPresentationState::Visible,
            PlatformCapability::Supported,
        );
        let first_pointer = PointerId::new(1);
        let enable = coordinator
            .begin_drag_routing(first_pointer, binding.surface())
            .expect("drag routing must begin")
            .expect("receiving input must request passthrough");
        let _ = coordinator.take_new_effects();
        coordinator
            .publish_snapshot(&routing_snapshot(
                binding,
                2,
                WindowInputState::PassThrough,
                WindowPresentationState::Visible,
                PlatformCapability::Supported,
                InputEffectAcknowledgement::known(Some(enable)),
            ))
            .expect("enable acknowledgement must publish");
        let restore = coordinator
            .end_drag_routing(first_pointer)
            .expect("drag routing must end")
            .expect("restoration must be requested");
        let _ = coordinator.take_new_effects();
        coordinator
            .publish_snapshot(&routing_snapshot(
                binding,
                3,
                WindowInputState::ReceivesInput,
                WindowPresentationState::Visible,
                PlatformCapability::Supported,
                InputEffectAcknowledgement::known(Some(enable)),
            ))
            .expect("safe state without restore attribution must publish");
        assert!(coordinator.pointer_passthrough_sagas.contains_key(&binding));
        assert!(matches!(
            coordinator
                .effects()
                .record(restore)
                .expect("restore history must remain")
                .phase(),
            EffectPhase::Requested
        ));

        let second_enable = coordinator
            .begin_drag_routing(PointerId::new(2), binding.surface())
            .expect("a new drag must preserve the pointer-input lane")
            .expect("the new drag must request pass-through again");
        assert!(matches!(
            coordinator.take_new_effects().as_slice(),
            [request]
                if request.id() == second_enable
                    && matches!(
                        request.effect(),
                        PlatformEffect::SetPointerPassthrough {
                            binding: actual,
                            enabled: true,
                            after: Some(predecessor),
                        } if *actual == binding && *predecessor == restore
                    )
        ));
    }

    #[test]
    fn restore_failure_requires_a_safe_observation_newer_than_its_report() {
        let (mut coordinator, binding) = routing_coordinator(
            WindowInputState::ReceivesInput,
            WindowPresentationState::Visible,
            PlatformCapability::Supported,
        );
        let pointer = PointerId::new(1);
        let enable = coordinator
            .begin_drag_routing(pointer, binding.surface())
            .expect("drag routing must begin")
            .expect("receiving input must request passthrough");
        let _ = coordinator.take_new_effects();
        let restore = coordinator
            .end_drag_routing(pointer)
            .expect("drag routing must end")
            .expect("release must queue restoration");
        let _ = coordinator.take_new_effects();

        coordinator
            .publish_snapshot(&routing_snapshot(
                binding,
                2,
                WindowInputState::ReceivesInput,
                WindowPresentationState::Visible,
                PlatformCapability::Supported,
                InputEffectAcknowledgement::known(None),
            ))
            .expect("the pre-dispatch safe observation must publish");
        assert!(matches!(
            coordinator
                .effects()
                .record(enable)
                .expect("enable history must remain")
                .phase(),
            EffectPhase::Requested
        ));

        assert_eq!(
            coordinator
                .report_effect(
                    binding.epoch(),
                    EffectResult::new(
                        restore,
                        binding.epoch(),
                        EffectDispatchResult::DispatchFailed(
                            crate::effect::DispatchFailureReason::AdapterRejected,
                        ),
                    ),
                )
                .expect("restore failure must reduce"),
            EffectTransition::Applied
        );
        let restore_attempt = coordinator
            .pointer_passthrough_sagas
            .get(&binding)
            .and_then(|saga| saga.restore)
            .and_then(|obligation| obligation.attempt)
            .expect("the pre-report observation must not settle restoration");
        assert_eq!(
            restore_attempt.terminal_reported_after,
            Some(InputObservationGeneration::new(2))
        );
        assert!(
            !coordinator
                .pointer_passthrough_sagas
                .get(&binding)
                .expect("restore saga must remain")
                .restore
                .expect("restore obligation must remain")
                .state_settled
        );

        coordinator
            .publish_snapshot(&routing_snapshot(
                binding,
                3,
                WindowInputState::ReceivesInput,
                WindowPresentationState::Visible,
                PlatformCapability::Supported,
                InputEffectAcknowledgement::known(None),
            ))
            .expect("a post-report safe observation must publish");
        assert!(!coordinator.pointer_passthrough_sagas.contains_key(&binding));
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
        assert!(!coordinator.pointer_passthrough_sagas.contains_key(&binding));
    }

    #[test]
    fn receiving_source_without_hit_test_control_still_freezes_the_source() {
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
                .expect("missing control must still freeze the drag source"),
            None
        );
        assert_eq!(coordinator.drag_source(pointer), Some(binding));
        assert!(coordinator.pointer_passthrough_sagas.contains_key(&binding));
        assert!(coordinator.take_new_effects().is_empty());

        assert_eq!(
            coordinator
                .end_drag_routing(pointer)
                .expect("uncontrolled source freeze must end cleanly"),
            None
        );
        assert!(!coordinator.pointer_passthrough_sagas.contains_key(&binding));
    }

    #[test]
    fn definitive_enable_failure_waits_for_a_new_fact_edge_before_retry() {
        let (mut coordinator, binding) = routing_coordinator(
            WindowInputState::ReceivesInput,
            WindowPresentationState::Visible,
            PlatformCapability::Supported,
        );
        let pointer = PointerId::new(1);
        let first = coordinator
            .begin_drag_routing(pointer, binding.surface())
            .expect("initial enable must begin")
            .expect("receiving input must request passthrough");
        let _ = coordinator.take_new_effects();
        assert_eq!(
            coordinator
                .report_effect(
                    binding.epoch(),
                    EffectResult::new(
                        first,
                        binding.epoch(),
                        EffectDispatchResult::DispatchFailed(
                            crate::effect::DispatchFailureReason::AdapterRejected,
                        ),
                    ),
                )
                .expect("effect report must reduce"),
            EffectTransition::Applied
        );
        let effect_count = coordinator.effects().records().count();

        for generation in 2..5 {
            coordinator
                .publish_snapshot(&routing_snapshot(
                    binding,
                    generation,
                    WindowInputState::ReceivesInput,
                    WindowPresentationState::Visible,
                    PlatformCapability::Supported,
                    InputEffectAcknowledgement::known(None),
                ))
                .expect("unchanged observation must publish without retrying");
            assert!(coordinator.take_new_effects().is_empty());
            assert_eq!(coordinator.effects().records().count(), effect_count);
        }

        coordinator
            .publish_snapshot(&routing_snapshot(
                binding,
                5,
                WindowInputState::ReceivesInput,
                WindowPresentationState::Hidden,
                PlatformCapability::Supported,
                InputEffectAcknowledgement::known(None),
            ))
            .expect("hidden edge must publish without enabling");
        assert!(coordinator.take_new_effects().is_empty());
        coordinator
            .publish_snapshot(&routing_snapshot(
                binding,
                6,
                WindowInputState::ReceivesInput,
                WindowPresentationState::Visible,
                PlatformCapability::Supported,
                InputEffectAcknowledgement::known(None),
            ))
            .expect("routeable edge must unlock one retry");
        assert!(matches!(
            coordinator.take_new_effects().as_slice(),
            [request]
                if request.id() != first
                    && matches!(
                        request.effect(),
                        PlatformEffect::SetPointerPassthrough {
                            binding: actual,
                            enabled: true,
                            ..
                        } if *actual == binding
                    )
        ));
        assert_eq!(coordinator.drag_source(pointer), Some(binding));
    }

    #[test]
    fn unsupported_and_indeterminate_enable_attempts_do_not_retry_each_snapshot() {
        for result in [
            EffectDispatchResult::Unsupported(
                crate::effect::EffectUnsupportedReason::BackendUnsupported,
            ),
            EffectDispatchResult::Indeterminate(
                crate::effect::EffectIndeterminateReason::AcknowledgementLost,
            ),
        ] {
            let (mut coordinator, binding) = routing_coordinator(
                WindowInputState::ReceivesInput,
                WindowPresentationState::Visible,
                PlatformCapability::Supported,
            );
            let first = coordinator
                .begin_drag_routing(PointerId::new(1), binding.surface())
                .expect("initial enable must begin")
                .expect("receiving input must request passthrough");
            let _ = coordinator.take_new_effects();
            assert_eq!(
                coordinator
                    .report_effect(
                        binding.epoch(),
                        EffectResult::new(first, binding.epoch(), result),
                    )
                    .expect("effect report must reduce"),
                EffectTransition::Applied
            );
            let effect_count = coordinator.effects().records().count();

            for generation in 2..5 {
                coordinator
                    .publish_snapshot(&routing_snapshot(
                        binding,
                        generation,
                        WindowInputState::ReceivesInput,
                        WindowPresentationState::Visible,
                        PlatformCapability::Supported,
                        InputEffectAcknowledgement::known(None),
                    ))
                    .expect("unchanged observation must remain stable");
                assert!(coordinator.take_new_effects().is_empty());
                assert_eq!(coordinator.effects().records().count(), effect_count);
            }
        }
    }

    #[test]
    fn late_enable_after_drag_end_cannot_overtake_the_queued_restore() {
        let (mut coordinator, binding) = routing_coordinator(
            WindowInputState::ReceivesInput,
            WindowPresentationState::Visible,
            PlatformCapability::Supported,
        );
        let pointer = PointerId::new(1);
        let enable = coordinator
            .begin_drag_routing(pointer, binding.surface())
            .expect("enable must begin")
            .expect("receiving input must request passthrough");
        let _ = coordinator.take_new_effects();
        let restore = coordinator
            .end_drag_routing(pointer)
            .expect("drag end must preserve restoration")
            .expect("drag end must queue restoration immediately");
        assert_eq!(
            take_single_pointer_restore(&mut coordinator, binding, Some(enable)),
            restore
        );
        assert!(coordinator.pointer_passthrough_sagas.contains_key(&binding));

        let effect_count = coordinator.effects().records().count();
        for generation in 2..5 {
            coordinator
                .publish_snapshot(&routing_snapshot(
                    binding,
                    generation,
                    WindowInputState::ReceivesInput,
                    WindowPresentationState::Visible,
                    PlatformCapability::Supported,
                    InputEffectAcknowledgement::known(None),
                ))
                .expect("pre-enable input observation must not satisfy restoration");
            assert!(coordinator.pointer_passthrough_sagas.contains_key(&binding));
            assert!(coordinator.take_new_effects().is_empty());
            assert_eq!(coordinator.effects().records().count(), effect_count);
        }

        coordinator
            .publish_snapshot(&routing_snapshot(
                binding,
                5,
                WindowInputState::PassThrough,
                WindowPresentationState::Visible,
                PlatformCapability::Supported,
                InputEffectAcknowledgement::known(Some(enable)),
            ))
            .expect("late enable observation must publish");
        assert!(coordinator.take_new_effects().is_empty());
        assert!(matches!(
            coordinator
                .effects()
                .record(enable)
                .expect("enable effect must remain queryable")
                .phase(),
            EffectPhase::ObservedApplied { .. }
        ));
        assert!(coordinator.pointer_passthrough_sagas.contains_key(&binding));

        coordinator
            .publish_snapshot(&routing_snapshot(
                binding,
                6,
                WindowInputState::ReceivesInput,
                WindowPresentationState::Visible,
                PlatformCapability::Supported,
                InputEffectAcknowledgement::known(Some(restore)),
            ))
            .expect("restored input observation must publish");
        assert!(matches!(
            coordinator
                .effects()
                .record(restore)
                .expect("restore effect must remain queryable")
                .phase(),
            EffectPhase::ObservedApplied { .. }
        ));
        assert!(!coordinator.pointer_passthrough_sagas.contains_key(&binding));
    }

    #[test]
    fn failed_restore_waits_for_a_capability_edge_before_retry() {
        let (mut coordinator, binding) = routing_coordinator(
            WindowInputState::ReceivesInput,
            WindowPresentationState::Visible,
            PlatformCapability::Supported,
        );
        let pointer = PointerId::new(1);
        let enable = coordinator
            .begin_drag_routing(pointer, binding.surface())
            .expect("enable must begin")
            .expect("receiving input must request passthrough");
        let _ = coordinator.take_new_effects();
        coordinator
            .publish_snapshot(&routing_snapshot(
                binding,
                2,
                WindowInputState::PassThrough,
                WindowPresentationState::Visible,
                PlatformCapability::Supported,
                InputEffectAcknowledgement::known(Some(enable)),
            ))
            .expect("passthrough observation must publish");

        let restore = coordinator
            .end_drag_routing(pointer)
            .expect("drag end must preserve restoration")
            .expect("drag end must request restoration");
        let _ = coordinator.take_new_effects();
        assert_eq!(
            coordinator
                .report_effect(
                    binding.epoch(),
                    EffectResult::new(
                        restore,
                        binding.epoch(),
                        EffectDispatchResult::DispatchFailed(
                            crate::effect::DispatchFailureReason::AdapterRejected,
                        ),
                    ),
                )
                .expect("effect report must reduce"),
            EffectTransition::Applied
        );
        let effect_count = coordinator.effects().records().count();
        for generation in 3..6 {
            coordinator
                .publish_snapshot(&routing_snapshot(
                    binding,
                    generation,
                    WindowInputState::PassThrough,
                    WindowPresentationState::Visible,
                    PlatformCapability::Supported,
                    InputEffectAcknowledgement::known(Some(enable)),
                ))
                .expect("unchanged passthrough must not retry restoration");
            assert!(coordinator.take_new_effects().is_empty());
            assert_eq!(coordinator.effects().records().count(), effect_count);
        }

        let unsupported = PlatformCapability::unsupported(
            crate::platform::PlatformRequirement::PointerHitTestControl,
            crate::platform::PlatformCapabilityReason::BackendUnsupported,
        );
        coordinator
            .publish_snapshot(&routing_snapshot(
                binding,
                6,
                WindowInputState::PassThrough,
                WindowPresentationState::Visible,
                unsupported,
                InputEffectAcknowledgement::known(Some(enable)),
            ))
            .expect("capability loss must publish without restoring");
        assert!(coordinator.take_new_effects().is_empty());
        coordinator
            .publish_snapshot(&routing_snapshot(
                binding,
                7,
                WindowInputState::PassThrough,
                WindowPresentationState::Visible,
                PlatformCapability::Supported,
                InputEffectAcknowledgement::known(Some(enable)),
            ))
            .expect("capability recovery must unlock one restore retry");
        assert!(matches!(
            coordinator.take_new_effects().as_slice(),
            [request]
                if request.id() != restore
                    && matches!(
                        request.effect(),
                        PlatformEffect::SetPointerPassthrough {
                            binding: actual,
                            enabled: false,
                            ..
                        } if *actual == binding
                    )
        ));
    }

    #[test]
    fn indeterminate_restore_is_not_reissued_concurrently() {
        let (mut coordinator, binding) = routing_coordinator(
            WindowInputState::ReceivesInput,
            WindowPresentationState::Visible,
            PlatformCapability::Supported,
        );
        let pointer = PointerId::new(1);
        let enable = coordinator
            .begin_drag_routing(pointer, binding.surface())
            .expect("enable must begin")
            .expect("receiving input must request passthrough");
        let _ = coordinator.take_new_effects();
        coordinator
            .publish_snapshot(&routing_snapshot(
                binding,
                2,
                WindowInputState::PassThrough,
                WindowPresentationState::Visible,
                PlatformCapability::Supported,
                InputEffectAcknowledgement::known(Some(enable)),
            ))
            .expect("passthrough observation must publish");
        let restore = coordinator
            .end_drag_routing(pointer)
            .expect("drag end must preserve restoration")
            .expect("drag end must request restoration");
        let _ = coordinator.take_new_effects();
        assert_eq!(
            coordinator
                .report_effect(
                    binding.epoch(),
                    EffectResult::new(
                        restore,
                        binding.epoch(),
                        EffectDispatchResult::Indeterminate(
                            crate::effect::EffectIndeterminateReason::AcknowledgementLost,
                        ),
                    ),
                )
                .expect("effect report must reduce"),
            EffectTransition::Applied
        );
        for generation in 3..6 {
            coordinator
                .publish_snapshot(&routing_snapshot(
                    binding,
                    generation,
                    WindowInputState::PassThrough,
                    WindowPresentationState::Visible,
                    PlatformCapability::Supported,
                    InputEffectAcknowledgement::known(Some(enable)),
                ))
                .expect("indeterminate restore must remain singular");
            assert!(coordinator.take_new_effects().is_empty());
        }
        coordinator
            .publish_snapshot(&routing_snapshot(
                binding,
                6,
                WindowInputState::ReceivesInput,
                WindowPresentationState::Visible,
                PlatformCapability::Supported,
                InputEffectAcknowledgement::known(Some(restore)),
            ))
            .expect("authoritative restored input must settle the attempt");
        assert!(!coordinator.pointer_passthrough_sagas.contains_key(&binding));
    }

    #[test]
    fn authoritative_destruction_terminates_active_and_released_pointer_sagas() {
        for release_before_destroy in [false, true] {
            let (mut coordinator, binding) = routing_coordinator(
                WindowInputState::ReceivesInput,
                WindowPresentationState::Visible,
                PlatformCapability::Supported,
            );
            let pointer = PointerId::new(1);
            let enable = coordinator
                .begin_drag_routing(pointer, binding.surface())
                .expect("drag routing must begin")
                .expect("enable effect must exist");
            let _ = coordinator.take_new_effects();
            if release_before_destroy {
                let restore = coordinator
                    .end_drag_routing(pointer)
                    .expect("drag routing must release")
                    .expect("release must queue restoration");
                assert_eq!(
                    take_single_pointer_restore(&mut coordinator, binding, Some(enable)),
                    restore
                );
            }

            coordinator
                .publish_snapshot(&snapshot(Vec::new()))
                .expect("authoritative absence must terminate pointer saga");
            assert_eq!(coordinator.drag_source(pointer), None);
            assert!(!coordinator.pointer_passthrough_sagas.contains_key(&binding));
            assert!(coordinator.take_new_effects().is_empty());
            assert!(matches!(
                coordinator
                    .effects()
                    .record(enable)
                    .expect("enable effect history must remain")
                    .phase(),
                EffectPhase::Destroyed { .. }
            ));
        }
    }

    #[test]
    fn window_unavailable_waits_for_authoritative_destruction_without_retrying() {
        let (mut coordinator, binding) = routing_coordinator(
            WindowInputState::ReceivesInput,
            WindowPresentationState::Visible,
            PlatformCapability::Supported,
        );
        let pointer = PointerId::new(1);
        let enable = coordinator
            .begin_drag_routing(pointer, binding.surface())
            .expect("drag routing must begin")
            .expect("enable effect must exist");
        let _ = coordinator.take_new_effects();
        assert_eq!(
            coordinator
                .report_effect(
                    binding.epoch(),
                    EffectResult::new(
                        enable,
                        binding.epoch(),
                        EffectDispatchResult::DispatchFailed(
                            crate::effect::DispatchFailureReason::WindowUnavailable,
                        ),
                    ),
                )
                .expect("effect report must reduce"),
            EffectTransition::Applied
        );
        coordinator
            .publish_snapshot(&snapshot(Vec::new()))
            .expect("authoritative absence must terminate unavailable binding");
        assert_eq!(coordinator.drag_source(pointer), None);
        assert!(!coordinator.pointer_passthrough_sagas.contains_key(&binding));
        assert!(coordinator.take_new_effects().is_empty());
        assert!(matches!(
            coordinator
                .effects()
                .record(enable)
                .expect("enable effect history must remain")
                .phase(),
            EffectPhase::Destroyed { .. }
        ));
    }

    #[test]
    fn workspace_rebound_requires_a_fresh_restore_ack_and_ignores_old_effects() {
        let (mut coordinator, old_binding, old_enable, old_restore) = released_unobserved_enable();

        let new_epoch = WorkspaceEpoch::new(1);
        let reconciliation = coordinator
            .reconcile_workspace_epoch(new_epoch, &BTreeSet::from([old_binding.surface()]))
            .expect("workspace replacement must reconcile the binding");
        let &[(actual_old, new_binding)] = reconciliation.rebound() else {
            panic!("one binding must rebound: {reconciliation:?}");
        };
        assert_eq!(actual_old, old_binding);
        let rebound_restore =
            take_single_pointer_restore(&mut coordinator, new_binding, Some(old_restore));
        assert!(
            coordinator
                .pointer_passthrough_sagas
                .contains_key(&new_binding)
        );

        coordinator
            .publish_snapshot(&routing_snapshot(
                new_binding,
                2,
                WindowInputState::ReceivesInput,
                WindowPresentationState::Visible,
                PlatformCapability::Supported,
                InputEffectAcknowledgement::unknown(
                    crate::intent::AuthorityUnavailableReason::NotReported,
                ),
            ))
            .expect("transient acknowledgement loss must preserve the queued safety restore");
        assert!(coordinator.take_new_effects().is_empty());
        assert!(
            coordinator
                .pointer_passthrough_sagas
                .contains_key(&new_binding)
        );
        assert_eq!(
            coordinator
                .report_effect(
                    new_epoch,
                    EffectResult::new(
                        old_enable,
                        old_binding.epoch(),
                        EffectDispatchResult::DispatchFailed(
                            crate::effect::DispatchFailureReason::AdapterRejected,
                        ),
                    ),
                )
                .expect("effect report must reduce"),
            EffectTransition::StaleEpoch
        );
        coordinator
            .publish_snapshot(&routing_snapshot(
                new_binding,
                3,
                WindowInputState::PassThrough,
                WindowPresentationState::Visible,
                PlatformCapability::Supported,
                InputEffectAcknowledgement::known(Some(old_enable)),
            ))
            .expect("an old-incarnation acknowledgement must not settle the new restore");
        assert!(
            coordinator
                .pointer_passthrough_sagas
                .contains_key(&new_binding)
        );
        assert!(coordinator.take_new_effects().is_empty());
        coordinator
            .publish_snapshot(&routing_snapshot(
                new_binding,
                4,
                WindowInputState::ReceivesInput,
                WindowPresentationState::Visible,
                PlatformCapability::Supported,
                InputEffectAcknowledgement::known(Some(rebound_restore)),
            ))
            .expect("post-barrier restored input must settle the rebound saga");
        assert!(
            !coordinator
                .pointer_passthrough_sagas
                .contains_key(&new_binding)
        );
        assert!(matches!(
            coordinator
                .effects()
                .record(rebound_restore)
                .expect("rebound restore must remain queryable")
                .phase(),
            EffectPhase::ObservedApplied { .. }
        ));
    }

    #[test]
    fn repeated_restore_in_one_boundary_skips_unemitted_cleanup_and_pointer_successors() {
        let RepeatedRestoreBoundary {
            mut coordinator,
            binding,
            enable,
            destructive,
            fourth_epoch,
            emitted_cleanup,
            emitted_restore,
            unemitted_cleanup,
            unemitted_restore,
            current,
        } = repeated_restore_boundary();
        assert_eq!(
            coordinator
                .effects()
                .record(unemitted_cleanup)
                .map(crate::effect::EffectRecord::phase),
            Some(EffectPhase::InvalidatedByRestore {
                replacement_epoch: fourth_epoch,
            })
        );
        assert_eq!(
            coordinator
                .effects()
                .record(unemitted_restore)
                .map(crate::effect::EffectRecord::phase),
            Some(EffectPhase::InvalidatedByRestore {
                replacement_epoch: fourth_epoch,
            })
        );
        let current_cleanup = current
            .iter()
            .find(|request| matches!(request.effect(), PlatformEffect::ContinueCleanup { .. }))
            .expect("cleanup observation must remain observation-only");
        assert!(matches!(
            current_cleanup.effect(),
            PlatformEffect::ContinueCleanup {
                binding: actual,
                predecessor,
                after: Some(after),
            } if *actual == binding
                && *predecessor == destructive
                && *after == emitted_cleanup
        ));
        let current_restore = current
            .iter()
            .find(|request| {
                matches!(
                    request.effect(),
                    PlatformEffect::SetPointerPassthrough { enabled: false, .. }
                )
            })
            .expect("pointer restore obligation must remain queued");
        assert!(matches!(
            current_restore.effect(),
            PlatformEffect::SetPointerPassthrough {
                binding: actual,
                enabled: false,
                after: Some(after),
            } if *actual == binding && *after == emitted_restore
        ));
        assert!(current.iter().all(|request| {
            !matches!(
                request.effect(),
                PlatformEffect::RequestRootClose { binding: actual }
                    | PlatformEffect::ReleaseChild { binding: actual }
                    if *actual == binding
            )
        }));

        coordinator
            .publish_snapshot(&routing_snapshot(
                binding,
                2,
                WindowInputState::PassThrough,
                WindowPresentationState::Visible,
                PlatformCapability::Supported,
                InputEffectAcknowledgement::known(Some(enable)),
            ))
            .expect("late enable acknowledgement must remain observable");
        assert!(coordinator.pointer_passthrough_sagas.contains_key(&binding));
        assert!(coordinator.take_new_effects().is_empty());
        coordinator
            .publish_snapshot(&routing_snapshot(
                binding,
                3,
                WindowInputState::ReceivesInput,
                WindowPresentationState::Visible,
                PlatformCapability::Supported,
                InputEffectAcknowledgement::known(Some(current_restore.id())),
            ))
            .expect("only the exact current restore may settle the obligation");
        assert!(!coordinator.pointer_passthrough_sagas.contains_key(&binding));
    }

    #[test]
    fn old_cleanup_result_requires_a_current_exact_binding_continuation() {
        let (mut coordinator, binding) = routing_coordinator(
            WindowInputState::ReceivesInput,
            WindowPresentationState::Visible,
            PlatformCapability::Supported,
        );
        let first_epoch = WorkspaceEpoch::new(1);
        coordinator
            .reconcile_workspace_epoch(first_epoch, &BTreeSet::new())
            .expect("first restore must retire the binding");
        let destructive = coordinator
            .take_new_effects()
            .into_iter()
            .find_map(|request| {
                matches!(
                    request.effect(),
                    PlatformEffect::RequestRootClose { binding: actual } if *actual == binding
                )
                .then_some(request.id())
            })
            .expect("first restore must emit destructive cleanup");
        let current_epoch = WorkspaceEpoch::new(2);
        coordinator
            .reconcile_workspace_epoch(current_epoch, &BTreeSet::new())
            .expect("second restore must create a cleanup continuation");
        let _ = coordinator.take_new_effects();

        let wrong_incarnation = ViewportBinding::new(
            binding.epoch(),
            binding.surface(),
            binding.token(),
            binding
                .incarnation()
                .checked_next()
                .expect("test incarnation must advance"),
        );
        coordinator
            .retired_viewports
            .get_mut(&binding.token())
            .expect("retired cleanup must remain queryable")
            .binding = wrong_incarnation;

        assert_eq!(
            report_effect_result(
                &mut coordinator,
                current_epoch,
                destructive,
                first_epoch,
                EffectDispatchResult::DispatchFailed(
                    crate::effect::DispatchFailureReason::ProviderStopped,
                ),
            ),
            EffectTransition::StaleEpoch
        );
        assert_eq!(
            coordinator
                .effects()
                .record(destructive)
                .map(crate::effect::EffectRecord::phase),
            Some(EffectPhase::Requested)
        );
    }

    #[test]
    fn old_cleanup_result_is_stale_while_same_boundary_continuation_is_unemitted() {
        let (mut coordinator, binding) = routing_coordinator(
            WindowInputState::ReceivesInput,
            WindowPresentationState::Visible,
            PlatformCapability::Supported,
        );
        let first_epoch = WorkspaceEpoch::new(1);
        coordinator
            .reconcile_workspace_epoch(first_epoch, &BTreeSet::new())
            .expect("first restore must retire the binding");
        let destructive = coordinator
            .take_new_effects()
            .into_iter()
            .find_map(|request| {
                matches!(
                    request.effect(),
                    PlatformEffect::RequestRootClose { binding: actual } if *actual == binding
                )
                .then_some(request.id())
            })
            .expect("first restore must emit destructive cleanup");

        let current_epoch = WorkspaceEpoch::new(2);
        let reconciliation = coordinator
            .reconcile_workspace_epoch(current_epoch, &BTreeSet::new())
            .expect("same boundary restore must queue a continuation");
        let continuation = reconciliation
            .cleanup_effects()
            .iter()
            .copied()
            .find(|effect| {
                matches!(
                    coordinator.effects().record(*effect).map(|record| record.request().effect()),
                    Some(PlatformEffect::ContinueCleanup {
                        predecessor,
                        ..
                    }) if *predecessor == destructive
                )
            })
            .expect("restore must queue the exact cleanup continuation");
        assert!(
            !coordinator
                .effects()
                .record(continuation)
                .expect("continuation must remain queryable")
                .was_emitted()
        );

        assert_eq!(
            report_effect_result(
                &mut coordinator,
                current_epoch,
                destructive,
                first_epoch,
                EffectDispatchResult::DispatchFailed(
                    crate::effect::DispatchFailureReason::ProviderStopped,
                ),
            ),
            EffectTransition::StaleEpoch
        );
        assert_eq!(
            coordinator
                .effects()
                .record(destructive)
                .map(crate::effect::EffectRecord::phase),
            Some(EffectPhase::Requested)
        );
    }

    #[test]
    fn cleanup_observation_retry_rejects_ineligible_protocol_state_atomically() {
        #[derive(Debug, Clone, Copy)]
        enum IneligibleState {
            WrongTombstoneBinding,
            TerminalDestructivePredecessor,
            InventoryRemoved,
        }

        for state in [
            IneligibleState::WrongTombstoneBinding,
            IneligibleState::TerminalDestructivePredecessor,
            IneligibleState::InventoryRemoved,
        ] {
            let (mut coordinator, binding, destructive, continuation, destructive_epoch, _) =
                failed_cleanup_observation();
            match state {
                IneligibleState::WrongTombstoneBinding => {
                    let wrong_binding = ViewportBinding::new(
                        binding.epoch(),
                        binding.surface(),
                        binding.token(),
                        binding
                            .incarnation()
                            .checked_next()
                            .expect("test incarnation must advance"),
                    );
                    coordinator
                        .retired_viewports
                        .get_mut(&binding.token())
                        .expect("failed observer tombstone must remain queryable")
                        .binding = wrong_binding;
                }
                IneligibleState::TerminalDestructivePredecessor => {
                    assert_eq!(
                        coordinator.effects.report_exact(EffectResult::new(
                            destructive,
                            destructive_epoch,
                            EffectDispatchResult::DispatchFailed(
                                crate::effect::DispatchFailureReason::ProviderStopped,
                            ),
                        )),
                        EffectTransition::Applied
                    );
                }
                IneligibleState::InventoryRemoved => {
                    coordinator
                        .publish_snapshot(&snapshot(Vec::new()))
                        .expect("authoritative inventory removal must publish");
                    assert!(!coordinator.retired_viewports.contains_key(&binding.token()));
                }
            }

            let before = coordinator.clone();
            let rejected = coordinator.retry_cleanup(continuation);
            match state {
                IneligibleState::WrongTombstoneBinding
                | IneligibleState::TerminalDestructivePredecessor => assert!(matches!(
                    rejected,
                    Err(ViewportCoordinatorError::InvalidCleanupContinuation {
                        effect,
                        predecessor,
                    }) if effect == continuation && predecessor == destructive
                )),
                IneligibleState::InventoryRemoved => assert!(matches!(
                    rejected,
                    Err(ViewportCoordinatorError::CleanupEffectNotRetryable {
                        effect,
                        phase: EffectPhase::Destroyed { .. },
                    })
                        if effect == continuation
                )),
            }
            assert_eq!(coordinator, before, "retry mutated {state:?}");
            assert!(coordinator.take_new_effects().is_empty());
        }
    }

    #[test]
    fn repeated_restore_migrates_retired_pointer_and_cleanup_obligations() {
        for restore_was_indeterminate in [false, true] {
            let mut fixture = migrated_retired_obligations(restore_was_indeterminate);
            assert_cleanup_observation_successor_is_retryable(&mut fixture);
            assert_destructive_cleanup_result_requires_its_issuance_epoch(&mut fixture);
            settle_migrated_pointer_restore(&mut fixture);
        }
    }

    #[test]
    fn retired_observed_window_keeps_restoration_after_close_cleanup_failure() {
        let (mut coordinator, binding) = routing_coordinator(
            WindowInputState::ReceivesInput,
            WindowPresentationState::Visible,
            PlatformCapability::Supported,
        );
        let pointer = PointerId::new(1);
        let enable = coordinator
            .begin_drag_routing(pointer, binding.surface())
            .expect("drag routing must begin")
            .expect("receiving input must request passthrough");
        let _ = coordinator.take_new_effects();
        coordinator
            .publish_snapshot(&routing_snapshot(
                binding,
                2,
                WindowInputState::PassThrough,
                WindowPresentationState::Visible,
                PlatformCapability::Supported,
                InputEffectAcknowledgement::known(Some(enable)),
            ))
            .expect("passthrough must be observed before replacement");
        let restore = coordinator
            .end_drag_routing(pointer)
            .expect("drag release must preserve restoration")
            .expect("old restore effect must exist");
        let _ = coordinator.take_new_effects();

        let new_epoch = WorkspaceEpoch::new(1);
        let reconciliation = coordinator
            .reconcile_workspace_epoch(new_epoch, &BTreeSet::new())
            .expect("workspace replacement must retire the old binding");
        assert_eq!(reconciliation.retired(), &[binding]);
        let cleanup = coordinator.take_new_effects();
        let close = cleanup
            .iter()
            .find_map(|request| match request.effect() {
                PlatformEffect::RequestRootClose { binding: actual } if *actual == binding => {
                    Some(request.id())
                }
                _ => None,
            })
            .expect("retired root must still request close cleanup");
        assert_eq!(
            coordinator
                .report_effect(
                    new_epoch,
                    EffectResult::new(
                        close,
                        new_epoch,
                        EffectDispatchResult::DispatchFailed(
                            crate::effect::DispatchFailureReason::AdapterRejected,
                        ),
                    ),
                )
                .expect("effect report must reduce"),
            EffectTransition::Applied
        );

        coordinator
            .publish_snapshot(&routing_snapshot(
                binding,
                3,
                WindowInputState::PassThrough,
                WindowPresentationState::Visible,
                PlatformCapability::Supported,
                InputEffectAcknowledgement::known(Some(enable)),
            ))
            .expect("failed close must not discard retired restoration");
        assert!(coordinator.pointer_passthrough_sagas.contains_key(&binding));
        coordinator
            .publish_snapshot(&routing_snapshot(
                binding,
                4,
                WindowInputState::ReceivesInput,
                WindowPresentationState::Visible,
                PlatformCapability::Supported,
                InputEffectAcknowledgement::known(Some(restore)),
            ))
            .expect("retired restored input must settle independently of close cleanup");
        assert!(!coordinator.pointer_passthrough_sagas.contains_key(&binding));
        assert!(matches!(
            coordinator
                .effects()
                .record(restore)
                .expect("retired restore must remain queryable")
                .phase(),
            EffectPhase::ObservedApplied { .. }
        ));
    }

    #[test]
    fn hidden_and_minimized_windows_cannot_start_pointer_routing() {
        for presentation in [
            WindowPresentationState::Hidden,
            WindowPresentationState::Minimized,
        ] {
            let (mut coordinator, binding) = routing_coordinator(
                WindowInputState::ReceivesInput,
                presentation,
                PlatformCapability::Supported,
            );
            assert_eq!(
                coordinator
                    .begin_drag_routing(PointerId::new(1), binding.surface())
                    .expect("non-routeable presentation is a supported no-op"),
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
