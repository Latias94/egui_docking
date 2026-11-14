use super::*;

use std::collections::BTreeMap;

use crate::close_plan::DeferredCloseToken;
use crate::graph::Workspace;

const ALL_ITEMS: [ItemId; 4] = [FIRST, SECOND, THIRD, FOURTH];

fn assert_items(workspace: &Workspace, expected: &[ItemId]) {
    workspace.validate().unwrap();
    let counts: BTreeMap<_, _> = expected.iter().map(|item| (*item, 1)).collect();
    assert_eq!(workspace.item_multiset(), counts);
}

fn defer_first(session: &mut DockspaceSession) -> (DockspaceClosePlan, DeferredCloseToken) {
    assert_items(session.workspace(), &ALL_ITEMS);
    let version = session.version();
    let prepared = session.prepare_close_item(FIRST);
    let mut frame = session.begin_host_frame().unwrap();
    let requested = frame.submit_prepared_close_request(prepared).unwrap();
    let report = commit(frame);
    let plan = requested_plan(&report, requested);
    assert_eq!(plan.target(), ClosePlanTarget::Item { item: FIRST });
    assert_eq!(plan.items().len(), 1);
    assert_eq!(plan.items()[0].item(), FIRST);
    assert_eq!(session.version(), version);
    assert_items(session.workspace(), &ALL_ITEMS);

    let mut frame = session.begin_host_frame().unwrap();
    let deferred = frame
        .resolve_close(
            plan.request(),
            plan.items()[0].token(),
            CloseDecision::Deferred,
        )
        .unwrap();
    let report = commit(frame);
    let continuation = match report.submitted_action_outcome(deferred) {
        Some(HostInputOutcome::CloseDecisionProcessed {
            resolution:
                DockspaceCloseResolution::Deferred {
                    request,
                    item: FIRST,
                    continuation,
                },
            application: None,
            changed: false,
            ..
        }) if *request == plan.request() => *continuation,
        outcome => panic!("expected the exact deferred close decision, got {outcome:?}"),
    };
    assert!(report.submitted_action_outcome(requested).is_none());
    let pending: Vec<_> = session.pending_content_close_plans().collect();
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].request(), plan.request());
    assert_eq!(pending[0].items()[0].deferred_token(), Some(continuation));
    assert_eq!(session.version(), version);
    assert_items(session.workspace(), &ALL_ITEMS);
    (plan, continuation)
}

fn assert_first_closed(
    session: &DockspaceSession,
    report: &HostFrameReport,
    action: DockspaceSubmittedAction,
    plan: &DockspaceClosePlan,
) {
    assert!(matches!(
        report.submitted_action_outcome(action),
        Some(HostInputOutcome::CloseDecisionProcessed {
            resolution: DockspaceCloseResolution::Approved { request },
            application: Some(Ok(DockspaceCloseOutcome::ItemClosed {
                item: FIRST,
                root: ROOT,
            })),
            changed: true,
            ..
        }) if *request == plan.request()
    ));
    assert_eq!(session.pending_content_close_plans().count(), 0);
    assert_items(session.workspace(), &[SECOND, THIRD, FOURTH]);
}

fn reopen_first(session: &mut DockspaceSession) {
    let mut frame = session.begin_host_frame().unwrap();
    let opened = frame
        .open_item_current(FIRST, DockPlacement::Center(DockAnchor::Item(SECOND)))
        .unwrap();
    assert_items(frame.frame.view().workspace(), &ALL_ITEMS);
    let report = commit(frame);
    assert_eq!(
        report.submitted_action_outcome(opened),
        Some(&HostInputOutcome::ProductActionApplied(
            DockspaceActionOutcome::Opened {
                item: FIRST,
                root: ROOT,
            },
        ))
    );
    assert_items(session.workspace(), &ALL_ITEMS);
}

