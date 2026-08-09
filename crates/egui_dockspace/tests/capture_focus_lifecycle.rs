use std::collections::BTreeSet;

use dockspace::backend::interaction::InteractionStatus;
use dockspace::backend::scene::PresentationPlan;
use dockspace::drop_guide::{DropGuideScope, DropGuideSlot};
use dockspace::geometry::LogicalRect;
use dockspace::graph::{Axis, ContainedFloating, Node, RootRecord, SurfacePresentation, Workspace};
use dockspace::ids::{FloatingPresentationId, ItemId, RootId, SurfaceId};
use dockspace::runtime::WorkspaceVersion;
use egui::{Context, Event, Key, Modifiers, PointerButton, Pos2, RawInput, Rect, Ui, vec2};
use egui_dockspace::backend::{
    EguiFrameScheduleKey, EguiRendererOutputDisposition, HostFrameResponse,
};
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

#[derive(Default)]
struct DiscardingPanes {
    passes: BTreeSet<usize>,
}

impl PaneView for DiscardingPanes {
    fn title(&self, item: ItemId) -> Option<egui::WidgetText> {
        matches!(item, ITEM_A | ITEM_B).then(|| format!("Pane {}", item.get()).into())
    }

    fn ui(&mut self, _item: ItemId, ui: &mut Ui) {
        self.passes.insert(ui.ctx().current_pass_index());
        if ui.ctx().current_pass_index() == 0 {
            ui.ctx()
                .request_discard("exercise terminal action normalization");
        }
    }
}

fn single_workspace() -> Workspace {
    let mut builder = Workspace::builder();
    let tabs = builder.insert_node(Node::tabs([ITEM_A]));
    builder.set_root(MAIN_ROOT, RootRecord::new(tabs));
    builder.set_surface(SURFACE, SurfacePresentation::with_main(MAIN_ROOT));
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
    builder.set_surface(SURFACE, SurfacePresentation::with_main(MAIN_ROOT));
    builder.build().expect("split fixture is valid")
}

fn contained_workspace(rect: LogicalRect) -> Workspace {
    let mut builder = Workspace::builder();
    let main = builder.insert_node(Node::tabs([ITEM_A]));
    let floating = builder.insert_node(Node::tabs([ITEM_B]));
    builder.set_root(MAIN_ROOT, RootRecord::new(main));
    builder.set_root(FLOATING_ROOT, RootRecord::new(floating));
    builder.set_surface(SURFACE, SurfacePresentation::with_main(MAIN_ROOT));
    builder.set_contained_floating(FLOATING, ContainedFloating::new(FLOATING_ROOT, rect));
    builder
        .attach_contained(SURFACE, FLOATING)
        .expect("surface exists");
    builder.build().expect("contained fixture is valid")
}

fn raw_input(events: Vec<Event>, focused: bool) -> RawInput {
    RawInput {
        screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(600.0, 400.0))),
        events: events.into_iter().map(Into::into).collect(),
        focused,
        ..RawInput::default()
    }
}

fn run_frame(
    context: &Context,
    dockspace: &mut Dockspace,
    panes: &mut dyn PaneView,
    events: Vec<Event>,
) -> HostFrameResponse {
    run_input(context, dockspace, panes, raw_input(events, true))
}

fn run_input(
    context: &Context,
    dockspace: &mut Dockspace,
    panes: &mut dyn PaneView,
    input: RawInput,
) -> HostFrameResponse {
    let sequence = context
        .cumulative_frame_nr()
        .checked_add(1)
        .expect("fixture frame sequence remains representable");
    let mut host = dockspace
        .begin_outer_frame(EguiFrameScheduleKey::new(sequence, 0))
        .expect("outer host frame begins");
    host.run_surface(SURFACE, context, input, panes)
        .expect("outer host owns the complete egui surface pass");
    let (response, outputs) = host
        .finish()
        .expect("outer host frame commits")
        .into_parts();
    outputs.submit_with(|surface, _, _| {
        assert_eq!(surface, SURFACE);
        EguiRendererOutputDisposition::Accepted
    });
    response
}

fn warm(context: &Context, dockspace: &mut Dockspace, panes: &mut dyn PaneView) {
    run_frame(context, dockspace, panes, Vec::new());
    run_frame(context, dockspace, panes, Vec::new());
    run_frame(context, dockspace, panes, Vec::new());
}

fn painted_plan(dockspace: &Dockspace) -> &PresentationPlan {
    dockspace
        .core_engine()
        .interaction_projection(SURFACE)
        .map(dockspace::backend::scene::SurfaceInteractionProjection::plan)
        .expect("surface has an acknowledged painted plan")
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
    let rect = painted_plan(dockspace)
        .tab_records()
        .iter()
        .find(|tab| tab.id().item == item)
        .expect("requested tab is painted")
        .drag_hit()
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
    let ready = painted_plan(dockspace);
    let tabs = ready
        .tab_records()
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
    let rect = painted_plan(dockspace)
        .splitter_records()
        .first()
        .expect("split fixture paints one splitter")
        .hit()
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
        .core_engine()
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
    response: &HostFrameResponse,
) {
    assert_eq!(
        dockspace.core_engine().interaction().status(),
        InteractionStatus::Idle
    );
    assert_eq!(dockspace.workspace(), original);
    assert_eq!(dockspace.version(), WorkspaceVersion::default());
    assert!(!response.mutation().workspace_changed());
}

