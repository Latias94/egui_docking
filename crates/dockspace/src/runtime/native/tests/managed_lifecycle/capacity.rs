use super::*;
use std::num::NonZeroUsize;

fn pending_programmatic_create() -> (DockspaceSession, crate::runtime::NativeEffectRequest) {
    let (mut session, _) = programmatic_tear_off_session();
    assert!(session.abandoned_native_effects.is_quiescent());
    session.abandoned_native_effects =
        crate::runtime::native_effect::NativeEffectDropQueue::with_capacity_limit(1).unwrap();
    let mut frame = session
        .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
        .unwrap();
    frame
        .move_root_to_new_window_current(
            ROOT,
            NativeWindowPlacement::new(PhysicalRect::new(920.0, 120.0, 420.0, 320.0).unwrap()),
        )
        .unwrap();
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    let mut report = frame.commit().unwrap();
    let request = take_only_native_effect(&mut report);
    assert!(matches!(
        request.operation(),
        NativeEffectOperation::CreateWindow { .. }
    ));
    (session, request)
}

fn limit_recorder(session: &mut DockspaceSession, records: usize) {
    let native = session.native.as_mut().unwrap();
    native
        .reclaim_committed_prefix(&mut session.engine)
        .unwrap();
    native
        .recorder_mut()
        .configure_native_transport_capacity(NonZeroUsize::new(records).unwrap(), 4 << 20)
        .unwrap();
}

#[test]
fn published_unemitted_create_retirement_keeps_the_producer_quiescence_guard() {
    for reset_layout in [false, true] {
        let (mut session, root) = programmatic_tear_off_session();
        session.abandoned_native_effects =
            crate::runtime::native_effect::NativeEffectDropQueue::with_capacity_limit(0).unwrap();
        session
            .record_native_pointer(unknown_native_pointer_input(91, NativePointerEvent::Moved))
            .unwrap();
        let mut frame = session
            .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
            .unwrap();
        frame
            .move_root_to_new_window_current(
                ROOT,
                NativeWindowPlacement::new(PhysicalRect::new(920.0, 120.0, 420.0, 320.0).unwrap()),
            )
            .unwrap();
        frame
            .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
            .unwrap();
        let mut report = frame.commit().unwrap();
        assert!(report.take_native_effects().is_empty());
        let child = *report
            .native_bindings()
            .iter()
            .find(|binding| **binding != root)
            .unwrap();
        let (_, saga) = session
            .engine
            .viewport()
            .native_create_sagas()
            .next()
            .unwrap();
        assert!(
            !session
                .engine
                .viewport()
                .effects()
                .record(saga.create())
                .unwrap()
                .was_emitted()
        );
        assert!(session.recognizes_native_binding(child));
        let reset = reset_layout.then(|| {
            session
                .prepare_layout_reset(DockspaceLayout::empty())
                .unwrap()
        });
        let mut frame = if let Some(reset) = &reset {
            session
                .begin_native_layout_reset_frame(reset, |_| NativeReceiverAnswer::Unknown)
                .unwrap()
        } else {
            session
                .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
                .unwrap()
        };
        if !reset_layout {
            assert!(frame.cancel_next_native_create().unwrap());
        }
        frame
            .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
            .unwrap();
        let mut report = frame.commit().unwrap();
        assert!(report.take_native_effects().is_empty());
        assert!(!session.is_current_native_binding(child));
        assert!(session.recognizes_native_binding(child));
        assert_eq!(
            session
                .engine
                .runtime_retention_manifest()
                .bindings()
                .terminal_binding_guards(),
            1
        );
        session.report_native_binding_quiescence(child).unwrap();
        commit_managed_frame(&mut session);
        commit_managed_frame(&mut session);
        assert_eq!(
            session
                .engine
                .runtime_retention_manifest()
                .bindings()
                .terminal_binding_guards(),
            0
        );
        assert!(!session.recognizes_native_binding(child));
    }
}

