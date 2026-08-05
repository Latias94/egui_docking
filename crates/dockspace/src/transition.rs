//! Atomic engine transition records returned after successful publication.

use crate::backend_ingress::{BackendIngressOrdinal, BackendIngressProviderReplacementTicket};
use crate::close_plan::{
    ClosePlan, CloseRequestId, CloseResolutionOutcome, NativeCloseEdge, SurfaceCloseRequest,
};
use crate::command::{CloseCommitOutcome, CommandOutcome, ContentCloseTarget};
use crate::effect::{EffectId, EffectTransition, PlatformEffectEmission};
use crate::engine::HostPresentationDispositionOutcome;
use crate::error::CommandError;
use crate::event::WorkspaceEvent;
use crate::frame::{ViewportFrameTransition, ViewportReconciliation};
use crate::ids::{
    InputSequence, ItemId, NativeCreateSagaId, ReducerTickId, RootId, SourceSequence,
    StableInputSourceId, SurfaceId, WorkspaceEpoch, WorkspaceRevision,
};
use crate::intent::Authority;
use crate::interaction::{InteractionEvent, InteractionOutcome};
use crate::platform_provider::{
    PlatformObservationAuthorityError, PlatformObservationLease, PlatformProviderReplacementTicket,
};
use crate::pointer_journal::{
    AnyButtonDownAuthority, PointerCaptureOwner, PointerEdge, PointerEdgeTicket, PointerStreamId,
};
use crate::policy::{PolicyRejection, PolicyRevision};
use crate::presentation_config::PresentationConfigRevision;
use crate::presentation_observation::{
    HostPresentationEmission, HostPresentationObservationOutcome, PresentationHostLease,
    PresentationHostRetirementReason, PresentationHostRetirementTombstone,
    PresentedSurfaceAuthority, SurfacePresentationOutputTicket,
};
use crate::scene::{PopupGeometryUnavailableReason, SurfaceSceneStamp};
use crate::scene_manifest::MeasurementAuthorityError;
use crate::tab_strip::TabListMenuSessionId;
use crate::viewport::ViewportBinding;
use crate::viewport_focus::FocusDelta;

/// Version of all workspace and policy state used to derive semantic input.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct WorkspaceVersion {
    epoch: WorkspaceEpoch,
    revision: WorkspaceRevision,
}

impl WorkspaceVersion {
    /// Creates a version from distinct replacement and mutation counters.
    #[must_use]
    pub const fn new(epoch: WorkspaceEpoch, revision: WorkspaceRevision) -> Self {
        Self { epoch, revision }
    }

    /// Returns the replacement epoch.
    #[must_use]
    pub const fn epoch(self) -> WorkspaceEpoch {
        self.epoch
    }

    /// Returns the mutation revision within the current epoch.
    #[must_use]
    pub const fn revision(self) -> WorkspaceRevision {
        self.revision
    }
}

/// Normative source-class priority used by the single reducer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum InputPriority {
    /// Workspace replacement and native lifecycle control.
    LifecycleControl,
    /// Authoritative facts supplied by a platform provider.
    PlatformObservation,
    /// Checked commands and application policy changes.
    ApplicationCommand,
    /// Validation and other state-neutral upkeep.
    Maintenance,
    /// Policy and presentation configuration published after the frozen host frame.
    ConfigurationCommit,
}

