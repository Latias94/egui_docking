//! Tokenless capture and receipt-state models, using real core close capabilities.
//!
//! No opaque fork token is fabricated here. These tests do not qualify canonical
//! token routing or real HoldClose/Veto callbacks; those need the fork fixture.

use dockspace::model::{
    DockspaceLayout, DockspaceNode, DockspaceRootLayout, DockspaceSurfaceLayout, ItemId, RootId,
    SurfaceId,
};
use dockspace::policy::DockPolicy;
use dockspace::runtime::{
    DockspaceSession, HostFrameReport, HostInputOutcome, HostWindowToken, NativeCloseState,
    NativeEffectAcknowledgement, NativePointerRoster, NativeWindowFacts, NativeWorkAreaRoster,
    SurfaceUnavailableReason,
};
use winit::event::WindowEvent;

use super::*;
use crate::mailbox::{HostRecord, NativeHostBridge};
use crate::viewport_map::NativeViewportMap;

fn commit(session: &mut DockspaceSession) -> HostFrameReport {
    let mut frame = session.begin_host_frame().unwrap();
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    frame.commit().unwrap()
}

fn fixture() -> (DockspaceSession, NativeSurfaceBinding) {
    let layout = DockspaceLayout::new([DockspaceSurfaceLayout::new(
        SurfaceId::new(1),
        DockspaceRootLayout::new(
            RootId::new(1),
            DockspaceNode::central_tabs([ItemId::new(1)]),
        ),
    )])
    .unwrap();
    let mut session = DockspaceSession::from_layout(layout, DockPolicy::default()).unwrap();
    session
        .enable_managed_native_host(NativePointerRoster::Exact(Vec::new()))
        .unwrap();
    session
        .configure_managed_native_capabilities(crate::capabilities::capabilities_for_backend(
            eframe::NativeWindowingBackend::Windows,
        ))
        .unwrap();
    session
        .register_native_root(SurfaceId::new(1), HostWindowToken::new(11))
        .unwrap();
    let report = commit(&mut session);
    let binding = report
        .inputs()
        .iter()
        .find_map(|input| match input {
            HostInputOutcome::NativeSurfaceRegistered { binding } => Some(*binding),
            _ => None,
        })
        .unwrap();
    session
        .report_managed_native_snapshot(
            [(binding, NativeWindowFacts::live())],
            NativeWorkAreaRoster::Unknown,
        )
        .unwrap();
    commit(&mut session);
    (session, binding)
}

fn event(binding: NativeSurfaceBinding, ordinal: u64, window: u64) -> NativeWindowEventRecord {
    NativeWindowEventRecord::for_test(
        ordinal,
        WindowId::from(window),
        Some(ViewportId::ROOT),
        Some(binding),
        WindowEvent::CloseRequested,
    )
}

fn core_request(
    session: &mut DockspaceSession,
    binding: NativeSurfaceBinding,
) -> NativeSurfaceCloseRequest {
    session
        .publish_native_close(binding, NativeCloseState::Requested, None)
        .unwrap();
    commit(session)
        .inputs()
        .iter()
        .find_map(|input| match input {
            HostInputOutcome::NativeCloseObservationApplied { close_requests } => {
                close_requests.first().copied()
            }
            _ => None,
        })
        .unwrap()
}

fn cancellation(
    session: &mut DockspaceSession,
    request: NativeSurfaceCloseRequest,
) -> NativeCloseEffectAcknowledgement {
    session.cancel_native_surface_close(request).unwrap();
    let mut report = commit(session);
    let mut effects = report.take_native_effects().into_iter();
    let effect = effects.next().unwrap();
    assert!(effects.next().is_none());
    let Some(NativeEffectAcknowledgement::Close(acknowledgement)) = effect.accepted() else {
        panic!("core cancellation must emit its exact close acknowledgement")
    };
    acknowledgement
}

fn parked_successor() -> (
    NativeHostBridge,
    NativeSurfaceBinding,
    NativeWindowEventRecord,
) {
    let (mut session, binding) = fixture();
    let first = event(binding, 71, 11);
    let successor = event(binding, 72, 11);
    let request = core_request(&mut session, binding);
    let acknowledgement = cancellation(&mut session, request);
    let bridge = NativeHostBridge::new(NativeViewportMap::default());
    bridge.with_close_control(|control| {
        assert!(control.observe_request(&first, binding));
        assert_eq!(control.bind_request(request), Ok(true));
        assert_eq!(
            control.begin_cancellation(request, acknowledgement),
            Some(ViewportId::ROOT),
        );
    });
    bridge.push_record(HostRecord::WindowEvent(successor.clone()));
    (bridge, binding, successor)
}

