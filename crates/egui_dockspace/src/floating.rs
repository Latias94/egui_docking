//! Explicit contained-floating chrome without `Area` or `Window` state.

use dockspace::command::MovePayload;
use dockspace::graph::Workspace;
use dockspace::ids::{FloatingPresentationId, RootId, SurfaceId};
use dockspace::intent::{
    ContainedHorizontalResizeEdge, ContainedResizeEdges, ContainedTransformKind,
    ContainedVerticalResizeEdge,
};
use dockspace::interaction::{
    ActiveContainedTransformView, ActiveDragView, DragPhase, InteractionStatus,
};
use egui::accesskit::{Action, Orientation, Role};
use egui::{
    CursorIcon, Id, PointerButton, Rect, Sense, Stroke, StrokeKind, TextStyle, Ui, pos2, vec2,
};

use crate::hit::{contains_half_open, interact_rect, semantic_activation};
#[cfg(test)]
use crate::projection::floating_resize_zones;
use crate::projection::{FloatingPlan, FloatingResizeDirection, RootPlan};
use crate::renderer::{
    PRIMARY_BUTTON, PRIMARY_POINTER, RenderAction, RenderOutput, accesskit_bounds, to_logical_point,
};
use crate::style::DockStyle;

#[derive(Clone, Copy)]
struct TransformContext<'a> {
    surface: SurfaceId,
    root: RootId,
    floating: FloatingPresentationId,
    minimum_size: dockspace::geometry::LogicalSize,
    status: InteractionStatus,
    active: Option<ActiveContainedTransformView<'a>>,
}

