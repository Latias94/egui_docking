use std::collections::BTreeMap;

use dockspace::command::{
    DockFraction, DockTarget, Edge, MovePayload, RootContent, RootPresentationTarget,
    WorkspaceCommand,
};
use dockspace::error::{CommandError, ReferenceRole, TransactionError};
use dockspace::graph::{
    Axis, Node, RootRecord, SplitWeight, SurfacePresentation, Workspace, WorkspaceBuilder,
};
use dockspace::ids::{ItemId, NodeId, RootId, SurfaceId};
use dockspace::policy::{DockPolicy, PolicyRejection};
use dockspace::transaction::WorkspaceTransaction;

const ROOT_A: RootId = RootId::new(1);
const ROOT_B: RootId = RootId::new(2);
const SURFACE_A: SurfaceId = SurfaceId::new(1);
const SURFACE_B: SurfaceId = SurfaceId::new(2);

fn item(value: u64) -> ItemId {
    ItemId::new(value)
}

fn apply(
    workspace: &mut Workspace,
    policy: &DockPolicy,
    command: WorkspaceCommand,
) -> Result<(), TransactionError> {
    WorkspaceTransaction::from_commands([command])
        .apply(workspace, policy)
        .map(|_| ())
}

fn two_roots() -> (Workspace, NodeId, NodeId) {
    let mut builder = WorkspaceBuilder::new();
    let tabs_a = builder.insert_node(Node::tabs([item(1), item(2), item(3)]));
    let tabs_b = builder.insert_node(Node::tabs([item(4), item(5)]));
    builder.set_root(ROOT_A, RootRecord::new(tabs_a));
    builder.set_root(ROOT_B, RootRecord::new(tabs_b));
    builder.set_surface(SURFACE_A, SurfacePresentation::new(ROOT_A));
    builder.set_surface(SURFACE_B, SurfacePresentation::new(ROOT_B));
    (
        builder.build().expect("two roots are valid"),
        tabs_a,
        tabs_b,
    )
}

fn tabs_items(workspace: &Workspace, tabs: NodeId) -> &[ItemId] {
    let Some(Node::Tabs { items, .. }) = workspace.node(tabs) else {
        panic!("expected tabs node {tabs:?}");
    };
    items
}

fn selected_item(workspace: &Workspace, tabs: NodeId) -> Option<ItemId> {
    let Some(Node::Tabs { selected, .. }) = workspace.node(tabs) else {
        panic!("expected tabs node {tabs:?}");
    };
    *selected
}

fn tabs_containing(workspace: &Workspace, needle: ItemId) -> NodeId {
    workspace
        .nodes()
        .find_map(|(node, value)| match value {
            Node::Tabs { items, .. } if items.contains(&needle) => Some(node),
            Node::Tabs { .. } | Node::Split { .. } => None,
        })
        .expect("item must remain reachable")
}

#[test]
fn same_stack_reorder_uses_pre_removal_gap_indices() {
    let (mut workspace, tabs, _) = two_roots();
    let source = workspace
        .capture_item_source(ROOT_A, tabs, item(1))
        .expect("source exists");
    let report = WorkspaceTransaction::from_commands([WorkspaceCommand::Reorder {
        source,
        insertion_index: 3,
    }])
    .apply(&mut workspace, &DockPolicy::default())
    .expect("reorder succeeds");

    assert_eq!(tabs_items(&workspace, tabs), [item(2), item(3), item(1)]);
    assert!(matches!(
        report.outcomes(),
        [dockspace::command::CommandOutcome::Reordered {
            from: 0,
            to: 2,
            changed: true,
            ..
        }]
    ));

    let source = workspace
        .capture_item_source(ROOT_A, tabs, item(1))
        .expect("reordered source exists");
    let report = WorkspaceTransaction::from_commands([WorkspaceCommand::Reorder {
        source,
        insertion_index: 3,
    }])
    .apply(&mut workspace, &DockPolicy::default())
    .expect("same gap is a checked no-op");
    assert!(matches!(
        report.outcomes(),
        [dockspace::command::CommandOutcome::Reordered { changed: false, .. }]
    ));
}

