use dockspace::model::{
    DockspaceLayout, DockspaceNode, DockspaceRootLayout, DockspaceSurfaceLayout, ItemId, RootId,
    SurfaceId,
};
use dockspace::policy::DockPolicy;
use dockspace::runtime::{HostInputOutcome, SurfaceUnavailableReason};
use winit::event::WindowEvent;

use super::*;

const FIRST_SURFACE: SurfaceId = SurfaceId::new(1);
const SECOND_SURFACE: SurfaceId = SurfaceId::new(2);

fn coordinator() -> NativeCoordinator {
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
    .expect("test layout validates");
    let session = DockspaceSession::from_layout(layout, DockPolicy::default())
        .expect("test session initializes");
    NativeCoordinator::new(session, NativePointerRoster::Exact(Vec::new()))
        .expect("managed coordinator enrolls")
}

fn register_roots(
    coordinator: &mut NativeCoordinator,
) -> (NativeSurfaceBinding, NativeSurfaceBinding) {
    coordinator
        .register_native_root(FIRST_SURFACE, HostWindowToken::new(11))
        .expect("first root registration queues");
    coordinator
        .register_native_root(SECOND_SURFACE, HostWindowToken::new(22))
        .expect("second root registration queues");
    let mut frame = coordinator
        .begin_host_frame(|_| NativeReceiverAnswer::Unknown)
        .expect("registration frame begins");
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .expect("test host settles every surface");
    let report = frame.commit().expect("registration frame commits");
    let bindings: Vec<_> = report
        .inputs()
        .iter()
        .filter_map(|outcome| match outcome {
            HostInputOutcome::NativeSurfaceRegistered { binding } => Some(*binding),
            _ => None,
        })
        .collect();
    assert_eq!(bindings.len(), 2);
    let first = bindings
        .iter()
        .copied()
        .find(|binding| binding.surface() == FIRST_SURFACE)
        .expect("first binding emitted");
    let second = bindings
        .iter()
        .copied()
        .find(|binding| binding.surface() == SECOND_SURFACE)
        .expect("second binding emitted");
    (first, second)
}

#[test]
fn coordinator_owns_registration_and_viewport_identity() {
    let mut native = coordinator();
    let (first, second) = register_roots(&mut native);
    let child = ViewportId::from_hash_of("second-native-surface");

    native
        .bind_viewport(ViewportId::ROOT, WindowId::from(11), first)
        .expect("root viewport binds");
    native
        .bind_viewport(child, WindowId::from(22), second)
        .expect("child viewport binds");

    assert_eq!(native.viewport_binding(ViewportId::ROOT), Some(first));
    assert_eq!(native.surface_viewport(SECOND_SURFACE), Some(child));
    assert_eq!(
        native
            .unbind_viewport(child, second)
            .expect("exact child binding retires"),
        second
    );
    assert_eq!(native.surface_viewport(SECOND_SURFACE), None);
}

#[test]
fn viewport_reservation_requires_exact_window_attachment() {
    let mut native = coordinator();
    let (_, second) = register_roots(&mut native);
    let child = ViewportId::from_hash_of("reserved-native-surface");
    let window = WindowId::from(22);

    native
        .reserve_viewport(child, second)
        .expect("child viewport reserves before OS creation");
    assert_eq!(native.viewport_binding(child), Some(second));
    assert_eq!(
        native
            .viewports
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .binding_for_event(window, Some(child)),
        None
    );

    native
        .attach_viewport_window(child, second, window)
        .expect("exact created window attaches");
    assert_eq!(
        native
            .viewports
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .binding_for_event(window, Some(child)),
        Some(second)
    );
    assert!(matches!(
        native.attach_viewport_window(child, second, WindowId::from(23)),
        Err(NativeViewportBindingError::ViewportWindowAlreadyAttached {
            viewport,
            existing,
        }) if viewport == child && existing == window
    ));
}

