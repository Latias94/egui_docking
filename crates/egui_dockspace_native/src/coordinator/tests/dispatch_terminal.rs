//! Independent work-area degradation and exact native terminal regression cases.

use dockspace::geometry::PhysicalPoint;

use super::*;
use crate::work_area::FrozenWorkAreaRoster;

/// Inject only the independently captured work-area validation error. All
/// window facts, destruction receipts, ingress ownership and core publication
/// still travel through the production roster compiler and settlement path.
fn record_invalid_work_areas(native: &mut NativeCoordinator) -> NativeViewportRosterEnvelope {
    let source = native.bridge.front_viewport_roster().unwrap();
    native
        .record_viewport_roster(
            source.roster(),
            source.clone(),
            Err(NativeHostProtocolError::InvalidWorkAreaRoster),
        )
        .unwrap();
    assert!(native.bridge.acknowledge_viewport_roster(&source));
    source
}

fn commit_snapshot(native: &mut NativeCoordinator) -> HostFrameReport {
    let mut frame = native
        .begin_host_frame(|_| NativeReceiverAnswer::Unknown)
        .unwrap();
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    let report = frame.commit().unwrap();
    assert!(native.settle_host_frame_inputs(report.inputs()).unwrap());
    report
}

#[test]
fn invalid_work_areas_keep_independent_exact_window_facts_and_revoke_only_geometry() {
    let (mut native, first, second) = presented_local_coordinate_routes();
    let bounds = PhysicalRect::new(0.0, 0.0, 1920.0, 1080.0).unwrap();
    let point = PhysicalPoint::new(1800.0, 800.0).unwrap();
    let facts = || {
        [
            CompiledWindowObservation::new(first, ready_window_facts(0.0)),
            CompiledWindowObservation::new(second, ready_window_facts(720.0)),
        ]
    };
    native
        .bridge
        .push_viewport_roster(NativeViewportRosterRecord::for_test_with_work_areas(
            facts(),
            FrozenWorkAreaRoster::for_test_exact([(
                1,
                bounds,
                bounds,
                ScaleFactor::new(1.0).unwrap(),
            )]),
        ));
    assert!(native.reduce_next_viewport_roster().unwrap());
    commit_snapshot(&mut native);
    assert!(native.work_area_for_point(point).is_some());
    assert!(native.session.native_local_coordinates(first).is_some());
    assert!(native.session.native_local_coordinates(second).is_some());

    native
        .bridge
        .push_viewport_roster(NativeViewportRosterRecord::for_test_with_work_areas(
            facts(),
            FrozenWorkAreaRoster::Unknown,
        ));
    let recorded = native.session.native_input_recorded_count();
    let source = record_invalid_work_areas(&mut native);
    assert_eq!(native.session.native_input_recorded_count(), recorded + 1);
    assert_eq!(native.queued_snapshot.as_ref().unwrap().roster, source);
    assert!(native.work_area_for_point(point).is_some());

    let version = native.session.version();
    let mut dropped = native
        .begin_host_frame(|_| NativeReceiverAnswer::Unknown)
        .unwrap();
    dropped
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    drop(dropped);
    assert_eq!(native.session.version(), version);
    assert_eq!(native.session.native_input_recorded_count(), recorded + 1);
    assert_eq!(native.queued_snapshot.as_ref().unwrap().roster, source);
    assert!(native.work_area_for_point(point).is_some());

    let report = commit_snapshot(&mut native);
    assert!(report.inputs().iter().any(|input| matches!(
        input,
        HostInputOutcome::NativePlatformSnapshotApplied { .. }
    )));
    assert_eq!(native.work_area_for_point(point), None);
    assert!(native.session.native_local_coordinates(first).is_some());
    assert!(native.session.native_local_coordinates(second).is_some());
    assert_eq!(native.session.native_input_recorded_count(), recorded + 1);
    assert!(native.queued_snapshot.is_none());
}

