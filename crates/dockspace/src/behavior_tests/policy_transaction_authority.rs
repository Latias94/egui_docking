use dockspace::command::{
    ContainedPosition, DockFraction, DockTarget, Edge, MovePayload, RootContent,
    RootPresentationTarget, WorkspaceCommand,
};
use dockspace::error::{CommandError, TransactionError};
use dockspace::geometry::LogicalRect;
use dockspace::graph::{
    Axis, ContainedFloating, Node, RootRecord, SplitWeight, SurfacePresentation, Workspace,
    WorkspaceBuilder,
};
use dockspace::ids::{FloatingPresentationId, ItemId, RootId, SurfaceId};
use dockspace::policy::{
    DockClassId, DockItemRule, DockOperation, DockPayloadKind, DockPolicy, DockPolicySnapshot,
    DockPresentationMode, DockSourceRule, DockSurfaceRule, DockTargetRule, DockTargetRuleKey,
    PolicyRejection, PolicyRevision, PolicyRuleScope, TabBarInteraction, TabBarPolicy,
    TabBarVisibility,
};
use dockspace::transaction::WorkspaceTransaction;

const SOURCE_ROOT: RootId = RootId::new(1);
const TARGET_ROOT: RootId = RootId::new(2);
const SOURCE_SURFACE: SurfaceId = SurfaceId::new(10);
const TARGET_SURFACE: SurfaceId = SurfaceId::new(20);
const SOURCE_ITEM: ItemId = ItemId::new(100);
const SOURCE_PEER: ItemId = ItemId::new(101);
const TARGET_ITEM: ItemId = ItemId::new(200);
const TARGET_PEER: ItemId = ItemId::new(201);
const NEW_ROOT: RootId = RootId::new(3);
const NEW_SURFACE: SurfaceId = SurfaceId::new(30);
const FLOATING: FloatingPresentationId = FloatingPresentationId::new(1);
const PEER_FLOATING: FloatingPresentationId = FloatingPresentationId::new(2);

fn rect(x: f64) -> LogicalRect {
    LogicalRect::new(x, 10.0, 320.0, 240.0).expect("test rectangle is valid")
}

fn two_root_workspace() -> (Workspace, dockspace::ids::NodeId, dockspace::ids::NodeId) {
    two_root_workspace_with_target_central(false)
}

fn two_root_workspace_with_target_central(
    target_is_central: bool,
) -> (Workspace, dockspace::ids::NodeId, dockspace::ids::NodeId) {
    let mut builder = WorkspaceBuilder::new();
    let source_tabs = builder.insert_node(Node::tabs([SOURCE_ITEM, SOURCE_PEER]));
    let target_tabs = builder.insert_node(Node::tabs([TARGET_ITEM, TARGET_PEER]));
    builder.set_root(SOURCE_ROOT, RootRecord::new(source_tabs));
    builder.set_root(
        TARGET_ROOT,
        if target_is_central {
            RootRecord::new(target_tabs).with_central(target_tabs)
        } else {
            RootRecord::new(target_tabs)
        },
    );
    builder.set_surface(SOURCE_SURFACE, SurfacePresentation::with_main(SOURCE_ROOT));
    builder.set_surface(TARGET_SURFACE, SurfacePresentation::with_main(TARGET_ROOT));
    (
        builder.build().expect("policy test workspace is valid"),
        source_tabs,
        target_tabs,
    )
}

fn snapshot(policy: &DockPolicy) -> DockPolicySnapshot {
    policy.snapshot(PolicyRevision::new(1))
}

fn assert_policy_rejection(error: TransactionError, expected: PolicyRejection) {
    match error {
        TransactionError::Command {
            index: 0,
            source: CommandError::Policy(actual),
        } => assert_eq!(actual, expected),
        actual => panic!("expected policy rejection {expected:?}, got {actual:?}"),
    }
}

fn assert_surface_target_payload_rejection(
    mut workspace: Workspace,
    policy: &DockPolicy,
    command: WorkspaceCommand,
    surface: SurfaceId,
    payload: DockPayloadKind,
) {
    let before = workspace.clone();
    let error = WorkspaceTransaction::from_commands([command])
        .apply(&mut workspace, &snapshot(policy))
        .expect_err("the presentation target must reject the payload kind");
    assert_policy_rejection(
        error,
        PolicyRejection::SurfaceTargetPayloadRejected { surface, payload },
    );
    assert_eq!(workspace, before);
}

fn item_move_to_center(
    workspace: &Workspace,
    source_tabs: dockspace::ids::NodeId,
    target_tabs: dockspace::ids::NodeId,
    item: ItemId,
) -> WorkspaceCommand {
    WorkspaceCommand::Move {
        payload: MovePayload::Item(
            workspace
                .capture_item_source(SOURCE_ROOT, source_tabs, item)
                .expect("source item is capturable"),
        ),
        target: DockTarget::Center(
            workspace
                .capture_tab_target(TARGET_ROOT, target_tabs)
                .expect("target tabs are capturable"),
        ),
    }
}

#[test]
fn direct_item_move_rejects_disabled_item_source_atomically() {
    let (mut workspace, source_tabs, target_tabs) = two_root_workspace();
    let source = workspace
        .capture_item_source(SOURCE_ROOT, source_tabs, SOURCE_ITEM)
        .expect("source item is capturable");
    let target = workspace
        .capture_tab_target(TARGET_ROOT, target_tabs)
        .expect("target tabs are capturable");

    let mut item_rule = DockItemRule::new();
    item_rule.set_source_enabled(false);
    let mut policy = DockPolicy::new();
    policy.set_item_rule(SOURCE_ITEM, item_rule);
    let before = workspace.clone();

    let error = WorkspaceTransaction::from_commands([WorkspaceCommand::Move {
        payload: MovePayload::Item(source),
        target: DockTarget::Center(target),
    }])
    .apply(&mut workspace, &snapshot(&policy))
    .expect_err("the transaction boundary must enforce item source policy");

    assert!(matches!(
        error,
        TransactionError::Command {
            index: 0,
            source: CommandError::Policy(PolicyRejection::ItemSourceDisabled { item: SOURCE_ITEM }),
        }
    ));
    assert_eq!(workspace, before);
}