#[test]
fn bounded_effect_submission_returns_the_affine_result_until_accepted_input_drains() {
    let (mut session, request) = pending_programmatic_create();
    let failed = request.dispatch_failed(NativeDispatchFailure::WindowUnavailable);
    let binding = failed.binding();
    let epoch = failed.receipt_epoch();
    assert!(!session.abandoned_native_effects.is_quiescent());
    limit_recorder(&mut session, 1);
    session
        .record_native_pointer(unknown_native_pointer_input(91, NativePointerEvent::Moved))
        .unwrap();
    let recorded = session.native_input_recorded_count();
    let rejected = session.report_native_effect_result(failed).unwrap_err();
    assert_eq!(rejected.kind(), NativeHostErrorKind::InputCapacity);
    let (_, failed) = rejected.into_parts();
    assert_eq!(failed.binding(), binding);
    assert_eq!(failed.receipt_epoch(), epoch);
    assert_eq!(session.native_input_recorded_count(), recorded);
    assert!(!session.abandoned_native_effects.is_quiescent());

    commit_managed_frame(&mut session);
    assert!(!session.abandoned_native_effects.is_quiescent());
    session.report_native_effect_result(failed).unwrap();
    assert!(session.abandoned_native_effects.is_quiescent());
    assert_eq!(session.native_input_recorded_count(), recorded + 1);
    let mut report = commit_managed_frame_report(&mut session);
    assert!(!report.native_input_pending());
    assert!(report.take_native_effects().is_empty());
    assert_eq!(session.view().item(ITEM).unwrap().surface(), SURFACE);
    commit_managed_frame(&mut session);
    assert_eq!(session.native_input_recorded_count(), recorded + 1);
}

#[test]
fn effect_report_storage_rejection_rolls_back_the_complete_programmatic_tear_off() {
    let (mut session, _) = programmatic_tear_off_session();
    assert!(session.abandoned_native_effects.is_quiescent());
    session.abandoned_native_effects =
        crate::runtime::native_effect::NativeEffectDropQueue::with_capacity_limit(0).unwrap();
    let version = session.version();
    let frontier = session.engine.presentation_identity_frontier();
    let recorded = session.native_input_recorded_count();
    let mut frame = session
        .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
        .unwrap();
    frame
        .move_root_to_new_window_current(
            ROOT,
            NativeWindowPlacement::new(PhysicalRect::new(920.0, 120.0, 420.0, 320.0).unwrap()),
        )
        .unwrap();
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    assert_eq!(
        frame.commit().unwrap_err().native_kind(),
        Some(NativeHostErrorKind::InputCapacity)
    );
    assert_eq!(session.version(), version);
    assert_eq!(session.engine.presentation_identity_frontier(), frontier);
    assert_eq!(session.native_input_recorded_count(), recorded);
    assert_eq!(session.view().item(ITEM).unwrap().surface(), SURFACE);
    assert!(session.abandoned_native_effects.is_quiescent());

    session.abandoned_native_effects =
        crate::runtime::native_effect::NativeEffectDropQueue::with_capacity_limit(1).unwrap();
    let mut frame = session
        .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
        .unwrap();
    frame
        .move_root_to_new_window_current(
            ROOT,
            NativeWindowPlacement::new(PhysicalRect::new(920.0, 120.0, 420.0, 320.0).unwrap()),
        )
        .unwrap();
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    let mut report = frame.commit().unwrap();
    let request = take_only_native_effect(&mut report);
    assert!(matches!(
        request.operation(),
        NativeEffectOperation::CreateWindow { .. }
    ));
    assert!(!session.abandoned_native_effects.is_quiescent());
    let capacity = session.abandoned_native_effects.owned_data_capacity_bytes();
    drop(request);
    assert_eq!(
        session.abandoned_native_effects.owned_data_capacity_bytes(),
        capacity
    );
    assert!(!session.abandoned_native_effects.is_quiescent());
    commit_managed_frame(&mut session);
    assert!(session.abandoned_native_effects.is_quiescent());
    assert_eq!(session.view().item(ITEM).unwrap().surface(), SURFACE);
}

