//! Tab-strip overflow controls and the core-owned tab-list popup.

use dockspace::runtime::{
    SurfaceTabListNavigation, TabListMenuBackdropPaintRecord, TabListMenuPaintRecord,
    TabListMenuRowPaintRecord, TabStripControlKind, TabStripControlPaintRecord,
};
use egui::accesskit::{Action, HasPopup, Role};
use egui::{Id, Key, PointerButton, Rect, Sense, Stroke, StrokeKind, pos2, vec2};

use super::RenderContext;
use super::geometry::{accesskit_bounds, egui_rect};

pub(crate) fn paint(context: &mut RenderContext<'_, '_>) {
    paint_controls(context);
    paint_popup(context);
}

fn paint_controls(context: &mut RenderContext<'_, '_>) {
    let controls = context.plan.tab_strip_controls().collect::<Vec<_>>();
    for control in controls {
        let Some(bounds) = egui_rect(control.bounds()) else {
            continue;
        };
        let Some(hit) = egui_rect(control.hit_bounds()) else {
            continue;
        };
        let id = context.ui.make_persistent_id((
            context.instance_id,
            "tab-strip-control",
            control.visual_id(),
        ));
        let response = context.interact_receiver(
            hit,
            id,
            Sense::click(),
            context.plan.receiver_for_tab_strip_control(control),
        );
        paint_control(context, control, bounds, response.hovered());
        configure_control_accessibility(context, control, id, bounds);

        let keyboard = response.has_focus()
            && context.ui.input_mut(|input| {
                input.consume_key(egui::Modifiers::NONE, Key::Enter)
                    || input.consume_key(egui::Modifiers::NONE, Key::Space)
            });
        let accesskit = context
            .ui
            .input(|input| input.has_accesskit_action_request(id, Action::Click));
        let pointer = context.pointer_authority.accepts_local_pointer_actions()
            && response.clicked_by(PointerButton::Primary);
        if control.enabled()
            && (pointer || keyboard || accesskit)
            && let Some(action) = context.plan.prepare_tab_strip_control_activation(control)
        {
            context.push_local_action(action);
        }
    }
}

fn paint_control(
    context: &RenderContext<'_, '_>,
    control: TabStripControlPaintRecord,
    bounds: Rect,
    hovered: bool,
) {
    let fill = if hovered && control.enabled() {
        context.style.tab_hover_fill
    } else {
        context.style.tab_bar_fill
    };
    let color = if control.enabled() {
        context.style.tab_text_color
    } else {
        context.style.tab_text_color.gamma_multiply(0.45)
    };
    context.ui.painter().rect_filled(bounds, 0.0, fill);
    match control.kind() {
        TabStripControlKind::ScrollBackward | TabStripControlKind::ScrollForward => {
            let direction = if control.kind() == TabStripControlKind::ScrollBackward {
                -1.0
            } else {
                1.0
            };
            let center = bounds.center();
            let half = bounds.width().min(bounds.height()) * 0.18;
            context.ui.painter().line_segment(
                [
                    center + vec2(direction * half, -half),
                    center + vec2(-direction * half, 0.0),
                ],
                Stroke::new(1.5, color),
            );
            context.ui.painter().line_segment(
                [
                    center + vec2(-direction * half, 0.0),
                    center + vec2(direction * half, half),
                ],
                Stroke::new(1.5, color),
            );
        }
        TabStripControlKind::TabListMenu => {
            let center = bounds.center();
            let spacing = bounds.width().min(bounds.height()) * 0.16;
            for x in [-1.0, 0.0, 1.0] {
                context
                    .ui
                    .painter()
                    .circle_filled(center + vec2(x * spacing, 0.0), 1.4, color);
            }
        }
    }
}

fn configure_control_accessibility(
    context: &RenderContext<'_, '_>,
    control: TabStripControlPaintRecord,
    id: Id,
    bounds: Rect,
) {
    let menu_open = control.kind() == TabStripControlKind::TabListMenu
        && context
            .plan
            .tab_list_menus()
            .any(|menu| menu.tab_bar_visual_id() == control.tab_bar_visual_id());
    context.ui.ctx().accesskit_node_builder(id, |node| {
        node.set_role(Role::Button);
        node.set_bounds(accesskit_bounds(bounds));
        node.set_label(match control.kind() {
            TabStripControlKind::ScrollBackward => "Scroll tabs backward",
            TabStripControlKind::ScrollForward => "Scroll tabs forward",
            TabStripControlKind::TabListMenu => "Show hidden tabs",
        });
        if control.kind() == TabStripControlKind::TabListMenu {
            node.set_has_popup(HasPopup::Menu);
            node.set_expanded(menu_open);
        }
        if control.enabled() {
            node.add_action(Action::Click);
        } else {
            node.set_disabled();
        }
    });
}

