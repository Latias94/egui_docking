//! Explicit contained-floating chrome without `Area` or `Window` state.

use dockspace::backend::engine::LocalContainedGesturePhase;
use dockspace::backend::interaction::{
    ActiveContainedTransformView, ActiveDragView, InteractionStatus,
};
use dockspace::backend::presentation_hit::PresentationHitRegionKind;
use dockspace::backend::scene::{
    ContainedRecord, ContainedResizeDirection, PresentationPlan, SurfaceSceneStamp,
};
use dockspace::geometry::{LogicalRect, LogicalSize};
use dockspace::graph::Workspace;
use dockspace::ids::{FloatingPresentationId, RootId, SurfaceId};
use dockspace::intent::{CloseSceneTarget, ContainedGestureKind, TabGestureSource};
use egui::accesskit::{Action, Orientation, Role};
use egui::{
    CursorIcon, EventFilter, FocusDirection, Id, Key, Rect, Sense, Stroke, StrokeKind, TextStyle,
    Ui, pos2, vec2,
};

use crate::hit::{interact_rect, semantic_activation_fact};
use crate::projection::EguiSurfacePaintResources;
use crate::renderer::{
    ContainedResizeEdge, RenderAction, RenderOutput, accesskit_bounds, from_logical_rect,
    to_logical_point,
};
use crate::style::DockStyle;
use crate::tabs::capture_local_tab_gesture;

#[derive(Clone, Copy)]
struct ResizeContext {
    scene: Option<SurfaceSceneStamp>,
    surface: SurfaceId,
    root: RootId,
    floating: FloatingPresentationId,
    status: InteractionStatus,
    expected_rect: Option<LogicalRect>,
    minimum_size: LogicalSize,
}

