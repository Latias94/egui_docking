//! Splitter rendering and current-pass pointer gestures.

use dockspace::model::DockspaceAxis;
use dockspace::runtime::{SplitterJunctionPaintRecord, SurfaceSplitterAdjustment};
use egui::accesskit::{Action, Orientation, Role};
use egui::{CursorIcon, EventFilter, Key, Sense, UiBuilder};

use super::RenderContext;
use super::actions::gesture_phase;
use super::geometry::{accesskit_bounds, egui_rect};
use super::schedule::RootPaintSchedule;

pub(crate) fn paint_root(context: &mut RenderContext<'_, '_, '_>, root: &RootPaintSchedule<'_>) {
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
        let emphasized =
            response.hovered() || context.response_dragged_locally(&response) || focused;
        let paint_omitted = context
            .plan
            .drag_decoration()
            .is_some_and(|decoration| decoration.omits_visual(splitter.visual_id()));
        if !paint_omitted {
            context.ui.painter().rect_filled(
                draw,
                0.0,
                context.splitter_fill(&response, emphasized),
            );
        }
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
        if response.hovered() || context.response_dragged_locally(&response) {
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
    for junction in root.splitter_junctions() {
        paint_junction(context, junction);
    }
}

fn paint_junction(
    context: &mut RenderContext<'_, '_, '_>,
    junction: SplitterJunctionPaintRecord<'_>,
) {
    let Some(hit) = egui_rect(junction.hit_bounds()) else {
        return;
    };
    let id = context.ui.make_persistent_id((
        context.instance_id,
        "splitter-junction",
        junction.visual_id(),
    ));
    let operable = context.plan.splitter_junction_operable(junction);
    let axis_operability = [DockspaceAxis::Horizontal, DockspaceAxis::Vertical].map(|axis| {
        (
            axis,
            junction.has_axis(axis) && context.plan.splitter_junction_axis_operable(junction, axis),
        )
    });
    let receiver = context.plan.receiver_for_splitter_junction(junction);
    let response = context.interact_receiver(
        hit,
        id,
        if operable {
            Sense::DRAG
        } else {
            Sense::hover()
        },
        receiver,
    );
    context
        .ui
        .ctx()
        .accesskit_node_builder(response.id, |node| {
            node.set_role(Role::Group);
            node.set_label("Resize pane grid");
            node.set_description("Drag to resize rows and columns");
            node.set_bounds(accesskit_bounds(hit));
            if !operable && axis_operability.iter().all(|(_, operable)| !operable) {
                node.set_disabled();
            }
        });
    for (axis, axis_operable) in axis_operability {
        if !axis_operable {
            continue;
        }
        let axis_response = junction_axis_response(context.ui, hit, response.id, id, axis);
        let horizontal = axis == DockspaceAxis::Horizontal;
        if axis_response.has_focus() {
            context.ui.memory_mut(|memory| {
                memory.set_focus_lock_filter(
                    axis_response.id,
                    EventFilter {
                        horizontal_arrows: horizontal,
                        vertical_arrows: !horizontal,
                        ..Default::default()
                    },
                );
            });
        }
        if let Some(action) = splitter_adjustment(
            context.ui,
            axis_response.id,
            horizontal,
            axis_response.has_focus(),
        )
        .and_then(|adjustment| {
            context.plan.prepare_splitter_junction_adjustment(
                junction.visual_id(),
                axis,
                adjustment,
            )
        }) {
            context.push_local_action(action);
        }
    }
    let paint_omitted = context
        .plan
        .drag_decoration()
        .is_some_and(|decoration| decoration.omits_visual(junction.visual_id()));
    if !paint_omitted
        && operable
        && (response.hovered() || context.response_dragged_locally(&response))
    {
        context.ui.ctx().set_cursor_icon(CursorIcon::AllScroll);
        context
            .ui
            .painter()
            .rect_filled(hit, 0.0, context.splitter_fill(&response, true));
    }
    if operable
        && context.pointer_authority.accepts_local_pointer_actions()
        && let Some(phase) = gesture_phase(&response)
        && let Some(action) = context
            .plan
            .prepare_splitter_gesture(junction.visual_id(), phase)
    {
        context.push_local_action(action);
    }
}

fn junction_axis_response(
    ui: &mut egui::Ui,
    hit: egui::Rect,
    parent_id: egui::Id,
    junction_id: egui::Id,
    axis: DockspaceAxis,
) -> egui::Response {
    let mut axis_ui = ui.new_child(
        UiBuilder::new()
            .id_salt((junction_id, "axis", axis))
            .max_rect(hit)
            .sense(Sense::focusable_noninteractive())
            .accessibility_parent(parent_id),
    );
    axis_ui.expand_to_include_rect(hit);
    let response = axis_ui.response();
    axis_ui.ctx().accesskit_node_builder(response.id, |node| {
        node.set_role(Role::Splitter);
        node.set_bounds(accesskit_bounds(hit));
        match axis {
            DockspaceAxis::Horizontal => {
                node.set_label("Resize pane grid horizontally");
                node.set_description("Use Left and Right Arrow to resize columns");
                node.set_orientation(Orientation::Vertical);
            }
            DockspaceAxis::Vertical => {
                node.set_label("Resize pane grid vertically");
                node.set_description("Use Up and Down Arrow to resize rows");
                node.set_orientation(Orientation::Horizontal);
            }
        }
        node.add_action(Action::Increment);
        node.add_action(Action::Decrement);
    });
    drop(axis_ui);
    response
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
