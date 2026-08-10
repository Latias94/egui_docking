//! Structural drop-target identities and immutable scene records.

use std::cmp::Ordering;

use crate::command::{DockTarget, Edge};
use crate::geometry::LogicalRect;
use crate::hit_region::HitRegion;
use crate::ids::{FloatingPresentationId, NodeId, RootId, SurfaceId};

/// Stable front-to-back presentation layer key validated by the core.
///
/// Larger values are frontmost. The resolver never derives this value from
/// focus, geometry, or time. Contained-presentation and surface-background
/// layers come only from the workspace's structural roster order.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(transparent)]
pub struct SceneLayerKey(u64);

impl SceneLayerKey {
    /// Creates an explicit layer key.
    #[must_use]
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    /// Returns the stable numeric representation.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }

    pub(crate) const fn surface_base() -> Self {
        Self(1)
    }

    pub(crate) fn contained(index: usize) -> Option<Self> {
        u64::try_from(index)
            .ok()
            .and_then(|index| index.checked_add(2))
            .map(Self)
    }
}

/// Typed destination representing the empty main area of one rootless surface.
///
/// This identity is authored by the core scene protocol. It is not inferred
/// from unused window geometry and does not imply whole-window pointer
/// pass-through.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SurfaceBackground {
    surface: SurfaceId,
}

impl SurfaceBackground {
    /// Creates the exact background destination for one logical surface.
    #[must_use]
    pub const fn new(surface: SurfaceId) -> Self {
        Self { surface }
    }

    /// Returns the rootless logical surface which can receive a main root.
    #[must_use]
    pub const fn surface(self) -> SurfaceId {
        self.surface
    }
}

/// Typed structural destination carried by one scene drop record.
#[derive(Debug, Clone, PartialEq)]
pub enum DropDestination {
    /// Move content into existing docking topology.
    Topology(DockTarget),
    /// Install content as the main root of an existing rootless surface.
    SurfaceBackground(SurfaceBackground),
}

/// Core-owned, unclipped outer rectangle in which one contained floating blocks lower layers.
///
/// Adapters publish the exact durable contained-floating rectangle, including
/// title bar and border chrome, even when a surface resize temporarily leaves
/// part or all of it outside the ready surface bounds. The effective hit domain
/// is this region intersected with the compiled surface bounds.
/// A target on the same layer remains eligible so the floating's own content can
/// receive drops; targets on lower layers are not candidates in that effective domain.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DropOcclusionRecord {
    floating: FloatingPresentationId,
    region: HitRegion,
    layer: SceneLayerKey,
}

impl DropOcclusionRecord {
    /// Creates one exact contained-floating occlusion fact.
    #[must_use]
    pub const fn new(
        floating: FloatingPresentationId,
        region: HitRegion,
        layer: SceneLayerKey,
    ) -> Self {
        Self {
            floating,
            region,
            layer,
        }
    }

    /// Returns the stable contained-floating identity.
    #[must_use]
    pub const fn floating(self) -> FloatingPresentationId {
        self.floating
    }

    /// Returns the exact unclipped, core-owned blocking region.
    ///
    /// Resolution intersects this region with the ready surface bounds.
    #[must_use]
    pub const fn region(self) -> HitRegion {
        self.region
    }

    /// Returns the front-to-back layer occupied by the floating.
    #[must_use]
    pub const fn layer(self) -> SceneLayerKey {
        self.layer
    }
}

/// Normative drop-target class used by deterministic resolution.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum DropTargetKind {
    /// An exact gap in a tab sequence.
    TabGap,
    /// The center of an existing tabs stack.
    Center,
    /// An edge local to one target branch.
    InnerEdge,
    /// An edge of the complete docking root.
    OuterEdge,
    /// The empty main area of an existing rootless surface.
    SurfaceBackground,
}

impl DropTargetKind {
    pub(crate) const fn resolution_priority(self) -> u8 {
        match self {
            Self::TabGap => 4,
            Self::Center => 3,
            Self::InnerEdge => 2,
            Self::OuterEdge => 1,
            Self::SurfaceBackground => 0,
        }
    }
}