pub(crate) fn paint_background(
    ui: &Ui,
    instance_id: Id,
    surface: SurfaceId,
    root: RootId,
    floating: &FloatingPlan,
    style: &DockStyle,
) {
    let painter = ui.painter_at(floating.outer_rect);
    painter.rect_filled(floating.outer_rect, 0.0, style.floating_fill);
    painter.rect_filled(
        floating.title_rect,
        0.0,
        if floating.frontmost {
            style.floating_title_active_fill
        } else {
            style.floating_title_fill
        },
    );
    painter.rect_stroke(
        floating.outer_rect,
        0.0,
        Stroke::new(style.floating_border_width, style.floating_border_color),
        StrokeKind::Inside,
    );

    let window_id = ui.make_persistent_id((
        "egui_dockspace",
        instance_id,
        surface,
        root,
        floating.id,
        "contained-window",
    ));
    ui.ctx().accesskit_node_builder(window_id, |node| {
        node.set_role(Role::Window);
        node.set_bounds(accesskit_bounds(floating.outer_rect));
    });

    let blocker_id = ui.make_persistent_id((
        "egui_dockspace",
        instance_id,
        surface,
        root,
        floating.id,
        "contained-window-blocker",
    ));
    let _ = ui.interact(
        interact_rect(floating.outer_rect),
        blocker_id,
        Sense::click_and_drag(),
    );
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn paint_chrome_and_interact(
    ui: &mut Ui,
    instance_id: Id,
    surface: SurfaceId,
    root: &RootPlan,
    floating: &FloatingPlan,
    workspace: &Workspace,
    style: &DockStyle,
    status: InteractionStatus,
    active_drag: Option<ActiveDragView<'_>>,
    active_transform: Option<ActiveContainedTransformView<'_>>,
    interactions_current: bool,
    output: &mut RenderOutput,
) {
    let title = floating_title(root);
    let title_drag_rect = floating.title_drag_rect;
    let transform = TransformContext {
        surface,
        root: root.root,
        floating: floating.id,
        minimum_size: floating.minimum_size,
        status,
        active: active_transform,
    };

    let title_id = ui.make_persistent_id((
        "egui_dockspace",
        instance_id,
        surface,
        root.root,
        floating.id,
        "contained-title",
    ));
    let title_response = ui.interact(interact_rect(title_drag_rect), title_id, Sense::drag());
    configure_title_accessibility(ui, &title_response, floating.title_rect, &title);
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

    if interactions_current {
        if title_response.hovered() || title_response.dragged() {
            ui.ctx().set_cursor_icon(if title_response.dragged() {
                CursorIcon::Grabbing
            } else {
                CursorIcon::Grab
            });
        }
        emit_transform_response(
            &title_response,
            title_drag_rect,
            transform,
            ContainedTransformKind::Move,
            output,
        );
    }

    paint_dock_grip(
        ui,
        instance_id,
        surface,
        root,
        floating,
        workspace,
        style,
        status,
        active_drag,
        interactions_current,
        output,
    );

    paint_resize_handles(
        ui,
        instance_id,
        floating,
        transform,
        interactions_current,
        output,
    );

    if let Some(close_rect) = floating.close_rect {
        paint_close(
            ui,
            instance_id,
            surface,
            root,
            floating.id,
            close_rect,
            workspace,
            style,
            interactions_current,
            output,
        );
    }
}

fn paint_resize_handles(
    ui: &Ui,
    instance_id: Id,
    floating: &FloatingPlan,
    transform: TransformContext<'_>,
    interactions_current: bool,
    output: &mut RenderOutput,
) {
    for &(direction, rect) in &floating.resize_zones {
        let id = ui.make_persistent_id((
            "egui_dockspace",
            instance_id,
            transform.surface,
            transform.root,
            transform.floating,
            direction,
            "contained-resize",
        ));
        let response = ui.interact(interact_rect(rect), id, Sense::drag());
        configure_resize_accessibility(ui, &response, rect, direction);
        if !interactions_current {
            continue;
        }
        if response.hovered() || response.dragged() {
            ui.ctx().set_cursor_icon(resize_cursor(direction));
        }
        emit_transform_response(
            &response,
            rect,
            transform,
            ContainedTransformKind::Resize(resize_edges(direction)),
            output,
        );
    }
}

#[allow(clippy::too_many_arguments)]
fn paint_dock_grip(
    ui: &Ui,
    instance_id: Id,
    surface: SurfaceId,
    root: &RootPlan,
    floating: &FloatingPlan,
    workspace: &Workspace,
    style: &DockStyle,
    status: InteractionStatus,
    active_drag: Option<ActiveDragView<'_>>,
    interactions_current: bool,
    output: &mut RenderOutput,
) {
    if !floating.dock_drag_rect.is_positive() {
        return;
    }
    let id = ui.make_persistent_id((
        "egui_dockspace",
        instance_id,
        surface,
        root.root,
        floating.id,
        "contained-dock-grip",
    ));
    let response = ui
        .interact(interact_rect(floating.dock_drag_rect), id, Sense::DRAG)
        .on_hover_text("Dock floating group");
    ui.ctx().accesskit_node_builder(id, |node| {
        node.set_role(Role::Button);
        node.set_bounds(accesskit_bounds(floating.dock_drag_rect));
        node.set_label("Dock floating group");
    });

    let active = interactions_current && (response.hovered() || response.dragged());
    if active {
        ui.ctx().set_cursor_icon(if response.dragged() {
            CursorIcon::Grabbing
        } else {
            CursorIcon::Grab
        });
    }
    paint_dock_grip_icon(ui, floating.dock_drag_rect, style, active);

    if interactions_current
        && status == InteractionStatus::Idle
        && response.is_pointer_button_down_on()
        && ui.input(|input| input.pointer.button_down(PointerButton::Primary))
        && ui
            .input(|input| input.pointer.press_origin())
            .is_some_and(|origin| contains_half_open(floating.dock_drag_rect, origin))
        && let Some(root_record) = workspace.root(root.root)
        && let Some(source) =
            output.capture(workspace.capture_node_source(root.root, root_record.node))
    {
        output.push(RenderAction::ArmDrag(MovePayload::Subtree(source)));
    }
    if interactions_current
        && (response.drag_started_by(PointerButton::Primary)
            || response.dragged_by(PointerButton::Primary))
        && let Some(active_drag) = active_drag.filter(|drag| drag.phase() == DragPhase::Armed)
        && let MovePayload::Subtree(source) = active_drag.payload()
        && source.root() == root.root
        && workspace
            .root(root.root)
            .is_some_and(|record| source.node() == record.node)
    {
        output.push(RenderAction::BeginDrag {
            session: active_drag.session(),
            pointer: PRIMARY_POINTER,
            button: PRIMARY_BUTTON,
        });
    }
}

fn paint_dock_grip_icon(ui: &Ui, rect: Rect, style: &DockStyle, active: bool) {
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
                rect.center() + vec2(x * spacing, y * spacing),
                radius,
                color,
            );
        }
    }
}

