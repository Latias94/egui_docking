//! Exact issuance and actual placement through the real checked executor.

use super::*;
use crate::canonical::{CanonicalizationError, canonicalize_workspace};
use crate::drop_resolver::structural_work;
use crate::transaction::WorkspaceTransaction;
use crate::validation::WorkspaceValidationError;
use crate::workspace::ParentLink;

const A: RootId = RootId::new(1);
const B: RootId = RootId::new(2);
const C: RootId = RootId::new(3);

fn fixture() -> (Workspace, [NodeId; 4]) {
    let mut builder = Workspace::builder();
    let tabs = [[1, 2], [3, 4], [5, 6], [7, 8]]
        .map(|items| builder.insert_node(Node::tabs(items.map(ItemId::new))));
    let split = builder
        .insert_node(Node::equal_split(Axis::Horizontal, tabs[..2].iter().copied()).unwrap());
    for (root, node) in [(A, split), (B, tabs[2]), (C, tabs[3])] {
        builder.set_root(root, RootRecord::new(node));
        builder.set_surface(
            SurfaceId::new(root.get()),
            SurfacePresentation::with_main(root),
        );
    }
    (builder.build().unwrap(), tabs)
}

/// Derive the expected directory from actual root records and child rosters,
/// without consulting owner placement while constructing the expectation.
pub(super) fn assert_actual_placement(workspace: &Workspace) {
    let mut expected = BTreeMap::new();
    for (root, record) in workspace.roots() {
        let mut pending = vec![(record.node, None, vec![record.node])];
        while let Some((node, parent, path)) = pending.pop() {
            assert!(
                expected
                    .insert(node, (root, parent, path.clone()))
                    .is_none()
            );
            if let Some(Node::Split { children, .. }) = workspace.node(node) {
                for (index, &child) in children.iter().enumerate() {
                    let mut child_path = path.clone();
                    child_path.push(child);
                    pending.push((
                        child,
                        Some(ParentLink {
                            parent: node,
                            index,
                        }),
                        child_path,
                    ));
                }
            }
        }
        assert_eq!(
            workspace.presentation_for_root(root),
            workspace.presentation_in_storage(root)
        );
    }
    for (node, _) in workspace.nodes() {
        assert_eq!(
            workspace.node_root(node),
            expected.get(&node).map(|entry| entry.0)
        );
        for (root, _) in workspace.roots() {
            let owned = expected.get(&node).filter(|entry| entry.0 == root);
            assert_eq!(workspace.root_contains_node(root, node), owned.is_some());
            assert_eq!(
                workspace.parent_link(root, node),
                owned.and_then(|entry| entry.1)
            );
            assert_eq!(
                workspace.path_to_node(root, node).as_ref(),
                owned.map(|entry| &entry.2)
            );
        }
    }
}

#[test]
fn clone_keeps_exact_refs_independent_build_and_published_aba_reject_them() {
    let (mut workspace, tabs) = fixture();
    let original = workspace.clone();
    let source = workspace
        .capture_item_source(A, tabs[0], ItemId::new(1))
        .unwrap();
    let unaffected = workspace.capture_node_source(C, tabs[3]).unwrap();
    let clone = workspace.clone();
    clone
        .verify_reference(A, tabs[0], source.reference(), ReferenceRole::Source)
        .unwrap();
    let independent = original.clone().into_builder().build().unwrap();
    assert_eq!(independent, original);
    assert!(matches!(
        independent.verify_reference(A, tabs[0], source.reference(), ReferenceRole::Source),
        Err(CommandError::StaleNode { .. })
    ));

    let policy = default_policy();
    WorkspaceTransaction::from_commands([WorkspaceCommand::Select {
        source: source.clone(),
    }])
    .apply(&mut workspace, &policy)
    .unwrap();
    canonicalize_workspace(&mut workspace).unwrap();
    workspace
        .verify_reference(A, tabs[0], source.reference(), ReferenceRole::Source)
        .unwrap();
    for item in [2, 1] {
        let source = workspace
            .capture_item_source(A, tabs[0], ItemId::new(item))
            .unwrap();
        WorkspaceTransaction::from_commands([WorkspaceCommand::Select { source }])
            .apply(&mut workspace, &policy)
            .unwrap();
    }
    assert_eq!(workspace, original);
    assert!(matches!(
        workspace.verify_reference(A, tabs[0], source.reference(), ReferenceRole::Source),
        Err(CommandError::StaleNode { .. })
    ));
    workspace
        .verify_reference(C, tabs[3], unaffected.reference(), ReferenceRole::Source)
        .unwrap();
    assert_actual_placement(&workspace);
}

