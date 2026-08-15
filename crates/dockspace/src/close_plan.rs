//! Revision-bound close plans and exactly-once application decisions.
//!
//! This module stages close intent without mutating the workspace or executing
//! platform effects. The engine supplies an opaque prepared payload containing
//! exact source fingerprints, surface rosters, and target proofs. Adapters see
//! only the stable [`ClosePlan`] view and return typed decisions.

use std::fmt;
use std::num::NonZeroU64;
use std::sync::Arc;

use crate::command::{ContainedPosition, DockTarget};
use crate::effect::EffectId;
use crate::geometry::LogicalRect;
use crate::ids::EngineAuthorityDomainId;
use crate::ids::{FloatingPresentationId, ItemId, RootId, SurfaceId};
use crate::policy::{CloseCapability, PolicyRevision};
use crate::retention::CloseRetentionManifest;
use crate::transition::WorkspaceVersion;
use crate::viewport::{CloseObservationGeneration, InventoryGeneration, ViewportBinding};

macro_rules! close_protocol_id {
    ($name:ident, $description:literal) => {
        #[doc = $description]
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name {
            domain: EngineAuthorityDomainId,
            sequence: NonZeroU64,
        }

        impl $name {
            #[allow(dead_code)]
            pub(crate) const fn domain(self) -> EngineAuthorityDomainId {
                self.domain
            }

            #[allow(dead_code)]
            pub(crate) const fn sequence(self) -> u64 {
                self.sequence.get()
            }

            const fn from_sequence(domain: EngineAuthorityDomainId, sequence: NonZeroU64) -> Self {
                Self { domain, sequence }
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(formatter, "{}:{}", self.domain.get(), self.sequence)
            }
        }
    };
}

close_protocol_id!(
    CloseRequestId,
    "Engine-domain-scoped monotonic identity of one exact close plan."
);
close_protocol_id!(
    CloseDecisionToken,
    "Engine-domain-scoped single-use token for one item's initial close decision."
);
close_protocol_id!(
    DeferredCloseToken,
    "Engine-domain-scoped continuation minted by an accepted deferred decision."
);

/// Engine, workspace, and policy authority against which a close plan was prepared.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CloseAuthority {
    domain: EngineAuthorityDomainId,
    workspace: WorkspaceVersion,
    policy: PolicyRevision,
}

impl CloseAuthority {
    /// Creates one exact close authority boundary.
    #[must_use]
    pub const fn new(
        domain: EngineAuthorityDomainId,
        workspace: WorkspaceVersion,
        policy: PolicyRevision,
    ) -> Self {
        Self {
            domain,
            workspace,
            policy,
        }
    }

    /// Returns the engine authority domain which prepared the plan.
    #[must_use]
    pub const fn domain(self) -> EngineAuthorityDomainId {
        self.domain
    }

    /// Returns the workspace version frozen by the plan.
    #[must_use]
    pub const fn workspace(self) -> WorkspaceVersion {
        self.workspace
    }

    /// Returns the policy revision frozen by the plan.
    #[must_use]
    pub const fn policy(self) -> PolicyRevision {
        self.policy
    }
}

/// Atomic disposition of a complete logical surface roster.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SurfaceCloseDisposition {
    /// Destroy the native binding while retaining the logical surface roster.
    RetainLayout,
    /// Move the complete optional-main plus contained roster to one exact host.
    RehomeAll {
        /// Stable logical target; exact geometry and graph proofs stay core-private.
        target: SurfaceId,
    },
    /// Close every content item in the complete source roster.
    CloseContent,
}

/// Exact target-local geometry for one pre-existing contained presentation.
///
/// The floating identity is the source roster identity and remains stable while
/// it moves to the destination surface. The complete sequence is supplied in
/// source roster order; core rejects any omission, duplication, or reorder.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SurfaceContainedRehomeTarget {
    floating: FloatingPresentationId,
    rect: LogicalRect,
}

