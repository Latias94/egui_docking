//! Empty central regions persist only while their main presentation owns them.

use super::*;
use crate::graph::transaction::WorkspaceTransaction;

const FIRST: ItemId = ItemId::new(3);
const SECOND: ItemId = ItemId::new(4);
const PEER: ItemId = ItemId::new(5);
const DESTINATION: ItemId = ItemId::new(9);

fn contained_fixture(items: &[ItemId], keep_main: bool, keep_peer: bool) -> (Workspace, NodeId) {
    let mut builder = Workspace::builder();
    let central = builder.insert_node(Node::tabs(items.iter().copied()));
    builder.set_root(CONTAINED_A, RootRecord::new(central).with_central(central));
    insert_root(&mut builder, SURVIVOR_ROOT, &[DESTINATION.get()]);
    builder.set_surface(SURVIVOR, SurfacePresentation::with_main(SURVIVOR_ROOT));
    let source = if keep_main {
        insert_root(&mut builder, MAIN, &[1]);
        SurfacePresentation::with_main(MAIN)
    } else {
        SurfacePresentation::rootless()
    };
    builder.set_surface(SOURCE, source);
    builder.set_contained_floating(FLOATING_A, ContainedFloating::new(CONTAINED_A, rect(10.0)));
    builder.attach_contained(SOURCE, FLOATING_A).unwrap();
    if keep_peer {
        insert_root(&mut builder, CONTAINED_B, &[PEER.get()]);
        builder.set_contained_floating(FLOATING_B, ContainedFloating::new(CONTAINED_B, rect(20.0)));
        builder.attach_contained(SOURCE, FLOATING_B).unwrap();
    }
    (builder.build().unwrap(), central)
}

fn nested_contained_fixture(
    rootless: bool,
    central_items: &[ItemId],
    sibling: ItemId,
) -> (Workspace, [NodeId; 3]) {
    let mut builder = Workspace::builder();
    let central = builder.insert_node(Node::tabs(central_items.iter().copied()));
    let donor = builder.insert_node(Node::tabs([sibling]));
    let split = builder.insert_node(Node::equal_split(Axis::Horizontal, [central, donor]).unwrap());
    builder.set_root(CONTAINED_A, RootRecord::new(split).with_central(central));
    insert_root(&mut builder, SURVIVOR_ROOT, &[DESTINATION.get()]);
    builder.set_surface(
        SOURCE,
        if rootless {
            SurfacePresentation::rootless()
        } else {
            SurfacePresentation::with_main(SURVIVOR_ROOT)
        },
    );
    if rootless {
        builder.set_surface(SURVIVOR, SurfacePresentation::with_main(SURVIVOR_ROOT));
    }
    builder.set_contained_floating(FLOATING_A, ContainedFloating::new(CONTAINED_A, rect(10.0)));
    builder.attach_contained(SOURCE, FLOATING_A).unwrap();
    (builder.build().unwrap(), [split, central, donor])
}

fn target(workspace: &Workspace) -> DockTarget {
    DockTarget::Center(
        workspace
            .capture_tab_target(SURVIVOR_ROOT, workspace.root(SURVIVOR_ROOT).unwrap().node)
            .unwrap(),
    )
}

fn move_item(workspace: &Workspace, item: ItemId) -> WorkspaceCommand {
    WorkspaceCommand::Move {
        payload: MovePayload::Item(workspace.capture_item_source_by_id(item).unwrap().unwrap()),
        target: target(workspace),
    }
}

fn assert_items(workspace: &Workspace, items: &[ItemId]) {
    workspace.validate().unwrap();
    assert_eq!(
        workspace.item_multiset(),
        items
            .iter()
            .copied()
            .map(|item| (item, 1))
            .collect::<BTreeMap<_, _>>(),
        "every surviving item must have exactly one owner",
    );
}

fn assert_retired(workspace: &Workspace, central: NodeId) {
    assert!(workspace.root(CONTAINED_A).is_none());
    assert!(workspace.node(central).is_none());
    assert!(workspace.contained_floating(FLOATING_A).is_none());
    assert!(workspace.presentation_for_root(CONTAINED_A).is_none());
    if let Some(surface) = workspace.surface(SOURCE) {
        assert!(!surface.contained.contains(&FLOATING_A));
    }
}

