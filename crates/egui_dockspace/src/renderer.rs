//! Stateless egui painting and semantic interaction extraction.

use dockspace::command::{ItemSource, MovePayload, NodeSource};
use dockspace::error::CommandError;
use dockspace::geometry::{LogicalPoint, LogicalRect, LogicalSize};
use dockspace::graph::{ContainedStackKey, SplitWeight, Workspace};
use dockspace::ids::{FloatingPresentationId, RootId, SurfaceId};
use dockspace::intent::{
    Authority, AuthorityUnavailableReason, PointerButton, PointerButtonState, PointerId,
    SurfacePointer, TargetAuthority,
};
use dockspace::interaction::{
    ActiveContainedTransformView, ActiveDragView, ContainedTransformPaintAcknowledgement,
    ContainedTransformPreview, ContainedTransformSessionId, DragPhase, DragSessionId,
    InteractionCancelReason, InteractionPreview, InteractionState, InteractionStatus,
    PaintAcknowledgement, PreviewVisual, ResizeSessionId,
};
use egui::{
    Align2, FontSelection, Id, Key, Modifiers, PointerButton as EguiPointerButton, Pos2, Rect,
    Stroke, StrokeKind, TextStyle, Ui, pos2, vec2,
};

use crate::drop_guides;
use crate::floating;
use crate::hit::contains_half_open;
use crate::pane::PaneView;
use crate::projection::{RootPlan, SurfacePlan};
use crate::splits;
use crate::style::DockStyle;
use crate::tabs;

pub(crate) const PRIMARY_POINTER: PointerId = PointerId::new(1);
pub(crate) const PRIMARY_BUTTON: PointerButton = PointerButton::Primary;

/// A semantic action observed while painting one exact surface plan.
///
/// Actions contain frozen core references or absolute coordinates. The facade
/// owns multipass deduplication and submits them at a complete frame boundary.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum RenderAction {
    Select(ItemSource),
    CloseRequested(ItemSource),
    CloseRootRequested(NodeSource),
    ArmDrag(MovePayload),
    BeginDrag {
        session: DragSessionId,
        pointer: PointerId,
        button: PointerButton,
    },
    UpdateDrag {
        session: DragSessionId,
        target: TargetAuthority,
        pointer_position: Option<LogicalPoint>,
        contained_move: Option<ContainedMoveCandidate>,
    },
    ReleaseDrag {
        session: DragSessionId,
        pointer: PointerId,
        button: PointerButton,
        button_state: Authority<PointerButtonState>,
        target: TargetAuthority,
    },
    CancelDrag {
        session: DragSessionId,
        reason: InteractionCancelReason,
    },
    BeginResize {
        pointer: PointerId,
        button: PointerButton,
        split: NodeSource,
    },
    UpdateResize {
        session: ResizeSessionId,
        weights: Vec<SplitWeight>,
    },
    ReleaseResize {
        session: ResizeSessionId,
        pointer: PointerId,
        button: PointerButton,
        button_state: Authority<PointerButtonState>,
    },
    CancelResize {
        session: ResizeSessionId,
        reason: InteractionCancelReason,
    },
    AdjustResize {
        split: NodeSource,
        weights: Vec<SplitWeight>,
    },
    RaiseContained {
        surface: SurfaceId,
        root: RootId,
        floating: FloatingPresentationId,
        expected_z_order: u64,
        expected_frontmost: ContainedStackKey,
    },
    BeginContainedTransform {
        surface: SurfaceId,
        root: RootId,
        floating: FloatingPresentationId,
        pointer: PointerId,
        button: PointerButton,
        initial_pointer: LogicalPoint,
        kind: dockspace::intent::ContainedTransformKind,
        minimum_size: LogicalSize,
    },
    UpdateContainedTransform {
        session: ContainedTransformSessionId,
        current_pointer: LogicalPoint,
    },
    ReleaseContainedTransform {
        session: ContainedTransformSessionId,
        pointer: PointerId,
        button: PointerButton,
        button_state: Authority<PointerButtonState>,
    },
    CancelContainedTransform {
        session: ContainedTransformSessionId,
        reason: InteractionCancelReason,
    },
    AcknowledgePreview(PaintAcknowledgement),
    AcknowledgeContainedTransformPreview(ContainedTransformPaintAcknowledgement),
}

