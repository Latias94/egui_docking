//! Tab chrome, keyboard navigation, accessibility, and pane dispatch.

use dockspace::backend::engine::{
    LocalTabGesturePhase, TabListMenuNavigation, TabScrollAdjustment,
};
use dockspace::backend::interaction::{ActiveDragView, DragPhase};
use dockspace::backend::presentation_hit::{PresentationHitManifest, PresentationHitRegionKind};
use dockspace::backend::scene::{
    PaneRecord, PresentationPlan, SurfaceSceneStamp, TabBarRecord, TabBarSceneId, TabRecord,
    TabSceneId, TabStripControlRecord, TabStripMemberVisibility,
};
use dockspace::command::MovePayload;
use dockspace::graph::{Node, Workspace};
use dockspace::ids::{ItemId, SurfaceId};
use dockspace::intent::CloseSceneTarget;
use dockspace::policy::TabBarInteraction;
use dockspace::tab_strip::TabStripControlId;
use egui::accesskit::{Action, HasPopup, Orientation, Role};
use egui::{
    Area, CursorIcon, EventFilter, FocusDirection, Id, Key, Order, Rect, Response, Sense, Stroke,
    StrokeKind, Ui, UiBuilder, pos2,
};

use crate::hit::{
    SemanticActivation, accesskit_focus_requested, contains_half_open, interact_rect,
    semantic_activation_fact,
};
use crate::pane::PaneView;
use crate::projection::{
    EguiSurfacePaintResources, TabPaintResource, TabRevealIdentity, TabStripKey, TabStripStateMap,
    complete_tab_keyboard_focus, pending_tab_keyboard_focus, request_tab_keyboard_focus,
    sync_tab_reveal_identity,
};
use crate::renderer::{
    RenderAction, RenderOutput, accesskit_bounds, from_logical_rect, paint_centered_label,
    to_logical_point,
};
use crate::style::DockStyle;

