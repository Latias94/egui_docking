//! Destination decoration consumes the same resolved target as the preview.

use dockspace::runtime::DockspaceDragPreview;
use egui::{CornerRadius, Stroke, UiBuilder};

use crate::{DropPreview, DropPreviewTab};

use super::{RenderContext, geometry};

pub(super) fn paint(
    context: &mut RenderContext<'_, '_, '_>,
    preview: DockspaceDragPreview<'_>,
    rect: egui::Rect,
) -> bool {
    let Some(target) = preview.target() else {
        return false;
    };
    let Some(target_rect) = geometry::egui_rect(target.bounds()) else {
        return false;
    };
    let bounds = context.ui.clip_rect().intersect(target_rect);
    let inset = 8.0;
    let label_rect = egui::Rect::from_min_max(
        egui::pos2(
            bounds.left() + inset,
            bounds.top() + context.style.tab_bar_height + inset,
        ),
        egui::pos2(bounds.right() - inset, bounds.bottom()),
    );
    let tabs = preview
        .tabs()
        .filter_map(|tab| {
            let resource = context.resources.item(tab.item())?;
            let selected = tab.selected();
            let text_color_override = if selected {
                context.style.visuals.tab_active_text_color
            } else {
                context.style.visuals.tab_text_color
            }
            .or(context.ui.visuals().override_text_color);
            let stroke = if selected {
                context.style.visuals.tab_active_stroke
            } else {
                context.style.visuals.tab_stroke
            }
            .unwrap_or_else(|| {
                if tab.is_payload() {
                    Stroke::new(1.0, context.visuals.drop_border_color)
                } else {
                    Stroke::NONE
                }
            });
            Some(DropPreviewTab {
                item: tab.item(),
                ordinal: tab.ordinal(),
                selected,
                is_payload: tab.is_payload(),
                rect: geometry::egui_rect(tab.full_bounds())?,
                visible_rect: tab.visible_bounds().and_then(geometry::egui_rect),
                title_rect: tab.text_layout_bounds().and_then(geometry::egui_rect),
                title_clip_rect: tab.text_bounds().and_then(geometry::egui_rect),
                title: resource.galley.clone(),
                fill: if selected {
                    context.visuals.tab_active_fill
                } else {
                    context.visuals.tab_fill
                },
                stroke,
                corner_radius: context.style.tab_corner_radius,
                text_color: if selected {
                    context.visuals.tab_active_text_color
                } else {
                    context.visuals.tab_text_color
                },
                text_color_override,
            })
        })
        .collect();
    let feedback = DropPreview {
        target,
        target_rect,
        rect,
        presentation_rect: {
            #[cfg(feature = "motion")]
            {
                super::motion::sample(context, super::motion::Effect::Drop, rect)
            }
            #[cfg(not(feature = "motion"))]
            {
                rect
            }
        },
        layout_projected: preview.is_layout_projected(),
        fill: context.visuals.drop_fill,
        stroke: Stroke::new(1.0, context.visuals.drop_border_color),
        target_stroke: Stroke::new(2.0, context.visuals.drop_border_color),
        corner_radius: CornerRadius::same(2),
        tab_bar_rect: preview
            .tab_bar()
            .and_then(|bar| geometry::egui_rect(bar.bounds())),
        tab_bar_fill: context.visuals.tab_bar_fill,
        tabs,
        label_rect,
        label_id: context.ui.make_persistent_id((
            context.instance_id,
            "drop-preview-hint",
            target.visual_id(),
        )),
    };
    let mut child = context.ui.new_child(
        UiBuilder::new()
            .id_salt((context.instance_id, "drop-preview"))
            .max_rect(context.ui.clip_rect()),
    );
    child.set_clip_rect(context.ui.clip_rect());
    context.panes.drop_preview_ui(&child, &feedback);
    true
}
