use std::sync::Arc;

use super::{
    SceneCompilationError, clip_rect, edge_availability, intersect_rect, midpoint, rect_contains,
    rect_has_area, rects_overlap, tab_availability,
};
use crate::command::{DockFraction, Edge};
use crate::drop_guide::{
    DropGuideClusterId, DropGuideClusterRecord, DropGuideEdgeSet, DropGuideTargetRecord,
};
use crate::drop_target::{
    DropOcclusionRecord, DropTargetAvailability, DropTargetLocation, DropTargetRecord, DropVisual,
    SceneLayerKey,
};
use crate::geometry::{GeometryError, LogicalPoint, LogicalRect};
use crate::hit_region::HitRegion;
use crate::ids::{NodeId, RootId, SurfaceId};
use crate::policy::DockPolicySnapshot;
use crate::presentation_config::DockPresentationConfig;
use crate::scene::PresentationPlan;
use crate::target_field::{pane_center_basin, root_edge_band};

#[derive(Clone, Copy)]
struct GuideButtonGeometry {
    draw: LogicalRect,
    hit: LogicalRect,
}

impl GuideButtonGeometry {
    fn clip_to_hit_cell(&mut self, cell: LogicalRect) -> Result<(), GeometryError> {
        self.hit = clip_rect(self.hit, cell)?;
        self.draw = clip_rect(self.draw, self.hit)?;
        Ok(())
    }
}

#[derive(Clone, Copy)]
struct GuideMetrics {
    half_extent: f64,
    hit_padding: f64,
    inner_offset: f64,
    outer_inset: f64,
}

impl GuideMetrics {
    fn compact(
        config: &DockPresentationConfig,
        bounds: LogicalRect,
        reference_span: f64,
    ) -> Option<Self> {
        // There is no origin-independent logical-pixel cutoff for f64 geometry.
        // Representability is therefore the exact lower bound and becomes a typed
        // compilation rejection instead of silently removing semantic slots.
        let scale = (bounds.width() / reference_span)
            .min(bounds.height() / reference_span)
            .min(1.0);
        if !scale.is_finite() || scale <= 0.0 {
            return None;
        }
        Some(Self {
            half_extent: config.guide_extent() * 0.5 * scale,
            hit_padding: config.guide_hit_padding() * scale,
            inner_offset: (config.guide_extent() + config.guide_gap()) * scale,
            outer_inset: config.guide_outer_inset() * scale,
        })
    }
}

#[derive(Clone, Copy)]
pub(super) struct GuideEdgeGeometry {
    left: GuideButtonGeometry,
    right: GuideButtonGeometry,
    top: GuideButtonGeometry,
    bottom: GuideButtonGeometry,
}

impl GuideEdgeGeometry {
    const fn get(self, edge: Edge) -> GuideButtonGeometry {
        match edge {
            Edge::Left => self.left,
            Edge::Right => self.right,
            Edge::Top => self.top,
            Edge::Bottom => self.bottom,
        }
    }

    const fn buttons(self) -> [GuideButtonGeometry; 4] {
        [self.left, self.right, self.top, self.bottom]
    }

    fn set(&mut self, edge: Edge, button: GuideButtonGeometry) {
        *match edge {
            Edge::Left => &mut self.left,
            Edge::Right => &mut self.right,
            Edge::Top => &mut self.top,
            Edge::Bottom => &mut self.bottom,
        } = button;
    }
}

