//! Tab chrome, keyboard navigation, accessibility, and pane dispatch.

use dockspace::command::MovePayload;
use dockspace::graph::Workspace;
use dockspace::ids::{ItemId, SurfaceId};
use dockspace::interaction::{ActiveDragView, DragPhase, InteractionStatus};
use egui::accesskit::{Action, Orientation, Role};
use egui::{
    CursorIcon, EventFilter, FocusDirection, Id, Key, PointerButton, Rect, Sense, Stroke,
    StrokeKind, Ui, UiBuilder, pos2,
};

use crate::hit::{
    accesskit_focus_requested, contains_half_open, interact_rect, semantic_activation,
};
use crate::pane::PaneView;
use crate::projection::{TabPlan, TabsPlan};
use crate::renderer::{
    PRIMARY_BUTTON, PRIMARY_POINTER, RenderAction, RenderOutput, accesskit_bounds,
    paint_centered_label,
};
use crate::style::DockStyle;

#[allow(clippy::too_many_arguments)]
pub(crate) fn paint_tabs(
    ui: &mut Ui,
    instance_id: Id,
    surface: SurfaceId,
    plan: &TabsPlan,
    workspace: &Workspace,
    panes: &mut dyn PaneView,
    style: &DockStyle,
    status: InteractionStatus,
    active_drag: Option<ActiveDragView<'_>>,
    interactions_current: bool,
    output: &mut RenderOutput,
) {
    let painter = ui.painter_at(plan.node_rect);
    painter.rect_filled(plan.node_rect, 0.0, style.workspace_fill);
    painter.rect_filled(plan.tab_bar_rect, 0.0, style.tab_bar_fill);

    let mut tab_ui = ui.new_child(
        UiBuilder::new()
            .id_salt((
                "egui_dockspace",
                instance_id,
                surface,
                plan.root,
                plan.node,
                "tab-list",
            ))
            .max_rect(plan.tab_bar_rect),
    );
    tab_ui.set_clip_rect(plan.tab_bar_rect.intersect(ui.clip_rect()));
    tab_ui.set_min_size(plan.tab_bar_rect.size());
    configure_tab_list_accessibility(&tab_ui, instance_id, plan);

    paint_group_grip(
        &mut tab_ui,
        instance_id,
        plan,
        workspace,
        style,
        status,
        active_drag,
        interactions_current,
        output,
    );

    for (index, tab) in plan.tabs.iter().enumerate() {
        paint_tab(
            &mut tab_ui,
            instance_id,
            plan,
            tab,
            index,
            workspace,
            style,
            status,
            active_drag,
            interactions_current,
            output,
        );
    }
    drop(tab_ui);

    paint_selected_panel(ui, instance_id, plan, panes, style, interactions_current);
}

#[allow(clippy::too_many_arguments)]
fn paint_group_grip(
    ui: &mut Ui,
    instance_id: Id,
    tabs: &TabsPlan,
    workspace: &Workspace,
    style: &DockStyle,
    status: InteractionStatus,
    active_drag: Option<ActiveDragView<'_>>,
    interactions_current: bool,
    output: &mut RenderOutput,
) {
    if !tabs.group_drag_rect.is_positive() {
        return;
    }
    let id = ui.make_persistent_id((instance_id, "tab-group-grip", tabs.root, tabs.node));
    let response = ui
        .interact(interact_rect(tabs.group_drag_rect), id, Sense::DRAG)
        .on_hover_text("Drag tab group");
    configure_group_grip_accessibility(ui, id, tabs.group_drag_rect);

    let active = interactions_current && (response.hovered() || response.dragged());
    if active {
        ui.ctx().set_cursor_icon(if response.dragged() {
            CursorIcon::Grabbing
        } else {
            CursorIcon::Grab
        });
    }
    paint_group_grip_icon(ui, tabs.group_drag_rect, style, active);

    if interactions_current
        && status == InteractionStatus::Idle
        && response.is_pointer_button_down_on()
        && ui.input(|input| input.pointer.button_down(PointerButton::Primary))
        && press_origin_is_inside(ui, tabs.group_drag_rect)
        && let Some(source) = output.capture(workspace.capture_node_source(tabs.root, tabs.node))
    {
        output.push(RenderAction::ArmDrag(MovePayload::Tabs(source)));
    }
    if interactions_current
        && (response.drag_started_by(PointerButton::Primary)
            || response.dragged_by(PointerButton::Primary))
        && press_origin_is_inside(ui, tabs.group_drag_rect)
    {
        maybe_begin_group_drag(active_drag, tabs, output);
    }
}

