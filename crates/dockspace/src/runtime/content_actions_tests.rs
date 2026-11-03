//! Core host-frame regressions for content actions and published close queries.

use super::*;
use crate::close_plan::{
    CloseDecision, CloseItemDecisionState, ClosePlanTarget, DeferredCloseDecision,
};
use crate::geometry::{LogicalRect, LogicalSize};
use crate::ids::{ItemId, WorkspaceRevision};
use crate::model::{DockspaceLayout, DockspaceNode, DockspaceRootLayout, DockspaceSurfaceLayout};
use crate::policy::{CloseCapability, DockPolicy};

const SURFACE: SurfaceId = SurfaceId::new(1);
const OTHER_SURFACE: SurfaceId = SurfaceId::new(2);
const ROOT: RootId = RootId::new(1);
const OTHER_ROOT: RootId = RootId::new(2);
const FIRST: ItemId = ItemId::new(1);
const SECOND: ItemId = ItemId::new(2);
const THIRD: ItemId = ItemId::new(3);
const FOURTH: ItemId = ItemId::new(4);

fn session() -> DockspaceSession {
    let layout = DockspaceLayout::new([
        DockspaceSurfaceLayout::new(
            SURFACE,
            DockspaceRootLayout::new(ROOT, DockspaceNode::tabs([FIRST, SECOND])),
        ),
        DockspaceSurfaceLayout::new(
            OTHER_SURFACE,
            DockspaceRootLayout::new(OTHER_ROOT, DockspaceNode::tabs([THIRD, FOURTH])),
        ),
    ])
    .unwrap();
    let mut policy = DockPolicy::default();
    policy.set_close_capability(CloseCapability::DeferredAllowed);
    DockspaceSession::from_layout(layout, policy).unwrap()
}

fn commit(mut frame: DockspaceHostFrame<'_>) -> HostFrameReport {
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    frame.commit().unwrap()
}

fn requested_plan(
    report: &HostFrameReport,
    action: DockspaceSubmittedAction,
) -> DockspaceClosePlan {
    match report.submitted_action_outcome(action) {
        Some(HostInputOutcome::CloseRequested {
            plan,
            origin: HostCloseRequestOrigin::Application,
            ..
        }) => plan.clone(),
        outcome => panic!("expected the exact application close request, got {outcome:?}"),
    }
}

#[test]
fn multiple_close_requests_correlate_new_reused_and_rejected_outcomes() {
    let mut session = session();
    let first = session.prepare_close_item(FIRST);
    let root = session.prepare_close_root(OTHER_ROOT);
    let reused = session.prepare_close_item(FIRST);
    let missing_item = ItemId::new(99);
    let rejected = session.prepare_close_item(missing_item);
    let mut frame = session.begin_host_frame().unwrap();
    let first = frame.submit_prepared_close_request(first).unwrap();
    let root = frame.submit_prepared_close_request(root).unwrap();
    let reused = frame.submit_prepared_close_request(reused).unwrap();
    let rejected = frame.submit_prepared_close_request(rejected).unwrap();
    let report = commit(frame);

    let first_plan = requested_plan(&report, first);
    let root_plan = requested_plan(&report, root);
    assert_eq!(first_plan.target(), ClosePlanTarget::Item { item: FIRST });
    assert_eq!(
        root_plan.target(),
        ClosePlanTarget::Root { root: OTHER_ROOT }
    );
    assert_ne!(first_plan.request(), root_plan.request());
    assert_ne!(first, reused);
    assert_eq!(requested_plan(&report, reused), first_plan);
    for action in [first, root] {
        assert!(matches!(
            report.submitted_action_outcome(action),
            Some(HostInputOutcome::CloseRequested { reused: false, .. })
        ));
    }
    assert!(matches!(
        report.submitted_action_outcome(reused),
        Some(HostInputOutcome::CloseRequested { reused: true, .. })
    ));
    assert_eq!(
        report.submitted_action_outcome(rejected),
        Some(&HostInputOutcome::CloseRejected(
            DockspaceCloseRequestRejection::ItemUnavailable { item: missing_item },
        ))
    );
    assert_eq!(
        session.pending_content_close_plans().collect::<Vec<_>>(),
        [first_plan, root_plan]
    );

    let next = commit(session.begin_host_frame().unwrap());
    for action in [first, root, reused, rejected] {
        assert!(next.submitted_action_outcome(action).is_none());
    }
    assert_eq!(session.pending_content_close_plans().count(), 2);
}

