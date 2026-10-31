use dockspace::runtime::{
    HostInputOutcome, NativeGlobalFocus, NativeWindowFacts, NativeWindowPresentationState,
};
use eframe::{NativeGlobalFocus as EframeGlobalFocus, NativeViewportFocusStatus};

use super::*;
use crate::focus_control::{
    NativeFocusControl, NativeFocusResultDisposition, NativeFocusTermination,
    NativeGlobalFocusRecord, NativeViewportFocusRecord,
};

#[test]
fn global_focus_before_the_first_roster_uses_the_initial_capability_snapshot() {
    let mut native = coordinator();
    native
        .bridge
        .push_record(HostRecord::GlobalFocus(NativeGlobalFocusRecord::for_test(
            1,
            NativeGlobalFocus::Unknown,
        )));

    assert!(
        native
            .reduce_callback_prefix(&egui::Context::default(), &mut test_budget())
            .expect("the exact focus callback records")
            .is_some()
    );
    let mut frame = native
        .begin_host_frame(|_| NativeReceiverAnswer::Unknown)
        .expect("the initial capability and focus boundary begins");
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .expect("the focus boundary settles every surface");
    let report = frame.commit().expect("the focus boundary commits");

    assert!(report.inputs().iter().any(|input| matches!(
        input,
        HostInputOutcome::NativePlatformSnapshotApplied { .. }
    )));
    assert!(
        report
            .inputs()
            .iter()
            .any(|input| matches!(input, HostInputOutcome::NativeFocusObservationApplied))
    );
}

#[test]
fn exact_dock_focus_requires_the_last_committed_visible_roster() {
    let mut native = coordinator();
    let (first, second) = register_roots(&mut native);
    let first_window = WindowId::from(11);
    let second_window = WindowId::from(22);
    native
        .bind_viewport(ViewportId::ROOT, first_window, first)
        .expect("the root route binds");
    let child = ViewportId::from_hash_of("focus-child");
    native
        .bind_viewport(child, second_window, second)
        .expect("the child route binds");

    let before_roster = native.bridge.with_viewports(|routes| {
        NativeGlobalFocusRecord::capture_for_test(
            2,
            EframeGlobalFocus::Viewport {
                viewport_id: ViewportId::ROOT,
                window_id: first_window,
            },
            routes,
        )
    });
    assert_eq!(before_roster.focus(), NativeGlobalFocus::Unknown);

    let visible = |binding| {
        crate::window_snapshot::CompiledWindowObservation::new(
            binding,
            NativeWindowFacts::live()
                .with_presentation(NativeWindowPresentationState::Visible, None),
        )
        .with_focus_reportable_for_test()
    };
    native
        .bridge
        .push_viewport_roster(NativeViewportRosterRecord::for_test(
            [visible(first), visible(second)],
            true,
        ));
    assert!(
        native
            .reduce_callback_prefix(&egui::Context::default(), &mut test_budget())
            .expect("the complete visible roster records")
            .is_some()
    );
    let mut visible_frame = native
        .begin_host_frame(|_| NativeReceiverAnswer::Unknown)
        .expect("the visible roster frame begins");
    visible_frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .expect("the visible roster frame settles every surface");
    let visible_report = visible_frame.commit().expect("the visible roster commits");
    assert!(
        native
            .settle_host_frame_inputs(visible_report.inputs())
            .expect("the exact visible snapshot settles")
    );

    let committed = native.bridge.with_viewports(|routes| {
        NativeGlobalFocusRecord::capture_for_test(
            3,
            EframeGlobalFocus::Viewport {
                viewport_id: ViewportId::ROOT,
                window_id: first_window,
            },
            routes,
        )
    });
    assert_eq!(committed.focus(), NativeGlobalFocus::Dock(first));

    native
        .bridge
        .push_viewport_roster(NativeViewportRosterRecord::for_test([visible(first)], true));
    assert!(
        native
            .reduce_callback_prefix(&egui::Context::default(), &mut test_budget())
            .expect("the incomplete roster records an unknown inventory")
            .is_some()
    );
    let mut unknown = native
        .begin_host_frame(|_| NativeReceiverAnswer::Unknown)
        .expect("the inventory tombstone frame begins");
    unknown
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .expect("the inventory tombstone frame settles every surface");
    let unknown_report = unknown.commit().expect("the inventory tombstone commits");
    assert!(
        native
            .settle_host_frame_inputs(unknown_report.inputs())
            .expect("the exact unknown snapshot settles")
    );

    let after_unknown = native.bridge.with_viewports(|routes| {
        NativeGlobalFocusRecord::capture_for_test(
            4,
            EframeGlobalFocus::Viewport {
                viewport_id: ViewportId::ROOT,
                window_id: first_window,
            },
            routes,
        )
    });
    assert_eq!(after_unknown.focus(), NativeGlobalFocus::Unknown);
}

