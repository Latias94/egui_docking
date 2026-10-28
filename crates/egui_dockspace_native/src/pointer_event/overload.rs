//! Regressions for translation after committed core overload cancellation.

use super::*;
use crate::viewport_map::NativeViewportMap;
use dockspace::model::{
    DockspaceLayout, DockspaceNode, DockspaceRootLayout, DockspaceSurfaceLayout, ItemId, RootId,
    SurfaceId,
};
use dockspace::policy::DockPolicy;
use dockspace::runtime::{
    DockspaceSession, HostInputOutcome, HostWindowToken, NativePointerRoster,
    SurfaceUnavailableReason,
};
use eframe::egui::ViewportId;
use winit::dpi::PhysicalPosition;
use winit::event::{PointerEventFacts, PointerWindowRoute, Touch};
use winit::window::WindowId;

fn record(event: WindowEvent) -> NativeWindowEventRecord {
    NativeWindowEventRecord::for_test(1, WindowId::from(1), None, None, event)
}

fn mouse(state: ElementState) -> WindowEvent {
    WindowEvent::MouseInput {
        device_id: DeviceId::dummy(),
        state,
        button: MouseButton::Left,
        facts: PointerEventFacts::default(),
    }
}

fn touch(phase: TouchPhase) -> WindowEvent {
    WindowEvent::Touch(Touch {
        device_id: DeviceId::dummy(),
        phase,
        location: PhysicalPosition::new(10.0, 20.0),
        force: None,
        id: 7,
    })
}

fn wheel(phase: TouchPhase) -> WindowEvent {
    WindowEvent::MouseWheel {
        device_id: DeviceId::dummy(),
        delta: MouseScrollDelta::LineDelta(0.0, 2.0),
        phase,
        facts: PointerEventFacts::default(),
    }
}

fn pan(phase: TouchPhase) -> WindowEvent {
    WindowEvent::PanGesture {
        device_id: DeviceId::dummy(),
        delta: PhysicalPosition::new(0.0, 2.0),
        phase,
    }
}

fn expected(pointer: u64, event: NativePointerEvent) -> NativePointerTranslation {
    NativePointerTranslation::Input(NativePointerInput::new(
        NativePointerId::new(pointer),
        event,
        NativeDesktopPointerLocation::new(
            NativeDesktopPosition::Unknown,
            NativePointerHover::Unknown,
            None,
        ),
        NativePointerOwner::Unknown,
        NativePointerOwner::Unknown,
    ))
}

fn scrolled(pointer: u64, phase: NativeScrollPhase) -> NativePointerTranslation {
    expected(
        pointer,
        NativePointerEvent::Scrolled(NativeScrollEvent::new(
            NativeScrollDeviceId::new(pointer),
            phase,
            NativeScrollMomentum::Unknown,
            NativeScrollModifiers::Unknown,
        )),
    )
}

fn binding() -> NativeSurfaceBinding {
    let surface = SurfaceId::new(1);
    let layout = DockspaceLayout::new([DockspaceSurfaceLayout::new(
        surface,
        DockspaceRootLayout::new(
            RootId::new(1),
            DockspaceNode::central_tabs([ItemId::new(1)]),
        ),
    )])
    .expect("test layout validates");
    let mut session = DockspaceSession::from_layout(layout, DockPolicy::default())
        .expect("test session initializes");
    session
        .enable_managed_native_host(NativePointerRoster::Unknown)
        .expect("managed native host enrolls");
    session
        .register_native_root(surface, HostWindowToken::new(1))
        .expect("root registration records");
    let mut frame = session
        .begin_host_frame()
        .expect("registration frame begins");
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .expect("registration frame settles the surface");
    frame
        .commit()
        .expect("registration frame commits")
        .inputs()
        .iter()
        .find_map(|outcome| match outcome {
            HostInputOutcome::NativeSurfaceRegistered { binding } => Some(*binding),
            _ => None,
        })
        .expect("registration emits one binding")
}

