//! Revision-bound close plans and exactly-once application decisions.
//!
//! This module stages close intent without mutating the workspace or executing
//! platform effects. The engine supplies an opaque prepared payload containing
//! exact source fingerprints, surface rosters, and target proofs. Adapters see
//! only the stable [`ClosePlan`] view and return typed decisions.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::num::NonZeroU64;
use std::sync::Arc;

use thiserror::Error;

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
            /// Returns the engine authority domain which minted this identity.
            #[must_use]
            pub const fn domain(self) -> EngineAuthorityDomainId {
                self.domain
            }

            /// Returns the non-zero sequence within this engine authority domain.
            #[must_use]
            pub const fn sequence(self) -> u64 {
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

    /// Returns the exact engine-domain close request proved cancelled.
    #[must_use]
    pub const fn request(self) -> CloseRequestId {
        self.request
    }

    /// Returns the exact live native-window incarnation observed after cancellation.
    #[must_use]
    pub const fn binding(self) -> ViewportBinding {
        self.binding
    }

    /// Returns the original destructive close effect.
    #[must_use]
    pub const fn close_effect(self) -> Option<EffectId> {
        self.close_effect
    }

    /// Returns the compensating cancellation effect, when core emitted one.
    #[must_use]
    pub const fn cancel_effect(self) -> Option<EffectId> {
        self.cancel_effect
    }

    /// Returns the provider's known close-lane effect frontier.
    ///
    /// `None` is authoritative only when the provider explicitly reports a
    /// known empty frontier; unknown provider authority cannot construct this
    /// proof.
    #[must_use]
    pub const fn known_effect_frontier(self) -> Option<EffectId> {
        self.known_effect_frontier
    }

    /// Returns the provider generation which observed the live, cleared binding.
    #[must_use]
    pub const fn observation_generation(self) -> CloseObservationGeneration {
        self.observation_generation
    }

    /// Returns the core inventory generation in which the provider fact arrived.
    #[must_use]
    pub const fn inventory_generation(self) -> InventoryGeneration {
        self.inventory_generation
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

/// Failure to create a plan or mint a required protocol identity.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub(crate) enum CloseCoordinatorError {
    #[error("close authority domain mismatch: expected {expected:?}, got {actual:?}")]
    AuthorityDomainMismatch {
        expected: EngineAuthorityDomainId,
        actual: EngineAuthorityDomainId,
    },
    #[error("native close edge domain mismatch: expected {expected:?}, got {actual:?}")]
    NativeEdgeDomainMismatch {
        expected: EngineAuthorityDomainId,
        actual: EngineAuthorityDomainId,
    },
    #[error("surface close plan requires an exact native close edge")]
    SurfaceRequiresNativeEdge,
    #[error("close request identity space exhausted")]
    RequestIdExhausted,
    #[error("close decision token identity space exhausted")]
    DecisionTokenExhausted,
    #[error("deferred close token identity space exhausted")]
    DeferredTokenExhausted,
    #[error("close plan contains duplicate item {item}")]
    DuplicateItem { item: ItemId },
    #[error("close plan contains disabled item {item}")]
    DisabledItem { item: ItemId },
    #[error("item close target {target} requires exactly that item, got {actual:?}")]
    ItemTargetMismatch { target: ItemId, actual: Vec<ItemId> },
    #[error("root close target {root} requires a non-empty item decision roster")]
    EmptyRootHasNoItemDecisions { root: RootId },
    #[error("surface close-content target {surface} requires a non-empty item decision roster")]
    SurfaceCloseContentHasNoItemDecisions { surface: SurfaceId },
    #[error("surface rehome target {surface} cannot request {item_count} pane-close decisions")]
    SurfaceRehomeHasItemDecisions {
        surface: SurfaceId,
        item_count: usize,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct TokenOwner {
    request: CloseRequestId,
    item_index: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct NativeCloseCausality {
    binding: ViewportBinding,
    edge_observed_at: CloseObservationGeneration,
    edge_received_at: InventoryGeneration,
    close: Option<NativeEffectCausality>,
    cancellation: Option<NativeEffectCausality>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct NativeEffectCausality {
    effect: EffectId,
    issued_after: CloseObservationGeneration,
    received_after: InventoryGeneration,
    after_effect: Option<EffectId>,
}

/// Core-private pair of a public plan and its exact checked command material.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PreparedClose<P> {
    plan: ClosePlan,
    prepared: P,
    native: Option<NativeCloseCausality>,
    terminal_published: bool,
}

impl<P> PreparedClose<P> {
    const fn plan(&self) -> &ClosePlan {
        &self.plan
    }

    const fn prepared(&self) -> &P {
        &self.prepared
    }

    fn refresh_resolution_phase(&mut self) {
        self.plan.phase = if self
            .plan
            .items
            .iter()
            .all(|item| item.state == CloseItemDecisionState::Allowed)
        {
            ClosePlanPhase::Approved
        } else if self
            .plan
            .items
            .iter()
            .any(|item| matches!(item.state, CloseItemDecisionState::Deferred { .. }))
        {
            ClosePlanPhase::Deferred
        } else if self
            .plan
            .items
            .iter()
            .any(|item| item.state == CloseItemDecisionState::Allowed)
        {
            ClosePlanPhase::Resolving
        } else {
            ClosePlanPhase::Requested
        };
    }
}

/// Borrow proving that the last required vote allowed one exact prepared plan.
pub(crate) struct ApprovedClose<'a, P> {
    plan: &'a ClosePlan,
    prepared: &'a P,
}

impl<'a, P> ApprovedClose<'a, P> {
    pub(crate) const fn plan(&self) -> &'a ClosePlan {
        self.plan
    }

    pub(crate) const fn prepared(&self) -> &'a P {
        self.prepared
    }
}

/// Exactly-once close-plan coordinator with independent monotonic token spaces.
///
/// The coordinator never evaluates frames or elapsed time. Deferred and
/// indeterminate plans remain pending until an exact continuation, authority
/// change, cancellation, or platform fact resolves them.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct CloseCoordinator<P> {
    domain: EngineAuthorityDomainId,
    last_request: u64,
    last_decision: u64,
    last_deferred: u64,
    requests: BTreeMap<CloseRequestId, PreparedClose<P>>,
    decision_owners: BTreeMap<CloseDecisionToken, TokenOwner>,
    deferred_owners: BTreeMap<DeferredCloseToken, TokenOwner>,
}

impl<P> CloseCoordinator<P> {
    pub(crate) const fn new(domain: EngineAuthorityDomainId) -> Self {
        Self {
            domain,
            last_request: 0,
            last_decision: 0,
            last_deferred: 0,
            requests: BTreeMap::new(),
            decision_owners: BTreeMap::new(),
            deferred_owners: BTreeMap::new(),
        }
    }

    /// Accounts for every retained close plan and token ownership entry.
    ///
    /// Terminal plans stay detailed through their successful publication boundary. The next
    /// atomic candidate compacts their payload and copied-token indexes while monotonic identity
    /// frontiers continue to classify late replay.
    pub(crate) fn retention_manifest(&self) -> CloseRetentionManifest {
        let terminal_plan_guards = self
            .requests
            .values()
            .filter(|record| record.plan.is_terminal())
            .count();
        CloseRetentionManifest::new(
            self.requests.len() - terminal_plan_guards,
            terminal_plan_guards,
            self.decision_owners.len(),
            self.deferred_owners.len(),
        )
    }

    /// Freezes one close plan while preserving the supplied item order exactly.
    ///
    /// `prepared` remains core-private and may contain checked graph sources,
    /// complete surface rosters, recovery proofs, and focus dispositions.
    pub(crate) fn open(
        &mut self,
        authority: CloseAuthority,
        target: ClosePlanTarget,
        requirements: impl IntoIterator<Item = CloseItemRequirement>,
        prepared: P,
    ) -> Result<ClosePlan, CloseCoordinatorError> {
        if target.kind() == ClosePlanTargetKind::Surface {
            return Err(CloseCoordinatorError::SurfaceRequiresNativeEdge);
        }
        self.open_unbound(authority, target, requirements, prepared)
    }

    /// Atomically freezes a surface close plan with the exact native edge that
    /// created its external close obligation.
    pub(crate) fn open_surface(
        &mut self,
        authority: CloseAuthority,
        edge: NativeCloseEdge,
        request: SurfaceCloseRequest,
        requirements: impl IntoIterator<Item = CloseItemRequirement>,
        prepared: P,
    ) -> Result<ClosePlan, CloseCoordinatorError> {
        if edge.domain != self.domain {
            return Err(CloseCoordinatorError::NativeEdgeDomainMismatch {
                expected: self.domain,
                actual: edge.domain,
            });
        }

        let plan = self.open_unbound(
            authority,
            ClosePlanTarget::Surface {
                surface: edge.binding.surface(),
                disposition: request.disposition(),
            },
            requirements,
            prepared,
        )?;
        let record = self
            .requests
            .get_mut(&plan.request)
            .expect("a successfully opened close plan is retained");
        record.native = Some(NativeCloseCausality {
            binding: edge.binding,
            edge_observed_at: edge.observed_at,
            edge_received_at: edge.received_at,
            close: None,
            cancellation: None,
        });
        Ok(plan)
    }

    fn open_unbound(
        &mut self,
        authority: CloseAuthority,
        target: ClosePlanTarget,
        requirements: impl IntoIterator<Item = CloseItemRequirement>,
        prepared: P,
    ) -> Result<ClosePlan, CloseCoordinatorError> {
        if authority.domain() != self.domain {
            return Err(CloseCoordinatorError::AuthorityDomainMismatch {
                expected: self.domain,
                actual: authority.domain(),
            });
        }
        let requirements = requirements.into_iter().collect::<Vec<_>>();
        Self::validate_requirements(target, &requirements)?;

        let request_raw = self
            .last_request
            .checked_add(1)
            .and_then(NonZeroU64::new)
            .ok_or(CloseCoordinatorError::RequestIdExhausted)?;
        let decision_count = u64::try_from(requirements.len())
            .map_err(|_| CloseCoordinatorError::DecisionTokenExhausted)?;
        let decision_end = self
            .last_decision
            .checked_add(decision_count)
            .ok_or(CloseCoordinatorError::DecisionTokenExhausted)?;

        let request = CloseRequestId::from_sequence(self.domain, request_raw);
        let mut items = Vec::with_capacity(requirements.len());
        for (offset, requirement) in (1..=decision_count).zip(requirements.iter().copied()) {
            let raw = self
                .last_decision
                .checked_add(offset)
                .and_then(NonZeroU64::new)
                .ok_or(CloseCoordinatorError::DecisionTokenExhausted)?;
            items.push(ClosePlanItem {
                item: requirement.item,
                capability: requirement.capability,
                token: CloseDecisionToken::from_sequence(self.domain, raw),
                state: CloseItemDecisionState::Pending,
            });
        }

        let phase = if items.is_empty() {
            ClosePlanPhase::Approved
        } else {
            ClosePlanPhase::Requested
        };
        let plan = ClosePlan {
            request,
            authority,
            target,
            items: Arc::from(items),
            destruction: CloseDestructionState::None,
            cancellation: CloseCancellationState::None,
            phase,
        };

        self.last_request = request.sequence();
        self.last_decision = decision_end;
        for (item_index, item) in plan.items.iter().copied().enumerate() {
            self.decision_owners.insert(
                item.token,
                TokenOwner {
                    request,
                    item_index,
                },
            );
        }
        self.requests.insert(
            request,
            PreparedClose {
                plan: plan.clone(),
                prepared,
                native: None,
                terminal_published: false,
            },
        );

        Ok(plan)
    }

    /// Returns the latest public view for a known request.
    pub(crate) fn plan(&self, request: CloseRequestId) -> Option<&ClosePlan> {
        self.requests.get(&request).map(PreparedClose::plan)
    }

