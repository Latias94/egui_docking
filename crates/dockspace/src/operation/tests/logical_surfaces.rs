use super::*;
use crate::transaction::WorkspaceTransaction;

const ATTACHED: SurfaceId = SurfaceId::new(30);
const ATTACHED_ROOT: RootId = RootId::new(30);

fn forest_workspace(main: Option<&[u64]>, back: &[u64], front: &[u64]) -> Workspace {
    let mut builder = Workspace::builder();
    let main_root = main.map(|items| {
        insert_central_root(&mut builder, MAIN, items);
        MAIN
    });
    builder.set_surface(
        SOURCE,
        SurfacePresentation {
            main_root,
            contained: Vec::new(),
        },
    );
    for (floating, root, items, bounds) in [
        (FLOATING_A, CONTAINED_A, back, rect(10.0)),
        (FLOATING_B, CONTAINED_B, front, rect(20.0)),
    ] {
        insert_central_root(&mut builder, root, items);
        builder.set_contained_floating(floating, ContainedFloating::new(root, bounds));
        builder.attach_contained(SOURCE, floating).unwrap();
    }
    insert_root(&mut builder, SURVIVOR_ROOT, &[9]);
    builder.set_surface(SURVIVOR, SurfacePresentation::with_main(SURVIVOR_ROOT));
    builder.build().unwrap()
}

fn insert_central_root(builder: &mut WorkspaceBuilder, root: RootId, items: &[u64]) {
    let central = builder.insert_node(Node::tabs(items.iter().copied().map(ItemId::new)));
    builder.set_root(root, RootRecord::new(central).with_central(central));
}

#[test]
fn attach_creates_an_empty_central_without_changing_existing_topology_or_items() {
    let mut workspace = workspace_with_surface(Some(MAIN));
    let before = workspace.clone();
    let command = WorkspaceCommand::AttachEmptySurface {
        surface: ATTACHED,
        root: ATTACHED_ROOT,
    };
    assert_eq!(command.opened_item(), None);

    let report = WorkspaceTransaction::from_commands([command])
        .apply(&mut workspace, &default_policy())
        .expect("logical attachment requires no native hosting policy");

    assert_eq!(
        report.outcomes(),
        &[CommandOutcome::EmptySurfaceAttached {
            surface: ATTACHED,
            root: ATTACHED_ROOT,
        }]
    );
    assert!(report.changed());
    assert!(report.outcomes()[0].changes_workspace());
    assert_eq!(
        workspace.surface(ATTACHED),
        Some(&SurfacePresentation::with_main(ATTACHED_ROOT))
    );
    let root = workspace.root(ATTACHED_ROOT).unwrap();
    assert_eq!(root.central, Some(root.node));
    assert!(matches!(
        workspace.node(root.node),
        Some(Node::Tabs { items, selected: None }) if items.is_empty()
    ));
    assert_eq!(workspace.tab_mru(root.node), Some(&[][..]));
    assert_eq!(workspace.item_multiset(), before.item_multiset());
    assert_eq!(workspace.nodes().count(), before.nodes().count() + 1);
    for (node, expected) in before.nodes() {
        assert_eq!(workspace.node(node), Some(expected));
        assert_eq!(workspace.tab_mru(node), before.tab_mru(node));
    }
    for (root, expected) in before.roots() {
        assert_eq!(workspace.root(root), Some(expected));
    }
    for (surface, expected) in before.surfaces() {
        assert_eq!(workspace.surface(surface), Some(expected));
    }
    for (floating, expected) in before.contained_floatings() {
        assert_eq!(workspace.contained_floating(floating), Some(expected));
    }
    workspace.validate().unwrap();
}

#[test]
fn duplicate_attachment_and_missing_retirement_reject_without_mutation() {
    let before = workspace_with_surface(Some(MAIN));
    for (command, expected) in [
        (
            WorkspaceCommand::AttachEmptySurface {
                surface: SOURCE,
                root: ATTACHED_ROOT,
            },
            CommandError::SurfaceIdCollision { surface: SOURCE },
        ),
        (
            WorkspaceCommand::AttachEmptySurface {
                surface: ATTACHED,
                root: MAIN,
            },
            CommandError::RootIdCollision { root: MAIN },
        ),
        (
            WorkspaceCommand::RetireEmptySurface { surface: ATTACHED },
            CommandError::MissingSurface { surface: ATTACHED },
        ),
    ] {
        let mut workspace = before.clone();
        let error = WorkspaceTransaction::from_commands([command])
            .apply(&mut workspace, &default_policy())
            .unwrap_err();
        assert_eq!(
            error,
            TransactionError::Command {
                index: 0,
                source: expected
            }
        );
        assert_eq!(workspace, before);
    }
}