/// Result of reducing one sequenced input.
#[derive(Debug, Clone, PartialEq)]
pub enum InputOutcome {
    /// A programmatic content-close request opened or reused one exact plan.
    ContentCloseRequested {
        /// Stable target supplied by the caller.
        target: ContentCloseTarget,
        /// Core-owned frozen close plan.
        plan: ClosePlan,
        /// Whether an unresolved plan for the same target was reused.
        reused: bool,
        /// Current workspace version after accepting the request.
        version: WorkspaceVersion,
    },
    /// A programmatic content-close request was rejected without opening a plan.
    ContentCloseRejected {
        /// Stable target supplied by the caller.
        target: ContentCloseTarget,
        /// Typed reason the target could not be captured.
        reason: ContentCloseRequestRejection,
        /// Published version, unchanged by this input.
        version: WorkspaceVersion,
    },
    /// One exact platform close edge opened a frozen complete-surface plan.
    SurfaceCloseRequested {
        /// Exact native close observation that authorized the request.
        edge: NativeCloseEdge,
        /// Full explicit request accepted by the core.
        request: SurfaceCloseRequest,
        /// Frozen application-facing plan and decision roster.
        plan: ClosePlan,
        /// Current workspace version after accepting the request.
        version: WorkspaceVersion,
    },
    /// A surface close request was consumed without opening a plan.
    SurfaceCloseRejected {
        /// Exact native close observation the caller attempted to resolve.
        edge: NativeCloseEdge,
        /// Full explicit request rejected by the core.
        request: SurfaceCloseRequest,
        /// Typed reason the request cannot be prepared.
        reason: SurfaceCloseRequestRejection,
        /// Published version, unchanged by this input.
        version: WorkspaceVersion,
    },
    /// One exact native close edge is now awaiting a causal cancel proof.
    SurfaceCloseCancellationRequested {
        /// Exact edge which remains live until cancellation is observed.
        edge: NativeCloseEdge,
        /// Core-owned plan used to carry the cancellation obligation.
        plan: ClosePlan,
        /// Current workspace version after accepting the cancellation request.
        version: WorkspaceVersion,
    },
    /// One exact initial or deferred close decision was consumed.
    CloseDecisionProcessed {
        /// Token-resolution result, including typed inert outcomes.
        resolution: CloseResolutionOutcome,
        /// Latest public plan state, absent only for an unknown request.
        plan: Option<ClosePlan>,
        /// Checked topology result, or typed commit rejection, after the final allow.
        application: Option<Result<CloseCommitOutcome, CommandError>>,
        /// Whether the same atomic boundary changed durable topology.
        changed: bool,
        /// Durable workspace version after processing the decision.
        version: WorkspaceVersion,
    },
    /// An existing adapter window was bound to a logical surface.
    ViewportRegistered {
        /// Complete core-owned binding identity.
        binding: ViewportBinding,
    },
    /// Viewport registration named no current logical surface.
    ViewportRegistrationRejected {
        /// Missing stable logical surface.
        surface: crate::ids::SurfaceId,
    },
    /// One complete platform fact snapshot was atomically published.
    PlatformSnapshotPublished {
        /// Structured capability, inventory, and route transition.
        transition: ViewportFrameTransition,
        /// Single global native-focus observation reduced in this same core input.
        focus: crate::viewport_focus::FocusObservationTransition,
        /// Explicit activations created by lifecycle commits in this same core input.
        activations: Vec<crate::viewport_focus::ActivationStart>,
        /// Exact close edges newly observed in this inventory batch.
        native_close_edges: Vec<NativeCloseEdge>,
    },
    /// One exact binding-scoped native-close fact was published.
    NativeCloseObservationPublished {
        /// Structured lifecycle transition caused by this one observation.
        transition: ViewportFrameTransition,
        /// Exact close edges newly observed at this backend position.
        native_close_edges: Vec<NativeCloseEdge>,
    },
    /// Platform facts from an earlier workspace epoch were consumed without mutation.
    PlatformSnapshotStale {
        expected_epoch: WorkspaceEpoch,
        current_epoch: WorkspaceEpoch,
    },
    /// Platform ingress from a foreign, unknown, superseded, or retired provider was consumed
    /// without mutation.
    PlatformProviderRejected {
        /// Opaque provider lease submitted with this input.
        provider: PlatformObservationLease,
        /// Exact authority classification retained for diagnostics.
        error: PlatformObservationAuthorityError,
    },
    /// A correlated adapter dispatch result was reduced.
    PlatformEffectReported {
        effect: EffectId,
        transition: EffectTransition,
        /// Activation state change when the effect belongs to the global focus lane.
        focus: Option<crate::viewport_focus::FocusEffectReportTransition>,
    },
    /// One explicit viewport activation request was reduced.
    ViewportActivationRequested {
        activation: crate::viewport_focus::ActivationStart,
    },
    /// A caller attempted to inject a core-owned lifecycle activation cause.
    ViewportActivationRejected {
        request: crate::viewport_focus::ViewportActivationRequest,
    },
    /// One exact pane-focus observation was reduced.
    PaneFocusObservationPublished {
        transition: crate::viewport_focus::PaneFocusObservationTransition,
    },
    /// Pane focus from an older workspace epoch was consumed without mutation.
    PaneFocusObservationStale {
        expected_epoch: WorkspaceEpoch,
        current_epoch: WorkspaceEpoch,
    },
    /// One unresolved native-create saga was explicitly cancelled.
    NativeCreateCancelled {
        saga: NativeCreateSagaId,
        /// Immediate compensation when the child had already become observable.
        compensation: Option<EffectId>,
    },
    /// One definitively failed cleanup was replaced by a new exact-once effect.
    ViewportCleanupRetried {
        failed_effect: EffectId,
        retry: EffectId,
    },
    /// The complete workspace was replaced and all older derived state became stale.
    WorkspaceReplaced {
        /// Version before replacement.
        before: WorkspaceVersion,
        /// Version after replacement.
        after: WorkspaceVersion,
        /// Exact durable identity frontier when this replacement came from a
        /// validated document restore. Ordinary workspace replacements carry
        /// `None` and therefore cannot satisfy a document publication proof.
        restored_identity_frontier: Option<crate::ids::PresentationIdentityFrontier>,
        /// Exact native binding invalidation and cleanup summary.
        reconciliation: ViewportReconciliation,
    },
    /// One checked command committed or produced a valid no-op.
    CommandProcessed {
        /// Structured command result.
        outcome: CommandOutcome,
        /// Whether the complete workspace changed.
        changed: bool,
        /// Version after processing this input.
        version: WorkspaceVersion,
    },
    /// A checked command was deterministically rejected and consumed.
    CommandRejected {
        /// Typed reason the command could not apply to the candidate state.
        error: CommandError,
        /// Published version, unchanged by this input.
        version: WorkspaceVersion,
    },
    /// Application policy was replaced or found equal.
    PolicyReplaced {
        /// Whether policy state changed.
        changed: bool,
        /// Version after processing this input.
        version: WorkspaceVersion,
    },
    /// Renderer-neutral semantic presentation geometry was replaced or found equal.
    PresentationConfigReplaced {
        /// Whether semantic geometry changed.
        changed: bool,
        /// Current independent presentation configuration revision.
        revision: PresentationConfigRevision,
    },
    /// One renderer interaction intent was reduced.
    InteractionProcessed {
        /// Structured interaction state-machine result.
        outcome: InteractionOutcome,
        /// Durable workspace version after processing the intent.
        version: WorkspaceVersion,
    },
    /// A maintenance validation completed without mutation.
    WorkspaceValidated {
        /// Version which was validated.
        version: WorkspaceVersion,
    },
    /// Input derived from an old state was rejected without mutation.
    StaleRejected {
        /// Version carried by the input.
        expected: WorkspaceVersion,
        /// Shared state version accepted for application inputs in this boundary.
        accepted_base: WorkspaceVersion,
    },
}