#[test]
fn reorder_is_rejected_when_tab_merge_policy_is_disabled() {
    let (mut workspace, tabs, _) = two_roots();
    let source = workspace
        .capture_item_source(ROOT_A, tabs, item(1))
        .expect("source exists");
    let before = workspace.clone();
    let mut policy = DockPolicy::default();
    policy.set_allow_tab_merge(false);

    let error = apply(
        &mut workspace,
        &policy,
        WorkspaceCommand::Reorder {
            source,
            insertion_index: 3,
        },
    )
    .expect_err("reorder must share the tab-merge policy gate");

    assert!(matches!(
        error,
        TransactionError::Command {
            source: CommandError::Policy(PolicyRejection::TabMergeDisabled),
            ..
        }
    ));
    assert_eq!(workspace, before);
}

#[test]
fn equivalent_edge_move_reports_no_change() {
    let mut builder = WorkspaceBuilder::new();
    let left = builder.insert_node(Node::tabs([item(1)]));
    let right = builder.insert_node(Node::tabs([item(2)]));
    let split = builder.insert_node(
        Node::split(Axis::Horizontal, [left, right], [0.25, 0.75]).expect("split is valid"),
    );
    builder.set_root(ROOT_A, RootRecord::new(split));
    builder.set_surface(SURFACE_A, SurfacePresentation::new(ROOT_A));
    let mut workspace = builder.build().expect("workspace is valid");
    let source = workspace
        .capture_node_source(ROOT_A, left)
        .expect("source exists");
    let target = workspace
        .capture_edge_target(
            ROOT_A,
            right,
            Edge::Left,
            DockFraction::new(0.25).expect("fraction is valid"),
        )
        .expect("target exists");
    let before = workspace.clone();

    let report = WorkspaceTransaction::from_commands([WorkspaceCommand::Move {
        payload: MovePayload::Tabs(source),
        target: DockTarget::Edge(target),
    }])
    .apply(&mut workspace, &DockPolicy::default())
    .expect("equivalent edge move is valid");

    assert_eq!(workspace, before);
    assert!(matches!(
        report.outcomes(),
        [dockspace::command::CommandOutcome::Moved { changed: false, .. }]
    ));
}

#[test]
fn center_then_edge_moves_cross_roots_without_changing_items() {
    let (mut workspace, tabs_a, tabs_b) = two_roots();
    let expected = workspace.item_multiset();
    let source = workspace
        .capture_item_source(ROOT_A, tabs_a, item(3))
        .expect("source exists");
    let target = workspace
        .capture_tab_target(ROOT_B, tabs_b)
        .expect("target exists");
    apply(
        &mut workspace,
        &DockPolicy::default(),
        WorkspaceCommand::Move {
            payload: MovePayload::Item(source),
            target: DockTarget::Center(target),
        },
    )
    .expect("center move succeeds");
    assert_eq!(tabs_items(&workspace, tabs_a), [item(1), item(2)]);
    assert_eq!(tabs_items(&workspace, tabs_b), [item(4), item(5), item(3)]);

    let source = workspace
        .capture_node_source(ROOT_A, tabs_a)
        .expect("remaining tabs exist");
    let target = workspace
        .capture_edge_target(
            ROOT_B,
            tabs_b,
            Edge::Left,
            DockFraction::new(0.25).expect("fraction is valid"),
        )
        .expect("edge target exists");
    apply(
        &mut workspace,
        &DockPolicy::default(),
        WorkspaceCommand::Move {
            payload: MovePayload::Tabs(source),
            target: DockTarget::Edge(target),
        },
    )
    .expect("edge move succeeds");

    assert_eq!(workspace.item_multiset(), expected);
    assert!(workspace.root(ROOT_A).is_none());
    assert!(workspace.surface(SURFACE_A).is_none());
    let root = workspace.root(ROOT_B).expect("target root survives");
    assert!(matches!(
        workspace.node(root.node),
        Some(Node::Split {
            axis: Axis::Horizontal,
            children,
            ..
        }) if children == &[tabs_a, tabs_b]
    ));
    workspace.validate().expect("result remains strict");
}

