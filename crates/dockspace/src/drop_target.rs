//! Structural drop-target identities and immutable scene records.

use std::cmp::Ordering;

use crate::command::{DockFraction, Edge, MovePayload};
use crate::drop_guide::{DropGuideClusterId, DropGuideScope};
use crate::geometry::LogicalRect;
use crate::graph::{Node, Workspace};
use crate::hit_region::HitRegion;
use crate::ids::{FloatingPresentationId, NodeId, RootId, SurfaceId};

/// Shared exclusion of source topology removed by a drag from destinations and hits.
/// A disappearing leaf retains its root's outer targets; central leaves remain targets.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct DropSourceSuppression {
    root: Option<RootId>,
    floating: Option<FloatingPresentationId>,
    leaf: Option<(RootId, NodeId)>,
}

impl DropSourceSuppression {
    pub(crate) const fn complete_root(
        root: RootId,
        floating: Option<FloatingPresentationId>,
    ) -> Self {
        Self {
            root: Some(root),
            floating,
            leaf: None,
        }
    }

    /// Derives geometry exclusions from an already captured payload and complete-root fact.
    /// This does not recapture source authority or walk a split payload's descendants.
    pub(crate) fn from_source(
        workspace: &Workspace,
        payload: &MovePayload,
        complete_root: Option<RootId>,
    ) -> Self {
        if let Some(root) = complete_root {
            return Self::from_complete_root(workspace, root).unwrap_or_default();
        }
        let (root, node) = match payload {
            MovePayload::Item(source) => (source.root(), source.tabs()),
            MovePayload::Tabs(source) | MovePayload::Subtree(source) => {
                (source.root(), source.node())
            }
        };
        let Some(record) = workspace.root(root) else {
            return Self::default();
        };
        if record.central == Some(node) {
            return Self::default();
        }
        let Some(Node::Tabs { items, .. }) = workspace.node(node) else {
            return Self::default();
        };
        let removes_leaf = match payload {
            MovePayload::Item(source) => items.as_slice() == [source.item()],
            MovePayload::Tabs(_) | MovePayload::Subtree(_) => !items.is_empty(),
        };
        Self {
            leaf: removes_leaf.then_some((root, node)),
            ..Self::default()
        }
    }

    pub(crate) fn from_complete_root(workspace: &Workspace, root: RootId) -> Option<Self> {
        let floating = match workspace.presentation_for_root(root)? {
            crate::RootPresentationOwner::Main { .. } => None,
            crate::RootPresentationOwner::Contained { floating, .. } => Some(floating),
        };
        Some(Self::complete_root(root, floating))
    }

    pub(crate) fn excludes_root(self, root: RootId) -> bool {
        self.root == Some(root)
    }

    pub(crate) fn excludes_cluster(self, cluster: DropGuideClusterId) -> bool {
        self.excludes_root(cluster.root)
            || matches!(cluster.scope, DropGuideScope::Inner(node)
                if self.leaf == Some((cluster.root, node)))
    }

    pub(crate) fn excludes_target(self, target: DropTargetId) -> bool {
        target.root().is_some_and(|root| self.excludes_root(root))
            || match target {
                DropTargetId::Center { root, tabs, .. }
                | DropTargetId::TabGap { root, tabs, .. } => self.leaf == Some((root, tabs)),
                DropTargetId::InnerEdge { root, node, .. } => self.leaf == Some((root, node)),
                DropTargetId::OuterEdge { .. } | DropTargetId::SurfaceBackground { .. } => false,
            }
    }

    pub(crate) fn excludes_occlusion(self, floating: FloatingPresentationId) -> bool {
        self.floating == Some(floating)
    }
}

#[cfg(test)]
#[path = "drop_target/ordering_tests.rs"]
mod ordering_tests;

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

/// Structural location and insertion geometry of one drop target.
///
/// Locations carry no command proof. The owning presentation plan binds the
/// root source used to resolve a location against current workspace authority.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DropTargetLocation {
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
    /// Edge split local to a tabs leaf.
    InnerEdge {
        /// Owning logical surface.
        surface: SurfaceId,
        /// Owning docking root.
        root: RootId,
        /// Target tabs leaf.
        node: NodeId,
        /// Physical insertion edge.
        edge: Edge,
        /// Share allocated to the inserted payload.
        fraction: DockFraction,
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
        /// Share allocated to the inserted payload.
        fraction: DockFraction,
    },
    /// Empty main area of one rootless logical surface.
    SurfaceBackground {
        /// Rootless logical surface.
        surface: SurfaceId,
    },
}

