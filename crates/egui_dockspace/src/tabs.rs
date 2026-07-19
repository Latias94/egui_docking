//! Tab chrome, keyboard navigation, accessibility, and pane dispatch.

use dockspace::command::MovePayload;
use dockspace::graph::Workspace;
use dockspace::ids::{ItemId, SurfaceId};
use dockspace::interaction::{ActiveDragView, DragPhase, InteractionStatus};
use egui::accesskit::{Action, HasPopup, Orientation, Role};
use egui::emath::GuiRounding as _;
use egui::scroll_area::ScrollBarVisibility;
use egui::{
    Align, CursorIcon, Event, EventFilter, FocusDirection, Frame, Id, Key, Layout, MouseWheelUnit,
    PointerButton, Popup, PopupCloseBehavior, Rect, RectAlign, Response, ScrollArea, Sense,
    SetOpenCommand, Stroke, StrokeKind, Ui, UiBuilder, pos2,
};

use crate::hit::{
    accesskit_focus_requested, contains_half_open, interact_rect, semantic_activation,
};
use crate::pane::PaneView;
use crate::projection::{
    OverflowMenuGeometry, OverflowMenuItemGeometry, TabPlan, TabRevealIdentity, TabStripKey,
    TabsPlan, complete_tab_keyboard_focus, consume_overflow_menu_toggle,
    overflow_menu_opened_in_frame, overflow_menu_requested_open, request_tab_keyboard_focus,
    set_overflow_menu_geometry, set_tab_strip_scroll, set_tab_strip_scroll_preserving_reveals,
    sync_tab_reveal_identity,
};
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
    tab_scroll_owner: Option<TabStripKey>,
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
    paint_overflow_control(
        &mut tab_ui,
        instance_id,
        plan,
        workspace,
        style,
        interactions_current,
        output,
    );
    let reveal_identity = current_tab_reveal_identity(&tab_ui, instance_id, plan, active_drag);
    if plan.overflow_rect.is_some()
        && sync_tab_reveal_identity(&tab_ui, instance_id, plan, reveal_identity)
    {
        tab_ui
            .ctx()
            .request_discard("dockspace tab reveal identity changed");
        tab_ui.ctx().request_repaint();
    }
    update_tab_strip_scroll(
        &mut tab_ui,
        instance_id,
        plan,
        style,
        active_drag,
        reveal_identity,
        tab_scroll_owner,
        interactions_current,
    );
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

