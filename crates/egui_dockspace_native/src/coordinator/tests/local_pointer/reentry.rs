use super::*;

#[test]
fn native_mouse_reentry_without_release_recovers_on_a_new_press() {
    for desktop in [false, true] {
        let (mut native, binding, scene, now) = fixture_with_desktop(desktop);
        let source = scene.point(FIRST, 0.25);
        let target = scene.point(THIRD, 0.625);
        let version = native.session.version();
        now.store(40, Ordering::SeqCst);

        let event = |point, state| {
            if desktop {
                raw_desktop_event(point, state)
            } else {
                raw_event(point, state, 2.0)
            }
        };
        for (ordinal, point, state) in [(1, source, Some(ElementState::Pressed)), (2, target, None)]
        {
            let record = capture(&native, binding, ordinal, event(point, state));
            native.bridge.push_record(HostRecord::WindowEvent(record));
            reduce_one(&mut native);
            input_boundary(&mut native, &scene, CAPTURED_AT);
        }
        assert!(native.session.has_active_gesture());

        // A remote desktop can stop forwarding events while the button is held.
        // Enter/leave events neither prove a release nor invalidate cross-window capture.
        for (ordinal, event) in [
            (
                3,
                WindowEvent::CursorLeft {
                    device_id: DeviceId::dummy(),
                },
            ),
            (
                4,
                WindowEvent::CursorEntered {
                    device_id: DeviceId::dummy(),
                },
            ),
        ] {
            native
                .bridge
                .push_record(HostRecord::WindowEvent(NativeWindowEventRecord::for_test(
                    ordinal,
                    WindowId::from(WINDOW),
                    Some(ViewportId::ROOT),
                    Some(binding),
                    event,
                )));
            native
                .reduce_callback_prefix(&egui::Context::default(), &mut test_budget())
                .unwrap();
            input_boundary(&mut native, &scene, CAPTURED_AT);
            assert!(native.session.has_active_gesture());
        }

        let fresh_press = capture(
            &native,
            binding,
            5,
            event(source, Some(ElementState::Pressed)),
        );
        native
            .bridge
            .push_record(HostRecord::WindowEvent(fresh_press));
        native
            .reduce_callback_prefix(&egui::Context::default(), &mut test_budget())
            .expect("a new native press must retire a lost-release gesture before starting its successor");
        let restarted = input_boundary(&mut native, &scene, CAPTURED_AT);
        assert!(native.session.has_active_gesture());
        assert_eq!(native.session.version(), version);
        assert!(!restarted.workspace_changed());

        for (ordinal, point, state) in
            [(6, target, None), (7, target, Some(ElementState::Released))]
        {
            let record = capture(&native, binding, ordinal, event(point, state));
            native.bridge.push_record(HostRecord::WindowEvent(record));
            reduce_one(&mut native);
            input_boundary(&mut native, &scene, CAPTURED_AT);
        }
        assert!(!native.session.has_active_gesture());
        assert_ne!(native.session.version(), version);
        assert_eq!(order(&native), [SECOND, THIRD, FIRST]);
        assert!(native.pointer_translator.can_coalesce_idle_cursor_moves());
        assert!(native.next_window_event().unwrap().is_none());
    }
}

#[test]
fn native_secondary_button_release_after_a_completed_primary_drag_keeps_the_stream_consistent() {
    for desktop in [false, true] {
        let (mut native, binding, scene, now) = fixture_with_desktop(desktop);
        let source = scene.point(FIRST, 0.25);
        let target = scene.point(THIRD, 0.625);
        now.store(40, Ordering::SeqCst);
        for (ordinal, point, state, button) in [
            (1, source, Some(ElementState::Pressed), MouseButton::Left),
            (2, source, Some(ElementState::Pressed), MouseButton::Right),
            (3, target, None, MouseButton::Left),
            (4, target, Some(ElementState::Released), MouseButton::Left),
            (5, target, Some(ElementState::Released), MouseButton::Right),
        ] {
            let mut event = if desktop {
                raw_desktop_event(point, state)
            } else {
                raw_event(point, state, 2.0)
            };
            if let WindowEvent::MouseInput { button: actual, .. } = &mut event {
                *actual = button;
            }
            let record = capture(&native, binding, ordinal, event);
            native.bridge.push_record(HostRecord::WindowEvent(record));
            reduce_one(&mut native);
            let report = input_boundary(&mut native, &scene, CAPTURED_AT);
            if ordinal == 4 {
                assert!(report.workspace_changed());
                assert_eq!(order(&native), [SECOND, THIRD, FIRST]);
            } else {
                assert!(!report.workspace_changed());
                assert!(report.manipulation_feedback().is_empty());
            }
        }
        assert!(!native.session.has_active_gesture());
        assert!(!native.session.has_pending_native_input());
        assert!(native.pointer_translator.can_coalesce_idle_cursor_moves());
    }
}
