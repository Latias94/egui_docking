use dockspace::drop_guide::{DropGuideScope, DropGuideSlot};
use dockspace::geometry::LogicalRect;
use dockspace::graph::{Axis, ContainedFloating, Node, RootRecord, SurfacePresentation, Workspace};
use dockspace::ids::{FloatingPresentationId, ItemId, RootId, SurfaceId};
use dockspace::interaction::{
    InteractionCancelReason, InteractionEventKind, InteractionOutcome, InteractionRejection,
    InteractionStatus,
};
use dockspace::scene::SurfaceScene;
use dockspace::transition::{InputOutcome, WorkspaceVersion};
use egui::{Context, Event, Key, Modifiers, PointerButton, Pos2, RawInput, Rect, Ui, vec2};
use egui_dockspace::{Dockspace, PaneView};

const SURFACE: SurfaceId = SurfaceId::new(1);
const MAIN_ROOT: RootId = RootId::new(10);
const FLOATING_ROOT: RootId = RootId::new(11);
const FLOATING: FloatingPresentationId = FloatingPresentationId::new(20);
const ITEM_A: ItemId = ItemId::new(100);
const ITEM_B: ItemId = ItemId::new(101);

struct TestPanes;

impl PaneView for TestPanes {
    fn title(&self, item: ItemId) -> Option<egui::WidgetText> {
        matches!(item, ITEM_A | ITEM_B).then(|| format!("Pane {}", item.get()).into())
    }

    fn ui(&mut self, _item: ItemId, _ui: &mut Ui) {}
}

fn single_workspace() -> Workspace {
    let mut builder = Workspace::builder();
    let tabs = builder.insert_node(Node::tabs([ITEM_A]));
    builder.set_root(MAIN_ROOT, RootRecord::new(tabs));
    builder.set_surface(SURFACE, SurfacePresentation::new(MAIN_ROOT));
    builder.build().expect("single surface fixture is valid")
}

fn split_workspace() -> Workspace {
    let mut builder = Workspace::builder();
    let left = builder.insert_node(Node::tabs([ITEM_A]));
    let right = builder.insert_node(Node::tabs([ITEM_B]));
    let split = builder.insert_node(
        Node::equal_split(Axis::Horizontal, [left, right]).expect("two children form a split"),
    );
    builder.set_root(MAIN_ROOT, RootRecord::new(split));
    builder.set_surface(SURFACE, SurfacePresentation::new(MAIN_ROOT));
    builder.build().expect("split fixture is valid")
}

fn contained_workspace(rect: LogicalRect) -> Workspace {
    let mut builder = Workspace::builder();
    let main = builder.insert_node(Node::tabs([ITEM_A]));
    let floating = builder.insert_node(Node::tabs([ITEM_B]));
    builder.set_root(MAIN_ROOT, RootRecord::new(main));
    builder.set_root(FLOATING_ROOT, RootRecord::new(floating));
    builder.set_surface(SURFACE, SurfacePresentation::new(MAIN_ROOT));
    builder.set_contained_floating(ContainedFloating::new(
        FLOATING,
        FLOATING_ROOT,
        SURFACE,
        rect,
        1,
    ));
    builder
        .attach_contained(SURFACE, FLOATING)
        .expect("surface exists");
    builder.build().expect("contained fixture is valid")
}

fn raw_input(events: Vec<Event>, focused: bool) -> RawInput {
    RawInput {
        screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(600.0, 400.0))),
        events,
        focused,
        ..RawInput::default()
    }
}

fn run_frame(
    context: &Context,
    dockspace: &mut Dockspace,
    panes: &mut TestPanes,
    events: Vec<Event>,
) -> Vec<InteractionCancelReason> {
    run_input(context, dockspace, panes, raw_input(events, true))
}

