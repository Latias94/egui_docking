use dockspace::geometry::PhysicalPoint;
use dockspace::runtime::{
    NativeDesktopPointerLocation, NativeDesktopPosition, NativeLocalPointerFact,
    NativeLocalPointerLocation, NativePointerButton, NativePointerEvent, NativePointerHover,
    NativePointerId, NativePointerInput, NativePointerOwner, NativePointerState,
};
use winit::dpi::PhysicalPosition;
use winit::event::{
    DeviceId, ElementState, MouseButton, MouseScrollDelta, PointerEventFacts, PointerWindowRoute,
    TouchPhase, WindowEvent,
};

use super::*;
use crate::event::NativePointerRouteSnapshot;

#[test]
fn captured_drag_ignores_target_cursor_notification_until_owner_callback() {
    let CrossWindowCaptureFixture {
        source,
        target,
        press,
        target_notification,
        source_callback,
        ..
    } = cross_window_capture_fixture();
    let mut translator = NativePointerTranslator::default();

    assert!(matches!(
        translator.translate_for_test_device(&press, 41, |_| None),
        NativePointerTranslation::Input(_)
    ));
    assert_eq!(
        translator.translate_for_test_device(&target_notification, 41, |_| None),
        NativePointerTranslation::Ignored,
        "capture-owner delivery must win over a target-window cursor notification"
    );
    assert_eq!(
        translator.translate_for_test_device(&source_callback, 41, |_| None),
        NativePointerTranslation::Input(NativePointerInput::new(
            NativePointerId::new(1),
            NativePointerEvent::Moved,
            NativeDesktopPointerLocation::new(
                NativeDesktopPosition::Exact(
                    PhysicalPoint::new(330.0, 440.0).expect("test point validates"),
                ),
                NativePointerHover::Dock(target.window()),
                None,
            ),
            NativePointerOwner::Native(source.window()),
            NativePointerOwner::Native(source.window()),
        )),
        "the capture-owner callback keeps source delivery with target hover"
    );
}

#[test]
fn ingress_ignores_target_cursor_notification_before_captured_source_delivery() {
    let CrossWindowCaptureFixture {
        mut native,
        press,
        target_notification,
        source_callback,
        ..
    } = cross_window_capture_fixture();

    for (record, expectation) in [
        (press, "the source press enters the core journal"),
        (
            target_notification,
            "the target notification is acknowledged without entering the journal",
        ),
        (
            source_callback,
            "the capture-owner callback enters the core journal",
        ),
    ] {
        native.bridge.push_record(HostRecord::WindowEvent(record));
        assert!(
            native
                .reduce_callback_prefix(&egui::Context::default(), &mut test_budget())
                .expect(expectation)
                .is_some()
        );
        commit_pending_pointer_frame(&mut native);
    }
}

struct CrossWindowCaptureFixture {
    native: NativeCoordinator,
    source: NativeSurfaceBinding,
    target: NativeSurfaceBinding,
    press: NativeWindowEventRecord,
    target_notification: NativeWindowEventRecord,
    source_callback: NativeWindowEventRecord,
}

fn cross_window_capture_fixture() -> CrossWindowCaptureFixture {
    let mut native = coordinator();
    let (source, target) = register_roots(&mut native);
    let source_window = WindowId::from(11);
    let target_window = WindowId::from(22);
    let target_viewport = ViewportId::from_hash_of("capture-target-window");
    native
        .bind_viewport(ViewportId::ROOT, source_window, source)
        .expect("source viewport binds");
    native
        .bind_viewport(target_viewport, target_window, target)
        .expect("target viewport binds");

    let device_id = DeviceId::dummy();
    let cursor_event = || WindowEvent::CursorMoved {
        device_id,
        position: PhysicalPosition::new(30.0, 40.0),
        facts: PointerEventFacts {
            surface_position: Some(PhysicalPosition::new(30.0, 40.0)),
            desktop_position: Some(PhysicalPosition::new(330.0, 440.0)),
            hover: PointerWindowRoute::Window(target_window),
            capture: PointerWindowRoute::Window(source_window),
            ..PointerEventFacts::default()
        },
    };
    let (press, target_notification, source_callback) = native.bridge.with_viewports(|routes| {
        (
            NativeWindowEventRecord::for_test_snapshot(
                1,
                source_window,
                Some(ViewportId::ROOT),
                WindowEvent::MouseInput {
                    primary_press: None,
                    device_id,
                    state: ElementState::Pressed,
                    button: MouseButton::Left,
                    facts: PointerEventFacts {
                        surface_position: Some(PhysicalPosition::new(10.0, 20.0)),
                        desktop_position: Some(PhysicalPosition::new(110.0, 220.0)),
                        hover: PointerWindowRoute::Window(source_window),
                        capture: PointerWindowRoute::Window(source_window),
                        ..PointerEventFacts::default()
                    },
                },
                routes,
            ),
            NativeWindowEventRecord::for_test_snapshot(
                2,
                target_window,
                Some(target_viewport),
                cursor_event(),
                routes,
            ),
            NativeWindowEventRecord::for_test_snapshot(
                3,
                source_window,
                Some(ViewportId::ROOT),
                cursor_event(),
                routes,
            ),
        )
    });

    CrossWindowCaptureFixture {
        native,
        source,
        target,
        press,
        target_notification,
        source_callback,
    }
}

