//! Single-writer input queue and atomic headless state reducer.

use std::collections::{BTreeMap, BTreeSet};

use thiserror::Error;

use crate::command::{
    CommandOutcome, MovePayload, NodeSource, RootContent, RootPresentationTarget, WorkspaceCommand,
};
use crate::coordinates::CoordinateSnapshot;
use crate::drop_resolver::{DropAffordance, DropResolution, DropResolutionError, query_drop};
use crate::effect::{EffectResult, EffectTransition};
use crate::error::TransactionError;
use crate::event::{WorkspaceEvent, WorkspaceEventKind};
use crate::frame::{
    NativeCreateSagaId, PanelFocus, ViewportCloseDecision, ViewportCloseDecisionRejection,
    ViewportClosePlan, ViewportCloseRequestId, ViewportCloseStatus, ViewportCoordinator,
    ViewportCoordinatorError,
};
use crate::graph::{ContainedStackKey, Node, Workspace};
use crate::ids::{InputSequence, WorkspaceRevision};
use crate::intent::{
    Authority, ContainedHorizontalResizeEdge, ContainedPlacementProof,
    ContainedPlacementUnavailable, ContainedPresentationOffer, ContainedStackPlacement,
    ContainedTransformKind, ContainedVerticalResizeEdge, DragOrigin, PointerButtonState,
    RendererIntent, SurfacePointer, TargetAuthority, TearOffRequest,
};
use crate::interaction::{
    ActiveContainedTransform, ContainedMutationKind, ContainedTransformSessionId,
    ContainedTransformStart, DragArmStart, DragObservationProtocol, FrozenContainedDragOrigin,
    FrozenDragOrigin, InteractionCancelReason, InteractionCounterError, InteractionDelivery,
    InteractionEvent, InteractionEventKind, InteractionOutcome, InteractionRejection,
    InteractionState, InteractionStatus, PreparedNativeTearOff, PreviewProof,
    PreviewResolutionStatus, PreviewVisual, WorkspaceDeliveryKind,
};
use crate::platform::{PlatformCapability, PlatformSnapshot};
use crate::policy::{DockPolicy, TearOffPresentation};
use crate::scene::{
    BuildingScene, SceneBuildError, SceneGeneration, SceneStamp, SealedScene, SurfaceScene,
};
use crate::surface_recovery::{
    ContainedRootPlacement, PendingSurfaceRecoveryDisposition, SurfaceForestPlacement,
    SurfaceRecoveryState, SurfaceRecoveryTargetFacts, SurfaceRosterCaptureError,
    SurfaceRosterDisposition, SurfaceRosterPlacement,
};
use crate::transaction::WorkspaceTransaction;
use crate::transition::{
    EngineTransition, EngineTransitionParts, InputOutcome, InputPriority, ReducedInput,
    WorkspaceVersion,
};
use crate::validation::WorkspaceValidationErrors;
use crate::viewport::{ViewportRole, WindowToken};
use crate::viewport_focus::{
    ActivationStart, ActivationStartOutcome, FocusDelta, FocusObservationTransition,
    ObservedPlatformFocusEffect, PaneFocusIntent, PaneFocusIntentGeneration, PaneFocusObservation,
    PaneFocusRevealRejection, PanelFocusRecord, PlatformFocusEvidence, PlatformFocusRestoreGate,
    ViewportActivationRequest, ViewportFocusCoordinator, ViewportFocusError,
};

/// Input accepted by the U3 engine boundary.
#[derive(Debug, Clone, PartialEq)]
pub enum EngineInput {
    /// Bind an existing adapter window to a current logical surface.
    RegisterViewport {
        /// Workspace version whose surface roster was inspected.
        expected: WorkspaceVersion,
        /// Stable logical surface already present in the workspace.
        surface: crate::ids::SurfaceId,
        /// Opaque adapter token, never an operating-system handle.
        token: WindowToken,
        /// Root or docking-owned child close semantics.
        role: ViewportRole,
        /// Whole-root fallback required for docking-owned child windows.
        recovery: Option<crate::intent::ContainedTearOffProposal>,
    },
    /// Publish one complete frame-before-paint platform snapshot.
    PublishPlatformSnapshot {
        /// Workspace epoch against which adapter bindings were observed.
        expected_epoch: crate::ids::WorkspaceEpoch,
        /// Complete capabilities, inventory, and pointer facts.
        snapshot: PlatformSnapshot,
    },
    /// Report an adapter dispatch result without claiming the effect was observed applied.
    ReportPlatformEffect {
        /// Workspace epoch in which the result was received.
        expected_epoch: crate::ids::WorkspaceEpoch,
        /// Exact effect identity and non-observational dispatch result.
        result: EffectResult,
    },
    /// Request explicit activation of one exact current docking viewport.
    ActivateViewport {
        /// Workspace version whose binding and pane were inspected.
        expected: WorkspaceVersion,
        /// Exact-incarnation activation with explicit item-or-none pane focus.
        request: ViewportActivationRequest,
    },
    /// Publish one adapter-observed pane-focus fact.
    PublishPaneFocusObservation {
        /// Workspace epoch in which the exact binding was observed.
        expected_epoch: crate::ids::WorkspaceEpoch,
        /// Provider-owned pane focus observation and optional intent acknowledgement.
        observation: PaneFocusObservation,
    },
    /// Decide one exact edge-triggered native close request.
    DecideViewportClose {
        /// Workspace version against which the close plan was captured.
        expected: WorkspaceVersion,
        /// Exact close request produced by a platform snapshot transition.
        request: ViewportCloseRequestId,
        /// Application veto or frozen accepted plan.
        decision: ViewportCloseDecision,
    },
    /// Explicitly cancel one unresolved native-create saga.
    CancelNativeCreate {
        /// Workspace version against which the saga was inspected.
        expected: WorkspaceVersion,
        /// Exact create saga to cancel without a timeout heuristic.
        saga: NativeCreateSagaId,
    },
    /// Explicitly retry one exact failed cleanup under its phase-specific protocol.
    ///
    /// A definitively failed destructive cleanup is retried only under its original
    /// cleanup-specific guards. `ObservationDispatchFailed` and `ObservationUnsupported` may
    /// retry only an observation-only `ContinueCleanup` which keeps the same destructive
    /// predecessor; they never redispatch that predecessor.
    RetryViewportCleanup {
        /// Workspace version against which the failed cleanup was inspected.
        expected: WorkspaceVersion,
        /// Exact failed cleanup effect; indeterminate and all other phases are not retryable.
        failed_effect: crate::effect::EffectId,
    },
    /// Authoritatively replace the complete workspace.
    ReplaceWorkspace(Workspace),
    /// Apply one checked command derived from an exact engine version.
    WorkspaceCommand {
        /// Version from which source and target references were captured.
        expected: WorkspaceVersion,
        /// Checked durable mutation.
        command: WorkspaceCommand,
    },
    /// Replace application policy if the input is still current.
    ReplacePolicy {
        /// Version observed when the application chose the policy.
        expected: WorkspaceVersion,
        /// Complete replacement policy.
        policy: DockPolicy,
    },
    /// Seal and publish the next scene after current renderer intents reduce.
    PublishScene {
        /// Workspace and policy version from which all scene facts were captured.
        expected: WorkspaceVersion,
        /// Complete frozen-roster scene builder.
        scene: BuildingScene,
    },
    /// Reduce one semantic renderer callback input.
    RendererIntent {
        /// Workspace and policy version from which the intent was derived.
        expected: WorkspaceVersion,
        /// Typed interaction input.
        intent: Box<RendererIntent>,
    },
    /// Re-run strict validation without changing state.
    ValidateWorkspace,
}

impl EngineInput {
    /// Returns the fixed source-class priority used during reduction.
    #[must_use]
    pub const fn priority(&self) -> InputPriority {
        match self {
            Self::RegisterViewport { .. }
            | Self::DecideViewportClose { .. }
            | Self::CancelNativeCreate { .. }
            | Self::RetryViewportCleanup { .. }
            | Self::ReplaceWorkspace(_) => InputPriority::LifecycleControl,
            Self::PublishPlatformSnapshot { .. } | Self::ReportPlatformEffect { .. } => {
                InputPriority::PlatformObservation
            }
            Self::PublishPaneFocusObservation { .. } => InputPriority::PlatformObservation,
            Self::ActivateViewport { .. }
            | Self::WorkspaceCommand { .. }
            | Self::ReplacePolicy { .. } => InputPriority::ApplicationCommand,
            Self::RendererIntent { .. } => InputPriority::RendererIntent,
            Self::PublishScene { .. } | Self::ValidateWorkspace => InputPriority::Maintenance,
        }
    }

    const fn reduction_rank(&self) -> u8 {
        match self {
            Self::RendererIntent { intent, .. } => intent.reduction_rank(),
            Self::PublishScene { .. } => 1,
            Self::ValidateWorkspace => 2,
            Self::RegisterViewport { .. }
            | Self::PublishPlatformSnapshot { .. }
            | Self::ReportPlatformEffect { .. }
            | Self::ActivateViewport { .. }
            | Self::PublishPaneFocusObservation { .. }
            | Self::DecideViewportClose { .. }
            | Self::CancelNativeCreate { .. }
            | Self::RetryViewportCleanup { .. }
            | Self::ReplaceWorkspace(_)
            | Self::WorkspaceCommand { .. }
            | Self::ReplacePolicy { .. } => 0,
        }
    }
}

/// Input after assignment by the engine's sole sequence writer.
#[derive(Debug, Clone, PartialEq)]
pub struct SequencedInput {
    sequence: InputSequence,
    input: EngineInput,
    scene_coordinate_proofs: BTreeMap<crate::ids::SurfaceId, CoordinateSnapshot>,
}

impl SequencedInput {
    /// Returns the monotonic writer sequence.
    #[must_use]
    pub const fn sequence(&self) -> InputSequence {
        self.sequence
    }

    /// Returns the typed input payload.
    #[must_use]
    pub const fn input(&self) -> &EngineInput {
        &self.input
    }
}

/// Failure to construct or atomically reduce engine state.
#[derive(Debug, Clone, PartialEq, Error)]
pub enum EngineError {
    /// Initial or replacement workspace violated a durable invariant.
    #[error("workspace is invalid: {0}")]
    InvalidWorkspace(#[source] WorkspaceValidationErrors),
    /// The single-writer input counter cannot advance without wrapping.
    #[error("engine input sequence is exhausted")]
    InputSequenceExhausted,
    /// A workspace replacement epoch cannot advance without wrapping.
    #[error("workspace epoch is exhausted while reducing input {input}")]
    WorkspaceEpochExhausted {
        /// Input which attempted replacement.
        input: InputSequence,
    },
    /// A state revision cannot advance without wrapping.
    #[error("workspace revision is exhausted while reducing input {input}")]
    WorkspaceRevisionExhausted {
        /// Input which attempted mutation.
        input: InputSequence,
    },
    /// A checked command transaction failed; no engine state was published.
    #[error("workspace command input {input} failed: {source}")]
    Command {
        /// Failing sequenced input.
        input: InputSequence,
        /// Atomic transaction failure.
        source: crate::error::TransactionError,
    },
    /// A successful one-command transaction omitted its required outcome.
    #[error("workspace command input {input} produced no command outcome")]
    MissingCommandOutcome {
        /// Input whose transaction violated the engine contract.
        input: InputSequence,
    },
    /// A scene generation counter cannot advance without wrapping.
    #[error("scene generation is exhausted while reducing input {input}")]
    SceneGenerationExhausted {
        /// Input which attempted to seal the next scene.
        input: InputSequence,
    },
    /// A transient interaction identity cannot advance without wrapping.
    #[error("interaction input {input} failed: {source}")]
    Interaction {
        /// Failing sequenced input.
        input: InputSequence,
        /// Fatal counter or internal state failure.
        source: InteractionCounterError,
    },
    /// Drop prevalidation encountered a fatal internal transaction failure.
    #[error("drop resolution input {input} failed: {source}")]
    DropResolution {
        /// Failing sequenced input.
        input: InputSequence,
        /// Fatal resolver failure.
        source: DropResolutionError,
    },
    /// Platform identity, inventory, route, or effect state could not advance atomically.
    #[error("viewport input {input} failed: {source}")]
    Viewport {
        /// Failing sequenced input.
        input: InputSequence,
        /// Typed coordinator failure.
        source: ViewportCoordinatorError,
    },
    /// Viewport activation or pane-focus identity could not advance atomically.
    #[error("viewport focus input {input} failed: {source}")]
    ViewportFocus {
        /// Failing sequenced input.
        input: InputSequence,
        /// Typed activation and pane-focus coordinator failure.
        source: ViewportFocusError,
    },
    /// A lifecycle edge could not freeze the complete logical surface roster.
    #[error("surface roster input {input} failed: {source}")]
    SurfaceRoster {
        /// Failing sequenced input.
        input: InputSequence,
        /// Exact roster capture failure.
        source: SurfaceRosterCaptureError,
    },
    /// An accepted close reached destruction without its edge-frozen roster.
    #[error("surface {surface} destroyed at input {input} without a frozen close roster")]
    MissingSurfaceRoster {
        /// Failing sequenced input.
        input: InputSequence,
        /// Destroyed logical surface.
        surface: crate::ids::SurfaceId,
    },
    /// Two lifecycle paths attempted to retain different rosters for one surface.
    #[error("surface {surface} has conflicting pending recovery rosters at input {input}")]
    ConflictingSurfaceRecovery {
        /// Failing sequenced input.
        input: InputSequence,
        /// Logical surface whose recovery ownership conflicted.
        surface: crate::ids::SurfaceId,
    },
}

/// Authoritative renderer-neutral docking engine.
#[derive(Debug, PartialEq)]
pub struct DockEngine {
    workspace: Workspace,
    policy: DockPolicy,
    version: WorkspaceVersion,
    scene: Option<SealedScene>,
    scene_coordinate_authority: BTreeMap<crate::ids::SurfaceId, SurfaceSceneCoordinateAuthority>,
    last_scene_generation: SceneGeneration,
    interaction: InteractionState,
    viewport: ViewportCoordinator,
    viewport_focus: ViewportFocusCoordinator,
    last_focus_reducer_generation: PaneFocusIntentGeneration,
    surface_recovery: SurfaceRecoveryState,
    last_input: InputSequence,
    pending: Vec<SequencedInput>,
}

#[derive(Debug, Clone)]
struct SurfaceRecoveryBatchContext {
    active_rosters: BTreeMap<crate::ids::SurfaceId, SurfaceRosterDisposition>,
    targets: BTreeMap<crate::ids::SurfaceId, SurfaceRecoveryTargetFacts>,
}

#[derive(Clone, Copy)]
struct SurfaceRecoveryTargetContext<'a> {
    action_barrier: &'a BTreeMap<crate::ids::SurfaceId, SurfaceRosterDisposition>,
    target_facts: Option<&'a SurfaceRecoveryTargetFacts>,
}

impl<'a> SurfaceRecoveryTargetContext<'a> {
    const fn new(
        action_barrier: &'a BTreeMap<crate::ids::SurfaceId, SurfaceRosterDisposition>,
        target_facts: Option<&'a SurfaceRecoveryTargetFacts>,
    ) -> Self {
        Self {
            action_barrier,
            target_facts,
        }
    }
}

struct DestroyedSurfaceContext<'a> {
    roster: Option<&'a SurfaceRosterDisposition>,
    action_barrier: &'a BTreeMap<crate::ids::SurfaceId, SurfaceRosterDisposition>,
    recovery_targets: &'a BTreeMap<crate::ids::SurfaceId, SurfaceRecoveryTargetFacts>,
    events: &'a mut Vec<WorkspaceEvent>,
}

struct PlatformSnapshotReductionContext<'a> {
    application_base: &'a mut WorkspaceVersion,
    events: &'a mut Vec<WorkspaceEvent>,
    interaction_events: &'a mut Vec<InteractionEvent>,
}

#[derive(Clone, Copy)]
struct MergeBackIntent<'a> {
    request: ViewportCloseRequestId,
    plan: &'a crate::frame::ViewportMergeBackPlan,
    dependency: crate::surface_recovery::SurfaceRecoveryTargetDependency,
    focus: PanelFocus,
}

struct MergeBackApplicationContext<'facts, 'output> {
    target: SurfaceRecoveryTargetContext<'facts>,
    activations: &'output mut Vec<ActivationStart>,
    events: &'output mut Vec<WorkspaceEvent>,
}

#[derive(Clone, Copy)]
struct CandidatePaneSelection {
    surface: crate::ids::SurfaceId,
    item: crate::ids::ItemId,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum PaneRevealDisposition {
    Pending,
    AppliedInCandidate,
}

enum MergeBackFreeze {
    Frozen {
        roster: Box<SurfaceRosterDisposition>,
        dependency: crate::surface_recovery::SurfaceRecoveryTargetDependency,
        focus: PanelFocus,
    },
    Rejected(ViewportCloseDecisionRejection),
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct SurfaceSceneCoordinateAuthority {
    scene: SceneStamp,
    coordinates: CoordinateSnapshot,
}

impl SurfaceSceneCoordinateAuthority {
    const fn new(scene: SceneStamp, coordinates: CoordinateSnapshot) -> Self {
        Self { scene, coordinates }
    }

    fn coordinates_match(captured: CoordinateSnapshot, current: CoordinateSnapshot) -> bool {
        captured.binding() == current.binding()
            && captured.coordinate_generation() == current.coordinate_generation()
            && captured.content_bounds() == current.content_bounds()
            && captured.scale_factor() == current.scale_factor()
    }

    fn matches(self, scene: SceneStamp, current: CoordinateSnapshot) -> bool {
        self.scene == scene && Self::coordinates_match(self.coordinates, current)
    }
}

#[derive(Debug, Clone, PartialEq)]
enum PreviewDecision {
    Publish {
        visual: PreviewVisual,
        proof: Box<PreviewProof>,
    },
    Clear(PreviewResolutionStatus),
    Cancel(InteractionCancelReason),
}

#[derive(Debug, Clone, PartialEq)]
struct PreviewEvaluation {
    decision: PreviewDecision,
    affordance: Option<DropAffordance>,
}

impl PreviewEvaluation {
    const fn new(decision: PreviewDecision, affordance: Option<DropAffordance>) -> Self {
        Self {
            decision,
            affordance,
        }
    }

    const fn without_affordance(decision: PreviewDecision) -> Self {
        Self::new(decision, None)
    }
}

enum ReleaseProofDecision {
    Deliver(Box<PreviewProof>),
    Reject(InteractionRejection),
}

#[derive(Clone, Copy)]
struct DragReleaseInput<'a> {
    session: crate::interaction::DragSessionId,
    pointer: crate::intent::PointerId,
    button: crate::intent::PointerButton,
    button_state: &'a Authority<PointerButtonState>,
    target: &'a TargetAuthority,
    tear_off: Option<&'a TearOffRequest>,
}

#[derive(Clone, Copy)]
struct ObservedDragReleaseInput<'a> {
    session: crate::interaction::DragSessionId,
    pointer: crate::intent::PointerId,
    button: crate::intent::PointerButton,
    button_state: &'a Authority<PointerButtonState>,
    target: &'a TargetAuthority,
    current_pointer: &'a Authority<SurfacePointer>,
    contained_offer: Option<ContainedPresentationOffer>,
}

enum CoreContainedCandidate {
    None,
    Request(TearOffRequest),
    Rejected,
    Cancel(InteractionCancelReason),
}

struct PreparedDragSource {
    complete_root: Option<NodeSource>,
    partial_detachable: bool,
}

#[cfg(test)]
thread_local! {
    static PARTIAL_DETACHABILITY_EVALUATIONS: std::cell::Cell<usize> = const {
        std::cell::Cell::new(0)
    };
}

#[derive(Clone, Copy)]
struct ContainedPlacementInput {
    root: crate::ids::RootId,
    floating: crate::ids::FloatingPresentationId,
    expected_rect: crate::geometry::LogicalRect,
    placement: ContainedPlacementProof,
}

#[derive(Clone, Copy)]
struct ContainedTransformBeginInput {
    surface: crate::ids::SurfaceId,
    root: crate::ids::RootId,
    floating: crate::ids::FloatingPresentationId,
    pointer: crate::intent::PointerId,
    button: crate::intent::PointerButton,
    initial_pointer: crate::geometry::LogicalPoint,
    kind: ContainedTransformKind,
    minimum_size: crate::geometry::LogicalSize,
}

#[derive(Clone, Copy)]
struct ContainedTransformReleaseInput<'a> {
    session: ContainedTransformSessionId,
    pointer: crate::intent::PointerId,
    button: crate::intent::PointerButton,
    button_state: &'a Authority<PointerButtonState>,
}

#[derive(Clone, Copy)]
struct WorkspaceDeliveryTarget {
    kind: WorkspaceDeliveryKind,
    focus_surface: crate::ids::SurfaceId,
    validation: WorkspaceDeliveryValidation,
}

#[derive(Clone, Copy)]
struct WorkspaceDeliveryInput<'a> {
    input: InputSequence,
    session: crate::interaction::DragSessionId,
    target: WorkspaceDeliveryTarget,
    pane_focus: PanelFocus,
    command: &'a WorkspaceCommand,
}

#[derive(Clone, Copy)]
enum WorkspaceDeliveryValidation {
    Policy,
    ExistingContainedRect,
}

#[derive(Default)]
struct PlatformInteractionDependencies {
    surfaces: BTreeSet<crate::ids::SurfaceId>,
    routed: bool,
    native: bool,
}

enum CommandApplication {
    Applied {
        outcome: CommandOutcome,
        changed: bool,
    },
    Rejected(crate::error::CommandError),
}

fn clamp_contained_rect(
    surface: crate::ids::SurfaceId,
    bounds: crate::geometry::LogicalRect,
    requested: crate::geometry::LogicalRect,
    minimum: crate::geometry::LogicalSize,
) -> Result<crate::geometry::LogicalRect, ContainedPlacementUnavailable> {
    let bounds_width = bounds.width();
    let bounds_height = bounds.height();
    let requested_width = requested.width();
    let requested_height = requested.height();
    if !bounds_width.is_finite()
        || !bounds_height.is_finite()
        || !requested_width.is_finite()
        || !requested_height.is_finite()
    {
        return Err(ContainedPlacementUnavailable::UnrepresentableGeometry { surface });
    }

    let width = requested_width.max(minimum.width()).min(bounds_width);
    let height = requested_height.max(minimum.height()).min(bounds_height);
    let (min_x, max_x) = clamp_axis(bounds.x(), bounds.max().x(), requested.x(), width)
        .ok_or(ContainedPlacementUnavailable::UnrepresentableGeometry { surface })?;
    let (min_y, max_y) = clamp_axis(bounds.y(), bounds.max().y(), requested.y(), height)
        .ok_or(ContainedPlacementUnavailable::UnrepresentableGeometry { surface })?;
    let min = crate::geometry::LogicalPoint::new(min_x, min_y)
        .map_err(|_| ContainedPlacementUnavailable::UnrepresentableGeometry { surface })?;
    let max = crate::geometry::LogicalPoint::new(max_x, max_y)
        .map_err(|_| ContainedPlacementUnavailable::UnrepresentableGeometry { surface })?;
    crate::geometry::LogicalRect::from_min_max(min, max)
        .map_err(|_| ContainedPlacementUnavailable::UnrepresentableGeometry { surface })
}

fn clamp_axis(
    bounds_min: f64,
    bounds_max: f64,
    requested_min: f64,
    extent: f64,
) -> Option<(f64, f64)> {
    let latest_min = bounds_max - extent;
    let (minimum, maximum) = if requested_min <= bounds_min {
        (bounds_min, bounds_min + extent)
    } else if requested_min >= latest_min {
        (latest_min, bounds_max)
    } else {
        (requested_min, requested_min + extent)
    };
    minimum
        .is_finite()
        .then_some(())
        .filter(|()| maximum.is_finite() && minimum <= maximum)
        .map(|()| (minimum, maximum))
}

fn translated_contained_rect(
    source: crate::geometry::LogicalRect,
    initial_pointer: crate::geometry::LogicalPoint,
    current_pointer: crate::geometry::LogicalPoint,
) -> Result<crate::geometry::LogicalRect, ()> {
    let delta_x = current_pointer.x() - initial_pointer.x();
    let delta_y = current_pointer.y() - initial_pointer.y();
    if !delta_x.is_finite() || !delta_y.is_finite() {
        return Err(());
    }
    crate::geometry::LogicalRect::new(
        source.x() + delta_x,
        source.y() + delta_y,
        source.width(),
        source.height(),
    )
    .map_err(|_| ())
}

#[derive(Clone, Copy)]
enum TransformAxisEdge {
    Minimum,
    Maximum,
}

fn contained_transform_requested_rect(
    transform: &ActiveContainedTransform,
    current_pointer: crate::geometry::LogicalPoint,
    bounds: crate::geometry::LogicalRect,
) -> Result<crate::geometry::LogicalRect, ()> {
    let delta_x = current_pointer.x() - transform.initial_pointer.x();
    let delta_y = current_pointer.y() - transform.initial_pointer.y();
    if !delta_x.is_finite() || !delta_y.is_finite() {
        return Err(());
    }
    match transform.kind {
        ContainedTransformKind::Move => crate::geometry::LogicalRect::new(
            transform.source_rect.x() + delta_x,
            transform.source_rect.y() + delta_y,
            transform.source_rect.width(),
            transform.source_rect.height(),
        )
        .map_err(|_| ()),
        ContainedTransformKind::Resize(edges) => {
            let horizontal = match edges.horizontal_edge() {
                Some(ContainedHorizontalResizeEdge::Left) => Some(TransformAxisEdge::Minimum),
                Some(ContainedHorizontalResizeEdge::Right) => Some(TransformAxisEdge::Maximum),
                None => None,
            };
            let vertical = match edges.vertical_edge() {
                Some(ContainedVerticalResizeEdge::Top) => Some(TransformAxisEdge::Minimum),
                Some(ContainedVerticalResizeEdge::Bottom) => Some(TransformAxisEdge::Maximum),
                None => None,
            };
            let (min_x, max_x) = contained_resize_axis(
                transform.source_rect.x(),
                transform.source_rect.max().x(),
                bounds.x(),
                bounds.max().x(),
                transform.minimum_size.width(),
                delta_x,
                horizontal,
            )
            .ok_or(())?;
            let (min_y, max_y) = contained_resize_axis(
                transform.source_rect.y(),
                transform.source_rect.max().y(),
                bounds.y(),
                bounds.max().y(),
                transform.minimum_size.height(),
                delta_y,
                vertical,
            )
            .ok_or(())?;
            let min = crate::geometry::LogicalPoint::new(min_x, min_y).map_err(|_| ())?;
            let max = crate::geometry::LogicalPoint::new(max_x, max_y).map_err(|_| ())?;
            crate::geometry::LogicalRect::from_min_max(min, max).map_err(|_| ())
        }
    }
}

fn contained_resize_axis(
    source_min: f64,
    source_max: f64,
    bounds_min: f64,
    bounds_max: f64,
    minimum_extent: f64,
    delta: f64,
    moving_edge: Option<TransformAxisEdge>,
) -> Option<(f64, f64)> {
    if !source_min.is_finite()
        || !source_max.is_finite()
        || !bounds_min.is_finite()
        || !bounds_max.is_finite()
        || !minimum_extent.is_finite()
        || !delta.is_finite()
    {
        return None;
    }
    match moving_edge {
        None => (source_min >= bounds_min && source_max <= bounds_max)
            .then_some((source_min, source_max)),
        Some(TransformAxisEdge::Minimum) => {
            let latest_min = source_max - minimum_extent;
            if source_max > bounds_max || bounds_min > latest_min {
                return None;
            }
            let requested = source_min + delta;
            requested
                .is_finite()
                .then(|| requested.clamp(bounds_min, latest_min))
                .map(|minimum| (minimum, source_max))
        }
        Some(TransformAxisEdge::Maximum) => {
            let earliest_max = source_min + minimum_extent;
            if source_min < bounds_min || earliest_max > bounds_max {
                return None;
            }
            let requested = source_max + delta;
            requested
                .is_finite()
                .then(|| requested.clamp(earliest_max, bounds_max))
                .map(|maximum| (source_min, maximum))
        }
    }
}

impl DockEngine {
    /// Creates an engine from a strictly validated workspace and explicit policy.
    ///
    /// # Errors
    ///
    /// Returns [`EngineError::InvalidWorkspace`] if `workspace` is corrupted.
    pub fn new(workspace: Workspace, policy: DockPolicy) -> Result<Self, EngineError> {
        workspace
            .validate()
            .map_err(EngineError::InvalidWorkspace)?;
        Ok(Self {
            workspace,
            policy,
            version: WorkspaceVersion::default(),
            scene: None,
            scene_coordinate_authority: BTreeMap::new(),
            last_scene_generation: SceneGeneration::default(),
            interaction: InteractionState::default(),
            viewport: ViewportCoordinator::default(),
            viewport_focus: ViewportFocusCoordinator::default(),
            last_focus_reducer_generation: PaneFocusIntentGeneration::default(),
            surface_recovery: SurfaceRecoveryState::default(),
            last_input: InputSequence::default(),
            pending: Vec::new(),
        })
    }

    /// Returns the published workspace.
    #[must_use]
    pub const fn workspace(&self) -> &Workspace {
        &self.workspace
    }

    /// Returns current application policy.
    #[must_use]
    pub const fn policy(&self) -> &DockPolicy {
        &self.policy
    }

    /// Returns the version required by state-derived inputs.
    #[must_use]
    pub const fn version(&self) -> WorkspaceVersion {
        self.version
    }

    /// Returns the current immutable scene eligible for painting.
    #[must_use]
    pub const fn scene(&self) -> Option<&SealedScene> {
        self.scene.as_ref()
    }

    /// Returns the published transient interaction state.
    #[must_use]
    pub const fn interaction(&self) -> &InteractionState {
        &self.interaction
    }

    /// Returns the core-owned platform, binding, route, and effect coordinator.
    #[must_use]
    pub const fn viewport(&self) -> &ViewportCoordinator {
        &self.viewport
    }

    /// Returns adapter-neutral native activation and pane-focus state.
    #[must_use]
    pub const fn viewport_focus(&self) -> &ViewportFocusCoordinator {
        &self.viewport_focus
    }

    /// Returns the exact current native binding eligible to publish pane-focus facts.
    #[must_use]
    pub fn viewport_focus_binding(
        &self,
        surface: crate::ids::SurfaceId,
    ) -> Option<crate::viewport::ViewportBinding> {
        self.viewport
            .registry()
            .record(surface)
            .filter(|record| record.is_focusable())
            .map(crate::viewport_registry::ViewportRecord::binding)
    }

    /// Returns the complete roster retained for one unresolved destroyed surface.
    #[must_use]
    pub fn pending_surface_recovery(
        &self,
        surface: crate::ids::SurfaceId,
    ) -> Option<&SurfaceRosterDisposition> {
        self.surface_recovery.pending(surface)
    }

    /// Returns queued inputs in writer order.
    #[must_use]
    pub fn pending_inputs(&self) -> &[SequencedInput] {
        &self.pending
    }

    /// Assigns the next monotonic sequence and queues an input.
    ///
    /// # Errors
    ///
    /// Returns [`EngineError::InputSequenceExhausted`] instead of wrapping.
    pub fn enqueue(&mut self, input: EngineInput) -> Result<InputSequence, EngineError> {
        let sequence = self
            .last_input
            .checked_next()
            .ok_or(EngineError::InputSequenceExhausted)?;
        let scene_coordinate_proofs = if matches!(&input, EngineInput::PublishScene { .. }) {
            self.viewport
                .registry()
                .records()
                .filter_map(|(surface, record)| {
                    record
                        .coordinates()
                        .map(|coordinates| (surface, coordinates))
                })
                .collect()
        } else {
            BTreeMap::new()
        };
        self.last_input = sequence;
        self.pending.push(SequencedInput {
            sequence,
            input,
            scene_coordinate_proofs,
        });
        Ok(sequence)
    }

