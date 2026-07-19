use std::collections::BTreeMap;

use dockspace::command::CommandOutcome;
use dockspace::drop_guide::{DropGuideScope, DropGuideSlot};
use dockspace::drop_target::DropTargetId;
use dockspace::geometry::LogicalRect;
use dockspace::graph::{ContainedFloating, Node, RootRecord, SurfacePresentation, Workspace};
use dockspace::ids::{FloatingPresentationId, ItemId, NodeId, RootId, SurfaceId};
use dockspace::interaction::{
    InteractionDelivery, InteractionOutcome, InteractionStatus, PreviewVisual,
    WorkspaceDeliveryKind,
};
use dockspace::scene::{ReadySurfaceScene, SurfaceScene};
use dockspace::transition::{EngineTransition, InputOutcome};
use egui::{Context, Event, Modifiers, PointerButton, Pos2, RawInput, Rect, Ui, vec2};
use egui_dockspace::{Dockspace, PaneView};

const SURFACE: SurfaceId = SurfaceId::new(1);
const MAIN_ROOT: RootId = RootId::new(10);
const FLOATING_A_ROOT: RootId = RootId::new(20);
const FLOATING_A: FloatingPresentationId = FloatingPresentationId::new(21);
const FLOATING_B_ROOT: RootId = RootId::new(30);
const FLOATING_B: FloatingPresentationId = FloatingPresentationId::new(31);
const MAIN_A: ItemId = ItemId::new(100);
const MAIN_B: ItemId = ItemId::new(101);
const FLOAT_A: ItemId = ItemId::new(102);
const FLOAT_B: ItemId = ItemId::new(103);
const FLOAT_C: ItemId = ItemId::new(104);
const FLOAT_D: ItemId = ItemId::new(105);
const TAB_A: ItemId = ItemId::new(200);
const TAB_B: ItemId = ItemId::new(201);
const TAB_C: ItemId = ItemId::new(202);

struct TestPanes;

impl PaneView for TestPanes {
    fn title(&self, item: ItemId) -> Option<egui::WidgetText> {
        Some(format!("Pane {}", item.get()).into())
    }

    fn ui(&mut self, _item: ItemId, _ui: &mut Ui) {}
}

struct Harness {
    context: Context,
    dockspace: Dockspace,
    panes: TestPanes,
}

struct DragDeliveryTransitions {
    release: Vec<EngineTransition>,
    delivery: Vec<EngineTransition>,
}

impl Harness {
    fn new(name: &'static str, workspace: Workspace) -> Self {
        Self {
            context: Context::default(),
            dockspace: Dockspace::builder(name, workspace)
                .build()
                .expect("gesture fixture builds"),
            panes: TestPanes,
        }
    }

    fn run_frame(&mut self, events: Vec<Event>) -> Vec<EngineTransition> {
        let mut transitions = Vec::new();
        let _ = self.context.run_ui(input(events), |ui| {
            let response = self
                .dockspace
                .show(SURFACE, ui, &mut self.panes)
                .expect("gesture frame advances");
            transitions.extend_from_slice(response.transitions());
        });
        transitions
    }

    fn warm(&mut self) {
        let _ = self.run_frame(Vec::new());
        let _ = self.run_frame(Vec::new());
    }

    fn ready_scene(&self) -> &ReadySurfaceScene {
        let SurfaceScene::Ready(ready) = self
            .dockspace
            .engine()
            .scene()
            .and_then(|scene| scene.surface(SURFACE))
            .expect("gesture fixture scene exists")
        else {
            panic!("gesture fixture scene is ready");
        };
        ready
    }

    fn tab_point(&self, root: RootId, item: ItemId) -> Pos2 {
        let rect = self
            .ready_scene()
            .tabs()
            .iter()
            .find(|tab| tab.id().root == root && tab.id().item == item)
            .expect("requested tab is published")
            .rect();
        logical_point(
            rect.min().x() + 8.0,
            (rect.min().y() + rect.max().y()) * 0.5,
        )
    }

    fn center_guide(&self, root: RootId) -> (DropTargetId, Pos2) {
        let target = self
            .ready_scene()
            .drop_guide_clusters()
            .iter()
            .find(|cluster| {
                cluster.id().root == root && matches!(cluster.id().scope, DropGuideScope::Inner(_))
            })
            .and_then(|cluster| cluster.target(DropGuideSlot::Center))
            .expect("requested root center guide is published")
            .target();
        (target.id(), logical_rect_center(target.region().rect()))
    }

