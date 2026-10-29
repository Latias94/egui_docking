use super::*;
use std::num::NonZeroUsize;

use dockspace::runtime::{NativeInputStorageLimit, NativeProducerStorageLimit};

use super::callback_prefix::commit_boundary;
use crate::work_area::FrozenWorkAreaRoster;

fn snapshot_limit(windows: usize, work_areas: usize) -> NativeSnapshotStorageLimit {
    NativeSnapshotStorageLimit {
        window_slots: windows,
        work_area_slots: work_areas,
        byte_limit: NativeSnapshotStorageLimit::required_bytes(windows, work_areas).unwrap(),
    }
}

fn bounded_snapshot_coordinator() -> NativeCoordinator {
    let layout = DockspaceLayout::new([
        DockspaceSurfaceLayout::new(
            FIRST_SURFACE,
            DockspaceRootLayout::new(
                RootId::new(1),
                DockspaceNode::central_tabs([ItemId::new(1)]),
            ),
        ),
        DockspaceSurfaceLayout::new(
            SECOND_SURFACE,
            DockspaceRootLayout::new(RootId::new(2), DockspaceNode::tabs([ItemId::new(2)])),
        ),
    ])
    .unwrap();
    NativeCoordinator::new_with_storage_limits(
        DockspaceSession::from_layout(layout, DockPolicy::default()).unwrap(),
        NativePointerRoster::Exact(Vec::new()),
        Some(
            NativeInputStorageLimit::new(NonZeroUsize::new(4).unwrap(), 1024 * 1024)
                .with_producer_storage_limit(NativeProducerStorageLimit::new(
                    NonZeroUsize::new(2).unwrap(),
                    NonZeroUsize::MIN,
                    1024 * 1024,
                )),
        ),
        None,
    )
    .unwrap()
}

fn work_area_roster(count: u64) -> FrozenWorkAreaRoster {
    FrozenWorkAreaRoster::for_test_exact((1..=count).map(|display| {
        let bounds = PhysicalRect::new(0.0, 0.0, 1920.0, 1080.0).unwrap();
        (display, bounds, bounds, ScaleFactor::new(1.0).unwrap())
    }))
}

#[test]
fn snapshot_configuration_checks_the_complete_peak_before_installing_storage() {
    let mut native = coordinator();
    let before = native.work_areas.owned_data_capacity_bytes();
    let version = native.session.version();
    let recorded = native.session.native_input_recorded_count();
    let mut limit = snapshot_limit(2, 1);
    limit.byte_limit -= 1;
    assert!(
        native
            .configure_snapshot_storage_limit(limit)
            .unwrap_err()
            .is_input_capacity()
    );
    assert!(native.snapshot_storage.is_none());
    assert_eq!(native.work_areas.owned_data_capacity_bytes(), before);
    assert_eq!(native.session.version(), version);
    assert_eq!(native.session.native_input_recorded_count(), recorded);
    assert!(NativeSnapshotStorageLimit::required_bytes(usize::MAX, 1).is_none());

    native
        .configure_snapshot_storage_limit(snapshot_limit(2, 1))
        .unwrap();
    assert!(native.snapshot_storage.is_some());
    assert!(native.work_areas.owned_data_capacity_bytes() > before);
    assert!(
        native
            .configure_snapshot_storage_limit(snapshot_limit(2, 1))
            .is_err()
    );
}

