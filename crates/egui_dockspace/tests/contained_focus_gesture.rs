use dockspace::backend::interaction::{InteractionStatus, PreviewVisual};
use dockspace::command::MovePayload;
use dockspace::geometry::{LogicalPoint, LogicalRect};
use dockspace::graph::{ContainedFloating, Node, RootRecord, SurfacePresentation, Workspace};
use dockspace::ids::{FloatingPresentationId, ItemId, RootId, SurfaceId};
use dockspace::intent::{
    ContainedHorizontalResizeEdge, ContainedResizeEdges, ContainedTransformKind,
};
use egui::{Context, Event, Modifiers, PointerButton, Pos2, RawInput, Rect, Ui, vec2};
use egui_dockspace::backend::{EguiFrameScheduleKey, EguiPresentationResult};
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
    contained: Vec<FloatingPresentationId>,
    preview: Option<LogicalRect>,
    raised: Option<(usize, usize)>,
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
    builder.set_surface(SURFACE, SurfacePresentation::with_main(MAIN_ROOT));
    builder.set_contained_floating(REAR_FLOATING, ContainedFloating::new(REAR_ROOT, rear_rect));
    builder.set_contained_floating(
        FRONT_FLOATING,
        ContainedFloating::new(FRONT_ROOT, front_rect),
    );
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
    let contained_before = dockspace
        .workspace()
        .surface(SURFACE)
        .expect("surface remains present")
        .contained
        .clone();
    let sequence = dockspace.last_egui_frame_schedule_key().map_or(1, |key| {
        key.sequence()
            .checked_add(1)
            .expect("fixture frame sequence must not overflow")
    });
    let input = RawInput {
        screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(640.0, 480.0))),
        events: events.into_iter().map(Into::into).collect(),
        ..RawInput::default()
    };
    let mut frame = dockspace
        .begin_outer_frame(EguiFrameScheduleKey::new(sequence, 0))
        .expect("outer egui frame begins");
    let paint = frame
        .run_surface(SURFACE, context, input, panes)
        .expect("outer host owns the complete surface pass");
    let interactions_current = paint.interactions_current();
    let (_, outputs) = frame
        .finish()
        .expect("outer egui frame commits")
        .into_parts();
    for output in outputs {
        output.settle_with(|surface, _| {
            assert_eq!(surface, SURFACE);
            EguiPresentationResult::Presented
        });
    }

    let floating = dockspace
        .workspace()
        .contained_floating(REAR_FLOATING)
        .expect("rear floating remains presented");
    let contained = dockspace
        .workspace()
        .surface(SURFACE)
        .expect("surface remains present")
        .contained
        .clone();
    let raised = contained_before
        .iter()
        .position(|floating| *floating == REAR_FLOATING)
        .zip(
            contained
                .iter()
                .position(|floating| *floating == REAR_FLOATING),
        )
        .filter(|(from, to)| from != to);
    FrameObservation {
        interactions_current,
        saw_stale_pass: !interactions_current,
        status: dockspace.core_engine().interaction().status(),
        rect: floating.rect,
        contained,
        preview: dockspace
            .core_engine()
            .interaction()
            .preview()
            .and_then(|preview| match preview.visual() {
                PreviewVisual::Contained { rect, .. } => Some(*rect),
                PreviewVisual::Dock { .. } | PreviewVisual::Native { .. } => None,
            })
            .or_else(|| {
                dockspace
                    .core_engine()
                    .interaction()
                    .contained_transform_preview()
                    .map(|preview| preview.rect())
            }),
        raised,
    }
}

fn warm(context: &Context, dockspace: &mut Dockspace, panes: &mut TestPanes) {
    let first = run_frame(context, dockspace, panes, Vec::new());
    assert!(first.saw_stale_pass);
    assert!(!first.interactions_current);
    let _ = run_frame(context, dockspace, panes, Vec::new());
    assert!(run_frame(context, dockspace, panes, Vec::new()).interactions_current);
}

