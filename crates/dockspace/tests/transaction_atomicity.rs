use super::support;

use dockspace::canonical::CanonicalizationError;
use dockspace::command::{
    CloseCommitOutcome, ContainedPosition, ContentCloseTarget, DockFraction, DockTarget, Edge,
    MovePayload, RootContent, RootPresentationTarget, WorkspaceCommand,
};
use dockspace::engine::{DockEngine, EngineInput};
use dockspace::error::{CommandError, TransactionError};
use dockspace::geometry::LogicalRect;
use dockspace::graph::{
    Axis, ContainedFloating, Node, RootRecord, SplitWeight, SurfacePresentation, Workspace,
    WorkspaceBuilder,
};
use dockspace::ids::{
    FloatingPresentationId, ItemId, NodeId, RootId, StableInputSourceId, SurfaceId,
};
use dockspace::policy::{DockPolicy, DockPolicySnapshot, PolicyRevision};
use dockspace::transaction::WorkspaceTransaction;
use dockspace::validation::WorkspaceValidationError;
use support::{TestPresentationHost, submit_input};

const ROOT_A: RootId = RootId::new(1);
const ROOT_B: RootId = RootId::new(2);
const ROOT_C: RootId = RootId::new(3);
const SURFACE_A: SurfaceId = SurfaceId::new(1);
const SURFACE_B: SurfaceId = SurfaceId::new(2);
const SURFACE_C: SurfaceId = SurfaceId::new(3);
const CLOSE_INPUT_SOURCE: StableInputSourceId = StableInputSourceId::new(0xC002);

fn item(value: u64) -> ItemId {
    ItemId::new(value)
}

fn rect(x: f64, y: f64) -> LogicalRect {
    LogicalRect::new(x, y, 320.0, 240.0).expect("test rectangle is valid")
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
                    dockspace::event::WorkspaceEventKind::CloseCommitted(actual)
                        if actual == outcome
                )
            }));
            committed = Some(outcome.clone());
        }
    }
    *workspace = engine.workspace().clone();
    committed.expect("final close decision must commit")
}

fn create_contained(
    workspace: &mut Workspace,
    root: RootId,
    floating: FloatingPresentationId,
    content_item: ItemId,
) {
    create_contained_at(workspace, root, floating, rect(0.0, 0.0), content_item);
}

fn create_contained_at(
    workspace: &mut Workspace,
    root: RootId,
    floating: FloatingPresentationId,
    bounds: LogicalRect,
    content_item: ItemId,
) {
    apply(
        workspace,
        &DockPolicySnapshot::default(),
        WorkspaceCommand::CreateContainedRoot {
            surface: SURFACE_A,
            root,
            floating,
            rect: bounds,
            position: ContainedPosition::Front,
            content: RootContent::OpenItem(content_item),
        },
    )
    .expect("contained root is created");
}

fn simple_workspace() -> (Workspace, NodeId) {
    let mut builder = WorkspaceBuilder::new();
    let tabs = builder.insert_node(Node::tabs([item(1), item(2), item(3)]));
    builder.set_root(ROOT_A, RootRecord::new(tabs));
    builder.set_surface(SURFACE_A, SurfacePresentation::with_main(ROOT_A));
    (builder.build().expect("workspace is valid"), tabs)
}