/// Frozen adapter facts for moving one existing contained presentation.
///
/// The facade validates these facts against the authoritative workspace before
/// asking the core to produce a scene-bound placement proof.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct ContainedMoveCandidate {
    pub(crate) surface: SurfaceId,
    pub(crate) root: RootId,
    pub(crate) floating: FloatingPresentationId,
    pub(crate) expected_rect: LogicalRect,
    pub(crate) requested_rect: LogicalRect,
    pub(crate) minimum_size: LogicalSize,
}

#[derive(Clone, Copy)]
struct GestureFrameFacts {
    escape_pressed: bool,
    primary_released: bool,
    lost_authority: Option<InteractionCancelReason>,
    interactions_current: bool,
}

/// Paint result kept separate from the docking engine mutation boundary.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct RenderOutput {
    pub(crate) actions: Vec<RenderAction>,
    pub(crate) capture_errors: Vec<CommandError>,
}

impl RenderOutput {
    pub(crate) fn push(&mut self, action: RenderAction) {
        if !self.actions.contains(&action) {
            self.actions.push(action);
        }
    }

    pub(crate) fn capture<T>(&mut self, result: Result<T, CommandError>) -> Option<T> {
        match result {
            Ok(value) => Some(value),
            Err(error) => {
                if !self.capture_errors.contains(&error) {
                    self.capture_errors.push(error);
                }
                None
            }
        }
    }
}

/// Paints one already validated surface plan and reports semantic input.
///
/// This function never mutates the workspace or the interaction state. When
/// `interactions_current` is false, stale chrome remains registered for layout
/// and accessibility, but geometry-derived response events and pane input are
/// ignored. Stable-ID keyboard/AccessKit actions remain valid. Global Escape
/// and matching release edges still terminate an active core session.
#[allow(clippy::too_many_arguments)]
pub(crate) fn paint_surface(
    ui: &mut Ui,
    instance_id: Id,
    plan: &SurfacePlan,
    workspace: &Workspace,
    panes: &mut dyn PaneView,
    style: &DockStyle,
    interaction: &InteractionState,
    interactions_current: bool,
) -> RenderOutput {
    let mut output = RenderOutput::default();
    let escape_pressed = interaction.status() != InteractionStatus::Idle
        && ui.input_mut(|input| input.consume_key(Modifiers::NONE, Key::Escape));
    let accept_events = interactions_current && !escape_pressed;
    let focused_floating = pointer_focused_floating(ui, plan, accept_events);

    ui.painter()
        .rect_filled(plan.bounds, 0.0, style.workspace_fill);

    for root in &plan.roots {
        paint_root(
            ui,
            instance_id,
            plan.surface,
            root,
            workspace,
            panes,
            style,
            interaction,
            interaction.active_contained_transform_view(),
            focused_floating,
            accept_events,
            &mut output,
        );
    }

    paint_preview(
        ui,
        plan,
        interaction.preview(),
        style,
        interactions_current,
        &mut output,
    );
    drop_guides::paint(
        &ui.painter_at(plan.bounds),
        plan.surface,
        interaction.drop_affordance(),
        style,
    );
    paint_contained_transform_preview(
        ui,
        plan,
        interaction.contained_transform_preview(),
        style,
        interactions_current,
        &mut output,
    );
    paint_drag_ghost(ui, plan, workspace, interaction.active_drag_view(), style);
    extract_global_gesture_actions(
        ui,
        plan,
        workspace,
        interaction,
        escape_pressed,
        accept_events,
        &mut output,
    );
    discard_drag_arm_without_press_edge(ui, interaction.status(), &mut output);

    output
}

