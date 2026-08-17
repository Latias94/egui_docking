//! Pane and tab rendering over opaque product paint records.

use dockspace::model::ItemId;
use dockspace::runtime::{SurfaceTabNavigation, TabPaintRecord};
use egui::accesskit::{Action, Role};
use egui::{
    Align, CursorIcon, EventFilter, Id, Key, Layout, PointerButton, Sense, Stroke, StrokeKind, Ui,
    UiBuilder, pos2,
};

use crate::style::DockStyle;

use super::RenderContext;
use super::actions::{button_activated, gesture_phase};
use super::geometry::{accesskit_bounds, egui_rect};
use super::measurement::TabPaintResource;
use super::schedule::RootPaintSchedule;

pub(crate) fn paint_root(context: &mut RenderContext<'_, '_, '_>, root: &RootPaintSchedule<'_>) {
    paint_panes(context, root);
    paint_tab_bars(context, root);
    paint_tabs(context, root);
}

fn paint_panes(context: &mut RenderContext<'_, '_, '_>, root: &RootPaintSchedule<'_>) {
    for pane in root.panes() {
        let Some(bounds) = egui_rect(pane.bounds()) else {
            continue;
        };
        context
            .ui
            .painter()
            .rect_filled(bounds, 0.0, context.style.workspace_fill);
        let Some(pane_rect) = egui_rect(pane.content_bounds()) else {
            continue;
        };
        let pane_id =
            context
                .ui
                .make_persistent_id((context.instance_id, "pane-body", pane.visual_id()));
        let _ = context.interact_receiver(
            pane_rect,
            pane_id,
            Sense::click_and_drag(),
            context.plan.receiver_for_pane(pane),
        );
        let Some(item) = pane.selected() else {
            continue;
        };
        let mut child = context.ui.new_child(
            UiBuilder::new()
                .id_salt((context.instance_id, "pane", pane.visual_id()))
                .max_rect(pane_rect)
                .layout(Layout::top_down(Align::Min)),
        );
        child.set_clip_rect(pane_rect);
        if context
            .resources
            .item(item)
            .is_some_and(|resource| resource.missing)
        {
            child.centered_and_justified(|ui| {
                ui.label(format!("Missing pane {}", item.get()));
            });
        } else {
            context.panes.ui(item, &mut child);
        }
    }
}

fn paint_tab_bars(context: &mut RenderContext<'_, '_, '_>, root: &RootPaintSchedule<'_>) {
    for bar in root.tab_bars() {
        if let Some(bounds) = egui_rect(bar.bounds()) {
            context
                .ui
                .painter()
                .rect_filled(bounds, 0.0, context.style.tab_bar_fill);
        }
        let id = context.ui.make_persistent_id((
            context.instance_id,
            "tab-strip-scroll",
            bar.visual_id(),
        ));
        let receiver = context.plan.receiver_for_tab_strip_scroll(bar);
        context.register_scroll_receiver(id, receiver);
        paint_group_grip(context, bar);
    }
}

fn paint_group_grip(
    context: &mut RenderContext<'_, '_, '_>,
    bar: dockspace::runtime::TabBarPaintRecord<'_>,
) {
    let Some(grip) = bar.group_grip_bounds().and_then(egui_rect) else {
        return;
    };
    let id =
        context
            .ui
            .make_persistent_id((context.instance_id, "tab-group-grip", bar.visual_id()));
    let response = context
        .interact_receiver(
            grip,
            id,
            Sense::drag(),
            context.plan.receiver_for_tab_group_grip(bar),
        )
        .on_hover_text("Drag tab group");
    let active = response.hovered() || response.dragged();
    if active {
        context.ui.ctx().set_cursor_icon(if response.dragged() {
            CursorIcon::Grabbing
        } else {
            CursorIcon::Grab
        });
    }
    paint_group_grip_icon(context.ui, grip, context.style, active);
    if context.pointer_authority.accepts_local_pointer_actions()
        && let Some(phase) = gesture_phase(&response)
        && let Some(action) = context
            .plan
            .prepare_tab_group_gesture(bar.visual_id(), phase)
    {
        context.push_preview_gesture_action(action);
    }
}