impl SurfaceContainedRehomeTarget {
    /// Creates one exact target-local placement for an existing contained root.
    #[must_use]
    pub const fn new(floating: FloatingPresentationId, rect: LogicalRect) -> Self {
        Self { floating, rect }
    }

    /// Returns the stable contained presentation identity from the source roster.
    #[must_use]
    pub const fn floating(self) -> FloatingPresentationId {
        self.floating
    }

    /// Returns the exact destination rectangle in destination-surface coordinates.
    #[must_use]
    pub const fn rect(self) -> LogicalRect {
        self.rect
    }
}

/// Exact presentation destination for the optional main root during a complete rehome.
#[derive(Debug, Clone, PartialEq)]
pub enum SurfaceMainRehomeTarget {
    /// Merge the exact source main node into one exact existing dock target.
    Dock(DockTarget),
    /// Install the exact source main root in a destination rootless main slot.
    Main,
    /// Convert the exact source main root into one explicit contained presentation.
    Contained {
        /// Stable floating identity reserved for the converted main root.
        floating: FloatingPresentationId,
        /// Exact destination rectangle in destination-surface coordinates.
        rect: LogicalRect,
        /// Exact insertion position in the destination contained roster.
        position: ContainedPosition,
    },
}

/// Exact presentation program for rehoming one complete surface roster.
///
/// There is deliberately no target-only shorthand. A source with a main root
/// requires one explicit [`SurfaceMainRehomeTarget`]; every source contained
/// presentation requires exactly one [`SurfaceContainedRehomeTarget`] in its
/// current normative order. This makes the request independently replayable
/// and prevents destruction-time geometry or target selection heuristics.
#[derive(Debug, Clone, PartialEq)]
pub struct SurfaceRehomeTarget {
    surface: SurfaceId,
    main: Option<SurfaceMainRehomeTarget>,
    contained: Vec<SurfaceContainedRehomeTarget>,
}

impl SurfaceRehomeTarget {
    /// Creates an exact complete-roster destination program.
    #[must_use]
    pub fn new(
        surface: SurfaceId,
        main: Option<SurfaceMainRehomeTarget>,
        contained: Vec<SurfaceContainedRehomeTarget>,
    ) -> Self {
        Self {
            surface,
            main,
            contained,
        }
    }

    /// Returns the logical surface receiving the complete roster.
    #[must_use]
    pub const fn surface(&self) -> SurfaceId {
        self.surface
    }

    /// Returns the explicit main-root destination, if the source is rooted.
    #[must_use]
    pub const fn main(&self) -> Option<&SurfaceMainRehomeTarget> {
        self.main.as_ref()
    }

    /// Returns contained destinations in normative source-roster order.
    #[must_use]
    pub fn contained(&self) -> &[SurfaceContainedRehomeTarget] {
        &self.contained
    }
}

/// Explicit application request for resolving one native surface close edge.
#[derive(Debug, Clone, PartialEq)]
pub enum SurfaceCloseRequest {
    /// Destroy the native binding while retaining its logical surface roster.
    RetainLayout,
    /// Move the complete source roster to one exact presentation destination.
    RehomeAll {
        /// Fully typed destination used to prepare the atomic transaction.
        target: SurfaceRehomeTarget,
    },
    /// Close every content item in the complete source roster.
    CloseContent,
}

impl SurfaceCloseRequest {
    /// Returns the stable summary exposed by [`ClosePlanTarget`].
    #[must_use]
    pub fn disposition(&self) -> SurfaceCloseDisposition {
        match self {
            Self::RetainLayout => SurfaceCloseDisposition::RetainLayout,
            Self::RehomeAll { target } => SurfaceCloseDisposition::RehomeAll {
                target: target.surface(),
            },
            Self::CloseContent => SurfaceCloseDisposition::CloseContent,
        }
    }

    /// Returns the logical rehome destination, when this request moves content.
    #[must_use]
    pub fn target_surface(&self) -> Option<SurfaceId> {
        match self {
            Self::RehomeAll { target } => Some(target.surface()),
            Self::RetainLayout | Self::CloseContent => None,
        }
    }
}