#[test]
fn destroyed_binding_retires_pressed_pointer_before_successor_press() {
    let mut native = coordinator();
    let (first, second) = register_roots(&mut native);
    let first_window = WindowId::from(11);
    let second_window = WindowId::from(22);
    let second_viewport = ViewportId::from_hash_of("successor-pointer-window");
    native
        .bind_viewport(ViewportId::ROOT, first_window, first)
        .expect("first viewport binds");
    native
        .bind_viewport(second_viewport, second_window, second)
        .expect("successor viewport binds");

    let first_press = NativeWindowEventRecord::for_test(
        1,
        first_window,
        Some(ViewportId::ROOT),
        Some(first),
        WindowEvent::MouseInput {
            primary_press: None,
            device_id: DeviceId::dummy(),
            state: ElementState::Pressed,
            button: MouseButton::Left,
            facts: PointerEventFacts::default(),
        },
    );
    native
        .bridge
        .push_record(HostRecord::WindowEvent(first_press));
    assert!(
        native
            .reduce_callback_prefix(&egui::Context::default(), &mut test_budget())
            .expect("the first press enters the native pointer journal")
            .is_some()
    );
    commit_pending_pointer_frame(&mut native);

    let destroyed = native.bridge.with_viewports_mut(|routes| {
        NativeWindowEventRecord::for_test_ingress(
            2,
            first_window,
            Some(ViewportId::ROOT),
            WindowEvent::Destroyed,
            routes,
        )
    });
    native
        .bridge
        .push_record(HostRecord::WindowEvent(destroyed));
    assert!(
        native
            .reduce_callback_prefix(&egui::Context::default(), &mut test_budget())
            .expect("one binding-retirement fact records with the destroyed callback")
            .is_some()
    );
    assert!(!native.pointer_translator.references_binding(first.window()));
    assert!(native.next_window_event().unwrap().is_none());
    commit_pending_pointer_frame(&mut native);

    let late_release = NativeWindowEventRecord::for_test(
        3,
        second_window,
        Some(second_viewport),
        Some(second),
        WindowEvent::MouseInput {
            primary_press: None,
            device_id: DeviceId::dummy(),
            state: ElementState::Released,
            button: MouseButton::Left,
            facts: PointerEventFacts::default(),
        },
    );
    native
        .bridge
        .push_record(HostRecord::WindowEvent(late_release));
    assert!(
        native
            .reduce_callback_prefix(&egui::Context::default(), &mut test_budget())
            .expect("the late physical release consumes the cancelled-button tombstone")
            .is_some()
    );
    commit_pending_pointer_frame(&mut native);

    let successor_press = NativeWindowEventRecord::for_test(
        4,
        second_window,
        Some(second_viewport),
        Some(second),
        WindowEvent::MouseInput {
            primary_press: None,
            device_id: DeviceId::dummy(),
            state: ElementState::Pressed,
            button: MouseButton::Left,
            facts: PointerEventFacts::default(),
        },
    );
    native
        .bridge
        .push_record(HostRecord::WindowEvent(successor_press));
    assert!(
        native
            .reduce_callback_prefix(&egui::Context::default(), &mut test_budget())
            .expect("the successor press opens a fresh pointer incarnation")
            .is_some()
    );
    commit_pending_pointer_frame(&mut native);

    assert!(!native.pointer_translator.references_binding(first.window()));
    assert!(
        native
            .pointer_translator
            .references_binding(second.window())
    );
}

#[test]
fn idle_delivery_does_not_become_a_persistent_pointer_owner() {
    let mut native = coordinator();
    let (binding, _) = register_roots(&mut native);
    let moved = NativeWindowEventRecord::for_test(
        1,
        WindowId::from(11),
        Some(ViewportId::ROOT),
        Some(binding),
        WindowEvent::CursorMoved {
            device_id: DeviceId::dummy(),
            position: PhysicalPosition::new(10.0, 20.0),
            facts: PointerEventFacts::default(),
        },
    );

    assert!(matches!(
        native.pointer_translator.translate(&moved, |_| None),
        NativePointerTranslation::Input(_)
    ));
    assert!(
        !native
            .pointer_translator
            .references_binding(binding.window())
    );
    let before = format!("{:?}", native.pointer_translator);
    native.pointer_translator.retire_binding(binding.window());
    assert_eq!(format!("{:?}", native.pointer_translator), before);
}

