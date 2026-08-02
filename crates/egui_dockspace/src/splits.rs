//! Splitter painting and scene-bound non-pointer actions.

use dockspace::ids::SurfaceId;
use dockspace::presentation_hit::PresentationHitRegionKind;
use dockspace::scene::{SplitterRecord, SurfaceSceneStamp};
use egui::accesskit::{Action, Orientation, Role};
use egui::{EventFilter, FocusDirection, Id, Key, Sense, Ui};

use crate::hit::interact_rect;
use crate::renderer::{RenderAction, RenderOutput, accesskit_bounds, from_logical_rect};
use crate::style::DockStyle;

#[allow(clippy::too_many_arguments)]
pub(crate) fn paint_splitter(
    ui: &mut Ui,
    instance_id: Id,
    surface: SurfaceId,
    plan: &SplitterRecord,
    style: &DockStyle,
    interaction_scene: Option<SurfaceSceneStamp>,
    semantic_scene: Option<SurfaceSceneStamp>,
    authoritative_hovered: bool,
    output: &mut RenderOutput,
) {
    let splitter = *plan.id();
    let Some(hit_rect) = from_logical_rect(plan.hit().rect()) else {
        return;
    };
    let Some(draw_rect) = from_logical_rect(plan.draw_bounds()) else {
        return;
    };
    let horizontal = plan.axis() == dockspace::graph::Axis::Horizontal;
    let id = ui.make_persistent_id((
        "egui_dockspace",
        instance_id,
        surface,
        splitter.root,
        splitter.split,
        splitter.index,
        "splitter",
    ));
    let sense = if plan.operable() {
        Sense::drag()
    } else {
        Sense::hover()
    };
    let response = ui.interact(interact_rect(hit_rect), id, sense);
    if plan.operable() {
        output.register_receiver(
            &response,
            PresentationHitRegionKind::SplitterHandle(splitter),
        );
    }
    let interactions_current = interaction_scene.is_some();
    if plan.operable() {
        configure_accessibility(ui, &response, plan, hit_rect, interactions_current);
    } else if response.has_focus() {
        response.surrender_focus();
    }

    let focused = response.has_focus();
    if focused {
        lock_splitter_navigation_focus(ui, response.id, horizontal);
    }
    let emphasized = focused || plan.operable() && interactions_current && authoritative_hovered;
    let color = if emphasized {
        style.splitter_hover_color
    } else {
        style.splitter_color
    };
    ui.painter().rect_filled(draw_rect, 0.0, color);
    if plan.operable()
        && let Some(scene) = semantic_scene
        && let Some(adjustment) = adjustment_direction(ui, response.id, horizontal, focused)
    {
        if adjustment.clear_cardinal_navigation {
            ui.memory_mut(|memory| memory.move_focus(FocusDirection::None));
        }
        let action = RenderAction::AdjustSplitterResize {
            scene,
            splitter,
            delta: f64::from(style.splitter_keyboard_step) * f64::from(adjustment.direction),
        };
        match adjustment.source {
            SplitterAdjustmentSource::Keyboard(key) => output.push_key(ui, action, key),
            SplitterAdjustmentSource::AccessKit(requested) => {
                output.push_accesskit(ui, action, response.id, requested);
            }
        }
    }
}

#[derive(Clone, Copy)]
struct SplitterAdjustment {
    direction: i8,
    clear_cardinal_navigation: bool,
    source: SplitterAdjustmentSource,
}

#[derive(Clone, Copy)]
enum SplitterAdjustmentSource {
    Keyboard(Key),
    AccessKit(Action),
}

fn adjustment_direction(
    ui: &Ui,
    id: egui::Id,
    horizontal: bool,
    focused: bool,
) -> Option<SplitterAdjustment> {
    let keyboard = focused.then(|| {
        ui.input(|input| {
            let negative = if horizontal {
                Key::ArrowLeft
            } else {
                Key::ArrowUp
            };
            let positive = if horizontal {
                Key::ArrowRight
            } else {
                Key::ArrowDown
            };
            if input.key_pressed(negative) {
                Some((-1, negative))
            } else if input.key_pressed(positive) {
                Some((1, positive))
            } else {
                None
            }
        })
    });
    if let Some((direction, key)) = keyboard.flatten() {
        return Some(SplitterAdjustment {
            direction,
            clear_cardinal_navigation: true,
            source: SplitterAdjustmentSource::Keyboard(key),
        });
    }

    ui.input(|input| {
        if input.has_accesskit_action_request(id, Action::Decrement) {
            Some(SplitterAdjustment {
                direction: -1,
                clear_cardinal_navigation: false,
                source: SplitterAdjustmentSource::AccessKit(Action::Decrement),
            })
        } else if input.has_accesskit_action_request(id, Action::Increment) {
            Some(SplitterAdjustment {
                direction: 1,
                clear_cardinal_navigation: false,
                source: SplitterAdjustmentSource::AccessKit(Action::Increment),
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

fn configure_accessibility(
    ui: &Ui,
    response: &egui::Response,
    plan: &SplitterRecord,
    hit_rect: egui::Rect,
    interactions_current: bool,
) {
    ui.ctx().accesskit_node_builder(response.id, |node| {
        node.set_role(Role::Splitter);
        node.set_bounds(accesskit_bounds(hit_rect));
        node.set_label("Resize panes");
        node.set_orientation(if plan.axis() == dockspace::graph::Axis::Horizontal {
            Orientation::Vertical
        } else {
            Orientation::Horizontal
        });
        if interactions_current {
            node.add_action(Action::Focus);
            node.add_action(Action::Increment);
            node.add_action(Action::Decrement);
        } else {
            node.clear_actions();
            node.set_disabled();
        }
        let before: f64 = plan.weights()[..=plan.id().index]
            .iter()
            .map(|weight| f64::from(weight.get()))
            .sum();
        node.set_numeric_value(before);
        node.set_min_numeric_value(0.0);
        node.set_max_numeric_value(1.0);
    });
}
