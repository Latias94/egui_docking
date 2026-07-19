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
struct ResizeContext<'a> {
    surface: SurfaceId,
    root: RootId,
    floating: FloatingPresentationId,
    minimum_size: dockspace::geometry::LogicalSize,
    status: InteractionStatus,
    active: Option<ActiveContainedTransformView<'a>>,
}

#[derive(Clone, Copy)]
struct TitleDragContext<'a> {
    root: RootId,
    workspace: &'a Workspace,
    payload_available: bool,
    status: InteractionStatus,
    active: Option<ActiveDragView<'a>>,
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
    let title_drag_available = root.tabs.iter().any(|tabs| !tabs.tabs.is_empty());
    let resize = ResizeContext {
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

    if interactions_current && title_drag_available {
        if title_response.hovered() || title_response.dragged() {
            ui.ctx().set_cursor_icon(if title_response.dragged() {
                CursorIcon::Grabbing
            } else {
                CursorIcon::Grab
            });
        }
        emit_title_drag_response(
            &title_response,
            title_drag_rect,
            TitleDragContext {
                root: root.root,
                workspace,
                payload_available: title_drag_available,
                status,
                active: active_drag,
            },
            output,
        );
    }

    paint_resize_handles(
        ui,
        instance_id,
        floating,
        resize,
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
    resize: ResizeContext<'_>,
    interactions_current: bool,
    output: &mut RenderOutput,
) {
    for &(direction, rect) in &floating.resize_zones {
        let id = ui.make_persistent_id((
            "egui_dockspace",
            instance_id,
            resize.surface,
            resize.root,
            resize.floating,
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
        emit_resize_response(&response, rect, resize, resize_edges(direction), output);
    }
}

fn emit_title_drag_response(
    response: &egui::Response,
    region: Rect,
    drag: TitleDragContext<'_>,
    output: &mut RenderOutput,
) {
    if !drag.payload_available {
        return;
    }
    if drag.status == InteractionStatus::Idle
        && response.is_pointer_button_down_on()
        && response
            .ctx
            .input(|input| input.pointer.button_down(PointerButton::Primary))
        && response
            .ctx
            .input(|input| input.pointer.press_origin())
            .is_some_and(|origin| contains_half_open(region, origin))
        && let Some(root_record) = drag.workspace.root(drag.root)
        && let Some(source) = output.capture(
            drag.workspace
                .capture_node_source(drag.root, root_record.node),
        )
    {
        output.push(RenderAction::ArmDrag(MovePayload::Subtree(source)));
    }
    if (response.drag_started_by(PointerButton::Primary)
        || response.dragged_by(PointerButton::Primary))
        && response
            .ctx
            .input(|input| input.pointer.press_origin())
            .is_some_and(|origin| contains_half_open(region, origin))
        && let Some(active_drag) = drag
            .active
            .filter(|active| active.phase() == DragPhase::Armed)
        && let MovePayload::Subtree(source) = active_drag.payload()
        && source.root() == drag.root
        && drag
            .workspace
            .root(drag.root)
            .is_some_and(|record| source.node() == record.node)
    {
        output.push(RenderAction::BeginDrag {
            session: active_drag.session(),
            pointer: PRIMARY_POINTER,
            button: PRIMARY_BUTTON,
        });
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

fn emit_resize_response(
    response: &egui::Response,
    region: Rect,
    resize: ResizeContext<'_>,
    edges: ContainedResizeEdges,
    output: &mut RenderOutput,
) {
    if (response.drag_started_by(PointerButton::Primary)
        || response.dragged_by(PointerButton::Primary))
        && resize.status == InteractionStatus::Idle
        && let Some(initial_position) = response.ctx.input(|input| input.pointer.press_origin())
        && contains_half_open(region, initial_position)
        && let Ok(initial_pointer) = to_logical_point(initial_position)
    {
        output.push(RenderAction::BeginContainedTransform {
            surface: resize.surface,
            root: resize.root,
            floating: resize.floating,
            pointer: PRIMARY_POINTER,
            button: PRIMARY_BUTTON,
            initial_pointer,
            kind: ContainedTransformKind::Resize(edges),
            minimum_size: resize.minimum_size,
        });
    } else if response.dragged_by(PointerButton::Primary)
        && let Some(current_pointer) = response
            .interact_pointer_pos()
            .and_then(|position| to_logical_point(position).ok())
        && let Some(active) = resize.active.filter(|active| {
            active.surface() == resize.surface
                && active.root() == resize.root
                && active.floating() == resize.floating
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
    use dockspace::geometry::LogicalRect;
    use dockspace::graph::{
        Axis, ContainedFloating, Node, RootRecord, SurfacePresentation, Workspace,
    };
    use dockspace::ids::ItemId;

    const SURFACE: SurfaceId = SurfaceId::new(1);
    const MAIN_ROOT: RootId = RootId::new(2);
    const FLOATING_ROOT: RootId = RootId::new(3);
    const FLOATING: FloatingPresentationId = FloatingPresentationId::new(4);

    fn contained_split_workspace() -> (Workspace, dockspace::ids::NodeId) {
        let mut builder = Workspace::builder();
        let main = builder.insert_node(Node::tabs([ItemId::new(10)]));
        let left = builder.insert_node(Node::tabs([ItemId::new(11)]));
        let right = builder.insert_node(Node::tabs([ItemId::new(12)]));
        let floating_root = builder.insert_node(
            Node::equal_split(Axis::Horizontal, [left, right]).expect("two children form a split"),
        );
        builder.set_root(MAIN_ROOT, RootRecord::new(main));
        builder.set_root(FLOATING_ROOT, RootRecord::new(floating_root));
        builder.set_surface(SURFACE, SurfacePresentation::new(MAIN_ROOT));
        builder.set_contained_floating(ContainedFloating::new(
            FLOATING,
            FLOATING_ROOT,
            SURFACE,
            LogicalRect::new(0.0, 0.0, 100.0, 100.0).expect("fixture rect is valid"),
            1,
        ));
        builder
            .attach_contained(SURFACE, FLOATING)
            .expect("surface exists");
        (
            builder.build().expect("contained fixture is valid"),
            floating_root,
        )
    }

    #[test]
    fn complete_contained_title_arms_the_complete_subtree_from_its_full_width() {
        let (workspace, floating_root) = contained_split_workspace();
        let context = egui::Context::default();
        let title_rect = Rect::from_min_size(pos2(0.0, 0.0), vec2(100.0, 28.0));
        let press = pos2(92.0, title_rect.center().y);
        let screen_rect = Rect::from_min_size(pos2(0.0, 0.0), vec2(120.0, 80.0));
        let widget_id = Id::new("contained-title-subtree-test");
        let _ = context.run_ui(
            egui::RawInput {
                screen_rect: Some(screen_rect),
                ..Default::default()
            },
            |ui| {
                let _ = ui.interact(title_rect, widget_id, Sense::drag());
            },
        );
        let mut input = egui::RawInput {
            screen_rect: Some(screen_rect),
            ..Default::default()
        };
        input.events.extend([
            egui::Event::PointerMoved(press),
            egui::Event::PointerButton {
                pos: press,
                button: PointerButton::Primary,
                pressed: true,
                modifiers: egui::Modifiers::NONE,
            },
        ]);
        let mut output = RenderOutput::default();
        let mut unavailable_output = RenderOutput::default();

        let _ = context.run_ui(input, |ui| {
            let response = ui.interact(title_rect, widget_id, Sense::drag());
            emit_title_drag_response(
                &response,
                title_rect,
                TitleDragContext {
                    root: FLOATING_ROOT,
                    workspace: &workspace,
                    payload_available: false,
                    status: InteractionStatus::Idle,
                    active: None,
                },
                &mut unavailable_output,
            );
            emit_title_drag_response(
                &response,
                title_rect,
                TitleDragContext {
                    root: FLOATING_ROOT,
                    workspace: &workspace,
                    payload_available: true,
                    status: InteractionStatus::Idle,
                    active: None,
                },
                &mut output,
            );
        });

        assert!(unavailable_output.actions.is_empty());
        let [RenderAction::ArmDrag(MovePayload::Subtree(source))] = output.actions.as_slice()
        else {
            panic!("title press must arm exactly one subtree drag");
        };
        assert_eq!(source.root(), FLOATING_ROOT);
        assert_eq!(source.node(), floating_root);
    }

    #[test]
    fn contained_resize_begins_at_press_origin() {
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
            emit_resize_response(
                &response,
                widget_rect,
                ResizeContext {
                    surface: SurfaceId::new(1),
                    root: RootId::new(2),
                    floating: FloatingPresentationId::new(3),
                    minimum_size: dockspace::geometry::LogicalSize::new(16.0, 12.0)
                        .expect("fixture minimum is valid"),
                    status: InteractionStatus::Idle,
                    active: None,
                },
                ContainedResizeEdges::horizontal(ContainedHorizontalResizeEdge::Right),
                &mut output,
            );
        });

        let [
            RenderAction::BeginContainedTransform {
                initial_pointer,
                kind: ContainedTransformKind::Resize(_),
                ..
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
