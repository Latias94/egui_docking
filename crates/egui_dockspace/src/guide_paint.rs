//! Shared pure drawing primitives for docking-guide buttons.

use egui::{Color32, Painter, Rect, Stroke, StrokeKind, vec2};

use crate::style::ResolvedDockVisuals;

const BUTTON_CORNER_RADIUS: f32 = 4.0;
const CUE_INSET: f32 = 5.0;
const CUE_CENTER_EXTENT: f32 = 5.0;
const CUE_EDGE_EXTENT: f32 = 4.0;
pub(crate) const DISABLED_FILL_OPACITY: f32 = 0.35;
const DISABLED_CUE_OPACITY: f32 = 0.5;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum GuideCueDirection {
    Center,
    Left,
    Right,
    Top,
    Bottom,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct GuideButtonPaint {
    pub(crate) draw: Rect,
    pub(crate) fill: Color32,
    pub(crate) outline: Stroke,
    pub(crate) cue: GuideCuePaint,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct GuideCuePaint {
    pub(crate) pane: Rect,
    pub(crate) emphasis: Rect,
    pub(crate) color: Color32,
}

pub(crate) fn describe_guide_button(
    draw: Rect,
    direction: GuideCueDirection,
    active: bool,
    eligible: bool,
    visuals: ResolvedDockVisuals,
) -> GuideButtonPaint {
    let base_fill = if active {
        visuals.drop_guide_active_fill
    } else {
        visuals.drop_guide_fill
    };
    let base_outline = if active {
        visuals.drop_guide_active_border_color
    } else {
        visuals.drop_guide_border_color
    };
    let fill = if eligible {
        base_fill
    } else {
        base_fill.gamma_multiply(DISABLED_FILL_OPACITY)
    };
    // An exact disabled hit keeps the active outline so rejection stays legible.
    let outline_color = if eligible || active {
        base_outline
    } else {
        base_outline.gamma_multiply(DISABLED_CUE_OPACITY)
    };
    let cue_color = if eligible {
        base_outline
    } else {
        base_outline.gamma_multiply(DISABLED_CUE_OPACITY)
    };
    GuideButtonPaint {
        draw,
        fill,
        outline: Stroke::new(1.0, outline_color),
        cue: describe_guide_cue(direction, draw, cue_color),
    }
}

pub(crate) fn describe_guide_cue(
    direction: GuideCueDirection,
    button: Rect,
    color: Color32,
) -> GuideCuePaint {
    let inset = CUE_INSET
        .min(0.5 * button.width())
        .min(0.5 * button.height());
    let pane = button.shrink(inset);
    let emphasis = match direction {
        GuideCueDirection::Center => Rect::from_center_size(
            pane.center(),
            vec2(
                CUE_CENTER_EXTENT.min(pane.width()),
                CUE_CENTER_EXTENT.min(pane.height()),
            ),
        ),
        GuideCueDirection::Left => Rect::from_min_max(
            pane.min,
            egui::pos2(pane.min.x + CUE_EDGE_EXTENT.min(pane.width()), pane.max.y),
        ),
        GuideCueDirection::Right => Rect::from_min_max(
            egui::pos2(pane.max.x - CUE_EDGE_EXTENT.min(pane.width()), pane.min.y),
            pane.max,
        ),
        GuideCueDirection::Top => Rect::from_min_max(
            pane.min,
            egui::pos2(pane.max.x, pane.min.y + CUE_EDGE_EXTENT.min(pane.height())),
        ),
        GuideCueDirection::Bottom => Rect::from_min_max(
            egui::pos2(pane.min.x, pane.max.y - CUE_EDGE_EXTENT.min(pane.height())),
            pane.max,
        ),
    };
    GuideCuePaint {
        pane,
        emphasis,
        color,
    }
}

pub(crate) fn paint_guide_button(painter: &Painter, button: GuideButtonPaint) {
    painter.rect(
        button.draw,
        BUTTON_CORNER_RADIUS,
        button.fill,
        button.outline,
        StrokeKind::Inside,
    );
    painter.rect_stroke(
        button.cue.pane,
        0.0,
        Stroke::new(1.0, button.cue.color),
        StrokeKind::Inside,
    );
    painter.rect_filled(button.cue.emphasis, 0.0, button.cue.color);
}
