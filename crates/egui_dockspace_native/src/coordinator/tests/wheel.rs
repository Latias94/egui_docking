use dockspace::geometry::PhysicalPoint;
use dockspace::runtime::{
    NativeDesktopPointerLocation, NativeDesktopPosition, NativePointerEvent, NativePointerHover,
    NativePointerId, NativePointerInput, NativePointerOwner, NativeScrollDelta,
    NativeScrollDeviceId, NativeScrollEvent, NativeScrollModifiers, NativeScrollMomentum,
    NativeScrollPhase,
};
use eframe::egui::ViewportId;
use winit::dpi::PhysicalPosition;
use winit::event::{
    DeviceId, MouseScrollDelta, PointerEventFacts, PointerWindowRoute, TouchPhase, WindowEvent,
};
use winit::keyboard::ModifiersState;
use winit::window::WindowId;

use super::{coordinator, register_roots};
use crate::event::NativeWindowEventRecord;
use crate::pointer_event::{NativePointerTranslation, NativePointerTranslator};
use crate::viewport_map::NativeViewportMap;

#[test]
fn wheel_translation_preserves_event_time_routes_position_and_modifiers() {
    let mut native = coordinator();
    let (delivery, hover) = register_roots(&mut native);
    let delivery_window = WindowId::from(11);
    let hover_window = WindowId::from(22);
    let hover_viewport = ViewportId::from_hash_of("wheel-hover-surface");
    native
        .bind_viewport(ViewportId::ROOT, delivery_window, delivery)
        .expect("delivery viewport binds");
    native
        .bind_viewport(hover_viewport, hover_window, hover)
        .expect("hover viewport binds");

    let event = WindowEvent::MouseWheel {
        device_id: DeviceId::dummy(),
        delta: MouseScrollDelta::LineDelta(1.25, -3.5),
        phase: TouchPhase::Moved,
        facts: PointerEventFacts {
            surface_position: Some(PhysicalPosition::new(10.0, 20.0)),
            desktop_position: Some(PhysicalPosition::new(110.0, 220.0)),
            modifiers: Some(ModifiersState::SHIFT | ModifiersState::ALT),
            hover: PointerWindowRoute::Window(hover_window),
            capture: PointerWindowRoute::Window(delivery_window),
        },
    };
    let record = {
        let viewports = native
            .viewports
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        NativeWindowEventRecord::for_test_snapshot(
            1,
            delivery_window,
            Some(ViewportId::ROOT),
            event,
            &viewports,
        )
    };

    let actual = NativePointerTranslator::default().translate(&record, |_| None);
    let expected = NativePointerInput::new(
        NativePointerId::new(1),
        NativePointerEvent::Scrolled(NativeScrollEvent::new(
            NativeScrollDeviceId::new(1),
            NativeScrollPhase::Discrete {
                delta: NativeScrollDelta::Lines { x: 1.25, y: -3.5 },
            },
            NativeScrollMomentum::Unknown,
            NativeScrollModifiers::Exact {
                shift: true,
                control: false,
                alt: true,
                command: false,
            },
        )),
        NativeDesktopPointerLocation::new(
            NativeDesktopPosition::Exact(
                PhysicalPoint::new(110.0, 220.0).expect("test point validates"),
            ),
            NativePointerHover::Dock(hover),
            None,
        ),
        NativePointerOwner::Native(delivery),
        NativePointerOwner::Native(delivery),
    );

    assert_eq!(actual, NativePointerTranslation::Input(expected));
}

#[test]
fn wheel_translation_never_upgrades_missing_event_time_facts() {
    let window = WindowId::from(41);
    let event = WindowEvent::MouseWheel {
        device_id: DeviceId::dummy(),
        delta: MouseScrollDelta::PixelDelta(PhysicalPosition::new(4.0, -8.0)),
        phase: TouchPhase::Moved,
        facts: PointerEventFacts::default(),
    };
    let record = NativeWindowEventRecord::for_test_snapshot(
        2,
        window,
        None,
        event,
        &NativeViewportMap::default(),
    );

    let actual = NativePointerTranslator::default().translate(&record, |_| {
        panic!("unknown hover must not request a work-area binding")
    });
    let expected = NativePointerInput::new(
        NativePointerId::new(1),
        NativePointerEvent::Scrolled(NativeScrollEvent::new(
            NativeScrollDeviceId::new(1),
            NativeScrollPhase::Discrete {
                delta: NativeScrollDelta::PhysicalPixels { x: 4.0, y: -8.0 },
            },
            NativeScrollMomentum::Unknown,
            NativeScrollModifiers::Unknown,
        )),
        NativeDesktopPointerLocation::new(
            NativeDesktopPosition::Unknown,
            NativePointerHover::Unknown,
            None,
        ),
        NativePointerOwner::Unknown,
        NativePointerOwner::Unknown,
    );

    assert_eq!(actual, NativePointerTranslation::Input(expected));
}
