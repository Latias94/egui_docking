use dockspace::runtime::NativeSurfaceRole;
use winit::dpi::PhysicalPosition;
use winit::event::{
    DeviceId, ElementState, MouseButton, PointerEventFacts, PointerWindowRoute, WindowEvent,
};

use super::*;
use crate::event::NativePointerRouteSnapshot;

#[test]
fn destroyed_child_route_retires_only_after_tombstone_commit_and_quiescence() {
    let mut native = coordinator();
    let (first, second) = register_roots(&mut native);
    let first_window = WindowId::from(11);
    let second_window = WindowId::from(22);
    let child = ViewportId::from_hash_of("retired-native-child");
    native
        .bind_viewport(ViewportId::ROOT, first_window, first)
        .expect("root viewport binds");
    native
        .bind_viewport(child, second_window, second)
        .expect("child viewport binds");
    assert!(native.deferred_viewports.insert(
        child,
        second,
        PhysicalRect::new(100.0, 100.0, 400.0, 300.0).expect("test placement is valid"),
        NativeSurfaceRole::Child,
    ));

    let destroyed = {
        let mut viewports = native
            .viewports
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        NativeWindowEventRecord::for_test_ingress(
            14,
            second_window,
            None,
            WindowEvent::Destroyed,
            &mut viewports,
        )
    };
    assert_eq!(destroyed.viewport_id(), Some(child));
    assert_eq!(destroyed.binding(), Some(second));
    native
        .bridge
        .push_record(HostRecord::WindowEvent(destroyed));
    let late_pointer = NativeWindowEventRecord::for_test_snapshot(
        15,
        first_window,
        Some(ViewportId::ROOT),
        WindowEvent::CursorMoved {
            device_id: DeviceId::dummy(),
            position: PhysicalPosition::new(10.0, 20.0),
            facts: PointerEventFacts {
                hover: PointerWindowRoute::Window(second_window),
                capture: PointerWindowRoute::Window(second_window),
                ..PointerEventFacts::default()
            },
        },
        &native
            .viewports
            .lock()
            .unwrap_or_else(PoisonError::into_inner),
    );
    let routes = late_pointer
        .pointer_routes()
        .expect("the late event retains explicit pointer authority");
    assert_eq!(routes.delivery(), NativePointerRouteSnapshot::Dock(first));
    assert_eq!(routes.hover(), NativePointerRouteSnapshot::Unknown);
    assert_eq!(routes.capture(), NativePointerRouteSnapshot::Unknown);
    assert!(!late_pointer.references_binding(second));
    native
        .bridge
        .push_record(HostRecord::ViewportRoster(live_roster([first])));
    native
        .bridge
        .push_record(HostRecord::WindowEvent(late_pointer));

    assert!(
        native
            .reduce_callback_head()
            .expect("the exact destruction callback is retained")
    );
    let mut event_frame = native
        .begin_host_frame(|_| NativeReceiverAnswer::Unknown)
        .expect("the destruction callback boundary begins");
    event_frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .expect("the callback boundary settles every surface");
    event_frame
        .commit()
        .expect("the destruction callback boundary commits");
    assert_eq!(native.viewport_binding(child), Some(second));
    assert!(native.session.is_current_native_binding(second));
    assert!(
        native
            .deferred_viewport_specs()
            .iter()
            .all(|spec| spec.binding() != second),
        "the destruction callback stops re-declaring the retired child"
    );

    assert!(
        native
            .reduce_callback_head()
            .expect("the complete roster publishes the tombstone")
    );
    let mut roster_frame = native
        .begin_host_frame(|_| NativeReceiverAnswer::Unknown)
        .expect("the roster boundary begins");
    roster_frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .expect("the roster boundary settles every surface");
    let report = roster_frame
        .commit()
        .expect("the destroyed tombstone commits");
    assert!(native.settle_host_frame_inputs(report.inputs()));

    assert!(
        native
            .reduce_callback_head()
            .expect("the post-tombstone pointer edge remains reducible")
    );
    let mut pointer_frame = native
        .begin_host_frame(|_| NativeReceiverAnswer::Unknown)
        .expect("the post-tombstone pointer boundary begins");
    pointer_frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .expect("the pointer boundary settles every surface");
    pointer_frame
        .commit()
        .expect("the post-tombstone pointer boundary commits");

    let prepared = native
        .prepare_committed_retirements()
        .expect("the exact committed route prepares atomically")
        .expect("one exact route is ready to retire");
    drop(prepared);
    assert_eq!(native.viewport_binding(child), Some(second));
    let prepared = native
        .prepare_committed_retirements()
        .expect("an abandoned route retirement remains retryable")
        .expect("the exact route can be prepared again");

    let mut retirement_frame = native
        .begin_host_frame(|_| NativeReceiverAnswer::Unknown)
        .expect("the route retirement boundary begins");
    retirement_frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .expect("the route retirement boundary settles every surface");
    retirement_frame
        .commit()
        .expect("the route retirement boundary commits");
    native.commit_retirements(prepared);

    assert_eq!(native.viewport_binding(child), None);
    assert!(!native.session.is_current_native_binding(second));
    assert!(native.session.recognizes_native_binding(second));
    assert!(
        native
            .try_report_retirement_quiescence()
            .expect("the retired route becomes quiescent")
    );
    assert!(!native.session.recognizes_native_binding(second));

    let mut quiescence_frame = native
        .begin_host_frame(|_| NativeReceiverAnswer::Unknown)
        .expect("the quiescence boundary begins");
    quiescence_frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .expect("the quiescence boundary settles every surface");
    quiescence_frame
        .commit()
        .expect("the quiescence boundary commits");
}

#[test]
fn queued_pointer_routes_keep_every_referenced_binding_live() {
    let mut native = coordinator();
    let (delivery, captured) = register_roots(&mut native);
    let delivery_window = WindowId::from(11);
    let captured_window = WindowId::from(22);
    let captured_viewport = ViewportId::from_hash_of("captured-native-child");
    native
        .bind_viewport(ViewportId::ROOT, delivery_window, delivery)
        .expect("delivery viewport binds");
    native
        .bind_viewport(captured_viewport, captured_window, captured)
        .expect("captured viewport binds");

    let event = WindowEvent::MouseInput {
        device_id: DeviceId::dummy(),
        state: ElementState::Released,
        button: MouseButton::Left,
        facts: PointerEventFacts {
            hover: PointerWindowRoute::Window(captured_window),
            capture: PointerWindowRoute::Window(captured_window),
            ..PointerEventFacts::default()
        },
    };
    let record = NativeWindowEventRecord::for_test_snapshot(
        15,
        delivery_window,
        Some(ViewportId::ROOT),
        event,
        &native
            .viewports
            .lock()
            .unwrap_or_else(PoisonError::into_inner),
    );

    assert_eq!(record.binding(), Some(delivery));
    assert!(record.references_binding(captured));
    native.bridge.push_record(HostRecord::WindowEvent(record));
    assert!(native.bridge.references_binding(captured));
}