pub(super) fn outer_guide_geometry(
    config: &DockPresentationConfig,
    bounds: LogicalRect,
    cluster: DropGuideClusterId,
    layer: SceneLayerKey,
    occlusions: &[DropOcclusionRecord],
) -> Result<Option<GuideEdgeGeometry>, SceneCompilationError> {
    let metrics = GuideMetrics::compact(config, bounds, config.outer_guide_reference_span())
        .ok_or(SceneCompilationError::UnrepresentableGuideGeometry { cluster, bounds })?;
    let center_x = midpoint(bounds.x(), bounds.max().x());
    let center_y = midpoint(bounds.y(), bounds.max().y());
    let button = |x, y| {
        guide_button(
            bounds,
            x,
            y,
            metrics.half_extent,
            metrics.half_extent,
            metrics.hit_padding,
        )
    };
    let mut geometry = GuideEdgeGeometry {
        left: button(bounds.x() + metrics.outer_inset, center_y)?,
        right: button(bounds.max().x() - metrics.outer_inset, center_y)?,
        top: button(center_x, bounds.y() + metrics.outer_inset)?,
        bottom: button(center_x, bounds.max().y() - metrics.outer_inset)?,
    };
    separate_outer_cross_hits(bounds, &mut geometry)?;
    ensure_complete_guide_geometry(cluster, bounds, &geometry.buttons())?;
    let foreground = occlusions
        .iter()
        .filter(|occlusion| occlusion.layer() > layer)
        .map(|occlusion| occlusion.region().rect())
        // Keep latent geometry behind a complete covering window. The exact
        // hover query still sees its blocker, unless that window is the whole
        // drag source being suppressed. Partial occluders remain obstacles.
        .filter(|occlusion| !rect_contains(*occlusion, bounds))
        .collect::<Vec<_>>();
    let original = geometry;
    let edges = [Edge::Left, Edge::Right, Edge::Top, Edge::Bottom];
    for edge in edges {
        let button = original.get(edge);
        if !foreground.iter().any(|cut| rects_overlap(button.hit, *cut)) {
            continue;
        }
        let obstacles = foreground.iter().copied().chain(
            edges
                .into_iter()
                .filter(|other| *other != edge)
                .map(|other| geometry.get(other).hit),
        );
        let Some(placement) = guide_placement(
            cluster,
            bounds,
            button.hit.width() * 0.5,
            button.hit.height() * 0.5,
            LogicalPoint::new(
                midpoint(button.hit.x(), button.hit.max().x()),
                midpoint(button.hit.y(), button.hit.max().y()),
            )?,
            obstacles,
        )?
        else {
            return Ok(None);
        };
        let move_rect = |rect: LogicalRect| {
            LogicalRect::new(
                placement.center_x
                    + (rect.x() - midpoint(button.hit.x(), button.hit.max().x())) * placement.scale,
                placement.center_y
                    + (rect.y() - midpoint(button.hit.y(), button.hit.max().y())) * placement.scale,
                rect.width() * placement.scale,
                rect.height() * placement.scale,
            )
        };
        let hit = clip_rect(move_rect(button.hit)?, placement.cell)?;
        geometry.set(
            edge,
            GuideButtonGeometry {
                draw: clip_rect(move_rect(button.draw)?, hit)?,
                hit,
            },
        );
    }
    ensure_complete_guide_geometry(cluster, bounds, &geometry.buttons())?;
    Ok(Some(geometry))
}

struct GuidePlacement {
    cell: LogicalRect,
    center_x: f64,
    center_y: f64,
    scale: f64,
    displacement: f64,
}

