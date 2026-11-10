// Audit-only characterization probes for the fixed 2289c88 baseline.
// Load as a temporary child module of crates/dockspace/tests/product_actions.rs.
// Assertions characterize the defects; invert them when implementing regressions.

use super::*;
use crate::drop_resolver::structural_work;

#[test]
fn equal_configuration_rehome_probe() {
    for replace_equal in [false, true] {
        let (mut session, _) = live_managed_native_root();
        let original = session.presentation_config().clone();
        let prepared = session.prepare_float_root(NATIVE_SOURCE_ROOT, MAIN_SURFACE);
        let mut stage = session
            .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
            .unwrap();
        stage.submit_prepared_action(prepared).unwrap();
        if replace_equal {
            stage.replace_presentation_config(original).unwrap();
        }
        let report = commit(stage);
        let pending = session.has_pending_presentation();
        eprintln!(
            "equal_config={replace_equal} pending={pending} transitions={:?}",
            report.presentation_transitions()
        );
        assert_eq!(pending, !replace_equal);
        if replace_equal {
            assert!(report.inputs().iter().any(|outcome| matches!(
                outcome,
                HostInputOutcome::PresentationConfigReplaced { changed: false, .. }
            )));
            assert!(matches!(
                report.presentation_transitions(),
                [terminal] if terminal.result()
                    == DockspacePresentationTransitionResult::TargetNotPresented
            ));
        } else {
            let report =
                present_surfaces_with_report(&mut session, &[MAIN_SURFACE, NATIVE_SOURCE_SURFACE]);
            assert!(matches!(
                report.presentation_transitions(),
                [terminal] if terminal.result() == DockspacePresentationTransitionResult::Applied
            ));
        }
    }
}

#[test]
fn no_op_selection_snapshot_probe() {
    for reselect_first in [false, true] {
        let mut session = session();
        if reselect_first {
            let mut frame = session.begin_host_frame().unwrap();
            frame.select_item_current(FIRST).unwrap();
            commit(frame);
        }
        let allocation = std::ptr::from_ref(session.workspace());
        structural_work::reset();
        let mut frame = session.begin_host_frame().unwrap();
        frame
            .open_item_current(FIRST, DockPlacement::Center(DockAnchor::Item(FIRST)))
            .unwrap();
        commit(frame);
        let builds = structural_work::snapshot().root_fingerprint_builds;
        assert_eq!(std::ptr::from_ref(session.workspace()), allocation);
        eprintln!("existing_open_after_reselect={reselect_first} root_fingerprint_builds={builds}");
        assert_eq!(builds > 0, reselect_first);
    }
    let mut session = session();
    assert!(session.view().item(FIRST).unwrap().is_selected());
    let before = session.version();
    // Both surfaces already have ready geometry before the no-op.
    present_surfaces(&mut session, &[MAIN_SURFACE, ROOTLESS_SURFACE]);
    let original_allocation = std::ptr::from_ref(session.workspace());
    structural_work::reset();
    let mut frame = session.begin_host_frame().unwrap();
    frame.select_item_current(FIRST).unwrap();
    let report = commit(frame);
    assert_eq!(report.after(), before);
    assert_ne!(std::ptr::from_ref(session.workspace()), original_allocation);
    eprintln!(
        "reselect_root_fingerprint_builds={}",
        structural_work::snapshot().root_fingerprint_builds
    );
    for turn in 1..=3 {
        structural_work::reset();
        commit(session.begin_host_frame().unwrap());
        let builds = structural_work::snapshot().root_fingerprint_builds;
        eprintln!("idle_after_reselect={turn} root_fingerprint_builds={builds}");
        // A truly input-free frame does not ask the reducer to rebuild.
        assert_eq!(builds, 0);
    }
    structural_work::reset();
    let mut frame = session.begin_host_frame().unwrap();
    frame.select_item_current(FIRST).unwrap();
    commit(frame);
    let builds = structural_work::snapshot().root_fingerprint_builds;
    eprintln!("second_reselect_root_fingerprint_builds={builds}");
    assert!(builds > 0);
}

#[test]
fn root_fingerprint_derivation_probe() {
    structural_work::reset();
    let mut session = session();
    let roots = session.workspace().roots().count();
    let builds = structural_work::snapshot().root_fingerprint_builds;
    eprintln!("initial_roots={roots} root_fingerprint_builds={builds}");
    assert_eq!(builds, 2 * roots);
    structural_work::reset();
    let mut frame = session.begin_host_frame().unwrap();
    frame
        .dock_item_current(FLOATING, DockPlacement::Center(DockAnchor::Item(FIRST)))
        .unwrap();
    let report = commit(frame);
    assert!(report.workspace_changed());
    let roots = session.workspace().roots().count();
    let builds = structural_work::snapshot().root_fingerprint_builds;
    eprintln!("dock_result_roots={roots} root_fingerprint_builds={builds}");
    assert!(builds >= 2 * roots);
}

