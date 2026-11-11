//! Core-owned magnetic docking acquisition fields.

use crate::command::Edge;
use crate::drop_target::{DropTargetId, SceneLayerKey};
use crate::geometry::{GeometryError, LogicalPoint, LogicalRect};
use crate::ids::{NodeId, RootId};
use crate::presentation_config::DockPresentationConfig;

const EDGE_ORDER: [Edge; 4] = [Edge::Left, Edge::Right, Edge::Top, Edge::Bottom];

/// Immutable magnetic acquisition geometry compiled into one presented surface.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct MagneticTargetField {
    panes: Vec<PaneTargetField>,
    roots: Vec<RootTargetField>,
}

/// Stable target identities occupying all physical edges of one field.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct MagneticEdgeTargets {
    left: DropTargetId,
    right: DropTargetId,
    top: DropTargetId,
    bottom: DropTargetId,
}

impl MagneticEdgeTargets {
    pub(crate) const fn new(
        left: DropTargetId,
        right: DropTargetId,
        top: DropTargetId,
        bottom: DropTargetId,
    ) -> Self {
        Self {
            left,
            right,
            top,
            bottom,
        }
    }

    pub(crate) const fn target(self, edge: Edge) -> DropTargetId {
        match edge {
            Edge::Left => self.left,
            Edge::Right => self.right,
            Edge::Top => self.top,
            Edge::Bottom => self.bottom,
        }
    }

    pub(crate) fn targets(self) -> [DropTargetId; 4] {
        [self.left, self.right, self.top, self.bottom]
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct PaneTargetField {
    root: RootId,
    tabs: NodeId,
    layer: SceneLayerKey,
    depth: usize,
    content: LogicalRect,
    center: LogicalRect,
    center_target: DropTargetId,
    edges: MagneticEdgeTargets,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct RootTargetField {
    root: RootId,
    layer: SceneLayerKey,
    bounds: LogicalRect,
    edge_band: f64,
    edges: MagneticEdgeTargets,
}

impl PaneTargetField {
    #[cfg(test)]
    pub(crate) const fn root(&self) -> RootId {
        self.root
    }

    #[cfg(test)]
    pub(crate) const fn tabs(&self) -> NodeId {
        self.tabs
    }

    #[cfg(test)]
    pub(crate) const fn layer(&self) -> SceneLayerKey {
        self.layer
    }

    #[cfg(test)]
    pub(crate) const fn depth(&self) -> usize {
        self.depth
    }

    #[cfg(test)]
    pub(crate) const fn content(&self) -> LogicalRect {
        self.content
    }

    #[cfg(test)]
    pub(crate) const fn center(&self) -> LogicalRect {
        self.center
    }

    pub(crate) const fn center_target(&self) -> DropTargetId {
        self.center_target
    }

    pub(crate) const fn edges(&self) -> MagneticEdgeTargets {
        self.edges
    }
}

impl RootTargetField {
    #[cfg(test)]
    pub(crate) const fn root(&self) -> RootId {
        self.root
    }

    #[cfg(test)]
    pub(crate) const fn layer(&self) -> SceneLayerKey {
        self.layer
    }

    #[cfg(test)]
    pub(crate) const fn bounds(&self) -> LogicalRect {
        self.bounds
    }

    #[cfg(test)]
    pub(crate) const fn edge_band(&self) -> f64 {
        self.edge_band
    }

    pub(crate) const fn edges(&self) -> MagneticEdgeTargets {
        self.edges
    }
}

/// Semantic class of one acquired magnetic candidate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MagneticTargetKind {
    /// Merge into the target pane's tabs stack.
    PaneCenter,
    /// Split the target pane at its normalized nearest edge.
    PaneEdge(Edge),
    /// Split the complete target root at its outer edge.
    RootEdge(Edge),
}

/// One unique geometry winner before policy and transaction preflight.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct MagneticTargetCandidate {
    target: DropTargetId,
    kind: MagneticTargetKind,
    domain: MagneticTargetDomain,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum MagneticTargetDomain {
    Pane {
        content: LogicalRect,
        center: LogicalRect,
    },
    Root {
        bounds: LogicalRect,
        edge_band: f64,
    },
}

impl MagneticTargetCandidate {
    pub(crate) const fn target(self) -> DropTargetId {
        self.target
    }

    #[allow(
        dead_code,
        reason = "the latch slice consumes acquisition provenance next"
    )]
    #[cfg(test)]
    pub(crate) const fn kind(self) -> MagneticTargetKind {
        self.kind
    }

