use super::*;
use crate::backend_ingress::{BackendIngressPayload, BackendIngressRecord, BackendIngressRecorder};
use std::time::Duration;

#[test]
fn pending_report_capacity_tracks_exact_emissions_and_shared_abandonment_once() {
    let (mut session, binding) =
        native_root_session_with_profile(NativeHostProfile::ManagedDesktop);
    let bounds = PhysicalRect::new(0.0, 0.0, 640.0, 480.0).unwrap();
    let scale = ScaleFactor::new(1.0).unwrap();
    session
        .report_managed_native_snapshot(
            [(
                binding,
                NativeWindowFacts::live()
                    .with_content_bounds(bounds)
                    .with_outer_bounds(bounds)
                    .with_native_scale_factor(scale)
                    .with_presentation_scale_factor(scale)
                    .with_input(NativeWindowInputState::ReceivesInput, None)
                    .with_presentation(NativeWindowPresentationState::Visible, None)
                    .with_close(NativeCloseState::Clear, None),
            )],
            NativeWorkAreaRoster::Exact(vec![work_area(1)]),
        )
        .unwrap();
    commit_managed_frame(&mut session);
    let baseline = session.native_input_pending_report_capacity_bytes();
    let (output, _) = paint_native_output(&mut session);
    let pending = session.native_input_pending_report_capacity_bytes();
    assert!(
        pending > baseline,
        "an affine emission has retained report data"
    );
    let shared_queue = session.abandoned_native_effects.clone();
    assert_eq!(
        session.native_input_pending_report_capacity_bytes(),
        pending
    );
    drop(shared_queue);
    assert_eq!(
        session.native_input_pending_report_capacity_bytes(),
        pending
    );
    drop(output);
    let abandoned = session.native_input_pending_report_capacity_bytes();
    assert_eq!(
        abandoned, pending,
        "Drop fills the original prepaid output slot"
    );
    commit_managed_frame(&mut session);
    let settled = session.native_input_pending_report_capacity_bytes();
    assert!(
        settled >= baseline,
        "settlement keeps reusable report backing charged"
    );
    commit_managed_frame(&mut session);
    assert_eq!(
        session.native_input_pending_report_capacity_bytes(),
        settled
    );
}

#[test]
fn native_semantic_box_capacity_is_charged_once_across_batch_clones() {
    let (session, _) = native_root_session();
    let recorder = &session.native.as_ref().unwrap().recorder;
    assert_eq!(recorder.retained_records().len(), 1);
    let recorded = &recorder.retained_records()[0];
    let BackendIngressPayload::SemanticInput(input) = recorded.payload() else {
        panic!("the observed-root session retains its registration payload");
    };
    assert!(matches!(
        input.as_ref(),
        EngineInput::RegisterViewport { .. }
    ));
    let expected = size_of::<BackendIngressRecorder>()
        + recorder.record_buffer_capacity_bytes()
        + recorder.shared_fixed_data_bytes()
        + size_of::<EngineInput>();
    assert_eq!(
        session.native_input_owned_capacity_bytes().unwrap(),
        expected
    );

    let batch = recorder.pending_batch().unwrap();
    let cloned = batch.clone();
    for frozen in [&batch, &cloned] {
        assert_ne!(
            frozen.records().as_ptr(),
            recorder.retained_records().as_ptr()
        );
        assert!(std::ptr::eq(
            recorded.payload(),
            frozen.records()[0].payload()
        ));
        let BackendIngressPayload::SemanticInput(frozen_input) = frozen.records()[0].payload()
        else {
            panic!("the frozen batch retains its boxed registration");
        };
        assert!(std::ptr::eq(input.as_ref(), frozen_input.as_ref()));
    }
    assert_ne!(batch.records().as_ptr(), cloned.records().as_ptr());
    assert_eq!(
        session.native_input_owned_capacity_bytes().unwrap(),
        expected
    );
}

