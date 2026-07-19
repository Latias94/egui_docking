//! Splitter painting and absolute-pointer resize actions.

use dockspace::graph::{SplitWeight, Workspace};
use dockspace::ids::SurfaceId;
use dockspace::interaction::InteractionStatus;
use egui::accesskit::{Action, Orientation, Role};
use egui::{CursorIcon, EventFilter, FocusDirection, Id, Key, PointerButton, Sense, Ui};

use crate::hit::{contains_half_open, interact_rect};
use crate::projection::SplitterPlan;
use crate::renderer::{
    PRIMARY_BUTTON, PRIMARY_POINTER, RenderAction, RenderOutput, accesskit_bounds,
};
use crate::style::DockStyle;

#[allow(clippy::too_many_arguments)]
pub(crate) fn paint_splitter(
    ui: &mut Ui,
    instance_id: Id,
    surface: SurfaceId,
    plan: &SplitterPlan,
    workspace: &Workspace,
    style: &DockStyle,
    status: InteractionStatus,
    interactions_current: bool,
    output: &mut RenderOutput,
) {
    let id = ui.make_persistent_id((
        "egui_dockspace",
        instance_id,
        surface,
        plan.root,
        plan.split,
        plan.index,
        "splitter",
    ));
    let cursor = splitter_cursor(plan.horizontal);
    let response = ui.interact(interact_rect(plan.hit_rect), id, Sense::drag());
    configure_accessibility(ui, &response, plan);

    let focused = response.has_focus();
    if focused {
        lock_splitter_navigation_focus(ui, response.id, plan.horizontal);
    }
    let emphasized = focused || interactions_current && response.hovered();
    let color = if emphasized {
        style.splitter_hover_color
    } else {
        style.splitter_color
    };
    ui.painter().rect_filled(plan.rect, 0.0, color);
    if interactions_current && (response.hovered() || response.dragged()) {
        ui.ctx().set_cursor_icon(cursor);
    }

    if interactions_current
        && status == InteractionStatus::Idle
        && response.drag_started_by(PointerButton::Primary)
        && ui.input(|input| {
            input
                .pointer
                .press_origin()
                .is_some_and(|point| contains_half_open(plan.hit_rect, point))
        })
        && let Some(split) = output.capture(workspace.capture_node_source(plan.root, plan.split))
    {
        output.push(RenderAction::BeginResize {
            pointer: PRIMARY_POINTER,
            button: PRIMARY_BUTTON,
            split,
        });
    }

    if interactions_current
        && let InteractionStatus::Resizing { session } = status
        && response.dragged_by(PointerButton::Primary)
        && let Some(position) = response.interact_pointer_pos()
        && let Some(weights) = weights_at_pointer(plan, position)
    {
        output.push(RenderAction::UpdateResize { session, weights });
    }

    if let Some(adjustment) = adjustment_direction(ui, response.id, plan.horizontal, focused) {
        if adjustment.clear_cardinal_navigation {
            ui.memory_mut(|memory| memory.move_focus(FocusDirection::None));
        }
        if let Some(weights) = weights_at_pointer(
            plan,
            plan.rect.center()
                + if plan.horizontal {
                    egui::vec2(
                        style.splitter_keyboard_step * f32::from(adjustment.direction),
                        0.0,
                    )
                } else {
                    egui::vec2(
                        0.0,
                        style.splitter_keyboard_step * f32::from(adjustment.direction),
                    )
                },
        ) && let Some(split) =
            output.capture(workspace.capture_node_source(plan.root, plan.split))
        {
            output.push(RenderAction::AdjustResize { split, weights });
        }
    }
}

fn splitter_cursor(horizontal: bool) -> CursorIcon {
    if horizontal {
        CursorIcon::ResizeHorizontal
    } else {
        CursorIcon::ResizeVertical
    }
}

#[derive(Clone, Copy)]
struct SplitterAdjustment {
    direction: i8,
    clear_cardinal_navigation: bool,
}

