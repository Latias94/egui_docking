//! The editor lock uses ordinary policy authority while navigation remains available.

use dockspace::geometry::{LogicalRect, LogicalSize};
use dockspace::model::{
    DockPlacement, DockspaceActionOutcome, DockspaceActionRejection, DockspaceLayout,
    DockspaceNode, DockspaceRootLayout, DockspaceSurfaceLayout, ItemId, RootId, SurfaceId,
};
use dockspace::policy::{CloseCapability, DockItemRule, DockPolicy};
use dockspace::runtime::{
    DockspaceHostFrame, DockspaceSession, HostFrameReport, HostInputOutcome,
    SurfaceUnavailableReason, UniformSurfaceMetrics,
};

const SURFACE: SurfaceId = SurfaceId::new(1);
const ROOT: RootId = RootId::new(1);
const FIRST: ItemId = ItemId::new(1);
const SECOND: ItemId = ItemId::new(2);
const CLOSED: ItemId = ItemId::new(3);

fn layout() -> DockspaceLayout {
    DockspaceLayout::new([DockspaceSurfaceLayout::new(
        SURFACE,
        DockspaceRootLayout::new(ROOT, DockspaceNode::central_tabs([FIRST, SECOND])),
    )])
    .unwrap()
}

fn policy() -> DockPolicy {
    let mut policy = DockPolicy::default();
    let mut protected = DockItemRule::new();
    protected.set_close_capability(Some(CloseCapability::Disabled));
    policy.set_item_rule(FIRST, protected);
    policy
}

fn commit_measured(mut frame: DockspaceHostFrame<'_>) -> HostFrameReport {
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
    frame.commit().unwrap()
}

fn set_lock(session: &mut DockspaceSession, locked: bool) {
    let mut policy = session.policy().clone();
    policy.set_arrangement_locked(locked);
    let mut frame = session.begin_host_frame().unwrap();
    frame.replace_policy(policy).unwrap();
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    frame.commit().unwrap();
    assert_eq!(session.policy().arrangement_locked(), locked);
}

#[test]
fn locked_product_commands_reject_structural_changes_but_select_still_works() {
    let mut session = DockspaceSession::from_layout(layout(), policy()).unwrap();
    commit_measured(session.begin_host_frame().unwrap());
    let prepared = session.prepare_open_item(CLOSED, DockPlacement::After(FIRST));
    set_lock(&mut session, true);
    commit_measured(session.begin_host_frame().unwrap());
    let version = session.version();
    let mut frame = session.begin_host_frame().unwrap();
    frame.submit_prepared_action(prepared).unwrap();
    frame
        .open_item_current(CLOSED, DockPlacement::After(FIRST))
        .unwrap();
    frame
        .dock_item_current(SECOND, DockPlacement::Before(FIRST))
        .unwrap();
    frame.float_item_current(SECOND, SURFACE).unwrap();
    frame.request_close_item_current(SECOND).unwrap();
    let report = commit_measured(frame);
    assert_eq!(session.version(), version);
    assert!(!report.workspace_changed());
    assert_eq!(report.inputs().len(), 5);
    assert!(matches!(
        report.inputs()[0],
        HostInputOutcome::StaleRejected { .. }
    ));
    assert!(
        report.inputs()[1..4].iter().all(|input| matches!(
            input,
            HostInputOutcome::ProductActionRejected(DockspaceActionRejection::PolicyDenied)
        )),
        "{:#?}",
        report.inputs()
    );
    assert!(matches!(
        report.inputs()[4],
        HostInputOutcome::CloseRejected(_)
    ));
    assert_eq!(
        session.view().item(FIRST).unwrap().tabs().items(),
        &[FIRST, SECOND]
    );
    assert!(session.view().item(CLOSED).is_none());

    let mut frame = session.begin_host_frame().unwrap();
    frame.select_item_current(SECOND).unwrap();
    let report = commit_measured(frame);
    assert!(report.workspace_changed());
    assert!(session.view().item(SECOND).unwrap().is_selected());
    assert_eq!(
        session.view().item(SECOND).unwrap().tabs().items(),
        &[FIRST, SECOND]
    );

    set_lock(&mut session, false);
    let mut frame = session.begin_host_frame().unwrap();
    frame
        .open_item_current(CLOSED, DockPlacement::After(SECOND))
        .unwrap();
    frame.request_close_item_current(FIRST).unwrap();
    let report = commit_measured(frame);
    assert!(session.view().item(CLOSED).unwrap().is_selected());
    assert!(
        matches!(report.inputs()[1], HostInputOutcome::CloseRejected(_)),
        "unlock preserves the original per-item restriction"
    );
}