fn paint_overflow_control(
    ui: &mut Ui,
    instance_id: Id,
    tabs: &TabsPlan,
    workspace: &Workspace,
    style: &DockStyle,
    interactions_current: bool,
    output: &mut RenderOutput,
) {
    let id = ui.make_persistent_id((instance_id, "tab-overflow", tabs.root, tabs.node));
    let Some(rect) = tabs.overflow_rect.filter(Rect::is_positive) else {
        Popup::close_id(ui.ctx(), id.with("popup"));
        return;
    };
    let response = paint_overflow_button(ui, id, rect, style, interactions_current);

    let popup_id = Popup::default_response_id(&response);
    let Some(placement) = overflow_menu_placement(ui, rect, tabs) else {
        close_unavailable_overflow_menu(ui, instance_id, tabs, id, popup_id);
        return;
    };
    let popup_was_open = Popup::is_id_open(ui.ctx(), popup_id);
    let frame = ui.ctx().cumulative_frame_nr();
    let keyboard_activation = response.has_focus()
        && ui.input(|input| input.key_pressed(Key::Enter) || input.key_pressed(Key::Space));
    let toggled = semantic_activation(ui, &response, rect, interactions_current)
        && consume_overflow_menu_toggle(ui, instance_id, tabs, frame, !popup_was_open);
    if toggled && keyboard_activation {
        consume_overflow_toggle_keys(ui);
    }
    let opened_this_frame = toggled && !popup_was_open;
    let adapter_open_after_toggle = overflow_menu_requested_open(ui, instance_id, tabs);
    let externally_closed = !toggled
        && adapter_open_after_toggle
        && !popup_was_open
        && set_overflow_menu_geometry(ui, instance_id, tabs, false, None);
    let requested_open = overflow_menu_requested_open(ui, instance_id, tabs);
    let popup_anchor = placement.anchor;
    let layout = placement.layout;
    let popup_response = Popup::menu(&response)
        .anchor(popup_anchor)
        .open_memory(Some(SetOpenCommand::Bool(requested_open)))
        .close_behavior(PopupCloseBehavior::IgnoreClicks)
        .align(layout.align)
        .align_alternatives(&[])
        .layout(Layout::top_down(Align::Min))
        .width(layout.outer_width)
        .show(|menu_ui| {
            paint_overflow_menu(menu_ui, instance_id, tabs, style, layout, opened_this_frame)
        });
    if let Some(popup) = &popup_response {
        ui.ctx()
            .accesskit_node_builder(popup.inner.menu_id, |node| {
                node.set_bounds(accesskit_bounds(popup.response.rect));
            });
    }
    let actual_geometry = popup_response.as_ref().map(overflow_menu_geometry);
    let popup_geometry_current = tabs.overflow_menu_geometry.as_ref() == actual_geometry.as_ref();
    let item_geometry_current = interactions_current && popup_geometry_current;
    let item_activated = popup_response
        .as_ref()
        .zip(actual_geometry.as_ref())
        .is_some_and(|(popup, geometry)| {
            activate_overflow_menu_item(
                ui,
                instance_id,
                tabs,
                workspace,
                &popup.inner.items,
                geometry,
                item_geometry_current,
                output,
            )
        });
    if let Some(popup) = &popup_response {
        consume_popup_wheel_residue(ui, popup.response.rect);
    }
    if item_activated {
        Popup::close_id(ui.ctx(), popup_id);
    }
    if popup_geometry_current
        && !item_activated
        && !overflow_menu_opened_in_frame(ui, instance_id, tabs, frame)
        && actual_geometry
            .as_ref()
            .is_some_and(|geometry| pointer_clicked_outside(ui, geometry.popup_rect))
    {
        Popup::close_id(ui.ctx(), popup_id);
    }
    let expanded = Popup::is_id_open(ui.ctx(), popup_id);
    let stored_geometry = expanded.then_some(actual_geometry).flatten();
    let geometry_changed =
        set_overflow_menu_geometry(ui, instance_id, tabs, expanded, stored_geometry);
    if toggled || externally_closed || geometry_changed {
        ui.ctx()
            .request_discard("dockspace overflow menu geometry changed");
        ui.ctx().request_repaint();
    }
    ui.ctx().accesskit_node_builder(id, |node| {
        node.set_has_popup(HasPopup::Menu);
        node.set_expanded(expanded);
    });
}

fn overflow_menu_geometry(
    popup: &egui::InnerResponse<OverflowMenuPaintOutput>,
) -> OverflowMenuGeometry {
    let viewport_rect = popup.inner.viewport_rect.intersect(popup.response.rect);
    OverflowMenuGeometry {
        popup_rect: popup.response.rect,
        viewport_rect,
        items: popup
            .inner
            .items
            .iter()
            .map(|item| OverflowMenuItemGeometry {
                item: item.item,
                index: item.index,
                actual_rect: item.response.rect,
                hit_rect: clipped_hit_rect(item.response.rect, viewport_rect),
            })
            .collect(),
    }
}

fn paint_overflow_button(
    ui: &mut Ui,
    id: Id,
    rect: Rect,
    style: &DockStyle,
    interactions_current: bool,
) -> Response {
    let response = ui
        .interact(interact_rect(rect), id, Sense::click())
        .on_hover_text("Show hidden tabs");
    configure_button_accessibility(ui, &response, rect, "Show hidden tabs".to_owned());
    let hovered = interactions_current && (response.hovered() || response.has_focus());
    ui.painter().rect_filled(
        rect,
        0.0,
        if hovered {
            style.tab_hover_fill
        } else {
            style.tab_bar_fill
        },
    );
    let color = if hovered {
        style.tab_active_text_color
    } else {
        style.tab_text_color
    };
    let spacing = (rect.width() * 0.18).min(4.0);
    let radius = (spacing * 0.28).min(1.25);
    for x in [-1.0, 0.0, 1.0] {
        ui.painter()
            .circle_filled(rect.center() + egui::vec2(x * spacing, 0.0), radius, color);
    }
    response
}