#[test]
fn armed_drag_survives_local_focus_loss_without_moving_the_item() {
    let context = Context::default();
    let original = single_workspace();
    let mut dockspace = Dockspace::backend_builder("armed-focus-loss", original.clone())
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
        dockspace.core_engine().interaction().status(),
        InteractionStatus::Armed { .. }
    ));

    let response = run_input(
        &context,
        &mut dockspace,
        &mut panes,
        raw_input(vec![Event::WindowFocused(false)], false),
    );

    assert!(matches!(
        dockspace.core_engine().interaction().status(),
        InteractionStatus::Armed { .. }
    ));
    assert_eq!(dockspace.workspace(), &original);
    assert!(!response.mutation().workspace_changed());

    let response = run_input(
        &context,
        &mut dockspace,
        &mut panes,
        raw_input(
            vec![
                Event::WindowFocused(true),
                Event::PointerMoved(source),
                pointer_button(source, false),
            ],
            true,
        ),
    );
    assert_cancelled_without_commit(&dockspace, &original, &response);
}

#[test]
fn active_drag_survives_local_pointer_gone_without_delivering_a_drop() {
    let context = Context::default();
    let original = split_workspace();
    let mut dockspace = Dockspace::backend_builder("drag-pointer-gone", original.clone())
        .build()
        .expect("dockspace builds");
    let mut panes = TestPanes;
    warm(&context, &mut dockspace, &mut panes);
    let source = tab_point(&dockspace, ITEM_A);
    let moved = inner_guide_point(
        &dockspace,
        ITEM_B,
        DropGuideSlot::Edge(dockspace::command::Edge::Right),
    );

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
        dockspace.core_engine().interaction().status(),
        InteractionStatus::Dragging { .. }
    ));
    let preview = dockspace
        .core_engine()
        .interaction()
        .preview()
        .cloned()
        .expect("the target preview is published");

    let response = run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![Event::PointerGone],
    );
    assert!(matches!(
        dockspace.core_engine().interaction().status(),
        InteractionStatus::Dragging { .. }
    ));
    assert_eq!(
        dockspace.core_engine().interaction().preview(),
        Some(&preview)
    );
    assert!(!response.mutation().workspace_changed());
    let response = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    assert!(
        matches!(
            dockspace.core_engine().interaction().status(),
            InteractionStatus::Dragging { .. }
        ),
        "one surface's local pointer loss is not global capture authority: status={:?}",
        dockspace.core_engine().interaction().status()
    );
    assert_eq!(dockspace.workspace(), &original);
    assert_eq!(
        dockspace.core_engine().interaction().preview(),
        Some(&preview)
    );
    assert!(!response.mutation().workspace_changed());

    let response = run_frame(&context, &mut dockspace, &mut panes, vec![escape_pressed()]);
    assert_cancelled_without_commit(&dockspace, &original, &response);
}

#[test]
fn active_resize_survives_local_button_state_without_a_release_edge() {
    let context = Context::default();
    let original = split_workspace();
    let mut dockspace = Dockspace::backend_builder("resize-capture-loss", original.clone())
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
        dockspace.core_engine().interaction().status(),
        InteractionStatus::Resizing { .. }
    ));

    let _ = crate::test_support::run_ui_without_renderer(
        &context,
        raw_input(
            vec![Event::PointerMoved(moved), pointer_button(moved, false)],
            true,
        ),
        |_ui| {},
    );
    let response = run_frame(&context, &mut dockspace, &mut panes, Vec::new());

    assert!(matches!(
        dockspace.core_engine().interaction().status(),
        InteractionStatus::Resizing { .. }
    ));
    assert_eq!(dockspace.workspace(), &original);
    assert!(!response.mutation().workspace_changed());

    let response = run_frame(&context, &mut dockspace, &mut panes, vec![escape_pressed()]);
    assert_cancelled_without_commit(&dockspace, &original, &response);
}