/// Exact provider close edge owned by one engine authority domain.
///
/// The provider generation is meaningful only for the exact binding
/// incarnation. The independent inventory generation records when core first
/// received that observation, preventing a pre-effect sample delivered late
/// from satisfying an emission barrier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct NativeCloseEdge {
    domain: EngineAuthorityDomainId,
    binding: ViewportBinding,
    observed_at: CloseObservationGeneration,
    received_at: InventoryGeneration,
}

impl NativeCloseEdge {
    pub(crate) const fn from_authoritative_requested(
        domain: EngineAuthorityDomainId,
        binding: ViewportBinding,
        observed_at: CloseObservationGeneration,
        received_at: InventoryGeneration,
    ) -> Self {
        Self {
            domain,
            binding,
            observed_at,
            received_at,
        }
    }

    /// Returns the engine authority domain which accepted this provider edge.
    #[must_use]
    pub const fn domain(self) -> EngineAuthorityDomainId {
        self.domain
    }

    /// Returns the exact native-window incarnation which requested closure.
    #[must_use]
    pub const fn binding(self) -> ViewportBinding {
        self.binding
    }

    /// Returns the binding-local provider generation of the close request.
    #[must_use]
    pub const fn observed_at(self) -> CloseObservationGeneration {
        self.observed_at
    }

    /// Returns the core inventory generation which first received the edge.
    #[must_use]
    pub const fn received_at(self) -> InventoryGeneration {
        self.received_at
    }
}

/// Stable public identity of the content or surface being closed.
///
/// Exact graph sources, fingerprints, roster records, and recovery proofs are
/// deliberately held in the coordinator's opaque prepared payload.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ClosePlanTarget {
    /// Close one stable pane item.
    Item { item: ItemId },
    /// Close all content in one stable root.
    Root { root: RootId },
    /// Resolve one complete logical-surface close operation.
    Surface {
        surface: SurfaceId,
        disposition: SurfaceCloseDisposition,
    },
}

impl ClosePlanTarget {
    /// Returns the broad target class used by lifecycle validation.
    #[must_use]
    pub const fn kind(self) -> ClosePlanTargetKind {
        match self {
            Self::Item { .. } => ClosePlanTargetKind::Item,
            Self::Root { .. } => ClosePlanTargetKind::Root,
            Self::Surface { .. } => ClosePlanTargetKind::Surface,
        }
    }
}

/// Broad close target class, without target-specific identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ClosePlanTargetKind {
    Item,
    Root,
    Surface,
}

/// One stable item and its policy capability before tokens are allocated.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CloseItemRequirement {
    item: ItemId,
    capability: CloseCapability,
}

impl CloseItemRequirement {
    /// Creates one ordered close-decision requirement.
    #[must_use]
    pub const fn new(item: ItemId, capability: CloseCapability) -> Self {
        Self { item, capability }
    }

    /// Returns the stable content item.
    #[must_use]
    pub const fn item(self) -> ItemId {
        self.item
    }

    /// Returns the capability frozen from the plan's policy revision.
    #[must_use]
    pub const fn capability(self) -> CloseCapability {
        self.capability
    }
}

/// One item in a close plan's normative decision order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClosePlanItem {
    item: ItemId,
    capability: CloseCapability,
    token: CloseDecisionToken,
    state: CloseItemDecisionState,
}

impl ClosePlanItem {
    /// Returns the stable content item.
    #[must_use]
    pub const fn item(self) -> ItemId {
        self.item
    }

    /// Returns the capability frozen from the exact policy revision.
    #[must_use]
    pub const fn capability(self) -> CloseCapability {
        self.capability
    }

    /// Returns the single-use initial decision token.
    #[must_use]
    pub const fn token(self) -> CloseDecisionToken {
        self.token
    }

    /// Returns the latest core-owned application decision state.
    #[must_use]
    pub const fn state(self) -> CloseItemDecisionState {
        self.state
    }
}