#[test]
fn captured_dock_focus_is_unknown_after_hiding_and_preserves_the_foreign_successor() {
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::Duration;

    let now = Arc::new(AtomicU64::new(0));
    let clock = Arc::clone(&now);
    let mut native =
        coordinator_with_clock(dockspace::runtime::NativeMonotonicClock::new(move || {
            Duration::from_millis(clock.load(Ordering::SeqCst))
        }));
    let (root, binding) = register_roots(&mut native);
    let context = egui::Context::default();
    let child = ViewportId::from_hash_of("hidden-focus-child");
    let window = WindowId::from(22);
    native
        .bind_viewport(ViewportId::ROOT, WindowId::from(11), root)
        .unwrap();
    native.bind_viewport(child, window, binding).unwrap();

    let visible = |binding| {
        crate::window_snapshot::CompiledWindowObservation::new(
            binding,
            NativeWindowFacts::live()
                .with_presentation(NativeWindowPresentationState::Visible, None),
        )
        .with_focus_reportable_for_test()
    };
    native
        .bridge
        .push_viewport_roster(NativeViewportRosterRecord::for_test(
            [visible(root), visible(binding)],
            true,
        ));
    assert!(
        native
            .reduce_callback_prefix(&context, &mut test_budget())
            .unwrap()
            .is_some()
    );
    let mut frame = native
        .begin_host_frame(|_| NativeReceiverAnswer::Unknown)
        .unwrap();
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    let report = frame.commit().unwrap();
    assert!(native.settle_host_frame_report(&report).unwrap());
    assert!(native.session.native_binding_can_report_focus(binding));

    let captured = native.bridge.with_viewports(|routes| {
        NativeGlobalFocusRecord::capture_for_test(
            41,
            EframeGlobalFocus::Viewport {
                viewport_id: child,
                window_id: window,
            },
            routes,
        )
    });
    assert_eq!(captured.focus(), NativeGlobalFocus::Dock(binding));

    native
        .report_snapshot(
            [
                (
                    root,
                    NativeWindowFacts::live()
                        .with_presentation(NativeWindowPresentationState::Visible, None),
                ),
                (
                    binding,
                    NativeWindowFacts::live()
                        .with_presentation(NativeWindowPresentationState::Hidden, None),
                ),
            ],
            NativeWorkAreaRoster::Unknown,
            test_managed_capabilities(),
        )
        .unwrap();
    let mut frame = native
        .begin_host_frame(|_| NativeReceiverAnswer::Unknown)
        .unwrap();
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    frame.commit().unwrap();
    assert!(native.session.is_current_native_binding(binding));
    assert!(!native.session.native_binding_can_report_focus(binding));

    let successor = NativeGlobalFocusRecord::for_test(42, NativeGlobalFocus::Foreign);
    native.bridge.push_record(HostRecord::GlobalFocus(captured));
    native
        .bridge
        .push_record(HostRecord::GlobalFocus(successor));
    let captured_through = native.bridge.captured_through_for_test();
    let recorded_before = native.session.native_input_recorded_count();
    now.store(17, Ordering::SeqCst);
    for (record, retained) in [(captured, 2), (successor, 1)] {
        assert_eq!(native.bridge.front_global_focus(), Some(record));
        assert_eq!(
            native
                .reduce_callback_prefix(&context, &mut test_budget())
                .unwrap()
                .unwrap()
                .get(),
            1
        );
        assert!(!native.bridge.acknowledge_global_focus(record));
        assert!(
            native
                .reduce_callback_prefix(&context, &mut test_budget())
                .unwrap()
                .is_none()
        );
        assert_eq!(native.bridge.captured_through_for_test(), captured_through);
        assert_eq!(
            native.bridge.input_backlog_for_test(),
            (retained, Some(Duration::from_millis(17))),
            "normalization and transfer preserve the original callback age"
        );
        let mut budget = test_budget();
        let mut frame = native
            .poll_resolved_host_frame(&context, &mut budget)
            .unwrap()
            .unwrap();
        frame
            .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
            .unwrap();
        let report = frame
            .commit()
            .expect("revoked Dock focus must commit as Unknown without retargeting");
        assert!(matches!(
            report.inputs(),
            [HostInputOutcome::NativeFocusObservationApplied]
        ));
        assert!(native.take_callback_error().is_none());
    }
    assert_eq!(
        native.session.native_input_recorded_count(),
        recorded_before + 2
    );
    assert!(!native.bridge.has_pending_callback_records());
    assert!(!native.reduce_next_global_focus().unwrap());
}