#[allow(clippy::too_many_arguments)]
fn paint_root(
    ui: &mut Ui,
    instance_id: Id,
    surface: SurfaceId,
    root: &RootPlan,
    workspace: &Workspace,
    panes: &mut dyn PaneView,
    style: &DockStyle,
    interaction: &InteractionState,
    active_contained_transform: Option<ActiveContainedTransformView<'_>>,
    focused_floating: Option<FloatingPresentationId>,
    interactions_current: bool,
    output: &mut RenderOutput,
) {
    if let Some(floating) = root.floating.as_ref() {
        floating::paint_background(ui, instance_id, surface, root.root, floating, style);
        if focused_floating == Some(floating.id) && interactions_current {
            floating::request_raise(surface, root.root, floating, workspace, output);
        }
    } else {
        ui.painter()
            .rect_filled(root.bounds, 0.0, style.workspace_fill);
    }

    for tabs_plan in &root.tabs {
        tabs::paint_tabs(
            ui,
            instance_id,
            surface,
            tabs_plan,
            workspace,
            panes,
            style,
            interaction.status(),
            interaction.active_drag_view(),
            interactions_current,
            output,
        );
    }

    for splitter in &root.splitters {
        splits::paint_splitter(
            ui,
            instance_id,
            surface,
            splitter,
            workspace,
            style,
            interaction.status(),
            interactions_current,
            output,
        );
    }

    if let Some(floating) = root.floating.as_ref() {
        floating::paint_chrome_and_interact(
            ui,
            instance_id,
            surface,
            root,
            floating,
            workspace,
            style,
            interaction.status(),
            interaction.active_drag_view(),
            active_contained_transform,
            interactions_current,
            output,
        );
    }
}

fn pointer_focused_floating(
    ui: &Ui,
    plan: &SurfacePlan,
    interactions_current: bool,
) -> Option<FloatingPresentationId> {
    if !interactions_current
        || !ui.input(|input| input.pointer.button_pressed(EguiPointerButton::Primary))
    {
        return None;
    }
    let pointer = ui.input(|input| input.pointer.interact_pos())?;
    plan.roots
        .iter()
        .filter_map(|root| root.floating.as_ref())
        .rfind(|floating| contains_half_open(floating.outer_rect, pointer))
        .map(|floating| floating.id)
}

fn paint_preview(
    ui: &Ui,
    plan: &SurfacePlan,
    preview: Option<&InteractionPreview>,
    style: &DockStyle,
    acknowledge: bool,
    output: &mut RenderOutput,
) {
    let Some(preview) = preview else {
        return;
    };
    let visual_rect = match preview.visual() {
        PreviewVisual::Dock { surface, rect, .. }
        | PreviewVisual::Contained { surface, rect, .. }
            if *surface == plan.surface =>
        {
            from_logical_rect(*rect)
        }
        PreviewVisual::Dock { .. }
        | PreviewVisual::Contained { .. }
        | PreviewVisual::Native { .. } => None,
    };
    let Some(rect) = visual_rect else {
        return;
    };

    let canvas = ui.painter_at(plan.bounds);
    canvas.rect(
        rect,
        0.0,
        style.drop_fill,
        Stroke::new(1.0, style.drop_border_color),
        StrokeKind::Inside,
    );
    if acknowledge {
        output.push(RenderAction::AcknowledgePreview(preview.acknowledgement()));
    }
}

fn paint_contained_transform_preview(
    ui: &Ui,
    plan: &SurfacePlan,
    preview: Option<&ContainedTransformPreview>,
    style: &DockStyle,
    acknowledge: bool,
    output: &mut RenderOutput,
) {
    let Some(preview) = preview.filter(|preview| preview.surface() == plan.surface) else {
        return;
    };
    let Some(rect) = from_logical_rect(preview.rect()) else {
        return;
    };
    ui.painter_at(plan.bounds).rect(
        rect,
        0.0,
        style.ghost_fill,
        Stroke::new(1.0, style.ghost_border_color),
        StrokeKind::Inside,
    );
    if acknowledge {
        output.push(RenderAction::AcknowledgeContainedTransformPreview(
            preview.acknowledgement(),
        ));
    }
}

