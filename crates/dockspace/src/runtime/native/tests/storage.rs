use super::*;
use crate::backend_ingress::{BackendIngressPayload, BackendIngressRecord};
use std::time::Duration;

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
