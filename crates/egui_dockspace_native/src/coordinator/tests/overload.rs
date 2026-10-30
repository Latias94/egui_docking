//! Fixed-boundary adapter regressions using real reservations and core frames.
//!
//! Close fixtures are tokenless owner models. They do not qualify canonical
//! fork close tokens, OS callback delivery, or real-window HoldClose/Veto routing.

use dockspace::runtime::{
    NativeDesktopPointerLocation, NativeDesktopPosition, NativePointerEvent, NativePointerHover,
    NativePointerId, NativePointerOwner,
};
use winit::dpi::PhysicalPosition;
use winit::event::{DeviceId, ElementState, MouseButton, PointerEventFacts, PointerWindowRoute};

use super::close::{begin_model_cancellation, observe_model_close};
use super::*;
use crate::input_control::NativePointerPassthroughRecord;
use crate::mailbox::NativeDispatch;

fn pointer_record(
    native: &NativeCoordinator,
    ordinal: u64,
    pressed: bool,
) -> NativeWindowEventRecord {
    let window = WindowId::from(11);
    let position = PhysicalPosition::new(40.0 + ordinal as f64, 70.0);
    let facts = PointerEventFacts {
        geometry: None,
        surface_position: Some(position),
        desktop_position: Some(position),
        modifiers: None,
        hover: PointerWindowRoute::Window(window),
        capture: PointerWindowRoute::Window(window),
    };
    let event = if pressed {
        WindowEvent::MouseInput {
            device_id: DeviceId::dummy(),
            state: ElementState::Pressed,
            button: MouseButton::Left,
            facts,
        }
    } else {
        WindowEvent::CursorMoved {
            device_id: DeviceId::dummy(),
            position,
            facts,
        }
    };
    native.bridge.with_viewports(|routes| {
        NativeWindowEventRecord::for_test_snapshot(
            ordinal,
            window,
            Some(ViewportId::ROOT),
            event,
            routes,
        )
    })
}

fn commit_poll(native: &mut NativeCoordinator) -> HostFrameReport {
    let mut budget = test_budget();
    let mut frame = native
        .poll_host_frame(&mut budget, |_| NativeReceiverAnswer::Unknown)
        .expect("the retained boundary polls")
        .expect("the retained boundary can seal");
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .expect("the control frame settles every surface");
    frame.commit().expect("the exact boundary commits")
}

fn cancellation_count(report: &HostFrameReport) -> usize {
    report
        .inputs()
        .iter()
        .filter(|outcome| matches!(outcome, HostInputOutcome::NativeInputCancelledForOverload))
        .count()
}