#[test]
fn viewport_mapping_rejects_aliasing_without_mutation() {
    let mut native = coordinator();
    let (first, second) = register_roots(&mut native);
    let child = ViewportId::from_hash_of("second-native-surface");
    native
        .bind_viewport(ViewportId::ROOT, WindowId::from(11), first)
        .expect("root viewport binds");
    native
        .bind_viewport(child, WindowId::from(22), second)
        .expect("child viewport binds");

    assert!(matches!(
        native.bind_viewport(ViewportId::ROOT, WindowId::from(22), second),
        Err(NativeViewportBindingError::ViewportAlreadyBound {
            viewport: ViewportId::ROOT,
            existing: FIRST_SURFACE,
        })
    ));
    assert!(matches!(
        native.bind_viewport(child, WindowId::from(11), first),
        Err(NativeViewportBindingError::ViewportAlreadyBound {
            viewport,
            existing: SECOND_SURFACE,
        }) if viewport == child
    ));
    assert_eq!(native.viewport_binding(ViewportId::ROOT), Some(first));
    assert_eq!(native.viewport_binding(child), Some(second));
}

#[test]
fn viewport_mapping_rejects_a_foreign_same_surface_binding() {
    let mut current_coordinator = coordinator();
    let (current, _) = register_roots(&mut current_coordinator);
    let mut foreign = coordinator();
    let (foreign_same_surface, _) = register_roots(&mut foreign);

    assert_eq!(current.surface(), foreign_same_surface.surface());
    assert_ne!(current, foreign_same_surface);
    assert!(matches!(
        current_coordinator.bind_viewport(
            ViewportId::ROOT,
            WindowId::from(11),
            foreign_same_surface,
        ),
        Err(NativeViewportBindingError::BindingNotCurrent {
            viewport: ViewportId::ROOT,
            surface: FIRST_SURFACE,
        })
    ));
    assert_eq!(current_coordinator.viewport_binding(ViewportId::ROOT), None);
}

#[test]
fn stale_unbind_cannot_remove_the_current_viewport_binding() {
    let mut native = coordinator();
    let (first, second) = register_roots(&mut native);
    let child = ViewportId::from_hash_of("second-native-surface");
    native
        .bind_viewport(child, WindowId::from(22), second)
        .expect("child viewport binds");

    assert!(matches!(
        native.unbind_viewport(child, first),
        Err(NativeViewportBindingError::BindingMismatch { viewport, .. })
            if viewport == child
    ));
    assert_eq!(native.viewport_binding(child), Some(second));
}

#[test]
fn reserved_replacement_revokes_the_predecessor_window_route() {
    let mut native = coordinator();
    let (_, predecessor) = register_roots(&mut native);
    let child = ViewportId::from_hash_of("replacement-native-surface");
    let predecessor_window = WindowId::from(22);
    native
        .bind_viewport(child, predecessor_window, predecessor)
        .expect("predecessor binds");

    let mut successor_source = coordinator();
    let (_, successor) = register_roots(&mut successor_source);
    native
        .viewports
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .reserve_replacement(child, predecessor, successor)
        .expect("successor reservation replaces the exact predecessor");

    let viewports = native
        .viewports
        .lock()
        .unwrap_or_else(PoisonError::into_inner);
    assert_eq!(viewports.binding(child), Some(successor));
    assert_eq!(
        viewports.binding_for_event(predecessor_window, Some(child)),
        None
    );
}

