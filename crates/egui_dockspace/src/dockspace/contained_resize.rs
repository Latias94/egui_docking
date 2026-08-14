//! Pure geometry for semantic contained-floating resize requests.

use dockspace::backend::ids::SurfaceId;
use dockspace::backend::intent::ContainedPlacementUnavailable;
use dockspace::geometry::{LogicalPoint, LogicalRect, LogicalSize};

use crate::renderer::ContainedResizeEdge;

pub(super) fn requested_contained_resize_rect(
    surface: SurfaceId,
    source: LogicalRect,
    bounds: LogicalRect,
    minimum: LogicalSize,
    edge: ContainedResizeEdge,
    delta: f64,
) -> Result<LogicalRect, ContainedPlacementUnavailable> {
    let horizontal_edge = match edge {
        ContainedResizeEdge::Left => Some(false),
        ContainedResizeEdge::Right => Some(true),
        ContainedResizeEdge::Top | ContainedResizeEdge::Bottom => None,
    };
    let vertical_edge = match edge {
        ContainedResizeEdge::Top => Some(false),
        ContainedResizeEdge::Bottom => Some(true),
        ContainedResizeEdge::Right | ContainedResizeEdge::Left => None,
    };
    let (min_x, max_x) = requested_contained_resize_axis(
        source.x(),
        source.max().x(),
        bounds.x(),
        bounds.max().x(),
        minimum.width(),
        delta,
        horizontal_edge,
    )
    .ok_or(ContainedPlacementUnavailable::UnrepresentableGeometry { surface })?;
    let (min_y, max_y) = requested_contained_resize_axis(
        source.y(),
        source.max().y(),
        bounds.y(),
        bounds.max().y(),
        minimum.height(),
        delta,
        vertical_edge,
    )
    .ok_or(ContainedPlacementUnavailable::UnrepresentableGeometry { surface })?;
    let min = LogicalPoint::new(min_x, min_y)
        .map_err(|_| ContainedPlacementUnavailable::UnrepresentableGeometry { surface })?;
    let max = LogicalPoint::new(max_x, max_y)
        .map_err(|_| ContainedPlacementUnavailable::UnrepresentableGeometry { surface })?;
    LogicalRect::from_min_max(min, max)
        .map_err(|_| ContainedPlacementUnavailable::UnrepresentableGeometry { surface })
}

/// Converts a one-dimensional semantic step into an explicit requested edge.
///
/// `moving_max` is `None` for an unchanged axis, `Some(false)` for its minimum
/// edge, and `Some(true)` for its maximum edge. The core still produces and
/// validates the final placement proof; this conversion only preserves the
/// opposite anchor expected from a splitter-style action.
fn requested_contained_resize_axis(
    source_min: f64,
    source_max: f64,
    bounds_min: f64,
    bounds_max: f64,
    minimum_extent: f64,
    delta: f64,
    moving_max: Option<bool>,
) -> Option<(f64, f64)> {
    if !source_min.is_finite()
        || !source_max.is_finite()
        || !bounds_min.is_finite()
        || !bounds_max.is_finite()
        || !minimum_extent.is_finite()
        || !delta.is_finite()
    {
        return None;
    }
    match moving_max {
        None => Some((source_min, source_max)),
        Some(false) => {
            let latest_min = source_max - minimum_extent;
            (bounds_min <= latest_min)
                .then_some(source_min + delta)
                .filter(|requested| requested.is_finite())
                .map(|requested| (requested.clamp(bounds_min, latest_min), source_max))
        }
        Some(true) => {
            let earliest_max = source_min + minimum_extent;
            (earliest_max <= bounds_max)
                .then_some(source_max + delta)
                .filter(|requested| requested.is_finite())
                .map(|requested| (source_min, requested.clamp(earliest_max, bounds_max)))
        }
    }
}