fn assert_fatal_overload_reaches_quiescence(fatal_before_cancel: bool) {
    let mut native = coordinator();
    let (root, child) = register_roots(&mut native);
    let child_viewport = ViewportId::from_hash_of("fatal-overload-child");
    native
        .bind_viewport(ViewportId::ROOT, WindowId::from(11), root)
        .unwrap();
    native
        .bind_viewport(child_viewport, WindowId::from(22), child)
        .unwrap();
    let roster = live_roster([root, child]);
    native.bridge.push_viewport_roster(roster.clone());
    assert!(native.reduce_next_viewport_roster().unwrap());
    let baseline = commit_poll(&mut native);
    assert!(native.settle_host_frame_report(&baseline).unwrap());
    assert!(native.is_quiescent());

    let boundary = native.bridge.freeze_ordinary_for_test();
    assert_eq!(
        native.bridge.admission_dispatch(),
        NativeDispatch::ContinueBoundary
    );
    let prelude = {
        let mut budget = test_budget();
        let mut frame = native
            .poll_host_frame(&mut budget, |_| NativeReceiverAnswer::Unknown)
            .expect("the frozen prelude polls")
            .expect("the frozen prelude can seal");
        assert!(
            frame.native_input_pending(),
            "Draining keeps product UI inert before cancellation is submitted"
        );
        frame
            .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
            .expect("the frozen prelude settles every surface");
        frame.commit().expect("the frozen prelude commits")
    };
    assert_eq!(cancellation_count(&prelude), 0);
    assert!(
        !prelude.native_input_pending(),
        "the core report releases a complete boundary despite the Draining UI gate"
    );
    assert_eq!(
        native.bridge.admission_dispatch(),
        NativeDispatch::Cancel { boundary }
    );
    if fatal_before_cancel {
        assert!(native.quarantine_after_fatal().is_empty());
        native.bridge.push_terminal_viewport_roster(roster.clone());
        assert!(native.bridge.front_viewport_roster().is_none());
        assert!(native.bridge.has_pending_callback_records());
    }

    let cancelled = commit_poll(&mut native);
    assert_eq!(cancellation_count(&cancelled), 1);
    assert!(!cancelled.native_input_pending());
    // Do not dispatch between this commit and fatal: that would resume Active
    // admission first and fail to exercise Recovering under Quarantined mode.
    if !fatal_before_cancel {
        assert!(native.quarantine_after_fatal().is_empty());
        native.bridge.push_terminal_viewport_roster(roster);
    }
    assert!(!native.is_quiescent());
    assert_eq!(
        native.bridge.admission_dispatch(),
        NativeDispatch::Replay { through: None }
    );
    assert!(native.reduce_next_viewport_roster().unwrap());
    let terminal = commit_poll(&mut native);
    assert_eq!(cancellation_count(&terminal), 0);
    assert!(terminal.inputs().iter().any(|outcome| matches!(
        outcome,
        HostInputOutcome::NativePlatformSnapshotApplied { .. }
    )));
    assert!(native.bridge.front_viewport_roster().is_none());
    assert!(
        !native.is_quiescent(),
        "full commit still awaits the terminal roster's exact snapshot settlement"
    );
    assert!(native.settle_host_frame_report(&terminal).unwrap());
    assert_eq!(native.bridge.admission_dispatch(), NativeDispatch::Idle);
    assert!(!native.bridge.has_pending_coordinator_work());
    assert!(native.is_quiescent());

    assert!(
        !native
            .bridge
            .reserve_create_for_test(child_viewport, child, native_rect()),
        "finishing overload recovery must not reopen ordinary work after fatal"
    );
    // This unowned callback has a real fork queue token, without a protected
    // command reservation; it must still pass through ordinary admission.
    let token = eframe::queue_native_viewport_focus(&egui::Context::default(), ViewportId::ROOT);
    native.bridge.push_record(HostRecord::ViewportFocus(
        crate::focus_control::NativeViewportFocusRecord::for_test(
            token,
            ViewportId::ROOT,
            WindowId::from(11),
            Some(root),
            eframe::NativeViewportFocusStatus::Requested,
        ),
    ));
    assert!(!native.bridge.has_pending_callback_records());
    assert_eq!(native.bridge.admission_dispatch(), NativeDispatch::Idle);
    assert!(native.is_quiescent());
    assert!(native.take_callback_error().is_none());
}

#[test]
fn fatal_before_overload_cancellation_commits_then_reaches_quiescence() {
    assert_fatal_overload_reaches_quiescence(true);
}

#[test]
fn fatal_after_overload_cancellation_commits_then_reaches_quiescence() {
    assert_fatal_overload_reaches_quiescence(false);
}