#[test]
fn direct_move_enforces_root_and_surface_source_rules() {
    let (workspace, source_tabs, target_tabs) = two_root_workspace();

    let mut source_rule = DockSourceRule::new();
    source_rule.set_enabled(false);
    let mut root_policy = DockPolicy::new();
    root_policy.set_source_rule(SOURCE_ROOT, source_rule);
    let mut root_workspace = workspace.clone();
    let before = root_workspace.clone();
    let error = WorkspaceTransaction::from_commands([item_move_to_center(
        &root_workspace,
        source_tabs,
        target_tabs,
        SOURCE_ITEM,
    )])
    .apply(&mut root_workspace, &snapshot(&root_policy))
    .expect_err("disabled source root must reject direct transactions");
    assert_policy_rejection(error, PolicyRejection::SourceDisabled { root: SOURCE_ROOT });
    assert_eq!(root_workspace, before);

    let mut surface_rule = DockSurfaceRule::new();
    surface_rule.set_source_enabled(false);
    let mut surface_policy = DockPolicy::new();
    surface_policy.set_surface_rule(SOURCE_SURFACE, surface_rule);
    let mut surface_workspace = workspace;
    let before = surface_workspace.clone();
    let error = WorkspaceTransaction::from_commands([item_move_to_center(
        &surface_workspace,
        source_tabs,
        target_tabs,
        SOURCE_ITEM,
    )])
    .apply(&mut surface_workspace, &snapshot(&surface_policy))
    .expect_err("disabled source surface must reject direct transactions");
    assert_policy_rejection(
        error,
        PolicyRejection::SurfaceSourceDisabled {
            surface: SOURCE_SURFACE,
        },
    );
    assert_eq!(surface_workspace, before);
}

#[test]
fn no_undocking_rejects_cross_stack_move_but_allows_same_stack_reorder() {
    let mut item_rule = DockItemRule::new();
    item_rule.set_allow_undocking(false);
    let mut policy = DockPolicy::new();
    policy.set_item_rule(SOURCE_PEER, item_rule);

    let (mut cross_workspace, source_tabs, target_tabs) = two_root_workspace();
    let before = cross_workspace.clone();
    let error = WorkspaceTransaction::from_commands([item_move_to_center(
        &cross_workspace,
        source_tabs,
        target_tabs,
        SOURCE_PEER,
    )])
    .apply(&mut cross_workspace, &snapshot(&policy))
    .expect_err("cross-stack movement leaves the current docking owner");
    assert_policy_rejection(
        error,
        PolicyRejection::ItemUndockingDisabled { item: SOURCE_PEER },
    );
    assert_eq!(cross_workspace, before);

    let (mut reorder_workspace, source_tabs, _) = two_root_workspace();
    let source = reorder_workspace
        .capture_item_source(SOURCE_ROOT, source_tabs, SOURCE_PEER)
        .expect("same-stack source is capturable");
    let target = reorder_workspace
        .capture_tab_target(SOURCE_ROOT, source_tabs)
        .expect("same-stack target is capturable");
    WorkspaceTransaction::from_commands([WorkspaceCommand::Move {
        payload: MovePayload::Item(source),
        target: DockTarget::TabGap { target, index: 0 },
    }])
    .apply(&mut reorder_workspace, &snapshot(&policy))
    .expect("same-stack reorder does not undock the item");
}

#[test]
fn pane_local_and_outer_edges_use_distinct_exact_target_rules() {
    let fraction = DockFraction::new(0.5).expect("test fraction is valid");

    let mut item_target = DockTargetRule::new();
    item_target.set_enabled(false);
    let mut item_policy = DockPolicy::new();
    item_policy.set_target_rule(DockTargetRuleKey::Item(TARGET_ITEM), item_target);

    let (mut inner_workspace, source_tabs, target_tabs) = two_root_workspace();
    let source = inner_workspace
        .capture_item_source(SOURCE_ROOT, source_tabs, SOURCE_ITEM)
        .expect("source is capturable");
    let inner = inner_workspace
        .capture_inner_edge_target(TARGET_ROOT, target_tabs, Edge::Left, fraction)
        .expect("inner target is capturable");
    let before = inner_workspace.clone();
    let error = WorkspaceTransaction::from_commands([WorkspaceCommand::Move {
        payload: MovePayload::Item(source),
        target: DockTarget::InnerEdge(inner),
    }])
    .apply(&mut inner_workspace, &snapshot(&item_policy))
    .expect_err("inner edge must use selected-item target policy");
    assert_policy_rejection(
        error,
        PolicyRejection::TargetDisabled {
            target: DockTargetRuleKey::Item(TARGET_ITEM),
        },
    );
    assert_eq!(inner_workspace, before);

    let (mut outer_workspace, source_tabs, _) = two_root_workspace();
    let source = outer_workspace
        .capture_item_source(SOURCE_ROOT, source_tabs, SOURCE_ITEM)
        .expect("source is capturable");
    let outer = outer_workspace
        .capture_outer_edge_target(TARGET_ROOT, Edge::Left, fraction)
        .expect("outer target is capturable");
    WorkspaceTransaction::from_commands([WorkspaceCommand::Move {
        payload: MovePayload::Item(source),
        target: DockTarget::OuterEdge(outer),
    }])
    .apply(&mut outer_workspace, &snapshot(&item_policy))
    .expect("item target rule must not leak onto the outer root edge");

    let mut root_target = DockTargetRule::new();
    root_target.set_enabled(false);
    let mut root_policy = DockPolicy::new();
    root_policy.set_target_rule(DockTargetRuleKey::Root(TARGET_ROOT), root_target);
    let (mut root_workspace, source_tabs, _) = two_root_workspace();
    let source = root_workspace
        .capture_item_source(SOURCE_ROOT, source_tabs, SOURCE_ITEM)
        .expect("source is capturable");
    let outer = root_workspace
        .capture_outer_edge_target(TARGET_ROOT, Edge::Right, fraction)
        .expect("outer target is capturable");
    let error = WorkspaceTransaction::from_commands([WorkspaceCommand::Move {
        payload: MovePayload::Item(source),
        target: DockTarget::OuterEdge(outer),
    }])
    .apply(&mut root_workspace, &snapshot(&root_policy))
    .expect_err("outer edge must use the exact root target rule");
    assert_policy_rejection(
        error,
        PolicyRejection::TargetDisabled {
            target: DockTargetRuleKey::Root(TARGET_ROOT),
        },
    );
}

