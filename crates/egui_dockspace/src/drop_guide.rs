//! Paint-only application decoration for core-owned docking-guide buttons.

use dockspace::runtime::DropAffordanceTargetPaintRecord;
pub use dockspace::runtime::{
    DockspaceDropDirection, DockspaceDropEligibility, DockspaceGuideScope,
};
use egui::{Color32, CornerRadius, Id, Rect, Stroke, StrokeKind, Ui, vec2};

use crate::RootId;
use crate::style::ResolvedDockVisuals;

const CUE_INSET: f32 = 5.0;
const CUE_CENTER_EXTENT: f32 = 5.0;
const CUE_EDGE_EXTENT: f32 = 4.0;
const DISABLED_FILL_OPACITY: f32 = 0.35;
const DISABLED_CUE_OPACITY: f32 = 0.5;

/// One non-interactive docking-guide button with its resolved appearance.
///
/// Customize this in [`crate::PaneView::drop_guide_ui`]. All rectangles use the
/// current egui viewport's logical coordinates. Painting must respect the Ui's
/// clip and stay on its layer; do not create widgets or change layout. Core
/// retains the receiver geometry, eligibility and active destination. Changing
/// these paint values does not change hit testing or the resulting docking move.
///
/// [`Self::id`] can key application-owned visual animation. Animate appearance
/// rather than moving the button away from its core-owned hit rectangle.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DropGuide {
    /// Stable adapter identity for this exact guide, including its docking instance.
    pub id: Id,
    /// Root containing this guide cluster.
    pub root: RootId,
    /// Whether the guide belongs to a pane's inner cluster or a root's outer edges.
    pub scope: DockspaceGuideScope,
    /// Merge or split direction represented by this button.
    pub direction: DockspaceDropDirection,
    /// Whether the current payload can commit to this destination.
    pub eligibility: DockspaceDropEligibility,
    /// Whether core resolved this exact button as active, including rejected targets.
    pub active: bool,
    /// Core-allocated visible button bounds.
    pub rect: Rect,
    /// Resolved button fill, including rejected-target opacity.
    pub fill: Color32,
    /// Outline painted inside the button.
    pub stroke: Stroke,
    /// Rounding of the button background and outline.
    pub corner_radius: CornerRadius,
    /// Pane outline used by the default directional cue.
    pub cue_rect: Rect,
    /// Center or edge emphasis within [`Self::cue_rect`].
    pub emphasis_rect: Rect,
    /// Resolved color of the directional cue.
    pub cue_color: Color32,
}

impl DropGuide {
    /// Paints the default button and directional cue without registering input.
    pub fn paint(&self, ui: &Ui) {
        self.paint_background(ui);
        self.paint_cue(ui);
    }

    /// Paints the background, leaving the interior available for custom icons.
    pub fn paint_background(&self, ui: &Ui) {
        ui.painter().rect(
            self.rect,
            self.corner_radius,
            self.fill,
            self.stroke,
            StrokeKind::Inside,
        );
    }

    /// Paints the directional cue independently of the button background.
    pub fn paint_cue(&self, ui: &Ui) {
        ui.painter().rect_stroke(
            self.cue_rect,
            0.0,
            Stroke::new(1.0, self.cue_color),
            StrokeKind::Inside,
        );
        ui.painter()
            .rect_filled(self.emphasis_rect, 0.0, self.cue_color);
    }
}

pub(crate) fn describe_guide_button(
    id: Id,
    root: RootId,
    scope: DockspaceGuideScope,
    target: DropAffordanceTargetPaintRecord<'_>,
    rect: Rect,
    visuals: ResolvedDockVisuals,
) -> DropGuide {
    let active = target.is_active();
    let eligible = target.eligibility() == DockspaceDropEligibility::Eligible;
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
    let (cue_rect, emphasis_rect) = describe_guide_cue(target.direction(), rect);
    DropGuide {
        id,
        root,
        scope,
        direction: target.direction(),
        eligibility: target.eligibility(),
        active,
        rect,
        fill,
        stroke: Stroke::new(1.0, outline_color),
        corner_radius: CornerRadius::same(4),
        cue_rect,
        emphasis_rect,
        cue_color,
    }
}

fn describe_guide_cue(direction: DockspaceDropDirection, button: Rect) -> (Rect, Rect) {
    let inset = CUE_INSET
        .min(0.5 * button.width())
        .min(0.5 * button.height());
    let pane = button.shrink(inset);
    let emphasis = match direction {
        DockspaceDropDirection::Center => Rect::from_center_size(
            pane.center(),
            vec2(
                CUE_CENTER_EXTENT.min(pane.width()),
                CUE_CENTER_EXTENT.min(pane.height()),
            ),
        ),
        DockspaceDropDirection::Left => Rect::from_min_max(
            pane.min,
            egui::pos2(pane.min.x + CUE_EDGE_EXTENT.min(pane.width()), pane.max.y),
        ),
        DockspaceDropDirection::Right => Rect::from_min_max(
            egui::pos2(pane.max.x - CUE_EDGE_EXTENT.min(pane.width()), pane.min.y),
            pane.max,
        ),
        DockspaceDropDirection::Top => Rect::from_min_max(
            pane.min,
            egui::pos2(pane.max.x, pane.min.y + CUE_EDGE_EXTENT.min(pane.height())),
        ),
        DockspaceDropDirection::Bottom => Rect::from_min_max(
            egui::pos2(pane.min.x, pane.max.y - CUE_EDGE_EXTENT.min(pane.height())),
            pane.max,
        ),
    };
    (pane, emphasis)
}