fn adjustment_direction(
    ui: &Ui,
    id: egui::Id,
    horizontal: bool,
    focused: bool,
) -> Option<SplitterAdjustment> {
    let keyboard = focused.then(|| {
        ui.input(|input| {
            if horizontal && input.key_pressed(Key::ArrowLeft)
                || !horizontal && input.key_pressed(Key::ArrowUp)
            {
                Some(-1)
            } else if horizontal && input.key_pressed(Key::ArrowRight)
                || !horizontal && input.key_pressed(Key::ArrowDown)
            {
                Some(1)
            } else {
                None
            }
        })
    });
    if let Some(direction) = keyboard.flatten() {
        return Some(SplitterAdjustment {
            direction,
            clear_cardinal_navigation: true,
        });
    }

    ui.input(|input| {
        if input.has_accesskit_action_request(id, Action::Decrement) {
            Some(SplitterAdjustment {
                direction: -1,
                clear_cardinal_navigation: false,
            })
        } else if input.has_accesskit_action_request(id, Action::Increment) {
            Some(SplitterAdjustment {
                direction: 1,
                clear_cardinal_navigation: false,
            })
        } else {
            None
        }
    })
}

fn lock_splitter_navigation_focus(ui: &Ui, id: Id, horizontal: bool) {
    ui.memory_mut(|memory| {
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

fn weights_at_pointer(plan: &SplitterPlan, pointer: egui::Pos2) -> Option<Vec<SplitWeight>> {
    let before_extent = if plan.horizontal {
        plan.before_rect.width()
    } else {
        plan.before_rect.height()
    };
    let after_extent = if plan.horizontal {
        plan.after_rect.width()
    } else {
        plan.after_rect.height()
    };
    let available = before_extent + after_extent;
    if !available.is_finite()
        || available <= 2.0 * f32::EPSILON
        || plan.index + 1 >= plan.weights.len()
    {
        return None;
    }

    let pair_min = if plan.horizontal {
        plan.before_rect.min.x
    } else {
        plan.before_rect.min.y
    };
    let splitter_extent = if plan.horizontal {
        plan.rect.width()
    } else {
        plan.rect.height()
    };
    let pointer_axis = if plan.horizontal {
        pointer.x
    } else {
        pointer.y
    };
    let before_ratio = ((pointer_axis - pair_min - splitter_extent * 0.5) / available)
        .clamp(f32::EPSILON, 1.0 - f32::EPSILON);
    let pair_weight = plan.weights[plan.index].get() + plan.weights[plan.index + 1].get();
    let mut values = plan
        .weights
        .iter()
        .map(|weight| weight.get())
        .collect::<Vec<_>>();
    values[plan.index] = pair_weight * before_ratio;
    values[plan.index + 1] = pair_weight * (1.0 - before_ratio);
    SplitWeight::normalize(values).ok()
}

fn configure_accessibility(ui: &Ui, response: &egui::Response, plan: &SplitterPlan) {
    ui.ctx().accesskit_node_builder(response.id, |node| {
        node.set_role(Role::Splitter);
        node.set_bounds(accesskit_bounds(plan.hit_rect));
        node.set_label("Resize panes");
        node.set_orientation(if plan.horizontal {
            Orientation::Vertical
        } else {
            Orientation::Horizontal
        });
        node.add_action(Action::Focus);
        node.add_action(Action::Increment);
        node.add_action(Action::Decrement);
        let before: f64 = plan.weights[..=plan.index]
            .iter()
            .map(|weight| f64::from(weight.get()))
            .sum();
        node.set_numeric_value(before);
        node.set_min_numeric_value(0.0);
        node.set_max_numeric_value(1.0);
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keyboard_axes_follow_split_direction() {
        assert_eq!(splitter_cursor(true), CursorIcon::ResizeHorizontal);
        assert_eq!(splitter_cursor(false), CursorIcon::ResizeVertical);
    }
}