#[test]
fn empty_central_leaf_uses_the_exact_root_target_rule() {
    let mut builder = WorkspaceBuilder::new();
    let source = builder.insert_node(Node::tabs([SOURCE_ITEM]));
    let empty_central = builder.insert_node(Node::tabs([]));
    builder.set_root(SOURCE_ROOT, RootRecord::new(source));
    builder.set_root(
        TARGET_ROOT,
        RootRecord::new(empty_central).with_central(empty_central),
    );
    builder.set_surface(SOURCE_SURFACE, SurfacePresentation::with_main(SOURCE_ROOT));
    builder.set_surface(TARGET_SURFACE, SurfacePresentation::with_main(TARGET_ROOT));
    let mut workspace = builder
        .build()
        .expect("empty central target workspace is valid");

    let mut target_rule = DockTargetRule::new();
    target_rule.set_enabled(false);
    let mut policy = DockPolicy::new();
    policy.set_target_rule(DockTargetRuleKey::Root(TARGET_ROOT), target_rule);
    let before = workspace.clone();

    let error = WorkspaceTransaction::from_commands([WorkspaceCommand::Move {
        payload: MovePayload::Item(
            workspace
                .capture_item_source(SOURCE_ROOT, source, SOURCE_ITEM)
                .expect("source item is capturable"),
        ),
        target: DockTarget::Center(
            workspace
                .capture_tab_target(TARGET_ROOT, empty_central)
                .expect("empty central target is capturable"),
        ),
    }])
    .apply(&mut workspace, &snapshot(&policy))
    .expect_err("empty central target must obey its root-scoped policy");

    assert_policy_rejection(
        error,
        PolicyRejection::TargetDisabled {
            target: DockTargetRuleKey::Root(TARGET_ROOT),
        },
    );
    assert_eq!(workspace, before);
}

#[test]
fn changing_selected_target_item_invalidates_the_frozen_command() {
    let (mut workspace, source_tabs, target_tabs) = two_root_workspace();
    let command = item_move_to_center(&workspace, source_tabs, target_tabs, SOURCE_ITEM);
    let selection = workspace
        .capture_item_source(TARGET_ROOT, target_tabs, TARGET_PEER)
        .expect("target peer is selectable");
    WorkspaceTransaction::from_commands([WorkspaceCommand::Select { source: selection }])
        .apply(&mut workspace, &DockPolicySnapshot::default())
        .expect("selection changes target semantics");
    let before = workspace.clone();

    let error = WorkspaceTransaction::from_commands([command])
        .apply(&mut workspace, &DockPolicySnapshot::default())
        .expect_err("old selected-item target proof must be stale");
    assert!(matches!(
        error,
        TransactionError::Command {
            index: 0,
            source: CommandError::StaleNode { .. },
        }
    ));
    assert_eq!(workspace, before);
}

#[test]
fn edge_target_scope_cannot_be_relabelled_by_the_command() {
    let (mut workspace, source_tabs, _) = two_root_workspace();
    let source = workspace
        .capture_item_source(SOURCE_ROOT, source_tabs, SOURCE_ITEM)
        .expect("source is capturable");
    let outer = workspace
        .capture_outer_edge_target(
            TARGET_ROOT,
            Edge::Left,
            DockFraction::new(0.5).expect("test fraction is valid"),
        )
        .expect("outer edge is capturable");
    let before = workspace.clone();

    let error = WorkspaceTransaction::from_commands([WorkspaceCommand::Move {
        payload: MovePayload::Item(source),
        target: DockTarget::InnerEdge(outer),
    }])
    .apply(&mut workspace, &DockPolicySnapshot::default())
    .expect_err("an outer proof cannot be relabelled as an inner target");
    assert!(matches!(
        error,
        TransactionError::Command {
            index: 0,
            source: CommandError::EdgeTargetScopeMismatch { .. },
        }
    ));
    assert_eq!(workspace, before);
}

#[test]
fn central_and_target_surface_class_rules_are_final_commit_authority() {
    let (mut central_workspace, source_tabs, target_tabs) =
        two_root_workspace_with_target_central(true);
    let mut central_policy = DockPolicy::new();
    central_policy.central_node_mut().set_allow_tab_merge(false);
    let before = central_workspace.clone();
    let error = WorkspaceTransaction::from_commands([item_move_to_center(
        &central_workspace,
        source_tabs,
        target_tabs,
        SOURCE_ITEM,
    )])
    .apply(&mut central_workspace, &snapshot(&central_policy))
    .expect_err("central target restriction must reject direct commit");
    assert_policy_rejection(
        error,
        PolicyRejection::CentralNodeOperationDisabled {
            operation: dockspace::policy::DockDropOperation::TabMerge,
        },
    );
    assert_eq!(central_workspace, before);

    let editor = DockClassId::new(1);
    let inspector = DockClassId::new(2);
    let mut source_item = DockItemRule::new();
    source_item.set_dock_class(Some(editor));
    let mut target_surface = DockSurfaceRule::new();
    target_surface.set_accepted_classes([inspector], false);
    let mut class_policy = DockPolicy::new();
    class_policy.set_item_rule(SOURCE_ITEM, source_item);
    class_policy.set_surface_rule(TARGET_SURFACE, target_surface);
    let (mut class_workspace, source_tabs, target_tabs) = two_root_workspace();
    let before = class_workspace.clone();
    let error = WorkspaceTransaction::from_commands([item_move_to_center(
        &class_workspace,
        source_tabs,
        target_tabs,
        SOURCE_ITEM,
    )])
    .apply(&mut class_workspace, &snapshot(&class_policy))
    .expect_err("target surface class restriction must reject direct commit");
    assert_policy_rejection(
        error,
        PolicyRejection::DockClassRejected {
            item: SOURCE_ITEM,
            class: Some(editor),
            scope: PolicyRuleScope::Surface(TARGET_SURFACE),
        },
    );
    assert_eq!(class_workspace, before);
}