fn run_input(
    context: &Context,
    dockspace: &mut Dockspace,
    panes: &mut TestPanes,
    input: RawInput,
) -> Vec<InteractionCancelReason> {
    let mut reasons = Vec::new();
    let _ = context.run_ui(input, |ui| {
        let response = dockspace
            .show(SURFACE, ui, panes)
            .expect("egui frame advances");
        reasons.extend(response.transitions().iter().flat_map(|transition| {
            transition
                .interaction_events()
                .iter()
                .filter_map(|event| match event.kind() {
                    InteractionEventKind::Cancelled { reason, .. } => Some(*reason),
                    _ => None,
                })
        }));
    });
    reasons
}

fn warm(context: &Context, dockspace: &mut Dockspace, panes: &mut TestPanes) {
    run_frame(context, dockspace, panes, Vec::new());
    run_frame(context, dockspace, panes, Vec::new());
}

fn pointer_button(position: Pos2, pressed: bool) -> Event {
    Event::PointerButton {
        pos: position,
        button: PointerButton::Primary,
        pressed,
        modifiers: Modifiers::NONE,
    }
}

fn escape_pressed() -> Event {
    Event::Key {
        key: Key::Escape,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: Modifiers::NONE,
    }
}

#[allow(
    clippy::cast_possible_truncation,
    reason = "finite scene coordinates intentionally become egui f32 input coordinates"
)]
fn tab_point(dockspace: &Dockspace, item: ItemId) -> Pos2 {
    let SurfaceScene::Ready(ready) = dockspace
        .engine()
        .scene()
        .and_then(|scene| scene.surface(SURFACE))
        .expect("surface scene exists")
    else {
        panic!("surface scene is ready");
    };
    let rect = ready
        .tabs()
        .iter()
        .find(|tab| tab.id().item == item)
        .expect("requested tab is painted")
        .rect();
    Pos2::new(
        (rect.min().x() + 8.0) as f32,
        ((rect.min().y() + rect.max().y()) * 0.5) as f32,
    )
}

#[allow(
    clippy::cast_possible_truncation,
    reason = "finite scene coordinates intentionally become egui f32 input coordinates"
)]
fn inner_guide_point(dockspace: &Dockspace, item: ItemId, slot: DropGuideSlot) -> Pos2 {
    let SurfaceScene::Ready(ready) = dockspace
        .engine()
        .scene()
        .and_then(|scene| scene.surface(SURFACE))
        .expect("surface scene exists")
    else {
        panic!("surface scene is ready");
    };
    let tabs = ready
        .tabs()
        .iter()
        .find(|tab| tab.id().item == item)
        .expect("requested tab is painted")
        .id()
        .tabs;
    let rect = ready
        .drop_guide_clusters()
        .iter()
        .find(|cluster| cluster.id().scope == DropGuideScope::Inner(tabs))
        .and_then(|cluster| cluster.target(slot))
        .expect("requested inner guide is published")
        .target()
        .region()
        .rect();
    Pos2::new(
        ((rect.min().x() + rect.max().x()) * 0.5) as f32,
        ((rect.min().y() + rect.max().y()) * 0.5) as f32,
    )
}

#[allow(
    clippy::cast_possible_truncation,
    reason = "finite scene coordinates intentionally become egui f32 input coordinates"
)]
fn splitter_point(dockspace: &Dockspace) -> Pos2 {
    let SurfaceScene::Ready(ready) = dockspace
        .engine()
        .scene()
        .and_then(|scene| scene.surface(SURFACE))
        .expect("surface scene exists")
    else {
        panic!("surface scene is ready");
    };
    let rect = ready
        .splitters()
        .first()
        .expect("split fixture paints one splitter")
        .rect();
    Pos2::new(
        ((rect.min().x() + rect.max().x()) * 0.5) as f32,
        ((rect.min().y() + rect.max().y()) * 0.5) as f32,
    )
}