#[test]
fn finite_snapshot_keeps_its_original_source_and_prepared_backing_through_capacity_and_drop_retry()
{
    let mut native = bounded_snapshot_coordinator();
    let (first, second) = register_roots(&mut native);
    assert_eq!(native.snapshot_storage.unwrap().window_slots, 2);
    assert_eq!(native.snapshot_storage.unwrap().work_area_slots, 1);
    let recorded = native.session.native_input_recorded_count();
    for _ in 0..4 {
        native.session.report_native_inventory_unknown().unwrap();
    }
    native
        .bridge
        .push_viewport_roster(NativeViewportRosterRecord::for_test_with_work_areas(
            [first, second]
                .map(|binding| CompiledWindowObservation::new(binding, NativeWindowFacts::live())),
            work_area_roster(1),
        ));
    let source = native.bridge.front_viewport_roster().unwrap();
    let version = native.session.version();
    let work_area_bytes = native.work_areas.owned_data_capacity_bytes();
    assert!(
        native
            .reduce_next_viewport_roster()
            .unwrap_err()
            .is_input_capacity()
    );
    assert_eq!(
        native.bridge.front_viewport_roster().as_ref(),
        Some(&source)
    );
    assert!(!native.bridge.callback_boundary_pending());
    assert!(native.queued_snapshot.is_none());
    assert_eq!(native.session.version(), version);
    assert_eq!(native.session.native_input_recorded_count(), recorded + 4);
    assert_eq!(
        native.work_areas.owned_data_capacity_bytes(),
        work_area_bytes
    );

    // The coordinator's public reduction boundary converts temporary core
    // saturation into a commit barrier while retaining the raw roster head.
    assert!(
        native
            .reduce_callback_prefix(&egui::Context::default(), &mut test_budget())
            .unwrap()
            .is_none()
    );
    assert!(native.bridge.callback_boundary_pending());
    assert_eq!(
        native.bridge.front_viewport_roster().as_ref(),
        Some(&source)
    );
    commit_boundary(&mut native);
    assert!(native.reduce_next_viewport_roster().unwrap());
    assert_eq!(native.session.native_input_recorded_count(), recorded + 5);
    let queued = native.queued_snapshot.as_ref().unwrap();
    assert_eq!(queued.roster, source);
    assert_eq!(queued.presentation_acknowledgements.capacity(), 2);
    assert_eq!(queued.focus_reportable.capacity(), 2);
    let acknowledgements = queued.presentation_acknowledgements.as_ptr();
    let focus = queued.focus_reportable.as_ptr();
    let prepared_bytes = queued.work_areas.owned_data_capacity_bytes();
    let version = native.session.version();
    let mut frame = native
        .begin_host_frame(|_| NativeReceiverAnswer::Unknown)
        .unwrap();
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    drop(frame);
    assert_eq!(native.session.version(), version);
    assert_eq!(native.session.native_input_recorded_count(), recorded + 5);
    let queued = native.queued_snapshot.as_ref().unwrap();
    assert_eq!(
        queued.presentation_acknowledgements.as_ptr(),
        acknowledgements
    );
    assert_eq!(queued.focus_reportable.as_ptr(), focus);
    assert_eq!(
        queued.work_areas.owned_data_capacity_bytes(),
        prepared_bytes
    );

    let mut frame = native
        .begin_host_frame(|_| NativeReceiverAnswer::Unknown)
        .unwrap();
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    let report = frame.commit().unwrap();
    assert_eq!(
        report
            .inputs()
            .iter()
            .filter(|outcome| matches!(
                outcome,
                HostInputOutcome::NativePlatformSnapshotApplied { .. }
            ))
            .count(),
        1
    );
    assert!(native.settle_host_frame_report(&report).unwrap());
    assert!(native.queued_snapshot.is_none());
    assert!(!native.reduce_next_viewport_roster().unwrap());
    assert_eq!(native.session.native_input_recorded_count(), recorded + 5);
}

#[test]
fn oversized_snapshot_sidecar_preserves_raw_head_without_recording_unknown() {
    let mut native = bounded_snapshot_coordinator();
    let (first, second) = register_roots(&mut native);
    native
        .bridge
        .push_viewport_roster(NativeViewportRosterRecord::for_test_with_work_areas(
            [first, second]
                .map(|binding| CompiledWindowObservation::new(binding, NativeWindowFacts::live())),
            work_area_roster(2),
        ));
    let source = native.bridge.front_viewport_roster().unwrap();
    let recorded = native.session.native_input_recorded_count();
    let version = native.session.version();
    assert!(
        native
            .reduce_next_viewport_roster()
            .unwrap_err()
            .is_input_capacity()
    );
    assert_eq!(
        native.bridge.front_viewport_roster().as_ref(),
        Some(&source)
    );
    assert!(native.queued_snapshot.is_none());
    assert_eq!(native.session.native_input_recorded_count(), recorded);
    assert_eq!(native.session.version(), version);
    assert!(!native.session.has_pending_native_input());
}

