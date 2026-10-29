use super::*;

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