#[allow(
    clippy::cast_possible_truncation,
    reason = "finite fixture coordinates intentionally become egui f32 input coordinates"
)]
fn contained_title_point(dockspace: &Dockspace) -> Pos2 {
    let floating = dockspace
        .engine()
        .workspace()
        .contained_floating(FLOATING)
        .expect("contained floating exists");
    let style = dockspace.style();
    let title_inset = style
        .floating_resize_extent
        .min(style.floating_title_height * 0.5);
    let dock_grip_width = style.floating_title_height - 2.0 * title_inset;
    Pos2::new(
        floating.rect.min().x() as f32
            + style.floating_border_width
            + style.floating_resize_extent
            + dock_grip_width
            + style.tab_horizontal_padding,
        floating.rect.min().y() as f32
            + style.floating_border_width
            + style.floating_title_height * 0.5,
    )
}

fn assert_cancelled_without_commit(
    dockspace: &Dockspace,
    original: &Workspace,
    reasons: &[InteractionCancelReason],
    expected: InteractionCancelReason,
) {
    assert_eq!(
        dockspace.engine().interaction().status(),
        InteractionStatus::Idle
    );
    assert_eq!(dockspace.engine().workspace(), original);
    assert_eq!(dockspace.engine().version(), WorkspaceVersion::default());
    assert!(
        reasons.contains(&expected),
        "expected {expected:?} cancellation, got {reasons:?}"
    );
}

#[test]
fn armed_drag_cancels_on_focus_loss_without_moving_the_item() {
    let context = Context::default();
    let original = single_workspace();
    let mut dockspace = Dockspace::builder("armed-focus-loss", original.clone())
        .build()
        .expect("dockspace builds");
    let mut panes = TestPanes;
    warm(&context, &mut dockspace, &mut panes);
    let source = tab_point(&dockspace, ITEM_A);

    run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![Event::PointerMoved(source), pointer_button(source, true)],
    );
    run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![Event::PointerMoved(source)],
    );
    assert!(matches!(
        dockspace.engine().interaction().status(),
        InteractionStatus::Armed { .. }
    ));

    run_input(
        &context,
        &mut dockspace,
        &mut panes,
        raw_input(vec![Event::WindowFocused(false)], false),
    );
    let reasons = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    run_frame(&context, &mut dockspace, &mut panes, Vec::new());

    assert_cancelled_without_commit(
        &dockspace,
        &original,
        &reasons,
        InteractionCancelReason::FocusLost,
    );
}

#[test]
fn active_drag_cancels_on_pointer_gone_without_delivering_a_drop() {
    let context = Context::default();
    let original = single_workspace();
    let mut dockspace = Dockspace::builder("drag-pointer-gone", original.clone())
        .build()
        .expect("dockspace builds");
    let mut panes = TestPanes;
    warm(&context, &mut dockspace, &mut panes);
    let source = tab_point(&dockspace, ITEM_A);
    let moved = source + vec2(60.0, 30.0);

    run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![Event::PointerMoved(source), pointer_button(source, true)],
    );
    run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![Event::PointerMoved(moved)],
    );
    run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![Event::PointerMoved(moved)],
    );
    assert!(matches!(
        dockspace.engine().interaction().status(),
        InteractionStatus::Dragging { .. }
    ));

    run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![Event::PointerGone],
    );
    let reasons = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    run_frame(&context, &mut dockspace, &mut panes, Vec::new());

    assert_cancelled_without_commit(
        &dockspace,
        &original,
        &reasons,
        InteractionCancelReason::CaptureLost,
    );
}

#[test]
fn active_resize_cancels_when_the_button_is_no_longer_down_without_a_release_edge() {
    let context = Context::default();
    let original = split_workspace();
    let mut dockspace = Dockspace::builder("resize-capture-loss", original.clone())
        .build()
        .expect("dockspace builds");
    let mut panes = TestPanes;
    warm(&context, &mut dockspace, &mut panes);
    let source = splitter_point(&dockspace);
    let moved = source + vec2(70.0, 0.0);

    run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![Event::PointerMoved(source), pointer_button(source, true)],
    );
    run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![Event::PointerMoved(moved)],
    );
    assert!(matches!(
        dockspace.engine().interaction().status(),
        InteractionStatus::Resizing { .. }
    ));

    let _ = context.run_ui(
        raw_input(
            vec![Event::PointerMoved(moved), pointer_button(moved, false)],
            true,
        ),
        |_ui| {},
    );
    run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let reasons = run_frame(&context, &mut dockspace, &mut panes, Vec::new());

    assert_cancelled_without_commit(
        &dockspace,
        &original,
        &reasons,
        InteractionCancelReason::CaptureLost,
    );
}