fn checked_in_both_paths(workspace: &Workspace, command: WorkspaceCommand) -> Workspace {
    let before = workspace.clone();
    let policy = default_policy();
    let commands = std::slice::from_ref(&command);
    let (local, local_report) = prepare_transaction(workspace, &policy, commands)
        .unwrap()
        .into_parts();
    local
        .validate()
        .expect("a single checked command must not leave orphaned nodes or vacant surfaces");
    let (general, general_report) = prepare_general_candidate(
        workspace,
        TransactionAuthority::Policy(&policy),
        commands,
        workspace.clone(),
    )
    .unwrap()
    .into_parts();
    assert_eq!(local, general);
    assert_eq!(local_report, general_report);
    assert_eq!(local_report.changed(), &local != workspace);
    assert_eq!(local.item_multiset(), workspace.item_multiset());

    // An unchanged selection forces the real multi-command path without
    // invalidating either reference captured for the following mutation.
    let tabs = workspace.root(SURVIVOR_ROOT).unwrap().node;
    let selected = match workspace.node(tabs).unwrap() {
        Node::Tabs { selected, .. } => selected.unwrap(),
        Node::Split { .. } => panic!("the destination fixture is one tabs leaf"),
    };
    let noop = WorkspaceCommand::Select {
        source: workspace
            .capture_item_source(SURVIVOR_ROOT, tabs, selected)
            .unwrap(),
    };
    let (batched, report) = prepare_transaction(workspace, &policy, &[noop, command])
        .unwrap()
        .into_parts();
    assert_eq!(batched, local);
    assert_eq!(&report.outcomes()[1..], local_report.outcomes());
    assert!(matches!(
        &report.outcomes()[0],
        CommandOutcome::Selected { changed: false, .. }
    ));
    assert_eq!(workspace, &before, "preparation must remain discardable");
    local.validate().unwrap();
    local
}

#[test]
fn moving_the_last_main_central_item_preserves_the_empty_main_region() {
    let mut builder = Workspace::builder();
    let central = builder.insert_node(Node::tabs([FIRST]));
    builder.set_root(MAIN, RootRecord::new(central).with_central(central));
    builder.set_surface(SOURCE, SurfacePresentation::with_main(MAIN));
    insert_root(&mut builder, SURVIVOR_ROOT, &[DESTINATION.get()]);
    builder.set_surface(SURVIVOR, SurfacePresentation::with_main(SURVIVOR_ROOT));
    let workspace = builder.build().unwrap();
    let candidate = checked_in_both_paths(&workspace, move_item(&workspace, FIRST));

    assert_eq!(candidate.root(MAIN), workspace.root(MAIN));
    assert_eq!(candidate.surface(SOURCE).unwrap().main_root, Some(MAIN));
    assert!(matches!(
        candidate.node(central),
        Some(Node::Tabs { items, selected: None, mru }) if items.is_empty() && mru.is_empty()
    ));
    assert_items(&candidate, &[FIRST, DESTINATION]);
}

#[test]
fn contained_central_two_item_moves_retire_only_the_last_empty_shell() {
    let (workspace, central) = contained_fixture(&[FIRST, SECOND], true, true);
    let after_first = checked_in_both_paths(&workspace, move_item(&workspace, FIRST));
    assert_eq!(after_first.root(CONTAINED_A), workspace.root(CONTAINED_A));
    assert_eq!(
        after_first.contained_floating(FLOATING_A),
        workspace.contained_floating(FLOATING_A),
    );
    assert_eq!(after_first.collect_items_in_subtree(central), [SECOND]);
    assert_items(
        &after_first,
        &[ItemId::new(1), FIRST, SECOND, PEER, DESTINATION],
    );

    let after_last = checked_in_both_paths(&after_first, move_item(&after_first, SECOND));
    assert_retired(&after_last, central);
    assert_eq!(after_last.surface(SOURCE).unwrap().main_root, Some(MAIN));
    assert_eq!(after_last.surface(SOURCE).unwrap().contained, [FLOATING_B]);
    assert_eq!(after_last.root(MAIN), workspace.root(MAIN));
    assert_eq!(after_last.root(CONTAINED_B), workspace.root(CONTAINED_B));
    assert_eq!(
        after_last.contained_floating(FLOATING_B),
        workspace.contained_floating(FLOATING_B),
    );
    assert_items(
        &after_last,
        &[ItemId::new(1), FIRST, SECOND, PEER, DESTINATION],
    );
}