#[test]
fn native_input_capacity_reclaims_committed_256_before_the_next_256_append() {
    let (mut session, _) = native_root_session_with_profile(NativeHostProfile::ManagedDesktop);
    commit_managed_frame(&mut session);
    session
        .record_native_pointer_sequence(
            (0..256).map(|_| unknown_native_pointer_input(11, NativePointerEvent::Moved)),
        )
        .unwrap();
    let first_through = session.native.as_ref().unwrap().recorder.recorded_through();
    let first_bytes = session.native_input_owned_capacity_bytes().unwrap();
    let first_buffer = session
        .native
        .as_ref()
        .unwrap()
        .recorder
        .record_buffer_capacity_bytes();
    let clock = NativeMonotonicClock::new(|| Duration::ZERO);
    let mut frame = session
        .poll_native_host_frame(&mut NativeInputBudget::new(&clock), |_| {
            panic!("unknown locations do not query a receiver")
        })
        .unwrap()
        .unwrap();
    assert!(!frame.native_input_pending());
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    frame.commit().unwrap();
    assert_eq!(
        session.engine.backend_ingress_committed_through(),
        first_through
    );
    assert_eq!(
        session.native_input_owned_capacity_bytes().unwrap(),
        first_bytes
    );
    assert_eq!(
        session
            .native
            .as_ref()
            .unwrap()
            .recorder
            .retained_records()
            .len(),
        256,
        "commit alone retains the first payloads",
    );

    session
        .record_native_pointer_sequence(
            (0..256).map(|_| unknown_native_pointer_input(11, NativePointerEvent::Moved)),
        )
        .unwrap();
    let recorder = &session.native.as_ref().unwrap().recorder;
    assert_eq!(recorder.retained_records().len(), 256);
    assert!(
        recorder
            .retained_records()
            .iter()
            .all(|record| record.ordinal() > first_through),
        "the next append retires every committed payload before adding its prefix",
    );
    assert_eq!(recorder.recorded_through().get(), first_through.get() + 256);
    assert_eq!(recorder.record_buffer_capacity_bytes(), first_buffer);
    assert!(first_buffer >= 256 * size_of::<BackendIngressRecord>());
    assert_eq!(
        session.native_input_owned_capacity_bytes().unwrap(),
        first_bytes
    );
    assert_eq!(
        session.engine.backend_ingress_committed_through(),
        first_through
    );
}

#[test]
fn native_input_capacity_counts_private_replay_buffers_without_recharging_payloads() {
    let (mut session, _) = native_root_session_with_profile(NativeHostProfile::ManagedDesktop);
    commit_managed_frame(&mut session);
    session
        .record_native_pointer_batch(
            (0..257).map(|_| unknown_native_pointer_input(11, NativePointerEvent::Moved)),
        )
        .unwrap();
    let before = session.native_input_owned_capacity_bytes().unwrap();
    let clock = NativeMonotonicClock::new(|| Duration::ZERO);
    assert!(
        session
            .poll_native_host_frame(&mut NativeInputBudget::new(&clock), |_| {
                panic!("unknown locations do not query a receiver")
            })
            .unwrap()
            .is_none()
    );
    let native = session.native.as_ref().unwrap();
    let pending = native.pending_replay.as_ref().unwrap();
    let replay = pending.frame.backend_ingress_buffer_capacity_bytes();
    assert!(replay >= 257 * size_of::<BackendIngressRecord>());
    assert_eq!(
        session.native_input_owned_capacity_bytes().unwrap(),
        before + replay
    );
    let retained_before_prepare = format!("{:?}", session.native);
    assert!(session.has_pending_native_input());
    assert_eq!(session.prepare_native_pointer_capacity().unwrap(), None);
    assert_eq!(
        format!("{:?}", session.native),
        retained_before_prepare,
        "capacity maintenance cannot retire storage retained by a suspended candidate"
    );

    let mut frame = session
        .poll_native_host_frame(&mut NativeInputBudget::new(&clock), |_| {
            panic!("unknown locations do not query a receiver")
        })
        .unwrap()
        .unwrap();
    let guard = frame.frame.backend_ingress_buffer_capacity_bytes();
    assert!(guard > 0);
    assert_eq!(
        frame.native_input_owned_capacity_bytes().unwrap(),
        before + guard
    );
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    frame.commit().unwrap();
    let after_commit = session.native_input_owned_capacity_bytes().unwrap();
    assert_eq!(
        after_commit, before,
        "commit alone does not retire payloads"
    );

    let frame = session.begin_host_frame().unwrap();
    let retired = frame.native_input_owned_capacity_bytes().unwrap();
    assert!(
        retired < before,
        "the next frame reclaims committed payload allocations"
    );
    let native = frame.session.native.as_ref().unwrap();
    assert!(
        native.recorder.record_buffer_capacity_bytes() >= 257 * size_of::<BackendIngressRecord>(),
        "retirement preserves spare recorder capacity"
    );
}