#[test]
fn actual_stage_supports_net_zero_selection_but_rejects_predicted_branch_refs() {
    let (mut workspace, tabs) = fixture();
    let original = workspace.clone();
    let policy = default_policy();
    let old = workspace
        .capture_item_source(A, tabs[0], ItemId::new(1))
        .unwrap();
    let first = WorkspaceCommand::Select {
        source: workspace
            .capture_item_source(A, tabs[0], ItemId::new(2))
            .unwrap(),
    };
    let mut predicted = workspace.clone();
    WorkspaceTransaction::from_commands([first.clone()])
        .apply(&mut predicted, &policy)
        .unwrap();
    let predicted_source = predicted
        .capture_item_source(A, tabs[0], ItemId::new(1))
        .unwrap();
    let error = WorkspaceTransaction::from_commands([
        first,
        WorkspaceCommand::Select {
            source: predicted_source,
        },
    ])
    .apply(&mut workspace, &policy)
    .unwrap_err();
    assert!(matches!(
        error,
        TransactionError::Command {
            index: 1,
            source: CommandError::StaleNode {
                role: ReferenceRole::Source,
                ..
            }
        }
    ));
    assert_eq!(workspace, original);

    let report = WorkspaceTransaction::apply_with(&mut workspace, &policy, |stage| {
        for item in [2, 1] {
            stage.push(|current| {
                Ok(WorkspaceCommand::Select {
                    source: current.capture_item_source(A, tabs[0], ItemId::new(item))?,
                })
            })?;
        }
        Ok(())
    })
    .unwrap();
    assert!(!report.changed());
    assert!(matches!(
        report.outcomes(),
        [
            CommandOutcome::Selected { changed: true, .. },
            CommandOutcome::Selected { changed: true, .. }
        ]
    ));
    assert_eq!(workspace, original);
    assert_eq!(
        workspace.tab_mru(tabs[0]),
        Some([ItemId::new(1), ItemId::new(2)].as_slice())
    );
    assert!(matches!(
        workspace.verify_reference(A, tabs[0], old.reference(), ReferenceRole::Source),
        Err(CommandError::StaleNode { .. })
    ));
}

#[test]
fn ignoring_a_stage_error_cannot_publish_or_capture_again() {
    let (mut workspace, tabs) = fixture();
    let original = workspace.clone();
    let old = workspace
        .capture_item_source(A, tabs[0], ItemId::new(1))
        .unwrap();
    let mut called_after_error = false;
    let result = WorkspaceTransaction::apply_with(&mut workspace, &default_policy(), |stage| {
        stage.push(|current| {
            Ok(WorkspaceCommand::Select {
                source: current.capture_item_source(A, tabs[0], ItemId::new(2))?,
            })
        })?;
        let ignored = stage.push(|_| {
            Ok(WorkspaceCommand::Select {
                source: old.clone(),
            })
        });
        assert!(ignored.is_err());
        let ignored = stage.push(|current| {
            called_after_error = true;
            Ok(WorkspaceCommand::Select {
                source: current.capture_item_source(A, tabs[0], ItemId::new(1))?,
            })
        });
        assert!(ignored.is_err());
        Ok(())
    });
    assert!(matches!(
        result,
        Err(TransactionError::Command {
            index: 1,
            source: CommandError::StaleNode { .. }
        })
    ));
    assert!(!called_after_error);
    assert_eq!(workspace, original);
    workspace
        .verify_reference(A, tabs[0], old.reference(), ReferenceRole::Source)
        .unwrap();
}

