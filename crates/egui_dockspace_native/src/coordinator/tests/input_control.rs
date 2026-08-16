use dockspace::runtime::{HostInputOutcome, NativeWindowInputState};
use eframe::NativeViewportPointerPassthroughStatus;

use super::*;
use crate::input_control::NativePointerPassthroughRecord;

#[test]
fn applied_pointer_passthrough_callback_becomes_an_exact_snapshot_fact() {
    let mut native = coordinator();
    let (first, second) = register_roots(&mut native);
    let first_window = WindowId::from(11);
    let second_window = WindowId::from(22);
    native
        .bind_viewport(ViewportId::ROOT, first_window, first)
        .expect("the root route binds");
    let child = ViewportId::from_hash_of("input-control-child");
    native
        .bind_viewport(child, second_window, second)
        .expect("the child route binds");

    let record = NativePointerPassthroughRecord::for_test(
        pointer_passthrough_token(ViewportId::ROOT, true),
        ViewportId::ROOT,
        first_window,
        Some(first),
        true,
        NativeViewportPointerPassthroughStatus::Applied,
    );
    native
        .bridge
        .push_record(HostRecord::ViewportPointerPassthrough(record));

    assert!(
        native
            .reduce_callback_head()
            .expect("the exact input callback records")
    );
    let mut callback_frame = native
        .begin_host_frame(|_| NativeReceiverAnswer::Unknown)
        .expect("the callback boundary begins");
    callback_frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .expect("the callback boundary settles every surface");
    callback_frame
        .commit()
        .expect("the callback boundary commits");
    assert_eq!(
        native.input_control.snapshot_fact(first),
        Some((NativeWindowInputState::PassThrough, None))
    );

    native
        .bridge
        .push_viewport_roster(live_roster([first, second]));
    assert!(
        native
            .reduce_callback_head()
            .expect("the complete roster records the input fact")
    );
    let mut roster_frame = native
        .begin_host_frame(|_| NativeReceiverAnswer::Unknown)
        .expect("the roster boundary begins");
    roster_frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .expect("the roster boundary settles every surface");
    let report = roster_frame.commit().expect("the roster boundary commits");
    assert!(native.settle_host_frame_inputs(report.inputs()));
    assert!(report.inputs().iter().any(|input| matches!(
        input,
        HostInputOutcome::NativePlatformSnapshotApplied { .. }
    )));
    assert_eq!(
        native.input_control.snapshot_fact(first),
        Some((NativeWindowInputState::PassThrough, None))
    );
}

#[test]
fn failed_pointer_passthrough_callback_does_not_invent_input_authority() {
    let mut native = coordinator();
    let (first, _) = register_roots(&mut native);
    let window = WindowId::from(11);
    native
        .bind_viewport(ViewportId::ROOT, window, first)
        .expect("the root route binds");
    let record = NativePointerPassthroughRecord::for_test(
        pointer_passthrough_token(ViewportId::ROOT, true),
        ViewportId::ROOT,
        window,
        Some(first),
        true,
        NativeViewportPointerPassthroughStatus::Failed,
    );
    native
        .bridge
        .push_record(HostRecord::ViewportPointerPassthrough(record));

    assert!(
        native
            .reduce_callback_head()
            .expect("the failed callback is consumed")
    );
    assert_eq!(native.input_control.snapshot_fact(first), None);
}