#[test]
fn tab_group_and_subtree_policy_checks_every_payload_item() {
    let editor = DockClassId::new(1);
    let inspector = DockClassId::new(2);
    let mut editor_item = DockItemRule::new();
    editor_item.set_dock_class(Some(editor));
    let mut inspector_item = DockItemRule::new();
    inspector_item.set_dock_class(Some(inspector));
    let mut target_surface = DockSurfaceRule::new();
    target_surface.set_accepted_classes([editor], false);
    let mut policy = DockPolicy::new();
    policy.set_item_rule(SOURCE_ITEM, editor_item);
    policy.set_item_rule(SOURCE_PEER, inspector_item);
    policy.set_surface_rule(TARGET_SURFACE, target_surface);

    let (mut tabs_workspace, source_tabs, target_tabs) = two_root_workspace();
    let source = tabs_workspace
        .capture_node_source(SOURCE_ROOT, source_tabs)
        .expect("tab-group source is capturable");
    let target = tabs_workspace
        .capture_tab_target(TARGET_ROOT, target_tabs)
        .expect("target tabs are capturable");
    let before = tabs_workspace.clone();
    let error = WorkspaceTransaction::from_commands([WorkspaceCommand::Move {
        payload: MovePayload::Tabs(source),
        target: DockTarget::Center(target),
    }])
    .apply(&mut tabs_workspace, &snapshot(&policy))
    .expect_err("one incompatible tab rejects the complete tab-group payload");
    assert_policy_rejection(
        error,
        PolicyRejection::DockClassRejected {
            item: SOURCE_PEER,
            class: Some(inspector),
            scope: PolicyRuleScope::Surface(TARGET_SURFACE),
        },
    );
    assert_eq!(tabs_workspace, before);

    let mut builder = WorkspaceBuilder::new();
    let source_a = builder.insert_node(Node::tabs([SOURCE_ITEM]));
    let source_b = builder.insert_node(Node::tabs([SOURCE_PEER]));
    let source_split = builder.insert_node(Node::Split {
        axis: Axis::Horizontal,
        children: vec![source_a, source_b],
        weights: vec![
            SplitWeight::new(0.5).expect("weight is valid"),
            SplitWeight::new(0.5).expect("weight is valid"),
        ],
    });
    let target_tabs = builder.insert_node(Node::tabs([TARGET_ITEM]));
    builder.set_root(SOURCE_ROOT, RootRecord::new(source_split));
    builder.set_root(TARGET_ROOT, RootRecord::new(target_tabs));
    builder.set_surface(SOURCE_SURFACE, SurfacePresentation::with_main(SOURCE_ROOT));
    builder.set_surface(TARGET_SURFACE, SurfacePresentation::with_main(TARGET_ROOT));
    let mut subtree_workspace = builder.build().expect("subtree workspace is valid");
    let source = subtree_workspace
        .capture_node_source(SOURCE_ROOT, source_split)
        .expect("subtree source is capturable");
    let target = subtree_workspace
        .capture_inner_edge_target(
            TARGET_ROOT,
            target_tabs,
            Edge::Left,
            DockFraction::new(0.5).expect("test fraction is valid"),
        )
        .expect("inner edge is capturable");
    let before = subtree_workspace.clone();
    let error = WorkspaceTransaction::from_commands([WorkspaceCommand::Move {
        payload: MovePayload::Subtree(source),
        target: DockTarget::InnerEdge(target),
    }])
    .apply(&mut subtree_workspace, &snapshot(&policy))
    .expect_err("one incompatible descendant rejects the complete subtree payload");
    assert_policy_rejection(
        error,
        PolicyRejection::DockClassRejected {
            item: SOURCE_PEER,
            class: Some(inspector),
            scope: PolicyRuleScope::Surface(TARGET_SURFACE),
        },
    );
    assert_eq!(subtree_workspace, before);
}

#[test]
fn reorder_and_tab_gap_commands_enforce_effective_tab_bar_policy() {
    let (mut workspace, source_tabs, _) = two_root_workspace();
    let source = workspace
        .capture_item_source(SOURCE_ROOT, source_tabs, SOURCE_PEER)
        .expect("reorder source is capturable");
    let mut policy = DockPolicy::new();
    policy.set_tab_bar(TabBarPolicy::new(
        TabBarVisibility::Hidden,
        TabBarInteraction::Enabled,
    ));
    let before = workspace.clone();

    let error = WorkspaceTransaction::from_commands([WorkspaceCommand::Reorder {
        source,
        insertion_index: 0,
    }])
    .apply(&mut workspace, &snapshot(&policy))
    .expect_err("hidden tab bars cannot be bypassed by direct reorder commands");
    assert_policy_rejection(error, PolicyRejection::TabBarHidden);
    assert_eq!(workspace, before);
}

#[test]
fn open_uses_exact_target_policy_without_fabricating_source_facts() {
    let (mut workspace, _, target_tabs) = two_root_workspace();
    let target = workspace
        .capture_tab_target(TARGET_ROOT, target_tabs)
        .expect("target tabs are capturable");
    let mut target_rule = DockTargetRule::new();
    target_rule.set_enabled(false);
    let mut policy = DockPolicy::new();
    policy.set_target_rule(DockTargetRuleKey::Item(TARGET_ITEM), target_rule);
    let before = workspace.clone();

    let error = WorkspaceTransaction::from_commands([WorkspaceCommand::Open {
        item: ItemId::new(999),
        target: DockTarget::Center(target),
    }])
    .apply(&mut workspace, &snapshot(&policy))
    .expect_err("new content must still obey the exact destination rule");
    assert_policy_rejection(
        error,
        PolicyRejection::TargetDisabled {
            target: DockTargetRuleKey::Item(TARGET_ITEM),
        },
    );
    assert_eq!(workspace, before);

    let opened = ItemId::new(1_000);
    let (mut workspace, _, target_tabs) = two_root_workspace();
    let target = workspace
        .capture_tab_target(TARGET_ROOT, target_tabs)
        .expect("target tabs are capturable");
    let mut item_rule = DockItemRule::new();
    item_rule.set_source_enabled(false);
    let mut policy = DockPolicy::new();
    policy.set_item_rule(opened, item_rule);
    WorkspaceTransaction::from_commands([WorkspaceCommand::Open {
        item: opened,
        target: DockTarget::Center(target),
    }])
    .apply(&mut workspace, &snapshot(&policy))
    .expect("newly opened content has no source owner to disable");
}