fn guide_placement(
    cluster: DropGuideClusterId,
    bounds: LogicalRect,
    half_width: f64,
    half_height: f64,
    preferred: LogicalPoint,
    obstacles: impl IntoIterator<Item = LogicalRect>,
) -> Result<Option<GuidePlacement>, SceneCompilationError> {
    let preferred_x = preferred.x();
    let preferred_y = preferred.y();
    let mut cells = vec![bounds];
    for obstacle in obstacles {
        let mut remaining = Vec::new();
        for cell in cells {
            let Some(cut) = intersect_rect(cell, obstacle)? else {
                if !remaining.contains(&cell) {
                    remaining.push(cell);
                }
                continue;
            };
            // Keep maximal free rectangles, rather than disjoint strips. This
            // permits a full-size cluster to move beside an outer button.
            for free in [
                LogicalRect::new(cell.x(), cell.y(), cut.x() - cell.x(), cell.height()),
                LogicalRect::new(
                    cut.max().x(),
                    cell.y(),
                    cell.max().x() - cut.max().x(),
                    cell.height(),
                ),
                LogicalRect::new(cell.x(), cell.y(), cell.width(), cut.y() - cell.y()),
                LogicalRect::new(
                    cell.x(),
                    cut.max().y(),
                    cell.width(),
                    cell.max().y() - cut.max().y(),
                ),
            ] {
                let free = free?;
                if rect_has_area(free) && !remaining.contains(&free) {
                    remaining.push(free);
                }
            }
        }
        cells = remaining;
    }
    if cells.is_empty() {
        return Ok(None);
    }
    let mut best: Option<GuidePlacement> = None;
    for cell in cells {
        let scale = (cell.width() / (2.0 * half_width))
            .min(cell.height() / (2.0 * half_height))
            .min(1.0);
        if !scale.is_finite() || scale <= 0.0 {
            continue;
        }
        let center_x = preferred_x
            .max(cell.x() + half_width * scale)
            .min(cell.max().x() - half_width * scale);
        let center_y = preferred_y
            .max(cell.y() + half_height * scale)
            .min(cell.max().y() - half_height * scale);
        let displacement = (center_x - preferred_x).hypot(center_y - preferred_y);
        if best.as_ref().is_some_and(|best| {
            best.scale > scale || (best.scale == scale && best.displacement <= displacement)
        }) {
            continue;
        }
        best = Some(GuidePlacement {
            cell,
            center_x,
            center_y,
            scale,
            displacement,
        });
    }
    // Placement depends only on immutable layout geometry, never the pointer
    // or active slot. Crossing guide boundaries therefore cannot move buttons.
    best.map(Some)
        .ok_or(SceneCompilationError::UnrepresentableGuideGeometry { cluster, bounds })
}

