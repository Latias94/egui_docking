//! Renderer-originated semantic interaction inputs.
//!
//! Adapters are responsible for turning framework callbacks into these values.
//! The core never derives button releases, hovered surfaces, drag thresholds, or
//! presentation fallback from timing or geometry history.

use crate::command::{MovePayload, NodeSource};
use crate::coordinates::{TearOffPlacementProof, ViewportPlacementProof};
use crate::geometry::{LogicalPoint, LogicalRect, LogicalSize, PhysicalRect};
use crate::graph::SplitWeight;
use crate::ids::{FloatingPresentationId, RootId, SurfaceId};
use crate::interaction::{
    ContainedTransformPaintAcknowledgement, ContainedTransformSessionId, DragSessionId,
    InteractionCancelReason, PaintAcknowledgement, ResizeSessionId,
};
use crate::scene::SceneStamp;
use crate::viewport_route::ViewportRouteProof;

/// Authority attached to a provider observation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Authority<T> {
    /// The provider authoritatively observed this value.
    Known(T),
    /// The provider cannot authoritatively answer this question.
    Unknown(AuthorityUnavailableReason),
}

impl<T> Authority<T> {
    /// Returns a shared known value, or `None` when authority is unavailable.
    #[must_use]
    pub const fn known(&self) -> Option<&T> {
        match self {
            Self::Known(value) => Some(value),
            Self::Unknown(_) => None,
        }
    }
}

/// Why a provider observation is not authoritative.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AuthorityUnavailableReason {
    /// The active backend does not expose the required fact.
    ProviderUnavailable,
    /// The operating environment withheld the required permission.
    PermissionDenied,
    /// The referenced surface is not currently observable.
    SurfaceUnavailable,
    /// A point cannot be converted into the target surface's coordinates.
    CoordinateUnavailable,
    /// The provider produced no observation for this boundary.
    NotReported,
}

/// Stable pointer identity supplied by the renderer or platform adapter.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(transparent)]
pub struct PointerId(u64);

impl PointerId {
    /// Creates a pointer identity from its adapter-owned representation.
    #[must_use]
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    /// Returns the adapter-owned representation.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}

/// Pointer button bound to one gesture session.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum PointerButton {
    /// Primary selection and drag button.
    Primary,
    /// Secondary context button.
    Secondary,
    /// Middle pointer button.
    Middle,
    /// Renderer-defined additional button.
    Other(u16),
}

/// Authoritative button state carried by a release intent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PointerButtonState {
    /// The matching button remains pressed.
    Pressed,
    /// The matching button was released.
    Released,
}

/// Horizontal edge moved by an explicit contained resize gesture.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ContainedHorizontalResizeEdge {
    /// Move the left edge while holding the right edge fixed.
    Left,
    /// Move the right edge while holding the left edge fixed.
    Right,
}

/// Vertical edge moved by an explicit contained resize gesture.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ContainedVerticalResizeEdge {
    /// Move the top edge while holding the bottom edge fixed.
    Top,
    /// Move the bottom edge while holding the top edge fixed.
    Bottom,
}

/// Non-empty explicit edge set for one contained resize gesture.
///
/// The private representation and constructors make an empty set and two
/// opposing edges on the same axis unrepresentable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ContainedResizeEdges {
    horizontal: Option<ContainedHorizontalResizeEdge>,
    vertical: Option<ContainedVerticalResizeEdge>,
}

impl ContainedResizeEdges {
    /// Creates a horizontal edge resize.
    #[must_use]
    pub const fn horizontal(edge: ContainedHorizontalResizeEdge) -> Self {
        Self {
            horizontal: Some(edge),
            vertical: None,
        }
    }

    /// Creates a vertical edge resize.
    #[must_use]
    pub const fn vertical(edge: ContainedVerticalResizeEdge) -> Self {
        Self {
            horizontal: None,
            vertical: Some(edge),
        }
    }