/// Typed fail-closed rejection while capturing a programmatic close target.
#[derive(Debug, Clone, PartialEq)]
pub enum ContentCloseRequestRejection {
    /// The requested item is not currently owned by the workspace.
    ItemUnavailable { item: ItemId },
    /// The requested root is not currently owned by the workspace.
    RootUnavailable { root: RootId },
    /// The requested root has no application-owned items.
    RootEmpty { root: RootId },
    /// A close capability explicitly disables this item.
    ItemCloseDisabled { item: ItemId },
    /// The exact source could not be captured from the current graph.
    SourceUnavailable(CommandError),
}

/// Typed fail-closed rejection while freezing one complete surface-close plan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SurfaceCloseRequestRejection {
    /// The edge no longer matches the registry's exact live close observation.
    EdgeUnavailable { binding: ViewportBinding },
    /// The exact native binding is still staging a create or replacement and has no graph
    /// ownership to close. Its lifecycle is compensated internally instead.
    StagingBinding { binding: ViewportBinding },
    /// The logical source surface disappeared before preparation.
    SurfaceUnavailable { surface: SurfaceId },
    /// The provider cannot explicitly cancel a native close obligation.
    CancellationUnsupported,
    /// The current close policy rejected the source surface.
    PolicyRejected(PolicyRejection),
    /// The exact transaction frozen for `RehomeAll` was rejected by docking policy.
    ///
    /// This is distinct from [`Self::PolicyRejected`], which authorizes the
    /// native surface-close request itself. The enclosed rejection identifies
    /// the source, target, payload, class, presentation, or undocking rule
    /// that rejected the planned recovery transaction.
    RehomePolicyRejected(PolicyRejection),
    /// The requested exact rehome program could not be compiled and preflighted.
    RehomeProgramUnavailable,
    /// One pane in the frozen source roster is statically non-closeable.
    PaneCloseDisabled { item: ItemId },
    /// The source surface does not contain closeable content for CloseContent.
    CloseContentUnavailable,
    /// A distinct non-terminal plan already owns this exact binding.
    ActivePlan { request: CloseRequestId },
}