fn paint_drag_ghost(
    ui: &Ui,
    plan: &SurfacePlan,
    workspace: &Workspace,
    active: Option<ActiveDragView<'_>>,
    style: &DockStyle,
) {
    let Some(active) = active.filter(|drag| drag.phase() == DragPhase::Dragging) else {
        return;
    };
    if is_complete_contained_root_drag(workspace, active.payload()) {
        return;
    }
    let Some(pointer) = ui.input(|input| input.pointer.interact_pos()) else {
        return;
    };
    let title = drag_title(plan, active.payload());
    let canvas = ui.painter_at(plan.bounds);
    let font_id = TextStyle::Button.resolve(ui.style());
    let galley = canvas.layout_no_wrap(title, font_id, style.tab_active_text_color);
    let size = vec2(
        (galley.size().x + 2.0 * style.tab_horizontal_padding)
            .clamp(style.tab_min_width, style.tab_max_width),
        style.tab_bar_height,
    );
    let rect = Rect::from_min_size(pointer + style.ghost_offset, size);
    canvas.rect(
        rect,
        0.0,
        style.ghost_fill,
        Stroke::new(1.0, style.ghost_border_color),
        StrokeKind::Inside,
    );
    canvas.galley(
        pos2(
            rect.min.x + style.tab_horizontal_padding,
            rect.center().y - galley.size().y * 0.5,
        ),
        galley,
        style.tab_active_text_color,
    );
}

fn is_complete_contained_root_drag(workspace: &Workspace, payload: &MovePayload) -> bool {
    let MovePayload::Subtree(source) = payload else {
        return false;
    };
    workspace
        .root(source.root())
        .is_some_and(|record| record.node == source.node())
        && matches!(
            workspace.presentation_for_root(source.root()),
            Some(dockspace::RootPresentationOwner::Contained { .. })
        )
}

fn drag_title(plan: &SurfacePlan, payload: &MovePayload) -> String {
    let (root, node, item) = match payload {
        MovePayload::Item(source) => (source.root(), source.tabs(), Some(source.item())),
        MovePayload::Tabs(source) | MovePayload::Subtree(source) => {
            (source.root(), source.node(), None)
        }
    };
    let matching_root = plan.roots.iter().find(|candidate| candidate.root == root);
    if let Some(item) = item {
        return matching_root
            .and_then(|root| root.tabs.iter().find(|tabs| tabs.node == node))
            .and_then(|tabs| tabs.tabs.iter().find(|tab| tab.item == item))
            .map_or_else(|| format!("Pane {}", item.get()), |tab| tab.title.clone());
    }
    matching_root
        .and_then(|root| {
            root.tabs
                .iter()
                .find(|tabs| tabs.node == node)
                .or_else(|| root.tabs.iter().find(|tabs| tabs.selected.is_some()))
        })
        .and_then(|tabs| tabs.tabs.iter().find(|tab| tab.selected))
        .map_or_else(|| "Dock group".to_owned(), |tab| tab.title.clone())
}

fn extract_global_gesture_actions(
    ui: &Ui,
    plan: &SurfacePlan,
    workspace: &Workspace,
    interaction: &InteractionState,
    escape_pressed: bool,
    interactions_current: bool,
    output: &mut RenderOutput,
) {
    let facts = GestureFrameFacts {
        escape_pressed,
        primary_released: primary_released(ui),
        lost_authority: lost_primary_gesture_authority(ui),
        interactions_current,
    };
    if interaction.status() != InteractionStatus::Idle
        && !facts.escape_pressed
        && !facts.primary_released
        && facts.lost_authority.is_some()
    {
        ui.ctx().stop_dragging();
    }
    match interaction.status() {
        InteractionStatus::Armed { session } => {
            if facts.escape_pressed {
                output.push(RenderAction::CancelDrag {
                    session,
                    reason: InteractionCancelReason::Escape,
                });
            } else if facts.primary_released {
                output.push(RenderAction::CancelDrag {
                    session,
                    reason: InteractionCancelReason::ReleasedBeforeDrag,
                });
            } else if let Some(reason) = facts.lost_authority {
                output.push(RenderAction::CancelDrag { session, reason });
            }
        }
        InteractionStatus::Dragging { session } => {
            extract_dragging_actions(ui, plan, workspace, interaction, session, facts, output);
        }
        InteractionStatus::Resizing { session } => {
            if facts.escape_pressed {
                output.push(RenderAction::CancelResize {
                    session,
                    reason: InteractionCancelReason::Escape,
                });
            } else if facts.primary_released {
                output.push(RenderAction::ReleaseResize {
                    session,
                    pointer: PRIMARY_POINTER,
                    button: PRIMARY_BUTTON,
                    button_state: Authority::Known(PointerButtonState::Released),
                });
            } else if let Some(reason) = facts.lost_authority {
                output.push(RenderAction::CancelResize { session, reason });
            }
        }
        InteractionStatus::ContainedTransforming { session } => {
            if facts.escape_pressed {
                output.push(RenderAction::CancelContainedTransform {
                    session,
                    reason: InteractionCancelReason::Escape,
                });
            } else if facts.primary_released
                && interaction
                    .active_contained_transform_view()
                    .is_some_and(|transform| {
                        transform.pointer() == PRIMARY_POINTER
                            && transform.button() == PRIMARY_BUTTON
                    })
            {
                output.push(RenderAction::ReleaseContainedTransform {
                    session,
                    pointer: PRIMARY_POINTER,
                    button: PRIMARY_BUTTON,
                    button_state: Authority::Known(PointerButtonState::Released),
                });
            } else if let Some(reason) = facts.lost_authority {
                output.push(RenderAction::CancelContainedTransform { session, reason });
            }
        }
        InteractionStatus::Idle => {}
    }
}

