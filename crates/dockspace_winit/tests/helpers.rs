use std::time::{Duration, Instant};

use backend::dpi::PhysicalPosition;
use backend::event::{DeviceId, ElementState, MouseButton, WindowEvent};
use backend::keyboard::ModifiersState;
use dockspace::geometry::{GeometryError, LogicalPoint, PhysicalPoint, ScaleFactor};
use dockspace::runtime::{
    SurfacePointerButton, SurfacePointerCancelReason, SurfacePointerEvent, SurfacePointerPosition,
};
use dockspace_winit::{
    MouseInput, RepaintScheduler, SurfaceCoordinates, backend, map_mouse_button,
};

#[test]
fn repaint_requests_keep_the_earliest_deadline() {
    let now = Instant::now();
    let early = now + Duration::from_millis(5);
    let late = now + Duration::from_millis(20);
    let mut scheduler = RepaintScheduler::default();
    scheduler.request(late);
    scheduler.request(early);
    scheduler.request(late);
    assert_eq!(scheduler.deadline(), Some(early));
    assert!(!scheduler.take_due(now));
    assert!(scheduler.take_due(early));
    assert!(
        !scheduler.take_due(late),
        "an old timer cannot request another repaint"
    );
    assert_eq!(scheduler.deadline(), None);
}

#[test]
fn immediate_frames_preserve_future_repaint_requests() {
    let now = Instant::now();
    let future = now + Duration::from_secs(1);
    let mut scheduler = RepaintScheduler::default();
    scheduler.request(future);
    scheduler.frame_started(now);
    assert_eq!(scheduler.deadline(), Some(future));
    scheduler.frame_started(future);
    assert_eq!(scheduler.deadline(), None);
    assert!(!scheduler.take_due(future));
}

#[test]
fn late_repaint_delivery_uses_the_original_absolute_deadline() {
    let requested = Instant::now();
    let deadline = requested + Duration::from_millis(5);
    let delivered = requested + Duration::from_millis(25);
    let mut scheduler = RepaintScheduler::default();
    scheduler.request(deadline);
    assert!(scheduler.take_due(delivered));
}

#[test]
fn coordinates_apply_client_offsets_and_surface_scales() {
    let origin = PhysicalPoint::new(100.0, 50.0).unwrap();
    for scale in [0.5, 1.0, 1.25, 2.0] {
        let coordinates = SurfaceCoordinates::new(origin, ScaleFactor::new(scale).unwrap());
        assert_eq!(
            coordinates.to_surface(PhysicalPosition::new(
                100.0 + 24.0 * scale,
                50.0 - 8.0 * scale
            )),
            Ok(LogicalPoint::new(24.0, -8.0).unwrap())
        );
    }
}

#[test]
fn coordinates_reject_non_finite_input_and_conversion_overflow() {
    let zero = PhysicalPoint::new(0.0, 0.0).unwrap();
    let normal = SurfaceCoordinates::new(zero, ScaleFactor::new(1.0).unwrap());
    let subtraction_overflow = SurfaceCoordinates::new(
        PhysicalPoint::new(-f64::MAX, 0.0).unwrap(),
        ScaleFactor::new(1.0).unwrap(),
    );
    let division_overflow =
        SurfaceCoordinates::new(zero, ScaleFactor::new(f64::MIN_POSITIVE).unwrap());
    for result in [
        normal.to_surface(PhysicalPosition::new(f64::NAN, 0.0)),
        normal.to_surface(PhysicalPosition::new(0.0, f64::INFINITY)),
        subtraction_overflow.to_surface(PhysicalPosition::new(f64::MAX, 0.0)),
        division_overflow.to_surface(PhysicalPosition::new(f64::MAX, 0.0)),
    ] {
        assert!(matches!(result, Err(GeometryError::NonFinite { .. })));
    }
}

fn moved(position: PhysicalPosition<f64>) -> WindowEvent {
    WindowEvent::CursorMoved {
        device_id: DeviceId::dummy(),
        position,
    }
}

fn button(state: ElementState) -> WindowEvent {
    WindowEvent::MouseInput {
        device_id: DeviceId::dummy(),
        state,
        button: MouseButton::Left,
    }
}