#[test]
fn staged_moves_keep_transient_structure_until_one_final_normalization_and_roll_back_late_errors() {
    for invalid_rect in [false, true] {
        let mut builder = Workspace::builder();
        let source = builder.insert_node(Node::tabs([ItemId::new(1)]));
        let peer = builder.insert_node(Node::tabs([ItemId::new(2)]));
        let split =
            builder.insert_node(Node::equal_split(Axis::Horizontal, [source, peer]).unwrap());
        let target = builder.insert_node(Node::tabs([ItemId::new(3)]));
        builder.set_root(A, RootRecord::new(split));
        builder.set_root(B, RootRecord::new(target));
        builder.set_surface(SurfaceId::new(1), SurfacePresentation::with_main(A));
        builder.set_surface(SurfaceId::new(2), SurfacePresentation::with_main(B));
        let mut workspace = builder.build().unwrap();
        let original = workspace.clone();
        let old = workspace.capture_node_source(A, split).unwrap();
        let floating = FloatingPresentationId::new(99);
        structural_work::reset();
        let result = WorkspaceTransaction::apply_with(&mut workspace, &default_policy(), |stage| {
            stage.push(|current| {
                Ok(WorkspaceCommand::Move {
                    payload: MovePayload::Item(current.capture_item_source(
                        A,
                        source,
                        ItemId::new(1),
                    )?),
                    target: DockTarget::Center(current.capture_tab_target(B, target)?),
                })
            })?;
            stage.push(|current| {
                assert!(matches!(current.node(source), Some(Node::Tabs { items, .. }) if items.is_empty()));
                assert_eq!(current.root(A).unwrap().node, split);
                Ok(WorkspaceCommand::CreateContainedRoot {
                    surface: SurfaceId::new(2), root: C, floating,
                    rect: LogicalRect::new(0.0, 0.0, if invalid_rect { 0.0 } else { 320.0 }, 240.0).unwrap(),
                    position: ContainedPosition::Front, content: RootContent::OpenItem(ItemId::new(4)),
                })
            })?;
            Ok(())
        });
        assert_eq!(structural_work::snapshot().canonical_full_passes, 1);
        if invalid_rect {
            assert!(
                matches!(result, Err(TransactionError::Canonicalization(CanonicalizationError::InvalidOutput(errors)))
                if errors.errors().iter().any(|error| matches!(error,
                    WorkspaceValidationError::NonPositiveContainedRect { floating: actual, .. } if *actual == floating)))
            );
            assert_eq!(workspace, original);
            workspace
                .verify_reference(A, split, old.reference(), ReferenceRole::Source)
                .unwrap();
        } else {
            assert!(result.unwrap().changed());
            assert_eq!(workspace.root(A).unwrap().node, peer);
            assert!(workspace.node(source).is_none());
            assert!(workspace.node(split).is_none());
            assert!(workspace.placement(source).is_none());
            assert!(workspace.placement(split).is_none());
            workspace.validate().unwrap();
            assert_actual_placement(&workspace);
        }
    }
}