    /// Creates a corner resize with one explicit edge on each axis.
    #[must_use]
    pub const fn corner(
        horizontal: ContainedHorizontalResizeEdge,
        vertical: ContainedVerticalResizeEdge,
    ) -> Self {
        Self {
            horizontal: Some(horizontal),
            vertical: Some(vertical),
        }
    }

    /// Returns the moving horizontal edge, when this gesture changes width.
    #[must_use]
    pub const fn horizontal_edge(self) -> Option<ContainedHorizontalResizeEdge> {
        self.horizontal
    }

    /// Returns the moving vertical edge, when this gesture changes height.
    #[must_use]
    pub const fn vertical_edge(self) -> Option<ContainedVerticalResizeEdge> {
        self.vertical
    }
}

/// Exact geometry operation owned by one contained transform session.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ContainedTransformKind {
    /// Translate the frozen rectangle without changing its size.
    Move,
    /// Move only the explicit edges while preserving the opposite anchors.
    Resize(ContainedResizeEdges),
}

/// A point already converted into one logical surface's coordinate space.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SurfacePointer {
    surface: SurfaceId,
    position: LogicalPoint,
}

impl SurfacePointer {
    /// Creates one authoritative surface-local pointer location.
    #[must_use]
    pub const fn new(surface: SurfaceId, position: LogicalPoint) -> Self {
        Self { surface, position }
    }

    /// Returns the logical target surface.
    #[must_use]
    pub const fn surface(&self) -> SurfaceId {
        self.surface
    }

    /// Returns the point in that target's logical coordinate space.
    #[must_use]
    pub const fn position(self) -> LogicalPoint {
        self.position
    }
}

/// Renderer-observed origin of one whole contained-presentation title drag.
///
/// The adapter supplies only stable identity, the absolute press location, and
/// its measured minimum size. The core validates that the payload is the exact
/// complete root owned by this presentation and freezes the durable source
/// rectangle itself.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ContainedDragOrigin {
    root: RootId,
    floating: FloatingPresentationId,
    initial_pointer: SurfacePointer,
    minimum_size: LogicalSize,
}

impl ContainedDragOrigin {
    /// Creates explicit contained-presentation origin facts.
    #[must_use]
    pub const fn new(
        root: RootId,
        floating: FloatingPresentationId,
        initial_pointer: SurfacePointer,
        minimum_size: LogicalSize,
    ) -> Self {
        Self {
            root,
            floating,
            initial_pointer,
            minimum_size,
        }
    }

    /// Returns the complete source root claimed by the title interaction.
    #[must_use]
    pub const fn root(self) -> RootId {
        self.root
    }

    /// Returns the contained presentation claimed by the title interaction.
    #[must_use]
    pub const fn floating(self) -> FloatingPresentationId {
        self.floating
    }

    /// Returns the authoritative press location in the host surface.
    #[must_use]
    pub const fn initial_pointer(self) -> SurfacePointer {
        self.initial_pointer
    }

    /// Returns the renderer-measured minimum size frozen for the drag.
    #[must_use]
    pub const fn minimum_size(self) -> LogicalSize {
        self.minimum_size
    }
}

/// Typed source semantics for the canonical core-owned drag protocol.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub enum DragOrigin {
    /// A tab, tabs stack, subtree, or main-surface root without contained move semantics.
    #[default]
    Workspace,
    /// The title of one existing contained presentation.
    Contained(ContainedDragOrigin),
}

/// Core-owned stacking request for a newly created contained presentation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ContainedStackPlacement {
    /// Place the new presentation in front of every current peer on its surface.
    Front,
}

/// One application-owned identity and geometry offer for a new contained presentation.
///
/// The first offer is frozen for the drag session. Later observations may omit
/// it and reuse the reservation, but cannot replace its identities or geometry.
/// The anchor binds the requested rectangle to one absolute pointer observation,
/// allowing the core to translate it from later pointer positions.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ContainedPresentationOffer {
    root: RootId,
    floating: FloatingPresentationId,
    anchor: SurfacePointer,
    requested_rect: LogicalRect,
    minimum_size: LogicalSize,
    stacking: ContainedStackPlacement,
}

