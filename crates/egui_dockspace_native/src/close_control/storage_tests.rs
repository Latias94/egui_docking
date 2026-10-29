//! Capacity and exact-owner models using real core close capabilities.
//!
//! Tokenless events exercise admission before HoldClose and inline receipt
//! transitions. Canonical fork token routing still requires the fork fixtures.

use dockspace::runtime::{
    DockspaceSession, HostInputOutcome, NativeCloseState, NativeEffectAcknowledgement,
    NativeMonotonicClock,
};
use std::time::Duration;
use winit::event::WindowEvent;

use super::capacity_tests::{commit, fixture, request};
use super::*;
use crate::mailbox::{HostRecord, journal::HostIngressJournal};

fn event(binding: NativeSurfaceBinding, ordinal: u64) -> NativeWindowEventRecord {
    NativeWindowEventRecord::for_test(
        ordinal,
        WindowId::from(11),
        Some(ViewportId::ROOT),
        Some(binding),
        WindowEvent::CloseRequested,
    )
}

fn cancellation(
    session: &mut DockspaceSession,
    close: NativeSurfaceCloseRequest,
) -> NativeCloseEffectAcknowledgement {
    session.cancel_native_surface_close(close).unwrap();
    let mut report = commit(session);
    let mut effects = report.take_native_effects();
    assert_eq!(effects.len(), 1);
    let Some(NativeEffectAcknowledgement::Close(acknowledgement)) =
        effects.pop().unwrap().accepted()
    else {
        panic!("core cancellation must provide its exact close acknowledgement");
    };
    acknowledgement
}

#[test]
fn finite_capture_rejects_before_hold_and_duplicate_admission_keeps_owner_and_progress() {
    let fixtures = [fixture(1), fixture(1), fixture(1)];
    for limit in [0, 1, 2] {
        let mut control = NativeCloseControl::with_capacity_limit(limit).unwrap();
        let baseline = control.owned_data_capacity_bytes();
        let backing = control.pending.as_ptr();
        assert!(!control.can_configure_capacity_limit());
        for (_, binding) in fixtures.iter().take(limit).rev() {
            let binding = *binding;
            let capture = event(binding, 11);
            let progress = control.progress();
            for _ in 0..2 {
                assert_eq!(
                    control.preflight_capture(&capture, binding),
                    NativeCloseCaptureAdmission::First
                );
                assert!(!control.references_binding(binding));
                assert_eq!(control.progress(), progress);
            }
            assert_eq!(
                control.capture_request(&capture, binding),
                NativeCloseCaptureAdmission::First
            );
            let retained = control.progress();
            assert!(control.needs_request_recording(&capture, binding));
            for _ in 0..2 {
                assert_eq!(
                    control.preflight_capture(&capture, binding),
                    NativeCloseCaptureAdmission::Duplicate
                );
                assert_eq!(
                    control.capture_request(&capture, binding),
                    NativeCloseCaptureAdmission::Duplicate
                );
                assert!(control.retains_request(&capture, binding));
                assert_eq!(control.progress(), retained);
            }
        }
        assert!(control.pending.windows(2).all(|pair| pair[0].0 < pair[1].0));
        let refused = fixtures[limit].1;
        let capture = event(refused, 12);
        let before = format!("{control:?}");
        assert_eq!(
            control.preflight_capture(&capture, refused),
            NativeCloseCaptureAdmission::Conflict
        );
        assert_eq!(
            control.capture_request(&capture, refused),
            NativeCloseCaptureAdmission::Conflict
        );
        assert_eq!(format!("{control:?}"), before);
        assert!(!control.references_binding(refused));
        assert_eq!(control.pending.len(), limit);
        assert_eq!(control.pending.as_ptr(), backing);
        assert_eq!(control.pending.capacity(), limit);
        assert_eq!(control.owned_data_capacity_bytes(), baseline);
        assert_eq!(
            control.progress().requests_retained(),
            u64::try_from(limit).unwrap()
        );

        if limit != 0 {
            let retired = fixtures[0].1;
            control.retire_binding(retired);
            assert!(!control.references_binding(retired));
            assert_eq!(
                control.capture_request(&capture, refused),
                NativeCloseCaptureAdmission::First
            );
            control.retire_binding(retired);
            assert!(control.references_binding(refused));
            assert_eq!(control.pending.len(), limit);
            assert_eq!(control.pending.as_ptr(), backing);
            assert_eq!(control.owned_data_capacity_bytes(), baseline);
        }
    }
}