/// Observable phase of one close plan.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ClosePlanPhase {
    /// No item decision has been recorded.
    Requested,
    /// At least one item allowed closure and unresolved items remain.
    Resolving,
    /// At least one item owns an unresolved deferred continuation.
    Deferred,
    /// Every required item allowed closure; an atomic commit may be attempted.
    Approved,
    /// The exact native close effect was emitted.
    EffectEmitted,
    /// The effect settled and only exact destruction authority may apply the plan.
    AwaitingDestroyed,
    /// The local commit or destroyed-surface transaction applied atomically.
    Applied,
    /// One item vetoed the complete plan.
    Vetoed,
    /// Workspace or policy authority changed before commit.
    Stale,
    /// Cancellation was requested and awaits its exact external settlement.
    CancelRequested,
    /// Cancellation settled without applying the close.
    Cancelled,
    /// The native binding was authoritatively destroyed, but no exact effect
    /// acknowledgement proved that this plan caused it. The engine must use its
    /// unplanned-destruction recovery path and may never apply this payload.
    ExternallyDestroyedUnproved,
    /// External effect settlement is unknown; no timeout or redispatch is implied.
    Indeterminate,
}

impl ClosePlanPhase {
    /// Returns whether this phase is terminal without target-specific context.
    ///
    /// `Vetoed` is deliberately excluded: an item/root veto is terminal, while a
    /// surface veto still owns the native close edge and must prove cancellation.
    #[must_use]
    pub const fn is_terminal(self) -> bool {
        matches!(
            self,
            Self::Applied | Self::Stale | Self::Cancelled | Self::ExternallyDestroyedUnproved
        )
    }

    /// Returns whether a newer workspace or policy authority may still cancel the plan.
    ///
    /// Once a native effect or cancellation obligation exists, its exact
    /// prepared roster must remain settleable even if unrelated workspace state
    /// advances. The engine still revalidates the prepared graph proof before
    /// applying a destroyed-surface transaction.
    #[must_use]
    pub const fn may_become_stale(self) -> bool {
        matches!(
            self,
            Self::Requested | Self::Resolving | Self::Deferred | Self::Approved
        )
    }
}

/// Immutable application-facing plan view.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClosePlan {
    request: CloseRequestId,
    authority: CloseAuthority,
    target: ClosePlanTarget,
    items: Arc<[ClosePlanItem]>,
    destruction: CloseDestructionState,
    cancellation: CloseCancellationState,
    phase: ClosePlanPhase,
}

impl ClosePlan {
    /// Returns the unique close request identity.
    #[must_use]
    pub const fn request(&self) -> CloseRequestId {
        self.request
    }

    /// Returns the exact workspace and policy authority frozen by the request.
    #[must_use]
    pub const fn authority(&self) -> CloseAuthority {
        self.authority
    }

    /// Returns the stable public target identity.
    #[must_use]
    pub const fn target(&self) -> ClosePlanTarget {
        self.target
    }

    /// Returns required content decisions in normative stable order.
    #[must_use]
    pub fn items(&self) -> &[ClosePlanItem] {
        &self.items
    }

    /// Returns the independently settleable native destruction obligation.
    #[must_use]
    pub const fn destruction_state(&self) -> CloseDestructionState {
        self.destruction
    }

    /// Returns the cancellation observation without hiding destruction state.
    #[must_use]
    pub const fn cancellation_state(&self) -> CloseCancellationState {
        self.cancellation
    }

    /// Returns the current close protocol phase.
    #[must_use]
    pub const fn phase(&self) -> ClosePlanPhase {
        self.phase
    }

    /// Returns whether this exact plan has no remaining semantic or native obligation.
    #[must_use]
    pub fn is_terminal(&self) -> bool {
        self.phase.is_terminal()
            || (self.phase == ClosePlanPhase::Vetoed
                && self.target.kind() != ClosePlanTargetKind::Surface)
    }
}

/// Current availability of one engine-domain close request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClosePlanLookup<'a> {
    /// The complete plan is still retained.
    Detailed(&'a ClosePlan),
    /// The request reached a published terminal state and its payload was compacted.
    RetiredTerminal,
    /// The identity was never allocated by this coordinator.
    Unknown,
}