#[test]
fn contained_title_drag_cancels_on_focus_loss_without_committing_the_preview() {
    let context = Context::default();
    let rect = LogicalRect::new(140.0, 90.0, 220.0, 160.0).expect("finite rect");
    let original = contained_workspace(rect);
    let mut dockspace = Dockspace::builder("contained-focus-loss", original.clone())
        .build()
        .expect("dockspace builds");
    let mut panes = TestPanes;
    warm(&context, &mut dockspace, &mut panes);
    let source = contained_title_point(&dockspace);
    let moved = source + vec2(45.0, 35.0);

    run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![Event::PointerMoved(source), pointer_button(source, true)],
    );
    run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![Event::PointerMoved(moved)],
    );
    run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![Event::PointerMoved(moved)],
    );
    assert!(matches!(
        dockspace.engine().interaction().status(),
        InteractionStatus::Dragging { .. }
    ));

    run_input(
        &context,
        &mut dockspace,
        &mut panes,
        raw_input(vec![Event::WindowFocused(false)], false),
    );
    let reasons = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    run_frame(&context, &mut dockspace, &mut panes, Vec::new());

    assert_cancelled_without_commit(
        &dockspace,
        &original,
        &reasons,
        InteractionCancelReason::FocusLost,
    );
}

#[test]
fn matching_release_keeps_pre_drag_release_semantics_when_focus_is_lost() {
    let context = Context::default();
    let original = single_workspace();
    let mut dockspace = Dockspace::builder("release-before-focus-loss", original.clone())
        .build()
        .expect("dockspace builds");
    let mut panes = TestPanes;
    warm(&context, &mut dockspace, &mut panes);
    let source = tab_point(&dockspace, ITEM_A);

    run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![Event::PointerMoved(source), pointer_button(source, true)],
    );
    run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![Event::PointerMoved(source)],
    );
    assert!(matches!(
        dockspace.engine().interaction().status(),
        InteractionStatus::Armed { .. }
    ));

    run_input(
        &context,
        &mut dockspace,
        &mut panes,
        raw_input(
            vec![
                Event::PointerMoved(source),
                pointer_button(source, false),
                Event::WindowFocused(false),
            ],
            false,
        ),
    );
    let reasons = run_frame(&context, &mut dockspace, &mut panes, Vec::new());

    assert_cancelled_without_commit(
        &dockspace,
        &original,
        &reasons,
        InteractionCancelReason::ReleasedBeforeDrag,
    );
    assert!(!reasons.contains(&InteractionCancelReason::FocusLost));
}

#[test]
fn escape_keeps_priority_when_pointer_capture_is_lost() {
    let context = Context::default();
    let original = single_workspace();
    let mut dockspace = Dockspace::builder("escape-before-capture-loss", original.clone())
        .build()
        .expect("dockspace builds");
    let mut panes = TestPanes;
    warm(&context, &mut dockspace, &mut panes);
    let source = tab_point(&dockspace, ITEM_A);
    let moved = source + vec2(60.0, 30.0);

    run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![Event::PointerMoved(source), pointer_button(source, true)],
    );
    run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![Event::PointerMoved(moved)],
    );
    run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![Event::PointerMoved(moved)],
    );
    assert!(matches!(
        dockspace.engine().interaction().status(),
        InteractionStatus::Dragging { .. }
    ));

    run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![Event::PointerGone, escape_pressed()],
    );
    let reasons = run_frame(&context, &mut dockspace, &mut panes, Vec::new());

    assert_cancelled_without_commit(
        &dockspace,
        &original,
        &reasons,
        InteractionCancelReason::Escape,
    );
    assert!(!reasons.contains(&InteractionCancelReason::CaptureLost));
}