#[allow(clippy::too_many_arguments)]
pub(super) fn compile_inner_targets_and_guide(
    policy: &DockPolicySnapshot,
    config: &DockPresentationConfig,
    outer_guides: GuideEdgeGeometry,
    surface: SurfaceId,
    root: RootId,
    tabs: NodeId,
    node_bounds: LogicalRect,
    content: LogicalRect,
    layer: SceneLayerKey,
    fraction: DockFraction,
    root_central: bool,
    ready: &mut PresentationPlan,
) -> Result<Option<DropGuideClusterRecord>, SceneCompilationError> {
    let cluster = DropGuideClusterId::inner(surface, root, tabs);
    let placement_bounds = if rect_has_area(content) {
        content
    } else {
        node_bounds
    };
    let nominal = GuideMetrics::compact(
        config,
        placement_bounds,
        config.inner_guide_reference_span(),
    )
    .ok_or(SceneCompilationError::UnrepresentableGuideGeometry {
        cluster,
        bounds: placement_bounds,
    })?;
    let half_span = nominal.half_extent
        + nominal.hit_padding
        + if root_central {
            0.0
        } else {
            nominal.inner_offset
        };
    let obstacles = outer_guides
        .buttons()
        .into_iter()
        .map(|button| button.hit)
        .chain(
            ready
                .drop_occlusions()
                .iter()
                .filter(|occlusion| occlusion.layer() > layer)
                .map(|occlusion| occlusion.region().rect())
                .filter(|occlusion| !rect_contains(*occlusion, placement_bounds)),
        );
    let Some(placement) = guide_placement(
        cluster,
        placement_bounds,
        half_span,
        half_span,
        LogicalPoint::new(
            midpoint(placement_bounds.x(), placement_bounds.max().x()),
            midpoint(placement_bounds.y(), placement_bounds.max().y()),
        )?,
        obstacles,
    )?
    else {
        return Ok(None);
    };
    let GuidePlacement {
        cell,
        center_x,
        center_y,
        scale,
        ..
    } = placement;
    let metrics = GuideMetrics {
        half_extent: nominal.half_extent * scale,
        hit_padding: nominal.hit_padding * scale,
        inner_offset: nominal.inner_offset * scale,
        outer_inset: nominal.outer_inset * scale,
    };
    let center_geometry = guide_button(
        cell,
        center_x,
        center_y,
        metrics.half_extent,
        metrics.half_extent,
        metrics.hit_padding,
    )?;
    ensure_complete_guide_geometry(cluster, placement_bounds, &[center_geometry])?;
    let center_location = DropTargetLocation::Center {
        surface,
        root,
        tabs,
    };
    let center_target = install_canonical_drop_target(
        center_location,
        tab_availability(policy),
        center_geometry,
        placement_bounds,
        layer,
        ready,
    );
    let center = guide_target_record(center_target, center_geometry);
    if root_central {
        return Ok(Some(DropGuideClusterRecord::inner_center(
            surface,
            root,
            tabs,
            HitRegion::new(node_bounds),
            layer,
            center,
        )));
    }

    let mut geometry = GuideEdgeGeometry {
        left: guide_button(
            cell,
            center_x - metrics.inner_offset,
            center_y,
            metrics.half_extent,
            metrics.half_extent,
            metrics.hit_padding,
        )?,
        right: guide_button(
            cell,
            center_x + metrics.inner_offset,
            center_y,
            metrics.half_extent,
            metrics.half_extent,
            metrics.hit_padding,
        )?,
        top: guide_button(
            cell,
            center_x,
            center_y - metrics.inner_offset,
            metrics.half_extent,
            metrics.half_extent,
            metrics.hit_padding,
        )?,
        bottom: guide_button(
            cell,
            center_x,
            center_y + metrics.inner_offset,
            metrics.half_extent,
            metrics.half_extent,
            metrics.hit_padding,
        )?,
    };
    separate_inner_cross_hits(cell, center_geometry, &mut geometry)?;
    let mut all = Vec::from(geometry.buttons());
    all.push(center_geometry);
    ensure_complete_guide_geometry(cluster, placement_bounds, &all)?;
    let mut edge_record = |edge| {
        let location = DropTargetLocation::InnerEdge {
            surface,
            root,
            node: tabs,
            edge,
            fraction,
        };
        let preview = edge_preview(node_bounds, edge, fraction)?;
        let target = install_canonical_drop_target(
            location,
            edge_availability(policy),
            geometry.get(edge),
            preview,
            layer,
            ready,
        );
        Ok::<_, SceneCompilationError>(guide_target_record(target, geometry.get(edge)))
    };
    let edges = DropGuideEdgeSet::new(
        edge_record(Edge::Left)?,
        edge_record(Edge::Right)?,
        edge_record(Edge::Top)?,
        edge_record(Edge::Bottom)?,
    );
    Ok(Some(DropGuideClusterRecord::inner(
        surface,
        root,
        tabs,
        HitRegion::new(node_bounds),
        layer,
        center,
        edges,
    )))
}

#[allow(clippy::too_many_arguments)]
pub(super) fn compile_outer_targets_and_guide(
    policy: &DockPolicySnapshot,
    geometry: GuideEdgeGeometry,
    surface: SurfaceId,
    root: RootId,
    root_node: NodeId,
    bounds: LogicalRect,
    layer: SceneLayerKey,
    fraction: DockFraction,
    ready: &mut PresentationPlan,
) -> Result<DropGuideClusterRecord, SceneCompilationError> {
    let edge_record = |edge, ready: &mut PresentationPlan| {
        let location = DropTargetLocation::OuterEdge {
            surface,
            root,
            node: root_node,
            edge,
            fraction,
        };
        let preview = edge_preview(bounds, edge, fraction)?;
        let target = install_canonical_drop_target(
            location,
            edge_availability(policy),
            geometry.get(edge),
            preview,
            layer,
            ready,
        );
        Ok::<_, SceneCompilationError>(guide_target_record(target, geometry.get(edge)))
    };
    let left = edge_record(Edge::Left, ready)?;
    let right = edge_record(Edge::Right, ready)?;
    let top = edge_record(Edge::Top, ready)?;
    let bottom = edge_record(Edge::Bottom, ready)?;
    Ok(DropGuideClusterRecord::outer(
        surface,
        root,
        HitRegion::new(bounds),
        layer,
        DropGuideEdgeSet::new(left, right, top, bottom),
    ))
}

