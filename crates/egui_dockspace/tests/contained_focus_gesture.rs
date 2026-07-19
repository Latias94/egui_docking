use dockspace::command::CommandOutcome;
use dockspace::geometry::{LogicalPoint, LogicalRect};
use dockspace::graph::{ContainedFloating, Node, RootRecord, SurfacePresentation, Workspace};
use dockspace::ids::{FloatingPresentationId, ItemId, RootId, SurfaceId};
use dockspace::intent::{
    ContainedHorizontalResizeEdge, ContainedResizeEdges, ContainedTransformKind,
};
use dockspace::interaction::{InteractionOutcome, InteractionStatus};
use dockspace::transition::InputOutcome;
use egui::{Context, Event, Modifiers, PointerButton, Pos2, RawInput, Rect, Ui, vec2};
use egui_dockspace::{Dockspace, PaneView};

const SURFACE: SurfaceId = SurfaceId::new(1);
const MAIN_ROOT: RootId = RootId::new(10);
const REAR_ROOT: RootId = RootId::new(11);
const FRONT_ROOT: RootId = RootId::new(12);
const REAR_FLOATING: FloatingPresentationId = FloatingPresentationId::new(20);
const FRONT_FLOATING: FloatingPresentationId = FloatingPresentationId::new(21);
const MAIN_ITEM: ItemId = ItemId::new(100);
const REAR_ITEM: ItemId = ItemId::new(101);
const FRONT_ITEM: ItemId = ItemId::new(102);
const REAR_Z: u64 = 10;
const FRONT_Z: u64 = 20;

struct TestPanes;

impl PaneView for TestPanes {
    fn title(&self, item: ItemId) -> Option<egui::WidgetText> {
        Some(format!("Pane {}", item.get()).into())
    }

    fn ui(&mut self, _item: ItemId, _ui: &mut Ui) {}
}

#[derive(Debug)]
struct FrameObservation {
    interactions_current: bool,
    saw_stale_pass: bool,
    status: InteractionStatus,
    rect: LogicalRect,
    z_order: u64,
    preview: Option<LogicalRect>,
    raised: Option<(u64, u64)>,
    transform_events: TransformEvents,
}

#[derive(Clone, Copy, Debug, Default)]
struct TransformEvents {
    began: usize,
    preview_acknowledged: bool,
    delivered: bool,
}

#[derive(Clone, Copy)]
enum Gesture {
    Move,
    ResizeWest,
}

fn overlapping_workspace(rear_rect: LogicalRect) -> Workspace {
    let front_rect = LogicalRect::new(250.0, 130.0, 240.0, 180.0).expect("finite fixture");
    let mut builder = Workspace::builder();
    let main = builder.insert_node(Node::tabs([MAIN_ITEM]));
    let rear = builder.insert_node(Node::tabs([REAR_ITEM]));
    let front = builder.insert_node(Node::tabs([FRONT_ITEM]));
    builder.set_root(MAIN_ROOT, RootRecord::new(main));
    builder.set_root(REAR_ROOT, RootRecord::new(rear));
    builder.set_root(FRONT_ROOT, RootRecord::new(front));
    builder.set_surface(SURFACE, SurfacePresentation::new(MAIN_ROOT));
    builder.set_contained_floating(ContainedFloating::new(
        REAR_FLOATING,
        REAR_ROOT,
        SURFACE,
        rear_rect,
        REAR_Z,
    ));
    builder.set_contained_floating(ContainedFloating::new(
        FRONT_FLOATING,
        FRONT_ROOT,
        SURFACE,
        front_rect,
        FRONT_Z,
    ));
    builder
        .attach_contained(SURFACE, REAR_FLOATING)
        .expect("surface exists");
    builder
        .attach_contained(SURFACE, FRONT_FLOATING)
        .expect("surface exists");
    builder.build().expect("overlapping fixture must build")
}

fn pointer_button(position: Pos2, pressed: bool) -> Event {
    Event::PointerButton {
        pos: position,
        button: PointerButton::Primary,
        pressed,
        modifiers: Modifiers::NONE,
    }
}