/// Initial application decision for one exact item token.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CloseDecision {
    Allow,
    Veto,
    Deferred,
}

/// Terminal application decision for one exact deferred continuation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeferredCloseDecision {
    Allow,
    Veto,
}

/// Current application decision recorded for one close-plan item.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CloseItemDecisionState {
    Pending,
    Deferred {
        /// Exact continuation required to finish this deferred decision.
        continuation: DeferredCloseToken,
    },
    Allowed,
    Vetoed,
}

/// Native destruction obligation retained independently from cancellation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CloseDestructionState {
    /// No destructive native effect is outstanding.
    None,
    /// The destructive effect was emitted but not yet acknowledged.
    EffectEmitted,
    /// Effect acknowledgement arrived and exact destruction remains outstanding.
    AwaitingDestroyed,
}

/// Cancellation observation retained independently from native destruction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CloseCancellationState {
    /// No cancellation was requested.
    None,
    /// Cancellation was requested but has not settled.
    Requested,
    /// Cancellation settlement is unknown.
    Indeterminate,
    /// Cancellation settled authoritatively and no destruction obligation remains.
    ///
    /// A surface cancellation always requires a causal platform proof, even when
    /// no destructive close effect was emitted. Local item/root cancellation can
    /// enter this state immediately because it has no native close edge.
    Settled,
}

/// Causal proof that one native close cancellation took effect.
///
/// The proof names the exact engine request, window incarnation, optional
/// destructive effect, optional compensating cancellation effect, and the
/// provider-owned close-lane frontier observed after the applicable causal
/// fence. Core constructs this value only after the provider proves that the
/// named binding is still live and its native close request is cleared.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CloseCancellationProof {
    request: CloseRequestId,
    binding: ViewportBinding,
    close_effect: Option<EffectId>,
    cancel_effect: Option<EffectId>,
    known_effect_frontier: Option<EffectId>,
    observation_generation: CloseObservationGeneration,
    inventory_generation: InventoryGeneration,
}

impl CloseCancellationProof {
    /// Constructs a proof from an already-authoritative live-and-cleared observation.
    ///
    /// This remains core-private so an adapter cannot turn receipt order or an
    /// unversioned boolean into cancellation authority.
    pub(crate) const fn from_authoritative_live_close_cleared(
        request: CloseRequestId,
        binding: ViewportBinding,
        close_effect: Option<EffectId>,
        cancel_effect: Option<EffectId>,
        known_effect_frontier: Option<EffectId>,
        observation_generation: CloseObservationGeneration,
        inventory_generation: InventoryGeneration,
    ) -> Self {
        Self {
            request,
            binding,
            close_effect,
            cancel_effect,
            known_effect_frontier,
            observation_generation,
            inventory_generation,
        }
    }
}

/// Causal proof that one exact native window incarnation was destroyed.
///
/// The proof names the exact engine request and binding observed destroyed at
/// a provider-owned generation after the destructive effect was emitted. Core
/// constructs this value only from an authoritative provider observation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CloseDestroyedProof {
    request: CloseRequestId,
    binding: ViewportBinding,
    observation_generation: CloseObservationGeneration,
    inventory_generation: InventoryGeneration,
    acknowledged_effect: EffectId,
}

impl CloseDestroyedProof {
    /// Constructs a proof from an already-authoritative destroyed observation.
    ///
    /// This remains core-private so adapters cannot synthesize destruction
    /// authority from callback order or an unversioned absence.
    pub(crate) const fn from_authoritative_destroyed(
        request: CloseRequestId,
        binding: ViewportBinding,
        observation_generation: CloseObservationGeneration,
        inventory_generation: InventoryGeneration,
        acknowledged_effect: EffectId,
    ) -> Self {
        Self {
            request,
            binding,
            observation_generation,
            inventory_generation,
            acknowledged_effect,
        }
    }

    /// Returns the exact engine-domain close request proved destroyed.
    #[must_use]
    pub const fn request(self) -> CloseRequestId {
        self.request
    }