    /// Queues a command against the currently published state version.
    ///
    /// # Errors
    ///
    /// Returns [`EngineError::InputSequenceExhausted`] instead of wrapping.
    pub fn enqueue_command(
        &mut self,
        command: WorkspaceCommand,
    ) -> Result<InputSequence, EngineError> {
        self.enqueue(EngineInput::WorkspaceCommand {
            expected: self.version,
            command,
        })
    }

    /// Queues a complete authoritative workspace replacement.
    ///
    /// # Errors
    ///
    /// Returns [`EngineError::InputSequenceExhausted`] instead of wrapping.
    pub fn enqueue_workspace_replacement(
        &mut self,
        workspace: Workspace,
    ) -> Result<InputSequence, EngineError> {
        self.enqueue(EngineInput::ReplaceWorkspace(workspace))
    }

    /// Queues registration of one existing native adapter window.
    ///
    /// # Errors
    ///
    /// Returns [`EngineError::InputSequenceExhausted`] instead of wrapping.
    pub fn enqueue_viewport_registration(
        &mut self,
        surface: crate::ids::SurfaceId,
        token: WindowToken,
        role: ViewportRole,
        recovery: Option<crate::intent::ContainedTearOffProposal>,
    ) -> Result<InputSequence, EngineError> {
        self.enqueue(EngineInput::RegisterViewport {
            expected: self.version,
            surface,
            token,
            role,
            recovery,
        })
    }

    /// Queues one complete frame-before-paint platform fact snapshot.
    ///
    /// # Errors
    ///
    /// Returns [`EngineError::InputSequenceExhausted`] instead of wrapping.
    pub fn enqueue_platform_snapshot(
        &mut self,
        snapshot: PlatformSnapshot,
    ) -> Result<InputSequence, EngineError> {
        self.enqueue(EngineInput::PublishPlatformSnapshot {
            expected_epoch: self.version.epoch(),
            snapshot,
        })
    }

    /// Queues one correlated platform effect dispatch result.
    ///
    /// # Errors
    ///
    /// Returns [`EngineError::InputSequenceExhausted`] instead of wrapping.
    pub fn enqueue_platform_effect_result(
        &mut self,
        result: EffectResult,
    ) -> Result<InputSequence, EngineError> {
        self.enqueue(EngineInput::ReportPlatformEffect {
            expected_epoch: self.version.epoch(),
            result,
        })
    }

    /// Queues explicit native activation and item-or-none pane focus for one exact binding.
    ///
    /// # Errors
    ///
    /// Returns [`EngineError::InputSequenceExhausted`] instead of wrapping.
    pub fn enqueue_viewport_activation(
        &mut self,
        target: crate::viewport::ViewportBinding,
        focus: PanelFocus,
    ) -> Result<InputSequence, EngineError> {
        self.enqueue(EngineInput::ActivateViewport {
            expected: self.version,
            request: ViewportActivationRequest::explicit(target, focus),
        })
    }

    /// Queues one exact adapter-observed pane-focus fact.
    ///
    /// # Errors
    ///
    /// Returns [`EngineError::InputSequenceExhausted`] instead of wrapping.
    pub fn enqueue_pane_focus_observation(
        &mut self,
        observation: PaneFocusObservation,
    ) -> Result<InputSequence, EngineError> {
        self.enqueue(EngineInput::PublishPaneFocusObservation {
            expected_epoch: self.version.epoch(),
            observation,
        })
    }

    /// Queues an application decision for one exact native close-request edge.
    ///
    /// # Errors
    ///
    /// Returns [`EngineError::InputSequenceExhausted`] instead of wrapping.
    pub fn enqueue_viewport_close_decision(
        &mut self,
        request: ViewportCloseRequestId,
        decision: ViewportCloseDecision,
    ) -> Result<InputSequence, EngineError> {
        self.enqueue(EngineInput::DecideViewportClose {
            expected: self.version,
            request,
            decision,
        })
    }

    /// Queues an explicit cancellation for one unresolved native-create saga.
    ///
    /// # Errors
    ///
    /// Returns [`EngineError::InputSequenceExhausted`] instead of wrapping.
    pub fn enqueue_native_create_cancellation(
        &mut self,
        saga: NativeCreateSagaId,
    ) -> Result<InputSequence, EngineError> {
        self.enqueue(EngineInput::CancelNativeCreate {
            expected: self.version,
            saga,
        })
    }

    /// Queues one explicit phase-qualified cleanup retry.
    ///
    /// A destructive cleanup is eligible only after a definitive dispatch failure and retains
    /// its existing cleanup-specific retry rules. An observation-only cleanup is eligible after
    /// `ObservationDispatchFailed` or `ObservationUnsupported`; its retry is another
    /// `ContinueCleanup` with the same original destructive predecessor and never re-executes
    /// that predecessor. Indeterminate and terminal effects are not retryable.
    ///
    /// # Errors
    ///
    /// Returns [`EngineError::InputSequenceExhausted`] instead of wrapping.
    pub fn enqueue_viewport_cleanup_retry(
        &mut self,
        failed_effect: crate::effect::EffectId,
    ) -> Result<InputSequence, EngineError> {
        self.enqueue(EngineInput::RetryViewportCleanup {
            expected: self.version,
            failed_effect,
        })
    }

    /// Produces a native placement proof from current acknowledged platform facts.
    ///
    /// # Errors
    ///
    /// Returns a typed coordinate error when the surface is not ready or its
    /// placement facts are incomplete.
    pub fn viewport_placement(
        &self,
        surface: crate::ids::SurfaceId,
        rect: crate::geometry::LogicalRect,
        work_area: crate::viewport::WorkAreaToken,
    ) -> Result<crate::coordinates::ViewportPlacementProof, crate::coordinates::CoordinateUnavailable>
    {
        self.viewport.placement(surface, rect, work_area)
    }

    /// Produces a native tear-off placement from the current authoritative
    /// desktop pointer route and an explicitly selected work area.
    ///
    /// The cursor offset, preferred size, and minimum size are logical values
    /// for the selected work area. They are scaled exactly once and the result
    /// is clamped without monitor-selection heuristics.
    ///
    /// # Errors
    ///
    /// Returns a typed viewport-coordinate error when the pointer route, work
    /// area, or placement facts are unavailable or stale.
    pub fn tear_off_placement(
        &self,
        pointer: crate::intent::PointerId,
        request: crate::coordinates::TearOffPlacementRequest,
    ) -> Result<crate::coordinates::TearOffPlacementProof, ViewportCoordinatorError> {
        self.viewport.tear_off_placement(pointer, request)
    }

    #[must_use]
    pub fn native_placement_is_current(&self, proof: &crate::intent::NativePlacementProof) -> bool {
        self.viewport.native_placement_is_current(proof)
    }

    /// Produces a deterministic contained placement from current ready surface bounds.
    ///
    /// The returned proof is valid only for the exact sealed scene generation from which it was
    /// derived. The requested size is first expanded to `minimum_size`, capped by the surface
    /// bounds, and then translated into those bounds without any history-based heuristic.
    ///
    /// # Errors
    ///
    /// Returns [`ContainedPlacementUnavailable`] when no current ready scene exists or finite
    /// corners cannot represent a finite clamp.
    pub fn contained_placement(
        &self,
        surface: crate::ids::SurfaceId,
        requested_rect: crate::geometry::LogicalRect,
        minimum_size: crate::geometry::LogicalSize,
    ) -> Result<ContainedPlacementProof, ContainedPlacementUnavailable> {
        let (stamp, bounds) = self.current_ready_surface_bounds(surface)?;
        let clamped_rect = clamp_contained_rect(surface, bounds, requested_rect, minimum_size)?;
        Ok(ContainedPlacementProof::new(
            stamp,
            surface,
            requested_rect,
            minimum_size,
            bounds,
            clamped_rect,
        ))
    }

    fn current_ready_surface_bounds(
        &self,
        surface: crate::ids::SurfaceId,
    ) -> Result<(SceneStamp, crate::geometry::LogicalRect), ContainedPlacementUnavailable> {
        let scene = self
            .scene
            .as_ref()
            .ok_or(ContainedPlacementUnavailable::SceneUnavailable)?;
        if scene.stamp().workspace() != self.version {
            return Err(ContainedPlacementUnavailable::SceneUnavailable);
        }
        let bounds = match scene.surface(surface) {
            Some(SurfaceScene::Ready(ready)) => ready.bounds(),
            Some(SurfaceScene::Bootstrap(_)) => {
                return Err(ContainedPlacementUnavailable::BootstrapSurface { surface });
            }
            None => return Err(ContainedPlacementUnavailable::MissingSurface { surface }),
        };
        Ok((scene.stamp(), bounds))
    }

    fn validate_contained_placement(
        &self,
        proof: ContainedPlacementProof,
    ) -> Result<(), ContainedPlacementUnavailable> {
        let Some(scene) = self.scene.as_ref() else {
            return Err(ContainedPlacementUnavailable::StaleScene {
                expected: proof.scene(),
                current: None,
            });
        };
        if scene.stamp() != proof.scene() || scene.stamp().workspace() != self.version {
            return Err(ContainedPlacementUnavailable::StaleScene {
                expected: proof.scene(),
                current: Some(scene.stamp()),
            });
        }
        let ready = match scene.surface(proof.surface()) {
            Some(SurfaceScene::Ready(ready)) => ready,
            Some(SurfaceScene::Bootstrap(_)) => {
                return Err(ContainedPlacementUnavailable::BootstrapSurface {
                    surface: proof.surface(),
                });
            }
            None => {
                return Err(ContainedPlacementUnavailable::MissingSurface {
                    surface: proof.surface(),
                });
            }
        };
        if ready.bounds() != proof.surface_bounds() {
            return Err(ContainedPlacementUnavailable::ProofMismatch {
                surface: proof.surface(),
            });
        }
        let reproduced = clamp_contained_rect(
            proof.surface(),
            ready.bounds(),
            proof.requested_rect(),
            proof.minimum_size(),
        )?;
        if reproduced != proof.clamped_rect() {
            return Err(ContainedPlacementUnavailable::ProofMismatch {
                surface: proof.surface(),
            });
        }
        Ok(())
    }

    /// Queues complete scene facts against the currently published state.
    ///
    /// # Errors
    ///
    /// Returns [`EngineError::InputSequenceExhausted`] instead of wrapping.
    pub fn enqueue_scene(&mut self, scene: BuildingScene) -> Result<InputSequence, EngineError> {
        self.enqueue(EngineInput::PublishScene {
            expected: self.version,
            scene,
        })
    }

    /// Queues one renderer intent against the currently published state.
    ///
    /// # Errors
    ///
    /// Returns [`EngineError::InputSequenceExhausted`] instead of wrapping.
    pub fn enqueue_renderer_intent(
        &mut self,
        intent: RendererIntent,
    ) -> Result<InputSequence, EngineError> {
        self.enqueue(EngineInput::RendererIntent {
            expected: self.version,
            intent: Box::new(intent),
        })
    }

    /// Discards all queued inputs without changing published state.
    pub fn discard_pending(&mut self) -> Vec<SequencedInput> {
        std::mem::take(&mut self.pending)
    }

    /// Reduces queued inputs into one candidate and publishes it atomically.
    ///
    /// Inputs are ordered by [`InputPriority`] and then writer sequence. Events
    /// are returned only if the complete candidate commits. Expected command
    /// rejections are consumed and recorded in the transition. A fatal internal
    /// transaction error leaves the engine, including its pending queue, unchanged.
    ///
    /// # Errors
    ///
    /// Returns [`EngineError`] on counter exhaustion, invalid replacement state,
    /// or an internal transaction failure. No engine state is changed on failure.
    pub fn reduce_pending(&mut self) -> Result<EngineTransition, EngineError> {
        let before = self.version;
        let before_scene = self.scene.clone();
        let before_interaction = self.interaction.clone();
        let before_viewport = self.viewport.clone();
        let before_viewport_focus = self.viewport_focus.clone();
        let mut candidate = self.candidate();
        let mut inputs = std::mem::take(&mut candidate.pending);
        inputs.sort_by_key(|input| {
            (
                input.input.priority(),
                input.input.reduction_rank(),
                input.sequence,
            )
        });

        let mut reduced = Vec::with_capacity(inputs.len());
        let mut events = Vec::new();
        let mut interaction_events = Vec::new();
        let mut application_base = before;
        let mut scene_published = false;
        for input in inputs {
            let priority = input.input.priority();
            let outcome = candidate.reduce_one(
                &input,
                &mut application_base,
                &mut scene_published,
                &mut events,
                &mut interaction_events,
            )?;
            reduced.push(ReducedInput::new(input.sequence, priority, outcome));
        }

        let platform_effects = candidate.viewport.take_new_effects();
        let observed_focus_effects = Self::observed_focus_effects(&reduced);
        let focus_delta = FocusDelta::between(
            &before_viewport_focus,
            &candidate.viewport_focus,
            before_viewport.effects(),
            candidate.viewport.effects(),
            &observed_focus_effects,
        );
        let published_state_changed = before != candidate.version
            || before_scene != candidate.scene
            || before_interaction != candidate.interaction
            || before_viewport != candidate.viewport
            || !focus_delta.is_empty();
        let transition = EngineTransition::new(EngineTransitionParts {
            before,
            after: candidate.version,
            reduced,
            events,
            interaction_events,
            platform_effects,
            focus_delta,
            published_state_changed,
        });
        *self = candidate;
        Ok(transition)
    }

    fn observed_focus_effects(reduced: &[ReducedInput]) -> Vec<ObservedPlatformFocusEffect> {
        reduced
            .iter()
            .flat_map(|input| {
                let observed = match input.outcome() {
                    InputOutcome::PlatformSnapshotPublished {
                        focus: FocusObservationTransition::Applied(applied),
                        ..
                    } => [
                        applied.observed_effect(),
                        applied.acknowledged_effect_settlement(),
                    ],
                    InputOutcome::ViewportRegistered { .. }
                    | InputOutcome::ViewportRegistrationRejected { .. }
                    | InputOutcome::PlatformSnapshotPublished { .. }
                    | InputOutcome::PlatformSnapshotStale { .. }
                    | InputOutcome::PlatformEffectReported { .. }
                    | InputOutcome::ViewportActivationRequested { .. }
                    | InputOutcome::ViewportActivationRejected { .. }
                    | InputOutcome::PaneFocusObservationPublished { .. }
                    | InputOutcome::PaneFocusObservationStale { .. }
                    | InputOutcome::ViewportCloseDecided { .. }
                    | InputOutcome::ViewportCloseDecisionRejected { .. }
                    | InputOutcome::NativeCreateCancelled { .. }
                    | InputOutcome::ViewportCleanupRetried { .. }
                    | InputOutcome::WorkspaceReplaced { .. }
                    | InputOutcome::CommandProcessed { .. }
                    | InputOutcome::CommandRejected { .. }
                    | InputOutcome::PolicyReplaced { .. }
                    | InputOutcome::ScenePublished { .. }
                    | InputOutcome::SceneRejected { .. }
                    | InputOutcome::InteractionProcessed { .. }
                    | InputOutcome::WorkspaceValidated { .. }
                    | InputOutcome::StaleRejected { .. } => [None, None],
                };
                observed.into_iter().flatten()
            })
            .collect()
    }

    // This is the sole sorted-input dispatch table; keeping every input variant visible here
    // makes reducer ordering auditable and prevents hidden secondary dispatch.
    #[allow(clippy::too_many_lines)]
    fn reduce_one(
        &mut self,
        input: &SequencedInput,
        application_base: &mut WorkspaceVersion,
        scene_published: &mut bool,
        events: &mut Vec<WorkspaceEvent>,
        interaction_events: &mut Vec<InteractionEvent>,
    ) -> Result<InputOutcome, EngineError> {
        let focus_generation = self.last_focus_reducer_generation.checked_next().ok_or(
            EngineError::ViewportFocus {
                input: input.sequence,
                source: ViewportFocusError::ReducerGenerationExhausted,
            },
        )?;
        self.last_focus_reducer_generation = focus_generation;
        match &input.input {
            EngineInput::RegisterViewport {
                expected,
                surface,
                token,
                role,
                recovery,
            } => self.reduce_viewport_registration(
                input.sequence,
                *expected,
                *surface,
                *token,
                *role,
                *recovery,
            ),
            EngineInput::PublishPlatformSnapshot {
                expected_epoch,
                snapshot,
            } => self.reduce_platform_snapshot(
                input.sequence,
                *expected_epoch,
                snapshot,
                focus_generation,
                PlatformSnapshotReductionContext {
                    application_base,
                    events,
                    interaction_events,
                },
            ),
            EngineInput::ReportPlatformEffect {
                expected_epoch,
                result,
            } => self.reduce_platform_effect(input.sequence, *expected_epoch, *result),
            EngineInput::ActivateViewport { expected, request } => self.reduce_viewport_activation(
                input.sequence,
                *expected,
                *request,
                focus_generation,
                *application_base,
                events,
            ),
            EngineInput::PublishPaneFocusObservation {
                expected_epoch,
                observation,
            } => Ok(self.reduce_pane_focus_observation(*expected_epoch, *observation)),
            EngineInput::DecideViewportClose {
                expected,
                request,
                decision,
            } => self.reduce_viewport_close_decision(
                input.sequence,
                *expected,
                *request,
                decision.clone(),
            ),
            EngineInput::CancelNativeCreate { expected, saga } => {
                self.reduce_native_create_cancellation(input.sequence, *expected, *saga)
            }
            EngineInput::RetryViewportCleanup {
                expected,
                failed_effect,
            } => self.reduce_viewport_cleanup_retry(input.sequence, *expected, *failed_effect),
            EngineInput::ReplaceWorkspace(workspace) => self.reduce_workspace_replacement(
                input.sequence,
                workspace,
                application_base,
                events,
                interaction_events,
            ),
            EngineInput::WorkspaceCommand { expected, command } => self.reduce_workspace_command(
                input.sequence,
                *expected,
                *application_base,
                command,
                events,
                interaction_events,
            ),
            EngineInput::ReplacePolicy { expected, policy } => self.reduce_policy_replacement(
                input.sequence,
                *expected,
                *application_base,
                policy,
                events,
                interaction_events,
            ),
            EngineInput::PublishScene { expected, scene } => self.reduce_scene(
                input.sequence,
                *expected,
                scene,
                &input.scene_coordinate_proofs,
                scene_published,
                interaction_events,
            ),
            EngineInput::RendererIntent { expected, intent } => self.reduce_renderer_input(
                input.sequence,
                *expected,
                intent,
                events,
                interaction_events,
            ),
            EngineInput::ValidateWorkspace => {
                self.workspace
                    .validate()
                    .map_err(EngineError::InvalidWorkspace)?;
                Ok(InputOutcome::WorkspaceValidated {
                    version: self.version,
                })
            }
        }
    }

    fn reduce_viewport_registration(
        &mut self,
        input: InputSequence,
        expected: WorkspaceVersion,
        surface: crate::ids::SurfaceId,
        token: WindowToken,
        role: ViewportRole,
        recovery: Option<crate::intent::ContainedTearOffProposal>,
    ) -> Result<InputOutcome, EngineError> {
        if expected != self.version {
            return Ok(InputOutcome::StaleRejected {
                expected,
                accepted_base: self.version,
            });
        }
        let Some(presentation) = self.workspace.surface(surface) else {
            return Ok(InputOutcome::ViewportRegistrationRejected { surface });
        };
        if role == ViewportRole::Child && recovery.is_none() {
            return Ok(InputOutcome::ViewportRegistrationRejected { surface });
        }
        let recovery_plan = recovery.map(crate::intent::ContainedRecoveryPlan::from_proposal);
        let exact_pending_adoption =
            self.viewport
                .recovery_pending(surface)
                .is_some_and(|pending| {
                    pending.replacement_binding().is_none()
                        && pending.role() == role
                        && recovery_plan.is_some_and(|candidate| {
                            pending.recovery().matches_registration(candidate)
                        })
                });
        if !exact_pending_adoption
            && recovery_plan.is_some_and(|recovery| {
                self.contained_placement(
                    recovery.surface(),
                    recovery.requested_rect(),
                    recovery.minimum_size(),
                )
                .is_err()
            })
        {
            return Ok(InputOutcome::ViewportRegistrationRejected { surface });
        }
        if let Some(recovery) = recovery_plan
            && (presentation.main_root != recovery.root()
                || recovery.surface() == surface
                || self.workspace.surface(recovery.surface()).is_none())
        {
            return Ok(InputOutcome::ViewportRegistrationRejected { surface });
        }
        let binding = match self.viewport.register_existing(
            self.version.epoch(),
            surface,
            token,
            role,
            recovery_plan,
        ) {
            Ok(binding) => binding,
            Err(ViewportCoordinatorError::PendingRecoveryRegistrationMismatch { .. }) => {
                return Ok(InputOutcome::ViewportRegistrationRejected { surface });
            }
            Err(source) => return Err(EngineError::Viewport { input, source }),
        };
        Ok(InputOutcome::ViewportRegistered { binding })
    }

    fn reduce_platform_snapshot(
        &mut self,
        input: InputSequence,
        expected_epoch: crate::ids::WorkspaceEpoch,
        snapshot: &PlatformSnapshot,
        focus_generation: PaneFocusIntentGeneration,
        context: PlatformSnapshotReductionContext<'_>,
    ) -> Result<InputOutcome, EngineError> {
        let PlatformSnapshotReductionContext {
            application_base,
            events,
            interaction_events,
        } = context;
        if expected_epoch != self.version.epoch() {
            return Ok(InputOutcome::PlatformSnapshotStale {
                expected_epoch,
                current_epoch: self.version.epoch(),
            });
        }
        let previous_native = self.viewport.native_tear_off_capability();
        let previous_routing = self.viewport.capabilities().cross_surface_routing();
        let previous_release = self.viewport.capabilities().authoritative_release();
        let interaction_dependencies = self.platform_interaction_dependencies();
        let version_before_actions = self.version;
        let transition = self
            .viewport
            .publish_snapshot(snapshot)
            .map_err(|source| EngineError::Viewport { input, source })?;
        for event in transition.registry_events() {
            if let crate::viewport_registry::RegistryEvent::Destroyed { binding } = event {
                let _ = self.viewport_focus.observe_destroyed_binding(*binding);
            }
        }
        self.reconcile_viewport_focus_authority();
        let focus = self.reduce_global_focus_observation(
            input,
            snapshot.focus(),
            focus_generation,
            Self::platform_focus_restore_gate(snapshot),
            events,
        )?;
        self.freeze_close_focus_edges(transition.close_requests());
        self.invalidate_changed_surface_scene_authority();
        let actions = transition.actions().to_vec();
        let recovery_batch = self.freeze_surface_recovery_batch(input, &actions)?;
        let mut activations = Vec::new();
        self.reduce_viewport_actions(
            input,
            focus_generation,
            &actions,
            &recovery_batch,
            &mut activations,
            events,
        )?;
        let viewport = &self.viewport;
        self.surface_recovery.retain_accepted_closes(|request| {
            viewport
                .viewport_close_request(request)
                .is_some_and(|request| {
                    matches!(
                        request.status(),
                        ViewportCloseStatus::AwaitingDestroyed { .. }
                            | ViewportCloseStatus::EffectFailed { .. }
                            | ViewportCloseStatus::Indeterminate { .. }
                    )
                })
        });
        self.surface_recovery.retain_close_focus(|request| {
            viewport
                .viewport_close_request(request)
                .is_some_and(|request| !matches!(request.status(), ViewportCloseStatus::Cleared))
        });
        self.surface_recovery
            .retain_pending(|surface| viewport.recovery_pending(surface).is_some());
        *application_base = self.version;
        if let Some(reason) = self.platform_interaction_cancel_reason(
            &interaction_dependencies,
            version_before_actions,
            &transition,
            previous_native,
            previous_routing,
            previous_release,
        ) {
            self.invalidate_transient(input, reason, interaction_events)?;
        }
        Ok(InputOutcome::PlatformSnapshotPublished {
            transition,
            focus,
            activations,
        })
    }

    fn freeze_close_focus_edges(&mut self, requests: &[ViewportCloseRequestId]) {
        for request in requests {
            let Some(surface) = self
                .viewport
                .viewport_close_request(*request)
                .map(|request| request.binding().surface())
            else {
                continue;
            };
            let focus = match self.viewport_focus.panel_focus(surface) {
                PanelFocusRecord::Item(item) if self.surface_items(surface).contains(&item) => {
                    PanelFocus::Item(item)
                }
                PanelFocusRecord::NoHistory
                | PanelFocusRecord::Item(_)
                | PanelFocusRecord::None => PanelFocus::None,
            };
            self.surface_recovery.freeze_close_focus(*request, focus);
        }
    }

    fn reduce_global_focus_observation(
        &mut self,
        input: InputSequence,
        observation: crate::viewport_focus::FocusObservationEnvelope,
        focus_generation: PaneFocusIntentGeneration,
        restore_gate: PlatformFocusRestoreGate,
        events: &mut Vec<WorkspaceEvent>,
    ) -> Result<FocusObservationTransition, EngineError> {
        let (bindings, items) = self.focus_validation_snapshot();
        let mut transition = self
            .viewport_focus
            .publish_platform_focus_observation(
                observation,
                focus_generation,
                restore_gate,
                |binding| bindings.contains(&binding),
                |surface, item| {
                    items
                        .get(&surface)
                        .is_some_and(|surface_items| surface_items.contains(&item))
                },
            )
            .map_err(|source| EngineError::ViewportFocus { input, source })?;
        if let FocusObservationTransition::Applied(applied) = &mut transition
            && let Some(intent) = applied.pane_intent()
            && let Some(reason) = self.reveal_pane_focus_intent(input, intent, events)?
        {
            applied.reject_pane_reveal(intent, reason);
        }
        if let FocusObservationTransition::Applied(applied) = &mut transition {
            if let Some(observed) = applied.observed_effect()
                && !self.settle_observed_focus_effect(observed)
            {
                applied.discard_observed_effect(observed.effect());
            }
            if let Some(observed) = self.settle_acknowledged_focus_effect(observation) {
                applied.record_acknowledged_effect_settlement(observed);
            }
        }
        Ok(transition)
    }

    fn settle_acknowledged_focus_effect(
        &mut self,
        observation: crate::viewport_focus::FocusObservationEnvelope,
    ) -> Option<ObservedPlatformFocusEffect> {
        let Authority::Known(Some(effect)) = *observation.acknowledged_effect() else {
            return None;
        };
        let binding = self.focus_effect_binding(effect)?;
        let observed = ObservedPlatformFocusEffect::new(
            effect,
            binding,
            observation.generation(),
            PlatformFocusEvidence::ExactEffectAcknowledgement,
        );
        self.settle_observed_focus_effect(observed)
            .then_some(observed)
    }

    fn settle_observed_focus_effect(&mut self, observed: ObservedPlatformFocusEffect) -> bool {
        let Some(binding) = self.focus_effect_binding(observed.effect()) else {
            return false;
        };
        if binding != observed.binding()
            || self
                .viewport
                .viewport(binding.surface())
                .filter(|record| record.is_focusable())
                .map(crate::viewport_registry::ViewportRecord::binding)
                != Some(binding)
        {
            return false;
        }
        self.viewport
            .observe_focus_effect(observed.effect(), binding)
            == EffectTransition::Applied
    }

    fn focus_effect_binding(
        &self,
        effect: crate::effect::EffectId,
    ) -> Option<crate::viewport::ViewportBinding> {
        let record = self.viewport.effects().record(effect)?;
        match record.request().effect() {
            crate::effect::PlatformEffect::RequestFocus { binding, .. } => Some(*binding),
            _ => None,
        }
    }

    fn platform_focus_restore_gate(snapshot: &PlatformSnapshot) -> PlatformFocusRestoreGate {
        let authoritative_mouse_down = snapshot
            .capabilities()
            .authoritative_button_state()
            .is_supported()
            && snapshot.pointers().iter().any(|pointer| {
                matches!(
                    pointer.button_states(),
                    Authority::Known(states)
                        if states
                            .iter()
                            .any(|state| state.state() == PointerButtonState::Pressed)
                )
            });
        if authoritative_mouse_down {
            PlatformFocusRestoreGate::AuthoritativeMouseDown
        } else {
            PlatformFocusRestoreGate::NoAuthoritativeMouseDown
        }
    }

    fn reduce_viewport_activation(
        &mut self,
        input: InputSequence,
        expected: WorkspaceVersion,
        request: ViewportActivationRequest,
        focus_generation: PaneFocusIntentGeneration,
        application_base: WorkspaceVersion,
        events: &mut Vec<WorkspaceEvent>,
    ) -> Result<InputOutcome, EngineError> {
        if expected != application_base {
            return Ok(InputOutcome::StaleRejected {
                expected,
                accepted_base: application_base,
            });
        }
        if request.cause() != crate::viewport_focus::ViewportActivationCause::Explicit {
            return Ok(InputOutcome::ViewportActivationRejected { request });
        }
        let activation =
            self.start_viewport_activation(input, request, focus_generation, events)?;
        Ok(InputOutcome::ViewportActivationRequested { activation })
    }

    fn start_viewport_activation(
        &mut self,
        input: InputSequence,
        request: ViewportActivationRequest,
        focus_generation: PaneFocusIntentGeneration,
        events: &mut Vec<WorkspaceEvent>,
    ) -> Result<ActivationStart, EngineError> {
        self.start_viewport_activation_with_reveal(
            input,
            request,
            focus_generation,
            PaneRevealDisposition::Pending,
            events,
        )
    }

    fn start_viewport_activation_with_reveal(
        &mut self,
        input: InputSequence,
        request: ViewportActivationRequest,
        focus_generation: PaneFocusIntentGeneration,
        reveal: PaneRevealDisposition,
        events: &mut Vec<WorkspaceEvent>,
    ) -> Result<ActivationStart, EngineError> {
        let (bindings, items) = self.focus_validation_snapshot();
        let control_supported = self
            .viewport
            .capabilities()
            .window_activation_control()
            .is_supported();
        let mut activation = self
            .viewport_focus
            .request_activation(
                request,
                focus_generation,
                control_supported,
                |binding| bindings.contains(&binding),
                |surface, item| {
                    items
                        .get(&surface)
                        .is_some_and(|surface_items| surface_items.contains(&item))
                },
            )
            .map_err(|source| EngineError::ViewportFocus { input, source })?;
        if let ActivationStartOutcome::RequestPlatformFocus { target } = activation.outcome() {
            let effect = self
                .viewport
                .request_focus_binding(target)
                .map_err(|source| EngineError::Viewport { input, source })?;
            if self
                .viewport_focus
                .attach_platform_focus_effect(activation.generation(), effect)
                != crate::viewport_focus::FocusEffectAttachment::Applied
            {
                return Err(EngineError::ViewportFocus {
                    input,
                    source: ViewportFocusError::EffectAttachmentInvariant,
                });
            }
        }
        if reveal == PaneRevealDisposition::Pending
            && let ActivationStartOutcome::PaneFocusReady { intent } = activation.outcome()
            && let Some(reason) = self.reveal_pane_focus_intent(input, intent, events)?
        {
            activation = activation.reject_pane_reveal(intent, reason);
        }
        Ok(activation)
    }