fn paint_group_grip_icon(ui: &Ui, rect: egui::Rect, style: &DockStyle, active: bool) {
    let color = if active {
        style.tab_active_text_color
    } else {
        style.tab_text_color
    };
    let spacing = 3.5_f32.min(rect.width() * 0.18).min(rect.height() * 0.18);
    let radius = 1.0_f32.min(spacing * 0.32);
    for x in [-0.5, 0.5] {
        for y in [-1.0, 0.0, 1.0] {
            ui.painter().circle_filled(
                rect.center() + egui::vec2(x * spacing, y * spacing),
                radius,
                color,
            );
        }
    }
}

fn paint_tabs(context: &mut RenderContext<'_, '_, '_>, root: &RootPaintSchedule<'_>) {
    for tab in root.tabs() {
        let Some(resource) = context.resources.tab(tab.visual_id()).cloned() else {
            continue;
        };
        paint_tab(context, tab, &resource);
    }
}

fn paint_tab(
    context: &mut RenderContext<'_, '_, '_>,
    tab: TabPaintRecord<'_>,
    resource: &TabPaintResource,
) {
    let Some(visible) = egui_rect(tab.visible_bounds()) else {
        return;
    };
    let Some(drag) = egui_rect(tab.drag_bounds()) else {
        return;
    };
    let id = tab_id(context.ui, context.instance_id, tab);
    let response = context.interact_receiver(
        drag,
        id,
        Sense::click_and_drag(),
        context.plan.receiver_for_tab_body(tab),
    );
    let selected = tab.selected();
    let focused = response.has_focus();
    let fill = if selected {
        context.style.tab_active_fill
    } else if response.hovered() {
        context.style.tab_hover_fill
    } else {
        context.style.tab_fill
    };
    context.ui.painter().rect_filled(visible, 0.0, fill);
    if focused {
        context.ui.painter().rect_stroke(
            visible.shrink(1.0),
            0.0,
            Stroke::new(1.0, context.style.drop_border_color),
            StrokeKind::Inside,
        );
    }
    if let Some(text) = egui_rect(tab.text_bounds()) {
        let color = if selected {
            context.style.tab_active_text_color
        } else {
            context.style.tab_text_color
        };
        context.ui.painter_at(text).galley_with_override_text_color(
            pos2(text.min.x, text.center().y - resource.galley.size().y * 0.5),
            resource.galley.clone(),
            color,
        );
    }
    configure_tab_accessibility(context.ui, id, visible, resource.title.as_str(), selected);
    capture_tab_actions(context, tab, resource, &response, focused);
}

fn tab_id(ui: &Ui, instance_id: Id, tab: TabPaintRecord<'_>) -> Id {
    ui.make_persistent_id((instance_id, "tab", tab.visual_id()))
}