#[test]
fn viewport_create_failure_freezes_binding_and_blocks_the_boundary() {
    let mut native = coordinator();
    let (_, failed_binding) = register_roots(&mut native);
    let child = ViewportId::from_hash_of("failed-native-surface");
    native
        .reserve_viewport(child, failed_binding)
        .expect("failed viewport was reserved before scheduling");
    assert!(native.bridge.reserve_create_for_test(child, failed_binding));
    assert_eq!(
        native.bridge.record_viewport_create_failure_for_test(child),
        NativeHostWake::RepaintRoot
    );
    assert_eq!(
        native.bridge.record_viewport_create_failure_for_test(child),
        NativeHostWake::RepaintRoot,
        "duplicate backend retries retain one terminal callback record"
    );

    let failure = native
        .next_viewport_create_failure()
        .expect("failure prefix is readable")
        .expect("failed create is retained");
    assert_eq!(failure.viewport(), child);
    assert_eq!(failure.binding(), failed_binding);
    assert_eq!(
        native
            .begin_host_frame(|_| NativeReceiverAnswer::Unknown)
            .expect_err("unacknowledged create failure blocks the host frame")
            .kind(),
        crate::NativeRuntimeErrorKind::HostProtocol
    );

    let mut successor_source = coordinator();
    let (_, successor) = register_roots(&mut successor_source);
    native
        .viewports
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .reserve_replacement(child, failed_binding, successor)
        .expect("test replaces the viewport after the callback");
    assert_eq!(failure.binding(), failed_binding);

    native
        .acknowledge_viewport_create_failure(failure)
        .expect("exact frozen failure is acknowledged");
    assert_eq!(
        native.bridge.create_binding(child),
        None,
        "acknowledgement clears the exact failed create reservation"
    );
    assert_eq!(native.viewport_binding(child), Some(successor));
    assert!(
        native
            .next_viewport_create_failure()
            .expect("failure prefix remains readable")
            .is_none(),
        "duplicate failure callbacks are coalesced"
    );
    let mut frame = native
        .begin_host_frame(|_| NativeReceiverAnswer::Unknown)
        .expect("acknowledged failure releases the callback boundary");
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .expect("test host settles every surface");
    frame.commit().expect("host frame commits");
}

#[test]
fn stale_failure_acknowledgement_cannot_clear_a_successor_create_reservation() {
    let mut native = coordinator();
    let (_, failed_binding) = register_roots(&mut native);
    let child = ViewportId::from_hash_of("failed-native-surface-aba");
    native
        .reserve_viewport(child, failed_binding)
        .expect("failed viewport was reserved before scheduling");
    assert!(native.bridge.reserve_create(child, failed_binding));
    assert_eq!(
        native.bridge.record_viewport_create_failure_for_test(child),
        NativeHostWake::RepaintRoot
    );
    let failure = native
        .next_viewport_create_failure()
        .expect("failure prefix is readable")
        .expect("failed create is retained");

    let mut successor_source = coordinator();
    let (_, successor) = register_roots(&mut successor_source);
    native
        .viewports
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .reserve_replacement(child, failed_binding, successor)
        .expect("successor replaces the failed route");
    assert!(native.bridge.clear_create(child, failed_binding));
    assert!(native.bridge.reserve_create(child, successor));

    native
        .acknowledge_viewport_create_failure(failure)
        .expect("stale failure is acknowledged without touching the successor");

    assert_eq!(native.bridge.create_binding(child), Some(successor));
    assert_eq!(native.viewport_binding(child), Some(successor));
}

#[test]
fn output_reservation_binds_only_after_the_ui_callback_updates_the_viewport() {
    let mut native = coordinator();
    let (first, second) = register_roots(&mut native);
    let mut reservation = OutputReservation::unbound();

    assert_eq!(reservation.binding(), None);
    assert!(reservation.attach(second));
    assert_eq!(reservation.binding(), Some(second));
    assert!(reservation.attach(second));
    assert!(!reservation.attach(first));
}

#[test]
fn staged_viewport_output_uses_reserved_binding_before_window_attachment() {
    let mut native = coordinator();
    let (_, binding) = register_roots(&mut native);
    let viewport = ViewportId::from_hash_of("staged-output");
    let window = WindowId::from(99);

    native
        .reserve_viewport(viewport, binding)
        .expect("the deferred viewport reserves its exact binding");

    let viewports = native
        .viewports
        .lock()
        .unwrap_or_else(PoisonError::into_inner);
    assert_eq!(
        viewports.binding_for_event(window, Some(viewport)),
        None,
        "the reverse window route is intentionally absent before attachment"
    );
    assert_eq!(
        viewports.binding_for_output(viewport, window),
        Some(binding),
        "the staged viewport route remains authoritative for its first output"
    );
}