    fn reveal_pane_focus_intent(
        &mut self,
        input: InputSequence,
        intent: PaneFocusIntent,
        events: &mut Vec<WorkspaceEvent>,
    ) -> Result<Option<PaneFocusRevealRejection>, EngineError> {
        let PanelFocus::Item(item) = intent.focus() else {
            return Ok(None);
        };
        let Some(source) =
            Self::capture_surface_item_source(&self.workspace, intent.target().surface(), item)
        else {
            return self.reject_pane_focus_reveal(
                input,
                intent,
                PaneFocusRevealRejection::ItemUnavailable { item },
            );
        };
        let application =
            self.apply_interaction_command(input, &WorkspaceCommand::Select { source }, events)?;
        match application {
            CommandApplication::Applied { .. } => Ok(None),
            CommandApplication::Rejected(error) => {
                let reason = match error {
                    crate::error::CommandError::SurfaceLifecycleFrozen { surface } => {
                        PaneFocusRevealRejection::SurfaceLifecycleFrozen { surface }
                    }
                    crate::error::CommandError::Policy(_) => {
                        PaneFocusRevealRejection::PolicyRejected
                    }
                    crate::error::CommandError::MissingRoot { .. }
                    | crate::error::CommandError::MissingNode { .. }
                    | crate::error::CommandError::NodeOutsideRoot { .. }
                    | crate::error::CommandError::StaleNode { .. }
                    | crate::error::CommandError::NodeIsNotTabs { .. }
                    | crate::error::CommandError::ItemNotInTabs { .. } => {
                        PaneFocusRevealRejection::ItemUnavailable { item }
                    }
                    _ => PaneFocusRevealRejection::WorkspaceRejected,
                };
                self.reject_pane_focus_reveal(input, intent, reason)
            }
        }
    }

    fn reject_pane_focus_reveal(
        &mut self,
        input: InputSequence,
        intent: PaneFocusIntent,
        reason: PaneFocusRevealRejection,
    ) -> Result<Option<PaneFocusRevealRejection>, EngineError> {
        if !self.viewport_focus.reject_pane_reveal(intent.id()) {
            return Err(EngineError::ViewportFocus {
                input,
                source: ViewportFocusError::PaneRevealInvariant,
            });
        }
        Ok(Some(reason))
    }

    fn capture_surface_item_source(
        workspace: &Workspace,
        surface: crate::ids::SurfaceId,
        item: crate::ids::ItemId,
    ) -> Option<crate::command::ItemSource> {
        let presentation = workspace.surface(surface)?;
        let mut roots = Vec::with_capacity(presentation.contained.len().saturating_add(1));
        roots.push(presentation.main_root);
        roots.extend(presentation.contained.iter().filter_map(|floating| {
            workspace
                .contained_floating(*floating)
                .map(|record| record.root)
        }));
        for root in roots {
            for (tabs, node) in workspace.nodes() {
                if !matches!(node, Node::Tabs { items, .. } if items.contains(&item)) {
                    continue;
                }
                if let Ok(source) = workspace.capture_item_source(root, tabs, item) {
                    return Some(source);
                }
            }
        }
        None
    }

    fn reduce_pane_focus_observation(
        &mut self,
        expected_epoch: crate::ids::WorkspaceEpoch,
        observation: PaneFocusObservation,
    ) -> InputOutcome {
        if expected_epoch != self.version.epoch() {
            return InputOutcome::PaneFocusObservationStale {
                expected_epoch,
                current_epoch: self.version.epoch(),
            };
        }
        let (bindings, items) = self.focus_validation_snapshot();
        let transition = self.viewport_focus.publish_pane_focus_observation(
            observation,
            |binding| bindings.contains(&binding),
            |surface, item| {
                items
                    .get(&surface)
                    .is_some_and(|surface_items| surface_items.contains(&item))
            },
        );
        InputOutcome::PaneFocusObservationPublished { transition }
    }

    fn focus_validation_snapshot(
        &self,
    ) -> (
        BTreeSet<crate::viewport::ViewportBinding>,
        BTreeMap<crate::ids::SurfaceId, BTreeSet<crate::ids::ItemId>>,
    ) {
        let bindings = self
            .viewport
            .registry()
            .records()
            .filter_map(|(_, record)| record.is_focusable().then_some(record.binding()))
            .collect();
        let items = self
            .workspace
            .surfaces()
            .map(|(surface, _)| (surface, self.surface_items(surface)))
            .collect();
        (bindings, items)
    }

    fn reconcile_viewport_focus_authority(&mut self) {
        let (bindings, items) = self.focus_validation_snapshot();
        let _ = self.viewport_focus.reconcile_authority(
            |surface| items.contains_key(&surface),
            |binding| bindings.contains(&binding),
            |surface, item| {
                items
                    .get(&surface)
                    .is_some_and(|surface_items| surface_items.contains(&item))
            },
        );
    }

    fn surface_items(&self, surface: crate::ids::SurfaceId) -> BTreeSet<crate::ids::ItemId> {
        let Some(presentation) = self.workspace.surface(surface) else {
            return BTreeSet::new();
        };
        std::iter::once(presentation.main_root)
            .chain(presentation.contained.iter().filter_map(|floating| {
                self.workspace
                    .contained_floating(*floating)
                    .map(|floating| floating.root)
            }))
            .filter_map(|root| self.workspace.root(root).map(|record| record.node))
            .flat_map(|node| self.workspace.collect_items_in_subtree(node))
            .collect()
    }

    fn reduce_platform_effect(
        &mut self,
        input: InputSequence,
        expected_epoch: crate::ids::WorkspaceEpoch,
        result: EffectResult,
    ) -> Result<InputOutcome, EngineError> {
        let is_focus_effect =
            self.viewport
                .effects()
                .record(result.effect())
                .is_some_and(|record| {
                    matches!(
                        record.request().effect(),
                        crate::effect::PlatformEffect::RequestFocus { .. }
                    )
                });
        let transition = if expected_epoch == self.version.epoch() {
            self.viewport
                .report_effect(expected_epoch, result)
                .map_err(|source| EngineError::Viewport { input, source })?
        } else {
            EffectTransition::StaleEpoch
        };
        let focus = (transition == EffectTransition::Applied && is_focus_effect).then(|| {
            self.viewport_focus
                .report_platform_focus_effect(result.effect(), result.result())
        });
        Ok(InputOutcome::PlatformEffectReported {
            effect: result.effect(),
            transition,
            focus,
        })
    }

    fn reduce_viewport_close_decision(
        &mut self,
        input: InputSequence,
        expected: WorkspaceVersion,
        request: ViewportCloseRequestId,
        decision: ViewportCloseDecision,
    ) -> Result<InputOutcome, EngineError> {
        if expected != self.version {
            return Ok(InputOutcome::StaleRejected {
                expected,
                accepted_base: self.version,
            });
        }
        let mut accepted_merge = None;
        if let ViewportCloseDecision::Accept(plan) = &decision {
            let capability = self.viewport.capabilities().authoritative_inventory();
            if !capability.is_supported() {
                return self.reject_viewport_close_decision(
                    input,
                    request,
                    ViewportCloseDecisionRejection::DestructionAuthorityUnavailable { capability },
                );
            }
            if let (ViewportClosePlan::MergeBack(merge), Some(close_request)) =
                (plan, self.viewport.viewport_close_request(request))
            {
                let focus = self
                    .surface_recovery
                    .close_focus(request)
                    .unwrap_or(PanelFocus::None);
                match self.freeze_accepted_merge_back(
                    input,
                    close_request.binding().surface(),
                    close_request.recovery(),
                    merge,
                    focus,
                )? {
                    MergeBackFreeze::Frozen {
                        roster,
                        dependency,
                        focus,
                    } => {
                        accepted_merge = Some((*roster, dependency, focus));
                    }
                    MergeBackFreeze::Rejected(reason) => {
                        return self.reject_viewport_close_decision(input, request, reason);
                    }
                }
            }
        }
        let effect = self
            .viewport
            .decide_viewport_close(request, decision)
            .map_err(|source| EngineError::Viewport { input, source })?;
        if let Some((roster, dependency, focus)) = accepted_merge {
            self.surface_recovery
                .freeze_accepted_close(request, roster, dependency, focus);
            self.surface_recovery.remove_close_focus(request);
        } else {
            self.surface_recovery.remove_accepted_close(request);
            self.surface_recovery.remove_close_focus(request);
            let _ = self.viewport_focus.clear_close_request(request);
        }
        Ok(InputOutcome::ViewportCloseDecided { request, effect })
    }

    fn freeze_accepted_merge_back(
        &self,
        input: InputSequence,
        source_surface: crate::ids::SurfaceId,
        source_recovery: Option<crate::intent::ContainedRecoveryPlan>,
        plan: &crate::frame::ViewportMergeBackPlan,
        focus: PanelFocus,
    ) -> Result<MergeBackFreeze, EngineError> {
        let target_surface = plan.target_surface();
        let target_is_closing = self
            .viewport
            .viewport(target_surface)
            .is_some_and(|record| {
                matches!(
                    record.lifecycle(),
                    crate::viewport_registry::ViewportLifecycle::CloseRequested
                        | crate::viewport_registry::ViewportLifecycle::AwaitingDestroyed
                )
            });
        if target_surface == source_surface || target_is_closing {
            return Ok(MergeBackFreeze::Rejected(
                ViewportCloseDecisionRejection::MergeBackTargetsClosingSurface {
                    surface: target_surface,
                },
            ));
        }
        if let Some(recovery) = source_recovery
            && recovery.surface() != target_surface
        {
            return Ok(MergeBackFreeze::Rejected(
                ViewportCloseDecisionRejection::MergeBackRecoveryTargetMismatch {
                    expected: recovery.surface(),
                    actual: target_surface,
                },
            ));
        }
        let roster = self
            .capture_surface_roster(source_surface)
            .map_err(|source| EngineError::SurfaceRoster { input, source })?;
        let focus = match focus {
            PanelFocus::Item(item) if roster.contains_item(&self.workspace, item) => {
                PanelFocus::Item(item)
            }
            PanelFocus::Item(_) | PanelFocus::None => PanelFocus::None,
        };
        if !roster.contained().is_empty() && roster.source_coordinates().is_none() {
            return Ok(MergeBackFreeze::Rejected(
                ViewportCloseDecisionRejection::SourceGeometryUnavailable,
            ));
        }
        let main_root = roster.main_root();
        let source_main = self
            .workspace
            .root(main_root)
            .and_then(|root| self.workspace.node(root.node));
        if !matches!(source_main, Some(Node::Tabs { .. })) {
            return Ok(MergeBackFreeze::Rejected(
                ViewportCloseDecisionRejection::MergeBackSourceNotTabs { root: main_root },
            ));
        }
        let target_is_current = self.workspace.presentation_for_root(plan.target().root())
            == Some(crate::RootPresentationOwner::Main {
                surface: target_surface,
            })
            && self
                .workspace
                .capture_tab_target(plan.target().root(), plan.target().tabs())
                .is_ok_and(|target| &target == plan.target());
        let Some(target_facts) = target_is_current
            .then(|| self.freeze_surface_recovery_target(target_surface))
            .flatten()
        else {
            return Ok(MergeBackFreeze::Rejected(
                ViewportCloseDecisionRejection::MergeBackTargetUnavailable {
                    surface: target_surface,
                },
            ));
        };
        Ok(MergeBackFreeze::Frozen {
            roster: Box::new(roster),
            dependency: target_facts.dependency(),
            focus,
        })
    }

    fn reject_viewport_close_decision(
        &mut self,
        input: InputSequence,
        request: ViewportCloseRequestId,
        reason: ViewportCloseDecisionRejection,
    ) -> Result<InputOutcome, EngineError> {
        let hold_effect = self
            .viewport
            .decide_viewport_close(request, ViewportCloseDecision::Prevent)
            .map_err(|source| EngineError::Viewport { input, source })?;
        self.surface_recovery.remove_accepted_close(request);
        self.surface_recovery.remove_close_focus(request);
        let _ = self.viewport_focus.clear_close_request(request);
        Ok(InputOutcome::ViewportCloseDecisionRejected {
            request,
            reason,
            hold_effect,
        })
    }

    fn reduce_native_create_cancellation(
        &mut self,
        input: InputSequence,
        expected: WorkspaceVersion,
        saga: NativeCreateSagaId,
    ) -> Result<InputOutcome, EngineError> {
        if expected != self.version {
            return Ok(InputOutcome::StaleRejected {
                expected,
                accepted_base: self.version,
            });
        }
        let compensation = self
            .viewport
            .cancel_native_create(saga)
            .map_err(|source| EngineError::Viewport { input, source })?;
        Ok(InputOutcome::NativeCreateCancelled { saga, compensation })
    }

    fn reduce_viewport_cleanup_retry(
        &mut self,
        input: InputSequence,
        expected: WorkspaceVersion,
        failed_effect: crate::effect::EffectId,
    ) -> Result<InputOutcome, EngineError> {
        if expected != self.version {
            return Ok(InputOutcome::StaleRejected {
                expected,
                accepted_base: self.version,
            });
        }
        let retry = self
            .viewport
            .retry_cleanup(failed_effect)
            .map_err(|source| EngineError::Viewport { input, source })?;
        Ok(InputOutcome::ViewportCleanupRetried {
            failed_effect,
            retry,
        })
    }

    // Lifecycle actions share one batch-frozen roster/scene barrier and must remain visibly
    // ordered in a single reduction loop.
    #[allow(clippy::too_many_lines)]
    fn reduce_viewport_actions(
        &mut self,
        input: InputSequence,
        focus_generation: PaneFocusIntentGeneration,
        actions: &[crate::frame::ViewportLifecycleAction],
        recovery_batch: &SurfaceRecoveryBatchContext,
        activations: &mut Vec<ActivationStart>,
        events: &mut Vec<WorkspaceEvent>,
    ) -> Result<(), EngineError> {
        let mut active_barrier = recovery_batch.active_rosters.clone();
        for action in actions {
            match action {
                crate::frame::ViewportLifecycleAction::CreateReady { saga, prepared } => match self
                    .apply_interaction_command_with_barrier(
                        input,
                        prepared.command(),
                        Some(&active_barrier),
                        events,
                    )? {
                    CommandApplication::Applied { .. } => {
                        self.viewport
                            .complete_native_create(*saga)
                            .map_err(|source| EngineError::Viewport { input, source })?;
                        if let Some(binding) = self
                            .viewport
                            .native_create_saga(*saga)
                            .filter(|saga| {
                                matches!(
                                    saga.status(),
                                    crate::frame::NativeCreateStatus::Committed { .. }
                                )
                            })
                            .map(crate::frame::NativeCreateSaga::binding)
                        {
                            activations.push(self.start_viewport_activation(
                                input,
                                ViewportActivationRequest::tear_off_committed(
                                    binding,
                                    prepared.focus(),
                                ),
                                focus_generation,
                                events,
                            )?);
                        }
                    }
                    CommandApplication::Rejected(_) => {
                        self.viewport
                            .reject_native_create(*saga)
                            .map_err(|source| EngineError::Viewport { input, source })?;
                    }
                },
                crate::frame::ViewportLifecycleAction::CreateVisible { saga, binding } => {
                    let focus = self
                        .viewport
                        .native_create_saga(*saga)
                        .ok_or(EngineError::Viewport {
                            input,
                            source: ViewportCoordinatorError::MissingCreateSaga { saga: *saga },
                        })?
                        .prepared()
                        .focus();
                    activations.push(self.start_viewport_activation(
                        input,
                        ViewportActivationRequest::tear_off_committed(*binding, focus),
                        focus_generation,
                        events,
                    )?);
                }
                crate::frame::ViewportLifecycleAction::SurfaceDestroyed {
                    binding,
                    resolution,
                } => {
                    let mut context = DestroyedSurfaceContext {
                        roster: active_barrier.get(&binding.surface()),
                        action_barrier: &active_barrier,
                        recovery_targets: &recovery_batch.targets,
                        events,
                    };
                    self.reduce_destroyed_surface(
                        input,
                        focus_generation,
                        *binding,
                        resolution,
                        activations,
                        &mut context,
                    )?;
                    active_barrier.remove(&binding.surface());
                }
                crate::frame::ViewportLifecycleAction::RetryRecovery {
                    destroyed_binding,
                    recovery,
                } => {
                    let surface = destroyed_binding.surface();
                    let roster = self
                        .surface_recovery
                        .pending(surface)
                        .cloned()
                        .ok_or(EngineError::MissingSurfaceRoster { input, surface })?;
                    let disposition = self
                        .surface_recovery
                        .pending_disposition(surface)
                        .cloned()
                        .ok_or(EngineError::MissingSurfaceRoster { input, surface })?;
                    let resolved = match &disposition {
                        PendingSurfaceRecoveryDisposition::Contained => self
                            .apply_destroyed_surface_recovery(
                                input,
                                &roster,
                                *recovery,
                                SurfaceRecoveryTargetContext::new(
                                    &active_barrier,
                                    recovery_batch.targets.get(&recovery.surface()),
                                ),
                                events,
                            )?,
                        PendingSurfaceRecoveryDisposition::MergeBack {
                            request,
                            plan,
                            dependency,
                            focus,
                        } => self.apply_merge_back(
                            input,
                            focus_generation,
                            &roster,
                            MergeBackIntent {
                                request: *request,
                                plan,
                                dependency: *dependency,
                                focus: *focus,
                            },
                            MergeBackApplicationContext {
                                target: SurfaceRecoveryTargetContext::new(
                                    &active_barrier,
                                    recovery_batch.targets.get(&plan.target_surface()),
                                ),
                                activations,
                                events,
                            },
                        )?,
                    };
                    if resolved {
                        self.viewport
                            .complete_pending_recovery(surface)
                            .map_err(|source| EngineError::Viewport { input, source })?;
                        self.surface_recovery.complete_pending(surface);
                    }
                }
            }
        }
        Ok(())
    }

    fn freeze_surface_recovery_batch(
        &self,
        input: InputSequence,
        actions: &[crate::frame::ViewportLifecycleAction],
    ) -> Result<SurfaceRecoveryBatchContext, EngineError> {
        let active_rosters = self.freeze_destroyed_surface_rosters(input, actions)?;
        let mut targets = BTreeMap::new();
        for action in actions {
            let target_surface = match action {
                crate::frame::ViewportLifecycleAction::CreateReady { .. }
                | crate::frame::ViewportLifecycleAction::CreateVisible { .. } => None,
                crate::frame::ViewportLifecycleAction::SurfaceDestroyed { resolution, .. } => {
                    match resolution {
                        crate::frame::ViewportDestructionResolution::Accepted { plan, .. } => {
                            match plan {
                                ViewportClosePlan::RetainLayout => None,
                                ViewportClosePlan::MergeBack(plan) => Some(plan.target_surface()),
                            }
                        }
                        crate::frame::ViewportDestructionResolution::Recover { recovery } => {
                            Some(recovery.surface())
                        }
                        crate::frame::ViewportDestructionResolution::Unplanned => None,
                    }
                }
                crate::frame::ViewportLifecycleAction::RetryRecovery {
                    destroyed_binding,
                    recovery,
                } => match self
                    .surface_recovery
                    .pending_disposition(destroyed_binding.surface())
                {
                    Some(PendingSurfaceRecoveryDisposition::Contained) => Some(recovery.surface()),
                    Some(PendingSurfaceRecoveryDisposition::MergeBack { plan, .. }) => {
                        Some(plan.target_surface())
                    }
                    None => None,
                },
            };
            let Some(target_surface) = target_surface else {
                continue;
            };
            if targets.contains_key(&target_surface) {
                continue;
            }
            if let Some(facts) = self.freeze_surface_recovery_target(target_surface) {
                targets.insert(target_surface, facts);
            }
        }
        Ok(SurfaceRecoveryBatchContext {
            active_rosters,
            targets,
        })
    }

    fn freeze_surface_recovery_target(
        &self,
        surface: crate::ids::SurfaceId,
    ) -> Option<SurfaceRecoveryTargetFacts> {
        let coordinates = self
            .viewport
            .viewport(surface)
            .filter(|record| record.is_ready())?
            .coordinates()?;
        let (scene, bounds) = self.current_ready_surface_bounds(surface).ok()?;
        if !self
            .scene_coordinate_authority
            .get(&surface)
            .is_some_and(|authority| authority.matches(scene, coordinates))
        {
            return None;
        }
        Some(SurfaceRecoveryTargetFacts::new(
            surface,
            scene,
            coordinates,
            bounds,
        ))
    }

    fn invalidate_changed_surface_scene_authority(&mut self) {
        let viewport = &self.viewport;
        self.scene_coordinate_authority
            .retain(|surface, authority| {
                viewport
                    .viewport(*surface)
                    .and_then(crate::viewport_registry::ViewportRecord::coordinates)
                    .is_some_and(|current| {
                        SurfaceSceneCoordinateAuthority::coordinates_match(
                            authority.coordinates,
                            current,
                        )
                    })
            });
    }

    fn freeze_destroyed_surface_rosters(
        &self,
        input: InputSequence,
        actions: &[crate::frame::ViewportLifecycleAction],
    ) -> Result<BTreeMap<crate::ids::SurfaceId, SurfaceRosterDisposition>, EngineError> {
        let mut rosters = BTreeMap::new();
        for action in actions {
            let crate::frame::ViewportLifecycleAction::SurfaceDestroyed {
                binding,
                resolution,
            } = action
            else {
                continue;
            };
            let surface = binding.surface();
            if self.workspace.surface(surface).is_none() {
                continue;
            }
            let roster = match resolution {
                crate::frame::ViewportDestructionResolution::Accepted {
                    request,
                    plan: ViewportClosePlan::MergeBack(_),
                    ..
                } => self
                    .surface_recovery
                    .accepted_close(*request)
                    .map(|accepted| accepted.roster().clone())
                    .ok_or(EngineError::MissingSurfaceRoster { input, surface })?,
                crate::frame::ViewportDestructionResolution::Accepted {
                    plan: ViewportClosePlan::RetainLayout,
                    ..
                }
                | crate::frame::ViewportDestructionResolution::Unplanned => continue,
                crate::frame::ViewportDestructionResolution::Recover { .. } => self
                    .capture_surface_roster(surface)
                    .map_err(|source| EngineError::SurfaceRoster { input, source })?,
            };
            rosters.insert(surface, roster);
        }
        Ok(rosters)
    }

    fn capture_surface_roster(
        &self,
        surface: crate::ids::SurfaceId,
    ) -> Result<SurfaceRosterDisposition, SurfaceRosterCaptureError> {
        let source_coordinates = self
            .viewport
            .viewport(surface)
            .and_then(crate::viewport_registry::ViewportRecord::coordinates);
        SurfaceRosterDisposition::capture(&self.workspace, surface, source_coordinates)
    }

    // Destruction, merge-back, and deferred recovery form one atomic state transition; splitting
    // the branches would obscure which paths may complete or retain the logical surface.
    #[allow(clippy::too_many_lines)]
    fn reduce_destroyed_surface(
        &mut self,
        input: InputSequence,
        focus_generation: PaneFocusIntentGeneration,
        binding: crate::viewport::ViewportBinding,
        resolution: &crate::frame::ViewportDestructionResolution,
        activations: &mut Vec<ActivationStart>,
        context: &mut DestroyedSurfaceContext<'_>,
    ) -> Result<(), EngineError> {
        let surface = binding.surface();
        if self.workspace.surface(surface).is_none() {
            self.complete_destroyed_surface(input, binding)?;
            if let crate::frame::ViewportDestructionResolution::Accepted { request, .. } =
                resolution
            {
                self.surface_recovery.remove_accepted_close(*request);
            }
            self.surface_recovery.complete_pending(surface);
            return Ok(());
        }

        if matches!(
            resolution,
            crate::frame::ViewportDestructionResolution::Unplanned
        ) {
            self.complete_destroyed_surface(input, binding)?;
            return Ok(());
        }

        if let crate::frame::ViewportDestructionResolution::Accepted {
            request,
            plan: ViewportClosePlan::RetainLayout,
            ..
        } = resolution
        {
            self.complete_destroyed_surface(input, binding)?;
            self.surface_recovery.remove_accepted_close(*request);
            return Ok(());
        }

        let roster = context
            .roster
            .ok_or(EngineError::MissingSurfaceRoster { input, surface })?;
        let (resolved, pending) = match resolution {
            crate::frame::ViewportDestructionResolution::Accepted {
                request,
                plan: ViewportClosePlan::MergeBack(plan),
                recovery,
            } => {
                let dependency = self
                    .surface_recovery
                    .accepted_close(*request)
                    .map(crate::surface_recovery::AcceptedSurfaceMergeBack::dependency)
                    .ok_or(EngineError::MissingSurfaceRoster { input, surface })?;
                let focus = self
                    .surface_recovery
                    .accepted_close(*request)
                    .map(crate::surface_recovery::AcceptedSurfaceMergeBack::focus)
                    .ok_or(EngineError::MissingSurfaceRoster { input, surface })?;
                let resolved = self.apply_merge_back(
                    input,
                    focus_generation,
                    roster,
                    MergeBackIntent {
                        request: *request,
                        plan,
                        dependency,
                        focus,
                    },
                    MergeBackApplicationContext {
                        target: SurfaceRecoveryTargetContext::new(
                            context.action_barrier,
                            context.recovery_targets.get(&plan.target_surface()),
                        ),
                        activations,
                        events: context.events,
                    },
                )?;
                let pending = recovery.map(|recovery| {
                    (
                        recovery,
                        PendingSurfaceRecoveryDisposition::MergeBack {
                            request: *request,
                            plan: plan.clone(),
                            dependency,
                            focus,
                        },
                    )
                });
                (resolved, pending)
            }
            crate::frame::ViewportDestructionResolution::Recover { recovery } => (
                self.apply_destroyed_surface_recovery(
                    input,
                    roster,
                    *recovery,
                    SurfaceRecoveryTargetContext::new(
                        context.action_barrier,
                        context.recovery_targets.get(&recovery.surface()),
                    ),
                    context.events,
                )?,
                Some((*recovery, PendingSurfaceRecoveryDisposition::Contained)),
            ),
            crate::frame::ViewportDestructionResolution::Accepted {
                plan: ViewportClosePlan::RetainLayout,
                ..
            }
            | crate::frame::ViewportDestructionResolution::Unplanned => unreachable!(),
        };
        if resolved && self.workspace.surface(surface).is_none() {
            self.complete_destroyed_surface(input, binding)?;
            self.surface_recovery.complete_pending(surface);
        } else if !resolved && let Some((recovery, disposition)) = pending {
            self.viewport
                .defer_destroyed_surface_recovery(binding, recovery)
                .map_err(|source| EngineError::Viewport { input, source })?;
            if !self.surface_recovery.defer(roster.clone(), disposition) {
                return Err(EngineError::ConflictingSurfaceRecovery { input, surface });
            }
        } else if !resolved {
            self.complete_destroyed_surface(input, binding)?;
        }
        if let crate::frame::ViewportDestructionResolution::Accepted { request, .. } = resolution {
            self.surface_recovery.remove_accepted_close(*request);
        }
        Ok(())
    }

    fn complete_destroyed_surface(
        &mut self,
        input: InputSequence,
        binding: crate::viewport::ViewportBinding,
    ) -> Result<(), EngineError> {
        self.viewport
            .complete_destroyed_surface(binding)
            .map_err(|source| EngineError::Viewport { input, source })?;
        let _ = self.viewport_focus.clear_binding(binding);
        if self.workspace.surface(binding.surface()).is_none() {
            let _ = self.viewport_focus.clear_surface(binding.surface());
        }
        Ok(())
    }

    fn apply_merge_back(
        &mut self,
        input: InputSequence,
        focus_generation: PaneFocusIntentGeneration,
        roster: &SurfaceRosterDisposition,
        intent: MergeBackIntent<'_>,
        context: MergeBackApplicationContext<'_, '_>,
    ) -> Result<bool, EngineError> {
        let MergeBackApplicationContext {
            target,
            activations,
            events,
        } = context;
        let Some(target_facts) = target.target_facts else {
            return Ok(false);
        };
        if !target_facts.satisfies(intent.dependency) {
            return Ok(false);
        }
        let Some(placement) = self.surface_forest_placement(roster, intent.plan, target_facts)
        else {
            return Ok(false);
        };
        let Some(transaction) =
            roster.compile_merge_back_transaction(&self.workspace, &placement, intent.plan)
        else {
            return Ok(false);
        };
        let target_binding = target_facts.coordinates().binding();
        let target_is_focused =
            self.viewport_focus
                .focus_observation()
                .is_some_and(|observation| {
                    matches!(
                        observation.focused(),
                        Authority::Known(crate::viewport_focus::GlobalFocusedWindow::Dock(focused))
                            if *focused == target_binding
                    )
                });
        let selection = match (target_is_focused, intent.focus) {
            (true, PanelFocus::Item(item)) => Some(CandidatePaneSelection {
                surface: intent.plan.target_surface(),
                item,
            }),
            (true, PanelFocus::None) | (false, _) => None,
        };
        let applied = self.apply_surface_roster_transaction(
            input,
            roster,
            &transaction,
            selection,
            target.action_barrier,
            events,
        )?;
        if applied {
            let reveal = selection.map_or(PaneRevealDisposition::Pending, |_| {
                PaneRevealDisposition::AppliedInCandidate
            });
            let activation = self.start_viewport_activation_with_reveal(
                input,
                ViewportActivationRequest::close_recovery(
                    target_binding,
                    intent.focus,
                    intent.request,
                ),
                focus_generation,
                reveal,
                events,
            )?;
            activations.push(activation);
        }
        Ok(applied)
    }

    fn apply_destroyed_surface_recovery(
        &mut self,
        input: InputSequence,
        roster: &SurfaceRosterDisposition,
        recovery: crate::intent::ContainedRecoveryPlan,
        context: SurfaceRecoveryTargetContext<'_>,
        events: &mut Vec<WorkspaceEvent>,
    ) -> Result<bool, EngineError> {
        let Some(target_facts) = context.target_facts else {
            return Ok(false);
        };
        let Some(placement) = self.surface_roster_placement(roster, recovery, target_facts) else {
            return Ok(false);
        };
        let target = RootPresentationTarget::Contained {
            surface: placement.target_surface(),
            floating: recovery.floating(),
            rect: placement.main_rect(),
            z_order: placement.main_z_order(),
        };
        let Some(transaction) =
            roster.compile_recovery_transaction(&self.workspace, &placement, target)
        else {
            return Ok(false);
        };
        self.apply_surface_roster_transaction(
            input,
            roster,
            &transaction,
            None,
            context.action_barrier,
            events,
        )
    }

    fn surface_roster_placement(
        &self,
        roster: &SurfaceRosterDisposition,
        recovery: crate::intent::ContainedRecoveryPlan,
        target_facts: &SurfaceRecoveryTargetFacts,
    ) -> Option<SurfaceRosterPlacement> {
        if roster.main_root() != recovery.root() {
            return None;
        }
        let target_coordinates = target_facts.coordinates();
        let source_coordinates = roster.source_coordinates()?;
        let target_bounds = target_facts.scene_bounds();
        let main_desktop = source_coordinates
            .outer_bounds()
            .unwrap_or_else(|| source_coordinates.content_bounds());
        let main_requested = target_coordinates
            .desktop_rect_to_surface(main_desktop)
            .ok()?;
        let main_rect = clamp_contained_rect(
            recovery.surface(),
            target_bounds,
            main_requested,
            recovery.minimum_size(),
        )
        .ok()?;
        let target_presentation = self.workspace.surface(recovery.surface())?;
        let existing_maximum = target_presentation
            .contained
            .iter()
            .filter_map(|floating| self.workspace.contained_floating(*floating))
            .map(|floating| floating.z_order)
            .max();
        let main_z_order = match existing_maximum {
            Some(existing) => recovery.z_order().max(existing.checked_add(1)?),
            None => recovery.z_order(),
        };
        let first_sibling_z = if roster.contained().is_empty() {
            main_z_order
        } else {
            main_z_order.checked_add(1)?
        };
        let contained = Self::contained_roster_placements(
            roster,
            recovery.surface(),
            target_facts,
            first_sibling_z,
        )?;
        Some(SurfaceRosterPlacement::new(
            recovery.surface(),
            main_rect,
            main_z_order,
            contained,
        ))
    }

