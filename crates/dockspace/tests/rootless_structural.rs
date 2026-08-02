use dockspace::command::{
    CommandOutcome, ContainedPosition, DockTarget, RootContent, RootPresentationTarget,
    WorkspaceCommand,
};
use dockspace::error::{CommandError, ReferenceRole, TransactionError};
use dockspace::geometry::LogicalRect;
use dockspace::graph::{ContainedFloating, Node, RootRecord, SurfacePresentation, Workspace};
use dockspace::ids::{FloatingPresentationId, ItemId, NodeId, RootId, SurfaceId};
use dockspace::policy::{DockPolicy, DockPolicySnapshot, PolicyRejection, PolicyRevision};
use dockspace::transaction::WorkspaceTransaction;
use dockspace::validation::WorkspaceValidationError;

const SOURCE: SurfaceId = SurfaceId::new(1);
const TARGET: SurfaceId = SurfaceId::new(2);
const THIRD: SurfaceId = SurfaceId::new(3);

const MAIN: RootId = RootId::new(10);
const FLOATING_A_ROOT: RootId = RootId::new(11);
const FLOATING_B_ROOT: RootId = RootId::new(12);
const TARGET_MAIN: RootId = RootId::new(20);

const FLOATING_A: FloatingPresentationId = FloatingPresentationId::new(101);
const FLOATING_B: FloatingPresentationId = FloatingPresentationId::new(102);
fn rect(offset: f64) -> LogicalRect {
    LogicalRect::new(offset, offset, 320.0, 240.0).expect("test rectangle must be valid")
}

fn insert_root(
    builder: &mut dockspace::graph::WorkspaceBuilder,
    root: RootId,
    item: u64,
) -> NodeId {
    let node = builder.insert_node(Node::tabs([ItemId::new(item)]));
    builder.set_root(root, RootRecord::new(node));
    node
}

fn composite_workspace() -> (Workspace, NodeId, NodeId, NodeId, NodeId) {
    let mut builder = Workspace::builder();
    let main = insert_root(&mut builder, MAIN, 1);
    let floating_a = insert_root(&mut builder, FLOATING_A_ROOT, 2);
    let floating_b = insert_root(&mut builder, FLOATING_B_ROOT, 3);
    let target_main = insert_root(&mut builder, TARGET_MAIN, 4);

    builder.set_surface(SOURCE, SurfacePresentation::with_main(MAIN));
    builder.set_surface(TARGET, SurfacePresentation::with_main(TARGET_MAIN));
    builder.set_contained_floating(
        FLOATING_A,
        ContainedFloating::new(FLOATING_A_ROOT, rect(10.0)),
    );
    builder.set_contained_floating(
        FLOATING_B,
        ContainedFloating::new(FLOATING_B_ROOT, rect(20.0)),
    );
    builder
        .attach_contained(SOURCE, FLOATING_A)
        .expect("source surface must exist");
    builder
        .attach_contained(SOURCE, FLOATING_B)
        .expect("source surface must exist");

    (
        builder.build().expect("fixture must be valid"),
        main,
        floating_a,
        floating_b,
        target_main,
    )
}

fn command_error(error: TransactionError) -> CommandError {
    let TransactionError::Command { source, .. } = error else {
        panic!("expected command failure, got {error:?}");
    };
    source
}

fn native_policy() -> DockPolicySnapshot {
    let mut policy = DockPolicy::default();
    policy.set_allow_native_surfaces(true);
    policy.snapshot(PolicyRevision::default())
}

#[test]
fn moving_main_away_keeps_the_same_rootless_surface_and_roster() {
    let (mut workspace, main, _, _, _) = composite_workspace();
    let source = workspace
        .capture_node_source(MAIN, main)
        .expect("main source must be current");

    WorkspaceTransaction::from_commands([WorkspaceCommand::RehomeRoot {
        source,
        target: RootPresentationTarget::NewSurface { surface: THIRD },
    }])
    .apply(&mut workspace, &native_policy())
    .expect("main root must move without evacuating contained siblings");

    assert_eq!(
        workspace.surface(SOURCE),
        Some(&SurfacePresentation {
            main_root: None,
            contained: vec![FLOATING_A, FLOATING_B],
        })
    );
    assert_eq!(
        workspace.surface(THIRD),
        Some(&SurfacePresentation::with_main(MAIN))
    );
}