#[test]
fn closing_the_last_contained_central_item_retires_the_root_and_only_vacant_surface() {
    for keep_source in [false, true] {
        let (workspace, central) = contained_fixture(&[FIRST], keep_source, keep_source);
        let policy = default_policy();
        let prepared = PreparedContentClose::Item {
            source: workspace.capture_item_source_by_id(FIRST).unwrap().unwrap(),
            expected_items: workspace.item_multiset(),
            requirement: CloseItemRequirement::new(FIRST, policy.pane_close_capability(FIRST)),
        };
        let before = workspace.clone();
        let closed = prepare_content_close(&workspace, &policy, &prepared).unwrap();
        assert_eq!(
            closed.outcome,
            CloseCommitOutcome::ItemClosed {
                item: FIRST,
                root: CONTAINED_A
            },
        );
        assert_eq!(workspace, before);
        assert_retired(&closed.candidate, central);
        assert_eq!(closed.candidate.surface(SOURCE).is_some(), keep_source);
        assert_eq!(
            closed.candidate.root(SURVIVOR_ROOT),
            workspace.root(SURVIVOR_ROOT)
        );
        if keep_source {
            assert_eq!(
                closed.candidate.surface(SOURCE).unwrap().contained,
                [FLOATING_B]
            );
            assert_eq!(closed.candidate.root(MAIN), workspace.root(MAIN));
            assert_eq!(
                closed.candidate.root(CONTAINED_B),
                workspace.root(CONTAINED_B)
            );
            assert_items(&closed.candidate, &[ItemId::new(1), PEER, DESTINATION]);
        } else {
            assert_items(&closed.candidate, &[DESTINATION]);
        }
    }
}

#[test]
fn moving_the_last_contained_central_item_prunes_its_rootless_source_surface() {
    let (workspace, central) = contained_fixture(&[FIRST], false, false);
    let candidate = checked_in_both_paths(&workspace, move_item(&workspace, FIRST));
    assert_retired(&candidate, central);
    assert!(candidate.surface(SOURCE).is_none());
    assert!(candidate.surface(SURVIVOR).is_some());
    assert_items(&candidate, &[FIRST, DESTINATION]);
}

#[test]
fn nested_contained_central_last_item_move_retires_its_entire_tree_with_main_survivor() {
    let (workspace, nodes) = nested_contained_fixture(false, &[], FIRST);
    let candidate = checked_in_both_paths(&workspace, move_item(&workspace, FIRST));
    assert_retired(&candidate, nodes[1]);
    for node in nodes {
        assert!(
            candidate.node(node).is_none(),
            "retired node {node:?} must not remain orphaned"
        );
    }
    assert_eq!(
        candidate.surface(SOURCE).unwrap().main_root,
        Some(SURVIVOR_ROOT)
    );
    assert!(candidate.surface(SOURCE).unwrap().contained.is_empty());
    assert_eq!(candidate.nodes().count(), 1);
    assert_items(&candidate, &[FIRST, DESTINATION]);
}

#[test]
fn nested_contained_central_last_item_move_prunes_its_rootless_surface() {
    let (workspace, nodes) = nested_contained_fixture(true, &[], FIRST);
    let candidate = checked_in_both_paths(&workspace, move_item(&workspace, FIRST));
    assert_retired(&candidate, nodes[1]);
    for node in nodes {
        assert!(
            candidate.node(node).is_none(),
            "retired node {node:?} must not remain orphaned"
        );
    }
    assert!(candidate.surface(SOURCE).is_none());
    assert_eq!(
        candidate.surface(SURVIVOR).unwrap().main_root,
        Some(SURVIVOR_ROOT)
    );
    assert_eq!(candidate.nodes().count(), 1);
    assert_items(&candidate, &[FIRST, DESTINATION]);
}