#[test]
fn native_pointer_appends_own_distinct_contents_even_at_the_same_location() {
    let (mut session, _) = native_root_session_with_profile(NativeHostProfile::ManagedDesktop);
    commit_managed_frame(&mut session);
    session
        .record_native_pointer_sequence([
            unknown_native_pointer_input(11, NativePointerEvent::Moved),
            unknown_native_pointer_input(11, NativePointerEvent::Moved),
        ])
        .unwrap();
    let native = session.native.as_ref().unwrap();
    let journals = native
        .recorder
        .retained_records()
        .iter()
        .filter_map(|record| match record.payload() {
            BackendIngressPayload::PointerSegment(journal) => Some(journal),
            _ => None,
        })
        .filter(|journal| !journal.edges().is_empty())
        .collect::<Vec<_>>();
    assert_eq!(journals.len(), 2);
    assert_ne!(journals[0].edges().as_ptr(), journals[1].edges().as_ptr());
    assert_eq!(
        journals[0].owned_contents_capacity_bytes(),
        journals[1].owned_contents_capacity_bytes()
    );
}

#[test]
fn native_input_capacity_rejects_unknown_semantic_storage_instead_of_undercounting() {
    let (mut session, _) = native_root_session_with_profile(NativeHostProfile::ManagedDesktop);
    let expected = session.version();
    session
        .native
        .as_mut()
        .unwrap()
        .recorder_mut()
        .record_semantic_input(EngineInput::SelectItem {
            expected,
            item: ITEM,
        })
        .unwrap();
    assert!(session.native_input_owned_capacity_bytes().is_err());
}

#[test]
fn native_input_capacity_rejects_dynamic_close_dispositions_outside_the_facade() {
    let (mut session, binding) =
        native_root_session_with_profile(NativeHostProfile::ManagedDesktop);
    session
        .publish_native_close(binding, NativeCloseState::Requested, None)
        .unwrap();
    let mut frame = session.begin_host_frame().unwrap();
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    let report = frame.commit().unwrap();
    let close = report
        .inputs()
        .iter()
        .find_map(|input| match input {
            crate::runtime::HostInputOutcome::NativeCloseObservationApplied { close_requests } => {
                close_requests.first().copied()
            }
            _ => None,
        })
        .unwrap();
    let expected = session.version();
    session
        .native
        .as_mut()
        .unwrap()
        .recorder_mut()
        .record_semantic_input(EngineInput::RequestSurfaceClose {
            expected,
            edge: close.edge,
            request: crate::close_plan::SurfaceCloseRequest::RehomeAll {
                target: crate::close_plan::SurfaceRehomeTarget::new(
                    SURFACE,
                    None,
                    Vec::with_capacity(32),
                ),
            },
        })
        .unwrap();
    assert!(session.native_input_owned_capacity_bytes().is_err());
}
