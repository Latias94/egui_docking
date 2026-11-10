//! Deterministic geometry constraints for contained floating presentations.

use crate::geometry::{LogicalPoint, LogicalRect, LogicalSize};
use crate::ids::SurfaceId;
use crate::intent::{
    ContainedHorizontalResizeEdge, ContainedPlacementUnavailable, ContainedResizeEdges,
    ContainedTransformKind, ContainedVerticalResizeEdge,
};
use crate::interaction::{ActiveContainedTransform, ManipulationPositionKey};
use crate::pointer_journal::{DesktopRouteValidation, PointerProviderScope, ValidatedDesktopRoute};
use crate::scene::ContainedResizeDirection;

pub(super) fn clamp_contained_rect(
    surface: SurfaceId,
    bounds: LogicalRect,
    requested: LogicalRect,
    minimum: LogicalSize,
) -> Result<LogicalRect, ContainedPlacementUnavailable> {
    solve_contained_geometry(
        bounds,
        ContainedGeometryRequest::Placement { requested, minimum },
    )
    .map_err(|()| ContainedPlacementUnavailable::UnrepresentableGeometry { surface })
}

// Placement may fit a new presentation to its destination. Movement preserves
// the existing size, while resize changes only its explicitly selected edges.
enum ContainedGeometryRequest {
    Placement {
        requested: LogicalRect,
        minimum: LogicalSize,
    },
    Move {
        requested: LogicalRect,
    },
    Resize {
        source: LogicalRect,
        minimum: LogicalSize,
        edges: ContainedResizeEdges,
        delta_x: f64,
        delta_y: f64,
    },
}

fn solve_contained_geometry(
    bounds: LogicalRect,
    request: ContainedGeometryRequest,
) -> Result<LogicalRect, ()> {
    match request {
        ContainedGeometryRequest::Placement { requested, minimum } => {
            placed_contained_rect(bounds, requested, minimum)
        }
        ContainedGeometryRequest::Move { requested } => moved_contained_rect(bounds, requested),
        ContainedGeometryRequest::Resize {
            source,
            minimum,
            edges,
            delta_x,
            delta_y,
        } => contained_resized_rect(source, bounds, minimum, edges, delta_x, delta_y),
    }
}

fn placed_contained_rect(
    bounds: LogicalRect,
    requested: LogicalRect,
    minimum: LogicalSize,
) -> Result<LogicalRect, ()> {
    let bounds_width = bounds.width();
    let bounds_height = bounds.height();
    let requested_width = requested.width();
    let requested_height = requested.height();
    if bounds_width <= 0.0 || bounds_height <= 0.0 {
        return Err(());
    }

    let width = requested_width.max(minimum.width()).min(bounds_width);
    let height = requested_height.max(minimum.height()).min(bounds_height);
    if width <= 0.0 || height <= 0.0 {
        return Err(());
    }
    let (min_x, max_x) = clamp_axis(bounds.x(), bounds.max().x(), requested.x(), width).ok_or(())?;
    let (min_y, max_y) =
        clamp_axis(bounds.y(), bounds.max().y(), requested.y(), height).ok_or(())?;
    let min = LogicalPoint::new(min_x, min_y).map_err(|_| ())?;
    let max = LogicalPoint::new(max_x, max_y).map_err(|_| ())?;
    let clamped = LogicalRect::from_min_max(min, max).map_err(|_| ())?;
    if clamped.width() <= 0.0 || clamped.height() <= 0.0 {
        return Err(());
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

/// Extracts only the already validated manipulation lane of a drag position.
pub(super) fn contained_manipulation_point(
    position: ManipulationPositionKey,
    surface: SurfaceId,
) -> Option<LogicalPoint> {
    use crate::intent::Authority;

    match position {
        ManipulationPositionKey::JournalHost { manipulation, .. } => manipulation
            .filter(|route| route.surface() == surface)
            .map(|route| route.position()),
        ManipulationPositionKey::JournalNativeLocal { manipulation, .. } => manipulation
            .filter(|route| route.binding().surface() == surface)
            .map(|route| route.surface_position()),
        ManipulationPositionKey::JournalSurface {
            scope: PointerProviderScope::SurfaceLocal(scope),
            position: Authority::Known(point),
        } if scope.surface() == surface => Some(point),
        ManipulationPositionKey::JournalDesktop(DesktopRouteValidation::Known(
            ValidatedDesktopRoute::Dock(route),
        )) if route.binding().surface() == surface => Some(route.surface_position()),
        _ => None,
    }
}

pub(super) fn contained_move_geometry(
    bounds: LogicalRect,
    source: LogicalRect,
    initial_pointer: LogicalPoint,
    current_pointer: LogicalPoint,
) -> Result<(LogicalRect, LogicalRect), ()> {
    let requested = translated_contained_rect(source, initial_pointer, current_pointer)?;
    let rect = solve_contained_geometry(bounds, ContainedGeometryRequest::Move { requested })?;
    Ok((requested, rect))
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
        ContainedTransformKind::Move => translated_contained_rect(
            transform.source_rect,
            transform.initial_pointer,
            current_pointer,
        ),
        ContainedTransformKind::Resize(edges) => solve_contained_geometry(
            bounds,
            ContainedGeometryRequest::Resize {
                source: transform.source_rect,
                minimum: transform.minimum_size,
                edges,
                delta_x,
                delta_y,
            },
        ),
    }
}

pub(super) fn cardinal_contained_resize_requested_rect(
    source: LogicalRect,
    bounds: LogicalRect,
    minimum: LogicalSize,
    direction: ContainedResizeDirection,
    delta: f64,
) -> Result<LogicalRect, ()> {
    let (delta_x, delta_y) = match direction {
        ContainedResizeDirection::North | ContainedResizeDirection::South => (0.0, delta),
        ContainedResizeDirection::East | ContainedResizeDirection::West => (delta, 0.0),
        ContainedResizeDirection::NorthEast
        | ContainedResizeDirection::SouthEast
        | ContainedResizeDirection::SouthWest
        | ContainedResizeDirection::NorthWest => return Err(()),
    };
    solve_contained_geometry(
        bounds,
        ContainedGeometryRequest::Resize {
            source,
            minimum,
            edges: contained_resize_edges(direction),
            delta_x,
            delta_y,
        },
    )
}

fn contained_resized_rect(
    source: LogicalRect,
    bounds: LogicalRect,
    minimum: LogicalSize,
    edges: ContainedResizeEdges,
    delta_x: f64,
    delta_y: f64,
) -> Result<LogicalRect, ()> {
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
        source.x(),
        source.max().x(),
        bounds.x(),
        bounds.max().x(),
        minimum.width(),
        delta_x,
        horizontal,
    )
    .ok_or(())?;
    let (min_y, max_y) = contained_resize_axis(
        source.y(),
        source.max().y(),
        bounds.y(),
        bounds.max().y(),
        minimum.height(),
        delta_y,
        vertical,
    )
    .ok_or(())?;
    let min = LogicalPoint::new(min_x, min_y).map_err(|_| ())?;
    let max = LogicalPoint::new(max_x, max_y).map_err(|_| ())?;
    LogicalRect::from_min_max(min, max).map_err(|_| ())
}