#[test]
fn distinct_provider_keys_on_the_same_host_route_never_overwrite_an_existing_owner() {
    // Independent sessions mint real provider-bound handles. Both are mapped
    // to the same adapter window, viewport and reusable numeric window token.
    let (mut first_session, first) = fixture(1);
    let (mut second_session, second) = fixture(1);
    let (_third_session, third) = fixture(1);
    assert_ne!(first, second);
    assert_eq!(first.surface(), second.surface());
    assert_eq!(first.window_token(), second.window_token());
    let first_close = request(&mut first_session, first);
    let second_close = request(&mut second_session, second);
    let mut control = NativeCloseControl::with_capacity_limit(2).unwrap();
    let baseline = control.owned_data_capacity_bytes();
    let backing = control.pending.as_ptr();
    for (binding, close) in [(second, second_close), (first, first_close)] {
        assert!(control.observe_request(&event(binding, 21), binding));
        assert_eq!(control.bind_request(close), Ok(true));
    }
    assert_eq!(control.pending.len(), 2);
    assert!(control.pending.windows(2).all(|pair| pair[0].0 < pair[1].0));
    assert_eq!(control.request_for_binding(first), Some(first_close));
    assert_eq!(control.request_for_binding(second), Some(second_close));
    assert_eq!(
        control.capture_request(&event(third, 21), third),
        NativeCloseCaptureAdmission::Conflict
    );
    assert!(control.accept_close(first_close));
    assert!(!control.accept_close(first_close));
    assert_eq!(control.request_for_binding(second), Some(second_close));

    let destroyed = |binding, ordinal| {
        NativeWindowEventRecord::for_test(
            ordinal,
            WindowId::from(11),
            Some(ViewportId::ROOT),
            Some(binding),
            WindowEvent::Destroyed,
        )
    };
    assert!(!control.settle_accepted_destroyed(&destroyed(second, 22), first));
    assert!(!control.settle_accepted_destroyed(&destroyed(first, 21), first));
    assert_eq!(
        control.accepted_destroyed_matches(&destroyed(first, 22), first),
        Ok(true)
    );
    assert!(control.settle_accepted_destroyed(&destroyed(first, 22), first));
    assert!(!control.settle_accepted_destroyed(&destroyed(first, 22), first));
    assert_eq!(control.request_for_binding(second), Some(second_close));
    assert!(control.observe_request(&event(third, 23), third));
    control.retire_binding(first);
    assert!(control.references_binding(third));
    assert!(control.references_binding(second));
    assert_eq!(control.pending.as_ptr(), backing);
    assert_eq!(control.owned_data_capacity_bytes(), baseline);
}

