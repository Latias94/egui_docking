//! Pane and tab rendering over opaque product paint records.

use dockspace::model::ItemId;
use dockspace::runtime::{
    ContainedPaintRecord, SurfaceGesturePhase, SurfaceTabNavigation, TabGroupDragRegionKind,
    TabPaintRecord,
};
use egui::accesskit::{Action, Role};
use egui::{
    Align, CursorIcon, EventFilter, Id, Key, Layout, PointerButton, Sense, Stroke, StrokeKind, Ui,
    UiBuilder, pos2,
};

use super::RenderContext;
use super::actions::{
    LocalScrollAxis, button_activated, consume_local_scroll_input, gesture_phase,
    local_scroll_input,
};
use super::geometry::{accesskit_bounds, egui_rect};
use super::measurement::TabPaintResource;
use super::schedule::RootPaintSchedule;

#[derive(Clone, Copy)]
pub(crate) enum RootVisualContext<'plan> {
    Main,
    Contained {
        record: ContainedPaintRecord<'plan>,
        pane_fill: egui::Color32,
    },
}

impl<'plan> RootVisualContext<'plan> {
    const fn contained(self) -> Option<ContainedPaintRecord<'plan>> {
        match self {
            Self::Main => None,
            Self::Contained { record, .. } => Some(record),
        }
    }

    const fn pane_fill(self, workspace_fill: egui::Color32) -> egui::Color32 {
        match self {
            Self::Main => workspace_fill,
            Self::Contained { pane_fill, .. } => pane_fill,
        }
    }
}

pub(crate) fn paint_root(
    context: &mut RenderContext<'_, '_, '_>,
    root: &RootPaintSchedule<'_>,
    visual_context: RootVisualContext<'_>,
) {
    paint_panes(context, root, visual_context);
    paint_tab_bars(context, root, visual_context.contained());
    paint_tabs(context, root, visual_context.contained());
}

fn paint_panes(
    context: &mut RenderContext<'_, '_, '_>,
    root: &RootPaintSchedule<'_>,
    visual_context: RootVisualContext<'_>,
) {
    let pane_fill = visual_context.pane_fill(context.visuals.workspace_fill);
    for pane in root.panes() {
        let paint_omitted = context
            .plan
            .drag_decoration()
            .is_some_and(|decoration| decoration.omits_visual(pane.visual_id()));
        let Some(bounds) = egui_rect(pane.bounds()) else {
            continue;
        };
        if !paint_omitted {
            context.ui.painter().rect_filled(bounds, 0.0, pane_fill);
        }
        let Some(pane_rect) = egui_rect(pane.content_bounds()) else {
            continue;
        };
        let pane_id =
            context
                .ui
                .make_persistent_id((context.instance_id, "pane-body", pane.visual_id()));
        let _response = context.interact_receiver(
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
        if paint_omitted {
            // Keep pane widget identities alive so focus and application-owned
            // accessibility state survive the temporary source gap.
            child.set_opacity(0.0);
        }
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
            if context
                .pane_focus_request
                .is_some_and(|request| request.item() == item)
                && !*context.pane_focus_target_requested
                && let Some(target) = context.panes.focus_target(item)
            {
                context.ui.memory_mut(|memory| memory.request_focus(target));
                *context.pane_focus_target_requested = true;
            }
            context.panes.ui(item, &mut child);
        }
    }
}

fn paint_tab_bars(
    context: &mut RenderContext<'_, '_, '_>,
    root: &RootPaintSchedule<'_>,
    contained: Option<ContainedPaintRecord<'_>>,
) {
    for bar in root.tab_bars() {
        let paint_omitted = context
            .plan
            .drag_decoration()
            .is_some_and(|decoration| decoration.omits_visual(bar.visual_id()));
        if !paint_omitted && let Some(bounds) = egui_rect(bar.bounds()) {
            context
                .ui
                .painter()
                .rect_filled(bounds, 0.0, context.visuals.tab_bar_fill);
        }
        let id = context.ui.make_persistent_id((
            context.instance_id,
            "tab-strip-scroll",
            bar.visual_id(),
        ));
        let receiver = context.plan.receiver_for_tab_strip_scroll(bar);
        context.register_scroll_receiver(id, receiver);
        if let Some(scroll) = local_scroll_input(
            context.ui,
            context.pointer_authority,
            LocalScrollAxis::Horizontal,
        ) && let Some(action) =
            context
                .plan
                .prepare_tab_strip_scroll_at(bar, scroll.point(), scroll.offset_delta())
        {
            consume_local_scroll_input(context.ui, scroll);
            context.push_local_action(action);
        }
        paint_group_drag_regions(context, bar, contained);
    }
}