#[test]
fn unrelated_viewport_focus_result_is_acknowledged_without_inventing_focus() {
    let mut native = coordinator();
    let context = egui::Context::default();
    let viewport = ViewportId::ROOT;
    let token = eframe::queue_native_viewport_focus(&context, viewport);
    let record = NativeViewportFocusRecord::for_test(
        token,
        viewport,
        WindowId::from(11),
        None,
        NativeViewportFocusStatus::Requested,
    );
    native.bridge.push_record(HostRecord::ViewportFocus(record));

    assert!(
        native
            .reduce_callback_prefix(&context, &mut test_budget())
            .expect("the unrelated command result is consumed")
            .is_some()
    );
    let mut frame = native
        .begin_host_frame(|_| NativeReceiverAnswer::Unknown)
        .expect("the acknowledgement boundary begins");
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .expect("the acknowledgement boundary settles every surface");
    let report = frame
        .commit()
        .expect("the acknowledgement boundary commits");
    assert!(
        !report
            .inputs()
            .iter()
            .any(|input| matches!(input, HostInputOutcome::NativeFocusObservationApplied))
    );
}

#[test]
fn global_focus_completes_only_after_the_requested_callback() {
    let mut native = coordinator();
    let (binding, other_binding) = register_roots(&mut native);
    let context = egui::Context::default();
    let viewport = ViewportId::ROOT;
    let window = WindowId::from(11);
    let mut focus = NativeFocusControl::<u8>::default();
    focus
        .retain(viewport, window, binding, 7)
        .expect("the test focus request is retained");
    let token = eframe::queue_native_viewport_focus(&context, viewport);
    assert!(focus.mark_dispatched(viewport, token));

    assert_eq!(
        focus.take_observed(binding),
        None,
        "an older global focus fact cannot complete a command awaiting its dispatch callback"
    );

    let requested = NativeViewportFocusRecord::for_test(
        token,
        viewport,
        window,
        Some(binding),
        NativeViewportFocusStatus::Requested,
    );
    assert!(focus.mark_requested(requested));
    assert!(focus.has_pending_work());
    assert_eq!(focus.take_observed(other_binding), None);
    assert_eq!(focus.take_observed(binding), Some(7));
    assert!(!focus.has_pending_work());
    assert_eq!(focus.take_observed(binding), None);
}

