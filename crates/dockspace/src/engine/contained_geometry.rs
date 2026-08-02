//! Deterministic geometry constraints for contained floating presentations.

use crate::geometry::{LogicalPoint, LogicalRect, LogicalSize};
use crate::ids::SurfaceId;
use crate::intent::{
    ContainedHorizontalResizeEdge, ContainedPlacementUnavailable, ContainedResizeEdges,
    ContainedTransformKind, ContainedVerticalResizeEdge,
};
use crate::interaction::ActiveContainedTransform;
use crate::scene::ContainedResizeDirection;

pub(super) fn clamp_contained_rect(
    surface: SurfaceId,
    bounds: LogicalRect,
    requested: LogicalRect,
    minimum: LogicalSize,
) -> Result<LogicalRect, ContainedPlacementUnavailable> {
    let bounds_width = bounds.width();
    let bounds_height = bounds.height();
    let requested_width = requested.width();
    let requested_height = requested.height();
    if !bounds_width.is_finite()
        || !bounds_height.is_finite()
        || !requested_width.is_finite()
        || !requested_height.is_finite()
        || bounds_width <= 0.0
        || bounds_height <= 0.0
    {
        return Err(ContainedPlacementUnavailable::UnrepresentableGeometry { surface });
    }

    let width = requested_width.max(minimum.width()).min(bounds_width);
    let height = requested_height.max(minimum.height()).min(bounds_height);
    if width <= 0.0 || height <= 0.0 {
        return Err(ContainedPlacementUnavailable::UnrepresentableGeometry { surface });
    }
    let (min_x, max_x) = clamp_axis(bounds.x(), bounds.max().x(), requested.x(), width)
        .ok_or(ContainedPlacementUnavailable::UnrepresentableGeometry { surface })?;
    let (min_y, max_y) = clamp_axis(bounds.y(), bounds.max().y(), requested.y(), height)
        .ok_or(ContainedPlacementUnavailable::UnrepresentableGeometry { surface })?;
    let min = LogicalPoint::new(min_x, min_y)
        .map_err(|_| ContainedPlacementUnavailable::UnrepresentableGeometry { surface })?;
    let max = LogicalPoint::new(max_x, max_y)
        .map_err(|_| ContainedPlacementUnavailable::UnrepresentableGeometry { surface })?;
    let clamped = LogicalRect::from_min_max(min, max)
        .map_err(|_| ContainedPlacementUnavailable::UnrepresentableGeometry { surface })?;
    if clamped.width() <= 0.0 || clamped.height() <= 0.0 {
        return Err(ContainedPlacementUnavailable::UnrepresentableGeometry { surface });
    }
    Ok(clamped)
}

fn clamp_axis(
    bounds_min: f64,
    bounds_max: f64,
    requested_min: f64,
    extent: f64,
) -> Option<(f64, f64)> {
    let latest_min = bounds_max - extent;
    let (minimum, maximum) = if requested_min <= bounds_min {
        (bounds_min, bounds_min + extent)
    } else if requested_min >= latest_min {
        (latest_min, bounds_max)
    } else {
        (requested_min, requested_min + extent)
    };
    minimum
        .is_finite()
        .then_some(())
        .filter(|()| maximum.is_finite() && minimum <= maximum)
        .map(|()| (minimum, maximum))
}

pub(super) fn translated_contained_rect(
    source: LogicalRect,
    initial_pointer: LogicalPoint,
    current_pointer: LogicalPoint,
) -> Result<LogicalRect, ()> {
    let delta_x = current_pointer.x() - initial_pointer.x();
    let delta_y = current_pointer.y() - initial_pointer.y();
    if !delta_x.is_finite() || !delta_y.is_finite() {
        return Err(());
    }
    LogicalRect::new(
        source.x() + delta_x,
        source.y() + delta_y,
        source.width(),
        source.height(),
    )
    .map_err(|_| ())
}

#[derive(Clone, Copy)]
enum TransformAxisEdge {
    Minimum,
    Maximum,
}

pub(super) const fn contained_resize_edges(
    direction: ContainedResizeDirection,
) -> ContainedResizeEdges {
    match direction {
        ContainedResizeDirection::North => {
            ContainedResizeEdges::vertical(ContainedVerticalResizeEdge::Top)
        }
        ContainedResizeDirection::NorthEast => ContainedResizeEdges::corner(
            ContainedHorizontalResizeEdge::Right,
            ContainedVerticalResizeEdge::Top,
        ),
        ContainedResizeDirection::East => {
            ContainedResizeEdges::horizontal(ContainedHorizontalResizeEdge::Right)
        }
        ContainedResizeDirection::SouthEast => ContainedResizeEdges::corner(
            ContainedHorizontalResizeEdge::Right,
            ContainedVerticalResizeEdge::Bottom,
        ),
        ContainedResizeDirection::South => {
            ContainedResizeEdges::vertical(ContainedVerticalResizeEdge::Bottom)
        }
        ContainedResizeDirection::SouthWest => ContainedResizeEdges::corner(
            ContainedHorizontalResizeEdge::Left,
            ContainedVerticalResizeEdge::Bottom,
        ),
        ContainedResizeDirection::West => {
            ContainedResizeEdges::horizontal(ContainedHorizontalResizeEdge::Left)
        }
        ContainedResizeDirection::NorthWest => ContainedResizeEdges::corner(
            ContainedHorizontalResizeEdge::Left,
            ContainedVerticalResizeEdge::Top,
        ),
    }
}

