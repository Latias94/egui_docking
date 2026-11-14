use super::*;
use crate::engine::surface_recovery::{
    ContainedRootPlacement, SurfaceRehomePlacement, SurfaceRosterDisposition,
};
use crate::graph::transaction::WorkspaceTransaction;
use crate::policy::{DockItemRule, DockSourceRule};

fn assert_locked(error: TransactionError, index: usize) {
    assert!(
        matches!(
            error,
            TransactionError::Command {
                index: actual,
                source: CommandError::Policy(PolicyRejection::ArrangementLocked),
            } if actual == index
        ),
        "{error:?}"
    );
}

#[test]
fn locked_commands_reject_without_mutation_and_work_again_after_unlock() {
    let workspace = workspace_with_surface(None);
    let source = workspace
        .capture_item_source_by_id(ItemId::new(4))
        .unwrap()
        .unwrap();
    let root_source = workspace
        .capture_node_source(CONTAINED_A, workspace.root(CONTAINED_A).unwrap().node)
        .unwrap();
    let target = workspace
        .capture_tab_target(CONTAINED_B, source.tabs())
        .unwrap();
    let survivor_target = workspace
        .capture_tab_target(SURVIVOR_ROOT, workspace.root(SURVIVOR_ROOT).unwrap().node)
        .unwrap();
    let commands = [
        WorkspaceCommand::Open {
            item: ItemId::new(6),
            target: DockTarget::Center(target),
        },
        WorkspaceCommand::Reorder {
            source: source.clone(),
            insertion_index: 2,
        },
        WorkspaceCommand::Move {
            payload: MovePayload::Item(source.clone()),
            target: DockTarget::Center(survivor_target),
        },
        WorkspaceCommand::CreateSurfaceRoot {
            surface: SurfaceId::new(30),
            root: RootId::new(30),
            content: RootContent::OpenItem(ItemId::new(6)),
        },
        WorkspaceCommand::CreateContainedRoot {
            surface: SOURCE,
            root: RootId::new(30),
            floating: FloatingPresentationId::new(103),
            rect: rect(30.0),
            position: ContainedPosition::Front,
            content: RootContent::Move(MovePayload::Item(source)),
        },
        WorkspaceCommand::InstallMainRoot {
            surface: SOURCE,
            root: RootId::new(30),
            content: RootContent::OpenItem(ItemId::new(6)),
        },
        WorkspaceCommand::RehomeRoot {
            source: root_source.clone(),
            target: RootPresentationTarget::NewSurface {
                surface: SurfaceId::new(30),
            },
        },
        WorkspaceCommand::PromoteContained {
            source: root_source.clone(),
            surface: SOURCE,
            floating: FLOATING_A,
        },
        WorkspaceCommand::UpdateContainedRect {
            surface: SOURCE,
            root: CONTAINED_A,
            floating: FLOATING_A,
            expected_rect: rect(10.0),
            rect: rect(30.0),
        },
        WorkspaceCommand::UpdateContainedPresentation {
            source: root_source,
            floating: FLOATING_A,
            expected_rect: rect(10.0),
            expected_roster: workspace.capture_contained_roster(SOURCE).unwrap(),
            rect: rect(10.0),
            position: ContainedPosition::Front,
        },
    ];
    let mut policy = DockPolicy::new();
    policy.set_allow_native_surfaces(true);
    policy.set_arrangement_locked(true);
    let locked = policy.snapshot(PolicyRevision::new(1));
    policy.set_arrangement_locked(false);
    let unlocked = policy.snapshot(PolicyRevision::new(2));
    for command in commands {
        let mut candidate = workspace.clone();
        let transaction = WorkspaceTransaction::from_commands([command.clone()]);
        assert_locked(
            transaction
                .apply(&mut candidate, &locked)
                .expect_err("locked mutation rejects"),
            0,
        );
        assert_eq!(candidate, workspace, "{command:?}");
        let report = transaction
            .apply(&mut candidate, &unlocked)
            .unwrap_or_else(|error| panic!("unlocked command {command:?}: {error:?}"));
        assert!(report.changed(), "{command:?}");
    }
}