    pub(crate) fn retains_at(self, point: LogicalPoint, margin: f64) -> bool {
        self.signed_depth(point) >= -margin
    }

    pub(crate) fn is_deeply_acquired_at(self, point: LogicalPoint, margin: f64) -> bool {
        self.signed_depth(point) >= margin
    }

    fn signed_depth(self, point: LogicalPoint) -> f64 {
        match (self.kind, self.domain) {
            (MagneticTargetKind::PaneCenter, MagneticTargetDomain::Pane { center, .. }) => {
                rect_signed_depth(center, point)
            }
            (
                MagneticTargetKind::PaneEdge(edge),
                MagneticTargetDomain::Pane { content, center },
            ) => rect_signed_depth(content, point)
                .min(-rect_signed_depth(center, point))
                .min(normalized_edge_advantage(content, point, edge)),
            (
                MagneticTargetKind::RootEdge(edge),
                MagneticTargetDomain::Root { bounds, edge_band },
            ) => rect_signed_depth(bounds, point)
                .min(edge_band - edge_distance(bounds, point, edge))
                .min(edge_advantage(bounds, point, edge)),
            _ => f64::NEG_INFINITY,
        }
    }
}

impl MagneticTargetField {
    #[cfg(test)]
    pub(crate) fn is_empty(&self) -> bool {
        self.panes.is_empty() && self.roots.is_empty()
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn push_pane(
        &mut self,
        root: RootId,
        tabs: NodeId,
        layer: SceneLayerKey,
        depth: usize,
        content: LogicalRect,
        center_target: DropTargetId,
        edges: MagneticEdgeTargets,
        config: &DockPresentationConfig,
    ) -> Result<(), GeometryError> {
        if content.width() <= 0.0 || content.height() <= 0.0 {
            return Ok(());
        }
        let center = pane_center_basin(content, config)?;
        self.panes.push(PaneTargetField {
            root,
            tabs,
            layer,
            depth,
            content,
            center,
            center_target,
            edges,
        });
        Ok(())
    }

    pub(crate) fn push_root(
        &mut self,
        root: RootId,
        layer: SceneLayerKey,
        bounds: LogicalRect,
        edges: MagneticEdgeTargets,
        config: &DockPresentationConfig,
    ) {
        let edge_band = root_edge_band(bounds, config);
        self.roots.push(RootTargetField {
            root,
            layer,
            bounds,
            edge_band,
            edges,
        });
    }

    /// Selects one magnetic geometry winner after direct-hit and occlusion resolution.
    ///
    /// `permits` removes source-suppressed identities only. Availability and policy
    /// remain the responsibility of the canonical target preflight.
    pub(crate) fn candidate_at(
        &self,
        point: LogicalPoint,
        layer: SceneLayerKey,
        mut permits: impl FnMut(DropTargetId) -> bool,
    ) -> Option<MagneticTargetCandidate> {
        let root = self
            .roots
            .iter()
            .filter(|field| field.layer == layer)
            .filter_map(|field| field.candidate_at(point))
            .filter(|candidate| permits(candidate.target))
            .min_by_key(|candidate| candidate.target);
        if root.is_some() {
            return root;
        }

        self.panes
            .iter()
            .filter(|field| field.layer == layer && field.content.contains(point))
            .filter_map(|field| {
                field
                    .candidate_at(point)
                    .filter(|candidate| permits(candidate.target))
                    .map(|candidate| (field.depth, field.root, field.tabs, candidate))
            })
            .max_by(|left, right| {
                left.0
                    .cmp(&right.0)
                    .then_with(|| right.1.cmp(&left.1))
                    .then_with(|| right.2.cmp(&left.2))
            })
            .map(|(_, _, _, candidate)| candidate)
    }

    pub(crate) fn canonicalize(&mut self) {
        self.roots
            .sort_unstable_by_key(|field| (field.layer, field.root));
        self.panes
            .sort_unstable_by_key(|field| (field.layer, field.root, field.tabs));
    }

    pub(crate) fn panes(&self) -> impl Iterator<Item = &PaneTargetField> {
        self.panes.iter()
    }

    pub(crate) fn roots(&self) -> impl Iterator<Item = &RootTargetField> {
        self.roots.iter()
    }
}

pub(crate) fn pane_center_basin(
    content: LogicalRect,
    config: &DockPresentationConfig,
) -> Result<LogicalRect, GeometryError> {
    let center_fraction = config.magnetic_center_fraction();
    let inset_x = content.width() * (1.0 - center_fraction) * 0.5;
    let inset_y = content.height() * (1.0 - center_fraction) * 0.5;
    LogicalRect::new(
        content.x() + inset_x,
        content.y() + inset_y,
        content.width() - 2.0 * inset_x,
        content.height() - 2.0 * inset_y,
    )
}

pub(crate) fn root_edge_band(bounds: LogicalRect, config: &DockPresentationConfig) -> f64 {
    let short_side = bounds.width().min(bounds.height());
    (short_side * config.magnetic_outer_band_fraction()).clamp(
        config.magnetic_outer_band_minimum(),
        config.magnetic_outer_band_maximum(),
    )
}

impl PaneTargetField {
    fn candidate_at(&self, point: LogicalPoint) -> Option<MagneticTargetCandidate> {
        if !self.content.contains(point) {
            return None;
        }
        if self.center.contains(point) {
            return Some(MagneticTargetCandidate {
                target: self.center_target,
                kind: MagneticTargetKind::PaneCenter,
                domain: MagneticTargetDomain::Pane {
                    content: self.content,
                    center: self.center,
                },
            });
        }
        let edge = normalized_nearest_edge(self.content, point);
        Some(MagneticTargetCandidate {
            target: self.edges.target(edge),
            kind: MagneticTargetKind::PaneEdge(edge),
            domain: MagneticTargetDomain::Pane {
                content: self.content,
                center: self.center,
            },
        })
    }
}

impl RootTargetField {
    fn candidate_at(&self, point: LogicalPoint) -> Option<MagneticTargetCandidate> {
        if !self.bounds.contains(point) {
            return None;
        }
        let distances = edge_distances(self.bounds, point);
        let (edge, distance) = stable_minimum_edge(distances);
        (distance < self.edge_band).then_some(MagneticTargetCandidate {
            target: self.edges.target(edge),
            kind: MagneticTargetKind::RootEdge(edge),
            domain: MagneticTargetDomain::Root {
                bounds: self.bounds,
                edge_band: self.edge_band,
            },
        })
    }
}

fn normalized_nearest_edge(bounds: LogicalRect, point: LogicalPoint) -> Edge {
    let distances = edge_distances(bounds, point);
    stable_minimum_edge([
        distances[0] / bounds.width(),
        distances[1] / bounds.width(),
        distances[2] / bounds.height(),
        distances[3] / bounds.height(),
    ])
    .0
}

fn edge_distances(bounds: LogicalRect, point: LogicalPoint) -> [f64; 4] {
    [
        point.x() - bounds.x(),
        bounds.max().x() - point.x(),
        point.y() - bounds.y(),
        bounds.max().y() - point.y(),
    ]
}

fn edge_distance(bounds: LogicalRect, point: LogicalPoint, edge: Edge) -> f64 {
    match edge {
        Edge::Left => point.x() - bounds.x(),
        Edge::Right => bounds.max().x() - point.x(),
        Edge::Top => point.y() - bounds.y(),
        Edge::Bottom => bounds.max().y() - point.y(),
    }
}

fn edge_advantage(bounds: LogicalRect, point: LogicalPoint, edge: Edge) -> f64 {
    EDGE_ORDER
        .into_iter()
        .filter(|competitor| *competitor != edge)
        .map(|competitor| {
            let delta =
                edge_distance(bounds, point, competitor) - edge_distance(bounds, point, edge);
            delta / edge_gradient_difference_norm(bounds, edge, competitor, false)
        })
        .fold(f64::INFINITY, f64::min)
}

fn normalized_edge_advantage(bounds: LogicalRect, point: LogicalPoint, edge: Edge) -> f64 {
    [Edge::Left, Edge::Right, Edge::Top, Edge::Bottom]
        .into_iter()
        .filter(|competitor| *competitor != edge)
        .map(|competitor| {
            let selected = edge_distance(bounds, point, edge) / edge_axis_length(bounds, edge);
            let other =
                edge_distance(bounds, point, competitor) / edge_axis_length(bounds, competitor);
            (other - selected) / edge_gradient_difference_norm(bounds, edge, competitor, true)
        })
        .fold(f64::INFINITY, f64::min)
}

fn edge_axis_length(bounds: LogicalRect, edge: Edge) -> f64 {
    match edge {
        Edge::Left | Edge::Right => bounds.width(),
        Edge::Top | Edge::Bottom => bounds.height(),
    }
}

fn edge_gradient(edge: Edge) -> (f64, f64) {
    match edge {
        Edge::Left => (1.0, 0.0),
        Edge::Right => (-1.0, 0.0),
        Edge::Top => (0.0, 1.0),
        Edge::Bottom => (0.0, -1.0),
    }
}

fn edge_gradient_difference_norm(
    bounds: LogicalRect,
    selected: Edge,
    competitor: Edge,
    normalized: bool,
) -> f64 {
    let (selected_x, selected_y) = edge_gradient(selected);
    let (competitor_x, competitor_y) = edge_gradient(competitor);
    let selected_scale = normalized
        .then(|| edge_axis_length(bounds, selected).recip())
        .unwrap_or(1.0);
    let competitor_scale = normalized
        .then(|| edge_axis_length(bounds, competitor).recip())
        .unwrap_or(1.0);
    let dx = competitor_x * competitor_scale - selected_x * selected_scale;
    let dy = competitor_y * competitor_scale - selected_y * selected_scale;
    dx.hypot(dy)
}

fn rect_signed_depth(bounds: LogicalRect, point: LogicalPoint) -> f64 {
    let min = bounds.min();
    let max = bounds.max();
    let outside_x = if point.x() < min.x() {
        min.x() - point.x()
    } else if point.x() > max.x() {
        point.x() - max.x()
    } else {
        0.0
    };
    let outside_y = if point.y() < min.y() {
        min.y() - point.y()
    } else if point.y() > max.y() {
        point.y() - max.y()
    } else {
        0.0
    };
    if outside_x > 0.0 || outside_y > 0.0 {
        return -outside_x.hypot(outside_y);
    }
    (point.x() - min.x())
        .min(max.x() - point.x())
        .min(point.y() - min.y())
        .min(max.y() - point.y())
}

fn stable_minimum_edge(distances: [f64; 4]) -> (Edge, f64) {
    EDGE_ORDER
        .into_iter()
        .zip(distances)
        .min_by(|left, right| left.1.total_cmp(&right.1))
        .expect("the complete physical edge set is never empty")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::{Node, Workspace};
    use crate::ids::{ItemId, SurfaceId};

    fn target(node: NodeId, edge: Edge) -> DropTargetId {
        DropTargetId::InnerEdge {
            surface: SurfaceId::new(1),
            root: RootId::new(2),
            node,
            edge,
        }
    }

    fn edges(node: NodeId) -> MagneticEdgeTargets {
        MagneticEdgeTargets::new(
            target(node, Edge::Left),
            target(node, Edge::Right),
            target(node, Edge::Top),
            target(node, Edge::Bottom),
        )
    }

    fn outer_target(node: NodeId, edge: Edge) -> DropTargetId {
        DropTargetId::OuterEdge {
            surface: SurfaceId::new(1),
            root: RootId::new(2),
            node,
            edge,
        }
    }

    fn outer_edges(node: NodeId) -> MagneticEdgeTargets {
        MagneticEdgeTargets::new(
            outer_target(node, Edge::Left),
            outer_target(node, Edge::Right),
            outer_target(node, Edge::Top),
            outer_target(node, Edge::Bottom),
        )
    }

    #[test]
    fn central_fifty_two_percent_is_a_center_basin() {
        let mut workspace = Workspace::builder();
        let tabs = workspace.insert_node(Node::tabs([ItemId::new(1)]));
        let mut field = MagneticTargetField::default();
        let center = DropTargetId::Center {
            surface: SurfaceId::new(1),
            root: RootId::new(2),
            tabs,
        };
        field
            .push_pane(
                RootId::new(2),
                tabs,
                SceneLayerKey::surface_base(),
                1,
                LogicalRect::new(0.0, 0.0, 100.0, 100.0).expect("bounds"),
                center,
                edges(tabs),
                &DockPresentationConfig::default(),
            )
            .expect("field geometry");

        let inside = LogicalPoint::new(65.0, 65.0).expect("point");
        let outside = LogicalPoint::new(77.0, 50.0).expect("point");
        assert_eq!(
            field
                .candidate_at(inside, SceneLayerKey::surface_base(), |_| true)
                .map(MagneticTargetCandidate::target),
            Some(center)
        );
        assert_eq!(
            field
                .candidate_at(outside, SceneLayerKey::surface_base(), |_| true)
                .map(MagneticTargetCandidate::target),
            Some(target(tabs, Edge::Right))
        );
    }

    #[test]
    fn normalized_distance_selects_edges_without_aspect_ratio_bias() {
        let mut workspace = Workspace::builder();
        let tabs = workspace.insert_node(Node::tabs([ItemId::new(1)]));
        let mut field = MagneticTargetField::default();
        field
            .push_pane(
                RootId::new(2),
                tabs,
                SceneLayerKey::surface_base(),
                1,
                LogicalRect::new(0.0, 0.0, 200.0, 100.0).expect("bounds"),
                DropTargetId::Center {
                    surface: SurfaceId::new(1),
                    root: RootId::new(2),
                    tabs,
                },
                edges(tabs),
                &DockPresentationConfig::default(),
            )
            .expect("field geometry");

        let point = LogicalPoint::new(100.0, 10.0).expect("point");
        assert_eq!(
            field
                .candidate_at(point, SceneLayerKey::surface_base(), |_| true)
                .map(MagneticTargetCandidate::target),
            Some(target(tabs, Edge::Top))
        );
    }

    #[test]
    fn pane_edge_release_hysteresis_uses_true_diagonal_distance() {
        let candidate = MagneticTargetCandidate {
            target: target(NodeId::default(), Edge::Left),
            kind: MagneticTargetKind::PaneEdge(Edge::Left),
            domain: MagneticTargetDomain::Pane {
                content: LogicalRect::new(0.0, 0.0, 100.0, 100.0).expect("bounds"),
                center: LogicalRect::new(24.0, 24.0, 52.0, 52.0).expect("center"),
            },
        };
        let diagonal = 7.5 / 2.0_f64.sqrt();
        assert!(candidate.retains_at(
            LogicalPoint::new(20.0 + diagonal, 20.0 - diagonal).expect("point"),
            8.0,
        ));
        assert!(
            !candidate.retains_at(
                LogicalPoint::new(20.0 + 8.5 / 2.0_f64.sqrt(), 20.0 - 8.5 / 2.0_f64.sqrt())
                    .expect("point"),
                8.0,
            )
        );
    }

    #[test]
    fn root_edge_release_hysteresis_uses_true_diagonal_distance() {
        let candidate = MagneticTargetCandidate {
            target: outer_target(NodeId::default(), Edge::Left),
            kind: MagneticTargetKind::RootEdge(Edge::Left),
            domain: MagneticTargetDomain::Root {
                bounds: LogicalRect::new(0.0, 0.0, 100.0, 100.0).expect("bounds"),
                edge_band: 32.0,
            },
        };
        let diagonal = 7.5 / 2.0_f64.sqrt();
        assert!(candidate.retains_at(
            LogicalPoint::new(20.0 + diagonal, 20.0 - diagonal).expect("point"),
            8.0,
        ));
        assert!(
            !candidate.retains_at(
                LogicalPoint::new(20.0 + 8.5 / 2.0_f64.sqrt(), 20.0 - 8.5 / 2.0_f64.sqrt())
                    .expect("point"),
                8.0,
            )
        );
    }

    #[test]
    fn outer_root_band_precedes_the_overlapping_pane_basin() {
        let mut workspace = Workspace::builder();
        let tabs = workspace.insert_node(Node::tabs([ItemId::new(1)]));
        let bounds = LogicalRect::new(0.0, 0.0, 200.0, 100.0).expect("bounds");
        let mut field = MagneticTargetField::default();
        field
            .push_pane(
                RootId::new(2),
                tabs,
                SceneLayerKey::surface_base(),
                1,
                bounds,
                DropTargetId::Center {
                    surface: SurfaceId::new(1),
                    root: RootId::new(2),
                    tabs,
                },
                edges(tabs),
                &DockPresentationConfig::default(),
            )
            .expect("field geometry");
        field.push_root(
            RootId::new(2),
            SceneLayerKey::surface_base(),
            bounds,
            outer_edges(tabs),
            &DockPresentationConfig::default(),
        );

        let point = LogicalPoint::new(10.0, 50.0).expect("point");
        assert_eq!(
            field
                .candidate_at(point, SceneLayerKey::surface_base(), |_| true)
                .map(MagneticTargetCandidate::target),
            Some(outer_target(tabs, Edge::Left))
        );
        assert_eq!(
            field
                .candidate_at(point, SceneLayerKey::surface_base(), |candidate| {
                    candidate != outer_target(tabs, Edge::Left)
                })
                .map(MagneticTargetCandidate::target),
            Some(target(tabs, Edge::Left))
        );
    }
}