#[test]
fn contained_title_drag_survives_local_focus_loss_without_committing_the_preview() {
    let context = Context::default();
    let rect = LogicalRect::new(140.0, 90.0, 220.0, 160.0).expect("finite rect");
    let original = contained_workspace(rect);
    let mut dockspace = Dockspace::backend_builder("contained-focus-loss", original.clone())
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
        dockspace.core_engine().interaction().status(),
        InteractionStatus::Dragging { .. }
    ));
    let preview = dockspace
        .core_engine()
        .interaction()
        .preview()
        .cloned()
        .expect("the contained docking preview is published");

    let response = run_input(
        &context,
        &mut dockspace,
        &mut panes,
        raw_input(vec![Event::WindowFocused(false)], false),
    );
    assert!(matches!(
        dockspace.core_engine().interaction().status(),
        InteractionStatus::Dragging { .. }
    ));
    assert_eq!(
        dockspace.core_engine().interaction().preview(),
        Some(&preview)
    );
    assert!(!response.mutation().workspace_changed());
    let response = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    assert!(
        matches!(
            dockspace.core_engine().interaction().status(),
            InteractionStatus::Dragging { .. }
        ),
        "one surface's local focus loss is not global capture authority"
    );
    assert_eq!(dockspace.workspace(), &original);
    assert_eq!(
        dockspace.core_engine().interaction().preview(),
        Some(&preview)
    );
    assert!(!response.mutation().workspace_changed());

    let response = run_frame(&context, &mut dockspace, &mut panes, vec![escape_pressed()]);
    assert_cancelled_without_commit(&dockspace, &original, &response);
}

#[test]
fn matching_release_keeps_pre_drag_release_semantics_when_focus_is_lost() {
    let context = Context::default();
    let original = single_workspace();
    let mut dockspace = Dockspace::backend_builder("release-before-focus-loss", original.clone())
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
        dockspace.core_engine().interaction().status(),
        InteractionStatus::Armed { .. }
    ));

    let response = run_input(
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

    assert_cancelled_without_commit(&dockspace, &original, &response);
}

#[test]
fn escape_keeps_priority_when_pointer_capture_is_lost() {
    let context = Context::default();
    let original = single_workspace();
    let mut dockspace = Dockspace::backend_builder("escape-before-capture-loss", original.clone())
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
        dockspace.core_engine().interaction().status(),
        InteractionStatus::Dragging { .. }
    ));

    let response = run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![Event::PointerGone, escape_pressed()],
    );

    assert_cancelled_without_commit(&dockspace, &original, &response);
}

fn prepare_multipass_drag(name: &'static str) -> (Context, Workspace, Dockspace, Pos2) {
    let context = Context::default();
    let original = split_workspace();
    let mut dockspace = Dockspace::backend_builder(name, original.clone())
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
        dockspace.core_engine().interaction().status(),
        InteractionStatus::Dragging { .. }
    ));
    assert!(dockspace.core_engine().interaction().preview().is_some());
    (context, original, dockspace, moved)
}

fn finish_multipass_terminal_frame(
    context: &Context,
    dockspace: &mut Dockspace,
    events: Vec<Event>,
) -> (HostFrameResponse, DiscardingPanes) {
    let sequence = context
        .cumulative_frame_nr()
        .checked_add(1)
        .expect("fixture frame sequence remains representable");
    let mut host = dockspace
        .begin_outer_frame(EguiFrameScheduleKey::new(sequence, 0))
        .expect("outer host frame begins");
    let mut panes = DiscardingPanes::default();
    host.run_surface(SURFACE, context, raw_input(events, true), &mut panes)
        .expect("outer host owns the multipass surface run");
    let (response, outputs) = host
        .finish()
        .expect("outer host frame commits")
        .into_parts();
    outputs.submit_with(|_, _, _| EguiRendererOutputDisposition::Accepted);
    assert_eq!(panes.passes, BTreeSet::from([0, 1]));
    (response, panes)
}

#[test]
fn multipass_release_before_escape_commits_the_painted_drop() {
    let (context, original, mut dockspace, moved) =
        prepare_multipass_drag("multipass-release-before-escape");
    let (response, _) = finish_multipass_terminal_frame(
        &context,
        &mut dockspace,
        vec![
            Event::PointerMoved(moved),
            pointer_button(moved, false),
            escape_pressed(),
        ],
    );

    assert!(response.mutation().workspace_changed());
    assert_eq!(
        dockspace.core_engine().interaction().status(),
        InteractionStatus::Idle
    );
    assert_ne!(dockspace.workspace(), &original);
}

#[test]
fn multipass_escape_before_release_cancels_the_painted_drop() {
    let (context, original, mut dockspace, moved) =
        prepare_multipass_drag("multipass-escape-before-release");
    let (response, _) = finish_multipass_terminal_frame(
        &context,
        &mut dockspace,
        vec![
            Event::PointerMoved(moved),
            escape_pressed(),
            pointer_button(moved, false),
        ],
    );

    assert!(!response.mutation().workspace_changed());
    assert_eq!(
        dockspace.core_engine().interaction().status(),
        InteractionStatus::Idle
    );
    assert_eq!(dockspace.workspace(), &original);
}

#[test]
fn contained_max_edge_is_excluded_from_resize_gesture_ownership() {
    let context = Context::default();
    let rect = LogicalRect::new(120.0, 80.0, 240.0, 180.0).expect("fixture rect is valid");
    let original = contained_workspace(rect);
    let mut dockspace =
        Dockspace::backend_builder("contained-half-open-max-edge", original.clone())
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
        dockspace.core_engine().interaction().status(),
        InteractionStatus::Idle
    );
    assert_eq!(dockspace.workspace(), &original);
}