#[test]
fn finite_snapshot_reserves_the_complete_live_and_destroyed_merge() {
    let mut native = bounded_snapshot_coordinator();
    let (first, second) = register_roots(&mut native);
    let window = WindowId::from(11);
    native
        .bind_viewport(ViewportId::ROOT, window, first)
        .unwrap();
    native
        .bridge
        .push_record(HostRecord::WindowEvent(NativeWindowEventRecord::for_test(
            1,
            window,
            Some(ViewportId::ROOT),
            Some(first),
            WindowEvent::Destroyed,
        )));
    assert!(
        native
            .reduce_callback_prefix(&egui::Context::default(), &mut test_budget())
            .unwrap()
            .is_some()
    );
    commit_boundary(&mut native);
    assert_eq!(native.bridge.retirement_destroyed_observations().len(), 1);
    native.bridge.push_viewport_roster(live_roster([second]));
    assert!(native.reduce_next_viewport_roster().unwrap());
    let queued = native.queued_snapshot.as_ref().unwrap();
    assert_eq!(queued.presentation_acknowledgements.capacity(), 2);
    assert_eq!(queued.focus_reportable.capacity(), 2);
    let mut frame = native
        .begin_host_frame(|_| NativeReceiverAnswer::Unknown)
        .unwrap();
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    let report = frame.commit().unwrap();
    assert!(report.inputs().iter().any(|outcome| matches!(
        outcome,
        HostInputOutcome::NativePlatformSnapshotApplied { .. }
    )));
    assert!(native.settle_host_frame_report(&report).unwrap());
    assert!(native.bridge.retirement_destroyed_observations().is_empty());
}

#[test]
fn owned_input_capacity_preserves_source_backing_across_transfer_and_reclaims_core_payloads() {
    let mut native = coordinator();
    let (first, second) = register_roots(&mut native);
    let before = native.owned_input_capacity_bytes().unwrap();
    let frame = native
        .begin_host_frame(|_| NativeReceiverAnswer::Unknown)
        .unwrap();
    drop(frame);
    assert!(native.owned_input_capacity_bytes().unwrap() <= before);
    let baseline = native.owned_input_capacity_bytes().unwrap();
    let window = WindowId::from(11);
    native
        .bind_viewport(ViewportId::ROOT, window, first)
        .unwrap();
    let routed = native.owned_input_capacity_bytes().unwrap();
    assert!(routed > baseline);
    native.bridge.push_record(HostRecord::WindowEvent(
        crate::event::NativeWindowEventRecord::for_test(
            1,
            window,
            Some(ViewportId::ROOT),
            Some(first),
            WindowEvent::CursorMoved {
                device_id: winit::event::DeviceId::dummy(),
                position: winit::dpi::PhysicalPosition::new(10.0, 20.0),
                facts: winit::event::PointerEventFacts::default(),
            },
        ),
    ));
    assert_eq!(native.owned_input_capacity_bytes().unwrap(), routed);
    let context = egui::Context::default();
    assert!(
        native
            .reduce_callback_prefix(&context, &mut test_budget())
            .unwrap()
            .is_some()
    );
    let transferred = native.owned_input_capacity_bytes().unwrap();
    assert!(
        transferred > routed,
        "core and translator data coexist with preallocated raw backing"
    );
    let mut frame = native
        .begin_host_frame(|_| NativeReceiverAnswer::Unknown)
        .unwrap();
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    let report = frame.commit().unwrap();
    assert!(native.settle_host_frame_report(&report).is_ok());
    let committed = native.owned_input_capacity_bytes().unwrap();
    let frame = native
        .begin_host_frame(|_| NativeReceiverAnswer::Unknown)
        .unwrap();
    drop(frame);
    assert!(native.owned_input_capacity_bytes().unwrap() < committed);
    assert!(native.session.is_current_native_binding(second));
}

#[test]
fn queued_snapshot_sidecars_charge_spare_acknowledgement_and_focus_capacity() {
    let mut native = coordinator();
    let (first, second) = register_roots(&mut native);
    native
        .bridge
        .push_viewport_roster(NativeViewportRosterRecord::for_test(
            [first, second]
                .into_iter()
                .map(|binding| CompiledWindowObservation::new(binding, NativeWindowFacts::live())),
            true,
        ));
    assert!(native.reduce_next_viewport_roster().unwrap());
    let before = native.owned_input_capacity_bytes().unwrap();
    let queued = native.queued_snapshot.as_mut().unwrap();
    let initial_slots =
        queued.presentation_acknowledgements.capacity() + queued.focus_reportable.capacity();
    queued.presentation_acknowledgements.reserve_exact(11);
    queued.focus_reportable.reserve_exact(13);
    let added_slots = queued.presentation_acknowledgements.capacity()
        + queued.focus_reportable.capacity()
        - initial_slots;
    assert!(added_slots > 0);
    let retained = native.owned_input_capacity_bytes().unwrap();
    assert_eq!(
        retained - before,
        added_slots * size_of::<NativeSurfaceBinding>()
    );
    let queued = native.queued_snapshot.as_mut().unwrap();
    queued.presentation_acknowledgements.clear();
    queued.focus_reportable.clear();
    assert_eq!(
        native.owned_input_capacity_bytes().unwrap(),
        retained,
        "empty sidecars retain backing until their exact snapshot owner is released"
    );
}