#[test]
fn moving_the_last_contained_item_into_its_own_central_region_keeps_the_floating_root() {
    for rootless in [false, true] {
        let (workspace, [split, central, donor]) = nested_contained_fixture(rootless, &[], FIRST);
        let command = WorkspaceCommand::Move {
            payload: MovePayload::Item(
                workspace
                    .capture_item_source(CONTAINED_A, donor, FIRST)
                    .unwrap(),
            ),
            target: DockTarget::Center(workspace.capture_tab_target(CONTAINED_A, central).unwrap()),
        };
        let candidate = checked_in_both_paths(&workspace, command);
        assert_eq!(
            candidate.root(CONTAINED_A),
            Some(&RootRecord::new(central).with_central(central)),
        );
        assert_eq!(
            candidate.contained_floating(FLOATING_A),
            workspace.contained_floating(FLOATING_A),
        );
        assert_eq!(candidate.surface(SOURCE), workspace.surface(SOURCE));
        assert!(candidate.node(split).is_none());
        assert!(candidate.node(donor).is_none());
        assert_eq!(candidate.collect_items_in_subtree(central), [FIRST]);
        assert_eq!(candidate.nodes().count(), 2);
        assert_items(&candidate, &[FIRST, DESTINATION]);
    }
}

#[test]
fn contained_central_singleton_edge_and_center_sequences_preserve_every_item_and_owner() {
    for rootless in [false, true] {
        for outer in [false, true] {
            for edge in [Edge::Left, Edge::Right, Edge::Top, Edge::Bottom] {
                let (workspace, [_, central, donor]) =
                    nested_contained_fixture(rootless, &[], FIRST);
                let fraction = DockFraction::new(0.5).unwrap();
                let target = if outer {
                    DockTarget::OuterEdge(
                        workspace
                            .capture_outer_edge_target(CONTAINED_A, edge, fraction)
                            .unwrap(),
                    )
                } else {
                    DockTarget::InnerEdge(
                        workspace
                            .capture_inner_edge_target(CONTAINED_A, central, edge, fraction)
                            .unwrap(),
                    )
                };
                let edge_command = WorkspaceCommand::Move {
                    payload: MovePayload::Item(
                        workspace
                            .capture_item_source(CONTAINED_A, donor, FIRST)
                            .unwrap(),
                    ),
                    target,
                };
                let edged = checked_in_both_paths(&workspace, edge_command.clone());
                assert_eq!(edged.root(CONTAINED_A).unwrap().central, Some(central));
                assert_eq!(
                    edged.contained_floating(FLOATING_A),
                    workspace.contained_floating(FLOATING_A),
                );
                assert_items(&edged, &[FIRST, DESTINATION]);

                let recenter = WorkspaceCommand::Move {
                    payload: MovePayload::Item(
                        edged.capture_item_source_by_id(FIRST).unwrap().unwrap(),
                    ),
                    target: DockTarget::Center(
                        edged.capture_tab_target(CONTAINED_A, central).unwrap(),
                    ),
                };
                let centered = checked_in_both_paths(&edged, recenter);
                assert_eq!(
                    centered.root(CONTAINED_A),
                    Some(&RootRecord::new(central).with_central(central)),
                );
                assert_eq!(
                    centered.contained_floating(FLOATING_A),
                    workspace.contained_floating(FLOATING_A),
                );
                assert_items(&centered, &[FIRST, DESTINATION]);

                let mut drained = checked_in_both_paths(&centered, move_item(&centered, FIRST));
                assert_retired(&drained, central);
                assert_eq!(drained.surface(SOURCE).is_none(), rootless);
                assert_items(&drained, &[FIRST, DESTINATION]);
                let before_rejected_replay = drained.clone();
                assert!(
                    WorkspaceTransaction::from_commands([edge_command])
                        .apply(&mut drained, &default_policy())
                        .is_err(),
                    "the retired source must reject its frozen command: rootless={rootless}, outer={outer}, edge={edge:?}",
                );
                assert_eq!(drained, before_rejected_replay);
                assert_items(&drained, &[FIRST, DESTINATION]);
            }
        }
    }
}

