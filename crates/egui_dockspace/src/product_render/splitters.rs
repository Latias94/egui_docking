//! Splitter rendering and current-pass pointer gestures.

use dockspace::model::DockspaceAxis;
use dockspace::runtime::SurfaceSplitterAdjustment;
use egui::accesskit::{Action, Orientation, Role};
use egui::{CursorIcon, EventFilter, Key, Sense};

use super::RenderContext;
use super::actions::gesture_phase;
use super::geometry::egui_rect;
use super::schedule::RootPaintSchedule;

pub(crate) fn paint_root(context: &mut RenderContext<'_, '_>, root: &RootPaintSchedule<'_>) {
    for splitter in root.splitters() {
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
        let receiver = context.plan.receiver_for_splitter(splitter);
        let response = context.interact_receiver(hit, id, sense, receiver);
        configure_accessibility(context.ui, &response, splitter);
        let focused = response.has_focus();
        let emphasized = response.hovered() || response.dragged() || focused;
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
        let horizontal = splitter.axis() == DockspaceAxis::Horizontal;
        if focused {
            context.ui.memory_mut(|memory| {
                memory.set_focus_lock_filter(
                    id,
                    EventFilter {
                        horizontal_arrows: horizontal,
                        vertical_arrows: !horizontal,
                        ..Default::default()
                    },
                );
            });
        }
        let adjustment = splitter_adjustment(context.ui, id, horizontal, focused);
        if let Some(action) = adjustment.and_then(|adjustment| {
            context
                .plan
                .prepare_splitter_adjustment(splitter.visual_id(), adjustment)
        }) {
            context.push_local_action(action);
        }
        if response.hovered() || response.dragged() {
            context.ui.ctx().set_cursor_icon(match splitter.axis() {
                DockspaceAxis::Horizontal => CursorIcon::ResizeHorizontal,
                DockspaceAxis::Vertical => CursorIcon::ResizeVertical,
            });
        }
        if context.pointer_authority.accepts_local_pointer_actions()
            && let Some(phase) = gesture_phase(&response)
            && let Some(action) = context
                .plan
                .prepare_splitter_gesture(splitter.visual_id(), phase)
        {
            context.push_local_action(action);
        }
    }
}

fn splitter_adjustment(
    ui: &egui::Ui,
    id: egui::Id,
    horizontal: bool,
    focused: bool,
) -> Option<SurfaceSplitterAdjustment> {
    if focused {
        let keyboard = ui.input_mut(|input| {
            let decrement = if horizontal {
                Key::ArrowLeft
            } else {
                Key::ArrowUp
            };
            let increment = if horizontal {
                Key::ArrowRight
            } else {
                Key::ArrowDown
            };
            if input.consume_key(egui::Modifiers::NONE, decrement) {
                Some(SurfaceSplitterAdjustment::Decrement)
            } else if input.consume_key(egui::Modifiers::NONE, increment) {
                Some(SurfaceSplitterAdjustment::Increment)
            } else {
                None
            }
        });
        if keyboard.is_some() {
            return keyboard;
        }
    }
    ui.input(|input| {
        if input.has_accesskit_action_request(id, Action::Decrement) {
            Some(SurfaceSplitterAdjustment::Decrement)
        } else if input.has_accesskit_action_request(id, Action::Increment) {
            Some(SurfaceSplitterAdjustment::Increment)
        } else {
            None
        }
    })
}

fn configure_accessibility(
    ui: &egui::Ui,
    response: &egui::Response,
    splitter: dockspace::runtime::SplitterPaintRecord<'_>,
) {
    ui.ctx().accesskit_node_builder(response.id, |node| {
        node.set_role(Role::Splitter);
        node.set_label("Resize panes");
        node.set_orientation(match splitter.axis() {
            DockspaceAxis::Horizontal => Orientation::Vertical,
            DockspaceAxis::Vertical => Orientation::Horizontal,
        });
        if splitter.operable() {
            node.add_action(Action::Increment);
            node.add_action(Action::Decrement);
        } else {
            node.set_disabled();
        }
        let before = splitter.before_bounds();
        let after = splitter.after_bounds();
        let (before_extent, after_extent) = match splitter.axis() {
            DockspaceAxis::Horizontal => (before.width(), after.width()),
            DockspaceAxis::Vertical => (before.height(), after.height()),
        };
        let total = before_extent + after_extent;
        if total > 0.0 {
            node.set_numeric_value(before_extent / total);
            node.set_min_numeric_value(0.0);
            node.set_max_numeric_value(1.0);
        }
    });
}
