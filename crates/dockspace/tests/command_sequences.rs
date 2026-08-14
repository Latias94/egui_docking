use super::support;

use std::collections::BTreeMap;

use dockspace::command::{
    CloseCommitOutcome, ContentCloseTarget, DockFraction, DockTarget, Edge, MovePayload,
    RootContent, RootPresentationTarget, SplitResize, WorkspaceCommand,
};
use dockspace::engine::{DockEngine, EngineInput};
use dockspace::error::{CommandError, ReferenceRole, TransactionError};
use dockspace::geometry::LogicalRect;
use dockspace::graph::{
    Axis, ContainedFloating, Node, RootRecord, SplitWeight, SurfacePresentation, Workspace,
    WorkspaceBuilder,
};
use dockspace::ids::{
    FloatingPresentationId, ItemId, NodeId, RootId, StableInputSourceId, SurfaceId,
};
use dockspace::policy::{DockPolicy, DockPolicySnapshot, PolicyRejection, PolicyRevision};
use dockspace::transaction::WorkspaceTransaction;
use support::{TestPresentationHost, submit_input};

const ROOT_A: RootId = RootId::new(1);
const ROOT_B: RootId = RootId::new(2);
const SURFACE_A: SurfaceId = SurfaceId::new(1);
const SURFACE_B: SurfaceId = SurfaceId::new(2);
const FLOATING_B: FloatingPresentationId = FloatingPresentationId::new(2);
const CLOSE_INPUT_SOURCE: StableInputSourceId = StableInputSourceId::new(0xC001);

fn item(value: u64) -> ItemId {
    ItemId::new(value)
}

fn apply(
    workspace: &mut Workspace,
    policy: &DockPolicySnapshot,
    command: WorkspaceCommand,
) -> Result<(), TransactionError> {
    WorkspaceTransaction::from_commands([command])
        .apply(workspace, policy)
        .map(|_| ())
}

fn close(workspace: &mut Workspace, target: ContentCloseTarget) -> CloseCommitOutcome {
    let mut engine = DockEngine::new(workspace.clone(), DockPolicy::default())
        .expect("close fixture must be valid");
    let mut host = TestPresentationHost::new(&mut engine);
    let expected = engine.version();
    let request = submit_input(
        &mut engine,
        &mut host,
        CLOSE_INPUT_SOURCE,
        EngineInput::RequestContentClose { expected, target },
    )
    .expect("close request must reduce");
    let dockspace::transition::InputOutcome::ContentCloseRequested { plan, .. } =
        request.reduced_inputs()[0].outcome()
    else {
        panic!("close target must open a plan");
    };
    let plan = plan.clone();
    let mut committed = None;
    for requirement in plan.items() {
        let transition = submit_input(
            &mut engine,
            &mut host,
            CLOSE_INPUT_SOURCE,
            EngineInput::ResolveClose {
                request: plan.request(),
                token: requirement.token(),
                decision: dockspace::close::CloseDecision::Allow,
            },
        )
        .expect("close decision must reduce");
        if let dockspace::transition::InputOutcome::CloseDecisionProcessed {
            application: Some(Ok(outcome)),
            ..
        } = transition.reduced_inputs()[0].outcome()
        {
            assert!(transition.events().iter().any(|event| {
                matches!(
                    event.kind(),
                    dockspace::event::WorkspaceEventKind::CloseCommitted(committed)
                        if committed == outcome
                )
            }));
            assert!(transition.events().iter().all(|event| !matches!(
                event.kind(),
                dockspace::event::WorkspaceEventKind::CommandCommitted(_)
            )));
            committed = Some(outcome.clone());
        }
    }
    *workspace = engine.workspace().clone();
    committed.expect("final close decision must commit")
}