fn run_frame(
    context: &Context,
    dockspace: &mut Dockspace,
    panes: &mut TestPanes,
    events: Vec<Event>,
) -> FrameObservation {
    let input = RawInput {
        screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(640.0, 480.0))),
        events,
        ..RawInput::default()
    };
    let mut observation = None;
    let mut saw_stale_pass = false;
    let mut raised_across_passes = None;
    let mut transform_events = TransformEvents::default();
    let _ = context.run_ui(input, |ui| {
        let response = dockspace
            .show(SURFACE, ui, panes)
            .expect("egui frame must advance");
        let mut raised = None;
        for outcome in response
            .transitions()
            .iter()
            .flat_map(dockspace::transition::EngineTransition::reduced_inputs)
            .map(dockspace::transition::ReducedInput::outcome)
        {
            match outcome {
                InputOutcome::CommandProcessed {
                    outcome:
                        CommandOutcome::ContainedRaised {
                            floating,
                            previous,
                            current,
                            changed: true,
                        },
                    ..
                } if *floating == REAR_FLOATING => raised = Some((*previous, *current)),
                InputOutcome::InteractionProcessed {
                    outcome: InteractionOutcome::ContainedTransformBegan { .. },
                    ..
                } => transform_events.began += 1,
                InputOutcome::InteractionProcessed {
                    outcome:
                        InteractionOutcome::ContainedTransformPreviewAcknowledged {
                            changed: true, ..
                        },
                    ..
                } => transform_events.preview_acknowledged = true,
                InputOutcome::InteractionProcessed {
                    outcome: InteractionOutcome::ContainedTransformDelivered { .. },
                    ..
                } => transform_events.delivered = true,
                _ => {}
            }
        }
        saw_stale_pass |= !response.interactions_current();
        raised_across_passes = raised_across_passes.or(raised);
        let floating = dockspace
            .engine()
            .workspace()
            .contained_floating(REAR_FLOATING)
            .expect("rear floating remains presented");
        observation = Some(FrameObservation {
            interactions_current: response.interactions_current(),
            saw_stale_pass,
            status: dockspace.engine().interaction().status(),
            rect: floating.rect,
            z_order: floating.z_order,
            preview: dockspace
                .engine()
                .interaction()
                .contained_transform_preview()
                .map(|preview| preview.rect()),
            raised: raised_across_passes,
            transform_events,
        });
    });
    observation.expect("one egui pass must paint")
}

fn warm(context: &Context, dockspace: &mut Dockspace, panes: &mut TestPanes) {
    let first = run_frame(context, dockspace, panes, Vec::new());
    assert!(first.saw_stale_pass);
    assert!(first.interactions_current);
    assert!(run_frame(context, dockspace, panes, Vec::new()).interactions_current);
}

#[allow(
    clippy::cast_possible_truncation,
    reason = "finite fixture geometry is deliberately converted into egui's f32 input space"
)]
fn gesture_points(dockspace: &Dockspace, gesture: Gesture) -> (Pos2, Pos2) {
    let rear = dockspace
        .engine()
        .workspace()
        .contained_floating(REAR_FLOATING)
        .expect("rear floating exists");
    match gesture {
        Gesture::Move => {
            let style = dockspace.style();
            let title_inset = style
                .floating_resize_extent
                .min(style.floating_title_height * 0.5);
            let dock_grip_width = style.floating_title_height - 2.0 * title_inset;
            let press = Pos2::new(
                rear.rect.min().x() as f32
                    + style.floating_border_width
                    + style.floating_resize_extent
                    + dock_grip_width
                    + style.tab_horizontal_padding,
                rear.rect.min().y() as f32
                    + style.floating_border_width
                    + style.floating_title_height * 0.5,
            );
            (press, press + vec2(45.0, 35.0))
        }
        Gesture::ResizeWest => {
            let press = Pos2::new(
                rear.rect.min().x() as f32 + dockspace.style().floating_resize_extent * 0.5,
                ((rear.rect.min().y() + rear.rect.max().y()) * 0.5) as f32,
            );
            (press, press + vec2(-30.0, 0.0))
        }
    }
}

fn expected_rect(
    original: LogicalRect,
    gesture: Gesture,
    press: Pos2,
    current: Pos2,
) -> LogicalRect {
    let delta_x = f64::from(current.x - press.x);
    let delta_y = f64::from(current.y - press.y);
    match gesture {
        Gesture::Move => LogicalRect::new(
            original.x() + delta_x,
            original.y() + delta_y,
            original.width(),
            original.height(),
        ),
        Gesture::ResizeWest => LogicalRect::new(
            original.x() + delta_x,
            original.y(),
            original.width() - delta_x,
            original.height(),
        ),
    }
    .expect("expected transform stays finite and positive")
}