#[cfg(egui_backend_event_envelope)]
fn native_scroll_config(projection: egui::ScrollProjection) -> egui::ScrollReceiverConfig {
    match projection {
        egui::ScrollProjection::HorizontalElseVertical => egui::ScrollReceiverConfig::new(
            projection,
            egui::Vec2::ONE,
            egui::ScrollAxisCapabilities::BOTH,
            egui::ScrollAxisCapabilities::NONE,
        ),
        egui::ScrollProjection::VerticalElseHorizontal => egui::ScrollReceiverConfig::new(
            projection,
            egui::Vec2::ONE,
            egui::ScrollAxisCapabilities::NONE,
            egui::ScrollAxisCapabilities::BOTH,
        ),
        egui::ScrollProjection::Independent
        | egui::ScrollProjection::SumToHorizontal
        | egui::ScrollProjection::SumToVertical => {
            unreachable!("dock scroll receivers use a primary-axis fallback projection")
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn paint_tabs(
    ui: &mut Ui,
    instance_id: Id,
    surface: SurfaceId,
    pane: &PaneRecord,
    bar: Option<&TabBarRecord>,
    visible_tabs: &[&TabRecord],
    controls: &[&TabStripControlRecord],
    resources: &EguiSurfacePaintResources,
    tab_strip_states: &mut TabStripStateMap,
    content_resources: &EguiSurfacePaintResources,
    workspace: &Workspace,
    panes: &mut dyn PaneView,
    style: &DockStyle,
    active_drag: Option<ActiveDragView<'_>>,
    tab_scroll_owner: Option<TabBarSceneId>,
    tab_interactions_current: bool,
    retained_controls_current: bool,
    pane_content_current: bool,
    interaction_scene: Option<SurfaceSceneStamp>,
    local_gesture_scene: Option<SurfaceSceneStamp>,
    authoritative_plan: &PresentationPlan,
    authoritative_hit_manifest: Option<&PresentationHitManifest>,
    output: &mut RenderOutput,
) {
    let pane_id = pane.id();
    let content_identity_current =
        resources.pane_identity(pane_id) == content_resources.pane_identity(pane_id);
    let semantic_selected = content_identity_current
        .then(|| workspace.node(pane_id.tabs))
        .flatten()
        .and_then(|node| match node {
            Node::Tabs { selected, .. } => *selected,
            Node::Split { .. } => None,
        });
    let Some(pane_bounds) = from_logical_rect(pane.bounds()) else {
        return;
    };
    let painter = ui.painter_at(pane_bounds);
    painter.rect_filled(pane_bounds, 0.0, style.workspace_fill);
    let Some(bar) = bar else {
        paint_selected_panel(
            ui,
            instance_id,
            pane,
            semantic_selected,
            content_resources,
            panes,
            style,
            pane_content_current,
            output,
        );
        return;
    };
    let Some(tab_bar_rect) = from_logical_rect(bar.bounds()) else {
        return;
    };
    let tab_interactions_current = tab_interactions_current
        && authoritative_plan.region_is_operable(bar.bounds(), bar.layer());
    painter.rect_filled(tab_bar_rect, 0.0, style.tab_bar_fill);

    let mut tab_ui = ui.new_child(
        UiBuilder::new()
            .id_salt((
                "egui_dockspace",
                instance_id,
                surface,
                bar.id().root,
                bar.id().tabs,
                "tab-list",
            ))
            .max_rect(tab_bar_rect),
    );
    tab_ui.set_clip_rect(tab_bar_rect.intersect(ui.clip_rect()));
    tab_ui.set_min_size(tab_bar_rect.size());
    if bar.interaction() == TabBarInteraction::Disabled {
        paint_static_tab_bar(&tab_ui, bar, visible_tabs, controls, resources, style);
        drop(tab_ui);
        paint_selected_panel(
            ui,
            instance_id,
            pane,
            semantic_selected,
            content_resources,
            panes,
            style,
            pane_content_current,
            output,
        );
        return;
    }
    configure_tab_list_accessibility(
        &tab_ui,
        instance_id,
        bar,
        visible_tabs,
        semantic_selected,
        tab_interactions_current,
    );
    #[cfg(egui_backend_event_envelope)]
    {
        if retained_controls_current
            && bar.maximum_scroll_offset() > 0.0
            && let Some(viewport) = from_logical_rect(bar.viewport()).filter(Rect::is_positive)
        {
            let reservation = tab_ui.reserve_scroll_receiver(tab_ui.make_persistent_id((
                instance_id,
                "tab-strip-scroll",
                *bar.id(),
            )));
            let config = native_scroll_config(egui::ScrollProjection::HorizontalElseVertical);
            if let Ok(receiver) =
                tab_ui.finalize_scroll_receiver(reservation, interact_rect(viewport), config)
            {
                output.register_scroll_receiver(
                    receiver,
                    PresentationHitRegionKind::TabStripScroll(*bar.id()),
                );
            }
        }
    }

    paint_group_grip(
        &mut tab_ui,
        instance_id,
        bar,
        style,
        retained_controls_current,
        output,
    );

    for tab in visible_tabs {
        paint_tab(
            &mut tab_ui,
            instance_id,
            surface,
            bar,
            tab,
            tab_resource(resources, tab),
            tab_strip_states,
            semantic_selected,
            workspace,
            style,
            tab_interactions_current,
            interaction_scene,
            local_gesture_scene,
            authoritative_plan,
            output,
        );
    }
    paint_overflow_control(
        &mut tab_ui,
        instance_id,
        surface,
        bar,
        controls,
        style,
        retained_controls_current,
        authoritative_plan,
        authoritative_hit_manifest,
        output,
    );
    let key = TabStripKey::new(surface, *bar.id());
    let reveal_identity = current_tab_reveal_identity(&tab_ui, instance_id, pane, bar, active_drag);
    let reveal_changed = retained_controls_current
        && controls
            .iter()
            .any(|control| matches!(control.id(), TabStripControlId::TabListMenu(_)))
        && sync_tab_reveal_identity(tab_strip_states, key, reveal_identity);
    let scrolled = update_tab_strip_scroll(
        &mut tab_ui,
        surface,
        bar,
        controls,
        style,
        active_drag,
        reveal_identity,
        tab_scroll_owner,
        retained_controls_current,
        output,
    );
    if reveal_changed {
        if retained_controls_current
            && !scrolled
            && let Some(item) = reveal_identity.primary()
        {
            output.push_post_batch_continuation(RenderAction::AdjustTabStripScroll {
                surface,
                bar: *bar.id(),
                adjustment: TabScrollAdjustment::reveal_item(item),
            });
        }
        tab_ui
            .ctx()
            .request_discard("dockspace tab reveal identity changed");
        tab_ui.ctx().request_repaint();
    }
    drop(tab_ui);

    paint_selected_panel(
        ui,
        instance_id,
        pane,
        semantic_selected,
        content_resources,
        panes,
        style,
        pane_content_current,
        output,
    );
}

fn paint_static_tab_bar(
    ui: &Ui,
    bar: &TabBarRecord,
    visible_tabs: &[&TabRecord],
    controls: &[&TabStripControlRecord],
    resources: &EguiSurfacePaintResources,
    style: &DockStyle,
) {
    if let Some(grip) = bar.group_grip_bounds().and_then(from_logical_rect) {
        paint_group_grip_icon(ui, grip, style, false);
    }
    for tab in visible_tabs {
        let Some(rect) = from_logical_rect(tab.visible_bounds()) else {
            continue;
        };
        let selected = tab.selected();
        paint_tab_body(
            ui,
            tab,
            tab_resource(resources, tab),
            rect,
            style,
            selected,
            false,
            false,
        );
        if let Some(close) = tab.close_visual_bounds().and_then(from_logical_rect) {
            paint_close_icon(ui, close, style, selected);
        }
    }
    if let Some(overflow) = controls.iter().find_map(|control| {
        matches!(control.id(), TabStripControlId::TabListMenu(_))
            .then(|| from_logical_rect(control.bounds()))
            .flatten()
    }) {
        paint_overflow_icon(ui, overflow, style, false);
    }
}

#[allow(clippy::too_many_arguments)]
fn paint_group_grip(
    ui: &mut Ui,
    instance_id: Id,
    bar: &TabBarRecord,
    style: &DockStyle,
    interactions_current: bool,
    output: &mut RenderOutput,
) {
    let Some(group_drag) = bar.group_drag() else {
        return;
    };
    let Some(grip) = from_logical_rect(group_drag.grip_bounds()) else {
        return;
    };
    let id = ui.make_persistent_id((instance_id, "tab-group-grip", bar.id().root, bar.id().tabs));
    let response = ui
        .interact(interact_rect(grip), id, Sense::DRAG)
        .on_hover_text("Drag tab group");
    output.register_receiver(
        &response,
        PresentationHitRegionKind::TabGroupGrip(*bar.id()),
    );
    configure_group_grip_accessibility(ui, id, grip, interactions_current);

    let active = interactions_current && (response.hovered() || response.dragged());
    if active {
        ui.ctx().set_cursor_icon(if response.dragged() {
            CursorIcon::Grabbing
        } else {
            CursorIcon::Grab
        });
    }
    paint_group_grip_icon(ui, grip, style, active);
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

fn configure_group_grip_accessibility(ui: &Ui, id: Id, rect: Rect, interactions_current: bool) {
    ui.ctx().accesskit_node_builder(id, |node| {
        node.set_role(Role::Button);
        node.set_bounds(accesskit_bounds(rect));
        node.set_label("Drag tab group");
        if !interactions_current {
            node.clear_actions();
            node.set_disabled();
        }
    });
}

fn paint_overflow_control(
    ui: &mut Ui,
    instance_id: Id,
    surface: SurfaceId,
    bar: &TabBarRecord,
    controls: &[&TabStripControlRecord],
    style: &DockStyle,
    interactions_current: bool,
    authoritative_plan: &PresentationPlan,
    authoritative_hit_manifest: Option<&PresentationHitManifest>,
    output: &mut RenderOutput,
) {
    let id = ui.make_persistent_id((instance_id, "tab-overflow", bar.id().root, bar.id().tabs));
    let control = TabStripControlId::TabListMenu(*bar.id());
    let Some(record) = controls
        .iter()
        .copied()
        .find(|record| record.id() == control)
    else {
        return;
    };
    let Some(rect) = from_logical_rect(record.bounds()) else {
        return;
    };
    let control_current = interactions_current && record.enabled();
    let response = paint_overflow_button(ui, id, rect, style, control_current);
    let control_kind = PresentationHitRegionKind::TabStripControl(control);
    if authoritative_hit_manifest.is_some_and(|manifest| {
        manifest
            .regions()
            .iter()
            .any(|region| region.id().kind() == control_kind)
    }) {
        output.register_receiver(&response, control_kind);
    }
    let keyboard_activation = response.has_focus()
        && ui.input(|input| input.key_pressed(Key::Enter) || input.key_pressed(Key::Space));
    let activation =
        semantic_activation_fact(ui, &response, rect, interactions_current, control_current);
    if let Some(activation) = activation {
        output.push_widget_activation(
            ui,
            RenderAction::ActivateTabStripControl { surface, control },
            response.id,
            response.has_focus(),
            activation,
        );
    }
    if keyboard_activation && activation.is_some() {
        consume_overflow_toggle_keys(ui);
    }
    let expanded = authoritative_plan
        .tab_list_menu_records()
        .iter()
        .any(|menu| menu.bar() == *bar.id());
    ui.ctx().accesskit_node_builder(id, |node| {
        node.set_has_popup(HasPopup::Menu);
        node.set_expanded(expanded);
        if !control_current {
            node.set_disabled();
        }
    });
}

pub(crate) fn paint_authoritative_tab_list_menu(
    ui: &mut Ui,
    instance_id: Id,
    surface: SurfaceId,
    resources: &EguiSurfacePaintResources,
    style: &DockStyle,
    authoritative_plan: Option<&PresentationPlan>,
    interactions_current: bool,
    output: &mut RenderOutput,
) {
    let Some(plan) = authoritative_plan else {
        return;
    };
    let Some(backdrop) = plan.tab_list_menu_backdrop_records().first().copied() else {
        return;
    };
    let Some(backdrop_rect) = from_logical_rect(backdrop.bounds()) else {
        return;
    };
    let backdrop_id = Id::new((
        "egui_dockspace",
        instance_id,
        "tab-list-menu-backdrop",
        backdrop.session(),
    ));
    let backdrop_response = Area::new(backdrop_id)
        .order(Order::Foreground)
        .fixed_pos(backdrop_rect.min)
        .default_size(backdrop_rect.size())
        .constrain(false)
        .sense(if interactions_current {
            Sense::click_and_drag()
        } else {
            Sense::hover()
        })
        .show(ui.ctx(), |backdrop_ui| {
            backdrop_ui.set_min_size(backdrop_rect.size());
        })
        .response;
    if interactions_current {
        output.register_receiver(
            &backdrop_response,
            PresentationHitRegionKind::TabListMenuBackdrop(backdrop.session()),
        );
    }

    let Some(menu) = plan
        .tab_list_menu_records()
        .iter()
        .find(|menu| menu.session() == backdrop.session())
    else {
        return;
    };
    let Some(menu_rect) = from_logical_rect(menu.bounds()) else {
        return;
    };
    let Some(viewport_rect) = from_logical_rect(menu.viewport()) else {
        return;
    };
    let menu_id = Id::new((
        "egui_dockspace",
        instance_id,
        "tab-list-menu",
        menu.session(),
    ));
    let menu_response = Area::new(menu_id)
        .order(Order::Foreground)
        .fixed_pos(menu_rect.min)
        .default_size(menu_rect.size())
        .constrain(false)
        .sense(if interactions_current {
            Sense::click_and_drag()
        } else {
            Sense::hover()
        })
        .show(ui.ctx(), |menu_ui| {
            menu_ui.set_min_size(menu_rect.size());
            paint_authoritative_menu_frame(menu_ui, menu_rect);
            #[cfg(egui_backend_event_envelope)]
            {
                if interactions_current {
                    let reservation =
                        menu_ui.reserve_scroll_receiver(menu_ui.make_persistent_id((
                            instance_id,
                            "tab-list-menu-scroll",
                            menu.session(),
                        )));
                    let config =
                        native_scroll_config(egui::ScrollProjection::VerticalElseHorizontal);
                    if let Ok(receiver) = menu_ui.finalize_scroll_receiver(
                        reservation,
                        interact_rect(viewport_rect),
                        config,
                    ) {
                        output.register_scroll_receiver(
                            receiver,
                            PresentationHitRegionKind::TabListMenuScroll(menu.session()),
                        );
                    }
                }
            }
            paint_authoritative_menu_rows(
                menu_ui,
                instance_id,
                surface,
                resources,
                menu,
                viewport_rect,
                style,
                interactions_current,
                output,
            );
            paint_authoritative_menu_scrollbar(
                menu_ui,
                instance_id,
                surface,
                menu,
                menu_rect,
                viewport_rect,
                interactions_current,
                output,
            );
            configure_authoritative_menu_accessibility(
                menu_ui,
                menu_id,
                menu_rect,
                interactions_current,
            );
        })
        .response;
    if interactions_current {
        output.register_receiver(
            &menu_response,
            PresentationHitRegionKind::TabListMenuBlocker(menu.session()),
        );
        capture_authoritative_menu_input(ui, surface, menu, output);
    }
}

fn paint_authoritative_menu_frame(ui: &Ui, rect: Rect) {
    let visuals = &ui.style().visuals;
    ui.painter().rect(
        rect,
        visuals.menu_corner_radius,
        visuals.window_fill(),
        visuals.window_stroke(),
        StrokeKind::Inside,
    );
}

fn paint_authoritative_menu_rows(
    ui: &mut Ui,
    instance_id: Id,
    surface: SurfaceId,
    resources: &EguiSurfacePaintResources,
    menu: &dockspace::backend::scene::TabListMenuRecord,
    viewport: Rect,
    style: &DockStyle,
    interactions_current: bool,
    output: &mut RenderOutput,
) {
    let key = TabStripKey {
        surface,
        root: menu.bar().root,
        node: menu.bar().tabs,
    };
    for row in menu.rows() {
        let tab = row.tab();
        let resource = resources
            .tab(tab)
            .expect("a retained core menu row has an exact egui tab resource");
        let Some(bounds) = from_logical_rect(row.bounds()) else {
            continue;
        };
        let hit = row
            .hit()
            .and_then(|hit| from_logical_rect(hit.rect()))
            .filter(Rect::is_positive);
        let id = overflow_menu_item_id(instance_id, key, tab.item);
        let response = ui.interact(
            hit.map_or(Rect::NOTHING, interact_rect),
            id,
            if interactions_current {
                Sense::click()
            } else {
                Sense::hover()
            },
        );
        ui.ctx().accesskit_node_builder(id, |node| {
            node.set_role(Role::MenuItem);
            node.set_bounds(accesskit_bounds(bounds));
            node.set_label(resource.title.clone());
            node.set_author_id(format!(
                "overflow-tab:{}:{}:{:?}:{}",
                key.surface, key.root, key.node, tab.item
            ));
            if interactions_current {
                node.add_action(Action::Focus);
                node.add_action(Action::Click);
                node.add_action(Action::ScrollIntoView);
            } else {
                node.clear_actions();
                node.set_disabled();
            }
            if row.selected() {
                node.set_selected(true);
            } else {
                node.clear_selected();
            }
        });
        if interactions_current {
            output.register_receiver(
                &response,
                PresentationHitRegionKind::TabListMenuRow {
                    menu: menu.session(),
                    tab: row.tab(),
                },
            );
        }
        if interactions_current && row.focused() && !response.has_focus() {
            response.request_focus();
        }
        let visuals = ui.style().interact_selectable(&response, row.selected());
        if row.selected() || row.focused() || response.hovered() || response.has_focus() {
            ui.painter().rect_filled(
                bounds.intersect(viewport),
                visuals.corner_radius,
                visuals.weak_bg_fill,
            );
        }
        if bounds.intersects(viewport) {
            ui.painter().galley_with_override_text_color(
                pos2(
                    bounds.min.x + style.tab_horizontal_padding,
                    bounds.center().y - resource.galley.size().y * 0.5,
                ),
                resource.galley.clone(),
                visuals.text_color(),
            );
        }
        if interactions_current && let Some(hit) = hit {
            let activation = semantic_activation_fact(ui, &response, hit, true, true);
            if let Some(activation) = activation {
                output.push_widget_activation(
                    ui,
                    RenderAction::ActivateTabListMenuRow {
                        surface: key.surface,
                        session: menu.session(),
                        tab: row.tab(),
                    },
                    response.id,
                    response.has_focus(),
                    activation,
                );
            }
        } else if interactions_current
            && ui.input(|input| input.has_accesskit_action_request(response.id, Action::Click))
        {
            output.push_accesskit(
                ui,
                RenderAction::ActivateTabListMenuRow {
                    surface: key.surface,
                    session: menu.session(),
                    tab: row.tab(),
                },
                response.id,
                Action::Click,
            );
        }
        if interactions_current
            && ui.input(|input| {
                input.has_accesskit_action_request(response.id, Action::ScrollIntoView)
            })
        {
            output.push_accesskit(
                ui,
                RenderAction::AdjustTabListMenuScroll {
                    surface: key.surface,
                    session: menu.session(),
                    adjustment: TabScrollAdjustment::reveal_item(tab.item),
                },
                response.id,
                Action::ScrollIntoView,
            );
        }
        if interactions_current
            && ui.input(|input| input.has_accesskit_action_request(response.id, Action::Focus))
        {
            output.push_accesskit(
                ui,
                RenderAction::NavigateTabListMenu {
                    surface: key.surface,
                    session: menu.session(),
                    navigation: TabListMenuNavigation::Focus(tab.item),
                },
                response.id,
                Action::Focus,
            );
        }
    }
}

fn tab_resource<'a>(
    resources: &'a EguiSurfacePaintResources,
    tab: &TabRecord,
) -> &'a TabPaintResource {
    resources
        .tab(*tab.id())
        .expect("one retained egui resource exists for every projected tab")
}

fn configure_authoritative_menu_accessibility(
    ui: &Ui,
    id: Id,
    viewport: Rect,
    interactions_current: bool,
) {
    ui.ctx().accesskit_node_builder(id, |node| {
        node.set_role(Role::Menu);
        node.set_bounds(accesskit_bounds(viewport));
        node.set_clips_children();
        if interactions_current {
            node.add_child_action(Action::ScrollIntoView);
        } else {
            node.clear_child_actions();
            node.set_disabled();
        }
    });
}

fn paint_authoritative_menu_scrollbar(
    ui: &Ui,
    instance_id: Id,
    surface: SurfaceId,
    menu: &dockspace::backend::scene::TabListMenuRecord,
    menu_rect: Rect,
    viewport: Rect,
    interactions_current: bool,
    output: &mut RenderOutput,
) {
    if menu.maximum_scroll_offset() <= 0.0 || viewport.max.x >= menu_rect.max.x {
        return;
    }
    let track = Rect::from_min_max(
        pos2(viewport.max.x, viewport.min.y),
        pos2(menu_rect.max.x, viewport.max.y),
    );
    let content_height = viewport.height() + menu.maximum_scroll_offset() as f32;
    let thumb_height = (viewport.height() * viewport.height() / content_height)
        .clamp(track.width(), track.height());
    let travel = (track.height() - thumb_height).max(0.0);
    let fraction = (menu.scroll_offset() / menu.maximum_scroll_offset()) as f32;
    let thumb = Rect::from_min_size(
        pos2(track.min.x, track.min.y + travel * fraction.clamp(0.0, 1.0)),
        egui::vec2(track.width(), thumb_height),
    );
    ui.painter()
        .rect_filled(track, 0.0, ui.visuals().extreme_bg_color);
    ui.painter().rect_filled(
        thumb,
        ui.visuals().window_corner_radius,
        ui.visuals().widgets.inactive.bg_fill,
    );
    let id = Id::new((
        "egui_dockspace",
        instance_id,
        "tab-list-menu-scrollbar",
        menu.session(),
    ));
    let response = ui.interact(interact_rect(track), id, Sense::hover());
    ui.ctx().accesskit_node_builder(id, |node| {
        node.set_role(Role::ScrollBar);
        node.set_bounds(accesskit_bounds(track));
        node.set_min_numeric_value(0.0);
        node.set_max_numeric_value(menu.maximum_scroll_offset());
        node.set_numeric_value(menu.scroll_offset());
        if interactions_current {
            node.add_action(Action::Increment);
            node.add_action(Action::Decrement);
        } else {
            node.clear_actions();
            node.set_disabled();
        }
    });
    if interactions_current {
        output.register_receiver(
            &response,
            PresentationHitRegionKind::TabListMenuScroll(menu.session()),
        );
    }
    if !interactions_current {
        return;
    }
    let step = menu
        .rows()
        .first()
        .map(|row| row.bounds().height())
        .filter(|height| height.is_finite() && *height > 0.0);
    let adjustment = ui.input(|input| {
        if input.has_accesskit_action_request(response.id, Action::Increment) {
            step.map(|delta| (delta, Action::Increment))
        } else if input.has_accesskit_action_request(response.id, Action::Decrement) {
            step.map(|delta| (-delta, Action::Decrement))
        } else {
            None
        }
    });
    if let Some((delta, requested)) = adjustment
        && let Ok(adjustment) = TabScrollAdjustment::scroll_by(delta)
    {
        output.push_accesskit(
            ui,
            RenderAction::AdjustTabListMenuScroll {
                surface,
                session: menu.session(),
                adjustment,
            },
            response.id,
            requested,
        );
    }
}

fn capture_authoritative_menu_input(
    ui: &Ui,
    surface: SurfaceId,
    menu: &dockspace::backend::scene::TabListMenuRecord,
    output: &mut RenderOutput,
) {
    if ui.input_mut(|input| input.consume_key(egui::Modifiers::NONE, Key::Escape)) {
        output.push_unmodified_key(
            ui,
            RenderAction::DismissTabListMenu {
                surface,
                session: menu.session(),
            },
            Key::Escape,
        );
        return;
    }
    if let Some((navigation, key)) = consume_tab_list_menu_navigation(ui) {
        output.push_unmodified_key(
            ui,
            RenderAction::NavigateTabListMenu {
                surface,
                session: menu.session(),
                navigation,
            },
            key,
        );
    }
}

fn consume_tab_list_menu_navigation(ui: &Ui) -> Option<(TabListMenuNavigation, Key)> {
    ui.input_mut(|input| {
        [
            (Key::ArrowUp, TabListMenuNavigation::Previous),
            (Key::ArrowDown, TabListMenuNavigation::Next),
            (Key::Home, TabListMenuNavigation::First),
            (Key::End, TabListMenuNavigation::Last),
        ]
        .into_iter()
        .find_map(|(key, navigation)| {
            input
                .consume_key(egui::Modifiers::NONE, key)
                .then_some((navigation, key))
        })
    })
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
    configure_button_accessibility(
        ui,
        &response,
        rect,
        "Show hidden tabs".to_owned(),
        interactions_current,
    );
    let hovered = interactions_current && (response.hovered() || response.has_focus());
    paint_overflow_icon(ui, rect, style, hovered);
    response
}

fn paint_overflow_icon(ui: &Ui, rect: Rect, style: &DockStyle, hovered: bool) {
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
}

fn consume_overflow_toggle_keys(ui: &Ui) {
    let modifiers = ui.input(|input| input.modifiers);
    ui.input_mut(|input| {
        input.consume_key(modifiers, Key::Enter);
        input.consume_key(modifiers, Key::Space);
    });
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

fn current_tab_reveal_identity(
    ui: &Ui,
    instance_id: Id,
    pane: &PaneRecord,
    bar: &TabBarRecord,
    active_drag: Option<ActiveDragView<'_>>,
) -> TabRevealIdentity {
    let keyboard_focused = bar
        .members()
        .iter()
        .map(|member| member.tab().item)
        .find(|item| ui.memory(|memory| memory.has_focus(tab_id(ui, instance_id, *item))));
    let active_dragged = active_drag.and_then(|drag| {
        let MovePayload::Item(source) = drag.payload() else {
            return None;
        };
        (source.root() == bar.id().root && source.tabs() == bar.id().tabs).then_some(source.item())
    });
    TabRevealIdentity::new(pane.selected(), keyboard_focused, active_dragged)
}

#[allow(
    clippy::too_many_arguments,
    reason = "scroll reduction explicitly binds widget, drag, reveal, ownership, and authority facts"
)]
fn update_tab_strip_scroll(
    ui: &mut Ui,
    surface: SurfaceId,
    bar: &TabBarRecord,
    controls: &[&TabStripControlRecord],
    style: &DockStyle,
    active_drag: Option<ActiveDragView<'_>>,
    reveal_identity: TabRevealIdentity,
    tab_scroll_owner: Option<TabBarSceneId>,
    interactions_current: bool,
    output: &mut RenderOutput,
) -> bool {
    let Some(viewport) = from_logical_rect(bar.viewport()) else {
        return false;
    };
    if !interactions_current
        || tab_scroll_owner != Some(*bar.id())
        || bar.maximum_scroll_offset() <= 0.0
        || !viewport.is_positive()
    {
        return false;
    }
    let Some(pointer) = ui.input(|input| input.pointer.hover_pos()) else {
        return false;
    };
    if !contains_half_open(viewport, pointer) {
        return false;
    }

    let mut scrolled = false;
    let mut drag_adjustment = 0.0;
    if active_drag.is_some_and(|drag| drag.phase() == DragPhase::Dragging) {
        let scroll_back = controls.iter().find_map(|control| {
            matches!(control.id(), TabStripControlId::ScrollBackward(id) if id == *bar.id())
                .then(|| from_logical_rect(control.bounds()))
                .flatten()
        });
        let scroll_forward = controls.iter().find_map(|control| {
            matches!(control.id(), TabStripControlId::ScrollForward(id) if id == *bar.id())
                .then(|| from_logical_rect(control.bounds()))
                .flatten()
        });
        if scroll_back.is_some_and(|rect| contains_half_open(rect, pointer)) {
            drag_adjustment -= style.tab_min_width;
        } else if scroll_forward.is_some_and(|rect| contains_half_open(rect, pointer)) {
            drag_adjustment += style.tab_min_width;
        }
    }

    if drag_adjustment != 0.0
        && let Ok(adjustment) = TabScrollAdjustment::scroll_by_preserving(
            f64::from(drag_adjustment),
            reveal_identity.prioritized_items(),
        )
    {
        output.push_post_batch_continuation(RenderAction::AdjustTabStripScroll {
            surface,
            bar: *bar.id(),
            adjustment,
        });
        scrolled = true;
    }
    scrolled
}

#[allow(clippy::too_many_arguments)]
fn paint_tab(
    ui: &mut Ui,
    instance_id: Id,
    surface: SurfaceId,
    bar: &TabBarRecord,
    tab: &TabRecord,
    resource: &TabPaintResource,
    tab_strip_states: &mut TabStripStateMap,
    semantic_selected: Option<ItemId>,
    workspace: &Workspace,
    style: &DockStyle,
    interactions_current: bool,
    interaction_scene: Option<SurfaceSceneStamp>,
    local_gesture_scene: Option<SurfaceSceneStamp>,
    authoritative_plan: &PresentationPlan,
    output: &mut RenderOutput,
) {
    let (Some(rect), Some(drag_rect)) = (
        from_logical_rect(tab.visible_bounds()),
        from_logical_rect(tab.drag_hit().rect()),
    ) else {
        return;
    };
    let interactions_current = interactions_current
        && authoritative_plan.region_is_operable(tab.drag_hit().rect(), tab.layer());
    let tab_scene = *tab.id();
    let tab_id = tab_id(ui, instance_id, tab_scene.item);
    let response = ui.interact(interact_rect(drag_rect), tab_id, Sense::click_and_drag());
    output.register_receiver(&response, PresentationHitRegionKind::TabBody(tab_scene));
    let key = TabStripKey::new(surface, *bar.id());
    let continued_keyboard_focus = interactions_current
        && pending_tab_keyboard_focus(tab_strip_states, key) == Some(tab_scene.item);
    if continued_keyboard_focus {
        response.request_focus();
        complete_tab_keyboard_focus(tab_strip_states, key, tab_scene.item);
    }
    configure_tab_accessibility(
        ui,
        &response,
        instance_id,
        tab,
        resource,
        semantic_selected,
        interactions_current,
    );

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
    let selected = tab.selected();
    paint_tab_body(ui, tab, resource, rect, style, selected, hovered, focused);

    capture_local_tab_gesture(
        &response,
        surface,
        tab_scene,
        local_gesture_scene.filter(|_| interactions_current),
        output,
    );

    if let Some(close_bounds) = tab.close_bounds()
        && let Some(close_rect) = from_logical_rect(close_bounds)
    {
        let close_interactions_current = interactions_current
            && authoritative_plan.region_is_operable(close_bounds, tab.layer());
        paint_close_button(
            ui,
            tab,
            resource,
            close_rect,
            instance_id,
            style,
            selected,
            close_interactions_current,
            close_interactions_current
                .then_some(interaction_scene)
                .flatten(),
            output,
        );
    }

    let activation = semantic_activation_fact(
        ui,
        &response,
        drag_rect,
        interactions_current,
        interactions_current,
    );
    if activation.is_some() {
        response.request_focus();
    }
    let selection_cause = interactions_current.then(|| {
        if let Some(activation) = activation {
            Some(TabSelectionCause::WidgetActivation(activation))
        } else if accesskit_focus_requested(ui, &response) {
            Some(TabSelectionCause::AccessKitFocus)
        } else if activation.is_none()
            && response.gained_focus()
            && !response.is_pointer_button_down_on()
            && !continued_keyboard_focus
        {
            Some(TabSelectionCause::KeyboardFocusTraversal)
        } else {
            None
        }
    });
    if let Some(selection_cause) = selection_cause.flatten()
        && semantic_selected != Some(tab_scene.item)
        && let Some(scene) = interaction_scene
        && let Some(source) = output.capture(workspace.capture_item_source(
            tab_scene.root,
            tab_scene.tabs,
            tab_scene.item,
        ))
    {
        let action = RenderAction::Select {
            scene,
            tab: tab_scene,
            source,
        };
        match selection_cause {
            TabSelectionCause::WidgetActivation(activation) => {
                output.push_widget_activation(
                    ui,
                    action,
                    response.id,
                    response.has_focus(),
                    activation,
                );
            }
            TabSelectionCause::AccessKitFocus => {
                output.push_accesskit(ui, action, response.id, Action::Focus);
            }
            TabSelectionCause::KeyboardFocusTraversal => output.push_key(ui, action, Key::Tab),
        }
    }

    if interactions_current && retained_focus {
        keyboard_select(
            ui,
            instance_id,
            surface,
            bar,
            tab_strip_states,
            tab.ordinal(),
            workspace,
            interaction_scene.expect("current tab interaction retains its Ready scene"),
            output,
        );
    }
}

fn capture_local_tab_gesture(
    response: &Response,
    surface: SurfaceId,
    source: TabSceneId,
    scene: Option<SurfaceSceneStamp>,
    output: &mut RenderOutput,
) {
    let Some(scene) = scene else {
        return;
    };
    let current = response
        .interact_pointer_pos()
        .and_then(|position| to_logical_point(position).ok());
    let phase = if response.drag_started_by(egui::PointerButton::Primary) {
        let (Some(current), Some(initial)) = (
            current,
            response
                .interact_pointer_pos()
                .map(|position| position - response.total_drag_delta().unwrap_or_default())
                .and_then(|position| to_logical_point(position).ok()),
        ) else {
            return;
        };
        LocalTabGesturePhase::Begin {
            scene,
            initial,
            current,
        }
    } else if response.drag_stopped_by(egui::PointerButton::Primary) {
        current.map_or(LocalTabGesturePhase::Cancel, |current| {
            LocalTabGesturePhase::Release { current }
        })
    } else if response.dragged_by(egui::PointerButton::Primary) {
        let Some(current) = current else {
            return;
        };
        LocalTabGesturePhase::Move { current }
    } else {
        return;
    };
    output.push_local_response_action(RenderAction::LocalTabGesture {
        surface,
        source,
        phase,
    });
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum TabSelectionCause {
    WidgetActivation(SemanticActivation),
    AccessKitFocus,
    KeyboardFocusTraversal,
}

fn paint_tab_body(
    ui: &Ui,
    tab: &TabRecord,
    resource: &TabPaintResource,
    rect: Rect,
    style: &DockStyle,
    selected: bool,
    hovered: bool,
    focused: bool,
) {
    let fill = if selected {
        style.tab_active_fill
    } else if hovered {
        style.tab_hover_fill
    } else {
        style.tab_fill
    };
    let text_color = if selected {
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
    let Some(text_rect) = from_logical_rect(tab.text_bounds()) else {
        return;
    };
    let Some(full_rect) = from_logical_rect(tab.full_bounds()) else {
        return;
    };
    ui.painter_at(text_rect).galley_with_override_text_color(
        pos2(
            full_rect.min.x + style.tab_horizontal_padding,
            text_rect.center().y - resource.galley.size().y * 0.5,
        ),
        resource.galley.clone(),
        text_color,
    );
}

#[allow(clippy::too_many_arguments)]
fn paint_close_button(
    ui: &mut Ui,
    tab: &TabRecord,
    resource: &TabPaintResource,
    rect: Rect,
    instance_id: Id,
    style: &DockStyle,
    selected: bool,
    interactions_current: bool,
    interaction_scene: Option<SurfaceSceneStamp>,
    output: &mut RenderOutput,
) {
    let tab_scene = *tab.id();
    let response = ui.interact(
        interact_rect(rect),
        ui.make_persistent_id((instance_id, "tab-close", tab_scene.item)),
        Sense::click(),
    );
    output.register_receiver(&response, PresentationHitRegionKind::TabClose(tab_scene));
    configure_button_accessibility(
        ui,
        &response,
        rect,
        format!("Close {}", resource.title),
        interactions_current,
    );
    let color = if selected || interactions_current && (response.hovered() || response.has_focus())
    {
        style.tab_active_text_color
    } else {
        style.tab_text_color
    };
    if interactions_current && response.hovered() {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }
    paint_close_icon_with_color(ui, rect, color);

    let Some(scene) = interaction_scene else {
        return;
    };
    let Some(activation) = semantic_activation_fact(
        ui,
        &response,
        rect,
        interactions_current,
        interactions_current,
    ) else {
        return;
    };
    output.push_widget_activation(
        ui,
        RenderAction::RequestSemanticClose {
            scene,
            target: CloseSceneTarget::Tab(tab_scene),
        },
        response.id,
        response.has_focus(),
        activation,
    );
}

fn paint_close_icon(ui: &Ui, rect: Rect, style: &DockStyle, selected: bool) {
    let color = if selected {
        style.tab_active_text_color
    } else {
        style.tab_text_color
    };
    paint_close_icon_with_color(ui, rect, color);
}

fn paint_close_icon_with_color(ui: &Ui, rect: Rect, color: egui::Color32) {
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
}

fn keyboard_select(
    ui: &Ui,
    instance_id: Id,
    surface: SurfaceId,
    bar: &TabBarRecord,
    tab_strip_states: &mut TabStripStateMap,
    current: usize,
    workspace: &Workspace,
    scene: SurfaceSceneStamp,
    output: &mut RenderOutput,
) {
    let members = bar.members();
    if members.len() < 2 {
        return;
    }
    let Some(current) = members
        .iter()
        .position(|member| member.ordinal() == current)
    else {
        return;
    };
    let next = ui.input(|input| {
        if input.key_pressed(Key::ArrowLeft) {
            Some((
                current.checked_sub(1).unwrap_or(members.len() - 1),
                true,
                Key::ArrowLeft,
            ))
        } else if input.key_pressed(Key::ArrowRight) {
            Some(((current + 1) % members.len(), true, Key::ArrowRight))
        } else if input.key_pressed(Key::Home) {
            Some((0, false, Key::Home))
        } else if input.key_pressed(Key::End) {
            Some((members.len() - 1, false, Key::End))
        } else {
            None
        }
    });
    let Some((next, clear_cardinal_navigation, key)) = next.filter(|(next, _, _)| *next != current)
    else {
        return;
    };
    if clear_cardinal_navigation {
        ui.memory_mut(|memory| memory.move_focus(FocusDirection::None));
    }
    let next_tab = members[next];
    let next_scene = next_tab.tab();
    if let Some(source) = output.capture(workspace.capture_item_source(
        next_scene.root,
        next_scene.tabs,
        next_scene.item,
    )) {
        output.push_key(
            ui,
            RenderAction::Select {
                scene,
                tab: next_scene,
                source,
            },
            key,
        );
        let target_requires_reprojection =
            next_tab.visibility() == TabStripMemberVisibility::Hidden;
        let key = TabStripKey::new(surface, *bar.id());
        let focus_continuation_changed =
            request_tab_keyboard_focus(tab_strip_states, key, next_scene.item);
        if target_requires_reprojection && focus_continuation_changed {
            ui.ctx()
                .request_discard("dockspace keyboard focus target must be revealed");
            ui.ctx().request_repaint();
        }
        ui.ctx().memory_mut(|memory| {
            memory.request_focus(tab_id(ui, instance_id, next_scene.item));
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

fn paint_selected_panel(
    ui: &mut Ui,
    instance_id: Id,
    pane: &PaneRecord,
    selected: Option<ItemId>,
    content_resources: &EguiSurfacePaintResources,
    panes: &mut dyn PaneView,
    style: &DockStyle,
    pane_content_current: bool,
    output: &mut RenderOutput,
) {
    let Some(item) = selected else {
        return;
    };
    let tab = TabSceneId {
        root: pane.id().root,
        tabs: pane.id().tabs,
        item,
    };
    let Some(resource) = content_resources.tab(tab) else {
        return;
    };
    let Some(content_rect) = from_logical_rect(pane.content_bounds()) else {
        return;
    };
    let mut panel_ui = ui.new_child(
        UiBuilder::new()
            .id(Id::new((instance_id, "pane", item)))
            .max_rect(content_rect),
    );
    panel_ui.set_clip_rect(content_rect.intersect(ui.clip_rect()));
    panel_ui.set_min_size(content_rect.size());
    let pane_receiver = panel_ui.interact(
        interact_rect(content_rect),
        panel_ui.id().with("dockspace-pane-receiver"),
        Sense::click_and_drag(),
    );
    output.register_receiver(
        &pane_receiver,
        PresentationHitRegionKind::PaneBody(pane.id()),
    );
    if !pane_content_current {
        panel_ui.disable();
    }
    panel_ui
        .ctx()
        .accesskit_node_builder(panel_ui.id(), |node| {
            node.set_role(Role::TabPanel);
            node.set_bounds(accesskit_bounds(content_rect));
            node.set_label(resource.title.clone());
            if !pane_content_current {
                node.set_disabled();
            }
        });

    if resource.missing {
        paint_centered_label(
            &panel_ui,
            content_rect,
            format!("Pane {} is unavailable", item.get()),
            style.tab_text_color,
        );
    } else {
        panes.ui(item, &mut panel_ui);
    }
}

fn configure_tab_list_accessibility(
    ui: &Ui,
    instance_id: Id,
    bar: &TabBarRecord,
    visible_tabs: &[&TabRecord],
    semantic_selected: Option<ItemId>,
    interactions_current: bool,
) {
    let selected_id = semantic_selected
        .filter(|item| visible_tabs.iter().any(|tab| tab.id().item == *item))
        .map(|item| tab_id(ui, instance_id, item).accesskit_id());
    let Some(viewport) = from_logical_rect(bar.viewport()) else {
        return;
    };
    ui.ctx().accesskit_node_builder(ui.id(), |node| {
        node.set_role(Role::TabList);
        node.set_bounds(accesskit_bounds(viewport));
        node.set_orientation(Orientation::Horizontal);
        if !interactions_current {
            node.set_disabled();
        }
        if let Some(selected_id) = selected_id {
            node.set_active_descendant(selected_id);
        }
    });
}

fn configure_tab_accessibility(
    ui: &Ui,
    response: &egui::Response,
    instance_id: Id,
    tab: &TabRecord,
    resource: &TabPaintResource,
    semantic_selected: Option<ItemId>,
    interactions_current: bool,
) {
    let item = tab.id().item;
    let panel_id = Id::new((instance_id, "pane", item));
    ui.ctx().accesskit_node_builder(response.id, |node| {
        node.set_role(Role::Tab);
        node.set_bounds(accesskit_bounds(response.rect));
        node.set_label(resource.title.clone());
        if interactions_current {
            node.add_action(Action::Focus);
            node.add_action(Action::Click);
        } else {
            node.clear_actions();
            node.set_disabled();
        }
        node.push_controlled(panel_id.accesskit_id());
        if semantic_selected == Some(item) {
            node.set_selected(true);
        } else {
            node.clear_selected();
        }
    });
}

fn configure_button_accessibility(
    ui: &Ui,
    response: &egui::Response,
    rect: Rect,
    label: String,
    interactions_current: bool,
) {
    ui.ctx().accesskit_node_builder(response.id, |node| {
        node.set_role(Role::Button);
        node.set_bounds(accesskit_bounds(rect));
        node.set_label(label);
        if interactions_current {
            node.add_action(Action::Focus);
            node.add_action(Action::Click);
        } else {
            node.clear_actions();
            node.set_disabled();
        }
    });
}

fn tab_id(ui: &Ui, instance_id: Id, item: ItemId) -> Id {
    ui.make_persistent_id((instance_id, "tab", item))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(egui_backend_event_envelope)]
    #[test]
    fn native_scroll_receiver_keeps_ownership_at_both_bounds() {
        assert_eq!(
            native_scroll_config(egui::ScrollProjection::HorizontalElseVertical),
            egui::ScrollReceiverConfig::new(
                egui::ScrollProjection::HorizontalElseVertical,
                egui::Vec2::ONE,
                egui::ScrollAxisCapabilities::BOTH,
                egui::ScrollAxisCapabilities::NONE,
            )
        );
        assert_eq!(
            native_scroll_config(egui::ScrollProjection::VerticalElseHorizontal),
            egui::ScrollReceiverConfig::new(
                egui::ScrollProjection::VerticalElseHorizontal,
                egui::Vec2::ONE,
                egui::ScrollAxisCapabilities::NONE,
                egui::ScrollAxisCapabilities::BOTH,
            )
        );
    }

    #[test]
    fn close_button_never_overlaps_tab_drag_rect() {
        let tab = Rect::from_min_max(egui::Pos2::new(0.0, 0.0), egui::Pos2::new(100.0, 28.0));
        let close = Rect::from_min_max(egui::Pos2::new(78.0, 6.0), egui::Pos2::new(94.0, 22.0));
        let drag = Rect::from_min_max(tab.min, pos2(close.min.x, tab.max.y));

        assert!(drag.max.x <= close.min.x);
        assert!(!drag.intersect(close).is_positive());
    }
}