#[test]
fn full_owner_promotes_cancel_successor_only_after_clear_and_callback_commit_without_growth() {
    for callback_already_received in [false, true] {
        let (mut session, binding) = fixture(1);
        let (_foreign_session, foreign) = fixture(1);
        let first = event(binding, 31);
        let successor = event(binding, 32);
        let close = request(&mut session, binding);
        let mut control = NativeCloseControl::with_capacity_limit(1).unwrap();
        let baseline = control.owned_data_capacity_bytes();
        let backing = control.pending.as_ptr();
        assert!(control.observe_request(&first, binding));
        assert_eq!(control.bind_request(close), Ok(true));
        let acknowledgement = cancellation(&mut session, close);
        assert_eq!(
            control.begin_cancellation(close, acknowledgement),
            Some(ViewportId::ROOT)
        );

        let mut journal =
            HostIngressJournal::new(NativeMonotonicClock::new(|| Duration::from_millis(17)));
        let mut receipt = CancellationReceipt::new();
        if callback_already_received {
            assert_eq!(
                receipt.receive(|| journal.reserve(HostRecord::WindowEvent(first.clone()))),
                NativeCloseCallbackAdmission::First
            );
        }
        control
            .pending_for_binding_mut(binding)
            .unwrap()
            .cancellation_receipt = Some(receipt);
        let progress = control.progress();
        assert_eq!(
            control.preflight_capture(&successor, binding),
            NativeCloseCaptureAdmission::First
        );
        assert_eq!(control.progress(), progress);
        assert_eq!(
            control.capture_request(&successor, binding),
            NativeCloseCaptureAdmission::First
        );
        let retained = control.progress();
        assert_eq!(
            control.capture_request(&successor, binding),
            NativeCloseCaptureAdmission::Duplicate
        );
        assert_eq!(
            control.capture_request(&event(binding, 33), binding),
            NativeCloseCaptureAdmission::Conflict
        );
        assert_eq!(
            control.capture_request(&event(foreign, 32), foreign),
            NativeCloseCaptureAdmission::Conflict
        );
        assert_eq!(control.progress(), retained);

        let ingress = journal.reserve(HostRecord::WindowEvent(successor.clone()));
        let ordinal = ingress.ordinal();
        let captured_at = ingress.captured_at();
        let ingress = control
            .retain_successor_ingress(foreign, ingress)
            .unwrap_err();
        assert_eq!(ingress.ordinal(), ordinal);
        assert_eq!(ingress.captured_at(), captured_at);
        control.retain_successor_ingress(binding, ingress).unwrap();
        assert_eq!(control.parked_ingress().count(), 1);
        assert!(!control.needs_request_recording(&successor, binding));
        assert_eq!(control.settle_clear(true), 0);
        assert_eq!(control.pending.capacity(), 1);

        let receipt = control
            .pending_for_binding_mut(binding)
            .unwrap()
            .cancellation_receipt
            .as_mut()
            .unwrap();
        if !callback_already_received {
            assert_eq!(
                receipt.receive(|| journal.reserve(HostRecord::WindowEvent(first.clone()))),
                NativeCloseCallbackAdmission::First
            );
        }
        assert_eq!(
            receipt.receive(|| panic!("a duplicate callback cannot reserve another ingress")),
            NativeCloseCallbackAdmission::Duplicate
        );
        let callback_ordinal = control.callback_ingress().next().unwrap().ordinal();
        assert!(
            control
                .take_callback_ingress(callback_ordinal + 100)
                .is_none()
        );
        assert_eq!(
            control
                .take_callback_ingress(callback_ordinal)
                .unwrap()
                .ordinal(),
            callback_ordinal
        );
        assert!(control.take_callback_ingress(callback_ordinal).is_none());
        session
            .publish_native_close(binding, NativeCloseState::Clear, Some(acknowledgement))
            .unwrap();
        assert!(control.mark_clear_recorded(binding));
        for _ in 0..2 {
            let mut frame = session.begin_host_frame().unwrap();
            frame
                .complete_unpainted_surfaces(dockspace::runtime::SurfaceUnavailableReason::Deferred)
                .unwrap();
            drop(frame);
            assert_eq!(control.settle_clear(false), 1);
            assert_eq!(control.settle_clear(true), 0);
            assert_eq!(control.request_for_binding(binding), Some(close));
            assert_eq!(control.parked_ingress().next().unwrap().ordinal(), ordinal);
            assert_eq!(control.pending.as_ptr(), backing);
            assert_eq!(control.owned_data_capacity_bytes(), baseline);
        }
        commit(&mut session);
        control.commit_transferred_callbacks();
        assert_eq!(control.settle_clear(false), 1);
        assert_eq!(control.request_for_binding(binding), Some(close));
        assert_eq!(control.settle_clear(true), 1);
        assert_eq!(control.settle_clear(true), 0);
        assert_eq!(control.progress().cancellations_committed(), 1);
        assert_eq!(control.request_for_binding(binding), None);
        assert!(!control.mark_request_recorded(&first, binding));
        assert!(control.needs_request_recording(&successor, binding));
        assert_eq!(control.parked_ingress().count(), 0);
        let promoted = control.callback_ingress().next().unwrap();
        assert_eq!(promoted.ordinal(), ordinal);
        assert_eq!(promoted.captured_at(), captured_at);
        assert_eq!(promoted.window_event(), Some(&successor));
        assert!(control.take_callback_ingress(ordinal + 100).is_none());
        let promoted = control.take_callback_ingress(ordinal).unwrap();
        assert_eq!(promoted.ordinal(), ordinal);
        assert_eq!(promoted.window_event(), Some(&successor));
        assert!(control.take_callback_ingress(ordinal).is_none());
        assert!(control.mark_request_recorded(&successor, binding));
        let next_close = request(&mut session, binding);
        assert_ne!(next_close, close);
        assert_eq!(control.bind_request(next_close), Ok(true));
        control.retire_binding(foreign);
        assert_eq!(control.request_for_binding(binding), Some(next_close));
        control.retire_binding(binding);
        control.retire_binding(binding);
        assert!(!control.has_pending_work());
        assert!(!control.can_configure_capacity_limit());
        assert_eq!(control.pending.as_ptr(), backing);
        assert_eq!(control.pending.capacity(), 1);
        assert_eq!(control.owned_data_capacity_bytes(), baseline);
    }
}