/// Public classification of one surface's current presentation authority.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SurfaceSceneStateKind {
    /// A current projection exists; painted interaction authority is reported separately.
    Ready,
    /// Only a previous paint fallback remains.
    Stale,
    /// No current or previous presentation can paint.
    Bootstrap,
}

/// One exact presentation-authority change committed by a reducer tick.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SurfaceSceneDelta {
    surface: SurfaceId,
    before: Option<(
        SurfaceSceneStamp,
        SurfaceSceneStateKind,
        Option<PresentedSurfaceAuthority>,
    )>,
    after: Option<(
        SurfaceSceneStamp,
        SurfaceSceneStateKind,
        Option<PresentedSurfaceAuthority>,
    )>,
}

impl SurfaceSceneDelta {
    pub(crate) const fn new(
        surface: SurfaceId,
        before: Option<(
            SurfaceSceneStamp,
            SurfaceSceneStateKind,
            Option<PresentedSurfaceAuthority>,
        )>,
        after: Option<(
            SurfaceSceneStamp,
            SurfaceSceneStateKind,
            Option<PresentedSurfaceAuthority>,
        )>,
    ) -> Self {
        Self {
            surface,
            before,
            after,
        }
    }

    /// Returns the affected logical surface.
    #[must_use]
    pub const fn surface(self) -> SurfaceId {
        self.surface
    }

    /// Returns authority before the tick, or `None` when the surface was absent.
    #[must_use]
    pub const fn before(
        self,
    ) -> Option<(
        SurfaceSceneStamp,
        SurfaceSceneStateKind,
        Option<PresentedSurfaceAuthority>,
    )> {
        self.before
    }

    /// Returns authority after the tick, or `None` when the surface was removed.
    #[must_use]
    pub const fn after(
        self,
    ) -> Option<(
        SurfaceSceneStamp,
        SurfaceSceneStateKind,
        Option<PresentedSurfaceAuthority>,
    )> {
        self.after
    }
}

/// Typed non-fatal rejection of one independently delivered surface contribution.
#[derive(Debug, Clone, PartialEq)]
pub enum SurfaceContributionRejection {
    /// The token's exact base authority was superseded before installation.
    StaleBase {
        /// Authority frozen when measurement began.
        submitted: SurfaceSceneStamp,
        /// Current authority, or `None` after roster removal.
        current: Option<SurfaceSceneStamp>,
    },
    /// Binding, lifecycle, or coordinates changed after the token was minted.
    CoordinateAuthorityChanged {
        /// Surface whose callback facts are late.
        surface: SurfaceId,
    },
    /// The policy used to compile the prepared plan is no longer authoritative.
    PolicyAuthorityChanged {
        /// Policy authority frozen during preparation.
        submitted: PolicyRevision,
        /// Policy authority current at reduction.
        current: PolicyRevision,
    },
}

/// Result of one independently compiled surface contribution in a reducer tick.
#[derive(Debug, Clone, PartialEq)]
pub enum SurfaceContributionOutcome {
    /// A complete paint candidate was installed with its embedded painted state.
    Ready {
        /// Surface whose entry was replaced.
        surface: SurfaceId,
        /// Exact newly installed authority.
        stamp: SurfaceSceneStamp,
        /// Core-minted output ticket for the installed candidate.
        ticket: SurfacePresentationOutputTicket,
    },
    /// The host explicitly retained the current ready candidate without replacing it.
    Retained {
        /// Surface whose current candidate remains installed.
        surface: SurfaceId,
        /// Exact unchanged ready authority.
        stamp: SurfaceSceneStamp,
        /// Exact current candidate output ticket retained by the host pass.
        ticket: SurfacePresentationOutputTicket,
    },
    /// An exact contribution explicitly left the surface non-interactive.
    Unavailable {
        /// Surface whose unavailable state was recorded.
        surface: SurfaceId,
        /// Exact current non-interactive authority.
        stamp: SurfaceSceneStamp,
        /// Explicit reason retained by the entry.
        reason: SurfaceContributionUnavailableReason,
    },
    /// A superseded prepared contribution was consumed without changing its entry.
    Rejected {
        /// Surface named by the contribution.
        surface: SurfaceId,
        /// Typed non-fatal rejection.
        reason: SurfaceContributionRejection,
    },
}

impl SurfaceContributionOutcome {
    /// Returns the surface addressed by this contribution.
    #[must_use]
    pub const fn surface(&self) -> SurfaceId {
        match self {
            Self::Ready { surface, .. }
            | Self::Retained { surface, .. }
            | Self::Unavailable { surface, .. }
            | Self::Rejected { surface, .. } => *surface,
        }
    }

