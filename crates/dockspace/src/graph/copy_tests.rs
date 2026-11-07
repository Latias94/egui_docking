//! Candidate isolation and actual payload-copy work through the normal owner.

use super::*;
use crate::command::{DockFraction, DockTarget, Edge, MovePayload, WorkspaceCommand};
use crate::drop_resolver::structural_work;
use crate::error::{CommandError, ReferenceRole};
use crate::policy::DockPolicySnapshot;
use crate::transaction::WorkspaceTransaction;

const A: RootId = RootId::new(1);
const B: RootId = RootId::new(2);
const C: RootId = RootId::new(3);

fn fixture() -> (Workspace, [NodeId; 4]) {
    let mut builder = Workspace::builder();
    let tabs = [[1, 2], [5, 6], [3, 4], [7, 8]]
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

fn movement(workspace: &Workspace, tabs: [NodeId; 4]) -> WorkspaceCommand {
    WorkspaceCommand::Move {
        payload: MovePayload::Item(
            workspace
                .capture_item_source(A, tabs[0], ItemId::new(2))
                .unwrap(),
        ),
        target: DockTarget::Center(workspace.capture_tab_target(B, tabs[2]).unwrap()),
    }
}

#[test]
fn semantic_edits_share_root_records_and_discarded_root_wrapping_is_isolated() {
    let (mut workspace, tabs) = fixture();
    let original = workspace.clone();
    let records = Arc::clone(&original.storage.roots);
    let old_target = original.capture_node_source(B, tabs[2]).unwrap();
    let policy = DockPolicySnapshot::default();
    let apply_shared = |workspace: &mut Workspace, command| {
        let report = WorkspaceTransaction::from_commands([command])
            .apply(workspace, &policy)
            .unwrap();
        assert!(report.changed());
        assert!(Arc::ptr_eq(&records, &workspace.storage.roots));
    };
    let command = WorkspaceCommand::Select {
        source: workspace
            .capture_item_source(A, tabs[0], ItemId::new(2))
            .unwrap(),
    };
    apply_shared(&mut workspace, command);
    let command = movement(&workspace, tabs);
    apply_shared(&mut workspace, command);

    let mut builder = workspace.clone().into_builder();
    let record = *workspace.root(A).unwrap();
    assert_eq!(builder.set_root(A, record), Some(record));
    assert!(Arc::ptr_eq(&records, &builder.workspace.roots));
    assert!(!builder.canonicalize().unwrap().changed);
    assert!(Arc::ptr_eq(&records, &builder.workspace.roots));

    let before = workspace.clone();
    let command = WorkspaceCommand::Move {
        payload: MovePayload::Item(
            workspace
                .capture_item_source(C, tabs[3], ItemId::new(7))
                .unwrap(),
        ),
        target: DockTarget::OuterEdge(
            workspace
                .capture_outer_edge_target(B, Edge::Top, DockFraction::new(0.5).unwrap())
                .unwrap(),
        ),
    };
    let prepared = crate::operation::prepare_transaction(&workspace, &policy, &[command]).unwrap();
    assert!(!Arc::ptr_eq(&records, &prepared.candidate().storage.roots));
    assert_ne!(prepared.candidate().root(B).unwrap().node, tabs[2]);
    assert_eq!(workspace.root(B).unwrap().node, tabs[2]);
    original
        .verify_reference(B, tabs[2], old_target.reference(), ReferenceRole::Source)
        .unwrap();
    assert!(Arc::ptr_eq(&records, &workspace.storage.roots));
    drop(prepared);
    assert_eq!(workspace, before);
    assert!(Arc::ptr_eq(&records, &original.storage.roots));
}

#[test]
fn a_workspace_fork_copies_all_live_handles_without_copying_payloads() {
    let (workspace, tabs) = fixture();
    structural_work::reset();
    let fork = workspace.clone();
    let copies = structural_work::snapshot().workspace_storage;
    assert_eq!(copies.node_store_forks, 1);
    assert_eq!(copies.node_handle_clones, workspace.nodes().count());
    assert_eq!(copies.node_payload_clones, 0);
    assert_eq!(copies.mru_item_copies, 0);
    for (node, value) in workspace.nodes() {
        assert!(std::ptr::eq(value, fork.node(node).unwrap()));
    }
    for tabs in tabs {
        assert!(std::ptr::eq(
            workspace.tab_mru(tabs).unwrap(),
            fork.tab_mru(tabs).unwrap()
        ));
    }
    assert_eq!(workspace, fork);
}

#[test]
fn center_move_copies_only_two_complete_tabs_payloads_and_unique_followup_copies_none() {
    let (workspace, tabs) = fixture();
    let old = workspace
        .capture_item_source(A, tabs[0], ItemId::new(2))
        .unwrap();
    let unaffected = workspace.capture_node_source(C, tabs[3]).unwrap();
    let mut candidate = workspace.clone();
    let policy = DockPolicySnapshot::default();
    let command = movement(&candidate, tabs);
    structural_work::reset();
    let report = WorkspaceTransaction::from_commands([command])
        .apply(&mut candidate, &policy)
        .unwrap();
    let work = structural_work::snapshot();
    let copies = work.workspace_storage;
    assert!(report.changed());
    assert_eq!(
        work.workspace_deep_clones
            .transaction_candidates
            .volume
            .nodes,
        workspace.nodes().count()
    );
    assert_eq!(copies.node_store_forks, 1);
    assert_eq!(copies.node_handle_clones, workspace.nodes().count());
    assert_eq!(copies.node_payload_clones, 2);
    assert_eq!(copies.tab_item_copies, 4);
    assert_eq!(copies.split_child_copies, 0);
    assert_eq!(copies.split_weight_copies, 0);
    assert_eq!(copies.mru_item_copies, 4);
    for untouched in [tabs[1], tabs[3], workspace.root(A).unwrap().node] {
        assert!(std::ptr::eq(
            workspace.node(untouched).unwrap(),
            candidate.node(untouched).unwrap()
        ));
    }
    assert_eq!(
        workspace.tab_mru(tabs[0]).unwrap(),
        [ItemId::new(1), ItemId::new(2)]
    );
    assert_eq!(candidate.tab_mru(tabs[0]).unwrap(), [ItemId::new(1)]);
    assert_eq!(
        candidate.tab_mru(tabs[2]).unwrap(),
        [ItemId::new(2), ItemId::new(3), ItemId::new(4)]
    );
    workspace
        .verify_reference(A, tabs[0], old.reference(), ReferenceRole::Source)
        .unwrap();
    assert!(matches!(
        candidate.verify_reference(A, tabs[0], old.reference(), ReferenceRole::Source),
        Err(CommandError::StaleNode { .. })
    ));
    candidate
        .verify_reference(C, tabs[3], unaffected.reference(), ReferenceRole::Source)
        .unwrap();

    let source = candidate
        .capture_item_source(B, tabs[2], ItemId::new(3))
        .unwrap();
    structural_work::reset();
    crate::operation::apply_owned_draft_command(
        &mut candidate,
        &policy,
        &WorkspaceCommand::Select { source },
    )
    .unwrap();
    let copies = structural_work::snapshot().workspace_storage;
    assert_eq!(copies.node_store_forks, 0);
    assert_eq!(copies.node_payload_clones, 0);
    assert_eq!(copies.mru_item_copies, 0);
    assert_eq!(
        workspace.tab_mru(tabs[2]).unwrap(),
        [ItemId::new(3), ItemId::new(4)]
    );
    assert_eq!(candidate.tab_mru(tabs[2]).unwrap()[0], ItemId::new(3));
    candidate.validate().unwrap();
    workspace.validate().unwrap();
}

#[test]
fn raw_builder_writes_copy_shared_values_and_leave_the_issued_workspace_untouched() {
    let (workspace, tabs) = fixture();
    let old = workspace.capture_node_source(A, tabs[0]).unwrap();
    let mut builder = workspace.clone().into_builder();
    structural_work::reset();
    let Node::Tabs { selected, mru, .. } = builder.draft_mut().nodes.get_mut(tabs[0]).unwrap()
    else {
        unreachable!()
    };
    *selected = Some(ItemId::new(2));
    mru.swap(0, 1);
    let changed = builder.build().unwrap();
    let copies = structural_work::snapshot().workspace_storage;
    assert_eq!(copies.node_payload_clones, 1);
    assert_eq!(copies.mru_item_copies, 2);
    assert_eq!(
        workspace.tab_mru(tabs[0]).unwrap(),
        [ItemId::new(1), ItemId::new(2)]
    );
    assert_eq!(
        changed.tab_mru(tabs[0]).unwrap(),
        [ItemId::new(2), ItemId::new(1)]
    );
    assert!(std::ptr::eq(
        workspace.node(tabs[3]).unwrap(),
        changed.node(tabs[3]).unwrap()
    ));
    workspace
        .verify_reference(A, tabs[0], old.reference(), ReferenceRole::Source)
        .unwrap();
    assert!(matches!(
        changed.verify_reference(A, tabs[0], old.reference(), ReferenceRole::Source),
        Err(CommandError::StaleNode { .. })
    ));
}

#[test]
fn canonical_noop_and_retirement_filters_do_not_detach_retained_payloads() {
    let (workspace, tabs) = fixture();
    let mut candidate = workspace.clone();
    structural_work::reset();
    assert!(
        !crate::canonical::canonicalize_workspace(&mut candidate)
            .unwrap()
            .changed
    );
    let copies = structural_work::snapshot().workspace_storage;
    assert_eq!(copies.node_payload_clones, 0);
    assert_eq!(copies.mru_item_copies, 0);
    for (node, value) in workspace.nodes() {
        assert!(std::ptr::eq(value, candidate.node(node).unwrap()));
    }

    let mut raw = candidate.into_builder();
    structural_work::reset();
    raw.draft_mut().nodes.retain(|_, _| true);
    drop(raw.draft_mut().remove_runtime_node(tabs[0]).unwrap());
    let copies = structural_work::snapshot().workspace_storage;
    assert_eq!(copies.node_payload_clones, 0);
    assert_eq!(copies.mru_item_copies, 0);
    assert!(workspace.node(tabs[0]).is_some());
    workspace.validate().unwrap();
}

#[test]
fn replacing_a_shared_builder_node_preserves_its_owned_return_and_other_payloads() {
    let (workspace, tabs) = fixture();
    let mut builder = workspace.clone().into_builder();
    structural_work::reset();
    let previous = builder
        .replace_node(tabs[0], Node::tabs([ItemId::new(9), ItemId::new(10)]))
        .unwrap();
    let copies = structural_work::snapshot().workspace_storage;
    assert_eq!(copies.node_payload_clones, 1);
    assert_eq!(copies.mru_item_copies, 2);
    assert_eq!(&previous, workspace.node(tabs[0]).unwrap());
    let changed = builder.build().unwrap();
    assert!(std::ptr::eq(
        workspace.node(tabs[3]).unwrap(),
        changed.node(tabs[3]).unwrap()
    ));
    assert_eq!(
        workspace.tab_mru(tabs[0]).unwrap(),
        [ItemId::new(1), ItemId::new(2)]
    );
    assert_eq!(
        changed.tab_mru(tabs[0]).unwrap(),
        [ItemId::new(9), ItemId::new(10)]
    );
}

#[test]
fn a_complete_tabs_replacement_keeps_explicit_history_and_returns_the_previous_history() {
    let mut builder = Workspace::builder();
    let tabs = builder.insert_node(Node::Tabs {
        items: vec![ItemId::new(1), ItemId::new(2), ItemId::new(3)],
        selected: Some(ItemId::new(2)),
        mru: vec![ItemId::new(2), ItemId::new(3), ItemId::new(1)],
    });
    builder.set_root(A, RootRecord::new(tabs));
    builder.set_surface(SurfaceId::new(1), SurfacePresentation::with_main(A));
    let original = builder.build().unwrap();
    let mut draft = original.clone().into_builder();
    let previous = draft
        .replace_node(
            tabs,
            Node::Tabs {
                items: vec![ItemId::new(4), ItemId::new(5), ItemId::new(6)],
                selected: Some(ItemId::new(5)),
                mru: vec![ItemId::new(5), ItemId::new(6), ItemId::new(4)],
            },
        )
        .unwrap();
    assert_eq!(&previous, original.node(tabs).unwrap());
    assert_eq!(
        draft.draft().tab_mru(tabs),
        Some([ItemId::new(5), ItemId::new(6), ItemId::new(4)].as_slice())
    );
    draft
        .set_tab_mru(tabs, [ItemId::new(5), ItemId::new(4), ItemId::new(6)])
        .unwrap();
    let changed = draft.build().unwrap();
    assert_eq!(
        changed.tab_mru(tabs),
        Some([ItemId::new(5), ItemId::new(4), ItemId::new(6)].as_slice())
    );
    assert_eq!(
        original.tab_mru(tabs),
        Some([ItemId::new(2), ItemId::new(3), ItemId::new(1)].as_slice())
    );
}

#[test]
fn visual_reorder_copies_owned_history_without_changing_its_order() {
    let (mut workspace, tabs) = fixture();
    let original = workspace.clone();
    let source = workspace
        .capture_item_source(A, tabs[0], ItemId::new(2))
        .unwrap();
    structural_work::reset();
    let report = WorkspaceTransaction::from_commands([WorkspaceCommand::Reorder {
        source,
        insertion_index: 0,
    }])
    .apply(&mut workspace, &DockPolicySnapshot::default())
    .unwrap();
    assert!(report.changed());
    let copies = structural_work::snapshot().workspace_storage;
    assert_eq!(copies.node_payload_clones, 1);
    assert_eq!(copies.tab_item_copies, 2);
    assert_eq!(copies.mru_item_copies, 2);
    assert_eq!(workspace.tab_mru(tabs[0]), original.tab_mru(tabs[0]));
    assert!(
        matches!(workspace.node(tabs[0]), Some(Node::Tabs { items, .. }) if items == &[ItemId::new(2), ItemId::new(1)])
    );
    original.validate().unwrap();
    workspace.validate().unwrap();
}

#[test]
fn a_late_batch_failure_discards_detached_payloads_and_preserves_original_refs() {
    let (mut workspace, tabs) = fixture();
    let original = workspace.clone();
    let old = workspace.capture_node_source(A, tabs[0]).unwrap();
    let first = movement(&workspace, tabs);
    let duplicate = WorkspaceCommand::Open {
        item: ItemId::new(2),
        target: DockTarget::Center(workspace.capture_tab_target(C, tabs[3]).unwrap()),
    };
    assert_eq!(
        WorkspaceTransaction::from_commands([first, duplicate])
            .apply(&mut workspace, &DockPolicySnapshot::default()),
        Err(crate::error::TransactionError::Command {
            index: 1,
            source: CommandError::ItemAlreadyOpen {
                item: ItemId::new(2),
            },
        })
    );
    assert_eq!(workspace, original);
    for (node, value) in original.nodes() {
        assert!(std::ptr::eq(value, workspace.node(node).unwrap()));
    }
    workspace
        .verify_reference(A, tabs[0], old.reference(), ReferenceRole::Source)
        .unwrap();
    WorkspaceTransaction::from_commands([movement(&workspace, tabs)])
        .apply(&mut workspace, &DockPolicySnapshot::default())
        .unwrap();
    assert_ne!(workspace, original);
    original.validate().unwrap();
    workspace.validate().unwrap();
}

#[test]
fn branch_slot_reuse_keeps_the_original_generation_and_candidate_issuance_separate() {
    let (workspace, tabs) = fixture();
    let old = workspace.capture_node_source(A, tabs[0]).unwrap();
    let mut builder = workspace.clone().into_builder();
    let removed = builder.draft_mut().remove_runtime_node(tabs[0]).unwrap();
    let replacement = builder.insert_node((*removed).clone());
    assert_ne!(replacement, tabs[0]);
    let split = builder.draft().root(A).unwrap().node;
    let Node::Split { children, .. } = builder.draft_mut().nodes.get_mut(split).unwrap() else {
        unreachable!()
    };
    children[0] = replacement;
    let candidate = builder.build().unwrap();
    assert!(workspace.node(tabs[0]).is_some());
    assert!(workspace.node(replacement).is_none());
    assert!(candidate.node(tabs[0]).is_none());
    assert!(candidate.node(replacement).is_some());
    workspace
        .verify_reference(A, tabs[0], old.reference(), ReferenceRole::Source)
        .unwrap();
    assert!(matches!(
        candidate.capture_node_source(A, tabs[0]),
        Err(CommandError::MissingNode { .. })
    ));
    candidate.capture_node_source(A, replacement).unwrap();
    assert!(std::ptr::eq(
        workspace.node(tabs[3]).unwrap(),
        candidate.node(tabs[3]).unwrap()
    ));
    workspace.validate().unwrap();
    candidate.validate().unwrap();
}