fn paint_popup(context: &mut RenderContext<'_, '_>) {
    let backdrops = context.plan.tab_list_menu_backdrops().collect::<Vec<_>>();
    for backdrop in backdrops {
        paint_backdrop(context, backdrop);
    }

    let menus = context.plan.tab_list_menus().collect::<Vec<_>>();
    for menu in menus {
        paint_menu(context, menu);
    }
}

fn paint_backdrop(context: &mut RenderContext<'_, '_>, backdrop: TabListMenuBackdropPaintRecord) {
    let Some(bounds) = egui_rect(backdrop.bounds()) else {
        return;
    };
    let id = context.ui.make_persistent_id((
        context.instance_id,
        "tab-list-menu-backdrop",
        backdrop.visual_id(),
    ));
    let response = context.interact_receiver(
        bounds,
        id,
        Sense::click(),
        context.plan.receiver_for_tab_list_menu_backdrop(backdrop),
    );
    if context.pointer_authority.accepts_local_pointer_actions()
        && response.clicked_by(PointerButton::Primary)
        && let Some(action) = context.plan.prepare_tab_list_menu_dismiss(backdrop)
    {
        context.push_local_action(action);
    }
}

fn paint_menu(context: &mut RenderContext<'_, '_>, menu: TabListMenuPaintRecord<'_>) {
    let Some(bounds) = egui_rect(menu.bounds()) else {
        return;
    };
    let Some(viewport) = egui_rect(menu.viewport()) else {
        return;
    };
    let frame_id =
        context
            .ui
            .make_persistent_id((context.instance_id, "tab-list-menu", menu.visual_id()));
    let _ = context.interact_receiver(
        bounds,
        frame_id,
        Sense::hover(),
        context.plan.receiver_for_tab_list_menu_frame(menu),
    );
    context.ui.painter().rect(
        bounds,
        3.0,
        context.style.floating_fill,
        Stroke::new(1.0, context.style.floating_border_color),
        StrokeKind::Inside,
    );
    context.ui.ctx().accesskit_node_builder(frame_id, |node| {
        node.set_role(Role::Menu);
        node.set_bounds(accesskit_bounds(bounds));
        node.set_label("Open tabs");
    });

    let rows = menu.rows().collect::<Vec<_>>();
    for row in rows.iter().copied() {
        paint_menu_row(context, menu, row, viewport);
    }
    paint_scrollbar(context, menu, viewport, bounds);
    capture_menu_keyboard(context, menu, &rows);
}

fn paint_menu_row(
    context: &mut RenderContext<'_, '_>,
    menu: TabListMenuPaintRecord<'_>,
    row: TabListMenuRowPaintRecord,
    viewport: Rect,
) {
    let Some(bounds) = egui_rect(row.bounds()) else {
        return;
    };
    let id =
        context
            .ui
            .make_persistent_id((context.instance_id, "tab-list-menu-row", row.visual_id()));
    let response = row.interaction_bounds().and_then(egui_rect).map(|hit| {
        context.interact_receiver(
            hit,
            id,
            Sense::click(),
            context.plan.receiver_for_tab_list_menu_row(row),
        )
    });
    let hovered = response.as_ref().is_some_and(egui::Response::hovered);
    let fill = if row.focused() || hovered {
        context.style.tab_hover_fill
    } else if row.selected() {
        context.style.tab_active_fill
    } else {
        context.style.floating_fill
    };
    let painter = context.ui.painter_at(viewport);
    painter.rect_filled(bounds, 0.0, fill);
    if let Some(resource) = context.resources.item(row.item()) {
        let text_pos = pos2(
            bounds.min.x + 8.0,
            bounds.center().y - resource.galley.size().y * 0.5,
        );
        painter.galley_with_override_text_color(
            text_pos,
            resource.galley.clone(),
            if row.selected() {
                context.style.tab_active_text_color
            } else {
                context.style.tab_text_color
            },
        );
    }
    context.ui.ctx().accesskit_node_builder(id, |node| {
        node.set_role(Role::MenuItem);
        node.set_bounds(accesskit_bounds(bounds));
        node.set_label(context.resources.item(row.item()).map_or_else(
            || format!("Pane {}", row.item().get()),
            |item| item.title.clone(),
        ));
        node.set_selected(row.selected());
        node.add_action(Action::Click);
        node.add_action(Action::Focus);
        node.add_action(Action::ScrollIntoView);
    });
    if row.focused() {
        context.ui.memory_mut(|memory| memory.request_focus(id));
    }

    let pointer = context.pointer_authority.accepts_local_pointer_actions()
        && response
            .as_ref()
            .is_some_and(|response| response.clicked_by(PointerButton::Primary));
    let click = context
        .ui
        .input(|input| input.has_accesskit_action_request(id, Action::Click));
    if pointer || click {
        if let Some(action) = context.plan.prepare_tab_list_menu_row_activation(row) {
            context.push_local_action(action);
        }
        return;
    }
    let focus = context
        .ui
        .input(|input| input.has_accesskit_action_request(id, Action::Focus));
    if focus && let Some(action) = context.plan.prepare_tab_list_menu_focus(menu, row.item()) {
        context.push_local_action(action);
    }
    let reveal = context
        .ui
        .input(|input| input.has_accesskit_action_request(id, Action::ScrollIntoView));
    if reveal && let Some(action) = context.plan.prepare_tab_list_menu_reveal(row) {
        context.push_local_action(action);
    }
}