    /// Returns whether reducing this contribution replaced the surface authority.
    #[must_use]
    pub const fn changes_surface_authority(&self) -> bool {
        matches!(self, Self::Ready { .. } | Self::Unavailable { .. })
    }
}

/// Why an accepted exact contribution did not produce hit-test authority.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SurfaceContributionUnavailableReason {
    /// One exact required measurement was explicitly unavailable.
    MeasurementsUnavailable(MeasurementAuthorityError),
    /// Surface bounds were non-positive, including minimized or hidden geometry.
    EmptyBounds,
    /// The native association was not ready to authorize coordinates.
    CoordinateAuthorityUnavailable,
    /// Exact geometry could not project the active workspace popup.
    PopupGeometryUnavailable {
        /// Exact popup session frozen by the contribution requirement.
        session: TabListMenuSessionId,
        /// Typed geometric reason for the fail-closed contribution.
        reason: PopupGeometryUnavailableReason,
    },
}

/// One input and its outcome in normative reduction order.
#[derive(Debug, Clone, PartialEq)]
pub struct ReducedInput {
    tick: ReducerTickId,
    ordinal: crate::ids::ReducerCausalOrdinal,
    sequence: InputSequence,
    source: StableInputSourceId,
    source_sequence: SourceSequence,
    priority: InputPriority,
    outcome: InputOutcome,
}

impl ReducedInput {
    pub(crate) const fn new(
        tick: ReducerTickId,
        ordinal: crate::ids::ReducerCausalOrdinal,
        sequence: InputSequence,
        source: StableInputSourceId,
        source_sequence: SourceSequence,
        priority: InputPriority,
        outcome: InputOutcome,
    ) -> Self {
        Self {
            tick,
            ordinal,
            sequence,
            source,
            source_sequence,
            priority,
            outcome,
        }
    }

    /// Returns the core-assigned causal reducer tick.
    #[must_use]
    pub const fn tick(&self) -> ReducerTickId {
        self.tick
    }

    /// Returns this input's exact causal position within the reducer boundary.
    #[must_use]
    pub const fn causal_ordinal(&self) -> crate::ids::ReducerCausalOrdinal {
        self.ordinal
    }

    /// Returns the writer-assigned sequence.
    #[must_use]
    pub const fn sequence(&self) -> InputSequence {
        self.sequence
    }

    /// Returns the stable semantic producer identity.
    #[must_use]
    pub const fn source(&self) -> StableInputSourceId {
        self.source
    }

    /// Returns the producer-assigned monotonic sequence.
    #[must_use]
    pub const fn source_sequence(&self) -> SourceSequence {
        self.source_sequence
    }

    /// Returns the exact backend-ingress ordinal when this input came from the
    /// joined backend recorder.
    #[must_use]
    pub fn backend_ingress_ordinal(&self) -> Option<BackendIngressOrdinal> {
        if self.source == crate::engine::BACKEND_INGRESS_INPUT_SOURCE {
            Some(BackendIngressOrdinal::from_committed_source_sequence(
                self.source_sequence.get(),
            ))
        } else {
            None
        }
    }

    /// Returns the normative source-class priority.
    #[must_use]
    pub const fn priority(&self) -> InputPriority {
        self.priority
    }

    /// Returns the structured reduction outcome.
    #[must_use]
    pub const fn outcome(&self) -> &InputOutcome {
        &self.outcome
    }
}

/// One provider-ordered pointer edge accepted by the atomic host-frame reducer.
///
/// Pointer edges are deliberately separate from [`ReducedInput`]. They carry a
/// core-minted stream incarnation and ticket rather than adapter-authored input
/// source coordinates, so downstream interaction events can retain their exact
/// journal cause without re-encoding the edge as a semantic input.
#[derive(Debug, Clone, PartialEq)]
pub struct ReducedPointerEdge {
    tick: ReducerTickId,
    ordinal: crate::ids::ReducerCausalOrdinal,
    stream: PointerStreamId,
    ticket: PointerEdgeTicket,
    edge: PointerEdge,
    button_authority_after: AnyButtonDownAuthority,
    capture_authority_after: Authority<PointerCaptureOwner>,
    interactions: Vec<InteractionOutcome>,
}

