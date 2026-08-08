//! Renderer-originated semantic interaction inputs.
//!
//! Adapters are responsible for turning framework callbacks into these values.
//! The core never derives button releases, hovered surfaces, drag thresholds, or
//! presentation fallback from timing or geometry history.

use crate::command::ContainedPosition;
use crate::coordinates::{TearOffPlacementProof, ViewportPlacementProof};
use crate::geometry::{LogicalPoint, LogicalRect, LogicalSize, PhysicalRect};
use crate::ids::{FloatingPresentationId, RootId, SurfaceId};
use crate::scene::{ContainedResizeDirection, SurfaceSceneStamp, TabBarSceneId, TabSceneId};
use crate::surface_recovery::ConvertedMainRecovery;

/// Authority attached to a provider observation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
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

/// Exact contained-floating chrome gesture authorized by one painted scene.
///
/// The adapter reports only the semantic chrome region it observed. The core
/// derives the root, payload, durable rectangle, minimum size, and structural
/// roster from the exact painted scene and current workspace.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ContainedGestureKind {
    /// Press the whole-root title drag region and arm a docking drag.
    TitleDrag,
    /// Press one exact directional resize region and begin a transform.
    Resize(ContainedResizeDirection),
}

/// Exact painted tab source claimed by a renderer gesture.
///
/// The identity carries no workspace snapshot or payload. The core resolves
/// those facts from the matching last-painted scene before arming the drag.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TabGestureSource {
    /// Drag one visible item from its exact painted tab record.
    Item(TabSceneId),
    /// Drag the whole exact painted tab stack from its grip region.
    Group(TabBarSceneId),
}

/// Exact painted close control claimed by a renderer activation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum CloseSceneTarget {
    /// Close one item through its exact painted tab control.
    Tab(TabSceneId),
    /// Close every item in one exact contained-root presentation.
    Contained(FloatingPresentationId),
}

/// Device-independent activation fact for one painted close control.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CloseActivation {
    /// A concrete pointer and button activated the control at this position.
    Pointer {
        pointer: PointerId,
        button: PointerButton,
        position: SurfacePointer,
    },
    /// Keyboard or accessibility activation targeted the control's stable identity.
    Semantic,
    /// Current-frame framework response targeted the control in the exact Ready candidate.
    LocalResponse,
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
    position: ContainedPosition,
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
            position: ContainedPosition::Front,
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

    /// Returns the core-owned structural roster position for this offer.
    #[must_use]
    pub const fn position(self) -> ContainedPosition {
        self.position
    }
}

/// Application-owned identity offer for a new main root on a rootless surface.
///
/// The value is only a proposal until the core proves that the root identity is
/// fresh. The drag state freezes the first supplied offer, so preview and
/// release cannot silently allocate or substitute a different root.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SurfaceBackgroundRootOffer {
    root: RootId,
}

impl SurfaceBackgroundRootOffer {
    /// Offers one stable root identity for partial content delivered to a surface background.
    #[must_use]
    pub const fn new(root: RootId) -> Self {
        Self { root }
    }

    /// Returns the proposed fresh root identity.
    #[must_use]
    pub const fn root(self) -> RootId {
        self.root
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
        expected: SurfaceSceneStamp,
        /// Currently published scene, when one exists.
        current: Option<SurfaceSceneStamp>,
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
    /// A projection exists but its prepared candidate was not confirmed painted.
    #[error("surface {surface} projection has no painted interaction authority")]
    PendingPaintSurface {
        /// Surface without painted hit authority.
        surface: SurfaceId,
    },
    /// The surface retains a paint fallback but has no current input authority.
    #[error("surface {surface} has stale presentation authority")]
    StaleSurface {
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
    scene: SurfaceSceneStamp,
    surface: SurfaceId,
    requested_rect: LogicalRect,
    minimum_size: LogicalSize,
    surface_bounds: LogicalRect,
    clamped_rect: LogicalRect,
}

impl ContainedPlacementProof {
    pub(crate) const fn new(
        scene: SurfaceSceneStamp,
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
    pub const fn scene(self) -> SurfaceSceneStamp {
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
    position: ContainedPosition,
}

impl ContainedTearOffProposal {
    /// Creates an explicit contained-floating proposal.
    #[must_use]
    pub const fn new(
        root: RootId,
        floating: FloatingPresentationId,
        placement: ContainedPlacementProof,
        position: ContainedPosition,
    ) -> Self {
        Self {
            root,
            floating,
            placement,
            position,
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

    /// Returns the structural position in the destination contained roster.
    #[must_use]
    pub const fn position(self) -> ContainedPosition {
        self.position
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
    converted_main: ConvertedMainRecovery,
}

/// Why a native proposal does not form one exact lifecycle contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum NativeTearOffProposalError {
    /// The recovery reservation names a different source root.
    #[error("native root {root:?} does not match recovery root {recovery_root:?}")]
    RecoveryRootMismatch { root: RootId, recovery_root: RootId },
}

impl NativeTearOffProposal {
    /// Creates an explicit native-surface proposal with authoritative placement.
    pub fn new(
        surface: SurfaceId,
        root: RootId,
        placement: impl Into<NativePlacementProof>,
        converted_main: ConvertedMainRecovery,
    ) -> Result<Self, NativeTearOffProposalError> {
        if converted_main.source_root() != root {
            return Err(NativeTearOffProposalError::RecoveryRootMismatch {
                root,
                recovery_root: converted_main.source_root(),
            });
        }
        Ok(Self {
            surface,
            root,
            placement: placement.into(),
            converted_main,
        })
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

    /// Returns the exact lifecycle recovery target reserved for this surface.
    #[must_use]
    pub const fn converted_main(&self) -> ConvertedMainRecovery {
        self.converted_main
    }
}

/// Application-owned identities and exact placement for native presentation.
///
/// The first offer is frozen for the drag session. Later observations may omit
/// it, but an explicit replacement must be exactly equal. Native presentation
/// is considered only when a routed observation proves the pointer is outside
/// every surface. The contained fallback is used only when policy explicitly
/// permits fallback from an authoritatively unsupported native capability.
#[derive(Debug, Clone, PartialEq)]
pub struct NativePresentationOffer {
    proposal: Box<NativeTearOffProposal>,
    contained_fallback: Option<ContainedTearOffProposal>,
}

impl NativePresentationOffer {
    /// Creates an exact native presentation offer and optional explicit fallback.
    #[must_use]
    pub fn new(
        proposal: NativeTearOffProposal,
        contained_fallback: Option<ContainedTearOffProposal>,
    ) -> Self {
        Self {
            proposal: Box::new(proposal),
            contained_fallback,
        }
    }

    /// Returns the exact native destination and placement proposal.
    #[must_use]
    pub const fn proposal(&self) -> &NativeTearOffProposal {
        &self.proposal
    }

    /// Returns the explicit contained fallback, when one was offered.
    #[must_use]
    pub const fn contained_fallback(&self) -> Option<ContainedTearOffProposal> {
        self.contained_fallback
    }
}