#[test]
fn released_button_no_longer_keeps_its_press_binding_live() {
    let mut native = coordinator();
    let (binding, _) = register_roots(&mut native);
    let window = WindowId::from(11);

    for (ordinal, state) in [(1, ElementState::Pressed), (2, ElementState::Released)] {
        let record = NativeWindowEventRecord::for_test(
            ordinal,
            window,
            Some(ViewportId::ROOT),
            Some(binding),
            WindowEvent::MouseInput {
                primary_press: None,
                device_id: DeviceId::dummy(),
                state,
                button: MouseButton::Left,
                facts: PointerEventFacts::default(),
            },
        );
        assert!(matches!(
            native.pointer_translator.translate(&record, |_| None),
            NativePointerTranslation::Input(_)
        ));
    }

    assert!(
        !native
            .pointer_translator
            .references_binding(binding.window())
    );
    let before = format!("{:?}", native.pointer_translator);
    native.pointer_translator.retire_binding(binding.window());
    assert_eq!(format!("{:?}", native.pointer_translator), before);
}

#[test]
fn pressed_delivery_and_capture_bindings_remain_independent() {
    let mut native = coordinator();
    let (delivery, captured) = register_roots(&mut native);
    let delivery_window = WindowId::from(11);
    let captured_window = WindowId::from(22);
    let captured_viewport = ViewportId::from_hash_of("captured-press-owner");
    native
        .bind_viewport(ViewportId::ROOT, delivery_window, delivery)
        .expect("delivery viewport binds");
    native
        .bind_viewport(captured_viewport, captured_window, captured)
        .expect("captured viewport binds");

    let press = native.bridge.with_viewports(|routes| {
        NativeWindowEventRecord::for_test_snapshot(
            1,
            delivery_window,
            Some(ViewportId::ROOT),
            WindowEvent::MouseInput {
                primary_press: None,
                device_id: DeviceId::dummy(),
                state: ElementState::Pressed,
                button: MouseButton::Left,
                facts: PointerEventFacts {
                    capture: PointerWindowRoute::Window(captured_window),
                    ..PointerEventFacts::default()
                },
            },
            routes,
        )
    });
    assert!(matches!(
        native.pointer_translator.translate(&press, |_| None),
        NativePointerTranslation::Input(_)
    ));
    assert!(
        native
            .pointer_translator
            .references_binding(delivery.window())
    );
    assert!(
        native
            .pointer_translator
            .references_binding(captured.window())
    );

    let capture_released = native.bridge.with_viewports(|routes| {
        NativeWindowEventRecord::for_test_snapshot(
            2,
            delivery_window,
            Some(ViewportId::ROOT),
            WindowEvent::PointerCaptureChanged {
                device_id: DeviceId::dummy(),
                capture: PointerWindowRoute::None,
            },
            routes,
        )
    });
    assert!(matches!(
        native
            .pointer_translator
            .translate(&capture_released, |_| None),
        NativePointerTranslation::Input(_)
    ));
    assert!(
        native
            .pointer_translator
            .references_binding(delivery.window())
    );
    assert!(
        !native
            .pointer_translator
            .references_binding(captured.window())
    );
    native.pointer_translator.retire_binding(delivery.window());
    assert!(
        !native
            .pointer_translator
            .references_binding(delivery.window())
    );
}

#[test]
fn capture_retirement_swallows_release_from_unknown_delivery() {
    let mut native = coordinator();
    let (captured, _) = register_roots(&mut native);
    let captured_window = WindowId::from(22);
    let captured_viewport = ViewportId::from_hash_of("captured-unknown-delivery");
    native
        .bind_viewport(captured_viewport, captured_window, captured)
        .expect("captured viewport binds");

    let unknown_window = WindowId::from(33);
    let press = native.bridge.with_viewports(|routes| {
        NativeWindowEventRecord::for_test_snapshot(
            1,
            unknown_window,
            None,
            WindowEvent::MouseInput {
                primary_press: None,
                device_id: DeviceId::dummy(),
                state: ElementState::Pressed,
                button: MouseButton::Left,
                facts: PointerEventFacts {
                    capture: PointerWindowRoute::Window(captured_window),
                    ..PointerEventFacts::default()
                },
            },
            routes,
        )
    });
    assert!(matches!(
        native.pointer_translator.translate(&press, |_| None),
        NativePointerTranslation::Input(_)
    ));
    assert!(
        native
            .pointer_translator
            .references_binding(captured.window())
    );
    let unknown_capture = native.bridge.with_viewports(|routes| {
        NativeWindowEventRecord::for_test_snapshot(
            2,
            unknown_window,
            None,
            WindowEvent::PointerCaptureChanged {
                device_id: DeviceId::dummy(),
                capture: PointerWindowRoute::Unknown,
            },
            routes,
        )
    });
    assert!(matches!(
        native
            .pointer_translator
            .translate(&unknown_capture, |_| None),
        NativePointerTranslation::Input(_)
    ));
    assert!(
        native
            .pointer_translator
            .references_binding(captured.window())
    );
    native.pointer_translator.retire_binding(captured.window());
    assert!(
        !native
            .pointer_translator
            .references_binding(captured.window())
    );

    let late_release = NativeWindowEventRecord::for_test(
        3,
        unknown_window,
        None,
        None,
        WindowEvent::MouseInput {
            primary_press: None,
            device_id: DeviceId::dummy(),
            state: ElementState::Released,
            button: MouseButton::Left,
            facts: PointerEventFacts::default(),
        },
    );
    assert_eq!(
        native.pointer_translator.translate(&late_release, |_| None),
        NativePointerTranslation::Ignored
    );
}