#[test]
fn promotion_is_explicit_and_consumes_only_the_named_presentation() {
    let (mut workspace, main, floating_a, _, _) = composite_workspace();
    let main_source = workspace
        .capture_node_source(MAIN, main)
        .expect("main source must be current");
    WorkspaceTransaction::from_commands([WorkspaceCommand::RehomeRoot {
        source: main_source,
        target: RootPresentationTarget::NewSurface { surface: THIRD },
    }])
    .apply(&mut workspace, &native_policy())
    .expect("source surface must become rootless");

    let source = workspace
        .capture_node_source(FLOATING_A_ROOT, floating_a)
        .expect("floating source must be current");
    WorkspaceTransaction::from_commands([WorkspaceCommand::PromoteContained {
        source,
        surface: SOURCE,
        floating: FLOATING_A,
    }])
    .apply(&mut workspace, &DockPolicySnapshot::default())
    .expect("the exact contained presentation must promote");

    assert_eq!(
        workspace.surface(SOURCE),
        Some(&SurfacePresentation {
            main_root: Some(FLOATING_A_ROOT),
            contained: vec![FLOATING_B],
        })
    );
    assert!(workspace.contained_floating(FLOATING_A).is_none());
    assert!(workspace.contained_floating(FLOATING_B).is_some());
}

#[test]
fn promotion_rejects_a_stale_node_source_and_rolls_back_the_workspace() {
    let (mut workspace, main, floating_a, _, _) = composite_workspace();
    let main_source = workspace
        .capture_node_source(MAIN, main)
        .expect("main source must be current");
    WorkspaceTransaction::from_commands([WorkspaceCommand::RehomeRoot {
        source: main_source,
        target: RootPresentationTarget::NewSurface { surface: THIRD },
    }])
    .apply(&mut workspace, &native_policy())
    .expect("source surface must become rootless");

    let stale_source = workspace
        .capture_node_source(FLOATING_A_ROOT, floating_a)
        .expect("floating source must initially be current");
    let floating_target = workspace
        .capture_tab_target(FLOATING_A_ROOT, floating_a)
        .expect("floating tabs must initially be current");
    WorkspaceTransaction::from_commands([WorkspaceCommand::Open {
        item: ItemId::new(5),
        target: DockTarget::Center(floating_target),
    }])
    .apply(&mut workspace, &DockPolicySnapshot::default())
    .expect("peer mutation must make the frozen source stale");

    let before = workspace.clone();
    let error = WorkspaceTransaction::from_commands([WorkspaceCommand::PromoteContained {
        source: stale_source,
        surface: SOURCE,
        floating: FLOATING_A,
    }])
    .apply(&mut workspace, &DockPolicySnapshot::default())
    .expect_err("stale promotion source must reject atomically");
    assert!(matches!(
        command_error(error),
        CommandError::StaleNode {
            role: ReferenceRole::Source,
            node,
            ..
        } if node == floating_a
    ));
    assert_eq!(workspace, before);
}

#[test]
fn promotion_rejects_the_wrong_floating_identity_and_rolls_back_the_workspace() {
    let (mut workspace, main, floating_a, _, _) = composite_workspace();
    let main_source = workspace
        .capture_node_source(MAIN, main)
        .expect("main source must be current");
    WorkspaceTransaction::from_commands([WorkspaceCommand::RehomeRoot {
        source: main_source,
        target: RootPresentationTarget::NewSurface { surface: THIRD },
    }])
    .apply(&mut workspace, &native_policy())
    .expect("source surface must become rootless");

    let source = workspace
        .capture_node_source(FLOATING_A_ROOT, floating_a)
        .expect("floating source must be current");
    let before = workspace.clone();
    let error = WorkspaceTransaction::from_commands([WorkspaceCommand::PromoteContained {
        source,
        surface: SOURCE,
        floating: FLOATING_B,
    }])
    .apply(&mut workspace, &DockPolicySnapshot::default())
    .expect_err("mismatched floating identity must reject atomically");
    assert!(matches!(
        command_error(error),
        CommandError::FloatingPresentationMismatch {
            floating: FLOATING_B,
            expected_root: FLOATING_A_ROOT,
            expected_surface: SOURCE,
            actual_root: FLOATING_B_ROOT,
            actual_surface: SOURCE,
        }
    ));
    assert_eq!(workspace, before);
}