pub(crate) fn paint_background(
    ui: &Ui,
    instance_id: Id,
    surface: SurfaceId,
    floating: &ContainedRecord,
    frontmost: bool,
    style: &DockStyle,
    output: &mut RenderOutput,
) {
    let Some(outer_rect) = from_logical_rect(floating.outer_bounds()) else {
        return;
    };
    let Some(title_rect) = from_logical_rect(floating.title_bounds()) else {
        return;
    };
    let root = floating.root();
    let floating_id = floating.floating();
    let painter = ui.painter_at(outer_rect);
    painter.rect_filled(outer_rect, 0.0, style.floating_fill);
    painter.rect_filled(
        title_rect,
        0.0,
        if frontmost {
            style.floating_title_active_fill
        } else {
            style.floating_title_fill
        },
    );
    painter.rect_stroke(
        outer_rect,
        0.0,
        Stroke::new(style.floating_border_width, style.floating_border_color),
        StrokeKind::Inside,
    );

    let window_id = ui.make_persistent_id((
        "egui_dockspace",
        instance_id,
        surface,
        root,
        floating_id,
        "contained-window",
    ));
    ui.ctx().accesskit_node_builder(window_id, |node| {
        node.set_role(Role::Window);
        node.set_bounds(accesskit_bounds(outer_rect));
    });

    // Egui tracks click and drag capture independently. Keeping two responses
    // preserves that distinction when chrome overrides only one receiver lane.
    for (lane, sense) in [("click", Sense::click()), ("drag", Sense::drag())] {
        let blocker_id = ui.make_persistent_id((
            "egui_dockspace",
            instance_id,
            surface,
            root,
            floating_id,
            "contained-window-blocker",
            lane,
        ));
        let blocker = ui.interact(interact_rect(outer_rect), blocker_id, sense);
        output.register_receiver(
            &blocker,
            PresentationHitRegionKind::ContainedFrameBlocker(floating_id),
        );
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn paint_chrome_and_interact(
    ui: &mut Ui,
    instance_id: Id,
    surface: SurfaceId,
    plan: &PresentationPlan,
    floating: &ContainedRecord,
    resources: &EguiSurfacePaintResources,
    workspace: &Workspace,
    style: &DockStyle,
    status: InteractionStatus,
    interaction_scene: Option<SurfaceSceneStamp>,
    local_gesture_scene: Option<SurfaceSceneStamp>,
    active_drag: Option<ActiveDragView<'_>>,
    active_transform: Option<ActiveContainedTransformView<'_>>,
    output: &mut RenderOutput,
) {
    let interactions_current = interaction_scene.is_some();
    let local_gesture_current = local_gesture_scene.is_some();
    let root = floating.root();
    let floating_id = floating.floating();
    let title = floating_title(plan, root, resources);
    let Some(title_drag_rect) = from_logical_rect(floating.title_drag_hit().rect()) else {
        return;
    };
    let Some(title_rect) = from_logical_rect(floating.title_bounds()) else {
        return;
    };
    let title_drag_available = plan
        .pane_records()
        .iter()
        .any(|pane| pane.id().root == root && pane.selected().is_some());
    let resize = ResizeContext {
        scene: interaction_scene,
        surface,
        root,
        floating: floating_id,
        status,
        expected_rect: workspace
            .contained_floating(floating_id)
            .filter(|record| record.root == root)
            .map(|record| record.rect),
        minimum_size: floating.minimum_size(),
    };

    let title_id = ui.make_persistent_id((
        "egui_dockspace",
        instance_id,
        surface,
        root,
        floating_id,
        "contained-title",
    ));
    let title_response = ui.interact(interact_rect(title_drag_rect), title_id, Sense::drag());
    output.register_receiver(
        &title_response,
        PresentationHitRegionKind::ContainedTitle(floating_id),
    );
    configure_title_accessibility(
        ui,
        &title_response,
        title_rect,
        &title,
        interactions_current,
    );
    ui.painter_at(title_drag_rect).text(
        pos2(
            title_drag_rect.min.x + style.tab_horizontal_padding,
            title_drag_rect.center().y,
        ),
        egui::Align2::LEFT_CENTER,
        &title,
        TextStyle::Button.resolve(ui.style()),
        style.tab_active_text_color,
    );

    if interactions_current && title_drag_available {
        if title_response.hovered() || title_response.dragged() {
            ui.ctx().set_cursor_icon(if title_response.dragged() {
                CursorIcon::Grabbing
            } else {
                CursorIcon::Grab
            });
        }
    }
    let local_drag_continuation =
        active_drag.is_some_and(|drag| drag.local_response_surface() == Some(surface));
    if local_gesture_current || local_drag_continuation {
        let allow_capture = title_drag_available || local_drag_continuation;
        if allow_capture {
            capture_local_tab_gesture(
                &title_response,
                surface,
                TabGestureSource::ContainedTitle {
                    root,
                    floating: floating_id,
                },
                local_gesture_scene,
                active_drag,
                output,
            );
        }
    }

    paint_resize_handles(
        ui,
        instance_id,
        floating,
        &resize,
        interactions_current,
        local_gesture_scene,
        active_transform,
        style.splitter_keyboard_step,
        output,
    );

    if let Some(close_rect) = floating.close_bounds().and_then(from_logical_rect) {
        paint_close(
            ui,
            instance_id,
            surface,
            root,
            floating_id,
            close_rect,
            style,
            interactions_current,
            interaction_scene,
            output,
        );
    }
}

fn paint_resize_handles(
    ui: &Ui,
    instance_id: Id,
    floating: &ContainedRecord,
    resize: &ResizeContext,
    interactions_current: bool,
    local_gesture_scene: Option<SurfaceSceneStamp>,
    active_transform: Option<ActiveContainedTransformView<'_>>,
    keyboard_step: f32,
    output: &mut RenderOutput,
) {
    for resize_record in floating.resize() {
        let direction = resize_record.direction();
        let Some(rect) = from_logical_rect(resize_record.hit().rect()) else {
            continue;
        };
        let edge = accessible_resize_edge(direction);
        let id = ui.make_persistent_id((
            "egui_dockspace",
            instance_id,
            resize.surface,
            resize.root,
            resize.floating,
            direction,
            "contained-resize",
        ));
        let retained_pending_focus =
            edge.is_some() && !interactions_current && ui.memory(|memory| memory.has_focus(id));
        let sense = if edge.is_some() {
            Sense::drag()
        } else {
            Sense::DRAG
        };
        let response = ui.interact(interact_rect(rect), id, sense);
        output.register_receiver(
            &response,
            PresentationHitRegionKind::ContainedResize {
                floating: resize.floating,
                direction,
            },
        );
        capture_local_contained_resize(
            &response,
            resize.surface,
            resize.floating,
            direction,
            local_gesture_scene,
            active_transform,
            output,
        );
        let semantic_enabled = interactions_current && resize.status == InteractionStatus::Idle;
        if let (Some(edge), Some(expected_rect)) = (edge, resize.expected_rect)
            && semantic_enabled
        {
            configure_resize_accessibility(ui, &response, rect, expected_rect, edge);
            if response.has_focus() {
                lock_resize_navigation_focus(ui, response.id, edge);
            }
            if let Some(adjustment) = resize_adjustment(ui, response.id, edge, response.has_focus())
            {
                if adjustment.clear_cardinal_navigation {
                    ui.memory_mut(|memory| memory.move_focus(FocusDirection::None));
                }
                if let Some(scene) = resize.scene {
                    let action = RenderAction::AdjustContainedResize {
                        scene,
                        surface: resize.surface,
                        root: resize.root,
                        floating: resize.floating,
                        expected_rect,
                        minimum_size: resize.minimum_size,
                        edge,
                        delta: f64::from(keyboard_step) * f64::from(adjustment.direction),
                    };
                    match adjustment.source {
                        ResizeAdjustmentSource::Keyboard(key) => output.push_key(ui, action, key),
                        ResizeAdjustmentSource::AccessKit(requested) => {
                            output.push_accesskit(ui, action, response.id, requested);
                        }
                    }
                }
            }
        } else if response.has_focus() && !retained_pending_focus {
            response.surrender_focus();
        }
        if !interactions_current && local_gesture_scene.is_none() {
            continue;
        }
        if response.hovered() || response.dragged() {
            ui.ctx().set_cursor_icon(resize_cursor(direction));
        }
    }
}

fn capture_local_contained_resize(
    response: &egui::Response,
    surface: SurfaceId,
    floating: FloatingPresentationId,
    direction: ContainedResizeDirection,
    scene: Option<SurfaceSceneStamp>,
    active_transform: Option<ActiveContainedTransformView<'_>>,
    output: &mut RenderOutput,
) {
    let owns_active = active_transform.is_some_and(|transform| {
        transform.surface() == surface && transform.floating() == floating
    });
    let Some(scene) = scene else {
        if owns_active && response.drag_stopped_by(egui::PointerButton::Primary) {
            output.push_local_response_action(RenderAction::LocalContainedGesture {
                surface,
                floating,
                kind: ContainedGestureKind::Resize(direction),
                phase: LocalContainedGesturePhase::Cancel,
            });
        }
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
        LocalContainedGesturePhase::Begin {
            scene,
            initial,
            current,
        }
    } else if response.drag_stopped_by(egui::PointerButton::Primary) {
        current.map_or(LocalContainedGesturePhase::Cancel, |current| {
            LocalContainedGesturePhase::Release { scene, current }
        })
    } else if response.dragged_by(egui::PointerButton::Primary) {
        let Some(current) = current else {
            return;
        };
        LocalContainedGesturePhase::Move { scene, current }
    } else {
        return;
    };
    output.push_local_response_action(RenderAction::LocalContainedGesture {
        surface,
        floating,
        kind: ContainedGestureKind::Resize(direction),
        phase,
    });
}

#[derive(Clone, Copy)]
struct ResizeAdjustment {
    direction: i8,
    clear_cardinal_navigation: bool,
    source: ResizeAdjustmentSource,
}

#[derive(Clone, Copy)]
enum ResizeAdjustmentSource {
    Keyboard(Key),
    AccessKit(Action),
}

fn resize_adjustment(
    ui: &Ui,
    id: Id,
    edge: ContainedResizeEdge,
    focused: bool,
) -> Option<ResizeAdjustment> {
    let keyboard = focused.then(|| {
        ui.input(|input| {
            let adjustment = match edge {
                ContainedResizeEdge::Top | ContainedResizeEdge::Bottom => {
                    if input.key_pressed(Key::ArrowUp) {
                        Some((-1, Key::ArrowUp))
                    } else if input.key_pressed(Key::ArrowDown) {
                        Some((1, Key::ArrowDown))
                    } else {
                        None
                    }
                }
                ContainedResizeEdge::Right | ContainedResizeEdge::Left => {
                    if input.key_pressed(Key::ArrowLeft) {
                        Some((-1, Key::ArrowLeft))
                    } else if input.key_pressed(Key::ArrowRight) {
                        Some((1, Key::ArrowRight))
                    } else {
                        None
                    }
                }
            };
            adjustment.map(|(direction, key)| ResizeAdjustment {
                direction,
                clear_cardinal_navigation: true,
                source: ResizeAdjustmentSource::Keyboard(key),
            })
        })
    });
    if let Some(adjustment) = keyboard.flatten() {
        return Some(adjustment);
    }

    ui.input(|input| {
        if input.has_accesskit_action_request(id, Action::Decrement) {
            Some(ResizeAdjustment {
                direction: -1,
                clear_cardinal_navigation: false,
                source: ResizeAdjustmentSource::AccessKit(Action::Decrement),
            })
        } else if input.has_accesskit_action_request(id, Action::Increment) {
            Some(ResizeAdjustment {
                direction: 1,
                clear_cardinal_navigation: false,
                source: ResizeAdjustmentSource::AccessKit(Action::Increment),
            })
        } else {
            None
        }
    })
}

fn lock_resize_navigation_focus(ui: &Ui, id: Id, edge: ContainedResizeEdge) {
    let horizontal = matches!(edge, ContainedResizeEdge::Right | ContainedResizeEdge::Left);
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

fn floating_title(
    plan: &PresentationPlan,
    root: RootId,
    resources: &EguiSurfacePaintResources,
) -> String {
    plan.pane_records()
        .iter()
        .filter(|pane| pane.id().root == root)
        .find_map(|pane| {
            pane.selected().and_then(|item| {
                resources.tab(dockspace::backend::scene::TabSceneId {
                    root,
                    tabs: pane.id().tabs,
                    item,
                })
            })
        })
        .map_or_else(
            || "Dock group".to_owned(),
            |resource| resource.title.clone(),
        )
}

#[allow(clippy::too_many_arguments)]
fn paint_close(
    ui: &Ui,
    instance_id: Id,
    surface: SurfaceId,
    root: RootId,
    floating: FloatingPresentationId,
    rect: Rect,
    style: &DockStyle,
    interactions_current: bool,
    interaction_scene: Option<SurfaceSceneStamp>,
    output: &mut RenderOutput,
) {
    let id = ui.make_persistent_id((
        "egui_dockspace",
        instance_id,
        surface,
        root,
        floating,
        "contained-close",
    ));
    let response = ui.interact(interact_rect(rect), id, Sense::click());
    output.register_receiver(
        &response,
        PresentationHitRegionKind::ContainedClose(floating),
    );
    ui.ctx().accesskit_node_builder(response.id, |node| {
        node.set_role(Role::Button);
        node.set_bounds(accesskit_bounds(rect));
        node.set_label("Close floating group");
        if interactions_current {
            node.add_action(Action::Focus);
            node.add_action(Action::Click);
        } else {
            node.clear_actions();
            node.set_disabled();
        }
    });
    let color = if interactions_current && (response.hovered() || response.has_focus()) {
        style.tab_active_text_color
    } else {
        style.tab_text_color
    };
    if interactions_current && response.hovered() {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }
    let radius = rect.width().min(rect.height()) * 0.22;
    ui.painter().line_segment(
        [
            rect.center() - vec2(radius, radius),
            rect.center() + vec2(radius, radius),
        ],
        Stroke::new(1.25, color),
    );
    ui.painter().line_segment(
        [
            rect.center() + vec2(-radius, radius),
            rect.center() + vec2(radius, -radius),
        ],
        Stroke::new(1.25, color),
    );

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
            target: CloseSceneTarget::Contained(floating),
        },
        response.id,
        response.has_focus(),
        activation,
    );
}

fn resize_cursor(direction: ContainedResizeDirection) -> CursorIcon {
    match direction {
        ContainedResizeDirection::North => CursorIcon::ResizeNorth,
        ContainedResizeDirection::NorthEast => CursorIcon::ResizeNorthEast,
        ContainedResizeDirection::East => CursorIcon::ResizeEast,
        ContainedResizeDirection::SouthEast => CursorIcon::ResizeSouthEast,
        ContainedResizeDirection::South => CursorIcon::ResizeSouth,
        ContainedResizeDirection::SouthWest => CursorIcon::ResizeSouthWest,
        ContainedResizeDirection::West => CursorIcon::ResizeWest,
        ContainedResizeDirection::NorthWest => CursorIcon::ResizeNorthWest,
    }
}

fn configure_title_accessibility(
    ui: &Ui,
    response: &egui::Response,
    rect: Rect,
    title: &str,
    interactions_current: bool,
) {
    ui.ctx().accesskit_node_builder(response.id, |node| {
        node.set_role(Role::TitleBar);
        node.set_bounds(accesskit_bounds(rect));
        node.set_label(title);
        if interactions_current {
            node.add_action(Action::Focus);
        } else {
            node.clear_actions();
            node.set_disabled();
        }
    });
}

fn configure_resize_accessibility(
    ui: &Ui,
    response: &egui::Response,
    rect: Rect,
    current_rect: LogicalRect,
    edge: ContainedResizeEdge,
) {
    ui.ctx().accesskit_node_builder(response.id, |node| {
        node.set_role(Role::Splitter);
        node.set_bounds(accesskit_bounds(rect));
        node.set_label(match edge {
            ContainedResizeEdge::Top => "Resize top edge",
            ContainedResizeEdge::Right => "Resize right edge",
            ContainedResizeEdge::Bottom => "Resize bottom edge",
            ContainedResizeEdge::Left => "Resize left edge",
        });
        node.set_orientation(match edge {
            ContainedResizeEdge::Top | ContainedResizeEdge::Bottom => Orientation::Horizontal,
            ContainedResizeEdge::Right | ContainedResizeEdge::Left => Orientation::Vertical,
        });
        node.add_action(Action::Focus);
        node.add_action(Action::Increment);
        node.add_action(Action::Decrement);
        node.set_numeric_value(match edge {
            ContainedResizeEdge::Top => current_rect.min().y(),
            ContainedResizeEdge::Right => current_rect.max().x(),
            ContainedResizeEdge::Bottom => current_rect.max().y(),
            ContainedResizeEdge::Left => current_rect.min().x(),
        });
    });
}

const fn accessible_resize_edge(
    direction: ContainedResizeDirection,
) -> Option<ContainedResizeEdge> {
    match direction {
        ContainedResizeDirection::North => Some(ContainedResizeEdge::Top),
        ContainedResizeDirection::East => Some(ContainedResizeEdge::Right),
        ContainedResizeDirection::South => Some(ContainedResizeEdge::Bottom),
        ContainedResizeDirection::West => Some(ContainedResizeEdge::Left),
        ContainedResizeDirection::NorthEast
        | ContainedResizeDirection::SouthEast
        | ContainedResizeDirection::SouthWest
        | ContainedResizeDirection::NorthWest => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_resize_direction_has_a_directional_cursor() {
        for direction in [
            ContainedResizeDirection::NorthWest,
            ContainedResizeDirection::North,
            ContainedResizeDirection::NorthEast,
            ContainedResizeDirection::East,
            ContainedResizeDirection::SouthEast,
            ContainedResizeDirection::South,
            ContainedResizeDirection::SouthWest,
            ContainedResizeDirection::West,
        ] {
            assert_ne!(resize_cursor(direction), CursorIcon::Default);
        }
    }
}