fn consume_overflow_toggle_keys(ui: &Ui) {
    let modifiers = ui.input(|input| input.modifiers);
    ui.input_mut(|input| {
        input.consume_key(modifiers, Key::Enter);
        input.consume_key(modifiers, Key::Space);
    });
}

fn consume_popup_wheel_residue(ui: &Ui, popup_rect: Rect) {
    let owns_pointer = ui
        .input(|input| input.pointer.hover_pos())
        .is_some_and(|pointer| contains_half_open(popup_rect, pointer));
    if owns_pointer {
        ui.input_mut(|input| input.smooth_scroll_delta = egui::Vec2::ZERO);
    }
}

fn close_unavailable_overflow_menu(
    ui: &Ui,
    instance_id: Id,
    tabs: &TabsPlan,
    button_id: Id,
    popup_id: Id,
) {
    let popup_was_open = Popup::is_id_open(ui.ctx(), popup_id);
    Popup::close_id(ui.ctx(), popup_id);
    let adapter_changed = set_overflow_menu_geometry(ui, instance_id, tabs, false, None);
    if popup_was_open || adapter_changed {
        ui.ctx()
            .request_discard("dockspace overflow menu became unavailable");
        ui.ctx().request_repaint();
    }
    ui.ctx().accesskit_node_builder(button_id, |node| {
        node.set_has_popup(HasPopup::Menu);
        node.set_expanded(false);
    });
}

#[derive(Clone, Copy)]
struct OverflowMenuLayout {
    align: RectAlign,
    content_width: f32,
    scroll_width: f32,
    outer_width: f32,
    max_content_height: f32,
    show_scrollbar: bool,
}

#[derive(Clone, Copy)]
struct OverflowMenuPlacement {
    anchor: Rect,
    layout: OverflowMenuLayout,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct OverflowWidthBudget {
    content: f32,
    scroll: f32,
    outer: f32,
}

struct OverflowMenuPaintOutput {
    menu_id: Id,
    viewport_rect: Rect,
    items: Vec<OverflowMenuItemPaint>,
}

struct OverflowMenuItemPaint {
    item: ItemId,
    index: usize,
    response: Response,
}

#[derive(Clone, Copy)]
struct OverflowMenuItemContext<'a> {
    instance_id: Id,
    tabs: &'a TabsPlan,
    style: &'a DockStyle,
    content_width: f32,
}

fn overflow_menu_placement(
    ui: &Ui,
    anchor: Rect,
    tabs: &TabsPlan,
) -> Option<OverflowMenuPlacement> {
    let pixels_per_point = ui.ctx().pixels_per_point();
    let content_rect = strict_popup_bounds(ui.ctx().content_rect(), pixels_per_point)?;
    let anchor = anchor.intersect(content_rect);
    if !anchor.is_positive() {
        return None;
    }
    let layout = overflow_menu_layout(ui, content_rect, anchor, tabs)?;
    Some(OverflowMenuPlacement { anchor, layout })
}

