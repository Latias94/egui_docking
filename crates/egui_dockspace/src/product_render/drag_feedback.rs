//! Non-interactive official-egui feedback for one core-owned drag source.

use dockspace::runtime::DockspaceDragSourceKind;
use egui::{CursorIcon, Rect, Stroke, StrokeKind, pos2};

use super::RenderContext;
use super::geometry::egui_rect;

pub(super) fn paint(context: &mut RenderContext<'_, '_, '_>) {
    let Some(decoration) = context.plan.drag_decoration() else {
        return;
    };
    let Some(source) = egui_rect(decoration.source_bounds()) else {
        return;
    };
    let Some(pointer) = context.ui.input(|input| input.pointer.hover_pos()) else {
        return;
    };
    context.ui.ctx().set_cursor_icon(CursorIcon::Grabbing);
    let resource = decoration
        .ghost_item()
        .and_then(|item| context.resources.item(item))
        .cloned();
    let ghost_size = if decoration.kind() == DockspaceDragSourceKind::Item {
        source.size()
    } else {
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
    };
    let ghost = Rect::from_min_size(pointer + context.style.ghost_offset, ghost_size);
    context.ui.painter().rect(
        ghost,
        3.0,
        context.visuals.ghost_fill,
        Stroke::new(1.0, context.visuals.ghost_border_color),
        StrokeKind::Inside,
    );
    if let Some(resource) = resource {
        let text = ghost.shrink2(egui::vec2(context.style.tab_horizontal_padding, 0.0));
        context.ui.painter_at(text).galley_with_override_text_color(
            pos2(text.min.x, text.center().y - resource.galley.size().y * 0.5),
            resource.galley,
            context.visuals.tab_active_text_color,
        );
    }
}