impl ContainedPresentationOffer {
    /// Offers stable identities and initial geometry with core-owned front placement.
    #[must_use]
    pub const fn front(
        root: RootId,
        floating: FloatingPresentationId,
        anchor: SurfacePointer,
        requested_rect: LogicalRect,
        minimum_size: LogicalSize,
    ) -> Self {
        Self {
            root,
            floating,
            anchor,
            requested_rect,
            minimum_size,
            stacking: ContainedStackPlacement::Front,
        }
    }

    /// Returns the stable root identity reserved by the application.
    #[must_use]
    pub const fn root(self) -> RootId {
        self.root
    }

    /// Returns the stable contained-presentation identity reserved by the application.
    #[must_use]
    pub const fn floating(self) -> FloatingPresentationId {
        self.floating
    }

    /// Returns the pointer location associated with the initial requested rectangle.
    #[must_use]
    pub const fn anchor(self) -> SurfacePointer {
        self.anchor
    }

    /// Returns the initial requested rectangle before deterministic clamping.
    #[must_use]
    pub const fn requested_rect(self) -> LogicalRect {
        self.requested_rect
    }

    /// Returns the minimum contained size frozen with this offer.
    #[must_use]
    pub const fn minimum_size(self) -> LogicalSize {
        self.minimum_size
    }

    /// Returns the core-owned stacking semantic for this offer.
    #[must_use]
    pub const fn stacking(self) -> ContainedStackPlacement {
        self.stacking
    }
}

/// Authoritative hovered-target observation and its provenance.
///
/// Local facts are valid only for one renderer callback surface. Cross-native-window facts must
/// use an opaque core-produced route proof whose generations are revalidated at delivery.
#[derive(Debug, Clone, PartialEq)]
pub enum TargetAuthority {
    Local(LocalTargetObservation),
    Routed(ViewportRouteProof),
}

/// Renderer-local target facts bound to the exact callback surface that observed them.
#[derive(Debug, Clone, PartialEq)]
pub struct LocalTargetObservation {
    observer: SurfaceId,
    target: Authority<Option<SurfacePointer>>,
}

impl LocalTargetObservation {
    #[must_use]
    pub const fn observer(&self) -> SurfaceId {
        self.observer
    }

    #[must_use]
    pub const fn target(&self) -> &Authority<Option<SurfacePointer>> {
        &self.target
    }
}

impl TargetAuthority {
    #[must_use]
    pub const fn local(observer: SurfaceId, target: Authority<Option<SurfacePointer>>) -> Self {
        Self::Local(LocalTargetObservation { observer, target })
    }

    #[must_use]
    pub const fn routed(proof: ViewportRouteProof) -> Self {
        Self::Routed(proof)
    }

    #[must_use]
    pub const fn route_proof(&self) -> Option<&ViewportRouteProof> {
        match self {
            Self::Local(_) => None,
            Self::Routed(proof) => Some(proof),
        }
    }
}

/// Why the core cannot authorize one contained-floating placement.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum ContainedPlacementUnavailable {
    /// No sealed scene is currently published.
    #[error("no sealed scene is available for contained placement")]
    SceneUnavailable,
    /// The published scene no longer matches the proof's exact generation.
    #[error("contained placement scene {expected:?} is stale; current scene is {current:?}")]
    StaleScene {
        /// Scene generation which authorized the placement.
        expected: SceneStamp,
        /// Currently published scene, when one exists.
        current: Option<SceneStamp>,
    },
    /// The surface is absent from the sealed scene roster.
    #[error("surface {surface} is absent from the sealed scene")]
    MissingSurface {
        /// Requested logical surface.
        surface: SurfaceId,
    },
    /// The surface exists but has not published authoritative bounds.
    #[error("surface {surface} is still in bootstrap scene state")]
    BootstrapSurface {
        /// Requested logical surface.
        surface: SurfaceId,
    },
    /// Finite corners produced an unrepresentable width, height, or clamp result.
    #[error("surface {surface} cannot represent the requested contained placement")]
    UnrepresentableGeometry {
        /// Surface whose bounds or requested geometry cannot be clamped safely.
        surface: SurfaceId,
    },
    /// A supposedly core-produced proof does not reproduce under its bound scene facts.
    #[error("contained placement proof for surface {surface} does not reproduce")]
    ProofMismatch {
        /// Surface named by the invalid proof.
        surface: SurfaceId,
    },
}