#[test]
fn multipass_escape_suppresses_release_and_preview_acknowledgement_for_the_same_drag() {
    let context = Context::default();
    let original = split_workspace();
    let mut dockspace = Dockspace::builder("multipass-terminal-priority", original.clone())
        .build()
        .expect("dockspace builds");
    let mut panes = TestPanes;
    warm(&context, &mut dockspace, &mut panes);
    let source = tab_point(&dockspace, ITEM_A);
    let moved = inner_guide_point(&dockspace, ITEM_B, DropGuideSlot::Center);

    run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![Event::PointerMoved(source), pointer_button(source, true)],
    );
    for _ in 0..3 {
        run_frame(
            &context,
            &mut dockspace,
            &mut panes,
            vec![Event::PointerMoved(moved)],
        );
    }
    assert!(matches!(
        dockspace.engine().interaction().status(),
        InteractionStatus::Dragging { .. }
    ));
    assert!(dockspace.engine().interaction().preview().is_some());

    let mut passes = 0;
    let _ = context.run_ui(
        raw_input(
            vec![
                Event::PointerMoved(moved),
                pointer_button(moved, false),
                escape_pressed(),
            ],
            true,
        ),
        |ui| {
            dockspace
                .show(SURFACE, ui, &mut panes)
                .expect("multipass terminal frame advances");
            passes += 1;
            if ui.ctx().current_pass_index() == 0 {
                ui.ctx()
                    .request_discard("exercise terminal action normalization");
            }
        },
    );
    assert_eq!(passes, 2);

    let mut reasons = Vec::new();
    let mut rejections = Vec::<InteractionRejection>::new();
    let _ = context.run_ui(raw_input(Vec::new(), true), |ui| {
        let response = dockspace
            .show(SURFACE, ui, &mut panes)
            .expect("normalized actions reduce");
        for transition in response.transitions() {
            reasons.extend(transition.interaction_events().iter().filter_map(|event| {
                match event.kind() {
                    InteractionEventKind::Cancelled { reason, .. } => Some(*reason),
                    _ => None,
                }
            }));
            rejections.extend(transition.reduced_inputs().iter().filter_map(|input| {
                match input.outcome() {
                    InputOutcome::InteractionProcessed {
                        outcome: InteractionOutcome::Rejected(rejection),
                        ..
                    } => Some(rejection.clone()),
                    _ => None,
                }
            }));
        }
    });

    assert_eq!(reasons, [InteractionCancelReason::Escape]);
    assert!(
        rejections.is_empty(),
        "unexpected rejections: {rejections:?}"
    );
    assert_eq!(
        dockspace.engine().interaction().status(),
        InteractionStatus::Idle
    );
    assert_eq!(dockspace.engine().workspace(), &original);
}

#[test]
fn contained_max_edge_is_excluded_from_resize_gesture_ownership() {
    let context = Context::default();
    let rect = LogicalRect::new(120.0, 80.0, 240.0, 180.0).expect("fixture rect is valid");
    let original = contained_workspace(rect);
    let mut dockspace = Dockspace::builder("contained-half-open-max-edge", original.clone())
        .build()
        .expect("dockspace builds");
    let mut panes = TestPanes;
    warm(&context, &mut dockspace, &mut panes);
    #[allow(
        clippy::cast_possible_truncation,
        reason = "finite fixture coordinates intentionally become egui f32 input coordinates"
    )]
    let edge = Pos2::new(
        rect.max().x() as f32,
        ((rect.min().y() + rect.max().y()) * 0.5) as f32,
    );
    let moved = edge + vec2(40.0, 0.0);

    run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![Event::PointerMoved(edge), pointer_button(edge, true)],
    );
    run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![Event::PointerMoved(moved)],
    );
    run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![Event::PointerMoved(moved), pointer_button(moved, false)],
    );
    run_frame(&context, &mut dockspace, &mut panes, Vec::new());

    assert_eq!(
        dockspace.engine().interaction().status(),
        InteractionStatus::Idle
    );
    assert_eq!(dockspace.engine().workspace(), &original);
}