#[test]
fn moving_a_nested_contained_central_tabs_group_retires_the_empty_shell() {
    let (workspace, [split, central, donor]) =
        nested_contained_fixture(false, &[FIRST, SECOND], PEER);
    let after_peer = checked_in_both_paths(&workspace, move_item(&workspace, PEER));
    assert_eq!(
        after_peer.contained_floating(FLOATING_A),
        workspace.contained_floating(FLOATING_A),
    );
    assert_eq!(
        after_peer.root(CONTAINED_A),
        Some(&RootRecord::new(central).with_central(central)),
    );
    assert!(after_peer.node(split).is_none());
    assert!(after_peer.node(donor).is_none());
    let command = WorkspaceCommand::Move {
        payload: MovePayload::Tabs(
            after_peer
                .capture_node_source(CONTAINED_A, central)
                .unwrap(),
        ),
        target: target(&after_peer),
    };
    let candidate = checked_in_both_paths(&after_peer, command);
    assert_retired(&candidate, central);
    assert!(candidate.surface(SOURCE).unwrap().contained.is_empty());
    assert_eq!(
        candidate.surface(SOURCE).unwrap().main_root,
        Some(SURVIVOR_ROOT)
    );
    assert_eq!(candidate.nodes().count(), 1);
    assert_items(&candidate, &[FIRST, SECOND, PEER, DESTINATION]);

    // Keep both mutations in one actual draft: the second payload still comes
    // from a nested central leaf before final canonicalization removes its split.
    let destination = workspace.root(SURVIVOR_ROOT).unwrap().node;
    let mut batched = workspace.clone();
    let report = WorkspaceTransaction::apply_with(&mut batched, &default_policy(), |stage| {
        stage.push(|current| {
            Ok(WorkspaceCommand::Move {
                payload: MovePayload::Item(current.capture_item_source(
                    CONTAINED_A,
                    donor,
                    PEER,
                )?),
                target: DockTarget::Center(current.capture_tab_target(SURVIVOR_ROOT, destination)?),
            })
        })?;
        stage.push(|current| {
            assert_eq!(current.root(CONTAINED_A).unwrap().node, split);
            Ok(WorkspaceCommand::Move {
                payload: MovePayload::Tabs(current.capture_node_source(CONTAINED_A, central)?),
                target: DockTarget::Center(current.capture_tab_target(SURVIVOR_ROOT, destination)?),
            })
        })?;
        Ok(())
    })
    .unwrap();
    assert!(report.changed());
    assert_eq!(report.outcomes().len(), 2);
    assert_retired(&batched, central);
    assert_eq!(batched, candidate);
    assert_items(&batched, &[FIRST, SECOND, PEER, DESTINATION]);
}

#[test]
fn creating_a_new_root_from_a_contained_central_singleton_requires_rehome() {
    let (mut workspace, central) = contained_fixture(&[FIRST], false, false);
    let before = workspace.clone();
    let root = RootId::new(30);
    let floating = FloatingPresentationId::new(103);
    let command = WorkspaceCommand::CreateContainedRoot {
        surface: SURVIVOR,
        root,
        floating,
        rect: rect(30.0),
        position: ContainedPosition::Front,
        content: RootContent::Move(MovePayload::Item(
            workspace.capture_item_source_by_id(FIRST).unwrap().unwrap(),
        )),
    };
    let destination = workspace.root(SURVIVOR_ROOT).unwrap().node;
    let noop = WorkspaceCommand::Select {
        source: workspace
            .capture_item_source(SURVIVOR_ROOT, destination, DESTINATION)
            .unwrap(),
    };
    for commands in [vec![command.clone()], vec![noop, command]] {
        let error = WorkspaceTransaction::from_commands(commands.clone())
            .apply(&mut workspace, &default_policy())
            .unwrap_err();
        assert!(matches!(
            error,
            TransactionError::Command {
                index,
                source: CommandError::WholeRootRequiresRehome { root: CONTAINED_A },
            } if index == commands.len() - 1
        ));
        assert_eq!(workspace, before);
        assert_eq!(workspace.root(CONTAINED_A).unwrap().central, Some(central));
        assert!(workspace.root(root).is_none());
        assert!(workspace.contained_floating(floating).is_none());
        assert_items(&workspace, &[FIRST, DESTINATION]);
    }
}

#[test]
fn rehoming_a_contained_central_singleton_preserves_root_central_and_floating_identity() {
    let (workspace, central) = contained_fixture(&[FIRST], false, false);
    let command = WorkspaceCommand::RehomeRoot {
        source: workspace.capture_node_source(CONTAINED_A, central).unwrap(),
        target: RootPresentationTarget::Contained {
            surface: SURVIVOR,
            floating: FLOATING_A,
            rect: rect(30.0),
            position: ContainedPosition::Front,
        },
    };
    let candidate = checked_in_both_paths(&workspace, command);
    assert!(candidate.surface(SOURCE).is_none());
    assert_eq!(candidate.root(CONTAINED_A), workspace.root(CONTAINED_A));
    assert_eq!(candidate.node(central), workspace.node(central));
    assert_eq!(
        candidate.contained_floating(FLOATING_A).unwrap().root,
        CONTAINED_A
    );
    assert_eq!(
        candidate.contained_floating(FLOATING_A).unwrap().rect,
        rect(30.0)
    );
    assert_eq!(candidate.surface(SURVIVOR).unwrap().contained, [FLOATING_A]);
    assert_eq!(candidate.root(SURVIVOR_ROOT), workspace.root(SURVIVOR_ROOT));
    assert_items(&candidate, &[FIRST, DESTINATION]);
}