#[test]
fn same_version_index_rejects_changed_and_foreign_issuers_and_retains_unrelated_roots() {
    let (mut workspace, tabs) = fixture();
    let version = WorkspaceVersion::initial();
    let index = WorkspaceIndex::build(&workspace, version).unwrap();
    let source = workspace
        .capture_item_source(A, tabs[0], ItemId::new(2))
        .unwrap();
    WorkspaceTransaction::from_commands([WorkspaceCommand::Select { source }])
        .apply(&mut workspace, &default_policy())
        .unwrap();
    let fraction = DockFraction::new(0.5).unwrap();
    for result in [
        index
            .capture_item_source(&workspace, version, A, tabs[0], ItemId::new(2))
            .map(|_| ()),
        index
            .capture_tab_target(&workspace, version, A, tabs[0])
            .map(|_| ()),
        index
            .capture_inner_edge_target(&workspace, version, A, tabs[0], Edge::Left, fraction)
            .map(|_| ()),
        index
            .capture_outer_edge_target(&workspace, version, A, Edge::Left, fraction)
            .map(|_| ()),
    ] {
        assert!(matches!(result, Err(CommandError::StaleNode { .. })));
    }
    index
        .capture_tab_target(&workspace, version, C, tabs[3])
        .unwrap();
    let rebound = index
        .after_selection(&workspace, version, A, tabs[0])
        .unwrap();
    rebound
        .capture_tab_target(&workspace, version, A, tabs[0])
        .unwrap();
    let foreign = workspace.clone().into_builder().build().unwrap();
    assert!(matches!(
        rebound.capture_tab_target(&foreign, version, A, tabs[0]),
        Err(CommandError::StaleNode { .. })
    ));
    assert!(matches!(
        rebound.after_selection(&foreign, version, A, tabs[0]),
        Err(CommandError::StaleNode { .. })
    ));

    let source = workspace
        .capture_item_source(A, tabs[0], ItemId::new(2))
        .unwrap();
    WorkspaceTransaction::from_commands([WorkspaceCommand::Reorder {
        source,
        insertion_index: 0,
    }])
    .apply(&mut workspace, &default_policy())
    .unwrap();
    let source = workspace
        .capture_item_source(A, tabs[0], ItemId::new(1))
        .unwrap();
    WorkspaceTransaction::from_commands([WorkspaceCommand::Select { source }])
        .apply(&mut workspace, &default_policy())
        .unwrap();
    assert!(matches!(
        rebound.after_selection(&workspace, version, A, tabs[0]),
        Err(CommandError::StaleNode { .. })
    ));
}

#[test]
fn common_capture_select_and_item_move_build_no_full_root_proofs() {
    let (mut workspace, tabs) = fixture();
    structural_work::reset();
    let source = workspace
        .capture_item_source(A, tabs[0], ItemId::new(2))
        .unwrap();
    WorkspaceTransaction::from_commands([WorkspaceCommand::Select { source }])
        .apply(&mut workspace, &default_policy())
        .unwrap();
    let movement = WorkspaceCommand::Move {
        payload: MovePayload::Item(
            workspace
                .capture_item_source(A, tabs[0], ItemId::new(2))
                .unwrap(),
        ),
        target: DockTarget::InnerEdge(
            workspace
                .capture_inner_edge_target(B, tabs[2], Edge::Top, DockFraction::new(0.5).unwrap())
                .unwrap(),
        ),
    };
    WorkspaceTransaction::from_commands([movement])
        .apply(&mut workspace, &default_policy())
        .unwrap();
    let work = structural_work::snapshot();
    assert_eq!(work.root_fingerprint_builds, 0);
    assert_eq!(work.root_fingerprint_node_visits, 0);
    assert_eq!(work.canonical_full_passes, 0);
    assert_eq!(work.workspace_full_validations, 0);
    assert_actual_placement(&workspace);
}

