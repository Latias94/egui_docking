//! Input translation regressions, including native identity and terminal boundaries.

mod docking_enabled;
mod duplicate_release;

use super::mouse::{MouseTransition, mouse_press_transition};
use super::scroll::NativeScrollState;
use super::*;
use dockspace::runtime::NativeScrollSequenceId;
use winit::event::DeviceId;

#[test]
fn mouse_press_transition_ignores_only_the_same_explicit_press_identity() {
    assert_eq!(
        mouse_press_transition::<u64>(None, None),
        MouseTransition::Forward
    );
    assert_eq!(
        mouse_press_transition(None, Some(&7)),
        MouseTransition::Forward
    );
    assert_eq!(
        mouse_press_transition(Some(Some(&7)), Some(&7)),
        MouseTransition::Ignore,
    );
    for (previous, observed) in [
        (Some(&7), Some(&8)),
        (Some(&7), None),
        (None, Some(&8)),
        (None, None),
    ] {
        assert_eq!(
            mouse_press_transition(Some(previous), observed),
            MouseTransition::CancelThenPress,
        );
    }
}

fn assert_read_only_mouse_device_lookup(
    translator: &NativePointerTranslator,
    pointer: NativePointerId,
    expected: Option<DeviceId>,
) {
    let storage_and_watermarks = || {
        (
            translator.next_device_identity,
            translator.next_scroll_sequence,
            translator.devices.len(),
            translator.owned_data_capacity_bytes(),
            translator.reserved_translation_capacity_bytes(),
        )
    };
    let before = storage_and_watermarks();
    for _ in 0..2 {
        assert_eq!(translator.mouse_device_for_pointer(pointer), expected);
    }
    assert_eq!(storage_and_watermarks(), before);
}

#[test]
fn mouse_device_lookup_is_read_only_and_requires_active_primary() {
    let mut translator = NativePointerTranslator::default();
    assert_read_only_mouse_device_lookup(&translator, NativePointerId::new(1), None);

    let device = DeviceId::dummy();
    let key = NativeInputDeviceKey::Mouse(NativePhysicalDeviceKey::Winit(device));
    let pointer = translator.device_mut(key).expect("mouse enrolls").pointer;
    assert_read_only_mouse_device_lookup(&translator, pointer, None);
    let routes = NativePointerRoutes::from_binding(None);
    assert_eq!(
        translator.device_mut(key).unwrap().mouse.observe(
            NativePointerEvent::ButtonPressed(NativePointerButton::Secondary),
            routes,
        ),
        MouseTransition::Forward
    );
    assert_read_only_mouse_device_lookup(&translator, pointer, None);
    assert_eq!(
        translator.device_mut(key).unwrap().mouse.observe(
            NativePointerEvent::ButtonPressed(NativePointerButton::Primary),
            routes,
        ),
        MouseTransition::Forward
    );
    assert_read_only_mouse_device_lookup(&translator, pointer, Some(device));
    assert_read_only_mouse_device_lookup(&translator, NativePointerId::new(u64::MAX), None);
    assert_eq!(
        translator.device_mut(key).unwrap().mouse.observe(
            NativePointerEvent::ButtonReleased(NativePointerButton::Primary),
            routes,
        ),
        MouseTransition::Forward
    );
    assert!(
        translator
            .device_mut(key)
            .unwrap()
            .mouse
            .is_pressed(NativePointerButton::Secondary)
    );
    assert_read_only_mouse_device_lookup(&translator, pointer, None);
    assert_eq!(
        translator.device_mut(key).unwrap().mouse.observe(
            NativePointerEvent::ButtonReleased(NativePointerButton::Secondary),
            routes,
        ),
        MouseTransition::Forward
    );
    assert_read_only_mouse_device_lookup(&translator, pointer, None);
}

#[test]
fn mouse_device_lookup_does_not_borrow_touch_scroll_or_test_devices() {
    let mut translator = NativePointerTranslator::default();
    let device = DeviceId::dummy();
    let physical = NativePhysicalDeviceKey::Winit(device);
    let mut other_pointers = Vec::new();
    for key in [
        NativeInputDeviceKey::Mouse(NativePhysicalDeviceKey::Test(1)),
        NativeInputDeviceKey::Touch {
            device: physical,
            contact: 7,
        },
        NativeInputDeviceKey::Scroll(physical),
    ] {
        let state = translator.device_mut(key).expect("device enrolls");
        // An active button cannot grant a different device lane mouse access.
        state.mouse.press(
            NativePointerButton::Primary,
            NativePointerRoutes::from_binding(None),
            None,
        );
        other_pointers.push(state.pointer);
    }
    for pointer in &other_pointers {
        assert_read_only_mouse_device_lookup(&translator, *pointer, None);
    }

    let mouse = translator
        .device_mut(NativeInputDeviceKey::Mouse(physical))
        .expect("mouse enrolls separately");
    mouse.mouse.press(
        NativePointerButton::Primary,
        NativePointerRoutes::from_binding(None),
        None,
    );
    let pointer = mouse.pointer;
    assert_read_only_mouse_device_lookup(&translator, pointer, Some(device));
    for pointer in other_pointers {
        assert_read_only_mouse_device_lookup(&translator, pointer, None);
    }
}

