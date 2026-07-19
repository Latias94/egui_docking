use std::cell::Cell;
use std::collections::BTreeSet;
use std::rc::Rc;

use dockspace::command::{Edge, MovePayload};
use dockspace::drop_guide::{DropGuideScope, DropGuideSlot};
use dockspace::drop_target::DropTargetId;
use dockspace::geometry::LogicalRect;
use dockspace::graph::{Axis, ContainedFloating, Node, RootRecord, SurfacePresentation, Workspace};
use dockspace::ids::{FloatingPresentationId, ItemId, NodeId, RootId, SurfaceId};
use dockspace::intent::TearOffRequest;
use dockspace::interaction::{
    InteractionDelivery, InteractionOutcome, InteractionStatus, PreviewVisual,
    WorkspaceDeliveryKind,
};
use dockspace::scene::SurfaceScene;
use dockspace::transition::{EngineTransition, InputOutcome};
use egui::{Context, Event, Modifiers, PointerButton, Pos2, RawInput, Rect, Ui, UiBuilder, vec2};
use egui_dockspace::{ContainedPresentationIds, Dockspace, PaneView, TearOffMode};

const SURFACE: SurfaceId = SurfaceId::new(1);
const MAIN_ROOT: RootId = RootId::new(2);
const FLOATING_ROOT: RootId = RootId::new(3);
const FLOATING: FloatingPresentationId = FloatingPresentationId::new(4);
const ITEM_A: ItemId = ItemId::new(10);
const ITEM_B: ItemId = ItemId::new(11);
const ITEM_C: ItemId = ItemId::new(12);
const ITEM_D: ItemId = ItemId::new(13);
const UNUSED_ROOT: RootId = RootId::new(30);
const UNUSED_FLOATING: FloatingPresentationId = FloatingPresentationId::new(31);

struct TestPanes;

impl PaneView for TestPanes {
    fn title(&self, item: ItemId) -> Option<egui::WidgetText> {
        match item {
            ITEM_A => Some("Main".into()),
            ITEM_B => Some("Left".into()),
            ITEM_C => Some("Right".into()),
            ITEM_D => Some("Selected left".into()),
            _ => None,
        }
    }

    fn ui(&mut self, _item: ItemId, _ui: &mut Ui) {}
}

fn workspace() -> Workspace {
    let mut builder = Workspace::builder();
    let main = builder.insert_node(Node::tabs([ITEM_A]));
    let left = builder.insert_node(Node::tabs_with_selection([ITEM_B, ITEM_D], Some(ITEM_D)));
    let right = builder.insert_node(Node::tabs([ITEM_C]));
    let floating = builder.insert_node(
        Node::equal_split(Axis::Horizontal, [left, right]).expect("two children form a split"),
    );
    builder.set_root(MAIN_ROOT, RootRecord::new(main));
    builder.set_root(FLOATING_ROOT, RootRecord::new(floating));
    builder.set_surface(SURFACE, SurfacePresentation::new(MAIN_ROOT));
    builder.set_contained_floating(ContainedFloating::new(
        FLOATING,
        FLOATING_ROOT,
        SURFACE,
        LogicalRect::new(120.0, 80.0, 340.0, 230.0).expect("fixture rect is valid"),
        1,
    ));
    builder
        .attach_contained(SURFACE, FLOATING)
        .expect("surface exists");
    builder.build().expect("fixture workspace is valid")
}

fn input(events: Vec<Event>) -> RawInput {
    RawInput {
        screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(600.0, 400.0))),
        events,
        ..RawInput::default()
    }
}

fn run_frame(
    context: &Context,
    dockspace: &mut Dockspace,
    panes: &mut TestPanes,
    events: Vec<Event>,
) -> Vec<EngineTransition> {
    let mut transitions = Vec::new();
    let _ = context.run_ui(input(events), |ui| {
        let mut surface = ui.new_child(
            UiBuilder::new().max_rect(Rect::from_min_size(Pos2::ZERO, vec2(480.0, 320.0))),
        );
        let response = dockspace
            .show(SURFACE, &mut surface, panes)
            .expect("fixture frame advances");
        transitions.extend_from_slice(response.transitions());
    });
    transitions
}

fn warm(context: &Context, dockspace: &mut Dockspace, panes: &mut TestPanes) {
    let _ = run_frame(context, dockspace, panes, Vec::new());
    let _ = run_frame(context, dockspace, panes, Vec::new());
}

fn pointer_button(position: Pos2, pressed: bool) -> Event {
    Event::PointerButton {
        pos: position,
        button: PointerButton::Primary,
        pressed,
        modifiers: Modifiers::NONE,
    }
}