fn extract_dragging_actions(
    ui: &Ui,
    plan: &SurfacePlan,
    workspace: &Workspace,
    interaction: &InteractionState,
    session: DragSessionId,
    facts: GestureFrameFacts,
    output: &mut RenderOutput,
) {
    if facts.escape_pressed {
        output.push(RenderAction::CancelDrag {
            session,
            reason: InteractionCancelReason::Escape,
        });
    } else if facts.primary_released {
        let (target, _) = if facts.interactions_current {
            local_target(ui, plan)
        } else {
            (unreported_local_target(plan), None)
        };
        output.push(RenderAction::ReleaseDrag {
            session,
            pointer: PRIMARY_POINTER,
            button: PRIMARY_BUTTON,
            button_state: Authority::Known(PointerButtonState::Released),
            target,
        });
    } else if let Some(reason) = facts.lost_authority {
        output.push(RenderAction::CancelDrag { session, reason });
    } else if facts.interactions_current {
        let (target, pointer_position) = local_target(ui, plan);
        let contained_move = interaction
            .active_drag_view()
            .and_then(|active| contained_move_candidate(ui, plan, workspace, active));
        output.push(RenderAction::UpdateDrag {
            session,
            target,
            pointer_position,
            contained_move,
        });
    }
}

fn contained_move_candidate(
    ui: &Ui,
    plan: &SurfacePlan,
    workspace: &Workspace,
    active: ActiveDragView<'_>,
) -> Option<ContainedMoveCandidate> {
    let MovePayload::Subtree(source) = active.payload() else {
        return None;
    };
    let root_record = workspace.root(source.root())?;
    if root_record.node != source.node() {
        return None;
    }
    let dockspace::RootPresentationOwner::Contained { surface, floating } =
        workspace.presentation_for_root(source.root())?
    else {
        return None;
    };
    if surface != plan.surface {
        return None;
    }
    let record = workspace.contained_floating(floating)?;
    if record.root != source.root() || record.surface != surface {
        return None;
    }
    let floating_plan = plan
        .roots
        .iter()
        .find(|root| root.root == source.root())?
        .floating
        .as_ref()
        .filter(|candidate| candidate.id == floating)?;
    let (initial, current) =
        ui.input(|input| (input.pointer.press_origin(), input.pointer.interact_pos()));
    let initial = initial?;
    let current = current?;
    let requested_min = LogicalPoint::new(
        record.rect.min().x() + f64::from(current.x) - f64::from(initial.x),
        record.rect.min().y() + f64::from(current.y) - f64::from(initial.y),
    )
    .ok()?;
    let requested_rect = LogicalRect::from_min_size(requested_min, record.rect.size()).ok()?;
    Some(ContainedMoveCandidate {
        surface,
        root: source.root(),
        floating,
        expected_rect: record.rect,
        requested_rect,
        minimum_size: floating_plan.minimum_size,
    })
}

fn discard_drag_arm_without_press_edge(
    ui: &Ui,
    status: InteractionStatus,
    output: &mut RenderOutput,
) {
    let held_without_press = ui.input(|input| {
        input.pointer.button_down(EguiPointerButton::Primary)
            && !input.pointer.button_pressed(EguiPointerButton::Primary)
    });
    if status == InteractionStatus::Idle && held_without_press {
        output
            .actions
            .retain(|action| !matches!(action, RenderAction::ArmDrag(_)));
    }
}