#[test]
fn promotion_rejects_an_occupied_main_slot_and_rolls_back_the_workspace() {
    let (mut workspace, _, floating_a, _, _) = composite_workspace();
    let source = workspace
        .capture_node_source(FLOATING_A_ROOT, floating_a)
        .expect("floating source must be current");
    let before = workspace.clone();
    let error = WorkspaceTransaction::from_commands([WorkspaceCommand::PromoteContained {
        source,
        surface: SOURCE,
        floating: FLOATING_A,
    }])
    .apply(&mut workspace, &DockPolicySnapshot::default())
    .expect_err("occupied main slot must reject atomically");
    assert!(matches!(
        command_error(error),
        CommandError::SurfaceMainOccupied {
            surface: SOURCE,
            root: MAIN,
        }
    ));
    assert_eq!(workspace, before);
}

#[test]
fn final_root_evacuation_prunes_only_at_transaction_finalization() {
    let (mut workspace, main, floating_a, floating_b, _) = composite_workspace();
    let main_source = workspace
        .capture_node_source(MAIN, main)
        .expect("main source");
    let a_source = workspace
        .capture_node_source(FLOATING_A_ROOT, floating_a)
        .expect("floating A source");
    let b_source = workspace
        .capture_node_source(FLOATING_B_ROOT, floating_b)
        .expect("floating B source");

    WorkspaceTransaction::from_commands([
        WorkspaceCommand::RehomeRoot {
            source: main_source,
            target: RootPresentationTarget::Contained {
                surface: TARGET,
                floating: FloatingPresentationId::new(100),
                rect: rect(1.0),
                position: ContainedPosition::Front,
            },
        },
        WorkspaceCommand::RehomeRoot {
            source: a_source,
            target: RootPresentationTarget::Contained {
                surface: TARGET,
                floating: FLOATING_A,
                rect: rect(10.0),
                position: ContainedPosition::Front,
            },
        },
        WorkspaceCommand::RehomeRoot {
            source: b_source,
            target: RootPresentationTarget::Contained {
                surface: TARGET,
                floating: FLOATING_B,
                rect: rect(20.0),
                position: ContainedPosition::Front,
            },
        },
    ])
    .apply(&mut workspace, &DockPolicySnapshot::default())
    .expect("one transaction must transfer the frozen block atomically");

    assert!(workspace.surface(SOURCE).is_none());
    assert_eq!(
        workspace
            .surface(TARGET)
            .expect("target surface must survive")
            .contained,
        [FloatingPresentationId::new(100), FLOATING_A, FLOATING_B,],
        "converted main, A, and B must remain one contiguous ordered block"
    );
}