/// Opaque authorization for one deterministic contained-floating placement.
///
/// Adapters cannot construct this type. The proof binds both the requested geometry and the
/// exact clamped geometry to one ready surface in one sealed scene generation.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ContainedPlacementProof {
    scene: SceneStamp,
    surface: SurfaceId,
    requested_rect: LogicalRect,
    minimum_size: LogicalSize,
    surface_bounds: LogicalRect,
    clamped_rect: LogicalRect,
}

impl ContainedPlacementProof {
    pub(crate) const fn new(
        scene: SceneStamp,
        surface: SurfaceId,
        requested_rect: LogicalRect,
        minimum_size: LogicalSize,
        surface_bounds: LogicalRect,
        clamped_rect: LogicalRect,
    ) -> Self {
        Self {
            scene,
            surface,
            requested_rect,
            minimum_size,
            surface_bounds,
            clamped_rect,
        }
    }

    /// Returns the exact sealed scene generation which authorized this placement.
    #[must_use]
    pub const fn scene(self) -> SceneStamp {
        self.scene
    }

    /// Returns the host logical surface.
    #[must_use]
    pub const fn surface(self) -> SurfaceId {
        self.surface
    }

    /// Returns the adapter's requested logical rectangle before deterministic clamping.
    #[must_use]
    pub const fn requested_rect(self) -> LogicalRect {
        self.requested_rect
    }

    /// Returns the explicit minimum size used by deterministic clamping.
    #[must_use]
    pub const fn minimum_size(self) -> LogicalSize {
        self.minimum_size
    }

    /// Returns the ready surface bounds which authorized the clamp.
    #[must_use]
    pub const fn surface_bounds(self) -> LogicalRect {
        self.surface_bounds
    }

    /// Returns the exact contained rectangle authorized by the core.
    #[must_use]
    pub const fn clamped_rect(self) -> LogicalRect {
        self.clamped_rect
    }
}

/// Exact proposal for a contained-floating destination.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ContainedTearOffProposal {
    root: RootId,
    floating: FloatingPresentationId,
    placement: ContainedPlacementProof,
    z_order: u64,
}

impl ContainedTearOffProposal {
    /// Creates an explicit contained-floating proposal.
    #[must_use]
    pub const fn new(
        root: RootId,
        floating: FloatingPresentationId,
        placement: ContainedPlacementProof,
        z_order: u64,
    ) -> Self {
        Self {
            root,
            floating,
            placement,
            z_order,
        }
    }

    /// Returns the host logical surface.
    #[must_use]
    pub const fn surface(self) -> SurfaceId {
        self.placement.surface()
    }

    /// Returns the root identity used for newly detached content.
    #[must_use]
    pub const fn root(&self) -> RootId {
        self.root
    }

    /// Returns the contained presentation identity.
    #[must_use]
    pub const fn floating(self) -> FloatingPresentationId {
        self.floating
    }

    /// Returns the acknowledged logical placement.
    #[must_use]
    pub const fn rect(self) -> LogicalRect {
        self.placement.clamped_rect()
    }

    /// Returns the core-produced placement authorization.
    #[must_use]
    pub const fn placement(self) -> ContainedPlacementProof {
        self.placement
    }

