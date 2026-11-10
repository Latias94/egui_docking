//! Application-owned close decisions through the renderer-neutral public API.

use dockspace::close::{
    CloseDecision, ClosePlanTarget, CloseRequestId, DeferredCloseDecision, DeferredCloseToken,
};
use dockspace::geometry::{LogicalRect, LogicalSize};
use dockspace::model::{
    DockAnchor, DockPlacement, DockspaceLayout, DockspaceNode, DockspaceRootLayout,
    DockspaceSurfaceLayout, ItemId, RootId, SurfaceId,
};
use dockspace::policy::{CloseCapability, DockItemRule, DockPolicy};
use dockspace::runtime::{
    DockspaceCloseOutcome, DockspaceCloseResolution, DockspaceHostFrame, DockspaceSession,
    HostFrameReport, HostInputOutcome, NativeDesktopPointerLocation, NativeDesktopPosition,
    NativeInputBudget, NativeMonotonicClock, NativePointerEvent, NativePointerHover,
    NativePointerId, NativePointerInput, NativePointerOwner, NativePointerRoster,
    SurfaceUnavailableReason, UniformSurfaceMetrics,
};

const SURFACE: SurfaceId = SurfaceId::new(1);
const ROOT: RootId = RootId::new(1);
const FIRST: ItemId = ItemId::new(1);
const SECOND: ItemId = ItemId::new(2);
const OTHER_SURFACE: SurfaceId = SurfaceId::new(2);
const OTHER_ROOT: RootId = RootId::new(2);
const THIRD: ItemId = ItemId::new(3);
const FOURTH: ItemId = ItemId::new(4);

fn session() -> DockspaceSession {
    let layout = DockspaceLayout::new([DockspaceSurfaceLayout::new(
        SURFACE,
        DockspaceRootLayout::new(ROOT, DockspaceNode::tabs([FIRST, SECOND])),
    )])
    .unwrap();
    let mut policy = DockPolicy::default();
    policy.set_close_capability(CloseCapability::DeferredAllowed);
    DockspaceSession::from_layout(layout, policy).unwrap()
}

fn measure(frame: &mut dockspace::runtime::DockspaceHostFrame<'_>) {
    frame
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
}

fn commit_deferred(mut frame: DockspaceHostFrame<'_>) -> HostFrameReport {
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    frame.commit().unwrap()
}