#[test]
fn locked_resize_and_empty_root_removal_reject_even_without_item_changes() {
    let mut builder = Workspace::builder();
    let left = builder.insert_node(Node::tabs([ItemId::new(1)]));
    let right = builder.insert_node(Node::tabs([ItemId::new(2)]));
    let split = builder.insert_node(Node::equal_split(Axis::Horizontal, [left, right]).unwrap());
    builder.set_root(MAIN, RootRecord::new(split));
    builder.set_surface(SOURCE, SurfacePresentation::with_main(MAIN));
    let empty = builder.insert_node(Node::tabs([]));
    builder.set_root(SURVIVOR_ROOT, RootRecord::new(empty).with_central(empty));
    builder.set_surface(SURVIVOR, SurfacePresentation::with_main(SURVIVOR_ROOT));
    let workspace = builder.build().unwrap();
    let commands = [
        WorkspaceCommand::ResizeSplits {
            splits: vec![SplitResize::new(
                workspace.capture_node_source(MAIN, split).unwrap(),
                SplitWeight::normalize([0.25, 0.75]).unwrap(),
            )],
        },
        WorkspaceCommand::ResizeSplits {
            splits: vec![SplitResize::new(
                workspace.capture_node_source(MAIN, split).unwrap(),
                SplitWeight::normalize([0.5, 0.5]).unwrap(),
            )],
        },
        WorkspaceCommand::RemoveEmptyRoot {
            source: workspace.capture_node_source(SURVIVOR_ROOT, empty).unwrap(),
        },
    ];
    let mut policy = DockPolicy::new();
    policy.set_arrangement_locked(true);
    let locked = policy.snapshot(PolicyRevision::new(1));
    policy.set_arrangement_locked(false);
    let unlocked = policy.snapshot(PolicyRevision::new(2));
    for command in commands {
        let mut candidate = workspace.clone();
        let transaction = WorkspaceTransaction::from_commands([command]);
        assert_locked(
            transaction
                .apply(&mut candidate, &locked)
                .expect_err("locked mutation rejects"),
            0,
        );
        assert_eq!(candidate, workspace);
        transaction
            .apply(&mut candidate, &unlocked)
            .expect("unlocked mutation is valid");
    }
}

#[test]
fn rejected_mutation_rolls_back_an_earlier_allowed_selection() {
    let mut workspace = workspace_with_surface(None);
    let before = workspace.clone();
    let transaction = WorkspaceTransaction::from_commands([
        WorkspaceCommand::Select {
            source: workspace
                .capture_item_source_by_id(ItemId::new(5))
                .unwrap()
                .unwrap(),
        },
        WorkspaceCommand::UpdateContainedRect {
            surface: SOURCE,
            root: CONTAINED_A,
            floating: FLOATING_A,
            expected_rect: rect(10.0),
            rect: rect(30.0),
        },
    ]);
    let mut policy = DockPolicy::new();
    policy.set_arrangement_locked(true);
    assert_locked(
        transaction
            .apply(&mut workspace, &policy.snapshot(PolicyRevision::new(1)))
            .expect_err("the second command is locked"),
        1,
    );
    assert_eq!(workspace, before);
    policy.set_arrangement_locked(false);
    assert!(
        transaction
            .apply(&mut workspace, &policy.snapshot(PolicyRevision::new(2)))
            .expect("the complete unlocked transaction is valid")
            .changed()
    );
}

#[test]
fn locked_selection_and_raise_remain_available_with_exact_roster_preconditions() {
    let mut workspace = workspace_with_surface(None);
    let source = workspace
        .capture_item_source_by_id(ItemId::new(5))
        .unwrap()
        .unwrap();
    let raise = WorkspaceCommand::RaiseContained {
        source: workspace
            .capture_node_source(CONTAINED_A, workspace.root(CONTAINED_A).unwrap().node)
            .unwrap(),
        floating: FLOATING_A,
        expected_roster: workspace.capture_contained_roster(SOURCE).unwrap(),
    };
    let items = workspace.item_multiset();
    let mut policy = DockPolicy::new();
    policy.set_arrangement_locked(true);
    let policy = policy.snapshot(PolicyRevision::new(1));
    let report = WorkspaceTransaction::from_commands([
        WorkspaceCommand::Select {
            source: source.clone(),
        },
        raise.clone(),
    ])
    .apply(&mut workspace, &policy)
    .expect("selection and focus raise remain available");
    assert!(matches!(
        report.outcomes(),
        [
            CommandOutcome::Selected { changed: true, .. },
            CommandOutcome::ContainedRaised { changed: true, .. },
        ]
    ));
    assert!(
        matches!(workspace.node(source.tabs()), Some(Node::Tabs { selected: Some(item), .. }) if *item == source.item())
    );
    assert_eq!(
        workspace.surface(SOURCE).unwrap().contained,
        [FLOATING_B, FLOATING_A]
    );
    assert_eq!(
        workspace.contained_floating(FLOATING_A).unwrap().rect,
        rect(10.0)
    );
    assert_eq!(
        workspace.contained_floating(FLOATING_B).unwrap().rect,
        rect(20.0)
    );
    assert_eq!(workspace.item_multiset(), items);

    let before = workspace.clone();
    let error = WorkspaceTransaction::from_commands([raise])
        .apply(&mut workspace, &policy)
        .expect_err("a delayed raise cannot reuse an obsolete roster");
    assert!(matches!(
        error,
        TransactionError::Command {
            index: 0,
            source: CommandError::StaleContainedRoster {
                surface: SOURCE,
                ..
            }
        }
    ));
    assert_eq!(workspace, before);
}