#[test]
fn create_and_rehome_commands_enforce_exact_presentation_target_surface() {
    let (workspace, source_tabs, _) = two_root_workspace();
    let source = workspace
        .capture_item_source(SOURCE_ROOT, source_tabs, SOURCE_ITEM)
        .expect("source item is capturable");
    let mut disabled_native_target = DockSurfaceRule::new();
    disabled_native_target.set_target_enabled(false);
    let mut native_policy = DockPolicy::new();
    native_policy.set_allow_native_surfaces(true);
    native_policy.set_surface_rule(NEW_SURFACE, disabled_native_target);
    let mut native_workspace = workspace.clone();
    let before = native_workspace.clone();
    let error = WorkspaceTransaction::from_commands([WorkspaceCommand::CreateSurfaceRoot {
        surface: NEW_SURFACE,
        root: NEW_ROOT,
        content: RootContent::Move(MovePayload::Item(source)),
    }])
    .apply(&mut native_workspace, &snapshot(&native_policy))
    .expect_err("native creation must evaluate the new logical surface rule");
    assert_policy_rejection(
        error,
        PolicyRejection::SurfaceTargetDisabled {
            surface: NEW_SURFACE,
        },
    );
    assert_eq!(native_workspace, before);

    let root_node = workspace
        .root(SOURCE_ROOT)
        .expect("source root exists")
        .node;
    let source = workspace
        .capture_node_source(SOURCE_ROOT, root_node)
        .expect("complete source root is capturable");
    let mut tiled_only = DockSurfaceRule::new();
    tiled_only.set_allowed_presentations([DockPresentationMode::Tiled]);
    let mut contained_policy = DockPolicy::new();
    contained_policy.set_surface_rule(TARGET_SURFACE, tiled_only);
    let mut contained_workspace = workspace;
    let before = contained_workspace.clone();
    let error = WorkspaceTransaction::from_commands([WorkspaceCommand::RehomeRoot {
        source,
        target: RootPresentationTarget::Contained {
            surface: TARGET_SURFACE,
            floating: FLOATING,
            rect: rect(20.0),
            position: ContainedPosition::Front,
        },
    }])
    .apply(&mut contained_workspace, &snapshot(&contained_policy))
    .expect_err("rehome must obey target-surface presentation policy");
    assert_policy_rejection(
        error,
        PolicyRejection::SurfacePresentationModeRejected {
            surface: TARGET_SURFACE,
            mode: DockPresentationMode::Contained,
        },
    );
    assert_eq!(contained_workspace, before);
}

fn contained_only_workspace() -> (Workspace, dockspace::ids::NodeId) {
    let mut builder = WorkspaceBuilder::new();
    let tabs = builder.insert_node(Node::tabs([SOURCE_ITEM]));
    let target_tabs = builder.insert_node(Node::tabs([TARGET_ITEM]));
    builder.set_root(SOURCE_ROOT, RootRecord::new(tabs));
    builder.set_root(TARGET_ROOT, RootRecord::new(target_tabs));
    builder.set_surface(
        SOURCE_SURFACE,
        SurfacePresentation {
            main_root: None,
            contained: vec![FLOATING],
        },
    );
    builder.set_contained_floating(FLOATING, ContainedFloating::new(SOURCE_ROOT, rect(10.0)));
    builder.set_surface(TARGET_SURFACE, SurfacePresentation::with_main(TARGET_ROOT));
    (
        builder.build().expect("contained-only workspace is valid"),
        tabs,
    )
}

fn contained_stack_workspace() -> (Workspace, dockspace::ids::NodeId) {
    let mut builder = WorkspaceBuilder::new();
    let source_tabs = builder.insert_node(Node::tabs([SOURCE_ITEM]));
    let peer_tabs = builder.insert_node(Node::tabs([TARGET_ITEM]));
    builder.set_root(SOURCE_ROOT, RootRecord::new(source_tabs));
    builder.set_root(TARGET_ROOT, RootRecord::new(peer_tabs));
    builder.set_surface(
        SOURCE_SURFACE,
        SurfacePresentation {
            main_root: None,
            contained: vec![FLOATING, PEER_FLOATING],
        },
    );
    builder.set_contained_floating(FLOATING, ContainedFloating::new(SOURCE_ROOT, rect(10.0)));
    builder.set_contained_floating(
        PEER_FLOATING,
        ContainedFloating::new(TARGET_ROOT, rect(30.0)),
    );
    (
        builder.build().expect("contained stack workspace is valid"),
        source_tabs,
    )
}

#[test]
fn raise_contained_enforces_transform_policy_before_changing_stacking() {
    let (mut rejected_workspace, source_tabs) = contained_stack_workspace();
    let source = rejected_workspace
        .capture_node_source(SOURCE_ROOT, source_tabs)
        .expect("rear contained root is capturable");
    let expected_roster = rejected_workspace
        .capture_contained_roster(SOURCE_SURFACE)
        .expect("contained roster is capturable");
    let mut rejected_policy = DockPolicy::new();
    rejected_policy.set_allow_contained_transform(false);
    let before = rejected_workspace.clone();

    let error = WorkspaceTransaction::from_commands([WorkspaceCommand::RaiseContained {
        source,
        floating: FLOATING,
        expected_roster,
    }])
    .apply(&mut rejected_workspace, &snapshot(&rejected_policy))
    .expect_err("disabled contained transforms must reject direct stacking raises");

    assert_policy_rejection(error, PolicyRejection::ContainedTransformDisabled);
    assert_eq!(rejected_workspace, before);

    let (mut allowed_workspace, source_tabs) = contained_stack_workspace();
    let source = allowed_workspace
        .capture_node_source(SOURCE_ROOT, source_tabs)
        .expect("rear contained root is capturable");
    let expected_roster = allowed_workspace
        .capture_contained_roster(SOURCE_SURFACE)
        .expect("contained roster is capturable");
    WorkspaceTransaction::from_commands([WorkspaceCommand::RaiseContained {
        source,
        floating: FLOATING,
        expected_roster,
    }])
    .apply(&mut allowed_workspace, &DockPolicySnapshot::default())
    .expect("default transform policy permits an exact contained raise");

    assert_eq!(
        allowed_workspace
            .surface(SOURCE_SURFACE)
            .expect("source surface remains present")
            .contained,
        vec![PEER_FLOATING, FLOATING],
    );
}

