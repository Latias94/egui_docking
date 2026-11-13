//! Paint-only destination feedback for a core-owned docking preview.

use std::sync::Arc;

pub use dockspace::runtime::{
    DockspaceDropAction, DockspaceDropTargetFeedback, DockspaceDropTargetScope,
};
use egui::{Color32, CornerRadius, Galley, Id, Rect, Stroke, StrokeKind, Ui};

use crate::ItemId;

/// One future tab, with candidate order and geometry but no input authority.
///
/// These values can be customized alongside the rest of [`DropPreview`]. The
/// title retains rich-text spans; `text_color_override` replaces them explicitly.
/// Rectangles use the current egui viewport's logical coordinates. Selection
/// and order describe the pending move's result, not the published workspace.
#[derive(Clone, Debug)]
pub struct DropPreviewTab {
    /// Stable application identity of the future tab's pane.
    pub item: ItemId,
    /// Zero-based index in the complete future target group, before clipping.
    pub ordinal: usize,
    /// Whether this tab would be selected after the move.
    pub selected: bool,
    /// Whether this tab belongs to the dragged payload.
    pub is_payload: bool,
    /// Complete future tab allocation, including any scrolled-out portion.
    pub rect: Rect,
    /// Paintable intersection with the future strip; absent when hidden or too narrow.
    pub visible_rect: Option<Rect>,
    /// Future title allocation with padding and close-button spacing reserved.
    pub title_rect: Option<Rect>,
    /// Visible title clip, absent when the title has no visible allocation.
    pub title_clip_rect: Option<Rect>,
    /// Original shaped title; painting helpers elide it to the future allocation.
    pub title: Arc<Galley>,
    /// Background of this future tab.
    pub fill: Color32,
    /// Outline painted inside the complete tab allocation.
    pub stroke: Stroke,
    /// Rounding of the tab background and outline.
    pub corner_radius: CornerRadius,
    /// Fallback color for title spans without an explicit color.
    pub text_color: Color32,
    /// Optional replacement color for every title span, including rich text.
    pub text_color_override: Option<Color32>,
}

impl DropPreviewTab {
    /// Paints the visible background and title without creating a tab receiver.
    pub fn paint(&self, ui: &Ui) {
        self.paint_background(ui);
        if let Some(rect) = self.title_rect {
            self.paint_title(ui, rect);
        }
    }

    /// Paints the background and outline, clipped to [`Self::visible_rect`].
    pub fn paint_background(&self, ui: &Ui) {
        let Some(visible) = self.visible_rect else {
            return;
        };
        ui.painter()
            .with_clip_rect(ui.clip_rect().intersect(visible))
            .rect(
                self.rect,
                self.corner_radius,
                self.fill,
                self.stroke,
                StrokeKind::Inside,
            );
    }

    /// Paints the title elided to `rect`, respecting the future title clip.
    ///
    /// Pass a subrectangle of [`Self::title_rect`] to reserve room for an icon
    /// or badge. A fully clipped title produces no drawing.
    pub fn paint_title(&self, ui: &Ui, rect: Rect) {
        let Some(clip) = self.title_clip_rect else {
            return;
        };
        crate::tab::paint_title(
            ui,
            &self.title,
            rect,
            ui.clip_rect().intersect(clip),
            self.text_color,
            self.text_color_override,
        );
    }
}