    /// Returns the exact native-window incarnation observed destroyed.
    #[must_use]
    pub const fn binding(self) -> ViewportBinding {
        self.binding
    }

    /// Returns the provider generation which observed destruction.
    #[must_use]
    pub const fn observation_generation(self) -> CloseObservationGeneration {
        self.observation_generation
    }

    /// Returns the core inventory generation in which the destroyed fact arrived.
    #[must_use]
    pub const fn inventory_generation(self) -> InventoryGeneration {
        self.inventory_generation
    }

    /// Returns the close-lane effect frontier acknowledged by the provider.
    #[must_use]
    pub const fn acknowledged_effect(self) -> EffectId {
        self.acknowledged_effect
    }
}

/// Authoritative native settlement facts reduced from one provider observation batch.
///
/// The combined variant makes the conflict rule explicit: when destruction and
/// cancellation proof arrive in the same batch, destruction has precedence.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CloseNativeSettlement {
    DestroyedProved(CloseDestroyedProof),
    CancellationProved(CloseCancellationProof),
    DestroyedProvedWithCancellation {
        destroyed: CloseDestroyedProof,
        cancellation: CloseCancellationProof,
    },
}

/// Lifecycle operation rejected because it arrived in the wrong phase or target.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CloseLifecycleAction {
    PrepareCommit,
    InvalidatePrepared,
    ApplyLocalCommit,
    EmitCloseEffect,
    AcknowledgeCloseEffect,
    ApplyDestroyed,
    RequestCancellation,
    EmitCancellationEffect,
    ApplyCancellationProof,
    MarkIndeterminate,
    MarkExternallyDestroyedUnproved,
    ResolveInitialDecision,
    ResolveDeferredDecision,
}