fn reserved_close_ingress() -> ReservedHostRecord {
    // Receipt-state models use a real journal reservation without fabricating a fork token.
    let (bridge, binding, _) = parked_successor();
    bridge.with_close_control(|control| {
        let ordinal = control.parked_ingress().next().unwrap().ordinal();
        assert!(control.mark_clear_recorded(binding));
        assert_eq!(control.settle_clear(true), 1);
        control.take_callback_ingress(ordinal).unwrap()
    })
}

#[test]
fn frozen_raw_admission_preserves_the_existing_close_successor_slot() {
    let (mut session, binding) = fixture();
    let first = event(binding, 71, 11);
    let successor = event(binding, 72, 11);
    let request = core_request(&mut session, binding);
    let acknowledgement = cancellation(&mut session, request);
    let bridge = NativeHostBridge::new(NativeViewportMap::default());
    bridge.with_close_control(|control| {
        assert!(control.observe_request(&first, binding));
        assert_eq!(control.bind_request(request), Ok(true));
        assert_eq!(
            control.begin_cancellation(request, acknowledgement),
            Some(ViewportId::ROOT),
        );
    });
    assert_eq!(bridge.freeze_ordinary_for_test(), 0);
    bridge.push_record(HostRecord::WindowEvent(successor.clone()));
    bridge.with_close_control(|control| {
        let parked = control.parked_ingress().next().unwrap();
        assert_eq!(parked.ordinal(), 1);
        assert_eq!(parked.window_event(), Some(&successor));
        assert_eq!(control.progress().requests_retained(), 2);
    });
    // A duplicate and a later conflicting successor cannot consume another slot.
    bridge.push_record(HostRecord::WindowEvent(successor.clone()));
    bridge.push_record(HostRecord::WindowEvent(event(binding, 73, 11)));
    bridge.with_close_control(|control| {
        assert_eq!(control.parked_ingress().count(), 1);
        assert_eq!(control.callback_ingress().count(), 0);
        assert_eq!(control.progress().requests_retained(), 2);
        let parked = control.parked_ingress().next().unwrap();
        assert_eq!(parked.ordinal(), 1);
        assert_eq!(parked.window_event(), Some(&successor));
    });
}

#[test]
fn capture_is_exact_and_cannot_bind_core_before_requested_was_recorded() {
    let (mut session, binding) = fixture();
    let (_, foreign) = fixture();
    let first = event(binding, 11, 11);
    let newer = event(binding, 12, 11);
    let mut control = NativeCloseControl::default();
    assert_eq!(
        control.capture_request(&first, foreign),
        NativeCloseCaptureAdmission::Conflict
    );
    assert!(!control.has_pending_work());
    assert_eq!(
        control.capture_request(&first, binding),
        NativeCloseCaptureAdmission::First
    );
    assert_eq!(
        control.capture_request(&first, binding),
        NativeCloseCaptureAdmission::Duplicate
    );
    assert_eq!(
        control.capture_request(&newer, binding),
        NativeCloseCaptureAdmission::Conflict
    );
    assert!(control.references_binding(binding));
    assert!(!control.has_runnable_policy_work());
    assert!(control.needs_request_recording(&first, binding));
    let request = core_request(&mut session, binding);
    assert_eq!(control.bind_request(request), Ok(false));
    assert!(!control.mark_request_recorded(&newer, binding));
    assert!(!control.mark_request_recorded(&first, foreign));
    assert!(control.needs_request_recording(&first, binding));
    assert!(control.mark_request_recorded(&first, binding));
    assert!(!control.mark_request_recorded(&first, binding));
    assert_eq!(control.bind_request(request), Ok(true));
    assert_eq!(control.bind_request(request), Ok(true));
    assert_eq!(
        control.unsubmitted_requests().collect::<Vec<_>>(),
        [request]
    );
}

#[test]
fn decision_request_settlement_releases_only_the_exact_completed_id() {
    let mut control = NativeCloseControl::default();
    let mut requests = Vec::new();
    for ordinal in [11, 12] {
        let (mut session, binding) = fixture();
        let request = core_request(&mut session, binding);
        assert!(control.observe_request(&event(binding, ordinal, 11), binding));
        assert_eq!(control.bind_request(request), Ok(true));
        control.mark_policy_recorded(request);
        session
            .request_native_surface_close(request, NativeSurfaceCloseAction::CloseContent)
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
        control.retain_decision_plan(request, plan);
        requests.push((binding, request, plan.request()));
    }
    assert_eq!(control.decision_requests().count(), 2);
    let [
        (first_binding, first_close, first),
        (second_binding, second_close, second),
    ] = requests.as_slice()
    else {
        panic!("both exact native closes must retain a decision request")
    };
    assert_ne!(first, second);
    control.settle_decision_requests(&[]);
    assert_eq!(control.decision_requests().count(), 2);
    control.settle_decision_requests(&[*first]);
    assert_eq!(control.decision_requests().collect::<Vec<_>>(), [*second]);
    control.settle_decision_requests(&[*first]);
    assert_eq!(control.decision_requests().collect::<Vec<_>>(), [*second]);
    assert_eq!(
        control.request_for_binding(*first_binding),
        Some(*first_close)
    );
    assert_eq!(
        control.request_for_binding(*second_binding),
        Some(*second_close)
    );
    control.retire_binding(*first_binding);
    assert_eq!(control.decision_requests().collect::<Vec<_>>(), [*second]);
    control.settle_decision_requests(&[*second]);
    assert!(control.decision_requests().next().is_none());
    assert!(!control.has_runnable_policy_work());
    assert!(control.references_binding(*second_binding));
    control.retire_binding(*second_binding);
    assert!(!control.has_pending_work());
}