/// Structural identity of a drop target in one sealed scene.
///
/// The identity names topology, not a renderer allocation or traversal slot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DropTargetId {
    /// Exact pre-removal gap in a tabs node.
    TabGap {
        /// Owning logical surface.
        surface: SurfaceId,
        /// Owning docking root.
        root: RootId,
        /// Target tabs node.
        tabs: NodeId,
        /// Gap index in the current tab sequence.
        index: usize,
    },
    /// Center merge into a tabs node.
    Center {
        /// Owning logical surface.
        surface: SurfaceId,
        /// Owning docking root.
        root: RootId,
        /// Target tabs node.
        tabs: NodeId,
    },
    /// Edge split local to a branch.
    InnerEdge {
        /// Owning logical surface.
        surface: SurfaceId,
        /// Owning docking root.
        root: RootId,
        /// Target branch node.
        node: NodeId,
        /// Physical insertion edge.
        edge: Edge,
    },
    /// Edge split at the outer boundary of a root.
    OuterEdge {
        /// Owning logical surface.
        surface: SurfaceId,
        /// Owning docking root.
        root: RootId,
        /// Target root branch node.
        node: NodeId,
        /// Physical insertion edge.
        edge: Edge,
    },
    /// Empty main area of one rootless logical surface.
    SurfaceBackground {
        /// Rootless logical surface.
        surface: SurfaceId,
    },
}

impl DropTargetId {
    /// Returns the target class.
    #[must_use]
    pub const fn kind(self) -> DropTargetKind {
        match self {
            Self::TabGap { .. } => DropTargetKind::TabGap,
            Self::Center { .. } => DropTargetKind::Center,
            Self::InnerEdge { .. } => DropTargetKind::InnerEdge,
            Self::OuterEdge { .. } => DropTargetKind::OuterEdge,
            Self::SurfaceBackground { .. } => DropTargetKind::SurfaceBackground,
        }
    }

    /// Returns the surface in whose logical coordinates this target is defined.
    #[must_use]
    pub const fn surface(self) -> SurfaceId {
        match self {
            Self::TabGap { surface, .. }
            | Self::Center { surface, .. }
            | Self::InnerEdge { surface, .. }
            | Self::OuterEdge { surface, .. }
            | Self::SurfaceBackground { surface } => surface,
        }
    }

    fn variant_order(self) -> u8 {
        match self {
            Self::TabGap { .. } => 0,
            Self::Center { .. } => 1,
            Self::InnerEdge { .. } => 2,
            Self::OuterEdge { .. } => 3,
            Self::SurfaceBackground { .. } => 4,
        }
    }
}

impl PartialOrd for DropTargetId {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for DropTargetId {
    fn cmp(&self, other: &Self) -> Ordering {
        self.variant_order()
            .cmp(&other.variant_order())
            .then_with(|| match (*self, *other) {
                (
                    Self::TabGap {
                        surface: left_surface,
                        root: left_root,
                        tabs: left_tabs,
                        index: left_index,
                    },
                    Self::TabGap {
                        surface: right_surface,
                        root: right_root,
                        tabs: right_tabs,
                        index: right_index,
                    },
                ) => left_surface
                    .cmp(&right_surface)
                    .then(left_root.cmp(&right_root))
                    .then(left_tabs.cmp(&right_tabs))
                    .then(left_index.cmp(&right_index)),
                (
                    Self::Center {
                        surface: left_surface,
                        root: left_root,
                        tabs: left_tabs,
                    },
                    Self::Center {
                        surface: right_surface,
                        root: right_root,
                        tabs: right_tabs,
                    },
                ) => left_surface
                    .cmp(&right_surface)
                    .then(left_root.cmp(&right_root))
                    .then(left_tabs.cmp(&right_tabs)),
                (
                    Self::InnerEdge {
                        surface: left_surface,
                        root: left_root,
                        node: left_node,
                        edge: left_edge,
                    },
                    Self::InnerEdge {
                        surface: right_surface,
                        root: right_root,
                        node: right_node,
                        edge: right_edge,
                    },
                )
                | (
                    Self::OuterEdge {
                        surface: left_surface,
                        root: left_root,
                        node: left_node,
                        edge: left_edge,
                    },
                    Self::OuterEdge {
                        surface: right_surface,
                        root: right_root,
                        node: right_node,
                        edge: right_edge,
                    },
                ) => left_surface
                    .cmp(&right_surface)
                    .then(left_root.cmp(&right_root))
                    .then(left_node.cmp(&right_node))
                    .then(edge_order(left_edge).cmp(&edge_order(right_edge))),
                (
                    Self::SurfaceBackground {
                        surface: left_surface,
                    },
                    Self::SurfaceBackground {
                        surface: right_surface,
                    },
                ) => left_surface.cmp(&right_surface),
                _ => Ordering::Equal,
            })
    }
}

const fn edge_order(edge: Edge) -> u8 {
    match edge {
        Edge::Left => 0,
        Edge::Right => 1,
        Edge::Top => 2,
        Edge::Bottom => 3,
    }
}

/// Explicit reason a target was published as unavailable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DropTargetUnavailable {
    /// The adapter knows the target was derived from stale semantic state.
    Stale,
    /// Application policy excluded this target when the scene was built.
    PolicyDisabled,
    /// The renderer cannot offer this semantic target in the current scene.
    AdapterUnavailable,
}