#[test]
fn locked_raise_preserves_operation_item_source_and_surface_rules() {
    let workspace = workspace_with_surface(None);
    let command = WorkspaceCommand::RaiseContained {
        source: workspace
            .capture_node_source(CONTAINED_A, workspace.root(CONTAINED_A).unwrap().node)
            .unwrap(),
        floating: FLOATING_A,
        expected_roster: workspace.capture_contained_roster(SOURCE).unwrap(),
    };
    let mut operation = DockPolicy::new();
    operation.set_allow_contained_transform(false);
    let mut item = DockPolicy::new();
    let mut item_rule = DockItemRule::new();
    item_rule.set_source_enabled(false);
    item.set_item_rule(ItemId::new(3), item_rule);
    let mut source = DockPolicy::new();
    let mut source_rule = DockSourceRule::new();
    source_rule.set_enabled(false);
    source.set_source_rule(CONTAINED_A, source_rule);
    let mut surface = DockPolicy::new();
    let mut surface_rule = DockSurfaceRule::new();
    surface_rule.set_target_enabled(false);
    surface.set_surface_rule(SOURCE, surface_rule);
    for (mut policy, expected) in [
        (operation, PolicyRejection::ContainedTransformDisabled),
        (
            item,
            PolicyRejection::ItemSourceDisabled {
                item: ItemId::new(3),
            },
        ),
        (
            source,
            PolicyRejection::SourceDisabled { root: CONTAINED_A },
        ),
        (
            surface,
            PolicyRejection::SurfaceTargetDisabled { surface: SOURCE },
        ),
    ] {
        let original = policy.clone();
        let unlocked = policy.snapshot(PolicyRevision::new(1));
        policy.set_arrangement_locked(true);
        let locked = policy.snapshot(PolicyRevision::new(2));
        for snapshot in [&unlocked, &locked] {
            let mut candidate = workspace.clone();
            let error = WorkspaceTransaction::from_commands([command.clone()])
                .apply(&mut candidate, snapshot)
                .expect_err("existing rule still rejects raise");
            assert!(
                matches!(error, TransactionError::Command { index: 0, source: CommandError::Policy(reason) } if reason == expected),
                "{error:?}"
            );
            assert_eq!(candidate, workspace);
        }
        policy.set_arrangement_locked(false);
        assert_eq!(policy, original);
    }
}

#[test]
fn approved_item_root_and_surface_closes_revalidate_the_arrangement_lock() {
    let workspace = workspace_with_surface(Some(MAIN));
    let unlocked = default_policy();
    let source = workspace
        .capture_item_source_by_id(ItemId::new(1))
        .unwrap()
        .unwrap();
    let item = PreparedContentClose::Item {
        source,
        expected_items: workspace.item_multiset(),
        requirement: CloseItemRequirement::new(ItemId::new(1), CloseCapability::Immediate),
    };
    let root = PreparedContentClose::Root {
        source: workspace
            .capture_node_source(MAIN, workspace.root(MAIN).unwrap().node)
            .unwrap(),
        items: vec![ItemId::new(1), ItemId::new(2)],
        expected_items: workspace.item_multiset(),
        requirements: [ItemId::new(1), ItemId::new(2)]
            .into_iter()
            .map(|item| CloseItemRequirement::new(item, CloseCapability::Immediate))
            .collect(),
    };
    let surface = capture_surface_close(&workspace, &unlocked);
    let mut policy = unlocked.to_policy();
    policy.set_arrangement_locked(true);
    let locked = policy.snapshot(PolicyRevision::new(2));
    let before = workspace.clone();
    for prepared in [item, root] {
        prepare_content_close(&workspace, &unlocked, &prepared)
            .expect("approved close is valid before locking");
        assert_locked(
            prepare_content_close(&workspace, &locked, &prepared)
                .expect_err("lock denies an already approved close"),
            0,
        );
    }
    prepare_surface_content_close(&workspace, &unlocked, &surface)
        .expect("approved surface close is valid before locking");
    assert_locked(
        prepare_surface_content_close(&workspace, &locked, &surface)
            .expect_err("lock denies an already approved surface close"),
        0,
    );
    assert_eq!(workspace, before);
}

#[test]
fn retained_recovery_authority_survives_lock_but_copied_commands_do_not_bypass_it() {
    let mut workspace = workspace_with_surface(None);
    let disposition = SurfaceRosterDisposition::capture(&workspace, SOURCE, None).unwrap();
    let recovery = disposition
        .compile_rehome_transaction(
            &workspace,
            &SurfaceRehomePlacement::new(
                SURVIVOR,
                None,
                vec![
                    ContainedRootPlacement::new(FLOATING_A, CONTAINED_A, rect(30.0)),
                    ContainedRootPlacement::new(FLOATING_B, CONTAINED_B, rect(40.0)),
                ],
            ),
        )
        .expect("the complete recovery roster compiles before locking");
    let before = workspace.clone();
    let mut policy = DockPolicy::new();
    policy.set_arrangement_locked(true);
    assert_locked(
        WorkspaceTransaction::from_commands(recovery.commands().to_vec())
            .apply(&mut workspace, &policy.snapshot(PolicyRevision::new(1)))
            .expect_err("copying commands does not copy recovery authority"),
        0,
    );
    assert_eq!(workspace, before);

    recovery
        .apply(&mut workspace)
        .expect("the retained recovery obligation remains applicable");
    assert!(workspace.surface(SOURCE).is_none());
    assert_eq!(
        workspace.surface(SURVIVOR).unwrap().contained,
        [FLOATING_A, FLOATING_B]
    );
    assert_eq!(workspace.item_multiset(), before.item_multiset());
}