#[test]
fn retire_counts_items_across_the_entire_main_and_contained_forest() {
    for (mut workspace, items) in [
        (forest_workspace(Some(&[1, 2]), &[3], &[4, 5]), 5),
        (forest_workspace(Some(&[]), &[3], &[4, 5]), 3),
        (forest_workspace(None, &[3], &[4, 5]), 3),
        (forest_workspace(Some(&[1, 2]), &[], &[]), 2),
    ] {
        let before = workspace.clone();
        let error = WorkspaceTransaction::from_commands([WorkspaceCommand::RetireEmptySurface {
            surface: SOURCE,
        }])
        .apply(&mut workspace, &default_policy())
        .unwrap_err();

        assert_eq!(
            error,
            TransactionError::Command {
                index: 0,
                source: CommandError::SurfaceNotEmpty {
                    surface: SOURCE,
                    items
                },
            }
        );
        assert!(error.is_expected_rejection());
        assert_eq!(workspace, before);
    }
}

#[test]
fn retire_removes_the_complete_empty_forest_and_preserves_the_other_surface() {
    for main in [Some(&[][..]), None] {
        let mut workspace = forest_workspace(main, &[], &[]);
        let before = workspace.clone();
        let retired_roots = main
            .map(|_| MAIN)
            .into_iter()
            .chain([CONTAINED_A, CONTAINED_B])
            .collect::<Vec<_>>();
        let retired_nodes = retired_roots
            .iter()
            .map(|root| workspace.root(*root).unwrap().node)
            .collect::<Vec<_>>();
        let command = WorkspaceCommand::RetireEmptySurface { surface: SOURCE };
        assert_eq!(command.opened_item(), None);

        let report = WorkspaceTransaction::from_commands([command])
            .apply(&mut workspace, &default_policy())
            .unwrap();

        assert_eq!(
            report.outcomes(),
            &[CommandOutcome::EmptySurfaceRetired { surface: SOURCE }]
        );
        assert!(report.changed());
        assert!(report.outcomes()[0].changes_workspace());
        assert!(workspace.surface(SOURCE).is_none());
        for root in retired_roots {
            assert!(workspace.root(root).is_none());
        }
        for node in retired_nodes {
            assert!(workspace.node(node).is_none());
            assert!(workspace.tab_mru(node).is_none());
        }
        assert_eq!(workspace.contained_floatings().count(), 0);
        assert_eq!(workspace.roots().count(), 1);
        assert_eq!(workspace.surfaces().count(), 1);
        assert_eq!(workspace.nodes().count(), 1);
        assert_eq!(workspace.surface(SURVIVOR), before.surface(SURVIVOR));
        assert_eq!(workspace.root(SURVIVOR_ROOT), before.root(SURVIVOR_ROOT));
        let survivor = workspace.root(SURVIVOR_ROOT).unwrap().node;
        assert_eq!(workspace.node(survivor), before.node(survivor));
        assert_eq!(workspace.tab_mru(survivor), before.tab_mru(survivor));
        assert_eq!(workspace.item_multiset(), before.item_multiset());
        workspace.validate().unwrap();
    }
}

#[test]
fn later_rejection_rolls_back_an_earlier_attachment_or_retirement() {
    for (mut workspace, first, second, expected) in [
        (
            workspace_with_surface(Some(MAIN)),
            WorkspaceCommand::AttachEmptySurface {
                surface: ATTACHED,
                root: ATTACHED_ROOT,
            },
            WorkspaceCommand::RetireEmptySurface { surface: SOURCE },
            CommandError::SurfaceNotEmpty {
                surface: SOURCE,
                items: 5,
            },
        ),
        (
            forest_workspace(Some(&[]), &[], &[]),
            WorkspaceCommand::RetireEmptySurface { surface: SOURCE },
            WorkspaceCommand::AttachEmptySurface {
                surface: SURVIVOR,
                root: ATTACHED_ROOT,
            },
            CommandError::SurfaceIdCollision { surface: SURVIVOR },
        ),
    ] {
        let before = workspace.clone();
        let error = WorkspaceTransaction::from_commands([first, second])
            .apply(&mut workspace, &default_policy())
            .unwrap_err();

        assert_eq!(
            error,
            TransactionError::Command {
                index: 1,
                source: expected
            }
        );
        assert_eq!(workspace, before);
    }
}