#[test]
fn derived_effect_failure_waits_for_retryable_overload_cancellation() {
    let mut native = coordinator();
    let (root, binding) = register_roots(&mut native);
    let close = observe_model_close(
        &mut native,
        binding,
        [root, binding],
        ViewportId::from_hash_of("overload-derived-close-failure"),
        WindowId::from(93),
        63,
    );
    native
        .session
        .cancel_native_surface_close(close)
        .expect("the real close cancellation records before freezing");
    let boundary = native.bridge.freeze_ordinary_for_test();
    assert_eq!(
        native.bridge.admission_dispatch(),
        NativeDispatch::ContinueBoundary
    );
    let mut prelude = commit_poll(&mut native);
    assert_eq!(cancellation_count(&prelude), 0);
    assert!(!prelude.native_input_pending());
    let mut effects = prelude.take_native_effects().into_iter();
    let request = effects.next().expect("the core emits one close effect");
    assert!(effects.next().is_none());
    assert_eq!(
        request.operation(),
        &NativeEffectOperation::ResolveNativeClose {
            close,
            resolution: NativeCloseDisposition::Cancel,
        }
    );

    let recorded = native.session.native_input_recorded_count();
    assert!(native.pending_effect_results.is_empty());
    native
        .submit_failed_effect(request, NativeDispatchFailure::AdapterRejected)
        .expect("the derived failure remains owned while Draining");
    assert_eq!(native.session.native_input_recorded_count(), recorded);
    assert_eq!(native.pending_effect_results.len(), 1);
    assert!(!native.try_report_pending_effect_results().unwrap());
    assert_eq!(
        native.bridge.admission_dispatch(),
        NativeDispatch::Cancel { boundary }
    );

    {
        let mut budget = test_budget();
        let mut frame = native
            .poll_host_frame(&mut budget, |_| NativeReceiverAnswer::Unknown)
            .expect("the overload cancellation polls")
            .expect("the overload cancellation can seal");
        assert!(frame.native_input_pending());
        frame
            .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
            .expect("the dropped cancellation settles every surface");
        drop(frame);
    }
    assert_eq!(
        native.bridge.admission_dispatch(),
        NativeDispatch::Cancel { boundary }
    );
    assert_eq!(native.session.native_input_recorded_count(), recorded);
    assert_eq!(native.pending_effect_results.len(), 1);

    let cancelled = commit_poll(&mut native);
    assert_eq!(cancellation_count(&cancelled), 1);
    assert_eq!(native.session.native_input_recorded_count(), recorded);
    assert_eq!(native.pending_effect_results.len(), 1);
    assert!(native.try_report_pending_effect_results().unwrap());
    assert_eq!(native.session.native_input_recorded_count(), recorded + 1);
    assert!(native.pending_effect_results.is_empty());

    let settled = commit_poll(&mut native);
    assert_eq!(cancellation_count(&settled), 0);
    assert!(!settled.native_input_pending());
    assert!(!native.try_report_pending_effect_results().unwrap());
    assert_eq!(native.session.native_input_recorded_count(), recorded + 1);
    assert!(native.pending_effect_results.is_empty());
}

