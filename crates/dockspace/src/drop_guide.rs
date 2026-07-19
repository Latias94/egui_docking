//! Explicit docking-guide clusters and their renderer geometry.

use crate::command::Edge;
use crate::drop_target::{DropTargetId, DropTargetRecord, SceneLayerKey};
use crate::geometry::LogicalRect;
use crate::hit_region::HitRegion;
use crate::ids::{NodeId, RootId, SurfaceId};

/// Structural scope of one docking-guide cluster.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum DropGuideScope {
    /// Guide cluster local to one tabs node.
    Inner(NodeId),
    /// Guide cluster at the boundary of the complete docking root.
    Outer,
}

/// Stable structural identity of one docking-guide cluster.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DropGuideClusterId {
    /// Owning logical surface.
    pub surface: SurfaceId,
    /// Owning docking root.
    pub root: RootId,
    /// Structural scope within the root.
    pub scope: DropGuideScope,
}

impl DropGuideClusterId {
    /// Creates an inner-cluster identity local to `node`.
    #[must_use]
    pub const fn inner(surface: SurfaceId, root: RootId, node: NodeId) -> Self {
        Self {
            surface,
            root,
            scope: DropGuideScope::Inner(node),
        }
    }

    /// Creates an outer-cluster identity for a complete root.
    #[must_use]
    pub const fn outer(surface: SurfaceId, root: RootId) -> Self {
        Self {
            surface,
            root,
            scope: DropGuideScope::Outer,
        }
    }
}

/// Stable slot within one docking-guide cluster.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum DropGuideSlot {
    /// Merge into the scoped tabs stack.
    Center,
    /// Split at one physical edge.
    Edge(Edge),
}

/// One guide-owned drop target with independent button geometry.
#[derive(Debug, Clone, PartialEq)]
pub struct DropGuideTargetRecord {
    target: DropTargetRecord,
    draw: LogicalRect,
}

impl DropGuideTargetRecord {
    /// Creates one guide target from exact hit, draw, and preview facts.
    ///
    /// The owned target's region is the exact hit area and its visual is the
    /// exact body preview. Scene sealing validates all geometry and semantics.
    #[must_use]
    pub const fn new(target: DropTargetRecord, draw: LogicalRect) -> Self {
        Self { target, draw }
    }

    /// Returns the owned structural target record.
    #[must_use]
    pub const fn target(&self) -> &DropTargetRecord {
        &self.target
    }

    /// Returns the structural target identity.
    #[must_use]
    pub const fn id(&self) -> DropTargetId {
        self.target.id()
    }

    /// Returns the exact passive or active guide-button rectangle.
    #[must_use]
    pub const fn draw(&self) -> LogicalRect {
        self.draw
    }

    pub(crate) fn target_mut(&mut self) -> &mut DropTargetRecord {
        &mut self.target
    }
}

/// Complete four-direction target set for a docking-guide cluster.
///
/// Named constructor arguments and private fields make an incomplete edge set
/// unrepresentable. Scene sealing additionally verifies that each record names
/// the direction of the field that owns it.
#[derive(Debug, Clone, PartialEq)]
pub struct DropGuideEdgeSet {
    left: DropGuideTargetRecord,
    right: DropGuideTargetRecord,
    top: DropGuideTargetRecord,
    bottom: DropGuideTargetRecord,
}

impl DropGuideEdgeSet {
    /// Creates a complete left, right, top, and bottom edge set.
    #[must_use]
    pub const fn new(
        left: DropGuideTargetRecord,
        right: DropGuideTargetRecord,
        top: DropGuideTargetRecord,
        bottom: DropGuideTargetRecord,
    ) -> Self {
        Self {
            left,
            right,
            top,
            bottom,
        }
    }

    /// Returns the target occupying one physical edge slot.
    #[must_use]
    pub const fn target(&self, edge: Edge) -> &DropGuideTargetRecord {
        match edge {
            Edge::Left => &self.left,
            Edge::Right => &self.right,
            Edge::Top => &self.top,
            Edge::Bottom => &self.bottom,
        }
    }

    /// Iterates every edge target in canonical physical order.
    pub fn targets(
        &self,
    ) -> impl ExactSizeIterator<Item = (Edge, &DropGuideTargetRecord)> + DoubleEndedIterator {
        [
            (Edge::Left, &self.left),
            (Edge::Right, &self.right),
            (Edge::Top, &self.top),
            (Edge::Bottom, &self.bottom),
        ]
        .into_iter()
    }