#[test]
fn raise_contained_uses_exact_contained_source_and_surface_policy_facts() {
    let (mut source_workspace, source_tabs) = contained_stack_workspace();
    let source = source_workspace
        .capture_node_source(SOURCE_ROOT, source_tabs)
        .expect("rear contained root is capturable");
    let expected_roster = source_workspace
        .capture_contained_roster(SOURCE_SURFACE)
        .expect("contained roster is capturable");
    let mut source_rule = DockSourceRule::new();
    source_rule.set_allowed_operations([]);
    let mut source_policy = DockPolicy::new();
    source_policy.set_source_rule(SOURCE_ROOT, source_rule);
    let before = source_workspace.clone();

    let error = WorkspaceTransaction::from_commands([WorkspaceCommand::RaiseContained {
        source,
        floating: FLOATING,
        expected_roster,
    }])
    .apply(&mut source_workspace, &snapshot(&source_policy))
    .expect_err("the actual contained source must authorize a direct stacking raise");

    assert_policy_rejection(
        error,
        PolicyRejection::SourceOperationRejected {
            root: SOURCE_ROOT,
            operation: DockOperation::ContainedTransform,
        },
    );
    assert_eq!(source_workspace, before);

    let (mut surface_workspace, source_tabs) = contained_stack_workspace();
    let source = surface_workspace
        .capture_node_source(SOURCE_ROOT, source_tabs)
        .expect("rear contained root is capturable");
    let expected_roster = surface_workspace
        .capture_contained_roster(SOURCE_SURFACE)
        .expect("contained roster is capturable");
    let mut surface_rule = DockSurfaceRule::new();
    surface_rule.set_target_enabled(false);
    let mut surface_policy = DockPolicy::new();
    surface_policy.set_surface_rule(SOURCE_SURFACE, surface_rule);
    let before = surface_workspace.clone();

    let error = WorkspaceTransaction::from_commands([WorkspaceCommand::RaiseContained {
        source,
        floating: FLOATING,
        expected_roster,
    }])
    .apply(&mut surface_workspace, &snapshot(&surface_policy))
    .expect_err("the actual contained surface must authorize a direct stacking raise");

    assert_policy_rejection(
        error,
        PolicyRejection::SurfaceTargetDisabled {
            surface: SOURCE_SURFACE,
        },
    );
    assert_eq!(surface_workspace, before);
}

#[test]
fn tiled_presentation_is_final_transaction_policy_authority() {
    let (mut disabled_workspace, source_tabs) = contained_only_workspace();
    let source = disabled_workspace
        .capture_node_source(SOURCE_ROOT, source_tabs)
        .expect("contained root source is capturable");
    let mut disabled_policy = DockPolicy::new();
    disabled_policy.set_allow_tiled_presentation(false);
    let before = disabled_workspace.clone();

    let error = WorkspaceTransaction::from_commands([WorkspaceCommand::PromoteContained {
        source,
        surface: SOURCE_SURFACE,
        floating: FLOATING,
    }])
    .apply(&mut disabled_workspace, &snapshot(&disabled_policy))
    .expect_err("disabled tiled presentation must reject direct contained promotion");

    assert_policy_rejection(
        error,
        PolicyRejection::PresentationModeDisabled {
            mode: DockPresentationMode::Tiled,
        },
    );
    assert_eq!(disabled_workspace, before);

    let (mut source_workspace, source_tabs) = contained_only_workspace();
    let source = source_workspace
        .capture_node_source(SOURCE_ROOT, source_tabs)
        .expect("contained root source is capturable");
    let mut source_rule = DockSourceRule::new();
    source_rule.set_allowed_operations([DockOperation::ContainedTransform]);
    let mut source_policy = DockPolicy::new();
    source_policy.set_source_rule(SOURCE_ROOT, source_rule);
    let before = source_workspace.clone();

    let error = WorkspaceTransaction::from_commands([WorkspaceCommand::PromoteContained {
        source,
        surface: SOURCE_SURFACE,
        floating: FLOATING,
    }])
    .apply(&mut source_workspace, &snapshot(&source_policy))
    .expect_err("the exact source must authorize tiled presentation");

    assert_policy_rejection(
        error,
        PolicyRejection::SourceOperationRejected {
            root: SOURCE_ROOT,
            operation: DockOperation::TiledPresentation,
        },
    );
    assert_eq!(source_workspace, before);

    let (mut allowed_workspace, source_tabs) = contained_only_workspace();
    let source = allowed_workspace
        .capture_node_source(SOURCE_ROOT, source_tabs)
        .expect("contained root source is capturable");
    WorkspaceTransaction::from_commands([WorkspaceCommand::PromoteContained {
        source,
        surface: SOURCE_SURFACE,
        floating: FLOATING,
    }])
    .apply(&mut allowed_workspace, &DockPolicySnapshot::default())
    .expect("default policy permits exact tiled presentation");

    let presentation = allowed_workspace
        .surface(SOURCE_SURFACE)
        .expect("source surface remains present after promotion");
    assert_eq!(presentation.main_root, Some(SOURCE_ROOT));
    assert!(presentation.contained.is_empty());
}

#[test]
fn contained_transform_is_local_but_cross_surface_rehome_is_undocking() {
    let (mut workspace, tabs) = contained_only_workspace();
    let mut item_rule = DockItemRule::new();
    item_rule.set_allow_undocking(false);
    let mut policy = DockPolicy::new();
    policy.set_item_rule(SOURCE_ITEM, item_rule);
    let policy = snapshot(&policy);

    WorkspaceTransaction::from_commands([WorkspaceCommand::UpdateContainedRect {
        surface: SOURCE_SURFACE,
        root: SOURCE_ROOT,
        floating: FLOATING,
        expected_rect: rect(10.0),
        rect: rect(30.0),
    }])
    .apply(&mut workspace, &policy)
    .expect("an in-place contained transform does not undock its content");

    let source = workspace
        .capture_node_source(SOURCE_ROOT, tabs)
        .expect("the transformed contained root is capturable");
    let before = workspace.clone();
    let error = WorkspaceTransaction::from_commands([WorkspaceCommand::RehomeRoot {
        source,
        target: RootPresentationTarget::Contained {
            surface: TARGET_SURFACE,
            floating: FLOATING,
            rect: rect(50.0),
            position: ContainedPosition::Front,
        },
    }])
    .apply(&mut workspace, &policy)
    .expect_err("cross-surface contained rehome must enforce no-undocking");

    assert_policy_rejection(
        error,
        PolicyRejection::ItemUndockingDisabled { item: SOURCE_ITEM },
    );
    assert_eq!(workspace, before);
}