#[test]
fn frozen_cursor_prefix_commits_before_retryable_cancel_and_post_boundary_command() {
    let mut native = coordinator();
    let (binding, _) = register_roots(&mut native);
    let viewport = ViewportId::ROOT;
    let window = WindowId::from(11);
    native.bind_viewport(viewport, window, binding).unwrap();
    native
        .bridge
        .push_record(HostRecord::WindowEvent(pointer_record(&native, 1, true)));
    assert_eq!(
        native
            .reduce_callback_prefix(&egui::Context::default(), &mut test_budget())
            .unwrap()
            .unwrap()
            .get(),
        1,
    );
    assert_eq!(cancellation_count(&commit_poll(&mut native)), 0);
    assert!(!native.pointer_translator.can_coalesce_idle_cursor_moves());

    let token = pointer_passthrough_token(viewport, true);
    assert!(
        native
            .bridge
            .reserve_pointer_callback(viewport, binding, window, token, true)
    );
    for ordinal in 2..=4 {
        native
            .bridge
            .push_record(HostRecord::WindowEvent(pointer_record(
                &native, ordinal, false,
            )));
    }
    let boundary = native.bridge.freeze_ordinary_for_test();
    let callback = NativePointerPassthroughRecord::for_test(
        token,
        viewport,
        window,
        Some(binding),
        true,
        NativeViewportPointerPassthroughStatus::Applied,
    );
    native
        .bridge
        .push_record(HostRecord::ViewportPointerPassthrough(callback));
    assert_eq!(
        native.bridge.admission_dispatch(),
        NativeDispatch::ContinueBoundary,
    );
    assert_eq!(cancellation_count(&commit_poll(&mut native)), 0);
    assert_eq!(
        native.bridge.admission_dispatch(),
        NativeDispatch::Replay {
            through: Some(boundary)
        },
    );
    assert_eq!(
        native
            .reduce_callback_prefix(&egui::Context::default(), &mut test_budget())
            .unwrap()
            .unwrap()
            .get(),
        3,
        "all three accepted cursors drain before cancellation",
    );
    assert!(native.bridge.front_viewport_pointer_passthrough().is_none());
    assert!(native.bridge.has_pending_callback_records());
    assert_eq!(
        native.bridge.admission_dispatch(),
        NativeDispatch::ContinueBoundary
    );
    let prefix = {
        let mut budget = test_budget();
        let mut frame = native
            .poll_host_frame(&mut budget, |_| NativeReceiverAnswer::Unknown)
            .expect("the frozen cursor prefix polls")
            .expect("the frozen cursor prefix can seal");
        assert!(
            frame.native_input_pending(),
            "Draining keeps product UI inert while the final accepted prefix commits"
        );
        frame
            .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
            .expect("the frozen cursor prefix settles every surface");
        frame.commit().expect("the frozen cursor prefix commits")
    };
    assert_eq!(cancellation_count(&prefix), 0);
    assert!(!prefix.native_input_pending());
    assert_eq!(
        native.bridge.admission_dispatch(),
        NativeDispatch::Cancel { boundary }
    );

    assert!(
        native
            .reduce_callback_prefix(&egui::Context::default(), &mut test_budget())
            .unwrap()
            .is_none(),
        "post-boundary callbacks cannot preempt cancellation"
    );
    {
        let mut budget = test_budget();
        let frame = native
            .poll_host_frame(&mut budget, |_| NativeReceiverAnswer::Unknown)
            .unwrap()
            .unwrap();
        assert!(
            frame.native_input_pending(),
            "a cancellation frame keeps product UI inert"
        );
        drop(frame);
    }
    assert_eq!(
        native.bridge.admission_dispatch(),
        NativeDispatch::Cancel { boundary }
    );
    assert!(
        !native.pointer_translator.can_coalesce_idle_cursor_moves(),
        "dropping cancellation cannot reset the published translator state"
    );
    assert!(native.bridge.front_viewport_pointer_passthrough().is_none());
    assert!(native.bridge.has_pending_callback_records());
    assert_eq!(native.input_control.snapshot_fact(binding), None);

    let cancelled = commit_poll(&mut native);
    assert_eq!(cancellation_count(&cancelled), 1);
    assert!(!cancelled.native_input_pending());
    assert!(native.pointer_translator.can_coalesce_idle_cursor_moves());
    assert_eq!(
        native.bridge.front_viewport_pointer_passthrough(),
        Some(callback)
    );
    assert_eq!(native.input_control.snapshot_fact(binding), None);
    assert_eq!(
        native.bridge.admission_dispatch(),
        NativeDispatch::Replay { through: None }
    );
    assert!(
        native
            .reduce_callback_prefix(&egui::Context::default(), &mut test_budget())
            .unwrap()
            .is_some()
    );
    assert_eq!(
        native.input_control.snapshot_fact(binding),
        Some((NativeWindowInputState::PassThrough, None))
    );
    let recovered = commit_poll(&mut native);
    assert_eq!(cancellation_count(&recovered), 0);
    assert!(native.bridge.front_viewport_pointer_passthrough().is_none());
    assert!(native.take_callback_error().is_none());
}

#[test]
fn atomic_core_suffix_commits_before_the_frozen_boundary_can_cancel() {
    let mut native = coordinator();
    register_roots(&mut native);
    native
        .session
        .record_native_pointer_batch((0..257).map(|_| {
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
        }))
        .unwrap();
    let mut first_budget = test_budget();
    assert!(
        native
            .poll_host_frame(&mut first_budget, |_| NativeReceiverAnswer::Unknown)
            .unwrap()
            .is_none()
    );
    assert_eq!(first_budget.remaining_records(), 0);
    assert!(native.bridge.callback_boundary_pending());
    let boundary = native.bridge.freeze_ordinary_for_test();
    assert_eq!(
        native.bridge.admission_dispatch(),
        NativeDispatch::ContinueBoundary
    );

    let original = commit_poll(&mut native);
    assert_eq!(cancellation_count(&original), 0);
    assert!(!original.native_input_pending());
    assert_eq!(
        native.bridge.admission_dispatch(),
        NativeDispatch::Cancel { boundary }
    );
    {
        let mut budget = test_budget();
        let frame = native
            .poll_host_frame(&mut budget, |_| NativeReceiverAnswer::Unknown)
            .unwrap()
            .unwrap();
        assert!(frame.native_input_pending());
        drop(frame);
    }
    assert_eq!(
        native.bridge.admission_dispatch(),
        NativeDispatch::Cancel { boundary }
    );
    assert_eq!(cancellation_count(&commit_poll(&mut native)), 1);
    assert_eq!(cancellation_count(&commit_poll(&mut native)), 0);
}