#[test]
fn overload_discards_mouse_buttons_origins_and_capture_before_fresh_press() {
    let binding = binding();
    let window = WindowId::from(1);
    let mut viewports = NativeViewportMap::default();
    viewports
        .bind(ViewportId::ROOT, window, binding)
        .expect("test viewport binds");
    let mut press = mouse(ElementState::Pressed);
    let WindowEvent::MouseInput { facts, .. } = &mut press else {
        unreachable!()
    };
    facts.capture = PointerWindowRoute::Window(window);
    let press = NativeWindowEventRecord::for_test_snapshot(
        1,
        window,
        Some(ViewportId::ROOT),
        press,
        &viewports,
    );
    let mut translator = NativePointerTranslator::default();
    assert!(matches!(
        translator.translate(&press, |_| None),
        NativePointerTranslation::Input(_)
    ));
    assert!(translator.references_binding(binding));
    assert!(!translator.can_coalesce_idle_cursor_moves());
    let old_identity = translator.next_device_identity;

    translator.finish_overload_cancellation();

    assert!(!translator.references_binding(binding));
    assert!(translator.can_coalesce_idle_cursor_moves());
    assert_eq!(translator.next_device_identity, old_identity);
    assert_eq!(
        translator.translate(&record(mouse(ElementState::Pressed)), |_| None),
        expected(
            old_identity + 1,
            NativePointerEvent::ButtonPressed(NativePointerButton::Primary)
        ),
    );
    assert!(!translator.references_binding(binding));
    assert_eq!(
        translator.translate(&record(mouse(ElementState::Released)), |_| None),
        expected(
            old_identity + 1,
            NativePointerEvent::ButtonReleased(NativePointerButton::Primary)
        ),
    );
    assert!(translator.can_coalesce_idle_cursor_moves());
}

#[test]
fn overload_allows_hover_and_moved_only_discrete_wheel_without_a_start() {
    let hover = record(WindowEvent::CursorMoved {
        device_id: DeviceId::dummy(),
        position: PhysicalPosition::new(1.0, 2.0),
        facts: PointerEventFacts::default(),
    });
    let discrete = record(wheel(TouchPhase::Moved));
    let delta = NativeScrollDelta::Lines { x: 0.0, y: 2.0 };
    let mut translator = NativePointerTranslator::default();
    assert_eq!(
        translator.translate(&hover, |_| None),
        expected(1, NativePointerEvent::Moved)
    );
    assert_eq!(
        translator.translate(&discrete, |_| None),
        scrolled(2, NativeScrollPhase::Discrete { delta })
    );

    translator.finish_overload_cancellation();

    assert_eq!(
        translator.translate(&hover, |_| None),
        expected(3, NativePointerEvent::Moved)
    );
    assert_eq!(
        translator.translate(&discrete, |_| None),
        scrolled(4, NativeScrollPhase::Discrete { delta })
    );
    assert_eq!(translator.next_scroll_sequence, 0);
    // A scroll lane not seen before overload also accepts a Moved-only wheel.
    assert_eq!(
        translator.translate_for_test_device(&discrete, 99, |_| None),
        scrolled(5, NativeScrollPhase::Discrete { delta }),
    );
}

#[test]
fn overload_forgets_touch_enrolment_and_reuses_contact_with_a_new_pointer() {
    let mut translator = NativePointerTranslator::default();
    let start = record(touch(TouchPhase::Started));
    assert_eq!(
        translator.translate(&start, |_| None),
        expected(
            1,
            NativePointerEvent::ButtonPressed(NativePointerButton::Primary)
        )
    );

    translator.finish_overload_cancellation();

    for phase in [TouchPhase::Moved, TouchPhase::Ended, TouchPhase::Cancelled] {
        assert_eq!(
            translator.translate(&record(touch(phase)), |_| None),
            NativePointerTranslation::Ignored
        );
    }
    assert_eq!(
        translator.translate(&start, |_| None),
        expected(
            2,
            NativePointerEvent::ButtonPressed(NativePointerButton::Primary)
        )
    );
    assert_eq!(
        translator.translate(&record(touch(TouchPhase::Moved)), |_| None),
        expected(2, NativePointerEvent::Moved)
    );
}