#[test]
fn focus_query_uses_output_capacity_probe() {
    use crate::runtime::{SurfaceWork, SurfaceWorkDeferred};
    fn emit_b(session: &mut DockspaceSession) -> crate::runtime::PaintedSurfaceOutput {
        let mut frame = session.begin_host_frame().unwrap();
        let receipt = frame
            .paint_plan(ROOTLESS_SURFACE)
            .unwrap()
            .unwrap()
            .execute_surface_program(|_| Ok::<(), ()>(()))
            .unwrap();
        frame.confirm_surface_painted(receipt).unwrap();
        commit(frame).take_painted_outputs().pop().unwrap()
    }
    let mut session = session();
    session
        .configure_output_capacity(std::num::NonZeroUsize::new(2).unwrap())
        .unwrap();
    present_surfaces(&mut session, &[MAIN_SURFACE, ROOTLESS_SURFACE]);
    let first_output = emit_b(&mut session);
    let mut frame = session.begin_host_frame().unwrap();
    frame.select_item_current(FIRST).unwrap();
    commit(frame);
    let mut frame = session.begin_host_frame().unwrap();
    let plan = frame.paint_plan(MAIN_SURFACE).unwrap().unwrap();
    assert!(plan.pane_focus_request().is_some());
    assert!(matches!(
        frame.surface_work(ROOTLESS_SURFACE).unwrap(),
        SurfaceWork::Deferred(SurfaceWorkDeferred::OutputCapacity)
    ));
    eprintln!("focus_query_without_encoding_a_reserves_last_free_output_slot=true");
    drop(frame);

    let second_output = emit_b(&mut session);
    let mut frame = session.begin_host_frame().unwrap();
    frame.select_item_current(FIRST).unwrap();
    commit(frame);
    let mut frame = session.begin_host_frame().unwrap();
    assert!(matches!(
        frame.surface_work(MAIN_SURFACE).unwrap(),
        SurfaceWork::Deferred(SurfaceWorkDeferred::OutputCapacity)
    ));
    assert!(frame.paint_plan(MAIN_SURFACE).unwrap().is_none());
    eprintln!("focus_query_unavailable_while_b_output_inflight=true");
    drop(frame);
    drop(first_output);
    drop(second_output);
}

#[test]
fn configuration_encoding_order_probe() {
    use crate::runtime::{
        SurfacePaintProgramOperation as Operation, SurfacePaintProgramStep as Step,
    };
    let mut session = session();
    let old_height = session.presentation_config().tab_bar_height();
    let new_height = old_height + 16.0;
    let config = session
        .presentation_config()
        .to_builder()
        .tab_bar_height(new_height)
        .build()
        .unwrap();
    let mut frame = session.begin_host_frame().unwrap();
    frame.replace_presentation_config(config).unwrap();
    assert_eq!(frame.presentation_config().tab_bar_height(), old_height);
    frame
        .measure_surface(MAIN_SURFACE, presentation_metrics())
        .unwrap();
    let mut heights = Vec::new();
    let receipt = frame
        .paint_plan(MAIN_SURFACE)
        .unwrap()
        .unwrap()
        .execute_surface_program(|step| {
            if let Step::Operation {
                operation: Operation::TabBar(bar),
                ..
            } = step
            {
                heights.push(bar.bounds().height());
            }
            Ok::<(), ()>(())
        })
        .unwrap();
    assert!(heights.iter().all(|height| *height == old_height));
    assert!(!heights.is_empty());
    frame.confirm_surface_painted(receipt).unwrap();
    commit(frame);
    assert_eq!(session.presentation_config().tab_bar_height(), new_height);
    eprintln!("configuration_new_height={new_height} same_update_encoded_heights={heights:?}");
}

#[test]
fn unavailable_hidden_minimum_probe() {
    use crate::runtime::{
        SurfaceMeasurementAnswer as Answer, SurfaceMeasurementRequest as Request, SurfaceWork,
        TabStripMetrics,
    };
    for hidden_unavailable in [false, true] {
        let mut session = session();
        let mut frame = session.begin_host_frame().unwrap();
        frame
            .measure_surface_with(MAIN_SURFACE, |request| match request {
                Request::DockBounds { .. } | Request::PopupPlaneBounds { .. } => {
                    Answer::Bounds(presentation_bounds())
                }
                Request::PaneMinimum {
                    item: Some(SECOND), ..
                } if hidden_unavailable => {
                    Answer::Unavailable(SurfaceUnavailableReason::ContentUnavailable)
                }
                Request::PaneMinimum { .. } => {
                    Answer::PaneMinimum(LogicalSize::new(64.0, 48.0).unwrap())
                }
                Request::TabIntrinsic { .. } => Answer::TabIntrinsic(96.0),
                Request::TabStrip { .. } => {
                    Answer::TabStrip(TabStripMetrics::new(0.0, 0.0).unwrap())
                }
            })
            .unwrap();
        let ready = matches!(
            frame.surface_work(MAIN_SURFACE).unwrap(),
            SurfaceWork::Ready(_)
        );
        eprintln!("hidden_minimum_unavailable={hidden_unavailable} visible_surface_ready={ready}");
        assert_eq!(ready, !hidden_unavailable);
    }
}