    fn tab_gap(&self, root: RootId, tabs: NodeId, index: usize) -> (DropTargetId, Pos2) {
        let target = self
            .ready_scene()
            .drop_targets()
            .iter()
            .find(|target| {
                target.id()
                    == (DropTargetId::TabGap {
                        surface: SURFACE,
                        root,
                        tabs,
                        index,
                    })
            })
            .expect("requested exact tab gap is published");
        (target.id(), logical_rect_center(target.region().rect()))
    }

    fn drag_and_deliver(
        &mut self,
        source: Pos2,
        exact_target: DropTargetId,
        target_point: Pos2,
    ) -> DragDeliveryTransitions {
        let _ = self.run_frame(vec![
            Event::PointerMoved(source),
            pointer_button(source, true),
        ]);
        for _ in 0..4 {
            let _ = self.run_frame(vec![Event::PointerMoved(target_point)]);
        }
        assert!(matches!(
            self.dockspace.engine().interaction().status(),
            InteractionStatus::Dragging { .. }
        ));
        let preview = self
            .dockspace
            .engine()
            .interaction()
            .preview()
            .expect("exact target publishes a preview");
        assert!(matches!(
            preview.visual(),
            PreviewVisual::Dock { target, .. } if *target == exact_target
        ));
        let preview_session = preview.token().session();
        let before_release = self.dockspace.engine().workspace().clone();
        let release = self.run_frame(vec![
            Event::PointerMoved(target_point),
            pointer_button(target_point, false),
        ]);
        assert_eq!(self.dockspace.engine().workspace(), &before_release);
        assert!(
            !interaction_outcomes(&release)
                .any(|outcome| matches!(outcome, InteractionOutcome::DragDelivered { .. }))
        );

        let delivery = self.run_frame(Vec::new());
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
                InteractionOutcome::DragDelivered { session: delivered, .. },
            ] if *acknowledged == preview_session && *delivered == preview_session
        ));
        assert_eq!(
            self.dockspace.engine().interaction().status(),
            InteractionStatus::Idle
        );
        DragDeliveryTransitions { release, delivery }
    }
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

fn input(events: Vec<Event>) -> RawInput {
    RawInput {
        screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(900.0, 600.0))),
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

fn logical_rect_center(rect: LogicalRect) -> Pos2 {
    logical_point(
        (rect.min().x() + rect.max().x()) * 0.5,
        (rect.min().y() + rect.max().y()) * 0.5,
    )
}

fn cross_presentation_workspace() -> Workspace {
    let mut builder = Workspace::builder();
    let main = builder.insert_node(Node::tabs_with_selection([MAIN_A, MAIN_B], Some(MAIN_A)));
    let floating_a =
        builder.insert_node(Node::tabs_with_selection([FLOAT_A, FLOAT_B], Some(FLOAT_B)));
    let floating_b =
        builder.insert_node(Node::tabs_with_selection([FLOAT_C, FLOAT_D], Some(FLOAT_D)));
    builder.set_root(MAIN_ROOT, RootRecord::new(main));
    builder.set_root(FLOATING_A_ROOT, RootRecord::new(floating_a));
    builder.set_root(FLOATING_B_ROOT, RootRecord::new(floating_b));
    builder.set_surface(SURFACE, SurfacePresentation::new(MAIN_ROOT));
    builder.set_contained_floating(ContainedFloating::new(
        FLOATING_A,
        FLOATING_A_ROOT,
        SURFACE,
        LogicalRect::new(40.0, 180.0, 340.0, 240.0).expect("fixture rect is valid"),
        1,
    ));
    builder.set_contained_floating(ContainedFloating::new(
        FLOATING_B,
        FLOATING_B_ROOT,
        SURFACE,
        LogicalRect::new(500.0, 180.0, 340.0, 240.0).expect("fixture rect is valid"),
        2,
    ));
    builder
        .attach_contained(SURFACE, FLOATING_A)
        .expect("fixture surface exists");
    builder
        .attach_contained(SURFACE, FLOATING_B)
        .expect("fixture surface exists");
    builder
        .build()
        .expect("cross-presentation fixture is valid")
}