#[test]
fn overload_ignores_old_phaseful_scroll_tails_and_starts_a_new_sequence() {
    for (event, delta) in [
        (
            wheel as fn(TouchPhase) -> WindowEvent,
            NativeScrollDelta::Lines { x: 0.0, y: 2.0 },
        ),
        (
            pan as fn(TouchPhase) -> WindowEvent,
            NativeScrollDelta::PhysicalPixels { x: 0.0, y: 2.0 },
        ),
    ] {
        let mut translator = NativePointerTranslator::default();
        let start = record(event(TouchPhase::Started));
        assert_eq!(
            translator.translate(&start, |_| None),
            scrolled(
                1,
                NativeScrollPhase::Begin {
                    sequence: NativeScrollSequenceId::new(1),
                    delta: Some(delta)
                }
            )
        );

        translator.finish_overload_cancellation();
        assert_eq!(translator.next_device_identity, 1);
        assert_eq!(translator.next_scroll_sequence, 1);

        for phase in [
            TouchPhase::Moved,
            TouchPhase::Ended,
            TouchPhase::Cancelled,
            TouchPhase::Ended,
        ] {
            assert_eq!(
                translator.translate(&record(event(phase)), |_| None),
                NativePointerTranslation::Ignored
            );
        }
        assert_eq!(translator.next_scroll_sequence, 1);
        assert_eq!(
            translator.translate(&start, |_| None),
            scrolled(
                2,
                NativeScrollPhase::Begin {
                    sequence: NativeScrollSequenceId::new(2),
                    delta: Some(delta)
                }
            )
        );
        assert_eq!(
            translator.translate(&record(event(TouchPhase::Moved)), |_| None),
            scrolled(
                2,
                NativeScrollPhase::Update {
                    sequence: NativeScrollSequenceId::new(2),
                    delta
                }
            )
        );
    }
}

#[test]
fn overload_preserves_device_removal_gates_and_binding_retired_tail_policy() {
    let mut translator = NativePointerTranslator::default();
    let hover = record(WindowEvent::CursorMoved {
        device_id: DeviceId::dummy(),
        position: PhysicalPosition::new(1.0, 2.0),
        facts: PointerEventFacts::default(),
    });
    assert!(matches!(
        translator.translate(&hover, |_| None),
        NativePointerTranslation::Input(_)
    ));
    translator.forget_removed_devices();
    translator.finish_overload_cancellation();
    assert_eq!(
        translator.translate(&hover, |_| None),
        NativePointerTranslation::Ignored
    );
    assert_eq!(
        translator.translate(&record(mouse(ElementState::Pressed)), |_| None),
        expected(
            2,
            NativePointerEvent::ButtonPressed(NativePointerButton::Primary)
        )
    );

    let binding = binding();
    let start = NativeWindowEventRecord::for_test(
        1,
        WindowId::from(1),
        None,
        Some(binding),
        wheel(TouchPhase::Started),
    );
    assert!(matches!(
        translator.translate(&start, |_| None),
        NativePointerTranslation::Input(_)
    ));
    assert_eq!(translator.cancel_destroyed_binding(binding).len(), 1);
    assert!(translator.has_pending_provider_tail());
    let tail = NativeWindowEventRecord::for_test(
        2,
        WindowId::from(1),
        None,
        Some(binding),
        wheel(TouchPhase::Ended),
    );
    assert_eq!(
        translator.translate(&tail, |_| None),
        NativePointerTranslation::Input(NativePointerInput::new(
            NativePointerId::new(3),
            NativePointerEvent::Scrolled(NativeScrollEvent::new(
                NativeScrollDeviceId::new(3),
                NativeScrollPhase::End {
                    sequence: NativeScrollSequenceId::new(1),
                    delta: Some(NativeScrollDelta::Lines { x: 0.0, y: 2.0 })
                },
                NativeScrollMomentum::Unknown,
                NativeScrollModifiers::Unknown,
            )),
            NativeDesktopPointerLocation::new(
                NativeDesktopPosition::Unknown,
                NativePointerHover::Unknown,
                None
            ),
            NativePointerOwner::Native(binding),
            NativePointerOwner::Unknown,
        )),
    );
    assert!(!translator.has_pending_provider_tail());
}