#[test]
fn typed_positions_and_raise_use_structural_roster_preconditions() {
    let (mut workspace, _, _, _, _) = composite_workspace();
    let before_b = FloatingPresentationId::new(103);
    let after_a = FloatingPresentationId::new(104);

    for (root, floating, position, item, offset) in [
        (
            RootId::new(13),
            before_b,
            ContainedPosition::Before(FLOATING_B),
            ItemId::new(5),
            30.0,
        ),
        (
            RootId::new(14),
            after_a,
            ContainedPosition::After(FLOATING_A),
            ItemId::new(6),
            40.0,
        ),
    ] {
        WorkspaceTransaction::from_commands([WorkspaceCommand::CreateContainedRoot {
            surface: SOURCE,
            root,
            floating,
            rect: rect(offset),
            position,
            content: RootContent::OpenItem(item),
        }])
        .apply(&mut workspace, &DockPolicySnapshot::default())
        .expect("anchored creation must succeed");
    }
    assert_eq!(
        workspace.surface(SOURCE).expect("surface").contained,
        [FLOATING_A, after_a, before_b, FLOATING_B]
    );

    let roster = workspace
        .capture_contained_roster(SOURCE)
        .expect("roster capture must succeed");
    let source = workspace
        .capture_node_source(
            FLOATING_A_ROOT,
            workspace.root(FLOATING_A_ROOT).unwrap().node,
        )
        .expect("floating source");
    let report = WorkspaceTransaction::from_commands([WorkspaceCommand::RaiseContained {
        source,
        floating: FLOATING_A,
        expected_roster: roster,
    }])
    .apply(&mut workspace, &DockPolicySnapshot::default())
    .expect("raise must move the identity to the roster end");
    assert!(matches!(
        report.outcomes(),
        [CommandOutcome::ContainedRaised {
            from: 0,
            to: 3,
            changed: true,
            ..
        }]
    ));
    assert_eq!(
        workspace.surface(SOURCE).expect("surface").contained,
        [after_a, before_b, FLOATING_B, FLOATING_A]
    );

    let roster = workspace
        .capture_contained_roster(SOURCE)
        .expect("roster capture must succeed");
    let source = workspace
        .capture_node_source(
            FLOATING_A_ROOT,
            workspace.root(FLOATING_A_ROOT).unwrap().node,
        )
        .expect("floating source");
    let report = WorkspaceTransaction::from_commands([WorkspaceCommand::RaiseContained {
        source,
        floating: FLOATING_A,
        expected_roster: roster,
    }])
    .apply(&mut workspace, &DockPolicySnapshot::default())
    .expect("raising the frontmost identity must be idempotent");
    assert!(matches!(
        report.outcomes(),
        [CommandOutcome::ContainedRaised { changed: false, .. }]
    ));
}

#[test]
fn invalid_anchors_and_stale_raise_roll_back_without_reordering() {
    let (mut workspace, _, _, _, _) = composite_workspace();
    let foreign = FloatingPresentationId::new(202);
    WorkspaceTransaction::from_commands([WorkspaceCommand::CreateContainedRoot {
        surface: TARGET,
        root: RootId::new(21),
        floating: foreign,
        rect: rect(30.0),
        position: ContainedPosition::Front,
        content: RootContent::OpenItem(ItemId::new(5)),
    }])
    .apply(&mut workspace, &DockPolicySnapshot::default())
    .expect("foreign anchor fixture must exist");

    let cases = [
        (
            FloatingPresentationId::new(301),
            ContainedPosition::Before(FloatingPresentationId::new(301)),
            "self",
        ),
        (
            FloatingPresentationId::new(302),
            ContainedPosition::After(FloatingPresentationId::new(999)),
            "missing",
        ),
        (
            FloatingPresentationId::new(303),
            ContainedPosition::Before(foreign),
            "foreign",
        ),
    ];
    for (offset, (floating, position, kind)) in cases.into_iter().enumerate() {
        let before = workspace.clone();
        let error = WorkspaceTransaction::from_commands([WorkspaceCommand::CreateContainedRoot {
            surface: SOURCE,
            root: RootId::new(30 + u64::try_from(offset).unwrap()),
            floating,
            rect: rect(40.0),
            position,
            content: RootContent::OpenItem(ItemId::new(10 + u64::try_from(offset).unwrap())),
        }])
        .apply(&mut workspace, &DockPolicySnapshot::default())
        .expect_err("invalid anchor must reject atomically");
        let source = command_error(error);
        assert!(
            matches!(
                (kind, source),
                ("self", CommandError::ContainedAnchorIsSelf { .. })
                    | ("missing", CommandError::MissingContainedAnchor { .. })
                    | (
                        "foreign",
                        CommandError::ContainedAnchorOnDifferentSurface { .. }
                    )
            ),
            "unexpected {kind} rejection"
        );
        assert_eq!(workspace, before);
    }

    let stale_roster = workspace
        .capture_contained_roster(SOURCE)
        .expect("roster capture must succeed");
    let source = workspace
        .capture_node_source(
            FLOATING_A_ROOT,
            workspace.root(FLOATING_A_ROOT).unwrap().node,
        )
        .expect("floating source");
    WorkspaceTransaction::from_commands([WorkspaceCommand::CreateContainedRoot {
        surface: SOURCE,
        root: RootId::new(40),
        floating: FloatingPresentationId::new(304),
        rect: rect(50.0),
        position: ContainedPosition::Front,
        content: RootContent::OpenItem(ItemId::new(20)),
    }])
    .apply(&mut workspace, &DockPolicySnapshot::default())
    .expect("peer change must succeed");
    let before = workspace.clone();
    let error = WorkspaceTransaction::from_commands([WorkspaceCommand::RaiseContained {
        source,
        floating: FLOATING_A,
        expected_roster: stale_roster,
    }])
    .apply(&mut workspace, &DockPolicySnapshot::default())
    .expect_err("stale roster source must reject");
    assert!(matches!(
        command_error(error),
        CommandError::StaleContainedRoster {
            surface: SOURCE,
            ..
        }
    ));
    assert_eq!(workspace, before);
}