#[test]
fn parked_close_successor_does_not_delay_cancel_and_promotes_with_original_capture() {
    let mut native = coordinator();
    let (root, binding) = register_roots(&mut native);
    let viewport = ViewportId::from_hash_of("overload-parked-close-model");
    let window = WindowId::from(92);
    let close = observe_model_close(&mut native, binding, [root, binding], viewport, window, 61);
    let acknowledgement = begin_model_cancellation(&mut native, close, viewport);
    let successor = NativeWindowEventRecord::for_test(
        62,
        window,
        Some(viewport),
        Some(binding),
        WindowEvent::CloseRequested,
    );
    assert!(
        successor.close_request().is_none(),
        "this is a tokenless close-owner model"
    );
    native
        .bridge
        .push_record(HostRecord::WindowEvent(successor.clone()));
    let original_capture = native.bridge.with_close_control(|control| {
        let mut parked = control.parked_ingress();
        let ingress = parked
            .next()
            .expect("the cancellation owner parks one successor");
        assert_eq!(ingress.window_event(), Some(&successor));
        let capture = (ingress.ordinal(), ingress.captured_at());
        assert!(parked.next().is_none());
        capture
    });
    let boundary = native.bridge.freeze_ordinary_for_test();
    assert!(original_capture.0 <= boundary);
    assert!(native.next_window_event().unwrap().is_none());
    assert_eq!(
        native.bridge.admission_dispatch(),
        NativeDispatch::ContinueBoundary,
    );
    assert_eq!(cancellation_count(&commit_poll(&mut native)), 0);
    assert_eq!(
        native.bridge.admission_dispatch(),
        NativeDispatch::Cancel { boundary },
        "a parked successor cannot require an external callback before cancellation"
    );
    {
        let mut budget = test_budget();
        let frame = native
            .poll_host_frame(&mut budget, |_| NativeReceiverAnswer::Unknown)
            .unwrap()
            .unwrap();
        assert!(frame.native_input_pending());
        drop(frame);
    }
    assert_eq!(
        native.bridge.admission_dispatch(),
        NativeDispatch::Cancel { boundary }
    );
    assert_eq!(cancellation_count(&commit_poll(&mut native)), 1);
    assert_eq!(native.close_progress().cancellations_committed(), 0);
    native.bridge.with_close_control(|control| {
        let ingress = control.parked_ingress().next().unwrap();
        assert_eq!((ingress.ordinal(), ingress.captured_at()), original_capture);
        assert_eq!(control.request_for_binding(binding), Some(close));
    });

    // Model only: the original core acknowledgement authorizes Clear. No OS
    // cancellation callback or canonical fork close token is fabricated.
    native
        .session
        .publish_native_close(binding, NativeCloseState::Clear, Some(acknowledgement))
        .expect("the original core cancellation acknowledgement publishes Clear");
    assert!(
        native
            .bridge
            .with_close_control(|control| control.mark_clear_recorded(binding))
    );
    let clear = commit_poll(&mut native);
    assert_eq!(cancellation_count(&clear), 0);
    assert!(
        native.next_window_event().unwrap().is_none(),
        "Clear commit still requires report settlement"
    );
    assert!(native.settle_close_control_report(&clear).unwrap());
    assert_eq!(native.close_progress().cancellations_committed(), 1);
    native.bridge.with_close_control(|control| {
        assert!(control.parked_ingress().next().is_none());
        let ingress = control
            .callback_ingress()
            .next()
            .expect("the original successor promotes in its owner");
        assert_eq!(ingress.window_event(), Some(&successor));
        assert_eq!((ingress.ordinal(), ingress.captured_at()), original_capture);
    });
    assert_eq!(native.next_window_event().unwrap(), Some(successor));
    assert!(
        native
            .reduce_callback_prefix(&egui::Context::default(), &mut test_budget())
            .unwrap()
            .is_some()
    );
    let requested = commit_poll(&mut native);
    assert_eq!(cancellation_count(&requested), 0);
    assert!(native.settle_close_control_report(&requested).unwrap());
    let next = native
        .bridge
        .with_close_control(|control| control.request_for_binding(binding))
        .expect("the promoted successor binds its own core request");
    assert_ne!(next, close);
}
