use std::collections::BTreeMap;

use dockspace::command::Edge;
use dockspace::drop_guide::{DropGuideClusterId, DropGuideSlot};
use dockspace::drop_resolver::DropAffordanceTarget;
use dockspace::drop_target::DropTargetId;
use dockspace::geometry::{LogicalPoint, LogicalRect};
use dockspace::graph::{Axis, ContainedFloating, Node, RootRecord, SurfacePresentation, Workspace};
use dockspace::ids::{FloatingPresentationId, ItemId, NodeId, RootId, SurfaceId};
use dockspace::interaction::{
    InteractionDelivery, InteractionOutcome, InteractionStatus, PreviewVisual,
    WorkspaceDeliveryKind,
};
use dockspace::policy::DockPolicy;
use dockspace::scene::{ReadySurfaceScene, SurfaceScene};
use dockspace::transition::{EngineTransition, InputOutcome};
use egui::{Context, Event, Modifiers, PointerButton, Pos2, RawInput, Rect, Ui, vec2};
use egui_dockspace::{Dockspace, PaneView, TearOffMode};

const SURFACE: SurfaceId = SurfaceId::new(1);
const MAIN_ROOT: RootId = RootId::new(10);
const FLOATING_ROOT: RootId = RootId::new(11);
const FLOATING: FloatingPresentationId = FloatingPresentationId::new(20);
const MAIN_LEFT: ItemId = ItemId::new(100);
const MAIN_RIGHT: ItemId = ItemId::new(101);
const FLOAT_A: ItemId = ItemId::new(102);
const FLOAT_B: ItemId = ItemId::new(103);
const SURFACE_WIDTH: f32 = 800.0;
const SURFACE_HEIGHT: f32 = 560.0;
const CANONICAL_INNER_SLOTS: [DropGuideSlot; 5] = [
    DropGuideSlot::Center,
    DropGuideSlot::Edge(Edge::Left),
    DropGuideSlot::Edge(Edge::Right),
    DropGuideSlot::Edge(Edge::Top),
    DropGuideSlot::Edge(Edge::Bottom),
];

fn initial_rect() -> LogicalRect {
    LogicalRect::new(520.0, 60.0, 240.0, 180.0).expect("fixture rectangle must be valid")
}

struct TestPanes;

impl PaneView for TestPanes {
    fn title(&self, item: ItemId) -> Option<egui::WidgetText> {
        Some(
            match item {
                MAIN_LEFT => "Main left",
                MAIN_RIGHT => "Main right",
                FLOAT_A => "Floating A",
                FLOAT_B => "Floating B",
                _ => return None,
            }
            .into(),
        )
    }

    fn ui(&mut self, _item: ItemId, _ui: &mut Ui) {}
}

struct Fixture {
    context: Context,
    dockspace: Dockspace,
    panes: TestPanes,
    main_split: NodeId,
    main_left: NodeId,
    main_right: NodeId,
    floating_tabs: NodeId,
}

impl Fixture {
    fn new(name: &'static str, policy: DockPolicy) -> Self {
        Self::with_mode(name, policy, TearOffMode::Contained)
    }

    fn with_mode(name: &'static str, policy: DockPolicy, tear_off_mode: TearOffMode) -> Self {
        let mut builder = Workspace::builder();
        let main_left = builder.insert_node(Node::tabs([MAIN_LEFT]));
        let main_right = builder.insert_node(Node::tabs([MAIN_RIGHT]));
        let main_split = builder.insert_node(
            Node::equal_split(Axis::Horizontal, [main_left, main_right])
                .expect("main fixture split is valid"),
        );
        let floating_tabs = builder.insert_node(Node::tabs([FLOAT_A, FLOAT_B]));
        builder.set_root(MAIN_ROOT, RootRecord::new(main_split));
        builder.set_root(FLOATING_ROOT, RootRecord::new(floating_tabs));
        builder.set_surface(SURFACE, SurfacePresentation::new(MAIN_ROOT));
        builder.set_contained_floating(ContainedFloating::new(
            FLOATING,
            FLOATING_ROOT,
            SURFACE,
            initial_rect(),
            1,
        ));
        builder
            .attach_contained(SURFACE, FLOATING)
            .expect("fixture surface exists");
        let workspace = builder.build().expect("title-drag fixture is valid");
        let dockspace = Dockspace::builder(name, workspace)
            .policy(policy)
            .tear_off_mode(tear_off_mode)
            .build()
            .expect("title-drag facade builds");
        Self {
            context: Context::default(),
            dockspace,
            panes: TestPanes,
            main_split,
            main_left,
            main_right,
            floating_tabs,
        }
    }