#[test]
fn one_successor_waits_for_received_transferred_and_committed_clear() {
    for callback_already_received in [false, true] {
        let (mut session, binding) = fixture();
        let first = event(binding, 21, 11);
        let successor = event(binding, 22, 11);
        let third = event(binding, 23, 11);
        let wrong_window = event(binding, 22, 12);
        let mut control = NativeCloseControl::default();
        assert!(control.observe_request(&first, binding));
        let request = core_request(&mut session, binding);
        assert_eq!(control.bind_request(request), Ok(true));
        assert_eq!(
            control.begin_cancellation(request, cancellation(&mut session, request)),
            Some(ViewportId::ROOT)
        );

        // Exercise the receipt state model without minting a fork token. The
        // production reserve/receive methods additionally require that token.
        let mut receipt = CancellationReceipt::new();
        if callback_already_received {
            assert_eq!(
                receipt.receive(reserved_close_ingress),
                NativeCloseCallbackAdmission::First,
            );
        }
        control
            .pending_for_binding_mut(binding)
            .unwrap()
            .cancellation_receipt = Some(receipt);
        assert_eq!(
            control.capture_request(&wrong_window, binding),
            NativeCloseCaptureAdmission::Conflict
        );
        assert_eq!(
            control.capture_request(&successor, binding),
            NativeCloseCaptureAdmission::First
        );
        assert_eq!(
            control.capture_request(&successor, binding),
            NativeCloseCaptureAdmission::Duplicate
        );
        assert_eq!(
            control.capture_request(&third, binding),
            NativeCloseCaptureAdmission::Conflict
        );
        assert!(!control.needs_request_recording(&successor, binding));
        assert!(!control.mark_request_recorded(&successor, binding));
        assert_eq!(control.request_for_binding(binding), Some(request));
        assert!(!control.mark_clear_recorded(binding));
        control.commit_transferred_callbacks();
        assert_eq!(control.settle_clear(true), 0);

        let receipt = control
            .pending_for_binding_mut(binding)
            .unwrap()
            .cancellation_receipt
            .as_mut()
            .unwrap();
        if !callback_already_received {
            assert_eq!(
                receipt.receive(reserved_close_ingress),
                NativeCloseCallbackAdmission::First,
            );
        }
        let ordinal = control.callback_ingress().next().unwrap().ordinal();
        assert!(control.take_callback_ingress(ordinal + 1).is_none());
        assert!(control.take_callback_ingress(ordinal).is_some());
        assert!(control.take_callback_ingress(ordinal).is_none());
        assert_eq!(control.callback_ingress().count(), 0);
        assert!(control.mark_clear_recorded(binding));
        assert_eq!(control.settle_clear(false), 1);
        assert_eq!(control.settle_clear(true), 0);
        assert!(!control.needs_request_recording(&successor, binding));
        control.commit_transferred_callbacks();
        assert_eq!(control.settle_clear(false), 1);
        assert_eq!(control.settle_clear(true), 1);

        assert!(control.references_binding(binding));
        assert_eq!(control.request_for_binding(binding), None);
        assert_eq!(control.bind_request(request), Ok(false));
        assert!(!control.needs_request_recording(&first, binding));
        assert!(control.needs_request_recording(&successor, binding));
        assert_eq!(control.settle_clear(true), 0);
        assert_eq!(
            control.capture_request(&first, binding),
            NativeCloseCaptureAdmission::Conflict
        );
        assert_eq!(
            control.capture_request(&successor, binding),
            NativeCloseCaptureAdmission::Duplicate
        );
        assert!(control.mark_request_recorded(&successor, binding));
        control.retire_binding(binding);
        assert!(!control.has_pending_work());
    }
}

