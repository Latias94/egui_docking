use super::*;
use crate::input::journal::AnyButtonDownAuthority;
use crate::runtime::{HostInputOutcome, NativeInputBudget, NativeMonotonicClock};
use std::time::Duration;

fn complete(mut frame: crate::runtime::DockspaceHostFrame<'_>) -> HostFrameReport {
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    frame.commit().unwrap()
}

#[test]
fn device_removal_is_one_ordered_record_and_same_batch_pointer_input_can_follow() {
    let (mut session, binding) =
        native_root_session_with_profile(NativeHostProfile::ManagedDesktop);
    commit_managed_frame(&mut session);
    session
        .record_native_pointer(unknown_native_pointer_input(
            11,
            NativePointerEvent::ButtonPressed(NativePointerButton::Primary),
        ))
        .unwrap();
    let before = session.native_input_recorded_count();
    let pointer = session.native.as_ref().unwrap().recorder.pointer_through();
    session.report_native_input_device_removal().unwrap();
    assert_eq!(session.native_input_recorded_count(), before + 1);
    assert_eq!(
        session.native.as_ref().unwrap().recorder.pointer_through(),
        pointer
    );
    session
        .record_native_pointer(unknown_native_pointer_input(
            12,
            NativePointerEvent::ButtonPressed(NativePointerButton::Primary),
        ))
        .unwrap();
    let clock = NativeMonotonicClock::new(|| Duration::ZERO);
    let frame = session
        .poll_native_host_frame(&mut NativeInputBudget::new(&clock), |_| {
            panic!("unknown pointer locations cannot query a receiver")
        })
        .unwrap()
        .unwrap();
    let report = complete(frame);
    assert_eq!(
        report.inputs(),
        [HostInputOutcome::NativeInputDeviceRemoval]
    );
    assert!(session.is_current_native_binding(binding));
    assert_eq!(session.native_input_recorded_count(), before + 2);
    assert_eq!(
        session
            .native
            .as_ref()
            .unwrap()
            .recorder
            .pointer_through()
            .get(),
        pointer.get() + 1,
    );
    assert_eq!(
        session
            .engine
            .runtime_retention_manifest()
            .pointer()
            .active_stream_count(),
        1,
        "only the post-removal pointer stream remains live",
    );
    assert_eq!(
        session.engine.pointer_button_authority(),
        AnyButtonDownAuthority::KnownDown
    );
}

#[test]
fn recorded_device_removal_drop_retries_without_publishing_reset_or_advancing_pointer_sequence() {
    let (mut session, binding) =
        native_root_session_with_profile(NativeHostProfile::ManagedDesktop);
    session
        .record_native_pointer(unknown_native_pointer_input(
            11,
            NativePointerEvent::ButtonPressed(NativePointerButton::Primary),
        ))
        .unwrap();
    complete(
        session
            .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
            .unwrap(),
    );
    let backend = session.engine.backend_ingress_committed_through();
    let pointer = session.native.as_ref().unwrap().recorder.pointer_through();
    let source = session.committed_source_sequence;
    let provider = session.engine.backend_ingress_provider();
    assert_eq!(
        session
            .engine
            .runtime_retention_manifest()
            .pointer()
            .active_stream_count(),
        1
    );
    session.report_native_input_device_removal().unwrap();
    let recorded = session.native_input_recorded_count();
    {
        let frame = session
            .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
            .unwrap();
        assert!(!frame.frame.input_terminated_for_overload());
    }
    assert_eq!(session.engine.backend_ingress_committed_through(), backend);
    assert_eq!(
        session
            .engine
            .runtime_retention_manifest()
            .pointer()
            .active_stream_count(),
        1
    );
    assert_eq!(session.native_input_recorded_count(), recorded);
    assert_eq!(
        session.native.as_ref().unwrap().recorder.pointer_through(),
        pointer
    );
    assert_eq!(session.committed_source_sequence, source);
    let report = complete(
        session
            .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
            .unwrap(),
    );
    assert_eq!(
        report.inputs(),
        [HostInputOutcome::NativeInputDeviceRemoval]
    );
    assert_eq!(
        session
            .engine
            .runtime_retention_manifest()
            .pointer()
            .active_stream_count(),
        0
    );
    assert!(matches!(
        session.engine.pointer_button_authority(),
        AnyButtonDownAuthority::Unknown(_)
    ));
    assert_eq!(session.engine.backend_ingress_provider(), provider);
    assert!(session.is_current_native_binding(binding));
    assert_eq!(
        session.native.as_ref().unwrap().recorder.pointer_through(),
        pointer
    );
    session
        .report_native_pointer_roster(NativePointerRoster::Exact(Vec::new()))
        .unwrap();
    complete(
        session
            .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
            .unwrap(),
    );
    assert_eq!(
        session.engine.pointer_button_authority(),
        AnyButtonDownAuthority::KnownAllReleased
    );
}

#[test]
fn observed_roots_cannot_report_a_managed_device_removal() {
    let (mut session, _) = native_root_session();
    let recorded = session.native_input_recorded_count();
    let error = session.report_native_input_device_removal().unwrap_err();
    assert_eq!(error.native_kind(), Some(NativeHostErrorKind::Unsupported));
    assert_eq!(session.native_input_recorded_count(), recorded);
}