fn assert_contained_rehome_identity_is_stable(
    workspace: &mut Workspace,
    node: NodeId,
    floating: FloatingPresentationId,
) {
    let source = workspace
        .capture_node_source(ROOT_A, node)
        .expect("contained root remains capturable");
    let before = workspace.clone();
    let error = WorkspaceTransaction::from_commands([WorkspaceCommand::RehomeRoot {
        source: source.clone(),
        target: RootPresentationTarget::Contained {
            surface: SURFACE_B,
            floating,
            rect: rect(30.0, 40.0),
            position: ContainedPosition::Front,
        },
    }])
    .apply(workspace, &DockPolicySnapshot::default())
    .expect_err("same-surface contained metadata needs its dedicated commands");
    assert!(matches!(
        error,
        TransactionError::Command {
            source: CommandError::RehomeMetadataRequiresDedicatedCommand { floating: actual },
            ..
        } if actual == floating
    ));
    assert_eq!(&*workspace, &before);

    let requested = FloatingPresentationId::new(11);
    let error = apply(
        workspace,
        &DockPolicySnapshot::default(),
        WorkspaceCommand::RehomeRoot {
            source,
            target: RootPresentationTarget::Contained {
                surface: SURFACE_B,
                floating: requested,
                rect: rect(30.0, 40.0),
                position: ContainedPosition::Front,
            },
        },
    )
    .expect_err("contained presentation identity cannot change during rehome");
    assert!(matches!(
        error,
        TransactionError::Command {
            source: CommandError::FloatingIdentityWouldChange {
                root: ROOT_A,
                existing,
                requested: actual,
            },
            ..
        } if existing == floating && actual == requested
    ));
    assert_eq!(&*workspace, &before);
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
fn ae1_post_extraction_edge_failure_discards_the_candidate() {
    let mut builder = WorkspaceBuilder::new();
    let source_tabs = builder.insert_node(Node::tabs([item(1)]));
    let tiny_target = builder.insert_node(Node::tabs([item(2)]));
    let normal_target = builder.insert_node(Node::tabs([item(3)]));
    let target_split = builder.insert_node(Node::Split {
        axis: Axis::Horizontal,
        children: vec![tiny_target, normal_target],
        weights: vec![
            SplitWeight::new(f32::MIN_POSITIVE).expect("positive tiny weight"),
            SplitWeight::new(1.0).expect("positive unit weight"),
        ],
    });
    builder.set_root(ROOT_A, RootRecord::new(source_tabs));
    builder.set_root(ROOT_B, RootRecord::new(target_split));
    builder.set_surface(SURFACE_A, SurfacePresentation::with_main(ROOT_A));
    builder.set_surface(SURFACE_B, SurfacePresentation::with_main(ROOT_B));
    let mut workspace = builder.build().expect("tiny weight is still valid");

    let source = workspace
        .capture_item_source(ROOT_A, source_tabs, item(1))
        .expect("source exists");
    let target = workspace
        .capture_inner_edge_target(
            ROOT_B,
            tiny_target,
            Edge::Left,
            DockFraction::new(f32::MIN_POSITIVE).expect("fraction is positive"),
        )
        .expect("target exists");
    let before = workspace.clone();
    let error = apply(
        &mut workspace,
        &DockPolicySnapshot::default(),
        WorkspaceCommand::Move {
            payload: MovePayload::Item(source),
            target: DockTarget::InnerEdge(target),
        },
    )
    .expect_err("multiplication underflow must fail instead of repairing weights");
    assert!(matches!(
        error,
        TransactionError::Command {
            index: 0,
            source: CommandError::InvalidEdgeWeight {
                axis: Axis::Horizontal
            }
        }
    ));
    assert_eq!(workspace, before);
}

#[test]
fn ordinary_command_index_failure_rolls_back_earlier_staged_commands() {
    let (mut workspace, tabs) = simple_workspace();
    let selection = workspace
        .capture_item_source(ROOT_A, tabs, item(2))
        .expect("selection exists");
    let stale_selection = workspace
        .capture_item_source(ROOT_A, tabs, item(3))
        .expect("second selection exists");
    let transaction = WorkspaceTransaction::from_commands([
        WorkspaceCommand::Select { source: selection },
        WorkspaceCommand::Select {
            source: stale_selection,
        },
    ]);
    let before = workspace.clone();
    let error = transaction
        .apply(&mut workspace, &DockPolicySnapshot::default())
        .expect_err("second command sees the first command's changed root snapshot");
    assert!(matches!(
        error,
        TransactionError::Command {
            index: 1,
            source: CommandError::StaleNode { .. }
        }
    ));
    assert_eq!(workspace, before);
}

#[test]
fn subtree_cannot_target_itself_or_a_descendant() {
    let mut builder = WorkspaceBuilder::new();
    let descendant = builder.insert_node(Node::tabs([item(1)]));
    let sibling = builder.insert_node(Node::tabs([item(2)]));
    let subtree = builder.insert_node(
        Node::equal_split(Axis::Vertical, [descendant, sibling]).expect("valid subtree"),
    );
    let outside = builder.insert_node(Node::tabs([item(3)]));
    let root_node = builder
        .insert_node(Node::equal_split(Axis::Horizontal, [subtree, outside]).expect("valid root"));
    builder.set_root(ROOT_A, RootRecord::new(root_node));
    builder.set_surface(SURFACE_A, SurfacePresentation::with_main(ROOT_A));
    let mut workspace = builder.build().expect("workspace is valid");
    let source = workspace
        .capture_node_source(ROOT_A, subtree)
        .expect("source exists");
    let target = workspace
        .capture_inner_edge_target(
            ROOT_A,
            descendant,
            Edge::Right,
            DockFraction::new(0.5).expect("fraction is valid"),
        )
        .expect("target exists");
    let before = workspace.clone();
    let error = apply(
        &mut workspace,
        &DockPolicySnapshot::default(),
        WorkspaceCommand::Move {
            payload: MovePayload::Subtree(source),
            target: DockTarget::InnerEdge(target),
        },
    )
    .expect_err("descendant target is forbidden");
    assert!(matches!(
        error,
        TransactionError::Command {
            source: CommandError::TargetInsidePayload {
                source_node,
                target
            },
            ..
        } if source_node == subtree && target == descendant
    ));
    assert_eq!(workspace, before);
}

#[test]
fn tabs_and_subtree_tabs_have_identical_self_center_noop_semantics() {
    for subtree_variant in [false, true] {
        let (mut workspace, tabs) = simple_workspace();
        let source = workspace
            .capture_node_source(ROOT_A, tabs)
            .expect("source exists");
        let target = workspace
            .capture_tab_target(ROOT_A, tabs)
            .expect("target exists");
        let payload = if subtree_variant {
            MovePayload::Subtree(source)
        } else {
            MovePayload::Tabs(source)
        };
        let before = workspace.clone();
        let report = WorkspaceTransaction::from_commands([WorkspaceCommand::Move {
            payload,
            target: DockTarget::Center(target),
        }])
        .apply(&mut workspace, &DockPolicySnapshot::default())
        .expect("self-center tabs move is a checked no-op");
        assert!(matches!(
            report.outcomes(),
            [dockspace::command::CommandOutcome::Moved { changed: false, .. }]
        ));
        assert_eq!(workspace, before);
    }
}

#[test]
fn moving_an_inner_central_subtree_is_rejected() {
    let mut builder = WorkspaceBuilder::new();
    let central = builder.insert_node(Node::tabs([item(1)]));
    let sibling = builder.insert_node(Node::tabs([item(2)]));
    let source_root_node = builder
        .insert_node(Node::equal_split(Axis::Horizontal, [central, sibling]).expect("valid split"));
    let target_tabs = builder.insert_node(Node::tabs([item(3)]));
    builder.set_root(
        ROOT_A,
        RootRecord::new(source_root_node).with_central(central),
    );
    builder.set_root(ROOT_B, RootRecord::new(target_tabs));
    builder.set_surface(SURFACE_A, SurfacePresentation::with_main(ROOT_A));
    builder.set_surface(SURFACE_B, SurfacePresentation::with_main(ROOT_B));
    let mut workspace = builder.build().expect("workspace is valid");
    let source = workspace
        .capture_node_source(ROOT_A, central)
        .expect("central source exists");
    let target = workspace
        .capture_tab_target(ROOT_B, target_tabs)
        .expect("target exists");
    let before = workspace.clone();
    let error = apply(
        &mut workspace,
        &DockPolicySnapshot::default(),
        WorkspaceCommand::Move {
            payload: MovePayload::Tabs(source),
            target: DockTarget::Center(target),
        },
    )
    .expect_err("central identity cannot silently move out of its root");
    assert!(matches!(
        error,
        TransactionError::Command {
            source: CommandError::CentralNodeWouldDetach {
                root: ROOT_A,
                source_node,
                central: central_node,
            },
            ..
        } if source_node == central && central_node == central
    ));
    assert_eq!(workspace, before);
}

#[test]
fn stable_identity_collisions_and_policy_rejections_are_typed() {
    let (mut workspace, _) = simple_workspace();
    let before = workspace.clone();
    let error = apply(
        &mut workspace,
        &DockPolicySnapshot::default(),
        WorkspaceCommand::CreateSurfaceRoot {
            surface: SURFACE_B,
            root: ROOT_B,
            content: RootContent::OpenItem(item(9)),
        },
    )
    .expect_err("native surfaces are disabled by default");
    assert!(matches!(
        error,
        TransactionError::Command {
            source: CommandError::Policy(_),
            ..
        }
    ));
    assert_eq!(workspace, before);

    let mut policy = DockPolicy::default();
    policy.set_allow_native_surfaces(true);
    let policy = policy.snapshot(PolicyRevision::default());
    let error = apply(
        &mut workspace,
        &policy,
        WorkspaceCommand::CreateSurfaceRoot {
            surface: SURFACE_B,
            root: ROOT_A,
            content: RootContent::OpenItem(item(9)),
        },
    )
    .expect_err("root identity collision is rejected");
    assert!(matches!(
        error,
        TransactionError::Command {
            source: CommandError::RootIdCollision { root: ROOT_A },
            ..
        }
    ));
    assert_eq!(workspace, before);

    let error = apply(
        &mut workspace,
        &policy,
        WorkspaceCommand::CreateSurfaceRoot {
            surface: SURFACE_A,
            root: ROOT_B,
            content: RootContent::OpenItem(item(9)),
        },
    )
    .expect_err("surface identity collision is rejected");
    assert!(matches!(
        error,
        TransactionError::Command {
            source: CommandError::SurfaceIdCollision { surface: SURFACE_A },
            ..
        }
    ));
    assert_eq!(workspace, before);
}

#[test]
fn same_axis_edge_insertion_splits_only_the_target_branch_weight() {
    let mut builder = WorkspaceBuilder::new();
    let source_tabs = builder.insert_node(Node::tabs([item(1)]));
    let left = builder.insert_node(Node::tabs([item(2)]));
    let right = builder.insert_node(Node::tabs([item(3)]));
    let target_root = builder.insert_node(Node::Split {
        axis: Axis::Horizontal,
        children: vec![left, right],
        weights: vec![
            SplitWeight::new(0.4).expect("positive"),
            SplitWeight::new(0.6).expect("positive"),
        ],
    });
    builder.set_root(ROOT_A, RootRecord::new(source_tabs));
    builder.set_root(ROOT_B, RootRecord::new(target_root));
    builder.set_surface(SURFACE_A, SurfacePresentation::with_main(ROOT_A));
    builder.set_surface(SURFACE_B, SurfacePresentation::with_main(ROOT_B));
    let mut workspace = builder.build().expect("workspace is valid");
    let source = workspace
        .capture_item_source(ROOT_A, source_tabs, item(1))
        .expect("source exists");
    let target = workspace
        .capture_inner_edge_target(
            ROOT_B,
            left,
            Edge::Left,
            DockFraction::new(0.25).expect("fraction is valid"),
        )
        .expect("target exists");
    apply(
        &mut workspace,
        &DockPolicySnapshot::default(),
        WorkspaceCommand::Move {
            payload: MovePayload::Item(source),
            target: DockTarget::InnerEdge(target),
        },
    )
    .expect("edge insertion succeeds");

    let Some(Node::Split {
        children, weights, ..
    }) = workspace.node(target_root)
    else {
        panic!("target root must remain the same split");
    };
    assert_eq!(children.len(), 3);
    assert_eq!(children[1..], [left, right]);
    let actual: Vec<f32> = weights.iter().map(|weight| weight.get()).collect();
    for (actual, expected) in actual.iter().zip([0.1_f32, 0.3, 0.6]) {
        assert!((actual - expected).abs() <= 1.0e-6);
    }
    assert_eq!(workspace.item_multiset().len(), 3);
}

#[test]
fn sole_noncentral_item_edge_to_its_own_tabs_is_a_checked_noop() {
    let mut builder = WorkspaceBuilder::new();
    let tabs = builder.insert_node(Node::tabs([item(1)]));
    builder.set_root(ROOT_A, RootRecord::new(tabs));
    builder.set_surface(SURFACE_A, SurfacePresentation::with_main(ROOT_A));
    let mut workspace = builder.build().expect("workspace is valid");
    let source = workspace
        .capture_item_source(ROOT_A, tabs, item(1))
        .expect("source exists");
    let target = workspace
        .capture_inner_edge_target(
            ROOT_A,
            tabs,
            Edge::Right,
            DockFraction::new(0.5).expect("fraction is valid"),
        )
        .expect("target exists");
    let before = workspace.clone();
    let report = WorkspaceTransaction::from_commands([WorkspaceCommand::Move {
        payload: MovePayload::Item(source),
        target: DockTarget::InnerEdge(target),
    }])
    .apply(&mut workspace, &DockPolicySnapshot::default())
    .expect("self-edge is a checked no-op");
    assert!(matches!(
        report.outcomes(),
        [dockspace::command::CommandOutcome::Moved { changed: false, .. }]
    ));
    assert_eq!(workspace, before);
}

#[test]
fn whole_root_content_requires_identity_preserving_rehome() {
    let mut builder = WorkspaceBuilder::new();
    let tabs = builder.insert_node(Node::tabs([item(1)]));
    builder.set_root(ROOT_A, RootRecord::new(tabs));
    builder.set_surface(SURFACE_A, SurfacePresentation::with_main(ROOT_A));
    let mut workspace = builder.build().expect("workspace is valid");
    let source = workspace
        .capture_item_source(ROOT_A, tabs, item(1))
        .expect("source exists");
    let before = workspace.clone();
    let error = apply(
        &mut workspace,
        &DockPolicySnapshot::default(),
        WorkspaceCommand::CreateContainedRoot {
            surface: SURFACE_A,
            root: ROOT_B,
            floating: FloatingPresentationId::new(10),
            rect: rect(0.0, 0.0),
            position: ContainedPosition::Front,
            content: RootContent::Move(MovePayload::Item(source)),
        },
    )
    .expect_err("contained presentation needs a surviving main host");
    assert!(matches!(
        error,
        TransactionError::Command {
            source: CommandError::WholeRootRequiresRehome { root: ROOT_A },
            ..
        }
    ));
    assert_eq!(workspace, before);
}

#[test]
fn rehoming_between_contained_hosts_preserves_root_node_and_floating_identity() {
    let mut builder = WorkspaceBuilder::new();
    let main_a = builder.insert_node(Node::tabs([item(1)]));
    let main_b = builder.insert_node(Node::tabs([item(2)]));
    let floating_node = builder.insert_node(Node::tabs([item(3)]));
    let floating = FloatingPresentationId::new(10);
    builder.set_root(ROOT_A, RootRecord::new(main_a));
    builder.set_root(ROOT_B, RootRecord::new(main_b));
    builder.set_root(ROOT_C, RootRecord::new(floating_node));
    builder.set_surface(SURFACE_A, SurfacePresentation::with_main(ROOT_A));
    builder.set_surface(SURFACE_B, SurfacePresentation::with_main(ROOT_B));
    builder.set_contained_floating(floating, ContainedFloating::new(ROOT_C, rect(10.0, 20.0)));
    builder
        .attach_contained(SURFACE_A, floating)
        .expect("source surface exists");
    let mut workspace = builder.build().expect("workspace is valid");
    let source = workspace
        .capture_node_source(ROOT_C, floating_node)
        .expect("floating root exists");

    apply(
        &mut workspace,
        &DockPolicySnapshot::default(),
        WorkspaceCommand::RehomeRoot {
            source,
            target: RootPresentationTarget::Contained {
                surface: SURFACE_B,
                floating,
                rect: rect(30.0, 40.0),
                position: ContainedPosition::Front,
            },
        },
    )
    .expect("contained root rehome succeeds");

    assert_eq!(
        workspace.root(ROOT_C),
        Some(&RootRecord::new(floating_node))
    );
    assert_eq!(
        workspace
            .surface(SURFACE_A)
            .expect("source host survives")
            .contained,
        []
    );
    assert_eq!(
        workspace
            .surface(SURFACE_B)
            .expect("target host exists")
            .contained,
        [floating]
    );
    let record = workspace
        .contained_floating(floating)
        .expect("floating identity survives");
    assert_eq!(record.root, ROOT_C);
    assert_eq!(record.rect, rect(30.0, 40.0));
    assert_eq!(
        workspace.presentation_for_root(ROOT_C),
        Some(dockspace::RootPresentationOwner::Contained {
            surface: SURFACE_B,
            floating,
        })
    );

    let source = workspace
        .capture_node_source(ROOT_C, floating_node)
        .expect("re-homed root remains capturable");
    let mut native_policy = DockPolicy::default();
    native_policy.set_allow_native_surfaces(true);
    let native_policy = native_policy.snapshot(PolicyRevision::default());
    apply(
        &mut workspace,
        &native_policy,
        WorkspaceCommand::RehomeRoot {
            source,
            target: RootPresentationTarget::NewSurface { surface: SURFACE_C },
        },
    )
    .expect("contained root can become a surface main root");
    assert_eq!(
        workspace.root(ROOT_C),
        Some(&RootRecord::new(floating_node))
    );
    assert_eq!(
        workspace.surface(SURFACE_C),
        Some(&SurfacePresentation::with_main(ROOT_C))
    );
    assert!(workspace.contained_floating(floating).is_none());
    assert_eq!(
        workspace
            .surface(SURFACE_B)
            .expect("previous host survives")
            .contained,
        []
    );
}

#[test]
fn rehoming_main_root_to_contained_supports_rootless_hosts_and_stable_floating_id() {
    let mut builder = WorkspaceBuilder::new();
    let main_a = builder.insert_node(Node::tabs([item(1)]));
    let main_b = builder.insert_node(Node::tabs([item(2)]));
    let floating = FloatingPresentationId::new(10);
    builder.set_root(ROOT_A, RootRecord::new(main_a));
    builder.set_root(ROOT_B, RootRecord::new(main_b));
    builder.set_surface(SURFACE_A, SurfacePresentation::with_main(ROOT_A));
    builder.set_surface(SURFACE_B, SurfacePresentation::with_main(ROOT_B));
    let mut workspace = builder.build().expect("workspace is valid");
    let source = workspace
        .capture_node_source(ROOT_A, main_a)
        .expect("main root exists");
    apply(
        &mut workspace,
        &DockPolicySnapshot::default(),
        WorkspaceCommand::RehomeRoot {
            source,
            target: RootPresentationTarget::Contained {
                surface: SURFACE_A,
                floating,
                rect: rect(10.0, 20.0),
                position: ContainedPosition::Front,
            },
        },
    )
    .expect("a main root can become contained on the same rootless surface");
    assert_eq!(
        workspace.surface(SURFACE_A),
        Some(&SurfacePresentation {
            main_root: None,
            contained: vec![floating],
        })
    );
    assert_eq!(
        workspace.presentation_for_root(ROOT_A),
        Some(dockspace::RootPresentationOwner::Contained {
            surface: SURFACE_A,
            floating,
        })
    );

    let source = workspace
        .capture_node_source(ROOT_A, main_a)
        .expect("contained root remains capturable");
    apply(
        &mut workspace,
        &DockPolicySnapshot::default(),
        WorkspaceCommand::RehomeRoot {
            source,
            target: RootPresentationTarget::Contained {
                surface: SURFACE_B,
                floating,
                rect: rect(30.0, 40.0),
                position: ContainedPosition::Front,
            },
        },
    )
    .expect("the same contained identity can move to another surface");
    assert_eq!(workspace.root(ROOT_A), Some(&RootRecord::new(main_a)));
    assert!(workspace.surface(SURFACE_A).is_none());
    assert_eq!(
        workspace.presentation_for_root(ROOT_A),
        Some(dockspace::RootPresentationOwner::Contained {
            surface: SURFACE_B,
            floating,
        })
    );

    assert_contained_rehome_identity_is_stable(&mut workspace, main_a, floating);
}

#[test]
fn moving_an_item_to_a_contained_root_preserves_single_ownership() {
    let (mut workspace, tabs) = simple_workspace();
    let source = workspace
        .capture_item_source(ROOT_A, tabs, item(3))
        .expect("source exists");
    apply(
        &mut workspace,
        &DockPolicySnapshot::default(),
        WorkspaceCommand::CreateContainedRoot {
            surface: SURFACE_A,
            root: ROOT_B,
            floating: FloatingPresentationId::new(10),
            rect: rect(0.0, 0.0),
            position: ContainedPosition::Front,
            content: RootContent::Move(MovePayload::Item(source)),
        },
    )
    .expect("main root survives the contained move");
    assert_eq!(workspace.item_multiset().get(&item(3)), Some(&1));
    let moved_tabs = tabs_containing(&workspace, item(3));
    assert_eq!(
        workspace.root(ROOT_B).map(|record| record.node),
        Some(moved_tabs)
    );
    workspace.validate().expect("result remains strict");
}

#[test]
#[allow(
    clippy::too_many_lines,
    reason = "the transaction sequence proves creation, raise, and final close share one boundary"
)]
fn contained_creation_rect_raise_and_last_close_share_one_transaction_boundary() {
    let (mut workspace, _) = simple_workspace();
    let floating_a = FloatingPresentationId::new(10);
    let floating_b = FloatingPresentationId::new(11);
    create_contained_at(
        &mut workspace,
        ROOT_B,
        floating_a,
        rect(10.0, 20.0),
        item(20),
    );
    let root_c = RootId::new(3);
    create_contained_at(
        &mut workspace,
        root_c,
        floating_b,
        rect(30.0, 40.0),
        item(30),
    );
    apply(
        &mut workspace,
        &DockPolicySnapshot::default(),
        WorkspaceCommand::UpdateContainedRect {
            surface: SURFACE_A,
            root: ROOT_B,
            floating: floating_a,
            expected_rect: rect(10.0, 20.0),
            rect: rect(50.0, 60.0),
        },
    )
    .expect("contained rectangle updates");
    let after_rect_update = workspace.clone();
    let error = apply(
        &mut workspace,
        &DockPolicySnapshot::default(),
        WorkspaceCommand::UpdateContainedRect {
            surface: SURFACE_A,
            root: ROOT_B,
            floating: floating_a,
            expected_rect: rect(10.0, 20.0),
            rect: rect(70.0, 80.0),
        },
    )
    .expect_err("stale rectangle precondition fails");
    assert!(matches!(
        error,
        TransactionError::Command {
            source: CommandError::StaleContainedRect { floating, .. },
            ..
        } if floating == floating_a
    ));
    assert_eq!(workspace, after_rect_update);
    let source = workspace
        .capture_node_source(
            ROOT_B,
            workspace.root(ROOT_B).expect("contained root exists").node,
        )
        .expect("contained source is current");
    let expected_roster = workspace
        .capture_contained_roster(SURFACE_A)
        .expect("contained roster is current");
    apply(
        &mut workspace,
        &DockPolicySnapshot::default(),
        WorkspaceCommand::RaiseContained {
            source,
            floating: floating_a,
            expected_roster,
        },
    )
    .expect("explicit focus raises the presentation");
    assert_eq!(
        workspace
            .surface(SURFACE_A)
            .expect("main surface survives")
            .contained,
        [floating_b, floating_a]
    );
    assert_eq!(
        workspace.presentation_for_root(ROOT_B),
        Some(dockspace::RootPresentationOwner::Contained {
            surface: SURFACE_A,
            floating: floating_a,
        })
    );

    assert_eq!(
        close(&mut workspace, ContentCloseTarget::Item(item(20))),
        CloseCommitOutcome::ItemClosed {
            item: item(20),
            root: ROOT_B,
        }
    );
    assert!(workspace.root(ROOT_B).is_none());
    assert!(workspace.contained_floating(floating_a).is_none());
    assert!(workspace.presentation_for_root(ROOT_B).is_none());
    assert_eq!(
        workspace
            .surface(SURFACE_A)
            .expect("main surface survives")
            .contained,
        vec![floating_b]
    );
    assert_eq!(workspace.item_multiset().get(&item(20)), None);
    assert_eq!(workspace.item_multiset().get(&item(30)), Some(&1));
    workspace.validate().expect("contained cleanup is strict");
}

