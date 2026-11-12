//! Paint-only application decoration for a core-owned docking drag.

use std::sync::Arc;

pub use dockspace::runtime::DockspaceDragSourceKind;
use egui::{Color32, CornerRadius, Galley, Rect, Stroke, StrokeKind, Ui};

use crate::ItemId;

/// Non-interactive drag preview. Core retains the source, target and gesture.
///
/// A callback may change the appearance and reserve part of `title_rect` for
/// icons or badges. Painting stays within the supplied Ui clip; do not create
/// interactive widgets or change layout in this callback.
#[derive(Clone, Debug)]
pub struct DragPreview {
    pub kind: DockspaceDragSourceKind,
    pub item: Option<ItemId>,
    pub rect: Rect,
    pub header_rect: Option<Rect>,
    pub title_rect: Rect,
    pub title: Option<Arc<Galley>>,
    pub fill: Color32,
    pub stroke: Stroke,
    pub corner_radius: CornerRadius,
    pub header_fill: Color32,
    pub text_color: Color32,
    pub text_color_override: Option<Color32>,
}

impl DragPreview {
    pub fn paint(&self, ui: &Ui) {
        self.paint_background(ui);
        self.paint_title(ui, self.title_rect);
    }

    pub fn paint_background(&self, ui: &Ui) {
        ui.painter().rect(
            self.rect,
            self.corner_radius,
            self.fill,
            self.stroke,
            StrokeKind::Inside,
        );
        if let Some(header) = self.header_rect {
            let mut corners = self.corner_radius;
            corners.sw = 0;
            corners.se = 0;
            ui.painter().rect_filled(header, corners, self.header_fill);
        }
    }

    pub fn paint_title(&self, ui: &Ui, rect: Rect) {
        if let Some(title) = &self.title {
            crate::tab::paint_title(
                ui,
                title,
                rect.intersect(self.title_rect),
                ui.clip_rect(),
                self.text_color,
                self.text_color_override,
            );
        }
    }
}
