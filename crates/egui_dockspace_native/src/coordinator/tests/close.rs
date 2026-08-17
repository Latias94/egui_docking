use eframe::egui::ViewportCommand;

use super::*;
use crate::close_control::{NativeViewportCloseCancellationRecord, NativeWindowClosePolicy};

pub(super) fn observe_close(
    native: &mut NativeCoordinator,
    binding: NativeSurfaceBinding,
    live_bindings: impl IntoIterator<Item = NativeSurfaceBinding>,
    viewport: ViewportId,
    window: WindowId,
    ordinal: u64,
) {
    native
        .report_snapshot(
            live_bindings
                .into_iter()
                .map(|binding| (binding, NativeWindowFacts::live())),
            NativeWorkAreaRoster::Unknown,
            test_managed_capabilities(),
        )
        .expect("the managed capability roster queues");
    native
        .bind_viewport(viewport, window, binding)
        .expect("the exact viewport binds");
    native
        .bridge
        .push_record(HostRecord::WindowEvent(NativeWindowEventRecord::for_test(
            ordinal,
            window,
            Some(viewport),
            Some(binding),
            WindowEvent::CloseRequested,
        )));
    assert!(
        native
            .reduce_callback_head()
            .expect("the exact native close callback records")
    );
    let mut frame = native
        .begin_host_frame(|_| NativeReceiverAnswer::Unknown)
        .expect("the native close observation frame begins");
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .expect("the native close observation settles every surface");
    let report = frame
        .commit()
        .expect("the native close observation frame commits");
    assert!(
        native
            .settle_close_control_inputs(report.inputs())
            .expect("the exact core close request binds the callback")
    );
}

#[test]
fn cancel_policy_waits_for_the_exact_child_viewport_callback_before_live_clear() {
    let mut native = coordinator();
    let (survivor, binding) = register_roots(&mut native);
    let viewport = ViewportId::from_hash_of("cancelled-native-child");
    let window = WindowId::from(41);
    observe_close(
        &mut native,
        binding,
        [survivor, binding],
        viewport,
        window,
        17,
    );

    assert!(
        native
            .drive_close_policy(NativeWindowClosePolicy::Cancel)
            .expect("the explicit cancellation policy commits")
    );
    assert_eq!(
        native.take_viewport_commands(),
        vec![(viewport, ViewportCommand::CancelClose)]
    );
    assert!(native.close_control.references_binding(binding));

    native
        .bridge
        .push_record(HostRecord::ViewportCloseCancelled(
            NativeViewportCloseCancellationRecord::for_test(17, viewport, window, binding),
        ));
    assert!(
        native
            .reduce_callback_head()
            .expect("the exact cancellation callback records LiveClear")
    );
    let mut frame = native
        .begin_host_frame(|_| NativeReceiverAnswer::Unknown)
        .expect("the cancellation observation frame begins");
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .expect("the cancellation observation settles every surface");
    let report = frame
        .commit()
        .expect("the exact cancellation observation commits");
    assert!(
        native
            .settle_close_control_inputs(report.inputs())
            .expect("the exact LiveClear acknowledgement settles")
    );
    assert!(!native.close_control.references_binding(binding));
    assert!(native.session.is_current_native_binding(binding));
}

#[test]
fn destroyed_before_cancel_callback_retires_close_sidecar_and_allows_quiescence() {
    let mut native = coordinator();
    let (survivor, binding) = register_roots(&mut native);
    let viewport = ViewportId::from_hash_of("destroyed-before-cancel-callback");
    let window = WindowId::from(45);
    observe_close(
        &mut native,
        binding,
        [survivor, binding],
        viewport,
        window,
        19,
    );

    assert!(
        native
            .drive_close_policy(NativeWindowClosePolicy::Cancel)
            .expect("the explicit cancellation policy commits")
    );
    assert_eq!(
        native.take_viewport_commands(),
        vec![(viewport, ViewportCommand::CancelClose)]
    );
    assert!(native.close_control.references_binding(binding));

    native
        .bridge
        .push_record(HostRecord::WindowEvent(NativeWindowEventRecord::for_test(
            20,
            window,
            Some(viewport),
            Some(binding),
            WindowEvent::Destroyed,
        )));
    native.bridge.push_viewport_roster(live_roster([survivor]));

    assert!(
        native
            .reduce_callback_head()
            .expect("the exact destroyed callback records")
    );
    let mut destroyed = native
        .begin_host_frame(|_| NativeReceiverAnswer::Unknown)
        .expect("the destroyed callback frame begins");
    destroyed
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .expect("the destroyed callback settles every surface");
    destroyed
        .commit()
        .expect("the destroyed callback frame commits");
    assert!(
        native.close_control.references_binding(binding),
        "the close correlation remains until the destroyed tombstone commits"
    );

    assert!(
        native
            .reduce_callback_head()
            .expect("the exact tombstone roster records")
    );
    let mut roster = native
        .begin_host_frame(|_| NativeReceiverAnswer::Unknown)
        .expect("the destroyed roster frame begins");
    roster
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .expect("the destroyed roster settles every surface");
    let report = roster
        .commit()
        .expect("the destroyed roster commits the exact tombstone");
    assert!(native.settle_host_frame_inputs(report.inputs()));

    let prepared = native
        .prepare_committed_retirements()
        .expect("the committed route prepares atomically")
        .expect("one exact route is ready to retire");
    let mut retirement = native
        .begin_host_frame(|_| NativeReceiverAnswer::Unknown)
        .expect("the route retirement frame begins");
    retirement
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .expect("the route retirement frame settles every surface");
    retirement
        .commit()
        .expect("the route retirement frame commits");
    assert!(native.commit_retirements(prepared).is_empty());

    assert!(!native.close_control.references_binding(binding));
    assert!(
        native
            .try_report_retirement_quiescence()
            .expect("terminal close correlation no longer blocks quiescence")
    );
}