#[test]
fn mouse_buttons_retain_cursor_and_ordered_modifiers() {
    let mut mouse = MouseInput::default();
    let position = PhysicalPosition::new(30.0, 40.0);
    assert!(
        mouse
            .on_window_event(&WindowEvent::ModifiersChanged(ModifiersState::SHIFT.into()))
            .is_none()
    );
    let motion = mouse.on_window_event(&moved(position)).unwrap();
    assert_eq!(motion.event, SurfacePointerEvent::Moved);
    assert_eq!(motion.position, Some(position));
    assert_eq!(motion.modifiers, ModifiersState::SHIFT);
    let pressed = mouse
        .on_window_event(&button(ElementState::Pressed))
        .unwrap();
    assert_eq!(
        pressed.event,
        SurfacePointerEvent::ButtonPressed(SurfacePointerButton::Primary)
    );
    assert_eq!(pressed.position, Some(position));
    assert_eq!(pressed.modifiers, ModifiersState::SHIFT);
    let released = mouse
        .on_window_event(&button(ElementState::Released))
        .unwrap();
    assert_eq!(
        released.event,
        SurfacePointerEvent::ButtonReleased(SurfacePointerButton::Primary)
    );
}

#[test]
fn cursor_departure_forgets_position_without_cancelling_drag() {
    let mut mouse = MouseInput::default();
    let _ = mouse.on_window_event(&moved(PhysicalPosition::new(30.0, 40.0)));
    assert!(
        mouse
            .on_window_event(&WindowEvent::CursorLeft {
                device_id: DeviceId::dummy()
            })
            .is_none()
    );
    let released = mouse
        .on_window_event(&button(ElementState::Released))
        .unwrap();
    assert_eq!(
        released.event,
        SurfacePointerEvent::ButtonReleased(SurfacePointerButton::Primary)
    );
    assert_eq!(released.position, None);
}

#[test]
fn mouse_button_without_cursor_stays_unknown_after_coordinate_mapping() {
    let mut mouse = MouseInput::default();
    let sample = mouse
        .on_window_event(&button(ElementState::Pressed))
        .unwrap();
    let coordinates = SurfaceCoordinates::new(
        PhysicalPoint::new(0.0, 0.0).unwrap(),
        ScaleFactor::new(2.0).unwrap(),
    );
    assert_eq!(
        sample.logical_position(coordinates),
        Ok(SurfacePointerPosition::Unknown)
    );
    let _ = mouse.on_window_event(&moved(PhysicalPosition::new(30.0, 40.0)));
    let sample = mouse
        .on_window_event(&button(ElementState::Pressed))
        .unwrap();
    assert_eq!(
        sample.logical_position(coordinates),
        Ok(SurfacePointerPosition::Known(
            LogicalPoint::new(15.0, 20.0).unwrap()
        ))
    );
}

#[test]
fn focus_loss_and_window_retirement_cancel_for_distinct_reasons() {
    for (event, reason) in [
        (
            WindowEvent::Focused(false),
            SurfacePointerCancelReason::ExplicitCancellation,
        ),
        (
            WindowEvent::Destroyed,
            SurfacePointerCancelReason::BindingRetired,
        ),
    ] {
        let mut mouse = MouseInput::default();
        let _ = mouse.on_window_event(&moved(PhysicalPosition::new(30.0, 40.0)));
        let _ = mouse.on_window_event(&WindowEvent::ModifiersChanged(
            ModifiersState::CONTROL.into(),
        ));
        let cancelled = mouse.on_window_event(&event).unwrap();
        assert_eq!(
            cancelled.event,
            SurfacePointerEvent::StreamCancelled(reason)
        );
        assert_eq!(cancelled.position, None);
        assert!(cancelled.modifiers.is_empty());
        let later = mouse
            .on_window_event(&button(ElementState::Pressed))
            .unwrap();
        assert_eq!(later.position, None);
        assert!(later.modifiers.is_empty());
    }
}

#[test]
fn mouse_button_mapping_preserves_standard_and_extra_buttons() {
    for (source, target) in [
        (MouseButton::Left, SurfacePointerButton::Primary),
        (MouseButton::Right, SurfacePointerButton::Secondary),
        (MouseButton::Middle, SurfacePointerButton::Middle),
        (MouseButton::Back, SurfacePointerButton::Other(4)),
        (MouseButton::Forward, SurfacePointerButton::Other(5)),
        (MouseButton::Other(42), SurfacePointerButton::Other(42)),
    ] {
        assert_eq!(map_mouse_button(source), target);
    }
}