fn root_tabs(workspace: &Workspace, root: RootId) -> (NodeId, &[ItemId], Option<ItemId>) {
    let node = workspace.root(root).expect("root remains present").node;
    let Some(Node::Tabs { items, selected }) = workspace.node(node) else {
        panic!("fixture root remains a tabs node");
    };
    (node, items, *selected)
}

#[test]
fn exact_guides_route_items_from_main_to_contained_and_between_contained_roots() {
    let original = cross_presentation_workspace();
    let expected_items = original.item_multiset();
    let mut harness = Harness::new("cross-presentation-item-routing", original);
    harness.warm();

    let main_source = harness.tab_point(MAIN_ROOT, MAIN_B);
    let (destination_target, destination_point) = harness.center_guide(FLOATING_A_ROOT);
    let main_to_contained =
        harness.drag_and_deliver(main_source, destination_target, destination_point);
    assert!(
        !interaction_outcomes(&main_to_contained.release)
            .any(|outcome| matches!(outcome, InteractionOutcome::DragDelivered { .. }))
    );
    assert!(
        interaction_outcomes(&main_to_contained.delivery).any(|outcome| matches!(
            outcome,
            InteractionOutcome::DragDelivered {
                delivery: InteractionDelivery::Workspace {
                    kind: WorkspaceDeliveryKind::Dock,
                    changed: true,
                    ..
                },
                ..
            }
        ))
    );

    let workspace = harness.dockspace.engine().workspace();
    assert_eq!(workspace.item_multiset(), expected_items);
    assert_eq!(
        (
            root_tabs(workspace, MAIN_ROOT).1,
            root_tabs(workspace, MAIN_ROOT).2
        ),
        (&[MAIN_A][..], Some(MAIN_A))
    );
    let (_, floating_a_items, floating_a_selected) = root_tabs(workspace, FLOATING_A_ROOT);
    assert_eq!(
        (floating_a_items, floating_a_selected),
        (&[FLOAT_A, FLOAT_B, MAIN_B][..], Some(MAIN_B))
    );
    assert_eq!(
        workspace
            .contained_floating(FLOATING_A)
            .map(|record| record.root),
        Some(FLOATING_A_ROOT)
    );
    assert_eq!(
        workspace
            .contained_floating(FLOATING_B)
            .map(|record| record.root),
        Some(FLOATING_B_ROOT)
    );

    harness.warm();
    let floating_a_source = harness.tab_point(FLOATING_A_ROOT, MAIN_B);
    let (destination_target, destination_point) = harness.center_guide(FLOATING_B_ROOT);
    let contained_to_contained =
        harness.drag_and_deliver(floating_a_source, destination_target, destination_point);
    assert!(
        interaction_outcomes(&contained_to_contained.delivery).any(|outcome| matches!(
            outcome,
            InteractionOutcome::DragDelivered {
                delivery: InteractionDelivery::Workspace {
                    kind: WorkspaceDeliveryKind::Dock,
                    changed: true,
                    ..
                },
                ..
            }
        ))
    );

    let workspace = harness.dockspace.engine().workspace();
    assert_eq!(workspace.item_multiset(), expected_items);
    assert_eq!(
        (
            root_tabs(workspace, FLOATING_A_ROOT).1,
            root_tabs(workspace, FLOATING_A_ROOT).2
        ),
        (&[FLOAT_A, FLOAT_B][..], Some(FLOAT_B))
    );
    assert_eq!(
        (
            root_tabs(workspace, FLOATING_B_ROOT).1,
            root_tabs(workspace, FLOATING_B_ROOT).2
        ),
        (&[FLOAT_C, FLOAT_D, MAIN_B][..], Some(MAIN_B))
    );
    let surface = workspace.surface(SURFACE).expect("surface remains present");
    assert_eq!(surface.main_root, MAIN_ROOT);
    assert_eq!(surface.contained, vec![FLOATING_A, FLOATING_B]);
}

fn tab_stack_workspace() -> Workspace {
    let mut builder = Workspace::builder();
    let tabs = builder.insert_node(Node::tabs([TAB_A, TAB_B, TAB_C]));
    builder.set_root(MAIN_ROOT, RootRecord::new(tabs));
    builder.set_surface(SURFACE, SurfacePresentation::new(MAIN_ROOT));
    builder.build().expect("tab-stack fixture is valid")
}