#[test]
fn current_action_tokens_correlate_mixed_batch_outcomes_in_submission_order() {
    let mut session = DockspaceSession::from_layout(layout(), policy()).unwrap();
    set_lock(&mut session, true);
    commit_measured(session.begin_host_frame().unwrap());
    let mut frame = session.begin_host_frame().unwrap();
    let actions = [
        frame
            .open_item_current(CLOSED, DockPlacement::After(FIRST))
            .unwrap(),
        frame.select_item_current(SECOND).unwrap(),
        frame
            .dock_item_current(SECOND, DockPlacement::Before(FIRST))
            .unwrap(),
        frame.request_close_item_current(SECOND).unwrap(),
        frame.select_item_current(FIRST).unwrap(),
        frame.float_item_current(SECOND, SURFACE).unwrap(),
        frame.request_close_root_current(ROOT).unwrap(),
    ];
    let report = commit_measured(frame);

    assert_eq!(report.inputs().len(), actions.len());
    for (index, action) in actions.iter().copied().enumerate() {
        assert!(actions[..index].iter().all(|earlier| *earlier != action));
        assert_eq!(
            report.submitted_action_outcome(action),
            Some(&report.inputs()[index]),
            "each submitted action resolves its own ordered outcome"
        );
    }
    for index in [0, 2] {
        assert!(
            matches!(
                report.inputs()[index],
                HostInputOutcome::ProductActionRejected(DockspaceActionRejection::PolicyDenied)
            ),
            "unexpected product outcome at {index}: {:?}",
            report.inputs()
        );
    }
    assert_eq!(
        report.inputs()[5],
        HostInputOutcome::ProductActionRejected(
            DockspaceActionRejection::PresentationUnavailable { surface: SURFACE }
        )
    );
    for (index, item) in [(1, SECOND), (4, FIRST)] {
        assert_eq!(
            report.inputs()[index],
            HostInputOutcome::ProductActionApplied(DockspaceActionOutcome::Selected {
                item,
                changed: true,
            })
        );
    }
    for index in [3, 6] {
        assert!(matches!(
            report.inputs()[index],
            HostInputOutcome::CloseRejected(_)
        ));
    }
    assert!(report.workspace_changed());
    assert!(session.view().item(FIRST).unwrap().is_selected());
    assert_eq!(
        session.view().item(FIRST).unwrap().tabs().items(),
        &[FIRST, SECOND]
    );
    assert!(session.view().item(CLOSED).is_none());
    assert_eq!(session.pending_content_close_plans().count(), 0);

    let next = commit_measured(session.begin_host_frame().unwrap());
    for action in actions {
        assert!(next.submitted_action_outcome(action).is_none());
    }
}

#[test]
fn dropping_the_unlock_frame_preserves_published_lock_and_close_plan_stales_on_lock() {
    use dockspace::close::CloseDecision;

    let mut session = DockspaceSession::from_layout(layout(), policy()).unwrap();
    let mut frame = session.begin_host_frame().unwrap();
    frame.request_close_item_current(SECOND).unwrap();
    let report = commit_measured(frame);
    let [HostInputOutcome::CloseRequested { plan, .. }] = report.inputs() else {
        panic!("the unlocked request opens one close plan");
    };
    set_lock(&mut session, true);
    let mut unlocked = session.policy().clone();
    unlocked.set_arrangement_locked(false);
    let mut dropped = session.begin_host_frame().unwrap();
    dropped.replace_policy(unlocked).unwrap();
    drop(dropped);
    assert!(session.policy().arrangement_locked());
    let version = session.version();
    let mut frame = session.begin_host_frame().unwrap();
    frame
        .resolve_close(
            plan.request(),
            plan.items()[0].token(),
            CloseDecision::Allow,
        )
        .unwrap();
    let report = commit_measured(frame);
    assert!(!report.workspace_changed());
    assert_eq!(session.version(), version);
    assert!(session.view().item(SECOND).is_some());
}

