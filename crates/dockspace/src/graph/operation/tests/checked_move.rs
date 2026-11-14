//! Candidate-local authority and execution regressions through real transactions.

use super::*;
use crate::engine::surface_recovery::{
    SurfaceMainRehome, SurfaceRehomePlacement, SurfaceRosterDisposition,
};
use crate::graph::transaction::WorkspaceTransaction;
use crate::interaction::targeting::resolver::structural_work;
use crate::policy::{DockTargetRuleKey, TabBarInteraction, TabBarPolicy, TabBarVisibility};

struct Fixture {
    workspace: Workspace,
    leaves: [[NodeId; 2]; 3],
}

fn root(index: usize) -> RootId {
    RootId::new(u64::try_from(index + 1).unwrap())
}

fn fixture() -> Fixture {
    let mut builder = Workspace::builder();
    let leaves = std::array::from_fn(|view| {
        let leaves = std::array::from_fn(|pane| {
            let item = u64::try_from(view * 4 + pane * 2 + 1).unwrap();
            builder.insert_node(Node::tabs([ItemId::new(item), ItemId::new(item + 1)]))
        });
        let split = builder.insert_node(Node::equal_split(Axis::Horizontal, leaves).unwrap());
        builder.set_root(root(view), RootRecord::new(split));
        builder.set_surface(
            SurfaceId::new(u64::try_from(view + 1).unwrap()),
            SurfacePresentation::with_main(root(view)),
        );
        leaves
    });
    Fixture {
        workspace: builder.build().unwrap(),
        leaves,
    }
}

fn item_source(fixture: &Fixture, view: usize, pane: usize, second: bool) -> ItemSource {
    let item = u64::try_from(view * 4 + pane * 2 + 1 + usize::from(second)).unwrap();
    fixture
        .workspace
        .capture_item_source(root(view), fixture.leaves[view][pane], ItemId::new(item))
        .unwrap()
}

fn target(fixture: &Fixture, view: usize, kind: usize) -> DockTarget {
    let workspace = &fixture.workspace;
    let tabs = fixture.leaves[view][1];
    let fraction = DockFraction::new(0.5).unwrap();
    match kind {
        0 => DockTarget::Center(workspace.capture_tab_target(root(view), tabs).unwrap()),
        1 => DockTarget::TabGap {
            target: workspace.capture_tab_target(root(view), tabs).unwrap(),
            index: 1,
        },
        2 => DockTarget::InnerEdge(
            workspace
                .capture_inner_edge_target(root(view), tabs, Edge::Top, fraction)
                .unwrap(),
        ),
        3 => DockTarget::OuterEdge(
            workspace
                .capture_outer_edge_target(root(view), Edge::Top, fraction)
                .unwrap(),
        ),
        _ => unreachable!(),
    }
}

fn assert_stale_batch(fixture: &mut Fixture, commands: [WorkspaceCommand; 2], role: ReferenceRole) {
    let before = fixture.workspace.clone();
    let error = WorkspaceTransaction::from_commands(commands)
        .apply(&mut fixture.workspace, &DockPolicySnapshot::default())
        .unwrap_err();
    assert!(matches!(
        error,
        TransactionError::Command {
            index: 1,
            source: CommandError::StaleNode { role: actual, .. },
        } if actual == role
    ));
    assert_eq!(fixture.workspace, before);
}

#[test]
fn selection_before_move_rechecks_every_target_against_staged_state() {
    for kind in 0..4 {
        let mut fixture = fixture();
        let commands = [
            WorkspaceCommand::Select {
                source: item_source(&fixture, 1, 1, true),
            },
            WorkspaceCommand::Move {
                payload: MovePayload::Item(item_source(&fixture, 0, 0, true)),
                target: target(&fixture, 1, kind),
            },
        ];
        assert_stale_batch(&mut fixture, commands, ReferenceRole::Target);
    }
}