#[test]
fn decision_count_and_borrowed_requests_preserve_exact_ids_and_capacity_through_settlement() {
    let mut control = NativeCloseControl::with_capacity_limit(2).unwrap();
    let baseline = control.owned_data_capacity_bytes();
    let backing = control.pending.as_ptr();
    let mut requests = Vec::with_capacity(2);
    for item_count in [1, 7] {
        let (mut session, binding) = fixture(item_count);
        let close = request(&mut session, binding);
        assert!(control.observe_request(&event(binding, 41), binding));
        assert_eq!(control.bind_request(close), Ok(true));
        control.mark_policy_recorded(close);
        session
            .request_native_surface_close(close, NativeSurfaceCloseAction::CloseContent)
            .unwrap();
        let report = commit(&mut session);
        let plan = report
            .inputs()
            .iter()
            .find_map(|input| match input {
                HostInputOutcome::NativeSurfaceCloseRequested { plan, .. } => Some(plan),
                _ => None,
            })
            .unwrap();
        control.retain_decision_plan(close, plan);
        requests.push((binding, close, plan.request()));
    }
    requests.sort_unstable_by_key(|(binding, _, _)| *binding);
    assert_eq!(control.decision_count(), 2);
    assert!(
        control
            .decision_requests()
            .eq(requests.iter().map(|(_, _, id)| *id))
    );
    assert!(control.unsubmitted_requests().next().is_none());
    assert_eq!(control.unresolved_requests().next(), Some(requests[0].1));
    control.settle_decision_requests(&[]);
    assert_eq!(control.decision_count(), 2);
    control.settle_decision_requests(&[requests[0].2]);
    control.settle_decision_requests(&[requests[0].2]);
    assert_eq!(control.decision_count(), 1);
    assert_eq!(control.decision_requests().next(), Some(requests[1].2));
    assert_eq!(
        control.request_for_binding(requests[0].0),
        Some(requests[0].1)
    );
    control.retire_binding(requests[0].0);
    assert_eq!(control.decision_count(), 1);
    control.settle_decision_requests(&[requests[1].2]);
    assert_eq!(control.decision_count(), 0);
    assert!(!control.has_runnable_policy_work());
    assert!(control.references_binding(requests[1].0));
    assert_eq!(control.pending.as_ptr(), backing);
    assert_eq!(control.owned_data_capacity_bytes(), baseline);
}

#[test]
fn unlimited_preflight_reserves_backing_without_capturing_and_reuses_it_after_retirement() {
    let (_session, binding) = fixture(1);
    let (_foreign_session, foreign) = fixture(1);
    let mut control = NativeCloseControl::default();
    let initial = control.owned_data_capacity_bytes();
    assert_eq!(initial, size_of::<NativeCloseControl>());
    let capture = event(binding, 51);
    assert_eq!(
        control.preflight_capture(&capture, foreign),
        NativeCloseCaptureAdmission::Conflict
    );
    assert_eq!(control.owned_data_capacity_bytes(), initial);
    assert_eq!(
        control.preflight_capture(&capture, binding),
        NativeCloseCaptureAdmission::First
    );
    let reserved = control.owned_data_capacity_bytes();
    let backing = control.pending.as_ptr();
    assert!(reserved > initial);
    assert!(control.pending.is_empty());
    assert!(control.can_configure_capacity_limit());
    assert_eq!(control.progress().requests_retained(), 0);
    assert_eq!(
        control.capture_request(&capture, binding),
        NativeCloseCaptureAdmission::First
    );
    control.retire_binding(binding);
    assert!(!control.can_configure_capacity_limit());
    assert_eq!(
        control.preflight_capture(&event(foreign, 52), foreign),
        NativeCloseCaptureAdmission::First
    );
    assert_eq!(
        control.capture_request(&event(foreign, 52), foreign),
        NativeCloseCaptureAdmission::First
    );
    assert_eq!(control.pending.as_ptr(), backing);
    assert_eq!(control.owned_data_capacity_bytes(), reserved);
}

#[test]
fn finite_reservations_match_actual_capacity_and_overflow_preserves_existing_configuration() {
    for limit in [0, 1, 2, 7, 65] {
        let control = NativeCloseControl::with_capacity_limit(limit).unwrap();
        assert_eq!(control.entry_limit, Some(limit));
        assert_eq!(control.pending.capacity(), limit);
        assert_eq!(
            control.owned_data_capacity_bytes(),
            NativeCloseControl::reservation_bytes(limit).unwrap()
        );
        assert!(!control.can_configure_capacity_limit());
    }
    let (_session, binding) = fixture(1);
    let mut control = NativeCloseControl::with_capacity_limit(1).unwrap();
    assert!(control.observe_request(&event(binding, 61), binding));
    let before = format!("{control:?}");
    let baseline = control.owned_data_capacity_bytes();
    let backing = control.pending.as_ptr();
    for limit in [
        usize::MAX,
        usize::MAX / size_of::<(NativeSurfaceBinding, PendingNativeClose)>() + 1,
    ] {
        assert_eq!(NativeCloseControl::reservation_bytes(limit), None);
        assert_eq!(
            NativeCloseControl::with_capacity_limit(limit).unwrap_err(),
            NativeHostProtocolError::NativeReportStorageUnavailable
        );
        assert_eq!(format!("{control:?}"), before);
        assert_eq!(control.pending.as_ptr(), backing);
        assert_eq!(control.owned_data_capacity_bytes(), baseline);
    }
}
