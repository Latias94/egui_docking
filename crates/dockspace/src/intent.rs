//! Renderer-originated semantic interaction inputs.
//!
//! Adapters are responsible for turning framework callbacks into these values.
//! The core never derives button releases, hovered surfaces, drag thresholds, or
//! presentation fallback from timing or geometry history.

use crate::command::{MovePayload, NodeSource};
use crate::geometry::{LogicalPoint, LogicalRect, PhysicalRect};
use crate::graph::SplitWeight;
use crate::ids::{FloatingPresentationId, RootId, SurfaceId};
use crate::interaction::{
    DragSessionId, InteractionCancelReason, PaintAcknowledgement, ResizeSessionId,
};

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
    pub const fn surface(self) -> SurfaceId {
        self.surface
    }

    /// Returns the point in that target's logical coordinate space.
    #[must_use]
    pub const fn position(self) -> LogicalPoint {
        self.position
    }
}

/// Authoritative hovered-target observation.
///
/// `Known(None)` means that the provider proved there is no dock target surface
/// under the pointer. It is intentionally distinct from `Unknown`.
pub type TargetAuthority = Authority<Option<SurfacePointer>>;

/// Capability required to prepare a native-surface tear-off.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NativeTearOffCapability {
    /// The provider can execute the required native lifecycle protocol.
    Supported,
    /// The provider authoritatively does not support the operation.
    Unsupported(NativeTearOffUnavailableReason),
    /// Support cannot be established for the current boundary.
    Unknown(NativeTearOffUnavailableReason),
}

/// Why native tear-off cannot currently be prepared.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NativeTearOffUnavailableReason {
    /// Native child surfaces are unsupported by the backend.
    BackendUnsupported,
    /// Authoritative desktop placement is unavailable.
    PlacementUnavailable,
    /// Authoritative cross-surface routing is unavailable.
    RoutingUnavailable,
    /// The relevant native surface is closing or unavailable.
    SurfaceUnavailable,
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
    pub const fn root(self) -> RootId {
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
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NativeTearOffProposal {
    surface: SurfaceId,
    root: RootId,
    placement: PhysicalRect,
}

impl NativeTearOffProposal {
    /// Creates an explicit native-surface proposal with authoritative placement.
    #[must_use]
    pub const fn new(surface: SurfaceId, root: RootId, placement: PhysicalRect) -> Self {
        Self {
            surface,
            root,
            placement,
        }
    }

    /// Returns the logical surface identity to create.
    #[must_use]
    pub const fn surface(self) -> SurfaceId {
        self.surface
    }

    /// Returns the root identity used for newly detached content.
    #[must_use]
    pub const fn root(self) -> RootId {
        self.root
    }

    /// Returns the authoritative desktop-physical placement.
    #[must_use]
    pub const fn placement(self) -> PhysicalRect {
        self.placement
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
        proposal: NativeTearOffProposal,
        /// Current provider capability.
        capability: NativeTearOffCapability,
        /// Optional contained proposal used only when policy enables fallback.
        contained_fallback: Option<ContainedTearOffProposal>,
    },
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