impl ReducedPointerEdge {
    pub(crate) fn new(
        tick: ReducerTickId,
        ordinal: crate::ids::ReducerCausalOrdinal,
        stream: PointerStreamId,
        ticket: PointerEdgeTicket,
        edge: PointerEdge,
        button_authority_after: AnyButtonDownAuthority,
        capture_authority_after: Authority<PointerCaptureOwner>,
        interactions: Vec<InteractionOutcome>,
    ) -> Self {
        debug_assert_eq!(stream.lease(), ticket.lease());
        debug_assert_eq!(stream.pointer(), edge.pointer());
        debug_assert_eq!(ticket.sequence(), edge.sequence());
        Self {
            tick,
            ordinal,
            stream,
            ticket,
            edge,
            button_authority_after,
            capture_authority_after,
            interactions,
        }
    }

    /// Returns the exact journal reduction cause.
    #[must_use]
    pub const fn cause(&self) -> crate::event::ReductionCause {
        crate::event::ReductionCause::PointerEdge {
            tick: self.tick,
            ordinal: self.ordinal,
            stream: self.stream,
            ticket: self.ticket,
        }
    }

    /// Returns the core reducer boundary which consumed this edge.
    #[must_use]
    pub const fn tick(&self) -> ReducerTickId {
        self.tick
    }

    /// Returns this journal batch's causal position within the reducer boundary.
    #[must_use]
    pub const fn causal_ordinal(&self) -> crate::ids::ReducerCausalOrdinal {
        self.ordinal
    }

    /// Returns the exact core-minted stream incarnation affected by this edge.
    #[must_use]
    pub const fn stream(&self) -> PointerStreamId {
        self.stream
    }

    /// Returns the core-minted identity of this accepted edge.
    #[must_use]
    pub const fn ticket(&self) -> PointerEdgeTicket {
        self.ticket
    }

    /// Returns the lossless provider edge in its original journal order.
    #[must_use]
    pub const fn edge(&self) -> &PointerEdge {
        &self.edge
    }

    /// Returns the scope-relative button authority immediately after this edge.
    #[must_use]
    pub const fn button_authority_after(&self) -> AnyButtonDownAuthority {
        self.button_authority_after
    }

    /// Returns this stream's capture authority immediately after this edge.
    #[must_use]
    pub const fn capture_authority_after(&self) -> Authority<PointerCaptureOwner> {
        self.capture_authority_after
    }

    /// Returns the ordered interaction-state transitions caused by this edge.
    ///
    /// One physical edge can legitimately cross the drag threshold and resolve
    /// its hover target in the same reduction. Keeping both outcomes preserves
    /// that causality without adding an artificial frame of input latency.
    #[must_use]
    pub fn interaction_outcomes(&self) -> &[InteractionOutcome] {
        &self.interactions
    }
}

/// Complete result of one successfully published engine boundary.
///
/// Surface contribution outcomes retain the batch's canonical surface order,
/// independent of adapter callback arrival order.
#[derive(Debug, Clone, PartialEq)]
pub struct EngineTransition {
    authority_domain: crate::ids::EngineAuthorityDomainId,
    tick: ReducerTickId,
    before: WorkspaceVersion,
    after: WorkspaceVersion,
    reduced: Vec<ReducedInput>,
    reduced_pointer_edges: Vec<ReducedPointerEdge>,
    events: Vec<WorkspaceEvent>,
    interaction_events: Vec<InteractionEvent>,
    platform_effects: Vec<PlatformEffectEmission>,
    focus_delta: FocusDelta,
    presentation_observations: Vec<HostPresentationObservationOutcome>,
    presentation_emissions: Vec<HostPresentationEmission>,
    presentation_dispositions: Vec<HostPresentationDispositionOutcome>,
    surface_contributions: Vec<SurfaceContributionOutcome>,
    surface_scene_deltas: Vec<SurfaceSceneDelta>,
    published_state_changed: bool,
}

/// Atomic start of one platform-provider handoff.
///
/// The predecessor has already lost authority when this value is returned.
/// The runtime must stop and join its dispatch worker before consuming the
/// ticket through `DockEngine::finish_platform_provider_replacement`.
#[derive(Debug, Clone, PartialEq)]
pub struct PlatformProviderReplacementStart {
    ticket: PlatformProviderReplacementTicket,
    transition: EngineTransition,
}

impl PlatformProviderReplacementStart {
    pub(crate) const fn new(
        ticket: PlatformProviderReplacementTicket,
        transition: EngineTransition,
    ) -> Self {
        Self { ticket, transition }
    }

    /// Returns the non-forgeable ticket reserved for the successor provider.
    #[must_use]
    pub const fn ticket(&self) -> PlatformProviderReplacementTicket {
        self.ticket
    }