    pub(crate) fn lookup(&self, request: CloseRequestId) -> ClosePlanLookup<'_> {
        match self.plan(request) {
            Some(plan) => ClosePlanLookup::Detailed(plan),
            None if self.request_was_retired_terminal(request) => ClosePlanLookup::RetiredTerminal,
            None => ClosePlanLookup::Unknown,
        }
    }

    /// Returns the exact core-private payload frozen for one known close plan.
    ///
    /// Callers use this after an effect barrier or an authoritative native
    /// settlement to apply the already-validated transaction, never to
    /// reconstruct a close operation from current workspace state.
    pub(crate) fn prepared(&self, request: CloseRequestId) -> Option<&P> {
        self.requests.get(&request).map(PreparedClose::prepared)
    }

    /// Returns every retained plan in stable request order.
    pub(crate) fn plans(&self) -> impl Iterator<Item = &ClosePlan> {
        self.requests.values().map(PreparedClose::plan)
    }

    /// Marks terminal snapshots visible at this successful publication boundary.
    pub(crate) fn mark_boundary_published(&mut self) {
        for record in self.requests.values_mut() {
            if record.plan.is_terminal() {
                record.terminal_published = true;
            }
        }
    }

    /// Removes detailed terminal state which was visible at an earlier publication boundary.
    ///
    /// Monotonic request and token frontiers remain authoritative replay guards. Compaction is
    /// performed only on an unpublished engine candidate, so a failed transaction cannot make
    /// terminal history disappear from the live engine.
    pub(crate) fn compact_published_terminal(&mut self) -> usize {
        let retired = self
            .requests
            .iter()
            .filter_map(|(request, record)| {
                (record.terminal_published && record.plan.is_terminal()).then_some(*request)
            })
            .collect::<BTreeSet<_>>();
        if retired.is_empty() {
            return 0;
        }

        self.requests
            .retain(|request, _| !retired.contains(request));
        self.decision_owners
            .retain(|_, owner| !retired.contains(&owner.request));
        self.deferred_owners
            .retain(|_, owner| !retired.contains(&owner.request));
        retired.len()
    }

    fn request_was_retired_terminal(&self, request: CloseRequestId) -> bool {
        request.domain() == self.domain
            && request.sequence() <= self.last_request
            && !self.requests.contains_key(&request)
    }

    /// Returns every non-terminal plan in stable request order.
    pub(crate) fn active_plans(&self) -> impl Iterator<Item = &ClosePlan> {
        self.plans().filter(|plan| !plan.is_terminal())
    }

    pub(crate) fn extend_referenced_effects(&self, effects: &mut BTreeSet<EffectId>) {
        for native in self.requests.values().filter_map(|record| record.native) {
            for causality in [native.close, native.cancellation].into_iter().flatten() {
                effects.insert(causality.effect);
                effects.extend(causality.after_effect);
            }
        }
    }

    /// Returns the newest reusable plan for one exact target and authority.
    ///
    /// UI retries may reuse only a non-terminal request derived from the same
    /// workspace and policy revisions. An authority change requires a newly
    /// prepared plan rather than reinterpreting frozen item capabilities.
    pub(crate) fn active_plan_for_target(
        &self,
        target: ClosePlanTarget,
        authority: CloseAuthority,
    ) -> Option<&ClosePlan> {
        self.requests.values().rev().find_map(|record| {
            let plan = record.plan();
            (plan.target == target && plan.authority == authority && !plan.is_terminal())
                .then_some(plan)
        })
    }

    /// Returns the active surface plan for one exact provider close edge.
    ///
    /// A native binding can observe several close edges over its lifetime. A
    /// superseded edge must never block, settle, or otherwise select a plan
    /// prepared for a later edge on the same binding.
    pub(crate) fn surface_request_for_edge(&self, edge: NativeCloseEdge) -> Option<CloseRequestId> {
        self.requests.values().rev().find_map(|record| {
            (!record.plan.is_terminal() && self.record_native_edge(record) == Some(edge))
                .then_some(record.plan.request)
        })
    }

    /// Returns every non-terminal surface plan still associated with one native binding.
    ///
    /// This is only for terminal cleanup after a typed destruction fact. New
    /// requests and normal settlement must use [`Self::surface_request_for_edge`]
    /// or [`Self::surface_request_for_effect`].
    pub(crate) fn active_surface_requests_for_binding(
        &self,
        binding: ViewportBinding,
    ) -> Vec<CloseRequestId> {
        self.requests
            .values()
            .filter_map(|record| {
                (!record.plan.is_terminal()
                    && record
                        .native
                        .is_some_and(|native| native.binding == binding))
                .then_some(record.plan.request)
            })
            .collect()
    }

    /// Returns the active surface plan named by one exact provider effect acknowledgement.
    ///
    /// Effect identities are engine-global, so this is unambiguous even while
    /// a binding has retained indeterminate plans from older close edges.
    pub(crate) fn surface_request_for_effect(
        &self,
        binding: ViewportBinding,
        effect: EffectId,
    ) -> Option<CloseRequestId> {
        self.requests.values().rev().find_map(|record| {
            let native = record.native?;
            (!record.plan.is_terminal()
                && native.binding == binding
                && (native.close.is_some_and(|close| close.effect == effect)
                    || native
                        .cancellation
                        .is_some_and(|cancellation| cancellation.effect == effect)))
            .then_some(record.plan.request)
        })
    }

    /// Returns the exact native edge frozen into a surface plan.
    pub(crate) fn native_edge(&self, request: CloseRequestId) -> Option<NativeCloseEdge> {
        self.requests
            .get(&request)
            .and_then(|record| self.record_native_edge(record))
    }

    fn record_native_edge(&self, record: &PreparedClose<P>) -> Option<NativeCloseEdge> {
        let native = record.native?;
        Some(NativeCloseEdge {
            domain: self.domain,
            binding: native.binding,
            observed_at: native.edge_observed_at,
            received_at: native.edge_received_at,
        })
    }

    /// Returns the emitted destructive native-close effect, when any.
    pub(crate) fn native_close_effect(&self, request: CloseRequestId) -> Option<EffectId> {
        self.requests
            .get(&request)?
            .native?
            .close
            .map(|effect| effect.effect)
    }

    /// Returns the emitted native-close cancellation effect, when any.
    pub(crate) fn native_cancellation_effect(&self, request: CloseRequestId) -> Option<EffectId> {
        self.requests
            .get(&request)?
            .native?
            .cancellation
            .map(|effect| effect.effect)
    }

    /// Consumes one exact initial decision token.
    pub(crate) fn resolve(
        &mut self,
        request: CloseRequestId,
        token: CloseDecisionToken,
        authority: CloseAuthority,
        decision: CloseDecision,
    ) -> Result<CloseResolutionOutcome, CloseCoordinatorError> {
        if let Err(reason) = self.guard_request(request, authority) {
            return Ok(CloseResolutionOutcome::Inert(reason));
        }

        let Some(owner) = self.decision_owners.get(&token).copied() else {
            return Ok(CloseResolutionOutcome::Inert(
                CloseInertReason::UnknownDecisionToken { request, token },
            ));
        };
        if owner.request != request {
            return Ok(CloseResolutionOutcome::Inert(
                CloseInertReason::WrongDecisionToken {
                    request,
                    token,
                    owner: owner.request,
                },
            ));
        }

        let phase = self.requests.get(&request).map(|record| record.plan.phase);
        if !phase.is_some_and(|phase| {
            matches!(
                phase,
                ClosePlanPhase::Requested
                    | ClosePlanPhase::Resolving
                    | ClosePlanPhase::Deferred
                    | ClosePlanPhase::Approved
            )
        }) {
            return Ok(CloseResolutionOutcome::Inert(
                CloseInertReason::PhaseMismatch {
                    request,
                    action: CloseLifecycleAction::ResolveInitialDecision,
                    actual: phase.unwrap_or(ClosePlanPhase::Stale),
                },
            ));
        }

        let Some((item, capability, state)) = self.item_snapshot(request, owner.item_index) else {
            return Ok(CloseResolutionOutcome::Inert(
                CloseInertReason::UnknownDecisionToken { request, token },
            ));
        };
        if state != CloseItemDecisionState::Pending {
            return Ok(CloseResolutionOutcome::Inert(
                CloseInertReason::DuplicateDecision {
                    request,
                    item,
                    state,
                },
            ));
        }

        match decision {
            CloseDecision::Allow => {
                let Some(record) = self.requests.get_mut(&request) else {
                    return Ok(CloseResolutionOutcome::Inert(
                        CloseInertReason::UnknownRequest { request },
                    ));
                };
                Arc::make_mut(&mut record.plan.items)[owner.item_index].state =
                    CloseItemDecisionState::Allowed;
                record.refresh_resolution_phase();
                if record.plan.phase == ClosePlanPhase::Approved {
                    Ok(CloseResolutionOutcome::Approved { request })
                } else {
                    Ok(CloseResolutionOutcome::Recorded {
                        request,
                        item,
                        phase: record.plan.phase,
                    })
                }
            }
            CloseDecision::Veto => {
                let Some(record) = self.requests.get_mut(&request) else {
                    return Ok(CloseResolutionOutcome::Inert(
                        CloseInertReason::UnknownRequest { request },
                    ));
                };
                Arc::make_mut(&mut record.plan.items)[owner.item_index].state =
                    CloseItemDecisionState::Vetoed;
                record.plan.phase = ClosePlanPhase::Vetoed;
                Ok(CloseResolutionOutcome::Vetoed { request, item })
            }
            CloseDecision::Deferred => {
                if !capability.allows_deferred() {
                    return Ok(CloseResolutionOutcome::Inert(
                        CloseInertReason::DeferredNotAllowed {
                            request,
                            item,
                            capability,
                        },
                    ));
                }
                let continuation = self.mint_deferred()?;
                let Some(record) = self.requests.get_mut(&request) else {
                    return Ok(CloseResolutionOutcome::Inert(
                        CloseInertReason::UnknownRequest { request },
                    ));
                };
                Arc::make_mut(&mut record.plan.items)[owner.item_index].state =
                    CloseItemDecisionState::Deferred { continuation };
                record.refresh_resolution_phase();
                self.deferred_owners.insert(continuation, owner);
                Ok(CloseResolutionOutcome::Deferred {
                    request,
                    item,
                    continuation,
                })
            }
        }
    }

    /// Consumes one exact deferred continuation token.
    pub(crate) fn continue_deferred(
        &mut self,
        request: CloseRequestId,
        token: DeferredCloseToken,
        authority: CloseAuthority,
        decision: DeferredCloseDecision,
    ) -> CloseResolutionOutcome {
        if let Err(reason) = self.guard_request(request, authority) {
            return CloseResolutionOutcome::Inert(reason);
        }

        let Some(owner) = self.deferred_owners.get(&token).copied() else {
            return CloseResolutionOutcome::Inert(CloseInertReason::UnknownDeferredToken {
                request,
                token,
            });
        };
        if owner.request != request {
            return CloseResolutionOutcome::Inert(CloseInertReason::WrongDeferredToken {
                request,
                token,
                owner: owner.request,
            });
        }

        let phase = self.requests.get(&request).map(|record| record.plan.phase);
        if !phase.is_some_and(|phase| {
            matches!(
                phase,
                ClosePlanPhase::Deferred | ClosePlanPhase::Resolving | ClosePlanPhase::Approved
            )
        }) {
            return CloseResolutionOutcome::Inert(CloseInertReason::PhaseMismatch {
                request,
                action: CloseLifecycleAction::ResolveDeferredDecision,
                actual: phase.unwrap_or(ClosePlanPhase::Stale),
            });
        }

        let Some((item, _, state)) = self.item_snapshot(request, owner.item_index) else {
            return CloseResolutionOutcome::Inert(CloseInertReason::UnknownDeferredToken {
                request,
                token,
            });
        };
        if !matches!(
            state,
            CloseItemDecisionState::Deferred { continuation } if continuation == token
        ) {
            return CloseResolutionOutcome::Inert(
                CloseInertReason::DuplicateDeferredContinuation {
                    request,
                    item,
                    state,
                },
            );
        }

        let Some(record) = self.requests.get_mut(&request) else {
            return CloseResolutionOutcome::Inert(CloseInertReason::UnknownRequest { request });
        };
        match decision {
            DeferredCloseDecision::Allow => {
                Arc::make_mut(&mut record.plan.items)[owner.item_index].state =
                    CloseItemDecisionState::Allowed;
                record.refresh_resolution_phase();
                if record.plan.phase == ClosePlanPhase::Approved {
                    CloseResolutionOutcome::Approved { request }
                } else {
                    CloseResolutionOutcome::Recorded {
                        request,
                        item,
                        phase: record.plan.phase,
                    }
                }
            }
            DeferredCloseDecision::Veto => {
                Arc::make_mut(&mut record.plan.items)[owner.item_index].state =
                    CloseItemDecisionState::Vetoed;
                record.plan.phase = ClosePlanPhase::Vetoed;
                CloseResolutionOutcome::Vetoed { request, item }
            }
        }
    }

    /// Borrows the exact prepared payload only while the plan is approved and current.
    pub(crate) fn approved(
        &mut self,
        request: CloseRequestId,
        authority: CloseAuthority,
    ) -> Result<ApprovedClose<'_, P>, CloseInertReason> {
        self.guard_request(request, authority)?;
        let phase = self.requests.get(&request).map(|record| record.plan.phase);
        if phase != Some(ClosePlanPhase::Approved) {
            return Err(CloseInertReason::PhaseMismatch {
                request,
                action: CloseLifecycleAction::PrepareCommit,
                actual: phase.unwrap_or(ClosePlanPhase::Stale),
            });
        }
        let record = self
            .requests
            .get(&request)
            .ok_or(CloseInertReason::UnknownRequest { request })?;
        Ok(ApprovedClose {
            plan: record.plan(),
            prepared: record.prepared(),
        })
    }

    /// Invalidates a prepared plan after its final checked commit is rejected.
    ///
    /// Item and root plans become stale only before an external effect creates
    /// an irreversible recovery obligation. A surface plan instead preserves
    /// that obligation and requests explicit cancellation, regardless of
    /// whether its close effect was already emitted.
    pub(crate) fn mark_stale(
        &mut self,
        request: CloseRequestId,
        authority: CloseAuthority,
    ) -> CloseAdvanceOutcome {
        if let Err(reason) = self.guard_request(request, authority) {
            return CloseAdvanceOutcome::Inert(reason);
        }
        let Some(record) = self.requests.get(&request) else {
            return CloseAdvanceOutcome::Inert(CloseInertReason::UnknownRequest { request });
        };
        if record.plan.target.kind() == ClosePlanTargetKind::Surface {
            return self.request_cancel(request, authority);
        }
        self.advance_exact(
            request,
            CloseLifecycleAction::InvalidatePrepared,
            &[
                ClosePlanPhase::Requested,
                ClosePlanPhase::Resolving,
                ClosePlanPhase::Deferred,
                ClosePlanPhase::Approved,
            ],
            ClosePlanPhase::Stale,
        )
    }

    /// Marks a successful item/root topology transaction as applied.
    pub(crate) fn mark_local_applied(
        &mut self,
        request: CloseRequestId,
        authority: CloseAuthority,
    ) -> CloseAdvanceOutcome {
        if let Err(reason) = self.guard_request(request, authority) {
            return CloseAdvanceOutcome::Inert(reason);
        }
        let Some(record) = self.requests.get(&request) else {
            return CloseAdvanceOutcome::Inert(CloseInertReason::UnknownRequest { request });
        };
        if record.plan.target.kind() == ClosePlanTargetKind::Surface {
            return CloseAdvanceOutcome::Inert(CloseInertReason::TargetMismatch {
                request,
                action: CloseLifecycleAction::ApplyLocalCommit,
                target: ClosePlanTargetKind::Surface,
            });
        }
        self.advance_exact(
            request,
            CloseLifecycleAction::ApplyLocalCommit,
            &[ClosePlanPhase::Approved],
            ClosePlanPhase::Applied,
        )
    }

    /// Records emission of the exact native close effect for a surface plan.
    pub(crate) fn mark_effect_emitted(
        &mut self,
        request: CloseRequestId,
        authority: CloseAuthority,
        binding: ViewportBinding,
        effect: EffectId,
        issued_after: CloseObservationGeneration,
        received_after: InventoryGeneration,
        after_effect: Option<EffectId>,
    ) -> CloseAdvanceOutcome {
        if let Err(reason) = self.guard_request(request, authority) {
            return CloseAdvanceOutcome::Inert(reason);
        }
        let Some(record) = self.requests.get_mut(&request) else {
            return CloseAdvanceOutcome::Inert(CloseInertReason::UnknownRequest { request });
        };
        let ClosePlanTarget::Surface { surface, .. } = record.plan.target else {
            return CloseAdvanceOutcome::Inert(CloseInertReason::TargetMismatch {
                request,
                action: CloseLifecycleAction::EmitCloseEffect,
                target: record.plan.target.kind(),
            });
        };
        if binding.surface() != surface {
            return CloseAdvanceOutcome::Inert(CloseInertReason::NativeSurfaceMismatch {
                request,
                expected: surface,
                actual: binding.surface(),
            });
        }
        if record.plan.phase != ClosePlanPhase::Approved {
            return CloseAdvanceOutcome::Inert(CloseInertReason::PhaseMismatch {
                request,
                action: CloseLifecycleAction::EmitCloseEffect,
                actual: record.plan.phase,
            });
        }
        let Some(native) = record.native.as_mut() else {
            return CloseAdvanceOutcome::Inert(CloseInertReason::NativeEdgeNotObserved { request });
        };
        if native.binding != binding {
            return CloseAdvanceOutcome::Inert(CloseInertReason::NativeBindingMismatch {
                request,
                expected: native.binding,
                actual: binding,
            });
        }
        if issued_after < native.edge_observed_at {
            return CloseAdvanceOutcome::Inert(
                CloseInertReason::NativeEmissionObservationPrecedesEdge {
                    request,
                    edge_observed_at: native.edge_observed_at,
                    issued_after,
                },
            );
        }
        if received_after < native.edge_received_at {
            return CloseAdvanceOutcome::Inert(
                CloseInertReason::NativeEmissionInventoryPrecedesEdge {
                    request,
                    edge_received_at: native.edge_received_at,
                    received_after,
                },
            );
        }
        if after_effect.is_some() {
            return CloseAdvanceOutcome::Inert(
                CloseInertReason::NativeCloseEffectPredecessorMismatch {
                    request,
                    expected: None,
                    actual: after_effect,
                },
            );
        }
        if native.close.is_some() {
            return CloseAdvanceOutcome::Inert(CloseInertReason::PhaseMismatch {
                request,
                action: CloseLifecycleAction::EmitCloseEffect,
                actual: record.plan.phase,
            });
        }
        let from = record.plan.phase;
        native.close = Some(NativeEffectCausality {
            effect,
            issued_after,
            received_after,
            after_effect,
        });
        record.plan.destruction = CloseDestructionState::EffectEmitted;
        record.plan.phase = ClosePlanPhase::EffectEmitted;
        CloseAdvanceOutcome::Advanced {
            request,
            from,
            to: ClosePlanPhase::EffectEmitted,
        }
    }

    /// Applies a surface plan only after the caller validates exact destruction proof.
    fn mark_destroyed_applied(
        &mut self,
        request: CloseRequestId,
        authority: CloseAuthority,
    ) -> CloseAdvanceOutcome {
        if let Err(reason) = self.guard_request(request, authority) {
            return CloseAdvanceOutcome::Inert(reason);
        }
        let Some(record) = self.requests.get(&request) else {
            return CloseAdvanceOutcome::Inert(CloseInertReason::UnknownRequest { request });
        };
        if record.plan.target.kind() != ClosePlanTargetKind::Surface {
            return CloseAdvanceOutcome::Inert(CloseInertReason::TargetMismatch {
                request,
                action: CloseLifecycleAction::ApplyDestroyed,
                target: record.plan.target.kind(),
            });
        }
        if record.plan.destruction == CloseDestructionState::None {
            return CloseAdvanceOutcome::Inert(CloseInertReason::PhaseMismatch {
                request,
                action: CloseLifecycleAction::ApplyDestroyed,
                actual: record.plan.phase,
            });
        }
        let Some(record) = self.requests.get_mut(&request) else {
            return CloseAdvanceOutcome::Inert(CloseInertReason::UnknownRequest { request });
        };
        let from = record.plan.phase;
        record.plan.destruction = CloseDestructionState::None;
        record.plan.cancellation = CloseCancellationState::None;
        record.plan.phase = ClosePlanPhase::Applied;
        CloseAdvanceOutcome::Advanced {
            request,
            from,
            to: ClosePlanPhase::Applied,
        }
    }

    /// Requests explicit cancellation without inferring a timeout.
    ///
    /// Item and root plans terminate immediately because they have no platform
    /// settlement. A surface plan always owns an observed native close edge, so
    /// cancellation remains pending until the provider proves the edge cleared.
    pub(crate) fn request_cancel(
        &mut self,
        request: CloseRequestId,
        authority: CloseAuthority,
    ) -> CloseAdvanceOutcome {
        if let Err(reason) = self.guard_request(request, authority) {
            return CloseAdvanceOutcome::Inert(reason);
        }
        let Some(record) = self.requests.get_mut(&request) else {
            return CloseAdvanceOutcome::Inert(CloseInertReason::UnknownRequest { request });
        };
        if record.plan.target.kind() != ClosePlanTargetKind::Surface {
            let from = record.plan.phase;
            record.plan.cancellation = CloseCancellationState::Settled;
            record.plan.phase = ClosePlanPhase::Cancelled;
            return CloseAdvanceOutcome::Advanced {
                request,
                from,
                to: ClosePlanPhase::Cancelled,
            };
        }
        if record.native.is_none() {
            return CloseAdvanceOutcome::Inert(CloseInertReason::NativeEdgeNotObserved { request });
        }
        if record.plan.cancellation != CloseCancellationState::None {
            return CloseAdvanceOutcome::Inert(CloseInertReason::PhaseMismatch {
                request,
                action: CloseLifecycleAction::RequestCancellation,
                actual: record.plan.phase,
            });
        }
        let from = record.plan.phase;
        record.plan.cancellation = CloseCancellationState::Requested;
        record.plan.phase = ClosePlanPhase::CancelRequested;
        CloseAdvanceOutcome::Advanced {
            request,
            from,
            to: ClosePlanPhase::CancelRequested,
        }
    }

    /// Freezes the exact compensating effect and the last provider observation
    /// which causally preceded its emission.
    pub(crate) fn mark_cancellation_effect_emitted(
        &mut self,
        request: CloseRequestId,
        authority: CloseAuthority,
        binding: ViewportBinding,
        effect: EffectId,
        issued_after: CloseObservationGeneration,
        received_after: InventoryGeneration,
        after_effect: Option<EffectId>,
    ) -> CloseAdvanceOutcome {
        if let Err(reason) = self.guard_request(request, authority) {
            return CloseAdvanceOutcome::Inert(reason);
        }
        let Some(record) = self.requests.get_mut(&request) else {
            return CloseAdvanceOutcome::Inert(CloseInertReason::UnknownRequest { request });
        };
        if record.plan.target.kind() != ClosePlanTargetKind::Surface {
            return CloseAdvanceOutcome::Inert(CloseInertReason::TargetMismatch {
                request,
                action: CloseLifecycleAction::EmitCancellationEffect,
                target: record.plan.target.kind(),
            });
        }
        if !matches!(
            record.plan.cancellation,
            CloseCancellationState::Requested | CloseCancellationState::Indeterminate
        ) {
            return CloseAdvanceOutcome::Inert(CloseInertReason::PhaseMismatch {
                request,
                action: CloseLifecycleAction::EmitCancellationEffect,
                actual: record.plan.phase,
            });
        }
        let Some(native) = record.native.as_mut() else {
            return CloseAdvanceOutcome::Inert(CloseInertReason::NativeEdgeNotObserved { request });
        };
        if binding != native.binding {
            return CloseAdvanceOutcome::Inert(CloseInertReason::NativeBindingMismatch {
                request,
                expected: native.binding,
                actual: binding,
            });
        }
        if issued_after < native.edge_observed_at {
            return CloseAdvanceOutcome::Inert(
                CloseInertReason::NativeEmissionObservationPrecedesEdge {
                    request,
                    edge_observed_at: native.edge_observed_at,
                    issued_after,
                },
            );
        }
        if received_after < native.edge_received_at {
            return CloseAdvanceOutcome::Inert(
                CloseInertReason::NativeEmissionInventoryPrecedesEdge {
                    request,
                    edge_received_at: native.edge_received_at,
                    received_after,
                },
            );
        }
        let expected_predecessor = native.close.map(|close| close.effect);
        if after_effect != expected_predecessor {
            return CloseAdvanceOutcome::Inert(
                CloseInertReason::CancellationEffectPredecessorMismatch {
                    request,
                    expected: expected_predecessor,
                    actual: after_effect,
                },
            );
        }
        if let Some(existing) = native.cancellation {
            return CloseAdvanceOutcome::Inert(CloseInertReason::NativeEffectMismatch {
                request,
                expected: existing.effect,
                actual: effect,
            });
        }

        let phase = record.plan.phase;
        native.cancellation = Some(NativeEffectCausality {
            effect,
            issued_after,
            received_after,
            after_effect,
        });
        CloseAdvanceOutcome::Advanced {
            request,
            from: phase,
            to: phase,
        }
    }

    /// Reduces authoritative native settlement from one provider observation batch.
    ///
    /// Destruction wins when both facts occur in the same batch. Callers must not
    /// split one batch into multiple calls because doing so would discard this
    /// explicit ordering rule.
    pub(crate) fn settle_native(
        &mut self,
        request: CloseRequestId,
        authority: CloseAuthority,
        settlement: CloseNativeSettlement,
    ) -> CloseAdvanceOutcome {
        if let Err(reason) = self.guard_request(request, authority) {
            return CloseAdvanceOutcome::Inert(reason);
        }

        match settlement {
            CloseNativeSettlement::DestroyedProved(proof) => {
                if let Err(reason) = self.validate_destroyed_proof(request, proof) {
                    return CloseAdvanceOutcome::Inert(reason);
                }
                self.mark_destroyed_applied(request, authority)
            }
            CloseNativeSettlement::CancellationProved(proof) => {
                self.apply_cancellation_proof(request, authority, proof)
            }
            CloseNativeSettlement::DestroyedProvedWithCancellation {
                destroyed,
                cancellation,
            } => {
                if let Err(reason) = self.validate_destroyed_proof(request, destroyed) {
                    return CloseAdvanceOutcome::Inert(reason);
                }
                if let Err(reason) = self.validate_cancellation_proof(request, cancellation) {
                    return CloseAdvanceOutcome::Inert(reason);
                }
                self.mark_destroyed_applied(request, authority)
            }
        }
    }

    fn validate_destroyed_proof(
        &self,
        request: CloseRequestId,
        proof: CloseDestroyedProof,
    ) -> Result<(), CloseInertReason> {
        if proof.request.domain() != self.domain {
            return Err(CloseInertReason::DestroyedProofDomainMismatch {
                request,
                expected: self.domain,
                actual: proof.request.domain(),
            });
        }
        if proof.request != request {
            return Err(CloseInertReason::DestroyedProofRequestMismatch {
                request,
                proof_request: proof.request,
            });
        }
        let Some(record) = self.requests.get(&request) else {
            return Err(CloseInertReason::UnknownRequest { request });
        };
        if record.plan.target.kind() != ClosePlanTargetKind::Surface {
            return Err(CloseInertReason::TargetMismatch {
                request,
                action: CloseLifecycleAction::ApplyDestroyed,
                target: record.plan.target.kind(),
            });
        }
        let Some(native) = record.native else {
            return Err(CloseInertReason::PhaseMismatch {
                request,
                action: CloseLifecycleAction::ApplyDestroyed,
                actual: record.plan.phase,
            });
        };
        if proof.binding != native.binding {
            return Err(CloseInertReason::NativeBindingMismatch {
                request,
                expected: native.binding,
                actual: proof.binding,
            });
        }
        let Some(close) = native.close else {
            return Err(CloseInertReason::CloseEffectNotEmitted { request });
        };
        if record.plan.destruction == CloseDestructionState::None {
            return Err(CloseInertReason::PhaseMismatch {
                request,
                action: CloseLifecycleAction::ApplyDestroyed,
                actual: record.plan.phase,
            });
        }
        if proof.observation_generation <= close.issued_after {
            return Err(CloseInertReason::DestroyedObservationNotNewer {
                request,
                issued_after: close.issued_after,
                actual: proof.observation_generation,
            });
        }
        if proof.inventory_generation <= close.received_after {
            return Err(CloseInertReason::DestroyedInventoryNotNewer {
                request,
                issued_after: close.received_after,
                actual: proof.inventory_generation,
            });
        }
        let cancellation_effect = native.cancellation.map(|cancellation| cancellation.effect);
        if proof.acknowledged_effect != close.effect
            && cancellation_effect != Some(proof.acknowledged_effect)
        {
            return Err(CloseInertReason::DestroyedEffectNotAcknowledged {
                request,
                close_effect: close.effect,
                cancellation_effect,
                actual: proof.acknowledged_effect,
            });
        }
        Ok(())
    }

    fn apply_cancellation_proof(
        &mut self,
        request: CloseRequestId,
        authority: CloseAuthority,
        proof: CloseCancellationProof,
    ) -> CloseAdvanceOutcome {
        if let Err(reason) = self.guard_request(request, authority) {
            return CloseAdvanceOutcome::Inert(reason);
        }
        if let Err(reason) = self.validate_cancellation_proof(request, proof) {
            return CloseAdvanceOutcome::Inert(reason);
        }

        let Some(record) = self.requests.get_mut(&request) else {
            return CloseAdvanceOutcome::Inert(CloseInertReason::UnknownRequest { request });
        };
        let from = record.plan.phase;
        record.plan.destruction = CloseDestructionState::None;
        record.plan.cancellation = CloseCancellationState::Settled;
        record.plan.phase = ClosePlanPhase::Cancelled;
        CloseAdvanceOutcome::Advanced {
            request,
            from,
            to: ClosePlanPhase::Cancelled,
        }
    }

    fn validate_cancellation_proof(
        &self,
        request: CloseRequestId,
        proof: CloseCancellationProof,
    ) -> Result<(), CloseInertReason> {
        if proof.request.domain() != self.domain {
            return Err(CloseInertReason::CancellationProofDomainMismatch {
                request,
                expected: self.domain,
                actual: proof.request.domain(),
            });
        }
        if proof.request != request {
            return Err(CloseInertReason::CancellationProofRequestMismatch {
                request,
                proof_request: proof.request,
            });
        }
        let Some(record) = self.requests.get(&request) else {
            return Err(CloseInertReason::UnknownRequest { request });
        };
        if record.plan.target.kind() != ClosePlanTargetKind::Surface {
            return Err(CloseInertReason::TargetMismatch {
                request,
                action: CloseLifecycleAction::ApplyCancellationProof,
                target: record.plan.target.kind(),
            });
        }
        if !matches!(
            record.plan.cancellation,
            CloseCancellationState::Requested | CloseCancellationState::Indeterminate
        ) {
            return Err(CloseInertReason::PhaseMismatch {
                request,
                action: CloseLifecycleAction::ApplyCancellationProof,
                actual: record.plan.phase,
            });
        }
        let Some(native) = record.native else {
            return Err(CloseInertReason::PhaseMismatch {
                request,
                action: CloseLifecycleAction::ApplyCancellationProof,
                actual: record.plan.phase,
            });
        };
        if proof.binding != native.binding {
            return Err(CloseInertReason::NativeBindingMismatch {
                request,
                expected: native.binding,
                actual: proof.binding,
            });
        }
        let expected_close_effect = native.close.map(|close| close.effect);
        let expected_cancel_effect = native.cancellation.map(|cancellation| cancellation.effect);
        if native
            .cancellation
            .is_some_and(|cancellation| cancellation.after_effect != expected_close_effect)
        {
            return Err(CloseInertReason::CancellationEffectPredecessorMismatch {
                request,
                expected: expected_close_effect,
                actual: native
                    .cancellation
                    .and_then(|cancellation| cancellation.after_effect),
            });
        }
        if proof.close_effect != expected_close_effect {
            return Err(CloseInertReason::CancellationEffectPredecessorMismatch {
                request,
                expected: expected_close_effect,
                actual: proof.close_effect,
            });
        }
        if proof.cancel_effect != expected_cancel_effect {
            return Err(CloseInertReason::CancellationEffectMismatch {
                request,
                expected: expected_cancel_effect,
                actual: proof.cancel_effect,
            });
        }
        let (expected_frontier, issued_after, received_after) = match native.cancellation {
            Some(cancellation) => (
                Some(cancellation.effect),
                cancellation.issued_after,
                cancellation.received_after,
            ),
            None => match native.close {
                Some(close) => (Some(close.effect), close.issued_after, close.received_after),
                None => (None, native.edge_observed_at, native.edge_received_at),
            },
        };
        if proof.known_effect_frontier != expected_frontier {
            return Err(CloseInertReason::CancellationKnownFrontierMismatch {
                request,
                expected: expected_frontier,
                actual: proof.known_effect_frontier,
            });
        }
        if proof.observation_generation <= issued_after {
            return Err(CloseInertReason::CancellationObservationNotNewer {
                request,
                issued_after,
                actual: proof.observation_generation,
            });
        }
        if proof.inventory_generation <= received_after {
            return Err(CloseInertReason::CancellationInventoryNotNewer {
                request,
                issued_after: received_after,
                actual: proof.inventory_generation,
            });
        }
        Ok(())
    }

    /// Records unknown external settlement while retaining the exact obligation.
    pub(crate) fn mark_indeterminate(
        &mut self,
        request: CloseRequestId,
        authority: CloseAuthority,
    ) -> CloseAdvanceOutcome {
        if let Err(reason) = self.guard_request(request, authority) {
            return CloseAdvanceOutcome::Inert(reason);
        }
        let Some(record) = self.requests.get_mut(&request) else {
            return CloseAdvanceOutcome::Inert(CloseInertReason::UnknownRequest { request });
        };
        let from = record.plan.phase;
        let has_external_effect = record
            .native
            .is_some_and(|native| native.close.is_some() || native.cancellation.is_some());
        if !has_external_effect
            || !matches!(
                from,
                ClosePlanPhase::EffectEmitted
                    | ClosePlanPhase::AwaitingDestroyed
                    | ClosePlanPhase::CancelRequested
            )
        {
            return CloseAdvanceOutcome::Inert(CloseInertReason::PhaseMismatch {
                request,
                action: CloseLifecycleAction::MarkIndeterminate,
                actual: from,
            });
        }
        if record.plan.cancellation == CloseCancellationState::Requested {
            record.plan.cancellation = CloseCancellationState::Indeterminate;
        }
        record.plan.phase = ClosePlanPhase::Indeterminate;
        CloseAdvanceOutcome::Advanced {
            request,
            from,
            to: ClosePlanPhase::Indeterminate,
        }
    }

    /// Retains an unresolved surface plan after typed provider facts prove that
    /// its exact close edge is no longer current.
    ///
    /// This differs from [`Self::mark_indeterminate`]: it is not a dispatch
    /// report and therefore may also apply before any platform effect was
    /// emitted. The plan remains auditable, but cannot block a later edge on
    /// the same binding or authorize its frozen topology payload.
    pub(crate) fn mark_native_edge_indeterminate(
        &mut self,
        request: CloseRequestId,
        authority: CloseAuthority,
    ) -> CloseAdvanceOutcome {
        if let Err(reason) = self.guard_request(request, authority) {
            return CloseAdvanceOutcome::Inert(reason);
        }
        let Some(record) = self.requests.get_mut(&request) else {
            return CloseAdvanceOutcome::Inert(CloseInertReason::UnknownRequest { request });
        };
        if record.plan.target.kind() != ClosePlanTargetKind::Surface {
            return CloseAdvanceOutcome::Inert(CloseInertReason::TargetMismatch {
                request,
                action: CloseLifecycleAction::MarkIndeterminate,
                target: record.plan.target.kind(),
            });
        }
        if record.native.is_none() {
            return CloseAdvanceOutcome::Inert(CloseInertReason::NativeEdgeNotObserved { request });
        }
        if record.plan.is_terminal() {
            return CloseAdvanceOutcome::Inert(CloseInertReason::Terminal {
                request,
                phase: record.plan.phase,
            });
        }
        if record.plan.phase == ClosePlanPhase::Indeterminate {
            return CloseAdvanceOutcome::Inert(CloseInertReason::PhaseMismatch {
                request,
                action: CloseLifecycleAction::MarkIndeterminate,
                actual: record.plan.phase,
            });
        }
        let from = record.plan.phase;
        record.plan.cancellation = CloseCancellationState::Indeterminate;
        record.plan.phase = ClosePlanPhase::Indeterminate;
        CloseAdvanceOutcome::Advanced {
            request,
            from,
            to: ClosePlanPhase::Indeterminate,
        }
    }

    /// Terminates a surface plan when the exact binding was externally destroyed
    /// without a valid causal proof for the plan's emitted effect lane.
    ///
    /// This deliberately bypasses workspace/policy authority freshness: the
    /// native resource is already gone, so retaining an active plan would leak
    /// an unreachable obligation. It never authorizes the frozen topology
    /// payload; the engine must take its explicit unplanned recovery path.
    pub(crate) fn mark_externally_destroyed_unproved(
        &mut self,
        request: CloseRequestId,
        binding: ViewportBinding,
    ) -> CloseAdvanceOutcome {
        if request.domain() != self.domain {
            return CloseAdvanceOutcome::Inert(CloseInertReason::RequestDomainMismatch {
                request,
                expected: self.domain,
                actual: request.domain(),
            });
        }
        let Some(record) = self.requests.get_mut(&request) else {
            return CloseAdvanceOutcome::Inert(CloseInertReason::UnknownRequest { request });
        };
        let Some(native) = record.native else {
            return CloseAdvanceOutcome::Inert(CloseInertReason::NativeEdgeNotObserved { request });
        };
        if native.binding != binding {
            return CloseAdvanceOutcome::Inert(CloseInertReason::NativeBindingMismatch {
                request,
                expected: native.binding,
                actual: binding,
            });
        }
        if record.plan.target.kind() != ClosePlanTargetKind::Surface {
            return CloseAdvanceOutcome::Inert(CloseInertReason::TargetMismatch {
                request,
                action: CloseLifecycleAction::MarkExternallyDestroyedUnproved,
                target: record.plan.target.kind(),
            });
        }
        if record.plan.is_terminal() {
            return CloseAdvanceOutcome::Inert(CloseInertReason::Terminal {
                request,
                phase: record.plan.phase,
            });
        }
        let from = record.plan.phase;
        record.plan.destruction = CloseDestructionState::None;
        record.plan.cancellation = CloseCancellationState::None;
        record.plan.phase = ClosePlanPhase::ExternallyDestroyedUnproved;
        CloseAdvanceOutcome::Advanced {
            request,
            from,
            to: ClosePlanPhase::ExternallyDestroyedUnproved,
        }
    }

    /// Invalidates every non-terminal plan whose authority no longer matches.
    pub(crate) fn invalidate_stale(&mut self, authority: CloseAuthority) -> Vec<CloseRequestId> {
        if authority.domain() != self.domain {
            return Vec::new();
        }
        let mut invalidated = Vec::new();
        for (request, record) in &mut self.requests {
            let cancels_on_authority_drift = Self::cancels_on_authority_drift(&record.plan);
            if (record.plan.phase.may_become_stale() || cancels_on_authority_drift)
                && record.plan.authority != authority
            {
                if cancels_on_authority_drift {
                    record.plan.cancellation = CloseCancellationState::Requested;
                    record.plan.phase = ClosePlanPhase::CancelRequested;
                } else {
                    record.plan.phase = ClosePlanPhase::Stale;
                }
                invalidated.push(*request);
            }
        }
        invalidated
    }

    fn validate_requirements(
        target: ClosePlanTarget,
        requirements: &[CloseItemRequirement],
    ) -> Result<(), CloseCoordinatorError> {
        let mut seen = BTreeSet::new();
        for requirement in requirements {
            if !seen.insert(requirement.item) {
                return Err(CloseCoordinatorError::DuplicateItem {
                    item: requirement.item,
                });
            }
            if !requirement.capability.allows_close() {
                return Err(CloseCoordinatorError::DisabledItem {
                    item: requirement.item,
                });
            }
        }

        match target {
            ClosePlanTarget::Item { item } => {
                let actual = requirements
                    .iter()
                    .map(|requirement| requirement.item)
                    .collect::<Vec<_>>();
                if actual.as_slice() != [item] {
                    return Err(CloseCoordinatorError::ItemTargetMismatch {
                        target: item,
                        actual,
                    });
                }
            }
            ClosePlanTarget::Root { root } if requirements.is_empty() => {
                return Err(CloseCoordinatorError::EmptyRootHasNoItemDecisions { root });
            }
            ClosePlanTarget::Surface {
                surface,
                disposition: SurfaceCloseDisposition::RehomeAll { .. },
            } if !requirements.is_empty() => {
                return Err(CloseCoordinatorError::SurfaceRehomeHasItemDecisions {
                    surface,
                    item_count: requirements.len(),
                });
            }
            ClosePlanTarget::Surface {
                surface,
                disposition: SurfaceCloseDisposition::CloseContent,
            } if requirements.is_empty() => {
                return Err(
                    CloseCoordinatorError::SurfaceCloseContentHasNoItemDecisions { surface },
                );
            }
            ClosePlanTarget::Root { .. } | ClosePlanTarget::Surface { .. } => {}
        }
        Ok(())
    }

    fn guard_request(
        &mut self,
        request: CloseRequestId,
        authority: CloseAuthority,
    ) -> Result<(), CloseInertReason> {
        if request.domain() != self.domain {
            return Err(CloseInertReason::RequestDomainMismatch {
                request,
                expected: self.domain,
                actual: request.domain(),
            });
        }
        if authority.domain() != self.domain {
            return Err(CloseInertReason::AuthorityDomainMismatch {
                request,
                expected: self.domain,
                actual: authority.domain(),
            });
        }
        let Some(record) = self.requests.get_mut(&request) else {
            return Err(if self.request_was_retired_terminal(request) {
                CloseInertReason::RetiredTerminal { request }
            } else {
                CloseInertReason::UnknownRequest { request }
            });
        };
        if record.plan.is_terminal() {
            return Err(CloseInertReason::Terminal {
                request,
                phase: record.plan.phase,
            });
        }
        let cancels_on_authority_drift = Self::cancels_on_authority_drift(&record.plan);
        if (record.plan.phase.may_become_stale() || cancels_on_authority_drift)
            && record.plan.authority != authority
        {
            let expected = record.plan.authority;
            if cancels_on_authority_drift {
                record.plan.cancellation = CloseCancellationState::Requested;
                record.plan.phase = ClosePlanPhase::CancelRequested;
            } else {
                record.plan.phase = ClosePlanPhase::Stale;
            }
            return Err(CloseInertReason::AuthorityStale {
                request,
                expected,
                actual: authority,
            });
        }
        Ok(())
    }

    fn cancels_on_authority_drift(plan: &ClosePlan) -> bool {
        plan.target.kind() == ClosePlanTargetKind::Surface
            && (plan.phase.may_become_stale() || plan.phase == ClosePlanPhase::Vetoed)
    }

    fn item_snapshot(
        &self,
        request: CloseRequestId,
        item_index: usize,
    ) -> Option<(ItemId, CloseCapability, CloseItemDecisionState)> {
        let record = self.requests.get(&request)?;
        let item = record.plan.items.get(item_index)?;
        Some((item.item, item.capability, item.state))
    }

    fn mint_deferred(&mut self) -> Result<DeferredCloseToken, CloseCoordinatorError> {
        let raw = self
            .last_deferred
            .checked_add(1)
            .and_then(NonZeroU64::new)
            .ok_or(CloseCoordinatorError::DeferredTokenExhausted)?;
        let token = DeferredCloseToken::from_sequence(self.domain, raw);
        self.last_deferred = token.sequence();
        Ok(token)
    }

    fn advance_exact(
        &mut self,
        request: CloseRequestId,
        action: CloseLifecycleAction,
        expected: &[ClosePlanPhase],
        next: ClosePlanPhase,
    ) -> CloseAdvanceOutcome {
        let Some(record) = self.requests.get(&request) else {
            return CloseAdvanceOutcome::Inert(CloseInertReason::UnknownRequest { request });
        };
        if !expected.contains(&record.plan.phase) {
            return CloseAdvanceOutcome::Inert(CloseInertReason::PhaseMismatch {
                request,
                action,
                actual: record.plan.phase,
            });
        }
        self.advance_unchecked(request, next)
    }

    fn advance_unchecked(
        &mut self,
        request: CloseRequestId,
        next: ClosePlanPhase,
    ) -> CloseAdvanceOutcome {
        let Some(record) = self.requests.get_mut(&request) else {
            return CloseAdvanceOutcome::Inert(CloseInertReason::UnknownRequest { request });
        };
        let from = record.plan.phase;
        record.plan.phase = next;
        CloseAdvanceOutcome::Advanced {
            request,
            from,
            to: next,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ids::{WorkspaceEpoch, WorkspaceRevision};
    use crate::viewport::{WindowIncarnation, WindowToken};

    const ITEM_A: ItemId = ItemId::new(11);
    const ITEM_B: ItemId = ItemId::new(12);
    const ITEM_C: ItemId = ItemId::new(13);
    const ROOT: RootId = RootId::new(21);
    const SURFACE: SurfaceId = SurfaceId::new(31);
    const TARGET_SURFACE: SurfaceId = SurfaceId::new(32);

    fn domain() -> EngineAuthorityDomainId {
        EngineAuthorityDomainId::new_for_test(7)
    }

    fn authority(workspace_revision: u64, policy_revision: u64) -> CloseAuthority {
        authority_in_domain(domain(), workspace_revision, policy_revision)
    }

    fn authority_in_domain(
        authority_domain: EngineAuthorityDomainId,
        workspace_revision: u64,
        policy_revision: u64,
    ) -> CloseAuthority {
        CloseAuthority::new(
            authority_domain,
            WorkspaceVersion::new(
                WorkspaceEpoch::new(7),
                WorkspaceRevision::new(workspace_revision),
            ),
            PolicyRevision::new(policy_revision),
        )
    }

    fn coordinator<P>() -> CloseCoordinator<P> {
        CloseCoordinator::new(domain())
    }

    fn requirement(item: ItemId, capability: CloseCapability) -> CloseItemRequirement {
        CloseItemRequirement::new(item, capability)
    }

    fn native_binding() -> ViewportBinding {
        ViewportBinding::new(
            domain(),
            WorkspaceEpoch::new(7),
            SURFACE,
            WindowToken::new(41),
            WindowIncarnation::new(51),
        )
    }

    fn native_edge() -> NativeCloseEdge {
        NativeCloseEdge::from_authoritative_requested(
            domain(),
            native_binding(),
            CloseObservationGeneration::new(7),
            InventoryGeneration::new(7),
        )
    }

    fn emit_native<P>(
        coordinator: &mut CloseCoordinator<P>,
        request: CloseRequestId,
    ) -> CloseAdvanceOutcome {
        coordinator.mark_effect_emitted(
            request,
            authority(3, 5),
            native_binding(),
            EffectId::new(101),
            CloseObservationGeneration::new(8),
            InventoryGeneration::new(8),
            None,
        )
    }

    fn emit_cancellation<P>(
        coordinator: &mut CloseCoordinator<P>,
        request: CloseRequestId,
    ) -> CloseAdvanceOutcome {
        coordinator.mark_cancellation_effect_emitted(
            request,
            authority(3, 5),
            native_binding(),
            EffectId::new(102),
            CloseObservationGeneration::new(10),
            InventoryGeneration::new(10),
            Some(EffectId::new(101)),
        )
    }

    fn apply_destroyed<P>(
        coordinator: &mut CloseCoordinator<P>,
        request: CloseRequestId,
        close_authority: CloseAuthority,
    ) -> CloseAdvanceOutcome {
        coordinator.settle_native(
            request,
            close_authority,
            CloseNativeSettlement::DestroyedProved(valid_destroyed_proof(request)),
        )
    }

    fn destroyed_proof(
        request: CloseRequestId,
        binding: ViewportBinding,
        generation: u64,
    ) -> CloseDestroyedProof {
        destroyed_proof_acknowledging(request, binding, generation, EffectId::new(101))
    }

    fn destroyed_proof_acknowledging(
        request: CloseRequestId,
        binding: ViewportBinding,
        generation: u64,
        acknowledged_effect: EffectId,
    ) -> CloseDestroyedProof {
        destroyed_proof_at(
            request,
            binding,
            generation,
            generation,
            acknowledged_effect,
        )
    }

    fn destroyed_proof_at(
        request: CloseRequestId,
        binding: ViewportBinding,
        observation_generation: u64,
        inventory_generation: u64,
        acknowledged_effect: EffectId,
    ) -> CloseDestroyedProof {
        CloseDestroyedProof::from_authoritative_destroyed(
            request,
            binding,
            CloseObservationGeneration::new(observation_generation),
            InventoryGeneration::new(inventory_generation),
            acknowledged_effect,
        )
    }

    fn valid_destroyed_proof(request: CloseRequestId) -> CloseDestroyedProof {
        destroyed_proof(request, native_binding(), 11)
    }

    fn cancellation_proof(
        request: CloseRequestId,
        binding: ViewportBinding,
        close_effect: EffectId,
        cancel_effect: EffectId,
        generation: u64,
    ) -> CloseCancellationProof {
        cancellation_proof_at(
            request,
            binding,
            Some(close_effect),
            cancel_effect,
            generation,
            generation,
        )
    }

    fn cancellation_proof_at(
        request: CloseRequestId,
        binding: ViewportBinding,
        close_effect: Option<EffectId>,
        cancel_effect: EffectId,
        observation_generation: u64,
        inventory_generation: u64,
    ) -> CloseCancellationProof {
        CloseCancellationProof::from_authoritative_live_close_cleared(
            request,
            binding,
            close_effect,
            Some(cancel_effect),
            Some(cancel_effect),
            CloseObservationGeneration::new(observation_generation),
            InventoryGeneration::new(inventory_generation),
        )
    }

    fn rehome_request() -> SurfaceCloseRequest {
        SurfaceCloseRequest::RehomeAll {
            target: SurfaceRehomeTarget::new(TARGET_SURFACE, None, Vec::new()),
        }
    }

    fn valid_cancellation_proof(request: CloseRequestId) -> CloseCancellationProof {
        cancellation_proof(
            request,
            native_binding(),
            EffectId::new(101),
            EffectId::new(102),
            11,
        )
    }

    fn approved_retain_surface_plan<P>(
        coordinator: &mut CloseCoordinator<P>,
        prepared: P,
    ) -> ClosePlan {
        let plan = coordinator
            .open_surface(
                authority(3, 5),
                native_edge(),
                SurfaceCloseRequest::RetainLayout,
                [],
                prepared,
            )
            .expect("retain-layout plan needs no content decision roster");
        coordinator
            .plan(plan.request())
            .cloned()
            .expect("approved retain plan remains available")
    }

    fn cancellation_ready_surface_plan(
        coordinator: &mut CloseCoordinator<&'static str>,
    ) -> CloseRequestId {
        let plan = approved_retain_surface_plan(coordinator, "complete-roster");
        let request = plan.request();
        emit_native(coordinator, request);
        coordinator.request_cancel(request, authority(3, 5));
        emit_cancellation(coordinator, request);
        request
    }

    fn vetoed_surface_plan(coordinator: &mut CloseCoordinator<&'static str>) -> CloseRequestId {
        let plan = coordinator
            .open_surface(
                authority(3, 5),
                native_edge(),
                SurfaceCloseRequest::CloseContent,
                [requirement(ITEM_A, CloseCapability::Immediate)],
                "complete-roster",
            )
            .expect("surface content-close plan needs one decision");
        let request = plan.request();
        assert_eq!(
            coordinator.resolve(
                request,
                plan.items()[0].token(),
                authority(3, 5),
                CloseDecision::Veto,
            ),
            Ok(CloseResolutionOutcome::Vetoed {
                request,
                item: ITEM_A,
            })
        );
        request
    }

    fn root_plan(
        coordinator: &mut CloseCoordinator<&'static str>,
        requirements: impl IntoIterator<Item = CloseItemRequirement>,
    ) -> ClosePlan {
        coordinator
            .open(
                authority(3, 5),
                ClosePlanTarget::Root { root: ROOT },
                requirements,
                "frozen-root-fingerprint",
            )
            .expect("valid root close plan")
    }

    #[test]
    fn published_terminal_close_plans_compact_without_losing_replay_classification() {
        let mut coordinator = coordinator();
        let mut last = None;

        for _ in 0..10_000 {
            let plan = root_plan(
                &mut coordinator,
                [requirement(ITEM_A, CloseCapability::Immediate)],
            );
            let request = plan.request();
            assert_eq!(
                coordinator.resolve(
                    request,
                    plan.items()[0].token(),
                    authority(3, 5),
                    CloseDecision::Allow,
                ),
                Ok(CloseResolutionOutcome::Approved { request }),
            );
            assert!(matches!(
                coordinator.mark_local_applied(request, authority(3, 5)),
                CloseAdvanceOutcome::Advanced {
                    to: ClosePlanPhase::Applied,
                    ..
                }
            ));
            last = Some((request, plan.items()[0].token()));
        }

        coordinator.mark_boundary_published();
        let retention = coordinator.retention_manifest();
        assert_eq!(retention.active_plans(), 0);
        assert_eq!(retention.terminal_plan_guards(), 10_000);
        assert_eq!(retention.decision_token_guards(), 10_000);
        assert_eq!(retention.deferred_token_guards(), 0);
        assert_eq!(retention.retained_structure_count(), 20_000);
        assert_eq!(coordinator.compact_published_terminal(), 10_000);
        let compacted = coordinator.retention_manifest();
        assert_eq!(compacted.active_plans(), 0);
        assert_eq!(compacted.terminal_plan_guards(), 0);
        assert_eq!(compacted.decision_token_guards(), 0);
        assert_eq!(compacted.deferred_token_guards(), 0);
        assert_eq!(compacted.retained_structure_count(), 0);

        let (request, token) = last.expect("the test creates terminal plans");
        assert_eq!(
            coordinator.resolve(request, token, authority(3, 5), CloseDecision::Allow,),
            Ok(CloseResolutionOutcome::Inert(
                CloseInertReason::RetiredTerminal { request },
            )),
        );
    }

    #[test]
    fn retention_accounts_for_live_decision_and_deferred_token_indexes() {
        let mut coordinator = coordinator();
        let plan = root_plan(
            &mut coordinator,
            [requirement(ITEM_A, CloseCapability::DeferredAllowed)],
        );
        let request = plan.request();
        assert!(matches!(
            coordinator.resolve(
                request,
                plan.items()[0].token(),
                authority(3, 5),
                CloseDecision::Deferred,
            ),
            Ok(CloseResolutionOutcome::Deferred { .. })
        ));

        let retention = coordinator.retention_manifest();
        assert_eq!(retention.active_plans(), 1);
        assert_eq!(retention.terminal_plan_guards(), 0);
        assert_eq!(retention.decision_token_guards(), 1);
        assert_eq!(retention.deferred_token_guards(), 1);
        assert_eq!(retention.retained_structure_count(), 3);
    }

    #[test]
    fn identities_are_non_zero_monotonic_and_item_order_is_stable() {
        let mut coordinator = coordinator();
        let first = root_plan(
            &mut coordinator,
            [
                requirement(ITEM_C, CloseCapability::Immediate),
                requirement(ITEM_A, CloseCapability::DeferredAllowed),
                requirement(ITEM_B, CloseCapability::Immediate),
            ],
        );
        let second = root_plan(
            &mut coordinator,
            [requirement(ITEM_A, CloseCapability::Immediate)],
        );

        assert_ne!(first.request().sequence(), 0);
        assert!(second.request().sequence() > first.request().sequence());
        assert_eq!(first.request().domain(), domain());
        assert_eq!(
            first
                .items()
                .iter()
                .map(|item| item.item())
                .collect::<Vec<_>>(),
            vec![ITEM_C, ITEM_A, ITEM_B]
        );
        let tokens = first
            .items()
            .iter()
            .map(|item| item.token().sequence())
            .collect::<Vec<_>>();
        assert!(tokens.iter().all(|token| *token != 0));
        assert!(tokens.windows(2).all(|pair| pair[0] < pair[1]));
        assert!(second.items()[0].token().sequence() > tokens[2]);
        assert_eq!(first.items()[0].token().domain(), domain());
    }

    #[test]
    fn active_target_lookup_reuses_only_current_non_terminal_plan() {
        let mut coordinator = coordinator();
        let target = ClosePlanTarget::Root { root: ROOT };
        let first = root_plan(
            &mut coordinator,
            [requirement(ITEM_A, CloseCapability::Immediate)],
        );

        assert_eq!(coordinator.clone(), coordinator);
        assert_eq!(
            coordinator
                .active_plan_for_target(target, authority(3, 5))
                .map(ClosePlan::request),
            Some(first.request())
        );
        assert!(
            coordinator
                .active_plan_for_target(target, authority(3, 6))
                .is_none()
        );

        coordinator
            .resolve(
                first.request(),
                first.items()[0].token(),
                authority(3, 5),
                CloseDecision::Veto,
            )
            .expect("veto does not allocate a continuation");
        assert!(
            coordinator
                .active_plan_for_target(target, authority(3, 5))
                .is_none()
        );

        let second = root_plan(
            &mut coordinator,
            [requirement(ITEM_B, CloseCapability::Immediate)],
        );
        assert_eq!(
            coordinator
                .active_plan_for_target(target, authority(3, 5))
                .map(ClosePlan::request),
            Some(second.request())
        );
        assert_eq!(
            coordinator.invalidate_stale(authority(4, 5)),
            vec![second.request()]
        );
        assert!(
            coordinator
                .active_plan_for_target(target, authority(3, 5))
                .is_none()
        );
        assert!(
            coordinator
                .active_plan_for_target(target, authority(4, 5))
                .is_none()
        );
    }

    #[test]
    fn decisions_may_arrive_out_of_order_but_only_the_last_allow_approves() {
        let mut coordinator = coordinator();
        let plan = root_plan(
            &mut coordinator,
            [
                requirement(ITEM_A, CloseCapability::Immediate),
                requirement(ITEM_B, CloseCapability::Immediate),
                requirement(ITEM_C, CloseCapability::Immediate),
            ],
        );
        let request = plan.request();
        let tokens = plan
            .items()
            .iter()
            .map(|item| item.token())
            .collect::<Vec<_>>();

        assert_eq!(
            coordinator.resolve(request, tokens[2], authority(3, 5), CloseDecision::Allow),
            Ok(CloseResolutionOutcome::Recorded {
                request,
                item: ITEM_C,
                phase: ClosePlanPhase::Resolving,
            })
        );
        assert_eq!(
            coordinator.resolve(request, tokens[0], authority(3, 5), CloseDecision::Allow),
            Ok(CloseResolutionOutcome::Recorded {
                request,
                item: ITEM_A,
                phase: ClosePlanPhase::Resolving,
            })
        );
        assert!(matches!(
            coordinator.approved(request, authority(3, 5)),
            Err(CloseInertReason::PhaseMismatch { .. })
        ));
        assert_eq!(
            coordinator.resolve(request, tokens[1], authority(3, 5), CloseDecision::Allow),
            Ok(CloseResolutionOutcome::Approved { request })
        );
        let approved = coordinator
            .approved(request, authority(3, 5))
            .expect("last vote exposes prepared close exactly once for commit");
        assert_eq!(approved.plan().phase(), ClosePlanPhase::Approved);
        assert_eq!(*approved.prepared(), "frozen-root-fingerprint");
    }

    #[test]
    fn duplicate_initial_decision_is_inert_and_cannot_replay_approval() {
        let mut coordinator = coordinator();
        let plan = root_plan(
            &mut coordinator,
            [requirement(ITEM_A, CloseCapability::Immediate)],
        );
        let request = plan.request();
        let token = plan.items()[0].token();

        assert_eq!(
            coordinator.resolve(request, token, authority(3, 5), CloseDecision::Allow),
            Ok(CloseResolutionOutcome::Approved { request })
        );
        assert_eq!(
            coordinator.resolve(request, token, authority(3, 5), CloseDecision::Allow),
            Ok(CloseResolutionOutcome::Inert(
                CloseInertReason::DuplicateDecision {
                    request,
                    item: ITEM_A,
                    state: CloseItemDecisionState::Allowed,
                }
            ))
        );
        assert_eq!(
            coordinator.plan(request).map(ClosePlan::phase),
            Some(ClosePlanPhase::Approved)
        );
    }

    #[test]
    fn deferred_decision_mints_one_continuation_and_consumes_it_once() {
        let mut coordinator = coordinator();
        let plan = root_plan(
            &mut coordinator,
            [requirement(ITEM_A, CloseCapability::DeferredAllowed)],
        );
        let request = plan.request();
        let initial = plan.items()[0].token();
        let first = coordinator
            .resolve(request, initial, authority(3, 5), CloseDecision::Deferred)
            .expect("continuation allocation succeeds");
        let CloseResolutionOutcome::Deferred { continuation, .. } = first else {
            panic!("expected deferred continuation, got {first:?}");
        };

        assert_ne!(continuation.sequence(), 0);
        assert_eq!(
            coordinator
                .plan(request)
                .map(|current| current.items()[0].state()),
            Some(CloseItemDecisionState::Deferred { continuation })
        );
        assert_eq!(
            coordinator.resolve(request, initial, authority(3, 5), CloseDecision::Deferred),
            Ok(CloseResolutionOutcome::Inert(
                CloseInertReason::DuplicateDecision {
                    request,
                    item: ITEM_A,
                    state: CloseItemDecisionState::Deferred { continuation },
                }
            ))
        );
        assert_eq!(
            coordinator.continue_deferred(
                request,
                continuation,
                authority(3, 5),
                DeferredCloseDecision::Allow,
            ),
            CloseResolutionOutcome::Approved { request }
        );
        assert_eq!(
            coordinator
                .plan(request)
                .map(|current| current.items()[0].state()),
            Some(CloseItemDecisionState::Allowed)
        );
        assert_eq!(
            coordinator.continue_deferred(
                request,
                continuation,
                authority(3, 5),
                DeferredCloseDecision::Allow,
            ),
            CloseResolutionOutcome::Inert(CloseInertReason::DuplicateDeferredContinuation {
                request,
                item: ITEM_A,
                state: CloseItemDecisionState::Allowed,
            })
        );
    }

    #[test]
    fn immediate_capability_rejects_deferred_without_consuming_initial_token() {
        let mut coordinator = coordinator();
        let plan = root_plan(
            &mut coordinator,
            [requirement(ITEM_A, CloseCapability::Immediate)],
        );
        let request = plan.request();
        let token = plan.items()[0].token();

        assert_eq!(
            coordinator.resolve(request, token, authority(3, 5), CloseDecision::Deferred),
            Ok(CloseResolutionOutcome::Inert(
                CloseInertReason::DeferredNotAllowed {
                    request,
                    item: ITEM_A,
                    capability: CloseCapability::Immediate,
                }
            ))
        );
        assert_eq!(
            coordinator
                .plan(request)
                .map(|current| current.items()[0].state()),
            Some(CloseItemDecisionState::Pending)
        );
        assert_eq!(
            coordinator.resolve(request, token, authority(3, 5), CloseDecision::Allow),
            Ok(CloseResolutionOutcome::Approved { request })
        );
    }

    #[test]
    fn veto_is_terminal_and_late_votes_are_inert() {
        let mut coordinator = coordinator();
        let plan = root_plan(
            &mut coordinator,
            [
                requirement(ITEM_A, CloseCapability::Immediate),
                requirement(ITEM_B, CloseCapability::Immediate),
            ],
        );
        let request = plan.request();

        assert_eq!(
            coordinator.resolve(
                request,
                plan.items()[1].token(),
                authority(3, 5),
                CloseDecision::Veto,
            ),
            Ok(CloseResolutionOutcome::Vetoed {
                request,
                item: ITEM_B,
            })
        );
        assert_eq!(
            coordinator.resolve(
                request,
                plan.items()[0].token(),
                authority(3, 5),
                CloseDecision::Allow,
            ),
            Ok(CloseResolutionOutcome::Inert(CloseInertReason::Terminal {
                request,
                phase: ClosePlanPhase::Vetoed,
            }))
        );
        assert!(matches!(
            coordinator.approved(request, authority(3, 5)),
            Err(CloseInertReason::Terminal {
                phase: ClosePlanPhase::Vetoed,
                ..
            })
        ));
    }

    #[test]
    fn authority_change_marks_plan_stale_and_never_reinterprets_old_vote() {
        let mut coordinator = coordinator();
        let plan = root_plan(
            &mut coordinator,
            [requirement(ITEM_A, CloseCapability::Immediate)],
        );
        let request = plan.request();
        let token = plan.items()[0].token();
        let actual = authority(4, 5);

        assert_eq!(
            coordinator.resolve(request, token, actual, CloseDecision::Allow),
            Ok(CloseResolutionOutcome::Inert(
                CloseInertReason::AuthorityStale {
                    request,
                    expected: authority(3, 5),
                    actual,
                }
            ))
        );
        assert_eq!(
            coordinator.plan(request).map(ClosePlan::phase),
            Some(ClosePlanPhase::Stale)
        );
        assert_eq!(
            coordinator.resolve(request, token, authority(3, 5), CloseDecision::Allow),
            Ok(CloseResolutionOutcome::Inert(CloseInertReason::Terminal {
                request,
                phase: ClosePlanPhase::Stale,
            }))
        );
    }

    #[test]
    fn surface_authority_drift_becomes_a_cancellable_native_obligation() {
        let mut coordinator = coordinator();
        let plan = approved_retain_surface_plan(&mut coordinator, "complete-roster");
        let request = plan.request();
        let newer_authority = authority(4, 6);

        assert_eq!(
            coordinator.mark_effect_emitted(
                request,
                newer_authority,
                native_binding(),
                EffectId::new(101),
                CloseObservationGeneration::new(8),
                InventoryGeneration::new(8),
                None,
            ),
            CloseAdvanceOutcome::Inert(CloseInertReason::AuthorityStale {
                request,
                expected: authority(3, 5),
                actual: newer_authority,
            })
        );
        assert_eq!(
            coordinator.plan(request).map(|current| (
                current.phase(),
                current.destruction_state(),
                current.cancellation_state(),
                current.is_terminal(),
            )),
            Some((
                ClosePlanPhase::CancelRequested,
                CloseDestructionState::None,
                CloseCancellationState::Requested,
                false,
            ))
        );

        assert_eq!(
            coordinator.mark_cancellation_effect_emitted(
                request,
                newer_authority,
                native_binding(),
                EffectId::new(102),
                CloseObservationGeneration::new(8),
                InventoryGeneration::new(8),
                None,
            ),
            CloseAdvanceOutcome::Advanced {
                request,
                from: ClosePlanPhase::CancelRequested,
                to: ClosePlanPhase::CancelRequested,
            }
        );
        let proof = CloseCancellationProof::from_authoritative_live_close_cleared(
            request,
            native_binding(),
            None,
            Some(EffectId::new(102)),
            Some(EffectId::new(102)),
            CloseObservationGeneration::new(9),
            InventoryGeneration::new(9),
        );
        assert_eq!(
            coordinator.settle_native(
                request,
                newer_authority,
                CloseNativeSettlement::CancellationProved(proof),
            ),
            CloseAdvanceOutcome::Advanced {
                request,
                from: ClosePlanPhase::CancelRequested,
                to: ClosePlanPhase::Cancelled,
            }
        );
    }

    #[test]
    fn invalidation_authority_drift_cancels_a_vetoed_surface_plan() {
        let mut coordinator = coordinator();
        let request = vetoed_surface_plan(&mut coordinator);
        let newer_authority = authority(4, 6);

        assert_eq!(coordinator.invalidate_stale(newer_authority), vec![request]);
        assert_eq!(
            coordinator.plan(request).map(|current| (
                current.phase(),
                current.destruction_state(),
                current.cancellation_state(),
                current.is_terminal(),
            )),
            Some((
                ClosePlanPhase::CancelRequested,
                CloseDestructionState::None,
                CloseCancellationState::Requested,
                false,
            ))
        );
        assert!(coordinator.invalidate_stale(newer_authority).is_empty());
    }

    #[test]
    fn guarded_authority_drift_cancels_a_vetoed_surface_plan() {
        let mut coordinator = coordinator();
        let request = vetoed_surface_plan(&mut coordinator);
        let newer_authority = authority(4, 6);

        assert_eq!(
            coordinator.request_cancel(request, newer_authority),
            CloseAdvanceOutcome::Inert(CloseInertReason::AuthorityStale {
                request,
                expected: authority(3, 5),
                actual: newer_authority,
            })
        );
        assert_eq!(
            coordinator.plan(request).map(|current| (
                current.phase(),
                current.destruction_state(),
                current.cancellation_state(),
                current.is_terminal(),
            )),
            Some((
                ClosePlanPhase::CancelRequested,
                CloseDestructionState::None,
                CloseCancellationState::Requested,
                false,
            ))
        );
    }

    #[test]
    fn authority_drift_does_not_reclassify_effect_emitted_or_indeterminate_surface_plans() {
        let mut coordinator = coordinator();
        let plan = approved_retain_surface_plan(&mut coordinator, "complete-roster");
        let request = plan.request();
        let newer_authority = authority(4, 6);
        emit_native(&mut coordinator, request);

        assert!(coordinator.invalidate_stale(newer_authority).is_empty());
        assert_eq!(
            coordinator.plan(request).map(ClosePlan::phase),
            Some(ClosePlanPhase::EffectEmitted)
        );
        assert_eq!(
            coordinator.mark_indeterminate(request, authority(3, 5)),
            CloseAdvanceOutcome::Advanced {
                request,
                from: ClosePlanPhase::EffectEmitted,
                to: ClosePlanPhase::Indeterminate,
            }
        );
        assert!(coordinator.invalidate_stale(newer_authority).is_empty());
        assert_eq!(
            coordinator.plan(request).map(|current| (
                current.phase(),
                current.destruction_state(),
                current.cancellation_state(),
            )),
            Some((
                ClosePlanPhase::Indeterminate,
                CloseDestructionState::EffectEmitted,
                CloseCancellationState::None,
            ))
        );
    }

    #[test]
    fn foreign_domains_are_inert_before_effect_without_staling_local_plan() {
        let mut coordinator = coordinator();
        let plan = root_plan(
            &mut coordinator,
            [requirement(ITEM_A, CloseCapability::Immediate)],
        );
        let foreign_domain = EngineAuthorityDomainId::new_for_test(99);
        let foreign_authority = authority_in_domain(foreign_domain, 3, 5);

        assert_eq!(
            coordinator.resolve(
                plan.request(),
                plan.items()[0].token(),
                foreign_authority,
                CloseDecision::Allow,
            ),
            Ok(CloseResolutionOutcome::Inert(
                CloseInertReason::AuthorityDomainMismatch {
                    request: plan.request(),
                    expected: domain(),
                    actual: foreign_domain,
                }
            ))
        );
        assert!(coordinator.invalidate_stale(foreign_authority).is_empty());
        assert_eq!(
            coordinator
                .plan(plan.request())
                .map(|current| (current.phase(), current.items()[0].state(),)),
            Some((ClosePlanPhase::Requested, CloseItemDecisionState::Pending))
        );

        let mut foreign_coordinator = CloseCoordinator::new(foreign_domain);
        let foreign_plan = foreign_coordinator
            .open(
                foreign_authority,
                ClosePlanTarget::Root { root: ROOT },
                [requirement(ITEM_A, CloseCapability::Immediate)],
                "foreign-root-fingerprint",
            )
            .expect("foreign fixture uses internally consistent authority");
        assert_eq!(
            coordinator.resolve(
                foreign_plan.request(),
                plan.items()[0].token(),
                authority(3, 5),
                CloseDecision::Allow,
            ),
            Ok(CloseResolutionOutcome::Inert(
                CloseInertReason::RequestDomainMismatch {
                    request: foreign_plan.request(),
                    expected: domain(),
                    actual: foreign_domain,
                }
            ))
        );
        assert_eq!(
            coordinator.plan(plan.request()).map(ClosePlan::phase),
            Some(ClosePlanPhase::Requested)
        );
    }

    #[test]
    fn rejected_final_commit_explicitly_stales_prepared_plan_once() {
        let mut coordinator = coordinator();
        let plan = root_plan(
            &mut coordinator,
            [requirement(ITEM_A, CloseCapability::Immediate)],
        );
        let request = plan.request();
        coordinator
            .resolve(
                request,
                plan.items()[0].token(),
                authority(3, 5),
                CloseDecision::Allow,
            )
            .expect("allow is infallible");

        assert_eq!(
            coordinator.mark_stale(request, authority(3, 5)),
            CloseAdvanceOutcome::Advanced {
                request,
                from: ClosePlanPhase::Approved,
                to: ClosePlanPhase::Stale,
            }
        );
        assert_eq!(
            coordinator.mark_stale(request, authority(3, 5)),
            CloseAdvanceOutcome::Inert(CloseInertReason::Terminal {
                request,
                phase: ClosePlanPhase::Stale,
            })
        );
    }

    #[test]
    fn rejected_final_commit_cancels_irreversible_surface_obligation() {
        let mut coordinator = coordinator();
        let plan = approved_retain_surface_plan(&mut coordinator, "complete-roster");
        let request = plan.request();
        emit_native(&mut coordinator, request);
        assert_eq!(
            coordinator
                .plan(request)
                .map(|current| (current.destruction_state(), current.cancellation_state())),
            Some((
                CloseDestructionState::EffectEmitted,
                CloseCancellationState::None,
            ))
        );

        assert_eq!(
            coordinator.mark_stale(request, authority(3, 5)),
            CloseAdvanceOutcome::Advanced {
                request,
                from: ClosePlanPhase::EffectEmitted,
                to: ClosePlanPhase::CancelRequested,
            }
        );
        assert_eq!(
            coordinator.plan(request).map(|current| (
                current.phase(),
                current.destruction_state(),
                current.cancellation_state(),
            )),
            Some((
                ClosePlanPhase::CancelRequested,
                CloseDestructionState::EffectEmitted,
                CloseCancellationState::Requested,
            ))
        );
    }

    #[test]
    fn a_token_owned_by_another_request_is_typed_and_inert() {
        let mut coordinator = coordinator();
        let first = root_plan(
            &mut coordinator,
            [requirement(ITEM_A, CloseCapability::Immediate)],
        );
        let second = root_plan(
            &mut coordinator,
            [requirement(ITEM_B, CloseCapability::Immediate)],
        );

        assert_eq!(
            coordinator.resolve(
                first.request(),
                second.items()[0].token(),
                authority(3, 5),
                CloseDecision::Allow,
            ),
            Ok(CloseResolutionOutcome::Inert(
                CloseInertReason::WrongDecisionToken {
                    request: first.request(),
                    token: second.items()[0].token(),
                    owner: second.request(),
                }
            ))
        );
        assert_eq!(
            coordinator
                .plan(first.request())
                .map(|current| current.items()[0].state()),
            Some(CloseItemDecisionState::Pending)
        );
    }

    #[test]
    fn local_commit_requires_approval_and_applies_once() {
        let mut coordinator = coordinator();
        let plan = root_plan(
            &mut coordinator,
            [requirement(ITEM_A, CloseCapability::Immediate)],
        );
        let request = plan.request();

        assert!(matches!(
            coordinator.mark_local_applied(request, authority(3, 5)),
            CloseAdvanceOutcome::Inert(CloseInertReason::PhaseMismatch { .. })
        ));
        coordinator
            .resolve(
                request,
                plan.items()[0].token(),
                authority(3, 5),
                CloseDecision::Allow,
            )
            .expect("allow is infallible");
        assert_eq!(
            coordinator.mark_local_applied(request, authority(3, 5)),
            CloseAdvanceOutcome::Advanced {
                request,
                from: ClosePlanPhase::Approved,
                to: ClosePlanPhase::Applied,
            }
        );
        assert_eq!(
            coordinator.mark_local_applied(request, authority(3, 5)),
            CloseAdvanceOutcome::Inert(CloseInertReason::Terminal {
                request,
                phase: ClosePlanPhase::Applied,
            })
        );
    }

    #[test]
    fn native_surface_requires_effect_then_destroyed_authority() {
        let mut coordinator = coordinator();
        let plan = coordinator
            .open_surface(
                authority(3, 5),
                native_edge(),
                rehome_request(),
                [],
                "complete-roster-and-target-proof",
            )
            .expect("rehome does not close content items");
        let request = plan.request();
        assert_eq!(plan.phase(), ClosePlanPhase::Approved);
        assert_eq!(coordinator.native_edge(request), Some(native_edge()));
        assert_eq!(
            coordinator.surface_request_for_edge(native_edge()),
            Some(request)
        );

        assert_eq!(
            apply_destroyed(&mut coordinator, request, authority(3, 5)),
            CloseAdvanceOutcome::Inert(CloseInertReason::CloseEffectNotEmitted { request })
        );
        assert_eq!(
            emit_native(&mut coordinator, request),
            CloseAdvanceOutcome::Advanced {
                request,
                from: ClosePlanPhase::Approved,
                to: ClosePlanPhase::EffectEmitted,
            }
        );
        assert_eq!(
            apply_destroyed(&mut coordinator, request, authority(3, 5)),
            CloseAdvanceOutcome::Advanced {
                request,
                from: ClosePlanPhase::EffectEmitted,
                to: ClosePlanPhase::Applied,
            }
        );
    }

    #[test]
    fn native_close_effect_requires_the_edge_causal_fence_and_an_empty_predecessor() {
        let mut coordinator = coordinator();
        let plan = approved_retain_surface_plan(&mut coordinator, "complete-roster");
        let request = plan.request();
        let cases = [
            (
                CloseObservationGeneration::new(6),
                InventoryGeneration::new(8),
                None,
                CloseInertReason::NativeEmissionObservationPrecedesEdge {
                    request,
                    edge_observed_at: CloseObservationGeneration::new(7),
                    issued_after: CloseObservationGeneration::new(6),
                },
            ),
            (
                CloseObservationGeneration::new(8),
                InventoryGeneration::new(6),
                None,
                CloseInertReason::NativeEmissionInventoryPrecedesEdge {
                    request,
                    edge_received_at: InventoryGeneration::new(7),
                    received_after: InventoryGeneration::new(6),
                },
            ),
            (
                CloseObservationGeneration::new(8),
                InventoryGeneration::new(8),
                Some(EffectId::new(100)),
                CloseInertReason::NativeCloseEffectPredecessorMismatch {
                    request,
                    expected: None,
                    actual: Some(EffectId::new(100)),
                },
            ),
        ];

        for (issued_after, received_after, predecessor, reason) in cases {
            assert_eq!(
                coordinator.mark_effect_emitted(
                    request,
                    authority(3, 5),
                    native_binding(),
                    EffectId::new(101),
                    issued_after,
                    received_after,
                    predecessor,
                ),
                CloseAdvanceOutcome::Inert(reason)
            );
            assert_eq!(
                coordinator
                    .plan(request)
                    .map(|current| (current.phase(), current.destruction_state(),)),
                Some((ClosePlanPhase::Approved, CloseDestructionState::None))
            );
        }

        assert_eq!(
            coordinator.mark_effect_emitted(
                request,
                authority(3, 5),
                native_binding(),
                EffectId::new(101),
                CloseObservationGeneration::new(7),
                InventoryGeneration::new(7),
                None,
            ),
            CloseAdvanceOutcome::Advanced {
                request,
                from: ClosePlanPhase::Approved,
                to: ClosePlanPhase::EffectEmitted,
            }
        );
    }

    #[test]
    fn emitted_native_effect_remains_a_settleable_recovery_obligation() {
        let mut coordinator = coordinator();
        let plan = approved_retain_surface_plan(&mut coordinator, "complete-roster");
        let request = plan.request();
        emit_native(&mut coordinator, request);

        let newer_authority = authority(4, 6);
        assert!(coordinator.invalidate_stale(newer_authority).is_empty());
        assert_eq!(
            apply_destroyed(&mut coordinator, request, newer_authority),
            CloseAdvanceOutcome::Advanced {
                request,
                from: ClosePlanPhase::EffectEmitted,
                to: ClosePlanPhase::Applied,
            }
        );
    }

    #[test]
    fn foreign_authority_cannot_advance_post_effect_destruction_lifecycle() {
        let mut coordinator = coordinator();
        let plan = approved_retain_surface_plan(&mut coordinator, "complete-roster");
        let request = plan.request();
        let foreign_domain = EngineAuthorityDomainId::new_for_test(99);
        let foreign_authority = authority_in_domain(foreign_domain, 3, 5);

        emit_native(&mut coordinator, request);
        assert_eq!(
            apply_destroyed(&mut coordinator, request, foreign_authority),
            CloseAdvanceOutcome::Inert(CloseInertReason::AuthorityDomainMismatch {
                request,
                expected: domain(),
                actual: foreign_domain,
            })
        );
        assert_eq!(
            coordinator
                .plan(request)
                .map(|current| (current.phase(), current.destruction_state(),)),
            Some((
                ClosePlanPhase::EffectEmitted,
                CloseDestructionState::EffectEmitted,
            ))
        );

        assert_eq!(
            apply_destroyed(&mut coordinator, request, authority(3, 5)),
            CloseAdvanceOutcome::Advanced {
                request,
                from: ClosePlanPhase::EffectEmitted,
                to: ClosePlanPhase::Applied,
            }
        );
    }

    #[test]
    fn indeterminate_native_effect_waits_without_timeout_and_can_still_settle() {
        let mut coordinator = coordinator();
        let plan = approved_retain_surface_plan(&mut coordinator, ());
        let request = plan.request();
        emit_native(&mut coordinator, request);

        assert_eq!(
            coordinator.mark_indeterminate(request, authority(3, 5)),
            CloseAdvanceOutcome::Advanced {
                request,
                from: ClosePlanPhase::EffectEmitted,
                to: ClosePlanPhase::Indeterminate,
            }
        );
        assert_eq!(
            coordinator.plan(request).map(ClosePlan::phase),
            Some(ClosePlanPhase::Indeterminate)
        );
        assert_eq!(
            apply_destroyed(&mut coordinator, request, authority(3, 5)),
            CloseAdvanceOutcome::Advanced {
                request,
                from: ClosePlanPhase::Indeterminate,
                to: ClosePlanPhase::Applied,
            }
        );
    }

    #[test]
    fn pre_effect_cancellation_is_immediately_terminal() {
        let mut coordinator = coordinator();
        let plan = root_plan(
            &mut coordinator,
            [requirement(ITEM_A, CloseCapability::DeferredAllowed)],
        );
        let request = plan.request();

        assert_eq!(
            coordinator.request_cancel(request, authority(3, 5)),
            CloseAdvanceOutcome::Advanced {
                request,
                from: ClosePlanPhase::Requested,
                to: ClosePlanPhase::Cancelled,
            }
        );
        assert_eq!(
            coordinator.request_cancel(request, authority(3, 5)),
            CloseAdvanceOutcome::Inert(CloseInertReason::Terminal {
                request,
                phase: ClosePlanPhase::Cancelled,
            })
        );
        let recovered = coordinator
            .plan(request)
            .expect("terminal cancellation remains recoverable");
        assert_eq!(recovered.phase(), ClosePlanPhase::Cancelled);
        assert_eq!(recovered.destruction_state(), CloseDestructionState::None);
        assert_eq!(
            recovered.cancellation_state(),
            CloseCancellationState::Settled
        );
        assert!(recovered.phase().is_terminal());
        assert!(
            coordinator
                .active_plans()
                .all(|plan| plan.request() != request)
        );
    }

    #[test]
    fn cancellation_effect_emission_cannot_clear_destroyed_obligation() {
        let mut coordinator = coordinator();
        let plan = approved_retain_surface_plan(&mut coordinator, "complete-roster");
        let request = plan.request();
        emit_native(&mut coordinator, request);

        assert_eq!(
            coordinator.request_cancel(request, authority(3, 5)),
            CloseAdvanceOutcome::Advanced {
                request,
                from: ClosePlanPhase::EffectEmitted,
                to: ClosePlanPhase::CancelRequested,
            }
        );
        assert_eq!(
            coordinator
                .plan(request)
                .map(|current| (current.destruction_state(), current.cancellation_state())),
            Some((
                CloseDestructionState::EffectEmitted,
                CloseCancellationState::Requested,
            ))
        );
        emit_cancellation(&mut coordinator, request);
        assert_eq!(
            coordinator
                .plan(request)
                .map(|current| (current.destruction_state(), current.cancellation_state())),
            Some((
                CloseDestructionState::EffectEmitted,
                CloseCancellationState::Requested,
            ))
        );
        assert_eq!(
            apply_destroyed(&mut coordinator, request, authority(3, 5)),
            CloseAdvanceOutcome::Advanced {
                request,
                from: ClosePlanPhase::CancelRequested,
                to: ClosePlanPhase::Applied,
            }
        );
        assert_eq!(
            coordinator
                .plan(request)
                .map(|current| (current.destruction_state(), current.cancellation_state())),
            Some((CloseDestructionState::None, CloseCancellationState::None,))
        );
    }

    #[test]
    fn destroyed_acknowledging_cancel_successor_wins_the_race() {
        let mut coordinator = coordinator();
        let plan = approved_retain_surface_plan(&mut coordinator, "complete-roster");
        let request = plan.request();
        emit_native(&mut coordinator, request);
        coordinator.request_cancel(request, authority(3, 5));
        emit_cancellation(&mut coordinator, request);

        assert_eq!(
            coordinator.settle_native(
                request,
                authority(3, 5),
                CloseNativeSettlement::DestroyedProved(destroyed_proof_acknowledging(
                    request,
                    native_binding(),
                    11,
                    EffectId::new(102),
                )),
            ),
            CloseAdvanceOutcome::Advanced {
                request,
                from: ClosePlanPhase::CancelRequested,
                to: ClosePlanPhase::Applied,
            }
        );
    }

    #[test]
    fn indeterminate_cancellation_keeps_destroyed_application_live() {
        let mut coordinator = coordinator();
        let plan = approved_retain_surface_plan(&mut coordinator, "complete-roster");
        let request = plan.request();
        emit_native(&mut coordinator, request);
        coordinator.request_cancel(request, authority(3, 5));
        emit_cancellation(&mut coordinator, request);

        assert_eq!(
            coordinator.mark_indeterminate(request, authority(3, 5)),
            CloseAdvanceOutcome::Advanced {
                request,
                from: ClosePlanPhase::CancelRequested,
                to: ClosePlanPhase::Indeterminate,
            }
        );
        assert_eq!(
            coordinator
                .plan(request)
                .map(|current| current.cancellation_state()),
            Some(CloseCancellationState::Indeterminate)
        );
        assert_eq!(
            apply_destroyed(&mut coordinator, request, authority(3, 5)),
            CloseAdvanceOutcome::Advanced {
                request,
                from: ClosePlanPhase::Indeterminate,
                to: ClosePlanPhase::Applied,
            }
        );
    }

    #[test]
    fn causal_cancellation_proof_is_the_only_post_effect_cancel_terminal() {
        let mut coordinator = coordinator();
        let request = cancellation_ready_surface_plan(&mut coordinator);

        assert_eq!(
            coordinator.settle_native(
                request,
                authority(3, 5),
                CloseNativeSettlement::CancellationProved(valid_cancellation_proof(request)),
            ),
            CloseAdvanceOutcome::Advanced {
                request,
                from: ClosePlanPhase::CancelRequested,
                to: ClosePlanPhase::Cancelled,
            }
        );
        let plan = coordinator
            .plan(request)
            .expect("terminal plan is retained");
        assert_eq!(plan.phase(), ClosePlanPhase::Cancelled);
        assert_eq!(plan.destruction_state(), CloseDestructionState::None);
        assert_eq!(plan.cancellation_state(), CloseCancellationState::Settled);
    }

    #[test]
    fn surface_veto_requires_proved_native_edge_cancellation() {
        let mut coordinator = coordinator();
        let plan = coordinator
            .open_surface(
                authority(3, 5),
                native_edge(),
                SurfaceCloseRequest::CloseContent,
                [requirement(ITEM_A, CloseCapability::Immediate)],
                "complete-roster",
            )
            .expect("surface plan carries complete decisions");
        let request = plan.request();
        assert_eq!(
            coordinator.resolve(
                request,
                plan.items()[0].token(),
                authority(3, 5),
                CloseDecision::Veto,
            ),
            Ok(CloseResolutionOutcome::Vetoed {
                request,
                item: ITEM_A,
            })
        );
        assert!(
            !coordinator
                .plan(request)
                .expect("plan remains")
                .is_terminal()
        );
        assert_eq!(
            coordinator.request_cancel(request, authority(3, 5)),
            CloseAdvanceOutcome::Advanced {
                request,
                from: ClosePlanPhase::Vetoed,
                to: ClosePlanPhase::CancelRequested,
            }
        );
        assert_eq!(
            coordinator.mark_cancellation_effect_emitted(
                request,
                authority(3, 5),
                native_binding(),
                EffectId::new(102),
                CloseObservationGeneration::new(8),
                InventoryGeneration::new(8),
                None,
            ),
            CloseAdvanceOutcome::Advanced {
                request,
                from: ClosePlanPhase::CancelRequested,
                to: ClosePlanPhase::CancelRequested,
            }
        );
        let proof = CloseCancellationProof::from_authoritative_live_close_cleared(
            request,
            native_binding(),
            None,
            Some(EffectId::new(102)),
            Some(EffectId::new(102)),
            CloseObservationGeneration::new(9),
            InventoryGeneration::new(9),
        );

        assert_eq!(
            coordinator.settle_native(
                request,
                authority(3, 5),
                CloseNativeSettlement::CancellationProved(proof),
            ),
            CloseAdvanceOutcome::Advanced {
                request,
                from: ClosePlanPhase::CancelRequested,
                to: ClosePlanPhase::Cancelled,
            }
        );
    }

    #[test]
    fn cancellation_proof_requires_a_strictly_later_provider_generation() {
        let mut coordinator = coordinator();
        let request = cancellation_ready_surface_plan(&mut coordinator);

        for generation in [9, 10] {
            assert_eq!(
                coordinator.settle_native(
                    request,
                    authority(3, 5),
                    CloseNativeSettlement::CancellationProved(cancellation_proof(
                        request,
                        native_binding(),
                        EffectId::new(101),
                        EffectId::new(102),
                        generation,
                    )),
                ),
                CloseAdvanceOutcome::Inert(CloseInertReason::CancellationObservationNotNewer {
                    request,
                    issued_after: CloseObservationGeneration::new(10),
                    actual: CloseObservationGeneration::new(generation),
                })
            );
            assert_eq!(
                coordinator.plan(request).map(ClosePlan::phase),
                Some(ClosePlanPhase::CancelRequested)
            );
        }
    }

    #[test]
    fn cancellation_proof_requires_a_strictly_later_core_receipt_generation() {
        let mut coordinator = coordinator();
        let request = cancellation_ready_surface_plan(&mut coordinator);

        for inventory_generation in [9, 10] {
            let proof = cancellation_proof_at(
                request,
                native_binding(),
                Some(EffectId::new(101)),
                EffectId::new(102),
                99,
                inventory_generation,
            );
            assert_eq!(
                coordinator.settle_native(
                    request,
                    authority(3, 5),
                    CloseNativeSettlement::CancellationProved(proof),
                ),
                CloseAdvanceOutcome::Inert(CloseInertReason::CancellationInventoryNotNewer {
                    request,
                    issued_after: InventoryGeneration::new(10),
                    actual: InventoryGeneration::new(inventory_generation),
                })
            );
        }
    }

    #[test]
    fn cancellation_proof_requires_the_exact_known_effect_frontier() {
        let mut coordinator = coordinator();
        let request = cancellation_ready_surface_plan(&mut coordinator);

        for actual in [None, Some(EffectId::new(101)), Some(EffectId::new(103))] {
            let proof = CloseCancellationProof::from_authoritative_live_close_cleared(
                request,
                native_binding(),
                Some(EffectId::new(101)),
                Some(EffectId::new(102)),
                actual,
                CloseObservationGeneration::new(11),
                InventoryGeneration::new(11),
            );
            assert_eq!(
                coordinator.settle_native(
                    request,
                    authority(3, 5),
                    CloseNativeSettlement::CancellationProved(proof),
                ),
                CloseAdvanceOutcome::Inert(CloseInertReason::CancellationKnownFrontierMismatch {
                    request,
                    expected: Some(EffectId::new(102)),
                    actual,
                })
            );
            assert_eq!(
                coordinator.plan(request).map(|current| (
                    current.phase(),
                    current.destruction_state(),
                    current.cancellation_state(),
                )),
                Some((
                    ClosePlanPhase::CancelRequested,
                    CloseDestructionState::EffectEmitted,
                    CloseCancellationState::Requested,
                ))
            );
        }

        assert_eq!(
            coordinator.settle_native(
                request,
                authority(3, 5),
                CloseNativeSettlement::CancellationProved(valid_cancellation_proof(request)),
            ),
            CloseAdvanceOutcome::Advanced {
                request,
                from: ClosePlanPhase::CancelRequested,
                to: ClosePlanPhase::Cancelled,
            }
        );
    }

    #[test]
    fn cancellation_effect_requires_the_exact_lane_predecessor_not_numeric_order() {
        let mut coordinator = coordinator();
        let plan = approved_retain_surface_plan(&mut coordinator, "complete-roster");
        let request = plan.request();
        emit_native(&mut coordinator, request);
        coordinator.request_cancel(request, authority(3, 5));

        assert_eq!(
            coordinator.mark_cancellation_effect_emitted(
                request,
                authority(3, 5),
                native_binding(),
                EffectId::new(999),
                CloseObservationGeneration::new(10),
                InventoryGeneration::new(10),
                Some(EffectId::new(100)),
            ),
            CloseAdvanceOutcome::Inert(CloseInertReason::CancellationEffectPredecessorMismatch {
                request,
                expected: Some(EffectId::new(101)),
                actual: Some(EffectId::new(100)),
            })
        );
        assert_eq!(
            coordinator.mark_cancellation_effect_emitted(
                request,
                authority(3, 5),
                native_binding(),
                EffectId::new(1),
                CloseObservationGeneration::new(10),
                InventoryGeneration::new(10),
                Some(EffectId::new(101)),
            ),
            CloseAdvanceOutcome::Advanced {
                request,
                from: ClosePlanPhase::CancelRequested,
                to: ClosePlanPhase::CancelRequested,
            }
        );
    }

    #[test]
    fn cancellation_proof_rejects_foreign_request_binding_and_effects_without_mutation() {
        let mut coordinator = coordinator();
        let request = cancellation_ready_surface_plan(&mut coordinator);
        let foreign_domain = EngineAuthorityDomainId::new_for_test(99);
        let foreign_request = CloseRequestId::from_sequence(
            foreign_domain,
            NonZeroU64::new(1).expect("non-zero fixture"),
        );
        let wrong_binding = ViewportBinding::new(
            domain(),
            WorkspaceEpoch::new(7),
            SURFACE,
            WindowToken::new(41),
            WindowIncarnation::new(52),
        );
        let cases = [
            (
                cancellation_proof(
                    foreign_request,
                    native_binding(),
                    EffectId::new(101),
                    EffectId::new(102),
                    11,
                ),
                CloseInertReason::CancellationProofDomainMismatch {
                    request,
                    expected: domain(),
                    actual: foreign_domain,
                },
            ),
            (
                cancellation_proof(
                    request,
                    wrong_binding,
                    EffectId::new(101),
                    EffectId::new(102),
                    11,
                ),
                CloseInertReason::NativeBindingMismatch {
                    request,
                    expected: native_binding(),
                    actual: wrong_binding,
                },
            ),
            (
                cancellation_proof(
                    request,
                    native_binding(),
                    EffectId::new(100),
                    EffectId::new(102),
                    11,
                ),
                CloseInertReason::CancellationEffectPredecessorMismatch {
                    request,
                    expected: Some(EffectId::new(101)),
                    actual: Some(EffectId::new(100)),
                },
            ),
            (
                cancellation_proof(
                    request,
                    native_binding(),
                    EffectId::new(101),
                    EffectId::new(103),
                    11,
                ),
                CloseInertReason::CancellationEffectMismatch {
                    request,
                    expected: Some(EffectId::new(102)),
                    actual: Some(EffectId::new(103)),
                },
            ),
        ];

        for (proof, reason) in cases {
            assert_eq!(
                coordinator.settle_native(
                    request,
                    authority(3, 5),
                    CloseNativeSettlement::CancellationProved(proof),
                ),
                CloseAdvanceOutcome::Inert(reason)
            );
            assert_eq!(
                coordinator.plan(request).map(|plan| (
                    plan.phase(),
                    plan.destruction_state(),
                    plan.cancellation_state(),
                )),
                Some((
                    ClosePlanPhase::CancelRequested,
                    CloseDestructionState::EffectEmitted,
                    CloseCancellationState::Requested,
                ))
            );
        }
    }

    #[test]
    fn destroyed_proof_rejects_foreign_request_binding_and_old_generation_without_mutation() {
        let mut coordinator = coordinator();
        let plan = approved_retain_surface_plan(&mut coordinator, "complete-roster");
        let request = plan.request();
        emit_native(&mut coordinator, request);

        let foreign_domain = EngineAuthorityDomainId::new_for_test(99);
        let foreign_request = CloseRequestId::from_sequence(
            foreign_domain,
            NonZeroU64::new(1).expect("non-zero fixture"),
        );
        let wrong_request = CloseRequestId::from_sequence(
            domain(),
            NonZeroU64::new(request.sequence() + 1).expect("non-zero fixture"),
        );
        let wrong_binding = ViewportBinding::new(
            domain(),
            WorkspaceEpoch::new(7),
            SURFACE,
            WindowToken::new(41),
            WindowIncarnation::new(52),
        );
        let cases = [
            (
                destroyed_proof(foreign_request, native_binding(), 11),
                CloseInertReason::DestroyedProofDomainMismatch {
                    request,
                    expected: domain(),
                    actual: foreign_domain,
                },
            ),
            (
                destroyed_proof(wrong_request, native_binding(), 11),
                CloseInertReason::DestroyedProofRequestMismatch {
                    request,
                    proof_request: wrong_request,
                },
            ),
            (
                destroyed_proof(request, wrong_binding, 11),
                CloseInertReason::NativeBindingMismatch {
                    request,
                    expected: native_binding(),
                    actual: wrong_binding,
                },
            ),
            (
                destroyed_proof(request, native_binding(), 7),
                CloseInertReason::DestroyedObservationNotNewer {
                    request,
                    issued_after: CloseObservationGeneration::new(8),
                    actual: CloseObservationGeneration::new(7),
                },
            ),
            (
                destroyed_proof(request, native_binding(), 8),
                CloseInertReason::DestroyedObservationNotNewer {
                    request,
                    issued_after: CloseObservationGeneration::new(8),
                    actual: CloseObservationGeneration::new(8),
                },
            ),
            (
                destroyed_proof_acknowledging(request, native_binding(), 99, EffectId::new(100)),
                CloseInertReason::DestroyedEffectNotAcknowledged {
                    request,
                    close_effect: EffectId::new(101),
                    cancellation_effect: None,
                    actual: EffectId::new(100),
                },
            ),
        ];

        for (proof, reason) in cases {
            assert_eq!(
                coordinator.settle_native(
                    request,
                    authority(3, 5),
                    CloseNativeSettlement::DestroyedProved(proof),
                ),
                CloseAdvanceOutcome::Inert(reason)
            );
            assert_eq!(
                coordinator.plan(request).map(|plan| (
                    plan.phase(),
                    plan.destruction_state(),
                    plan.cancellation_state(),
                )),
                Some((
                    ClosePlanPhase::EffectEmitted,
                    CloseDestructionState::EffectEmitted,
                    CloseCancellationState::None,
                ))
            );
        }
    }

    #[test]
    fn destroyed_proof_requires_a_strictly_later_core_receipt_generation() {
        let mut coordinator = coordinator();
        let plan = approved_retain_surface_plan(&mut coordinator, "complete-roster");
        let request = plan.request();
        emit_native(&mut coordinator, request);

        for inventory_generation in [7, 8] {
            let proof = destroyed_proof_at(
                request,
                native_binding(),
                99,
                inventory_generation,
                EffectId::new(101),
            );
            assert_eq!(
                coordinator.settle_native(
                    request,
                    authority(3, 5),
                    CloseNativeSettlement::DestroyedProved(proof),
                ),
                CloseAdvanceOutcome::Inert(CloseInertReason::DestroyedInventoryNotNewer {
                    request,
                    issued_after: InventoryGeneration::new(8),
                    actual: InventoryGeneration::new(inventory_generation),
                })
            );
        }
    }

    #[test]
    fn planned_destroyed_settlement_requires_a_destructive_close_effect() {
        let mut coordinator = coordinator();
        let plan = coordinator
            .open_surface(
                authority(3, 5),
                native_edge(),
                rehome_request(),
                [],
                "complete-roster-and-target-proof",
            )
            .expect("rehome plan requires no content decisions");
        let request = plan.request();

        assert_eq!(
            coordinator.settle_native(
                request,
                authority(3, 5),
                CloseNativeSettlement::DestroyedProved(destroyed_proof(
                    request,
                    native_binding(),
                    99,
                )),
            ),
            CloseAdvanceOutcome::Inert(CloseInertReason::CloseEffectNotEmitted { request })
        );
        assert_eq!(
            coordinator.plan(request).map(ClosePlan::phase),
            Some(ClosePlanPhase::Approved)
        );
    }

    #[test]
    fn unproved_external_destruction_is_terminal_without_applying_the_plan() {
        let mut coordinator = coordinator();
        let plan = approved_retain_surface_plan(&mut coordinator, "complete-roster");
        let request = plan.request();
        emit_native(&mut coordinator, request);

        assert_eq!(
            coordinator.mark_externally_destroyed_unproved(request, native_binding()),
            CloseAdvanceOutcome::Advanced {
                request,
                from: ClosePlanPhase::EffectEmitted,
                to: ClosePlanPhase::ExternallyDestroyedUnproved,
            }
        );
        assert_eq!(
            coordinator.plan(request).map(|current| (
                current.phase(),
                current.destruction_state(),
                current.cancellation_state(),
                current.is_terminal(),
            )),
            Some((
                ClosePlanPhase::ExternallyDestroyedUnproved,
                CloseDestructionState::None,
                CloseCancellationState::None,
                true,
            ))
        );
        assert_eq!(coordinator.surface_request_for_edge(native_edge()), None);
        assert!(coordinator.active_plans().next().is_none());
        assert_eq!(
            coordinator.mark_externally_destroyed_unproved(request, native_binding()),
            CloseAdvanceOutcome::Inert(CloseInertReason::Terminal {
                request,
                phase: ClosePlanPhase::ExternallyDestroyedUnproved,
            })
        );
    }

    #[test]
    fn same_batch_requires_both_destroyed_and_cancellation_proofs_to_be_exact() {
        let wrong_binding = ViewportBinding::new(
            domain(),
            WorkspaceEpoch::new(7),
            SURFACE,
            WindowToken::new(41),
            WindowIncarnation::new(52),
        );

        let mut first_coordinator = coordinator();
        let request = cancellation_ready_surface_plan(&mut first_coordinator);
        assert_eq!(
            first_coordinator.settle_native(
                request,
                authority(3, 5),
                CloseNativeSettlement::DestroyedProvedWithCancellation {
                    destroyed: destroyed_proof(request, wrong_binding, 11),
                    cancellation: valid_cancellation_proof(request),
                },
            ),
            CloseAdvanceOutcome::Inert(CloseInertReason::NativeBindingMismatch {
                request,
                expected: native_binding(),
                actual: wrong_binding,
            })
        );
        assert_eq!(
            first_coordinator.plan(request).map(|plan| (
                plan.phase(),
                plan.destruction_state(),
                plan.cancellation_state(),
            )),
            Some((
                ClosePlanPhase::CancelRequested,
                CloseDestructionState::EffectEmitted,
                CloseCancellationState::Requested,
            ))
        );

        let mut second_coordinator = coordinator();
        let request = cancellation_ready_surface_plan(&mut second_coordinator);
        assert_eq!(
            second_coordinator.settle_native(
                request,
                authority(3, 5),
                CloseNativeSettlement::DestroyedProvedWithCancellation {
                    destroyed: valid_destroyed_proof(request),
                    cancellation: cancellation_proof(
                        request,
                        native_binding(),
                        EffectId::new(101),
                        EffectId::new(103),
                        11,
                    ),
                },
            ),
            CloseAdvanceOutcome::Inert(CloseInertReason::CancellationEffectMismatch {
                request,
                expected: Some(EffectId::new(102)),
                actual: Some(EffectId::new(103)),
            })
        );
        assert_eq!(
            second_coordinator.plan(request).map(|plan| (
                plan.phase(),
                plan.destruction_state(),
                plan.cancellation_state(),
            )),
            Some((
                ClosePlanPhase::CancelRequested,
                CloseDestructionState::EffectEmitted,
                CloseCancellationState::Requested,
            ))
        );
    }

    #[test]
    fn same_batch_destruction_wins_over_valid_cancellation_proof() {
        let mut coordinator = coordinator();
        let request = cancellation_ready_surface_plan(&mut coordinator);
        let proof = valid_cancellation_proof(request);

        assert_eq!(
            coordinator.settle_native(
                request,
                authority(3, 5),
                CloseNativeSettlement::DestroyedProvedWithCancellation {
                    destroyed: valid_destroyed_proof(request),
                    cancellation: proof,
                },
            ),
            CloseAdvanceOutcome::Advanced {
                request,
                from: ClosePlanPhase::CancelRequested,
                to: ClosePlanPhase::Applied,
            }
        );
        assert_eq!(
            coordinator.settle_native(
                request,
                authority(3, 5),
                CloseNativeSettlement::CancellationProved(proof),
            ),
            CloseAdvanceOutcome::Inert(CloseInertReason::Terminal {
                request,
                phase: ClosePlanPhase::Applied,
            })
        );
    }

    #[test]
    fn invalid_plan_shapes_are_rejected_before_any_identity_is_consumed() {
        let mut coordinator = coordinator();
        assert_eq!(
            coordinator.open(
                authority(3, 5),
                ClosePlanTarget::Item { item: ITEM_A },
                [requirement(ITEM_B, CloseCapability::Immediate)],
                (),
            ),
            Err(CloseCoordinatorError::ItemTargetMismatch {
                target: ITEM_A,
                actual: vec![ITEM_B],
            })
        );
        assert_eq!(
            coordinator.open(
                authority(3, 5),
                ClosePlanTarget::Root { root: ROOT },
                [
                    requirement(ITEM_A, CloseCapability::Immediate),
                    requirement(ITEM_A, CloseCapability::Immediate),
                ],
                (),
            ),
            Err(CloseCoordinatorError::DuplicateItem { item: ITEM_A })
        );
        assert_eq!(
            coordinator.open(
                authority(3, 5),
                ClosePlanTarget::Root { root: ROOT },
                [requirement(ITEM_A, CloseCapability::Disabled)],
                (),
            ),
            Err(CloseCoordinatorError::DisabledItem { item: ITEM_A })
        );

        let first_valid = coordinator
            .open(
                authority(3, 5),
                ClosePlanTarget::Item { item: ITEM_A },
                [requirement(ITEM_A, CloseCapability::Immediate)],
                (),
            )
            .expect("invalid plans do not consume request identities");
        assert_eq!(first_valid.request().sequence(), 1);
        assert_eq!(first_valid.items()[0].token().sequence(), 1);
    }

    #[test]
    fn coordinator_rejects_authority_from_another_domain_before_minting_ids() {
        let mut coordinator = coordinator();
        let foreign = CloseAuthority::new(
            EngineAuthorityDomainId::new_for_test(99),
            WorkspaceVersion::new(WorkspaceEpoch::new(7), WorkspaceRevision::new(3)),
            PolicyRevision::new(5),
        );

        assert_eq!(
            coordinator.open(
                foreign,
                ClosePlanTarget::Item { item: ITEM_A },
                [requirement(ITEM_A, CloseCapability::Immediate)],
                (),
            ),
            Err(CloseCoordinatorError::AuthorityDomainMismatch {
                expected: domain(),
                actual: EngineAuthorityDomainId::new_for_test(99),
            })
        );

        let first = coordinator
            .open(
                authority(3, 5),
                ClosePlanTarget::Item { item: ITEM_A },
                [requirement(ITEM_A, CloseCapability::Immediate)],
                (),
            )
            .expect("foreign authority does not consume identities");
        assert_eq!(first.request().sequence(), 1);
        assert_eq!(first.items()[0].token().sequence(), 1);
    }

    #[test]
    fn plan_inventory_retains_terminal_snapshots_and_filters_active_plans() {
        let mut coordinator = coordinator();
        let first = root_plan(
            &mut coordinator,
            [requirement(ITEM_A, CloseCapability::Immediate)],
        );
        let second = root_plan(
            &mut coordinator,
            [requirement(ITEM_B, CloseCapability::Immediate)],
        );
        coordinator
            .resolve(
                first.request(),
                first.items()[0].token(),
                authority(3, 5),
                CloseDecision::Veto,
            )
            .expect("veto does not allocate a continuation");

        assert_eq!(
            coordinator
                .plans()
                .map(ClosePlan::request)
                .collect::<Vec<_>>(),
            vec![first.request(), second.request()]
        );
        assert_eq!(
            coordinator
                .active_plans()
                .map(ClosePlan::request)
                .collect::<Vec<_>>(),
            vec![second.request()]
        );
        assert_eq!(
            coordinator.plan(first.request()).map(ClosePlan::phase),
            Some(ClosePlanPhase::Vetoed)
        );
    }

    #[test]
    fn explicit_invalidation_marks_all_active_authority_mismatches_once() {
        let mut coordinator = coordinator();
        let first = root_plan(
            &mut coordinator,
            [requirement(ITEM_A, CloseCapability::Immediate)],
        );
        let second = root_plan(
            &mut coordinator,
            [requirement(ITEM_B, CloseCapability::Immediate)],
        );

        assert_eq!(
            coordinator.invalidate_stale(authority(3, 6)),
            vec![first.request(), second.request()]
        );
        assert!(coordinator.invalidate_stale(authority(3, 6)).is_empty());
        assert_eq!(
            coordinator.plan(first.request()).map(ClosePlan::phase),
            Some(ClosePlanPhase::Stale)
        );
    }

    #[test]
    fn root_and_surface_plan_shapes_require_the_exact_decision_class() {
        let mut coordinator = coordinator();

        assert_eq!(
            coordinator.open(
                authority(3, 5),
                ClosePlanTarget::Root { root: ROOT },
                [],
                (),
            ),
            Err(CloseCoordinatorError::EmptyRootHasNoItemDecisions { root: ROOT })
        );
        let retain = coordinator
            .open_surface(
                authority(3, 5),
                native_edge(),
                SurfaceCloseRequest::RetainLayout,
                [],
                (),
            )
            .expect("an empty retain-layout surface needs no pane decisions");
        assert_eq!(retain.phase(), ClosePlanPhase::Approved);
        let retain_with_decisions = coordinator
            .open_surface(
                authority(3, 5),
                native_edge(),
                SurfaceCloseRequest::RetainLayout,
                [requirement(ITEM_A, CloseCapability::Immediate)],
                (),
            )
            .expect("retain-layout may freeze pane-close decisions");
        assert_eq!(retain_with_decisions.phase(), ClosePlanPhase::Requested);
        assert_eq!(
            coordinator.open_surface(
                authority(3, 5),
                native_edge(),
                rehome_request(),
                [requirement(ITEM_A, CloseCapability::Immediate)],
                (),
            ),
            Err(CloseCoordinatorError::SurfaceRehomeHasItemDecisions {
                surface: SURFACE,
                item_count: 1,
            })
        );

        assert_eq!(
            coordinator.open_surface(
                authority(3, 5),
                native_edge(),
                SurfaceCloseRequest::CloseContent,
                [],
                (),
            ),
            Err(CloseCoordinatorError::SurfaceCloseContentHasNoItemDecisions { surface: SURFACE })
        );

        let close_content = coordinator
            .open_surface(
                authority(3, 5),
                native_edge(),
                SurfaceCloseRequest::CloseContent,
                [
                    requirement(ITEM_A, CloseCapability::Immediate),
                    requirement(ITEM_B, CloseCapability::DeferredAllowed),
                ],
                (),
            )
            .expect("close-content carries the complete content-decision roster");
        assert_eq!(close_content.phase(), ClosePlanPhase::Requested);

        let rehome = coordinator
            .open_surface(authority(3, 5), native_edge(), rehome_request(), [], ())
            .expect("rehome carries no content-close decisions");
        assert_eq!(rehome.phase(), ClosePlanPhase::Approved);
    }

    #[test]
    fn surface_plan_cannot_exist_without_an_exact_native_edge() {
        let mut coordinator = coordinator();
        assert_eq!(
            coordinator.open(
                authority(3, 5),
                ClosePlanTarget::Surface {
                    surface: SURFACE,
                    disposition: SurfaceCloseDisposition::RehomeAll {
                        target: TARGET_SURFACE,
                    },
                },
                [],
                (),
            ),
            Err(CloseCoordinatorError::SurfaceRequiresNativeEdge)
        );

        let foreign_domain = EngineAuthorityDomainId::new_for_test(8);
        let foreign_edge = NativeCloseEdge::from_authoritative_requested(
            foreign_domain,
            native_binding(),
            CloseObservationGeneration::new(7),
            InventoryGeneration::new(7),
        );
        assert_eq!(
            coordinator.open_surface(authority(3, 5), foreign_edge, rehome_request(), [], (),),
            Err(CloseCoordinatorError::NativeEdgeDomainMismatch {
                expected: domain(),
                actual: foreign_domain,
            })
        );

        let plan = coordinator
            .open_surface(authority(3, 5), native_edge(), rehome_request(), [], ())
            .expect("failed surface opens do not consume request identity");
        assert_eq!(plan.request().sequence(), 1);
        assert_eq!(coordinator.native_edge(plan.request()), Some(native_edge()));
    }

    #[test]
    fn destroyed_proof_exposes_only_its_exact_causal_facts() {
        let request =
            CloseRequestId::from_sequence(domain(), NonZeroU64::new(1).expect("non-zero fixture"));
        let proof = CloseDestroyedProof::from_authoritative_destroyed(
            request,
            native_binding(),
            CloseObservationGeneration::new(11),
            InventoryGeneration::new(12),
            EffectId::new(101),
        );

        assert_eq!(proof.request(), request);
        assert_eq!(proof.binding(), native_binding());
        assert_eq!(
            proof.observation_generation(),
            CloseObservationGeneration::new(11)
        );
        assert_eq!(proof.inventory_generation(), InventoryGeneration::new(12));
        assert_eq!(proof.acknowledged_effect(), EffectId::new(101));
    }
}
