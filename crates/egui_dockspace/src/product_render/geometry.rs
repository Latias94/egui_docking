//! Lossless-enough conversion at the egui/product geometry boundary.

use dockspace::geometry::LogicalRect;
use egui::{Rect, emath::GuiRounding as _, pos2, vec2};

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

/// Fits child UI layout to egui's grid without expanding its semantic region.
/// Paint and receiver bounds retain the core's continuous coordinates.
pub(crate) fn inset_ui_rect(rect: Rect) -> Option<Rect> {
    let min = pos2(-(-rect.min.x).floor_ui(), -(-rect.min.y).floor_ui());
    let aligned = Rect::from_min_max(min, rect.max.floor_ui());
    (aligned.is_finite() && aligned.is_positive()).then_some(aligned)
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

pub(crate) fn accesskit_bounds(rect: Rect) -> egui::accesskit::Rect {
    egui::accesskit::Rect {
        x0: f64::from(rect.min.x),
        y0: f64::from(rect.min.y),
        x1: f64::from(rect.max.x),
        y1: f64::from(rect.max.y),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn child_layout_stays_inside_its_semantic_region_or_is_omitted() {
        for rect in [
            Rect::from_min_max(pos2(-100.1, -80.3), pos2(221.6, 167.6)),
            Rect::from_min_max(pos2(0.031, 0.001), pos2(0.064, 0.063)),
        ] {
            let aligned = inset_ui_rect(rect).unwrap();
            assert!(rect.contains_rect(aligned));
            assert_eq!(aligned, aligned.round_ui());
            assert!(aligned.min.x - rect.min.x < egui::emath::GUI_ROUNDING);
            assert!(aligned.min.y - rect.min.y < egui::emath::GUI_ROUNDING);
            assert!(rect.max.x - aligned.max.x < egui::emath::GUI_ROUNDING);
            assert!(rect.max.y - aligned.max.y < egui::emath::GUI_ROUNDING);
        }

        for rect in [
            Rect::from_min_max(pos2(0.001, 0.001), pos2(0.02, 0.02)),
            Rect::from_min_max(pos2(0.032, 0.001), pos2(0.063, 0.064)),
            Rect::from_min_max(pos2(0.0, 0.0), pos2(f32::INFINITY, 1.0)),
        ] {
            assert_eq!(inset_ui_rect(rect), None);
        }
    }
}
