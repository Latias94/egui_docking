use dockspace::runtime::{
    HostInputOutcome, NativeGlobalFocus, NativeWindowFacts, NativeWindowPresentationState,
};
use eframe::{NativeGlobalFocus as EframeGlobalFocus, NativeViewportFocusStatus};

use super::*;
use crate::focus_control::{
    NativeFocusControl, NativeFocusTermination, NativeGlobalFocusRecord, NativeViewportFocusRecord,
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
            .reduce_callback_head()
            .expect("the exact focus callback records")
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

    let before_roster = NativeGlobalFocusRecord::capture_for_test(
        2,
        EframeGlobalFocus::Viewport {
            viewport_id: ViewportId::ROOT,
            window_id: first_window,
        },
        &native
            .viewports
            .lock()
            .unwrap_or_else(PoisonError::into_inner),
    );
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
            .reduce_callback_head()
            .expect("the complete visible roster records")
    );
    let mut visible_frame = native
        .begin_host_frame(|_| NativeReceiverAnswer::Unknown)
        .expect("the visible roster frame begins");
    visible_frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .expect("the visible roster frame settles every surface");
    let visible_report = visible_frame.commit().expect("the visible roster commits");
    assert!(native.settle_host_frame_inputs(visible_report.inputs()));

    let committed = NativeGlobalFocusRecord::capture_for_test(
        3,
        EframeGlobalFocus::Viewport {
            viewport_id: ViewportId::ROOT,
            window_id: first_window,
        },
        &native
            .viewports
            .lock()
            .unwrap_or_else(PoisonError::into_inner),
    );
    assert_eq!(committed.focus(), NativeGlobalFocus::Dock(first));

    native
        .bridge
        .push_viewport_roster(NativeViewportRosterRecord::for_test([visible(first)], true));
    assert!(
        native
            .reduce_callback_head()
            .expect("the incomplete roster records an unknown inventory")
    );
    let mut unknown = native
        .begin_host_frame(|_| NativeReceiverAnswer::Unknown)
        .expect("the inventory tombstone frame begins");
    unknown
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .expect("the inventory tombstone frame settles every surface");
    let unknown_report = unknown.commit().expect("the inventory tombstone commits");
    assert!(native.settle_host_frame_inputs(unknown_report.inputs()));

    let after_unknown = NativeGlobalFocusRecord::capture_for_test(
        4,
        EframeGlobalFocus::Viewport {
            viewport_id: ViewportId::ROOT,
            window_id: first_window,
        },
        &native
            .viewports
            .lock()
            .unwrap_or_else(PoisonError::into_inner),
    );
    assert_eq!(after_unknown.focus(), NativeGlobalFocus::Unknown);
}

#[test]
fn unrelated_viewport_focus_result_is_acknowledged_without_inventing_focus() {
    let mut native = coordinator();
    let record = NativeViewportFocusRecord::for_test(
        ViewportId::ROOT,
        WindowId::from(11),
        None,
        NativeViewportFocusStatus::Requested,
    );
    native.bridge.push_record(HostRecord::ViewportFocus(record));

    assert!(
        native
            .reduce_callback_head()
            .expect("the unrelated command result is consumed")
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
    let (binding, _) = register_roots(&mut native);
    let viewport = ViewportId::ROOT;
    let window = WindowId::from(11);
    let mut focus = NativeFocusControl::<u8>::default();
    focus
        .retain(viewport, window, binding, 7)
        .expect("the test focus request is retained");
    assert!(focus.mark_dispatched(viewport));

    assert_eq!(
        focus.take_observed(binding),
        None,
        "an older global focus fact cannot complete a command awaiting its dispatch callback"
    );

    let requested = NativeViewportFocusRecord::for_test(
        viewport,
        window,
        Some(binding),
        NativeViewportFocusStatus::Requested,
    );
    assert!(focus.mark_requested(requested));
    assert_eq!(focus.take_observed(binding), Some(7));
}

#[test]
fn quarantine_cancels_only_an_undispatched_focus_request() {
    let mut native = coordinator();
    let (binding, _) = register_roots(&mut native);
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
    assert!(focus.mark_dispatched(viewport));
    assert!(focus.take_queued_terminal(binding).is_none());
    assert!(focus.references_binding(binding));
    assert!(matches!(
        focus.take_terminal(binding),
        Some(NativeFocusTermination::Dispatched(9))
    ));
}