fn paint_group_grip_icon(ui: &Ui, rect: Rect, style: &DockStyle, active: bool) {
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

fn configure_group_grip_accessibility(ui: &Ui, id: Id, rect: Rect) {
    ui.ctx().accesskit_node_builder(id, |node| {
        node.set_role(Role::Button);
        node.set_bounds(accesskit_bounds(rect));
        node.set_label("Drag tab group");
    });
}

#[allow(clippy::too_many_arguments)]
fn paint_tab(
    ui: &mut Ui,
    instance_id: Id,
    tabs: &TabsPlan,
    tab: &TabPlan,
    index: usize,
    workspace: &Workspace,
    style: &DockStyle,
    status: InteractionStatus,
    active_drag: Option<ActiveDragView<'_>>,
    interactions_current: bool,
    output: &mut RenderOutput,
) {
    let tab_id = tab_id(ui, instance_id, tab.item);
    let drag_rect = tab.close_rect.map_or(tab.rect, |close| {
        Rect::from_min_max(tab.rect.min, pos2(close.min.x, tab.rect.max.y))
    });
    let response = ui.interact(interact_rect(drag_rect), tab_id, Sense::click_and_drag());
    configure_tab_accessibility(ui, &response, instance_id, tab);

    let hovered = interactions_current && response.hovered();
    let focused = response.has_focus();
    let retained_focus = focused && ui.memory(|memory| memory.had_focus_last_frame(response.id));
    if focused {
        lock_tab_navigation_focus(ui, response.id);
    }
    if interactions_current && (response.hovered() || response.dragged()) {
        ui.ctx().set_cursor_icon(if response.dragged() {
            CursorIcon::Grabbing
        } else {
            CursorIcon::Grab
        });
    }
    let fill = if tab.selected {
        style.tab_active_fill
    } else if hovered {
        style.tab_hover_fill
    } else {
        style.tab_fill
    };
    let text_color = if tab.selected {
        style.tab_active_text_color
    } else {
        style.tab_text_color
    };
    ui.painter().rect_filled(tab.rect, 0.0, fill);
    if focused {
        ui.painter().rect_stroke(
            tab.rect.shrink(1.0),
            0.0,
            Stroke::new(1.0, style.drop_border_color),
            StrokeKind::Inside,
        );
    }
    let text_painter = ui.painter_at(tab.text_rect);
    text_painter.galley_with_override_text_color(
        pos2(
            tab.text_rect.min.x,
            tab.text_rect.center().y - tab.galley.size().y * 0.5,
        ),
        tab.galley.clone(),
        text_color,
    );

    if let Some(close_rect) = tab.close_rect {
        paint_close_button(
            ui,
            tabs,
            tab,
            close_rect,
            workspace,
            instance_id,
            style,
            interactions_current,
            output,
        );
    }

    let activated = semantic_activation(ui, &response, drag_rect, interactions_current);
    if activated {
        response.request_focus();
    }
    if (activated
        || accesskit_focus_requested(ui, &response)
        || interactions_current && response.gained_focus())
        && !tab.selected
        && let Some(source) =
            output.capture(workspace.capture_item_source(tabs.root, tabs.node, tab.item))
    {
        output.push(RenderAction::Select(source));
    }

    if interactions_current
        && status == InteractionStatus::Idle
        && response.is_pointer_button_down_on()
        && ui.input(|input| input.pointer.button_down(PointerButton::Primary))
        && press_origin_is_inside(ui, drag_rect)
        && let Some(source) =
            output.capture(workspace.capture_item_source(tabs.root, tabs.node, tab.item))
    {
        output.push(RenderAction::ArmDrag(MovePayload::Item(source)));
    }

    if interactions_current
        && (response.drag_started_by(PointerButton::Primary)
            || response.dragged_by(PointerButton::Primary))
        && press_origin_is_inside(ui, drag_rect)
    {
        maybe_begin_drag(active_drag, tabs, tab.item, output);
    }
    if retained_focus {
        keyboard_select(ui, instance_id, tabs, index, workspace, output);
    }
}

#[allow(clippy::too_many_arguments)]
fn paint_close_button(
    ui: &mut Ui,
    tabs: &TabsPlan,
    tab: &TabPlan,
    rect: Rect,
    workspace: &Workspace,
    instance_id: Id,
    style: &DockStyle,
    interactions_current: bool,
    output: &mut RenderOutput,
) {
    let response = ui.interact(
        interact_rect(rect),
        ui.make_persistent_id((instance_id, "tab-close", tab.item)),
        Sense::click(),
    );
    configure_button_accessibility(ui, &response, rect, format!("Close {}", tab.title));
    let color =
        if tab.selected || interactions_current && (response.hovered() || response.has_focus()) {
            style.tab_active_text_color
        } else {
            style.tab_text_color
        };
    if interactions_current && response.hovered() {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }
    let radius = rect.width().min(rect.height()) * 0.22;
    let center = rect.center();
    ui.painter().line_segment(
        [
            center - egui::vec2(radius, radius),
            center + egui::vec2(radius, radius),
        ],
        Stroke::new(1.25, color),
    );
    ui.painter().line_segment(
        [
            center + egui::vec2(-radius, radius),
            center + egui::vec2(radius, -radius),
        ],
        Stroke::new(1.25, color),
    );

    if semantic_activation(ui, &response, rect, interactions_current)
        && let Some(source) =
            output.capture(workspace.capture_item_source(tabs.root, tabs.node, tab.item))
    {
        output.push(RenderAction::CloseRequested(source));
    }
}

fn maybe_begin_drag(
    active: Option<ActiveDragView<'_>>,
    tabs: &TabsPlan,
    item: ItemId,
    output: &mut RenderOutput,
) {
    let Some(active) = active.filter(|drag| drag.phase() == DragPhase::Armed) else {
        return;
    };
    let MovePayload::Item(source) = active.payload() else {
        return;
    };
    if source.root() == tabs.root && source.tabs() == tabs.node && source.item() == item {
        output.push(RenderAction::BeginDrag {
            session: active.session(),
            pointer: PRIMARY_POINTER,
            button: PRIMARY_BUTTON,
        });
    }
}

fn maybe_begin_group_drag(
    active: Option<ActiveDragView<'_>>,
    tabs: &TabsPlan,
    output: &mut RenderOutput,
) {
    let Some(active) = active.filter(|drag| drag.phase() == DragPhase::Armed) else {
        return;
    };
    let MovePayload::Tabs(source) = active.payload() else {
        return;
    };
    if source.root() == tabs.root && source.node() == tabs.node {
        output.push(RenderAction::BeginDrag {
            session: active.session(),
            pointer: PRIMARY_POINTER,
            button: PRIMARY_BUTTON,
        });
    }
}

fn keyboard_select(
    ui: &Ui,
    instance_id: Id,
    tabs: &TabsPlan,
    current: usize,
    workspace: &Workspace,
    output: &mut RenderOutput,
) {
    if tabs.tabs.len() < 2 {
        return;
    }
    let next = ui.input(|input| {
        if input.key_pressed(Key::ArrowLeft) {
            Some((current.checked_sub(1).unwrap_or(tabs.tabs.len() - 1), true))
        } else if input.key_pressed(Key::ArrowRight) {
            Some(((current + 1) % tabs.tabs.len(), true))
        } else if input.key_pressed(Key::Home) {
            Some((0, false))
        } else if input.key_pressed(Key::End) {
            Some((tabs.tabs.len() - 1, false))
        } else {
            None
        }
    });
    let Some((next, clear_cardinal_navigation)) = next.filter(|(next, _)| *next != current) else {
        return;
    };
    if clear_cardinal_navigation {
        ui.memory_mut(|memory| memory.move_focus(FocusDirection::None));
    }
    let next_tab = &tabs.tabs[next];
    if let Some(source) =
        output.capture(workspace.capture_item_source(tabs.root, tabs.node, next_tab.item))
    {
        output.push(RenderAction::Select(source));
        ui.ctx().memory_mut(|memory| {
            memory.request_focus(tab_id(ui, instance_id, next_tab.item));
        });
    }
}

fn lock_tab_navigation_focus(ui: &Ui, id: Id) {
    ui.memory_mut(|memory| {
        memory.set_focus_lock_filter(
            id,
            EventFilter {
                horizontal_arrows: true,
                ..Default::default()
            },
        );
    });
}

fn press_origin_is_inside(ui: &Ui, rect: Rect) -> bool {
    ui.input(|input| {
        input
            .pointer
            .press_origin()
            .is_some_and(|point| contains_half_open(rect, point))
    })
}

fn paint_selected_panel(
    ui: &mut Ui,
    instance_id: Id,
    plan: &TabsPlan,
    panes: &mut dyn PaneView,
    style: &DockStyle,
    interactions_current: bool,
) {
    let Some(tab) = plan.tabs.iter().find(|tab| tab.selected) else {
        return;
    };
    let mut panel_ui = ui.new_child(
        UiBuilder::new()
            .id(Id::new((instance_id, "pane", tab.item)))
            .max_rect(plan.content_rect),
    );
    panel_ui.set_clip_rect(plan.content_rect.intersect(ui.clip_rect()));
    panel_ui.set_min_size(plan.content_rect.size());
    panel_ui
        .ctx()
        .accesskit_node_builder(panel_ui.id(), |node| {
            node.set_role(Role::TabPanel);
            node.set_bounds(accesskit_bounds(plan.content_rect));
            node.set_label(tab.title.clone());
        });

    if tab.missing {
        paint_centered_label(
            &panel_ui,
            plan.content_rect,
            format!("Pane {} is unavailable", tab.item.get()),
            style.tab_text_color,
        );
    } else if interactions_current {
        panes.ui(tab.item, &mut panel_ui);
    }
}

fn configure_tab_list_accessibility(ui: &Ui, instance_id: Id, plan: &TabsPlan) {
    let selected_id = plan
        .selected
        .map(|item| tab_id(ui, instance_id, item).accesskit_id());
    ui.ctx().accesskit_node_builder(ui.id(), |node| {
        node.set_role(Role::TabList);
        node.set_bounds(accesskit_bounds(plan.tab_bar_rect));
        node.set_orientation(Orientation::Horizontal);
        if let Some(selected_id) = selected_id {
            node.set_active_descendant(selected_id);
        }
    });
}

fn configure_tab_accessibility(ui: &Ui, response: &egui::Response, instance_id: Id, tab: &TabPlan) {
    let panel_id = Id::new((instance_id, "pane", tab.item));
    ui.ctx().accesskit_node_builder(response.id, |node| {
        node.set_role(Role::Tab);
        node.set_bounds(accesskit_bounds(response.rect));
        node.set_label(tab.title.clone());
        node.add_action(Action::Focus);
        node.add_action(Action::Click);
        node.push_controlled(panel_id.accesskit_id());
        if tab.selected {
            node.set_selected(true);
        } else {
            node.clear_selected();
        }
    });
}

fn configure_button_accessibility(ui: &Ui, response: &egui::Response, rect: Rect, label: String) {
    ui.ctx().accesskit_node_builder(response.id, |node| {
        node.set_role(Role::Button);
        node.set_bounds(accesskit_bounds(rect));
        node.set_label(label);
        node.add_action(Action::Focus);
        node.add_action(Action::Click);
    });
}

fn tab_id(ui: &Ui, instance_id: Id, item: ItemId) -> Id {
    ui.make_persistent_id((instance_id, "tab", item))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn close_button_never_overlaps_tab_drag_rect() {
        let tab = Rect::from_min_max(egui::Pos2::new(0.0, 0.0), egui::Pos2::new(100.0, 28.0));
        let close = Rect::from_min_max(egui::Pos2::new(78.0, 6.0), egui::Pos2::new(94.0, 22.0));
        let drag = Rect::from_min_max(tab.min, pos2(close.min.x, tab.max.y));

        assert!(drag.max.x <= close.min.x);
        assert!(!drag.intersect(close).is_positive());
    }
}