fn raise_rear_and_recover_begin(
    context: &Context,
    dockspace: &mut Dockspace,
    panes: &mut TestPanes,
    original: LogicalRect,
    press: Pos2,
    current: Pos2,
) -> FrameObservation {
    let pressed = run_frame(
        context,
        dockspace,
        panes,
        vec![Event::PointerMoved(press), pointer_button(press, true)],
    );
    assert_eq!(pressed.status, InteractionStatus::Idle);
    assert_eq!(pressed.rect, original);
    assert_eq!(pressed.z_order, REAR_Z);

    let stale_move = run_frame(
        context,
        dockspace,
        panes,
        vec![Event::PointerMoved(current)],
    );
    assert!(stale_move.saw_stale_pass);
    assert!(stale_move.interactions_current);
    assert_eq!(stale_move.raised, Some((REAR_Z, FRONT_Z + 1)));
    assert_eq!(stale_move.status, InteractionStatus::Idle);
    assert_eq!(stale_move.rect, original);
    assert_eq!(stale_move.z_order, FRONT_Z + 1);
    assert_eq!(
        dockspace
            .engine()
            .workspace()
            .contained_frontmost(SURFACE)
            .expect("surface exists")
            .expect("fixture has contained roots")
            .floating(),
        REAR_FLOATING
    );

    let stable_begin = run_frame(
        context,
        dockspace,
        panes,
        vec![Event::PointerMoved(current)],
    );
    assert!(stable_begin.interactions_current);
    assert_eq!(stable_begin.transform_events.began, 1);
    assert!(matches!(
        stable_begin.status,
        InteractionStatus::ContainedTransforming { .. }
    ));
    assert_eq!(stable_begin.rect, original);
    stable_begin
}

fn exercise_rear_gesture(gesture: Gesture, expected_kind: ContainedTransformKind) {
    let context = Context::default();
    let original = LogicalRect::new(80.0, 70.0, 240.0, 180.0).expect("finite fixture");
    let mut dockspace =
        Dockspace::builder("rear-contained-gesture", overlapping_workspace(original))
            .build()
            .expect("facade must build");
    let mut panes = TestPanes;
    warm(&context, &mut dockspace, &mut panes);
    let (press, current) = gesture_points(&dockspace, gesture);
    let expected = expected_rect(original, gesture, press, current);
    let active = raise_rear_and_recover_begin(
        &context,
        &mut dockspace,
        &mut panes,
        original,
        press,
        current,
    );
    let active_view = dockspace
        .engine()
        .interaction()
        .active_contained_transform_view()
        .expect("transform is active");
    let session = active_view.session();
    assert_eq!(
        active_view.initial_pointer(),
        LogicalPoint::new(f64::from(press.x), f64::from(press.y)).expect("press origin is finite")
    );
    assert_eq!(active_view.kind(), expected_kind);
    assert_eq!(active.rect, original);

    let preview_painted = run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![Event::PointerMoved(current)],
    );
    assert_eq!(preview_painted.preview, Some(expected));
    assert_eq!(preview_painted.rect, original);
    assert_eq!(
        preview_painted.transform_events.began, 0,
        "an active session cannot restart"
    );
    assert_eq!(
        dockspace
            .engine()
            .interaction()
            .active_contained_transform_view()
            .expect("same transform remains active")
            .session(),
        session
    );

    let released = run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![Event::PointerMoved(current), pointer_button(current, false)],
    );
    assert!(released.transform_events.preview_acknowledged);
    assert!(!released.transform_events.delivered);
    assert_eq!(released.rect, original);
    assert!(matches!(
        released.status,
        InteractionStatus::ContainedTransforming { .. }
    ));

    let idle = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    assert!(idle.transform_events.delivered);
    assert_eq!(idle.status, InteractionStatus::Idle);
    assert_eq!(idle.rect, expected);
    assert_eq!(idle.z_order, FRONT_Z + 1);
}

#[test]
fn rear_contained_move_survives_raise_staleness_and_commits_a_painted_preview() {
    exercise_rear_gesture(Gesture::Move, ContainedTransformKind::Move);
}

#[test]
fn rear_contained_resize_survives_raise_staleness_and_commits_a_painted_preview() {
    exercise_rear_gesture(
        Gesture::ResizeWest,
        ContainedTransformKind::Resize(ContainedResizeEdges::horizontal(
            ContainedHorizontalResizeEdge::Left,
        )),
    );
}
