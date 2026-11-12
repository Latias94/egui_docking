//! Tab-strip overflow controls and the core-owned tab-list popup.

use dockspace::runtime::{
    DockspaceVisualId, SurfacePaintDisposition, TabListMenuBackdropPaintRecord,
    TabListMenuPaintRecord, TabListMenuRowPaintRecord, TabStripControlKind,
    TabStripControlPaintRecord,
};
use egui::accesskit::{Action, HasPopup, Role};
use egui::{Id, Key, PointerButton, Rect, Sense, Stroke, pos2, vec2};

use super::RenderContext;
use super::actions::{button_activation_origin, effective_enabled};
use super::geometry::{accesskit_bounds, egui_rect};

pub(crate) fn paint_control_record(
    context: &mut RenderContext<'_, '_, '_>,
    control: TabStripControlPaintRecord,
    paint: SurfacePaintDisposition,
) {
    paint_control_records(context, std::iter::once(control), paint);
}

fn paint_control_records(
    context: &mut RenderContext<'_, '_, '_>,
    controls: impl IntoIterator<Item = TabStripControlPaintRecord>,
    paint: SurfacePaintDisposition,
) {
    for control in controls {
        let Some(bounds) = egui_rect(control.bounds()) else {
            continue;
        };
        let Some(hit) = egui_rect(control.hit_bounds()) else {
            continue;
        };
        let id = if control.kind() == TabStripControlKind::TabListMenu {
            tab_list_menu_trigger_id(context, control.tab_bar_visual_id())
        } else {
            context.ui.make_persistent_id((
                context.instance_id,
                "tab-strip-control",
                control.visual_id(),
            ))
        };
        let enabled = effective_enabled(context.ui, control.enabled());
        let response = context.interact_receiver(
            hit,
            id,
            // Overlay controls own both input lanes above underlying tab bodies.
            if enabled {
                Sense::click_and_drag()
            } else {
                Sense::CLICK | Sense::DRAG
            },
            enabled
                .then(|| context.plan.receiver_for_tab_strip_control(control))
                .flatten(),
        );
        let response = response.on_hover_text(match control.kind() {
            TabStripControlKind::ScrollBackward => "Show earlier tabs",
            TabStripControlKind::ScrollForward => "Show later tabs",
            TabStripControlKind::TabListMenu if control.menu_open() => {
                "Close the list of open tabs"
            }
            TabStripControlKind::TabListMenu => "Show all open tabs",
        });
        let paint_omitted = paint.is_source_gap();
        if !paint_omitted {
            paint_control(context, control, bounds, &response, enabled);
        }
        configure_control_accessibility(context, control, id, bounds, enabled);

        if enabled
            && let Some(origin) =
                button_activation_origin(context.ui, &response, context.pointer_authority)
            && let Some(action) = context.plan.prepare_tab_strip_control_activation(control)
        {
            context.push_local_action(origin, action);
        }
    }
}

fn tab_list_menu_trigger_id(context: &RenderContext<'_, '_, '_>, tab_bar: DockspaceVisualId) -> Id {
    context
        .ui
        .make_persistent_id((context.instance_id, "tab-list-menu-trigger", tab_bar))
}

fn paint_control(
    context: &RenderContext<'_, '_, '_>,
    control: TabStripControlPaintRecord,
    bounds: Rect,
    response: &egui::Response,
    enabled: bool,
) {
    let emphasized = enabled && RenderContext::response_emphasized(response);
    let fill = if emphasized {
        context.interaction_fill(response)
    } else {
        context.visuals.tab_bar_fill
    };
    let color = if enabled {
        context
            .interaction_stroke(response, context.style.visuals.tab_text_color)
            .color
    } else {
        let alpha = if context.ui.is_enabled() {
            context.ui.visuals().disabled_alpha
        } else {
            1.0
        };
        context.visuals.tab_text_color.gamma_multiply(alpha)
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
                    center + vec2(-direction * half, -half),
                    center + vec2(direction * half, 0.0),
                ],
                Stroke::new(1.5, color),
            );
            context.ui.painter().line_segment(
                [
                    center + vec2(direction * half, 0.0),
                    center + vec2(-direction * half, half),
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
    context: &RenderContext<'_, '_, '_>,
    control: TabStripControlPaintRecord,
    id: Id,
    bounds: Rect,
    enabled: bool,
) {
    let menu_open = control.kind() == TabStripControlKind::TabListMenu && control.menu_open();
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
        if enabled {
            node.add_action(Action::Click);
        } else {
            node.set_disabled();
            node.clear_actions();
        }
    });
}

