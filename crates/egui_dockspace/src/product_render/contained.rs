//! Contained-floating chrome and local pointer gestures.

use dockspace::runtime::{
    ContainedPaintRecord, ContainedResizeDirection, SurfaceContainedResizeAdjustment,
};
use egui::accesskit::{Action, Orientation, Role};
use egui::{CursorIcon, EventFilter, Key, Sense, Stroke, StrokeKind, pos2};

use super::RenderContext;
use super::actions::{button_activated, gesture_phase};
use super::geometry::{accesskit_bounds, egui_rect};
use super::schedule::RootPaintSchedule;

pub(crate) fn paint_background(
    context: &mut RenderContext<'_, '_, '_>,
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

    let scroll_id = context.ui.make_persistent_id((
        context.instance_id,
        "contained-window-scroll-blocker",
        contained.visual_id(),
    ));
    let scroll_receiver = context.plan.receiver_for_contained_frame(contained);
    context.register_scroll_receiver(scroll_id, scroll_receiver);

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

    let label = title_label(context, root);
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
    context: &mut RenderContext<'_, '_, '_>,
    contained: ContainedPaintRecord<'_>,
    root: &RootPaintSchedule<'_>,
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
            context
                .ui
                .is_enabled()
                .then(|| context.plan.receiver_for_contained_title(contained))
                .flatten(),
        );
        let label = title_label(context, root);
        context.ui.ctx().accesskit_node_builder(id, |node| {
            node.set_role(Role::TitleBar);
            node.set_bounds(accesskit_bounds(title));
            node.set_label(label);
        });
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
            context
                .ui
                .is_enabled()
                .then(|| context.plan.receiver_for_contained_close(contained))
                .flatten(),
        );
        let label = title_label(context, root);
        context.ui.ctx().accesskit_node_builder(id, |node| {
            node.set_role(Role::Button);
            node.set_bounds(accesskit_bounds(close));
            node.set_label(format!("Close floating {label}"));
        });
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
        if button_activated(context.ui, &response, context.pointer_authority)
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
        let cardinal = cardinal_resize_axis(resize.direction());
        let enabled = resize.operable() && context.ui.is_enabled();
        let response = context.interact_receiver(
            bounds,
            id,
            match (enabled, cardinal) {
                (true, Some(_)) => Sense::drag(),
                (true, None) => Sense::DRAG,
                (false, _) => Sense::hover(),
            },
            enabled
                .then(|| {
                    context
                        .plan
                        .receiver_for_contained_resize(contained, resize)
                })
                .flatten(),
        );
        let enabled = enabled && response.enabled();
        if let Some((orientation, horizontal_arrows)) = cardinal {
            configure_resize_accessibility(
                context.ui,
                &response,
                bounds,
                resize.direction(),
                orientation,
                enabled,
            );
            if enabled && response.has_focus() {
                context.ui.memory_mut(|memory| {
                    memory.set_focus_lock_filter(
                        id,
                        EventFilter {
                            horizontal_arrows,
                            vertical_arrows: !horizontal_arrows,
                            ..Default::default()
                        },
                    );
                });
            }
            if enabled
                && let Some(adjustment) = contained_resize_adjustment(
                    context.ui,
                    id,
                    resize.direction(),
                    response.has_focus(),
                )
                && let Some(action) = context.plan.prepare_contained_resize_adjustment(
                    contained.floating(),
                    resize.direction(),
                    adjustment,
                )
            {
                context.push_local_action(action);
            }
        }
        if response.hovered() || response.dragged() {
            context
                .ui
                .ctx()
                .set_cursor_icon(resize_cursor(resize.direction()));
        }
        if enabled
            && context.pointer_authority.accepts_local_pointer_actions()
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

fn contained_resize_adjustment(
    ui: &egui::Ui,
    id: egui::Id,
    direction: ContainedResizeDirection,
    focused: bool,
) -> Option<SurfaceContainedResizeAdjustment> {
    if focused {
        let keyboard = ui.input_mut(|input| {
            let (increment, decrement) = match direction {
                ContainedResizeDirection::North => (Key::ArrowDown, Key::ArrowUp),
                ContainedResizeDirection::East => (Key::ArrowRight, Key::ArrowLeft),
                ContainedResizeDirection::South => (Key::ArrowDown, Key::ArrowUp),
                ContainedResizeDirection::West => (Key::ArrowRight, Key::ArrowLeft),
                ContainedResizeDirection::NorthEast
                | ContainedResizeDirection::SouthEast
                | ContainedResizeDirection::SouthWest
                | ContainedResizeDirection::NorthWest => return None,
            };
            if input.consume_key(egui::Modifiers::NONE, increment) {
                Some(SurfaceContainedResizeAdjustment::Increment)
            } else if input.consume_key(egui::Modifiers::NONE, decrement) {
                Some(SurfaceContainedResizeAdjustment::Decrement)
            } else {
                None
            }
        });
        if keyboard.is_some() {
            return keyboard;
        }
    }
    ui.input(|input| {
        if input.has_accesskit_action_request(id, Action::Increment) {
            Some(SurfaceContainedResizeAdjustment::Increment)
        } else if input.has_accesskit_action_request(id, Action::Decrement) {
            Some(SurfaceContainedResizeAdjustment::Decrement)
        } else {
            None
        }
    })
}

fn configure_resize_accessibility(
    ui: &egui::Ui,
    response: &egui::Response,
    bounds: egui::Rect,
    direction: ContainedResizeDirection,
    orientation: Orientation,
    operable: bool,
) {
    ui.ctx().accesskit_node_builder(response.id, |node| {
        node.set_role(Role::Splitter);
        node.set_bounds(accesskit_bounds(bounds));
        node.set_label(match direction {
            ContainedResizeDirection::North => "Resize floating top edge",
            ContainedResizeDirection::East => "Resize floating right edge",
            ContainedResizeDirection::South => "Resize floating bottom edge",
            ContainedResizeDirection::West => "Resize floating left edge",
            ContainedResizeDirection::NorthEast
            | ContainedResizeDirection::SouthEast
            | ContainedResizeDirection::SouthWest
            | ContainedResizeDirection::NorthWest => {
                unreachable!("diagonal resize handles are not accessibility controls")
            }
        });
        node.set_orientation(orientation);
        if operable {
            node.add_action(Action::Increment);
            node.add_action(Action::Decrement);
        } else {
            node.set_disabled();
        }
    });
}

const fn cardinal_resize_axis(direction: ContainedResizeDirection) -> Option<(Orientation, bool)> {
    match direction {
        ContainedResizeDirection::North | ContainedResizeDirection::South => {
            Some((Orientation::Horizontal, false))
        }
        ContainedResizeDirection::East | ContainedResizeDirection::West => {
            Some((Orientation::Vertical, true))
        }
        ContainedResizeDirection::NorthEast
        | ContainedResizeDirection::SouthEast
        | ContainedResizeDirection::SouthWest
        | ContainedResizeDirection::NorthWest => None,
    }
}

fn title_label<'a>(
    context: &'a RenderContext<'_, '_, '_>,
    root: &RootPaintSchedule<'_>,
) -> &'a str {
    root.panes()
        .next()
        .and_then(dockspace::runtime::PanePaintRecord::selected)
        .and_then(|item| context.resources.item(item))
        .map_or("Floating", |resource| resource.title.as_str())
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
