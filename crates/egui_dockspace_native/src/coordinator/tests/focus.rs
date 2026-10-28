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
fn quarantine_cancels_only_an_undispatched_focus_request() {
    let mut native = coordinator();
    let (binding, _) = register_roots(&mut native);
    let context = egui::Context::default();
    let viewport = ViewportId::ROOT;
    let window = WindowId::from(11);
    let mut focus = NativeFocusControl::<u8>::default();
    focus
        .retain(viewport, window, binding, 7)
        .expect("the queued test focus request is retained");
    assert!(matches!(
        focus.take_queued_terminal(binding),
        Some(NativeFocusTermination::Queued {
            viewport: queued_viewport,
            request: 7,
        }) if queued_viewport == viewport
    ));

    focus
        .retain(viewport, window, binding, 9)
        .expect("the dispatched test focus request is retained");
    let token = eframe::queue_native_viewport_focus(&context, viewport);
    assert!(focus.mark_dispatched(viewport, token));
    assert!(focus.take_queued_terminal(binding).is_none());
    assert!(focus.references_binding(binding));
    assert!(matches!(
        focus.take_terminal(binding),
        Some(NativeFocusTermination::Dispatched(9))
    ));
}