fn paint_scrollbar(
    context: &mut RenderContext<'_, '_>,
    menu: TabListMenuPaintRecord<'_>,
    viewport: Rect,
    bounds: Rect,
) {
    if menu.maximum_scroll_offset() <= 0.0 || bounds.max.x <= viewport.max.x {
        return;
    }
    let track = Rect::from_min_max(pos2(viewport.max.x, viewport.min.y), bounds.right_bottom());
    if !track.is_positive() {
        return;
    }
    let id = context.ui.make_persistent_id((
        context.instance_id,
        "tab-list-menu-scrollbar",
        menu.visual_id(),
    ));
    let _ = context.interact_receiver(
        track,
        id,
        Sense::hover(),
        context.plan.receiver_for_tab_list_menu_scroll(menu),
    );
    context
        .ui
        .painter()
        .rect_filled(track, 2.0, context.style.tab_bar_fill);
    let content_extent = f64::from(viewport.height()) + menu.maximum_scroll_offset();
    let thumb_height = (f64::from(track.height()) * f64::from(viewport.height()) / content_extent)
        .max(12.0)
        .min(f64::from(track.height())) as f32;
    let travel = (track.height() - thumb_height).max(0.0);
    let fraction = (menu.scroll_offset() / menu.maximum_scroll_offset()).clamp(0.0, 1.0) as f32;
    let thumb = Rect::from_min_size(
        pos2(track.min.x + 2.0, track.min.y + travel * fraction),
        vec2((track.width() - 4.0).max(1.0), thumb_height),
    );
    context
        .ui
        .painter()
        .rect_filled(thumb, 2.0, context.style.splitter_hover_color);
    context.ui.ctx().accesskit_node_builder(id, |node| {
        node.set_role(Role::ScrollBar);
        node.set_bounds(accesskit_bounds(track));
        node.set_label("Open tabs scroll position");
        node.set_numeric_value(menu.scroll_offset());
        node.set_min_numeric_value(0.0);
        node.set_max_numeric_value(menu.maximum_scroll_offset());
        node.add_action(Action::Increment);
        node.add_action(Action::Decrement);
    });
    let adjustment = context.ui.input(|input| {
        if input.has_accesskit_action_request(id, Action::Increment) {
            Some(1.0)
        } else if input.has_accesskit_action_request(id, Action::Decrement) {
            Some(-1.0)
        } else {
            None
        }
    });
    if let Some(direction) = adjustment {
        let step = menu
            .rows()
            .next()
            .map_or(24.0, |row| row.bounds().height().max(1.0));
        if let Some(action) = context
            .plan
            .prepare_tab_list_menu_scroll_by(menu, direction * step)
        {
            context.push_local_action(action);
        }
    }
}

fn capture_menu_keyboard(
    context: &mut RenderContext<'_, '_>,
    menu: TabListMenuPaintRecord<'_>,
    rows: &[TabListMenuRowPaintRecord],
) {
    let navigation = context.ui.input_mut(|input| {
        if input.consume_key(egui::Modifiers::NONE, Key::ArrowUp) {
            Some(SurfaceTabListNavigation::Previous)
        } else if input.consume_key(egui::Modifiers::NONE, Key::ArrowDown) {
            Some(SurfaceTabListNavigation::Next)
        } else if input.consume_key(egui::Modifiers::NONE, Key::Home) {
            Some(SurfaceTabListNavigation::First)
        } else if input.consume_key(egui::Modifiers::NONE, Key::End) {
            Some(SurfaceTabListNavigation::Last)
        } else {
            None
        }
    });
    if let Some(action) = navigation.and_then(|navigation| {
        context
            .plan
            .prepare_tab_list_menu_navigation(menu, navigation)
    }) {
        context.push_local_action(action);
        return;
    }

    let activate = context.ui.input_mut(|input| {
        input.consume_key(egui::Modifiers::NONE, Key::Enter)
            || input.consume_key(egui::Modifiers::NONE, Key::Space)
    });
    if activate
        && let Some(row) = rows.iter().copied().find(|row| row.focused())
        && let Some(action) = context.plan.prepare_tab_list_menu_row_activation(row)
    {
        context.push_local_action(action);
        return;
    }

    if context
        .ui
        .input_mut(|input| input.consume_key(egui::Modifiers::NONE, Key::Escape))
        && let Some(backdrop) = context
            .plan
            .tab_list_menu_backdrops()
            .find(|backdrop| backdrop.menu_visual_id() == menu.visual_id())
        && let Some(action) = context.plan.prepare_tab_list_menu_dismiss(backdrop)
    {
        context.push_local_action(action);
    }
}