#[test]
fn cross_root_subtree_and_staged_merged_residue_have_actual_placement() {
    let (mut workspace, tabs) = fixture();
    let unaffected = workspace.capture_node_source(C, tabs[3]).unwrap();
    let moved_split = workspace.root(A).unwrap().node;
    let movement = WorkspaceCommand::Move {
        payload: MovePayload::Subtree(workspace.capture_node_source(A, moved_split).unwrap()),
        target: DockTarget::InnerEdge(
            workspace
                .capture_inner_edge_target(B, tabs[2], Edge::Top, DockFraction::new(0.5).unwrap())
                .unwrap(),
        ),
    };
    WorkspaceTransaction::from_commands([movement])
        .apply(&mut workspace, &default_policy())
        .unwrap();
    assert!(workspace.root(A).is_none());
    assert_eq!(workspace.node_root(moved_split), Some(B));
    assert_eq!(workspace.node_root(tabs[0]), Some(B));
    workspace
        .verify_reference(C, tabs[3], unaffected.reference(), ReferenceRole::Source)
        .unwrap();
    assert_actual_placement(&workspace);

    let original = workspace.clone();
    let policy = default_policy();
    let mut stage = TransactionDraft::new(
        &workspace,
        TransactionAuthority::Policy(&policy),
        workspace.clone(),
    );
    let movement = WorkspaceCommand::Move {
        payload: MovePayload::Tabs(workspace.capture_node_source(B, tabs[0]).unwrap()),
        target: DockTarget::Center(workspace.capture_tab_target(C, tabs[3]).unwrap()),
    };
    stage.push_command(&movement).unwrap();
    assert!(stage.candidate.node(tabs[0]).is_some());
    assert_eq!(stage.candidate.node_root(tabs[0]), None);
    for root in [B, C] {
        assert!(matches!(
            stage.candidate.capture_node_source(root, tabs[0]),
            Err(CommandError::NodeOutsideRoot { .. })
        ));
    }
    assert_actual_placement(&stage.candidate);
    let prepared = stage.finish(&original).unwrap();
    assert!(prepared.candidate().node(tabs[0]).is_none());
    assert_actual_placement(prepared.candidate());
}

#[test]
fn recycled_primary_node_generation_never_inherits_old_membership() {
    let (workspace, tabs) = fixture();
    let version = WorkspaceVersion::initial();
    let index = WorkspaceIndex::build(&workspace, version).unwrap();
    let original = workspace.capture_node_source(A, tabs[0]).unwrap();
    let mut builder = workspace.into_builder();
    let removed = builder.workspace.remove_runtime_node(tabs[0]).unwrap();
    let replacement = builder.insert_node((*removed).clone());
    assert_ne!(replacement, tabs[0]);
    let root_node = builder.draft().root(A).unwrap().node;
    let Node::Split { children, .. } = builder.draft_mut().nodes.get_mut(root_node).unwrap() else {
        unreachable!()
    };
    children[0] = replacement;
    let replacement_workspace = builder.build().unwrap();
    assert_eq!(replacement_workspace.node_root(tabs[0]), None);
    assert_eq!(replacement_workspace.node_root(replacement), Some(A));
    assert!(
        matches!(replacement_workspace.verify_reference(A, tabs[0], original.reference(), ReferenceRole::Source),
        Err(CommandError::MissingNode { node, .. }) if node == tabs[0])
    );
    assert!(matches!(
        index.capture_tab_target(&replacement_workspace, version, A, replacement),
        Err(CommandError::StaleNode { .. })
    ));
    replacement_workspace
        .capture_node_source(A, replacement)
        .unwrap();
    assert_actual_placement(&replacement_workspace);
}

#[test]
fn retained_selection_rebind_rejects_an_unrelated_intervening_root_edit() {
    let (mut workspace, tabs) = fixture();
    let version = WorkspaceVersion::initial();
    let index = WorkspaceIndex::build(&workspace, version).unwrap();
    for (root, tabs, item) in [(C, tabs[3], 8), (A, tabs[0], 2)] {
        let source = workspace
            .capture_item_source(root, tabs, ItemId::new(item))
            .unwrap();
        WorkspaceTransaction::from_commands([WorkspaceCommand::Select { source }])
            .apply(&mut workspace, &default_policy())
            .unwrap();
    }
    assert!(matches!(
        index.after_selection(&workspace, version, A, tabs[0]),
        Err(CommandError::StaleNode { .. })
    ));
}