pub(crate) fn paint_backdrop(
    context: &mut RenderContext<'_, '_, '_>,
    backdrop: TabListMenuBackdropPaintRecord,
) {
    let Some(bounds) = egui_rect(backdrop.bounds()) else {
        return;
    };
    let id = context.ui.make_persistent_id((
        context.instance_id,
        "tab-list-menu-backdrop",
        backdrop.visual_id(),
    ));
    let enabled = effective_enabled(context.ui, true);
    let receiver = enabled
        .then(|| context.plan.receiver_for_tab_list_menu_backdrop(backdrop))
        .flatten();
    let scroll_id = context.ui.make_persistent_id((
        context.instance_id,
        "tab-list-menu-backdrop-scroll",
        backdrop.visual_id(),
    ));
    context.register_scroll_receiver(
        scroll_id,
        context
            .ui
            .is_enabled()
            .then(|| context.plan.receiver_for_tab_list_menu_backdrop(backdrop))
            .flatten(),
    );
    let response = context.interact_receiver(
        bounds,
        id,
        if enabled {
            Sense::click_and_drag()
        } else {
            Sense::hover()
        },
        receiver,
    );
    if enabled
        && context.pointer_authority.accepts_local_pointer_actions()
        && response.clicked_by(PointerButton::Primary)
        && let Some(action) = context.plan.prepare_tab_list_menu_dismiss(backdrop)
    {
        context.push_local_action(id.with("pointer-click"), action);
    }
}

pub(crate) fn paint_menu(
    context: &mut RenderContext<'_, '_, '_>,
    menu: TabListMenuPaintRecord<'_>,
    paint: SurfacePaintDisposition,
) {
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
    let enabled = effective_enabled(context.ui, true);
    let frame_receiver = enabled
        .then(|| context.plan.receiver_for_tab_list_menu_frame(menu))
        .flatten();
    let frame_scroll_id = context.ui.make_persistent_id((
        context.instance_id,
        "tab-list-menu-frame-scroll-blocker",
        menu.visual_id(),
    ));
    // Keep the underlying scroll footprint even when a modal blocks this layer.
    context.register_scroll_receiver(
        frame_scroll_id,
        context
            .ui
            .is_enabled()
            .then(|| context.plan.receiver_for_tab_list_menu_frame(menu))
            .flatten(),
    );
    let _ = context.interact_receiver(
        bounds,
        frame_id,
        if enabled {
            Sense::CLICK | Sense::DRAG
        } else {
            Sense::hover()
        },
        frame_receiver,
    );
    let paint_omitted = paint.is_source_gap();
    if !paint_omitted {
        let mut frame = context.style.resolved_popup_frame(context.ui.style());
        frame.inner_margin = egui::Margin::ZERO;
        frame.outer_margin = egui::Margin::ZERO;
        context
            .ui
            .painter()
            .add(frame.paint(bounds.shrink(frame.stroke.width)));
    }
    context.ui.ctx().accesskit_node_builder(frame_id, |node| {
        node.set_role(Role::Menu);
        node.set_bounds(accesskit_bounds(bounds));
        node.set_label("Open tabs");
        if !enabled {
            node.set_disabled();
        }
    });

    let scroll_id = context.ui.make_persistent_id((
        context.instance_id,
        "tab-list-menu-scroll",
        menu.visual_id(),
    ));
    let scroll_receiver = context
        .ui
        .is_enabled()
        .then(|| context.plan.receiver_for_tab_list_menu_scroll(menu))
        .flatten();
    context.register_scroll_receiver(scroll_id, scroll_receiver);

    let rows = menu.rows().collect::<Vec<_>>();
    if enabled {
        // One instance slot observes semantic focus edges, without retaining popup history.
        // Ordinary passes must leave Tab traversal and external focus intact.
        let marker = context.instance_id.with("tab-list-menu-focus-edge");
        let edge = (
            menu.visual_id(),
            rows.iter()
                .copied()
                .find(|row| row.focused())
                .map(|row| tab_list_menu_row_id(context, row)),
        );
        let previous = context.ui.ctx().data_mut(|data| {
            let previous = data.get_temp::<(DockspaceVisualId, Option<Id>)>(marker);
            if previous != Some(edge) {
                data.insert_temp(marker, edge);
            }
            previous
        });
        if previous != Some(edge)
            && let Some(target) = edge.1
        {
            let can_retarget = context.ui.memory(|memory| {
                let focused = memory.focused();
                let newly_focused = focused.is_some_and(|id| !memory.had_focus_last_frame(id));
                let still_on_previous = previous.is_none_or(|(menu_id, row_id)| {
                    menu_id != edge.0 || focused.is_none() || focused == row_id
                });
                !newly_focused && still_on_previous
            });
            if can_retarget {
                // A core acknowledgement must not reclaim focus already moved by the host.
                context.ui.memory_mut(|memory| memory.request_focus(target));
            }
        }
    }
    for row in rows.iter().copied() {
        paint_menu_row(context, menu, row, viewport, paint);
    }
    paint_scrollbar(context, menu, viewport, bounds, paint);
    capture_menu_keyboard(context, menu, frame_id, &rows);
}

