//! Exact command witnesses remain independent of a shared window's historical surface tag.

use dockspace::geometry::{LogicalSize, PhysicalPoint, PhysicalRect, ScaleFactor};
use dockspace::model::{
    DockspaceLayout, DockspaceNode, DockspaceRootLayout, DockspaceSurfaceLayout, ItemId, RootId,
};
use dockspace::runtime::{
    HostWindowToken, NativePointerRoster, NativeSurfacePlacement, SurfaceUnavailableReason,
};
use eframe::{
    NativeViewportFocusStatus, NativeViewportPointerObservation,
    NativeViewportPointerPassthroughStatus, NativeViewportPropertyCommandRetirement, egui,
};
use winit::event::DeviceId;

use super::*;

fn fixture() -> (
    NativeHostBridge,
    NativeSurfaceBinding,
    [NativeSurfaceBinding; 2],
) {
    let layout = DockspaceLayout::new([1, 2].map(|id| {
        DockspaceSurfaceLayout::new(
            SurfaceId::new(id),
            DockspaceRootLayout::new(
                RootId::new(id),
                DockspaceNode::central_tabs([ItemId::new(id)]),
            ),
        )
    }))
    .unwrap();
    let mut session =
        DockspaceSession::from_layout(layout, dockspace::policy::DockPolicy::default()).unwrap();
    session
        .enable_managed_native_host(NativePointerRoster::Exact(Vec::new()))
        .unwrap();
    session
        .register_native_root(SurfaceId::new(1), HostWindowToken::new(1))
        .unwrap();
    let mut frame = session.begin_host_frame().unwrap();
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    let original = frame.commit().unwrap().native_bindings()[0];
    let placements = [1, 2].map(|id| {
        let origin = if id == 1 { 10.0 } else { 330.0 };
        NativeSurfacePlacement::new(
            SurfaceId::new(id),
            LogicalSize::new(300.0, 240.0).unwrap(),
            PhysicalPoint::new(origin, 10.0).unwrap(),
            ScaleFactor::new(1.0).unwrap(),
            PhysicalRect::new(origin, 10.0, 300.0, 240.0).unwrap(),
        )
    });
    let mut frame = session.begin_host_frame().unwrap();
    frame
        .set_native_surface_placements(original.window(), &placements)
        .unwrap();
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    frame.commit().unwrap();
    let bindings = session
        .native_surface_bindings_for_window(original.window())
        .collect::<Vec<_>>()
        .try_into()
        .unwrap();
    let bridge = NativeHostBridge::new(NativeViewportMap::default());
    bridge
        .bind_viewport_route(ViewportId::ROOT, WindowId::from(71), original)
        .unwrap();
    bridge.enable_observation_attachment_for_test();
    (bridge, original, bindings)
}

#[test]
fn shared_observation_receipt_keeps_the_exact_source_across_placement_tags_and_retirement() {
    let (bridge, original, [updated, sibling]) = fixture();
    let (_, foreign, _) = fixture();
    assert_ne!(updated, original);
    let viewport = ViewportId::ROOT;
    let window = WindowId::from(71);
    for binding in [sibling, updated] {
        let command = eframe::prepare_native_viewport_pointer_observation(
            viewport,
            window,
            DeviceId::dummy(),
            None,
        );
        let key = NativePointerObservationKey {
            token: command.token(),
            viewport,
            window,
            device: DeviceId::dummy(),
        };
        assert!(bridge.reserve_pointer_observation(key, binding));
        assert!(bridge.reserve_pointer_observation(key, binding));
        assert!(!bridge.reserve_pointer_observation(key, original));
        assert!(!bridge.reserve_pointer_observation(key, foreign));
        let wrong = NativePointerObservationRecord::for_test(
            key,
            original,
            NativeViewportPointerObservation::Unavailable,
            &bridge.lock().viewports,
        );
        assert!(!bridge.record_pointer_observation_for_test(wrong));
        let retirement = NativeViewportPropertyCommandRetirement::PointerObservation {
            viewport_id: viewport,
            token: key.token,
            expected_window_id: window,
            device_id: key.device,
        };
        assert_eq!(
            NativeHostHandler::on_viewport_property_command_retired(&bridge, retirement),
            NativeHostWake::RepaintRootAfterCurrent
        );
        let (record, _) = bridge.front_pointer_observation().unwrap();
        assert_eq!(record.binding(), binding);
        assert!(!bridge.record_pointer_observation_for_test(record));
        assert!(bridge.acknowledge_pointer_observation(key));
        bridge.commit_frame_boundary(false);
    }
    let records = bridge.lock();
    assert_eq!(
        records.window_resources.bindings().collect::<Vec<_>>(),
        [original]
    );
}

#[test]
fn shared_passthrough_callbacks_keep_each_exact_effect_source_in_one_physical_owner() {
    let (bridge, original, [updated, sibling]) = fixture();
    let (_, foreign, _) = fixture();
    let context = egui::Context::default();
    let viewport = ViewportId::ROOT;
    let window = WindowId::from(71);
    for (binding, retire) in [(sibling, false), (updated, true)] {
        let command = eframe::prepare_native_viewport_pointer_passthrough(viewport, false);
        let token = command.token();
        assert!(bridge.reserve_pointer_callback(viewport, binding, window, token, false));
        assert!(bridge.reserve_pointer_callback(viewport, binding, window, token, false));
        assert!(!bridge.reserve_pointer_callback(viewport, original, window, token, false));
        assert!(!bridge.reserve_pointer_callback(viewport, foreign, window, token, false));
        if retire {
            assert_eq!(
                NativeHostHandler::on_viewport_property_command_retired(
                    &bridge,
                    NativeViewportPropertyCommandRetirement::PointerPassthrough {
                        viewport_id: viewport,
                        token,
                        enabled: false,
                    },
                ),
                NativeHostWake::RepaintRootAfterCurrent
            );
        } else {
            assert!(bridge.lock().record_viewport_pointer_passthrough(
                NativePointerPassthroughRecord::for_test(
                    token,
                    viewport,
                    window,
                    Some(original),
                    false,
                    NativeViewportPointerPassthroughStatus::Applied,
                )
            ));
        }
        let mut records = bridge.lock();
        let HostRecord::ViewportPointerPassthrough(record) = records.front_record().unwrap() else {
            panic!("the exact effect terminal remains at the journal head");
        };
        assert_eq!(record.binding(), Some(binding));
        records.transfer_front_record();
        drop(records);
        bridge.commit_frame_boundary(false);
    }
    let focus = eframe::queue_native_viewport_focus(&context, viewport);
    assert!(bridge.reserve_focus_callback(viewport, sibling, window, focus));
    assert!(!bridge.reserve_focus_callback(viewport, foreign, window, focus));
    assert!(
        bridge
            .lock()
            .record_viewport_focus(NativeViewportFocusRecord::for_test(
                focus,
                viewport,
                window,
                Some(original),
                NativeViewportFocusStatus::Requested,
            ))
    );
    let records = bridge.lock();
    let HostRecord::ViewportFocus(record) = records.front_record().unwrap() else {
        panic!("focus is correlated to the exact physical window");
    };
    assert!(record.references_binding(sibling));
    assert!(!record.references_binding(foreign));
}
