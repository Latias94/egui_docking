//! Tokenless adapter models for retained close ingress and successor ordering.
//!
//! `for_test` carries no canonical fork close token. These tests use real core
//! requests and cancellation acknowledgements, but do not qualify fork token
//! routing, HoldClose/Veto delivery, or real platform cancellation callbacks.

use dockspace::runtime::{
    NativeDesktopPointerLocation, NativeDesktopPosition, NativePointerEvent, NativePointerHover,
    NativePointerId, NativePointerOwner, NativeSurfaceCloseRequest,
};

use super::close::{begin_model_cancellation, observe_model_close};
use super::*;

fn close_event(
    binding: NativeSurfaceBinding,
    viewport: ViewportId,
    window: WindowId,
    ordinal: u64,
) -> NativeWindowEventRecord {
    let event = NativeWindowEventRecord::for_test(
        ordinal,
        window,
        Some(viewport),
        Some(binding),
        WindowEvent::CloseRequested,
    );
    assert!(event.close_request().is_none());
    event
}

fn deferred_events(native: &NativeCoordinator) -> Vec<NativeWindowEventRecord> {
    native.bridge.with_close_control(|control| {
        control
            .deferred_ingress()
            .map(|ingress| {
                ingress
                    .window_event()
                    .expect("a deferred close retains its original window event")
                    .clone()
            })
            .collect()
    })
}

fn close_requests(report: &HostFrameReport) -> Vec<NativeSurfaceCloseRequest> {
    report
        .inputs()
        .iter()
        .filter_map(|input| match input {
            HostInputOutcome::NativeCloseObservationApplied { close_requests }
            | HostInputOutcome::NativePlatformSnapshotApplied { close_requests } => {
                Some(close_requests.as_slice())
            }
            _ => None,
        })
        .flatten()
        .copied()
        .collect()
}

fn commit_frame(native: &mut NativeCoordinator, budget: &mut NativeInputBudget) -> HostFrameReport {
    let mut frame = native
        .poll_host_frame(budget, |_| NativeReceiverAnswer::Unknown)
        .expect("the retained native input polls")
        .expect("the input prefix can seal");
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .expect("the model settles every surface");
    frame.commit().expect("the model input prefix commits")
}

fn unknown_move() -> NativePointerInput {
    NativePointerInput::new(
        NativePointerId::new(1),
        NativePointerEvent::Moved,
        NativeDesktopPointerLocation::new(
            NativeDesktopPosition::Unknown,
            NativePointerHover::Unknown,
            None,
        ),
        NativePointerOwner::Unknown,
        NativePointerOwner::Unknown,
    )
}

#[test]
fn close_ingress_owns_one_capture_before_reduction_and_survives_dropped_frames() {
    let mut native = coordinator();
    let (binding, _) = register_roots(&mut native);
    let viewport = ViewportId::ROOT;
    let window = WindowId::from(91);
    native
        .bind_viewport(viewport, window, binding)
        .expect("the close fixture binds its exact window");
    let event = close_event(binding, viewport, window, 51);
    let recorded_before = native.session.native_input_recorded_count();

    native
        .bridge
        .push_record(HostRecord::WindowEvent(event.clone()));
    assert!(native.bridge.with_close_control(|control| {
        control.references_binding(binding)
            && control.needs_request_recording(&event, binding)
            && control.request_for_binding(binding).is_none()
    }));
    assert_eq!(native.close_progress().requests_retained(), 1);
    assert_eq!(
        native.session.native_input_recorded_count(),
        recorded_before
    );
    native
        .bridge
        .push_record(HostRecord::WindowEvent(event.clone()));
    assert_eq!(native.close_progress().requests_retained(), 1);
    assert_eq!(native.next_window_event().unwrap(), Some(event.clone()));

    assert_eq!(
        native
            .reduce_callback_prefix(&egui::Context::default(), &mut test_budget())
            .unwrap()
            .unwrap()
            .get(),
        1
    );
    assert!(native.next_window_event().unwrap().is_none());
    assert_eq!(
        native.session.native_input_recorded_count(),
        recorded_before + 1
    );

    for _ in 0..2 {
        let frame = native
            .begin_host_frame(|_| NativeReceiverAnswer::Unknown)
            .expect("the same Requested observation is retryable");
        drop(frame);
        assert!(native.bridge.callback_boundary_pending());
        assert!(native.bridge.with_close_control(|control| {
            control.references_binding(binding)
                && !control.needs_request_recording(&event, binding)
                && control.request_for_binding(binding).is_none()
        }));
        native
            .bridge
            .push_record(HostRecord::WindowEvent(event.clone()));
        assert!(native.next_window_event().unwrap().is_none());
        assert!(
            native
                .reduce_callback_prefix(&egui::Context::default(), &mut test_budget())
                .unwrap()
                .is_none()
        );
        assert_eq!(native.close_progress().requests_retained(), 1);
        assert_eq!(
            native.session.native_input_recorded_count(),
            recorded_before + 1
        );
    }

    let report = commit_frame(&mut native, &mut test_budget());
    assert!(!report.native_input_pending());
    let requests = close_requests(&report);
    assert_eq!(requests.len(), 1);
    let close = requests[0];
    assert_eq!(close.binding(), binding);
    assert!(native.settle_close_control_report(&report).unwrap());
    assert_eq!(
        native
            .bridge
            .with_close_control(|control| control.request_for_binding(binding)),
        Some(close)
    );

    native.bridge.push_record(HostRecord::WindowEvent(event));
    assert!(native.next_window_event().unwrap().is_none());
    assert!(
        native
            .reduce_callback_prefix(&egui::Context::default(), &mut test_budget())
            .unwrap()
            .is_none()
    );
    let replay = commit_frame(&mut native, &mut test_budget());
    assert!(close_requests(&replay).is_empty());
    assert_eq!(
        native
            .bridge
            .with_close_control(|control| control.request_for_binding(binding)),
        Some(close)
    );
    assert_eq!(native.close_progress().requests_retained(), 1);
    assert_eq!(native.close_progress().cancellations_committed(), 0);
    assert_eq!(
        native.session.native_input_recorded_count(),
        recorded_before + 1
    );
}