    /// Returns the reducer transition which revoked predecessor authority.
    #[must_use]
    pub const fn transition(&self) -> &EngineTransition {
        &self.transition
    }

    /// Separates the handoff ticket from its already-published transition.
    #[must_use]
    pub fn into_parts(self) -> (PlatformProviderReplacementTicket, EngineTransition) {
        (self.ticket, self.transition)
    }
}

/// Atomic start of one joined backend-provider handoff.
///
/// The predecessor platform, pointer, and backend-order lanes have already lost
/// authority. The affine ticket can only be consumed by the joined finish API.
#[derive(Debug, PartialEq)]
pub struct BackendIngressProviderReplacementStart {
    ticket: BackendIngressProviderReplacementTicket,
    transition: EngineTransition,
}

impl BackendIngressProviderReplacementStart {
    pub(crate) const fn new(
        ticket: BackendIngressProviderReplacementTicket,
        transition: EngineTransition,
    ) -> Self {
        Self { ticket, transition }
    }

    /// Returns the reducer transition which revoked all predecessor lanes.
    #[must_use]
    pub const fn transition(&self) -> &EngineTransition {
        &self.transition
    }

    /// Separates the affine joined ticket from its already-published transition.
    #[must_use]
    pub fn into_parts(self) -> (BackendIngressProviderReplacementTicket, EngineTransition) {
        (self.ticket, self.transition)
    }
}

/// Result of explicitly terminating one presentation-host lease.
///
/// Only the first retirement publishes an engine transition. A duplicate returns either the
/// still-detailed terminal record or an explicit compacted classification without manufacturing
/// another reducer tick.
#[derive(Debug, Clone, PartialEq)]
pub enum PresentationHostRetirementOutcome {
    /// A live host, all of its streams, and their pending outputs were retired.
    Retired {
        /// Exact lease which reached its terminal state.
        host: PresentationHostLease,
        /// Owner-supplied terminal reason.
        reason: PresentationHostRetirementReason,
        /// Number of streams terminally reclaimed by the core.
        retired_stream_count: usize,
        /// Number of pending concrete emissions discarded without promotion.
        retired_output_count: usize,
        /// Surfaces whose active stream ownership belonged to this host.
        released_active_surfaces: Vec<SurfaceId>,
        /// Surfaces whose exact interaction authority was revoked.
        affected_surfaces: Vec<SurfaceId>,
        /// Atomic publication produced by the first retirement.
        transition: EngineTransition,
    },
    /// The host had already reached a terminal state.
    AlreadyRetired {
        /// Exact lease named by the duplicate request.
        host: PresentationHostLease,
        /// Original terminal record; the duplicate reason is intentionally inert.
        tombstone: PresentationHostRetirementTombstone,
    },
    /// The host was retired and its detailed diagnostic tombstone was safely compacted.
    Compacted {
        /// Exact lease named by the duplicate request.
        host: PresentationHostLease,
    },
}

impl PresentationHostRetirementOutcome {
    /// Returns the transition only when this call performed the retirement.
    #[must_use]
    pub const fn transition(&self) -> Option<&EngineTransition> {
        match self {
            Self::Retired { transition, .. } => Some(transition),
            Self::AlreadyRetired { .. } | Self::Compacted { .. } => None,
        }
    }
}

pub(crate) struct EngineTransitionParts {
    pub(crate) authority_domain: crate::ids::EngineAuthorityDomainId,
    pub(crate) tick: ReducerTickId,
    pub(crate) before: WorkspaceVersion,
    pub(crate) after: WorkspaceVersion,
    pub(crate) reduced: Vec<ReducedInput>,
    pub(crate) reduced_pointer_edges: Vec<ReducedPointerEdge>,
    pub(crate) events: Vec<WorkspaceEvent>,
    pub(crate) interaction_events: Vec<InteractionEvent>,
    pub(crate) platform_effects: Vec<PlatformEffectEmission>,
    pub(crate) focus_delta: FocusDelta,
    pub(crate) presentation_observations: Vec<HostPresentationObservationOutcome>,
    pub(crate) presentation_emissions: Vec<HostPresentationEmission>,
    pub(crate) presentation_dispositions: Vec<HostPresentationDispositionOutcome>,
    pub(crate) surface_contributions: Vec<SurfaceContributionOutcome>,
    pub(crate) surface_scene_deltas: Vec<SurfaceSceneDelta>,
    pub(crate) published_state_changed: bool,
}