#[test]
fn close_submission_identity_does_not_alias_a_dropped_attempt_or_another_session() {
    let mut session = session();
    let prepared = session.prepare_close_item(FIRST);
    let mut dropped = session.begin_host_frame().unwrap();
    let abandoned = dropped.submit_prepared_close_request(prepared).unwrap();
    drop(dropped);
    assert_eq!(session.pending_content_close_plans().count(), 0);

    let prepared = session.prepare_close_item(FIRST);
    let mut frame = session.begin_host_frame().unwrap();
    let accepted = frame.submit_prepared_close_request(prepared).unwrap();
    let report = commit(frame);
    assert_ne!(accepted, abandoned);
    assert!(report.submitted_action_outcome(abandoned).is_none());
    requested_plan(&report, accepted);

    let mut foreign = self::session();
    let prepared = foreign.prepare_close_item(FIRST);
    let mut frame = foreign.begin_host_frame().unwrap();
    let foreign_action = frame.submit_prepared_close_request(prepared).unwrap();
    let foreign_report = commit(frame);
    assert!(report.submitted_action_outcome(foreign_action).is_none());
    assert!(foreign_report.submitted_action_outcome(accepted).is_none());
}

#[test]
fn multiple_deferred_decisions_and_continuations_keep_their_exact_outcomes() {
    let mut session = session();
    let prepared = session.prepare_close_root(ROOT);
    let mut frame = session.begin_host_frame().unwrap();
    let requested = frame.submit_prepared_close_request(prepared).unwrap();
    let report = commit(frame);
    let plan = requested_plan(&report, requested);
    let request = plan.request();
    let first = plan.items()[0];
    let second = plan.items()[1];

    let mut frame = session.begin_host_frame().unwrap();
    let first_deferred = frame
        .resolve_close(request, first.token(), CloseDecision::Deferred)
        .unwrap();
    let second_deferred = frame
        .resolve_close(request, second.token(), CloseDecision::Deferred)
        .unwrap();
    let report = commit(frame);
    assert_ne!(first_deferred, second_deferred);
    let continuations =
        [(first_deferred, FIRST), (second_deferred, SECOND)].map(|(action, item)| {
            match report.submitted_action_outcome(action) {
                Some(HostInputOutcome::CloseDecisionProcessed {
                    resolution:
                        DockspaceCloseResolution::Deferred {
                            request: actual,
                            item: actual_item,
                            continuation,
                        },
                    changed: false,
                    application: None,
                    ..
                }) if *actual == request && *actual_item == item => *continuation,
                outcome => panic!("expected the exact deferred decision, got {outcome:?}"),
            }
        });
    assert_ne!(continuations[0], continuations[1]);
    let pending = session.pending_content_close_plans().collect::<Vec<_>>();
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].request(), request);
    assert!(pending[0].phase().requires_decision());
    assert_eq!(
        pending[0].items()[0].deferred_token(),
        Some(continuations[0])
    );
    assert_eq!(
        pending[0].items()[1].deferred_token(),
        Some(continuations[1])
    );

    let mut frame = session.begin_host_frame().unwrap();
    let second_allowed = frame
        .continue_deferred_close(request, continuations[1], DeferredCloseDecision::Allow)
        .unwrap();
    let duplicate = frame
        .continue_deferred_close(request, continuations[1], DeferredCloseDecision::Allow)
        .unwrap();
    let approved = frame
        .continue_deferred_close(request, continuations[0], DeferredCloseDecision::Allow)
        .unwrap();
    let report = commit(frame);
    assert!(matches!(
        report.submitted_action_outcome(second_allowed),
        Some(HostInputOutcome::CloseDecisionProcessed {
            resolution: DockspaceCloseResolution::Recorded { request: actual, item: SECOND },
            application: None,
            changed: false,
            ..
        }) if *actual == request
    ));
    assert!(matches!(
        report.submitted_action_outcome(duplicate),
        Some(HostInputOutcome::CloseDecisionProcessed {
            resolution: DockspaceCloseResolution::Inert(DockspaceCloseInertReason::AlreadyResolved),
            application: None,
            changed: false,
            ..
        })
    ));
    assert!(matches!(
        report.submitted_action_outcome(approved),
        Some(HostInputOutcome::CloseDecisionProcessed {
            resolution: DockspaceCloseResolution::Approved { request: actual },
            application: Some(Ok(DockspaceCloseOutcome::RootClosed { root: ROOT, items })),
            changed: true,
            ..
        }) if *actual == request && items == &[FIRST, SECOND]
    ));
    assert!(report.submitted_action_outcome(first_deferred).is_none());
    assert_eq!(session.pending_content_close_plans().count(), 0);
    assert!(session.view().root(ROOT).is_none());
}

