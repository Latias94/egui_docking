//! Paint-only destination feedback for a core-owned docking preview.

use std::sync::Arc;

pub use dockspace::runtime::{
    DockspaceDropAction, DockspaceDropTargetFeedback, DockspaceDropTargetScope,
};
use egui::{Color32, CornerRadius, Galley, Id, Rect, Stroke, StrokeKind, Ui};

use crate::ItemId;

/// One future tab, with committed order and geometry but no input authority.
///
/// These values can be customized alongside the rest of [`DropPreview`]. The
/// title retains rich-text spans; `text_color_override` replaces them explicitly.
#[derive(Clone, Debug)]
pub struct DropPreviewTab {
    pub item: ItemId,
    pub ordinal: usize,
    pub selected: bool,
    pub is_payload: bool,
    pub rect: Rect,
    pub visible_rect: Option<Rect>,
    pub title_rect: Option<Rect>,
    pub title_clip_rect: Option<Rect>,
    pub title: Arc<Galley>,
    pub fill: Color32,
    pub stroke: Stroke,
    pub corner_radius: CornerRadius,
    pub text_color: Color32,
    pub text_color_override: Option<Color32>,
}

impl DropPreviewTab {
    pub fn paint(&self, ui: &Ui) {
        self.paint_background(ui);
        if let Some(rect) = self.title_rect {
            self.paint_title(ui, rect);
        }
    }

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
///
/// [`Self::paint`] draws the result and its future tabs. Customize this in
/// [`crate::PaneView::drop_preview_ui`] to add an outline, icon or optional
/// [`Self::paint_label`] hint. Do not create interactive widgets or alter layout.
#[derive(Clone, Debug)]
pub struct DropPreview {
    pub target: DockspaceDropTargetFeedback,
    pub target_rect: Rect,
    pub rect: Rect,
    /// The candidate layout already draws the resulting area itself.
    pub layout_projected: bool,
    pub fill: Color32,
    pub stroke: Stroke,
    pub target_stroke: Stroke,
    pub corner_radius: CornerRadius,
    /// Future target strip, absent for edge splitting or hidden tab bars.
    pub tab_bar_rect: Option<Rect>,
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

    pub fn paint_result(&self, ui: &Ui) {
        if !self.layout_projected {
            ui.painter().rect(
                self.rect,
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