    fn run_frame(&mut self, events: Vec<Event>) -> Vec<EngineTransition> {
        let mut transitions = Vec::new();
        let _ = self.context.run_ui(input(events), |ui| {
            let response = self
                .dockspace
                .show(SURFACE, ui, &mut self.panes)
                .expect("title-drag frame advances");
            transitions.extend_from_slice(response.transitions());
        });
        transitions
    }

    fn warm(&mut self) {
        self.run_frame(Vec::new());
        self.run_frame(Vec::new());
    }

    fn title_point(&self) -> Pos2 {
        let floating = self.floating();
        let style = self.dockspace.style();
        logical_pos(
            floating.rect.min().x()
                + f64::from(style.floating_border_width + style.floating_resize_extent + 48.0),
            floating.rect.min().y()
                + f64::from(style.floating_border_width + style.floating_title_height * 0.5),
        )
    }

    fn splitter_point(&self) -> Pos2 {
        let splitter = self
            .ready_scene()
            .splitters()
            .iter()
            .find(|splitter| {
                splitter.id().root == MAIN_ROOT && splitter.id().split == self.main_split
            })
            .expect("main splitter is published")
            .rect();
        let point = LogicalPoint::new(
            (splitter.min().x() + splitter.max().x()) * 0.5,
            (splitter.min().y() + splitter.max().y()) * 0.5,
        )
        .expect("splitter center is finite");
        assert!(
            self.ready_scene()
                .drop_guide_clusters()
                .iter()
                .flat_map(|cluster| cluster.targets().map(|(_, target)| target))
                .all(|target| !target.target().region().contains(point))
        );
        assert!(
            self.ready_scene()
                .drop_targets()
                .iter()
                .all(|target| !target.region().contains(point))
        );
        logical_pos(point.x(), point.y())
    }

    fn inner_activation_only_point(&self) -> Pos2 {
        let cluster = self.main_left_cluster();
        let activation = cluster.activation().rect();
        let candidates = [
            (activation.max().x() - 24.0, activation.min().y() + 48.0),
            (activation.max().x() - 24.0, activation.max().y() - 48.0),
            (activation.min().x() + 24.0, activation.min().y() + 48.0),
        ];
        let point = candidates
            .into_iter()
            .filter_map(|(x, y)| LogicalPoint::new(x, y).ok())
            .find(|point| {
                cluster.activation().contains(*point)
                    && cluster
                        .targets()
                        .all(|(_, target)| !target.target().region().contains(*point))
                    && self
                        .ready_scene()
                        .drop_targets()
                        .iter()
                        .all(|target| !target.region().contains(*point))
            })
            .expect("inner activation contains a declared non-button point");
        logical_pos(point.x(), point.y())
    }

    fn exact_guide(&self, slot: DropGuideSlot) -> (DropTargetId, Pos2) {
        let target = self
            .main_left_cluster()
            .target(slot)
            .expect("requested main inner guide exists");
        let hit = target.target().region().rect();
        (
            target.id(),
            logical_pos(
                (hit.min().x() + hit.max().x()) * 0.5,
                (hit.min().y() + hit.max().y()) * 0.5,
            ),
        )
    }

    fn main_left_cluster(&self) -> &dockspace::drop_guide::DropGuideClusterRecord {
        let cluster = self
            .ready_scene()
            .drop_guide_clusters()
            .iter()
            .find(|cluster| {
                cluster.id() == DropGuideClusterId::inner(SURFACE, MAIN_ROOT, self.main_left)
            })
            .expect("main left inner guide is published");
        assert_eq!(
            cluster.targets().map(|(slot, _)| slot).collect::<Vec<_>>(),
            CANONICAL_INNER_SLOTS
        );
        cluster
    }