#[test]
fn retain_layout_accepts_close_and_retires_only_after_destroyed_acknowledgement() {
    let mut native = coordinator();
    let (binding, survivor) = register_roots(&mut native);
    let window = WindowId::from(51);
    observe_close(
        &mut native,
        binding,
        [binding, survivor],
        ViewportId::ROOT,
        window,
        23,
    );

    assert!(
        native
            .drive_close_policy(NativeWindowClosePolicy::RetainLayout)
            .expect("the explicit retain-layout policy commits")
    );
    assert!(
        native.take_viewport_commands().is_empty(),
        "accepting the operating-system close must not enqueue a second close request"
    );
    assert!(
        native.close_control.references_binding(binding),
        "the accepted close remains correlated until the exact window is destroyed"
    );
    assert!(native.session.is_current_native_binding(binding));

    native
        .bridge
        .push_record(HostRecord::WindowEvent(NativeWindowEventRecord::for_test(
            24,
            window,
            Some(ViewportId::ROOT),
            Some(binding),
            WindowEvent::Destroyed,
        )));
    native.bridge.push_viewport_roster(live_roster([survivor]));
    assert!(
        native
            .reduce_callback_head()
            .expect("the exact destroyed callback records")
    );
    assert!(!native.close_control.references_binding(binding));
    let mut destroyed = native
        .begin_host_frame(|_| NativeReceiverAnswer::Unknown)
        .expect("the destroyed callback frame begins");
    destroyed
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .expect("the destroyed callback settles every surface");
    destroyed
        .commit()
        .expect("the destroyed callback frame commits");

    assert!(
        native
            .reduce_callback_head()
            .expect("the exact tombstone roster records")
    );
    let mut roster = native
        .begin_host_frame(|_| NativeReceiverAnswer::Unknown)
        .expect("the destroyed roster frame begins");
    roster
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .expect("the destroyed roster settles every surface");
    let mut report = roster
        .commit()
        .expect("the destroyed roster commits the exact acknowledgement");
    assert!(report.take_native_effects().is_empty());
    assert!(native.settle_host_frame_inputs(report.inputs()));
    assert!(!native.session.is_current_native_binding(binding));
    assert!(native.session.view().surface(FIRST_SURFACE).is_some());
}

#[test]
fn accepted_close_rejects_destroyed_from_a_different_native_window() {
    let mut native = coordinator();
    let (binding, survivor) = register_roots(&mut native);
    let viewport = ViewportId::ROOT;
    let window = WindowId::from(61);
    observe_close(
        &mut native,
        binding,
        [binding, survivor],
        viewport,
        window,
        31,
    );

    assert!(
        native
            .drive_close_policy(NativeWindowClosePolicy::RetainLayout)
            .expect("the explicit retain-layout policy commits")
    );
    native
        .bridge
        .push_record(HostRecord::WindowEvent(NativeWindowEventRecord::for_test(
            32,
            WindowId::from(62),
            Some(viewport),
            Some(binding),
            WindowEvent::Destroyed,
        )));

    let error = native
        .reduce_callback_head()
        .expect_err("a different native window cannot settle the accepted close");
    assert_eq!(error.kind(), crate::NativeRuntimeErrorKind::HostProtocol);
    assert!(native.close_control.references_binding(binding));
}