#[test]
fn pending_query_discovers_interaction_and_application_plans_and_drops_terminal_plans() {
    let mut session = session();
    let mut measured = session.begin_host_frame().unwrap();
    measured
        .measure_surface(
            SURFACE,
            UniformSurfaceMetrics::new(
                LogicalRect::new(0.0, 0.0, 800.0, 600.0).unwrap(),
                LogicalSize::new(64.0, 48.0).unwrap(),
                96.0,
            )
            .unwrap(),
        )
        .unwrap();
    commit(measured);

    let application = session.prepare_close_root(OTHER_ROOT);
    let mut frame = session.begin_host_frame().unwrap();
    let interaction = frame
        .paint_plan(SURFACE)
        .unwrap()
        .unwrap()
        .prepare_tab_close(FIRST)
        .unwrap();
    frame.submit_surface_action(interaction).unwrap();
    let application = frame.submit_prepared_close_request(application).unwrap();
    let report = commit(frame);
    let [
        HostInputOutcome::CloseRequested {
            plan: interaction,
            origin: HostCloseRequestOrigin::Interaction,
            ..
        },
        _,
    ] = report.inputs()
    else {
        panic!("the exact surface action must publish its interaction close plan")
    };
    let interaction = interaction.clone();
    let application = requested_plan(&report, application);
    drop(report);
    assert_eq!(
        session.pending_content_close_plans().collect::<Vec<_>>(),
        [interaction.clone(), application.clone()]
    );

    let mut frame = session.begin_host_frame().unwrap();
    let recorded = frame
        .resolve_close(
            application.request(),
            application.items()[0].token(),
            CloseDecision::Allow,
        )
        .unwrap();
    let vetoed = frame
        .resolve_close(
            application.request(),
            application.items()[1].token(),
            CloseDecision::Veto,
        )
        .unwrap();
    let report = commit(frame);
    assert!(matches!(
        report.submitted_action_outcome(recorded),
        Some(HostInputOutcome::CloseDecisionProcessed {
            resolution: DockspaceCloseResolution::Recorded { request, item: THIRD },
            application: None,
            changed: false,
            ..
        }) if *request == application.request()
    ));
    assert!(matches!(
        report.submitted_action_outcome(vetoed),
        Some(HostInputOutcome::CloseDecisionProcessed {
            resolution: DockspaceCloseResolution::Vetoed { request, item: FOURTH },
            plan: Some(plan),
            application: None,
            changed: false,
        }) if *request == application.request() && plan.phase() == DockspaceClosePhase::NotApplied
    ));
    assert_eq!(
        session.pending_content_close_plans().collect::<Vec<_>>(),
        [interaction.clone()]
    );

    let mut frame = session.begin_host_frame().unwrap();
    let approved = frame
        .resolve_close(
            interaction.request(),
            interaction.items()[0].token(),
            CloseDecision::Allow,
        )
        .unwrap();
    let report = commit(frame);
    assert!(matches!(
        report.submitted_action_outcome(approved),
        Some(HostInputOutcome::CloseDecisionProcessed {
            resolution: DockspaceCloseResolution::Approved { request },
            application: Some(Ok(DockspaceCloseOutcome::ItemClosed { item: FIRST, root: ROOT })),
            changed: true,
            ..
        }) if *request == interaction.request()
    ));
    assert_eq!(session.pending_content_close_plans().count(), 0);
    assert!(session.view().item(FIRST).is_none());
    assert!(session.view().item(THIRD).is_some());
    assert!(session.view().item(FOURTH).is_some());
}