fn two_surface_session() -> DockspaceSession {
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

fn defer_close(
    session: &mut DockspaceSession,
    target: ClosePlanTarget,
) -> (CloseRequestId, DeferredCloseToken) {
    let prepared = match target {
        ClosePlanTarget::Item { item } => session.prepare_close_item(item),
        ClosePlanTarget::Root { root } => session.prepare_close_root(root),
        ClosePlanTarget::Surface { .. } => panic!("content-close fixture"),
    };
    let mut frame = session.begin_host_frame().unwrap();
    frame.submit_prepared_close_request(prepared).unwrap();
    let report = commit_deferred(frame);
    let [HostInputOutcome::CloseRequested { plan, .. }] = report.inputs() else {
        panic!("the target must publish its approval roster");
    };
    assert_eq!(plan.target(), target);
    let request = plan.request();
    let mut frame = session.begin_host_frame().unwrap();
    for (index, item) in plan.items().iter().enumerate() {
        frame
            .resolve_close(
                request,
                item.token(),
                if index + 1 == plan.items().len() {
                    CloseDecision::Deferred
                } else {
                    CloseDecision::Allow
                },
            )
            .unwrap();
    }
    let report = commit_deferred(frame);
    let continuation = report
        .inputs()
        .iter()
        .rev()
        .find_map(|outcome| match outcome {
            HostInputOutcome::CloseDecisionProcessed {
                plan: Some(plan), ..
            } => plan.items().iter().find_map(|item| item.deferred_token()),
            _ => None,
        })
        .expect("the last item must own one exact deferred continuation");
    assert!(!report.workspace_changed());
    (request, continuation)
}

fn close_item(session: &mut DockspaceSession, item: ItemId) {
    let prepared = session.prepare_close_item(item);
    let mut frame = session.begin_host_frame().unwrap();
    frame.submit_prepared_close_request(prepared).unwrap();
    let report = commit_deferred(frame);
    let [HostInputOutcome::CloseRequested { plan, .. }] = report.inputs() else {
        panic!("item close plan");
    };
    let mut frame = session.begin_host_frame().unwrap();
    frame
        .resolve_close(
            plan.request(),
            plan.items()[0].token(),
            CloseDecision::Allow,
        )
        .unwrap();
    assert!(commit_deferred(frame).workspace_changed());
    assert!(session.view().item(item).is_none());
}

#[test]
fn root_close_waits_for_every_decision_and_deferred_continuation_commits_once() {
    let mut session = session();
    let before = session.version();
    let request = session.prepare_close_root(ROOT);
    let mut frame = session.begin_host_frame().unwrap();
    frame.submit_prepared_close_request(request).unwrap();
    measure(&mut frame);
    let report = frame.commit().unwrap();
    let [
        HostInputOutcome::CloseRequested {
            plan,
            reused: false,
            ..
        },
    ] = report.inputs()
    else {
        panic!("an independent host receives the complete application close plan")
    };
    assert_eq!(plan.target(), ClosePlanTarget::Root { root: ROOT });
    let request = plan.request();
    let items = plan.items().to_vec();
    assert_eq!(
        items.iter().map(|item| item.item()).collect::<Vec<_>>(),
        [FIRST, SECOND]
    );
    assert!(!report.workspace_changed());

    let mut frame = session.begin_host_frame().unwrap();
    frame
        .resolve_close(request, items[0].token(), CloseDecision::Allow)
        .unwrap();
    frame
        .resolve_close(request, items[1].token(), CloseDecision::Deferred)
        .unwrap();
    measure(&mut frame);
    let report = frame.commit().unwrap();
    assert!(!report.workspace_changed());
    assert_eq!(session.version(), before);
    assert!(session.view().item(FIRST).is_some() && session.view().item(SECOND).is_some());
    let continuation = report
        .inputs()
        .iter()
        .find_map(|input| match input {
            HostInputOutcome::CloseDecisionProcessed {
                plan: Some(plan), ..
            } => plan
                .items()
                .iter()
                .find(|item| item.item() == SECOND)
                .and_then(|item| item.deferred_token()),
            _ => None,
        })
        .expect("the deferred decision publishes its exact continuation");

    let mut frame = session.begin_host_frame().unwrap();
    frame
        .continue_deferred_close(request, continuation, DeferredCloseDecision::Allow)
        .unwrap();
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    let report = frame.commit().unwrap();
    assert!(report.workspace_changed());
    assert!(
        matches!(report.inputs(), [HostInputOutcome::CloseDecisionProcessed {
        application: Some(Ok(DockspaceCloseOutcome::RootClosed { root: ROOT, items })), changed: true, ..
    }] if items == &[FIRST, SECOND])
    );
    assert!(session.view().item(FIRST).is_none() && session.view().item(SECOND).is_none());
    let committed = session.version();
    let mut frame = session.begin_host_frame().unwrap();
    frame
        .resolve_close(request, items[0].token(), CloseDecision::Allow)
        .unwrap();
    frame
        .continue_deferred_close(request, continuation, DeferredCloseDecision::Allow)
        .unwrap();
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    let report = frame.commit().unwrap();
    assert!(!report.workspace_changed());
    assert_eq!(session.version(), committed);
    assert!(report.inputs().iter().all(|input| matches!(
        input,
        HostInputOutcome::CloseDecisionProcessed {
            resolution: DockspaceCloseResolution::Inert(_),
            application: None,
            changed: false,
            ..
        }
    )));
}

#[test]
fn veto_keeps_the_entire_group_and_later_local_selection_remains_usable() {
    let mut session = session();
    let request = session.prepare_close_root(ROOT);
    let mut frame = session.begin_host_frame().unwrap();
    frame.submit_prepared_close_request(request).unwrap();
    measure(&mut frame);
    let report = frame.commit().unwrap();
    let [HostInputOutcome::CloseRequested { plan, .. }] = report.inputs() else {
        panic!("close plan")
    };
    let request = plan.request();
    let items = plan.items().to_vec();
    let before = session.version();
    let mut frame = session.begin_host_frame().unwrap();
    frame
        .resolve_close(request, items[0].token(), CloseDecision::Allow)
        .unwrap();
    frame
        .resolve_close(request, items[1].token(), CloseDecision::Veto)
        .unwrap();
    measure(&mut frame);
    assert!(!frame.commit().unwrap().workspace_changed());
    assert_eq!(session.version(), before);
    assert_eq!(
        session.view().item(FIRST).unwrap().tabs().items(),
        &[FIRST, SECOND]
    );
    let select = session.prepare_select_item(SECOND);
    let mut frame = session.begin_host_frame().unwrap();
    frame.submit_prepared_action(select).unwrap();
    measure(&mut frame);
    assert!(frame.commit().unwrap().workspace_changed());
    assert!(session.view().item(SECOND).unwrap().is_selected());
}

#[test]
fn current_close_decisions_resume_bounded_prefixes_and_drop_retries_the_original_tokens() {
    const ITEM_COUNT: usize = 300;
    const PREFIX: usize = 64;
    let items = (1..=u64::try_from(ITEM_COUNT).unwrap()).map(ItemId::new);
    let layout = DockspaceLayout::new([DockspaceSurfaceLayout::new(
        SURFACE,
        DockspaceRootLayout::new(ROOT, DockspaceNode::tabs(items)),
    )])
    .unwrap();
    let mut session = DockspaceSession::from_layout(layout, DockPolicy::default()).unwrap();
    let prepared = session.prepare_close_root(ROOT);
    let mut frame = session.begin_host_frame().unwrap();
    frame.submit_prepared_close_request(prepared).unwrap();
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    let report = frame.commit().unwrap();
    let [HostInputOutcome::CloseRequested { plan, .. }] = report.inputs() else {
        panic!("the core must issue one complete root request");
    };
    let request = plan.request();
    let first = plan.items()[0];
    let version = session.version();
    {
        let mut dropped = session.begin_host_frame().unwrap();
        assert_eq!(dropped.pending_close_decisions(request).count(), ITEM_COUNT);
        assert_eq!(dropped.pending_close_decisions(request).next(), Some(first));
        dropped
            .resolve_close(request, first.token(), CloseDecision::Allow)
            .unwrap();
        assert_eq!(
            dropped.pending_close_decisions(request).count(),
            ITEM_COUNT - 1
        );
    }
    assert_eq!(session.version(), version);

    let mut resolved = 0;
    while resolved < ITEM_COUNT {
        let mut frame = session.begin_host_frame().unwrap();
        let pending = frame
            .pending_close_decisions(request)
            .take(PREFIX)
            .collect::<Vec<_>>();
        assert_eq!(pending.len(), PREFIX.min(ITEM_COUNT - resolved));
        assert_eq!(
            pending[0].item(),
            ItemId::new(u64::try_from(resolved + 1).unwrap())
        );
        for decision in &pending {
            frame
                .resolve_close(request, decision.token(), CloseDecision::Allow)
                .unwrap();
        }
        resolved += pending.len();
        assert_eq!(
            frame.pending_close_decisions(request).count(),
            ITEM_COUNT - resolved
        );
        frame
            .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
            .unwrap();
        let report = frame.commit().unwrap();
        assert_eq!(report.workspace_changed(), resolved == ITEM_COUNT);
        if resolved != ITEM_COUNT {
            assert_eq!(session.version(), version);
            assert_eq!(
                session.view().item(FIRST).unwrap().tabs().items().len(),
                ITEM_COUNT
            );
        }
    }
    let frame = session.begin_host_frame().unwrap();
    assert!(frame.pending_close_decisions(request).next().is_none());
    assert!(frame.view().item(FIRST).is_none());
}

#[test]
fn current_close_decisions_exclude_deferred_foreign_and_vetoed_requests() {
    let mut session = session();
    let prepared = session.prepare_close_root(ROOT);
    let mut frame = session.begin_host_frame().unwrap();
    frame.submit_prepared_close_request(prepared).unwrap();
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    let report = frame.commit().unwrap();
    let [HostInputOutcome::CloseRequested { plan, .. }] = report.inputs() else {
        panic!("the root close must return its request");
    };
    let request = plan.request();
    let mut foreign = self::session();
    assert!(
        foreign
            .begin_host_frame()
            .unwrap()
            .pending_close_decisions(request)
            .next()
            .is_none()
    );
    let mut frame = session.begin_host_frame().unwrap();
    let pending = frame.pending_close_decisions(request).collect::<Vec<_>>();
    frame
        .resolve_close(request, pending[0].token(), CloseDecision::Deferred)
        .unwrap();
    assert_eq!(
        frame.pending_close_decisions(request).collect::<Vec<_>>(),
        [pending[1]]
    );
    frame
        .resolve_close(request, pending[1].token(), CloseDecision::Veto)
        .unwrap();
    assert!(frame.pending_close_decisions(request).next().is_none());
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    frame.commit().unwrap();
    assert!(
        session
            .begin_host_frame()
            .unwrap()
            .pending_close_decisions(request)
            .next()
            .is_none()
    );
}

#[test]
fn current_close_decisions_wait_for_the_complete_native_prefix() {
    let mut session = session();
    let prepared = session.prepare_close_root(ROOT);
    let mut frame = session.begin_host_frame().unwrap();
    frame.submit_prepared_close_request(prepared).unwrap();
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    let report = frame.commit().unwrap();
    let [HostInputOutcome::CloseRequested { plan, .. }] = report.inputs() else {
        panic!("the root request must precede native input enrollment");
    };
    let request = plan.request();
    let first = plan.items()[0];
    session
        .enable_managed_native_host(NativePointerRoster::Exact(Vec::new()))
        .unwrap();
    let movement = NativePointerInput::new(
        NativePointerId::new(9),
        NativePointerEvent::Moved,
        NativeDesktopPointerLocation::new(
            NativeDesktopPosition::Unknown,
            NativePointerHover::Unknown,
            None,
        ),
        NativePointerOwner::Unknown,
        NativePointerOwner::Unknown,
    );
    session
        .record_native_pointer_sequence([movement; 256])
        .unwrap();
    let clock = NativeMonotonicClock::new(|| std::time::Duration::ZERO);
    let mut prefix = session
        .poll_native_host_frame(&mut NativeInputBudget::new(&clock), |_| unreachable!())
        .unwrap()
        .unwrap();
    assert!(prefix.native_input_pending());
    assert!(prefix.pending_close_decisions(request).next().is_none());
    prefix
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    assert!(prefix.commit().unwrap().native_input_pending());
    for _ in 0..2 {
        let final_prefix = session
            .poll_native_host_frame(&mut NativeInputBudget::new(&clock), |_| unreachable!())
            .unwrap()
            .unwrap();
        assert!(!final_prefix.native_input_pending());
        assert_eq!(
            final_prefix.pending_close_decisions(request).next(),
            Some(first)
        );
        assert_eq!(final_prefix.pending_close_decisions(request).count(), 2);
    }
}

#[test]
fn review_deferred_close_survives_tab_selection_on_another_surface() {
    const OTHER_SURFACE: SurfaceId = SurfaceId::new(2);
    const THIRD: ItemId = ItemId::new(3);
    const FOURTH: ItemId = ItemId::new(4);
    for select_other_surface in [false, true] {
        let layout = DockspaceLayout::new([
            DockspaceSurfaceLayout::new(
                SURFACE,
                DockspaceRootLayout::new(ROOT, DockspaceNode::central_tabs([FIRST, SECOND])),
            ),
            DockspaceSurfaceLayout::new(
                OTHER_SURFACE,
                DockspaceRootLayout::new(
                    RootId::new(2),
                    DockspaceNode::central_tabs([THIRD, FOURTH]),
                ),
            ),
        ])
        .unwrap();
        let mut policy = DockPolicy::default();
        policy.set_close_capability(CloseCapability::DeferredAllowed);
        let mut session = DockspaceSession::from_layout(layout, policy).unwrap();
        let prepared = session.prepare_close_item(FIRST);
        let mut frame = session.begin_host_frame().unwrap();
        frame.submit_prepared_close_request(prepared).unwrap();
        frame
            .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
            .unwrap();
        let report = frame.commit().unwrap();
        let [HostInputOutcome::CloseRequested { plan, .. }] = report.inputs() else {
            panic!("closing one pane must issue its application close plan");
        };
        let request = plan.request();
        let [item] = plan.items() else {
            panic!("only the pane being saved may need approval");
        };
        assert_eq!(item.item(), FIRST);
        let mut frame = session.begin_host_frame().unwrap();
        frame
            .resolve_close(request, item.token(), CloseDecision::Deferred)
            .unwrap();
        frame
            .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
            .unwrap();
        let report = frame.commit().unwrap();
        let [
            HostInputOutcome::CloseDecisionProcessed {
                plan: Some(plan), ..
            },
        ] = report.inputs()
        else {
            panic!("deferring the save must publish its continuation");
        };
        let continuation = plan.items()[0].deferred_token().unwrap();
        if select_other_surface {
            let mut frame = session.begin_host_frame().unwrap();
            frame.select_item_current(FOURTH).unwrap();
            frame
                .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
                .unwrap();
            assert!(frame.commit().unwrap().workspace_changed());
            assert!(session.view().item(FOURTH).unwrap().is_selected());
        }
        let mut frame = session.begin_host_frame().unwrap();
        frame
            .continue_deferred_close(request, continuation, DeferredCloseDecision::Allow)
            .unwrap();
        frame
            .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
            .unwrap();
        let report = frame.commit().unwrap();
        assert!(
            session.view().item(FIRST).is_none(),
            "selecting another surface must preserve the saved pane's approval: {:?}",
            report.inputs()
        );
        assert!(report.workspace_changed());
        assert_eq!(
            session.view().item(SECOND).unwrap().tabs().items(),
            &[SECOND]
        );
        assert_eq!(
            session.view().item(THIRD).unwrap().tabs().items(),
            &[THIRD, FOURTH]
        );
        if select_other_surface {
            assert!(session.view().item(FOURTH).unwrap().is_selected());
        }
    }
}

#[test]
fn deferred_root_close_scopes_to_content_not_selection_order_or_unrelated_edits() {
    const FIFTH: ItemId = ItemId::new(5);
    let mut session = two_surface_session();
    let (request, continuation) = defer_close(&mut session, ClosePlanTarget::Root { root: ROOT });

    let mut frame = session.begin_host_frame().unwrap();
    frame.select_item_current(SECOND).unwrap();
    frame
        .dock_item_current(SECOND, DockPlacement::Before(FIRST))
        .unwrap();
    frame
        .open_item_current(FIFTH, DockPlacement::Center(DockAnchor::Item(FOURTH)))
        .unwrap();
    assert!(commit_deferred(frame).workspace_changed());
    assert_eq!(
        session.view().item(FIRST).unwrap().tabs().items(),
        &[SECOND, FIRST]
    );
    close_item(&mut session, THIRD);

    let mut policy = session.policy().clone();
    let mut rule = DockItemRule::default();
    rule.set_close_capability(Some(CloseCapability::Immediate));
    policy.set_item_rule(FOURTH, rule);
    let mut frame = session.begin_host_frame().unwrap();
    frame.replace_policy(policy).unwrap();
    commit_deferred(frame);
    assert!(
        session
            .pending_content_close_plans()
            .any(|plan| plan.request() == request)
    );

    let mut frame = session.begin_host_frame().unwrap();
    frame
        .continue_deferred_close(request, continuation, DeferredCloseDecision::Allow)
        .unwrap();
    let report = commit_deferred(frame);
    assert!(report.workspace_changed());
    assert!(matches!(
        report.inputs(),
        [HostInputOutcome::CloseDecisionProcessed {
            application: Some(Ok(DockspaceCloseOutcome::RootClosed { root: ROOT, items })),
            changed: true,
            ..
        }] if items == &[FIRST, SECOND]
    ));
    assert!(session.view().item(FIRST).is_none());
    assert!(session.view().item(SECOND).is_none());
    assert_eq!(
        session.view().item(FOURTH).unwrap().tabs().items(),
        &[FOURTH, FIFTH]
    );
}

#[test]
fn deferred_root_close_rejects_changed_target_membership() {
    for add_member in [true, false] {
        let mut session = two_surface_session();
        let (request, continuation) =
            defer_close(&mut session, ClosePlanTarget::Root { root: ROOT });
        if add_member {
            let mut frame = session.begin_host_frame().unwrap();
            frame
                .dock_item_current(THIRD, DockPlacement::Center(DockAnchor::Item(FIRST)))
                .unwrap();
            assert!(commit_deferred(frame).workspace_changed());
        } else {
            close_item(&mut session, SECOND);
        }
        assert!(
            session
                .pending_content_close_plans()
                .all(|plan| plan.request() != request)
        );
        let before = session.version();
        let mut frame = session.begin_host_frame().unwrap();
        frame
            .continue_deferred_close(request, continuation, DeferredCloseDecision::Allow)
            .unwrap();
        let report = commit_deferred(frame);
        assert_eq!(session.version(), before);
        assert!(!report.workspace_changed());
        assert!(matches!(
            report.inputs(),
            [HostInputOutcome::CloseDecisionProcessed {
                resolution: DockspaceCloseResolution::Inert(_),
                application: None,
                ..
            }]
        ));
        assert!(session.view().item(FIRST).is_some());
        assert!(session.view().item(THIRD).is_some());
    }
}

#[test]
fn deferred_root_close_rejects_changed_surface_owner_with_identical_content() {
    let layout = DockspaceLayout::new([
        DockspaceSurfaceLayout::new(
            SURFACE,
            DockspaceRootLayout::new(ROOT, DockspaceNode::tabs([FIRST, SECOND])),
        ),
        DockspaceSurfaceLayout::rootless(OTHER_SURFACE).with_contained(
            dockspace::model::DockspaceContainedLayout::new(
                dockspace::model::FloatingPresentationId::new(1),
                DockspaceRootLayout::new(OTHER_ROOT, DockspaceNode::tabs([THIRD])),
                LogicalRect::new(30.0, 30.0, 240.0, 180.0).unwrap(),
            ),
        ),
    ])
    .unwrap();
    let mut policy = DockPolicy::default();
    policy.set_close_capability(CloseCapability::DeferredAllowed);
    let mut session = DockspaceSession::from_layout(layout, policy).unwrap();
    let (request, continuation) = defer_close(&mut session, ClosePlanTarget::Root { root: ROOT });
    let mut frame = session.begin_host_frame().unwrap();
    frame
        .dock_root_current(ROOT, DockPlacement::Main(OTHER_SURFACE))
        .unwrap();
    assert!(commit_deferred(frame).workspace_changed());
    assert_eq!(session.view().item(FIRST).unwrap().surface(), OTHER_SURFACE);
    let before = session.version();
    let mut frame = session.begin_host_frame().unwrap();
    frame
        .continue_deferred_close(request, continuation, DeferredCloseDecision::Allow)
        .unwrap();
    let report = commit_deferred(frame);
    assert!(!report.workspace_changed());
    assert_eq!(session.version(), before);
    assert!(matches!(
        report.inputs(),
        [HostInputOutcome::CloseDecisionProcessed {
            resolution: DockspaceCloseResolution::Inert(_),
            application: None,
            ..
        }]
    ));
    assert_eq!(
        session.view().item(FIRST).unwrap().tabs().items(),
        &[FIRST, SECOND]
    );
}

#[test]
fn deferred_item_close_rejects_relocation_and_cannot_close_a_reopened_item() {
    for relocate in [true, false] {
        let mut session = two_surface_session();
        let (request, continuation) =
            defer_close(&mut session, ClosePlanTarget::Item { item: FIRST });
        let mut frame = session.begin_host_frame().unwrap();
        frame
            .dock_item_current(FIRST, DockPlacement::Center(DockAnchor::Item(THIRD)))
            .unwrap();
        assert!(commit_deferred(frame).workspace_changed());
        if !relocate {
            close_item(&mut session, FIRST);
            let mut frame = session.begin_host_frame().unwrap();
            frame
                .open_item_current(FIRST, DockPlacement::Center(DockAnchor::Item(SECOND)))
                .unwrap();
            assert!(commit_deferred(frame).workspace_changed());
            assert_eq!(session.view().item(FIRST).unwrap().root(), ROOT);
        }
        let before = session.version();
        let mut frame = session.begin_host_frame().unwrap();
        frame
            .continue_deferred_close(request, continuation, DeferredCloseDecision::Allow)
            .unwrap();
        let report = commit_deferred(frame);
        assert!(!report.workspace_changed());
        assert_eq!(session.version(), before);
        assert!(matches!(
            report.inputs(),
            [HostInputOutcome::CloseDecisionProcessed {
                resolution: DockspaceCloseResolution::Inert(_),
                application: None,
                ..
            }]
        ));
        assert!(session.view().item(FIRST).is_some());
        assert!(session.view().item(SECOND).is_some());
    }
}

#[test]
fn deferred_close_rejects_effective_policy_change_even_after_restoration() {
    for capability in [CloseCapability::Disabled, CloseCapability::Immediate] {
        let mut session = session();
        let (request, continuation) =
            defer_close(&mut session, ClosePlanTarget::Item { item: FIRST });
        let original = session.policy().clone();
        let mut changed = original.clone();
        let mut rule = DockItemRule::default();
        rule.set_close_capability(Some(capability));
        changed.set_item_rule(FIRST, rule);
        let mut frame = session.begin_host_frame().unwrap();
        frame.replace_policy(changed).unwrap();
        commit_deferred(frame);
        assert!(
            session
                .pending_content_close_plans()
                .all(|plan| plan.request() != request)
        );
        let mut frame = session.begin_host_frame().unwrap();
        frame.replace_policy(original).unwrap();
        commit_deferred(frame);
        let before = session.version();
        let mut frame = session.begin_host_frame().unwrap();
        frame
            .continue_deferred_close(request, continuation, DeferredCloseDecision::Allow)
            .unwrap();
        let report = commit_deferred(frame);
        assert!(!report.workspace_changed());
        assert_eq!(session.version(), before);
        assert!(matches!(
            report.inputs(),
            [HostInputOutcome::CloseDecisionProcessed {
                resolution: DockspaceCloseResolution::Inert(_),
                application: None,
                ..
            }]
        ));
        assert!(session.view().item(FIRST).is_some());
    }
}

#[test]
fn deferred_close_rejects_replacement_epoch_even_when_target_content_matches() {
    let mut session = session();
    let (request, continuation) = defer_close(&mut session, ClosePlanTarget::Item { item: FIRST });
    let layout = DockspaceLayout::new([DockspaceSurfaceLayout::new(
        SURFACE,
        DockspaceRootLayout::new(ROOT, DockspaceNode::tabs([FIRST, SECOND])),
    )])
    .unwrap();
    let reset = session.prepare_layout_reset(layout).unwrap();
    commit_deferred(session.begin_layout_reset_frame(&reset).unwrap());
    let before = session.version();
    let mut frame = session.begin_host_frame().unwrap();
    frame
        .continue_deferred_close(request, continuation, DeferredCloseDecision::Allow)
        .unwrap();
    let report = commit_deferred(frame);
    assert!(!report.workspace_changed());
    assert_eq!(session.version(), before);
    assert!(matches!(
        report.inputs(),
        [HostInputOutcome::CloseDecisionProcessed {
            resolution: DockspaceCloseResolution::Inert(_),
            application: None,
            ..
        }]
    ));
    assert!(session.view().item(FIRST).is_some());
}