#[allow(
    clippy::cast_possible_truncation,
    reason = "finite workspace coordinates are converted to egui's f32 input space"
)]
fn floating_title_point(dockspace: &Dockspace) -> Pos2 {
    let floating = dockspace
        .engine()
        .workspace()
        .contained_floating(FLOATING)
        .expect("fixture has a contained presentation");
    let style = dockspace.style();
    let inset = style
        .floating_resize_extent
        .min(style.floating_title_height * 0.5);
    let grip_height = style.floating_title_height - 2.0 * inset;
    Pos2::new(
        floating.rect.min().x() as f32
            + style.floating_border_width
            + style.floating_resize_extent
            + grip_height * 0.5,
        floating.rect.min().y() as f32
            + style.floating_border_width
            + style.floating_title_height * 0.5,
    )
}

fn exact_main_edge(dockspace: &Dockspace, edge: Edge) -> (DropTargetId, Pos2) {
    let SurfaceScene::Ready(ready) = dockspace
        .engine()
        .scene()
        .and_then(|scene| scene.surface(SURFACE))
        .expect("fixture scene exists")
    else {
        panic!("fixture scene is ready");
    };
    let target = ready
        .drop_guide_clusters()
        .iter()
        .find(|cluster| {
            cluster.id().root == MAIN_ROOT && matches!(cluster.id().scope, DropGuideScope::Inner(_))
        })
        .and_then(|cluster| cluster.target(DropGuideSlot::Edge(edge)))
        .expect("requested main edge guide exists")
        .target();
    let rect = target.region().rect();
    (
        target.id(),
        logical_point(
            (rect.min().x() + rect.max().x()) * 0.5,
            (rect.min().y() + rect.max().y()) * 0.5,
        ),
    )
}

fn interaction_outcomes(
    transitions: &[EngineTransition],
) -> impl Iterator<Item = &InteractionOutcome> {
    transitions
        .iter()
        .flat_map(EngineTransition::reduced_inputs)
        .filter_map(|input| match input.outcome() {
            InputOutcome::InteractionProcessed { outcome, .. } => Some(outcome),
            _ => None,
        })
}

#[allow(
    clippy::cast_possible_truncation,
    reason = "finite scene coordinates are converted to egui's f32 input space"
)]
fn logical_point(x: f64, y: f64) -> Pos2 {
    Pos2::new(x as f32, y as f32)
}

fn subtree_items(workspace: &Workspace, root: NodeId) -> Vec<ItemId> {
    let mut stack = vec![root];
    let mut items = Vec::new();
    while let Some(node) = stack.pop() {
        match workspace.node(node).expect("reachable node exists") {
            Node::Tabs {
                items: tab_items, ..
            } => items.extend(tab_items),
            Node::Split { children, .. } => stack.extend(children.iter().rev().copied()),
        }
    }
    items
}

fn reachable_nodes(workspace: &Workspace, root: NodeId) -> BTreeSet<NodeId> {
    let mut stack = vec![root];
    let mut reachable = BTreeSet::new();
    while let Some(node) = stack.pop() {
        if !reachable.insert(node) {
            continue;
        }
        if let Some(Node::Split { children, .. }) = workspace.node(node) {
            stack.extend(children.iter().rev().copied());
        }
    }
    reachable
}

