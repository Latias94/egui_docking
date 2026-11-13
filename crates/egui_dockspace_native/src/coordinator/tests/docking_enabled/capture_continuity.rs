use super::*;
use dockspace::runtime::DockspaceManipulationCancelReason;

#[test]
fn cross_window_drag_preserves_unknown_capture_and_accepts_an_exact_uncaptured_release() {
    let (mut native, _, source_binding) = presented_routes();
    let (receivers, source, target) = source_and_target(&mut native);
    let version = native.session.version();
    for (ordinal, point, state, hover, capture) in [
        (
            1,
            source,
            Some(ElementState::Pressed),
            22,
            PointerWindowRoute::Window(WindowId::from(22)),
        ),
        (
            2,
            target,
            None,
            11,
            PointerWindowRoute::Window(WindowId::from(22)),
        ),
        // AppKit can emit an unknown-capture position update before mouseUp.
        (3, target, None, 11, PointerWindowRoute::Unknown),
        (
            4,
            target,
            Some(ElementState::Released),
            11,
            PointerWindowRoute::None,
        ),
    ] {
        let mut event = mouse_event(point, state, hover, None);
        let (WindowEvent::MouseInput { facts, .. } | WindowEvent::CursorMoved { facts, .. }) =
            &mut event
        else {
            unreachable!();
        };
        facts.capture = capture;
        let (preview, report) =
            apply_input_callback(&mut native, source_binding, ordinal, event, &receivers);
        if ordinal == 2 {
            assert!(matches!(
                preview,
                Some(DockspacePreviewVisual::Dock {
                    surface: FIRST_SURFACE,
                    ..
                })
            ));
        }
        if ordinal < 4 {
            assert!(native.session.has_active_gesture());
            assert!(!report.workspace_changed());
            assert!(report.manipulation_feedback().is_empty());
        } else {
            assert!(!native.session.has_active_gesture());
            assert!(
                report.workspace_changed(),
                "an exact source-delivered release must complete cross-window docking after an unobservable capture sample: feedback={:?}, pending_presentation={}",
                report.manipulation_feedback(),
                native.session.has_pending_presentation(),
            );
            assert_ne!(native.session.version(), version);
            assert_eq!(
                native
                    .session
                    .view()
                    .item(ItemId::new(2))
                    .unwrap()
                    .surface(),
                FIRST_SURFACE
            );
        }
    }
}

#[test]
fn cross_window_drag_cancels_when_motion_proves_capture_is_gone() {
    let (mut native, _, source_binding) = presented_routes();
    let (receivers, source, target) = source_and_target(&mut native);
    for (ordinal, point, state, hover) in [
        (1, source, Some(ElementState::Pressed), 22),
        (2, target, None, 11),
    ] {
        apply_input_callback(
            &mut native,
            source_binding,
            ordinal,
            mouse_event(point, state, hover, None),
            &receivers,
        );
    }
    assert!(native.session.has_active_gesture());
    let version = native.session.version();
    let mut motion = mouse_event(target, None, 11, None);
    let WindowEvent::CursorMoved { facts, .. } = &mut motion else {
        unreachable!();
    };
    facts.capture = PointerWindowRoute::None;
    let (_, report) = apply_input_callback(&mut native, source_binding, 3, motion, &receivers);
    assert!(!native.session.has_active_gesture());
    assert!(!report.workspace_changed());
    assert_eq!(native.session.version(), version);
    assert!(matches!(
        report.manipulation_feedback(),
        [feedback] if feedback.status() == Status::Cancelled(DockspaceManipulationCancelReason::CaptureLost)
    ));
}

#[test]
fn cross_window_release_with_unknown_capture_preserves_its_exact_pre_edge_capture() {
    let (mut native, _, source_binding) = presented_routes();
    let (receivers, source, target) = source_and_target(&mut native);
    for (ordinal, point, state, hover) in [
        (1, source, Some(ElementState::Pressed), 22),
        (2, target, None, 11),
    ] {
        apply_input_callback(
            &mut native,
            source_binding,
            ordinal,
            mouse_event(point, state, hover, None),
            &receivers,
        );
    }
    let mut release = mouse_event(target, Some(ElementState::Released), 11, None);
    let WindowEvent::MouseInput { facts, .. } = &mut release else {
        unreachable!();
    };
    facts.capture = PointerWindowRoute::Unknown;
    let (_, report) = apply_input_callback(&mut native, source_binding, 3, release, &receivers);
    assert!(report.workspace_changed());
    assert!(!native.session.has_active_gesture());
    assert_eq!(
        native
            .session
            .view()
            .item(ItemId::new(2))
            .unwrap()
            .surface(),
        FIRST_SURFACE
    );
}

#[test]
fn source_focus_loss_preserves_capture_during_a_normal_cross_window_drag() {
    let (mut native, target_binding, source_binding) = presented_routes();
    let (receivers, source, target) = source_and_target(&mut native);
    for (ordinal, point, state, hover) in [
        (1, source, Some(ElementState::Pressed), 22),
        (2, target, None, 11),
    ] {
        apply_input_callback(
            &mut native,
            source_binding,
            ordinal,
            mouse_event(point, state, hover, None),
            &receivers,
        );
    }
    let capture = native.session.native_pointer_capture_request().unwrap();
    let lost_focus = captured_record(&native, source_binding, 3, WindowEvent::Focused(false));
    native
        .bridge
        .push_record(HostRecord::WindowEvent(lost_focus));
    native
        .reduce_callback_prefix(&egui::Context::default(), &mut test_budget())
        .unwrap();
    let unfocused = finish(
        native
            .begin_host_frame(|query| resolve(query, &receivers))
            .unwrap(),
    );
    assert!(unfocused.manipulation_feedback().is_empty());
    assert!(native.session.has_active_gesture());
    assert_eq!(
        native.session.native_pointer_capture_request(),
        Some(capture)
    );
    native
        .session
        .report_native_global_focus(dockspace::runtime::NativeGlobalFocus::Dock(
            target_binding.window(),
        ))
        .unwrap();
    let transferred = finish(
        native
            .begin_host_frame(|query| resolve(query, &receivers))
            .unwrap(),
    );
    assert!(transferred.manipulation_feedback().is_empty());
    assert!(native.session.has_active_gesture());
    assert_eq!(
        native.session.native_pointer_capture_request(),
        Some(capture)
    );
    let (_, released) = apply_input_callback(
        &mut native,
        source_binding,
        4,
        mouse_event(target, Some(ElementState::Released), 11, None),
        &receivers,
    );
    assert!(released.workspace_changed());
    assert!(!native.session.has_active_gesture());
}
