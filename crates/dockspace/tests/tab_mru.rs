use super::support;

use dockspace::command::{
    ContentCloseTarget, DockTarget, MovePayload, RootPresentationTarget, WorkspaceCommand,
};
use dockspace::engine::{DockEngine, EngineInput};
use dockspace::graph::{Node, RootRecord, SurfacePresentation, Workspace, WorkspaceBuilder};
use dockspace::ids::{ItemId, NodeId, RootId, StableInputSourceId, SurfaceId};
use dockspace::policy::{DockPolicy, DockPolicySnapshot, PolicyRevision};
use dockspace::transaction::WorkspaceTransaction;
use dockspace::validation::WorkspaceValidationError;

const SOURCE_ROOT: RootId = RootId::new(1);
const TARGET_ROOT: RootId = RootId::new(2);
const SOURCE_SURFACE: SurfaceId = SurfaceId::new(1);
const TARGET_SURFACE: SurfaceId = SurfaceId::new(2);
const INPUT_SOURCE: StableInputSourceId = StableInputSourceId::new(0x74_61_62_6d_72_75);

fn item(value: u64) -> ItemId {
    ItemId::new(value)
}

fn apply(workspace: &mut Workspace, command: WorkspaceCommand) {
    WorkspaceTransaction::from_commands([command])
        .apply(workspace, &DockPolicySnapshot::default())
        .expect("MRU command must succeed");
}

fn close(workspace: &mut Workspace, item: ItemId) {
    let mut engine = DockEngine::new(workspace.clone(), DockPolicy::default())
        .expect("MRU close engine must be valid");
    let mut host = support::TestPresentationHost::new(&mut engine);
    let expected = engine.version();
    let request = support::submit_input(
        &mut engine,
        &mut host,
        INPUT_SOURCE,
        EngineInput::RequestContentClose {
            expected,
            target: ContentCloseTarget::Item(item),
        },
    )
    .expect("MRU close request must reduce");
    let dockspace::transition::InputOutcome::ContentCloseRequested { plan, .. } =
        request.reduced_inputs()[0].outcome()
    else {
        panic!("MRU close request must open a plan");
    };
    let plan = plan.clone();
    support::submit_input(
        &mut engine,
        &mut host,
        INPUT_SOURCE,
        EngineInput::ResolveClose {
            request: plan.request(),
            token: plan.items()[0].token(),
            decision: dockspace::CloseDecision::Allow,
        },
    )
    .expect("MRU close decision must commit");
    *workspace = engine.workspace().clone();
}

fn selected(workspace: &Workspace, tabs: NodeId) -> Option<ItemId> {
    let Some(Node::Tabs { selected, .. }) = workspace.node(tabs) else {
        panic!("expected tabs node {tabs:?}");
    };
    *selected
}

fn two_roots() -> (Workspace, NodeId, NodeId) {
    let mut builder = WorkspaceBuilder::new();
    let source = builder.insert_node(Node::tabs([item(1), item(2), item(3)]));
    let target = builder.insert_node(Node::tabs([item(10), item(11)]));
    builder.set_root(SOURCE_ROOT, RootRecord::new(source));
    builder.set_root(TARGET_ROOT, RootRecord::new(target));
    builder.set_surface(SOURCE_SURFACE, SurfacePresentation::with_main(SOURCE_ROOT));
    builder.set_surface(TARGET_SURFACE, SurfacePresentation::with_main(TARGET_ROOT));
    (
        builder.build().expect("MRU fixture must be valid"),
        source,
        target,
    )
}

#[test]
fn selection_and_reselection_keep_a_complete_most_recent_first_permutation() {
    let (mut workspace, source, _) = two_roots();
    assert_eq!(
        workspace.tab_mru(source),
        Some([item(1), item(2), item(3)].as_slice())
    );

    let select = workspace
        .capture_item_source(SOURCE_ROOT, source, item(2))
        .expect("selection source must exist");
    apply(&mut workspace, WorkspaceCommand::Select { source: select });
    assert_eq!(
        workspace.tab_mru(source),
        Some([item(2), item(1), item(3)].as_slice())
    );

    let reselect = workspace
        .capture_item_source(SOURCE_ROOT, source, item(2))
        .expect("reselection source must exist");
    let report =
        WorkspaceTransaction::from_commands([WorkspaceCommand::Select { source: reselect }])
            .apply(&mut workspace, &DockPolicySnapshot::default())
            .expect("reselection must be valid");
    assert!(!report.changed());
    assert_eq!(
        workspace.tab_mru(source),
        Some([item(2), item(1), item(3)].as_slice())
    );
}