#[test]
fn cross_surface_contained_rehome_uses_presentation_policy_not_transform_policy() {
    let (mut globally_disabled_workspace, tabs) = contained_only_workspace();
    let source = globally_disabled_workspace
        .capture_node_source(SOURCE_ROOT, tabs)
        .expect("contained root is capturable");
    let mut globally_disabled_policy = DockPolicy::new();
    globally_disabled_policy.set_allow_contained_floating(false);
    let before = globally_disabled_workspace.clone();

    let error = WorkspaceTransaction::from_commands([WorkspaceCommand::RehomeRoot {
        source,
        target: RootPresentationTarget::Contained {
            surface: TARGET_SURFACE,
            floating: FLOATING,
            rect: rect(50.0),
            position: ContainedPosition::Front,
        },
    }])
    .apply(
        &mut globally_disabled_workspace,
        &snapshot(&globally_disabled_policy),
    )
    .expect_err("cross-surface contained rehome must require contained presentation admission");

    assert_policy_rejection(
        error,
        PolicyRejection::PresentationModeDisabled {
            mode: DockPresentationMode::Contained,
        },
    );
    assert_eq!(globally_disabled_workspace, before);

    let (mut source_restricted_workspace, tabs) = contained_only_workspace();
    let source = source_restricted_workspace
        .capture_node_source(SOURCE_ROOT, tabs)
        .expect("contained root is capturable");
    let mut source_rule = DockSourceRule::new();
    source_rule.set_allowed_operations([DockOperation::ContainedTransform]);
    let mut source_restricted_policy = DockPolicy::new();
    source_restricted_policy.set_source_rule(SOURCE_ROOT, source_rule);
    let before = source_restricted_workspace.clone();

    let error = WorkspaceTransaction::from_commands([WorkspaceCommand::RehomeRoot {
        source,
        target: RootPresentationTarget::Contained {
            surface: TARGET_SURFACE,
            floating: FLOATING,
            rect: rect(50.0),
            position: ContainedPosition::Front,
        },
    }])
    .apply(
        &mut source_restricted_workspace,
        &snapshot(&source_restricted_policy),
    )
    .expect_err("cross-surface contained rehome must require contained-floating source admission");

    assert_policy_rejection(
        error,
        PolicyRejection::SourceOperationRejected {
            root: SOURCE_ROOT,
            operation: DockOperation::ContainedFloating,
        },
    );
    assert_eq!(source_restricted_workspace, before);

    let (mut target_restricted_workspace, tabs) = contained_only_workspace();
    let source = target_restricted_workspace
        .capture_node_source(SOURCE_ROOT, tabs)
        .expect("contained root is capturable");
    let mut target_rule = DockSurfaceRule::new();
    target_rule.set_allowed_presentations([DockPresentationMode::Tiled]);
    let mut target_restricted_policy = DockPolicy::new();
    target_restricted_policy.set_surface_rule(TARGET_SURFACE, target_rule);
    let before = target_restricted_workspace.clone();

    let error = WorkspaceTransaction::from_commands([WorkspaceCommand::RehomeRoot {
        source,
        target: RootPresentationTarget::Contained {
            surface: TARGET_SURFACE,
            floating: FLOATING,
            rect: rect(50.0),
            position: ContainedPosition::Front,
        },
    }])
    .apply(
        &mut target_restricted_workspace,
        &snapshot(&target_restricted_policy),
    )
    .expect_err("cross-surface contained rehome must require target contained admission");

    assert_policy_rejection(
        error,
        PolicyRejection::SurfacePresentationModeRejected {
            surface: TARGET_SURFACE,
            mode: DockPresentationMode::Contained,
        },
    );
    assert_eq!(target_restricted_workspace, before);
}

