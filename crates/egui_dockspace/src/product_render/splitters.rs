//! Splitter rendering and current-pass pointer gestures.

use dockspace::model::DockspaceAxis;
use dockspace::runtime::{
    SplitterJunctionPaintRecord, SurfacePaintDisposition, SurfaceSplitterAdjustment,
};
use egui::accesskit::{Action, Orientation, Role};
use egui::{CursorIcon, EventFilter, Key, Sense, UiBuilder};

use super::RenderContext;
use super::actions::{effective_enabled, gesture_phase};
use super::geometry::{accesskit_bounds, egui_rect};
pub(crate) fn paint_splitter(
    context: &mut RenderContext<'_, '_, '_>,
    splitter: dockspace::runtime::SplitterPaintRecord<'_>,
    paint: SurfacePaintDisposition,
) {
    let Some(draw) = egui_rect(splitter.draw_bounds()) else {
        return;
    };
    let Some(hit) = egui_rect(splitter.hit_bounds()) else {
        return;
    };
    let id = context
        .ui
        .make_persistent_id((context.instance_id, "splitter", splitter.visual_id()));
    let enabled = effective_enabled(context.ui, splitter.operable());
    let drag_enabled = enabled && context.pointer_authority.supports_preview_gestures();
    let sense = if drag_enabled {
        Sense::drag()
    } else if enabled {
        Sense::focusable_noninteractive()
    } else {
        Sense::hover()
    };
    let receiver = drag_enabled
        .then(|| context.plan.receiver_for_splitter(splitter))
        .flatten();
    let response = context.interact_receiver(hit, id, sense, receiver);
    configure_accessibility(context.ui, &response, splitter, enabled);
    let focused = enabled && response.has_focus();
    let emphasized = focused
        || (drag_enabled && (response.hovered() || context.response_dragged_locally(&response)));
    let paint_omitted = context.source_gap_active(paint);
    if !paint_omitted {
        context
            .ui
            .painter()
            .rect_filled(draw, 0.0, context.splitter_fill(&response, emphasized));
    }
    if !enabled {
        return;
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
    if drag_enabled && (response.hovered() || context.response_dragged_locally(&response)) {
        context.ui.ctx().set_cursor_icon(match splitter.axis() {
            DockspaceAxis::Horizontal => CursorIcon::ResizeHorizontal,
            DockspaceAxis::Vertical => CursorIcon::ResizeVertical,
        });
    }
    if context.pointer_authority.emits_local_preview_gestures()
        && let Some(phase) = gesture_phase(&response)
        && let Some(action) = context
            .plan
            .prepare_splitter_gesture(splitter.visual_id(), phase)
    {
        context.push_local_action(action);
    }
}

pub(crate) fn paint_junction(
    context: &mut RenderContext<'_, '_, '_>,
    junction: SplitterJunctionPaintRecord<'_>,
    paint: SurfacePaintDisposition,
) {
    let Some(hit) = egui_rect(junction.hit_bounds()) else {
        return;
    };
    let id = context.ui.make_persistent_id((
        context.instance_id,
        "splitter-junction",
        junction.visual_id(),
    ));
    let core_operable = context.plan.splitter_junction_operable(junction);
    let operable = effective_enabled(context.ui, core_operable);
    let drag_operable = operable && context.pointer_authority.supports_preview_gestures();
    let axis_operability = [DockspaceAxis::Horizontal, DockspaceAxis::Vertical].map(|axis| {
        let present = junction.has_axis(axis);
        let core_operable = present && context.plan.splitter_junction_axis_operable(junction, axis);
        (
            axis,
            present,
            core_operable,
            effective_enabled(context.ui, core_operable),
        )
    });
    let receiver = drag_operable
        .then(|| context.plan.receiver_for_splitter_junction(junction))
        .flatten();
    let response = context.interact_receiver(
        hit,
        id,
        if drag_operable {
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
            let horizontal_operable = axis_operability[0].3;
            let vertical_operable = axis_operability[1].3;
            let any_axis_operable = horizontal_operable || vertical_operable;
            node.set_role(Role::Group);
            node.set_label("Resize pane grid");
            node.set_description(junction_description(
                drag_operable,
                horizontal_operable,
                vertical_operable,
            ));
            node.set_bounds(accesskit_bounds(hit));
            if !drag_operable && !any_axis_operable {
                node.set_disabled();
            }
        });
    for (axis, present, _core_axis_operable, axis_operable) in axis_operability {
        if !present {
            continue;
        }
        let axis_response =
            junction_axis_response(context.ui, hit, response.id, id, axis, axis_operable);
        if !axis_operable {
            continue;
        }
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
    let paint_omitted = context.source_gap_active(paint);
    if !paint_omitted
        && drag_operable
        && (response.hovered() || context.response_dragged_locally(&response))
    {
        context.ui.ctx().set_cursor_icon(CursorIcon::AllScroll);
        context
            .ui
            .painter()
            .rect_filled(hit, 0.0, context.splitter_fill(&response, true));
    }
    if drag_operable
        && context.pointer_authority.emits_local_preview_gestures()
        && let Some(phase) = gesture_phase(&response)
        && let Some(action) = context
            .plan
            .prepare_splitter_gesture(junction.visual_id(), phase)
    {
        context.push_local_action(action);
    }
}

fn junction_description(
    drag_operable: bool,
    horizontal_operable: bool,
    vertical_operable: bool,
) -> &'static str {
    match (drag_operable, horizontal_operable, vertical_operable) {
        (true, true, true) => {
            "Drag to resize rows and columns, or use the horizontal and vertical splitter controls"
        }
        (true, true, false) => {
            "Drag to resize rows and columns, or use the horizontal splitter control"
        }
        (true, false, true) => {
            "Drag to resize rows and columns, or use the vertical splitter control"
        }
        (true, false, false) => "Drag to resize rows and columns",
        (false, true, true) => {
            "Use the horizontal and vertical splitter controls to resize rows and columns"
        }
        (false, true, false) => "Use the horizontal splitter control to resize columns",
        (false, false, true) => "Use the vertical splitter control to resize rows",
        (false, false, false) => "Pane grid resizing is unavailable",
    }
}

fn junction_axis_response(
    ui: &mut egui::Ui,
    hit: egui::Rect,
    parent_id: egui::Id,
    junction_id: egui::Id,
    axis: DockspaceAxis,
    enabled: bool,
) -> egui::Response {
    let mut axis_ui = ui.new_child(
        UiBuilder::new()
            .id_salt((junction_id, "axis", axis))
            .max_rect(hit)
            .sense(if enabled {
                Sense::focusable_noninteractive()
            } else {
                Sense::hover()
            })
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
        if enabled {
            node.add_action(Action::Increment);
            node.add_action(Action::Decrement);
        } else {
            node.set_disabled();
        }
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
    if !effective_enabled(ui, true) {
        return None;
    }
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
    enabled: bool,
) {
    ui.ctx().accesskit_node_builder(response.id, |node| {
        node.set_role(Role::Splitter);
        node.set_label("Resize panes");
        node.set_orientation(match splitter.axis() {
            DockspaceAxis::Horizontal => Orientation::Vertical,
            DockspaceAxis::Vertical => Orientation::Horizontal,
        });
        if enabled {
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