#[test]
fn surface_attachment_and_retirement_obey_the_arrangement_lock() {
    let mut policy = DockPolicy::new();
    policy.set_arrangement_locked(true);
    let locked = policy.snapshot(PolicyRevision::new(1));
    policy.set_arrangement_locked(false);
    let unlocked = policy.snapshot(PolicyRevision::new(2));

    for command in [
        WorkspaceCommand::AttachEmptySurface {
            surface: ATTACHED,
            root: ATTACHED_ROOT,
        },
        WorkspaceCommand::RetireEmptySurface { surface: SOURCE },
    ] {
        let mut workspace = forest_workspace(Some(&[]), &[], &[]);
        let before = workspace.clone();
        let transaction = WorkspaceTransaction::from_commands([command]);
        let error = transaction.apply(&mut workspace, &locked).unwrap_err();
        assert_eq!(
            error,
            TransactionError::Command {
                index: 0,
                source: CommandError::Policy(PolicyRejection::ArrangementLocked),
            }
        );
        assert_eq!(workspace, before);
        assert!(
            transaction
                .apply(&mut workspace, &unlocked)
                .unwrap()
                .changed()
        );
    }
}

#[test]
fn retired_surface_and_root_names_can_be_reattached_with_a_fresh_runtime_node() {
    let mut workspace = forest_workspace(Some(&[]), &[], &[]);
    let items = workspace.item_multiset();
    let previous = workspace.root(MAIN).unwrap().node;

    let report = WorkspaceTransaction::from_commands([
        WorkspaceCommand::RetireEmptySurface { surface: SOURCE },
        WorkspaceCommand::AttachEmptySurface {
            surface: SOURCE,
            root: MAIN,
        },
    ])
    .apply(&mut workspace, &default_policy())
    .unwrap();

    assert_eq!(
        report.outcomes(),
        &[
            CommandOutcome::EmptySurfaceRetired { surface: SOURCE },
            CommandOutcome::EmptySurfaceAttached {
                surface: SOURCE,
                root: MAIN
            },
        ]
    );
    let attached = workspace.root(MAIN).unwrap().node;
    assert_ne!(attached, previous);
    assert!(workspace.node(previous).is_none());
    assert_eq!(workspace.root(MAIN).unwrap().central, Some(attached));
    assert_eq!(workspace.item_multiset(), items);

    WorkspaceTransaction::from_commands([WorkspaceCommand::RetireEmptySurface { surface: SOURCE }])
        .apply(&mut workspace, &default_policy())
        .unwrap();
    WorkspaceTransaction::from_commands([WorkspaceCommand::AttachEmptySurface {
        surface: SOURCE,
        root: MAIN,
    }])
    .apply(&mut workspace, &default_policy())
    .unwrap();

    assert_ne!(workspace.root(MAIN).unwrap().node, attached);
    assert!(workspace.node(attached).is_none());
    assert_eq!(workspace.item_multiset(), items);
    workspace.validate().unwrap();
}

#[test]
fn an_attached_central_remains_after_its_last_item_moves_out_until_explicit_retirement() {
    let mut workspace = workspace_with_surface(Some(MAIN));
    let items = workspace.item_multiset();
    WorkspaceTransaction::from_commands([WorkspaceCommand::AttachEmptySurface {
        surface: ATTACHED,
        root: ATTACHED_ROOT,
    }])
    .apply(&mut workspace, &default_policy())
    .unwrap();
    let central = workspace.root(ATTACHED_ROOT).unwrap().node;
    let source = workspace
        .capture_item_source_by_id(ItemId::new(2))
        .unwrap()
        .unwrap();
    let original_tabs = source.tabs();
    let target = workspace
        .capture_tab_target(ATTACHED_ROOT, central)
        .unwrap();
    WorkspaceTransaction::from_commands([WorkspaceCommand::Move {
        payload: MovePayload::Item(source),
        target: DockTarget::Center(target),
    }])
    .apply(&mut workspace, &default_policy())
    .unwrap();
    assert_eq!(
        workspace.locate_item_by_id(ItemId::new(2)).unwrap().0,
        ATTACHED_ROOT
    );

    let source = workspace
        .capture_item_source_by_id(ItemId::new(2))
        .unwrap()
        .unwrap();
    let target = workspace.capture_tab_target(MAIN, original_tabs).unwrap();
    WorkspaceTransaction::from_commands([WorkspaceCommand::Move {
        payload: MovePayload::Item(source),
        target: DockTarget::Center(target),
    }])
    .apply(&mut workspace, &default_policy())
    .unwrap();

    assert_eq!(
        workspace.root(ATTACHED_ROOT).unwrap().central,
        Some(central)
    );
    assert!(workspace.surface(ATTACHED).is_some());
    assert!(matches!(workspace.node(central), Some(Node::Tabs { items, .. }) if items.is_empty()));
    WorkspaceTransaction::from_commands([WorkspaceCommand::RetireEmptySurface {
        surface: ATTACHED,
    }])
    .apply(&mut workspace, &default_policy())
    .unwrap();
    assert!(workspace.surface(ATTACHED).is_none());
    assert!(workspace.root(ATTACHED_ROOT).is_none());
    assert!(workspace.node(central).is_none());
    assert_eq!(workspace.item_multiset(), items);
    workspace.validate().unwrap();
}