#[test]
fn window_event_remains_pending_until_exact_acknowledgement() {
    let mut coordinator = coordinator();
    let (first, _) = register_roots(&mut coordinator);
    let window = WindowId::from(11);
    coordinator
        .bind_viewport(ViewportId::ROOT, window, first)
        .expect("root viewport binds");
    let event = NativeWindowEventRecord::for_test(
        7,
        window,
        Some(ViewportId::ROOT),
        Some(first),
        WindowEvent::Focused(true),
    );
    coordinator
        .bridge
        .push_record(HostRecord::WindowEvent(event));

    let pending = coordinator
        .next_window_event()
        .expect("journal inspection succeeds")
        .expect("event remains visible");
    assert_eq!(pending.ordinal(), 7);
    assert_eq!(pending.binding(), Some(first));
    assert_eq!(
        coordinator
            .begin_host_frame(|_| NativeReceiverAnswer::Unknown)
            .expect_err("unacknowledged event blocks the core boundary")
            .kind(),
        crate::NativeRuntimeErrorKind::HostProtocol
    );
    assert_eq!(
        coordinator
            .acknowledge_window_event(8)
            .expect_err("wrong event identity is rejected")
            .kind(),
        crate::NativeRuntimeErrorKind::HostProtocol
    );
    assert_eq!(
        coordinator
            .next_window_event()
            .expect("journal remains readable")
            .map(|event| event.ordinal()),
        Some(7)
    );
    coordinator
        .acknowledge_window_event(7)
        .expect("exact event acknowledgement advances the journal");
    assert!(coordinator.bridge.event_boundary_pending());

    let mut frame = coordinator
        .begin_host_frame(|_| NativeReceiverAnswer::Unknown)
        .expect("acknowledged journal permits the next core boundary");
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .expect("test host settles every surface");
    frame.commit().expect("frame commits");
    assert!(!coordinator.bridge.event_boundary_pending());
}

#[test]
fn pointer_routes_are_frozen_before_viewport_replacement() {
    use winit::dpi::PhysicalPosition;
    use winit::event::{
        DeviceId, ElementState, MouseButton, PointerEventFacts, PointerWindowRoute,
    };

    use crate::event::NativePointerRouteSnapshot;

    let mut native = coordinator();
    let (first, second) = register_roots(&mut native);
    let first_window = WindowId::from(11);
    let second_window = WindowId::from(22);
    let child = ViewportId::from_hash_of("second-native-surface");
    native
        .bind_viewport(ViewportId::ROOT, first_window, first)
        .expect("root viewport binds");
    native
        .bind_viewport(child, second_window, second)
        .expect("child viewport binds");

    let event = WindowEvent::MouseInput {
        device_id: DeviceId::dummy(),
        state: ElementState::Released,
        button: MouseButton::Left,
        facts: PointerEventFacts {
            surface_position: Some(PhysicalPosition::new(10.0, 20.0)),
            desktop_position: Some(PhysicalPosition::new(110.0, 220.0)),
            modifiers: None,
            hover: PointerWindowRoute::Window(second_window),
            capture: PointerWindowRoute::Window(first_window),
        },
    };
    let record = {
        let viewports = native
            .viewports
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        NativeWindowEventRecord::for_test_snapshot(
            7,
            first_window,
            Some(ViewportId::ROOT),
            event,
            &viewports,
        )
    };

    let mut successor_source = coordinator();
    let (successor, _) = register_roots(&mut successor_source);
    native
        .viewports
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .replace(ViewportId::ROOT, first, WindowId::from(33), successor)
        .expect("test viewport replacement succeeds");

    let routes = record.pointer_routes().expect("pointer routes are frozen");
    assert_eq!(routes.delivery(), NativePointerRouteSnapshot::Dock(first));
    assert_eq!(routes.hover(), NativePointerRouteSnapshot::Dock(second));
    assert_eq!(routes.capture(), NativePointerRouteSnapshot::Dock(first));
    assert_ne!(
        routes.delivery(),
        NativePointerRouteSnapshot::Dock(successor)
    );
}