#[test]
fn open_edge_and_close_restore_the_original_item_multiset() {
    let (mut workspace, _, tabs_b) = two_roots();
    let expected = workspace.item_multiset();
    let target = workspace
        .capture_edge_target(
            ROOT_B,
            tabs_b,
            Edge::Bottom,
            DockFraction::new(0.4).expect("fraction is valid"),
        )
        .expect("target exists");
    apply(
        &mut workspace,
        &DockPolicy::default(),
        WorkspaceCommand::Open {
            item: item(99),
            target: DockTarget::Edge(target),
        },
    )
    .expect("open succeeds");
    assert_eq!(workspace.item_multiset().get(&item(99)), Some(&1));

    let opened_tabs = tabs_containing(&workspace, item(99));
    let source = workspace
        .capture_item_source(ROOT_B, opened_tabs, item(99))
        .expect("opened item has a source");
    apply(
        &mut workspace,
        &DockPolicy::default(),
        WorkspaceCommand::Close { source },
    )
    .expect("close succeeds");
    assert_eq!(workspace.item_multiset(), expected);
    workspace.validate().expect("cleanup is canonical");
}

#[test]
fn stale_source_and_target_snapshots_fail_closed() {
    let (mut workspace, tabs_a, tabs_b) = two_roots();
    let stale_source = workspace
        .capture_item_source(ROOT_A, tabs_a, item(1))
        .expect("source exists");
    let changing_source = workspace
        .capture_item_source(ROOT_A, tabs_a, item(2))
        .expect("source exists");
    apply(
        &mut workspace,
        &DockPolicy::default(),
        WorkspaceCommand::Select {
            source: changing_source,
        },
    )
    .expect("selection changes the source root snapshot");
    let before = workspace.clone();
    let error = apply(
        &mut workspace,
        &DockPolicy::default(),
        WorkspaceCommand::Close {
            source: stale_source,
        },
    )
    .expect_err("stale source must fail");
    assert!(matches!(
        error,
        TransactionError::Command {
            index: 0,
            source: CommandError::StaleNode {
                role: ReferenceRole::Source,
                ..
            }
        }
    ));
    assert_eq!(workspace, before);

    let stale_target = workspace
        .capture_tab_target(ROOT_B, tabs_b)
        .expect("target exists");
    let current_target = workspace
        .capture_tab_target(ROOT_B, tabs_b)
        .expect("target exists");
    apply(
        &mut workspace,
        &DockPolicy::default(),
        WorkspaceCommand::Open {
            item: item(100),
            target: DockTarget::Center(current_target),
        },
    )
    .expect("opening changes the target root snapshot");
    let before = workspace.clone();
    let error = apply(
        &mut workspace,
        &DockPolicy::default(),
        WorkspaceCommand::Open {
            item: item(101),
            target: DockTarget::Center(stale_target),
        },
    )
    .expect_err("stale target must fail");
    assert!(matches!(
        error,
        TransactionError::Command {
            index: 0,
            source: CommandError::StaleNode {
                role: ReferenceRole::Target,
                ..
            }
        }
    ));
    assert_eq!(workspace, before);
}