#[test]
fn selection_before_move_rechecks_all_payload_kinds() {
    for kind in 0..3 {
        let mut fixture = fixture();
        let payload = match kind {
            0 => MovePayload::Item(item_source(&fixture, 0, 0, false)),
            1 => MovePayload::Tabs(
                fixture
                    .workspace
                    .capture_node_source(root(0), fixture.leaves[0][0])
                    .unwrap(),
            ),
            2 => MovePayload::Subtree(
                fixture
                    .workspace
                    .capture_node_source(root(0), fixture.workspace.root(root(0)).unwrap().node)
                    .unwrap(),
            ),
            _ => unreachable!(),
        };
        let commands = [
            WorkspaceCommand::Select {
                source: item_source(&fixture, 0, 0, true),
            },
            WorkspaceCommand::Move {
                payload,
                target: target(&fixture, 1, 2),
            },
        ];
        assert_stale_batch(&mut fixture, commands, ReferenceRole::Source);
    }
}

#[test]
fn earlier_move_invalidates_later_target_without_reusing_batch_facts() {
    let mut fixture = fixture();
    let commands = [
        WorkspaceCommand::Move {
            payload: MovePayload::Item(item_source(&fixture, 0, 0, true)),
            target: target(&fixture, 1, 0),
        },
        WorkspaceCommand::Move {
            payload: MovePayload::Item(item_source(&fixture, 2, 0, true)),
            target: target(&fixture, 1, 0),
        },
    ];
    assert_stale_batch(&mut fixture, commands, ReferenceRole::Target);
}

#[test]
fn current_same_root_source_does_not_accept_an_older_target_snapshot() {
    let mut fixture = fixture();
    let stale_target = target(&fixture, 0, 0);
    let selection = item_source(&fixture, 0, 1, true);
    WorkspaceTransaction::from_commands([WorkspaceCommand::Select { source: selection }])
        .apply(&mut fixture.workspace, &DockPolicySnapshot::default())
        .unwrap();
    let payload = MovePayload::Item(item_source(&fixture, 0, 0, true));
    let before = fixture.workspace.clone();
    let error = WorkspaceTransaction::from_commands([WorkspaceCommand::Move {
        payload,
        target: stale_target,
    }])
    .apply(&mut fixture.workspace, &DockPolicySnapshot::default())
    .unwrap_err();
    assert!(matches!(
        error,
        TransactionError::Command {
            index: 0,
            source: CommandError::StaleNode {
                role: ReferenceRole::Target,
                ..
            },
        }
    ));
    assert_eq!(fixture.workspace, before);
}

#[test]
fn current_fingerprint_cannot_authorize_forged_target_policy_facts() {
    for kind in 0..4 {
        for field in 0..3 {
            let mut fixture = fixture();
            let mut target = target(&fixture, 1, kind);
            let (surface, central, rule) = match &mut target {
                DockTarget::Center(target) | DockTarget::TabGap { target, .. } => {
                    (&mut target.surface, &mut target.central, &mut target.rule)
                }
                DockTarget::InnerEdge(target) | DockTarget::OuterEdge(target) => {
                    (&mut target.surface, &mut target.central, &mut target.rule)
                }
            };
            match field {
                0 => *surface = SurfaceId::new(3),
                1 => *central = !*central,
                2 => *rule = DockTargetRuleKey::Item(ItemId::new(1)),
                _ => unreachable!(),
            }
            let command = WorkspaceCommand::Move {
                payload: MovePayload::Item(item_source(&fixture, 0, 0, true)),
                target,
            };
            let before = fixture.workspace.clone();
            let error = WorkspaceTransaction::from_commands([command])
                .apply(&mut fixture.workspace, &DockPolicySnapshot::default())
                .unwrap_err();
            assert!(matches!(
                error,
                TransactionError::Command {
                    index: 0,
                    source: CommandError::Invariant { .. },
                }
            ));
            assert_eq!(fixture.workspace, before);
        }
    }
}

