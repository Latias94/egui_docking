//! Private device-specific hit fragments retained with one exact output.

use crate::drop_target::{DropOcclusionRecord, SceneLayerKey};
use crate::geometry::{LogicalPoint, LogicalRect};
use crate::graph::Axis;
use crate::scene::{ContainedResizeDirection, PresentationPlan, SplitterJunctionId};

use super::{PresentationHitRegion, PresentationHitRegionId, PresentationHitRegionKind};

#[cfg(test)]
#[path = "touch_tests.rs"]
mod tests;

pub(super) const fn replaces(kind: PresentationHitRegionKind) -> bool {
    matches!(
        kind,
        PresentationHitRegionKind::SplitterHandle(_)
            | PresentationHitRegionKind::SplitterJunction(_)
            | PresentationHitRegionKind::ContainedResize { .. }
    )
}

pub(super) fn compile(
    plan: &PresentationPlan,
    regions: &[PresentationHitRegion],
) -> Vec<(PresentationHitRegionId, LogicalRect)> {
    let Some(facts) = plan.layout_facts() else {
        // Synthetic plans have no exact configuration authority. Fail closed.
        return Vec::new();
    };
    let config = facts.config();
    let splitter_extent = config
        .touch_resize_hit_extent()
        .max(config.splitter_hit_extent());
    let contained_extent = config
        .touch_resize_hit_extent()
        .max(config.floating_resize_extent());
    let protected = protected_regions(plan, regions);
    let mut compiled = Vec::new();
    for region in regions {
        let rect = match region.id().kind() {
            PresentationHitRegionKind::SplitterHandle(id) => {
                let Some(splitter) = plan.splitter_record(id) else {
                    continue;
                };
                let Some(root) = facts.root(id.root) else {
                    continue;
                };
                let Some(clip) = intersection(root.bounds(), plan.bounds()) else {
                    continue;
                };
                splitter_band(
                    splitter.draw_bounds(),
                    splitter.axis(),
                    splitter_extent,
                    clip,
                )
            }
            PresentationHitRegionKind::SplitterJunction(id) => {
                let Some(root) = facts.root(id.root()) else {
                    continue;
                };
                let Some(clip) = intersection(root.bounds(), plan.bounds()) else {
                    continue;
                };
                let extent = splitter_extent
                    .max(region.hit().rect().width())
                    .max(region.hit().rect().height());
                junction_square(plan, id, extent, clip)
            }
            PresentationHitRegionKind::ContainedResize {
                floating,
                direction,
            } => {
                let Some(outer) = plan.unclipped_contained_bounds(floating) else {
                    continue;
                };
                contained_partition(
                    outer,
                    config.floating_resize_extent(),
                    contained_extent,
                    direction,
                    plan.bounds(),
                )
            }
            _ => continue,
        };
        let Some(rect) = rect else {
            continue;
        };
        let mut fragments = visible_fragments(rect, region.stack().layer(), plan.drop_occlusions());
        for &(layer, protected_rect) in &protected {
            if layer <= region.stack().layer() {
                subtract_from(&mut fragments, protected_rect);
            }
        }
        compiled.extend(fragments.into_iter().map(|rect| (region.id(), rect)));
    }
    debug_assert!(compiled.windows(2).all(|pair| pair[0].0 <= pair[1].0));
    compiled
}

fn protected_regions(
    plan: &PresentationPlan,
    regions: &[PresentationHitRegion],
) -> Vec<(SceneLayerKey, LogicalRect)> {
    let mut protected = Vec::new();
    let mut protect = |rect, layer| {
        if let Some(rect) = intersection(rect, plan.bounds()) {
            // Hidden background chrome must not punch holes in a front window's band.
            protected.extend(
                visible_fragments(rect, layer, plan.drop_occlusions())
                    .into_iter()
                    .map(|rect| (layer, rect)),
            );
        }
    };
    for region in regions {
        if matches!(
            region.id().kind(),
            PresentationHitRegionKind::TabBody(_)
                | PresentationHitRegionKind::TabClose(_)
                | PresentationHitRegionKind::TabGroupDrag { .. }
                | PresentationHitRegionKind::TabStripControl(_)
                | PresentationHitRegionKind::ContainedTitle(_)
                | PresentationHitRegionKind::ContainedClose(_)
        ) {
            protect(region.hit().rect(), region.stack().layer());
        }
    }
    // Complete strip/title bounds also protect disabled and non-receiver chrome.
    for bar in plan.tab_bar_records() {
        protect(bar.bounds(), bar.layer());
    }
    for contained in plan.contained_records() {
        protect(contained.title_bounds(), contained.layer());
    }
    for anchor in plan.presentation_menu_anchor_records() {
        if let Some(bounds) = anchor.ready_bounds() {
            protect(bounds, anchor.layer());
        }
    }
    protected
}