#[test]
fn multiple_policy_replacements_correlate_noops_changes_and_intermediate_versions() {
    let mut session = session();
    let original = session.policy().clone();
    let mut replacement = original.clone();
    replacement.set_allow_tab_merge(false);
    let selection = session.prepare_select_item(SECOND);
    let mut frame = session.begin_host_frame().unwrap();
    let selection = frame.submit_prepared_action(selection).unwrap();
    let selected_version = frame.version();
    let unchanged = frame.replace_policy(original.clone()).unwrap();
    let changed = frame.replace_policy(replacement.clone()).unwrap();
    let repeated = frame.replace_policy(replacement).unwrap();
    let restored = frame.replace_policy(original).unwrap();
    let report = commit(frame);

    assert!(matches!(
        report.submitted_action_outcome(selection),
        Some(HostInputOutcome::ProductActionApplied(
            DockspaceActionOutcome::Selected {
                item: SECOND,
                changed: true,
            }
        ))
    ));
    let intermediate = WorkspaceVersion::new(
        selected_version.epoch(),
        WorkspaceRevision::new(selected_version.revision().get() + 1),
    );
    for (action, changed, version) in [
        (unchanged, false, selected_version),
        (changed, true, intermediate),
        (repeated, false, intermediate),
        (restored, true, report.after()),
    ] {
        assert_eq!(
            report.submitted_action_outcome(action),
            Some(&HostInputOutcome::PolicyReplaced { changed, version })
        );
    }
    assert_ne!(unchanged, repeated);
    assert_ne!(changed, restored);
    assert_eq!(report.inputs().len(), 5);
    assert_eq!(report.after().epoch(), selected_version.epoch());
    assert_eq!(
        report.after().revision().get(),
        selected_version.revision().get() + 2
    );
    let next = commit(session.begin_host_frame().unwrap());
    assert!(next.submitted_action_outcome(changed).is_none());
    assert!(next.submitted_action_outcome(unchanged).is_none());
}

#[test]
fn pending_query_preserves_target_scope_across_selection_and_unrelated_policy_changes() {
    let mut session = session();
    let prepared = session.prepare_close_root(ROOT);
    let mut frame = session.begin_host_frame().unwrap();
    let requested = frame.submit_prepared_close_request(prepared).unwrap();
    let report = commit(frame);
    let plan = requested_plan(&report, requested);

    let mut frame = session.begin_host_frame().unwrap();
    frame.select_item_current(SECOND).unwrap();
    commit(frame);
    assert_eq!(
        session.pending_content_close_plans().collect::<Vec<_>>(),
        [plan.clone()]
    );
    let mut replacement = session.policy().clone();
    replacement.set_allow_tab_merge(false);
    let mut frame = session.begin_host_frame().unwrap();
    frame.replace_policy(replacement).unwrap();
    commit(frame);
    assert_eq!(
        session.pending_content_close_plans().collect::<Vec<_>>(),
        [plan.clone()]
    );

    let mut frame = session.begin_host_frame().unwrap();
    let first = frame
        .resolve_close(
            plan.request(),
            plan.items()[0].token(),
            CloseDecision::Allow,
        )
        .unwrap();
    let second = frame
        .resolve_close(
            plan.request(),
            plan.items()[1].token(),
            CloseDecision::Allow,
        )
        .unwrap();
    let report = commit(frame);
    assert!(matches!(
        report.submitted_action_outcome(first),
        Some(HostInputOutcome::CloseDecisionProcessed {
            resolution: DockspaceCloseResolution::Recorded { item: FIRST, .. },
            application: None,
            changed: false,
            ..
        })
    ));
    assert!(matches!(
        report.submitted_action_outcome(second),
        Some(HostInputOutcome::CloseDecisionProcessed {
            resolution: DockspaceCloseResolution::Approved { request },
            application: Some(Ok(DockspaceCloseOutcome::RootClosed { root: ROOT, items })),
            changed: true,
            ..
        }) if *request == plan.request() && items == &[FIRST, SECOND]
    ));
    assert_eq!(session.pending_content_close_plans().count(), 0);
    assert!(session.view().root(ROOT).is_none());
    assert!(session.view().item(THIRD).is_some());
    assert!(session.view().item(FOURTH).is_some());
}