#[test]
fn disabling_contained_presentation_keeps_only_same_surface_operations_available() {
    let (mut workspace, _, floating_a, _, _) = composite_workspace();
    let mut policy = DockPolicy::default();
    policy.set_allow_contained_floating(false);
    let policy = policy.snapshot(PolicyRevision::default());

    WorkspaceTransaction::from_commands([WorkspaceCommand::UpdateContainedRect {
        surface: SOURCE,
        root: FLOATING_A_ROOT,
        floating: FLOATING_A,
        expected_rect: rect(10.0),
        rect: rect(11.0),
    }])
    .apply(&mut workspace, &policy)
    .expect("existing contained geometry must remain editable");

    let roster = workspace
        .capture_contained_roster(SOURCE)
        .expect("roster capture must succeed");
    let source = workspace
        .capture_node_source(FLOATING_A_ROOT, floating_a)
        .expect("floating source must be current");
    WorkspaceTransaction::from_commands([WorkspaceCommand::RaiseContained {
        source,
        floating: FLOATING_A,
        expected_roster: roster,
    }])
    .apply(&mut workspace, &policy)
    .expect("existing contained presentation must remain raisable");

    let source = workspace
        .capture_node_source(FLOATING_A_ROOT, floating_a)
        .expect("floating source must remain current");
    let before_cross_surface_rehome = workspace.clone();
    let error = WorkspaceTransaction::from_commands([WorkspaceCommand::RehomeRoot {
        source,
        target: RootPresentationTarget::Contained {
            surface: TARGET,
            floating: FLOATING_A,
            rect: rect(12.0),
            position: ContainedPosition::Front,
        },
    }])
    .apply(&mut workspace, &policy)
    .expect_err("cross-surface contained rehome must require contained presentation admission");
    assert!(matches!(
        command_error(error),
        CommandError::Policy(PolicyRejection::PresentationModeDisabled {
            mode: dockspace::policy::DockPresentationMode::Contained,
        })
    ));
    assert_eq!(workspace, before_cross_surface_rehome);

    let before = workspace.clone();
    assert!(
        WorkspaceTransaction::from_commands([WorkspaceCommand::CreateContainedRoot {
            surface: TARGET,
            root: RootId::new(70),
            floating: FloatingPresentationId::new(700),
            rect: rect(13.0),
            position: ContainedPosition::Front,
            content: RootContent::OpenItem(ItemId::new(70)),
        }])
        .apply(&mut workspace, &policy)
        .is_err(),
        "policy must still reject creation of a new contained presentation"
    );
    assert_eq!(workspace, before);
}