#[test]
fn binding_retirement_keeps_button_tombstones_isolated_by_device() {
    let mut native = coordinator();
    let (first, second) = register_roots(&mut native);
    let first_window = WindowId::from(51);
    let second_window = WindowId::from(52);
    let first_press = pointer_button_record(1, first_window, first, ElementState::Pressed);
    let second_press = pointer_button_record(2, second_window, second, ElementState::Pressed);
    let mut translator = NativePointerTranslator::default();

    assert!(matches!(
        translator.translate_for_test_device(&first_press, 101, |_| None),
        NativePointerTranslation::Input(_)
    ));
    assert!(matches!(
        translator.translate_for_test_device(&second_press, 202, |_| None),
        NativePointerTranslation::Input(_)
    ));

    translator.retire_binding(first.window());
    assert!(!translator.references_binding(first.window()));
    assert!(translator.references_binding(second.window()));

    let second_release = pointer_button_record(3, second_window, second, ElementState::Released);
    assert!(matches!(
        translator.translate_for_test_device(&second_release, 202, |_| None),
        NativePointerTranslation::Input(_)
    ));

    let first_late_release = pointer_button_record(4, first_window, first, ElementState::Released);
    assert_eq!(
        translator.translate_for_test_device(&first_late_release, 101, |_| None),
        NativePointerTranslation::Ignored,
        "only the retired device consumes its cancelled-button tombstone"
    );
}

#[test]
fn binding_retirement_preserves_device_identities_in_any_device_key_order() {
    let mut native = coordinator();
    let (binding, _) = register_roots(&mut native);
    let window = WindowId::from(61);
    let first_press = pointer_button_record(1, window, binding, ElementState::Pressed);
    let second_press = pointer_button_record(2, window, binding, ElementState::Pressed);
    let mut translator = NativePointerTranslator::default();

    assert!(matches!(
        translator.translate_for_test_device(&first_press, 901, |_| None),
        NativePointerTranslation::Input(_)
    ));
    assert!(matches!(
        translator.translate_for_test_device(&second_press, 303, |_| None),
        NativePointerTranslation::Input(_)
    ));

    translator.retire_binding(binding.window());
    assert!(!translator.references_binding(binding.window()));
    for (record, device, pointer) in [
        (&first_press, 901, NativePointerId::new(1)),
        (&second_press, 303, NativePointerId::new(2)),
    ] {
        assert_eq!(
            translator.translate_for_test_device(record, device, |_| None),
            NativePointerTranslation::Input(NativePointerInput::new(
                pointer,
                NativePointerEvent::ButtonPressed(NativePointerButton::Primary),
                NativeDesktopPointerLocation::new(
                    NativeDesktopPosition::Unknown,
                    NativePointerHover::Unknown,
                    None,
                ),
                NativePointerOwner::Native(binding.window()),
                NativePointerOwner::Unknown,
            )),
        );
    }
}

#[test]
fn destroyed_callback_records_one_binding_fact_for_all_devices() {
    let mut native = coordinator();
    let (binding, _) = register_roots(&mut native);
    let window = WindowId::from(71);
    native
        .bind_viewport(ViewportId::ROOT, window, binding)
        .expect("retiring viewport binds");

    for (ordinal, device) in [(1, 101), (2, 202)] {
        let press = pointer_button_record(ordinal, window, binding, ElementState::Pressed);
        let input =
            match native
                .pointer_translator
                .translate_for_test_device(&press, device, |_| None)
            {
                NativePointerTranslation::Input(input) => input,
                translation => panic!("press must translate, got {translation:?}"),
            };
        native
            .record_pointer(input)
            .expect("test press enters the core journal");
    }
    commit_pending_pointer_frame(&mut native);

    let recorded = native.session.native_input_recorded_count();
    native
        .bridge
        .push_record(HostRecord::WindowEvent(NativeWindowEventRecord::for_test(
            3,
            window,
            Some(ViewportId::ROOT),
            Some(binding),
            WindowEvent::Destroyed,
        )));
    assert!(
        native
            .reduce_callback_prefix(&egui::Context::default(), &mut test_budget())
            .expect("one binding-retirement fact records")
            .is_some()
    );
    assert_eq!(native.session.native_input_recorded_count(), recorded + 1);
    assert!(
        !native
            .pointer_translator
            .references_binding(binding.window())
    );
    assert!(native.next_window_event().unwrap().is_none());
    commit_pending_pointer_frame(&mut native);
}

fn pointer_button_record(
    ordinal: u64,
    window: WindowId,
    binding: dockspace::runtime::NativeSurfaceBinding,
    state: ElementState,
) -> NativeWindowEventRecord {
    NativeWindowEventRecord::for_test(
        ordinal,
        window,
        None,
        Some(binding),
        WindowEvent::MouseInput {
            primary_press: None,
            device_id: DeviceId::dummy(),
            state,
            button: MouseButton::Left,
            facts: PointerEventFacts::default(),
        },
    )
}