fn exercise_complete_split_root_redock(edge: Edge) {
    let context = Context::default();
    let original = workspace();
    let source_root = original
        .root(FLOATING_ROOT)
        .expect("fixture floating root exists")
        .node;
    let Some(Node::Split {
        axis: Axis::Horizontal,
        children: source_children,
        ..
    }) = original.node(source_root)
    else {
        panic!("fixture floating root is a horizontal split");
    };
    let source_left = source_children[0];
    let source_right = source_children[1];
    let original_items = original.item_multiset();
    let mut dockspace = Dockspace::builder(("complete-subtree-redock", edge), original.clone())
        .build()
        .expect("fixture facade builds");
    let mut panes = TestPanes;
    warm(&context, &mut dockspace, &mut panes);
    let source = floating_title_point(&dockspace);
    let (target_id, target) = exact_main_edge(&dockspace, edge);

    let _ = run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![Event::PointerMoved(source), pointer_button(source, true)],
    );
    for _ in 0..4 {
        let _ = run_frame(
            &context,
            &mut dockspace,
            &mut panes,
            vec![Event::PointerMoved(target)],
        );
    }
    let active = dockspace
        .engine()
        .interaction()
        .active_drag_view()
        .expect("complete split-root drag is active");
    let MovePayload::Subtree(payload) = active.payload() else {
        panic!("contained title drag must carry the complete split subtree");
    };
    assert_eq!(payload.root(), FLOATING_ROOT);
    assert_eq!(payload.node(), source_root);
    let preview = dockspace
        .engine()
        .interaction()
        .preview()
        .expect("exact edge produces a dock preview");
    assert!(matches!(
        preview.visual(),
        PreviewVisual::Dock { target, .. } if *target == target_id
    ));
    let preview_session = preview.token().session();
    assert_eq!(dockspace.engine().workspace(), &original);

    let release = run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![Event::PointerMoved(target), pointer_button(target, false)],
    );
    assert_eq!(dockspace.engine().workspace(), &original);
    assert!(
        !interaction_outcomes(&release)
            .any(|outcome| matches!(outcome, InteractionOutcome::DragDelivered { .. }))
    );
    let delivery = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let protocol = interaction_outcomes(&delivery)
        .filter(|outcome| {
            matches!(
                outcome,
                InteractionOutcome::PreviewAcknowledged { .. }
                    | InteractionOutcome::DragDelivered { .. }
            )
        })
        .collect::<Vec<_>>();
    assert!(matches!(
        protocol.as_slice(),
        [
            InteractionOutcome::PreviewAcknowledged { session: acknowledged, .. },
            InteractionOutcome::DragDelivered {
                session: delivered,
                delivery: InteractionDelivery::Workspace {
                    kind: WorkspaceDeliveryKind::Dock,
                    changed: true,
                    ..
                },
            },
        ] if *acknowledged == preview_session && *delivered == preview_session
    ));

    let workspace = dockspace.engine().workspace();
    assert!(workspace.contained_floating(FLOATING).is_none());
    assert!(workspace.root(FLOATING_ROOT).is_none());
    let surface = workspace.surface(SURFACE).expect("surface remains present");
    assert_eq!(surface.main_root, MAIN_ROOT);
    assert!(surface.contained.is_empty());
    assert_eq!(workspace.item_multiset(), original_items);
    assert!(matches!(
        workspace.node(source_left),
        Some(Node::Tabs { items, selected })
            if items == &[ITEM_B, ITEM_D] && *selected == Some(ITEM_D)
    ));
    assert!(matches!(
        workspace.node(source_right),
        Some(Node::Tabs { items, selected })
            if items == &[ITEM_C] && *selected == Some(ITEM_C)
    ));
    let main_node = workspace
        .root(MAIN_ROOT)
        .expect("main root remains present")
        .node;
    let reachable = reachable_nodes(workspace, main_node);
    assert!(reachable.contains(&source_left));
    assert!(reachable.contains(&source_right));
    let Some(Node::Split { axis, children, .. }) = workspace.node(main_node) else {
        panic!("edge docking produces a split main root");
    };
    let expected_axis = match edge {
        Edge::Left | Edge::Right => Axis::Horizontal,
        Edge::Top | Edge::Bottom => Axis::Vertical,
    };
    assert_eq!(*axis, expected_axis);
    let branch_items = children
        .iter()
        .map(|child| subtree_items(workspace, *child))
        .collect::<Vec<_>>();
    let expected = match edge {
        Edge::Left => vec![vec![ITEM_B, ITEM_D], vec![ITEM_C], vec![ITEM_A]],
        Edge::Right => vec![vec![ITEM_A], vec![ITEM_B, ITEM_D], vec![ITEM_C]],
        Edge::Top => vec![vec![ITEM_B, ITEM_D, ITEM_C], vec![ITEM_A]],
        Edge::Bottom => vec![vec![ITEM_A], vec![ITEM_B, ITEM_D, ITEM_C]],
    };
    assert_eq!(branch_items, expected);
    assert_eq!(
        dockspace.engine().interaction().status(),
        InteractionStatus::Idle
    );
}

