//! Pane and tab rendering over opaque product paint records.

use dockspace::model::{ItemId, RootId};
use dockspace::runtime::TabPaintRecord;
use egui::accesskit::{Action, Role};
use egui::{Align, Id, Layout, PointerButton, Sense, Stroke, StrokeKind, Ui, UiBuilder, pos2};

use super::RenderContext;
use super::actions::gesture_phase;
use super::geometry::{accesskit_bounds, egui_rect};
use super::measurement::TabPaintResource;

pub(crate) fn paint_root(context: &mut RenderContext<'_, '_>, root: RootId) {
    paint_panes(context, root);
    paint_tab_bars(context, root);
    paint_tabs(context, root);
}

fn paint_panes(context: &mut RenderContext<'_, '_>, root: RootId) {
    for pane in context.plan.panes().filter(|pane| pane.root() == root) {
        let Some(bounds) = egui_rect(pane.bounds()) else {
            continue;
        };
        context
            .ui
            .painter()
            .rect_filled(bounds, 0.0, context.style.workspace_fill);
        let Some(item) = pane.selected() else {
            continue;
        };
        let Some(pane_rect) = egui_rect(pane.content_bounds()) else {
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

fn paint_tab_bars(context: &mut RenderContext<'_, '_>, root: RootId) {
    for bar in context.plan.tab_bars().filter(|bar| bar.root() == root) {
        if let Some(bounds) = egui_rect(bar.bounds()) {
            context
                .ui
                .painter()
                .rect_filled(bounds, 0.0, context.style.tab_bar_fill);
        }
    }
}

fn paint_tabs(context: &mut RenderContext<'_, '_>, root: RootId) {
    let tabs = context
        .plan
        .tabs()
        .filter(|tab| tab.root() == root)
        .collect::<Vec<_>>();
    for tab in tabs {
        let Some(resource) = context.resources.tab(tab.visual_id()).cloned() else {
            continue;
        };
        paint_tab(context, tab, &resource);
    }
}

fn paint_tab(
    context: &mut RenderContext<'_, '_>,
    tab: TabPaintRecord<'_>,
    resource: &TabPaintResource,
) {
    let Some(visible) = egui_rect(tab.visible_bounds()) else {
        return;
    };
    let Some(drag) = egui_rect(tab.drag_bounds()) else {
        return;
    };
    let id = context
        .ui
        .make_persistent_id((context.instance_id, "tab", tab.visual_id()));
    let response = context.ui.interact(drag, id, Sense::click_and_drag());
    let selected = tab.selected();
    let fill = if selected {
        context.style.tab_active_fill
    } else if response.hovered() {
        context.style.tab_hover_fill
    } else {
        context.style.tab_fill
    };
    context.ui.painter().rect_filled(visible, 0.0, fill);
    if response.has_focus() {
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
    capture_tab_actions(context, tab, resource, &response);
}

fn capture_tab_actions(
    context: &mut RenderContext<'_, '_>,
    tab: TabPaintRecord<'_>,
    resource: &TabPaintResource,
    response: &egui::Response,
) {
    let id = response.id;
    let keyboard_activation = response.has_focus()
        && context.ui.input(|input| {
            input.key_pressed(egui::Key::Enter) || input.key_pressed(egui::Key::Space)
        });
    let accesskit_activation = context.ui.input(|input| {
        input.has_accesskit_action_request(id, Action::Click)
            || input.has_accesskit_action_request(id, Action::Focus)
    });
    if response.clicked_by(PointerButton::Primary) || keyboard_activation || accesskit_activation {
        response.request_focus();
        if !tab.selected()
            && let Some(action) = context.plan.prepare_tab_select(tab.item())
        {
            context.actions.push(action);
        }
    }
    if let Some(phase) = gesture_phase(response)
        && let Some(action) = context.plan.prepare_tab_gesture(tab.item(), phase)
    {
        context.push_preview_gesture_action(action);
    }

    if let Some(close_bounds) = tab.close_bounds().and_then(egui_rect) {
        paint_close(context, tab.item(), close_bounds, resource.title.as_str());
    }
}

fn paint_close(context: &mut RenderContext<'_, '_>, item: ItemId, rect: egui::Rect, title: &str) {
    let id = context
        .ui
        .make_persistent_id((context.instance_id, "tab-close", item));
    let response = context.ui.interact(rect, id, Sense::click());
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
    let accesskit = context
        .ui
        .input(|input| input.has_accesskit_action_request(id, Action::Click));
    if (response.clicked_by(PointerButton::Primary) || accesskit)
        && let Some(action) = context.plan.prepare_tab_close(item)
    {
        context.actions.push(action);
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