#[cfg(feature = "serde")]
#[test]
fn live_layout_restore_requires_an_explicit_committed_unlock_and_preserves_rules() {
    use dockspace::runtime::{
        DockspaceDocumentBootstrap, DockspaceDocumentId, DockspaceRuntimeErrorKind,
    };

    let document = DockspaceDocumentId::from_bytes([0x43; 16]);
    let mut bootstrap = DockspaceDocumentBootstrap::new(document);
    assert_eq!(bootstrap.ensure_item("first").unwrap(), FIRST);
    assert_eq!(bootstrap.ensure_item("second").unwrap(), SECOND);
    let mut session =
        DockspaceSession::from_persistent_layout(layout(), policy(), bootstrap).unwrap();
    let snapshot = session.save_document_json().unwrap();
    let prepared = session
        .prepare_document_restore_json(&snapshot, |_, _| true)
        .unwrap();
    set_lock(&mut session, true);
    let version = session.version();
    let generation = session.next_document_generation();
    let error = match session.begin_document_restore_frame(&prepared) {
        Ok(_) => panic!("a locked arrangement cannot accept a saved preset"),
        Err(error) => error,
    };
    assert_eq!(error.kind(), DockspaceRuntimeErrorKind::OperationConflict);
    assert_eq!(session.version(), version);
    assert_eq!(session.next_document_generation(), generation);
    assert!(session.policy().arrangement_locked());

    set_lock(&mut session, false);
    let prepared = session
        .prepare_document_restore_json(&snapshot, |_, _| true)
        .unwrap();
    let mut restore = session.begin_document_restore_frame(&prepared).unwrap();
    restore
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    restore.commit().unwrap();
    let mut frame = session.begin_host_frame().unwrap();
    frame.request_close_item_current(FIRST).unwrap();
    let report = commit_measured(frame);
    assert!(matches!(
        report.inputs(),
        [HostInputOutcome::CloseRejected(_)]
    ));
    assert!(!session.policy().arrangement_locked());
}

#[test]
fn locked_tab_pointer_press_selects_without_arming_a_drag_or_changing_arrangement() {
    use dockspace::geometry::LogicalPoint;
    use dockspace::runtime::{
        HostPointerInput, HostPointerRoute, SurfacePointerButton, SurfacePointerCapture,
        SurfacePointerEvent, SurfacePointerId, SurfacePointerReceiverFacts,
        SurfacePresentationResult,
    };
    use dockspace_host_harness::RecordingRenderer;

    for (same_batch, travel) in [(true, 0.0), (true, 120.0), (false, 120.0)] {
        let mut session = DockspaceSession::from_layout(layout(), policy()).unwrap();
        set_lock(&mut session, true);
        session.enable_host_pointer().unwrap();
        commit_measured(session.begin_host_frame().unwrap());
        let mut frame = session.begin_host_frame().unwrap();
        let plan = frame.paint_plan(SURFACE).unwrap().unwrap();
        let descriptor = plan.tab_receiver(SECOND).unwrap();
        let (_, receipt) = RecordingRenderer::execute(plan).unwrap();
        frame.confirm_surface_painted(receipt).unwrap();
        let mut report = frame.commit().unwrap();
        for output in report.take_painted_outputs() {
            session
                .report_surface_presentation(output, SurfacePresentationResult::Presented)
                .unwrap();
        }
        let mut settle = session.begin_host_frame().unwrap();
        settle
            .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
            .unwrap();
        settle.commit().unwrap();
        let receiver = session.bind_presented_receiver(&descriptor).unwrap();
        let presented = session.presented_surface(SURFACE).unwrap();
        let origin = HostPointerRoute::new(presented, receiver.center());
        let moved = HostPointerRoute::new(
            presented,
            LogicalPoint::new(receiver.center().x() + travel, receiver.center().y()).unwrap(),
        );
        let press = HostPointerInput::new(
            SurfacePointerId::new(1),
            SurfacePointerEvent::ButtonPressed(SurfacePointerButton::Primary),
            origin,
            origin,
            SurfacePointerCapture::ProviderEndpoint,
            SurfacePointerReceiverFacts::delivery(&receiver),
        );
        let tail = [
            HostPointerInput::new(
                SurfacePointerId::new(1),
                SurfacePointerEvent::Moved,
                moved,
                moved,
                SurfacePointerCapture::ProviderEndpoint,
                SurfacePointerReceiverFacts::unknown(),
            ),
            HostPointerInput::new(
                SurfacePointerId::new(1),
                SurfacePointerEvent::ButtonReleased(SurfacePointerButton::Primary),
                moved,
                moved,
                SurfacePointerCapture::ProviderEndpoint,
                SurfacePointerReceiverFacts::unknown(),
            ),
        ];
        let mut frame = session.begin_host_frame().unwrap();
        if same_batch {
            frame
                .submit_host_pointer_batch(std::iter::once(press).chain(tail))
                .unwrap();
            commit_measured(frame);
        } else {
            frame.submit_host_pointer_batch([press]).unwrap();
            commit_measured(frame);
            assert!(session.view().item(SECOND).unwrap().is_selected());
            assert!(
                !session.has_active_gesture(),
                "a policy-denied source must never arm a drag on press"
            );
            let mut frame = session.begin_host_frame().unwrap();
            frame.submit_host_pointer_batch(tail).unwrap();
            commit_measured(frame);
        }
        assert!(session.view().item(SECOND).unwrap().is_selected());
        assert_eq!(
            session.view().item(SECOND).unwrap().tabs().items(),
            &[FIRST, SECOND]
        );
        assert!(session.view().item(SECOND).unwrap().contained().is_none());
        assert!(!session.has_active_gesture());
    }
}