    /// Returns the explicit contained stacking order.
    #[must_use]
    pub const fn z_order(self) -> u64 {
        self.z_order
    }
}

/// Durable recovery intent for a native surface.
///
/// Unlike [`ContainedTearOffProposal`], this value deliberately carries no scene
/// generation or clamping proof. The engine re-authorizes it against the current
/// sealed scene when recovery is actually committed.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ContainedRecoveryPlan {
    root: RootId,
    floating: FloatingPresentationId,
    surface: SurfaceId,
    requested_rect: LogicalRect,
    minimum_size: LogicalSize,
    z_order: u64,
}

impl ContainedRecoveryPlan {
    /// Creates a durable recovery intent from explicit logical geometry.
    #[must_use]
    pub const fn new(
        root: RootId,
        floating: FloatingPresentationId,
        surface: SurfaceId,
        requested_rect: LogicalRect,
        minimum_size: LogicalSize,
        z_order: u64,
    ) -> Self {
        Self {
            root,
            floating,
            surface,
            requested_rect,
            minimum_size,
            z_order,
        }
    }

    /// Copies the durable fields from a scene-bound proposal.
    #[must_use]
    pub const fn from_proposal(proposal: ContainedTearOffProposal) -> Self {
        let placement = proposal.placement();
        Self::new(
            proposal.root(),
            proposal.floating(),
            proposal.surface(),
            placement.requested_rect(),
            placement.minimum_size(),
            proposal.z_order(),
        )
    }

    #[must_use]
    pub const fn root(self) -> RootId {
        self.root
    }

    #[must_use]
    pub const fn floating(self) -> FloatingPresentationId {
        self.floating
    }

    #[must_use]
    pub const fn surface(self) -> SurfaceId {
        self.surface
    }

    #[must_use]
    pub const fn requested_rect(self) -> LogicalRect {
        self.requested_rect
    }

    #[must_use]
    pub const fn minimum_size(self) -> LogicalSize {
        self.minimum_size
    }

    #[must_use]
    pub const fn z_order(self) -> u64 {
        self.z_order
    }

    /// Returns a copy with a newer authoritative requested rectangle.
    #[must_use]
    pub const fn with_requested_rect(self, requested_rect: LogicalRect) -> Self {
        Self {
            requested_rect,
            ..self
        }
    }

    /// Returns whether two plans identify the same replacement-registration contract.
    ///
    /// The core may reproject `requested_rect` from newer authoritative native
    /// geometry while a surface is awaiting replacement. All other durable
    /// fields must still match before the replacement may adopt that pending
    /// recovery.
    pub(crate) fn matches_registration(self, candidate: Self) -> bool {
        self.root == candidate.root
            && self.floating == candidate.floating
            && self.surface == candidate.surface
            && self.minimum_size == candidate.minimum_size
            && self.z_order == candidate.z_order
    }
}

impl From<ContainedTearOffProposal> for ContainedRecoveryPlan {
    fn from(proposal: ContainedTearOffProposal) -> Self {
        Self::from_proposal(proposal)
    }
}

/// Exact proposal for a future native-surface lifecycle saga.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum NativePlacementProof {
    Surface(ViewportPlacementProof),
    TearOff(TearOffPlacementProof),
}

impl NativePlacementProof {
    #[must_use]
    pub const fn physical_rect(self) -> PhysicalRect {
        match self {
            Self::Surface(proof) => proof.physical_rect(),
            Self::TearOff(proof) => proof.physical_rect(),
        }
    }
}

impl From<ViewportPlacementProof> for NativePlacementProof {
    fn from(proof: ViewportPlacementProof) -> Self {
        Self::Surface(proof)
    }
}

impl From<TearOffPlacementProof> for NativePlacementProof {
    fn from(proof: TearOffPlacementProof) -> Self {
        Self::TearOff(proof)
    }
}