/// The current destination and the area occupied after the pending move.
///
/// `target_rect` identifies the pane or workspace being targeted now. `rect`
/// shows the resulting layout, which can extend into neighboring panes when
/// moving the source removes its old column. Core owns both facts.
/// All rectangles use the current egui viewport's logical coordinates.
///
/// [`Self::paint`] draws the result and its future tabs. Customize this in
/// [`crate::PaneView::drop_preview_ui`] to add an outline, icon or optional
/// [`Self::paint_label`] hint. Do not create interactive widgets or alter layout.
#[derive(Clone, Debug)]
pub struct DropPreview {
    /// Core-resolved destination, scope, and action before the pending move.
    pub target: DockspaceDropTargetFeedback,
    /// Bounds of the current target, before removing or relocating the source.
    pub target_rect: Rect,
    /// Area occupied by the payload in the resulting candidate layout.
    pub rect: Rect,
    /// Animated decoration bounds; `rect` remains the canonical candidate area.
    pub presentation_rect: Rect,
    /// The candidate layout already draws the resulting area itself.
    ///
    /// When true, [`Self::paint_result`] skips its overlay to avoid drawing it twice.
    pub layout_projected: bool,
    /// Result-area overlay fill when the layout is not already projected.
    pub fill: Color32,
    /// Result-area outline when the layout is not already projected.
    pub stroke: Stroke,
    /// Optional current-target outline used only by [`Self::paint_target`].
    pub target_stroke: Stroke,
    /// Rounding of result and target outlines and the optional label background.
    pub corner_radius: CornerRadius,
    /// Future target strip, absent for edge splitting or hidden tab bars.
    pub tab_bar_rect: Option<Rect>,
    /// Background of the future target tab strip.
    pub tab_bar_fill: Color32,
    /// Complete future order, including members clipped out of the viewport.
    pub tabs: Vec<DropPreviewTab>,
    /// Maximum space available for a single-line destination hint.
    pub label_rect: Rect,
    pub(crate) label_id: Id,
}

impl DropPreview {
    /// Paints the resulting area and future tabs without extra annotations.
    pub fn paint(&self, ui: &Ui) {
        self.paint_tabs(ui);
        self.paint_result(ui);
    }

    /// Paints future tab shapes and titles; it never registers tab interactions.
    pub fn paint_tabs(&self, ui: &Ui) {
        if let Some(rect) = self.tab_bar_rect {
            ui.painter().rect_filled(rect, 0.0, self.tab_bar_fill);
        }
        for tab in &self.tabs {
            tab.paint(ui);
        }
    }

    /// Paints the resulting area unless the candidate layout already draws it.
    pub fn paint_result(&self, ui: &Ui) {
        if !self.layout_projected {
            ui.painter().rect(
                self.presentation_rect,
                self.corner_radius,
                self.fill,
                self.stroke,
                StrokeKind::Inside,
            );
        }
    }

    /// Optionally outlines the current destination, before source removal.
    pub fn paint_target(&self, ui: &Ui) {
        ui.painter().rect_stroke(
            self.target_rect,
            self.corner_radius,
            self.target_stroke,
            StrokeKind::Inside,
        );
    }

    /// Optionally paints an application-provided hint with passive accessibility.
    ///
    /// The default renderer adds no text. Applications can choose their own
    /// wording and localization; the full text remains accessible when elided.
    pub fn paint_label(&self, ui: &Ui, label: &str) {
        let available = self.label_rect.intersect(ui.clip_rect());
        let padding = egui::vec2(6.0, 4.0);
        if label.is_empty()
            || available.width() <= padding.x * 2.0
            || available.height() <= padding.y * 2.0
        {
            return;
        }
        let mut job = egui::text::LayoutJob::simple_singleline(
            label.to_owned(),
            egui::TextStyle::Button.resolve(ui.style()),
            ui.visuals().text_color(),
        );
        job.wrap.max_width = available.width() - padding.x * 2.0;
        job.wrap.max_rows = 1;
        job.wrap.break_anywhere = true;
        let galley = ui.fonts_mut(|fonts| fonts.layout_job(job));
        let rect =
            Rect::from_min_size(available.min, galley.size() + padding * 2.0).intersect(available);
        let painter = ui.painter().with_clip_rect(available);
        painter.rect(
            rect,
            self.corner_radius,
            ui.visuals().window_fill(),
            ui.visuals().window_stroke,
            StrokeKind::Inside,
        );
        painter.galley(rect.min + padding, galley, ui.visuals().text_color());
        ui.ctx().accesskit_node_builder(self.label_id, |node| {
            node.set_role(egui::accesskit::Role::Label);
            node.set_label(label);
            node.set_bounds(egui::accesskit::Rect {
                x0: f64::from(rect.min.x),
                y0: f64::from(rect.min.y),
                x1: f64::from(rect.max.x),
                y1: f64::from(rect.max.y),
            });
        });
    }
}