fn two_roots() -> (Workspace, NodeId, NodeId) {
    let mut builder = WorkspaceBuilder::new();
    let tabs_a = builder.insert_node(Node::tabs([item(1), item(2), item(3)]));
    let tabs_b = builder.insert_node(Node::tabs([item(4), item(5)]));
    builder.set_root(ROOT_A, RootRecord::new(tabs_a));
    builder.set_root(ROOT_B, RootRecord::new(tabs_b));
    builder.set_surface(SURFACE_A, SurfacePresentation::with_main(ROOT_A));
    builder.set_surface(SURFACE_B, SurfacePresentation::with_main(ROOT_B));
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
    .apply(&mut workspace, &DockPolicySnapshot::default())
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
    .apply(&mut workspace, &DockPolicySnapshot::default())
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
    let policy = policy.snapshot(PolicyRevision::default());

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
    builder.set_surface(SURFACE_A, SurfacePresentation::with_main(ROOT_A));
    let mut workspace = builder.build().expect("workspace is valid");
    let source = workspace
        .capture_node_source(ROOT_A, left)
        .expect("source exists");
    let target = workspace
        .capture_inner_edge_target(
            ROOT_A,
            right,
            Edge::Left,
            DockFraction::new(0.25).expect("fraction is valid"),
        )
        .expect("target exists");
    let before = workspace.clone();

    let report = WorkspaceTransaction::from_commands([WorkspaceCommand::Move {
        payload: MovePayload::Tabs(source),
        target: DockTarget::InnerEdge(target),
    }])
    .apply(&mut workspace, &DockPolicySnapshot::default())
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
        &DockPolicySnapshot::default(),
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
        .capture_inner_edge_target(
            ROOT_B,
            tabs_b,
            Edge::Left,
            DockFraction::new(0.25).expect("fraction is valid"),
        )
        .expect("edge target exists");
    apply(
        &mut workspace,
        &DockPolicySnapshot::default(),
        WorkspaceCommand::Move {
            payload: MovePayload::Tabs(source),
            target: DockTarget::InnerEdge(target),
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
        .capture_inner_edge_target(
            ROOT_B,
            tabs_b,
            Edge::Bottom,
            DockFraction::new(0.4).expect("fraction is valid"),
        )
        .expect("target exists");
    apply(
        &mut workspace,
        &DockPolicySnapshot::default(),
        WorkspaceCommand::Open {
            item: item(99),
            target: DockTarget::InnerEdge(target),
        },
    )
    .expect("open succeeds");
    assert_eq!(workspace.item_multiset().get(&item(99)), Some(&1));

    close(&mut workspace, ContentCloseTarget::Item(item(99)));
    assert_eq!(workspace.item_multiset(), expected);
    workspace.validate().expect("cleanup is canonical");
}

#[test]
fn stale_source_and_target_snapshots_fail_closed() {
    let (mut workspace, tabs_a, tabs_b) = two_roots();
    let changing_source = workspace
        .capture_item_source(ROOT_A, tabs_a, item(2))
        .expect("source exists");
    apply(
        &mut workspace,
        &DockPolicySnapshot::default(),
        WorkspaceCommand::Select {
            source: changing_source,
        },
    )
    .expect("selection changes the source root snapshot");
    let before = workspace.clone();
    // Programmatic close carries only stable identity, so it never accepts a
    // caller-supplied stale graph source.
    close(&mut workspace, ContentCloseTarget::Item(item(1)));
    assert_ne!(workspace, before);

    let stale_target = workspace
        .capture_tab_target(ROOT_B, tabs_b)
        .expect("target exists");
    let current_target = workspace
        .capture_tab_target(ROOT_B, tabs_b)
        .expect("target exists");
    apply(
        &mut workspace,
        &DockPolicySnapshot::default(),
        WorkspaceCommand::Open {
            item: item(100),
            target: DockTarget::Center(current_target),
        },
    )
    .expect("opening changes the target root snapshot");
    let before = workspace.clone();
    let error = apply(
        &mut workspace,
        &DockPolicySnapshot::default(),
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
    builder.set_surface(SURFACE_A, SurfacePresentation::with_main(ROOT_A));
    let mut workspace = builder.build().expect("workspace is valid");

    let source = workspace
        .capture_node_source(ROOT_A, split)
        .expect("split exists");
    let before = workspace.clone();
    let error = apply(
        &mut workspace,
        &DockPolicySnapshot::default(),
        WorkspaceCommand::ResizeSplits {
            splits: vec![SplitResize::new(
                source,
                vec![SplitWeight::new(0.2).expect("positive")],
            )],
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
        &DockPolicySnapshot::default(),
        WorkspaceCommand::ResizeSplits {
            splits: vec![SplitResize::new(
                source,
                vec![
                    SplitWeight::new(0.25).expect("positive"),
                    SplitWeight::new(0.75).expect("positive"),
                ],
            )],
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
    builder.set_surface(SURFACE_A, SurfacePresentation::with_main(ROOT_A));
    let mut workspace = builder.build().expect("workspace is valid");
    let outcome = close(&mut workspace, ContentCloseTarget::Item(item(1)));
    assert_eq!(
        outcome,
        CloseCommitOutcome::ItemClosed {
            item: item(1),
            root: ROOT_A,
        }
    );
    assert_eq!(workspace, Workspace::new());
}

#[test]
fn closing_last_central_item_preserves_the_empty_central_leaf() {
    let mut builder = WorkspaceBuilder::new();
    let central = builder.insert_node(Node::tabs([item(1)]));
    builder.set_root(ROOT_A, RootRecord::new(central).with_central(central));
    builder.set_surface(SURFACE_A, SurfacePresentation::with_main(ROOT_A));
    let mut workspace = builder.build().expect("workspace is valid");
    let outcome = close(&mut workspace, ContentCloseTarget::Item(item(1)));
    assert_eq!(
        outcome,
        CloseCommitOutcome::ItemClosed {
            item: item(1),
            root: ROOT_A,
        }
    );
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
fn close_selection_follows_the_durable_mru_order() {
    let (mut workspace, tabs, _) = two_roots();
    let select_two = workspace
        .capture_item_source(ROOT_A, tabs, item(2))
        .expect("source exists");
    apply(
        &mut workspace,
        &DockPolicySnapshot::default(),
        WorkspaceCommand::Select { source: select_two },
    )
    .expect("selection succeeds");

    assert_eq!(
        close(&mut workspace, ContentCloseTarget::Item(item(2))),
        CloseCommitOutcome::ItemClosed {
            item: item(2),
            root: ROOT_A,
        }
    );
    assert_eq!(tabs_items(&workspace, tabs), [item(1), item(3)]);
    assert_eq!(selected_item(&workspace, tabs), Some(item(1)));
    assert_eq!(workspace.tab_mru(tabs), Some([item(1), item(3)].as_slice()));

    let open_target = workspace
        .capture_tab_target(ROOT_A, tabs)
        .expect("target exists");
    apply(
        &mut workspace,
        &DockPolicySnapshot::default(),
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
        &DockPolicySnapshot::default(),
        WorkspaceCommand::Select {
            source: select_three,
        },
    )
    .expect("selection succeeds");

    assert_eq!(
        close(&mut workspace, ContentCloseTarget::Item(item(1))),
        CloseCommitOutcome::ItemClosed {
            item: item(1),
            root: ROOT_A,
        }
    );
    assert_eq!(selected_item(&workspace, tabs), Some(item(3)));

    let select_last = workspace
        .capture_item_source(ROOT_A, tabs, item(9))
        .expect("source exists");
    apply(
        &mut workspace,
        &DockPolicySnapshot::default(),
        WorkspaceCommand::Select {
            source: select_last,
        },
    )
    .expect("last item selection succeeds");
    assert_eq!(
        close(&mut workspace, ContentCloseTarget::Item(item(9))),
        CloseCommitOutcome::ItemClosed {
            item: item(9),
            root: ROOT_A,
        }
    );
    assert_eq!(tabs_items(&workspace, tabs), [item(3)]);
    assert_eq!(selected_item(&workspace, tabs), Some(item(3)));
}

#[test]
fn moving_a_whole_root_to_a_new_surface_preserves_its_central_identity() {
    let mut builder = WorkspaceBuilder::new();
    let central = builder.insert_node(Node::tabs([item(1), item(2)]));
    builder.set_root(ROOT_A, RootRecord::new(central).with_central(central));
    builder.set_surface(SURFACE_A, SurfacePresentation::with_main(ROOT_A));
    let mut workspace = builder.build().expect("workspace is valid");
    let source = workspace
        .capture_node_source(ROOT_A, central)
        .expect("source root exists");
    let mut policy = DockPolicy::default();
    policy.set_allow_native_surfaces(true);
    let policy = policy.snapshot(PolicyRevision::default());
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
            target: RootPresentationTarget::NewSurface { surface: SURFACE_B },
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
        Some(&SurfacePresentation::with_main(ROOT_A))
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
    builder.set_surface(SURFACE_A, SurfacePresentation::with_main(ROOT_A));
    let mut workspace = builder.build().expect("empty central root is valid");
    let source = workspace
        .capture_node_source(ROOT_A, central)
        .expect("empty root exists");
    apply(
        &mut workspace,
        &DockPolicySnapshot::default(),
        WorkspaceCommand::RemoveEmptyRoot { source },
    )
    .expect("empty root removal succeeds");
    assert_eq!(workspace, Workspace::new());
}

#[test]
fn close_root_removes_main_surface_and_complete_topology() {
    let mut builder = WorkspaceBuilder::new();
    let first = builder.insert_node(Node::tabs([item(1), item(2)]));
    let second = builder.insert_node(Node::tabs([item(3)]));
    let split = builder
        .insert_node(Node::equal_split(Axis::Horizontal, [first, second]).expect("split is valid"));
    let survivor = builder.insert_node(Node::tabs([item(4), item(5)]));
    builder.set_root(ROOT_A, RootRecord::new(split));
    builder.set_root(ROOT_B, RootRecord::new(survivor));
    builder.set_surface(SURFACE_A, SurfacePresentation::with_main(ROOT_A));
    builder.set_surface(SURFACE_B, SurfacePresentation::with_main(ROOT_B));
    let mut workspace = builder.build().expect("workspace is valid");
    assert_eq!(
        close(&mut workspace, ContentCloseTarget::Root(ROOT_A)),
        CloseCommitOutcome::RootClosed {
            root: ROOT_A,
            items: vec![item(1), item(2), item(3)],
        }
    );
    assert!(workspace.root(ROOT_A).is_none());
    assert!(workspace.surface(SURFACE_A).is_none());
    assert!(workspace.node(first).is_none());
    assert!(workspace.node(second).is_none());
    assert!(workspace.node(split).is_none());
    assert_eq!(workspace.root(ROOT_B), Some(&RootRecord::new(survivor)));
    assert_eq!(
        workspace.item_multiset(),
        BTreeMap::from([(item(4), 1), (item(5), 1)])
    );
    workspace.validate().expect("remaining workspace is valid");
}

#[test]
fn close_root_removes_only_the_contained_presentation() {
    let mut builder = WorkspaceBuilder::new();
    let main = builder.insert_node(Node::tabs([item(1)]));
    let contained = builder.insert_node(Node::tabs([item(4), item(5)]));
    builder.set_root(ROOT_A, RootRecord::new(main));
    builder.set_root(ROOT_B, RootRecord::new(contained));
    builder.set_surface(SURFACE_A, SurfacePresentation::with_main(ROOT_A));
    builder.set_contained_floating(
        FLOATING_B,
        ContainedFloating::new(
            ROOT_B,
            LogicalRect::new(10.0, 20.0, 300.0, 200.0).expect("rectangle is valid"),
        ),
    );
    builder
        .attach_contained(SURFACE_A, FLOATING_B)
        .expect("surface exists");
    let mut workspace = builder.build().expect("workspace is valid");
    assert_eq!(
        close(&mut workspace, ContentCloseTarget::Root(ROOT_B)),
        CloseCommitOutcome::RootClosed {
            root: ROOT_B,
            items: vec![item(4), item(5)],
        }
    );
    assert!(workspace.root(ROOT_B).is_none());
    assert!(workspace.node(contained).is_none());
    assert!(workspace.contained_floating(FLOATING_B).is_none());
    assert_eq!(
        workspace.surface(SURFACE_A),
        Some(&SurfacePresentation::with_main(ROOT_A))
    );
    assert_eq!(workspace.item_multiset(), BTreeMap::from([(item(1), 1)]));
    workspace.validate().expect("remaining workspace is valid");
}

#[test]
fn close_root_leaves_a_rootless_surface_when_contained_roots_survive() {
    let mut builder = WorkspaceBuilder::new();
    let main = builder.insert_node(Node::tabs([item(1)]));
    let contained = builder.insert_node(Node::tabs([item(2)]));
    builder.set_root(ROOT_A, RootRecord::new(main));
    builder.set_root(ROOT_B, RootRecord::new(contained));
    builder.set_surface(SURFACE_A, SurfacePresentation::with_main(ROOT_A));
    builder.set_contained_floating(
        FLOATING_B,
        ContainedFloating::new(
            ROOT_B,
            LogicalRect::new(10.0, 20.0, 300.0, 200.0).expect("rectangle is valid"),
        ),
    );
    builder
        .attach_contained(SURFACE_A, FLOATING_B)
        .expect("surface exists");
    let mut workspace = builder.build().expect("workspace is valid");
    assert_eq!(
        close(&mut workspace, ContentCloseTarget::Root(ROOT_A)),
        CloseCommitOutcome::RootClosed {
            root: ROOT_A,
            items: vec![item(1)],
        }
    );
    assert!(workspace.root(ROOT_A).is_none());
    assert!(workspace.node(main).is_none());
    assert_eq!(workspace.root(ROOT_B), Some(&RootRecord::new(contained)));
    assert_eq!(
        workspace.surface(SURFACE_A),
        Some(&SurfacePresentation {
            main_root: None,
            contained: vec![FLOATING_B],
        })
    );
    assert_eq!(
        workspace.presentation_for_root(ROOT_B),
        Some(dockspace::RootPresentationOwner::Contained {
            surface: SURFACE_A,
            floating: FLOATING_B,
        })
    );
    workspace.validate().expect("rootless survivor is valid");
}

#[test]
fn root_close_recaptures_current_graph_from_stable_identity() {
    let mut builder = WorkspaceBuilder::new();
    let first = builder.insert_node(Node::tabs([item(1), item(2)]));
    let second = builder.insert_node(Node::tabs([item(3)]));
    let split = builder
        .insert_node(Node::equal_split(Axis::Horizontal, [first, second]).expect("split is valid"));
    builder.set_root(ROOT_A, RootRecord::new(split));
    builder.set_surface(SURFACE_A, SurfacePresentation::with_main(ROOT_A));
    let mut workspace = builder.build().expect("workspace is valid");
    let reorder = workspace
        .capture_item_source(ROOT_A, first, item(1))
        .expect("item source exists");
    apply(
        &mut workspace,
        &DockPolicySnapshot::default(),
        WorkspaceCommand::Reorder {
            source: reorder,
            insertion_index: 2,
        },
    )
    .expect("reorder succeeds");
    assert_eq!(
        close(&mut workspace, ContentCloseTarget::Root(ROOT_A)),
        CloseCommitOutcome::RootClosed {
            root: ROOT_A,
            items: vec![item(2), item(1), item(3)],
        }
    );
    assert_eq!(workspace, Workspace::new());
}

#[test]
fn content_close_rejects_empty_and_missing_roots_without_mutation() {
    let mut empty_builder = WorkspaceBuilder::new();
    let empty = empty_builder.insert_node(Node::tabs([]));
    empty_builder.set_root(ROOT_A, RootRecord::new(empty).with_central(empty));
    empty_builder.set_surface(SURFACE_A, SurfacePresentation::with_main(ROOT_A));
    let empty_workspace = empty_builder.build().expect("empty central root is valid");
    let mut engine = DockEngine::new(empty_workspace.clone(), DockPolicy::default())
        .expect("empty close fixture is valid");
    let mut host = TestPresentationHost::new(&mut engine);
    let expected = engine.version();
    let empty_rejection = submit_input(
        &mut engine,
        &mut host,
        CLOSE_INPUT_SOURCE,
        EngineInput::RequestContentClose {
            expected,
            target: ContentCloseTarget::Root(ROOT_A),
        },
    )
    .expect("empty root close request reduces");
    assert!(matches!(
        empty_rejection.reduced_inputs()[0].outcome(),
        dockspace::transition::InputOutcome::ContentCloseRejected {
            target: ContentCloseTarget::Root(ROOT_A),
            reason: dockspace::transition::ContentCloseRequestRejection::RootEmpty { root: ROOT_A },
            ..
        }
    ));
    assert_eq!(engine.workspace(), &empty_workspace);
    assert!(empty_rejection.events().is_empty());

    let expected = engine.version();
    let missing_rejection = submit_input(
        &mut engine,
        &mut host,
        CLOSE_INPUT_SOURCE,
        EngineInput::RequestContentClose {
            expected,
            target: ContentCloseTarget::Root(ROOT_B),
        },
    )
    .expect("missing root close request reduces");
    assert!(matches!(
        missing_rejection.reduced_inputs()[0].outcome(),
        dockspace::transition::InputOutcome::ContentCloseRejected {
            target: ContentCloseTarget::Root(ROOT_B),
            reason: dockspace::transition::ContentCloseRequestRejection::RootUnavailable {
                root: ROOT_B,
            },
            ..
        }
    ));
    assert_eq!(engine.workspace(), &empty_workspace);
    assert!(missing_rejection.events().is_empty());
}

#[test]
fn ordinary_workspace_transactions_roll_back_without_a_close_command() {
    let (mut workspace, closing_node, failing_node) = two_roots();
    let selection = workspace
        .capture_item_source(ROOT_A, closing_node, item(2))
        .expect("selection source exists");
    let nonempty_b = workspace
        .capture_node_source(ROOT_B, failing_node)
        .expect("second root source exists");
    let before_batch = workspace.clone();
    let batch_error = WorkspaceTransaction::from_commands([
        WorkspaceCommand::Select { source: selection },
        WorkspaceCommand::RemoveEmptyRoot { source: nonempty_b },
    ])
    .apply(&mut workspace, &DockPolicySnapshot::default())
    .expect_err("a later command failure rolls back ordinary topology mutation");
    assert!(matches!(
        batch_error,
        TransactionError::Command {
            index: 1,
            source: CommandError::RootNotEmpty {
                root: ROOT_B,
                items: 2,
            },
        }
    ));
    assert_eq!(workspace, before_batch);
}
