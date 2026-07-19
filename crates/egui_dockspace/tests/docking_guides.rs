use std::collections::BTreeMap;

use dockspace::command::Edge;
use dockspace::drop_guide::{DropGuideClusterId, DropGuideSlot};
use dockspace::drop_target::DropTargetId;
use dockspace::graph::{Axis, Node, RootRecord, SurfacePresentation, Workspace};
use dockspace::ids::{ItemId, NodeId, RootId, SurfaceId};
use dockspace::interaction::{InteractionOutcome, InteractionStatus, PreviewVisual};
use dockspace::scene::SurfaceScene;
use dockspace::transition::{EngineTransition, InputOutcome};
use egui::{Context, Event, Modifiers, PointerButton, Pos2, RawInput, Rect, Ui, vec2};
use egui_dockspace::{Dockspace, PaneView};

const SURFACE: SurfaceId = SurfaceId::new(1);
const ROOT: RootId = RootId::new(2);
const ITEM_A: ItemId = ItemId::new(3);
const ITEM_B: ItemId = ItemId::new(4);
const CANONICAL_SLOTS: [DropGuideSlot; 5] = [
    DropGuideSlot::Center,
    DropGuideSlot::Edge(Edge::Left),
    DropGuideSlot::Edge(Edge::Right),
    DropGuideSlot::Edge(Edge::Top),
    DropGuideSlot::Edge(Edge::Bottom),
];

struct TestPanes;

impl PaneView for TestPanes {
    fn title(&self, item: ItemId) -> Option<egui::WidgetText> {
        match item {
            ITEM_A => Some("Left".into()),
            ITEM_B => Some("Right".into()),
            _ => None,
        }
    }

    fn ui(&mut self, _item: ItemId, _ui: &mut Ui) {}
}

struct Fixture {
    context: Context,
    dockspace: Dockspace,
    panes: TestPanes,
    left_tabs: NodeId,
}

impl Fixture {
    fn new(slot: DropGuideSlot) -> Self {
        let mut builder = Workspace::builder();
        let left_tabs = builder.insert_node(Node::tabs([ITEM_A]));
        let right_tabs = builder.insert_node(Node::tabs([ITEM_B]));
        let split = builder.insert_node(
            Node::equal_split(Axis::Horizontal, [left_tabs, right_tabs])
                .expect("fixture split is valid"),
        );
        builder.set_root(ROOT, RootRecord::new(split));
        builder.set_surface(SURFACE, SurfacePresentation::new(ROOT));
        let workspace = builder.build().expect("guide fixture is valid");
        let dockspace = Dockspace::builder(("docking-guide", slot), workspace)
            .build()
            .expect("guide facade builds");
        Self {
            context: Context::default(),
            dockspace,
            panes: TestPanes,
            left_tabs,
        }
    }

    fn run_frame(&mut self, events: Vec<Event>) -> Vec<EngineTransition> {
        let mut transitions = Vec::new();
        let _ = self.context.run_ui(input(events), |ui| {
            let response = self
                .dockspace
                .show(SURFACE, ui, &mut self.panes)
                .expect("guide frame advances");
            transitions.extend_from_slice(response.transitions());
        });
        transitions
    }

    fn warm(&mut self) {
        self.run_frame(Vec::new());
        self.run_frame(Vec::new());
    }

    fn source_tab_point(&self) -> Pos2 {
        let ready = self.ready_scene();
        let rect = ready
            .tabs()
            .iter()
            .find(|tab| tab.id().item == ITEM_B)
            .expect("right item tab is painted")
            .rect();
        logical_point(
            rect.min().x() + 8.0,
            (rect.min().y() + rect.max().y()) * 0.5,
        )
    }

    fn guide_target(&self, slot: DropGuideSlot) -> (DropGuideClusterId, DropTargetId, Pos2) {
        let cluster = self
            .ready_scene()
            .drop_guide_clusters()
            .iter()
            .find(|cluster| {
                cluster.id() == DropGuideClusterId::inner(SURFACE, ROOT, self.left_tabs)
            })
            .expect("left leaf publishes its inner guide cluster");
        assert_eq!(
            cluster
                .targets()
                .map(|(published_slot, _)| published_slot)
                .collect::<Vec<_>>(),
            CANONICAL_SLOTS,
            "sealed scene must publish the complete canonical inner guide"
        );
        let target = cluster
            .target(slot)
            .expect("requested inner guide slot is published");
        let hit = target.target().region().rect();
        (
            cluster.id(),
            target.id(),
            logical_point(
                (hit.min().x() + hit.max().x()) * 0.5,
                (hit.min().y() + hit.max().y()) * 0.5,
            ),
        )
    }

    fn ready_scene(&self) -> &dockspace::scene::ReadySurfaceScene {
        let SurfaceScene::Ready(ready) = self
            .dockspace
            .engine()
            .scene()
            .and_then(|scene| scene.surface(SURFACE))
            .expect("fixture surface scene exists")
        else {
            panic!("fixture surface scene is ready");
        };
        ready
    }
}

fn input(events: Vec<Event>) -> RawInput {
    RawInput {
        screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(800.0, 500.0))),
        events,
        ..RawInput::default()
    }
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
    reason = "finite sealed-scene coordinates intentionally become egui f32 input coordinates"
)]
fn logical_point(x: f64, y: f64) -> Pos2 {
    Pos2::new(x as f32, y as f32)
}