pub(super) fn contained_transform_requested_rect(
    transform: &ActiveContainedTransform,
    current_pointer: LogicalPoint,
    bounds: LogicalRect,
) -> Result<LogicalRect, ()> {
    let delta_x = current_pointer.x() - transform.initial_pointer.x();
    let delta_y = current_pointer.y() - transform.initial_pointer.y();
    if !delta_x.is_finite() || !delta_y.is_finite() {
        return Err(());
    }
    match transform.kind {
        ContainedTransformKind::Move => LogicalRect::new(
            transform.source_rect.x() + delta_x,
            transform.source_rect.y() + delta_y,
            transform.source_rect.width(),
            transform.source_rect.height(),
        )
        .map_err(|_| ()),
        ContainedTransformKind::Resize(edges) => {
            let horizontal = match edges.horizontal_edge() {
                Some(ContainedHorizontalResizeEdge::Left) => Some(TransformAxisEdge::Minimum),
                Some(ContainedHorizontalResizeEdge::Right) => Some(TransformAxisEdge::Maximum),
                None => None,
            };
            let vertical = match edges.vertical_edge() {
                Some(ContainedVerticalResizeEdge::Top) => Some(TransformAxisEdge::Minimum),
                Some(ContainedVerticalResizeEdge::Bottom) => Some(TransformAxisEdge::Maximum),
                None => None,
            };
            let (min_x, max_x) = contained_resize_axis(
                transform.source_rect.x(),
                transform.source_rect.max().x(),
                bounds.x(),
                bounds.max().x(),
                transform.minimum_size.width(),
                delta_x,
                horizontal,
            )
            .ok_or(())?;
            let (min_y, max_y) = contained_resize_axis(
                transform.source_rect.y(),
                transform.source_rect.max().y(),
                bounds.y(),
                bounds.max().y(),
                transform.minimum_size.height(),
                delta_y,
                vertical,
            )
            .ok_or(())?;
            let min = LogicalPoint::new(min_x, min_y).map_err(|_| ())?;
            let max = LogicalPoint::new(max_x, max_y).map_err(|_| ())?;
            LogicalRect::from_min_max(min, max).map_err(|_| ())
        }
    }
}

pub(super) fn clamp_moved_contained_rect(
    bounds: LogicalRect,
    requested: LogicalRect,
) -> Result<LogicalRect, ()> {
    let x = clamp_translated_axis(
        bounds.x(),
        bounds.max().x(),
        requested.x(),
        requested.width(),
    )
    .ok_or(())?;
    let y = clamp_translated_axis(
        bounds.y(),
        bounds.max().y(),
        requested.y(),
        requested.height(),
    )
    .ok_or(())?;
    LogicalRect::new(x, y, requested.width(), requested.height()).map_err(|_| ())
}

fn clamp_translated_axis(
    bounds_min: f64,
    bounds_max: f64,
    requested_min: f64,
    extent: f64,
) -> Option<f64> {
    if !bounds_min.is_finite()
        || !bounds_max.is_finite()
        || !requested_min.is_finite()
        || !extent.is_finite()
        || bounds_max <= bounds_min
        || extent <= 0.0
    {
        return None;
    }
    let fit_max = bounds_max - extent;
    let (minimum, maximum) = if extent <= bounds_max - bounds_min {
        (bounds_min, fit_max)
    } else {
        (fit_max, bounds_min)
    };
    Some(requested_min.clamp(minimum, maximum))
}

fn contained_resize_axis(
    source_min: f64,
    source_max: f64,
    bounds_min: f64,
    bounds_max: f64,
    minimum_extent: f64,
    delta: f64,
    moving_edge: Option<TransformAxisEdge>,
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
    match moving_edge {
        None => Some((source_min, source_max)),
        Some(TransformAxisEdge::Minimum) => {
            let latest_min = source_max - minimum_extent;
            if bounds_min > latest_min {
                return None;
            }
            let requested = source_min + delta;
            requested
                .is_finite()
                .then(|| requested.clamp(bounds_min, latest_min))
                .map(|minimum| (minimum, source_max))
        }
        Some(TransformAxisEdge::Maximum) => {
            let earliest_max = source_min + minimum_extent;
            if earliest_max > bounds_max {
                return None;
            }
            let requested = source_max + delta;
            requested
                .is_finite()
                .then(|| requested.clamp(earliest_max, bounds_max))
                .map(|maximum| (source_min, maximum))
        }
    }
}