impl DropTargetLocation {
    /// Returns the stable structural identity derived from this location.
    #[must_use]
    pub const fn id(self) -> DropTargetId {
        match self {
            Self::TabGap {
                surface,
                root,
                tabs,
                index,
            } => DropTargetId::TabGap {
                surface,
                root,
                tabs,
                index,
            },
            Self::Center {
                surface,
                root,
                tabs,
            } => DropTargetId::Center {
                surface,
                root,
                tabs,
            },
            Self::InnerEdge {
                surface,
                root,
                node,
                edge,
                ..
            } => DropTargetId::InnerEdge {
                surface,
                root,
                node,
                edge,
            },
            Self::OuterEdge {
                surface,
                root,
                node,
                edge,
                ..
            } => DropTargetId::OuterEdge {
                surface,
                root,
                node,
                edge,
            },
            Self::SurfaceBackground { surface } => DropTargetId::SurfaceBackground { surface },
        }
    }

    /// Returns the surface whose logical coordinates define this location.
    #[must_use]
    pub const fn surface(self) -> SurfaceId {
        self.id().surface()
    }

    /// Returns the owning root, or `None` for a rootless surface background.
    #[must_use]
    pub const fn root(self) -> Option<RootId> {
        match self {
            Self::TabGap { root, .. }
            | Self::Center { root, .. }
            | Self::InnerEdge { root, .. }
            | Self::OuterEdge { root, .. } => Some(root),
            Self::SurfaceBackground { .. } => None,
        }
    }
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
    pub(crate) const fn root(self) -> Option<RootId> {
        match self {
            Self::TabGap { root, .. }
            | Self::Center { root, .. }
            | Self::InnerEdge { root, .. }
            | Self::OuterEdge { root, .. } => Some(root),
            Self::SurfaceBackground { .. } => None,
        }
    }

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

    fn variant_order(&self) -> u8 {
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
        match (self, other) {
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
                .cmp(right_surface)
                .then_with(|| left_root.cmp(right_root))
                .then_with(|| left_tabs.cmp(right_tabs))
                .then_with(|| left_index.cmp(right_index)),
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
                .cmp(right_surface)
                .then_with(|| left_root.cmp(right_root))
                .then_with(|| left_tabs.cmp(right_tabs)),
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
                .cmp(right_surface)
                .then_with(|| left_root.cmp(right_root))
                .then_with(|| left_node.cmp(right_node))
                .then_with(|| edge_order(*left_edge).cmp(&edge_order(*right_edge))),
            (
                Self::SurfaceBackground {
                    surface: left_surface,
                },
                Self::SurfaceBackground {
                    surface: right_surface,
                },
            ) => left_surface.cmp(right_surface),
            _ => self.variant_order().cmp(&other.variant_order()),
        }
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
    #[allow(
        dead_code,
        reason = "Retained internal protocol case is matched by the shared reducer and exercised by protocol tests"
    )]
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
    location: DropTargetLocation,
    availability: DropTargetAvailability,
    region: HitRegion,
    layer: SceneLayerKey,
    visual: DropVisual,
}

impl DropTargetRecord {
    /// Creates an explicit target record.
    ///
    /// Structural membership and geometric coverage are checked when the
    /// complete building scene is sealed.
    #[must_use]
    pub const fn new(
        location: DropTargetLocation,
        availability: DropTargetAvailability,
        region: HitRegion,
        layer: SceneLayerKey,
        visual: DropVisual,
    ) -> Self {
        Self {
            location,
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
            location: DropTargetLocation::SurfaceBackground {
                surface: destination.surface(),
            },
            availability: DropTargetAvailability::Available,
            region,
            layer: SceneLayerKey::surface_base(),
            visual,
        }
    }

    /// Returns the structural target identity.
    #[must_use]
    pub const fn id(&self) -> DropTargetId {
        self.location.id()
    }

    /// Returns the structural location without command authority.
    #[must_use]
    pub const fn location(&self) -> DropTargetLocation {
        self.location
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

    pub(crate) fn set_availability(&mut self, availability: DropTargetAvailability) {
        self.availability = availability;
    }
}