/// Exact proposal for a future native-surface lifecycle saga.
#[derive(Debug, Clone, PartialEq)]
pub struct NativeTearOffProposal {
    surface: SurfaceId,
    root: RootId,
    placement: NativePlacementProof,
    recovery: ContainedTearOffProposal,
}

impl NativeTearOffProposal {
    /// Creates an explicit native-surface proposal with authoritative placement.
    #[must_use]
    pub fn new(
        surface: SurfaceId,
        root: RootId,
        placement: impl Into<NativePlacementProof>,
        recovery: ContainedTearOffProposal,
    ) -> Self {
        Self {
            surface,
            root,
            placement: placement.into(),
            recovery,
        }
    }

    /// Returns the logical surface identity to create.
    #[must_use]
    pub const fn surface(&self) -> SurfaceId {
        self.surface
    }

    /// Returns the root identity used for newly detached content.
    #[must_use]
    pub const fn root(&self) -> RootId {
        self.root
    }

    /// Returns the authoritative desktop-physical placement.
    #[must_use]
    pub const fn placement(&self) -> &NativePlacementProof {
        &self.placement
    }

    /// Returns the exact desktop-physical placement carried by the proof.
    #[must_use]
    pub fn physical_placement(&self) -> PhysicalRect {
        self.placement.physical_rect()
    }

    /// Returns the whole-root contained recovery plan used after destruction.
    #[must_use]
    pub const fn recovery(&self) -> ContainedTearOffProposal {
        self.recovery
    }
}

/// Explicit non-docking presentation request and all facts required to preview it.
///
/// With an authoritative surface-local pointer, a contained request is considered
/// only after exact drop resolution proves [`crate::drop_resolver::DropResolution::KnownNone`].
/// A resolved or rejected exact target always wins, and unavailable authority never
/// falls back. With authoritative `Known(None)`, the request retains its direct
/// tear-off semantics.
#[derive(Debug, Clone, PartialEq)]
pub enum TearOffRequest {
    /// Request an immediate contained creation, rehome, or same-presentation move.
    Contained(ContainedTearOffProposal),
    /// Request a native lifecycle saga, with an independently explicit fallback.
    Native {
        /// Native destination and placement.
        proposal: Box<NativeTearOffProposal>,
        /// Optional contained proposal used only when policy enables fallback.
        contained_fallback: Option<ContainedTearOffProposal>,
    },
}

impl TearOffRequest {
    /// Creates a native tear-off request without inflating every request to the native payload
    /// size.
    #[must_use]
    pub fn native(
        proposal: NativeTearOffProposal,
        contained_fallback: Option<ContainedTearOffProposal>,
    ) -> Self {
        Self::Native {
            proposal: Box::new(proposal),
            contained_fallback,
        }
    }
}

