//! Canonical edge-drop contracts derived from Open GPUI docking behavior.
//!
//! Source: `repo-ref/open-gpui` revision
//! `56604588ee0a047c59e9ef6a2346f4c5839d90de`, Apache-2.0:
//! - `crates/gpui_docking/src/host_interaction_tests.rs`:
//!   `cross_window_tab_drag_to_edge_creates_split` and
//!   `dragging_tab_bar_empty_area_moves_whole_stack`.
//! - `crates/gpui_docking/src/graph_split_tests.rs`:
//!   `cross_axis_edge_dock_wraps_target_without_flattening_parent_axis` and the
//!   two `inner_edge_dock_does_not_cross_opposing_axis_ancestor` cases.
//!
//! The port deliberately strengthens the source behavior by pairing the exact
//! core-compiled guide identity with one checked semantic move command. Native
//! route/receiver causality is covered independently by the desktop-global
//! pointer-journal contract, so these tests stay focused on Open GPUI's topology
//! and state-preservation guarantees.

mod support;

use dockspace::command::{
    CommandOutcome, DockFraction, DockTarget, Edge, MovePayload, WorkspaceCommand,
};
use dockspace::drop_guide::{DropGuideScope, DropGuideSlot};
use dockspace::drop_target::DropTargetId;
use dockspace::engine::{DockEngine, EngineInput};
use dockspace::geometry::LogicalRect;
use dockspace::graph::{Axis, Node, RootRecord, SurfacePresentation, Workspace};
use dockspace::ids::{ItemId, NodeId, RootId, StableInputSourceId, SurfaceId};
use dockspace::policy::DockPolicy;
use dockspace::scene::PresentationPlan;
use dockspace::transition::InputOutcome;

const SOURCE_ROOT: RootId = RootId::new(1);
const TARGET_ROOT: RootId = RootId::new(2);
const SOURCE_SURFACE: SurfaceId = SurfaceId::new(1);
const TARGET_SURFACE: SurfaceId = SurfaceId::new(2);
const INPUT_SOURCE: StableInputSourceId = StableInputSourceId::new(0x6f_70_65_6e_67_70_75_69);

const MOVED_ITEM: ItemId = ItemId::new(1);
const SOURCE_SECOND: ItemId = ItemId::new(2);
const SOURCE_THIRD: ItemId = ItemId::new(3);
const TARGET_FIRST: ItemId = ItemId::new(10);
const TARGET_SELECTED: ItemId = ItemId::new(11);
const OUTER_SIBLING_ITEM: ItemId = ItemId::new(20);
const OPPOSING_SIBLING_ITEM: ItemId = ItemId::new(30);

const EDGES: [Edge; 4] = [Edge::Left, Edge::Right, Edge::Top, Edge::Bottom];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PayloadKind {
    Item,
    Tabs,
}

struct Fixture {
    engine: DockEngine,
    host: support::TestPresentationHost,
    source_tabs: NodeId,
    target_tabs: NodeId,
    outer_sibling: NodeId,
    opposing_parent: NodeId,
    opposing_sibling: NodeId,
    target_root_node: NodeId,
    payload_kind: PayloadKind,
}

impl Fixture {
    fn new(edge: Edge, payload_kind: PayloadKind) -> Self {
        let drop_axis = axis_for(edge);
        let opposing_axis = opposite_axis(drop_axis);
        let mut builder = Workspace::builder();
        let source_tabs = builder.insert_node(match payload_kind {
            PayloadKind::Item => Node::tabs([MOVED_ITEM, SOURCE_SECOND, SOURCE_THIRD]),
            PayloadKind::Tabs => Node::tabs_with_selection(
                [MOVED_ITEM, SOURCE_SECOND, SOURCE_THIRD],
                Some(SOURCE_SECOND),
            ),
        });
        let target_tabs = builder.insert_node(Node::tabs_with_selection(
            [TARGET_FIRST, TARGET_SELECTED],
            Some(TARGET_SELECTED),
        ));
        let outer_sibling = builder.insert_node(Node::tabs([OUTER_SIBLING_ITEM]));
        let opposing_sibling = builder.insert_node(Node::tabs([OPPOSING_SIBLING_ITEM]));
        let opposing_parent = builder.insert_node(
            Node::equal_split(opposing_axis, [target_tabs, opposing_sibling])
                .expect("opposing-axis target parent must be valid"),
        );
        let target_root_node = builder.insert_node(
            Node::equal_split(drop_axis, [outer_sibling, opposing_parent])
                .expect("target root split must be valid"),
        );
        builder.set_root(SOURCE_ROOT, RootRecord::new(source_tabs));
        builder.set_root(TARGET_ROOT, RootRecord::new(target_root_node));
        builder.set_surface(SOURCE_SURFACE, SurfacePresentation::with_main(SOURCE_ROOT));
        builder.set_surface(TARGET_SURFACE, SurfacePresentation::with_main(TARGET_ROOT));
        let workspace = builder
            .build()
            .expect("drop contract fixture must be valid");
        let mut engine =
            DockEngine::new(workspace, DockPolicy::default()).expect("drop engine must be valid");
        let host = support::TestPresentationHost::new(&mut engine);

        Self {
            engine,
            host,
            source_tabs,
            target_tabs,
            outer_sibling,
            opposing_parent,
            opposing_sibling,
            target_root_node,
            payload_kind,
        }
    }