    fn surface_forest_placement(
        &self,
        roster: &SurfaceRosterDisposition,
        plan: &crate::frame::ViewportMergeBackPlan,
        target_facts: &SurfaceRecoveryTargetFacts,
    ) -> Option<SurfaceForestPlacement> {
        let target = self.workspace.surface(plan.target_surface())?;
        let first_z_order = match target
            .contained
            .iter()
            .filter_map(|floating| self.workspace.contained_floating(*floating))
            .map(|floating| floating.z_order)
            .max()
        {
            Some(maximum) => maximum.checked_add(1)?,
            None => 0,
        };
        let contained = Self::contained_roster_placements(
            roster,
            plan.target_surface(),
            target_facts,
            first_z_order,
        )?;
        Some(SurfaceForestPlacement::new(
            plan.target_surface(),
            contained,
        ))
    }

    fn contained_roster_placements(
        roster: &SurfaceRosterDisposition,
        target_surface: crate::ids::SurfaceId,
        target_facts: &SurfaceRecoveryTargetFacts,
        first_z_order: u64,
    ) -> Option<Vec<ContainedRootPlacement>> {
        if roster.contained().is_empty() {
            return Some(Vec::new());
        }
        let source_coordinates = roster.source_coordinates()?;
        let target_coordinates = target_facts.coordinates();
        let target_bounds = target_facts.scene_bounds();
        let last_offset = u64::try_from(roster.contained().len().checked_sub(1)?).ok()?;
        first_z_order.checked_add(last_offset)?;

        let mut stack_order: Vec<_> = (0..roster.contained().len()).collect();
        stack_order.sort_by_key(|index| {
            let sibling = &roster.contained()[*index];
            ContainedStackKey::new(sibling.z_order(), sibling.floating())
        });
        let mut remapped_z = vec![0; roster.contained().len()];
        for (offset, index) in stack_order.into_iter().enumerate() {
            remapped_z[index] = first_z_order.checked_add(u64::try_from(offset).ok()?)?;
        }

        let minimum = crate::geometry::LogicalSize::new(0.0, 0.0).ok()?;
        roster
            .contained()
            .iter()
            .enumerate()
            .map(|(index, sibling)| {
                let desktop = source_coordinates
                    .surface_rect_to_desktop(sibling.rect())
                    .ok()?;
                let target_local = target_coordinates.desktop_rect_to_surface(desktop).ok()?;
                let rect =
                    clamp_contained_rect(target_surface, target_bounds, target_local, minimum)
                        .ok()?;
                Some(ContainedRootPlacement::new(
                    sibling.floating(),
                    sibling.root(),
                    rect,
                    remapped_z[index],
                ))
            })
            .collect()
    }

    fn apply_surface_roster_transaction(
        &mut self,
        input: InputSequence,
        roster: &SurfaceRosterDisposition,
        transaction: &WorkspaceTransaction,
        selection: Option<CandidatePaneSelection>,
        action_barrier: &BTreeMap<crate::ids::SurfaceId, SurfaceRosterDisposition>,
        events: &mut Vec<WorkspaceEvent>,
    ) -> Result<bool, EngineError> {
        let mut candidate = self.workspace.clone();
        let report = match transaction.apply(&mut candidate, &self.policy) {
            Ok(report) => report,
            Err(TransactionError::Command { source, .. }) if source.is_expected_rejection() => {
                return Ok(false);
            }
            Err(source) => return Err(EngineError::Command { input, source }),
        };
        let mut changed = report.changed();
        let mut outcomes = report.into_outcomes();
        if let Some(selection) = selection {
            let Some(source) =
                Self::capture_surface_item_source(&candidate, selection.surface, selection.item)
            else {
                return Ok(false);
            };
            let selection_report =
                match WorkspaceTransaction::from_commands([WorkspaceCommand::Select { source }])
                    .apply(&mut candidate, &self.policy)
                {
                    Ok(report) => report,
                    Err(TransactionError::Command { source, .. })
                        if source.is_expected_rejection() =>
                    {
                        return Ok(false);
                    }
                    Err(source) => return Err(EngineError::Command { input, source }),
                };
            changed |= selection_report.changed();
            outcomes.extend(selection_report.into_outcomes());
        }
        if candidate.surface(roster.surface()).is_some()
            || self
                .first_workspace_publication_mismatch(
                    &candidate,
                    Some(action_barrier),
                    Some(roster.surface()),
                )
                .is_some()
        {
            return Ok(false);
        }

        self.workspace = candidate;
        self.reconcile_viewport_focus_authority();
        if changed {
            self.advance_revision(input)?;
            self.invalidate_scene();
            events.extend(outcomes.into_iter().map(|outcome| {
                WorkspaceEvent::new(
                    input,
                    self.version,
                    WorkspaceEventKind::CommandCommitted(outcome),
                )
            }));
        }
        Ok(true)
    }

    fn reduce_workspace_replacement(
        &mut self,
        input: InputSequence,
        workspace: &Workspace,
        application_base: &mut WorkspaceVersion,
        events: &mut Vec<WorkspaceEvent>,
        interaction_events: &mut Vec<InteractionEvent>,
    ) -> Result<InputOutcome, EngineError> {
        workspace
            .validate()
            .map_err(EngineError::InvalidWorkspace)?;
        let before = self.version;
        let epoch = before
            .epoch()
            .checked_next()
            .ok_or(EngineError::WorkspaceEpochExhausted { input })?;
        let desired_surfaces = workspace
            .surfaces()
            .map(|(surface, _)| surface)
            .collect::<std::collections::BTreeSet<_>>();
        let reconciliation = self
            .viewport
            .reconcile_workspace_epoch(epoch, &desired_surfaces)
            .map_err(|source| EngineError::Viewport { input, source })?;
        self.workspace = workspace.clone();
        self.surface_recovery.clear();
        self.viewport_focus.reconcile_workspace_replacement();
        self.version = WorkspaceVersion::new(epoch, WorkspaceRevision::default());
        *application_base = self.version;
        self.invalidate_transient(
            input,
            InteractionCancelReason::WorkspaceRestored,
            interaction_events,
        )?;
        events.push(WorkspaceEvent::new(
            input,
            self.version,
            WorkspaceEventKind::WorkspaceReplaced,
        ));
        Ok(InputOutcome::WorkspaceReplaced {
            before,
            after: self.version,
            reconciliation,
        })
    }

    #[allow(clippy::too_many_arguments)]
    fn reduce_policy_replacement(
        &mut self,
        input: InputSequence,
        expected: WorkspaceVersion,
        accepted_base: WorkspaceVersion,
        policy: &DockPolicy,
        events: &mut Vec<WorkspaceEvent>,
        interaction_events: &mut Vec<InteractionEvent>,
    ) -> Result<InputOutcome, EngineError> {
        if expected != accepted_base {
            return Ok(InputOutcome::StaleRejected {
                expected,
                accepted_base,
            });
        }
        let changed = self.policy != *policy;
        if changed {
            self.policy = policy.clone();
            self.advance_revision(input)?;
            self.invalidate_transient(
                input,
                InteractionCancelReason::PolicyChanged,
                interaction_events,
            )?;
            events.push(WorkspaceEvent::new(
                input,
                self.version,
                WorkspaceEventKind::PolicyReplaced,
            ));
        }
        Ok(InputOutcome::PolicyReplaced {
            changed,
            version: self.version,
        })
    }

    fn reduce_renderer_input(
        &mut self,
        input: InputSequence,
        expected: WorkspaceVersion,
        intent: &RendererIntent,
        events: &mut Vec<WorkspaceEvent>,
        interaction_events: &mut Vec<InteractionEvent>,
    ) -> Result<InputOutcome, EngineError> {
        if expected != self.version {
            return Ok(InputOutcome::StaleRejected {
                expected,
                accepted_base: self.version,
            });
        }
        let outcome = self.reduce_renderer_intent(input, intent, events, interaction_events)?;
        Ok(InputOutcome::InteractionProcessed {
            outcome,
            version: self.version,
        })
    }

    fn advance_revision(&mut self, input: InputSequence) -> Result<(), EngineError> {
        let revision = self
            .version
            .revision()
            .checked_next()
            .ok_or(EngineError::WorkspaceRevisionExhausted { input })?;
        self.version = WorkspaceVersion::new(self.version.epoch(), revision);
        Ok(())
    }

    fn invalidate_transient(
        &mut self,
        input: InputSequence,
        reason: InteractionCancelReason,
        interaction_events: &mut Vec<InteractionEvent>,
    ) -> Result<(), EngineError> {
        self.viewport
            .end_all_drag_routing()
            .map_err(|source| EngineError::Viewport { input, source })?;
        self.invalidate_scene();
        if let Some(status) = self.interaction.cancel_active() {
            interaction_events.push(InteractionEvent::new(
                input,
                self.version,
                InteractionEventKind::Cancelled { status, reason },
            ));
        }
        Ok(())
    }

    fn platform_interaction_dependencies(&self) -> PlatformInteractionDependencies {
        let mut dependencies = PlatformInteractionDependencies::default();
        match self.interaction.status() {
            InteractionStatus::Idle => {}
            InteractionStatus::Armed { session } => {
                if let Ok(drag) = self.interaction.armed_drag(session)
                    && let Some(surface) = self.payload_surface(&drag.payload)
                {
                    dependencies.surfaces.insert(surface);
                }
                if let Ok(drag) = self.interaction.armed_drag(session)
                    && let FrozenDragOrigin::Contained(origin) = drag.origin
                {
                    dependencies.surfaces.insert(origin.surface);
                }
            }
            InteractionStatus::Dragging { session } => {
                let Ok(drag) = self.interaction.active_drag(session) else {
                    return dependencies;
                };
                if let Some(surface) = self.payload_surface(&drag.payload) {
                    dependencies.surfaces.insert(surface);
                }
                if let Some(target) = &drag.target {
                    match target {
                        TargetAuthority::Local(local) => {
                            dependencies.surfaces.insert(local.observer());
                            if let Authority::Known(Some(target)) = local.target() {
                                dependencies.surfaces.insert(target.surface());
                            }
                        }
                        TargetAuthority::Routed(route) => {
                            dependencies.routed = true;
                            if let Authority::Known(Some(target)) = route.target() {
                                dependencies.surfaces.insert(target.surface());
                            }
                        }
                    }
                } else if self.viewport.drag_source(drag.pointer).is_some() {
                    dependencies.routed = true;
                }
                if let Some(Authority::Known(pointer)) = &drag.current_pointer {
                    dependencies.surfaces.insert(pointer.surface());
                }
                if let Some(offer) = drag.contained_offer {
                    dependencies.surfaces.insert(offer.anchor().surface());
                }
                if let Some(preview) = &drag.preview {
                    match preview.public().visual() {
                        PreviewVisual::Dock { surface, .. }
                        | PreviewVisual::Contained { surface, .. } => {
                            dependencies.surfaces.insert(*surface);
                        }
                        PreviewVisual::Native { .. } => dependencies.native = true,
                    }
                } else if matches!(drag.tear_off, Some(TearOffRequest::Native { .. }))
                    && self.viewport.native_tear_off_capability().is_supported()
                {
                    dependencies.native = true;
                }
            }
            InteractionStatus::Resizing { session } => {
                if let Ok(resize) = self.interaction.active_resize(session)
                    && let Some(surface) = self.root_surface(resize.split.root())
                {
                    dependencies.surfaces.insert(surface);
                }
            }
            InteractionStatus::ContainedTransforming { session } => {
                if let Ok(transform) = self.interaction.active_contained_transform(session) {
                    dependencies.surfaces.insert(transform.surface);
                }
            }
        }
        dependencies
    }

    #[allow(clippy::too_many_arguments)]
    fn platform_interaction_cancel_reason(
        &self,
        dependencies: &PlatformInteractionDependencies,
        version_before_actions: WorkspaceVersion,
        transition: &crate::frame::ViewportFrameTransition,
        previous_native: PlatformCapability,
        previous_routing: PlatformCapability,
        previous_release: PlatformCapability,
    ) -> Option<InteractionCancelReason> {
        if self.version != version_before_actions {
            return Some(InteractionCancelReason::WorkspaceChanged);
        }
        for event in transition.registry_events() {
            let (binding, destroyed) = match event {
                crate::viewport_registry::RegistryEvent::Destroyed { binding } => (*binding, true),
                crate::viewport_registry::RegistryEvent::FactsUnavailable { binding } => {
                    (*binding, false)
                }
                crate::viewport_registry::RegistryEvent::Ready { .. }
                | crate::viewport_registry::RegistryEvent::CloseRequested { .. }
                | crate::viewport_registry::RegistryEvent::CloseRequestCleared { .. } => continue,
            };
            if !dependencies.surfaces.contains(&binding.surface()) {
                continue;
            }
            if destroyed {
                return Some(InteractionCancelReason::SurfaceClosed);
            }
            if dependencies.native {
                return Some(InteractionCancelReason::NativePlacementUnavailable);
            }
            if dependencies.routed {
                return Some(InteractionCancelReason::UnknownTargetAuthority);
            }
        }
        let current_native = self.viewport.native_tear_off_capability();
        if dependencies.native && previous_native.is_supported() && !current_native.is_supported() {
            return Some(match current_native {
                PlatformCapability::Unknown(_) => InteractionCancelReason::NativeCapabilityUnknown,
                PlatformCapability::Unsupported(_) => {
                    InteractionCancelReason::NativeCapabilityUnavailable
                }
                PlatformCapability::Supported => return None,
            });
        }
        if dependencies.native && transition.work_areas_changed() {
            return Some(InteractionCancelReason::NativePlacementUnavailable);
        }
        let current_routing = self.viewport.capabilities().cross_surface_routing();
        if dependencies.routed && previous_routing.is_supported() && !current_routing.is_supported()
        {
            return Some(InteractionCancelReason::UnknownTargetAuthority);
        }
        let current_release = self.viewport.capabilities().authoritative_release();
        if dependencies.routed && previous_release.is_supported() && !current_release.is_supported()
        {
            return Some(InteractionCancelReason::UnknownButtonState);
        }
        None
    }

    fn reduce_scene(
        &mut self,
        input: InputSequence,
        expected: WorkspaceVersion,
        building: &BuildingScene,
        coordinate_proofs: &BTreeMap<crate::ids::SurfaceId, CoordinateSnapshot>,
        scene_published: &mut bool,
        interaction_events: &mut Vec<InteractionEvent>,
    ) -> Result<InputOutcome, EngineError> {
        if expected != self.version {
            return Ok(InputOutcome::StaleRejected {
                expected,
                accepted_base: self.version,
            });
        }
        if *scene_published {
            return Ok(InputOutcome::SceneRejected {
                error: SceneBuildError::AlreadyPublishedInBoundary,
            });
        }
        let generation = self
            .last_scene_generation
            .checked_next()
            .ok_or(EngineError::SceneGenerationExhausted { input })?;
        let stamp = SceneStamp::new(self.version, generation);
        let sealed = match building.clone().seal(stamp, &self.workspace, &self.policy) {
            Ok(scene) => scene,
            Err(error) => {
                self.invalidate_transient(
                    input,
                    InteractionCancelReason::SceneUnavailable,
                    interaction_events,
                )?;
                return Ok(InputOutcome::SceneRejected { error });
            }
        };
        let (ready_surfaces, bootstrap_surfaces) =
            sealed
                .surfaces()
                .fold(
                    (0_usize, 0_usize),
                    |(ready, bootstrap), (_, surface)| match surface {
                        SurfaceScene::Ready(_) => (ready + 1, bootstrap),
                        SurfaceScene::Bootstrap(_) => (ready, bootstrap + 1),
                    },
                );
        self.scene_coordinate_authority = sealed
            .surfaces()
            .filter_map(|(surface, state)| {
                matches!(state, SurfaceScene::Ready(_))
                    .then(|| {
                        let captured = coordinate_proofs.get(surface).copied()?;
                        let current = self
                            .viewport
                            .viewport(*surface)
                            .and_then(crate::viewport_registry::ViewportRecord::coordinates)?;
                        SurfaceSceneCoordinateAuthority::coordinates_match(captured, current)
                            .then_some((
                                *surface,
                                SurfaceSceneCoordinateAuthority::new(stamp, captured),
                            ))
                    })
                    .flatten()
            })
            .collect();
        self.scene = Some(sealed);
        self.last_scene_generation = generation;
        *scene_published = true;
        self.refresh_drag_preview(input, interaction_events)?;
        self.refresh_contained_transform_preview(input, interaction_events)?;
        Ok(InputOutcome::ScenePublished {
            stamp,
            ready_surfaces,
            bootstrap_surfaces,
        })
    }

    fn reduce_renderer_intent(
        &mut self,
        input: InputSequence,
        intent: &RendererIntent,
        events: &mut Vec<WorkspaceEvent>,
        interaction_events: &mut Vec<InteractionEvent>,
    ) -> Result<InteractionOutcome, EngineError> {
        match intent {
            RendererIntent::ArmDragFrom { .. }
            | RendererIntent::UpdateDragObservation { .. }
            | RendererIntent::ReleaseDragObservation { .. } => {
                self.reduce_core_drag_renderer_intent(input, intent, events, interaction_events)
            }
            RendererIntent::ArmDrag {
                pointer,
                button,
                payload,
            } => self.arm_drag(input, *pointer, *button, payload, interaction_events),
            RendererIntent::BeginDrag {
                session,
                pointer,
                button,
            } => self.begin_drag(input, *session, *pointer, *button),
            RendererIntent::UpdateDrag {
                session,
                target,
                tear_off,
            } => self.update_drag(
                input,
                *session,
                target,
                tear_off.as_ref(),
                interaction_events,
            ),
            RendererIntent::AcknowledgePreview(acknowledgement) => {
                match self.interaction.acknowledge_preview(acknowledgement) {
                    Ok((session, changed)) => {
                        Ok(InteractionOutcome::PreviewAcknowledged { session, changed })
                    }
                    Err(error) => Ok(InteractionOutcome::Rejected(error)),
                }
            }
            RendererIntent::ReleaseDrag {
                session,
                pointer,
                button,
                button_state,
                target,
                tear_off,
            } => self.release_drag(
                input,
                DragReleaseInput {
                    session: *session,
                    pointer: *pointer,
                    button: *button,
                    button_state,
                    target,
                    tear_off: tear_off.as_ref(),
                },
                events,
                interaction_events,
            ),
            RendererIntent::CancelDrag { session, reason } => {
                self.cancel_drag(input, *session, *reason, interaction_events)
            }
            RendererIntent::BeginResize {
                pointer,
                button,
                split,
            } => self.begin_resize(input, *pointer, *button, split, interaction_events),
            RendererIntent::UpdateResize { session, weights } => {
                self.update_resize(input, *session, weights)
            }
            RendererIntent::ReleaseResize {
                session,
                pointer,
                button,
                button_state,
            } => self.release_resize(
                input,
                *session,
                *pointer,
                *button,
                *button_state,
                events,
                interaction_events,
            ),
            RendererIntent::CancelResize { session, reason } => {
                Ok(self.cancel_resize(input, *session, *reason, interaction_events))
            }
            RendererIntent::ApplyContainedPlacement { .. }
            | RendererIntent::BeginContainedTransform { .. }
            | RendererIntent::UpdateContainedTransform { .. }
            | RendererIntent::AcknowledgeContainedTransformPreview(_)
            | RendererIntent::ReleaseContainedTransform { .. }
            | RendererIntent::CancelContainedTransform { .. } => {
                self.reduce_contained_renderer_intent(input, intent, events, interaction_events)
            }
        }
    }

    fn reduce_core_drag_renderer_intent(
        &mut self,
        input: InputSequence,
        intent: &RendererIntent,
        events: &mut Vec<WorkspaceEvent>,
        interaction_events: &mut Vec<InteractionEvent>,
    ) -> Result<InteractionOutcome, EngineError> {
        match intent {
            RendererIntent::ArmDragFrom {
                pointer,
                button,
                payload,
                origin,
            } => self.arm_drag_from(
                input,
                *pointer,
                *button,
                payload,
                *origin,
                interaction_events,
            ),
            RendererIntent::UpdateDragObservation {
                session,
                target,
                current_pointer,
                contained_offer,
            } => self.update_drag_observation(
                input,
                *session,
                target,
                current_pointer,
                *contained_offer,
                interaction_events,
            ),
            RendererIntent::ReleaseDragObservation {
                session,
                pointer,
                button,
                button_state,
                target,
                current_pointer,
                contained_offer,
            } => self.release_drag_observation(
                input,
                ObservedDragReleaseInput {
                    session: *session,
                    pointer: *pointer,
                    button: *button,
                    button_state,
                    target,
                    current_pointer,
                    contained_offer: *contained_offer,
                },
                events,
                interaction_events,
            ),
            RendererIntent::ArmDrag { .. }
            | RendererIntent::BeginDrag { .. }
            | RendererIntent::UpdateDrag { .. }
            | RendererIntent::AcknowledgePreview(_)
            | RendererIntent::ReleaseDrag { .. }
            | RendererIntent::CancelDrag { .. }
            | RendererIntent::BeginResize { .. }
            | RendererIntent::UpdateResize { .. }
            | RendererIntent::ReleaseResize { .. }
            | RendererIntent::CancelResize { .. }
            | RendererIntent::ApplyContainedPlacement { .. }
            | RendererIntent::BeginContainedTransform { .. }
            | RendererIntent::UpdateContainedTransform { .. }
            | RendererIntent::AcknowledgeContainedTransformPreview(_)
            | RendererIntent::ReleaseContainedTransform { .. }
            | RendererIntent::CancelContainedTransform { .. } => Err(EngineError::Interaction {
                input,
                source: InteractionCounterError::StateInvariant,
            }),
        }
    }

    fn reduce_contained_renderer_intent(
        &mut self,
        input: InputSequence,
        intent: &RendererIntent,
        events: &mut Vec<WorkspaceEvent>,
        interaction_events: &mut Vec<InteractionEvent>,
    ) -> Result<InteractionOutcome, EngineError> {
        match intent {
            RendererIntent::ApplyContainedPlacement {
                root,
                floating,
                expected_rect,
                placement,
            } => self.apply_contained_placement(
                input,
                ContainedPlacementInput {
                    root: *root,
                    floating: *floating,
                    expected_rect: *expected_rect,
                    placement: *placement,
                },
                events,
                interaction_events,
            ),
            RendererIntent::BeginContainedTransform {
                surface,
                root,
                floating,
                pointer,
                button,
                initial_pointer,
                kind,
                minimum_size,
            } => self.begin_contained_transform(
                input,
                ContainedTransformBeginInput {
                    surface: *surface,
                    root: *root,
                    floating: *floating,
                    pointer: *pointer,
                    button: *button,
                    initial_pointer: *initial_pointer,
                    kind: *kind,
                    minimum_size: *minimum_size,
                },
                interaction_events,
            ),
            RendererIntent::UpdateContainedTransform {
                session,
                current_pointer,
            } => self.update_contained_transform(
                input,
                *session,
                *current_pointer,
                interaction_events,
            ),
            RendererIntent::AcknowledgeContainedTransformPreview(acknowledgement) => {
                match self
                    .interaction
                    .acknowledge_contained_transform_preview(*acknowledgement)
                {
                    Ok((session, changed)) => {
                        Ok(InteractionOutcome::ContainedTransformPreviewAcknowledged {
                            session,
                            changed,
                        })
                    }
                    Err(error) => Ok(InteractionOutcome::Rejected(error)),
                }
            }
            RendererIntent::ReleaseContainedTransform {
                session,
                pointer,
                button,
                button_state,
            } => self.release_contained_transform(
                input,
                ContainedTransformReleaseInput {
                    session: *session,
                    pointer: *pointer,
                    button: *button,
                    button_state,
                },
                events,
                interaction_events,
            ),
            RendererIntent::CancelContainedTransform { session, reason } => {
                Ok(self.cancel_contained_transform(input, *session, *reason, interaction_events))
            }
            RendererIntent::ArmDragFrom { .. }
            | RendererIntent::ArmDrag { .. }
            | RendererIntent::BeginDrag { .. }
            | RendererIntent::UpdateDrag { .. }
            | RendererIntent::UpdateDragObservation { .. }
            | RendererIntent::AcknowledgePreview(_)
            | RendererIntent::ReleaseDrag { .. }
            | RendererIntent::ReleaseDragObservation { .. }
            | RendererIntent::CancelDrag { .. }
            | RendererIntent::BeginResize { .. }
            | RendererIntent::UpdateResize { .. }
            | RendererIntent::ReleaseResize { .. }
            | RendererIntent::CancelResize { .. } => {
                unreachable!("non-contained renderer intent reached the contained intent reducer")
            }
        }
    }

    fn apply_contained_placement(
        &mut self,
        input: InputSequence,
        update: ContainedPlacementInput,
        events: &mut Vec<WorkspaceEvent>,
        interaction_events: &mut Vec<InteractionEvent>,
    ) -> Result<InteractionOutcome, EngineError> {
        let command = match self.checked_contained_placement_command(update) {
            Ok(command) => command,
            Err(error) => return Ok(InteractionOutcome::Rejected(error)),
        };
        match self.apply_interaction_command(input, &command, events)? {
            CommandApplication::Applied { outcome, changed } => {
                if changed {
                    self.invalidate_transient(
                        input,
                        InteractionCancelReason::WorkspaceChanged,
                        interaction_events,
                    )?;
                }
                Ok(InteractionOutcome::ContainedPlacementApplied { outcome, changed })
            }
            CommandApplication::Rejected(error) => Ok(InteractionOutcome::Rejected(
                InteractionRejection::CommandRejected(error),
            )),
        }
    }

    fn checked_contained_placement_command(
        &self,
        update: ContainedPlacementInput,
    ) -> Result<WorkspaceCommand, InteractionRejection> {
        self.validate_contained_placement(update.placement)
            .map_err(|error| match error {
                ContainedPlacementUnavailable::StaleScene { .. }
                | ContainedPlacementUnavailable::SceneUnavailable => {
                    InteractionRejection::StaleScene
                }
                other => InteractionRejection::ContainedPlacementUnavailable(other),
            })?;
        Ok(WorkspaceCommand::UpdateContainedRect {
            surface: update.placement.surface(),
            root: update.root,
            floating: update.floating,
            expected_rect: update.expected_rect,
            rect: update.placement.clamped_rect(),
        })
    }

    fn begin_contained_transform(
        &mut self,
        input: InputSequence,
        begin: ContainedTransformBeginInput,
        interaction_events: &mut Vec<InteractionEvent>,
    ) -> Result<InteractionOutcome, EngineError> {
        let Some(record) = self.workspace.contained_floating(begin.floating).copied() else {
            return Ok(InteractionOutcome::Rejected(
                InteractionRejection::CommandRejected(
                    crate::error::CommandError::MissingFloating {
                        floating: begin.floating,
                    },
                ),
            ));
        };
        let validation = WorkspaceCommand::UpdateContainedRect {
            surface: begin.surface,
            root: begin.root,
            floating: begin.floating,
            expected_rect: record.rect,
            rect: record.rect,
        };
        if let CommandApplication::Rejected(error) = self.preflight_command(input, &validation)? {
            return Ok(InteractionOutcome::Rejected(
                InteractionRejection::CommandRejected(error),
            ));
        }
        let placement =
            match self.contained_placement(begin.surface, record.rect, begin.minimum_size) {
                Ok(placement) => placement,
                Err(error) => {
                    return Ok(InteractionOutcome::Rejected(
                        InteractionRejection::ContainedPlacementUnavailable(error),
                    ));
                }
            };
        if placement.clamped_rect() != record.rect {
            return Ok(InteractionOutcome::Rejected(
                InteractionRejection::ContainedTransformInitialRectUnavailable,
            ));
        }
        let (session, replaced) = self
            .interaction
            .begin_contained_transform(
                self.version.epoch(),
                ContainedTransformStart {
                    pointer: begin.pointer,
                    button: begin.button,
                    surface: begin.surface,
                    root: begin.root,
                    floating: begin.floating,
                    source_rect: record.rect,
                    initial_pointer: begin.initial_pointer,
                    kind: begin.kind,
                    minimum_size: begin.minimum_size,
                },
            )
            .map_err(|source| EngineError::Interaction { input, source })?;
        if let Some(status) = replaced {
            self.viewport
                .end_all_drag_routing()
                .map_err(|source| EngineError::Viewport { input, source })?;
            interaction_events.push(InteractionEvent::new(
                input,
                self.version,
                InteractionEventKind::Cancelled {
                    status,
                    reason: InteractionCancelReason::ReplacedByNewGesture,
                },
            ));
        }
        Ok(InteractionOutcome::ContainedTransformBegan { session, replaced })
    }

    fn update_contained_transform(
        &mut self,
        input: InputSequence,
        session: ContainedTransformSessionId,
        current_pointer: crate::geometry::LogicalPoint,
        interaction_events: &mut Vec<InteractionEvent>,
    ) -> Result<InteractionOutcome, EngineError> {
        let transform = match self.interaction.active_contained_transform(session) {
            Ok(transform) => transform.clone(),
            Err(error) => return Ok(InteractionOutcome::Rejected(error)),
        };
        let placement =
            match self.resolve_contained_transform_placement(&transform, current_pointer) {
                Ok(placement) => placement,
                Err(error) => return Ok(InteractionOutcome::Rejected(error)),
            };
        self.interaction
            .set_contained_transform_pointer(session, current_pointer)
            .map_err(|_| EngineError::Interaction {
                input,
                source: InteractionCounterError::StateInvariant,
            })?;
        let (preview, changed) = self
            .interaction
            .publish_contained_transform_preview(session, placement.scene(), placement)
            .map_err(|source| EngineError::Interaction { input, source })?;
        if changed {
            interaction_events.push(InteractionEvent::new(
                input,
                self.version,
                InteractionEventKind::ContainedTransformPreviewPublished { preview },
            ));
        }
        Ok(InteractionOutcome::ContainedTransformPreviewUpdated { session, preview })
    }