#[test]
fn resize_requires_a_complete_positive_normalized_vector() {
    let mut builder = WorkspaceBuilder::new();
    let left = builder.insert_node(Node::tabs([item(1)]));
    let right = builder.insert_node(Node::tabs([item(2)]));
    let split = builder
        .insert_node(Node::equal_split(Axis::Horizontal, [left, right]).expect("valid split"));
    builder.set_root(ROOT_A, RootRecord::new(split));
    builder.set_surface(SURFACE_A, SurfacePresentation::new(ROOT_A));
    let mut workspace = builder.build().expect("workspace is valid");

    let source = workspace
        .capture_node_source(ROOT_A, split)
        .expect("split exists");
    let before = workspace.clone();
    let error = apply(
        &mut workspace,
        &DockPolicy::default(),
        WorkspaceCommand::ResizeSplit {
            split: source,
            weights: vec![SplitWeight::new(0.2).expect("positive")],
        },
    )
    .expect_err("incomplete vector fails");
    assert!(matches!(
        error,
        TransactionError::Command {
            source: CommandError::SplitWeightCountMismatch { .. },
            ..
        }
    ));
    assert_eq!(workspace, before);

    let source = workspace
        .capture_node_source(ROOT_A, split)
        .expect("split exists");
    apply(
        &mut workspace,
        &DockPolicy::default(),
        WorkspaceCommand::ResizeSplit {
            split: source,
            weights: vec![
                SplitWeight::new(0.25).expect("positive"),
                SplitWeight::new(0.75).expect("positive"),
            ],
        },
    )
    .expect("normalized resize succeeds");
    assert!(matches!(
        workspace.node(split),
        Some(Node::Split { weights, .. })
            if weights.iter().map(|weight| weight.get()).collect::<Vec<_>>() == [0.25, 0.75]
    ));
}

#[test]
fn closing_last_noncentral_item_removes_root_and_surface_atomically() {
    let mut builder = WorkspaceBuilder::new();
    let tabs = builder.insert_node(Node::tabs([item(1)]));
    builder.set_root(ROOT_A, RootRecord::new(tabs));
    builder.set_surface(SURFACE_A, SurfacePresentation::new(ROOT_A));
    let mut workspace = builder.build().expect("workspace is valid");
    let source = workspace
        .capture_item_source(ROOT_A, tabs, item(1))
        .expect("source exists");
    apply(
        &mut workspace,
        &DockPolicy::default(),
        WorkspaceCommand::Close { source },
    )
    .expect("last close succeeds");
    assert_eq!(workspace, Workspace::new());
}

#[test]
fn closing_last_central_item_preserves_the_empty_central_leaf() {
    let mut builder = WorkspaceBuilder::new();
    let central = builder.insert_node(Node::tabs([item(1)]));
    builder.set_root(ROOT_A, RootRecord::new(central).with_central(central));
    builder.set_surface(SURFACE_A, SurfacePresentation::new(ROOT_A));
    let mut workspace = builder.build().expect("workspace is valid");
    let source = workspace
        .capture_item_source(ROOT_A, central, item(1))
        .expect("source exists");
    apply(
        &mut workspace,
        &DockPolicy::default(),
        WorkspaceCommand::Close { source },
    )
    .expect("central close succeeds");
    assert_eq!(workspace.item_multiset(), BTreeMap::new());
    assert_eq!(
        workspace.root(ROOT_A),
        Some(&RootRecord::new(central).with_central(central))
    );
    assert!(matches!(
        workspace.node(central),
        Some(Node::Tabs { items, selected }) if items.is_empty() && selected.is_none()
    ));
}