fn paint_group_drag_regions(
    context: &mut RenderContext<'_, '_, '_>,
    bar: dockspace::runtime::TabBarPaintRecord<'_>,
    contained: Option<ContainedPaintRecord<'_>>,
) {
    for region in bar.group_drag_regions() {
        let Some(bounds) = egui_rect(region.bounds()) else {
            continue;
        };
        let id = context.ui.make_persistent_id((
            context.instance_id,
            "tab-group-drag-region",
            region.visual_id(),
        ));
        let response = context
            .interact_receiver(
                bounds,
                id,
                Sense::click_and_drag(),
                context.plan.receiver_for_tab_group_drag_region(region),
            )
            .on_hover_text("Drag tab group");
        let locally_dragged = context.response_dragged_locally(&response);
        if response.hovered() || locally_dragged {
            context.ui.ctx().set_cursor_icon(if locally_dragged {
                CursorIcon::Grabbing
            } else {
                CursorIcon::Grab
            });
        }
        if region.kind() == TabGroupDragRegionKind::LeadingGrip {
            let paint_omitted = context
                .plan
                .drag_decoration()
                .is_some_and(|decoration| decoration.omits_visual(region.visual_id()));
            if !paint_omitted {
                paint_group_grip_icon(
                    context.ui,
                    bounds,
                    context.interaction_stroke(&response, context.style.visuals.tab_text_color),
                );
            }
        }
        let phase = context
            .pointer_authority
            .accepts_local_pointer_actions()
            .then(|| gesture_phase(&response))
            .flatten();
        if let Some(phase) = phase
            && let Some(action) = context
                .plan
                .prepare_tab_group_gesture(bar.visual_id(), phase)
            && context.accept_drag_phase_once(
                response.id,
                phase,
                "egui_dockspace: settle tab-group drag decoration",
            )
        {
            if matches!(phase, SurfaceGesturePhase::Begin { .. })
                && let Some(contained) = contained
            {
                context.claim_contained_activation(contained);
            }
            context.push_preview_gesture_action(action, phase);
        }
    }
}

fn paint_group_grip_icon(ui: &Ui, rect: egui::Rect, stroke: Stroke) {
    let spacing = 3.5_f32.min(rect.width() * 0.18).min(rect.height() * 0.18);
    let radius = 1.0_f32.min(spacing * 0.32);
    for x in [-0.5, 0.5] {
        for y in [-1.0, 0.0, 1.0] {
            ui.painter().circle_filled(
                rect.center() + egui::vec2(x * spacing, y * spacing),
                radius,
                stroke.color,
            );
        }
    }
}

fn paint_tabs(
    context: &mut RenderContext<'_, '_, '_>,
    root: &RootPaintSchedule<'_>,
    contained: Option<ContainedPaintRecord<'_>>,
) {
    for tab in root.tabs() {
        let Some(resource) = context.resources.tab(tab.visual_id()).cloned() else {
            continue;
        };
        paint_tab(context, tab, &resource, contained);
    }
}

