//! Splitter rendering and current-pass pointer gestures.

use dockspace::model::{DockspaceAxis, RootId};
use egui::{CursorIcon, Sense};

use super::RenderContext;
use super::actions::gesture_phase;
use super::geometry::egui_rect;

pub(crate) fn paint_root(context: &mut RenderContext<'_, '_>, root: RootId) {
    for splitter in context
        .plan
        .splitters()
        .filter(|splitter| splitter.root() == root)
    {
        let Some(draw) = egui_rect(splitter.draw_bounds()) else {
            continue;
        };
        let Some(hit) = egui_rect(splitter.hit_bounds()) else {
            continue;
        };
        let id =
            context
                .ui
                .make_persistent_id((context.instance_id, "splitter", splitter.visual_id()));
        let sense = if splitter.operable() {
            Sense::drag()
        } else {
            Sense::hover()
        };
        let response = context.ui.interact(hit, id, sense);
        let emphasized = response.hovered() || response.dragged() || response.has_focus();
        context.ui.painter().rect_filled(
            draw,
            0.0,
            if emphasized {
                context.style.splitter_hover_color
            } else {
                context.style.splitter_color
            },
        );
        if !splitter.operable() {
            continue;
        }
        if response.hovered() || response.dragged() {
            context.ui.ctx().set_cursor_icon(match splitter.axis() {
                DockspaceAxis::Horizontal => CursorIcon::ResizeHorizontal,
                DockspaceAxis::Vertical => CursorIcon::ResizeVertical,
            });
        }
        if let Some(phase) = gesture_phase(&response)
            && let Some(action) = context
                .plan
                .prepare_splitter_gesture(splitter.visual_id(), phase)
        {
            context.actions.push(action);
        }
    }
}