#[test]
fn scoped_facts_reject_foreign_nodes_for_both_reference_roles() {
    for role in [ReferenceRole::Source, ReferenceRole::Target] {
        let mut fixture = fixture();
        let mut source = item_source(&fixture, 0, 0, true);
        let mut target = fixture
            .workspace
            .capture_tab_target(root(1), fixture.leaves[1][1])
            .unwrap();
        match role {
            ReferenceRole::Source => source.root = root(1),
            ReferenceRole::Target => target.root = root(0),
        }
        let command = WorkspaceCommand::Move {
            payload: MovePayload::Item(source),
            target: DockTarget::Center(target),
        };
        let before = fixture.workspace.clone();
        let error = WorkspaceTransaction::from_commands([command])
            .apply(&mut fixture.workspace, &DockPolicySnapshot::default())
            .unwrap_err();
        assert!(matches!(
            error,
            TransactionError::Command {
                index: 0,
                source: CommandError::NodeOutsideRoot { role: actual, .. },
            } if actual == role
        ));
        assert_eq!(fixture.workspace, before);
    }
}

#[test]
fn same_tabs_noop_move_still_checks_tab_gap_policy() {
    let mut fixture = fixture();
    let command = WorkspaceCommand::Move {
        payload: MovePayload::Item(item_source(&fixture, 0, 1, false)),
        target: DockTarget::TabGap {
            target: fixture
                .workspace
                .capture_tab_target(root(0), fixture.leaves[0][1])
                .unwrap(),
            index: 0,
        },
    };
    let mut policy = DockPolicy::default();
    policy.set_tab_bar(TabBarPolicy::new(
        TabBarVisibility::Visible,
        TabBarInteraction::Disabled,
    ));
    let before = fixture.workspace.clone();
    let error = WorkspaceTransaction::from_commands([command])
        .apply(
            &mut fixture.workspace,
            &policy.snapshot(PolicyRevision::new(1)),
        )
        .unwrap_err();
    assert!(matches!(
        error,
        TransactionError::Command {
            index: 0,
            source: CommandError::Policy(_),
        }
    ));
    assert_eq!(fixture.workspace, before);
}

#[test]
fn one_move_uses_current_owner_refs_without_building_root_fingerprints() {
    for view in [0, 1] {
        for kind in 0..4 {
            let mut fixture = fixture();
            let command = WorkspaceCommand::Move {
                payload: MovePayload::Item(item_source(&fixture, 0, 0, true)),
                target: target(&fixture, view, kind),
            };
            structural_work::reset();
            let report = WorkspaceTransaction::from_commands([command])
                .apply(&mut fixture.workspace, &DockPolicySnapshot::default())
                .unwrap();
            let work = structural_work::snapshot();
            assert!(report.changed());
            assert_eq!(work.root_fingerprint_builds, 0);
            assert_eq!(work.root_fingerprint_node_visits, 0);
            fixture.workspace.validate().unwrap();
        }
    }
}

#[test]
fn moving_sibling_tabs_uses_parent_weights_after_detach() {
    let mut builder = Workspace::builder();
    let leaves = [1, 2, 3].map(|id| builder.insert_node(Node::tabs([ItemId::new(id)])));
    let split =
        builder.insert_node(Node::split(Axis::Horizontal, leaves, [0.2, 0.3, 0.5]).unwrap());
    builder.set_root(root(0), RootRecord::new(split));
    builder.set_surface(SurfaceId::new(1), SurfacePresentation::with_main(root(0)));
    let mut workspace = builder.build().unwrap();
    let command = WorkspaceCommand::Move {
        payload: MovePayload::Tabs(workspace.capture_node_source(root(0), leaves[0]).unwrap()),
        target: DockTarget::InnerEdge(
            workspace
                .capture_inner_edge_target(
                    root(0),
                    leaves[1],
                    Edge::Right,
                    DockFraction::new(0.5).unwrap(),
                )
                .unwrap(),
        ),
    };
    WorkspaceTransaction::from_commands([command])
        .apply(&mut workspace, &DockPolicySnapshot::default())
        .unwrap();
    let Node::Split {
        children, weights, ..
    } = workspace.node(split).unwrap()
    else {
        panic!("split must survive the move");
    };
    assert_eq!(children, &[leaves[1], leaves[0], leaves[2]]);
    for (actual, expected) in weights.iter().zip([0.1875, 0.1875, 0.625]) {
        assert!((actual.get() - expected).abs() < 1e-6);
    }
    workspace.validate().unwrap();
}