#[test]
fn cancelling_close_defers_one_successor_until_full_clear_commit_and_report_settlement() {
    let mut native = coordinator();
    let (root, binding) = register_roots(&mut native);
    let viewport = ViewportId::from_hash_of("held-close-successor-model");
    let window = WindowId::from(92);
    let close = observe_model_close(&mut native, binding, [root, binding], viewport, window, 61);
    let acknowledgement = begin_model_cancellation(&mut native, close, viewport);
    let successor = close_event(binding, viewport, window, 63);

    native
        .bridge
        .push_record(HostRecord::WindowEvent(close_event(
            binding,
            viewport,
            WindowId::from(93),
            62,
        )));
    assert!(deferred_events(&native).is_empty());
    assert!(native.next_window_event().unwrap().is_none());
    assert_eq!(native.close_progress().requests_retained(), 1);

    for _ in 0..2 {
        native
            .bridge
            .push_record(HostRecord::WindowEvent(successor.clone()));
    }
    for (candidate_window, ordinal) in [(WindowId::from(93), 64), (window, 65)] {
        native
            .bridge
            .push_record(HostRecord::WindowEvent(close_event(
                binding,
                viewport,
                candidate_window,
                ordinal,
            )));
    }
    assert_eq!(deferred_events(&native), [successor.clone()]);
    assert_eq!(native.close_progress().requests_retained(), 2);
    assert!(native.next_window_event().unwrap().is_none());
    assert_eq!(
        native
            .bridge
            .with_close_control(|control| control.request_for_binding(binding)),
        Some(close)
    );

    native
        .bridge
        .push_record(HostRecord::WindowEvent(NativeWindowEventRecord::for_test(
            66,
            window,
            Some(viewport),
            Some(binding),
            WindowEvent::RedrawRequested,
        )));
    assert_eq!(native.next_window_event().unwrap().unwrap().ordinal(), 66);
    assert!(
        native
            .reduce_callback_prefix(&egui::Context::default(), &mut test_budget())
            .unwrap()
            .is_some(),
        "the deferred successor cannot block unrelated raw callbacks"
    );
    let unrelated = commit_frame(&mut native, &mut test_budget());
    assert!(close_requests(&unrelated).is_empty());
    assert!(!native.settle_close_control_report(&unrelated).unwrap());
    assert_eq!(deferred_events(&native), [successor.clone()]);

    // Model only: the real core ACK authorizes Clear; no fork Veto is forged.
    native
        .session
        .publish_native_close(binding, NativeCloseState::Clear, Some(acknowledgement))
        .expect("the model publishes the exact core cancellation acknowledgement");
    assert!(
        native
            .bridge
            .with_close_control(|control| control.mark_clear_recorded(binding))
    );
    native
        .session
        .record_native_pointer_sequence((0..2).map(|_| unknown_move()))
        .expect("independent input keeps a suffix after Clear");

    // Leave one record of allowance so Clear appears in an incomplete prefix.
    let mut budget = test_budget();
    budget.complete_unsequenced_records(budget.remaining_records() - 1);
    let frame = native
        .poll_host_frame(&mut budget, |_| NativeReceiverAnswer::Unknown)
        .unwrap()
        .unwrap();
    assert!(frame.native_input_pending());
    drop(frame);
    assert_eq!(deferred_events(&native), [successor.clone()]);
    assert!(native.next_window_event().unwrap().is_none());
    assert_eq!(native.close_progress().cancellations_committed(), 0);
    assert_eq!(
        native
            .bridge
            .with_close_control(|control| control.request_for_binding(binding)),
        Some(close)
    );

    let mut budget = test_budget();
    budget.complete_unsequenced_records(budget.remaining_records() - 1);
    let partial = commit_frame(&mut native, &mut budget);
    assert!(partial.native_input_pending());
    assert!(partial.inputs().iter().any(|input| matches!(
        input,
        HostInputOutcome::NativeCloseObservationApplied { close_requests }
            if close_requests.is_empty()
    )));
    assert!(!native.settle_close_control_report(&partial).unwrap());
    assert_eq!(deferred_events(&native), [successor.clone()]);
    assert!(native.next_window_event().unwrap().is_none());
    assert_eq!(native.close_progress().cancellations_committed(), 0);
    assert_eq!(
        native
            .bridge
            .with_close_control(|control| control.request_for_binding(binding)),
        Some(close)
    );

    let frame = native
        .begin_host_frame(|_| NativeReceiverAnswer::Unknown)
        .expect("the remaining native interval can retry");
    assert!(!frame.native_input_pending());
    drop(frame);
    assert_eq!(deferred_events(&native), [successor.clone()]);
    assert!(native.next_window_event().unwrap().is_none());
    assert_eq!(native.close_progress().cancellations_committed(), 0);

    let complete = commit_frame(&mut native, &mut test_budget());
    assert!(!complete.native_input_pending());
    assert!(!complete.inputs().iter().any(|input| matches!(
        input,
        HostInputOutcome::NativeCloseObservationApplied { .. }
    )));
    assert_eq!(deferred_events(&native), [successor.clone()]);
    assert!(native.next_window_event().unwrap().is_none());
    assert_eq!(native.close_progress().cancellations_committed(), 0);
    assert!(native.settle_close_control_report(&complete).unwrap());
    assert!(!native.settle_close_control_report(&complete).unwrap());
    assert!(deferred_events(&native).is_empty());
    assert_eq!(native.close_progress().cancellations_committed(), 1);
    assert_eq!(
        native
            .bridge
            .with_close_control(|control| control.request_for_binding(binding)),
        None
    );
    assert_eq!(native.next_window_event().unwrap(), Some(successor.clone()));

    let recorded_before = native.session.native_input_recorded_count();
    native
        .bridge
        .push_record(HostRecord::WindowEvent(successor.clone()));
    assert_eq!(native.close_progress().requests_retained(), 2);
    assert_eq!(
        native
            .reduce_callback_prefix(&egui::Context::default(), &mut test_budget())
            .unwrap()
            .unwrap()
            .get(),
        1
    );
    assert!(native.next_window_event().unwrap().is_none());
    assert_eq!(
        native.session.native_input_recorded_count(),
        recorded_before + 1
    );
    let report = commit_frame(&mut native, &mut test_budget());
    let requests = close_requests(&report);
    assert_eq!(requests.len(), 1);
    let next_close = requests[0];
    assert_eq!(next_close.binding(), binding);
    assert_ne!(next_close, close);
    assert!(native.settle_close_control_report(&report).unwrap());

    for _ in 0..2 {
        native
            .bridge
            .push_record(HostRecord::WindowEvent(successor.clone()));
    }
    assert!(native.next_window_event().unwrap().is_none());
    assert!(
        native
            .reduce_callback_prefix(&egui::Context::default(), &mut test_budget())
            .unwrap()
            .is_none()
    );
    assert!(deferred_events(&native).is_empty());
    assert_eq!(native.close_progress().requests_retained(), 2);
    assert_eq!(native.close_progress().cancellations_committed(), 1);
    assert_eq!(
        native.session.native_input_recorded_count(),
        recorded_before + 1
    );
    assert_eq!(
        native
            .bridge
            .with_close_control(|control| control.request_for_binding(binding)),
        Some(next_close)
    );
    let replay = commit_frame(&mut native, &mut test_budget());
    assert!(close_requests(&replay).is_empty());
}