#[test]
fn dropping_a_cross_surface_move_preserves_the_deferred_close_scope() {
    let mut session = session();
    let (plan, continuation) = defer_first(&mut session);
    let workspace = session.workspace().clone();
    let version = session.version();
    let pending: Vec<_> = session.pending_content_close_plans().collect();

    let mut frame = session.begin_host_frame().unwrap();
    let abandoned = frame
        .dock_item_current(FIRST, DockPlacement::Center(DockAnchor::Item(THIRD)))
        .unwrap();
    assert_eq!(frame.view().item(FIRST).unwrap().surface(), OTHER_SURFACE);
    assert_eq!(frame.view().item(FIRST).unwrap().root(), OTHER_ROOT);
    assert_ne!(frame.version(), version);
    assert_items(frame.frame.view().workspace(), &ALL_ITEMS);
    drop(frame);

    assert_eq!(session.workspace(), &workspace);
    assert_eq!(session.version(), version);
    assert_eq!(
        session.pending_content_close_plans().collect::<Vec<_>>(),
        pending
    );
    assert_items(session.workspace(), &ALL_ITEMS);

    let mut frame = session.begin_host_frame().unwrap();
    let allowed = frame
        .continue_deferred_close(plan.request(), continuation, DeferredCloseDecision::Allow)
        .unwrap();
    assert_items(frame.frame.view().workspace(), &[SECOND, THIRD, FOURTH]);
    let report = commit(frame);
    assert_ne!(allowed, abandoned);
    assert!(report.submitted_action_outcome(abandoned).is_none());
    assert_first_closed(&session, &report, allowed, &plan);
    reopen_first(&mut session);
}

#[test]
fn committed_cross_surface_moves_permanently_revoke_old_deferred_close_tokens() {
    let mut session = session();
    let (old_plan, continuation) = defer_first(&mut session);

    // Returning to the original tabs must not revive the earlier authorization.
    for (target, source_root, target_root, target_surface) in [
        (THIRD, ROOT, OTHER_ROOT, OTHER_SURFACE),
        (SECOND, OTHER_ROOT, ROOT, SURFACE),
    ] {
        let mut frame = session.begin_host_frame().unwrap();
        let moved = frame
            .dock_item_current(FIRST, DockPlacement::Center(DockAnchor::Item(target)))
            .unwrap();
        assert_eq!(frame.view().item(FIRST).unwrap().surface(), target_surface);
        assert_eq!(frame.view().item(FIRST).unwrap().root(), target_root);
        assert_items(frame.frame.view().workspace(), &ALL_ITEMS);
        let report = commit(frame);
        assert_eq!(
            report.submitted_action_outcome(moved),
            Some(&HostInputOutcome::ProductActionApplied(
                DockspaceActionOutcome::Docked {
                    item: FIRST,
                    source_root,
                    target_root,
                    changed: true,
                },
            ))
        );
        assert_eq!(session.pending_content_close_plans().count(), 0);
        assert_items(session.workspace(), &ALL_ITEMS);

        let workspace = session.workspace().clone();
        let version = session.version();
        let mut frame = session.begin_host_frame().unwrap();
        let stale = frame
            .continue_deferred_close(
                old_plan.request(),
                continuation,
                DeferredCloseDecision::Allow,
            )
            .unwrap();
        assert_items(frame.frame.view().workspace(), &ALL_ITEMS);
        let report = commit(frame);
        assert!(matches!(
            report.submitted_action_outcome(stale),
            Some(HostInputOutcome::CloseDecisionProcessed {
                resolution: DockspaceCloseResolution::Inert(_),
                application: None,
                changed: false,
                ..
            })
        ));
        assert!(report.submitted_action_outcome(moved).is_none());
        assert_eq!(session.workspace(), &workspace);
        assert_eq!(session.version(), version);
        assert_items(session.workspace(), &ALL_ITEMS);
    }

    let prepared = session.prepare_close_item(FIRST);
    let mut frame = session.begin_host_frame().unwrap();
    let requested = frame.submit_prepared_close_request(prepared).unwrap();
    let report = commit(frame);
    let new_plan = requested_plan(&report, requested);
    assert_ne!(new_plan.request(), old_plan.request());
    assert_ne!(new_plan.items()[0].token(), old_plan.items()[0].token());
    assert_items(session.workspace(), &ALL_ITEMS);

    let mut frame = session.begin_host_frame().unwrap();
    let allowed = frame
        .resolve_close(
            new_plan.request(),
            new_plan.items()[0].token(),
            CloseDecision::Allow,
        )
        .unwrap();
    assert_items(frame.frame.view().workspace(), &[SECOND, THIRD, FOURTH]);
    let report = commit(frame);
    assert_first_closed(&session, &report, allowed, &new_plan);
    reopen_first(&mut session);
}
