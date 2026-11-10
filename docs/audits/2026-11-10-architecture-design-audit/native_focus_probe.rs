// Audit-only characterization probe for the fixed 2289c88 baseline.
// Load as a temporary child module of native coordinator/tests.rs.

use super::*;
use crate::focus_control::NativeViewportFocusRecord;
use eframe::NativeViewportFocusStatus;

fn select(native: &mut NativeCoordinator, item: ItemId) -> HostFrameReport {
    let mut frame = native
        .begin_host_frame(|_| NativeReceiverAnswer::Unknown)
        .unwrap();
    frame.frame.select_item_current(item).unwrap();
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    frame.commit().unwrap()
}

#[test]
fn requested_focus_blocks_successor_and_quarantine_probe() {
    let (mut native, first, second) = presented_local_coordinate_routes();
    let context = egui::Context::default();
    native
        .bridge
        .push_viewport_roster(NativeViewportRosterRecord::for_test(
            [
                CompiledWindowObservation::new(first, ready_window_facts(0.0))
                    .with_focus_reportable_for_test(),
                CompiledWindowObservation::new(second, ready_window_facts(720.0))
                    .with_focus_reportable_for_test(),
            ],
            true,
        ));
    native.reduce_next_viewport_roster().unwrap();
    let mut frame = native
        .begin_host_frame(|_| NativeReceiverAnswer::Unknown)
        .unwrap();
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    let report = frame.commit().unwrap();
    native.settle_host_frame_report(&report).unwrap();
    let mut report = select(&mut native, ItemId::new(1));
    let effects = report.take_native_effects();
    assert!(matches!(
        effects.as_slice(),
        [effect] if matches!(effect.operation(), NativeEffectOperation::RequestFocus { binding } if *binding == first)
    ));
    native.accept_native_effects(&context, effects).unwrap();
    assert!(native.dispatch_viewport_commands(&context).unwrap());
    let token = native.focus_control.pending_token().unwrap();
    native.bridge.push_record(HostRecord::ViewportFocus(
        NativeViewportFocusRecord::for_test(
            token,
            ViewportId::ROOT,
            WindowId::from(11),
            Some(first),
            NativeViewportFocusStatus::Requested,
        ),
    ));
    assert!(native.reduce_next_viewport_focus().unwrap());
    assert!(native.focus_control.has_pending_work());
    native
        .session
        .report_native_global_focus(NativeGlobalFocus::Unknown)
        .unwrap();
    callback_prefix::commit_boundary(&mut native);

    let mut report = select(&mut native, ItemId::new(2));
    let effects = report.take_native_effects();
    assert!(matches!(
        effects.as_slice(),
        [effect] if matches!(effect.operation(), NativeEffectOperation::RequestFocus { binding } if *binding == second)
    ));
    native.accept_native_effects(&context, effects).unwrap();
    assert_eq!(native.focus_control.pending_binding(), Some(first));
    assert!(!native.dispatch_viewport_commands(&context).unwrap());
    eprintln!("successor_focus_emitted_by_core=true dispatched=false owner=first");

    native.quarantine_after_fatal();
    for turn in 1..=4 {
        let advance = native.advance_shutdown_boundary(&context, &mut test_budget());
        eprintln!(
            "shutdown_turn={turn} focus_pending={} quiescent={} errors={:?}",
            native.focus_control.has_pending_work(),
            native.is_quiescent(),
            advance.errors
        );
        assert!(advance.errors.is_empty());
        assert_eq!(native.focus_control.pending_binding(), Some(first));
        assert!(!native.is_quiescent());
    }
}