#[test]
fn invalid_work_areas_preserve_destroyed_settlement_and_exact_child_quiescence() {
    let mut native = coordinator();
    let (root, child) = register_root_and_child(&mut native);
    let viewport = ViewportId::from_hash_of("invalid-work-area-retirement");
    let window = WindowId::from(22);
    native
        .bind_viewport(ViewportId::ROOT, WindowId::from(11), root)
        .unwrap();
    native.bind_viewport(viewport, window, child).unwrap();
    let mut presented = paint_and_present_all_surfaces(&mut native);
    assert!(presented.take_native_effects().is_empty());

    let mut redock = native
        .begin_host_frame(|_| NativeReceiverAnswer::Unknown)
        .unwrap();
    redock
        .frame
        .dock_root_current(
            RootId::new(2),
            DockPlacement::Center(DockAnchor::Item(ItemId::new(1))),
        )
        .unwrap();
    redock
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    assert!(redock.commit().unwrap().take_native_effects().is_empty());
    let mut target_presented = paint_and_present_all_surfaces(&mut native);
    let effects = target_presented.take_native_effects();
    assert_eq!(effects.len(), 1);
    assert!(matches!(
        effects[0].operation(),
        NativeEffectOperation::ReleaseChild { binding } if *binding == child
    ));
    native
        .accept_native_effects(&egui::Context::default(), effects)
        .unwrap();

    let destroyed = native.bridge.with_viewports_mut(|routes| {
        NativeWindowEventRecord::for_test_ingress(
            98,
            window,
            Some(viewport),
            WindowEvent::Destroyed,
            routes,
        )
    });
    assert_eq!(destroyed.binding(), Some(child));
    native
        .bridge
        .push_record(HostRecord::WindowEvent(destroyed));
    assert!(
        native
            .reduce_callback_prefix(&egui::Context::default(), &mut test_budget())
            .unwrap()
            .is_some()
    );
    callback_prefix::commit_boundary(&mut native);
    let exact = native.bridge.retirement_destroyed_observations();
    assert_eq!(exact.len(), 1);
    assert_eq!(exact[0].0, child);
    assert_ne!(exact[0].1, NativeWindowFacts::destroyed());
    assert_eq!(native.lifecycle_progress().destroyed_observations(), 1);
    assert_eq!(native.viewport_binding(viewport), Some(child));

    native.bridge.push_viewport_roster(live_roster([root]));
    let recorded = native.session.native_input_recorded_count();
    let source = record_invalid_work_areas(&mut native);
    assert_eq!(native.session.native_input_recorded_count(), recorded + 1);
    assert_eq!(native.queued_snapshot.as_ref().unwrap().roster, source);
    assert_eq!(native.viewport_binding(viewport), Some(child));
    assert!(native.session.recognizes_native_binding(child));

    let version = native.session.version();
    drop(
        native
            .begin_host_frame(|_| NativeReceiverAnswer::Unknown)
            .unwrap(),
    );
    assert_eq!(native.session.version(), version);
    assert_eq!(native.queued_snapshot.as_ref().unwrap().roster, source);
    assert_eq!(native.session.native_input_recorded_count(), recorded + 1);
    assert_eq!(native.viewport_binding(viewport), Some(child));

    let mut report = commit_snapshot(&mut native);
    assert!(report.take_native_effects().is_empty());
    assert!(native.bridge.retirement_destroyed_observations().is_empty());
    let prepared = native
        .prepare_committed_retirements()
        .unwrap()
        .expect("the exact tombstone permits retiring its original route");
    callback_prefix::commit_boundary(&mut native);
    assert!(native.commit_retirements(prepared).is_empty());
    assert_eq!(native.viewport_binding(viewport), None);
    assert!(native.try_report_retirement_quiescence().unwrap());
    assert!(!native.session.recognizes_native_binding(child));
    assert!(native.session.is_current_native_binding(root));
    assert_eq!(native.viewport_binding(ViewportId::ROOT), Some(root));
    assert_eq!(native.lifecycle_progress().quiescent_retirements(), 1);
    assert_eq!(native.lifecycle_progress().destroyed_observations(), 1);
}