fn contains_outcome(
    transitions: &[EngineTransition],
    predicate: impl Fn(&InteractionOutcome) -> bool,
) -> bool {
    transitions.iter().any(|transition| {
        transition.reduced_inputs().iter().any(|input| {
            matches!(
                input.outcome(),
                InputOutcome::InteractionProcessed { outcome, .. } if predicate(outcome)
            )
        })
    })
}

fn run_guide_case(slot: DropGuideSlot) {
    let mut fixture = Fixture::new(slot);
    fixture.warm();
    let source = fixture.source_tab_point();
    let (cluster_id, target_id, target_point) = fixture.guide_target(slot);
    let original = fixture.dockspace.engine().workspace().clone();
    let expected_items = BTreeMap::from([(ITEM_A, 1), (ITEM_B, 1)]);

    fixture.run_frame(vec![
        Event::PointerMoved(source),
        pointer_button(source, true),
    ]);
    for _ in 0..4 {
        fixture.run_frame(vec![Event::PointerMoved(target_point)]);
    }

    assert!(matches!(
        fixture.dockspace.engine().interaction().status(),
        InteractionStatus::Dragging { .. }
    ));
    let affordance = fixture
        .dockspace
        .engine()
        .interaction()
        .drop_affordance()
        .expect("exact guide hit publishes an affordance");
    let inner = affordance
        .clusters()
        .iter()
        .find(|cluster| cluster.id() == cluster_id)
        .expect("affordance contains the left inner cluster");
    assert_eq!(
        inner
            .targets()
            .iter()
            .map(dockspace::drop_resolver::DropAffordanceTarget::slot)
            .collect::<Vec<_>>(),
        CANONICAL_SLOTS,
        "active affordance must retain every inner direction"
    );
    let active = affordance
        .active_target()
        .expect("exact guide button is active");
    assert_eq!(active.cluster(), cluster_id);
    assert_eq!(active.slot(), slot);
    assert_eq!(active.target_id(), target_id);
    assert!(active.eligibility().is_eligible());
    assert!(matches!(
        fixture
            .dockspace
            .engine()
            .interaction()
            .preview()
            .expect("eligible exact guide publishes a preview")
            .visual(),
        PreviewVisual::Dock { target, .. } if *target == target_id
    ));
    assert_eq!(fixture.dockspace.engine().workspace(), &original);

    let release = fixture.run_frame(vec![
        Event::PointerMoved(target_point),
        pointer_button(target_point, false),
    ]);
    assert!(
        !contains_outcome(&release, |outcome| matches!(
            outcome,
            InteractionOutcome::DragDelivered { .. }
        )),
        "release painted this frame must not reduce immediately"
    );
    assert_eq!(fixture.dockspace.engine().workspace(), &original);

    let delivery = fixture.run_frame(Vec::new());
    assert!(contains_outcome(&delivery, |outcome| matches!(
        outcome,
        InteractionOutcome::PreviewAcknowledged { .. }
    )));
    assert!(contains_outcome(&delivery, |outcome| matches!(
        outcome,
        InteractionOutcome::DragDelivered { .. }
    )));
    assert_eq!(
        fixture.dockspace.engine().workspace().item_multiset(),
        expected_items
    );
    assert_eq!(
        fixture.dockspace.engine().interaction().status(),
        InteractionStatus::Idle
    );
    assert!(
        fixture
            .dockspace
            .engine()
            .interaction()
            .drop_affordance()
            .is_none()
    );
    assert_final_topology(fixture.dockspace.engine().workspace(), slot);
}

fn assert_final_topology(workspace: &Workspace, slot: DropGuideSlot) {
    let root = workspace.root(ROOT).expect("root remains present");
    if slot == DropGuideSlot::Center {
        assert!(matches!(
            workspace.node(root.node),
            Some(Node::Tabs { items, selected })
                if items == &[ITEM_A, ITEM_B] && *selected == Some(ITEM_B)
        ));
        return;
    }

    let DropGuideSlot::Edge(edge) = slot else {
        unreachable!("center returned above");
    };
    let expected_axis = match edge {
        Edge::Left | Edge::Right => Axis::Horizontal,
        Edge::Top | Edge::Bottom => Axis::Vertical,
    };
    let Some(Node::Split { axis, children, .. }) = workspace.node(root.node) else {
        panic!("edge guide must leave a split root");
    };
    assert_eq!(*axis, expected_axis);
    assert_eq!(children.len(), 2);
    let child_items = children
        .iter()
        .map(|child| match workspace.node(*child) {
            Some(Node::Tabs { items, .. }) => items.as_slice(),
            node => panic!("edge child must be tabs, got {node:?}"),
        })
        .collect::<Vec<_>>();
    let expected = match edge {
        Edge::Left | Edge::Top => vec![&[ITEM_B][..], &[ITEM_A][..]],
        Edge::Right | Edge::Bottom => vec![&[ITEM_A][..], &[ITEM_B][..]],
    };
    assert_eq!(child_items, expected);
}

#[test]
fn center_guide_merges_the_right_item_into_the_left_tabs() {
    run_guide_case(DropGuideSlot::Center);
}

#[test]
fn left_guide_splits_before_the_left_tabs() {
    run_guide_case(DropGuideSlot::Edge(Edge::Left));
}

#[test]
fn right_guide_splits_after_the_left_tabs() {
    run_guide_case(DropGuideSlot::Edge(Edge::Right));
}

#[test]
fn top_guide_splits_above_the_left_tabs() {
    run_guide_case(DropGuideSlot::Edge(Edge::Top));
}

#[test]
fn bottom_guide_splits_below_the_left_tabs() {
    run_guide_case(DropGuideSlot::Edge(Edge::Bottom));
}