    fn ready_scene(&self) -> &ReadySurfaceScene {
        let SurfaceScene::Ready(ready) = self
            .dockspace
            .engine()
            .scene()
            .and_then(|scene| scene.surface(SURFACE))
            .expect("fixture scene exists")
        else {
            panic!("fixture scene is ready");
        };
        ready
    }

    fn floating(&self) -> &ContainedFloating {
        self.dockspace
            .engine()
            .workspace()
            .contained_floating(FLOATING)
            .expect("contained presentation remains present")
    }

    fn drag_to_preview(&mut self, target: Pos2) -> Pos2 {
        let source = self.title_point();
        self.run_frame(vec![
            Event::PointerMoved(source),
            pointer_button(source, true),
        ]);
        for _ in 0..4 {
            self.run_frame(vec![Event::PointerMoved(target)]);
        }
        assert!(matches!(
            self.dockspace.engine().interaction().status(),
            InteractionStatus::Dragging { .. }
        ));
        source
    }

    fn release_frame(&mut self, target: Pos2) -> Vec<EngineTransition> {
        self.run_frame(vec![
            Event::PointerMoved(target),
            pointer_button(target, false),
        ])
    }
}

fn input(events: Vec<Event>) -> RawInput {
    RawInput {
        screen_rect: Some(Rect::from_min_size(
            Pos2::ZERO,
            vec2(SURFACE_WIDTH, SURFACE_HEIGHT),
        )),
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
fn logical_pos(x: f64, y: f64) -> Pos2 {
    Pos2::new(x as f32, y as f32)
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

fn contains_acknowledgement(transitions: &[EngineTransition]) -> bool {
    interaction_outcomes(transitions)
        .any(|outcome| matches!(outcome, InteractionOutcome::PreviewAcknowledged { .. }))
}

fn contains_delivery(transitions: &[EngineTransition], expected: WorkspaceDeliveryKind) -> bool {
    interaction_outcomes(transitions).any(|outcome| {
        matches!(
            outcome,
            InteractionOutcome::DragDelivered {
                delivery: InteractionDelivery::Workspace { kind, .. },
                ..
            } if *kind == expected
        )
    })
}

fn expected_translation(rect: LogicalRect, source: Pos2, target: Pos2) -> LogicalRect {
    let delta_x = f64::from(target.x) - f64::from(source.x);
    let delta_y = f64::from(target.y) - f64::from(source.y);
    LogicalRect::new(
        rect.x() + delta_x,
        rect.y() + delta_y,
        rect.width(),
        rect.height(),
    )
    .expect("fixture translation remains inside the host bounds")
}

fn assert_move_preview(fixture: &Fixture, expected: LogicalRect) {
    assert!(matches!(
        fixture
            .dockspace
            .engine()
            .interaction()
            .preview()
            .expect("move fallback publishes a preview")
            .visual(),
        PreviewVisual::Contained {
            surface: SURFACE,
            rect,
            fallback: false,
        } if *rect == expected
    ));
}

fn assert_move_delivery(
    fixture: &mut Fixture,
    target: Pos2,
    original: &Workspace,
    expected: LogicalRect,
) {
    let release = fixture.release_frame(target);
    assert!(!contains_delivery(
        &release,
        WorkspaceDeliveryKind::Contained
    ));
    assert_eq!(fixture.dockspace.engine().workspace(), original);
    let delivery = fixture.run_frame(Vec::new());
    assert!(contains_acknowledgement(&delivery));
    assert!(contains_delivery(
        &delivery,
        WorkspaceDeliveryKind::Contained
    ));
    let moved = fixture.floating();
    let original_floating = original
        .contained_floating(FLOATING)
        .expect("original contained presentation exists");
    assert_eq!(moved.id, original_floating.id);
    assert_eq!(moved.root, original_floating.root);
    assert_eq!(moved.surface, original_floating.surface);
    assert_eq!(moved.z_order, original_floating.z_order);
    assert_eq!(moved.rect, expected);
    assert_eq!(moved.rect.size(), original_floating.rect.size());
    assert_eq!(
        fixture
            .dockspace
            .engine()
            .workspace()
            .root(FLOATING_ROOT)
            .expect("floating root identity remains present")
            .node,
        fixture.floating_tabs
    );
    assert_eq!(
        fixture.dockspace.engine().interaction().status(),
        InteractionStatus::Idle
    );
}

fn item_multiset() -> BTreeMap<ItemId, usize> {
    BTreeMap::from([(MAIN_LEFT, 1), (MAIN_RIGHT, 1), (FLOAT_A, 1), (FLOAT_B, 1)])
}

#[test]
fn title_drag_without_an_exact_button_moves_with_frozen_size_and_pointer_offset() {
    let mut fixture = Fixture::new("contained-title-move", DockPolicy::default());
    fixture.warm();
    let original = fixture.dockspace.engine().workspace().clone();
    let target = fixture.splitter_point();
    let source = fixture.drag_to_preview(target);
    let expected = expected_translation(initial_rect(), source, target);

    assert_move_preview(&fixture, expected);
    assert!(
        fixture
            .dockspace
            .engine()
            .interaction()
            .drop_affordance()
            .is_some()
    );
    assert_eq!(fixture.dockspace.engine().workspace(), &original);
    assert_move_delivery(&mut fixture, target, &original, expected);
}

#[test]
fn existing_contained_title_moves_when_new_tear_off_is_disabled() {
    let mut fixture = Fixture::with_mode(
        "contained-title-disabled-tear-off",
        DockPolicy::default(),
        TearOffMode::Disabled,
    );
    fixture.warm();
    let original = fixture.dockspace.engine().workspace().clone();
    let target = fixture.splitter_point();
    let source = fixture.drag_to_preview(target);
    let expected = expected_translation(initial_rect(), source, target);

    assert_move_preview(&fixture, expected);
    assert_move_delivery(&mut fixture, target, &original, expected);
}

#[test]
fn inner_activation_without_a_button_keeps_the_affordance_and_moves() {
    let mut fixture = Fixture::new("contained-title-activation", DockPolicy::default());
    fixture.warm();
    let original = fixture.dockspace.engine().workspace().clone();
    let target = fixture.inner_activation_only_point();
    let source = fixture.drag_to_preview(target);
    let expected = expected_translation(initial_rect(), source, target);

    assert_move_preview(&fixture, expected);
    let affordance = fixture
        .dockspace
        .engine()
        .interaction()
        .drop_affordance()
        .expect("inner activation publishes guide affordance");
    let inner = affordance
        .clusters()
        .iter()
        .find(|cluster| {
            cluster.id() == DropGuideClusterId::inner(SURFACE, MAIN_ROOT, fixture.main_left)
        })
        .expect("main inner cluster remains visible during move fallback");
    assert_eq!(
        inner
            .targets()
            .iter()
            .map(DropAffordanceTarget::slot)
            .collect::<Vec<_>>(),
        CANONICAL_INNER_SLOTS
    );
    assert!(affordance.active_target().is_none());
    assert_move_delivery(&mut fixture, target, &original, expected);
}

fn exercise_exact_dock(slot: DropGuideSlot) {
    let mut fixture = Fixture::new("contained-title-dock", DockPolicy::default());
    fixture.warm();
    let original = fixture.dockspace.engine().workspace().clone();
    let (target_id, target) = fixture.exact_guide(slot);
    fixture.drag_to_preview(target);

    let affordance = fixture
        .dockspace
        .engine()
        .interaction()
        .drop_affordance()
        .expect("exact guide publishes affordance");
    let active = affordance
        .active_target()
        .expect("exact guide button is active");
    assert_eq!(active.slot(), slot);
    assert_eq!(active.target_id(), target_id);
    assert!(active.eligibility().is_eligible());
    assert!(matches!(
        fixture
            .dockspace
            .engine()
            .interaction()
            .preview()
            .expect("eligible exact guide publishes dock preview")
            .visual(),
        PreviewVisual::Dock { target, .. } if *target == target_id
    ));

    let release = fixture.release_frame(target);
    assert!(!contains_delivery(&release, WorkspaceDeliveryKind::Dock));
    assert_eq!(fixture.dockspace.engine().workspace(), &original);
    let delivery = fixture.run_frame(Vec::new());
    assert!(contains_acknowledgement(&delivery));
    assert!(contains_delivery(&delivery, WorkspaceDeliveryKind::Dock));
    let workspace = fixture.dockspace.engine().workspace();
    assert!(workspace.contained_floating(FLOATING).is_none());
    assert!(workspace.root(FLOATING_ROOT).is_none());
    assert_eq!(workspace.item_multiset(), item_multiset());
    assert_exact_dock_topology(&fixture, slot);
}

fn assert_exact_dock_topology(fixture: &Fixture, slot: DropGuideSlot) {
    let workspace = fixture.dockspace.engine().workspace();
    let main = workspace
        .root(MAIN_ROOT)
        .expect("main root remains present");
    let Some(Node::Split {
        axis: Axis::Horizontal,
        children,
        ..
    }) = workspace.node(main.node)
    else {
        panic!("main root remains a horizontal split");
    };
    assert_eq!(children.len(), 2);
    assert_eq!(children[1], fixture.main_right);
    match slot {
        DropGuideSlot::Center => assert!(matches!(
            workspace.node(children[0]),
            Some(Node::Tabs { items, selected })
                if items == &[MAIN_LEFT, FLOAT_A, FLOAT_B] && *selected == Some(FLOAT_A)
        )),
        DropGuideSlot::Edge(edge @ (Edge::Top | Edge::Bottom)) => {
            let Some(Node::Split {
                axis: Axis::Vertical,
                children,
                ..
            }) = workspace.node(children[0])
            else {
                panic!("vertical exact guide wraps the main left branch");
            };
            let items = children
                .iter()
                .map(|node| match workspace.node(*node) {
                    Some(Node::Tabs { items, .. }) => items.as_slice(),
                    node => panic!("vertical child must be tabs, got {node:?}"),
                })
                .collect::<Vec<_>>();
            let expected = match edge {
                Edge::Top => vec![&[FLOAT_A, FLOAT_B][..], &[MAIN_LEFT][..]],
                Edge::Bottom => vec![&[MAIN_LEFT][..], &[FLOAT_A, FLOAT_B][..]],
                Edge::Left | Edge::Right => unreachable!("test uses vertical slots"),
            };
            assert_eq!(items, expected);
        }
        DropGuideSlot::Edge(Edge::Left | Edge::Right) => {
            unreachable!("test uses center and vertical slots")
        }
    }
}

#[test]
fn exact_center_docks_instead_of_moving() {
    exercise_exact_dock(DropGuideSlot::Center);
}

#[test]
fn exact_top_docks_instead_of_moving() {
    exercise_exact_dock(DropGuideSlot::Edge(Edge::Top));
}

#[test]
fn exact_bottom_docks_instead_of_moving() {
    exercise_exact_dock(DropGuideSlot::Edge(Edge::Bottom));
}

#[test]
fn rejected_exact_button_blocks_the_move_fallback() {
    let mut policy = DockPolicy::default();
    policy.set_allow_edge_split(false);
    let mut fixture = Fixture::new("contained-title-rejected", policy);
    fixture.warm();
    let original = fixture.dockspace.engine().workspace().clone();
    let (target_id, target) = fixture.exact_guide(DropGuideSlot::Edge(Edge::Top));
    fixture.drag_to_preview(target);

    let active = fixture
        .dockspace
        .engine()
        .interaction()
        .drop_affordance()
        .and_then(dockspace::drop_resolver::DropAffordance::active_target)
        .expect("rejected exact guide remains active and visible");
    assert_eq!(active.target_id(), target_id);
    assert_eq!(active.slot(), DropGuideSlot::Edge(Edge::Top));
    assert!(!active.eligibility().is_eligible());
    assert!(fixture.dockspace.engine().interaction().preview().is_none());

    let release = fixture.release_frame(target);
    assert_eq!(fixture.dockspace.engine().workspace(), &original);
    let settled = fixture.run_frame(Vec::new());
    assert!(
        !interaction_outcomes(&release)
            .chain(interaction_outcomes(&settled))
            .any(|outcome| matches!(outcome, InteractionOutcome::DragDelivered { .. }))
    );
    assert_eq!(fixture.dockspace.engine().workspace(), &original);
    assert_eq!(
        fixture.dockspace.engine().interaction().status(),
        InteractionStatus::Idle
    );
}