    fn for_each_target_mut(&mut self, mut visit: impl FnMut(Edge, &mut DropGuideTargetRecord)) {
        visit(Edge::Left, &mut self.left);
        visit(Edge::Right, &mut self.right);
        visit(Edge::Top, &mut self.top);
        visit(Edge::Bottom, &mut self.bottom);
    }
}

/// Complete guide cluster with an exact activation region and paint layer.
///
/// The enum shape makes inner clusters contain center plus four edges, while
/// outer clusters contain exactly four edges. Callers cannot construct a
/// partially populated cluster.
#[derive(Debug, Clone, PartialEq)]
pub struct DropGuideClusterRecord {
    id: DropGuideClusterId,
    activation: HitRegion,
    layer: SceneLayerKey,
    targets: DropGuideClusterTargets,
}

#[derive(Debug, Clone, PartialEq)]
enum DropGuideClusterTargets {
    Inner {
        center: DropGuideTargetRecord,
        edges: DropGuideEdgeSet,
    },
    Outer {
        edges: DropGuideEdgeSet,
    },
}

impl DropGuideClusterRecord {
    /// Creates a complete inner guide cluster local to `node`.
    #[must_use]
    pub const fn inner(
        surface: SurfaceId,
        root: RootId,
        node: NodeId,
        activation: HitRegion,
        layer: SceneLayerKey,
        center: DropGuideTargetRecord,
        edges: DropGuideEdgeSet,
    ) -> Self {
        Self {
            id: DropGuideClusterId::inner(surface, root, node),
            activation,
            layer,
            targets: DropGuideClusterTargets::Inner { center, edges },
        }
    }

    /// Creates a complete outer guide cluster for one docking root.
    #[must_use]
    pub const fn outer(
        surface: SurfaceId,
        root: RootId,
        activation: HitRegion,
        layer: SceneLayerKey,
        edges: DropGuideEdgeSet,
    ) -> Self {
        Self {
            id: DropGuideClusterId::outer(surface, root),
            activation,
            layer,
            targets: DropGuideClusterTargets::Outer { edges },
        }
    }

    /// Returns the stable structural cluster identity.
    #[must_use]
    pub const fn id(&self) -> DropGuideClusterId {
        self.id
    }

    /// Returns the exact region that activates this guide cluster.
    #[must_use]
    pub const fn activation(&self) -> HitRegion {
        self.activation
    }

    /// Returns the explicit front-to-back paint and hit-test layer.
    #[must_use]
    pub const fn layer(&self) -> SceneLayerKey {
        self.layer
    }

    /// Returns the complete four-direction edge set.
    #[must_use]
    pub const fn edges(&self) -> &DropGuideEdgeSet {
        match &self.targets {
            DropGuideClusterTargets::Inner { edges, .. }
            | DropGuideClusterTargets::Outer { edges } => edges,
        }
    }

    /// Returns the target occupying `slot`, if that slot exists in this cluster.
    #[must_use]
    pub const fn target(&self, slot: DropGuideSlot) -> Option<&DropGuideTargetRecord> {
        match (&self.targets, slot) {
            (DropGuideClusterTargets::Inner { center, .. }, DropGuideSlot::Center) => Some(center),
            (DropGuideClusterTargets::Outer { .. }, DropGuideSlot::Center) => None,
            (
                DropGuideClusterTargets::Inner { edges, .. }
                | DropGuideClusterTargets::Outer { edges },
                DropGuideSlot::Edge(edge),
            ) => Some(edges.target(edge)),
        }
    }

    /// Iterates every guide target in canonical slot order.
    pub fn targets(
        &self,
    ) -> impl DoubleEndedIterator<Item = (DropGuideSlot, &DropGuideTargetRecord)> {
        let center = match &self.targets {
            DropGuideClusterTargets::Inner { center, .. } => Some(center),
            DropGuideClusterTargets::Outer { .. } => None,
        };
        center
            .into_iter()
            .map(|target| (DropGuideSlot::Center, target))
            .chain(
                self.edges()
                    .targets()
                    .map(|(edge, target)| (DropGuideSlot::Edge(edge), target)),
            )
    }

    pub(crate) fn for_each_target_mut(
        &mut self,
        mut visit: impl FnMut(DropGuideSlot, &mut DropGuideTargetRecord),
    ) {
        match &mut self.targets {
            DropGuideClusterTargets::Inner { center, edges } => {
                visit(DropGuideSlot::Center, center);
                edges.for_each_target_mut(|edge, target| {
                    visit(DropGuideSlot::Edge(edge), target);
                });
            }
            DropGuideClusterTargets::Outer { edges } => {
                edges.for_each_target_mut(|edge, target| {
                    visit(DropGuideSlot::Edge(edge), target);
                });
            }
        }
    }
}