fn commit_pending_pointer_frame(native: &mut NativeCoordinator) {
    let mut frame = native
        .begin_host_frame(|_| NativeReceiverAnswer::Unknown)
        .expect("the pending pointer frame begins");
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .expect("the pending pointer frame settles every surface");
    frame.commit().expect("the pending pointer frame commits");
}

#[test]
fn release_child_preserves_scroll_correlation_for_the_provider_terminal() {
    let mut native = coordinator();
    let (root, child_binding) = register_root_and_child(&mut native);
    let root_window = WindowId::from(11);
    let child_window = WindowId::from(22);
    let child = ViewportId::from_hash_of("scrolling-release-child");
    native
        .bind_viewport(ViewportId::ROOT, root_window, root)
        .expect("root viewport binds");
    native
        .bind_viewport(child, child_window, child_binding)
        .expect("child viewport binds");
    let mut initially_presented = paint_and_present_all_surfaces(&mut native);
    assert!(initially_presented.take_native_effects().is_empty());
    native
        .session
        .report_native_global_focus(NativeGlobalFocus::Dock(root.window()))
        .expect("the redock destination is already the active root");
    super::callback_prefix::commit_boundary(&mut native);

    let started = NativeWindowEventRecord::for_test(
        9,
        child_window,
        Some(child),
        Some(child_binding),
        WindowEvent::MouseWheel {
            device_id: DeviceId::dummy(),
            delta: MouseScrollDelta::LineDelta(0.0, 1.0),
            phase: TouchPhase::Started,
            facts: PointerEventFacts::default(),
        },
    );
    assert!(matches!(
        native.pointer_translator.translate(&started, |_| None),
        NativePointerTranslation::Input(_)
    ));

    let mut redock = native
        .begin_host_frame(|_| NativeReceiverAnswer::Unknown)
        .expect("the child redock frame begins");
    redock
        .frame
        .dock_root_current(
            RootId::new(2),
            DockPlacement::Center(DockAnchor::Item(ItemId::new(1))),
        )
        .expect("the complete child root redocks");
    redock
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .expect("the child redock settles every surface");
    let mut requested = redock.commit().expect("the child redock request commits");
    assert!(
        requested.take_native_effects().is_empty(),
        "ReleaseChild waits for the target presentation"
    );
    let mut report = paint_and_present_all_surfaces(&mut native);
    let effects = report.take_native_effects();
    assert!(matches!(
        effects.as_slice(),
        [request]
            if matches!(
                request.operation(),
                NativeEffectOperation::ReleaseChild { binding } if *binding == child_binding
            )
    ));
    native
        .accept_native_effects(&egui::Context::default(), effects)
        .expect("the child release is accepted");

    assert!(
        native
            .pointer_translator
            .references_binding(child_binding.window()),
        "ReleaseChild cannot replace the provider's terminal scroll edge"
    );
    native
        .pointer_translator
        .retire_binding(child_binding.window());
    assert!(
        !native
            .pointer_translator
            .references_binding(child_binding.window())
    );
    let late_terminal = NativeWindowEventRecord::for_test(
        10,
        child_window,
        Some(child),
        Some(child_binding),
        WindowEvent::MouseWheel {
            device_id: DeviceId::dummy(),
            delta: MouseScrollDelta::LineDelta(0.0, 1.0),
            phase: TouchPhase::Ended,
            facts: PointerEventFacts::default(),
        },
    );
    assert_eq!(
        native
            .pointer_translator
            .translate(&late_terminal, |_| None),
        NativePointerTranslation::Ignored,
    );
}