fn exercise_same_stack_reorder(source: ItemId, gap: usize, expected: &[ItemId]) {
    let original = tab_stack_workspace();
    let original_selection = root_tabs(&original, MAIN_ROOT).2;
    let expected_multiset = BTreeMap::from([(TAB_A, 1), (TAB_B, 1), (TAB_C, 1)]);
    let mut harness = Harness::new("same-stack-reorder", original);
    harness.warm();
    let (tabs, _, _) = root_tabs(harness.dockspace.engine().workspace(), MAIN_ROOT);
    let source_point = harness.tab_point(MAIN_ROOT, source);
    let (exact_target, target_point) = harness.tab_gap(MAIN_ROOT, tabs, gap);
    let delivery = harness.drag_and_deliver(source_point, exact_target, target_point);
    let expected_changed = expected != [TAB_A, TAB_B, TAB_C];
    assert!(
        interaction_outcomes(&delivery.delivery).any(|outcome| matches!(
            outcome,
            InteractionOutcome::DragDelivered {
                delivery: InteractionDelivery::Workspace {
                    kind: WorkspaceDeliveryKind::Dock,
                    outcome: CommandOutcome::Moved { changed, .. },
                    changed: delivery_changed,
                },
                ..
            } if *changed == expected_changed && *delivery_changed == expected_changed
        ))
    );

    let workspace = harness.dockspace.engine().workspace();
    assert_eq!(workspace.item_multiset(), expected_multiset);
    let (_, items, selected) = root_tabs(workspace, MAIN_ROOT);
    assert_eq!(items, expected);
    assert_eq!(selected, original_selection);
}

#[test]
fn same_stack_exact_gap_moves_first_to_last() {
    exercise_same_stack_reorder(TAB_A, 3, &[TAB_B, TAB_C, TAB_A]);
}

#[test]
fn same_stack_exact_gap_moves_last_to_first() {
    exercise_same_stack_reorder(TAB_C, 0, &[TAB_C, TAB_A, TAB_B]);
}

#[test]
fn same_stack_adjacent_gap_is_a_no_op() {
    exercise_same_stack_reorder(TAB_A, 1, &[TAB_A, TAB_B, TAB_C]);
}

#[allow(
    clippy::cast_possible_truncation,
    reason = "finite sealed-scene coordinates intentionally become egui f32 input coordinates"
)]
fn tab_close_rect(harness: &Harness, item: ItemId) -> Rect {
    let tab = harness
        .ready_scene()
        .tabs()
        .iter()
        .find(|tab| tab.id().item == item)
        .expect("requested tab exists")
        .rect();
    let style = harness.dockspace.style();
    let close_size = f64::from(style.tab_close_size)
        .min(tab.width())
        .min(tab.height());
    Rect::from_center_size(
        Pos2::new(
            (tab.max().x() - f64::from(style.tab_horizontal_padding) - close_size * 0.5) as f32,
            ((tab.min().y() + tab.max().y()) * 0.5) as f32,
        ),
        egui::Vec2::splat(close_size as f32),
    )
}

#[test]
fn pressing_the_close_rect_never_arms_a_tab_drag() {
    let original = tab_stack_workspace();
    let mut harness = Harness::new("close-rect-does-not-drag", original.clone());
    harness.warm();
    let close = tab_close_rect(&harness, TAB_B).center();
    let (tabs, _, _) = root_tabs(harness.dockspace.engine().workspace(), MAIN_ROOT);
    let (_, gap) = harness.tab_gap(MAIN_ROOT, tabs, 3);

    let _ = harness.run_frame(vec![
        Event::PointerMoved(close),
        pointer_button(close, true),
    ]);
    for _ in 0..4 {
        let _ = harness.run_frame(vec![Event::PointerMoved(gap)]);
        assert_eq!(
            harness.dockspace.engine().interaction().status(),
            InteractionStatus::Idle
        );
    }
    let _ = harness.run_frame(vec![Event::PointerMoved(gap), pointer_button(gap, false)]);
    let _ = harness.run_frame(Vec::new());

    assert_eq!(harness.dockspace.engine().workspace(), &original);
    assert_eq!(
        harness.dockspace.engine().interaction().status(),
        InteractionStatus::Idle
    );
}