fn overflow_menu_layout(
    ui: &Ui,
    content_rect: Rect,
    anchor: Rect,
    tabs: &TabsPlan,
) -> Option<OverflowMenuLayout> {
    let popup_style = ui.ctx().global_style();
    let item_spacing = finite_non_negative(popup_style.spacing.item_spacing.y);
    let hidden = tabs
        .tabs
        .iter()
        .filter(|tab| tab.visible_rect != Some(tab.rect));
    let (_, desired_content_height) = hidden.fold((0_usize, 0.0_f32), |(count, height), tab| {
        let preceding_spacing = if count == 0 { 0.0 } else { item_spacing };
        (
            count + 1,
            height + preceding_spacing + tab.overflow_menu_size.y,
        )
    });
    let frame_margin = Frame::popup(&popup_style).total_margin();
    let frame_height = finite_non_negative(frame_margin.top + frame_margin.bottom);
    let frame_width = finite_non_negative(frame_margin.left + frame_margin.right);
    if content_rect.width() <= frame_width {
        return None;
    }
    let desired_outer_height = desired_content_height + frame_height;
    let below = finite_non_negative(content_rect.max.y - anchor.max.y);
    let above = finite_non_negative(anchor.min.y - content_rect.min.y);
    let (align, available_height) = if desired_outer_height <= below || below >= above {
        (RectAlign::BOTTOM_END, below)
    } else {
        (RectAlign::TOP_END, above)
    };
    if available_height <= frame_height {
        return None;
    }
    let max_content_height = finite_strict_floor_ui_extent(available_height - frame_height);
    let show_scrollbar = desired_content_height > max_content_height;
    let scrollbar_width = if show_scrollbar {
        finite_non_negative(popup_style.spacing.scroll.allocated_width())
    } else {
        0.0
    };
    if content_rect.width() <= frame_width + scrollbar_width {
        return None;
    }
    let desired_content_width = tabs
        .tabs
        .iter()
        .filter(|tab| tab.visible_rect != Some(tab.rect))
        .map(|tab| tab.overflow_menu_size.x)
        .reduce(f32::max)
        .unwrap_or_default();
    let width = overflow_width_budget(
        desired_content_width,
        anchor.width() - frame_width - scrollbar_width,
        scrollbar_width,
        content_rect.width(),
        frame_width,
    );
    if width.content <= 0.0 || width.scroll < scrollbar_width {
        return None;
    }
    Some(OverflowMenuLayout {
        align,
        content_width: width.content,
        scroll_width: width.scroll,
        outer_width: width.outer,
        max_content_height,
        show_scrollbar,
    })
}

fn finite_non_negative(value: f32) -> f32 {
    if value.is_finite() {
        value.max(0.0)
    } else {
        0.0
    }
}

fn strict_popup_bounds(mut bounds: Rect, pixels_per_point: f32) -> Option<Rect> {
    let half_pixel = if pixels_per_point.is_finite() && pixels_per_point > 0.0 {
        0.5 / pixels_per_point
    } else {
        0.0
    };
    let inset = half_pixel + egui::emath::GUI_ROUNDING;
    bounds.min += egui::Vec2::splat(inset);
    bounds.max -= egui::Vec2::splat(inset);
    bounds.is_positive().then_some(bounds)
}

fn finite_strict_floor_ui_extent(value: f32) -> f32 {
    let value = finite_non_negative(value);
    if value > 0.0 {
        // Keep the reassembled outer edge inside the host after egui rounds to its UI grid.
        value.next_down().floor_ui()
    } else {
        0.0
    }
}

fn finite_ceil_ui_extent(value: f32) -> f32 {
    -(-finite_non_negative(value)).floor_ui()
}

fn overflow_width_budget(
    desired_content: f32,
    minimum_content: f32,
    scrollbar: f32,
    available_outer: f32,
    frame: f32,
) -> OverflowWidthBudget {
    let scrollbar = finite_non_negative(scrollbar);
    let frame = finite_non_negative(frame);
    let available_outer = finite_non_negative(available_outer);
    let available_content = finite_strict_floor_ui_extent(available_outer - frame - scrollbar);
    let content = finite_non_negative(desired_content)
        .max(finite_non_negative(minimum_content))
        .min(available_content);
    let max_scroll = finite_strict_floor_ui_extent(available_outer - frame);
    let scroll = finite_ceil_ui_extent(content + scrollbar).min(max_scroll);
    let content = content.min(finite_non_negative(scroll - scrollbar));
    OverflowWidthBudget {
        content,
        scroll,
        outer: scroll + frame,
    }
}