#[test]
fn non_positive_contained_rect_update_is_rejected_atomically() {
    let (mut workspace, _) = simple_workspace();
    let floating = FloatingPresentationId::new(10);
    let original = rect(10.0, 20.0);
    create_contained_at(&mut workspace, ROOT_B, floating, original, item(20));
    let before = workspace.clone();
    let zero_width = LogicalRect::new(10.0, 20.0, 0.0, 240.0)
        .expect("zero-width logical rectangles are representable command inputs");

    let error = apply(
        &mut workspace,
        &DockPolicySnapshot::default(),
        WorkspaceCommand::UpdateContainedRect {
            surface: SURFACE_A,
            root: ROOT_B,
            floating,
            expected_rect: original,
            rect: zero_width,
        },
    )
    .expect_err("zero-area durable geometry must not publish");

    let TransactionError::Canonicalization(CanonicalizationError::InvalidOutput(errors)) = error
    else {
        panic!("zero-area candidate must fail strict canonical output validation: {error:?}");
    };
    assert!(errors.errors().iter().any(|error| matches!(
        error,
        WorkspaceValidationError::NonPositiveContainedRect {
            floating: id,
            width,
            height,
        } if *id == floating
            && width.to_bits() == 0.0_f64.to_bits()
            && height.to_bits() == 240.0_f64.to_bits()
    )));
    assert_eq!(workspace, before);
}