/// Semantic interaction input queued by a renderer callback.
#[derive(Debug, Clone, PartialEq)]
pub enum RendererIntent {
    /// Arm the canonical core-owned drag protocol from typed source semantics.
    ArmDragFrom {
        /// Pointer which pressed the source.
        pointer: PointerId,
        /// Button which pressed the source.
        button: PointerButton,
        /// Frozen workspace payload.
        payload: MovePayload,
        /// Explicit source presentation semantics validated by the core.
        origin: DragOrigin,
    },
    /// Arm a drag without inferring a movement threshold.
    ArmDrag {
        /// Pointer which pressed the source.
        pointer: PointerId,
        /// Button which pressed the source.
        button: PointerButton,
        /// Frozen workspace payload.
        payload: MovePayload,
    },
    /// Explicitly cross the renderer-owned drag threshold.
    BeginDrag {
        /// Armed drag generation.
        session: DragSessionId,
        /// Matching pointer.
        pointer: PointerId,
        /// Matching button.
        button: PointerButton,
    },
    /// Replace the authoritative target observation for an active drag.
    UpdateDrag {
        /// Active drag generation.
        session: DragSessionId,
        /// Authoritative target surface and location, known none, or unknown.
        target: TargetAuthority,
        /// Explicit non-docking request for `Known(None)`, or a contained
        /// alternative used only when exact surface resolution is known none.
        tear_off: Option<TearOffRequest>,
    },
    /// Replace one canonical drag's independent pointer and target observations.
    UpdateDragObservation {
        /// Active drag generation.
        session: DragSessionId,
        /// Authoritative target-surface observation, independent from pointer position.
        target: TargetAuthority,
        /// Current absolute pointer position or an explicit lack of authority.
        current_pointer: Authority<SurfacePointer>,
        /// Optional first offer for a newly contained presentation.
        contained_offer: Option<ContainedPresentationOffer>,
    },
    /// Confirm that the exact published preview was painted.
    AcknowledgePreview(PaintAcknowledgement),
    /// Attempt one authoritative matching release.
    ReleaseDrag {
        /// Active drag generation.
        session: DragSessionId,
        /// Matching pointer.
        pointer: PointerId,
        /// Matching button.
        button: PointerButton,
        /// Authoritative state of that exact button.
        button_state: Authority<PointerButtonState>,
        /// Authoritative release target and location.
        target: TargetAuthority,
        /// Exact non-docking request which participated in the painted preview.
        tear_off: Option<TearOffRequest>,
    },
    /// Attempt canonical release using an independent current pointer observation.
    ReleaseDragObservation {
        /// Active drag generation.
        session: DragSessionId,
        /// Matching pointer.
        pointer: PointerId,
        /// Matching button.
        button: PointerButton,
        /// Authoritative state of that exact button.
        button_state: Authority<PointerButtonState>,
        /// Authoritative release target, independent from pointer position.
        target: TargetAuthority,
        /// Current absolute pointer position or an explicit lack of authority.
        current_pointer: Authority<SurfacePointer>,
        /// Optional first offer, or an exact repeat of the session's frozen offer.
        contained_offer: Option<ContainedPresentationOffer>,
    },
    /// Cancel an active drag for an explicit reason.
    CancelDrag {
        /// Drag generation being cancelled.
        session: DragSessionId,
        /// Explicit cancellation cause.
        reason: InteractionCancelReason,
    },
    /// Begin a mutually exclusive splitter-resize gesture.
    BeginResize {
        /// Pointer which pressed the splitter.
        pointer: PointerId,
        /// Button which pressed the splitter.
        button: PointerButton,
        /// Frozen split source.
        split: NodeSource,
    },
    /// Replace the exact proposed split weights.
    UpdateResize {
        /// Active resize generation.
        session: ResizeSessionId,
        /// Already normalized exact weights.
        weights: Vec<SplitWeight>,
    },
    /// Commit the last validated resize proposal on authoritative release.
    ReleaseResize {
        /// Active resize generation.
        session: ResizeSessionId,
        /// Matching pointer.
        pointer: PointerId,
        /// Matching button.
        button: PointerButton,
        /// Authoritative state of that exact button.
        button_state: Authority<PointerButtonState>,
    },
    /// Cancel an active resize for an explicit reason.
    CancelResize {
        /// Resize generation being cancelled.
        session: ResizeSessionId,
        /// Explicit cancellation cause.
        reason: InteractionCancelReason,
    },
    /// Apply one exact programmatic placement using a current core proof.
    ///
    /// This one-shot path is intended for scene-bound reconciliation and
    /// keyboard or programmatic movement. Continuous pointer gestures use the
    /// contained transform session protocol below.
    ApplyContainedPlacement {
        /// Root presented by the contained floating.
        root: RootId,
        /// Exact contained presentation identity.
        floating: FloatingPresentationId,
        /// Exact previous rectangle captured from the workspace.
        expected_rect: LogicalRect,
        /// Current scene-bound placement authorization.
        placement: ContainedPlacementProof,
    },
    /// Begin a mutually exclusive core-owned contained move or resize.
    BeginContainedTransform {
        /// Ready surface which owns the contained floating.
        surface: SurfaceId,
        /// Root presented by the contained floating.
        root: RootId,
        /// Exact contained presentation identity.
        floating: FloatingPresentationId,
        /// Pointer which pressed the move or resize affordance.
        pointer: PointerId,
        /// Button which owns the gesture.
        button: PointerButton,
        /// Absolute pointer location in the host surface's logical coordinates.
        initial_pointer: LogicalPoint,
        /// Explicit move or edge-resize operation.
        kind: ContainedTransformKind,
        /// Minimum contained size enforced by the core.
        minimum_size: LogicalSize,
    },
    /// Recompute one contained transform from the frozen rectangle and absolute pointer.
    UpdateContainedTransform {
        /// Active transform generation.
        session: ContainedTransformSessionId,
        /// Current absolute pointer location in the frozen host surface.
        current_pointer: LogicalPoint,
    },
    /// Confirm that the exact contained transform preview was painted.
    AcknowledgeContainedTransformPreview(ContainedTransformPaintAcknowledgement),
    /// Commit the exact painted transform preview on authoritative release.
    ReleaseContainedTransform {
        /// Active transform generation.
        session: ContainedTransformSessionId,
        /// Matching pointer.
        pointer: PointerId,
        /// Matching button.
        button: PointerButton,
        /// Authoritative state of that exact button.
        button_state: Authority<PointerButtonState>,
    },
    /// Cancel an active contained transform for an explicit reason.
    CancelContainedTransform {
        /// Transform generation being cancelled.
        session: ContainedTransformSessionId,
        /// Explicit cancellation cause.
        reason: InteractionCancelReason,
    },
}