pub(super) fn clamp_moved_contained_rect(
    bounds: LogicalRect,
    requested: LogicalRect,
) -> Result<LogicalRect, ()> {
    solve_contained_geometry(bounds, ContainedGeometryRequest::Move { requested })
}

fn moved_contained_rect(bounds: LogicalRect, requested: LogicalRect) -> Result<LogicalRect, ()> {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::intent::Authority;

    fn rect(x: f64, y: f64, width: f64, height: f64) -> LogicalRect {
        LogicalRect::new(x, y, width, height).unwrap()
    }

    #[test]
    fn movement_preserves_oversized_dimensions_while_placement_fits_the_destination() {
        let bounds = rect(0.0, 0.0, 960.0, 720.0);
        let source = rect(0.0, 0.0, 1000.0, 800.0);
        let press = LogicalPoint::new(200.0, 20.0).unwrap();
        for (dx, dy, expected) in [
            (30.0, 20.0, source),
            (-30.0, -20.0, rect(-30.0, -20.0, 1000.0, 800.0)),
            (-100.0, -100.0, rect(-40.0, -80.0, 1000.0, 800.0)),
        ] {
            let moved = LogicalPoint::new(press.x() + dx, press.y() + dy).unwrap();
            let (requested, actual) =
                contained_move_geometry(bounds, source, press, moved).unwrap();
            assert_eq!(actual, expected);
            assert_eq!(actual.width(), source.width());
            assert_eq!(actual.height(), source.height());
            assert_eq!(
                clamp_contained_rect(
                    SurfaceId::new(1),
                    bounds,
                    requested,
                    LogicalSize::new(64.0, 48.0).unwrap(),
                )
                .unwrap(),
                bounds,
            );
        }
    }

    #[test]
    fn every_resize_direction_keeps_its_opposite_edges_fixed() {
        use ContainedResizeDirection as Direction;

        let bounds = rect(0.0, 0.0, 960.0, 720.0);
        let source = rect(100.0, 100.0, 300.0, 240.0);
        let minimum = LogicalSize::new(120.0, 80.0).unwrap();
        for (direction, expected) in [
            (Direction::North, rect(100.0, 115.0, 300.0, 225.0)),
            (Direction::NorthEast, rect(100.0, 115.0, 320.0, 225.0)),
            (Direction::East, rect(100.0, 100.0, 320.0, 240.0)),
            (Direction::SouthEast, rect(100.0, 100.0, 320.0, 255.0)),
            (Direction::South, rect(100.0, 100.0, 300.0, 255.0)),
            (Direction::SouthWest, rect(120.0, 100.0, 280.0, 255.0)),
            (Direction::West, rect(120.0, 100.0, 280.0, 240.0)),
            (Direction::NorthWest, rect(120.0, 115.0, 280.0, 225.0)),
        ] {
            assert_eq!(
                solve_contained_geometry(
                    bounds,
                    ContainedGeometryRequest::Resize {
                        source,
                        minimum,
                        edges: contained_resize_edges(direction),
                        delta_x: 20.0,
                        delta_y: 15.0,
                    },
                ),
                Ok(expected),
                "{direction:?}",
            );
        }

        let oversized = rect(0.0, 100.0, 1000.0, 800.0);
        assert_eq!(
            cardinal_contained_resize_requested_rect(
                oversized,
                bounds,
                minimum,
                Direction::East,
                -50.0,
            ),
            Ok(rect(0.0, 100.0, 950.0, 800.0)),
            "horizontal resize must preserve an oversized vertical extent",
        );
    }

    #[test]
    fn unavailable_manipulation_and_foreign_desktop_routes_cannot_move_a_source() {
        let surface = SurfaceId::new(1);
        for position in [
            ManipulationPositionKey::JournalHost {
                manipulation: None,
                hover: None,
            },
            ManipulationPositionKey::JournalNativeLocal {
                manipulation: None,
                hover: None,
            },
            ManipulationPositionKey::JournalDesktop(DesktopRouteValidation::Known(
                ValidatedDesktopRoute::Foreign {
                    position: Authority::Known(
                        crate::geometry::PhysicalPoint::new(50.0, 50.0).unwrap(),
                    ),
                },
            )),
        ] {
            assert_eq!(contained_manipulation_point(position, surface), None);
        }
    }
}