#[test]
fn selection_and_resize_change_exact_refs_at_distinct_geometry_boundaries() {
    let (mut workspace, tabs) = fixture();
    let split = workspace.root(A).unwrap().node;
    let before = workspace.root_fingerprint(A).unwrap();
    let old = workspace.capture_node_source(A, split).unwrap();
    let unaffected = workspace.capture_node_source(C, tabs[3]).unwrap();
    let source = workspace
        .capture_item_source(A, tabs[0], ItemId::new(2))
        .unwrap();
    WorkspaceTransaction::from_commands([WorkspaceCommand::Select { source }])
        .apply(&mut workspace, &default_policy())
        .unwrap();
    let selected = workspace.root_fingerprint(A).unwrap();
    assert_ne!(before, selected);
    assert!(before.presentation_structure_eq(&selected));
    assert!(before.presentation_topology_eq(&selected));
    assert!(matches!(
        workspace.verify_reference(A, split, old.reference(), ReferenceRole::Source),
        Err(CommandError::StaleNode { .. })
    ));

    let source = workspace.capture_node_source(A, split).unwrap();
    let resize = SplitResize::new(source.clone(), SplitWeight::normalize([0.3, 0.7]).unwrap());
    let command = WorkspaceCommand::ResizeSplits {
        splits: vec![resize],
    };
    WorkspaceTransaction::from_commands([command.clone()])
        .apply(&mut workspace, &default_policy())
        .unwrap();
    let resized = workspace.root_fingerprint(A).unwrap();
    assert!(!selected.presentation_structure_eq(&resized));
    assert!(selected.presentation_topology_eq(&resized));
    assert!(matches!(
        workspace.verify_reference(A, split, source.reference(), ReferenceRole::Source),
        Err(CommandError::StaleNode { .. })
    ));
    let current = workspace.capture_node_source(A, split).unwrap();
    let noop = SplitResize::new(current.clone(), SplitWeight::normalize([0.3, 0.7]).unwrap());
    let report = WorkspaceTransaction::from_commands([WorkspaceCommand::ResizeSplits {
        splits: vec![noop],
    }])
    .apply(&mut workspace, &default_policy())
    .unwrap();
    assert!(!report.changed());
    workspace
        .verify_reference(A, split, current.reference(), ReferenceRole::Source)
        .unwrap();
    workspace
        .verify_reference(C, tabs[3], unaffected.reference(), ReferenceRole::Source)
        .unwrap();
    assert_actual_placement(&workspace);
}

#[test]
fn a_new_root_and_its_donor_issue_actual_refs_before_the_next_prefix() {
    let (mut workspace, tabs) = fixture();
    let old_donor = workspace
        .capture_item_source(A, tabs[0], ItemId::new(1))
        .unwrap();
    let unrelated = workspace.capture_node_source(C, tabs[3]).unwrap();
    let new_root = RootId::new(4);
    let report = WorkspaceTransaction::apply_with(&mut workspace, &default_policy(), |stage| {
        stage.push(|current| {
            Ok(WorkspaceCommand::CreateContainedRoot {
                surface: SurfaceId::new(2),
                root: new_root,
                floating: FloatingPresentationId::new(4),
                rect: rect(0.0),
                position: ContainedPosition::Front,
                content: RootContent::Move(MovePayload::Item(current.capture_item_source(
                    A,
                    tabs[0],
                    ItemId::new(2),
                )?)),
            })
        })?;
        stage.push(|current| {
            let new_tabs = current.root(new_root).unwrap().node;
            Ok(WorkspaceCommand::Select {
                source: current.capture_item_source(new_root, new_tabs, ItemId::new(2))?,
            })
        })?;
        stage.push(|current| {
            let source = current.capture_item_source(A, tabs[0], ItemId::new(1))?;
            assert_ne!(source.reference(), old_donor.reference());
            Ok(WorkspaceCommand::Select { source })
        })?;
        Ok(())
    })
    .unwrap();
    assert!(report.changed());
    assert!(matches!(
        report.outcomes(),
        [
            CommandOutcome::ContainedRootCreated { .. },
            CommandOutcome::Selected { changed: false, .. },
            CommandOutcome::Selected { changed: false, .. }
        ]
    ));
    workspace
        .verify_reference(C, tabs[3], unrelated.reference(), ReferenceRole::Source)
        .unwrap();
    assert_actual_placement(&workspace);
}