impl RendererIntent {
    /// Returns the deterministic sub-order inside renderer inputs.
    ///
    /// Paint acknowledgements are reduced before release intents from the same
    /// complete render boundary, removing viewport callback ordering from the
    /// delivery protocol.
    #[must_use]
    pub(crate) const fn reduction_rank(&self) -> u8 {
        match self {
            Self::AcknowledgePreview(_) | Self::AcknowledgeContainedTransformPreview(_) => 0,
            _ => 1,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replacement_registration_ignores_only_reprojected_rect() {
        let original = ContainedRecoveryPlan::new(
            RootId::new(1),
            FloatingPresentationId::new(2),
            SurfaceId::new(3),
            LogicalRect::new(10.0, 20.0, 300.0, 200.0).expect("original rect must be valid"),
            LogicalSize::new(80.0, 60.0).expect("minimum size must be valid"),
            4,
        );
        let reprojected = original.with_requested_rect(
            LogicalRect::new(40.0, 50.0, 320.0, 240.0).expect("reprojected rect must be valid"),
        );

        assert!(original.matches_registration(reprojected));

        let mismatches = [
            ContainedRecoveryPlan::new(
                RootId::new(9),
                original.floating(),
                original.surface(),
                original.requested_rect(),
                original.minimum_size(),
                original.z_order(),
            ),
            ContainedRecoveryPlan::new(
                original.root(),
                FloatingPresentationId::new(9),
                original.surface(),
                original.requested_rect(),
                original.minimum_size(),
                original.z_order(),
            ),
            ContainedRecoveryPlan::new(
                original.root(),
                original.floating(),
                SurfaceId::new(9),
                original.requested_rect(),
                original.minimum_size(),
                original.z_order(),
            ),
            ContainedRecoveryPlan::new(
                original.root(),
                original.floating(),
                original.surface(),
                original.requested_rect(),
                LogicalSize::new(81.0, 60.0).expect("different minimum size must be valid"),
                original.z_order(),
            ),
            ContainedRecoveryPlan::new(
                original.root(),
                original.floating(),
                original.surface(),
                original.requested_rect(),
                original.minimum_size(),
                5,
            ),
        ];

        for mismatch in mismatches {
            assert!(!original.matches_registration(mismatch));
        }
    }
}