#[test]
#[allow(
    clippy::too_many_lines,
    reason = "the transaction sequence proves stale roster rejection without partial mutation"
)]
fn contained_raise_uses_a_frozen_roster_and_rejects_stale_peers_atomically() {
    let (mut workspace, _) = simple_workspace();
    let floating_a = FloatingPresentationId::new(10);
    let floating_b = FloatingPresentationId::new(11);
    create_contained(&mut workspace, ROOT_B, floating_a, item(20));
    let root_c = RootId::new(3);
    create_contained(&mut workspace, root_c, floating_b, item(30));
    assert_eq!(
        workspace
            .surface(SURFACE_A)
            .expect("surface exists")
            .contained,
        [floating_a, floating_b]
    );

    let root_d = RootId::new(4);
    let floating_c = FloatingPresentationId::new(12);
    let source = workspace
        .capture_node_source(
            ROOT_B,
            workspace.root(ROOT_B).expect("contained root exists").node,
        )
        .expect("contained source is current");
    let expected_roster = workspace
        .capture_contained_roster(SURFACE_A)
        .expect("contained roster is current");
    let stale_raise = WorkspaceCommand::RaiseContained {
        source,
        floating: floating_a,
        expected_roster,
    };
    create_contained(&mut workspace, root_d, floating_c, item(40));
    let after_peer_change = workspace.clone();
    let error = apply(&mut workspace, &DockPolicySnapshot::default(), stale_raise)
        .expect_err("a peer change invalidates the frozen roster precondition");
    assert!(matches!(
        error,
        TransactionError::Command {
            source: CommandError::StaleContainedRoster {
                surface: SURFACE_A,
                ..
            },
            ..
        }
    ));
    assert_eq!(workspace, after_peer_change);

    let source = workspace
        .capture_node_source(
            ROOT_B,
            workspace.root(ROOT_B).expect("contained root exists").node,
        )
        .expect("contained source remains current");
    let expected_roster = workspace
        .capture_contained_roster(SURFACE_A)
        .expect("refreshed roster is current");
    let report = WorkspaceTransaction::from_commands([WorkspaceCommand::RaiseContained {
        source,
        floating: floating_a,
        expected_roster,
    }])
    .apply(&mut workspace, &DockPolicySnapshot::default())
    .expect("a refreshed roster permits the structural raise");
    assert!(matches!(
        report.outcomes(),
        [dockspace::command::CommandOutcome::ContainedRaised {
            from: 0,
            to: 2,
            changed: true,
            ..
        }]
    ));
    assert_eq!(
        workspace
            .surface(SURFACE_A)
            .expect("surface exists")
            .contained,
        [floating_b, floating_c, floating_a]
    );
    assert_eq!(
        workspace.presentation_for_root(ROOT_B),
        Some(dockspace::RootPresentationOwner::Contained {
            surface: SURFACE_A,
            floating: floating_a,
        })
    );

    let source = workspace
        .capture_node_source(
            ROOT_B,
            workspace.root(ROOT_B).expect("contained root exists").node,
        )
        .expect("frontmost source is current");
    let expected_roster = workspace
        .capture_contained_roster(SURFACE_A)
        .expect("frontmost roster is current");
    let before_idempotent_raise = workspace.clone();
    let report = WorkspaceTransaction::from_commands([WorkspaceCommand::RaiseContained {
        source,
        floating: floating_a,
        expected_roster,
    }])
    .apply(&mut workspace, &DockPolicySnapshot::default())
    .expect("raising the frontmost presentation is idempotent");
    assert!(matches!(
        report.outcomes(),
        [dockspace::command::CommandOutcome::ContainedRaised { changed: false, .. }]
    ));
    assert_eq!(workspace, before_idempotent_raise);
}