#[test]
fn closing_the_selected_tab_restores_the_most_recent_remaining_item() {
    let (mut workspace, source, _) = two_roots();
    for selected in [item(2), item(3)] {
        let source_item = workspace
            .capture_item_source(SOURCE_ROOT, source, selected)
            .expect("selection source must exist");
        apply(
            &mut workspace,
            WorkspaceCommand::Select {
                source: source_item,
            },
        );
    }
    assert_eq!(
        workspace.tab_mru(source),
        Some([item(3), item(2), item(1)].as_slice())
    );

    close(&mut workspace, item(3));

    assert_eq!(selected(&workspace, source), Some(item(2)));
    assert_eq!(
        workspace.tab_mru(source),
        Some([item(2), item(1)].as_slice())
    );
}

#[test]
fn reorder_preserves_mru_while_a_single_item_move_updates_both_stacks() {
    let (mut workspace, source, target) = two_roots();
    let select = workspace
        .capture_item_source(SOURCE_ROOT, source, item(2))
        .expect("selection source must exist");
    apply(&mut workspace, WorkspaceCommand::Select { source: select });

    let reorder = workspace
        .capture_item_source(SOURCE_ROOT, source, item(3))
        .expect("reorder source must exist");
    apply(
        &mut workspace,
        WorkspaceCommand::Reorder {
            source: reorder,
            insertion_index: 0,
        },
    );
    assert_eq!(
        workspace.tab_mru(source),
        Some([item(2), item(1), item(3)].as_slice()),
        "visual tab order must not become focus history"
    );

    let moved = workspace
        .capture_item_source(SOURCE_ROOT, source, item(2))
        .expect("move source must exist");
    let destination = workspace
        .capture_tab_target(TARGET_ROOT, target)
        .expect("move target must exist");
    apply(
        &mut workspace,
        WorkspaceCommand::Move {
            payload: MovePayload::Item(moved),
            target: DockTarget::Center(destination),
        },
    );

    assert_eq!(selected(&workspace, source), Some(item(1)));
    assert_eq!(
        workspace.tab_mru(source),
        Some([item(1), item(3)].as_slice())
    );
    assert_eq!(selected(&workspace, target), Some(item(2)));
    assert_eq!(
        workspace.tab_mru(target),
        Some([item(2), item(10), item(11)].as_slice())
    );
}

#[test]
fn merging_tabs_preserves_target_local_history_without_importing_source_history() {
    let (mut workspace, source, target) = two_roots();
    for selected in [item(2), item(3)] {
        let source_item = workspace
            .capture_item_source(SOURCE_ROOT, source, selected)
            .expect("source selection must exist");
        apply(
            &mut workspace,
            WorkspaceCommand::Select {
                source: source_item,
            },
        );
    }
    let target_item = workspace
        .capture_item_source(TARGET_ROOT, target, item(11))
        .expect("target selection must exist");
    apply(
        &mut workspace,
        WorkspaceCommand::Select {
            source: target_item,
        },
    );

    let source_stack = workspace
        .capture_node_source(SOURCE_ROOT, source)
        .expect("source stack must exist");
    let target_stack = workspace
        .capture_tab_target(TARGET_ROOT, target)
        .expect("target stack must exist");
    apply(
        &mut workspace,
        WorkspaceCommand::Move {
            payload: MovePayload::Tabs(source_stack),
            target: DockTarget::Center(target_stack),
        },
    );

    assert_eq!(selected(&workspace, target), Some(item(3)));
    assert_eq!(
        workspace.tab_mru(target),
        Some([item(3), item(11), item(10), item(1), item(2)].as_slice())
    );

    close(&mut workspace, item(3));
    assert_eq!(selected(&workspace, target), Some(item(11)));
}

