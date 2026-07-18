use dockspace::command::{
    DockFraction, DockTarget, Edge, MovePayload, RootContent, RootPresentationTarget,
    WorkspaceCommand,
};
use dockspace::error::{CommandError, TransactionError};
use dockspace::geometry::LogicalRect;
use dockspace::graph::{
    Axis, Node, RootRecord, SplitWeight, SurfacePresentation, Workspace, WorkspaceBuilder,
};
use dockspace::ids::{FloatingPresentationId, ItemId, NodeId, RootId, SurfaceId};
use dockspace::policy::DockPolicy;
use dockspace::transaction::WorkspaceTransaction;

const ROOT_A: RootId = RootId::new(1);
const ROOT_B: RootId = RootId::new(2);
const ROOT_C: RootId = RootId::new(3);
const SURFACE_A: SurfaceId = SurfaceId::new(1);
const SURFACE_B: SurfaceId = SurfaceId::new(2);
const SURFACE_C: SurfaceId = SurfaceId::new(3);

fn item(value: u64) -> ItemId {
    ItemId::new(value)
}

fn rect(x: f64, y: f64) -> LogicalRect {
    LogicalRect::new(x, y, 320.0, 240.0).expect("test rectangle is valid")
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

fn create_contained(
    workspace: &mut Workspace,
    root: RootId,
    floating: FloatingPresentationId,
    z_order: u64,
    content_item: ItemId,
) {
    create_contained_at(
        workspace,
        root,
        floating,
        rect(0.0, 0.0),
        z_order,
        content_item,
    );
}

fn create_contained_at(
    workspace: &mut Workspace,
    root: RootId,
    floating: FloatingPresentationId,
    bounds: LogicalRect,
    z_order: u64,
    content_item: ItemId,
) {
    apply(
        workspace,
        &DockPolicy::default(),
        WorkspaceCommand::CreateContainedRoot {
            surface: SURFACE_A,
            root,
            floating,
            rect: bounds,
            z_order,
            content: RootContent::OpenItem(content_item),
        },
    )
    .expect("contained root is created");
}

fn simple_workspace() -> (Workspace, NodeId) {
    let mut builder = WorkspaceBuilder::new();
    let tabs = builder.insert_node(Node::tabs([item(1), item(2), item(3)]));
    builder.set_root(ROOT_A, RootRecord::new(tabs));
    builder.set_surface(SURFACE_A, SurfacePresentation::new(ROOT_A));
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
    let report = WorkspaceTransaction::from_commands([WorkspaceCommand::RehomeRoot {
        source: source.clone(),
        target: RootPresentationTarget::Contained {
            surface: SURFACE_B,
            floating,
            rect: rect(30.0, 40.0),
            z_order: 2,
        },
    }])
    .apply(workspace, &DockPolicy::default())
    .expect("same contained presentation is a checked no-op");
    assert!(!report.changed());
    assert!(matches!(
        report.outcomes(),
        [dockspace::command::CommandOutcome::RootRehomed { changed: false, .. }]
    ));
    assert_eq!(&*workspace, &before);

    let requested = FloatingPresentationId::new(11);
    let error = apply(
        workspace,
        &DockPolicy::default(),
        WorkspaceCommand::RehomeRoot {
            source,
            target: RootPresentationTarget::Contained {
                surface: SURFACE_B,
                floating: requested,
                rect: rect(30.0, 40.0),
                z_order: 2,
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
    builder.set_surface(SURFACE_A, SurfacePresentation::new(ROOT_A));
    builder.set_surface(SURFACE_B, SurfacePresentation::new(ROOT_B));
    let mut workspace = builder.build().expect("tiny weight is still valid");

    let source = workspace
        .capture_item_source(ROOT_A, source_tabs, item(1))
        .expect("source exists");
    let target = workspace
        .capture_edge_target(
            ROOT_B,
            tiny_target,
            Edge::Left,
            DockFraction::new(f32::MIN_POSITIVE).expect("fraction is positive"),
        )
        .expect("target exists");
    let before = workspace.clone();
    let error = apply(
        &mut workspace,
        &DockPolicy::default(),
        WorkspaceCommand::Move {
            payload: MovePayload::Item(source),
            target: DockTarget::Edge(target),
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
fn command_index_failure_rolls_back_earlier_staged_commands() {
    let (mut workspace, tabs) = simple_workspace();
    let selection = workspace
        .capture_item_source(ROOT_A, tabs, item(2))
        .expect("selection exists");
    let close = workspace
        .capture_item_source(ROOT_A, tabs, item(1))
        .expect("close source exists");
    let transaction = WorkspaceTransaction::from_commands([
        WorkspaceCommand::Select { source: selection },
        WorkspaceCommand::Close { source: close },
    ]);
    let before = workspace.clone();
    let error = transaction
        .apply(&mut workspace, &DockPolicy::default())
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
    builder.set_surface(SURFACE_A, SurfacePresentation::new(ROOT_A));
    let mut workspace = builder.build().expect("workspace is valid");
    let source = workspace
        .capture_node_source(ROOT_A, subtree)
        .expect("source exists");
    let target = workspace
        .capture_edge_target(
            ROOT_A,
            descendant,
            Edge::Right,
            DockFraction::new(0.5).expect("fraction is valid"),
        )
        .expect("target exists");
    let before = workspace.clone();
    let error = apply(
        &mut workspace,
        &DockPolicy::default(),
        WorkspaceCommand::Move {
            payload: MovePayload::Subtree(source),
            target: DockTarget::Edge(target),
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
        .apply(&mut workspace, &DockPolicy::default())
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
    builder.set_surface(SURFACE_A, SurfacePresentation::new(ROOT_A));
    builder.set_surface(SURFACE_B, SurfacePresentation::new(ROOT_B));
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
        &DockPolicy::default(),
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
        &DockPolicy::default(),
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
    builder.set_surface(SURFACE_A, SurfacePresentation::new(ROOT_A));
    builder.set_surface(SURFACE_B, SurfacePresentation::new(ROOT_B));
    let mut workspace = builder.build().expect("workspace is valid");
    let source = workspace
        .capture_item_source(ROOT_A, source_tabs, item(1))
        .expect("source exists");
    let target = workspace
        .capture_edge_target(
            ROOT_B,
            left,
            Edge::Left,
            DockFraction::new(0.25).expect("fraction is valid"),
        )
        .expect("target exists");
    apply(
        &mut workspace,
        &DockPolicy::default(),
        WorkspaceCommand::Move {
            payload: MovePayload::Item(source),
            target: DockTarget::Edge(target),
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
    builder.set_surface(SURFACE_A, SurfacePresentation::new(ROOT_A));
    let mut workspace = builder.build().expect("workspace is valid");
    let source = workspace
        .capture_item_source(ROOT_A, tabs, item(1))
        .expect("source exists");
    let target = workspace
        .capture_edge_target(
            ROOT_A,
            tabs,
            Edge::Right,
            DockFraction::new(0.5).expect("fraction is valid"),
        )
        .expect("target exists");
    let before = workspace.clone();
    let report = WorkspaceTransaction::from_commands([WorkspaceCommand::Move {
        payload: MovePayload::Item(source),
        target: DockTarget::Edge(target),
    }])
    .apply(&mut workspace, &DockPolicy::default())
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
    builder.set_surface(SURFACE_A, SurfacePresentation::new(ROOT_A));
    let mut workspace = builder.build().expect("workspace is valid");
    let source = workspace
        .capture_item_source(ROOT_A, tabs, item(1))
        .expect("source exists");
    let before = workspace.clone();
    let error = apply(
        &mut workspace,
        &DockPolicy::default(),
        WorkspaceCommand::CreateContainedRoot {
            surface: SURFACE_A,
            root: ROOT_B,
            floating: FloatingPresentationId::new(10),
            rect: rect(0.0, 0.0),
            z_order: 0,
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
    builder.set_surface(SURFACE_A, SurfacePresentation::new(ROOT_A));
    builder.set_surface(SURFACE_B, SurfacePresentation::new(ROOT_B));
    builder.set_contained_floating(dockspace::graph::ContainedFloating::new(
        floating,
        ROOT_C,
        SURFACE_A,
        rect(10.0, 20.0),
        7,
    ));
    builder
        .attach_contained(SURFACE_A, floating)
        .expect("source surface exists");
    let mut workspace = builder.build().expect("workspace is valid");
    let source = workspace
        .capture_node_source(ROOT_C, floating_node)
        .expect("floating root exists");

    apply(
        &mut workspace,
        &DockPolicy::default(),
        WorkspaceCommand::RehomeRoot {
            source,
            target: RootPresentationTarget::Contained {
                surface: SURFACE_B,
                floating,
                rect: rect(30.0, 40.0),
                z_order: 9,
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
    assert_eq!(record.surface, SURFACE_B);
    assert_eq!(record.rect, rect(30.0, 40.0));
    assert_eq!(record.z_order, 9);

    let source = workspace
        .capture_node_source(ROOT_C, floating_node)
        .expect("re-homed root remains capturable");
    let mut native_policy = DockPolicy::default();
    native_policy.set_allow_native_surfaces(true);
    apply(
        &mut workspace,
        &native_policy,
        WorkspaceCommand::RehomeRoot {
            source,
            target: RootPresentationTarget::Surface { surface: SURFACE_C },
        },
    )
    .expect("contained root can become a surface main root");
    assert_eq!(
        workspace.root(ROOT_C),
        Some(&RootRecord::new(floating_node))
    );
    assert_eq!(
        workspace.surface(SURFACE_C),
        Some(&SurfacePresentation::new(ROOT_C))
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
fn rehoming_main_root_to_contained_requires_a_surviving_host_and_stable_floating_id() {
    let mut builder = WorkspaceBuilder::new();
    let main_a = builder.insert_node(Node::tabs([item(1)]));
    let main_b = builder.insert_node(Node::tabs([item(2)]));
    let floating = FloatingPresentationId::new(10);
    builder.set_root(ROOT_A, RootRecord::new(main_a));
    builder.set_root(ROOT_B, RootRecord::new(main_b));
    builder.set_surface(SURFACE_A, SurfacePresentation::new(ROOT_A));
    builder.set_surface(SURFACE_B, SurfacePresentation::new(ROOT_B));
    let mut workspace = builder.build().expect("workspace is valid");
    let source = workspace
        .capture_node_source(ROOT_A, main_a)
        .expect("main root exists");
    let before = workspace.clone();

    let error = apply(
        &mut workspace,
        &DockPolicy::default(),
        WorkspaceCommand::RehomeRoot {
            source: source.clone(),
            target: RootPresentationTarget::Contained {
                surface: SURFACE_A,
                floating,
                rect: rect(10.0, 20.0),
                z_order: 1,
            },
        },
    )
    .expect_err("a main root cannot become a child of its own disappearing surface");
    assert!(matches!(
        error,
        TransactionError::Command {
            source: CommandError::ContainedHostWouldBeRemoved {
                surface: SURFACE_A,
                root: ROOT_A,
            },
            ..
        }
    ));
    assert_eq!(workspace, before);

    apply(
        &mut workspace,
        &DockPolicy::default(),
        WorkspaceCommand::RehomeRoot {
            source,
            target: RootPresentationTarget::Contained {
                surface: SURFACE_B,
                floating,
                rect: rect(30.0, 40.0),
                z_order: 2,
            },
        },
    )
    .expect("another main surface can host the root");
    assert_eq!(workspace.root(ROOT_A), Some(&RootRecord::new(main_a)));
    assert!(workspace.surface(SURFACE_A).is_none());
    assert_eq!(
        workspace
            .contained_floating(floating)
            .map(|record| (record.root, record.surface)),
        Some((ROOT_A, SURFACE_B))
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
        &DockPolicy::default(),
        WorkspaceCommand::CreateContainedRoot {
            surface: SURFACE_A,
            root: ROOT_B,
            floating: FloatingPresentationId::new(10),
            rect: rect(0.0, 0.0),
            z_order: 0,
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
fn contained_creation_rect_raise_and_last_close_share_one_transaction_boundary() {
    let (mut workspace, _) = simple_workspace();
    let floating_a = FloatingPresentationId::new(10);
    let floating_b = FloatingPresentationId::new(11);
    create_contained_at(
        &mut workspace,
        ROOT_B,
        floating_a,
        rect(10.0, 20.0),
        1,
        item(20),
    );
    let root_c = RootId::new(3);
    create_contained_at(
        &mut workspace,
        root_c,
        floating_b,
        rect(30.0, 40.0),
        2,
        item(30),
    );
    apply(
        &mut workspace,
        &DockPolicy::default(),
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
        &DockPolicy::default(),
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
    let expected_frontmost = workspace
        .contained_frontmost(SURFACE_A)
        .expect("surface exists")
        .expect("a contained presentation exists");
    apply(
        &mut workspace,
        &DockPolicy::default(),
        WorkspaceCommand::RaiseContained {
            surface: SURFACE_A,
            root: ROOT_B,
            floating: floating_a,
            expected_z_order: 1,
            expected_frontmost,
        },
    )
    .expect("explicit focus raises the presentation");
    assert_eq!(
        workspace
            .contained_floating(floating_a)
            .expect("floating remains")
            .z_order,
        3
    );

    let contained_tabs = tabs_containing(&workspace, item(20));
    let source = workspace
        .capture_item_source(ROOT_B, contained_tabs, item(20))
        .expect("contained item source exists");
    apply(
        &mut workspace,
        &DockPolicy::default(),
        WorkspaceCommand::Close { source },
    )
    .expect("last contained item close removes its presentation");
    assert!(workspace.root(ROOT_B).is_none());
    assert!(workspace.contained_floating(floating_a).is_none());
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
fn contained_equal_z_orders_are_valid_but_raise_overflow_fails_atomically() {
    let (mut workspace, _) = simple_workspace();
    let floating_a = FloatingPresentationId::new(10);
    let floating_b = FloatingPresentationId::new(11);
    create_contained(&mut workspace, ROOT_B, floating_a, 0, item(20));
    let root_c = RootId::new(3);
    create_contained(&mut workspace, root_c, floating_b, u64::MAX, item(30));
    let expected_frontmost = workspace
        .contained_frontmost(SURFACE_A)
        .expect("surface exists")
        .expect("a contained presentation exists");
    let before_overflow = workspace.clone();
    let error = apply(
        &mut workspace,
        &DockPolicy::default(),
        WorkspaceCommand::RaiseContained {
            surface: SURFACE_A,
            root: ROOT_B,
            floating: floating_a,
            expected_z_order: 0,
            expected_frontmost,
        },
    )
    .expect_err("raising above u64::MAX fails");
    assert!(matches!(
        error,
        TransactionError::Command {
            source: CommandError::ZOrderOverflow { floating },
            ..
        } if floating == floating_a
    ));
    assert_eq!(workspace, before_overflow);

    let root_d = RootId::new(4);
    let floating_c = FloatingPresentationId::new(12);
    let stale_raise = WorkspaceCommand::RaiseContained {
        surface: SURFACE_A,
        root: ROOT_B,
        floating: floating_a,
        expected_z_order: 0,
        expected_frontmost,
    };
    create_contained(&mut workspace, root_d, floating_c, u64::MAX, item(40));
    let after_peer_change = workspace.clone();
    let error = apply(&mut workspace, &DockPolicy::default(), stale_raise)
        .expect_err("a peer change invalidates the frozen stacking precondition");
    assert!(matches!(
        error,
        TransactionError::Command {
            source: CommandError::StaleContainedStack {
                surface: SURFACE_A,
                ..
            },
            ..
        }
    ));
    assert_eq!(workspace, after_peer_change);

    let expected_frontmost = workspace
        .contained_frontmost(SURFACE_A)
        .expect("surface exists")
        .expect("a contained presentation exists");
    let before_frontmost_raise = workspace.clone();
    let report = WorkspaceTransaction::from_commands([WorkspaceCommand::RaiseContained {
        surface: SURFACE_A,
        root: root_d,
        floating: floating_c,
        expected_z_order: u64::MAX,
        expected_frontmost,
    }])
    .apply(&mut workspace, &DockPolicy::default())
    .expect("larger stable identity already wins the tied maximum");
    assert!(matches!(
        report.outcomes(),
        [dockspace::command::CommandOutcome::ContainedRaised { changed: false, .. }]
    ));
    assert_eq!(workspace, before_frontmost_raise);

    let before_duplicate_raise = workspace.clone();
    let error = apply(
        &mut workspace,
        &DockPolicy::default(),
        WorkspaceCommand::RaiseContained {
            surface: SURFACE_A,
            root: root_c,
            floating: floating_b,
            expected_z_order: u64::MAX,
            expected_frontmost,
        },
    )
    .expect_err("a tied maximum cannot be raised past u64::MAX");
    assert!(matches!(
        error,
        TransactionError::Command {
            source: CommandError::ZOrderOverflow { floating },
            ..
        } if floating == floating_b
    ));
    assert_eq!(workspace, before_duplicate_raise);
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
    builder.set_surface(SURFACE_A, SurfacePresentation::new(ROOT_A));
    let mut workspace = builder.build().expect("deep workspace is valid");
    let source = workspace
        .capture_node_source(ROOT_A, root_node)
        .expect("deep source fingerprint is iterative");
    let target = workspace
        .capture_edge_target(
            ROOT_A,
            descendant,
            Edge::Right,
            DockFraction::new(0.5).expect("fraction is valid"),
        )
        .expect("deep target fingerprint is iterative");
    let before = workspace.clone();
    let error = apply(
        &mut workspace,
        &DockPolicy::default(),
        WorkspaceCommand::Move {
            payload: MovePayload::Subtree(source),
            target: DockTarget::Edge(target),
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