fn floating_title(root: &RootPlan) -> String {
    root.tabs
        .iter()
        .find_map(|tabs| tabs.tabs.iter().find(|tab| tab.selected))
        .map_or_else(|| "Dock group".to_owned(), |tab| tab.title.clone())
}

#[allow(clippy::too_many_arguments)]
fn paint_close(
    ui: &Ui,
    instance_id: Id,
    surface: SurfaceId,
    root: &RootPlan,
    floating: FloatingPresentationId,
    rect: Rect,
    workspace: &Workspace,
    style: &DockStyle,
    interactions_current: bool,
    output: &mut RenderOutput,
) {
    let id = ui.make_persistent_id((
        "egui_dockspace",
        instance_id,
        surface,
        root.root,
        floating,
        "contained-close",
    ));
    let response = ui.interact(interact_rect(rect), id, Sense::click());
    ui.ctx().accesskit_node_builder(response.id, |node| {
        node.set_role(Role::Button);
        node.set_bounds(accesskit_bounds(rect));
        node.set_label("Close floating group");
        node.add_action(Action::Focus);
        node.add_action(Action::Click);
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

    if semantic_activation(ui, &response, rect, interactions_current)
        && let Some(root_record) = workspace.root(root.root)
        && let Some(source) =
            output.capture(workspace.capture_node_source(root.root, root_record.node))
    {
        output.push(RenderAction::CloseRootRequested(source));
    }
}

pub(crate) fn request_raise(
    surface: SurfaceId,
    root: RootId,
    floating: &FloatingPlan,
    workspace: &Workspace,
    output: &mut RenderOutput,
) {
    let Some(Some(frontmost)) = output.capture(workspace.contained_frontmost(surface)) else {
        return;
    };
    if frontmost.floating() == floating.id {
        return;
    }
    output.push(RenderAction::RaiseContained {
        surface,
        root,
        floating: floating.id,
        expected_z_order: floating.z_order,
        expected_frontmost: frontmost,
    });
}

fn emit_transform_response(
    response: &egui::Response,
    region: Rect,
    transform: TransformContext<'_>,
    kind: ContainedTransformKind,
    output: &mut RenderOutput,
) {
    if (response.drag_started_by(PointerButton::Primary)
        || response.dragged_by(PointerButton::Primary))
        && transform.status == InteractionStatus::Idle
        && let Some(initial_position) = response.ctx.input(|input| input.pointer.press_origin())
        && contains_half_open(region, initial_position)
        && let Ok(initial_pointer) = to_logical_point(initial_position)
    {
        output.push(RenderAction::BeginContainedTransform {
            surface: transform.surface,
            root: transform.root,
            floating: transform.floating,
            pointer: PRIMARY_POINTER,
            button: PRIMARY_BUTTON,
            initial_pointer,
            kind,
            minimum_size: transform.minimum_size,
        });
    } else if response.dragged_by(PointerButton::Primary)
        && let Some(current_pointer) = response
            .interact_pointer_pos()
            .and_then(|position| to_logical_point(position).ok())
        && let Some(active) = transform.active.filter(|active| {
            active.surface() == transform.surface
                && active.root() == transform.root
                && active.floating() == transform.floating
                && active.pointer() == PRIMARY_POINTER
                && active.button() == PRIMARY_BUTTON
        })
    {
        output.push(RenderAction::UpdateContainedTransform {
            session: active.session(),
            current_pointer,
        });
    }
}

const fn resize_edges(direction: FloatingResizeDirection) -> ContainedResizeEdges {
    match direction {
        FloatingResizeDirection::North => {
            ContainedResizeEdges::vertical(ContainedVerticalResizeEdge::Top)
        }
        FloatingResizeDirection::NorthEast => ContainedResizeEdges::corner(
            ContainedHorizontalResizeEdge::Right,
            ContainedVerticalResizeEdge::Top,
        ),
        FloatingResizeDirection::East => {
            ContainedResizeEdges::horizontal(ContainedHorizontalResizeEdge::Right)
        }
        FloatingResizeDirection::SouthEast => ContainedResizeEdges::corner(
            ContainedHorizontalResizeEdge::Right,
            ContainedVerticalResizeEdge::Bottom,
        ),
        FloatingResizeDirection::South => {
            ContainedResizeEdges::vertical(ContainedVerticalResizeEdge::Bottom)
        }
        FloatingResizeDirection::SouthWest => ContainedResizeEdges::corner(
            ContainedHorizontalResizeEdge::Left,
            ContainedVerticalResizeEdge::Bottom,
        ),
        FloatingResizeDirection::West => {
            ContainedResizeEdges::horizontal(ContainedHorizontalResizeEdge::Left)
        }
        FloatingResizeDirection::NorthWest => ContainedResizeEdges::corner(
            ContainedHorizontalResizeEdge::Left,
            ContainedVerticalResizeEdge::Top,
        ),
    }
}

fn resize_cursor(direction: FloatingResizeDirection) -> CursorIcon {
    match direction {
        FloatingResizeDirection::North => CursorIcon::ResizeNorth,
        FloatingResizeDirection::NorthEast => CursorIcon::ResizeNorthEast,
        FloatingResizeDirection::East => CursorIcon::ResizeEast,
        FloatingResizeDirection::SouthEast => CursorIcon::ResizeSouthEast,
        FloatingResizeDirection::South => CursorIcon::ResizeSouth,
        FloatingResizeDirection::SouthWest => CursorIcon::ResizeSouthWest,
        FloatingResizeDirection::West => CursorIcon::ResizeWest,
        FloatingResizeDirection::NorthWest => CursorIcon::ResizeNorthWest,
    }
}

fn configure_title_accessibility(ui: &Ui, response: &egui::Response, rect: Rect, title: &str) {
    ui.ctx().accesskit_node_builder(response.id, |node| {
        node.set_role(Role::TitleBar);
        node.set_bounds(accesskit_bounds(rect));
        node.set_label(title);
        node.add_action(Action::Focus);
    });
}

fn configure_resize_accessibility(
    ui: &Ui,
    response: &egui::Response,
    rect: Rect,
    direction: FloatingResizeDirection,
) {
    ui.ctx().accesskit_node_builder(response.id, |node| {
        node.set_role(Role::Splitter);
        node.set_bounds(accesskit_bounds(rect));
        node.set_label(format!("Resize {direction:?}"));
        node.set_orientation(match direction {
            FloatingResizeDirection::North
            | FloatingResizeDirection::NorthEast
            | FloatingResizeDirection::SouthEast
            | FloatingResizeDirection::South
            | FloatingResizeDirection::SouthWest
            | FloatingResizeDirection::NorthWest => Orientation::Horizontal,
            FloatingResizeDirection::East | FloatingResizeDirection::West => Orientation::Vertical,
        });
        node.add_action(Action::Focus);
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn contained_transform_begins_at_press_origin() {
        let context = egui::Context::default();
        let press_origin = pos2(18.0, 22.0);
        let latest_position = pos2(41.0, 37.0);
        let screen_rect = Rect::from_min_size(pos2(0.0, 0.0), vec2(100.0, 100.0));
        let widget_rect = Rect::from_min_size(pos2(0.0, 0.0), vec2(100.0, 100.0));
        let widget_id = Id::new("contained-transform-test");
        let _ = context.run_ui(
            egui::RawInput {
                screen_rect: Some(screen_rect),
                ..Default::default()
            },
            |ui| {
                let _ = ui.interact(widget_rect, widget_id, Sense::drag());
            },
        );
        let mut input = egui::RawInput {
            screen_rect: Some(screen_rect),
            ..Default::default()
        };
        input.events.extend([
            egui::Event::PointerMoved(press_origin),
            egui::Event::PointerButton {
                pos: press_origin,
                button: PointerButton::Primary,
                pressed: true,
                modifiers: egui::Modifiers::NONE,
            },
            egui::Event::PointerMoved(latest_position),
        ]);
        let mut output = RenderOutput::default();

        let _ = context.run_ui(input, |ui| {
            let response = ui.interact(widget_rect, widget_id, Sense::drag());
            emit_transform_response(
                &response,
                widget_rect,
                TransformContext {
                    surface: SurfaceId::new(1),
                    root: RootId::new(2),
                    floating: FloatingPresentationId::new(3),
                    minimum_size: dockspace::geometry::LogicalSize::new(16.0, 12.0)
                        .expect("fixture minimum is valid"),
                    status: InteractionStatus::Idle,
                    active: None,
                },
                ContainedTransformKind::Move,
                &mut output,
            );
        });

        let [
            RenderAction::BeginContainedTransform {
                initial_pointer, ..
            },
        ] = output.actions.as_slice()
        else {
            panic!("expected one contained transform begin action");
        };
        assert_eq!(
            *initial_pointer,
            dockspace::geometry::LogicalPoint::new(
                f64::from(press_origin.x),
                f64::from(press_origin.y),
            )
            .expect("press origin is finite"),
        );
        assert_ne!(
            *initial_pointer,
            dockspace::geometry::LogicalPoint::new(
                f64::from(latest_position.x),
                f64::from(latest_position.y),
            )
            .expect("latest position is finite"),
        );
    }

    #[test]
    fn resize_zones_cover_all_directions_without_positive_overlap() {
        let rect = Rect::from_min_max(pos2(0.0, 0.0), pos2(120.0, 90.0));
        let zones = floating_resize_zones(rect, 7.0);

        assert_eq!(zones.len(), 8);
        for (index, (_, left)) in zones.iter().enumerate() {
            assert!(left.is_positive());
            assert!(rect.contains_rect(*left));
            for (_, right) in zones.iter().skip(index + 1) {
                assert!(!left.intersect(*right).is_positive());
            }
        }
    }

    #[test]
    fn resize_zone_shared_and_max_edges_have_at_most_one_half_open_owner() {
        let rect = Rect::from_min_max(pos2(0.0, 0.0), pos2(120.0, 90.0));
        let zones = floating_resize_zones(rect, 7.0);
        let mut coordinates = Vec::new();
        for (_, zone) in zones {
            coordinates.extend([zone.min, zone.max]);
        }

        for point in coordinates {
            let owners = zones
                .iter()
                .filter(|(_, zone)| contains_half_open(*zone, point))
                .count();
            assert!(owners <= 1, "point {point:?} has {owners} owners");
        }
        assert!(
            zones
                .iter()
                .all(|(_, zone)| !contains_half_open(*zone, rect.max))
        );
        assert!(
            zones
                .iter()
                .all(|(_, zone)| !contains_half_open(*zone, pos2(rect.max.x, rect.center().y)))
        );
    }

    #[test]
    fn each_resize_direction_has_a_directional_cursor() {
        for (direction, _) in
            floating_resize_zones(Rect::from_min_size(pos2(0.0, 0.0), vec2(50.0, 50.0)), 5.0)
        {
            assert_ne!(resize_cursor(direction), CursorIcon::Default);
        }
    }
}