/// Typed reason why a late, duplicate, stale, or malformed input was inert.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CloseInertReason {
    RequestDomainMismatch {
        request: CloseRequestId,
        expected: EngineAuthorityDomainId,
        actual: EngineAuthorityDomainId,
    },
    AuthorityDomainMismatch {
        request: CloseRequestId,
        expected: EngineAuthorityDomainId,
        actual: EngineAuthorityDomainId,
    },
    UnknownRequest {
        request: CloseRequestId,
    },
    /// The request was allocated and reached a published terminal state, but its detailed
    /// payload and copied-token indexes have since been compacted.
    RetiredTerminal {
        request: CloseRequestId,
    },
    AuthorityStale {
        request: CloseRequestId,
        expected: CloseAuthority,
        actual: CloseAuthority,
    },
    Terminal {
        request: CloseRequestId,
        phase: ClosePlanPhase,
    },
    UnknownDecisionToken {
        request: CloseRequestId,
        token: CloseDecisionToken,
    },
    WrongDecisionToken {
        request: CloseRequestId,
        token: CloseDecisionToken,
        owner: CloseRequestId,
    },
    DuplicateDecision {
        request: CloseRequestId,
        item: ItemId,
        state: CloseItemDecisionState,
    },
    DeferredNotAllowed {
        request: CloseRequestId,
        item: ItemId,
        capability: CloseCapability,
    },
    UnknownDeferredToken {
        request: CloseRequestId,
        token: DeferredCloseToken,
    },
    WrongDeferredToken {
        request: CloseRequestId,
        token: DeferredCloseToken,
        owner: CloseRequestId,
    },
    DuplicateDeferredContinuation {
        request: CloseRequestId,
        item: ItemId,
        state: CloseItemDecisionState,
    },
    PhaseMismatch {
        request: CloseRequestId,
        action: CloseLifecycleAction,
        actual: ClosePlanPhase,
    },
    TargetMismatch {
        request: CloseRequestId,
        action: CloseLifecycleAction,
        target: ClosePlanTargetKind,
    },
    CancellationProofRequestMismatch {
        request: CloseRequestId,
        proof_request: CloseRequestId,
    },
    CancellationProofDomainMismatch {
        request: CloseRequestId,
        expected: EngineAuthorityDomainId,
        actual: EngineAuthorityDomainId,
    },
    DestroyedProofRequestMismatch {
        request: CloseRequestId,
        proof_request: CloseRequestId,
    },
    DestroyedProofDomainMismatch {
        request: CloseRequestId,
        expected: EngineAuthorityDomainId,
        actual: EngineAuthorityDomainId,
    },
    NativeBindingMismatch {
        request: CloseRequestId,
        expected: ViewportBinding,
        actual: ViewportBinding,
    },
    NativeSurfaceMismatch {
        request: CloseRequestId,
        expected: SurfaceId,
        actual: SurfaceId,
    },
    NativeEffectMismatch {
        request: CloseRequestId,
        expected: EffectId,
        actual: EffectId,
    },
    NativeEdgeNotObserved {
        request: CloseRequestId,
    },
    CloseEffectNotEmitted {
        request: CloseRequestId,
    },
    NativeEmissionObservationPrecedesEdge {
        request: CloseRequestId,
        edge_observed_at: CloseObservationGeneration,
        issued_after: CloseObservationGeneration,
    },
    NativeEmissionInventoryPrecedesEdge {
        request: CloseRequestId,
        edge_received_at: InventoryGeneration,
        received_after: InventoryGeneration,
    },
    NativeCloseEffectPredecessorMismatch {
        request: CloseRequestId,
        expected: Option<EffectId>,
        actual: Option<EffectId>,
    },
    CancellationEffectPredecessorMismatch {
        request: CloseRequestId,
        expected: Option<EffectId>,
        actual: Option<EffectId>,
    },
    CancellationEffectMismatch {
        request: CloseRequestId,
        expected: Option<EffectId>,
        actual: Option<EffectId>,
    },
    CancellationKnownFrontierMismatch {
        request: CloseRequestId,
        expected: Option<EffectId>,
        actual: Option<EffectId>,
    },
    CancellationObservationNotNewer {
        request: CloseRequestId,
        issued_after: CloseObservationGeneration,
        actual: CloseObservationGeneration,
    },
    CancellationInventoryNotNewer {
        request: CloseRequestId,
        issued_after: InventoryGeneration,
        actual: InventoryGeneration,
    },
    DestroyedObservationNotNewer {
        request: CloseRequestId,
        issued_after: CloseObservationGeneration,
        actual: CloseObservationGeneration,
    },
    DestroyedInventoryNotNewer {
        request: CloseRequestId,
        issued_after: InventoryGeneration,
        actual: InventoryGeneration,
    },
    DestroyedEffectNotAcknowledged {
        request: CloseRequestId,
        close_effect: EffectId,
        cancellation_effect: Option<EffectId>,
        actual: EffectId,
    },
}

/// Result of consuming an initial decision or deferred continuation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CloseResolutionOutcome {
    /// One non-final allow decision was recorded.
    Recorded {
        request: CloseRequestId,
        item: ItemId,
        phase: ClosePlanPhase,
    },
    /// One initial token was exchanged exactly once for a continuation token.
    Deferred {
        request: CloseRequestId,
        item: ItemId,
        continuation: DeferredCloseToken,
    },
    /// The last required allow vote made one atomic commit eligible.
    Approved { request: CloseRequestId },
    /// One veto rejected the semantic close operation without applying topology.
    ///
    /// For a surface plan the native close edge remains active until an exact
    /// cancellation proof settles it.
    Vetoed {
        request: CloseRequestId,
        item: ItemId,
    },
    /// No semantic action, topology mutation, or effect is authorized.
    Inert(CloseInertReason),
}

/// Result of advancing a post-approval local or native lifecycle phase.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CloseAdvanceOutcome {
    Advanced {
        request: CloseRequestId,
        from: ClosePlanPhase,
        to: ClosePlanPhase,
    },
    Inert(CloseInertReason),
}

mod coordinator;

pub(crate) use coordinator::CloseCoordinator;

#[cfg(test)]
use coordinator::CloseCoordinatorError;

#[cfg(test)]
mod tests;