#[test]
fn floating_title_arms_the_complete_split_subtree() {
    let context = Context::default();
    let original = workspace();
    let root_node = original
        .root(FLOATING_ROOT)
        .expect("fixture floating root exists")
        .node;
    let mut dockspace = Dockspace::builder("subtree-drag", original.clone())
        .build()
        .expect("fixture facade builds");
    let mut panes = TestPanes;
    warm(&context, &mut dockspace, &mut panes);
    let title = floating_title_point(&dockspace);
    let moved = title + vec2(100.0, 45.0);

    let _ = run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![Event::PointerMoved(title), pointer_button(title, true)],
    );
    let _ = run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![Event::PointerMoved(moved)],
    );
    assert!(matches!(
        dockspace.engine().interaction().status(),
        InteractionStatus::Armed { .. }
    ));

    let _ = run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![Event::PointerMoved(moved)],
    );
    let active = dockspace
        .engine()
        .interaction()
        .active_drag_view()
        .expect("subtree drag is active");
    let MovePayload::Subtree(source) = active.payload() else {
        panic!("floating title drag must preserve the complete split subtree");
    };
    assert_eq!(source.root(), FLOATING_ROOT);
    assert_eq!(source.node(), root_node);
    assert_eq!(dockspace.engine().workspace(), &original);

    let _ = run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![Event::PointerGone],
    );
    let _ = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    assert_eq!(
        dockspace.engine().interaction().status(),
        InteractionStatus::Idle
    );
    assert_eq!(dockspace.engine().workspace(), &original);
}

#[test]
fn complete_split_root_redocks_at_the_left_guide() {
    exercise_complete_split_root_redock(Edge::Left);
}

#[test]
fn complete_split_root_redocks_at_the_right_guide() {
    exercise_complete_split_root_redock(Edge::Right);
}

#[test]
fn complete_split_root_redocks_at_the_top_guide() {
    exercise_complete_split_root_redock(Edge::Top);
}

#[test]
fn complete_split_root_redocks_at_the_bottom_guide() {
    exercise_complete_split_root_redock(Edge::Bottom);
}

#[test]
fn complete_contained_root_does_not_allocate_a_new_identity_in_its_current_host() {
    let context = Context::default();
    let original = workspace();
    let original_floating = *original
        .contained_floating(FLOATING)
        .expect("original contained presentation exists");
    let allocations = Rc::new(Cell::new(0));
    let observed_allocations = Rc::clone(&allocations);
    let mut dockspace = Dockspace::builder("contained-root-identity", original.clone())
        .tear_off_mode(TearOffMode::Contained)
        .presentation_ids(move || {
            observed_allocations.set(observed_allocations.get() + 1);
            Some(ContainedPresentationIds::new(UNUSED_ROOT, UNUSED_FLOATING))
        })
        .build()
        .expect("fixture facade builds");
    let mut panes = TestPanes;
    warm(&context, &mut dockspace, &mut panes);
    let title = floating_title_point(&dockspace);
    let outside_host = Pos2::new(550.0, 210.0);

    let _ = run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![Event::PointerMoved(title), pointer_button(title, true)],
    );
    for _ in 0..4 {
        let _ = run_frame(
            &context,
            &mut dockspace,
            &mut panes,
            vec![Event::PointerMoved(outside_host)],
        );
    }

    let active = dockspace
        .engine()
        .interaction()
        .active_drag_view()
        .expect("whole-root drag remains active");
    let TearOffRequest::Contained(proposal) = active
        .tear_off()
        .expect("whole-root title drag proposes moving its existing presentation")
    else {
        panic!("whole-root title drag must use a contained move proposal");
    };
    let proposal = *proposal;
    assert_eq!(proposal.root(), FLOATING_ROOT);
    assert_eq!(proposal.floating(), FLOATING);
    assert!(matches!(
        dockspace
            .engine()
            .interaction()
            .preview()
            .expect("contained move proposal is painted")
            .visual(),
        PreviewVisual::Contained {
            surface: SURFACE,
            rect,
            fallback: false,
        } if *rect == proposal.rect()
    ));
    assert_eq!(allocations.get(), 0);
    assert_eq!(dockspace.engine().workspace(), &original);

    let _ = run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![
            Event::PointerMoved(outside_host),
            pointer_button(outside_host, false),
        ],
    );
    let _ = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let _ = run_frame(&context, &mut dockspace, &mut panes, Vec::new());

    assert_eq!(allocations.get(), 0);
    let moved = dockspace
        .engine()
        .workspace()
        .contained_floating(FLOATING)
        .expect("existing contained presentation remains");
    assert_eq!(moved.root, FLOATING_ROOT);
    assert_eq!(moved.surface, SURFACE);
    assert_eq!(moved.rect, proposal.rect());
    assert_eq!(moved.rect.size(), original_floating.rect.size());
    assert!(dockspace.engine().workspace().root(UNUSED_ROOT).is_none());
    assert!(
        dockspace
            .engine()
            .workspace()
            .contained_floating(UNUSED_FLOATING)
            .is_none()
    );
    assert_eq!(
        dockspace.engine().workspace().item_multiset(),
        original.item_multiset()
    );
    assert_eq!(
        dockspace.engine().interaction().status(),
        InteractionStatus::Idle
    );
}
