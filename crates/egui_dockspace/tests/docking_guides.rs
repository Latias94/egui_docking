use std::collections::BTreeMap;

use dockspace::backend::interaction::{InteractionStatus, PreviewVisual};
use dockspace::backend::scene::PresentationPlan;
use dockspace::command::Edge;
use dockspace::drop_guide::{DropGuideClusterId, DropGuideSlot};
use dockspace::drop_target::DropTargetId;
use dockspace::graph::{Axis, Node, RootRecord, SurfacePresentation, Workspace};
use dockspace::ids::{ItemId, NodeId, RootId, SurfaceId};
use egui::{Context, Event, Modifiers, PointerButton, Pos2, RawInput, Rect, Ui, vec2};
use egui_dockspace::backend::{EguiFrameScheduleKey, EguiPresentationResult};
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
    next_frame_sequence: u64,
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
        builder.set_surface(SURFACE, SurfacePresentation::with_main(ROOT));
        let workspace = builder.build().expect("guide fixture is valid");
        let dockspace = Dockspace::builder(("docking-guide", slot), workspace)
            .build()
            .expect("guide facade builds");
        Self {
            context: Context::default(),
            dockspace,
            panes: TestPanes,
            left_tabs,
            next_frame_sequence: 1,
        }
    }

    fn run_frame(&mut self, events: Vec<Event>) {
        let key = EguiFrameScheduleKey::new(self.next_frame_sequence, 0);
        self.next_frame_sequence = self
            .next_frame_sequence
            .checked_add(1)
            .expect("guide fixture frame sequence must not overflow");
        let mut host = self
            .dockspace
            .begin_outer_frame(key)
            .expect("outer guide frame begins");
        host.run_surface(SURFACE, &self.context, input(events), &mut self.panes)
            .expect("outer guide host owns the complete surface run");
        let (response, outputs) = host
            .finish()
            .expect("outer guide frame commits")
            .into_parts();
        for output in outputs {
            output.settle_with(|surface, _| {
                assert_eq!(surface, SURFACE);
                EguiPresentationResult::Presented
            });
        }
        let _ = response;
    }

    fn warm(&mut self) {
        self.run_frame(Vec::new());
        self.run_frame(Vec::new());
        self.run_frame(Vec::new());
        assert!(
            self.dockspace
                .core_engine()
                .interaction_projection(SURFACE)
                .is_some(),
            "a presented outer-host frame must authorize the guide fixture"
        );
    }

    fn source_tab_point(&self) -> Pos2 {
        let ready = self.painted_plan();
        let rect = ready
            .tab_records()
            .iter()
            .find(|tab| tab.id().item == ITEM_B)
            .expect("right item tab is painted")
            .drag_hit()
            .rect();
        logical_point(
            rect.min().x() + 8.0,
            (rect.min().y() + rect.max().y()) * 0.5,
        )
    }

    fn guide_target(&self, slot: DropGuideSlot) -> (DropGuideClusterId, DropTargetId, Pos2) {
        let cluster = self
            .painted_plan()
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

    fn painted_plan(&self) -> &PresentationPlan {
        self.dockspace
            .core_engine()
            .interaction_projection(SURFACE)
            .map(dockspace::backend::scene::SurfaceInteractionProjection::plan)
            .expect("fixture surface has an acknowledged painted plan")
    }
}

fn input(events: Vec<Event>) -> RawInput {
    RawInput {
        screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(800.0, 500.0))),
        events: events.into_iter().map(Into::into).collect(),
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

fn run_guide_case(slot: DropGuideSlot) {
    let mut fixture = Fixture::new(slot);
    fixture.warm();
    let source = fixture.source_tab_point();
    let (cluster_id, target_id, target_point) = fixture.guide_target(slot);
    let original = fixture.dockspace.core_engine().workspace().clone();
    let expected_items = BTreeMap::from([(ITEM_A, 1), (ITEM_B, 1)]);

    fixture.run_frame(vec![
        Event::PointerMoved(source),
        pointer_button(source, true),
    ]);
    for _ in 0..4 {
        fixture.run_frame(vec![Event::PointerMoved(target_point)]);
    }

    assert!(matches!(
        fixture.dockspace.core_engine().interaction().status(),
        InteractionStatus::Dragging { .. }
    ));
    let affordance = fixture
        .dockspace
        .core_engine()
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
            .map(dockspace::backend::drop_resolver::DropAffordanceTarget::slot)
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
            .core_engine()
            .interaction()
            .preview()
            .expect("eligible exact guide publishes a preview")
            .visual(),
        PreviewVisual::Dock { target, .. } if *target == target_id
    ));
    assert_eq!(fixture.dockspace.core_engine().workspace(), &original);

    fixture.run_frame(vec![
        Event::PointerMoved(target_point),
        pointer_button(target_point, false),
    ]);
    assert_eq!(
        fixture.dockspace.core_engine().workspace().item_multiset(),
        expected_items
    );
    assert_eq!(
        fixture.dockspace.core_engine().interaction().status(),
        InteractionStatus::Idle
    );
    assert!(
        fixture
            .dockspace
            .core_engine()
            .interaction()
            .drop_affordance()
            .is_none()
    );
    assert_final_topology(fixture.dockspace.core_engine().workspace(), slot);
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
