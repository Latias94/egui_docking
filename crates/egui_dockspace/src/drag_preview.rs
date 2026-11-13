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
/// All rectangles use the current egui viewport's logical coordinates.
#[derive(Clone, Debug)]
pub struct DragPreview {
    /// Whether the payload is one item, a tabs group, a subtree, or a complete root.
    pub kind: DockspaceDragSourceKind,
    /// Representative title item, when the source has one; not the complete payload.
    pub item: Option<ItemId>,
    /// Complete ghost allocation, independent of the destination preview.
    pub rect: Rect,
    /// Optional title-bar allocation for a complete-root ghost.
    pub header_rect: Option<Rect>,
    /// Available title area after docking padding.
    pub title_rect: Rect,
    /// Original shaped title, absent when no representative title is available.
    pub title: Option<Arc<Galley>>,
    /// Fill of the ghost body.
    pub fill: Color32,
    /// Outline painted inside the ghost allocation.
    pub stroke: Stroke,
    /// Rounding of the body and the title bar's top corners.
    pub corner_radius: CornerRadius,
    /// Fill used when [`Self::header_rect`] is present.
    pub header_fill: Color32,
    /// Fallback color for title spans without an explicit color.
    pub text_color: Color32,
    /// Optional replacement color for every title span, including rich text.
    pub text_color_override: Option<Color32>,
}

impl DragPreview {
    /// Paints the body, optional title bar, and title without registering input.
    pub fn paint(&self, ui: &Ui) {
        self.paint_background(ui);
        self.paint_title(ui, self.title_rect);
    }

    /// Paints the body and optional title bar, leaving the title area available.
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

    /// Paints the title elided to `rect`, intersected with [`Self::title_rect`].
    ///
    /// Reserve space for icons or badges by passing a smaller rectangle.
    /// Painting also respects the supplied Ui's clip.
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