#[test]
fn full_recorder_drains_before_retrying_an_abandoned_create_result() {
    let (mut session, request) = pending_programmatic_create();
    limit_recorder(&mut session, 1);
    session
        .record_native_pointer(unknown_native_pointer_input(91, NativePointerEvent::Moved))
        .unwrap();
    let recorded = session.native_input_recorded_count();
    drop(request);

    let mut prefix = session
        .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
        .expect("an abandoned effect cannot trap accepted input behind a full prelude");
    assert!(prefix.native_input_pending());
    assert!(prefix.cancel_native_input_for_overload().is_err());
    prefix
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    let report = prefix.commit().unwrap();
    assert!(report.native_input_pending());
    assert_eq!(session.native_input_recorded_count(), recorded);
    assert_eq!(
        session.engine.backend_ingress_committed_through().get(),
        recorded
    );

    let mut report = commit_managed_frame_report(&mut session);
    assert!(!report.native_input_pending());
    assert!(report.take_native_effects().is_empty());
    assert_eq!(session.native_input_recorded_count(), recorded + 1);
    assert_eq!(session.view().item(ITEM).unwrap().surface(), SURFACE);
    commit_managed_frame(&mut session);
    assert_eq!(session.native_input_recorded_count(), recorded + 1);
}

#[test]
fn suspended_atomic_replay_preserves_blocked_prelude_across_poll_and_drop() {
    for abandon_before_poll in [true, false] {
        let (mut session, request) = pending_programmatic_create();
        let mut request = Some(request);
        limit_recorder(&mut session, 257);
        session
            .record_native_pointer_batch(
                (0..257).map(|_| unknown_native_pointer_input(91, NativePointerEvent::Moved)),
            )
            .unwrap();
        if abandon_before_poll {
            drop(request.take());
        }
        let recorded = session.native_input_recorded_count();
        let committed = session.engine.backend_ingress_committed_through();
        let clock = NativeMonotonicClock::new(|| std::time::Duration::ZERO);
        assert!(
            session
                .poll_native_host_frame(&mut NativeInputBudget::new(&clock), |_| {
                    NativeReceiverAnswer::Unknown
                })
                .unwrap()
                .is_none()
        );
        drop(request.take());
        assert_eq!(
            session.engine.backend_ingress_committed_through(),
            committed
        );
        let candidate = session
            .poll_native_host_frame(&mut NativeInputBudget::new(&clock), |_| {
                NativeReceiverAnswer::Unknown
            })
            .unwrap()
            .unwrap();
        assert!(candidate.native_input_pending());
        drop(candidate);
        assert_eq!(
            session.engine.backend_ingress_committed_through(),
            committed
        );
        assert_eq!(session.native_input_recorded_count(), recorded);

        let report = commit_managed_frame_report(&mut session);
        assert!(report.native_input_pending());
        assert_eq!(session.native_input_recorded_count(), recorded);
        assert_eq!(
            session.engine.backend_ingress_committed_through().get(),
            recorded
        );
        let report = commit_managed_frame_report(&mut session);
        assert!(!report.native_input_pending());
        assert_eq!(session.native_input_recorded_count(), recorded + 1);
        commit_managed_frame(&mut session);
        assert_eq!(session.native_input_recorded_count(), recorded + 1);
    }
}

#[test]
fn effect_abandoned_after_poll_keeps_the_committed_report_pending() {
    let (mut session, request) = pending_programmatic_create();
    limit_recorder(&mut session, 1);
    let mut frame = session
        .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
        .unwrap();
    assert!(!frame.native_input_pending());
    drop(request);
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    let report = frame.commit().unwrap();
    assert!(report.native_input_pending());
    assert!(!commit_managed_frame_report(&mut session).native_input_pending());
}