    fn payload(&self) -> MovePayload {
        match self.payload_kind {
            PayloadKind::Item => MovePayload::Item(
                self.engine
                    .workspace()
                    .capture_item_source(SOURCE_ROOT, self.source_tabs, MOVED_ITEM)
                    .expect("item payload source must be current"),
            ),
            PayloadKind::Tabs => MovePayload::Tabs(
                self.engine
                    .workspace()
                    .capture_node_source(SOURCE_ROOT, self.source_tabs)
                    .expect("tabs payload source must be current"),
            ),
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct GuideFact {
    id: DropTargetId,
}

fn axis_for(edge: Edge) -> Axis {
    match edge {
        Edge::Left | Edge::Right => Axis::Horizontal,
        Edge::Top | Edge::Bottom => Axis::Vertical,
    }
}

fn opposite_axis(axis: Axis) -> Axis {
    match axis {
        Axis::Horizontal => Axis::Vertical,
        Axis::Vertical => Axis::Horizontal,
    }
}

fn rect() -> LogicalRect {
    LogicalRect::new(0.0, 0.0, 640.0, 480.0).expect("surface bounds must be valid")
}

fn publish_required_surfaces(fixture: &mut Fixture) {
    support::publish_surfaces(
        &mut fixture.engine,
        &mut fixture.host,
        [(SOURCE_SURFACE, rect()), (TARGET_SURFACE, rect())],
    );
}

fn target_plan(fixture: &Fixture) -> &PresentationPlan {
    support::painted_plan(&fixture.engine, TARGET_SURFACE)
}

fn inner_edge_guide(fixture: &Fixture, edge: Edge) -> GuideFact {
    let guide = target_plan(fixture)
        .drop_guide_clusters()
        .iter()
        .find(|cluster| {
            cluster.id().root == TARGET_ROOT
                && cluster.id().scope == DropGuideScope::Inner(fixture.target_tabs)
        })
        .and_then(|cluster| cluster.target(DropGuideSlot::Edge(edge)))
        .expect("core compiler must expose all four inner-edge guides for the target leaf");
    GuideFact { id: guide.id() }
}

fn submit_move(fixture: &mut Fixture, edge: Edge) {
    let payload = fixture.payload();
    let target = fixture
        .engine
        .workspace()
        .capture_inner_edge_target(
            TARGET_ROOT,
            fixture.target_tabs,
            edge,
            DockFraction::new(fixture.engine.presentation_config().dock_fraction() as f32)
                .expect("the configured dock fraction is valid"),
        )
        .expect("the exact inner-edge target is current");
    let expected = fixture.engine.version();
    let transition = support::submit_input(
        &mut fixture.engine,
        &mut fixture.host,
        INPUT_SOURCE,
        EngineInput::WorkspaceCommand {
            expected,
            command: WorkspaceCommand::Move {
                payload,
                target: DockTarget::InnerEdge(target),
            },
        },
    )
    .expect("the checked Open GPUI move command reduces");
    assert!(matches!(
        transition.reduced_inputs()[0].outcome(),
        InputOutcome::CommandProcessed {
            outcome: CommandOutcome::Moved {
                source_root: SOURCE_ROOT,
                target_root: TARGET_ROOT,
                changed: true,
                ..
            },
            changed: true,
            ..
        }
    ));
}

fn tabs_state(workspace: &Workspace, tabs: NodeId) -> (Vec<ItemId>, Option<ItemId>, Vec<ItemId>) {
    let Node::Tabs { items, selected } = workspace
        .node(tabs)
        .expect("expected tabs node must remain present")
    else {
        panic!("expected node {tabs:?} to remain a tabs node");
    };
    (
        items.clone(),
        *selected,
        workspace
            .tab_mru(tabs)
            .expect("tabs node must retain a complete MRU permutation")
            .to_vec(),
    )
}

fn split_state(workspace: &Workspace, split: NodeId) -> (Axis, Vec<NodeId>) {
    let Node::Split { axis, children, .. } = workspace
        .node(split)
        .expect("expected split node must remain present")
    else {
        panic!("expected node {split:?} to remain a split");
    };
    (*axis, children.clone())
}

fn tabs_containing(workspace: &Workspace, item: ItemId) -> NodeId {
    workspace
        .nodes()
        .find_map(|(node, record)| match record {
            Node::Tabs { items, .. } if items.contains(&item) => Some(node),
            Node::Tabs { .. } | Node::Split { .. } => None,
        })
        .expect("moved item must remain owned by exactly one tabs node")
}

fn assert_drop_contract(edge: Edge, payload_kind: PayloadKind) {
    let mut fixture = Fixture::new(edge, payload_kind);
    let before_items = fixture.engine.workspace().item_multiset();
    let before_source = tabs_state(fixture.engine.workspace(), fixture.source_tabs);
    let before_target = tabs_state(fixture.engine.workspace(), fixture.target_tabs);
    let before_outer = tabs_state(fixture.engine.workspace(), fixture.outer_sibling);
    let before_opposing = tabs_state(fixture.engine.workspace(), fixture.opposing_sibling);
    publish_required_surfaces(&mut fixture);
    let guide = inner_edge_guide(&fixture, edge);
    assert_eq!(
        guide.id,
        DropTargetId::InnerEdge {
            surface: TARGET_SURFACE,
            root: TARGET_ROOT,
            node: fixture.target_tabs,
            edge,
        },
        "the preview winner must be the exact compiled inner guide for {payload_kind:?} {edge:?}"
    );

    submit_move(&mut fixture, edge);

    let workspace = fixture.engine.workspace();
    assert_eq!(workspace.item_multiset(), before_items);
    workspace
        .validate()
        .expect("delivered drop must keep strict workspace invariants");
    assert_eq!(
        tabs_state(workspace, fixture.target_tabs),
        before_target,
        "edge docking must not alter the target stack selection or MRU"
    );
    assert_eq!(tabs_state(workspace, fixture.outer_sibling), before_outer);
    assert_eq!(
        tabs_state(workspace, fixture.opposing_sibling),
        before_opposing
    );

    let (root_axis, root_children) = split_state(workspace, fixture.target_root_node);
    assert_eq!(root_axis, axis_for(edge));
    assert_eq!(
        root_children,
        vec![fixture.outer_sibling, fixture.opposing_parent],
        "drop must not flatten across the target's opposing-axis parent"
    );
    let (opposing_axis, opposing_children) = split_state(workspace, fixture.opposing_parent);
    assert_eq!(opposing_axis, opposite_axis(axis_for(edge)));
    assert_eq!(opposing_children.len(), 2);
    assert_eq!(
        opposing_children[1], fixture.opposing_sibling,
        "drop must preserve the non-target sibling"
    );

    let local_split = opposing_children[0];
    let (local_axis, local_children) = split_state(workspace, local_split);
    assert_eq!(local_axis, axis_for(edge));
    let payload_node = match payload_kind {
        PayloadKind::Item => tabs_containing(workspace, MOVED_ITEM),
        PayloadKind::Tabs => fixture.source_tabs,
    };
    let expected_children = match edge {
        Edge::Left | Edge::Top => vec![payload_node, fixture.target_tabs],
        Edge::Right | Edge::Bottom => vec![fixture.target_tabs, payload_node],
    };
    assert_eq!(
        local_children, expected_children,
        "{payload_kind:?} {edge:?} must preserve physical child order"
    );

    match payload_kind {
        PayloadKind::Item => {
            assert!(fixture.engine.workspace().root(SOURCE_ROOT).is_some());
            assert!(fixture.engine.workspace().surface(SOURCE_SURFACE).is_some());
            assert_eq!(
                tabs_state(workspace, fixture.source_tabs),
                (
                    vec![SOURCE_SECOND, SOURCE_THIRD],
                    Some(SOURCE_SECOND),
                    vec![SOURCE_SECOND, SOURCE_THIRD],
                ),
                "moving the selected item must deterministically select the next MRU item"
            );
            assert_eq!(
                tabs_state(workspace, payload_node),
                (vec![MOVED_ITEM], Some(MOVED_ITEM), vec![MOVED_ITEM])
            );
        }
        PayloadKind::Tabs => {
            assert!(workspace.root(SOURCE_ROOT).is_none());
            assert!(workspace.surface(SOURCE_SURFACE).is_none());
            assert_eq!(
                tabs_state(workspace, fixture.source_tabs),
                before_source,
                "moving a complete tabs payload must preserve order, selection, and MRU"
            );
        }
    }
}

#[test]
fn open_gpui_item_drop_contract_covers_four_edges_and_exact_cross_axis_target() {
    // Ported from Open GPUI's cross-window edge-drag and graph split tests.
    for edge in EDGES {
        assert_drop_contract(edge, PayloadKind::Item);
    }
}

#[test]
fn open_gpui_tabs_drop_contract_covers_four_edges_and_preserves_stack_state() {
    // Ported from Open GPUI's whole-stack drag contract.
    for edge in EDGES {
        assert_drop_contract(edge, PayloadKind::Tabs);
    }
}