fn lost_primary_gesture_authority(ui: &Ui) -> Option<InteractionCancelReason> {
    ui.input(|input| {
        if !input.focused {
            return Some(InteractionCancelReason::FocusLost);
        }

        // egui 0.35 deliberately retains interact_pos and button state for PointerGone.
        let pointer_gone = input
            .events
            .iter()
            .any(|event| matches!(event, egui::Event::PointerGone));
        let release_reported = input.pointer.button_released(EguiPointerButton::Primary);
        let button_down = input.pointer.button_down(EguiPointerButton::Primary);
        (pointer_gone || !release_reported && !button_down)
            .then_some(InteractionCancelReason::CaptureLost)
    })
}

fn unreported_local_target(plan: &SurfacePlan) -> TargetAuthority {
    TargetAuthority::local(
        plan.surface,
        Authority::Unknown(AuthorityUnavailableReason::NotReported),
    )
}

fn primary_released(ui: &Ui) -> bool {
    ui.input(|input| input.pointer.button_released(EguiPointerButton::Primary))
}

fn local_target(ui: &Ui, plan: &SurfacePlan) -> (TargetAuthority, Option<LogicalPoint>) {
    let Some(position) = ui.input(|input| input.pointer.interact_pos()) else {
        return (
            TargetAuthority::local(
                plan.surface,
                Authority::Unknown(AuthorityUnavailableReason::NotReported),
            ),
            None,
        );
    };
    let Ok(logical) = to_logical_point(position) else {
        return (
            TargetAuthority::local(
                plan.surface,
                Authority::Unknown(AuthorityUnavailableReason::CoordinateUnavailable),
            ),
            None,
        );
    };
    let inside = position.x >= plan.bounds.min.x
        && position.x < plan.bounds.max.x
        && position.y >= plan.bounds.min.y
        && position.y < plan.bounds.max.y;
    let viewport = ui.input(egui::InputState::viewport_rect);
    let inside_viewport = position.x >= viewport.min.x
        && position.x < viewport.max.x
        && position.y >= viewport.min.y
        && position.y < viewport.max.y;
    if !inside_viewport {
        return (
            TargetAuthority::local(
                plan.surface,
                Authority::Unknown(AuthorityUnavailableReason::SurfaceUnavailable),
            ),
            None,
        );
    }
    let target = inside.then(|| SurfacePointer::new(plan.surface, logical));
    (
        TargetAuthority::local(plan.surface, Authority::Known(target)),
        Some(logical),
    )
}

pub(crate) fn to_logical_point(
    point: Pos2,
) -> Result<LogicalPoint, dockspace::geometry::GeometryError> {
    LogicalPoint::new(f64::from(point.x), f64::from(point.y))
}

pub(crate) fn from_logical_rect(rect: LogicalRect) -> Option<Rect> {
    let values = [
        rect.min().x(),
        rect.min().y(),
        rect.max().x(),
        rect.max().y(),
    ];
    if values
        .iter()
        .any(|value| !value.is_finite() || value.abs() > f64::from(f32::MAX))
    {
        return None;
    }
    #[allow(
        clippy::cast_possible_truncation,
        reason = "validated egui coordinates intentionally use f32 points"
    )]
    Some(Rect::from_min_max(
        pos2(values[0] as f32, values[1] as f32),
        pos2(values[2] as f32, values[3] as f32),
    ))
}

pub(crate) fn accesskit_bounds(rect: Rect) -> egui::accesskit::Rect {
    egui::accesskit::Rect {
        x0: f64::from(rect.min.x),
        y0: f64::from(rect.min.y),
        x1: f64::from(rect.max.x),
        y1: f64::from(rect.max.y),
    }
}

pub(crate) fn paint_centered_label(ui: &Ui, rect: Rect, text: impl ToString, color: egui::Color32) {
    ui.painter_at(rect).text(
        rect.center(),
        Align2::CENTER_CENTER,
        text,
        FontSelection::Style(TextStyle::Body).resolve(ui.style()),
        color,
    );
}
