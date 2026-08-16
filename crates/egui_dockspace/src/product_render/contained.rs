//! Contained-floating chrome and local pointer gestures.

use dockspace::runtime::{ContainedPaintRecord, ContainedResizeDirection};
use egui::{CursorIcon, Sense, Stroke, StrokeKind, pos2};

use super::RenderContext;
use super::actions::gesture_phase;
use super::geometry::egui_rect;
use super::schedule::RootPaintSchedule;

pub(crate) fn paint_background(
    context: &mut RenderContext<'_, '_>,
    contained: ContainedPaintRecord<'_>,
    root: &RootPaintSchedule<'_>,
) {
    let Some(outer) = egui_rect(contained.outer_bounds()) else {
        return;
    };
    let Some(title) = egui_rect(contained.title_bounds()) else {
        return;
    };
    context
        .ui
        .painter()
        .rect_filled(outer, 3.0, context.style.floating_fill);
    context
        .ui
        .painter()
        .rect_filled(title, 3.0, context.style.floating_title_fill);
    context.ui.painter().rect_stroke(
        outer,
        3.0,
        Stroke::new(
            context.style.floating_border_width,
            context.style.floating_border_color,
        ),
        StrokeKind::Inside,
    );

    // A contained surface is a foreground receiver even when its pane does not
    // create widgets. Register both egui capture lanes before the pane widgets
    // so actual foreground content can supersede the blocker while background
    // widgets remain unreachable through the floating surface.
    for (lane, sense) in [("click", Sense::click()), ("drag", Sense::drag())] {
        let id = context.ui.make_persistent_id((
            context.instance_id,
            "contained-window-blocker",
            contained.visual_id(),
            lane,
        ));
        let _ = context.interact_receiver(
            outer,
            id,
            sense,
            context.plan.receiver_for_contained_frame(contained),
        );
    }

    let label = root
        .panes()
        .next()
        .and_then(dockspace::runtime::PanePaintRecord::selected)
        .and_then(|item| context.resources.item(item))
        .map_or("Floating", |resource| resource.title.as_str());
    context.ui.painter().text(
        pos2(
            title.min.x + context.style.tab_horizontal_padding,
            title.center().y,
        ),
        egui::Align2::LEFT_CENTER,
        label,
        egui::TextStyle::Button.resolve(context.ui.style()),
        context.style.tab_active_text_color,
    );
}

pub(crate) fn paint_controls(
    context: &mut RenderContext<'_, '_>,
    contained: ContainedPaintRecord<'_>,
) {
    if let Some(title) = egui_rect(contained.title_drag_bounds()) {
        let id = context.ui.make_persistent_id((
            context.instance_id,
            "contained-title",
            contained.visual_id(),
        ));
        let response = context.interact_receiver(
            title,
            id,
            Sense::drag(),
            context.plan.receiver_for_contained_title(contained),
        );
        if response.hovered() || response.dragged() {
            context.ui.ctx().set_cursor_icon(if response.dragged() {
                CursorIcon::Grabbing
            } else {
                CursorIcon::Grab
            });
        }
        if context.pointer_authority.accepts_local_pointer_actions()
            && let Some(phase) = gesture_phase(&response)
            && let Some(action) = context
                .plan
                .prepare_contained_title_gesture(contained.floating(), phase)
        {
            context.push_preview_gesture_action(action);
        }
    }

    if let Some(close) = contained.close_bounds().and_then(egui_rect) {
        let id = context.ui.make_persistent_id((
            context.instance_id,
            "contained-close",
            contained.floating(),
        ));
        let response = context.interact_receiver(
            close,
            id,
            Sense::click(),
            context.plan.receiver_for_contained_close(contained),
        );
        let color = if response.hovered() {
            context.style.tab_active_text_color
        } else {
            context.style.tab_text_color
        };
        let inset = close.width().min(close.height()) * 0.28;
        let stroke = Stroke::new(1.5, color);
        context.ui.painter().line_segment(
            [
                close.left_top() + egui::vec2(inset, inset),
                close.right_bottom() - egui::vec2(inset, inset),
            ],
            stroke,
        );
        context.ui.painter().line_segment(
            [
                close.right_top() + egui::vec2(-inset, inset),
                close.left_bottom() + egui::vec2(inset, -inset),
            ],
            stroke,
        );
        if context.pointer_authority.accepts_local_pointer_actions()
            && response.clicked()
            && let Some(action) = context.plan.prepare_contained_close(contained.floating())
        {
            context.push_local_action(action);
        }
    }

    for resize in contained.resize() {
        let Some(bounds) = egui_rect(resize.hit_bounds()) else {
            continue;
        };
        let id = context.ui.make_persistent_id((
            context.instance_id,
            "contained-resize",
            contained.floating(),
            resize.direction(),
        ));
        let response = context.interact_receiver(
            bounds,
            id,
            Sense::drag(),
            context
                .plan
                .receiver_for_contained_resize(contained, resize),
        );
        if response.hovered() || response.dragged() {
            context
                .ui
                .ctx()
                .set_cursor_icon(resize_cursor(resize.direction()));
        }
        if context.pointer_authority.accepts_local_pointer_actions()
            && let Some(phase) = gesture_phase(&response)
            && let Some(action) = context.plan.prepare_contained_resize_gesture(
                contained.floating(),
                resize.direction(),
                phase,
            )
        {
            context.push_local_action(action);
        }
    }
}

const fn resize_cursor(direction: ContainedResizeDirection) -> CursorIcon {
    match direction {
        ContainedResizeDirection::North | ContainedResizeDirection::South => {
            CursorIcon::ResizeVertical
        }
        ContainedResizeDirection::East | ContainedResizeDirection::West => {
            CursorIcon::ResizeHorizontal
        }
        ContainedResizeDirection::NorthEast | ContainedResizeDirection::SouthWest => {
            CursorIcon::ResizeNeSw
        }
        ContainedResizeDirection::SouthEast | ContainedResizeDirection::NorthWest => {
            CursorIcon::ResizeNwSe
        }
    }
}
