use super::*;
use crate::interaction::targeting::resolver::structural_work;
use proptest::prelude::*;

fn selection_fixture() -> Workspace {
    let mut builder = Workspace::builder();
    let tabs = builder.insert_node(Node::tabs([1, 2, 3].map(ItemId::new)));
    let other = builder.insert_node(Node::tabs([4, 5].map(ItemId::new)));
    let central = builder.insert_node(Node::tabs([]));
    let split = builder.insert_node(
        Node::split(Axis::Horizontal, [tabs, other, central], [0.2, 0.3, 0.5]).unwrap(),
    );
    builder.set_root(MAIN, RootRecord::new(split).with_central(central));
    builder.set_surface(SOURCE, SurfacePresentation::with_main(MAIN));
    for (root, floating, items) in [
        (CONTAINED_A, FLOATING_A, vec![6, 7, 8]),
        (CONTAINED_B, FLOATING_B, vec![9, 10]),
    ] {
        insert_root(&mut builder, root, &items);
        builder.set_contained_floating(floating, ContainedFloating::new(root, rect(10.0)));
        builder.attach_contained(SOURCE, floating).unwrap();
    }
    builder.build().unwrap()
}

fn selection(workspace: &Workspace, item: u64) -> WorkspaceCommand {
    WorkspaceCommand::Select {
        source: workspace
            .capture_item_source_by_id(ItemId::new(item))
            .unwrap()
            .unwrap(),
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    #[test]
    fn local_selection_matches_general_transactions(
        items in proptest::collection::vec(1_u64..=10, 1..24),
        locked in any::<bool>(),
    ) {
        let mut snapshot = WorkspaceSnapshot::validated(selection_fixture()).unwrap();
        let mut policy = DockPolicy::default();
        policy.set_arrangement_locked(locked);
        let policy = policy.snapshot(PolicyRevision::default());
        for item in items {
            let command = selection(&snapshot, item);
            let original = snapshot.clone();
            let (expected, expected_report) = prepare_general_candidate(
                &snapshot, TransactionAuthority::Policy(&policy), std::slice::from_ref(&command),
                snapshot.as_ref().clone(),
            ).unwrap().into_parts();
            structural_work::reset();
            let (actual, report) = snapshot.prepare_transaction(
                &policy, std::slice::from_ref(&command),
            ).unwrap().into_snapshot_parts();
            let work = structural_work::snapshot();
            prop_assert_eq!(work.item_multiset_scans, 0);
            prop_assert_eq!(work.transaction_prepares, 1);
            prop_assert_eq!(
                work.workspace_deep_clones.transaction_candidates.calls,
                usize::from(report.changed()),
            );
            prop_assert_eq!(Arc::ptr_eq(actual.as_arc(), snapshot.as_arc()), !report.changed());
            prop_assert_eq!(&*actual, &expected);
            prop_assert_eq!(report, expected_report);
            prop_assert_eq!(&snapshot, &original);
            prop_assert!(actual.validate().is_ok());
            snapshot = actual;
        }
    }
}

#[test]
fn stale_wrong_root_missing_and_recycled_sources_keep_general_rejections() {
    let snapshot = WorkspaceSnapshot::validated(selection_fixture()).unwrap();
    let policy = default_policy();
    let WorkspaceCommand::Select { source } = selection(&snapshot, 2) else {
        unreachable!()
    };
    let retired_source = source.clone();
    let (changed, _) = snapshot
        .prepare_transaction(
            &policy,
            &[WorkspaceCommand::Select {
                source: source.clone(),
            }],
        )
        .unwrap()
        .into_snapshot_parts();
    let mut wrong_root = source.clone();
    wrong_root.root = CONTAINED_B;
    let mut absent_item = source.clone();
    absent_item.item = ItemId::new(99);
    let mut missing_root = source.clone();
    missing_root.root = RootId::new(99);
    let mut arena = snapshot.storage().nodes.clone();
    arena.remove(source.tabs());
    let recycled = arena.insert(Node::tabs([ItemId::new(99)]));
    assert_ne!(recycled, source.tabs());
    let mut recycled_source = source.clone();
    recycled_source.tabs = recycled;
    for (base, source) in [
        (&changed, source),
        (&snapshot, wrong_root),
        (&snapshot, absent_item),
        (&snapshot, missing_root),
        (&snapshot, recycled_source),
    ] {
        let commands = [WorkspaceCommand::Select { source }];
        let expected = prepare_transaction(base, &policy, &commands).unwrap_err();
        let actual = base.prepare_transaction(&policy, &commands).unwrap_err();
        assert_eq!(actual, expected);
    }

    let mut replacement = snapshot.as_ref().clone().into_builder();
    let tabs = replacement
        .workspace
        .nodes
        .remove(retired_source.tabs())
        .unwrap();
    let recycled = replacement.workspace.nodes.insert((*tabs).clone());
    assert_ne!(recycled, retired_source.tabs());
    let root = replacement.draft().root(MAIN).unwrap().node;
    let Node::Split { children, .. } = replacement.workspace.nodes.get_mut(root).unwrap() else {
        unreachable!()
    };
    *children
        .iter_mut()
        .find(|child| **child == retired_source.tabs())
        .unwrap() = recycled;
    let replacement = WorkspaceSnapshot::validated(replacement.build().unwrap()).unwrap();
    let commands = [WorkspaceCommand::Select {
        source: retired_source,
    }];
    assert_eq!(
        replacement
            .prepare_transaction(&policy, &commands)
            .unwrap_err(),
        prepare_transaction(&replacement, &policy, &commands).unwrap_err(),
    );
}

#[test]
fn raw_corruption_must_cross_builder_checks_before_snapshot_ownership() {
    let original = WorkspaceSnapshot::validated(selection_fixture()).unwrap();
    for corrupt in [false, true] {
        let mut builder = original.as_ref().clone().into_builder();
        let orphan = builder.insert_node(Node::tabs(if corrupt {
            vec![ItemId::new(99)]
        } else {
            vec![]
        }));
        let built = builder.build();
        assert_eq!(built.is_err(), corrupt);
        if let Ok(workspace) = built {
            assert!(workspace.node(orphan).is_none());
            let snapshot = WorkspaceSnapshot::from(workspace);
            let command = selection(&snapshot, 2);
            structural_work::reset();
            snapshot
                .prepare_transaction(&default_policy(), &[command])
                .unwrap();
            assert_eq!(structural_work::snapshot().item_multiset_scans, 0);
        }
        original.validate().unwrap();
    }
    let other = original
        .capture_item_source_by_id(ItemId::new(6))
        .unwrap()
        .unwrap()
        .tabs();
    let mut builder = original.as_ref().clone().into_builder();
    builder.set_tab_mru(other, []).unwrap();
    assert!(builder.build().is_err());
}

#[test]
fn mixed_and_multi_command_batches_preserve_general_atomicity() {
    let snapshot = WorkspaceSnapshot::validated(selection_fixture()).unwrap();
    let policy = default_policy();
    let selected = selection(&snapshot, 1);
    let change = selection(&snapshot, 2);
    let WorkspaceCommand::Select { source } = &selected else {
        unreachable!()
    };
    for commands in [
        vec![selected.clone(), change.clone()],
        vec![
            WorkspaceCommand::Reorder {
                source: source.clone(),
                insertion_index: 0,
            },
            change.clone(),
        ],
        vec![change.clone(), change],
    ] {
        let original = snapshot.clone();
        let expected = prepare_transaction(&snapshot, &policy, &commands)
            .map(|prepared| prepared.into_parts());
        structural_work::reset();
        let actual = snapshot
            .prepare_transaction(&policy, &commands)
            .map(|prepared| prepared.into_parts());
        match (actual, expected) {
            (
                Err(TransactionError::Command {
                    index: actual_index,
                    source:
                        CommandError::StaleNode {
                            role: actual_role,
                            node: actual_node,
                            expected: actual_expected,
                            actual: actual_current,
                        },
                }),
                Err(TransactionError::Command {
                    index: expected_index,
                    source:
                        CommandError::StaleNode {
                            role: expected_role,
                            node: expected_node,
                            expected: expected_expected,
                            actual: expected_current,
                        },
                }),
            ) => {
                assert_eq!(
                    (actual_index, actual_role, actual_node),
                    (expected_index, expected_role, expected_node)
                );
                assert_eq!(actual_expected, expected_expected);
                assert_eq!(actual_current.root(), expected_current.root());
                assert_ne!(actual_expected, actual_current);
                assert_ne!(expected_expected, expected_current);
            }
            (actual, expected) => assert_eq!(actual, expected),
        }
        assert!(structural_work::snapshot().item_multiset_scans > 0);
        assert_eq!(snapshot, original);
    }
}

#[test]
fn unchanged_selection_batches_keep_their_transaction_candidate() {
    let snapshot = WorkspaceSnapshot::validated(selection_fixture()).unwrap();
    let command = selection(&snapshot, 1);
    structural_work::reset();
    let (candidate, report) = snapshot
        .prepare_transaction(&default_policy(), &[command.clone(), command])
        .unwrap()
        .into_snapshot_parts();

    assert!(!report.changed());
    assert_eq!(candidate, snapshot);
    assert!(!Arc::ptr_eq(candidate.as_arc(), snapshot.as_arc()));
    assert_eq!(
        structural_work::snapshot()
            .workspace_deep_clones
            .transaction_candidates
            .calls,
        1
    );
}

#[test]
fn checked_batches_and_followups_preserve_eligibility_for_the_next_selection() {
    let policy = default_policy();
    for followup in [false, true] {
        let mut snapshot = WorkspaceSnapshot::validated(selection_fixture()).unwrap();
        let noop = selection(&snapshot, 1);
        let command = selection(&snapshot, 2);
        structural_work::reset();
        if followup {
            snapshot.apply_draft_command(&policy, &command).unwrap();
        } else {
            let (candidate, _) = snapshot
                .prepare_transaction(&policy, &[noop, command])
                .unwrap()
                .into_snapshot_parts();
            snapshot = candidate;
        }
        assert!(structural_work::snapshot().item_multiset_scans > 0);
        let command = selection(&snapshot, 3);
        structural_work::reset();
        snapshot.prepare_transaction(&policy, &[command]).unwrap();
        assert_eq!(structural_work::snapshot().item_multiset_scans, 0);
    }
}