fn paint_overflow_menu(
    menu_ui: &mut Ui,
    instance_id: Id,
    tabs: &TabsPlan,
    style: &DockStyle,
    layout: OverflowMenuLayout,
    opened_this_frame: bool,
) -> OverflowMenuPaintOutput {
    menu_ui.set_width(layout.scroll_width);
    let scroll_salt = (
        "overflow-menu-scroll",
        tabs.key.surface,
        tabs.key.root,
        tabs.key.node,
    );
    let scroll_id = menu_ui.make_persistent_id(egui::IdSalt::new(scroll_salt));
    // Solid scrollbar space is semantic layout, so it must not inherit egui's visual fade.
    let _ = menu_ui
        .ctx()
        .animate_bool_with_time(scroll_id.with("h"), false, 0.0);
    let _ = menu_ui
        .ctx()
        .animate_bool_with_time(scroll_id.with("v"), layout.show_scrollbar, 0.0);
    let scroll = ScrollArea::vertical()
        .id_salt(scroll_salt)
        .scroll_bar_visibility(if layout.show_scrollbar {
            ScrollBarVisibility::AlwaysVisible
        } else {
            ScrollBarVisibility::AlwaysHidden
        })
        .max_width(layout.scroll_width)
        .max_height(layout.max_content_height)
        .min_scrolled_height(layout.max_content_height)
        .content_margin(0)
        .animated(false)
        .show(menu_ui, |items_ui| {
            let context = OverflowMenuItemContext {
                instance_id,
                tabs,
                style,
                content_width: layout.content_width,
            };
            let mut items = tabs
                .tabs
                .iter()
                .enumerate()
                .filter(|(_, tab)| tab.visible_rect != Some(tab.rect))
                .enumerate()
                .map(|(hidden_index, (index, tab))| {
                    paint_overflow_menu_item(
                        items_ui,
                        context,
                        tab,
                        index,
                        opened_this_frame && hidden_index == 0,
                    )
                })
                .collect::<Vec<_>>();
            navigate_overflow_menu(items_ui, &mut items);
            items
        });
    let viewport_rect = scroll
        .inner_rect
        .intersect(menu_ui.clip_rect())
        .intersect(menu_ui.ctx().content_rect());
    menu_ui.ctx().accesskit_node_builder(menu_ui.id(), |node| {
        node.set_role(Role::Menu);
        node.set_bounds(accesskit_bounds(viewport_rect));
        node.set_clips_children();
        node.add_child_action(Action::ScrollIntoView);
    });
    OverflowMenuPaintOutput {
        menu_id: menu_ui.id(),
        viewport_rect,
        items: scroll.inner,
    }
}

fn paint_overflow_menu_item(
    ui: &mut Ui,
    context: OverflowMenuItemContext<'_>,
    tab: &TabPlan,
    index: usize,
    focus_on_open: bool,
) -> OverflowMenuItemPaint {
    let item_size = egui::vec2(context.content_width, tab.overflow_menu_size.y);
    let (_, rect) = ui.allocate_space(item_size);
    let id = overflow_menu_item_id(context.instance_id, context.tabs.key, tab.item);
    let response = ui.interact(interact_rect(rect), id, Sense::click());
    ui.ctx().accesskit_node_builder(id, |node| {
        node.set_role(Role::MenuItem);
        node.set_bounds(accesskit_bounds(rect));
        node.set_label(tab.title.clone());
        node.set_author_id(format!(
            "overflow-tab:{}:{}:{:?}:{}",
            context.tabs.key.surface, context.tabs.key.root, context.tabs.key.node, tab.item
        ));
        node.add_action(Action::Focus);
        node.add_action(Action::Click);
        node.add_action(Action::ScrollIntoView);
        if tab.selected {
            node.set_selected(true);
        } else {
            node.clear_selected();
        }
    });
    let visuals = ui.style().interact_selectable(&response, tab.selected);
    if tab.selected || response.hovered() || response.has_focus() {
        ui.painter()
            .rect_filled(rect, visuals.corner_radius, visuals.weak_bg_fill);
    }
    ui.painter().galley_with_override_text_color(
        pos2(
            rect.min.x + context.style.tab_horizontal_padding,
            rect.center().y - tab.galley.size().y * 0.5,
        ),
        tab.galley.clone(),
        visuals.text_color(),
    );
    if focus_on_open || response.gained_focus() || accesskit_focus_requested(ui, &response) {
        if focus_on_open {
            response.request_focus();
        }
        response.scroll_to_me(None);
    }
    OverflowMenuItemPaint {
        item: tab.item,
        index,
        response,
    }
}