fn tab_list_menu_row_id(context: &RenderContext<'_, '_, '_>, row: TabListMenuRowPaintRecord) -> Id {
    context
        .ui
        .make_persistent_id((context.instance_id, "tab-list-menu-row", row.visual_id()))
}

fn paint_menu_row(
    context: &mut RenderContext<'_, '_, '_>,
    menu: TabListMenuPaintRecord<'_>,
    row: TabListMenuRowPaintRecord,
    viewport: Rect,
    paint: SurfacePaintDisposition,
) {
    let Some(bounds) = egui_rect(row.bounds()) else {
        return;
    };
    let id = tab_list_menu_row_id(context, row);
    let enabled = effective_enabled(context.ui, true);
    let (click, focus, reveal) = if enabled {
        context.ui.input(|input| {
            (
                input.has_accesskit_action_request(id, Action::Click),
                input.has_accesskit_action_request(id, Action::Focus),
                input.has_accesskit_action_request(id, Action::ScrollIntoView),
            )
        })
    } else {
        (false, false, false)
    };
    let receiver = enabled
        .then(|| context.plan.receiver_for_tab_list_menu_row(row))
        .flatten();
    let pointer_operable = receiver.is_some();
    let response = row.interaction_bounds().and_then(egui_rect).map(|hit| {
        let response = context.interact_receiver(
            hit,
            id,
            if pointer_operable {
                Sense::click()
            } else {
                Sense::hover()
            },
            receiver,
        );
        if let Some(resource) = context.resources.item(row.item()) {
            response.on_hover_text(resource.title.as_str())
        } else {
            response
        }
    });
    let paint_omitted = paint.is_source_gap();
    let fill = if row.focused() {
        context
            .style
            .visuals
            .tab_hover_fill
            .unwrap_or(context.ui.visuals().widgets.active.weak_bg_fill)
    } else if let Some(response) = response
        .as_ref()
        .filter(|response| enabled && RenderContext::response_emphasized(response))
    {
        context.interaction_fill(response)
    } else if row.selected() {
        context.ui.visuals().selection.bg_fill
    } else {
        context.visuals.popup_fill
    };
    let painter = context.ui.painter_at(viewport);
    if !paint_omitted {
        painter.rect_filled(bounds, 0.0, fill);
    }
    if !paint_omitted && let Some(resource) = context.resources.item(row.item()) {
        let galley = resource.galley_for_width(context.ui, bounds.width());
        let text_pos = pos2(bounds.min.x, bounds.center().y - galley.size().y * 0.5);
        context.paint_tab_title(
            &painter,
            text_pos,
            galley,
            row.selected(),
            if row.selected() {
                context
                    .style
                    .visuals
                    .tab_active_text_color
                    .or(context.ui.visuals().override_text_color)
                    .unwrap_or(context.ui.visuals().selection.stroke.color)
            } else if let Some(response) = response
                .as_ref()
                .filter(|response| enabled && RenderContext::response_emphasized(response))
            {
                context.interaction_text_color(response, context.style.visuals.tab_text_color)
            } else {
                context.visuals.tab_text_color
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
        if enabled {
            node.add_action(Action::Click);
            node.add_action(Action::Focus);
            node.add_action(Action::ScrollIntoView);
        } else {
            node.set_disabled();
        }
    });
    let pointer = pointer_operable
        && context.pointer_authority.accepts_local_pointer_actions()
        && response
            .as_ref()
            .is_some_and(|response| response.clicked_by(PointerButton::Primary));
    if pointer || click {
        if let Some(action) = context.plan.prepare_tab_list_menu_row_activation(row) {
            let origin = id.with(if click {
                "accesskit-click"
            } else {
                "pointer-click"
            });
            context.push_local_action(origin, action);
        }
        return;
    }
    if focus && let Some(action) = context.plan.prepare_tab_list_menu_focus(menu, row.item()) {
        context.push_local_action(id.with("accesskit-focus"), action);
    }
    if reveal && let Some(action) = context.plan.prepare_tab_list_menu_reveal(row) {
        context.push_local_action(id.with("accesskit-scroll-into-view"), action);
    }
}

fn paint_scrollbar(
    context: &mut RenderContext<'_, '_, '_>,
    menu: TabListMenuPaintRecord<'_>,
    viewport: Rect,
    bounds: Rect,
    paint: SurfacePaintDisposition,
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
    let enabled = effective_enabled(context.ui, true);
    let receiver = enabled
        .then(|| context.plan.receiver_for_tab_list_menu_frame(menu))
        .flatten();
    let response = context.interact_receiver(
        track,
        id,
        if enabled {
            Sense::click_and_drag()
        } else {
            Sense::hover()
        },
        receiver,
    );
    let paint_omitted = paint.is_source_gap();
    if !paint_omitted {
        context
            .ui
            .painter()
            .rect_filled(track, 2.0, context.visuals.tab_bar_fill);
    }
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
    if !paint_omitted {
        context
            .ui
            .painter()
            .rect_filled(thumb, 2.0, context.visuals.splitter_hover_color);
    }
    context.ui.ctx().accesskit_node_builder(id, |node| {
        node.set_role(Role::ScrollBar);
        node.set_bounds(accesskit_bounds(track));
        node.set_label("Open tabs scroll position");
        node.set_numeric_value(menu.scroll_offset());
        node.set_min_numeric_value(0.0);
        node.set_max_numeric_value(menu.maximum_scroll_offset());
        if enabled {
            node.add_action(Action::Increment);
            node.add_action(Action::Decrement);
        } else {
            node.set_disabled();
        }
    });
    let adjustment = enabled
        .then(|| {
            context.ui.input(|input| {
                if input.has_accesskit_action_request(id, Action::Increment) {
                    Some((id.with("scrollbar-accesskit-increment"), 1.0))
                } else if input.has_accesskit_action_request(id, Action::Decrement) {
                    Some((id.with("scrollbar-accesskit-decrement"), -1.0))
                } else {
                    None
                }
            })
        })
        .flatten();
    if let Some((origin, direction)) = adjustment {
        let step = menu
            .rows()
            .next()
            .map_or(24.0, |row| row.bounds().height().max(1.0));
        if let Some(action) = context
            .plan
            .prepare_tab_list_menu_scroll_by(menu, direction * step)
        {
            context.push_local_action(origin, action);
        }
    }
    capture_scrollbar_pointer(context, menu, track, thumb, &response);
}

fn capture_scrollbar_pointer(
    context: &mut RenderContext<'_, '_, '_>,
    menu: TabListMenuPaintRecord<'_>,
    track: Rect,
    thumb: Rect,
    response: &egui::Response,
) {
    // Scrollbar responses belong to the scroll subsystem under every pointer authority.
    if !effective_enabled(context.ui, response.enabled()) {
        return;
    }

    let anchor_id = response.id.with("drag-anchor");
    let primary_down = response.is_pointer_button_down_on()
        && context.ui.input(|input| input.pointer.primary_down());
    let primary_finished = response.clicked_by(PointerButton::Primary)
        || response.drag_stopped_by(PointerButton::Primary);
    let pointer_target = if primary_down || primary_finished {
        response.interact_pointer_pos().and_then(|pointer| {
            let anchor = context
                .ui
                .data(|data| data.get_temp::<f32>(anchor_id))
                .unwrap_or_else(|| {
                    let anchor = if thumb.contains(pointer) {
                        pointer.y - thumb.min.y
                    } else {
                        thumb.height() * 0.5
                    };
                    context
                        .ui
                        .data_mut(|data| data.insert_temp(anchor_id, anchor));
                    anchor
                });
            scrollbar_target_offset(
                track,
                thumb.height(),
                pointer.y,
                anchor,
                menu.maximum_scroll_offset(),
            )
        })
    } else {
        None
    };

    if !primary_down {
        context.ui.data_mut(|data| data.remove::<f32>(anchor_id));
    }
    let Some(target) = pointer_target else {
        return;
    };
    let delta = target - menu.scroll_offset();
    if delta != 0.0
        && let Some(action) = context.plan.prepare_tab_list_menu_scroll_by(menu, delta)
    {
        context.push_local_action(response.id.with("scrollbar-pointer-drag"), action);
    }
}

fn scrollbar_target_offset(
    track: Rect,
    thumb_height: f32,
    pointer_y: f32,
    grab_offset: f32,
    maximum_scroll_offset: f64,
) -> Option<f64> {
    let travel = track.height() - thumb_height;
    if !travel.is_finite()
        || travel <= 0.0
        || !pointer_y.is_finite()
        || !grab_offset.is_finite()
        || !maximum_scroll_offset.is_finite()
        || maximum_scroll_offset <= 0.0
    {
        return None;
    }
    let thumb_top = (pointer_y - grab_offset).clamp(track.min.y, track.max.y - thumb_height);
    let fraction = (thumb_top - track.min.y) / travel;
    Some(f64::from(fraction) * maximum_scroll_offset)
}

fn capture_menu_keyboard(
    context: &mut RenderContext<'_, '_, '_>,
    menu: TabListMenuPaintRecord<'_>,
    id: Id,
    rows: &[TabListMenuRowPaintRecord],
) {
    if !effective_enabled(context.ui, true) {
        return;
    }
    let focused = context.ui.memory(|memory| memory.focused());
    let focused_index = rows
        .iter()
        .position(|row| Some(tab_list_menu_row_id(context, *row)) == focused);
    if let Some(index) = focused_index {
        let navigation = context.ui.input_mut(|input| {
            if input.consume_key(egui::Modifiers::NONE, Key::ArrowUp) {
                Some((Key::ArrowUp, index.saturating_sub(1)))
            } else if input.consume_key(egui::Modifiers::NONE, Key::ArrowDown) {
                Some((Key::ArrowDown, (index + 1).min(rows.len() - 1)))
            } else if input.consume_key(egui::Modifiers::NONE, Key::Home) {
                Some((Key::Home, 0))
            } else if input.consume_key(egui::Modifiers::NONE, Key::End) {
                Some((Key::End, rows.len() - 1))
            } else {
                None
            }
        });
        if let Some((key, destination)) = navigation
            && let Some(action) = context
                .plan
                .prepare_tab_list_menu_focus(menu, rows[destination].item())
        {
            let target = tab_list_menu_row_id(context, rows[destination]);
            context.ui.memory_mut(|memory| {
                memory.move_focus(egui::FocusDirection::None);
                memory.request_focus(target);
            });
            context.push_local_action(id.with(("keyboard-navigation", key)), action);
            return;
        }

        let activate = context.ui.input_mut(|input| {
            [Key::Enter, Key::Space]
                .into_iter()
                .find(|&key| input.consume_key(egui::Modifiers::NONE, key))
        });
        if let Some(key) = activate
            && let Some(action) = context
                .plan
                .prepare_tab_list_menu_row_activation(rows[index])
        {
            context.push_local_action(id.with(("keyboard-activate", key)), action);
            return;
        }
        if !rows[index].focused()
            && let Some(action) = context
                .plan
                .prepare_tab_list_menu_focus(menu, rows[index].item())
        {
            let origin = tab_list_menu_row_id(context, rows[index]).with("framework-focus");
            context.push_local_action(origin, action);
        }
    }

    if context
        .ui
        .input_mut(|input| input.consume_key(egui::Modifiers::NONE, Key::Escape))
        && let Some(action) = context.plan.prepare_tab_list_menu_dismiss(menu.backdrop())
    {
        context.push_local_action(id.with(("keyboard-dismiss", Key::Escape)), action);
        let target = tab_list_menu_trigger_id(context, menu.tab_bar_visual_id());
        if context.ui.ctx().viewport(|viewport| {
            viewport
                .this_pass
                .widgets
                .get(target)
                .is_some_and(|widget| {
                    widget.enabled && widget.visible && widget.sense.is_focusable()
                })
        }) {
            context.ui.memory_mut(|memory| memory.request_focus(target));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::scrollbar_target_offset;
    use crate::{
        DockStyle, Dockspace, DockspaceLayout, DockspaceNode, DockspaceRootLayout,
        DockspaceSurfaceLayout, ItemId, PaneResponse, PaneView, RootId, SurfaceId,
    };
    use egui::accesskit::Role;
    use egui::{Context, RawInput, Rect, Shape, Ui, WidgetText, pos2, vec2};

    struct OverflowPanes;

    impl PaneView for OverflowPanes {
        fn title(&self, _item: ItemId) -> Option<WidgetText> {
            Some("An overflowing tab".into())
        }

        fn ui(&mut self, _item: ItemId, _ui: &mut Ui) -> PaneResponse {
            PaneResponse::default()
        }
    }

    #[test]
    fn overflow_scroll_chevrons_point_toward_the_tabs_they_reveal() {
        let context = Context::default();
        context.enable_accesskit();
        let layout = DockspaceLayout::new([DockspaceSurfaceLayout::new(
            SurfaceId::new(1),
            DockspaceRootLayout::new(
                RootId::new(1),
                DockspaceNode::tabs((1..=4).map(ItemId::new)),
            ),
        )])
        .unwrap();
        let mut dockspace = Dockspace::builder("overflow-chevron-direction", layout)
            .style(DockStyle {
                tab_min_width: 220.0,
                tab_max_width: 220.0,
                ..DockStyle::default()
            })
            .build()
            .unwrap();
        let mut panes = OverflowPanes;
        for pass in 0..2 {
            let mut output = context.run_ui(
                RawInput {
                    screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), vec2(640.0, 480.0))),
                    ..Default::default()
                },
                |ui| {
                    dockspace
                        .show_single_surface(SurfaceId::new(1), ui, &mut panes)
                        .unwrap();
                },
            );
            if pass == 0 {
                output.textures_delta.clear();
                continue;
            }
            let tree = output.platform_output.accesskit_update.as_ref().unwrap();
            for (label, direction) in [("Scroll tabs backward", -1.0), ("Scroll tabs forward", 1.0)]
            {
                let bounds = tree
                    .nodes
                    .iter()
                    .find_map(|(_, node)| {
                        (node.role() == Role::Button && node.label() == Some(label))
                            .then(|| node.bounds().unwrap())
                    })
                    .expect("overflow buttons remain visible even at the end of the strip");
                let arms = output
                    .shapes
                    .iter()
                    .filter_map(|shape| match &shape.shape {
                        Shape::LineSegment { points, .. }
                            if points.iter().all(|point| {
                                (bounds.x0..=bounds.x1).contains(&f64::from(point.x))
                                    && (bounds.y0..=bounds.y1).contains(&f64::from(point.y))
                            }) =>
                        {
                            Some(points)
                        }
                        _ => None,
                    })
                    .collect::<Vec<_>>();
                assert_eq!(arms.len(), 2, "the {label} icon is one chevron");
                let middle = (bounds.y0 + bounds.y1) * 0.5;
                for points in arms {
                    let (tip, tail) = if (f64::from(points[0].y) - middle).abs() < 0.01 {
                        (points[0], points[1])
                    } else {
                        (points[1], points[0])
                    };
                    assert!(
                        (tip.x - tail.x) * direction > 0.0,
                        "{label} must point toward its scroll direction",
                    );
                }
            }
            output.textures_delta.clear();
        }
    }

    #[test]
    fn scrollbar_target_preserves_the_pointer_grab_offset_and_clamps_to_track() {
        let track = Rect::from_min_max(pos2(0.0, 10.0), pos2(12.0, 110.0));

        assert_eq!(
            scrollbar_target_offset(track, 20.0, 50.0, 10.0, 400.0),
            Some(150.0)
        );
        assert_eq!(
            scrollbar_target_offset(track, 20.0, -50.0, 10.0, 400.0),
            Some(0.0)
        );
        assert_eq!(
            scrollbar_target_offset(track, 20.0, 250.0, 10.0, 400.0),
            Some(400.0)
        );
    }

    #[test]
    fn scrollbar_target_rejects_non_operable_geometry() {
        let track = Rect::from_min_max(pos2(0.0, 10.0), pos2(12.0, 110.0));

        assert_eq!(
            scrollbar_target_offset(track, 100.0, 50.0, 10.0, 400.0),
            None
        );
        assert_eq!(
            scrollbar_target_offset(track, 20.0, 50.0, 10.0, f64::NAN),
            None
        );
    }
}