#[test]
fn pointer_translation_preserves_delivery_hover_and_capture() {
    use dockspace::geometry::PhysicalPoint;
    use dockspace::runtime::{
        NativeDesktopPointerLocation, NativeDesktopPosition, NativePointerButton,
        NativePointerEvent, NativePointerHover, NativePointerId, NativePointerInput,
        NativePointerOwner,
    };
    use winit::dpi::PhysicalPosition;
    use winit::event::{
        DeviceId, ElementState, MouseButton, PointerEventFacts, PointerWindowRoute,
    };

    use crate::pointer_event::{NativePointerTranslation, NativePointerTranslator};

    let mut native = coordinator();
    let (first, second) = register_roots(&mut native);
    let first_window = WindowId::from(11);
    let second_window = WindowId::from(22);
    let child = ViewportId::from_hash_of("second-native-surface");
    native
        .bind_viewport(ViewportId::ROOT, first_window, first)
        .expect("root viewport binds");
    native
        .bind_viewport(child, second_window, second)
        .expect("child viewport binds");
    let event = WindowEvent::MouseInput {
        device_id: DeviceId::dummy(),
        state: ElementState::Released,
        button: MouseButton::Left,
        facts: PointerEventFacts {
            surface_position: Some(PhysicalPosition::new(10.0, 20.0)),
            desktop_position: Some(PhysicalPosition::new(110.0, 220.0)),
            modifiers: None,
            hover: PointerWindowRoute::Window(second_window),
            capture: PointerWindowRoute::Window(first_window),
        },
    };
    let record = {
        let viewports = native
            .viewports
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        NativeWindowEventRecord::for_test_snapshot(
            8,
            first_window,
            Some(ViewportId::ROOT),
            event,
            &viewports,
        )
    };

    let mut translator = NativePointerTranslator::default();
    let actual = translator.translate(&record);
    let expected = NativePointerInput::new(
        NativePointerId::new(1),
        NativePointerEvent::ButtonReleased(NativePointerButton::Primary),
        NativeDesktopPointerLocation::new(
            NativeDesktopPosition::Exact(
                PhysicalPoint::new(110.0, 220.0).expect("test point validates"),
            ),
            NativePointerHover::Dock(second),
            None,
        ),
        NativePointerOwner::Native(first),
        NativePointerOwner::Native(first),
    );
    assert_eq!(actual, NativePointerTranslation::Input(expected));
}

#[test]
fn pointer_reduction_acknowledges_only_after_core_acceptance() {
    use winit::dpi::PhysicalPosition;
    use winit::event::{
        DeviceId, ElementState, MouseButton, PointerEventFacts, PointerWindowRoute,
    };

    let mut native = coordinator();
    let (first, _) = register_roots(&mut native);
    let window = WindowId::from(11);
    native
        .bind_viewport(ViewportId::ROOT, window, first)
        .expect("root viewport binds");
    let event = WindowEvent::MouseInput {
        device_id: DeviceId::dummy(),
        state: ElementState::Pressed,
        button: MouseButton::Left,
        facts: PointerEventFacts {
            surface_position: Some(PhysicalPosition::new(10.0, 20.0)),
            desktop_position: Some(PhysicalPosition::new(10.0, 20.0)),
            modifiers: None,
            hover: PointerWindowRoute::Window(window),
            capture: PointerWindowRoute::Window(window),
        },
    };
    let record = {
        let viewports = native
            .viewports
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        NativeWindowEventRecord::for_test_snapshot(
            9,
            window,
            Some(ViewportId::ROOT),
            event,
            &viewports,
        )
    };
    native.bridge.push_record(HostRecord::WindowEvent(record));

    assert!(
        native
            .reduce_next_pointer_event()
            .expect("core accepts the current pointer binding")
    );
    assert!(
        native
            .next_window_event()
            .expect("the accepted pointer event is acknowledged")
            .is_none()
    );
}