impl EngineTransition {
    pub(crate) fn new(parts: EngineTransitionParts) -> Self {
        let EngineTransitionParts {
            authority_domain,
            tick,
            before,
            after,
            reduced,
            reduced_pointer_edges,
            events,
            interaction_events,
            platform_effects,
            focus_delta,
            presentation_observations,
            presentation_emissions,
            presentation_dispositions,
            surface_contributions,
            surface_scene_deltas,
            published_state_changed,
        } = parts;
        Self {
            authority_domain,
            tick,
            before,
            after,
            reduced,
            reduced_pointer_edges,
            events,
            interaction_events,
            platform_effects,
            focus_delta,
            presentation_observations,
            presentation_emissions,
            presentation_dispositions,
            surface_contributions,
            surface_scene_deltas,
            published_state_changed,
        }
    }

    /// Returns the engine authority domain which published this transition.
    #[must_use]
    pub const fn authority_domain(&self) -> crate::ids::EngineAuthorityDomainId {
        self.authority_domain
    }

    /// Returns the core-assigned causal reducer tick for this transition.
    #[must_use]
    pub const fn tick(&self) -> ReducerTickId {
        self.tick
    }

    /// Returns the state version before reduction.
    #[must_use]
    pub const fn before(&self) -> WorkspaceVersion {
        self.before
    }

    /// Returns the published state version.
    #[must_use]
    pub const fn after(&self) -> WorkspaceVersion {
        self.after
    }

    /// Returns inputs in normative reduction order.
    #[must_use]
    pub fn reduced_inputs(&self) -> &[ReducedInput] {
        &self.reduced
    }

    /// Returns accepted pointer edges in exact provider sequence order.
    #[must_use]
    pub fn reduced_pointer_edges(&self) -> &[ReducedPointerEdge] {
        &self.reduced_pointer_edges
    }

    /// Returns events generated only after the candidate committed.
    #[must_use]
    pub fn events(&self) -> &[WorkspaceEvent] {
        &self.events
    }

    /// Returns committed transient interaction events.
    #[must_use]
    pub fn interaction_events(&self) -> &[InteractionEvent] {
        &self.interaction_events
    }

    /// Returns exact provider-bound platform effects emitted after the candidate committed.
    ///
    /// Each emission retains the exact platform-provider incarnation authorized to dispatch
    /// and acknowledge its request. Adapters must not detach the underlying request from this
    /// delivery identity.
    #[must_use]
    pub fn platform_effects(&self) -> &[PlatformEffectEmission] {
        &self.platform_effects
    }

    /// Returns the adapter-facing net focus change for this atomic boundary.
    #[must_use]
    pub const fn focus_delta(&self) -> &FocusDelta {
        &self.focus_delta
    }

    /// Returns all core-reduced final-presentation observations for this boundary.
    #[must_use]
    pub fn presentation_observations(&self) -> &[HostPresentationObservationOutcome] {
        &self.presentation_observations
    }

    /// Returns concrete presentation emissions minted after this boundary committed.
    #[must_use]
    pub fn presentation_emissions(&self) -> &[HostPresentationEmission] {
        &self.presentation_emissions
    }

    /// Returns the complete canonical physical-output disposition roster.
    #[must_use]
    pub fn presentation_dispositions(&self) -> &[HostPresentationDispositionOutcome] {
        &self.presentation_dispositions
    }

    /// Returns all independently reduced surface contribution outcomes in canonical surface order.
    #[must_use]
    pub fn surface_contributions(&self) -> &[SurfaceContributionOutcome] {
        &self.surface_contributions
    }

    /// Returns every presentation authority changed by this tick in stable surface order.
    #[must_use]
    pub fn surface_scene_deltas(&self) -> &[SurfaceSceneDelta] {
        &self.surface_scene_deltas
    }

    /// Iterates logical surfaces whose presentation authority changed.
    pub fn affected_surfaces(&self) -> impl ExactSizeIterator<Item = SurfaceId> + '_ {
        self.surface_scene_deltas
            .iter()
            .map(|delta| delta.surface())
    }

    /// Returns whether any durable or policy state changed.
    #[must_use]
    pub fn changed(&self) -> bool {
        self.before != self.after
    }

    /// Returns whether any published durable, scene, or interaction state changed.
    ///
    /// Consuming inputs or advancing the private writer sequence alone does not
    /// count as a published change.
    #[must_use]
    pub const fn published_state_changed(&self) -> bool {
        self.published_state_changed
    }
}