/// Explicit scene-time availability of one target.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DropTargetAvailability {
    /// The target may proceed to policy and workspace prevalidation.
    Available,
    /// The target remains in the scene for deterministic rejection reporting.
    Unavailable(DropTargetUnavailable),
}

impl DropTargetAvailability {
    /// Returns whether this target may proceed to resolver checks.
    #[must_use]
    pub const fn is_available(self) -> bool {
        matches!(self, Self::Available)
    }
}

/// Exact preview geometry attached to a structural target.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DropVisual {
    rect: LogicalRect,
}

impl DropVisual {
    /// Creates preview geometry without deriving it from the hit region.
    #[must_use]
    pub const fn new(rect: LogicalRect) -> Self {
        Self { rect }
    }

    /// Returns the exact preview rectangle.
    #[must_use]
    pub const fn rect(self) -> LogicalRect {
        self.rect
    }
}

/// One immutable drop target published by a ready surface fact.
#[derive(Debug, Clone, PartialEq)]
pub struct DropTargetRecord {
    id: DropTargetId,
    destination: DropDestination,
    availability: DropTargetAvailability,
    region: HitRegion,
    layer: SceneLayerKey,
    visual: DropVisual,
}

impl DropTargetRecord {
    /// Creates an explicit target record.
    ///
    /// Structural agreement between `id` and `target` is checked only when the
    /// complete building scene is sealed.
    #[must_use]
    pub const fn new(
        id: DropTargetId,
        target: DockTarget,
        availability: DropTargetAvailability,
        region: HitRegion,
        layer: SceneLayerKey,
        visual: DropVisual,
    ) -> Self {
        Self {
            id,
            destination: DropDestination::Topology(target),
            availability,
            region,
            layer,
            visual,
        }
    }

    /// Creates the one explicit rootless-surface background record.
    ///
    /// Its layer is core-owned and always below every contained presentation.
    /// Its availability is also core-owned and always starts available. The
    /// adapter supplies only measured hit and preview geometry.
    #[must_use]
    pub const fn surface_background(
        destination: SurfaceBackground,
        region: HitRegion,
        visual: DropVisual,
    ) -> Self {
        Self {
            id: DropTargetId::SurfaceBackground {
                surface: destination.surface(),
            },
            destination: DropDestination::SurfaceBackground(destination),
            availability: DropTargetAvailability::Available,
            region,
            layer: SceneLayerKey::surface_base(),
            visual,
        }
    }

    /// Returns the structural target identity.
    #[must_use]
    pub const fn id(&self) -> DropTargetId {
        self.id
    }

    /// Returns the exact typed destination captured for this scene.
    #[must_use]
    pub const fn destination(&self) -> &DropDestination {
        &self.destination
    }

    /// Returns explicit scene-time availability.
    #[must_use]
    pub const fn availability(&self) -> DropTargetAvailability {
        self.availability
    }

    /// Returns the exact half-open hit region.
    #[must_use]
    pub const fn region(&self) -> HitRegion {
        self.region
    }

    /// Returns the explicit front-to-back layer.
    #[must_use]
    pub const fn layer(&self) -> SceneLayerKey {
        self.layer
    }

    /// Returns exact renderer preview geometry.
    #[must_use]
    pub const fn visual(&self) -> DropVisual {
        self.visual
    }

    pub(crate) fn semantics_match(&self) -> bool {
        match (&self.id, &self.destination) {
            (
                DropTargetId::TabGap {
                    root, tabs, index, ..
                },
                DropDestination::Topology(DockTarget::TabGap {
                    target,
                    index: target_index,
                }),
            ) => *root == target.root() && *tabs == target.tabs() && *index == *target_index,
            (
                DropTargetId::Center { root, tabs, .. },
                DropDestination::Topology(DockTarget::Center(target)),
            ) => *root == target.root() && *tabs == target.tabs(),
            (
                DropTargetId::InnerEdge {
                    root, node, edge, ..
                },
                DropDestination::Topology(DockTarget::InnerEdge(target)),
            ) => *root == target.root() && *node == target.node() && *edge == target.edge(),
            (
                DropTargetId::OuterEdge {
                    root, node, edge, ..
                },
                DropDestination::Topology(DockTarget::OuterEdge(target)),
            ) => *root == target.root() && *node == target.node() && *edge == target.edge(),
            (
                DropTargetId::SurfaceBackground { surface },
                DropDestination::SurfaceBackground(destination),
            ) => *surface == destination.surface(),
            _ => false,
        }
    }

    pub(crate) fn set_availability(&mut self, availability: DropTargetAvailability) {
        self.availability = availability;
    }
}