#[test]
fn rehoming_a_contained_central_singleton_to_main_changes_its_empty_retention_rule() {
    let new_surface = SurfaceId::new(3);
    let mut builder = Workspace::builder();
    let central = builder.insert_node(Node::tabs([FIRST]));
    builder.set_root(CONTAINED_A, RootRecord::new(central).with_central(central));
    insert_root(&mut builder, SURVIVOR_ROOT, &[DESTINATION.get()]);
    insert_root(&mut builder, CONTAINED_B, &[PEER.get()]);
    builder.set_surface(SOURCE, SurfacePresentation::rootless());
    builder.set_surface(SURVIVOR, SurfacePresentation::with_main(SURVIVOR_ROOT));
    builder.set_surface(new_surface, SurfacePresentation::rootless());
    builder.set_contained_floating(FLOATING_A, ContainedFloating::new(CONTAINED_A, rect(10.0)));
    builder.set_contained_floating(FLOATING_B, ContainedFloating::new(CONTAINED_B, rect(20.0)));
    builder.attach_contained(SOURCE, FLOATING_A).unwrap();
    builder.attach_contained(new_surface, FLOATING_B).unwrap();
    let workspace = builder.build().unwrap();
    let command = WorkspaceCommand::RehomeRoot {
        source: workspace.capture_node_source(CONTAINED_A, central).unwrap(),
        target: RootPresentationTarget::Main {
            surface: new_surface,
        },
    };
    let rehomed = checked_in_both_paths(&workspace, command);
    assert!(rehomed.surface(SOURCE).is_none());
    assert!(rehomed.contained_floating(FLOATING_A).is_none());
    assert_eq!(rehomed.root(CONTAINED_A), workspace.root(CONTAINED_A));
    assert_eq!(
        rehomed.surface(new_surface).unwrap().main_root,
        Some(CONTAINED_A)
    );
    assert_eq!(
        rehomed.surface(new_surface).unwrap().contained,
        [FLOATING_B]
    );
    assert_items(&rehomed, &[FIRST, PEER, DESTINATION]);

    let emptied = checked_in_both_paths(&rehomed, move_item(&rehomed, FIRST));
    assert_eq!(emptied.root(CONTAINED_A), rehomed.root(CONTAINED_A));
    assert_eq!(
        emptied.surface(new_surface).unwrap().main_root,
        Some(CONTAINED_A)
    );
    assert!(emptied.collect_items_in_subtree(central).is_empty());
    assert_eq!(emptied.root(CONTAINED_B), workspace.root(CONTAINED_B));
    assert_eq!(
        emptied.contained_floating(FLOATING_B),
        workspace.contained_floating(FLOATING_B),
    );
    assert_items(&emptied, &[FIRST, PEER, DESTINATION]);
}

#[test]
fn a_failed_followup_rolls_back_contained_central_retirement_and_other_content() {
    for keep_source in [false, true] {
        let (mut workspace, central) = contained_fixture(&[FIRST], keep_source, keep_source);
        let before = workspace.clone();
        let movement = move_item(&workspace, FIRST);
        let duplicate = WorkspaceCommand::Open {
            item: DESTINATION,
            target: target(&workspace),
        };
        let error = WorkspaceTransaction::from_commands([movement, duplicate])
            .apply(&mut workspace, &default_policy())
            .unwrap_err();
        assert!(matches!(
            error,
            TransactionError::Command {
                index: 1,
                source: CommandError::ItemAlreadyOpen { item: DESTINATION },
            }
        ));
        assert_eq!(workspace, before);
        assert!(workspace.node(central).is_some());
        assert!(workspace.contained_floating(FLOATING_A).is_some());
        workspace.validate().unwrap();
    }
}
