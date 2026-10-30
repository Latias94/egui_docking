//! Application-owned close decisions through the renderer-neutral public API.

use dockspace::close::{CloseDecision, ClosePlanTarget, DeferredCloseDecision};
use dockspace::geometry::{LogicalRect, LogicalSize};
use dockspace::model::{
    DockspaceLayout, DockspaceNode, DockspaceRootLayout, DockspaceSurfaceLayout, ItemId, RootId,
    SurfaceId,
};
use dockspace::policy::{CloseCapability, DockPolicy};
use dockspace::runtime::{
    DockspaceCloseOutcome, DockspaceCloseResolution, DockspaceSession, HostInputOutcome,
    NativeDesktopPointerLocation, NativeDesktopPosition, NativeInputBudget, NativeMonotonicClock,
    NativePointerEvent, NativePointerHover, NativePointerId, NativePointerInput,
    NativePointerOwner, NativePointerRoster, SurfaceUnavailableReason, UniformSurfaceMetrics,
};

const SURFACE: SurfaceId = SurfaceId::new(1);
const ROOT: RootId = RootId::new(1);
const FIRST: ItemId = ItemId::new(1);
const SECOND: ItemId = ItemId::new(2);

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
        .enable_managed_native_host(
            dockspace::runtime::NativeCreateStrategy::Staged,
            NativePointerRoster::Exact(Vec::new()),
        )
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