fn navigate_overflow_menu(ui: &Ui, items: &mut [OverflowMenuItemPaint]) {
    let Some(current) = items.iter().position(|item| item.response.has_focus()) else {
        return;
    };
    lock_overflow_menu_focus(ui, items[current].response.id);
    let target = ui.input_mut(|input| {
        if input.consume_key(egui::Modifiers::NONE, Key::ArrowDown) {
            Some((current + 1).min(items.len() - 1))
        } else if input.consume_key(egui::Modifiers::NONE, Key::ArrowUp) {
            Some(current.saturating_sub(1))
        } else if input.consume_key(egui::Modifiers::NONE, Key::Home) {
            Some(0)
        } else if input.consume_key(egui::Modifiers::NONE, Key::End) {
            Some(items.len() - 1)
        } else {
            None
        }
    });
    let Some(target) = target.filter(|target| *target != current) else {
        return;
    };
    ui.memory_mut(|memory| memory.move_focus(FocusDirection::None));
    items[target].response.request_focus();
    items[target].response.scroll_to_me(None);
}

fn lock_overflow_menu_focus(ui: &Ui, id: Id) {
    ui.memory_mut(|memory| {
        memory.set_focus_lock_filter(
            id,
            EventFilter {
                vertical_arrows: true,
                ..Default::default()
            },
        );
    });
}

#[allow(clippy::too_many_arguments)]
fn activate_overflow_menu_item(
    ui: &Ui,
    instance_id: Id,
    tabs: &TabsPlan,
    workspace: &Workspace,
    paints: &[OverflowMenuItemPaint],
    geometry: &OverflowMenuGeometry,
    geometry_current: bool,
    output: &mut RenderOutput,
) -> bool {
    for (paint, item_geometry) in paints.iter().zip(&geometry.items) {
        let hit_rect = item_geometry.hit_rect.unwrap_or(Rect::NOTHING);
        if !semantic_activation(
            ui,
            &paint.response,
            hit_rect,
            geometry_current && item_geometry.hit_rect.is_some(),
        ) {
            continue;
        }
        let tab = &tabs.tabs[paint.index];
        let reveal_offset = tab_reveal_offset(tabs, tab);
        if set_tab_strip_scroll(ui, instance_id, tabs, reveal_offset) {
            ui.ctx().request_repaint();
        }
        if !tab.selected
            && let Some(source) =
                output.capture(workspace.capture_item_source(tabs.root, tabs.node, tab.item))
        {
            output.push(RenderAction::Select(source));
        }
        return true;
    }
    false
}

fn clipped_hit_rect(actual_rect: Rect, viewport_rect: Rect) -> Option<Rect> {
    let clipped = actual_rect.intersect(viewport_rect);
    clipped.is_positive().then_some(clipped)
}

fn pointer_clicked_outside(ui: &Ui, popup_rect: Rect) -> bool {
    ui.input(|input| {
        input.pointer.any_click()
            && input
                .pointer
                .interact_pos()
                .is_some_and(|position| !contains_half_open(popup_rect, position))
    })
}

fn overflow_menu_item_id(instance_id: Id, key: TabStripKey, item: ItemId) -> Id {
    Id::new((
        "egui_dockspace",
        instance_id,
        "overflow-tab",
        key.surface,
        key.root,
        key.node,
        item,
    ))
}

fn tab_reveal_offset(tabs: &TabsPlan, tab: &TabPlan) -> f32 {
    if tab.rect.width() > tabs.tab_viewport_rect.width()
        || tab.rect.min.x < tabs.tab_viewport_rect.min.x
    {
        tabs.scroll_offset + tab.rect.min.x - tabs.tab_viewport_rect.min.x
    } else if tab.rect.max.x > tabs.tab_viewport_rect.max.x {
        tabs.scroll_offset + tab.rect.max.x - tabs.tab_viewport_rect.max.x
    } else {
        tabs.scroll_offset
    }
}