#[test]
fn close_selection_follows_the_normative_visual_neighbor_rule() {
    let (mut workspace, tabs, _) = two_roots();
    let select_two = workspace
        .capture_item_source(ROOT_A, tabs, item(2))
        .expect("source exists");
    apply(
        &mut workspace,
        &DockPolicy::default(),
        WorkspaceCommand::Select { source: select_two },
    )
    .expect("selection succeeds");

    let close_middle = workspace
        .capture_item_source(ROOT_A, tabs, item(2))
        .expect("source exists");
    apply(
        &mut workspace,
        &DockPolicy::default(),
        WorkspaceCommand::Close {
            source: close_middle,
        },
    )
    .expect("closing selected middle item succeeds");
    assert_eq!(tabs_items(&workspace, tabs), [item(1), item(3)]);
    assert_eq!(selected_item(&workspace, tabs), Some(item(3)));

    let open_target = workspace
        .capture_tab_target(ROOT_A, tabs)
        .expect("target exists");
    apply(
        &mut workspace,
        &DockPolicy::default(),
        WorkspaceCommand::Open {
            item: item(9),
            target: DockTarget::Center(open_target),
        },
    )
    .expect("append succeeds");
    let select_three = workspace
        .capture_item_source(ROOT_A, tabs, item(3))
        .expect("source exists");
    apply(
        &mut workspace,
        &DockPolicy::default(),
        WorkspaceCommand::Select {
            source: select_three,
        },
    )
    .expect("selection succeeds");

    let close_inactive = workspace
        .capture_item_source(ROOT_A, tabs, item(1))
        .expect("source exists");
    apply(
        &mut workspace,
        &DockPolicy::default(),
        WorkspaceCommand::Close {
            source: close_inactive,
        },
    )
    .expect("closing inactive item succeeds");
    assert_eq!(selected_item(&workspace, tabs), Some(item(3)));

    let select_last = workspace
        .capture_item_source(ROOT_A, tabs, item(9))
        .expect("source exists");
    apply(
        &mut workspace,
        &DockPolicy::default(),
        WorkspaceCommand::Select {
            source: select_last,
        },
    )
    .expect("last item selection succeeds");
    let close_selected_last = workspace
        .capture_item_source(ROOT_A, tabs, item(9))
        .expect("refreshed source exists");
    apply(
        &mut workspace,
        &DockPolicy::default(),
        WorkspaceCommand::Close {
            source: close_selected_last,
        },
    )
    .expect("closing selected last item succeeds");
    assert_eq!(tabs_items(&workspace, tabs), [item(3)]);
    assert_eq!(selected_item(&workspace, tabs), Some(item(3)));
}

#[test]
fn moving_a_whole_root_to_a_new_surface_preserves_its_central_identity() {
    let mut builder = WorkspaceBuilder::new();
    let central = builder.insert_node(Node::tabs([item(1), item(2)]));
    builder.set_root(ROOT_A, RootRecord::new(central).with_central(central));
    builder.set_surface(SURFACE_A, SurfacePresentation::new(ROOT_A));
    let mut workspace = builder.build().expect("workspace is valid");
    let source = workspace
        .capture_node_source(ROOT_A, central)
        .expect("source root exists");
    let mut policy = DockPolicy::default();
    policy.set_allow_native_surfaces(true);
    let before = workspace.clone();
    let error = apply(
        &mut workspace,
        &policy,
        WorkspaceCommand::CreateSurfaceRoot {
            surface: SURFACE_B,
            root: ROOT_B,
            content: RootContent::Move(MovePayload::Tabs(source.clone())),
        },
    )
    .expect_err("creating a new root must not replace a complete stable root identity");
    assert!(matches!(
        error,
        TransactionError::Command {
            source: CommandError::WholeRootRequiresRehome { root: ROOT_A },
            ..
        }
    ));
    assert_eq!(workspace, before);

    apply(
        &mut workspace,
        &policy,
        WorkspaceCommand::RehomeRoot {
            source,
            target: RootPresentationTarget::Surface { surface: SURFACE_B },
        },
    )
    .expect("surface move succeeds");
    assert!(workspace.surface(SURFACE_A).is_none());
    assert_eq!(
        workspace.root(ROOT_A),
        Some(&RootRecord::new(central).with_central(central))
    );
    assert_eq!(
        workspace.surface(SURFACE_B),
        Some(&SurfacePresentation::new(ROOT_A))
    );
    assert_eq!(
        workspace.item_multiset(),
        BTreeMap::from([(item(1), 1), (item(2), 1)])
    );
}

#[test]
fn remove_empty_root_removes_its_complete_presentation() {
    let mut builder = WorkspaceBuilder::new();
    let central = builder.insert_node(Node::tabs([]));
    builder.set_root(ROOT_A, RootRecord::new(central).with_central(central));
    builder.set_surface(SURFACE_A, SurfacePresentation::new(ROOT_A));
    let mut workspace = builder.build().expect("empty central root is valid");
    let source = workspace
        .capture_node_source(ROOT_A, central)
        .expect("empty root exists");
    apply(
        &mut workspace,
        &DockPolicy::default(),
        WorkspaceCommand::RemoveEmptyRoot { source },
    )
    .expect("empty root removal succeeds");
    assert_eq!(workspace, Workspace::new());
}