#[test]
fn delayed_foreign_focus_tokens_cannot_claim_the_current_owner() {
    let mut native = coordinator();
    let (binding, _) = register_roots(&mut native);
    let context = egui::Context::default();
    let viewport = ViewportId::ROOT;
    let window = WindowId::from(11);

    for awaiting_global_focus in [false, true] {
        for status in [
            NativeViewportFocusStatus::Requested,
            NativeViewportFocusStatus::AlreadyFocused,
        ] {
            let foreign_token = eframe::queue_native_viewport_focus(&context, viewport);
            let mut focus = NativeFocusControl::<u8>::default();
            focus
                .retain(viewport, window, binding, 7)
                .expect("the current focus request is retained");
            let token = eframe::queue_native_viewport_focus(&context, viewport);
            assert_ne!(foreign_token, token);
            assert!(focus.mark_dispatched(viewport, token));
            let requested = NativeViewportFocusRecord::for_test(
                token,
                viewport,
                window,
                Some(binding),
                NativeViewportFocusStatus::Requested,
            );
            if awaiting_global_focus {
                assert!(focus.mark_requested(requested));
            }

            let foreign = NativeViewportFocusRecord::for_test(
                foreign_token,
                viewport,
                window,
                Some(binding),
                status,
            );
            assert_eq!(
                focus.classify(foreign),
                NativeFocusResultDisposition::Unrelated,
                "a delayed same-window callback with {status:?} has no command authority"
            );
            assert_eq!(focus.take_callback(foreign), None);
            assert!(!focus.mark_requested(foreign));
            assert!(focus.has_pending_work());
            assert_eq!(
                focus.classify(requested),
                NativeFocusResultDisposition::Current {
                    binding,
                    status: NativeViewportFocusStatus::Requested,
                }
            );

            if !awaiting_global_focus {
                assert_eq!(
                    focus.take_observed(binding),
                    None,
                    "a foreign callback cannot authorize completion by a global focus fact"
                );
                assert!(focus.mark_requested(requested));
            }
            assert!(focus.has_pending_work());
            assert_eq!(focus.take_observed(binding), Some(7));
            assert!(!focus.has_pending_work());
        }
    }
}

#[test]
fn already_focused_callback_completes_only_its_exact_command_token() {
    let mut native = coordinator();
    let (binding, _) = register_roots(&mut native);
    let context = egui::Context::default();
    let viewport = ViewportId::ROOT;
    let window = WindowId::from(11);
    let mut focus = NativeFocusControl::<u8>::default();
    focus
        .retain(viewport, window, binding, 7)
        .expect("the current focus request is retained");
    let token = eframe::queue_native_viewport_focus(&context, viewport);
    assert!(focus.mark_dispatched(viewport, token));
    let record = NativeViewportFocusRecord::for_test(
        token,
        viewport,
        window,
        Some(binding),
        NativeViewportFocusStatus::AlreadyFocused,
    );

    assert_eq!(
        focus.classify(record),
        NativeFocusResultDisposition::Current {
            binding,
            status: NativeViewportFocusStatus::AlreadyFocused,
        }
    );
    assert!(!focus.mark_requested(record));
    assert_eq!(focus.take_callback(record), Some(7));
    assert!(!focus.has_pending_work());
    assert_eq!(focus.take_callback(record), None);
}

#[test]
fn single_focus_owner_rejects_successors_and_quarantine_cancels_only_undispatched() {
    let mut native = coordinator();
    let (binding, _) = register_roots(&mut native);
    let context = egui::Context::default();
    let viewport = ViewportId::ROOT;
    let window = WindowId::from(11);
    let mut focus = NativeFocusControl::<u8>::default();
    focus
        .retain(viewport, window, binding, 7)
        .expect("the queued test focus request is retained");
    assert_eq!(focus.retain(viewport, window, binding, 8), Err(8));
    assert_eq!(focus.queued_route(), Some((viewport, binding, window)));
    assert_eq!(focus.pending_token(), None);
    assert!(matches!(
        focus.take_queued_terminal(binding),
        Some(NativeFocusTermination::Queued(7))
    ));

    focus
        .retain(viewport, window, binding, 9)
        .expect("the dispatched test focus request is retained");
    let token = eframe::queue_native_viewport_focus(&context, viewport);
    assert!(focus.mark_dispatched(viewport, token));
    assert_eq!(focus.retain(viewport, window, binding, 10), Err(10));
    assert_eq!(focus.pending_token(), Some(token));
    assert!(focus.queued_route().is_none());
    assert!(focus.take_queued_terminal(binding).is_none());
    assert!(focus.references_binding(binding));
    assert!(matches!(
        focus.take_terminal(binding),
        Some(NativeFocusTermination::Dispatched(9))
    ));
}