#[test]
fn exact_main_move_recovery_bypasses_policy_but_rechecks_its_target() {
    for split_payload in [false, true] {
        let mut builder = Workspace::builder();
        let first = builder.insert_node(Node::tabs([ItemId::new(1), ItemId::new(2)]));
        let source = if split_payload {
            let second = builder.insert_node(Node::tabs([ItemId::new(3)]));
            builder.insert_node(Node::equal_split(Axis::Horizontal, [first, second]).unwrap())
        } else {
            first
        };
        let target_tabs = builder.insert_node(Node::tabs([ItemId::new(5), ItemId::new(6)]));
        builder.set_root(root(0), RootRecord::new(source));
        builder.set_root(root(1), RootRecord::new(target_tabs));
        builder.set_surface(SurfaceId::new(1), SurfacePresentation::with_main(root(0)));
        builder.set_surface(SurfaceId::new(2), SurfacePresentation::with_main(root(1)));
        let workspace = builder.build().unwrap();
        let target = if split_payload {
            DockTarget::InnerEdge(
                workspace
                    .capture_inner_edge_target(
                        root(1),
                        target_tabs,
                        Edge::Top,
                        DockFraction::new(0.5).unwrap(),
                    )
                    .unwrap(),
            )
        } else {
            DockTarget::Center(workspace.capture_tab_target(root(1), target_tabs).unwrap())
        };
        let roster =
            SurfaceRosterDisposition::capture(&workspace, SurfaceId::new(1), None).unwrap();
        let recovery = roster
            .compile_rehome_transaction(
                &workspace,
                &SurfaceRehomePlacement::new(
                    SurfaceId::new(2),
                    Some(SurfaceMainRehome::Dock(target)),
                    Vec::new(),
                ),
            )
            .unwrap();
        assert!(
            matches!(
                recovery.commands(),
                [WorkspaceCommand::Move { payload: MovePayload::Subtree(_), .. }] if split_payload
            ) || matches!(
                recovery.commands(),
                [WorkspaceCommand::Move { payload: MovePayload::Tabs(_), .. }] if !split_payload
            )
        );

        let mut denied = DockPolicy::default();
        denied.set_arrangement_locked(true);
        denied.set_allow_tab_merge(false);
        denied.set_allow_edge_split(false);
        let mut ordinary = workspace.clone();
        assert!(matches!(
            WorkspaceTransaction::from_commands(recovery.commands().to_vec())
                .apply(&mut ordinary, &denied.snapshot(PolicyRevision::new(1))),
            Err(TransactionError::Command {
                index: 0,
                source: CommandError::Policy(PolicyRejection::ArrangementLocked),
            })
        ));
        assert_eq!(ordinary, workspace);

        let mut stale = workspace.clone();
        let selection = stale
            .capture_item_source(root(1), target_tabs, ItemId::new(6))
            .unwrap();
        WorkspaceTransaction::from_commands([WorkspaceCommand::Select { source: selection }])
            .apply(&mut stale, &DockPolicySnapshot::default())
            .unwrap();
        let before = stale.clone();
        assert!(matches!(
            recovery.apply(&mut stale),
            Err(TransactionError::Command {
                index: 0,
                source: CommandError::StaleNode {
                    role: ReferenceRole::Target,
                    ..
                },
            })
        ));
        assert_eq!(stale, before);

        let items = workspace.item_multiset();
        let mut recovered = workspace;
        recovery.apply(&mut recovered).unwrap();
        assert!(recovered.root(root(0)).is_none());
        assert!(recovered.surface(SurfaceId::new(1)).is_none());
        assert_eq!(recovered.item_multiset(), items);
        recovered.validate().unwrap();
    }
}