fn current_tab_reveal_identity(
    ui: &Ui,
    instance_id: Id,
    tabs: &TabsPlan,
    active_drag: Option<ActiveDragView<'_>>,
) -> TabRevealIdentity {
    let keyboard_focused = tabs
        .tabs
        .iter()
        .find(|tab| ui.memory(|memory| memory.has_focus(tab_id(ui, instance_id, tab.item))))
        .map(|tab| tab.item);
    let active_dragged = active_drag.and_then(|drag| {
        let MovePayload::Item(source) = drag.payload() else {
            return None;
        };
        (source.root() == tabs.root && source.tabs() == tabs.node).then_some(source.item())
    });
    TabRevealIdentity::new(tabs.selected, keyboard_focused, active_dragged)
}

#[allow(
    clippy::too_many_arguments,
    reason = "scroll reduction explicitly binds widget, drag, reveal, ownership, and authority facts"
)]
fn update_tab_strip_scroll(
    ui: &mut Ui,
    instance_id: Id,
    tabs: &TabsPlan,
    style: &DockStyle,
    active_drag: Option<ActiveDragView<'_>>,
    reveal_identity: TabRevealIdentity,
    tab_scroll_owner: Option<TabStripKey>,
    interactions_current: bool,
) {
    if !interactions_current
        || tab_scroll_owner != Some(tabs.key)
        || tabs.overflow_rect.is_none()
        || !tabs.tab_viewport_rect.is_positive()
    {
        return;
    }
    let Some(pointer) = ui.input(|input| input.pointer.hover_pos()) else {
        return;
    };
    if !contains_half_open(tabs.tab_viewport_rect, pointer) {
        return;
    }

    let mut requested_offset = tabs.scroll_offset;
    let wheel_delta = ui.input_mut(|input| {
        let mut wheel_delta = 0.0;
        input.events.retain(|event| match event {
            Event::MouseWheel {
                unit,
                delta,
                modifiers,
                ..
            } if !modifiers.ctrl && !modifiers.command => {
                let points = match unit {
                    MouseWheelUnit::Point => 1.0,
                    MouseWheelUnit::Line => style.tab_min_width,
                    MouseWheelUnit::Page => tabs.tab_viewport_rect.width(),
                };
                wheel_delta += wheel_axis_delta(*delta) * points;
                false
            }
            _ => true,
        });
        if wheel_delta != 0.0 {
            input.smooth_scroll_delta = egui::Vec2::ZERO;
        }
        wheel_delta
    });
    if wheel_delta != 0.0 {
        requested_offset -= wheel_delta;
    }

    if active_drag.is_some_and(|drag| drag.phase() == DragPhase::Dragging) {
        if tabs
            .scroll_back_rect
            .is_some_and(|rect| contains_half_open(rect, pointer))
        {
            requested_offset -= style.tab_min_width;
        } else if tabs
            .scroll_forward_rect
            .is_some_and(|rect| contains_half_open(rect, pointer))
        {
            requested_offset += style.tab_min_width;
        }
    }

    if set_tab_strip_scroll_preserving_reveals(
        ui,
        instance_id,
        tabs,
        style,
        reveal_identity,
        requested_offset,
    ) {
        ui.ctx().request_repaint();
    }
}

