//! Pane and tab rendering over opaque product paint records.

use dockspace::model::ItemId;
use dockspace::runtime::{
    DockspaceVisualId, PanePaintRecord, SurfacePaintDisposition, SurfaceTabNavigation,
    TabGroupDragRegionKind, TabPaintRecord,
};
use egui::accesskit::{Action, Role};
use egui::{
    Align, CursorIcon, EventFilter, Id, Key, Layout, PointerButton, Sense, Stroke, StrokeKind, Ui,
    UiBuilder, pos2,
};

use super::actions::{button_activation_origin, effective_enabled};
use super::geometry::{accesskit_bounds, egui_rect, inset_ui_rect};
use super::measurement::TabPaintResource;
use super::{ProductReceiverBinding, RenderContext};

#[derive(Clone, Copy)]
pub(crate) enum RootVisualContext {
    Main,
    Contained { pane_fill: egui::Color32 },
}

impl RootVisualContext {
    const fn pane_fill(self, workspace_fill: egui::Color32) -> egui::Color32 {
        match self {
            Self::Main => workspace_fill,
            Self::Contained { pane_fill, .. } => pane_fill,
        }
    }
}

pub(crate) fn paint_pane(
    context: &mut RenderContext<'_, '_, '_>,
    pane: PanePaintRecord<'_>,
    visual_context: RootVisualContext,
    paint: SurfacePaintDisposition,
) {
    let pane_fill = visual_context.pane_fill(context.visuals.workspace_fill);
    let paint_omitted = context.source_gap_active(paint);
    let Some(bounds) = egui_rect(pane.bounds()) else {
        return;
    };
    if !paint_omitted {
        context.ui.painter().rect_filled(bounds, 0.0, pane_fill);
    }
    let Some(pane_rect) = egui_rect(pane.content_bounds()) else {
        return;
    };
    let pane_id =
        context
            .ui
            .make_persistent_id((context.instance_id, "pane-body", pane.visual_id()));
    let pane_receiver = context.plan.receiver_for_pane(pane);
    let _response =
        context.interact_receiver(pane_rect, pane_id, Sense::click_and_drag(), pane_receiver);
    let Some(item) = pane.selected() else {
        return;
    };
    let Some(content_rect) = inset_ui_rect(pane_rect) else {
        return;
    };
    let mut child = context.ui.new_child(
        UiBuilder::new()
            .id_salt((context.instance_id, "pane", pane.visual_id()))
            .max_rect(content_rect)
            .layout(Layout::top_down(Align::Min)),
    );
    if paint_omitted {
        // Keep pane widget identities alive so focus and application-owned
        // accessibility state survive the temporary source gap.
        child.set_opacity(0.0);
    }
    child.set_clip_rect(pane_rect);
    let mut paint_audit = context.paint_audit.take();
    let painter = child.painter().clone();
    // Keep ownership outside the callback so an unwind releases the append
    // checkpoint before Ui::drop, while normal finalization remains audited.
    let mut child = Some(child);
    let mut paint_contents = || {
        let child_ui = child.as_mut().expect("the pane child is still open");
        if context
            .resources
            .item(item)
            .is_some_and(|resource| resource.missing)
        {
            child_ui.centered_and_justified(|ui| {
                ui.label(format!("Missing pane {}", item.get()));
            });
        } else {
            if context
                .pane_focus_request
                .is_some_and(|request| request.item() == Some(item))
                && context.pointer_authority.supports_preview_gestures()
                && !*context.pane_focus_target_requested
                && let Some(target) = context.panes.focus_target(item)
            {
                context.ui.memory_mut(|memory| memory.request_focus(target));
                *context.pane_focus_target_requested = true;
                *context.pane_focus_target = Some(target);
            }
            let pane_response = context.panes.ui(item, child_ui);
            if let Some(target) = context.panes.focus_target(item)
                && child_ui.ctx().viewport(|viewport| {
                    viewport
                        .this_pass
                        .widgets
                        .get(target)
                        .is_some_and(|widget| {
                            widget.enabled
                                && widget.visible
                                && widget.sense.is_focusable()
                                && widget.layer_id == child_ui.layer_id()
                                && widget.rect.is_finite()
                                && widget.rect.is_positive()
                                && pane_rect.contains_rect(widget.rect)
                        })
                })
            {
                context.pane_focus_bindings.push((item, target));
            }
            if let Some(receiver) = pane_receiver
                && pane_rect.is_finite()
                && pane_rect.is_positive()
            {
                for widget_id in pane_response.content_widgets() {
                    let Some(widget) = child_ui
                        .ctx()
                        .viewport(|viewport| viewport.this_pass.widgets.get(widget_id).copied())
                    else {
                        continue;
                    };
                    if widget.enabled
                        && widget.layer_id == child_ui.layer_id()
                        && widget.interact_rect.is_finite()
                        && widget.interact_rect.is_positive()
                        && pane_rect.contains_rect(widget.interact_rect)
                    {
                        context.receivers.push(ProductReceiverBinding {
                            interact_rect: widget.interact_rect,
                            interactive: widget.sense.interactive(),
                            widget_id: widget.id,
                            layer_id: widget.layer_id,
                            receiver,
                        });
                    }
                }
            }
        }
        drop(child.take());
    };
    if let Some(audit) = paint_audit.as_mut() {
        audit.check_append(&painter, paint_contents);
    } else {
        paint_contents();
    }
    context.paint_audit = paint_audit;
}