fn splitter_band(
    draw: LogicalRect,
    axis: Axis,
    extent: f64,
    clip: LogicalRect,
) -> Option<LogicalRect> {
    let half = extent * 0.5;
    match axis {
        Axis::Horizontal => {
            let center = midpoint(draw.x(), draw.max().x());
            clipped_edges(center - half, draw.y(), center + half, draw.max().y(), clip)
        }
        Axis::Vertical => {
            let center = midpoint(draw.y(), draw.max().y());
            clipped_edges(draw.x(), center - half, draw.max().x(), center + half, clip)
        }
    }
}

fn junction_square(
    plan: &PresentationPlan,
    id: SplitterJunctionId,
    extent: f64,
    clip: LogicalRect,
) -> Option<LogicalRect> {
    let mut x = None;
    let mut y = None;
    for splitter in id.splitters() {
        let record = plan.splitter_record(splitter)?;
        let draw = record.draw_bounds();
        match record.axis() {
            Axis::Horizontal => x = Some(midpoint(draw.x(), draw.max().x())),
            Axis::Vertical => y = Some(midpoint(draw.y(), draw.max().y())),
        }
    }
    let (x, y) = (x?, y?);
    let half = extent * 0.5;
    clipped_edges(x - half, y - half, x + half, y + half, clip)
}

fn contained_partition(
    outer: LogicalRect,
    mouse_extent: f64,
    extent: f64,
    direction: ContainedResizeDirection,
    clip: LogicalRect,
) -> Option<LogicalRect> {
    // Expand around the original mouse band's center, not the clipped frame.
    let expansion = (extent - mouse_extent) * 0.5;
    let x0 = outer.x() - expansion;
    let y0 = outer.y() - expansion;
    let x1 = outer.max().x() + expansion;
    let y1 = outer.max().y() + expansion;
    // Midpoint limits preserve the existing disjoint eight-way small-window partition.
    let inset = midpoint(extent, mouse_extent);
    let center_x = midpoint(outer.x(), outer.max().x());
    let center_y = midpoint(outer.y(), outer.max().y());
    let left = (outer.x() + inset).min(center_x);
    let right = (outer.max().x() - inset).max(center_x);
    let top = (outer.y() + inset).min(center_y);
    let bottom = (outer.max().y() - inset).max(center_y);
    let (left, top, right, bottom) = match direction {
        ContainedResizeDirection::NorthWest => (x0, y0, left, top),
        ContainedResizeDirection::North => (left, y0, right, top),
        ContainedResizeDirection::NorthEast => (right, y0, x1, top),
        ContainedResizeDirection::East => (right, top, x1, bottom),
        ContainedResizeDirection::SouthEast => (right, bottom, x1, y1),
        ContainedResizeDirection::South => (left, bottom, right, y1),
        ContainedResizeDirection::SouthWest => (x0, bottom, left, y1),
        ContainedResizeDirection::West => (x0, top, left, bottom),
    };
    clipped_edges(left, top, right, bottom, clip)
}

fn midpoint(minimum: f64, maximum: f64) -> f64 {
    minimum * 0.5 + maximum * 0.5
}

fn clipped_edges(
    left: f64,
    top: f64,
    right: f64,
    bottom: f64,
    clip: LogicalRect,
) -> Option<LogicalRect> {
    let left = left.max(clip.x());
    let top = top.max(clip.y());
    let right = right.min(clip.max().x());
    let bottom = bottom.min(clip.max().y());
    if left >= right || top >= bottom {
        return None;
    }
    // Clip endpoints before constructing geometry, including very large finite extents.
    LogicalRect::from_min_max(
        LogicalPoint::new(left, top).ok()?,
        LogicalPoint::new(right, bottom).ok()?,
    )
    .ok()
}

fn intersection(rect: LogicalRect, clip: LogicalRect) -> Option<LogicalRect> {
    clipped_edges(rect.x(), rect.y(), rect.max().x(), rect.max().y(), clip)
}

fn visible_fragments(
    rect: LogicalRect,
    layer: SceneLayerKey,
    occlusions: &[DropOcclusionRecord],
) -> Vec<LogicalRect> {
    let mut fragments = vec![rect];
    for occlusion in occlusions {
        if occlusion.layer() > layer {
            subtract_from(&mut fragments, occlusion.region().rect());
        }
    }
    fragments
}

fn subtract_from(fragments: &mut Vec<LogicalRect>, cut: LogicalRect) {
    let mut remaining = Vec::new();
    for rect in fragments.drain(..) {
        let Some(overlap) = intersection(rect, cut) else {
            remaining.push(rect);
            continue;
        };
        for (left, top, right, bottom) in [
            (rect.x(), rect.y(), rect.max().x(), overlap.y()),
            (rect.x(), overlap.max().y(), rect.max().x(), rect.max().y()),
            (rect.x(), overlap.y(), overlap.x(), overlap.max().y()),
            (
                overlap.max().x(),
                overlap.y(),
                rect.max().x(),
                overlap.max().y(),
            ),
        ] {
            if let Some(fragment) = clipped_edges(left, top, right, bottom, rect) {
                remaining.push(fragment);
            }
        }
    }
    *fragments = remaining;
}