    fn release_contained_transform(
        &mut self,
        input: InputSequence,
        release: ContainedTransformReleaseInput<'_>,
        events: &mut Vec<WorkspaceEvent>,
        interaction_events: &mut Vec<InteractionEvent>,
    ) -> Result<InteractionOutcome, EngineError> {
        if let Err(error) = self.validate_contained_transform_release_binding(
            release.session,
            release.pointer,
            release.button,
        ) {
            return Ok(InteractionOutcome::Rejected(error));
        }
        match release.button_state {
            Authority::Known(PointerButtonState::Pressed) => {
                return Ok(InteractionOutcome::Rejected(
                    InteractionRejection::ButtonStillPressed,
                ));
            }
            Authority::Unknown(_) => {
                return Ok(self.cancel_contained_transform(
                    input,
                    release.session,
                    InteractionCancelReason::UnknownButtonState,
                    interaction_events,
                ));
            }
            Authority::Known(PointerButtonState::Released) => {}
        }
        let transform = match self.interaction.take_contained_transform_for_release(
            release.session,
            release.pointer,
            release.button,
        ) {
            Ok(transform) => transform,
            Err(error) => return Ok(InteractionOutcome::Rejected(error)),
        };
        let Some(preview) = transform.preview.as_ref() else {
            return Ok(InteractionOutcome::Rejected(
                InteractionRejection::PreviewMissing,
            ));
        };
        if !preview.painted() {
            return Ok(InteractionOutcome::Rejected(
                InteractionRejection::PreviewNotPainted,
            ));
        }
        let placement = match self
            .resolve_contained_transform_placement(&transform, transform.current_pointer)
        {
            Ok(placement) => placement,
            Err(error) => return Ok(InteractionOutcome::Rejected(error)),
        };
        if preview.placement() != placement || preview.public().rect() != placement.clamped_rect() {
            return Ok(InteractionOutcome::Rejected(
                InteractionRejection::ContainedTransformChanged,
            ));
        }
        let command = match self.checked_contained_placement_command(ContainedPlacementInput {
            root: transform.root,
            floating: transform.floating,
            expected_rect: transform.source_rect,
            placement,
        }) {
            Ok(command) => command,
            Err(error) => return Ok(InteractionOutcome::Rejected(error)),
        };
        match self.apply_interaction_command(input, &command, events)? {
            CommandApplication::Applied { outcome, changed } => {
                interaction_events.push(InteractionEvent::new(
                    input,
                    self.version,
                    InteractionEventKind::ContainedTransformDelivered {
                        session: release.session,
                    },
                ));
                Ok(InteractionOutcome::ContainedTransformDelivered {
                    session: release.session,
                    outcome,
                    changed,
                })
            }
            CommandApplication::Rejected(error) => Ok(InteractionOutcome::Rejected(
                InteractionRejection::CommandRejected(error),
            )),
        }
    }

    fn validate_contained_transform_release_binding(
        &self,
        session: ContainedTransformSessionId,
        pointer: crate::intent::PointerId,
        button: crate::intent::PointerButton,
    ) -> Result<(), InteractionRejection> {
        let transform = self
            .interaction
            .active_contained_transform(session)
            .map_err(|error| {
                if matches!(
                    error,
                    InteractionRejection::ContainedTransformSessionConsumed { .. }
                ) {
                    InteractionRejection::DuplicateContainedTransformRelease { session }
                } else {
                    error
                }
            })?;
        if transform.pointer != pointer {
            return Err(InteractionRejection::PointerMismatch);
        }
        if transform.button != button {
            return Err(InteractionRejection::ButtonMismatch);
        }
        Ok(())
    }

    fn cancel_contained_transform(
        &mut self,
        input: InputSequence,
        session: ContainedTransformSessionId,
        reason: InteractionCancelReason,
        interaction_events: &mut Vec<InteractionEvent>,
    ) -> InteractionOutcome {
        match self.interaction.cancel_contained_transform(session) {
            Ok(status) => {
                interaction_events.push(InteractionEvent::new(
                    input,
                    self.version,
                    InteractionEventKind::Cancelled { status, reason },
                ));
                InteractionOutcome::Cancelled { status, reason }
            }
            Err(error) => InteractionOutcome::Rejected(error),
        }
    }

    fn arm_drag(
        &mut self,
        input: InputSequence,
        pointer: crate::intent::PointerId,
        button: crate::intent::PointerButton,
        payload: &MovePayload,
        interaction_events: &mut Vec<InteractionEvent>,
    ) -> Result<InteractionOutcome, EngineError> {
        let prepared = match self.prepare_drag_source(payload) {
            Ok(prepared) => prepared,
            Err(error) => return Ok(InteractionOutcome::Rejected(error)),
        };
        self.arm_validated_drag(
            input,
            pointer,
            button,
            payload,
            prepared,
            DragObservationProtocol::Legacy,
            FrozenDragOrigin::Workspace,
            interaction_events,
        )
    }

