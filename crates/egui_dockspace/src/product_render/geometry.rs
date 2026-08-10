//! Lossless-enough conversion at the egui/product geometry boundary.

use dockspace::geometry::{LogicalPoint, LogicalRect};
use egui::{Pos2, Rect, pos2, vec2};

pub(crate) fn logical_rect(rect: Rect) -> Result<LogicalRect, ()> {
    LogicalRect::new(
        f64::from(rect.min.x),
        f64::from(rect.min.y),
        f64::from(rect.width()),
        f64::from(rect.height()),
    )
    .map_err(|_| ())
}

pub(crate) fn egui_rect(rect: LogicalRect) -> Option<Rect> {
    Some(Rect::from_min_size(
        pos2(finite_f32(rect.x())?, finite_f32(rect.y())?),
        vec2(finite_f32(rect.width())?, finite_f32(rect.height())?),
    ))
}

fn finite_f32(value: f64) -> Option<f32> {
    if !value.is_finite() || value.abs() > f64::from(f32::MAX) {
        return None;
    }
    // Egui's coordinate domain is f32. The range check above makes the only
    // unavoidable loss here a precision reduction at the adapter boundary.
    #[allow(clippy::cast_possible_truncation)]
    Some(value as f32)
}

pub(crate) fn logical_point(point: Pos2) -> Option<LogicalPoint> {
    LogicalPoint::new(f64::from(point.x), f64::from(point.y)).ok()
}

pub(crate) fn accesskit_bounds(rect: Rect) -> egui::accesskit::Rect {
    egui::accesskit::Rect {
        x0: f64::from(rect.min.x),
        y0: f64::from(rect.min.y),
        x1: f64::from(rect.max.x),
        y1: f64::from(rect.max.y),
    }
}