#[test]
fn mouse_device_lookup_invalidates_retired_removed_and_overloaded_mouse() {
    use dockspace::model::{
        DockspaceLayout, DockspaceNode, DockspaceRootLayout, DockspaceSurfaceLayout, ItemId,
        RootId, SurfaceId,
    };
    use dockspace::policy::DockPolicy;
    use dockspace::runtime::{
        DockspaceSession, HostInputOutcome, HostWindowToken, NativePointerRoster,
        SurfaceUnavailableReason,
    };

    let surface = SurfaceId::new(1);
    let layout = DockspaceLayout::new([DockspaceSurfaceLayout::new(
        surface,
        DockspaceRootLayout::new(
            RootId::new(1),
            DockspaceNode::central_tabs([ItemId::new(1)]),
        ),
    )])
    .unwrap();
    let mut session = DockspaceSession::from_layout(layout, DockPolicy::default()).unwrap();
    session
        .enable_managed_native_host(NativePointerRoster::Unknown)
        .unwrap();
    session
        .register_native_root(surface, HostWindowToken::new(1))
        .unwrap();
    let mut frame = session.begin_host_frame().unwrap();
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    let report = frame.commit().unwrap();
    let binding = report
        .inputs()
        .iter()
        .find_map(|outcome| match outcome {
            HostInputOutcome::NativeSurfaceRegistered { binding } => Some(*binding),
            _ => None,
        })
        .expect("registration emits a binding");

    let mut translator = NativePointerTranslator::default();
    let device = DeviceId::dummy();
    let key = NativeInputDeviceKey::Mouse(NativePhysicalDeviceKey::Winit(device));
    let mouse = translator.device_mut(key).expect("mouse enrolls");
    mouse.mouse.press(
        NativePointerButton::Primary,
        NativePointerRoutes::from_binding(Some(binding.window())),
        None,
    );
    let retired_pointer = mouse.pointer;
    assert_read_only_mouse_device_lookup(&translator, retired_pointer, Some(device));

    translator.retire_binding(binding.window());
    assert!(matches!(
        translator.devices.get(&key),
        Some(NativePointerDeviceEntry::Live(state))
            if !state.mouse.is_pressed(NativePointerButton::Primary)
    ));
    assert_read_only_mouse_device_lookup(&translator, retired_pointer, None);

    translator.forget_removed_devices();
    assert!(matches!(
        translator.devices.get(&key),
        Some(NativePointerDeviceEntry::Removed)
    ));
    assert_read_only_mouse_device_lookup(&translator, retired_pointer, None);

    let mouse = translator.device_mut(key).expect("fresh mouse enrolls");
    mouse.mouse.press(
        NativePointerButton::Primary,
        NativePointerRoutes::from_binding(None),
        None,
    );
    let overloaded_pointer = mouse.pointer;
    assert_ne!(overloaded_pointer, retired_pointer);
    assert_read_only_mouse_device_lookup(&translator, retired_pointer, None);
    assert_read_only_mouse_device_lookup(&translator, overloaded_pointer, Some(device));

    translator.finish_overload_cancellation();
    assert!(!translator.devices.contains_key(&key));
    assert_read_only_mouse_device_lookup(&translator, overloaded_pointer, None);
    let mouse = translator
        .device_mut(key)
        .expect("post-overload mouse enrolls");
    mouse.mouse.press(
        NativePointerButton::Primary,
        NativePointerRoutes::from_binding(None),
        None,
    );
    let fresh_pointer = mouse.pointer;
    assert_ne!(fresh_pointer, overloaded_pointer);
    assert_read_only_mouse_device_lookup(&translator, overloaded_pointer, None);
    assert_read_only_mouse_device_lookup(&translator, fresh_pointer, Some(device));

    *translator.devices.get_mut(&key).unwrap() = NativePointerDeviceEntry::ScrollTail;
    assert_read_only_mouse_device_lookup(&translator, fresh_pointer, None);
}