fn capture_tab_actions(
    context: &mut RenderContext<'_, '_, '_>,
    tab: TabPaintRecord<'_>,
    resource: &TabPaintResource,
    response: &egui::Response,
    focused: bool,
) {
    let id = response.id;
    let keyboard_activation = focused
        && context.ui.input(|input| {
            input.key_pressed(egui::Key::Enter) || input.key_pressed(egui::Key::Space)
        });
    let accesskit_activation = context.ui.input(|input| {
        input.has_accesskit_action_request(id, Action::Click)
            || input.has_accesskit_action_request(id, Action::Focus)
    });
    let pointer_activation = context.pointer_authority.accepts_local_pointer_actions()
        && response.clicked_by(PointerButton::Primary);
    if pointer_activation || keyboard_activation || accesskit_activation {
        response.request_focus();
        if !tab.selected()
            && let Some(action) = context.plan.prepare_tab_select(tab.item())
        {
            context.push_local_action(action);
        }
    }
    if focused {
        context.ui.memory_mut(|memory| {
            memory.set_focus_lock_filter(
                id,
                EventFilter {
                    horizontal_arrows: true,
                    ..Default::default()
                },
            );
        });
        let navigation = context.ui.input_mut(|input| {
            if input.consume_key(egui::Modifiers::NONE, Key::ArrowLeft) {
                Some(SurfaceTabNavigation::Previous)
            } else if input.consume_key(egui::Modifiers::NONE, Key::ArrowRight) {
                Some(SurfaceTabNavigation::Next)
            } else if input.consume_key(egui::Modifiers::NONE, Key::Home) {
                Some(SurfaceTabNavigation::First)
            } else if input.consume_key(egui::Modifiers::NONE, Key::End) {
                Some(SurfaceTabNavigation::Last)
            } else {
                None
            }
        });
        if let Some(action) = navigation
            .and_then(|navigation| context.plan.prepare_tab_navigation(tab.item(), navigation))
        {
            context
                .ui
                .memory_mut(|memory| memory.move_focus(egui::FocusDirection::None));
            if let Some(target) = action.tab_focus_target()
                && let Some(target_tab) = context.plan.tabs().find(|tab| tab.item() == target)
            {
                context.ui.memory_mut(|memory| {
                    memory.request_focus(tab_id(context.ui, context.instance_id, target_tab))
                });
            }
            context.push_local_action(action);
        }
    }
    if context.pointer_authority.accepts_local_pointer_actions()
        && let Some(phase) = gesture_phase(response)
        && let Some(action) = context.plan.prepare_tab_gesture(tab.item(), phase)
    {
        context.push_preview_gesture_action(action);
    }

    if let Some(close_bounds) = tab.close_bounds().and_then(egui_rect) {
        paint_close(
            context,
            tab.item(),
            close_bounds,
            resource.title.as_str(),
            context.plan.receiver_for_tab_close(tab),
        );
    }
}

fn paint_close(
    context: &mut RenderContext<'_, '_, '_>,
    item: ItemId,
    rect: egui::Rect,
    title: &str,
    receiver: Option<dockspace::runtime::DockspaceReceiverDescriptor>,
) {
    let id = context
        .ui
        .make_persistent_id((context.instance_id, "tab-close", item));
    let response = context.interact_receiver(rect, id, Sense::click(), receiver);
    let color = if response.hovered() {
        context.style.tab_active_text_color
    } else {
        context.style.tab_text_color
    };
    let inset = rect.width().min(rect.height()) * 0.28;
    let stroke = Stroke::new(1.5, color);
    context.ui.painter().line_segment(
        [
            rect.left_top() + egui::vec2(inset, inset),
            rect.right_bottom() - egui::vec2(inset, inset),
        ],
        stroke,
    );
    context.ui.painter().line_segment(
        [
            rect.right_top() + egui::vec2(-inset, inset),
            rect.left_bottom() + egui::vec2(inset, -inset),
        ],
        stroke,
    );
    context.ui.ctx().accesskit_node_builder(id, |node| {
        node.set_role(Role::Button);
        node.set_bounds(accesskit_bounds(rect));
        node.set_label(format!("Close {title}"));
        node.add_action(Action::Click);
    });
    if button_activated(context.ui, &response, context.pointer_authority)
        && let Some(action) = context.plan.prepare_tab_close(item)
    {
        context.push_local_action(action);
    }
}

fn configure_tab_accessibility(ui: &Ui, id: Id, rect: egui::Rect, title: &str, selected: bool) {
    ui.ctx().accesskit_node_builder(id, |node| {
        node.set_role(Role::Tab);
        node.set_bounds(accesskit_bounds(rect));
        node.set_label(title);
        node.add_action(Action::Focus);
        node.add_action(Action::Click);
        if selected {
            node.set_selected(true);
        } else {
            node.clear_selected();
        }
    });
}