#[test]
fn pending_query_retires_target_scope_after_effective_close_policy_changes() {
    let mut session = session();
    let prepared = session.prepare_close_root(ROOT);
    let mut frame = session.begin_host_frame().unwrap();
    let requested = frame.submit_prepared_close_request(prepared).unwrap();
    let report = commit(frame);
    let plan = requested_plan(&report, requested);

    let original = session.policy().clone();
    let mut replacement = original.clone();
    replacement.set_close_capability(CloseCapability::Disabled);
    let mut frame = session.begin_host_frame().unwrap();
    frame.replace_policy(replacement).unwrap();
    commit(frame);
    assert_eq!(session.pending_content_close_plans().count(), 0);

    let mut frame = session.begin_host_frame().unwrap();
    frame.replace_policy(original).unwrap();
    commit(frame);
    let before = session.version();
    let mut frame = session.begin_host_frame().unwrap();
    let stale = frame
        .resolve_close(
            plan.request(),
            plan.items()[0].token(),
            CloseDecision::Allow,
        )
        .unwrap();
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
    assert_eq!(session.version(), before);
    assert_eq!(session.pending_content_close_plans().count(), 0);
    assert!(session.view().item(FIRST).is_some());
    assert!(session.view().item(SECOND).is_some());
}

#[test]
fn pending_content_query_excludes_native_surface_decisions() {
    let mut session = session();
    session
        .enable_managed_native_host(
            NativeCreateStrategy::Staged,
            NativePointerRoster::Exact(vec![]),
        )
        .unwrap();
    session
        .configure_managed_native_capabilities(
            NativeHostCapabilities::none_supported()
                .with(NativeHostCapability::NativeWindowLifecycle)
                .with(NativeHostCapability::AuthoritativeInventory)
                .with(NativeHostCapability::CloseCancellation),
        )
        .unwrap();
    session
        .register_native_root(SURFACE, HostWindowToken::new(1))
        .unwrap();
    let report = commit(session.begin_host_frame().unwrap());
    let [HostInputOutcome::NativeSurfaceRegistered { binding }] = report.inputs() else {
        panic!("one exact native root registration is expected")
    };
    let binding = *binding;
    session
        .report_managed_native_snapshot(
            [(
                binding,
                NativeWindowFacts::live().with_close(NativeCloseState::Requested, None),
            )],
            NativeWorkAreaRoster::Unknown,
        )
        .unwrap();
    let report = commit(
        session
            .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
            .unwrap(),
    );
    let [HostInputOutcome::NativePlatformSnapshotApplied { close_requests }] = report.inputs()
    else {
        panic!("the exact native close edge must be published")
    };
    assert_eq!(close_requests.len(), 1);
    session
        .request_native_surface_close(close_requests[0], NativeSurfaceCloseAction::RetainLayout)
        .unwrap();
    let mut frame = session
        .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
        .unwrap();
    frame.request_close_item_current(THIRD).unwrap();
    let report = commit(frame);
    assert!(report.inputs().iter().any(|outcome| matches!(
        outcome,
        HostInputOutcome::NativeSurfaceCloseRequested { plan, .. }
            if matches!(plan.target(), ClosePlanTarget::Surface { surface: SURFACE, .. })
                && plan.phase().requires_decision()
    )));
    let pending = session.pending_content_close_plans().collect::<Vec<_>>();
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].target(), ClosePlanTarget::Item { item: THIRD });
    assert_eq!(
        pending[0].items()[0].state(),
        CloseItemDecisionState::Pending
    );
}