#[test]
fn desktop_location_uses_only_same_record_desktop_facts_and_frozen_hover() {
    use crate::viewport_map::NativeViewportMap;
    use winit::dpi::PhysicalPosition;
    use winit::event::{PointerEventFacts, PointerWindowRoute};

    let point = PhysicalPoint::new(110.0, -220.0).unwrap();
    let raw_point = Some(PhysicalPosition::new(point.x(), point.y()));
    for (desktop_position, hover, expected_position, expected_hover, expected_calls) in [
        (
            raw_point,
            PointerWindowRoute::Foreign,
            NativeDesktopPosition::Exact(point),
            NativePointerHover::Foreign,
            0,
        ),
        (
            raw_point,
            PointerWindowRoute::None,
            NativeDesktopPosition::Exact(point),
            NativePointerHover::OutsideAll,
            1,
        ),
        (
            None,
            PointerWindowRoute::None,
            NativeDesktopPosition::Unknown,
            NativePointerHover::OutsideAll,
            0,
        ),
        (
            Some(PhysicalPosition::new(f64::NAN, 0.0)),
            PointerWindowRoute::None,
            NativeDesktopPosition::Unknown,
            NativePointerHover::OutsideAll,
            0,
        ),
        (
            Some(PhysicalPosition::new(0.0, f64::INFINITY)),
            PointerWindowRoute::None,
            NativeDesktopPosition::Unknown,
            NativePointerHover::OutsideAll,
            0,
        ),
    ] {
        let record = NativeWindowEventRecord::for_test_snapshot(
            1,
            winit::window::WindowId::from(1),
            None,
            WindowEvent::CursorMoved {
                device_id: DeviceId::dummy(),
                position: PhysicalPosition::new(900.0, 800.0),
                facts: PointerEventFacts {
                    desktop_position,
                    hover,
                    ..PointerEventFacts::default()
                },
            },
            &NativeViewportMap::default(),
        );
        let mut calls = 0;
        let location =
            native_desktop_location(&record, record.pointer_routes().unwrap(), &mut |observed| {
                calls += 1;
                assert_eq!(observed, point);
                None
            });
        assert_eq!(
            location,
            NativeDesktopPointerLocation::new(expected_position, expected_hover, None)
        );
        assert_eq!(calls, expected_calls);
    }
}

#[test]
fn owned_capacity_keeps_device_slots_charged_when_they_become_tombstones() {
    let mut translator = NativePointerTranslator::default();
    let empty = translator.owned_data_capacity_bytes();
    let mouse = NativeInputDeviceKey::Mouse(NativePhysicalDeviceKey::Test(1));
    translator.device_mut(mouse).expect("mouse enrolls");
    let idle = translator.owned_data_capacity_bytes();
    let routes = NativePointerRoutes::from_binding(None);
    assert_eq!(
        translator
            .device_mut(mouse)
            .expect("mouse remains enrolled")
            .mouse
            .observe(
                NativePointerEvent::ButtonPressed(NativePointerButton::Primary),
                routes,
            ),
        MouseTransition::Forward
    );
    let pressed = translator.owned_data_capacity_bytes();
    assert!(pressed > idle);
    assert_eq!(
        translator
            .device_mut(mouse)
            .expect("mouse remains enrolled")
            .mouse
            .observe(
                NativePointerEvent::ButtonReleased(NativePointerButton::Primary),
                routes,
            ),
        MouseTransition::Forward
    );
    // Released buttons retain a charged tail gate until device retirement.
    assert_eq!(translator.owned_data_capacity_bytes(), pressed);

    translator.forget_removed_devices();
    assert_eq!(translator.devices.len(), 1);
    assert!(matches!(
        translator.devices.get(&mouse),
        Some(NativePointerDeviceEntry::Removed)
    ));
    let removed = translator.owned_data_capacity_bytes();
    assert!(removed > empty);
    assert_eq!(removed, idle);

    let scroll = NativeInputDeviceKey::Scroll(NativePhysicalDeviceKey::Test(2));
    translator
        .device_mut(scroll)
        .expect("scroll device enrolls")
        .scroll = NativeScrollState::Live {
        sequence: NativeScrollSequenceId::new(1),
        delivery: None,
    };
    translator.finish_overload_cancellation();
    assert_eq!(translator.devices.len(), 2);
    assert!(matches!(
        translator.devices.get(&mouse),
        Some(NativePointerDeviceEntry::Removed)
    ));
    assert!(matches!(
        translator.devices.get(&scroll),
        Some(NativePointerDeviceEntry::ScrollTail)
    ));
    let both_tombstones = translator.owned_data_capacity_bytes();
    assert!(both_tombstones > removed);
    assert_eq!(both_tombstones - removed, removed - empty);

    translator.forget_removed_devices();
    translator.finish_overload_cancellation();
    assert_eq!(translator.owned_data_capacity_bytes(), both_tombstones);
}

#[test]
fn idle_cursor_coalescing_stops_for_buttons_and_scroll() {
    let routes = NativePointerRoutes::from_binding(None);
    let mut translator = NativePointerTranslator::default();
    assert!(translator.can_coalesce_idle_cursor_moves());

    let device = NativeInputDeviceKey::Mouse(NativePhysicalDeviceKey::Test(1));
    assert_eq!(
        translator
            .device_mut(device)
            .expect("test device enrolls")
            .mouse
            .observe(
                NativePointerEvent::ButtonPressed(NativePointerButton::Primary),
                routes,
            ),
        MouseTransition::Forward
    );
    assert!(!translator.can_coalesce_idle_cursor_moves());

    assert_eq!(
        translator
            .device_mut(device)
            .expect("test device remains enrolled")
            .mouse
            .observe(
                NativePointerEvent::ButtonReleased(NativePointerButton::Primary),
                routes,
            ),
        MouseTransition::Forward
    );
    assert!(translator.can_coalesce_idle_cursor_moves());

    translator
        .device_mut(device)
        .expect("test device remains enrolled")
        .scroll = NativeScrollState::Live {
        sequence: NativeScrollSequenceId::new(1),
        delivery: None,
    };
    assert!(!translator.can_coalesce_idle_cursor_moves());
}