#[test]
fn promote_enforces_presentation_policy_while_contained_transform_is_independent() {
    let (mut workspace, tabs) = contained_only_workspace();
    let source = workspace
        .capture_node_source(SOURCE_ROOT, tabs)
        .expect("contained root source is capturable");
    let mut contained_only = DockSurfaceRule::new();
    contained_only.set_allowed_presentations([DockPresentationMode::Contained]);
    let mut promote_policy = DockPolicy::new();
    promote_policy.set_surface_rule(SOURCE_SURFACE, contained_only);
    let mut promote_workspace = workspace.clone();
    let before = promote_workspace.clone();
    let error = WorkspaceTransaction::from_commands([WorkspaceCommand::PromoteContained {
        source,
        surface: SOURCE_SURFACE,
        floating: FLOATING,
    }])
    .apply(&mut promote_workspace, &snapshot(&promote_policy))
    .expect_err("promotion must obey tiled presentation policy");
    assert_policy_rejection(
        error,
        PolicyRejection::SurfacePresentationModeRejected {
            surface: SOURCE_SURFACE,
            mode: DockPresentationMode::Tiled,
        },
    );
    assert_eq!(promote_workspace, before);

    let mut transform_policy = DockPolicy::new();
    transform_policy.set_allow_contained_floating(false);
    let mut transform_workspace = workspace.clone();
    WorkspaceTransaction::from_commands([WorkspaceCommand::UpdateContainedRect {
        surface: SOURCE_SURFACE,
        root: SOURCE_ROOT,
        floating: FLOATING,
        expected_rect: rect(10.0),
        rect: rect(30.0),
    }])
    .apply(&mut transform_workspace, &snapshot(&transform_policy))
    .expect("disabling future contained creation must not freeze an existing presentation");
    assert_eq!(
        transform_workspace
            .contained_floating(FLOATING)
            .expect("contained presentation remains owned")
            .rect,
        rect(30.0)
    );

    transform_policy.set_allow_contained_transform(false);
    let before = workspace.clone();
    let error = WorkspaceTransaction::from_commands([WorkspaceCommand::UpdateContainedRect {
        surface: SOURCE_SURFACE,
        root: SOURCE_ROOT,
        floating: FLOATING,
        expected_rect: rect(10.0),
        rect: rect(30.0),
    }])
    .apply(&mut workspace, &snapshot(&transform_policy))
    .expect_err("the explicit contained transform policy remains final commit authority");
    assert_policy_rejection(error, PolicyRejection::ContainedTransformDisabled);
    assert_eq!(workspace, before);

    let (mut source_workspace, _) = contained_only_workspace();
    let mut source_rule = DockSourceRule::new();
    source_rule.set_allowed_operations([]);
    let mut source_policy = DockPolicy::new();
    source_policy.set_source_rule(SOURCE_ROOT, source_rule);
    let before = source_workspace.clone();
    let error = WorkspaceTransaction::from_commands([WorkspaceCommand::UpdateContainedRect {
        surface: SOURCE_SURFACE,
        root: SOURCE_ROOT,
        floating: FLOATING,
        expected_rect: rect(10.0),
        rect: rect(30.0),
    }])
    .apply(&mut source_workspace, &snapshot(&source_policy))
    .expect_err("the exact source root must authorize contained transforms");
    assert_policy_rejection(
        error,
        PolicyRejection::SourceOperationRejected {
            root: SOURCE_ROOT,
            operation: DockOperation::ContainedTransform,
        },
    );
    assert_eq!(source_workspace, before);

    let (mut target_workspace, _) = contained_only_workspace();
    let mut target_rule = DockSurfaceRule::new();
    target_rule.set_target_enabled(false);
    let mut target_policy = DockPolicy::new();
    target_policy.set_surface_rule(SOURCE_SURFACE, target_rule);
    let before = target_workspace.clone();
    let error = WorkspaceTransaction::from_commands([WorkspaceCommand::UpdateContainedRect {
        surface: SOURCE_SURFACE,
        root: SOURCE_ROOT,
        floating: FLOATING,
        expected_rect: rect(10.0),
        rect: rect(30.0),
    }])
    .apply(&mut target_workspace, &snapshot(&target_policy))
    .expect_err("the exact target surface must authorize contained transforms");
    assert_policy_rejection(
        error,
        PolicyRejection::SurfaceTargetDisabled {
            surface: SOURCE_SURFACE,
        },
    );
    assert_eq!(target_workspace, before);
}

#[test]
fn root_presentation_commands_enforce_surface_target_payload_kind_atomically() {
    let (workspace, _, _) = two_root_workspace();
    let source_node = workspace
        .root(SOURCE_ROOT)
        .expect("source root exists")
        .node;
    let source = workspace
        .capture_node_source(SOURCE_ROOT, source_node)
        .expect("complete source root is capturable");
    let mut target_rule = DockSurfaceRule::new();
    target_rule.set_allowed_target_payloads([DockPayloadKind::Item]);
    let mut policy = DockPolicy::new();
    policy.set_surface_rule(TARGET_SURFACE, target_rule);
    assert_surface_target_payload_rejection(
        workspace,
        &policy,
        WorkspaceCommand::RehomeRoot {
            source,
            target: RootPresentationTarget::Contained {
                surface: TARGET_SURFACE,
                floating: FLOATING,
                rect: rect(20.0),
                position: ContainedPosition::Front,
            },
        },
        TARGET_SURFACE,
        DockPayloadKind::Root,
    );

    let (workspace, source_tabs) = contained_only_workspace();
    let source = workspace
        .capture_node_source(SOURCE_ROOT, source_tabs)
        .expect("contained source root is capturable");
    let mut target_rule = DockSurfaceRule::new();
    target_rule.set_allowed_target_payloads([DockPayloadKind::Item]);
    let mut policy = DockPolicy::new();
    policy.set_surface_rule(SOURCE_SURFACE, target_rule);
    assert_surface_target_payload_rejection(
        workspace,
        &policy,
        WorkspaceCommand::PromoteContained {
            source,
            surface: SOURCE_SURFACE,
            floating: FLOATING,
        },
        SOURCE_SURFACE,
        DockPayloadKind::Root,
    );
}

#[test]
fn item_presentation_commands_enforce_surface_target_payload_kind_atomically() {
    let (workspace, _, _) = two_root_workspace();
    let mut target_rule = DockSurfaceRule::new();
    target_rule.set_allowed_target_payloads([DockPayloadKind::Root]);
    let mut policy = DockPolicy::new();
    policy.set_allow_native_surfaces(true);
    policy.set_surface_rule(NEW_SURFACE, target_rule);
    assert_surface_target_payload_rejection(
        workspace,
        &policy,
        WorkspaceCommand::CreateSurfaceRoot {
            surface: NEW_SURFACE,
            root: NEW_ROOT,
            content: RootContent::OpenItem(ItemId::new(1_001)),
        },
        NEW_SURFACE,
        DockPayloadKind::Item,
    );

    let (workspace, _, _) = two_root_workspace();
    let mut target_rule = DockSurfaceRule::new();
    target_rule.set_allowed_target_payloads([DockPayloadKind::Root]);
    let mut policy = DockPolicy::new();
    policy.set_surface_rule(TARGET_SURFACE, target_rule);
    assert_surface_target_payload_rejection(
        workspace,
        &policy,
        WorkspaceCommand::CreateContainedRoot {
            surface: TARGET_SURFACE,
            root: NEW_ROOT,
            floating: FLOATING,
            rect: rect(20.0),
            position: ContainedPosition::Front,
            content: RootContent::OpenItem(ItemId::new(1_002)),
        },
        TARGET_SURFACE,
        DockPayloadKind::Item,
    );

    let (workspace, _) = contained_only_workspace();
    let mut target_rule = DockSurfaceRule::new();
    target_rule.set_allowed_target_payloads([DockPayloadKind::Root]);
    let mut policy = DockPolicy::new();
    policy.set_surface_rule(SOURCE_SURFACE, target_rule);
    assert_surface_target_payload_rejection(
        workspace,
        &policy,
        WorkspaceCommand::InstallMainRoot {
            surface: SOURCE_SURFACE,
            root: NEW_ROOT,
            content: RootContent::OpenItem(ItemId::new(1_003)),
        },
        SOURCE_SURFACE,
        DockPayloadKind::Item,
    );
}