#[test]
fn cancellation_receipt_is_once_only_and_neither_receive_nor_transfer_is_commit() {
    let ingress = reserved_close_ingress();
    let ordinal = ingress.ordinal();
    let captured_at = ingress.captured_at();
    let mut receipt = CancellationReceipt::new();
    assert!(receipt.take_ingress(ordinal).is_none());
    receipt.commit_transferred();
    assert_eq!(receipt.state, CancellationReceiptState::Awaiting);
    assert_eq!(
        receipt.receive(|| ingress),
        NativeCloseCallbackAdmission::First,
    );
    receipt.commit_transferred();
    assert_eq!(receipt.state, CancellationReceiptState::Received);
    assert_eq!(
        receipt.receive(|| panic!("a received duplicate cannot reserve ingress")),
        NativeCloseCallbackAdmission::Duplicate,
    );
    assert!(receipt.take_ingress(ordinal + 1).is_none());
    assert_eq!(receipt.state, CancellationReceiptState::Received);
    assert_eq!(receipt.ingress.as_ref().unwrap().captured_at(), captured_at);
    let transferred = receipt.take_ingress(ordinal).unwrap();
    assert_eq!(transferred.ordinal(), ordinal);
    assert_eq!(transferred.captured_at(), captured_at);
    assert!(receipt.ingress.is_none());
    assert_eq!(receipt.state, CancellationReceiptState::Transferred);
    assert_eq!(
        receipt.receive(|| panic!("a transferred duplicate cannot reserve ingress")),
        NativeCloseCallbackAdmission::Duplicate,
    );
    assert!(receipt.take_ingress(ordinal).is_none());
    receipt.commit_transferred();
    assert_eq!(receipt.state, CancellationReceiptState::Committed);
    assert_eq!(
        receipt.receive(|| panic!("a committed duplicate cannot reserve ingress")),
        NativeCloseCallbackAdmission::Duplicate,
    );
}

#[test]
fn promoted_successor_keeps_its_owner_and_capture_until_exact_take() {
    let (bridge, binding, successor) = parked_successor();
    bridge.with_close_control(|control| {
        let parked = control.parked_ingress().next().unwrap();
        let ordinal = parked.ordinal();
        let captured_at = parked.captured_at();
        assert_eq!(parked.window_event(), Some(&successor));
        assert_eq!(control.parked_ingress().count(), 1);
        assert_eq!(control.deferred_ingress().count(), 1);
        assert_eq!(control.callback_ingress().count(), 0);
        assert!(control.take_callback_ingress(ordinal).is_none());
        assert_eq!(control.settle_clear(true), 0);

        assert!(control.mark_clear_recorded(binding));
        assert_eq!(control.settle_clear(false), 1);
        assert_eq!(control.parked_ingress().count(), 1);
        assert_eq!(control.callback_ingress().count(), 0);
        assert_eq!(control.settle_clear(true), 1);
        assert_eq!(control.parked_ingress().count(), 0);
        assert_eq!(control.deferred_ingress().count(), 1);
        let promoted = control.callback_ingress().next().unwrap();
        assert_eq!(promoted.ordinal(), ordinal);
        assert_eq!(promoted.captured_at(), captured_at);
        assert_eq!(promoted.window_event(), Some(&successor));

        assert!(control.take_callback_ingress(ordinal + 1).is_none());
        control.commit_transferred_callbacks();
        assert_eq!(control.callback_ingress().count(), 1);
        assert!(control.needs_request_recording(&successor, binding));
        let taken = control.take_callback_ingress(ordinal).unwrap();
        assert_eq!(taken.ordinal(), ordinal);
        assert_eq!(taken.captured_at(), captured_at);
        assert_eq!(taken.window_event(), Some(&successor));
        assert!(control.take_callback_ingress(ordinal).is_none());
        assert_eq!(control.callback_ingress().count(), 0);
        assert_eq!(control.deferred_ingress().count(), 0);
        assert!(
            control
                .pending_for_binding(binding)
                .unwrap()
                .cancellation_receipt
                .is_none()
        );
        assert!(control.needs_request_recording(&successor, binding));
        assert!(control.references_binding(binding));
    });
}

#[test]
fn legacy_observe_uses_the_same_owner_and_accepted_close_cannot_capture_a_successor() {
    let (mut session, binding) = fixture();
    let first = event(binding, 31, 11);
    let mut control = NativeCloseControl::default();
    let request = core_request(&mut session, binding);
    assert!(control.observe_request(&first, binding));
    assert!(!control.observe_request(&first, binding));
    assert!(!control.needs_request_recording(&first, binding));
    assert_eq!(control.bind_request(request), Ok(true));
    assert!(control.accept_close(request));
    assert!(!control.accept_close(request));
    assert_eq!(
        control.capture_request(&event(binding, 32, 11), binding),
        NativeCloseCaptureAdmission::Conflict
    );
    assert_eq!(control.request_for_binding(binding), Some(request));
    control.retire_binding(binding);
    assert!(!control.references_binding(binding));
}