#[test]
fn deep_root_fingerprints_and_descendant_checks_are_iterative() {
    const DEPTH: usize = 10_000;

    let mut builder = WorkspaceBuilder::new();
    let descendant = builder.insert_node(Node::tabs([item(1)]));
    let mut root_node = descendant;
    for depth in 0..DEPTH {
        let side = builder.insert_node(Node::tabs([item(
            u64::try_from(depth).expect("depth fits u64") + 2,
        )]));
        let axis = if depth % 2 == 0 {
            Axis::Horizontal
        } else {
            Axis::Vertical
        };
        root_node = builder
            .insert_node(Node::equal_split(axis, [root_node, side]).expect("deep split is valid"));
    }
    builder.set_root(ROOT_A, RootRecord::new(root_node));
    builder.set_surface(SURFACE_A, SurfacePresentation::with_main(ROOT_A));
    let mut workspace = builder.build().expect("deep workspace is valid");
    let source = workspace
        .capture_node_source(ROOT_A, root_node)
        .expect("deep source fingerprint is iterative");
    let target = workspace
        .capture_inner_edge_target(
            ROOT_A,
            descendant,
            Edge::Right,
            DockFraction::new(0.5).expect("fraction is valid"),
        )
        .expect("deep target fingerprint is iterative");
    let before = workspace.clone();
    let error = apply(
        &mut workspace,
        &DockPolicySnapshot::default(),
        WorkspaceCommand::Move {
            payload: MovePayload::Subtree(source),
            target: DockTarget::InnerEdge(target),
        },
    )
    .expect_err("root cannot dock into its own deepest descendant");
    assert!(matches!(
        error,
        TransactionError::Command {
            source: CommandError::TargetInsidePayload { .. },
            ..
        }
    ));
    assert_eq!(workspace, before);
}