#[allow(
    clippy::cast_possible_truncation,
    reason = "finite fixture geometry is deliberately converted into egui's f32 input space"
)]
fn gesture_points(dockspace: &Dockspace, gesture: Gesture) -> (Pos2, Pos2) {
    let rear = dockspace
        .core_engine()
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

fn activate_rear_and_cross_drag_threshold(
    context: &Context,
    dockspace: &mut Dockspace,
    panes: &mut TestPanes,
    original: LogicalRect,
    gesture: Gesture,
    press: Pos2,
    current: Pos2,
) -> FrameObservation {
    let pressed = run_frame(
        context,
        dockspace,
        panes,
        vec![Event::PointerMoved(press), pointer_button(press, true)],
    );
    match gesture {
        Gesture::Move => {
            assert!(matches!(pressed.status, InteractionStatus::Armed { .. }));
        }
        Gesture::ResizeWest => {
            assert!(matches!(
                pressed.status,
                InteractionStatus::ContainedTransforming { .. }
            ));
        }
    }
    assert_eq!(pressed.rect, original);
    assert_eq!(pressed.raised, Some((0, 1)));
    assert_eq!(pressed.contained, vec![FRONT_FLOATING, REAR_FLOATING]);

    let mut saw_stale_projection = false;
    let mut projection_became_current = false;
    for _ in 0..4 {
        let settlement = run_frame(context, dockspace, panes, Vec::new());
        saw_stale_projection |= settlement.saw_stale_pass;
        assert_eq!(settlement.rect, original);
        assert_eq!(settlement.raised, None);
        assert_eq!(settlement.contained, vec![FRONT_FLOATING, REAR_FLOATING]);
        match gesture {
            Gesture::Move => assert!(matches!(settlement.status, InteractionStatus::Armed { .. })),
            Gesture::ResizeWest => assert!(matches!(
                settlement.status,
                InteractionStatus::ContainedTransforming { .. }
            )),
        }
        if settlement.interactions_current {
            projection_became_current = true;
            break;
        }
    }
    assert!(saw_stale_projection);
    assert!(
        projection_became_current,
        "the raised projection must regain exact interaction authority"
    );

    let threshold_move = run_frame(
        context,
        dockspace,
        panes,
        vec![Event::PointerMoved(current)],
    );

    assert!(!threshold_move.saw_stale_pass);
    assert!(threshold_move.interactions_current);
    match gesture {
        Gesture::Move => {
            assert_eq!(threshold_move.raised, None);
            assert!(matches!(
                threshold_move.status,
                InteractionStatus::Dragging { .. }
            ));
        }
        Gesture::ResizeWest => {
            assert_eq!(threshold_move.raised, None);
            assert!(matches!(
                threshold_move.status,
                InteractionStatus::ContainedTransforming { .. }
            ));
        }
    }
    assert_eq!(threshold_move.rect, original);
    assert_eq!(
        threshold_move.contained,
        vec![FRONT_FLOATING, REAR_FLOATING]
    );
    assert_eq!(
        dockspace
            .core_engine()
            .workspace()
            .surface(SURFACE)
            .expect("surface exists")
            .contained
            .last(),
        Some(&REAR_FLOATING)
    );

    let active = run_frame(
        context,
        dockspace,
        panes,
        vec![Event::PointerMoved(current)],
    );
    assert!(active.interactions_current);
    match gesture {
        Gesture::Move => assert!(matches!(active.status, InteractionStatus::Dragging { .. })),
        Gesture::ResizeWest => assert!(matches!(
            active.status,
            InteractionStatus::ContainedTransforming { .. }
        )),
    }
    assert_eq!(active.rect, original);
    active
}

fn exercise_rear_gesture(gesture: Gesture) {
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
    let active = activate_rear_and_cross_drag_threshold(
        &context,
        &mut dockspace,
        &mut panes,
        original,
        gesture,
        press,
        current,
    );
    let active_status = active.status;
    match gesture {
        Gesture::Move => {
            let active_view = dockspace
                .core_engine()
                .interaction()
                .active_drag_view()
                .expect("title drag is active");
            let MovePayload::Subtree(source) = active_view.payload() else {
                panic!("contained title drag preserves the complete rear root");
            };
            assert_eq!(source.root(), REAR_ROOT);
        }
        Gesture::ResizeWest => {
            let active_view = dockspace
                .core_engine()
                .interaction()
                .active_contained_transform_view()
                .expect("resize transform is active");
            assert_eq!(
                active_view.initial_pointer(),
                LogicalPoint::new(f64::from(press.x), f64::from(press.y))
                    .expect("press origin is finite")
            );
            assert_eq!(
                active_view.kind(),
                ContainedTransformKind::Resize(ContainedResizeEdges::horizontal(
                    ContainedHorizontalResizeEdge::Left,
                ))
            );
        }
    }
    assert_eq!(active.rect, original);

    let preview_painted = run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![Event::PointerMoved(current)],
    );
    assert_eq!(preview_painted.preview, Some(expected));
    assert_eq!(preview_painted.rect, original);
    assert_eq!(preview_painted.status, active_status);
    let released = run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![Event::PointerMoved(current), pointer_button(current, false)],
    );
    assert_eq!(released.rect, expected);
    assert_eq!(released.status, InteractionStatus::Idle);

    let idle = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    assert_eq!(idle.status, InteractionStatus::Idle);
    assert_eq!(idle.rect, expected);
    assert_eq!(idle.contained, vec![FRONT_FLOATING, REAR_FLOATING]);
}

#[test]
fn rear_contained_move_survives_raise_staleness_and_commits_a_painted_preview() {
    exercise_rear_gesture(Gesture::Move);
}

#[test]
fn rear_contained_resize_survives_raise_staleness_and_commits_a_painted_preview() {
    exercise_rear_gesture(Gesture::ResizeWest);
}
