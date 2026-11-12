//! Non-interactive official-egui feedback for one core-owned drag source.

use dockspace::runtime::DockspaceDragSourceKind;
use egui::{CornerRadius, CursorIcon, Rect, Stroke, UiBuilder};

use crate::DragPreview;

use super::RenderContext;
use super::geometry::egui_rect;

pub(super) fn paint(
    context: &mut RenderContext<'_, '_, '_>,
    decoration: dockspace::runtime::DockspaceDragDecoration<'_>,
) {
    context.ui.ctx().set_cursor_icon(CursorIcon::Grabbing);
    if decoration.source_is_projected() {
        return;
    }
    let Some(source) = egui_rect(decoration.source_bounds()) else {
        return;
    };
    let Some(pointer) = context.ui.input(|input| input.pointer.hover_pos()) else {
        return;
    };
    let resource = decoration
        .ghost_item()
        .and_then(|item| context.resources.item(item))
        .cloned();
    let projected_root = decoration.projected_root_bounds().and_then(egui_rect);
    let ghost_size = match decoration.kind() {
        DockspaceDragSourceKind::Item => source.size(),
        DockspaceDragSourceKind::Root => {
            projected_root.map_or_else(|| source.size(), |rect| rect.size())
        }
        DockspaceDragSourceKind::TabGroup | DockspaceDragSourceKind::Subtree => {
            let width = resource.as_ref().map_or(160.0, |resource| {
                resource.content_width + context.style.tab_horizontal_padding * 2.0
            });
            egui::vec2(
                width.clamp(96.0, 280.0),
                context
                    .style
                    .tab_bar_height
                    .max(context.ui.spacing().interact_size.y),
            )
        }
    };
    let ghost = projected_root
        .unwrap_or_else(|| Rect::from_min_size(pointer + context.style.ghost_offset, ghost_size));
    let header_rect = if decoration.kind() == DockspaceDragSourceKind::Root {
        let title_height = context.style.tab_bar_height.min(ghost.height());
        Some(Rect::from_min_size(
            ghost.min,
            egui::vec2(ghost.width(), title_height),
        ))
    } else {
        None
    };
    let preview = DragPreview {
        kind: decoration.kind(),
        item: decoration.ghost_item(),
        rect: ghost,
        header_rect,
        title_rect: header_rect
            .unwrap_or(ghost)
            .shrink2(egui::vec2(context.style.tab_horizontal_padding, 0.0)),
        title: resource.map(|resource| resource.galley),
        fill: context.visuals.ghost_fill,
        stroke: Stroke::new(1.0, context.visuals.ghost_border_color),
        corner_radius: CornerRadius::same(3),
        header_fill: context.visuals.floating_title_fill,
        text_color: context.visuals.tab_active_text_color,
        text_color_override: context
            .style
            .visuals
            .tab_active_text_color
            .or(context.ui.visuals().override_text_color),
    };
    let mut child = context.ui.new_child(
        UiBuilder::new()
            .id_salt((context.instance_id, "drag-preview"))
            .max_rect(ghost),
    );
    child.set_clip_rect(context.ui.clip_rect().intersect(ghost));
    context.panes.drag_preview_ui(&child, &preview);
}
