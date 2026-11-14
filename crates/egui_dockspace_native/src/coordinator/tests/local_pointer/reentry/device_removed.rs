//! Provider removal must isolate old mouse/scroll tails and permit real docking.

use super::*;
use crate::event::NativeDeviceRemovalRecord;
use winit::event::{MouseScrollDelta, TouchPhase};

fn mouse(
    desktop: bool,
    point: LogicalPoint,
    state: Option<ElementState>,
    button: MouseButton,
) -> WindowEvent {
    let mut event = if desktop {
        raw_desktop_event(point, state)
    } else {
        raw_event(point, state, 2.0)
    };
    if let WindowEvent::MouseInput { button: actual, .. } = &mut event {
        *actual = button;
    }
    event
}

fn scroll(desktop: bool, point: LogicalPoint, phase: TouchPhase) -> WindowEvent {
    let WindowEvent::CursorMoved {
        device_id, facts, ..
    } = mouse(desktop, point, None, MouseButton::Left)
    else {
        unreachable!("the fixture creates cursor motion without a button edge")
    };
    WindowEvent::MouseWheel {
        device_id,
        delta: MouseScrollDelta::LineDelta(0.0, 1.0),
        phase,
        facts,
    }
}

fn answer(scene: &Scene, query: NativeReceiverQuery) -> NativeReceiverAnswer {
    match query {
        // The fixture publishes mouse receivers, not an application scroll widget.
        // Unknown preserves that distinction while core still tracks scroll phases.
        NativeReceiverQuery::Surface(query)
            if query.metadata().device() != PointerDeviceKind::Mouse =>
        {
            NativeReceiverAnswer::Unknown
        }
        _ => scene.answer(query, CAPTURED_AT),
    }
}

fn commit(native: &mut NativeCoordinator, scene: &Scene) -> HostFrameReport {
    let report = finish(
        native
            .begin_host_frame(|query| answer(scene, query))
            .unwrap(),
    );
    native.settle_host_frame_report(&report).unwrap();
    report
}

fn apply(
    native: &mut NativeCoordinator,
    binding: NativeSurfaceBinding,
    scene: &Scene,
    ordinal: u64,
    event: WindowEvent,
) -> HostFrameReport {
    let record = capture(native, binding, ordinal, event);
    native.bridge.push_record(HostRecord::WindowEvent(record));
    reduce_one(native);
    commit(native, scene)
}

fn assert_items(native: &NativeCoordinator) {
    let view = native.session.view();
    assert_eq!(view.surfaces().count(), 1);
    let surface = view.surface(FIRST_SURFACE).unwrap();
    assert_eq!(surface.contained_count(), 0);
    let root = surface.main_root().unwrap();
    let tabs = root.content().unwrap().tabs().unwrap();
    let mut items = tabs.items().to_vec();
    items.sort_unstable();
    assert_eq!(items, [FIRST, SECOND, THIRD]);
    for item in items {
        let location = view.item(item).unwrap();
        assert_eq!(location.surface(), FIRST_SURFACE);
        assert_eq!(location.root(), root.id());
    }
}