pub(crate) fn paint_tab_bar(
    context: &mut RenderContext<'_, '_, '_>,
    bar: dockspace::runtime::TabBarPaintRecord<'_>,
    paint: SurfacePaintDisposition,
) {
    let paint_omitted = context.source_gap_active(paint);
    if !paint_omitted && let Some(bounds) = egui_rect(bar.bounds()) {
        context
            .ui
            .painter()
            .rect_filled(bounds, 0.0, context.visuals.tab_bar_fill);
    }
    let id =
        context
            .ui
            .make_persistent_id((context.instance_id, "tab-strip-scroll", bar.visual_id()));
    let receiver = effective_enabled(context.ui, true)
        .then(|| context.plan.receiver_for_tab_strip_scroll(bar))
        .flatten();
    context.register_scroll_receiver(id, receiver);
    paint_group_drag_regions(context, bar, paint);
    #[cfg(feature = "native-render-support")]
    publish_native_tab_strip_diagnostics(context, bar);
}

#[cfg(feature = "native-render-support")]
fn publish_native_tab_strip_diagnostics(
    context: &RenderContext<'_, '_, '_>,
    bar: dockspace::runtime::TabBarPaintRecord<'_>,
) {
    use dockspace::runtime::TabStripMemberVisibility;

    if !super::native_drag_preview_diagnostic_enabled(context.ui.ctx()) {
        return;
    }
    let Some(viewport) = egui_rect(bar.viewport()) else {
        return;
    };
    for member in bar.members() {
        let visibility = match member.visibility() {
            TabStripMemberVisibility::Visible => "fully-visible",
            TabStripMemberVisibility::PartiallyVisible => "partially-visible",
            TabStripMemberVisibility::Hidden => continue,
        };
        if context.resources.tab_visual(context.plan, member.item()) != Some(member.visual_id()) {
            continue;
        }
        let Some(resource) = context.resources.item(member.item()) else {
            continue;
        };
        let Some(full) = egui_rect(member.full_bounds()) else {
            continue;
        };
        let title = resource.title.as_str();
        let id = tab_id_for_visual(context.ui, context.instance_id, member.visual_id());
        context
            .ui
            .ctx()
            .accesskit_node_builder(id.with("native-tab-full-bounds"), |node| {
                node.set_role(Role::Label);
                node.set_label(format!("Dockspace tab full bounds {title}"));
                node.set_value(visibility);
                node.set_bounds(accesskit_bounds(full));
            });
        context
            .ui
            .ctx()
            .accesskit_node_builder(id.with("native-tab-strip-viewport"), |node| {
                node.set_role(Role::Label);
                node.set_label(format!("Dockspace tab strip viewport {title}"));
                node.set_value(format!(
                    "offset={};maximum={}",
                    bar.scroll_offset(),
                    bar.maximum_scroll_offset(),
                ));
                node.set_bounds(accesskit_bounds(viewport));
            });
    }
}

