//! Non-interactive official-egui feedback for one core-owned drag source.

use dockspace::runtime::DockspaceDragSourceKind;
use egui::{CursorIcon, Rect, Stroke, StrokeKind, pos2};

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
                resource.galley.size().x + context.style.tab_horizontal_padding * 2.0
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
    context.ui.painter().rect(
        ghost,
        3.0,
        context.visuals.ghost_fill,
        Stroke::new(1.0, context.visuals.ghost_border_color),
        StrokeKind::Inside,
    );
    if decoration.kind() == DockspaceDragSourceKind::Root {
        let title_height = context.style.tab_bar_height.min(ghost.height());
        let title = Rect::from_min_size(ghost.min, egui::vec2(ghost.width(), title_height));
        context
            .ui
            .painter()
            .rect_filled(title, 3.0, context.visuals.floating_title_fill);
    }
    if let Some(resource) = resource {
        let text = ghost.shrink2(egui::vec2(context.style.tab_horizontal_padding, 0.0));
        let galley = resource.galley_for_width(context.ui, text.width());
        context.paint_tab_title(
            &context.ui.painter_at(text),
            pos2(text.min.x, text.center().y - galley.size().y * 0.5),
            galley,
            true,
            context.visuals.tab_active_text_color,
        );
    }
}
