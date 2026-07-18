//! Renderer-originated semantic interaction inputs.
//!
//! Adapters are responsible for turning framework callbacks into these values.
//! The core never derives button releases, hovered surfaces, drag thresholds, or
//! presentation fallback from timing or geometry history.

use crate::command::{MovePayload, NodeSource};
use crate::coordinates::ViewportPlacementProof;
use crate::geometry::{LogicalPoint, LogicalRect, PhysicalRect};
use crate::graph::SplitWeight;
use crate::ids::{FloatingPresentationId, RootId, SurfaceId};
use crate::interaction::{
    DragSessionId, InteractionCancelReason, PaintAcknowledgement, ResizeSessionId,
};
use crate::viewport_route::ViewportRouteProof;

/// Authority attached to a provider observation.
#[derive(Debug, Clone, PartialEq, Eq)]
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

/// Exact proposal for a contained-floating destination.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ContainedTearOffProposal {
    surface: SurfaceId,
    root: RootId,
    floating: FloatingPresentationId,
    rect: LogicalRect,
    z_order: u64,
}

impl ContainedTearOffProposal {
    /// Creates an explicit contained-floating proposal.
    #[must_use]
    pub const fn new(
        surface: SurfaceId,
        root: RootId,
        floating: FloatingPresentationId,
        rect: LogicalRect,
        z_order: u64,
    ) -> Self {
        Self {
            surface,
            root,
            floating,
            rect,
            z_order,
        }
    }

    /// Returns the host logical surface.
    #[must_use]
    pub const fn surface(self) -> SurfaceId {
        self.surface
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
        self.rect
    }

    /// Returns the explicit contained stacking order.
    #[must_use]
    pub const fn z_order(self) -> u64 {
        self.z_order
    }
}

/// Exact proposal for a future native-surface lifecycle saga.
#[derive(Debug, Clone, PartialEq)]
pub struct NativeTearOffProposal {
    surface: SurfaceId,
    root: RootId,
    placement: ViewportPlacementProof,
    recovery: ContainedTearOffProposal,
}

impl NativeTearOffProposal {
    /// Creates an explicit native-surface proposal with authoritative placement.
    #[must_use]
    pub const fn new(
        surface: SurfaceId,
        root: RootId,
        placement: ViewportPlacementProof,
        recovery: ContainedTearOffProposal,
    ) -> Self {
        Self {
            surface,
            root,
            placement,
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
    pub const fn placement(&self) -> &ViewportPlacementProof {
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

/// Explicit tear-off mode and all facts required to preview it.
#[derive(Debug, Clone, PartialEq)]
pub enum TearOffRequest {
    /// Request an immediate contained-floating command.
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
        /// Explicit tear-off request used only with `Known(None)`.
        tear_off: Option<TearOffRequest>,
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
        /// Exact tear-off request painted for a known-none target.
        tear_off: Option<TearOffRequest>,
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
            Self::AcknowledgePreview(_) => 0,
            _ => 1,
        }
    }
}