fn paint_group_drag_regions(
    context: &mut RenderContext<'_, '_, '_>,
    bar: dockspace::runtime::TabBarPaintRecord<'_>,
    paint: SurfacePaintDisposition,
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
        let receiver = context.plan.receiver_for_tab_group_drag_region(region);
        let enabled = effective_enabled(
            context.ui,
            receiver.is_some() && context.pointer_authority.supports_preview_gestures(),
        );
        let response = context.interact_receiver(
            bounds,
            id,
            if enabled {
                Sense::click_and_drag()
            } else {
                Sense::hover()
            },
            enabled.then_some(receiver).flatten(),
        );
        let response = if enabled {
            response.on_hover_text("Drag tab group")
        } else {
            response
        };
        let source_gap = context.source_gap_active(paint);
        if enabled && (response.hovered() || source_gap) {
            context.ui.ctx().set_cursor_icon(if source_gap {
                CursorIcon::Grabbing
            } else {
                CursorIcon::Grab
            });
        }
        if region.kind() == TabGroupDragRegionKind::LeadingGrip {
            let paint_omitted = context.source_gap_active(paint);
            if !paint_omitted && enabled {
                paint_group_grip_icon(
                    context.ui,
                    bounds,
                    context.interaction_stroke(&response, context.style.visuals.tab_text_color),
                );
            }
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

pub(crate) fn paint_tab_record(
    context: &mut RenderContext<'_, '_, '_>,
    tab: TabPaintRecord<'_>,
    paint: SurfacePaintDisposition,
) {
    let resources = context.resources;
    let Some(resource) = resources.item(tab.item()) else {
        return;
    };
    paint_tab(context, tab, resource, paint);
}

pub(crate) fn paint_tab(
    context: &mut RenderContext<'_, '_, '_>,
    tab: TabPaintRecord<'_>,
    resource: &TabPaintResource,
    paint: SurfacePaintDisposition,
) {
    let Some(visible) = egui_rect(tab.visible_bounds()) else {
        return;
    };
    let Some(drag) = egui_rect(tab.drag_bounds()) else {
        return;
    };
    let id = tab_id(context.ui, context.instance_id, tab);
    let operable = tab.operable() && context.ui.is_enabled();
    let preview_gestures_available = context.pointer_authority.supports_preview_gestures();
    let receiver = (operable && preview_gestures_available)
        .then(|| context.plan.receiver_for_tab_body(tab))
        .flatten();
    let response = context.interact_receiver(
        drag,
        id,
        if operable && preview_gestures_available {
            Sense::click_and_drag()
        } else if operable {
            Sense::click()
        } else {
            Sense::hover()
        },
        receiver,
    );
    let response = response.on_hover_text(resource.title.as_str());
    let source_gap = context.source_gap_active(paint);
    if operable && preview_gestures_available && (response.hovered() || source_gap) {
        context.ui.ctx().set_cursor_icon(if source_gap {
            CursorIcon::Grabbing
        } else {
            CursorIcon::Grab
        });
    }
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
        drag,
        resource.title.as_str(),
        selected,
        operable,
    );
    #[cfg(feature = "native-render-support")]
    publish_native_tab_bounds_diagnostic(context.ui, id, visible, resource.title.as_str());
    if operable {
        capture_tab_actions(context, tab, resource, &response, focused, !source_gap);
    }
}

#[cfg(feature = "native-render-support")]
fn publish_native_tab_bounds_diagnostic(ui: &Ui, tab: Id, visible: egui::Rect, title: &str) {
    if !super::native_drag_preview_diagnostic_enabled(ui.ctx()) {
        return;
    }
    ui.ctx()
        .accesskit_node_builder(tab.with("native-tab-bounds"), |node| {
            node.set_role(Role::Label);
            node.set_label(format!("Dockspace tab bounds {title}"));
            node.set_bounds(accesskit_bounds(visible));
        });
}

fn tab_id(ui: &Ui, instance_id: Id, tab: TabPaintRecord<'_>) -> Id {
    tab_id_for_visual(ui, instance_id, tab.visual_id())
}

fn tab_id_for_visual(ui: &Ui, instance_id: Id, visual: DockspaceVisualId) -> Id {
    ui.make_persistent_id((instance_id, "tab", visual))
}

fn capture_tab_actions(
    context: &mut RenderContext<'_, '_, '_>,
    tab: TabPaintRecord<'_>,
    resource: &TabPaintResource,
    response: &egui::Response,
    focused: bool,
    paint_close_icon: bool,
) {
    let id = response.id;
    let keyboard_activation = focused
        .then(|| {
            context.ui.input(|input| {
                [Key::Enter, Key::Space]
                    .into_iter()
                    .find(|&key| input.key_pressed(key))
            })
        })
        .flatten();
    let accesskit_activation = context.ui.input(|input| {
        if input.has_accesskit_action_request(id, Action::Click) {
            Some(id.with("accesskit-click"))
        } else if input.has_accesskit_action_request(id, Action::Focus) {
            Some(id.with("accesskit-focus"))
        } else {
            None
        }
    });
    let pointer_activation = context.pointer_authority.accepts_local_pointer_actions()
        && response.clicked_by(PointerButton::Primary);
    let origin = keyboard_activation
        .map(|key| id.with(("keyboard-activate", key)))
        .or(accesskit_activation)
        .or_else(|| pointer_activation.then(|| id.with("pointer-click")));
    if let Some(origin) = origin {
        response.request_focus();
        if (!tab.selected() || keyboard_activation.is_some() || accesskit_activation.is_some())
            && let Some(action) = context.plan.prepare_tab_select(tab.item())
        {
            context.push_local_action(origin, action);
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
                Some((Key::ArrowLeft, SurfaceTabNavigation::Previous))
            } else if input.consume_key(egui::Modifiers::NONE, Key::ArrowRight) {
                Some((Key::ArrowRight, SurfaceTabNavigation::Next))
            } else if input.consume_key(egui::Modifiers::NONE, Key::Home) {
                Some((Key::Home, SurfaceTabNavigation::First))
            } else if input.consume_key(egui::Modifiers::NONE, Key::End) {
                Some((Key::End, SurfaceTabNavigation::Last))
            } else {
                None
            }
        });
        if let Some((key, navigation)) = navigation
            && let Some(action) = context.plan.prepare_tab_navigation(tab.item(), navigation)
        {
            context
                .ui
                .memory_mut(|memory| memory.move_focus(egui::FocusDirection::None));
            if let Some(target) = action.tab_focus_target()
                && let Some(target_visual) = context.resources.tab_visual(context.plan, target)
            {
                context.ui.memory_mut(|memory| {
                    memory.request_focus(tab_id_for_visual(
                        context.ui,
                        context.instance_id,
                        target_visual,
                    ));
                });
            }
            context.push_local_action(id.with(("keyboard-navigation", key)), action);
        }
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
    let enabled = effective_enabled(context.ui, receiver.is_some());
    let response = context.interact_receiver(
        rect,
        id,
        if enabled {
            Sense::click()
        } else {
            Sense::hover()
        },
        enabled.then_some(receiver).flatten(),
    );
    let response = response.on_hover_text(format!("Close {title}"));
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
        if enabled {
            node.add_action(Action::Click);
        } else {
            node.set_disabled();
        }
    });
    if enabled
        && let Some(origin) =
            button_activation_origin(context.ui, &response, context.pointer_authority)
        && let Some(action) = context.plan.prepare_tab_close(item)
    {
        context.push_local_action(origin, action);
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

#[cfg(all(test, feature = "native-render-support"))]
mod tests {
    use super::*;

    #[test]
    fn native_tab_diagnostic_is_opt_in_and_passive_without_changing_drag_semantics() {
        for enabled in [false, true] {
            let context = egui::Context::default();
            context.enable_accesskit();
            if enabled {
                super::super::enable_native_drag_preview_diagnostic(&context);
            }
            let full = egui::Rect::from_min_max(pos2(28.0, 0.0), pos2(110.0, 28.0));
            let drag = egui::Rect::from_min_max(pos2(28.0, 0.0), pos2(76.0, 28.0));
            let output = context.run_ui(egui::RawInput::default(), |ui| {
                let id = ui.make_persistent_id("native-tab-diagnostic-test");
                configure_tab_accessibility(ui, id, drag, "Main", true, true);
                publish_native_tab_bounds_diagnostic(ui, id, full, "Main");
            });
            let nodes = &output
                .platform_output
                .accesskit_update
                .as_ref()
                .unwrap()
                .nodes;
            let tab = nodes
                .iter()
                .find(|(_, node)| node.role() == Role::Tab && node.label() == Some("Main"))
                .unwrap();
            assert_eq!(tab.1.bounds(), Some(accesskit_bounds(drag)));
            let diagnostic: Vec<_> = nodes
                .iter()
                .filter(|(_, node)| node.label() == Some("Dockspace tab bounds Main"))
                .collect();
            assert_eq!(diagnostic.len(), usize::from(enabled));
            if let [(_, node)] = diagnostic.as_slice() {
                assert_eq!(node.role(), Role::Label);
                assert_eq!(node.bounds(), Some(accesskit_bounds(full)));
                assert!(!node.supports_action(Action::Click));
                assert!(!node.supports_action(Action::Focus));
            }
            output.drop_without_applying_deltas();
        }
    }
}