    fn arm_drag_from(
        &mut self,
        input: InputSequence,
        pointer: crate::intent::PointerId,
        button: crate::intent::PointerButton,
        payload: &MovePayload,
        origin: DragOrigin,
        interaction_events: &mut Vec<InteractionEvent>,
    ) -> Result<InteractionOutcome, EngineError> {
        let prepared = match self.prepare_drag_source(payload) {
            Ok(prepared) => prepared,
            Err(error) => return Ok(InteractionOutcome::Rejected(error)),
        };
        let origin = match self.freeze_drag_origin(origin, prepared.complete_root.as_ref()) {
            Ok(origin) => origin,
            Err(error) => return Ok(InteractionOutcome::Rejected(error)),
        };
        self.arm_validated_drag(
            input,
            pointer,
            button,
            payload,
            prepared,
            DragObservationProtocol::CoreOwned,
            origin,
            interaction_events,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn arm_validated_drag(
        &mut self,
        input: InputSequence,
        pointer: crate::intent::PointerId,
        button: crate::intent::PointerButton,
        payload: &MovePayload,
        prepared: PreparedDragSource,
        protocol: DragObservationProtocol,
        origin: FrozenDragOrigin,
        interaction_events: &mut Vec<InteractionEvent>,
    ) -> Result<InteractionOutcome, EngineError> {
        let (session, replaced) = self
            .interaction
            .arm_drag(DragArmStart {
                epoch: self.version.epoch(),
                pointer,
                button,
                payload: payload.clone(),
                complete_root: prepared.complete_root,
                partial_detachable: prepared.partial_detachable,
                protocol,
                origin,
                source_validated_at: self.version,
            })
            .map_err(|source| EngineError::Interaction { input, source })?;
        if let Some(status) = replaced {
            self.viewport
                .end_all_drag_routing()
                .map_err(|source| EngineError::Viewport { input, source })?;
            interaction_events.push(InteractionEvent::new(
                input,
                self.version,
                InteractionEventKind::Cancelled {
                    status,
                    reason: InteractionCancelReason::ReplacedByNewGesture,
                },
            ));
        }
        Ok(InteractionOutcome::DragArmed { session, replaced })
    }

    fn prepare_drag_source(
        &self,
        payload: &MovePayload,
    ) -> Result<PreparedDragSource, InteractionRejection> {
        self.validate_payload(payload)
            .map_err(InteractionRejection::CommandRejected)?;
        let complete_root = self
            .complete_root_source(payload)
            .map_err(InteractionRejection::CommandRejected)?;
        let partial_detachable =
            complete_root.is_some() || self.partial_payload_is_detachable(payload);
        Ok(PreparedDragSource {
            complete_root,
            partial_detachable,
        })
    }

    fn freeze_drag_origin(
        &self,
        origin: DragOrigin,
        complete_root: Option<&NodeSource>,
    ) -> Result<FrozenDragOrigin, InteractionRejection> {
        let DragOrigin::Contained(origin) = origin else {
            return Ok(FrozenDragOrigin::Workspace);
        };
        let Some(source) = complete_root else {
            return Err(InteractionRejection::ContainedDragOriginMismatch);
        };
        let surface = origin.initial_pointer().surface();
        if source.root() != origin.root()
            || self.workspace.presentation_for_root(origin.root())
                != Some(crate::RootPresentationOwner::Contained {
                    surface,
                    floating: origin.floating(),
                })
        {
            return Err(InteractionRejection::ContainedDragOriginMismatch);
        }
        let Some(record) = self.workspace.contained_floating(origin.floating()) else {
            return Err(InteractionRejection::ContainedDragOriginMismatch);
        };
        if record.root != origin.root() || record.surface != surface {
            return Err(InteractionRejection::ContainedDragOriginMismatch);
        }
        let placement = self
            .contained_placement(surface, record.rect, origin.minimum_size())
            .map_err(InteractionRejection::ContainedPlacementUnavailable)?;
        if placement.clamped_rect() != record.rect {
            return Err(InteractionRejection::ContainedDragOriginMismatch);
        }
        Ok(FrozenDragOrigin::Contained(FrozenContainedDragOrigin {
            surface,
            root: origin.root(),
            floating: origin.floating(),
            source_rect: record.rect,
            initial_pointer: origin.initial_pointer().position(),
            minimum_size: origin.minimum_size(),
        }))
    }

    fn begin_drag(
        &mut self,
        input: InputSequence,
        session: crate::interaction::DragSessionId,
        pointer: crate::intent::PointerId,
        button: crate::intent::PointerButton,
    ) -> Result<InteractionOutcome, EngineError> {
        if let Err(error) = self.interaction.begin_drag(session, pointer, button) {
            return Ok(InteractionOutcome::Rejected(error));
        }
        let payload = self
            .interaction
            .active_drag(session)
            .map_err(|_| EngineError::Interaction {
                input,
                source: InteractionCounterError::StateInvariant,
            })?
            .payload
            .clone();
        if let Some(surface) = self.payload_surface(&payload) {
            let _ = self
                .viewport
                .begin_drag_routing(pointer, surface)
                .map_err(|source| EngineError::Viewport { input, source })?;
        }
        Ok(InteractionOutcome::DragBegan { session })
    }

    fn update_drag(
        &mut self,
        input: InputSequence,
        session: crate::interaction::DragSessionId,
        target: &TargetAuthority,
        tear_off: Option<&TearOffRequest>,
        interaction_events: &mut Vec<InteractionEvent>,
    ) -> Result<InteractionOutcome, EngineError> {
        if let Err(error) =
            self.interaction
                .set_drag_observation(session, target.clone(), tear_off.cloned())
        {
            return Ok(InteractionOutcome::Rejected(error));
        }
        let (payload, pointer) = {
            let drag =
                self.interaction
                    .active_drag(session)
                    .map_err(|_| EngineError::Interaction {
                        input,
                        source: InteractionCounterError::StateInvariant,
                    })?;
            (drag.payload.clone(), drag.pointer)
        };
        let evaluation =
            self.resolve_preview_evaluation(input, session, pointer, &payload, target, tear_off)?;
        self.apply_preview_evaluation(input, session, evaluation, interaction_events)
    }

    fn update_drag_observation(
        &mut self,
        input: InputSequence,
        session: crate::interaction::DragSessionId,
        target: &TargetAuthority,
        current_pointer: &Authority<SurfacePointer>,
        contained_offer: Option<ContainedPresentationOffer>,
        interaction_events: &mut Vec<InteractionEvent>,
    ) -> Result<InteractionOutcome, EngineError> {
        if let Err(error) = self.interaction.set_core_drag_observation(
            session,
            target.clone(),
            *current_pointer,
            contained_offer,
        ) {
            return Ok(InteractionOutcome::Rejected(error));
        }
        if !self.core_drag_source_is_current(session, input)? {
            return self.cancel_drag(
                input,
                session,
                InteractionCancelReason::SourceVanished,
                interaction_events,
            );
        }
        let drag = self
            .interaction
            .active_drag(session)
            .map_err(|_| EngineError::Interaction {
                input,
                source: InteractionCounterError::StateInvariant,
            })?;
        let evaluation =
            self.resolve_core_preview_evaluation(input, drag, target, current_pointer)?;
        self.apply_preview_evaluation(input, session, evaluation, interaction_events)
    }

    fn core_drag_source_is_current(
        &mut self,
        session: crate::interaction::DragSessionId,
        input: InputSequence,
    ) -> Result<bool, EngineError> {
        let (payload, complete_root, origin) = {
            let drag =
                self.interaction
                    .active_drag(session)
                    .map_err(|_| EngineError::Interaction {
                        input,
                        source: InteractionCounterError::StateInvariant,
                    })?;
            if drag.source_validated_at == self.version {
                return Ok(true);
            }
            (
                drag.payload.clone(),
                drag.complete_root.clone(),
                drag.origin,
            )
        };
        let Ok(prepared) = self.prepare_drag_source(&payload) else {
            return Ok(false);
        };
        let valid =
            prepared.complete_root == complete_root && self.frozen_drag_origin_is_current(origin);
        if valid {
            let drag = self.interaction.active_drag_mut(session).map_err(|_| {
                EngineError::Interaction {
                    input,
                    source: InteractionCounterError::StateInvariant,
                }
            })?;
            drag.source_validated_at = self.version;
            drag.partial_detachable = prepared.partial_detachable;
        }
        Ok(valid)
    }

    fn frozen_drag_origin_is_current(&self, origin: FrozenDragOrigin) -> bool {
        let FrozenDragOrigin::Contained(origin) = origin else {
            return true;
        };
        self.workspace.presentation_for_root(origin.root)
            == Some(crate::RootPresentationOwner::Contained {
                surface: origin.surface,
                floating: origin.floating,
            })
            && self
                .workspace
                .contained_floating(origin.floating)
                .is_some_and(|record| {
                    record.root == origin.root
                        && record.surface == origin.surface
                        && record.rect == origin.source_rect
                })
    }

    fn resolve_core_preview_evaluation(
        &self,
        input: InputSequence,
        drag: &crate::interaction::ActiveDrag,
        target: &TargetAuthority,
        current_pointer: &Authority<SurfacePointer>,
    ) -> Result<PreviewEvaluation, EngineError> {
        let (target, current_pointer, local) =
            match self.normalize_core_drag_observation(drag.pointer, target, current_pointer) {
                Ok(observation) => observation,
                Err(reason) => {
                    return Ok(PreviewEvaluation::without_affordance(
                        PreviewDecision::Cancel(reason),
                    ));
                }
            };
        let mut evaluation = self.resolve_preview_evaluation(
            input,
            drag.session,
            drag.pointer,
            &drag.payload,
            &target,
            None,
        )?;
        if !local
            || !matches!(
                evaluation.decision,
                PreviewDecision::Clear(PreviewResolutionStatus::KnownNone)
            )
        {
            return Ok(evaluation);
        }
        evaluation.decision = match self.core_contained_candidate(drag, current_pointer) {
            CoreContainedCandidate::None => evaluation.decision,
            CoreContainedCandidate::Request(request) => {
                let TearOffRequest::Contained(proposal) = &request else {
                    return Err(EngineError::Interaction {
                        input,
                        source: InteractionCounterError::StateInvariant,
                    });
                };
                let command = self.contained_presentation_command(
                    &drag.payload,
                    *proposal,
                    drag.complete_root.clone(),
                );
                if let Some(mutation) = self.valid_core_contained_command(drag, *proposal, &command)
                {
                    Self::contained_preview_decision(*proposal, &request, false, command, mutation)
                } else {
                    PreviewDecision::Clear(PreviewResolutionStatus::Rejected)
                }
            }
            CoreContainedCandidate::Rejected => {
                PreviewDecision::Clear(PreviewResolutionStatus::Rejected)
            }
            CoreContainedCandidate::Cancel(reason) => PreviewDecision::Cancel(reason),
        };
        Ok(evaluation)
    }

    fn valid_core_contained_command(
        &self,
        drag: &crate::interaction::ActiveDrag,
        proposal: crate::intent::ContainedTearOffProposal,
        command: &WorkspaceCommand,
    ) -> Option<ContainedMutationKind> {
        if self
            .validate_contained_placement(proposal.placement())
            .is_err()
        {
            return None;
        }
        match command {
            WorkspaceCommand::UpdateContainedRect {
                surface,
                root,
                floating,
                expected_rect,
                rect,
            } => {
                let FrozenDragOrigin::Contained(origin) = drag.origin else {
                    return None;
                };
                let source_matches = drag.complete_root.as_ref().is_some_and(|source| {
                    source.root() == origin.root
                        && match &drag.payload {
                            MovePayload::Item(item) => {
                                item.root() == source.root() && item.tabs() == source.node()
                            }
                            MovePayload::Tabs(payload) | MovePayload::Subtree(payload) => {
                                payload == source
                            }
                        }
                });
                let owner_matches = self.workspace.presentation_for_root(origin.root)
                    == Some(crate::RootPresentationOwner::Contained {
                        surface: origin.surface,
                        floating: origin.floating,
                    });
                let record_matches = self
                    .workspace
                    .contained_floating(origin.floating)
                    .is_some_and(|record| {
                        record.root == origin.root
                            && record.surface == origin.surface
                            && record.rect == origin.source_rect
                            && record.z_order == proposal.z_order()
                    });
                (drag.source_validated_at == self.version
                    && source_matches
                    && owner_matches
                    && record_matches
                    && proposal.surface() == origin.surface
                    && proposal.root() == origin.root
                    && proposal.floating() == origin.floating
                    && *surface == origin.surface
                    && *root == origin.root
                    && *floating == origin.floating
                    && *expected_rect == origin.source_rect
                    && *rect == proposal.rect())
                .then_some(ContainedMutationKind::ExistingRectUpdate)
            }
            WorkspaceCommand::CreateContainedRoot {
                surface,
                root,
                floating,
                content: RootContent::Move(payload),
                ..
            } => (self
                .policy
                .check_tear_off(TearOffPresentation::Contained)
                .is_ok()
                && drag.complete_root.is_none()
                && payload == &drag.payload
                && *surface == proposal.surface()
                && *root == proposal.root()
                && *floating == proposal.floating()
                && self.workspace.surface(*surface).is_some()
                && self.workspace.root(*root).is_none()
                && self.workspace.contained_floating(*floating).is_none()
                && drag.partial_detachable)
                .then_some(ContainedMutationKind::PresentationChange),
            WorkspaceCommand::RehomeRoot { source, target } => (self
                .policy
                .check_tear_off(TearOffPresentation::Contained)
                .is_ok()
                && drag.complete_root.as_ref() == Some(source)
                && self.core_contained_rehome_is_valid(source, *target, proposal))
            .then_some(ContainedMutationKind::PresentationChange),
            WorkspaceCommand::CreateContainedRoot { .. }
            | WorkspaceCommand::Select { .. }
            | WorkspaceCommand::Reorder { .. }
            | WorkspaceCommand::Open { .. }
            | WorkspaceCommand::Close { .. }
            | WorkspaceCommand::CloseRoot { .. }
            | WorkspaceCommand::Move { .. }
            | WorkspaceCommand::ResizeSplit { .. }
            | WorkspaceCommand::CreateSurfaceRoot { .. }
            | WorkspaceCommand::RaiseContained { .. }
            | WorkspaceCommand::RemoveEmptyRoot { .. } => None,
        }
    }

    fn partial_payload_is_detachable(&self, payload: &MovePayload) -> bool {
        #[cfg(test)]
        PARTIAL_DETACHABILITY_EVALUATIONS.with(|evaluations| {
            evaluations.set(evaluations.get().saturating_add(1));
        });
        let (MovePayload::Tabs(source) | MovePayload::Subtree(source)) = payload else {
            return true;
        };
        let Some(root) = self.workspace.root(source.root()) else {
            return false;
        };
        root.central
            .is_none_or(|central| !self.workspace.subtree_contains(source.node(), central))
    }

    fn core_contained_rehome_is_valid(
        &self,
        source: &NodeSource,
        target: RootPresentationTarget,
        proposal: crate::intent::ContainedTearOffProposal,
    ) -> bool {
        let RootPresentationTarget::Contained {
            surface,
            floating,
            rect,
            z_order,
        } = target
        else {
            return false;
        };
        if source.root() != proposal.root()
            || surface != proposal.surface()
            || floating != proposal.floating()
            || rect != proposal.rect()
            || z_order != proposal.z_order()
            || self.workspace.surface(surface).is_none()
        {
            return false;
        }
        match self.workspace.presentation_for_root(source.root()) {
            Some(crate::RootPresentationOwner::Main {
                surface: source_surface,
            }) => {
                source_surface != surface
                    && self
                        .workspace
                        .surface(source_surface)
                        .is_some_and(|presentation| presentation.contained.is_empty())
                    && self.workspace.contained_floating(floating).is_none()
            }
            Some(crate::RootPresentationOwner::Contained {
                surface: source_surface,
                floating: source_floating,
            }) => {
                source_floating == floating
                    && (source_surface != surface
                        || self
                            .workspace
                            .contained_floating(floating)
                            .is_some_and(|record| record.rect == rect && record.z_order == z_order))
            }
            None => false,
        }
    }

    fn normalize_core_drag_observation(
        &self,
        pointer: crate::intent::PointerId,
        target: &TargetAuthority,
        current_pointer: &Authority<SurfacePointer>,
    ) -> Result<(TargetAuthority, SurfacePointer, bool), InteractionCancelReason> {
        let Authority::Known(current_pointer) = current_pointer else {
            return Err(InteractionCancelReason::UnknownTargetAuthority);
        };
        if self.workspace.surface(current_pointer.surface()).is_none() {
            return Err(InteractionCancelReason::UnknownTargetAuthority);
        }
        let (observed_target, routed) = self.target_observation(pointer, target)?;
        match observed_target {
            Authority::Unknown(_) => Err(InteractionCancelReason::UnknownTargetAuthority),
            Authority::Known(Some(observed)) if observed != current_pointer => {
                Err(InteractionCancelReason::UnknownTargetAuthority)
            }
            Authority::Known(Some(_) | None) => match target {
                TargetAuthority::Local(local) if current_pointer.surface() == local.observer() => {
                    Ok((target.clone(), *current_pointer, true))
                }
                TargetAuthority::Routed(_) if routed => {
                    Ok((target.clone(), *current_pointer, false))
                }
                TargetAuthority::Local(_) | TargetAuthority::Routed(_) => {
                    Err(InteractionCancelReason::UnknownTargetAuthority)
                }
            },
        }
    }

    fn core_contained_candidate(
        &self,
        drag: &crate::interaction::ActiveDrag,
        current_pointer: SurfacePointer,
    ) -> CoreContainedCandidate {
        let (root, floating, source_rect, initial_pointer, minimum_size, z_order) =
            match drag.origin {
                FrozenDragOrigin::Contained(origin) => {
                    if current_pointer.surface() != origin.surface {
                        return CoreContainedCandidate::None;
                    }
                    let Some(record) = self.workspace.contained_floating(origin.floating) else {
                        return CoreContainedCandidate::Rejected;
                    };
                    if record.root != origin.root
                        || record.surface != origin.surface
                        || record.rect != origin.source_rect
                    {
                        return CoreContainedCandidate::Rejected;
                    }
                    (
                        origin.root,
                        origin.floating,
                        origin.source_rect,
                        origin.initial_pointer,
                        origin.minimum_size,
                        record.z_order,
                    )
                }
                FrozenDragOrigin::Workspace => {
                    let Some(offer) = drag.contained_offer else {
                        return CoreContainedCandidate::None;
                    };
                    if current_pointer.surface() != offer.anchor().surface() {
                        return CoreContainedCandidate::None;
                    }
                    let z_order = match offer.stacking() {
                        ContainedStackPlacement::Front => {
                            let Some(z_order) =
                                self.next_contained_front_z_order(current_pointer.surface())
                            else {
                                return CoreContainedCandidate::Rejected;
                            };
                            z_order
                        }
                    };
                    (
                        offer.root(),
                        offer.floating(),
                        offer.requested_rect(),
                        offer.anchor().position(),
                        offer.minimum_size(),
                        z_order,
                    )
                }
            };
        let Ok(requested_rect) =
            translated_contained_rect(source_rect, initial_pointer, current_pointer.position())
        else {
            return CoreContainedCandidate::Rejected;
        };
        let placement =
            match self.contained_placement(current_pointer.surface(), requested_rect, minimum_size)
            {
                Ok(placement) => placement,
                Err(
                    ContainedPlacementUnavailable::UnrepresentableGeometry { .. }
                    | ContainedPlacementUnavailable::ProofMismatch { .. },
                ) => {
                    return CoreContainedCandidate::Rejected;
                }
                Err(
                    ContainedPlacementUnavailable::SceneUnavailable
                    | ContainedPlacementUnavailable::StaleScene { .. }
                    | ContainedPlacementUnavailable::MissingSurface { .. }
                    | ContainedPlacementUnavailable::BootstrapSurface { .. },
                ) => {
                    return CoreContainedCandidate::Cancel(
                        InteractionCancelReason::SceneUnavailable,
                    );
                }
            };
        CoreContainedCandidate::Request(TearOffRequest::Contained(
            crate::intent::ContainedTearOffProposal::new(root, floating, placement, z_order),
        ))
    }

    fn next_contained_front_z_order(&self, surface: crate::ids::SurfaceId) -> Option<u64> {
        match self.workspace.contained_frontmost(surface).ok()? {
            Some(frontmost) => frontmost.z_order().checked_add(1),
            None => Some(1),
        }
    }

    fn cancel_drag(
        &mut self,
        input: InputSequence,
        session: crate::interaction::DragSessionId,
        reason: InteractionCancelReason,
        interaction_events: &mut Vec<InteractionEvent>,
    ) -> Result<InteractionOutcome, EngineError> {
        let pointer = self
            .interaction
            .active_drag(session)
            .ok()
            .map(|drag| drag.pointer);
        match self.interaction.cancel_drag(session) {
            Ok(status) => {
                if let Some(pointer) = pointer {
                    let _ = self
                        .viewport
                        .end_drag_routing(pointer)
                        .map_err(|source| EngineError::Viewport { input, source })?;
                }
                interaction_events.push(InteractionEvent::new(
                    input,
                    self.version,
                    InteractionEventKind::Cancelled { status, reason },
                ));
                Ok(InteractionOutcome::Cancelled { status, reason })
            }
            Err(error) => Ok(InteractionOutcome::Rejected(error)),
        }
    }

    fn begin_resize(
        &mut self,
        input: InputSequence,
        pointer: crate::intent::PointerId,
        button: crate::intent::PointerButton,
        split: &crate::command::NodeSource,
        interaction_events: &mut Vec<InteractionEvent>,
    ) -> Result<InteractionOutcome, EngineError> {
        if let Err(error) = self.validate_node_source(split) {
            return Ok(InteractionOutcome::Rejected(
                InteractionRejection::ResizeRejected(error),
            ));
        }
        let (session, replaced) = self
            .interaction
            .begin_resize(self.version.epoch(), pointer, button, split.clone())
            .map_err(|source| EngineError::Interaction { input, source })?;
        if let Some(status) = replaced {
            self.viewport
                .end_all_drag_routing()
                .map_err(|source| EngineError::Viewport { input, source })?;
            interaction_events.push(InteractionEvent::new(
                input,
                self.version,
                InteractionEventKind::Cancelled {
                    status,
                    reason: InteractionCancelReason::ReplacedByNewGesture,
                },
            ));
        }
        Ok(InteractionOutcome::ResizeBegan { session, replaced })
    }

    fn update_resize(
        &mut self,
        input: InputSequence,
        session: crate::interaction::ResizeSessionId,
        weights: &[crate::graph::SplitWeight],
    ) -> Result<InteractionOutcome, EngineError> {
        let split = match self.interaction.active_resize(session) {
            Ok(resize) => resize.split.clone(),
            Err(error) => return Ok(InteractionOutcome::Rejected(error)),
        };
        let command = WorkspaceCommand::ResizeSplit {
            split,
            weights: weights.to_vec(),
        };
        match self.preflight_command(input, &command)? {
            CommandApplication::Applied { .. } => {
                self.interaction
                    .set_resize_weights(session, weights.to_vec())
                    .map_err(|_| EngineError::Interaction {
                        input,
                        source: InteractionCounterError::StateInvariant,
                    })?;
                Ok(InteractionOutcome::ResizeUpdated {
                    session,
                    weights: weights.to_vec(),
                })
            }
            CommandApplication::Rejected(error) => Ok(InteractionOutcome::Rejected(
                InteractionRejection::ResizeRejected(error),
            )),
        }
    }

    fn cancel_resize(
        &mut self,
        input: InputSequence,
        session: crate::interaction::ResizeSessionId,
        reason: InteractionCancelReason,
        interaction_events: &mut Vec<InteractionEvent>,
    ) -> InteractionOutcome {
        match self.interaction.cancel_resize(session) {
            Ok(status) => {
                interaction_events.push(InteractionEvent::new(
                    input,
                    self.version,
                    InteractionEventKind::Cancelled { status, reason },
                ));
                InteractionOutcome::Cancelled { status, reason }
            }
            Err(error) => InteractionOutcome::Rejected(error),
        }
    }

    fn apply_preview_evaluation(
        &mut self,
        input: InputSequence,
        session: crate::interaction::DragSessionId,
        evaluation: PreviewEvaluation,
        interaction_events: &mut Vec<InteractionEvent>,
    ) -> Result<InteractionOutcome, EngineError> {
        self.interaction
            .set_drop_affordance(session, evaluation.affordance)
            .map_err(|_| EngineError::Interaction {
                input,
                source: InteractionCounterError::StateInvariant,
            })?;
        match evaluation.decision {
            PreviewDecision::Publish { visual, proof } => {
                let stamp = self.scene.as_ref().map(SealedScene::stamp).ok_or(
                    EngineError::Interaction {
                        input,
                        source: InteractionCounterError::StateInvariant,
                    },
                )?;
                let (preview, changed) = self
                    .interaction
                    .publish_preview(session, stamp, visual, *proof)
                    .map_err(|source| EngineError::Interaction { input, source })?;
                if changed {
                    interaction_events.push(InteractionEvent::new(
                        input,
                        self.version,
                        InteractionEventKind::PreviewPublished {
                            preview: preview.clone(),
                        },
                    ));
                }
                Ok(InteractionOutcome::PreviewUpdated {
                    session,
                    preview: Some(preview),
                    status: PreviewResolutionStatus::Resolved,
                })
            }
            PreviewDecision::Clear(status) => {
                self.interaction
                    .clear_preview(session)
                    .map_err(|_| EngineError::Interaction {
                        input,
                        source: InteractionCounterError::StateInvariant,
                    })?;
                Ok(InteractionOutcome::PreviewUpdated {
                    session,
                    preview: None,
                    status,
                })
            }
            PreviewDecision::Cancel(reason) => {
                self.cancel_drag(input, session, reason, interaction_events)
            }
        }
    }

    fn resolve_preview_evaluation(
        &self,
        input: InputSequence,
        session: crate::interaction::DragSessionId,
        pointer: crate::intent::PointerId,
        payload: &MovePayload,
        target: &TargetAuthority,
        tear_off: Option<&TearOffRequest>,
    ) -> Result<PreviewEvaluation, EngineError> {
        let Some(scene) = self.scene.as_ref() else {
            return Ok(PreviewEvaluation::without_affordance(
                PreviewDecision::Cancel(InteractionCancelReason::SceneUnavailable),
            ));
        };
        if scene.stamp().workspace() != self.version {
            return Ok(PreviewEvaluation::without_affordance(
                PreviewDecision::Cancel(InteractionCancelReason::SceneUnavailable),
            ));
        }
        let (target, routed) = match self.target_observation(pointer, target) {
            Ok(target) => target,
            Err(reason) => {
                return Ok(PreviewEvaluation::without_affordance(
                    PreviewDecision::Cancel(reason),
                ));
            }
        };
        match target {
            Authority::Unknown(_) => Ok(PreviewEvaluation::without_affordance(
                PreviewDecision::Cancel(InteractionCancelReason::UnknownTargetAuthority),
            )),
            Authority::Known(Some(pointer)) => {
                let query = query_drop(
                    scene,
                    &self.workspace,
                    &self.policy,
                    session,
                    payload.clone(),
                    pointer.surface(),
                    pointer.position(),
                )
                .map_err(|source| EngineError::DropResolution { input, source })?;
                let (resolution, affordance) = query.into_parts();
                let decision = match resolution {
                    DropResolution::Resolved(resolved) => {
                        if resolved.scene_stamp() != scene.stamp()
                            || resolved.session() != session
                            || resolved.source() != payload
                        {
                            return Err(EngineError::Interaction {
                                input,
                                source: InteractionCounterError::StateInvariant,
                            });
                        }
                        let target = resolved.target_id();
                        let visual = PreviewVisual::Dock {
                            surface: pointer.surface(),
                            target,
                            rect: resolved.visual().rect(),
                        };
                        PreviewDecision::Publish {
                            visual,
                            proof: Box::new(PreviewProof::Dock {
                                target,
                                command: resolved.into_command(),
                            }),
                        }
                    }
                    DropResolution::KnownNone(_) => match tear_off {
                        Some(request @ TearOffRequest::Contained(proposal)) => self
                            .resolve_contained_tear_off(
                                input, payload, *proposal, request, false,
                            )?,
                        None | Some(TearOffRequest::Native { .. }) => {
                            PreviewDecision::Clear(PreviewResolutionStatus::KnownNone)
                        }
                    },
                    DropResolution::Rejected(_) => {
                        PreviewDecision::Clear(PreviewResolutionStatus::Rejected)
                    }
                    DropResolution::Unavailable(_) => {
                        PreviewDecision::Cancel(InteractionCancelReason::SceneUnavailable)
                    }
                };
                Ok(PreviewEvaluation::new(decision, affordance))
            }
            Authority::Known(None) => self
                .resolve_tear_off_decision(input, payload, tear_off, routed)
                .map(PreviewEvaluation::without_affordance),
        }
    }

    fn target_observation<'a>(
        &self,
        pointer: crate::intent::PointerId,
        target: &'a TargetAuthority,
    ) -> Result<(&'a Authority<Option<crate::intent::SurfacePointer>>, bool), InteractionCancelReason>
    {
        match target {
            TargetAuthority::Local(local) => {
                if self.workspace.surface(local.observer()).is_none()
                    || matches!(
                        local.target(),
                        Authority::Known(Some(target))
                            if target.surface() != local.observer()
                    )
                {
                    return Err(InteractionCancelReason::UnknownTargetAuthority);
                }
                Ok((local.target(), false))
            }
            TargetAuthority::Routed(proof) => {
                if proof.pointer() != pointer || !self.viewport.route_is_current(proof) {
                    return Err(InteractionCancelReason::UnknownTargetAuthority);
                }
                Ok((proof.target(), true))
            }
        }
    }

    fn resolve_tear_off_decision(
        &self,
        input: InputSequence,
        payload: &MovePayload,
        request: Option<&TearOffRequest>,
        routed: bool,
    ) -> Result<PreviewDecision, EngineError> {
        let Some(request) = request else {
            return Ok(PreviewDecision::Clear(PreviewResolutionStatus::KnownNone));
        };
        match request {
            TearOffRequest::Contained(proposal) => {
                self.resolve_contained_tear_off(input, payload, *proposal, request, false)
            }
            TearOffRequest::Native {
                proposal,
                contained_fallback,
            } => self.resolve_native_tear_off(
                input,
                payload,
                proposal.as_ref().clone(),
                *contained_fallback,
                request,
                routed,
            ),
        }
    }

    fn resolve_contained_tear_off(
        &self,
        input: InputSequence,
        payload: &MovePayload,
        proposal: crate::intent::ContainedTearOffProposal,
        request: &TearOffRequest,
        fallback: bool,
    ) -> Result<PreviewDecision, EngineError> {
        // Lifecycle commands can advance the revision without cancelling an
        // unrelated gesture, so the frozen source must be checked every frame.
        if self.validate_payload(payload).is_err() {
            return Ok(PreviewDecision::Clear(PreviewResolutionStatus::Rejected));
        }
        let complete_root =
            self.complete_root_source(payload)
                .map_err(|_| EngineError::Interaction {
                    input,
                    source: InteractionCounterError::StateInvariant,
                })?;
        self.resolve_validated_contained_tear_off(
            input,
            payload,
            proposal,
            request,
            fallback,
            complete_root,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn resolve_validated_contained_tear_off(
        &self,
        input: InputSequence,
        payload: &MovePayload,
        proposal: crate::intent::ContainedTearOffProposal,
        request: &TearOffRequest,
        fallback: bool,
        complete_root: Option<NodeSource>,
    ) -> Result<PreviewDecision, EngineError> {
        if self
            .validate_contained_placement(proposal.placement())
            .is_err()
            || self
                .policy
                .check_tear_off(TearOffPresentation::Contained)
                .is_err()
        {
            return Ok(PreviewDecision::Clear(PreviewResolutionStatus::Rejected));
        }
        if let Some(command) = self.same_contained_move_command_for_valid_payload(payload, proposal)
        {
            return Ok(Self::contained_preview_decision(
                proposal,
                request,
                fallback,
                command,
                ContainedMutationKind::PresentationChange,
            ));
        }
        if complete_root
            .as_ref()
            .is_some_and(|source| source.root() != proposal.root())
        {
            return Ok(PreviewDecision::Clear(PreviewResolutionStatus::Rejected));
        }
        let command = self.contained_presentation_command(payload, proposal, complete_root);
        match self.preflight_command(input, &command)? {
            CommandApplication::Applied { .. } => Ok(Self::contained_preview_decision(
                proposal,
                request,
                fallback,
                command,
                ContainedMutationKind::PresentationChange,
            )),
            CommandApplication::Rejected(_) => {
                Ok(PreviewDecision::Clear(PreviewResolutionStatus::Rejected))
            }
        }
    }

    fn contained_preview_decision(
        proposal: crate::intent::ContainedTearOffProposal,
        request: &TearOffRequest,
        fallback: bool,
        command: WorkspaceCommand,
        mutation: ContainedMutationKind,
    ) -> PreviewDecision {
        PreviewDecision::Publish {
            visual: PreviewVisual::Contained {
                surface: proposal.surface(),
                rect: proposal.rect(),
                fallback,
            },
            proof: Box::new(PreviewProof::Contained {
                command,
                request: request.clone(),
                fallback,
                mutation,
            }),
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn resolve_native_tear_off(
        &self,
        input: InputSequence,
        payload: &MovePayload,
        proposal: crate::intent::NativeTearOffProposal,
        contained_fallback: Option<crate::intent::ContainedTearOffProposal>,
        request: &TearOffRequest,
        routed: bool,
    ) -> Result<PreviewDecision, EngineError> {
        if self
            .validate_contained_placement(proposal.recovery().placement())
            .is_err()
            || self
                .policy
                .check_tear_off(TearOffPresentation::Native)
                .is_err()
        {
            return Ok(PreviewDecision::Clear(PreviewResolutionStatus::Rejected));
        }
        match self.viewport.native_tear_off_capability() {
            PlatformCapability::Supported => {
                if !routed {
                    return Ok(PreviewDecision::Cancel(
                        InteractionCancelReason::UnknownTargetAuthority,
                    ));
                }
                if !self
                    .viewport
                    .native_placement_is_current(proposal.placement())
                {
                    return Ok(PreviewDecision::Cancel(
                        InteractionCancelReason::NativePlacementUnavailable,
                    ));
                }
                if self.workspace.surface(proposal.surface()).is_some() {
                    return Ok(PreviewDecision::Clear(PreviewResolutionStatus::Rejected));
                }
                if !self.tear_off_root_identity_matches(input, payload, proposal.root())? {
                    return Ok(PreviewDecision::Clear(PreviewResolutionStatus::Rejected));
                }
                let command = self.tear_off_command(
                    input,
                    payload,
                    RootPresentationTarget::Surface {
                        surface: proposal.surface(),
                    },
                    proposal.root(),
                )?;
                match self.preflight_command(input, &command)? {
                    CommandApplication::Applied { .. } => Ok(PreviewDecision::Publish {
                        visual: PreviewVisual::Native {
                            surface: proposal.surface(),
                            placement: proposal.physical_placement(),
                        },
                        proof: Box::new(PreviewProof::Native {
                            command,
                            request: request.clone(),
                            proposal: Box::new(proposal),
                        }),
                    }),
                    CommandApplication::Rejected(_) => {
                        Ok(PreviewDecision::Clear(PreviewResolutionStatus::Rejected))
                    }
                }
            }
            PlatformCapability::Unsupported(_) => {
                if self.policy.native_unavailable_fallback().is_err() {
                    return Ok(PreviewDecision::Clear(PreviewResolutionStatus::Rejected));
                }
                let Some(proposal) = contained_fallback else {
                    return Ok(PreviewDecision::Clear(PreviewResolutionStatus::Rejected));
                };
                self.resolve_contained_tear_off(input, payload, proposal, request, true)
            }
            PlatformCapability::Unknown(_) => Ok(PreviewDecision::Cancel(
                InteractionCancelReason::NativeCapabilityUnknown,
            )),
        }
    }

    fn release_drag(
        &mut self,
        input: InputSequence,
        release: DragReleaseInput<'_>,
        events: &mut Vec<WorkspaceEvent>,
        interaction_events: &mut Vec<InteractionEvent>,
    ) -> Result<InteractionOutcome, EngineError> {
        if let Err(error) = self.validate_drag_release_binding(
            release.session,
            release.pointer,
            release.button,
            DragObservationProtocol::Legacy,
        ) {
            return Ok(InteractionOutcome::Rejected(error));
        }
        let target_unknown = match self.target_observation(release.pointer, release.target) {
            Ok((target, _)) => matches!(target, Authority::Unknown(_)),
            Err(reason) => {
                return self.cancel_drag(input, release.session, reason, interaction_events);
            }
        };
        let button_state = match release.target {
            TargetAuthority::Local(_) => *release.button_state,
            TargetAuthority::Routed(proof) => proof.button_state(release.button),
        };
        if let Some(outcome) =
            self.require_released_button(input, release.session, button_state, interaction_events)?
        {
            return Ok(outcome);
        }
        let drag = match self.interaction.take_drag_for_release(
            release.session,
            release.pointer,
            release.button,
        ) {
            Ok(drag) => drag,
            Err(error) => return Ok(InteractionOutcome::Rejected(error)),
        };
        if target_unknown {
            let _ = self
                .viewport
                .end_drag_routing(drag.pointer)
                .map_err(|source| EngineError::Viewport { input, source })?;
            return Ok(self.cancel_consumed_drag(
                input,
                release.session,
                InteractionCancelReason::UnknownTargetAuthority,
                interaction_events,
            ));
        }
        let proof = match self.resolve_release_proof(
            input,
            release.session,
            &drag,
            release.target,
            release.tear_off,
        )? {
            ReleaseProofDecision::Deliver(proof) => proof,
            ReleaseProofDecision::Reject(error) => {
                let _ = self
                    .viewport
                    .end_drag_routing(drag.pointer)
                    .map_err(|source| EngineError::Viewport { input, source })?;
                return Ok(InteractionOutcome::Rejected(error));
            }
        };
        let pane_focus = self.freeze_payload_focus(&drag.payload);
        let _ = self
            .viewport
            .end_drag_routing(drag.pointer)
            .map_err(|source| EngineError::Viewport { input, source })?;
        self.finish_drag_delivery(
            input,
            release.session,
            pane_focus,
            *proof,
            events,
            interaction_events,
        )
    }

    fn release_drag_observation(
        &mut self,
        input: InputSequence,
        release: ObservedDragReleaseInput<'_>,
        events: &mut Vec<WorkspaceEvent>,
        interaction_events: &mut Vec<InteractionEvent>,
    ) -> Result<InteractionOutcome, EngineError> {
        if let Err(error) = self.validate_drag_release_binding(
            release.session,
            release.pointer,
            release.button,
            DragObservationProtocol::CoreOwned,
        ) {
            return Ok(InteractionOutcome::Rejected(error));
        }
        if let Err(error) = self.interaction.set_core_drag_observation(
            release.session,
            release.target.clone(),
            *release.current_pointer,
            release.contained_offer,
        ) {
            return Ok(InteractionOutcome::Rejected(error));
        }
        if !self.core_drag_source_is_current(release.session, input)? {
            return self.cancel_drag(
                input,
                release.session,
                InteractionCancelReason::SourceVanished,
                interaction_events,
            );
        }
        if let Err(reason) = self.normalize_core_drag_observation(
            release.pointer,
            release.target,
            release.current_pointer,
        ) {
            return self.cancel_drag(input, release.session, reason, interaction_events);
        }
        let button_state = match release.target {
            TargetAuthority::Local(_) => *release.button_state,
            TargetAuthority::Routed(proof) => proof.button_state(release.button),
        };
        if let Some(outcome) =
            self.require_released_button(input, release.session, button_state, interaction_events)?
        {
            return Ok(outcome);
        }
        let drag = match self.interaction.take_drag_for_release(
            release.session,
            release.pointer,
            release.button,
        ) {
            Ok(drag) => drag,
            Err(error) => return Ok(InteractionOutcome::Rejected(error)),
        };
        let proof = match self.resolve_core_release_proof(
            input,
            &drag,
            release.target,
            release.current_pointer,
        )? {
            ReleaseProofDecision::Deliver(proof) => proof,
            ReleaseProofDecision::Reject(error) => {
                let _ = self
                    .viewport
                    .end_drag_routing(drag.pointer)
                    .map_err(|source| EngineError::Viewport { input, source })?;
                return Ok(InteractionOutcome::Rejected(error));
            }
        };
        let pane_focus = self.freeze_payload_focus(&drag.payload);
        let _ = self
            .viewport
            .end_drag_routing(drag.pointer)
            .map_err(|source| EngineError::Viewport { input, source })?;
        self.finish_drag_delivery(
            input,
            release.session,
            pane_focus,
            *proof,
            events,
            interaction_events,
        )
    }

    fn validate_drag_release_binding(
        &self,
        session: crate::interaction::DragSessionId,
        pointer: crate::intent::PointerId,
        button: crate::intent::PointerButton,
        expected_protocol: DragObservationProtocol,
    ) -> Result<(), InteractionRejection> {
        let drag = self.interaction.active_drag(session).map_err(|error| {
            if matches!(error, InteractionRejection::SessionConsumed { .. }) {
                InteractionRejection::DuplicateRelease { session }
            } else {
                error
            }
        })?;
        if drag.pointer != pointer {
            return Err(InteractionRejection::PointerMismatch);
        }
        if drag.button != button {
            return Err(InteractionRejection::ButtonMismatch);
        }
        if drag.protocol != expected_protocol {
            return Err(InteractionRejection::DragObservationProtocolMismatch);
        }
        Ok(())
    }

    fn require_released_button(
        &mut self,
        input: InputSequence,
        session: crate::interaction::DragSessionId,
        button_state: Authority<PointerButtonState>,
        interaction_events: &mut Vec<InteractionEvent>,
    ) -> Result<Option<InteractionOutcome>, EngineError> {
        match button_state {
            Authority::Known(PointerButtonState::Released) => Ok(None),
            Authority::Known(PointerButtonState::Pressed) => Ok(Some(
                InteractionOutcome::Rejected(InteractionRejection::ButtonStillPressed),
            )),
            Authority::Unknown(_) => {
                let reason = InteractionCancelReason::UnknownButtonState;
                self.cancel_drag(input, session, reason, interaction_events)
                    .map(Some)
            }
        }
    }

    fn cancel_consumed_drag(
        &self,
        input: InputSequence,
        session: crate::interaction::DragSessionId,
        reason: InteractionCancelReason,
        interaction_events: &mut Vec<InteractionEvent>,
    ) -> InteractionOutcome {
        let status = InteractionStatus::Dragging { session };
        interaction_events.push(InteractionEvent::new(
            input,
            self.version,
            InteractionEventKind::Cancelled { status, reason },
        ));
        InteractionOutcome::Cancelled { status, reason }
    }

    fn resolve_release_proof(
        &self,
        input: InputSequence,
        session: crate::interaction::DragSessionId,
        drag: &crate::interaction::ActiveDrag,
        target: &TargetAuthority,
        tear_off: Option<&TearOffRequest>,
    ) -> Result<ReleaseProofDecision, EngineError> {
        let Some(preview) = drag.preview.as_ref() else {
            return Ok(ReleaseProofDecision::Reject(
                InteractionRejection::PreviewMissing,
            ));
        };
        if !preview.painted() {
            return Ok(ReleaseProofDecision::Reject(
                InteractionRejection::PreviewNotPainted,
            ));
        }
        let Some(scene) = self.scene.as_ref() else {
            return Ok(ReleaseProofDecision::Reject(
                InteractionRejection::StaleScene,
            ));
        };
        if scene.stamp() != preview.public().token().scene()
            || scene.stamp().workspace() != self.version
        {
            return Ok(ReleaseProofDecision::Reject(
                InteractionRejection::StaleScene,
            ));
        }
        let evaluation = self.resolve_preview_evaluation(
            input,
            session,
            drag.pointer,
            &drag.payload,
            target,
            tear_off,
        )?;
        let PreviewDecision::Publish { visual, proof } = evaluation.decision else {
            return Ok(ReleaseProofDecision::Reject(
                InteractionRejection::TargetChanged,
            ));
        };
        if preview.public().visual() != &visual || preview.proof() != proof.as_ref() {
            return Ok(ReleaseProofDecision::Reject(
                InteractionRejection::TargetChanged,
            ));
        }
        Ok(ReleaseProofDecision::Deliver(proof))
    }

    fn resolve_core_release_proof(
        &self,
        input: InputSequence,
        drag: &crate::interaction::ActiveDrag,
        target: &TargetAuthority,
        current_pointer: &Authority<SurfacePointer>,
    ) -> Result<ReleaseProofDecision, EngineError> {
        let Some(preview) = drag.preview.as_ref() else {
            return Ok(ReleaseProofDecision::Reject(
                InteractionRejection::PreviewMissing,
            ));
        };
        if !preview.painted() {
            return Ok(ReleaseProofDecision::Reject(
                InteractionRejection::PreviewNotPainted,
            ));
        }
        let Some(scene) = self.scene.as_ref() else {
            return Ok(ReleaseProofDecision::Reject(
                InteractionRejection::StaleScene,
            ));
        };
        if scene.stamp() != preview.public().token().scene()
            || scene.stamp().workspace() != self.version
        {
            return Ok(ReleaseProofDecision::Reject(
                InteractionRejection::StaleScene,
            ));
        }
        let evaluation =
            self.resolve_core_preview_evaluation(input, drag, target, current_pointer)?;
        let PreviewDecision::Publish { visual, proof } = evaluation.decision else {
            return Ok(ReleaseProofDecision::Reject(
                InteractionRejection::TargetChanged,
            ));
        };
        if preview.public().visual() != &visual || preview.proof() != proof.as_ref() {
            return Ok(ReleaseProofDecision::Reject(
                InteractionRejection::TargetChanged,
            ));
        }
        Ok(ReleaseProofDecision::Deliver(proof))
    }

    fn finish_drag_delivery(
        &mut self,
        input: InputSequence,
        session: crate::interaction::DragSessionId,
        pane_focus: PanelFocus,
        proof: PreviewProof,
        events: &mut Vec<WorkspaceEvent>,
        interaction_events: &mut Vec<InteractionEvent>,
    ) -> Result<InteractionOutcome, EngineError> {
        match proof {
            PreviewProof::Dock { target, command } => self.finish_workspace_delivery(
                WorkspaceDeliveryInput {
                    input,
                    session,
                    target: WorkspaceDeliveryTarget {
                        kind: WorkspaceDeliveryKind::Dock,
                        focus_surface: target.surface(),
                        validation: WorkspaceDeliveryValidation::Policy,
                    },
                    pane_focus,
                    command: &command,
                },
                events,
                interaction_events,
            ),
            PreviewProof::Contained {
                command,
                request,
                fallback,
                mutation,
            } => {
                let kind = if fallback {
                    WorkspaceDeliveryKind::ContainedFallback
                } else {
                    WorkspaceDeliveryKind::Contained
                };
                let focus_surface = match request {
                    TearOffRequest::Contained(proposal)
                    | TearOffRequest::Native {
                        contained_fallback: Some(proposal),
                        ..
                    } => proposal.surface(),
                    TearOffRequest::Native {
                        contained_fallback: None,
                        ..
                    } => {
                        return Err(EngineError::Interaction {
                            input,
                            source: InteractionCounterError::StateInvariant,
                        });
                    }
                };
                self.finish_workspace_delivery(
                    WorkspaceDeliveryInput {
                        input,
                        session,
                        target: WorkspaceDeliveryTarget {
                            kind,
                            focus_surface,
                            validation: match mutation {
                                ContainedMutationKind::ExistingRectUpdate => {
                                    WorkspaceDeliveryValidation::ExistingContainedRect
                                }
                                ContainedMutationKind::PresentationChange => {
                                    WorkspaceDeliveryValidation::Policy
                                }
                            },
                        },
                        pane_focus,
                        command: &command,
                    },
                    events,
                    interaction_events,
                )
            }
            PreviewProof::Native {
                command, proposal, ..
            } => {
                let prepared = PreparedNativeTearOff::new(
                    session,
                    self.version,
                    command,
                    *proposal,
                    pane_focus,
                );
                let request = self
                    .viewport
                    .start_native_create(prepared)
                    .map_err(|source| EngineError::Viewport { input, source })?;
                interaction_events.push(InteractionEvent::new(
                    input,
                    self.version,
                    InteractionEventKind::NativeTearOffRequested(request),
                ));
                Ok(InteractionOutcome::DragDelivered {
                    session,
                    delivery: InteractionDelivery::NativeRequested(request),
                })
            }
        }
    }

    fn finish_workspace_delivery(
        &mut self,
        delivery: WorkspaceDeliveryInput<'_>,
        events: &mut Vec<WorkspaceEvent>,
        interaction_events: &mut Vec<InteractionEvent>,
    ) -> Result<InteractionOutcome, EngineError> {
        let application = match delivery.target.validation {
            WorkspaceDeliveryValidation::Policy => {
                self.apply_interaction_command(delivery.input, delivery.command, events)?
            }
            WorkspaceDeliveryValidation::ExistingContainedRect => {
                self.apply_existing_contained_rect_update(delivery.input, delivery.command, events)?
            }
        };
        match application {
            CommandApplication::Applied { outcome, changed } => {
                if let Some(binding) = self
                    .viewport
                    .viewport(delivery.target.focus_surface)
                    .filter(|record| record.is_focusable())
                    .map(crate::viewport_registry::ViewportRecord::binding)
                {
                    let _ = self.start_viewport_activation(
                        delivery.input,
                        ViewportActivationRequest::drop_committed(binding, delivery.pane_focus),
                        self.last_focus_reducer_generation,
                        events,
                    )?;
                }
                interaction_events.push(InteractionEvent::new(
                    delivery.input,
                    self.version,
                    InteractionEventKind::Delivered {
                        session: delivery.session,
                        kind: delivery.target.kind,
                    },
                ));
                Ok(InteractionOutcome::DragDelivered {
                    session: delivery.session,
                    delivery: InteractionDelivery::Workspace {
                        kind: delivery.target.kind,
                        outcome,
                        changed,
                    },
                })
            }
            CommandApplication::Rejected(error) => Ok(InteractionOutcome::Rejected(
                InteractionRejection::CommandRejected(error),
            )),
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn release_resize(
        &mut self,
        input: InputSequence,
        session: crate::interaction::ResizeSessionId,
        pointer: crate::intent::PointerId,
        button: crate::intent::PointerButton,
        button_state: Authority<PointerButtonState>,
        events: &mut Vec<WorkspaceEvent>,
        interaction_events: &mut Vec<InteractionEvent>,
    ) -> Result<InteractionOutcome, EngineError> {
        let binding = match self.interaction.active_resize(session) {
            Ok(resize) => (resize.pointer, resize.button),
            Err(error) => return Ok(InteractionOutcome::Rejected(error)),
        };
        if binding.0 != pointer {
            return Ok(InteractionOutcome::Rejected(
                InteractionRejection::PointerMismatch,
            ));
        }
        if binding.1 != button {
            return Ok(InteractionOutcome::Rejected(
                InteractionRejection::ButtonMismatch,
            ));
        }
        match button_state {
            Authority::Unknown(_) => {
                let status = self.interaction.cancel_resize(session).map_err(|_| {
                    EngineError::Interaction {
                        input,
                        source: InteractionCounterError::StateInvariant,
                    }
                })?;
                let reason = InteractionCancelReason::UnknownButtonState;
                interaction_events.push(InteractionEvent::new(
                    input,
                    self.version,
                    InteractionEventKind::Cancelled { status, reason },
                ));
                return Ok(InteractionOutcome::Cancelled { status, reason });
            }
            Authority::Known(PointerButtonState::Pressed) => {
                return Ok(InteractionOutcome::Rejected(
                    InteractionRejection::ButtonStillPressed,
                ));
            }
            Authority::Known(PointerButtonState::Released) => {}
        }
        let resize = match self
            .interaction
            .take_resize_for_release(session, pointer, button)
        {
            Ok(resize) => resize,
            Err(error) => return Ok(InteractionOutcome::Rejected(error)),
        };
        let Some(weights) = resize.weights else {
            return Ok(InteractionOutcome::Rejected(
                InteractionRejection::ResizeProposalMissing,
            ));
        };
        let command = WorkspaceCommand::ResizeSplit {
            split: resize.split,
            weights,
        };
        match self.apply_interaction_command(input, &command, events)? {
            CommandApplication::Applied { outcome, changed } => {
                interaction_events.push(InteractionEvent::new(
                    input,
                    self.version,
                    InteractionEventKind::ResizeDelivered { session },
                ));
                Ok(InteractionOutcome::ResizeDelivered {
                    session,
                    outcome,
                    changed,
                })
            }
            CommandApplication::Rejected(error) => Ok(InteractionOutcome::Rejected(
                InteractionRejection::ResizeRejected(error),
            )),
        }
    }

    fn resolve_contained_transform_placement(
        &self,
        transform: &ActiveContainedTransform,
        current_pointer: crate::geometry::LogicalPoint,
    ) -> Result<ContainedPlacementProof, InteractionRejection> {
        let (_, bounds) = self
            .current_ready_surface_bounds(transform.surface)
            .map_err(InteractionRejection::ContainedPlacementUnavailable)?;
        let requested = contained_transform_requested_rect(transform, current_pointer, bounds)
            .map_err(|()| InteractionRejection::ContainedTransformGeometryUnavailable)?;
        let placement = self
            .contained_placement(transform.surface, requested, transform.minimum_size)
            .map_err(InteractionRejection::ContainedPlacementUnavailable)?;
        if matches!(transform.kind, ContainedTransformKind::Resize(_))
            && placement.clamped_rect() != requested
        {
            return Err(InteractionRejection::ContainedTransformGeometryUnavailable);
        }
        Ok(placement)
    }

    fn refresh_contained_transform_preview(
        &mut self,
        input: InputSequence,
        interaction_events: &mut Vec<InteractionEvent>,
    ) -> Result<(), EngineError> {
        let InteractionStatus::ContainedTransforming { session } = self.interaction.status() else {
            return Ok(());
        };
        let transform = self
            .interaction
            .active_contained_transform(session)
            .map_err(|_| EngineError::Interaction {
                input,
                source: InteractionCounterError::StateInvariant,
            })?
            .clone();
        if transform.preview.is_none() {
            return Ok(());
        }
        let Ok(placement) =
            self.resolve_contained_transform_placement(&transform, transform.current_pointer)
        else {
            let _ = self.cancel_contained_transform(
                input,
                session,
                InteractionCancelReason::SceneUnavailable,
                interaction_events,
            );
            return Ok(());
        };
        let (preview, changed) = self
            .interaction
            .publish_contained_transform_preview(session, placement.scene(), placement)
            .map_err(|source| EngineError::Interaction { input, source })?;
        if changed {
            interaction_events.push(InteractionEvent::new(
                input,
                self.version,
                InteractionEventKind::ContainedTransformPreviewPublished { preview },
            ));
        }
        Ok(())
    }

    fn refresh_drag_preview(
        &mut self,
        input: InputSequence,
        interaction_events: &mut Vec<InteractionEvent>,
    ) -> Result<(), EngineError> {
        let InteractionStatus::Dragging { session } = self.interaction.status() else {
            return Ok(());
        };
        let protocol = self
            .interaction
            .active_drag(session)
            .map_err(|_| EngineError::Interaction {
                input,
                source: InteractionCounterError::StateInvariant,
            })?
            .protocol;
        if protocol == DragObservationProtocol::CoreOwned {
            return self.refresh_core_drag_preview(input, session, interaction_events);
        }
        let (pointer, payload, target, tear_off) = {
            let drag =
                self.interaction
                    .active_drag(session)
                    .map_err(|_| EngineError::Interaction {
                        input,
                        source: InteractionCounterError::StateInvariant,
                    })?;
            (
                drag.pointer,
                drag.payload.clone(),
                drag.target.clone(),
                drag.tear_off.clone(),
            )
        };
        let Some(target) = target else {
            self.interaction
                .clear_preview(session)
                .map_err(|_| EngineError::Interaction {
                    input,
                    source: InteractionCounterError::StateInvariant,
                })?;
            self.interaction
                .set_drop_affordance(session, None)
                .map_err(|_| EngineError::Interaction {
                    input,
                    source: InteractionCounterError::StateInvariant,
                })?;
            return Ok(());
        };
        let tear_off = tear_off.map(|request| {
            self.refresh_scene_bound_tear_off(&request)
                .unwrap_or(request)
        });
        self.interaction
            .set_drag_observation(session, target.clone(), tear_off.clone())
            .map_err(|_| EngineError::Interaction {
                input,
                source: InteractionCounterError::StateInvariant,
            })?;
        let evaluation = self.resolve_preview_evaluation(
            input,
            session,
            pointer,
            &payload,
            &target,
            tear_off.as_ref(),
        )?;
        let _ = self.apply_preview_evaluation(input, session, evaluation, interaction_events)?;
        Ok(())
    }

    fn refresh_core_drag_preview(
        &mut self,
        input: InputSequence,
        session: crate::interaction::DragSessionId,
        interaction_events: &mut Vec<InteractionEvent>,
    ) -> Result<(), EngineError> {
        if !self.core_drag_source_is_current(session, input)? {
            let _ = self.cancel_drag(
                input,
                session,
                InteractionCancelReason::SourceVanished,
                interaction_events,
            )?;
            return Ok(());
        }
        let drag = self
            .interaction
            .active_drag(session)
            .map_err(|_| EngineError::Interaction {
                input,
                source: InteractionCounterError::StateInvariant,
            })?;
        let (Some(target), Some(current_pointer)) =
            (drag.target.as_ref(), drag.current_pointer.as_ref())
        else {
            self.interaction
                .clear_preview(session)
                .map_err(|_| EngineError::Interaction {
                    input,
                    source: InteractionCounterError::StateInvariant,
                })?;
            self.interaction
                .set_drop_affordance(session, None)
                .map_err(|_| EngineError::Interaction {
                    input,
                    source: InteractionCounterError::StateInvariant,
                })?;
            return Ok(());
        };
        let evaluation =
            self.resolve_core_preview_evaluation(input, drag, target, current_pointer)?;
        let _ = self.apply_preview_evaluation(input, session, evaluation, interaction_events)?;
        Ok(())
    }

    fn refresh_scene_bound_tear_off(&self, request: &TearOffRequest) -> Option<TearOffRequest> {
        match request {
            TearOffRequest::Contained(proposal) => self
                .refresh_contained_proposal(*proposal)
                .map(TearOffRequest::Contained),
            TearOffRequest::Native {
                proposal,
                contained_fallback,
            } => {
                let recovery = self.refresh_contained_proposal(proposal.recovery())?;
                let contained_fallback = match contained_fallback {
                    Some(fallback) => Some(self.refresh_contained_proposal(*fallback)?),
                    None => None,
                };
                Some(TearOffRequest::native(
                    crate::intent::NativeTearOffProposal::new(
                        proposal.surface(),
                        proposal.root(),
                        *proposal.placement(),
                        recovery,
                    ),
                    contained_fallback,
                ))
            }
        }
    }

    fn refresh_contained_proposal(
        &self,
        proposal: crate::intent::ContainedTearOffProposal,
    ) -> Option<crate::intent::ContainedTearOffProposal> {
        let prior = proposal.placement();
        let placement = self
            .contained_placement(
                proposal.surface(),
                prior.requested_rect(),
                prior.minimum_size(),
            )
            .ok()?;
        Some(crate::intent::ContainedTearOffProposal::new(
            proposal.root(),
            proposal.floating(),
            placement,
            proposal.z_order(),
        ))
    }

    fn freeze_payload_focus(&self, payload: &MovePayload) -> PanelFocus {
        if self.validate_payload(payload).is_err() {
            return PanelFocus::None;
        }
        let Some(surface) = self.payload_surface(payload) else {
            return PanelFocus::None;
        };
        let PanelFocusRecord::Item(item) = self.viewport_focus.panel_focus(surface) else {
            return PanelFocus::None;
        };
        let payload_contains_item = match payload {
            MovePayload::Item(source) => source.item() == item,
            MovePayload::Tabs(source) | MovePayload::Subtree(source) => self
                .workspace
                .collect_items_in_subtree(source.node())
                .contains(&item),
        };
        if payload_contains_item {
            PanelFocus::Item(item)
        } else {
            PanelFocus::None
        }
    }

    fn validate_payload(&self, payload: &MovePayload) -> Result<(), crate::error::CommandError> {
        use crate::error::ReferenceRole;

        match payload {
            MovePayload::Item(source) => {
                self.workspace.verify_reference(
                    source.root(),
                    source.tabs(),
                    source.fingerprint(),
                    ReferenceRole::Source,
                )?;
                self.workspace
                    .capture_item_source(source.root(), source.tabs(), source.item())?;
            }
            MovePayload::Tabs(source) => {
                self.validate_node_source(source)?;
                match self.workspace.node(source.node()) {
                    Some(crate::graph::Node::Tabs { items, .. }) if !items.is_empty() => {}
                    Some(crate::graph::Node::Tabs { .. }) => {
                        return Err(crate::error::CommandError::EmptyPayload {
                            node: source.node(),
                        });
                    }
                    Some(crate::graph::Node::Split { .. }) => {
                        return Err(crate::error::CommandError::NodeIsNotTabs {
                            node: source.node(),
                        });
                    }
                    None => {
                        return Err(crate::error::CommandError::MissingNode {
                            role: ReferenceRole::Source,
                            node: source.node(),
                        });
                    }
                }
            }
            MovePayload::Subtree(source) => {
                self.validate_node_source(source)?;
                if self
                    .workspace
                    .collect_items_in_subtree(source.node())
                    .is_empty()
                {
                    return Err(crate::error::CommandError::EmptyPayload {
                        node: source.node(),
                    });
                }
            }
        }
        Ok(())
    }

    fn validate_node_source(
        &self,
        source: &crate::command::NodeSource,
    ) -> Result<(), crate::error::CommandError> {
        use crate::error::ReferenceRole;

        self.workspace.verify_reference(
            source.root(),
            source.node(),
            source.fingerprint(),
            ReferenceRole::Source,
        )
    }

    fn tear_off_command(
        &self,
        input: InputSequence,
        payload: &MovePayload,
        target: RootPresentationTarget,
        new_root: crate::ids::RootId,
    ) -> Result<WorkspaceCommand, EngineError> {
        let complete_root =
            self.complete_root_source(payload)
                .map_err(|_| EngineError::Interaction {
                    input,
                    source: InteractionCounterError::StateInvariant,
                })?;
        Ok(Self::tear_off_command_from_complete_root(
            payload,
            target,
            new_root,
            complete_root,
        ))
    }

    fn tear_off_command_from_complete_root(
        payload: &MovePayload,
        target: RootPresentationTarget,
        new_root: crate::ids::RootId,
        complete_root: Option<NodeSource>,
    ) -> WorkspaceCommand {
        match (complete_root, target) {
            (Some(source), target) => WorkspaceCommand::RehomeRoot { source, target },
            (None, RootPresentationTarget::Surface { surface }) => {
                WorkspaceCommand::CreateSurfaceRoot {
                    surface,
                    root: new_root,
                    content: RootContent::Move(payload.clone()),
                }
            }
            (
                None,
                RootPresentationTarget::Contained {
                    surface,
                    floating,
                    rect,
                    z_order,
                },
            ) => WorkspaceCommand::CreateContainedRoot {
                surface,
                root: new_root,
                floating,
                rect,
                z_order,
                content: RootContent::Move(payload.clone()),
            },
        }
    }

    fn contained_presentation_command(
        &self,
        payload: &MovePayload,
        proposal: crate::intent::ContainedTearOffProposal,
        complete_root: Option<NodeSource>,
    ) -> WorkspaceCommand {
        if let Some(source) = complete_root.as_ref()
            && let Some(current) = self.workspace.contained_floating(proposal.floating())
            && current.root == source.root()
            && current.surface == proposal.surface()
            && current.z_order == proposal.z_order()
        {
            return WorkspaceCommand::UpdateContainedRect {
                surface: current.surface,
                root: current.root,
                floating: current.id,
                expected_rect: current.rect,
                rect: proposal.rect(),
            };
        }

        Self::tear_off_command_from_complete_root(
            payload,
            RootPresentationTarget::Contained {
                surface: proposal.surface(),
                floating: proposal.floating(),
                rect: proposal.rect(),
                z_order: proposal.z_order(),
            },
            proposal.root(),
            complete_root,
        )
    }

    fn same_contained_move_command_for_valid_payload(
        &self,
        payload: &MovePayload,
        proposal: crate::intent::ContainedTearOffProposal,
    ) -> Option<WorkspaceCommand> {
        let source = match payload {
            MovePayload::Tabs(source) | MovePayload::Subtree(source) => source,
            MovePayload::Item(_) => return None,
        };
        let root = self.workspace.root(source.root())?;
        if source.node() != root.node || source.root() != proposal.root() {
            return None;
        }
        let current = self.workspace.contained_floating(proposal.floating())?;
        if current.root != source.root()
            || current.surface != proposal.surface()
            || current.z_order != proposal.z_order()
        {
            return None;
        }
        Some(WorkspaceCommand::UpdateContainedRect {
            surface: current.surface,
            root: current.root,
            floating: current.id,
            expected_rect: current.rect,
            rect: proposal.rect(),
        })
    }

    fn tear_off_root_identity_matches(
        &self,
        input: InputSequence,
        payload: &MovePayload,
        requested: crate::ids::RootId,
    ) -> Result<bool, EngineError> {
        Ok(self
            .complete_root_source(payload)
            .map_err(|_| EngineError::Interaction {
                input,
                source: InteractionCounterError::StateInvariant,
            })?
            .is_none_or(|source| source.root() == requested))
    }

    fn complete_root_source(
        &self,
        payload: &MovePayload,
    ) -> Result<Option<crate::command::NodeSource>, crate::error::CommandError> {
        let root = match payload {
            MovePayload::Item(source) => source.root(),
            MovePayload::Tabs(source) | MovePayload::Subtree(source) => source.root(),
        };
        let record = self
            .workspace
            .root(root)
            .ok_or(crate::error::CommandError::MissingRoot { root })?;
        let complete = match payload {
            MovePayload::Item(_) => {
                record.central.is_none()
                    && self.workspace.collect_items_in_subtree(record.node).len() == 1
            }
            MovePayload::Tabs(source) | MovePayload::Subtree(source) => {
                source.node() == record.node
            }
        };
        if complete {
            self.workspace
                .capture_node_source(root, record.node)
                .map(Some)
        } else {
            Ok(None)
        }
    }

    fn payload_surface(&self, payload: &MovePayload) -> Option<crate::ids::SurfaceId> {
        let root = match payload {
            MovePayload::Item(source) => source.root(),
            MovePayload::Tabs(source) | MovePayload::Subtree(source) => source.root(),
        };
        self.root_surface(root)
    }

    fn root_surface(&self, root: crate::ids::RootId) -> Option<crate::ids::SurfaceId> {
        match self.workspace.presentation_for_root(root)? {
            crate::RootPresentationOwner::Main { surface }
            | crate::RootPresentationOwner::Contained { surface, .. } => Some(surface),
        }
    }

    fn preflight_command(
        &self,
        input: InputSequence,
        command: &WorkspaceCommand,
    ) -> Result<CommandApplication, EngineError> {
        self.stage_workspace_command(input, &self.policy, command, None)
            .map(|(_, application)| application)
    }

    fn apply_interaction_command(
        &mut self,
        input: InputSequence,
        command: &WorkspaceCommand,
        events: &mut Vec<WorkspaceEvent>,
    ) -> Result<CommandApplication, EngineError> {
        self.apply_interaction_command_with_barrier(input, command, None, events)
    }

    fn apply_interaction_command_with_barrier(
        &mut self,
        input: InputSequence,
        command: &WorkspaceCommand,
        action_barrier: Option<&BTreeMap<crate::ids::SurfaceId, SurfaceRosterDisposition>>,
        events: &mut Vec<WorkspaceEvent>,
    ) -> Result<CommandApplication, EngineError> {
        let (candidate, application) =
            self.stage_workspace_command(input, &self.policy, command, action_barrier)?;
        if let Some(candidate) = candidate {
            self.workspace = candidate;
            self.reconcile_viewport_focus_authority();
        }
        self.record_interaction_command_application(input, application, events)
    }

    fn apply_existing_contained_rect_update(
        &mut self,
        input: InputSequence,
        command: &WorkspaceCommand,
        events: &mut Vec<WorkspaceEvent>,
    ) -> Result<CommandApplication, EngineError> {
        if !matches!(command, WorkspaceCommand::UpdateContainedRect { .. }) {
            return Err(EngineError::Interaction {
                input,
                source: InteractionCounterError::StateInvariant,
            });
        }
        let mut policy = self.policy.clone();
        policy.set_allow_contained_floating(true);
        let (candidate, application) =
            self.stage_workspace_command(input, &policy, command, None)?;
        if let Some(candidate) = candidate {
            self.workspace = candidate;
        }
        self.record_interaction_command_application(input, application, events)
    }

    fn record_interaction_command_application(
        &mut self,
        input: InputSequence,
        application: CommandApplication,
        events: &mut Vec<WorkspaceEvent>,
    ) -> Result<CommandApplication, EngineError> {
        if let CommandApplication::Applied { outcome, changed } = &application
            && *changed
        {
            self.advance_revision(input)?;
            self.invalidate_scene();
            events.push(WorkspaceEvent::new(
                input,
                self.version,
                WorkspaceEventKind::CommandCommitted(outcome.clone()),
            ));
        }
        Ok(application)
    }

    fn invalidate_scene(&mut self) {
        self.scene = None;
        self.scene_coordinate_authority.clear();
    }

    fn stage_workspace_command(
        &self,
        input: InputSequence,
        policy: &DockPolicy,
        command: &WorkspaceCommand,
        action_barrier: Option<&BTreeMap<crate::ids::SurfaceId, SurfaceRosterDisposition>>,
    ) -> Result<(Option<Workspace>, CommandApplication), EngineError> {
        let mut candidate = self.workspace.clone();
        let report = match WorkspaceTransaction::from_commands([command.clone()])
            .apply(&mut candidate, policy)
        {
            Ok(report) => report,
            Err(TransactionError::Command { index: 0, source })
                if source.is_expected_rejection() =>
            {
                return Ok((None, CommandApplication::Rejected(source)));
            }
            Err(source) => return Err(EngineError::Command { input, source }),
        };
        if let Some(surface) =
            self.first_workspace_publication_mismatch(&candidate, action_barrier, None)
        {
            return Ok((
                None,
                CommandApplication::Rejected(crate::error::CommandError::SurfaceLifecycleFrozen {
                    surface,
                }),
            ));
        }
        let application = Self::command_application_from_report(input, report)?;
        Ok((Some(candidate), application))
    }

    fn first_workspace_publication_mismatch(
        &self,
        candidate: &Workspace,
        action_barrier: Option<&BTreeMap<crate::ids::SurfaceId, SurfaceRosterDisposition>>,
        excluded: Option<crate::ids::SurfaceId>,
    ) -> Option<crate::ids::SurfaceId> {
        let persistent = match excluded {
            Some(surface) => self
                .surface_recovery
                .first_workspace_mismatch_excluding(candidate, surface),
            None => self.surface_recovery.first_workspace_mismatch(candidate),
        };
        persistent.or_else(|| {
            action_barrier?.values().find_map(|roster| {
                (Some(roster.surface()) != excluded && !roster.matches_workspace(candidate))
                    .then_some(roster.surface())
            })
        })
    }

    fn command_application_from_report(
        input: InputSequence,
        report: crate::transaction::TransactionReport,
    ) -> Result<CommandApplication, EngineError> {
        let changed = report.changed();
        let outcome = report
            .into_outcomes()
            .into_iter()
            .next()
            .ok_or(EngineError::MissingCommandOutcome { input })?;
        Ok(CommandApplication::Applied { outcome, changed })
    }

    fn reduce_workspace_command(
        &mut self,
        input: InputSequence,
        expected: WorkspaceVersion,
        application_base: WorkspaceVersion,
        command: &WorkspaceCommand,
        events: &mut Vec<WorkspaceEvent>,
        interaction_events: &mut Vec<InteractionEvent>,
    ) -> Result<InputOutcome, EngineError> {
        if expected != application_base {
            return Ok(InputOutcome::StaleRejected {
                expected,
                accepted_base: application_base,
            });
        }

        let (candidate, application) =
            self.stage_workspace_command(input, &self.policy, command, None)?;
        let CommandApplication::Applied { outcome, changed } = application else {
            let CommandApplication::Rejected(error) = application else {
                unreachable!("command application variants are exhaustive")
            };
            return Ok(InputOutcome::CommandRejected {
                error,
                version: self.version,
            });
        };
        self.workspace = candidate.ok_or(EngineError::MissingCommandOutcome { input })?;
        self.reconcile_viewport_focus_authority();
        if changed {
            self.advance_revision(input)?;
            self.invalidate_transient(
                input,
                InteractionCancelReason::WorkspaceChanged,
                interaction_events,
            )?;
        }
        if changed {
            events.push(WorkspaceEvent::new(
                input,
                self.version,
                WorkspaceEventKind::CommandCommitted(outcome.clone()),
            ));
        }
        Ok(InputOutcome::CommandProcessed {
            outcome,
            changed,
            version: self.version,
        })
    }

    fn candidate(&self) -> Self {
        Self {
            workspace: self.workspace.clone(),
            policy: self.policy.clone(),
            version: self.version,
            scene: self.scene.clone(),
            scene_coordinate_authority: self.scene_coordinate_authority.clone(),
            last_scene_generation: self.last_scene_generation,
            interaction: self.interaction.clone(),
            viewport: self.viewport.clone(),
            viewport_focus: self.viewport_focus.clone(),
            last_focus_reducer_generation: self.last_focus_reducer_generation,
            surface_recovery: self.surface_recovery.clone(),
            last_input: self.last_input,
            pending: self.pending.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::DockTarget;
    use crate::drop_target::{DropTargetAvailability, DropTargetId, DropTargetRecord, DropVisual};
    use crate::geometry::{LogicalPoint, LogicalRect, LogicalSize};
    use crate::graph::{Axis, ContainedFloating, Node, RootRecord, SurfacePresentation};
    use crate::hit_region::HitRegion;
    use crate::ids::{FloatingPresentationId, ItemId, RootId, SurfaceId, WorkspaceEpoch};
    use crate::intent::{
        Authority, ContainedTearOffProposal, DragOrigin, PointerButton, PointerId, RendererIntent,
        SurfacePointer,
    };
    use crate::interaction::{DragSessionId, InteractionOutcome, InteractionStatus};
    use crate::platform::{
        ObservedWindow, PlatformCapabilities, PlatformCapability, WindowPresentationState,
    };
    use crate::scene::{NodeSceneId, ReadySurfaceScene, SceneLayerKey, SemanticRect};
    use crate::transition::InputOutcome;
    use crate::viewport::{ViewportBinding, ViewportRole, WindowIncarnation, WindowToken};
    use crate::viewport_focus::{
        FocusObservationEnvelope, FocusObservationGeneration, GlobalFocusedWindow,
        PaneFocusIntentGeneration, PaneFocusObservationGeneration, PaneFocusObservationTransition,
        PanelFocusRecord, ViewportActivationRequest,
    };

    const SOURCE_ROOT: RootId = RootId::new(1);
    const TARGET_ROOT: RootId = RootId::new(2);
    const SOURCE_SURFACE: SurfaceId = SurfaceId::new(1);
    const TARGET_SURFACE: SurfaceId = SurfaceId::new(2);
    const TEST_POINTER: PointerId = PointerId::new(1);

    struct CounterFixture {
        engine: DockEngine,
        source_tabs: crate::ids::NodeId,
        target_tabs: crate::ids::NodeId,
    }

    fn test_rect() -> LogicalRect {
        LogicalRect::new(0.0, 0.0, 100.0, 100.0).expect("test rectangle must be valid")
    }

    fn counter_fixture() -> CounterFixture {
        let mut builder = Workspace::builder();
        let source_tabs = builder.insert_node(Node::tabs([ItemId::new(1)]));
        let target_tabs = builder.insert_node(Node::tabs([ItemId::new(2)]));
        builder.set_root(SOURCE_ROOT, RootRecord::new(source_tabs));
        builder.set_root(TARGET_ROOT, RootRecord::new(target_tabs));
        builder.set_surface(SOURCE_SURFACE, SurfacePresentation::new(SOURCE_ROOT));
        builder.set_surface(TARGET_SURFACE, SurfacePresentation::new(TARGET_ROOT));
        let workspace = builder.build().expect("counter workspace must be valid");
        CounterFixture {
            engine: DockEngine::new(workspace, DockPolicy::default())
                .expect("counter engine must be valid"),
            source_tabs,
            target_tabs,
        }
    }

    #[test]
    fn stale_cleanup_retry_version_is_rejected_without_effects_or_state_change() {
        let mut fixture = counter_fixture();
        let stale = fixture.engine.version();
        fixture
            .engine
            .enqueue_workspace_replacement(fixture.engine.workspace().clone())
            .expect("workspace replacement must enqueue");
        fixture
            .engine
            .reduce_pending()
            .expect("workspace replacement must advance the epoch");
        assert_ne!(fixture.engine.version(), stale);

        let before = fixture.engine.candidate();
        let effect_count = fixture.engine.viewport.effects().records().count();
        let outcome = fixture
            .engine
            .reduce_viewport_cleanup_retry(
                InputSequence::new(900),
                stale,
                crate::effect::EffectId::new(901),
            )
            .expect("stale retry must be a typed nonfatal rejection");

        assert_eq!(
            outcome,
            InputOutcome::StaleRejected {
                expected: stale,
                accepted_base: fixture.engine.version(),
            }
        );
        assert_eq!(fixture.engine, before);
        assert_eq!(
            fixture.engine.viewport.effects().records().count(),
            effect_count
        );
    }

    struct FocusRevealFixture {
        engine: DockEngine,
        tabs: crate::ids::NodeId,
        binding: ViewportBinding,
        focus_generation: u64,
    }

    fn focus_reveal_fixture() -> FocusRevealFixture {
        let mut builder = Workspace::builder();
        let tabs = builder.insert_node(Node::tabs([ItemId::new(1), ItemId::new(2)]));
        builder.set_root(SOURCE_ROOT, RootRecord::new(tabs));
        builder.set_surface(SOURCE_SURFACE, SurfacePresentation::new(SOURCE_ROOT));
        let workspace = builder
            .build()
            .expect("focus reveal workspace must be valid");
        let mut engine =
            DockEngine::new(workspace, DockPolicy::default()).expect("engine must be valid");
        let token = WindowToken::new(1);
        engine
            .enqueue_viewport_registration(SOURCE_SURFACE, token, ViewportRole::Root, None)
            .expect("viewport registration must enqueue");
        let registered = engine
            .reduce_pending()
            .expect("viewport registration must reduce");
        let InputOutcome::ViewportRegistered { binding } = registered.reduced_inputs()[0].outcome()
        else {
            panic!("viewport registration must publish its exact binding");
        };
        let binding = *binding;
        let mut fixture = FocusRevealFixture {
            engine,
            tabs,
            binding,
            focus_generation: 0,
        };
        publish_focus_snapshot(
            &mut fixture,
            Authority::Known(GlobalFocusedWindow::Dock(binding)),
        );
        fixture
    }

    fn publish_focus_snapshot(
        fixture: &mut FocusRevealFixture,
        focused: Authority<GlobalFocusedWindow>,
    ) -> EngineTransition {
        fixture.focus_generation += 1;
        let mut capabilities = PlatformCapabilities::default();
        capabilities.set_authoritative_inventory(PlatformCapability::Supported);
        capabilities.set_global_focus_observation(PlatformCapability::Supported);
        capabilities.set_window_activation_control(PlatformCapability::Supported);
        let snapshot = PlatformSnapshot::new(
            capabilities,
            FocusObservationEnvelope::new(
                FocusObservationGeneration::new(fixture.focus_generation),
                focused,
                Authority::Known(None),
            ),
            vec![
                ObservedWindow::new(fixture.binding.token())
                    .with_presentation(Authority::Known(WindowPresentationState::Visible)),
            ],
            Vec::new(),
            Vec::new(),
        )
        .expect("focus snapshot must be canonical");
        fixture
            .engine
            .enqueue_platform_snapshot(snapshot)
            .expect("focus snapshot must enqueue");
        fixture
            .engine
            .reduce_pending()
            .expect("focus snapshot must reduce")
    }

    fn selected_item(fixture: &FocusRevealFixture) -> Option<ItemId> {
        let Node::Tabs { selected, .. } = fixture
            .engine
            .workspace()
            .node(fixture.tabs)
            .expect("focus tabs must remain current")
        else {
            panic!("focus fixture node must remain tabs");
        };
        *selected
    }

    struct FocusEffectFixture {
        engine: DockEngine,
        binding_a: ViewportBinding,
        binding_b: ViewportBinding,
        focus_generation: u64,
    }

    fn focus_effect_fixture() -> FocusEffectFixture {
        let mut builder = Workspace::builder();
        let tabs_a = builder.insert_node(Node::tabs([ItemId::new(1)]));
        let tabs_b = builder.insert_node(Node::tabs([ItemId::new(2)]));
        builder.set_root(SOURCE_ROOT, RootRecord::new(tabs_a));
        builder.set_root(TARGET_ROOT, RootRecord::new(tabs_b));
        builder.set_surface(SOURCE_SURFACE, SurfacePresentation::new(SOURCE_ROOT));
        builder.set_surface(TARGET_SURFACE, SurfacePresentation::new(TARGET_ROOT));
        let workspace = builder
            .build()
            .expect("focus effect workspace must be valid");
        let mut engine =
            DockEngine::new(workspace, DockPolicy::default()).expect("engine must be valid");
        engine
            .enqueue_viewport_registration(
                SOURCE_SURFACE,
                WindowToken::new(1),
                ViewportRole::Root,
                None,
            )
            .expect("first viewport registration must enqueue");
        engine
            .enqueue_viewport_registration(
                TARGET_SURFACE,
                WindowToken::new(2),
                ViewportRole::Root,
                None,
            )
            .expect("second viewport registration must enqueue");
        engine
            .reduce_pending()
            .expect("viewport registrations must reduce");
        let binding_a = engine
            .viewport()
            .viewport(SOURCE_SURFACE)
            .expect("first viewport must be current")
            .binding();
        let binding_b = engine
            .viewport()
            .viewport(TARGET_SURFACE)
            .expect("second viewport must be current")
            .binding();
        let mut fixture = FocusEffectFixture {
            engine,
            binding_a,
            binding_b,
            focus_generation: 0,
        };
        publish_effect_focus_snapshot(
            &mut fixture,
            Authority::Known(GlobalFocusedWindow::Foreign),
            Authority::Known(None),
        );
        fixture
    }

    fn publish_effect_focus_snapshot(
        fixture: &mut FocusEffectFixture,
        focused: Authority<GlobalFocusedWindow>,
        acknowledged_effect: Authority<Option<crate::effect::EffectId>>,
    ) -> EngineTransition {
        fixture.focus_generation += 1;
        publish_effect_focus_snapshot_at(
            fixture,
            fixture.focus_generation,
            focused,
            acknowledged_effect,
        )
    }

    fn publish_effect_focus_snapshot_at(
        fixture: &mut FocusEffectFixture,
        generation: u64,
        focused: Authority<GlobalFocusedWindow>,
        acknowledged_effect: Authority<Option<crate::effect::EffectId>>,
    ) -> EngineTransition {
        let mut capabilities = PlatformCapabilities::default();
        capabilities.set_authoritative_inventory(PlatformCapability::Supported);
        capabilities.set_global_focus_observation(PlatformCapability::Supported);
        capabilities.set_window_activation_control(PlatformCapability::Supported);
        let snapshot = PlatformSnapshot::new(
            capabilities,
            FocusObservationEnvelope::new(
                FocusObservationGeneration::new(generation),
                focused,
                acknowledged_effect,
            ),
            vec![
                ObservedWindow::new(fixture.binding_a.token())
                    .with_presentation(Authority::Known(WindowPresentationState::Visible)),
                ObservedWindow::new(fixture.binding_b.token())
                    .with_presentation(Authority::Known(WindowPresentationState::Visible)),
            ],
            Vec::new(),
            Vec::new(),
        )
        .expect("focus effect snapshot must be canonical");
        fixture
            .engine
            .enqueue_platform_snapshot(snapshot)
            .expect("focus effect snapshot must enqueue");
        fixture
            .engine
            .reduce_pending()
            .expect("focus effect snapshot must reduce")
    }

    fn request_focus_effect(
        fixture: &mut FocusEffectFixture,
        binding: ViewportBinding,
        focus: PanelFocus,
    ) -> (
        crate::effect::EffectId,
        crate::viewport_focus::ActivationGeneration,
    ) {
        fixture
            .engine
            .enqueue_viewport_activation(binding, focus)
            .expect("activation must enqueue");
        let transition = fixture
            .engine
            .reduce_pending()
            .expect("activation must reduce");
        let activation = transition
            .reduced_inputs()
            .iter()
            .find_map(|input| match input.outcome() {
                InputOutcome::ViewportActivationRequested { activation } => Some(*activation),
                _ => None,
            })
            .expect("activation input must publish its generation");
        let effect = transition
            .platform_effects()
            .iter()
            .find_map(|request| match request.effect() {
                crate::effect::PlatformEffect::RequestFocus {
                    binding: requested, ..
                } if *requested == binding => Some(request.id()),
                _ => None,
            })
            .expect("activation must emit one exact focus effect");
        (effect, activation.generation())
    }

    fn observed_focus_effect_ids(transition: &EngineTransition) -> Vec<crate::effect::EffectId> {
        transition
            .focus_delta()
            .effects()
            .iter()
            .filter_map(|change| {
                change
                    .observed()
                    .map(crate::viewport_focus::ObservedPlatformFocusEffect::effect)
            })
            .collect()
    }

    fn assert_focus_effect_observed(engine: &DockEngine, effect: crate::effect::EffectId) {
        assert!(matches!(
            engine
                .viewport()
                .effects()
                .record(effect)
                .map(crate::effect::EffectRecord::phase),
            Some(crate::effect::EffectPhase::ObservedApplied { .. })
        ));
    }

    #[test]
    fn late_superseded_focus_ack_settles_only_its_effect_before_successor_completion() {
        let mut fixture = focus_effect_fixture();
        let binding_a = fixture.binding_a;
        let binding_b = fixture.binding_b;
        let (effect_a, activation_a) =
            request_focus_effect(&mut fixture, binding_a, PanelFocus::Item(ItemId::new(1)));
        let (effect_b, activation_b) =
            request_focus_effect(&mut fixture, binding_b, PanelFocus::Item(ItemId::new(2)));

        let late_a = publish_effect_focus_snapshot(
            &mut fixture,
            Authority::Known(GlobalFocusedWindow::Foreign),
            Authority::Known(Some(effect_a)),
        );
        assert_eq!(observed_focus_effect_ids(&late_a), vec![effect_a]);
        assert_focus_effect_observed(&fixture.engine, effect_a);
        assert_eq!(
            fixture
                .engine
                .viewport_focus()
                .pending_activation()
                .map(crate::viewport_focus::PendingViewportActivation::generation),
            Some(activation_b),
            "late predecessor acknowledgement must not complete or cancel the successor"
        );
        assert!(
            fixture
                .engine
                .viewport_focus()
                .pending_pane_intent()
                .is_none()
        );

        let completed_b = publish_effect_focus_snapshot(
            &mut fixture,
            Authority::Known(GlobalFocusedWindow::Dock(binding_b)),
            Authority::Known(Some(effect_b)),
        );
        assert_eq!(observed_focus_effect_ids(&completed_b), vec![effect_b]);
        assert_focus_effect_observed(&fixture.engine, effect_b);
        assert!(
            fixture
                .engine
                .viewport_focus()
                .pending_activation()
                .is_none()
        );
        let intent = fixture
            .engine
            .viewport_focus()
            .pending_pane_intent()
            .expect("successor target observation must install its pane intent");
        assert_eq!(intent.activation(), Some(activation_b));
        assert_eq!(intent.target(), binding_b);
        assert_eq!(intent.focus(), PanelFocus::Item(ItemId::new(2)));
        assert_ne!(intent.activation(), Some(activation_a));
    }

    #[test]
    fn one_envelope_can_settle_late_predecessor_and_complete_current_target() {
        let mut fixture = focus_effect_fixture();
        let binding_a = fixture.binding_a;
        let binding_b = fixture.binding_b;
        let (effect_a, _) =
            request_focus_effect(&mut fixture, binding_a, PanelFocus::Item(ItemId::new(1)));
        let (effect_b, activation_b) =
            request_focus_effect(&mut fixture, binding_b, PanelFocus::Item(ItemId::new(2)));

        let transition = publish_effect_focus_snapshot(
            &mut fixture,
            Authority::Known(GlobalFocusedWindow::Dock(binding_b)),
            Authority::Known(Some(effect_a)),
        );
        assert_eq!(
            observed_focus_effect_ids(&transition),
            vec![effect_a, effect_b],
            "FocusDelta must retain both exact-A and target-B settlements in effect order"
        );
        assert_focus_effect_observed(&fixture.engine, effect_a);
        assert_focus_effect_observed(&fixture.engine, effect_b);
        assert_eq!(
            transition.focus_delta().effects()[0]
                .observed()
                .map(crate::viewport_focus::ObservedPlatformFocusEffect::evidence),
            Some(crate::viewport_focus::PlatformFocusEvidence::ExactEffectAcknowledgement)
        );
        assert_eq!(
            transition.focus_delta().effects()[1]
                .observed()
                .map(crate::viewport_focus::ObservedPlatformFocusEffect::evidence),
            Some(crate::viewport_focus::PlatformFocusEvidence::NewerMatchingObservation)
        );
        let intent = fixture
            .engine
            .viewport_focus()
            .pending_pane_intent()
            .expect("current target evidence must complete the successor");
        assert_eq!(intent.activation(), Some(activation_b));
        assert_eq!(intent.target(), binding_b);
    }

    #[test]
    fn wrong_stale_incarnation_and_duplicate_focus_acks_do_not_settle_again() {
        let mut fixture = focus_effect_fixture();
        let binding_a = fixture.binding_a;
        let binding_b = fixture.binding_b;
        let (effect_a, _) = request_focus_effect(&mut fixture, binding_a, PanelFocus::None);
        let (_, activation_b) = request_focus_effect(&mut fixture, binding_b, PanelFocus::None);

        let wrong = publish_effect_focus_snapshot(
            &mut fixture,
            Authority::Known(GlobalFocusedWindow::Foreign),
            Authority::Known(Some(crate::effect::EffectId::new(9_999))),
        );
        assert!(observed_focus_effect_ids(&wrong).is_empty());
        assert_eq!(
            fixture
                .engine
                .viewport()
                .effects()
                .record(effect_a)
                .map(crate::effect::EffectRecord::phase),
            Some(crate::effect::EffectPhase::Requested)
        );

        let stale_generation = fixture.focus_generation;
        let stale = publish_effect_focus_snapshot_at(
            &mut fixture,
            stale_generation,
            Authority::Known(GlobalFocusedWindow::Foreign),
            Authority::Known(Some(effect_a)),
        );
        assert!(observed_focus_effect_ids(&stale).is_empty());
        assert_eq!(
            fixture
                .engine
                .viewport()
                .effects()
                .record(effect_a)
                .map(crate::effect::EffectRecord::phase),
            Some(crate::effect::EffectPhase::Requested)
        );

        let first = publish_effect_focus_snapshot(
            &mut fixture,
            Authority::Known(GlobalFocusedWindow::Foreign),
            Authority::Known(Some(effect_a)),
        );
        assert_eq!(observed_focus_effect_ids(&first), vec![effect_a]);
        let duplicate = publish_effect_focus_snapshot(
            &mut fixture,
            Authority::Known(GlobalFocusedWindow::Foreign),
            Authority::Known(Some(effect_a)),
        );
        assert!(observed_focus_effect_ids(&duplicate).is_empty());
        assert_eq!(
            fixture
                .engine
                .viewport_focus()
                .pending_activation()
                .map(crate::viewport_focus::PendingViewportActivation::generation),
            Some(activation_b)
        );

        let stale_binding = ViewportBinding::new(
            binding_a.epoch(),
            binding_a.surface(),
            binding_a.token(),
            WindowIncarnation::new(binding_a.incarnation().get() + 1),
        );
        let stale_effect = fixture
            .engine
            .viewport
            .request_focus_binding(stale_binding)
            .expect("stale-incarnation effect must allocate for the invariant test");
        let stale_incarnation = publish_effect_focus_snapshot(
            &mut fixture,
            Authority::Known(GlobalFocusedWindow::Foreign),
            Authority::Known(Some(stale_effect)),
        );
        assert!(observed_focus_effect_ids(&stale_incarnation).is_empty());
        assert_eq!(
            fixture
                .engine
                .viewport()
                .effects()
                .record(stale_effect)
                .map(crate::effect::EffectRecord::phase),
            Some(crate::effect::EffectPhase::Requested)
        );
    }

    #[test]
    fn explicit_focus_atomically_reveals_hidden_item_without_acknowledging_it() {
        let mut fixture = focus_reveal_fixture();
        assert_eq!(selected_item(&fixture), Some(ItemId::new(1)));

        fixture
            .engine
            .enqueue_viewport_activation(fixture.binding, PanelFocus::Item(ItemId::new(2)))
            .expect("explicit activation must enqueue");
        let transition = fixture
            .engine
            .reduce_pending()
            .expect("explicit activation must reduce");

        assert_eq!(selected_item(&fixture), Some(ItemId::new(2)));
        assert!(
            fixture
                .engine
                .viewport_focus()
                .pending_pane_intent()
                .is_some()
        );
        assert_eq!(
            fixture.engine.viewport_focus().panel_focus(SOURCE_SURFACE),
            PanelFocusRecord::NoHistory,
            "selection and pane rendering cannot acknowledge focus"
        );
        assert!(transition.focus_delta().pane_intent().is_some());
    }

    #[test]
    fn platform_restore_reveals_the_exact_hidden_focus_history_item() {
        let mut fixture = focus_reveal_fixture();
        fixture
            .engine
            .enqueue_pane_focus_observation(PaneFocusObservation::new(
                PaneFocusObservationGeneration::new(1),
                fixture.binding,
                PanelFocus::Item(ItemId::new(2)),
            ))
            .expect("pane focus history must enqueue");
        fixture
            .engine
            .reduce_pending()
            .expect("pane focus history must reduce");
        let source = fixture
            .engine
            .workspace()
            .capture_item_source(SOURCE_ROOT, fixture.tabs, ItemId::new(1))
            .expect("first item source must be current");
        fixture
            .engine
            .enqueue_command(WorkspaceCommand::Select { source })
            .expect("selection must enqueue");
        fixture
            .engine
            .reduce_pending()
            .expect("selection must reduce");
        assert_eq!(selected_item(&fixture), Some(ItemId::new(1)));

        publish_focus_snapshot(&mut fixture, Authority::Known(GlobalFocusedWindow::Foreign));
        let binding = fixture.binding;
        let restored = publish_focus_snapshot(
            &mut fixture,
            Authority::Known(GlobalFocusedWindow::Dock(binding)),
        );

        assert_eq!(selected_item(&fixture), Some(ItemId::new(2)));
        assert!(
            fixture
                .engine
                .viewport_focus()
                .pending_pane_intent()
                .is_some()
        );
        assert!(restored.focus_delta().pane_intent().is_some());
    }

    #[test]
    fn close_recovery_reveals_only_when_its_target_is_already_focused() {
        let mut focused = focus_reveal_fixture();
        let mut events = Vec::new();
        let activation = focused
            .engine
            .start_viewport_activation(
                InputSequence::new(100),
                ViewportActivationRequest::close_recovery(
                    focused.binding,
                    PanelFocus::Item(ItemId::new(2)),
                    ViewportCloseRequestId::new(1),
                ),
                PaneFocusIntentGeneration::new(100),
                &mut events,
            )
            .expect("focused close recovery must reduce");
        assert!(matches!(
            activation.outcome(),
            ActivationStartOutcome::PaneFocusReady { .. }
        ));
        assert_eq!(selected_item(&focused), Some(ItemId::new(2)));

        let mut unfocused = focus_reveal_fixture();
        publish_focus_snapshot(
            &mut unfocused,
            Authority::Known(GlobalFocusedWindow::Foreign),
        );
        let activation = unfocused
            .engine
            .start_viewport_activation(
                InputSequence::new(101),
                ViewportActivationRequest::close_recovery(
                    unfocused.binding,
                    PanelFocus::Item(ItemId::new(2)),
                    ViewportCloseRequestId::new(2),
                ),
                PaneFocusIntentGeneration::new(101),
                &mut events,
            )
            .expect("unfocused close recovery must remain observable");
        assert!(matches!(
            activation.outcome(),
            ActivationStartOutcome::ObserveOnlyRecorded { .. }
        ));
        assert_eq!(selected_item(&unfocused), Some(ItemId::new(1)));
    }

    struct PayloadFocusFixture {
        engine: DockEngine,
        binding: ViewportBinding,
        item_payload: MovePayload,
        stale_item_payload: MovePayload,
        tabs_payload: MovePayload,
        subtree_payload: MovePayload,
    }

    fn payload_focus_fixture() -> PayloadFocusFixture {
        let mut builder = Workspace::builder();
        let left_tabs = builder.insert_node(Node::tabs([ItemId::new(1), ItemId::new(3)]));
        let right_tabs = builder.insert_node(Node::tabs([ItemId::new(4)]));
        let source_split = builder.insert_node(
            Node::split(
                crate::graph::Axis::Horizontal,
                [left_tabs, right_tabs],
                [0.5, 0.5],
            )
            .expect("source split must be valid"),
        );
        let target_tabs = builder.insert_node(Node::tabs([ItemId::new(2)]));
        builder.set_root(SOURCE_ROOT, RootRecord::new(source_split));
        builder.set_root(TARGET_ROOT, RootRecord::new(target_tabs));
        builder.set_surface(SOURCE_SURFACE, SurfacePresentation::new(SOURCE_ROOT));
        builder.set_surface(TARGET_SURFACE, SurfacePresentation::new(TARGET_ROOT));
        let workspace = builder
            .build()
            .expect("payload focus workspace must be valid");
        let item_payload = MovePayload::Item(
            workspace
                .capture_item_source(SOURCE_ROOT, left_tabs, ItemId::new(1))
                .expect("item payload must be current"),
        );
        let tabs_payload = MovePayload::Tabs(
            workspace
                .capture_node_source(SOURCE_ROOT, left_tabs)
                .expect("tabs payload must be current"),
        );
        let subtree_payload = MovePayload::Subtree(
            workspace
                .capture_node_source(SOURCE_ROOT, source_split)
                .expect("subtree payload must be current"),
        );
        let mut stale_item_payload = item_payload.clone();
        let wrong_fingerprint = workspace
            .capture_node_source(TARGET_ROOT, target_tabs)
            .expect("target source must be current")
            .fingerprint()
            .clone();
        let MovePayload::Item(stale_source) = &mut stale_item_payload else {
            unreachable!("fixture creates an item payload");
        };
        stale_source.fingerprint = wrong_fingerprint;

        PayloadFocusFixture {
            engine: DockEngine::new(workspace, DockPolicy::default())
                .expect("engine must be valid"),
            binding: ViewportBinding::new(
                WorkspaceEpoch::new(0),
                SOURCE_SURFACE,
                WindowToken::new(1),
                WindowIncarnation::new(1),
            ),
            item_payload,
            stale_item_payload,
            tabs_payload,
            subtree_payload,
        }
    }

    fn record_payload_focus(
        engine: &mut DockEngine,
        binding: ViewportBinding,
        observation_generation: &mut u64,
        focus: PanelFocus,
    ) {
        *observation_generation += 1;
        assert!(matches!(
            engine.viewport_focus.publish_pane_focus_observation(
                PaneFocusObservation::new(
                    PaneFocusObservationGeneration::new(*observation_generation),
                    binding,
                    focus,
                ),
                |candidate| candidate == binding,
                |surface, item| {
                    surface == SOURCE_SURFACE
                        && [ItemId::new(1), ItemId::new(3), ItemId::new(4)].contains(&item)
                },
            ),
            PaneFocusObservationTransition::Applied { .. }
        ));
    }

    #[test]
    fn payload_focus_is_frozen_only_for_an_exact_payload_member() {
        let PayloadFocusFixture {
            mut engine,
            binding,
            item_payload,
            stale_item_payload,
            tabs_payload,
            subtree_payload,
        } = payload_focus_fixture();
        let mut observation_generation = 0_u64;

        record_payload_focus(
            &mut engine,
            binding,
            &mut observation_generation,
            PanelFocus::Item(ItemId::new(1)),
        );
        assert_eq!(
            engine.freeze_payload_focus(&item_payload),
            PanelFocus::Item(ItemId::new(1))
        );
        assert_eq!(
            engine.freeze_payload_focus(&stale_item_payload),
            PanelFocus::None
        );

        record_payload_focus(
            &mut engine,
            binding,
            &mut observation_generation,
            PanelFocus::Item(ItemId::new(3)),
        );
        assert_eq!(
            engine.freeze_payload_focus(&item_payload),
            PanelFocus::None,
            "an inactive item drag must not invent pane focus"
        );
        assert_eq!(
            engine.freeze_payload_focus(&tabs_payload),
            PanelFocus::Item(ItemId::new(3))
        );

        record_payload_focus(
            &mut engine,
            binding,
            &mut observation_generation,
            PanelFocus::Item(ItemId::new(4)),
        );
        assert_eq!(
            engine.freeze_payload_focus(&tabs_payload),
            PanelFocus::None,
            "a sibling item on the same surface is outside the exact tabs payload"
        );
        assert_eq!(
            engine.freeze_payload_focus(&subtree_payload),
            PanelFocus::Item(ItemId::new(4))
        );

        record_payload_focus(
            &mut engine,
            binding,
            &mut observation_generation,
            PanelFocus::None,
        );
        assert_eq!(
            engine.freeze_payload_focus(&subtree_payload),
            PanelFocus::None
        );
    }

    #[test]
    fn private_focus_generations_do_not_publish_state_or_focus_delta() {
        let mut fixture = counter_fixture();
        let stale_binding = ViewportBinding::new(
            WorkspaceEpoch::new(0),
            SOURCE_SURFACE,
            WindowToken::new(99),
            WindowIncarnation::new(99),
        );
        fixture
            .engine
            .enqueue_viewport_activation(stale_binding, PanelFocus::None)
            .expect("suppressed activation must enqueue");
        let transition = fixture
            .engine
            .reduce_pending()
            .expect("suppressed activation must reduce");
        let InputOutcome::ViewportActivationRequested { activation } =
            transition.reduced_inputs()[0].outcome()
        else {
            panic!("explicit activation must produce an activation outcome");
        };
        assert_eq!(
            activation.outcome(),
            ActivationStartOutcome::Suppressed(
                crate::viewport_focus::ActivationSuppression::StaleBinding
            )
        );
        assert!(transition.focus_delta().is_empty());
        assert!(!transition.published_state_changed());

        fixture
            .engine
            .enqueue(EngineInput::ValidateWorkspace)
            .expect("maintenance input must enqueue");
        let maintenance = fixture
            .engine
            .reduce_pending()
            .expect("maintenance input must reduce");
        assert!(maintenance.focus_delta().is_empty());
        assert!(!maintenance.published_state_changed());
    }

    #[test]
    fn action_batch_barrier_rejects_a_create_ready_mutation_of_a_destroyed_source() {
        let mut fixture = counter_fixture();
        let before = fixture.engine.workspace().clone();
        let roster =
            SurfaceRosterDisposition::capture(fixture.engine.workspace(), SOURCE_SURFACE, None)
                .expect("direct edge roster must freeze without coordinate authority");
        let barrier = BTreeMap::from([(SOURCE_SURFACE, roster)]);
        let source = fixture
            .engine
            .workspace()
            .capture_node_source(SOURCE_ROOT, fixture.source_tabs)
            .expect("source root must be current");
        let mut events = Vec::new();

        let application = fixture
            .engine
            .apply_interaction_command_with_barrier(
                InputSequence::new(1),
                &WorkspaceCommand::CloseRoot { source },
                Some(&barrier),
                &mut events,
            )
            .expect("same-edge action must reduce as an expected rejection");

        assert!(matches!(
            application,
            CommandApplication::Rejected(crate::error::CommandError::SurfaceLifecycleFrozen {
                surface: SOURCE_SURFACE
            })
        ));
        assert_eq!(fixture.engine.workspace(), &before);
        assert!(events.is_empty());
    }

    #[test]
    fn roster_merge_selection_failure_does_not_publish_the_candidate() {
        let mut fixture = counter_fixture();
        let before_workspace = fixture.engine.workspace().clone();
        let before_version = fixture.engine.version();
        let roster =
            SurfaceRosterDisposition::capture(fixture.engine.workspace(), SOURCE_SURFACE, None)
                .expect("source roster must freeze");
        let source = fixture
            .engine
            .workspace()
            .capture_node_source(SOURCE_ROOT, fixture.source_tabs)
            .expect("source root must be current");
        let target = fixture
            .engine
            .workspace()
            .capture_tab_target(TARGET_ROOT, fixture.target_tabs)
            .expect("target tabs must be current");
        let transaction = WorkspaceTransaction::from_commands([WorkspaceCommand::Move {
            payload: MovePayload::Tabs(source),
            target: crate::command::DockTarget::Center(target),
        }]);
        let barrier = BTreeMap::from([(SOURCE_SURFACE, roster.clone())]);
        let mut events = Vec::new();

        let applied = fixture
            .engine
            .apply_surface_roster_transaction(
                InputSequence::new(1),
                &roster,
                &transaction,
                Some(CandidatePaneSelection {
                    surface: TARGET_SURFACE,
                    item: ItemId::new(999),
                }),
                &barrier,
                &mut events,
            )
            .expect("missing candidate selection is an expected rejection");

        assert!(!applied);
        assert_eq!(fixture.engine.workspace(), &before_workspace);
        assert_eq!(fixture.engine.version(), before_version);
        assert!(events.is_empty());
    }

    #[test]
    fn core_drag_reuses_partial_detachability_until_workspace_version_changes() {
        let mut builder = Workspace::builder();
        let central = builder.insert_node(Node::tabs([ItemId::new(1)]));
        let movable = builder.insert_node(Node::tabs([ItemId::new(2)]));
        let root_node = builder.insert_node(
            Node::equal_split(Axis::Horizontal, [central, movable]).expect("split must be valid"),
        );
        builder.set_root(
            SOURCE_ROOT,
            RootRecord::new(root_node).with_central(central),
        );
        builder.set_surface(SOURCE_SURFACE, SurfacePresentation::new(SOURCE_ROOT));
        let workspace = builder.build().expect("cache workspace must be valid");
        let mut engine =
            DockEngine::new(workspace, DockPolicy::default()).expect("cache engine must be valid");
        let payload = MovePayload::Subtree(
            engine
                .workspace()
                .capture_node_source(SOURCE_ROOT, movable)
                .expect("partial source must be current"),
        );
        PARTIAL_DETACHABILITY_EVALUATIONS.with(|evaluations| evaluations.set(0));

        engine
            .enqueue_renderer_intent(RendererIntent::ArmDragFrom {
                pointer: TEST_POINTER,
                button: PointerButton::Primary,
                payload,
                origin: DragOrigin::Workspace,
            })
            .expect("arm sequence must be available");
        let armed = engine.reduce_pending().expect("arm must reduce");
        let session = match armed.reduced_inputs()[0].outcome() {
            InputOutcome::InteractionProcessed {
                outcome: InteractionOutcome::DragArmed { session, .. },
                ..
            } => *session,
            outcome => panic!("unexpected arm outcome: {outcome:?}"),
        };
        engine
            .enqueue_renderer_intent(RendererIntent::BeginDrag {
                session,
                pointer: TEST_POINTER,
                button: PointerButton::Primary,
            })
            .expect("begin sequence must be available");
        engine.reduce_pending().expect("begin must reduce");
        assert_eq!(
            PARTIAL_DETACHABILITY_EVALUATIONS.with(std::cell::Cell::get),
            1
        );

        assert!(
            engine
                .core_drag_source_is_current(session, InputSequence::new(100))
                .expect("same-version source check must succeed")
        );
        assert!(
            engine
                .core_drag_source_is_current(session, InputSequence::new(101))
                .expect("second same-version source check must succeed")
        );
        assert_eq!(
            PARTIAL_DETACHABILITY_EVALUATIONS.with(std::cell::Cell::get),
            1
        );

        engine.version = WorkspaceVersion::new(
            engine.version.epoch(),
            engine
                .version
                .revision()
                .checked_next()
                .expect("test revision must advance"),
        );
        assert!(
            engine
                .core_drag_source_is_current(session, InputSequence::new(102))
                .expect("new-version source check must succeed")
        );
        assert_eq!(
            PARTIAL_DETACHABILITY_EVALUATIONS.with(std::cell::Cell::get),
            2
        );
        assert!(
            engine
                .core_drag_source_is_current(session, InputSequence::new(103))
                .expect("cached new-version source check must succeed")
        );
        assert_eq!(
            PARTIAL_DETACHABILITY_EVALUATIONS.with(std::cell::Cell::get),
            2
        );
    }

    #[test]
    fn contained_preview_resolution_rejects_a_stale_source_fingerprint() {
        let floating = FloatingPresentationId::new(1);
        let mut builder = Workspace::builder();
        let host_tabs = builder.insert_node(Node::tabs([ItemId::new(1)]));
        let floating_tabs = builder.insert_node(Node::tabs([ItemId::new(2), ItemId::new(3)]));
        builder.set_root(SOURCE_ROOT, RootRecord::new(host_tabs));
        builder.set_root(TARGET_ROOT, RootRecord::new(floating_tabs));
        builder.set_surface(SOURCE_SURFACE, SurfacePresentation::new(SOURCE_ROOT));
        builder.set_contained_floating(ContainedFloating::new(
            floating,
            TARGET_ROOT,
            SOURCE_SURFACE,
            test_rect(),
            7,
        ));
        builder
            .attach_contained(SOURCE_SURFACE, floating)
            .expect("host surface must exist");
        let workspace = builder.build().expect("workspace must be valid");
        let mut engine =
            DockEngine::new(workspace, DockPolicy::default()).expect("engine must be valid");
        let payload = MovePayload::Subtree(
            engine
                .workspace()
                .capture_node_source(TARGET_ROOT, floating_tabs)
                .expect("source must be current"),
        );
        let mut scene = BuildingScene::new([SOURCE_SURFACE]).expect("roster must be unique");
        scene
            .insert_ready(ReadySurfaceScene::new(
                SOURCE_SURFACE,
                LogicalRect::new(0.0, 0.0, 400.0, 300.0).expect("scene rectangle must be valid"),
            ))
            .expect("surface facts must be unique");
        engine
            .enqueue_scene(scene)
            .expect("scene sequence must be available");
        engine.reduce_pending().expect("scene must publish");
        let placement = engine
            .contained_placement(
                SOURCE_SURFACE,
                LogicalRect::new(40.0, 30.0, 100.0, 100.0)
                    .expect("requested rectangle must be valid"),
                LogicalSize::new(0.0, 0.0).expect("minimum size must be valid"),
            )
            .expect("contained placement must be available");
        let proposal = ContainedTearOffProposal::new(TARGET_ROOT, floating, placement, 7);
        let session = begin_drag(&mut engine, payload);
        let Some(Node::Tabs { selected, .. }) = engine.workspace.nodes.get_mut(floating_tabs)
        else {
            panic!("floating source must remain tabs");
        };
        *selected = Some(ItemId::new(3));
        engine
            .workspace
            .validate()
            .expect("selection mutation must keep the workspace valid");
        engine
            .enqueue_renderer_intent(RendererIntent::UpdateDrag {
                session,
                target: TargetAuthority::local(
                    SOURCE_SURFACE,
                    Authority::Known(Some(SurfacePointer::new(
                        SOURCE_SURFACE,
                        LogicalPoint::new(200.0, 150.0).expect("pointer must be valid"),
                    ))),
                ),
                tear_off: Some(TearOffRequest::Contained(proposal)),
            })
            .expect("update sequence must be available");

        let update = engine.reduce_pending().expect("update must reduce");
        assert!(matches!(
            update.reduced_inputs()[0].outcome(),
            InputOutcome::InteractionProcessed {
                outcome: InteractionOutcome::PreviewUpdated {
                    preview: None,
                    status: PreviewResolutionStatus::Rejected,
                    ..
                },
                ..
            }
        ));
    }

    fn ready_counter_scene(fixture: &CounterFixture) -> BuildingScene {
        let mut building = BuildingScene::new([SOURCE_SURFACE, TARGET_SURFACE])
            .expect("counter roster must be unique");
        building
            .insert_ready(ReadySurfaceScene::new(SOURCE_SURFACE, test_rect()))
            .expect("source facts must be unique");
        let target = fixture
            .engine
            .workspace()
            .capture_tab_target(TARGET_ROOT, fixture.target_tabs)
            .expect("target must be current");
        let mut target_scene = ReadySurfaceScene::new(TARGET_SURFACE, test_rect());
        target_scene.push_drop_target(DropTargetRecord::new(
            DropTargetId::Center {
                surface: TARGET_SURFACE,
                root: TARGET_ROOT,
                tabs: fixture.target_tabs,
            },
            DockTarget::Center(target),
            DropTargetAvailability::Available,
            HitRegion::new(test_rect()),
            SceneLayerKey::new(0),
            DropVisual::new(test_rect()),
        ));
        building
            .insert_ready(target_scene)
            .expect("target facts must be unique");
        building
    }

    fn begin_counter_drag(fixture: &mut CounterFixture) -> DragSessionId {
        let payload = MovePayload::Item(
            fixture
                .engine
                .workspace()
                .capture_item_source(SOURCE_ROOT, fixture.source_tabs, ItemId::new(1))
                .expect("source must be current"),
        );
        begin_drag(&mut fixture.engine, payload)
    }

    fn begin_drag(engine: &mut DockEngine, payload: MovePayload) -> DragSessionId {
        engine
            .enqueue_renderer_intent(RendererIntent::ArmDrag {
                pointer: TEST_POINTER,
                button: PointerButton::Primary,
                payload,
            })
            .expect("arm sequence must be available");
        let armed = engine.reduce_pending().expect("arm must reduce");
        let session = match armed.reduced_inputs()[0].outcome() {
            InputOutcome::InteractionProcessed {
                outcome: InteractionOutcome::DragArmed { session, .. },
                ..
            } => *session,
            outcome => panic!("unexpected arm outcome: {outcome:?}"),
        };
        engine
            .enqueue_renderer_intent(RendererIntent::BeginDrag {
                session,
                pointer: TEST_POINTER,
                button: PointerButton::Primary,
            })
            .expect("begin sequence must be available");
        engine.reduce_pending().expect("begin must reduce");
        session
    }

    #[test]
    fn fatal_counter_exhaustion_rolls_back_the_complete_boundary() {
        let root = RootId::new(1);
        let surface = SurfaceId::new(1);
        let mut builder = Workspace::builder();
        let tabs = builder.insert_node(Node::tabs([ItemId::new(1), ItemId::new(2)]));
        builder.set_root(root, RootRecord::new(tabs));
        builder.set_surface(surface, SurfacePresentation::new(root));
        let workspace = builder.build().expect("test workspace must be valid");
        let command = WorkspaceCommand::Select {
            source: workspace
                .capture_item_source(root, tabs, ItemId::new(2))
                .expect("source must be capturable"),
        };
        let mut engine = DockEngine::new(workspace.clone(), DockPolicy::default())
            .expect("engine must be valid");
        engine.version = WorkspaceVersion::new(
            WorkspaceEpoch::default(),
            WorkspaceRevision::new(u64::MAX - 1),
        );
        let expected = engine.version;
        let mut policy = engine.policy.clone();
        policy.set_allow_native_surfaces(true);
        engine
            .enqueue(EngineInput::ReplacePolicy { expected, policy })
            .expect("first sequence must be available");
        engine
            .enqueue(EngineInput::WorkspaceCommand { expected, command })
            .expect("second sequence must be available");
        let pending = engine.pending.clone();

        assert!(matches!(
            engine.reduce_pending(),
            Err(EngineError::WorkspaceRevisionExhausted { .. })
        ));
        assert_eq!(engine.workspace, workspace);
        assert!(!engine.policy.allows_native_surfaces());
        assert_eq!(engine.version, expected);
        assert_eq!(engine.pending, pending);
    }

    #[test]
    fn scene_generation_exhaustion_rolls_back_scene_and_pending_inputs() {
        let mut fixture = counter_fixture();
        fixture.engine.last_scene_generation = SceneGeneration::new(u64::MAX);
        fixture
            .engine
            .enqueue_scene(ready_counter_scene(&fixture))
            .expect("scene sequence must be available");
        let before = fixture.engine.candidate();

        assert!(matches!(
            fixture.engine.reduce_pending(),
            Err(EngineError::SceneGenerationExhausted { .. })
        ));
        assert_eq!(fixture.engine, before);
    }

    #[test]
    fn drag_generation_exhaustion_rolls_back_gesture_and_pending_inputs() {
        let mut fixture = counter_fixture();
        fixture.engine.interaction.exhaust_drag_generation();
        let payload = MovePayload::Item(
            fixture
                .engine
                .workspace()
                .capture_item_source(SOURCE_ROOT, fixture.source_tabs, ItemId::new(1))
                .expect("source must be current"),
        );
        fixture
            .engine
            .enqueue_renderer_intent(RendererIntent::ArmDrag {
                pointer: TEST_POINTER,
                button: PointerButton::Primary,
                payload,
            })
            .expect("arm sequence must be available");
        let before = fixture.engine.candidate();

        assert!(matches!(
            fixture.engine.reduce_pending(),
            Err(EngineError::Interaction {
                source: InteractionCounterError::DragGenerationExhausted,
                ..
            })
        ));
        assert_eq!(fixture.engine, before);
        assert_eq!(
            fixture.engine.interaction().status(),
            InteractionStatus::Idle
        );
    }

    #[test]
    fn preview_sequence_exhaustion_rolls_back_observation_and_pending_inputs() {
        let mut fixture = counter_fixture();
        fixture
            .engine
            .enqueue_scene(ready_counter_scene(&fixture))
            .expect("scene sequence must be available");
        fixture.engine.reduce_pending().expect("scene must publish");
        let session = begin_counter_drag(&mut fixture);
        fixture.engine.interaction.exhaust_preview_sequence();
        fixture
            .engine
            .enqueue_renderer_intent(RendererIntent::UpdateDrag {
                session,
                target: TargetAuthority::local(
                    TARGET_SURFACE,
                    Authority::Known(Some(SurfacePointer::new(
                        TARGET_SURFACE,
                        LogicalPoint::new(50.0, 50.0).expect("test point must be valid"),
                    ))),
                ),
                tear_off: None,
            })
            .expect("update sequence must be available");
        let before = fixture.engine.candidate();

        assert!(matches!(
            fixture.engine.reduce_pending(),
            Err(EngineError::Interaction {
                source: InteractionCounterError::PreviewSequenceExhausted,
                ..
            })
        ));
        assert_eq!(fixture.engine, before);
        assert!(fixture.engine.interaction().preview().is_none());
    }

    #[test]
    fn malformed_scene_does_not_consume_the_boundary_publication_slot() {
        let mut fixture = counter_fixture();
        let mut malformed = BuildingScene::new([SOURCE_SURFACE, TARGET_SURFACE])
            .expect("malformed roster must be unique");
        let mut source = ReadySurfaceScene::new(SOURCE_SURFACE, test_rect());
        source.push_node(SemanticRect::new(
            NodeSceneId {
                root: TARGET_ROOT,
                node: fixture.target_tabs,
            },
            test_rect(),
            SceneLayerKey::new(0),
        ));
        malformed
            .insert_ready(source)
            .expect("malformed facts are structurally unique");
        fixture
            .engine
            .enqueue_scene(malformed)
            .expect("malformed scene sequence must be available");
        fixture
            .engine
            .enqueue_scene(ready_counter_scene(&fixture))
            .expect("valid scene sequence must be available");

        let transition = fixture
            .engine
            .reduce_pending()
            .expect("malformed scene is a nonfatal rejection");
        assert!(matches!(
            transition.reduced_inputs()[0].outcome(),
            InputOutcome::SceneRejected {
                error: SceneBuildError::InvalidNodeSemantic { .. }
            }
        ));
        assert!(matches!(
            transition.reduced_inputs()[1].outcome(),
            InputOutcome::ScenePublished { stamp, .. }
                if stamp.generation() == SceneGeneration::new(1)
        ));
    }

    #[test]
    fn only_one_valid_scene_can_publish_in_a_boundary() {
        let mut fixture = counter_fixture();
        fixture
            .engine
            .enqueue_scene(ready_counter_scene(&fixture))
            .expect("first scene sequence must be available");
        fixture
            .engine
            .enqueue_scene(ready_counter_scene(&fixture))
            .expect("second scene sequence must be available");

        let transition = fixture
            .engine
            .reduce_pending()
            .expect("duplicate publication is a nonfatal rejection");
        assert!(matches!(
            transition.reduced_inputs()[0].outcome(),
            InputOutcome::ScenePublished { .. }
        ));
        assert!(matches!(
            transition.reduced_inputs()[1].outcome(),
            InputOutcome::SceneRejected {
                error: SceneBuildError::AlreadyPublishedInBoundary
            }
        ));
        assert_eq!(
            fixture
                .engine
                .scene()
                .expect("first scene must remain published")
                .stamp()
                .generation(),
            SceneGeneration::new(1)
        );
    }
}