#[test]
fn destroyed_child_route_retires_only_after_tombstone_commit_and_quiescence() {
    let mut native = coordinator();
    assert_eq!(
        native.lifecycle_progress(),
        NativeLifecycleProgress::default()
    );
    let (first, second) = register_root_and_child(&mut native);
    let first_window = WindowId::from(11);
    let second_window = WindowId::from(22);
    let child = ViewportId::from_hash_of("retired-native-child");
    native
        .bind_viewport(ViewportId::ROOT, first_window, first)
        .expect("root viewport binds");
    native
        .bind_viewport(child, second_window, second)
        .expect("child viewport binds");
    let mut initially_presented = paint_and_present_all_surfaces(&mut native);
    assert!(initially_presented.take_native_effects().is_empty());
    native
        .session
        .report_native_global_focus(NativeGlobalFocus::Dock(first.window()))
        .expect("the redock destination is already the active root");
    super::callback_prefix::commit_boundary(&mut native);
    let point = PhysicalPoint::new(10.0, 20.0).unwrap();
    let coordinates = native.session.native_local_coordinates(second).unwrap();
    let geometry = coordinates.geometry();
    let retained_local = coordinates
        .capture(
            second.window(),
            NativeLocalPointerFact::new(
                point,
                geometry.content_size(),
                geometry.native_scale_factor(),
            ),
            geometry,
        )
        .unwrap();

    let mut redock = native
        .begin_host_frame(|_| NativeReceiverAnswer::Unknown)
        .expect("the child redock frame begins");
    redock
        .frame
        .dock_root_current(
            RootId::new(2),
            DockPlacement::Center(DockAnchor::Item(ItemId::new(1))),
        )
        .expect("the complete child root redocks through the product action");
    redock
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .expect("the child redock settles every surface");
    let mut requested = redock.commit().expect("the child redock request commits");
    assert!(
        requested.take_native_effects().is_empty(),
        "ReleaseChild waits for the target presentation"
    );
    let mut redocked = paint_and_present_all_surfaces(&mut native);
    let effects = redocked.take_native_effects();
    assert_eq!(
        effects.len(),
        1,
        "redocking one child emits one release; inputs={:#?}; view={:#?}",
        redocked.inputs(),
        native.session().view(),
    );
    assert!(matches!(
        effects[0].operation(),
        NativeEffectOperation::ReleaseChild { binding } if *binding == second
    ));
    // Retain this callback before release suppression; later ingress must not
    // reinterpret its frozen child facts using the now-suppressed route map.
    let historical_pointer = native.bridge.with_viewports(|routes| {
        NativeWindowEventRecord::for_test_snapshot(
            13,
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
            routes,
        )
    });
    let historical_routes = historical_pointer.pointer_routes().unwrap();
    assert_eq!(
        historical_routes.delivery(),
        NativePointerRouteSnapshot::Dock(first.window())
    );
    assert_eq!(
        historical_routes.hover(),
        NativePointerRouteSnapshot::Dock(second.window())
    );
    assert_eq!(
        historical_routes.capture(),
        NativePointerRouteSnapshot::Dock(second.window())
    );
    native
        .bridge
        .push_record(HostRecord::WindowEvent(historical_pointer));
    native
        .accept_native_effects(&egui::Context::default(), effects)
        .expect("the exact child release acknowledgement is retained");

    assert!(!native.session.is_current_native_binding(second));
    assert!(native.session.recognizes_native_binding(second));
    let version_before_history = native.session.version();
    let front = native.bridge.front_event().unwrap();
    assert_eq!(front.ordinal(), 13);
    assert_eq!(front.pointer_routes(), Some(historical_routes));
    assert!(
        native
            .reduce_callback_prefix(&egui::Context::default(), &mut test_budget())
            .expect("the retained pre-suppression callback must drain without InvalidPointerFacts")
            .is_some()
    );
    assert!(native.bridge.front_event().is_none());
    commit_pending_pointer_frame(&mut native);
    assert_eq!(native.session.version(), version_before_history);
    assert!(!native.session.has_active_gesture());
    drop(front);

    // Recognition admits history, never a fresh recovery capture checkpoint.
    let recorded = native.session.native_input_recorded_count();
    let error = native
        .session
        .report_native_pointer_roster(NativePointerRoster::Exact(vec![NativePointerState::new(
            NativePointerId::new(1),
            [NativePointerButton::Primary],
            NativePointerOwner::Native(second.window()),
        )]))
        .expect_err("a retained retired binding cannot authorize current roster capture");
    assert_eq!(error.native_kind(), Some(NativeHostErrorKind::InvalidFacts));
    assert_eq!(native.session.native_input_recorded_count(), recorded);

    assert!(native.session.native_local_coordinates(second).is_none());
    for input in [
        NativePointerInput::new(
            NativePointerId::new(70),
            NativePointerEvent::ButtonPressed(NativePointerButton::Primary),
            NativeDesktopPointerLocation::new(
                NativeDesktopPosition::Exact(point),
                NativePointerHover::Dock(first.window()),
                None,
            ),
            NativePointerOwner::Native(second.window()),
            NativePointerOwner::Native(second.window()),
        ),
        NativePointerInput::new_local(
            NativePointerId::new(71),
            NativePointerEvent::ButtonPressed(NativePointerButton::Primary),
            NativeLocalPointerLocation::Exact(retained_local),
            NativePointerOwner::Native(second.window()),
            NativePointerOwner::Native(second.window()),
        ),
    ] {
        native
            .session
            .record_native_pointer(input)
            .expect("the exact retained delivery and local facts remain recordable");
        let mut frame = native
            .begin_host_frame(|_| panic!("a retired delivery cannot query a current receiver"))
            .unwrap();
        frame
            .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
            .unwrap();
        frame.commit().unwrap();
        assert_eq!(native.session.version(), version_before_history);
        assert!(!native.session.has_active_gesture());
    }

    let destroyed = native.bridge.with_viewports_mut(|routes| {
        NativeWindowEventRecord::for_test_ingress(
            14,
            second_window,
            None,
            WindowEvent::Destroyed,
            routes,
        )
    });
    assert_eq!(destroyed.viewport_id(), Some(child));
    assert_eq!(destroyed.binding(), Some(second));
    native
        .bridge
        .push_record(HostRecord::WindowEvent(destroyed.clone()));
    native
        .bridge
        .push_record(HostRecord::WindowEvent(destroyed));
    let late_pointer = native.bridge.with_viewports(|routes| {
        NativeWindowEventRecord::for_test_snapshot(
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
            routes,
        )
    });
    let routes = late_pointer
        .pointer_routes()
        .expect("the late event retains explicit pointer authority");
    assert_eq!(
        routes.delivery(),
        NativePointerRouteSnapshot::Dock(first.window())
    );
    assert_eq!(routes.hover(), NativePointerRouteSnapshot::Unknown);
    assert_eq!(routes.capture(), NativePointerRouteSnapshot::Unknown);
    assert!(!late_pointer.references_binding(second));
    native.bridge.push_viewport_roster(live_roster([first]));
    native
        .bridge
        .push_record(HostRecord::WindowEvent(late_pointer));

    assert!(
        native
            .reduce_callback_prefix(&egui::Context::default(), &mut test_budget())
            .expect("the exact destruction callback is retained")
            .is_some()
    );
    assert_eq!(native.lifecycle_progress().destroyed_observations(), 1);
    {
        let mut dropped = native
            .begin_host_frame(|_| NativeReceiverAnswer::Unknown)
            .expect("the raw transfer can be retried after a dropped frame");
        dropped
            .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
            .unwrap();
    }
    assert!(native.bridge.retirement_destroyed_observations().is_empty());
    assert!(native.bridge.retirement_quiescence_candidates().is_empty());
    let mut event_frame = native
        .begin_host_frame(|_| NativeReceiverAnswer::Unknown)
        .expect("the destruction callback boundary begins");
    event_frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .expect("the callback boundary settles every surface");
    let event_report = event_frame
        .commit()
        .expect("the destruction callback boundary commits");
    assert!(event_report.inputs().iter().any(|input| matches!(
        input,
        HostInputOutcome::NativeInputBindingRetired { binding } if *binding == second
    )));
    assert!(!native.session.has_active_gesture());
    assert!(
        !native
            .pointer_translator
            .references_binding(second.window())
    );

    assert_eq!(native.viewport_binding(child), Some(second));
    assert!(!native.session.is_current_native_binding(second));
    assert!(native.session.recognizes_native_binding(second));
    assert!(
        native
            .deferred_viewport_specs()
            .iter()
            .all(|spec| spec.binding() != second),
        "the destruction callback stops re-declaring the retired child"
    );
    let (_, destroyed_facts) = native
        .bridge
        .retirement_destroyed_observations()
        .into_iter()
        .next()
        .expect("the exact destroyed child is awaiting snapshot publication");
    assert_ne!(
        destroyed_facts,
        NativeWindowFacts::destroyed(),
        "the destroyed tombstone retains the ReleaseChild acknowledgement"
    );

    assert!(
        native
            .reduce_callback_prefix(&egui::Context::default(), &mut test_budget())
            .expect("the complete roster publishes the tombstone")
            .is_some()
    );
    let mut roster_frame = native
        .begin_host_frame(|_| NativeReceiverAnswer::Unknown)
        .expect("the roster boundary begins");
    roster_frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .expect("the roster boundary settles every surface");
    let mut report = roster_frame
        .commit()
        .expect("the destroyed tombstone commits");
    assert!(
        report.take_native_effects().is_empty(),
        "the exact close acknowledgement settles ReleaseChild without cleanup fallback"
    );
    assert!(
        native
            .settle_host_frame_inputs(report.inputs())
            .expect("the exact native snapshot settles")
    );

    assert!(
        native
            .reduce_callback_prefix(&egui::Context::default(), &mut test_budget())
            .expect("the post-tombstone pointer edge remains reducible")
            .is_some()
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
    assert!(
        native.commit_retirements(prepared).is_empty(),
        "the fixture has no abandoned renderer outputs"
    );

    assert_eq!(native.viewport_binding(child), None);
    assert!(!native.session.is_current_native_binding(second));
    assert!(native.session.recognizes_native_binding(second));
    assert!(
        native
            .try_report_retirement_quiescence()
            .expect("the retired route becomes quiescent")
    );
    assert_eq!(native.lifecycle_progress().quiescent_retirements(), 1);
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

    let mut reclaimed_frame = native
        .begin_host_frame(|_| NativeReceiverAnswer::Unknown)
        .expect("the committed quiescence prefix is reclaimed");
    reclaimed_frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .expect("the reclamation boundary settles every surface");
    reclaimed_frame
        .commit()
        .expect("the quiescence prefix retires its exact destroyed guard");

    let mut foreign = coordinator();
    let (foreign_binding, _) = register_roots(&mut foreign);
    for rejected_binding in [second, foreign_binding] {
        assert!(!native.session.recognizes_native_binding(rejected_binding));
        let recorded = native.session.native_input_recorded_count();
        let input = NativePointerInput::new(
            NativePointerId::new(72),
            NativePointerEvent::Moved,
            NativeDesktopPointerLocation::new(
                NativeDesktopPosition::Unknown,
                NativePointerHover::Dock(rejected_binding.window()),
                None,
            ),
            NativePointerOwner::Native(first.window()),
            NativePointerOwner::Native(rejected_binding.window()),
        );
        let error = native
            .session
            .record_native_pointer(input)
            .expect_err("forgotten and foreign incarnations cannot enter pointer history");
        assert_eq!(error.native_kind(), Some(NativeHostErrorKind::InvalidFacts));
        assert_eq!(native.session.native_input_recorded_count(), recorded);
    }
}

#[test]
fn shutdown_drains_destroyed_child_to_quiescence_without_paint() {
    let mut native = coordinator();
    let (root, child_binding) = register_root_and_child(&mut native);
    let root_window = WindowId::from(31);
    let child_window = WindowId::from(32);
    let child = ViewportId::from_hash_of("shutdown-drain-child");
    native
        .bind_viewport(ViewportId::ROOT, root_window, root)
        .expect("root viewport binds");
    native
        .bind_viewport(child, child_window, child_binding)
        .expect("child viewport binds");
    let mut initially_presented = paint_and_present_all_surfaces(&mut native);
    assert!(initially_presented.take_native_effects().is_empty());

    let mut redock = native
        .begin_host_frame(|_| NativeReceiverAnswer::Unknown)
        .expect("the child redock frame begins");
    redock
        .frame
        .dock_root_current(
            RootId::new(2),
            DockPlacement::Center(DockAnchor::Item(ItemId::new(1))),
        )
        .expect("the complete child root redocks");
    redock
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .expect("the child redock settles every surface");
    let mut requested = redock.commit().expect("the child redock request commits");
    assert!(
        requested.take_native_effects().is_empty(),
        "ReleaseChild waits for the target presentation"
    );
    let mut report = paint_and_present_all_surfaces(&mut native);
    native
        .accept_native_effects(&egui::Context::default(), report.take_native_effects())
        .expect("the child release is accepted before shutdown");
    assert!(native.quarantine_after_fatal().is_empty());

    let destroyed = native.bridge.with_viewports_mut(|routes| {
        NativeWindowEventRecord::for_test_ingress(
            41,
            child_window,
            Some(child),
            WindowEvent::Destroyed,
            routes,
        )
    });
    native
        .bridge
        .push_record(HostRecord::WindowEvent(destroyed));
    native
        .bridge
        .push_terminal_viewport_roster(live_roster([root]));

    for phase in ["destroyed", "roster", "route", "quiescence"] {
        let advance =
            native.advance_shutdown_boundary(&egui::Context::default(), &mut test_budget());
        assert!(
            advance.progress,
            "shutdown {phase} boundary made no progress"
        );
        assert!(!advance.commands_dispatched);
        assert!(advance.abandoned_outputs.is_empty());
    }

    assert_eq!(native.viewport_binding(child), None);
    assert!(!native.session.is_current_native_binding(child_binding));
    assert!(!native.session.recognizes_native_binding(child_binding));
}

#[test]
fn redocked_external_root_does_not_request_platform_close() {
    let mut native = coordinator();
    let (first, second) = register_roots(&mut native);
    let second_viewport = ViewportId::from_hash_of("externally-owned-secondary-root");
    native
        .bind_viewport(ViewportId::ROOT, WindowId::from(11), first)
        .expect("primary root viewport binds");
    native
        .bind_viewport(second_viewport, WindowId::from(22), second)
        .expect("secondary root viewport binds");
    native
        .report_snapshot(
            [
                (first, NativeWindowFacts::live()),
                (second, NativeWindowFacts::live()),
            ],
            NativeWorkAreaRoster::Unknown,
            test_managed_capabilities(),
        )
        .expect("the exact root inventory records");
    let mut inventory = native
        .begin_host_frame(|_| NativeReceiverAnswer::Unknown)
        .expect("the root inventory frame begins");
    inventory
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .expect("the root inventory settles every surface");
    inventory.commit().expect("the root inventory commits");

    let mut redock = native
        .begin_host_frame(|_| NativeReceiverAnswer::Unknown)
        .expect("the application root redock frame begins");
    redock
        .frame
        .dock_root_current(
            RootId::new(2),
            DockPlacement::Center(DockAnchor::Item(ItemId::new(1))),
        )
        .expect("the complete application root redocks through the product action");
    redock
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .expect("the application root redock settles every surface");
    let mut report = redock.commit().expect("the external root redock commits");
    assert!(
        report.take_native_effects().is_empty(),
        "an externally owned root unbinds without a platform close effect"
    );
    assert!(
        !native
            .dispatch_viewport_commands(&egui::Context::default())
            .expect("no native focus command is queued")
    );
    assert!(!native.session().is_current_native_binding(second));
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
        primary_press: None,
        device_id: DeviceId::dummy(),
        state: ElementState::Released,
        button: MouseButton::Left,
        facts: PointerEventFacts {
            hover: PointerWindowRoute::Window(captured_window),
            capture: PointerWindowRoute::Window(captured_window),
            ..PointerEventFacts::default()
        },
    };
    let record = native.bridge.with_viewports(|routes| {
        NativeWindowEventRecord::for_test_snapshot(
            15,
            delivery_window,
            Some(ViewportId::ROOT),
            event,
            routes,
        )
    });

    assert_eq!(record.binding(), Some(delivery));
    assert!(record.references_binding(captured));
    native.bridge.push_record(HostRecord::WindowEvent(record));
    assert!(native.bridge.references_binding(captured));
}