fn paint_tab(
    context: &mut RenderContext<'_, '_, '_>,
    tab: TabPaintRecord<'_>,
    resource: &TabPaintResource,
    contained: Option<ContainedPaintRecord<'_>>,
) {
    let Some(visible) = egui_rect(tab.visible_bounds()) else {
        return;
    };
    let Some(drag) = egui_rect(tab.drag_bounds()) else {
        return;
    };
    let id = tab_id(context.ui, context.instance_id, tab);
    let operable = tab.operable() && context.ui.is_enabled();
    let receiver = operable
        .then(|| context.plan.receiver_for_tab_body(tab))
        .flatten();
    let response = context.interact_receiver(
        drag,
        id,
        if operable {
            Sense::click_and_drag()
        } else {
            Sense::hover()
        },
        receiver,
    );
    let locally_dragged = context.response_dragged_locally(&response);
    if operable && (response.hovered() || locally_dragged) {
        context.ui.ctx().set_cursor_icon(if locally_dragged {
            CursorIcon::Grabbing
        } else {
            CursorIcon::Grab
        });
    }
    let source_gap = context
        .plan
        .drag_decoration()
        .is_some_and(|decoration| decoration.omits_visual(tab.visual_id()));
    let selected = tab.selected();
    let focused = response.has_focus();
    if !source_gap {
        let fill = if selected {
            context.visuals.tab_active_fill
        } else if RenderContext::response_emphasized(&response) {
            context.interaction_fill(&response)
        } else {
            context.visuals.tab_fill
        };
        context.ui.painter().rect_filled(visible, 0.0, fill);
        if focused {
            context.ui.painter().rect_stroke(
                visible.shrink(1.0),
                0.0,
                Stroke::new(1.0, context.visuals.drop_border_color),
                StrokeKind::Inside,
            );
        }
        if let Some(text) = egui_rect(tab.text_bounds()) {
            let color = if selected {
                context.visuals.tab_active_text_color
            } else if RenderContext::response_emphasized(&response) {
                context
                    .interaction_stroke(&response, context.style.visuals.tab_text_color)
                    .color
            } else {
                context.visuals.tab_text_color
            };
            context.ui.painter_at(text).galley_with_override_text_color(
                pos2(text.min.x, text.center().y - resource.galley.size().y * 0.5),
                resource.galley.clone(),
                color,
            );
        }
    }
    configure_tab_accessibility(
        context.ui,
        id,
        visible,
        resource.title.as_str(),
        selected,
        operable,
    );
    if operable {
        capture_tab_actions(
            context,
            tab,
            resource,
            &response,
            focused,
            !source_gap,
            contained,
        );
    }
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
    paint_close_icon: bool,
    contained: Option<ContainedPaintRecord<'_>>,
) {
    let id = response.id;
    let phase = context
        .pointer_authority
        .accepts_local_pointer_actions()
        .then(|| gesture_phase(response))
        .flatten();
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
        if (!tab.selected() || keyboard_activation || accesskit_activation)
            && let Some(action) = context.plan.prepare_tab_select(tab.item())
        {
            if pointer_activation && let Some(contained) = contained {
                context.claim_contained_activation(contained);
            }
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
                    memory.request_focus(tab_id(context.ui, context.instance_id, target_tab));
                });
            }
            context.push_local_action(action);
        }
    }
    if let Some(phase) = phase
        && let Some(action) = context.plan.prepare_tab_gesture(tab.item(), phase)
        && context.accept_drag_phase_once(
            response.id,
            phase,
            "egui_dockspace: settle tab drag decoration",
        )
    {
        if matches!(phase, SurfaceGesturePhase::Begin { .. })
            && let Some(contained) = contained
        {
            context.claim_contained_activation(contained);
        }
        context.push_preview_gesture_action(action, phase);
    }

    if let Some(close_bounds) = tab.close_bounds().and_then(egui_rect) {
        paint_close(
            context,
            tab.item(),
            close_bounds,
            resource.title.as_str(),
            tab.selected() || response.hovered() || response.has_focus(),
            context.plan.receiver_for_tab_close(tab),
            paint_close_icon,
        );
    }
}

fn paint_close(
    context: &mut RenderContext<'_, '_, '_>,
    item: ItemId,
    rect: egui::Rect,
    title: &str,
    tab_emphasized: bool,
    receiver: Option<dockspace::runtime::DockspaceReceiverDescriptor>,
    paint_icon: bool,
) {
    let id = context
        .ui
        .make_persistent_id((context.instance_id, "tab-close", item));
    let response = context.interact_receiver(rect, id, Sense::click(), receiver);
    let visuals = *context.ui.style().interact(&response);
    if paint_icon && (tab_emphasized || RenderContext::response_emphasized(&response)) {
        let paint_rect = rect.expand(visuals.expansion);
        let inset = paint_rect.width().min(paint_rect.height()) * 0.28;
        let stroke = context.interaction_stroke(&response, context.style.visuals.tab_text_color);
        context.ui.painter().line_segment(
            [
                paint_rect.left_top() + egui::vec2(inset, inset),
                paint_rect.right_bottom() - egui::vec2(inset, inset),
            ],
            stroke,
        );
        context.ui.painter().line_segment(
            [
                paint_rect.right_top() + egui::vec2(-inset, inset),
                paint_rect.left_bottom() + egui::vec2(inset, -inset),
            ],
            stroke,
        );
    }
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

fn configure_tab_accessibility(
    ui: &Ui,
    id: Id,
    rect: egui::Rect,
    title: &str,
    selected: bool,
    operable: bool,
) {
    ui.ctx().accesskit_node_builder(id, |node| {
        node.set_role(Role::Tab);
        node.set_bounds(accesskit_bounds(rect));
        node.set_label(title);
        if operable {
            node.add_action(Action::Focus);
            node.add_action(Action::Click);
        } else {
            node.set_disabled();
        }
        if selected {
            node.set_selected(true);
        } else {
            node.clear_selected();
        }
    });
}