#[test]
fn merging_a_tabs_stack_into_an_empty_central_leaf_preserves_its_complete_mru() {
    let mut builder = WorkspaceBuilder::new();
    let source = builder.insert_node(Node::tabs([item(1), item(2), item(3)]));
    let central = builder.insert_node(Node::tabs([]));
    builder.set_root(SOURCE_ROOT, RootRecord::new(source));
    builder.set_root(TARGET_ROOT, RootRecord::new(central).with_central(central));
    builder.set_surface(SOURCE_SURFACE, SurfacePresentation::with_main(SOURCE_ROOT));
    builder.set_surface(TARGET_SURFACE, SurfacePresentation::with_main(TARGET_ROOT));
    let mut workspace = builder
        .build()
        .expect("empty central fixture must be valid");

    for recent in [item(2), item(3)] {
        let source_item = workspace
            .capture_item_source(SOURCE_ROOT, source, recent)
            .expect("source selection must exist");
        apply(
            &mut workspace,
            WorkspaceCommand::Select {
                source: source_item,
            },
        );
    }
    assert_eq!(
        workspace.tab_mru(source),
        Some([item(3), item(2), item(1)].as_slice())
    );

    let source_stack = workspace
        .capture_node_source(SOURCE_ROOT, source)
        .expect("source stack must exist");
    let central_target = workspace
        .capture_tab_target(TARGET_ROOT, central)
        .expect("empty central target must exist");
    apply(
        &mut workspace,
        WorkspaceCommand::Move {
            payload: MovePayload::Tabs(source_stack),
            target: DockTarget::Center(central_target),
        },
    );

    assert_eq!(selected(&workspace, central), Some(item(3)));
    assert_eq!(
        workspace.tab_mru(central),
        Some([item(3), item(2), item(1)].as_slice())
    );

    close(&mut workspace, item(3));
    assert_eq!(selected(&workspace, central), Some(item(2)));
    assert_eq!(
        workspace.tab_mru(central),
        Some([item(2), item(1)].as_slice())
    );
}

#[test]
fn moving_a_complete_root_preserves_tab_identity_selection_and_mru() {
    let (mut workspace, source, _) = two_roots();
    let select = workspace
        .capture_item_source(SOURCE_ROOT, source, item(3))
        .expect("selection source must exist");
    apply(&mut workspace, WorkspaceCommand::Select { source: select });
    let expected = workspace
        .tab_mru(source)
        .expect("tabs history must exist")
        .to_vec();

    let root = workspace
        .capture_node_source(SOURCE_ROOT, source)
        .expect("root source must exist");
    let mut policy = DockPolicy::default();
    policy.set_allow_native_surfaces(true);
    let policy = policy.snapshot(PolicyRevision::default());
    WorkspaceTransaction::from_commands([WorkspaceCommand::RehomeRoot {
        source: root,
        target: RootPresentationTarget::NewSurface {
            surface: SurfaceId::new(3),
        },
    }])
    .apply(&mut workspace, &policy)
    .expect("complete root rehome must succeed");

    assert_eq!(
        workspace.root(SOURCE_ROOT).map(|record| record.node),
        Some(source)
    );
    assert_eq!(selected(&workspace, source), Some(item(3)));
    assert_eq!(workspace.tab_mru(source), Some(expected.as_slice()));
}

#[test]
fn validation_rejects_any_history_that_is_not_an_exact_selected_first_permutation() {
    fn errors_for(mru: impl IntoIterator<Item = ItemId>) -> Vec<WorkspaceValidationError> {
        let mut builder = WorkspaceBuilder::new();
        let tabs = builder.insert_node(Node::tabs([item(1), item(2)]));
        builder
            .set_tab_mru(tabs, mru)
            .expect("test target must be tabs");
        builder.set_root(SOURCE_ROOT, RootRecord::new(tabs));
        builder.set_surface(SOURCE_SURFACE, SurfacePresentation::with_main(SOURCE_ROOT));
        builder
            .validate()
            .expect_err("malformed MRU must be rejected")
            .into_errors()
    }

    assert!(errors_for([item(1), item(1)]).iter().any(|error| matches!(
        error,
        WorkspaceValidationError::DuplicateTabMruItem { item: duplicate, .. }
            if *duplicate == item(1)
    )));
    assert!(errors_for([item(1)]).iter().any(|error| matches!(
        error,
        WorkspaceValidationError::TabItemMissingFromMru { item: missing, .. }
            if *missing == item(2)
    )));
    assert!(errors_for([item(1), item(99)]).iter().any(|error| matches!(
        error,
        WorkspaceValidationError::TabMruItemNotInTabs { item: foreign, .. }
            if *foreign == item(99)
    )));
    assert!(errors_for([item(2), item(1)]).iter().any(|error| matches!(
        error,
        WorkspaceValidationError::SelectedTabNotMostRecent {
            selected: Some(selected),
            most_recent: Some(most_recent),
            ..
        } if *selected == item(1) && *most_recent == item(2)
    )));
}