fn assert_device_removal_recovery(desktop: bool) {
    let (mut native, binding, scene, now) = fixture_with_desktop(desktop);
    now.store(40, Ordering::SeqCst);
    let source = scene.point(FIRST, 0.25);
    let target = scene.point(THIRD, 0.625);
    let version = native.session.version();

    for (ordinal, event) in [
        (
            1,
            mouse(
                desktop,
                source,
                Some(ElementState::Pressed),
                MouseButton::Left,
            ),
        ),
        (
            2,
            mouse(
                desktop,
                source,
                Some(ElementState::Pressed),
                MouseButton::Right,
            ),
        ),
        (3, mouse(desktop, target, None, MouseButton::Left)),
        (4, scroll(desktop, target, TouchPhase::Started)),
    ] {
        let report = apply(&mut native, binding, &scene, ordinal, event);
        assert!(!report.workspace_changed(), "ordinal={ordinal}: {report:?}");
        assert!(report.manipulation_feedback().is_empty(), "{report:?}");
        assert!(native.session.has_active_gesture());
        assert_eq!(order(&native), [FIRST, SECOND, THIRD]);
        assert_items(&native);
    }
    let old_pointer = NativePointerId::new(1);
    assert_eq!(
        native
            .pointer_translator
            .mouse_device_for_pointer(old_pointer),
        Some(DeviceId::dummy())
    );
    let recorded = native.session.native_input_recorded_count();
    native.bridge.push_record(HostRecord::DeviceRemoval(
        NativeDeviceRemovalRecord::for_test(5),
    ));
    assert_eq!(
        native
            .reduce_callback_prefix(&egui::Context::default(), &mut test_budget())
            .unwrap()
            .unwrap()
            .get(),
        1
    );
    assert_eq!(native.session.native_input_recorded_count(), recorded + 1);
    let removed = format!("{:?}", native.pointer_translator);
    drop(
        native
            .begin_host_frame(|query| answer(&scene, query))
            .unwrap(),
    );
    assert!(native.session.has_active_gesture());
    assert!(native.session.has_pending_native_input());
    assert_eq!(native.session.version(), version);
    assert_eq!(order(&native), [FIRST, SECOND, THIRD]);
    assert_eq!(native.session.native_input_recorded_count(), recorded + 1);
    assert_eq!(format!("{:?}", native.pointer_translator), removed);

    let reset = commit(&mut native, &scene);
    assert_eq!(reset.inputs(), [HostInputOutcome::NativeInputDeviceRemoval]);
    let cancelled = feedback(&reset);
    assert_eq!(
        cancelled.status(),
        Status::Cancelled(CancelReason::PointerStreamCancelled)
    );
    assert!(!reset.workspace_changed());
    assert!(!native.session.has_active_gesture());
    assert!(native.session.native_pointer_capture_request().is_none());
    assert!(native.pointer_translator.can_coalesce_idle_cursor_moves());
    assert!(
        !native
            .pointer_translator
            .references_binding(binding.window())
    );
    assert!(
        native
            .pointer_translator
            .mouse_device_for_pointer(old_pointer)
            .is_none()
    );
    assert_items(&native);

    for (ordinal, mut event) in [
        (
            6,
            mouse(
                desktop,
                target,
                Some(ElementState::Released),
                MouseButton::Left,
            ),
        ),
        (
            7,
            mouse(
                desktop,
                target,
                Some(ElementState::Released),
                MouseButton::Right,
            ),
        ),
        (8, mouse(desktop, target, None, MouseButton::Left)),
        (9, scroll(desktop, target, TouchPhase::Moved)),
        (10, scroll(desktop, target, TouchPhase::Ended)),
        (11, scroll(desktop, target, TouchPhase::Cancelled)),
    ] {
        // Even exact-looking old capture facts cannot revive a removed stream.
        match &mut event {
            WindowEvent::MouseInput { facts, .. }
            | WindowEvent::CursorMoved { facts, .. }
            | WindowEvent::MouseWheel { facts, .. } => {
                facts.capture = PointerWindowRoute::Window(WindowId::from(WINDOW));
            }
            _ => unreachable!("only mouse and wheel tails are queued"),
        }
        let record = capture(&native, binding, ordinal, event);
        native.bridge.push_record(HostRecord::WindowEvent(record));
        assert_eq!(
            native
                .reduce_callback_prefix(&egui::Context::default(), &mut test_budget())
                .unwrap()
                .unwrap()
                .get(),
            1,
            "tail ordinal={ordinal}"
        );
        let report = commit(&mut native, &scene);
        assert!(
            report.inputs().is_empty(),
            "tail ordinal={ordinal}: {report:?}"
        );
        assert!(report.manipulation_feedback().is_empty(), "{report:?}");
        assert!(!report.workspace_changed());
        assert_eq!(native.session.native_input_recorded_count(), recorded + 1);
        assert_eq!(format!("{:?}", native.pointer_translator), removed);
        assert_eq!(native.session.version(), version);
        assert_eq!(order(&native), [FIRST, SECOND, THIRD]);
        assert!(!native.session.has_active_gesture());
        assert!(native.session.native_pointer_capture_request().is_none());
        assert_items(&native);
    }
    assert!(idle(&mut native).manipulation_feedback().is_empty());
    let scene = paint_and_present(&mut native, binding);
    let recorded = native.session.native_input_recorded_count();
    // Mouse and scroll owned identities 1 and 2 before removal. Their replacement
    // lifetimes must enroll independently, beginning with mouse identity 3.
    let new_pointer = NativePointerId::new(3);
    let mut new_capture = None;
    for (ordinal, event) in [
        (
            12,
            mouse(
                desktop,
                source,
                Some(ElementState::Pressed),
                MouseButton::Left,
            ),
        ),
        (13, scroll(desktop, target, TouchPhase::Started)),
        (14, mouse(desktop, target, None, MouseButton::Left)),
        (15, scroll(desktop, target, TouchPhase::Moved)),
        (16, scroll(desktop, target, TouchPhase::Ended)),
    ] {
        let report = apply(&mut native, binding, &scene, ordinal, event);
        assert!(!report.workspace_changed(), "ordinal={ordinal}: {report:?}");
        assert!(report.manipulation_feedback().is_empty(), "{report:?}");
        assert!(native.session.has_active_gesture());
        assert_eq!(native.session.version(), version);
        assert_eq!(order(&native), [FIRST, SECOND, THIRD]);
        assert_eq!(
            native
                .pointer_translator
                .mouse_device_for_pointer(new_pointer),
            Some(DeviceId::dummy())
        );
        assert!(
            native
                .pointer_translator
                .mouse_device_for_pointer(old_pointer)
                .is_none()
        );
        assert_items(&native);
        if ordinal == 14 {
            new_capture = native.session.native_pointer_capture_request();
            if desktop {
                assert_eq!(new_capture.unwrap().pointer(), new_pointer);
            } else {
                assert!(new_capture.is_none());
            }
        } else if ordinal > 14 {
            assert_eq!(native.session.native_pointer_capture_request(), new_capture);
        }
    }
    assert_eq!(native.session.native_input_recorded_count(), recorded + 5);

    let release = capture(
        &native,
        binding,
        17,
        mouse(
            desktop,
            target,
            Some(ElementState::Released),
            MouseButton::Left,
        ),
    );
    native.bridge.push_record(HostRecord::WindowEvent(release));
    reduce_one(&mut native);
    let mut frame = native
        .begin_host_frame(|query| answer(&scene, query))
        .unwrap();
    let successor = render(&mut frame);
    let mut report = finish(frame);
    let delivered = feedback(&report);
    assert_eq!(delivered.status(), Status::Delivered(Delivery::Dock));
    assert_ne!(delivered.manipulation_id(), cancelled.manipulation_id());
    assert_ne!(native.session.version(), version);
    assert_eq!(report.before(), version);
    assert_eq!(report.after(), native.session.version());
    assert_eq!(native.session.native_input_recorded_count(), recorded + 6);
    assert_committed_reorder_survives_presentation(&mut native, binding, &successor, &mut report);
    assert!(!native.session.has_pending_presentation());
    assert!(!native.session.has_pending_native_input());
    assert!(native.pointer_translator.can_coalesce_idle_cursor_moves());
    assert_eq!(native.bridge.input_backlog_for_test(), (0, None));
    assert_items(&native);
}

#[test]
fn native_local_device_removal_isolates_mouse_and_scroll_tails_before_the_next_dock() {
    assert_device_removal_recovery(false);
}

#[test]
fn native_desktop_device_removal_isolates_mouse_and_scroll_tails_before_the_next_dock() {
    assert_device_removal_recovery(true);
}