#[test]
fn cross_surface_contained_rehome_uses_presentation_policy_not_transform_policy() {
    let (mut workspace, _, floating_a, _, _) = composite_workspace();
    let source = workspace
        .capture_node_source(FLOATING_A_ROOT, floating_a)
        .expect("floating source must be current");
    let mut policy = DockPolicy::default();
    policy.set_allow_contained_transform(false);
    WorkspaceTransaction::from_commands([WorkspaceCommand::RehomeRoot {
        source,
        target: RootPresentationTarget::Contained {
            surface: TARGET,
            floating: FLOATING_A,
            rect: rect(12.0),
            position: ContainedPosition::Front,
        },
    }])
    .apply(&mut workspace, &policy.snapshot(PolicyRevision::default()))
    .expect("cross-surface rehome is a presentation, not a local transform");

    assert_eq!(
        workspace
            .surface(TARGET)
            .expect("target surface exists")
            .contained,
        vec![FLOATING_A]
    );
    assert!(
        workspace
            .surface(SOURCE)
            .expect("source surface exists")
            .contained
            .iter()
            .all(|floating| *floating != FLOATING_A)
    );
}

#[test]
fn complete_and_partial_payloads_fill_rootless_main_slots_explicitly() {
    let (mut workspace, main, _, _, target_main) = composite_workspace();
    let main_source = workspace
        .capture_node_source(MAIN, main)
        .expect("main source");
    WorkspaceTransaction::from_commands([WorkspaceCommand::RehomeRoot {
        source: main_source,
        target: RootPresentationTarget::NewSurface { surface: THIRD },
    }])
    .apply(&mut workspace, &native_policy())
    .expect("source must become rootless");

    let complete = workspace
        .capture_node_source(TARGET_MAIN, target_main)
        .expect("complete root source");
    WorkspaceTransaction::from_commands([WorkspaceCommand::RehomeRoot {
        source: complete,
        target: RootPresentationTarget::Main { surface: SOURCE },
    }])
    .apply(&mut workspace, &DockPolicySnapshot::default())
    .expect("complete root must preserve its identity when installed");
    assert_eq!(
        workspace.surface(SOURCE).unwrap().main_root,
        Some(TARGET_MAIN)
    );

    let complete = workspace
        .capture_node_source(TARGET_MAIN, target_main)
        .expect("complete root source");
    WorkspaceTransaction::from_commands([WorkspaceCommand::RehomeRoot {
        source: complete,
        target: RootPresentationTarget::NewSurface { surface: TARGET },
    }])
    .apply(&mut workspace, &native_policy())
    .expect("source must become rootless again");
    let main_target = workspace
        .capture_tab_target(MAIN, main)
        .expect("main tabs must remain current");
    WorkspaceTransaction::from_commands([WorkspaceCommand::Open {
        item: ItemId::new(5),
        target: DockTarget::Center(main_target),
    }])
    .apply(&mut workspace, &DockPolicySnapshot::default())
    .expect("the source root must contain content beyond the partial item");
    let item_source = workspace
        .capture_item_source(MAIN, main, ItemId::new(1))
        .expect("partial source must be current");
    let fresh = RootId::new(99);
    WorkspaceTransaction::from_commands([WorkspaceCommand::InstallMainRoot {
        surface: SOURCE,
        root: fresh,
        content: RootContent::Move(dockspace::command::MovePayload::Item(item_source)),
    }])
    .apply(&mut workspace, &DockPolicySnapshot::default())
    .expect("partial payload must use the caller-provided fresh root identity");
    assert_eq!(workspace.surface(SOURCE).unwrap().main_root, Some(fresh));
}

#[test]
fn external_builder_cannot_publish_an_empty_surface() {
    let mut builder = Workspace::builder();
    builder.set_surface(SOURCE, SurfacePresentation::rootless());
    let errors = builder
        .validate()
        .expect_err("external empty surface corruption must not be pruned")
        .into_errors();
    assert!(errors.iter().any(|error| matches!(
        error,
        WorkspaceValidationError::EmptySurfacePresentation { surface: SOURCE }
    )));
    assert!(builder.build().is_err());
}