#[allow(clippy::too_many_arguments)]
pub(super) fn compile_outer_targets_without_guide(
    policy: &DockPolicySnapshot,
    config: &DockPresentationConfig,
    surface: SurfaceId,
    root: RootId,
    root_node: NodeId,
    bounds: LogicalRect,
    layer: SceneLayerKey,
    fraction: DockFraction,
    ready: &mut PresentationPlan,
) -> Result<(), SceneCompilationError> {
    for edge in [Edge::Left, Edge::Right, Edge::Top, Edge::Bottom] {
        let location = DropTargetLocation::OuterEdge {
            surface,
            root,
            node: root_node,
            edge,
            fraction,
        };
        install_canonical_drop_target_with_region(
            location,
            edge_availability(policy),
            HitRegion::new(outer_edge_region(bounds, edge, config)?),
            edge_preview(bounds, edge, fraction)?,
            layer,
            ready,
        );
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub(super) fn compile_inner_targets_without_guide(
    policy: &DockPolicySnapshot,
    config: &DockPresentationConfig,
    surface: SurfaceId,
    root: RootId,
    tabs: NodeId,
    node_bounds: LogicalRect,
    content: LogicalRect,
    layer: SceneLayerKey,
    fraction: DockFraction,
    root_central: bool,
    ready: &mut PresentationPlan,
) -> Result<(), SceneCompilationError> {
    let placement_bounds = if rect_has_area(content) {
        content
    } else {
        node_bounds
    };
    let center = DropTargetLocation::Center {
        surface,
        root,
        tabs,
    };
    install_canonical_drop_target_with_region(
        center,
        tab_availability(policy),
        HitRegion::new(pane_center_basin(placement_bounds, config)?),
        placement_bounds,
        layer,
        ready,
    );

    if root_central {
        return Ok(());
    }

    for edge in [Edge::Left, Edge::Right, Edge::Top, Edge::Bottom] {
        let location = DropTargetLocation::InnerEdge {
            surface,
            root,
            node: tabs,
            edge,
            fraction,
        };
        install_canonical_drop_target_with_region(
            location,
            edge_availability(policy),
            HitRegion::new(pane_edge_region(placement_bounds, edge, config)?),
            edge_preview(node_bounds, edge, fraction)?,
            layer,
            ready,
        );
    }
    Ok(())
}

fn install_canonical_drop_target(
    location: DropTargetLocation,
    availability: DropTargetAvailability,
    geometry: GuideButtonGeometry,
    preview: LogicalRect,
    layer: SceneLayerKey,
    ready: &mut PresentationPlan,
) -> Arc<DropTargetRecord> {
    install_canonical_drop_target_with_region(
        location,
        availability,
        HitRegion::new(geometry.hit),
        preview,
        layer,
        ready,
    )
}

fn install_canonical_drop_target_with_region(
    location: DropTargetLocation,
    availability: DropTargetAvailability,
    region: HitRegion,
    preview: LogicalRect,
    layer: SceneLayerKey,
    ready: &mut PresentationPlan,
) -> Arc<DropTargetRecord> {
    let record = Arc::new(DropTargetRecord::new(
        location,
        availability,
        region,
        layer,
        DropVisual::new(preview),
    ));
    ready.push_drop_target(Arc::clone(&record));
    record
}

fn guide_target_record(
    target: Arc<DropTargetRecord>,
    geometry: GuideButtonGeometry,
) -> DropGuideTargetRecord {
    DropGuideTargetRecord::new(target, geometry.draw)
}

fn guide_button(
    bounds: LogicalRect,
    center_x: f64,
    center_y: f64,
    half_width: f64,
    half_height: f64,
    hit_padding: f64,
) -> Result<GuideButtonGeometry, GeometryError> {
    let draw = LogicalRect::new(
        center_x - half_width,
        center_y - half_height,
        2.0 * half_width,
        2.0 * half_height,
    )?;
    let expanded = LogicalRect::new(
        draw.x() - hit_padding,
        draw.y() - hit_padding,
        draw.width() + 2.0 * hit_padding,
        draw.height() + 2.0 * hit_padding,
    )?;
    Ok(GuideButtonGeometry {
        draw: clip_rect(draw, bounds)?,
        hit: clip_rect(expanded, bounds)?,
    })
}

fn pane_edge_region(
    bounds: LogicalRect,
    edge: Edge,
    config: &DockPresentationConfig,
) -> Result<LogicalRect, GeometryError> {
    let center = pane_center_basin(bounds, config)?;
    match edge {
        Edge::Left => LogicalRect::new(
            bounds.x(),
            bounds.y(),
            (center.x() - bounds.x()).max(0.0),
            bounds.height(),
        ),
        Edge::Right => LogicalRect::new(
            center.max().x(),
            bounds.y(),
            (bounds.max().x() - center.max().x()).max(0.0),
            bounds.height(),
        ),
        Edge::Top => LogicalRect::new(
            bounds.x(),
            bounds.y(),
            bounds.width(),
            (center.y() - bounds.y()).max(0.0),
        ),
        Edge::Bottom => LogicalRect::new(
            bounds.x(),
            center.max().y(),
            bounds.width(),
            (bounds.max().y() - center.max().y()).max(0.0),
        ),
    }
}

fn outer_edge_region(
    bounds: LogicalRect,
    edge: Edge,
    config: &DockPresentationConfig,
) -> Result<LogicalRect, GeometryError> {
    let band = root_edge_band(bounds, config);
    match edge {
        Edge::Left => LogicalRect::new(
            bounds.x(),
            bounds.y(),
            band.min(bounds.width()),
            bounds.height(),
        ),
        Edge::Right => LogicalRect::new(
            (bounds.max().x() - band).max(bounds.x()),
            bounds.y(),
            band.min(bounds.width()),
            bounds.height(),
        ),
        Edge::Top => LogicalRect::new(
            bounds.x(),
            bounds.y(),
            bounds.width(),
            band.min(bounds.height()),
        ),
        Edge::Bottom => LogicalRect::new(
            bounds.x(),
            (bounds.max().y() - band).max(bounds.y()),
            bounds.width(),
            band.min(bounds.height()),
        ),
    }
}

fn separate_inner_cross_hits(
    bounds: LogicalRect,
    center: GuideButtonGeometry,
    edges: &mut GuideEdgeGeometry,
) -> Result<(), GeometryError> {
    let left_cell = LogicalRect::new(
        bounds.x(),
        bounds.y(),
        (center.hit.x() - bounds.x()).max(0.0),
        bounds.height(),
    )?;
    let right_cell = LogicalRect::new(
        center.hit.max().x(),
        bounds.y(),
        (bounds.max().x() - center.hit.max().x()).max(0.0),
        bounds.height(),
    )?;
    let top_cell = LogicalRect::new(
        bounds.x(),
        bounds.y(),
        bounds.width(),
        (center.hit.y() - bounds.y()).max(0.0),
    )?;
    let bottom_cell = LogicalRect::new(
        bounds.x(),
        center.hit.max().y(),
        bounds.width(),
        (bounds.max().y() - center.hit.max().y()).max(0.0),
    )?;
    edges.left.clip_to_hit_cell(left_cell)?;
    edges.right.clip_to_hit_cell(right_cell)?;
    edges.top.clip_to_hit_cell(top_cell)?;
    edges.bottom.clip_to_hit_cell(bottom_cell)
}

fn separate_outer_cross_hits(
    bounds: LogicalRect,
    edges: &mut GuideEdgeGeometry,
) -> Result<(), GeometryError> {
    let vertical_min_x = edges.top.hit.x().min(edges.bottom.hit.x());
    let vertical_max_x = edges.top.hit.max().x().max(edges.bottom.hit.max().x());
    let horizontal_min_y = edges.left.hit.y().min(edges.right.hit.y());
    let horizontal_max_y = edges.left.hit.max().y().max(edges.right.hit.max().y());
    let left_cell = LogicalRect::new(
        bounds.x(),
        bounds.y(),
        (vertical_min_x - bounds.x()).max(0.0),
        bounds.height(),
    )?;
    let right_cell = LogicalRect::new(
        vertical_max_x,
        bounds.y(),
        (bounds.max().x() - vertical_max_x).max(0.0),
        bounds.height(),
    )?;
    let top_cell = LogicalRect::new(
        bounds.x(),
        bounds.y(),
        bounds.width(),
        (horizontal_min_y - bounds.y()).max(0.0),
    )?;
    let bottom_cell = LogicalRect::new(
        bounds.x(),
        horizontal_max_y,
        bounds.width(),
        (bounds.max().y() - horizontal_max_y).max(0.0),
    )?;
    edges.left.clip_to_hit_cell(left_cell)?;
    edges.right.clip_to_hit_cell(right_cell)?;
    edges.top.clip_to_hit_cell(top_cell)?;
    edges.bottom.clip_to_hit_cell(bottom_cell)
}

fn ensure_complete_guide_geometry(
    cluster: DropGuideClusterId,
    bounds: LogicalRect,
    buttons: &[GuideButtonGeometry],
) -> Result<(), SceneCompilationError> {
    let complete = buttons.iter().all(|button| {
        rect_has_area(button.draw)
            && rect_has_area(button.hit)
            && rect_contains(bounds, button.hit)
            && rect_contains(button.hit, button.draw)
    });
    if complete && !guide_hits_overlap(buttons) {
        Ok(())
    } else {
        Err(SceneCompilationError::UnrepresentableGuideGeometry { cluster, bounds })
    }
}

fn guide_hits_overlap(buttons: &[GuideButtonGeometry]) -> bool {
    buttons.iter().enumerate().any(|(index, first)| {
        buttons
            .iter()
            .skip(index + 1)
            .any(|second| rects_overlap(first.hit, second.hit))
    })
}

pub(in crate::scene_compiler) fn edge_preview(
    bounds: LogicalRect,
    edge: Edge,
    fraction: DockFraction,
) -> Result<LogicalRect, GeometryError> {
    let fraction = f64::from(fraction.get());
    let min = bounds.min();
    let max = bounds.max();
    match edge {
        Edge::Left => LogicalRect::from_min_max(
            min,
            LogicalPoint::new(min.x() + bounds.width() * fraction, max.y())?,
        ),
        Edge::Right => {
            let width = bounds.width() * fraction;
            LogicalRect::from_min_max(LogicalPoint::new(max.x() - width, min.y())?, max)
        }
        Edge::Top => LogicalRect::from_min_max(
            min,
            LogicalPoint::new(max.x(), min.y() + bounds.height() * fraction)?,
        ),
        Edge::Bottom => {
            let height = bounds.height() * fraction;
            LogicalRect::from_min_max(LogicalPoint::new(min.x(), max.y() - height)?, max)
        }
    }
}