fn wheel_axis_delta(delta: egui::Vec2) -> f32 {
    if delta.x == 0.0 { delta.y } else { delta.x }
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
    let (Some(rect), Some(drag_rect)) = (tab.visible_rect, tab.drag_rect) else {
        return;
    };
    let tab_id = tab_id(ui, instance_id, tab.item);
    let response = ui.interact(interact_rect(drag_rect), tab_id, Sense::click_and_drag());
    if interactions_current && tabs.pending_keyboard_focus == Some(tab.item) {
        response.request_focus();
        complete_tab_keyboard_focus(ui, instance_id, tabs, tab.item);
    }
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
    ui.painter().rect_filled(rect, 0.0, fill);
    if focused {
        ui.painter().rect_stroke(
            rect.shrink(1.0),
            0.0,
            Stroke::new(1.0, style.drop_border_color),
            StrokeKind::Inside,
        );
    }
    let text_painter = ui.painter_at(tab.text_rect);
    text_painter.galley_with_override_text_color(
        pos2(
            tab.rect.min.x + style.tab_horizontal_padding,
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
        let target_requires_reprojection =
            next_tab.visible_rect.is_none() || next_tab.drag_rect.is_none();
        if target_requires_reprojection
            && request_tab_keyboard_focus(ui, instance_id, tabs, next_tab.item)
        {
            ui.ctx()
                .request_discard("dockspace keyboard focus target must be revealed");
            ui.ctx().request_repaint();
        }
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
    } else {
        if !interactions_current {
            panel_ui.style_mut().visuals.disabled_alpha = 1.0;
            panel_ui.disable();
        }
        panes.ui(tab.item, &mut panel_ui);
    }
}

fn configure_tab_list_accessibility(ui: &Ui, instance_id: Id, plan: &TabsPlan) {
    let selected_id = plan
        .selected
        .filter(|item| {
            plan.tabs
                .iter()
                .any(|tab| tab.item == *item && tab.visible_rect.is_some())
        })
        .map(|item| tab_id(ui, instance_id, item).accesskit_id());
    ui.ctx().accesskit_node_builder(ui.id(), |node| {
        node.set_role(Role::TabList);
        node.set_bounds(accesskit_bounds(plan.tab_viewport_rect));
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

    #[test]
    fn horizontal_wheel_axis_has_priority_and_vertical_is_the_fallback() {
        assert!((wheel_axis_delta(egui::vec2(12.0, -30.0)) - 12.0).abs() < f32::EPSILON);
        assert!((wheel_axis_delta(egui::vec2(-12.0, 30.0)) + 12.0).abs() < f32::EPSILON);
        assert!((wheel_axis_delta(egui::vec2(0.0, -30.0)) + 30.0).abs() < f32::EPSILON);
    }

    #[test]
    fn overflow_width_budget_preserves_strict_bounds_without_whole_point_loss() {
        let compact = overflow_width_budget(91.02, 0.0, 0.0, 180.0, 4.0);
        assert!(compact.content >= 91.02);
        assert!(compact.scroll >= compact.content);
        assert!(compact.scroll - compact.content < egui::emath::GUI_ROUNDING);
        assert!(compact.outer <= 180.0);

        let clamped = overflow_width_budget(300.0, 0.0, 17.23, 180.03, 4.0);
        assert!(clamped.content + 17.23 <= clamped.scroll);
        assert!(clamped.outer <= 180.03);
        assert!(180.03 - clamped.outer < egui::emath::GUI_ROUNDING);

        let exact_boundary = overflow_width_budget(300.0, 0.0, 17.23, 180.0, 4.0);
        assert!(exact_boundary.outer < 180.0);
        assert!(180.0 - exact_boundary.outer <= egui::emath::GUI_ROUNDING + f32::EPSILON);
    }

    #[test]
    fn strict_popup_bounds_survive_area_rounding_on_all_edges() {
        let host = Rect::from_min_max(pos2(10.17, 20.23), pos2(190.20, 160.26));
        for pixels_per_point in [0.75, 1.0, 1.25, 1.5, 2.0, 3.0] {
            let strict = strict_popup_bounds(host, pixels_per_point)
                .expect("the fixture host has room for an inset popup");
            let rounded_min = strict.min.round_to_pixels(pixels_per_point).round_ui();
            let rounded_max = strict.max.round_to_pixels(pixels_per_point).round_ui();

            assert!(rounded_min.x > host.min.x);
            assert!(rounded_min.y > host.min.y);
            assert!(rounded_max.x < host.max.x);
            assert!(rounded_max.y < host.max.y);
        }
    }

    #[test]
    fn strict_popup_bounds_reject_a_host_smaller_than_its_rounding_inset() {
        let tiny = Rect::from_min_size(pos2(10.17, 20.23), egui::vec2(1.0, 1.0));

        assert_eq!(strict_popup_bounds(tiny, 1.0), None);
    }
}
