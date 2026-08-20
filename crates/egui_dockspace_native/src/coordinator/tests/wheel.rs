use dockspace::geometry::PhysicalPoint;
use dockspace::runtime::{
    NativeDesktopPointerLocation, NativeDesktopPosition, NativePointerCancelReason,
    NativePointerEvent, NativePointerHover, NativePointerId, NativePointerInput,
    NativePointerOwner, NativeReceiverAnswer, NativeScrollCancelReason, NativeScrollDelta,
    NativeScrollDeviceId, NativeScrollEvent, NativeScrollModifiers, NativeScrollMomentum,
    NativeScrollPhase, NativeScrollSequenceId, NativeSurfaceBinding, SurfaceUnavailableReason,
};
use eframe::egui::ViewportId;
use winit::dpi::PhysicalPosition;
use winit::event::{
    DeviceId, MouseScrollDelta, PointerEventFacts, PointerWindowRoute, TouchPhase, WindowEvent,
};
use winit::keyboard::ModifiersState;
use winit::window::WindowId;

use super::{coordinator, register_roots};
use crate::close_control::NativeWindowClosePolicy;
use crate::event::NativeWindowEventRecord;
use crate::mailbox::HostRecord;
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

#[test]
fn device_removal_cancels_live_scroll_before_terminating_its_pointer() {
    let mut native = coordinator();
    let (binding, _) = register_roots(&mut native);
    let window = WindowId::from(45);
    let started = phaseful_scroll_record(1, window, binding, TouchPhase::Started, 1.0);
    let mut translator = NativePointerTranslator::default();

    assert!(matches!(
        translator.translate_for_test_device(&started, 303, |_| None),
        NativePointerTranslation::Input(_)
    ));
    assert_eq!(
        translator.remove_all_devices(),
        vec![
            phaseful_scroll_input_for_device(
                NativePointerId::new(1),
                NativeScrollDeviceId::new(1),
                None,
                NativeScrollPhase::Cancel {
                    sequence: NativeScrollSequenceId::new(1),
                    reason: NativeScrollCancelReason::DeviceRemoved,
                },
            ),
            NativePointerInput::new(
                NativePointerId::new(1),
                NativePointerEvent::StreamCancelled(NativePointerCancelReason::DeviceRemoved),
                NativeDesktopPointerLocation::new(
                    NativeDesktopPosition::Unknown,
                    NativePointerHover::Unknown,
                    None,
                ),
                NativePointerOwner::Unknown,
                NativePointerOwner::Unknown,
            ),
        ],
        "the scroll sequence closes before the shared pointer stream terminates"
    );
}

#[test]
fn removed_device_ignores_wheel_moved_and_ended_until_a_new_phase_begins() {
    let mut native = coordinator();
    let (binding, _) = register_roots(&mut native);
    let window = WindowId::from(46);
    let started = phaseful_scroll_record(1, window, binding, TouchPhase::Started, 1.0);
    let moved = phaseful_scroll_record(2, window, binding, TouchPhase::Moved, 2.0);
    let ended = phaseful_scroll_record(3, window, binding, TouchPhase::Ended, 0.0);
    let mut translator = NativePointerTranslator::default();

    assert!(matches!(
        translator.translate_for_test_device(&started, 304, |_| None),
        NativePointerTranslation::Input(_)
    ));
    assert_eq!(translator.remove_all_devices().len(), 2);
    assert_eq!(
        translator.translate_for_test_device(&moved, 304, |_| None),
        NativePointerTranslation::Ignored,
        "a moved tail cannot create a discrete sequence after removal"
    );
    assert_eq!(
        translator.translate_for_test_device(&ended, 304, |_| None),
        NativePointerTranslation::Ignored
    );
    assert_eq!(
        translator.translate_for_test_device(&started, 304, |_| None),
        NativePointerTranslation::Input(phaseful_scroll_input_for_device(
            NativePointerId::new(2),
            NativeScrollDeviceId::new(2),
            Some(binding),
            NativeScrollPhase::Begin {
                sequence: NativeScrollSequenceId::new(2),
                delta: Some(NativeScrollDelta::Lines { x: 0.0, y: 1.0 }),
            },
        ))
    );
}

#[test]
fn destroyed_delivery_binding_resets_before_a_successor_sequence() {
    let mut native = coordinator();
    let (first, second) = register_roots(&mut native);
    let first_window = WindowId::from(51);
    let second_window = WindowId::from(52);
    let first_record = phaseful_scroll_record(3, first_window, first, TouchPhase::Started, 1.0);
    let second_record = phaseful_scroll_record(4, second_window, second, TouchPhase::Started, 1.0);

    let mut translator = NativePointerTranslator::default();
    let expected_first = phaseful_scroll_input(
        Some(first),
        NativeScrollPhase::Begin {
            sequence: NativeScrollSequenceId::new(1),
            delta: Some(NativeScrollDelta::Lines { x: 0.0, y: 1.0 }),
        },
    );
    assert_eq!(
        translator.translate(&first_record, |_| None),
        NativePointerTranslation::Input(expected_first)
    );
    assert!(translator.references_binding(first));
    assert!(!translator.references_binding(second));

    let expected_retired = phaseful_scroll_input(
        None,
        NativeScrollPhase::Cancel {
            sequence: NativeScrollSequenceId::new(1),
            reason: NativeScrollCancelReason::BindingRetired,
        },
    );
    assert_eq!(
        translator.cancel_destroyed_binding(first),
        vec![expected_retired]
    );
    assert!(translator.references_binding(first));
    assert!(translator.has_pending_provider_tail());
    assert!(translator.cancel_destroyed_binding(first).is_empty());

    let expected_reset = phaseful_scroll_input(
        None,
        NativeScrollPhase::Cancel {
            sequence: NativeScrollSequenceId::new(1),
            reason: NativeScrollCancelReason::ProviderReset,
        },
    );
    assert_eq!(
        translator.reset_destroyed_binding(first),
        vec![expected_reset]
    );
    assert!(!translator.has_pending_provider_tail());
    assert!(!translator.references_binding(first));

    let expected_second = phaseful_scroll_input(
        Some(second),
        NativeScrollPhase::Begin {
            sequence: NativeScrollSequenceId::new(2),
            delta: Some(NativeScrollDelta::Lines { x: 0.0, y: 1.0 }),
        },
    );
    assert_eq!(
        translator.translate(&second_record, |_| None),
        NativePointerTranslation::Input(expected_second)
    );

    let late_terminal = phaseful_scroll_record(5, first_window, first, TouchPhase::Ended, 0.0);
    assert_eq!(
        translator.translate(&late_terminal, |_| None),
        NativePointerTranslation::Ignored,
        "a predecessor terminal cannot terminate the successor sequence"
    );
    let successor_update = phaseful_scroll_record(6, second_window, second, TouchPhase::Moved, 2.0);
    let expected_update = phaseful_scroll_input(
        Some(second),
        NativeScrollPhase::Update {
            sequence: NativeScrollSequenceId::new(2),
            delta: NativeScrollDelta::Lines { x: 0.0, y: 2.0 },
        },
    );
    assert_eq!(
        translator.translate(&successor_update, |_| None),
        NativePointerTranslation::Input(expected_update)
    );
}

#[test]
fn destroying_one_binding_preserves_another_devices_live_scroll_sequence() {
    let mut native = coordinator();
    let (first, second) = register_roots(&mut native);
    let first_window = WindowId::from(53);
    let second_window = WindowId::from(54);
    let first_begin = phaseful_scroll_record(1, first_window, first, TouchPhase::Started, 1.0);
    let second_begin = phaseful_scroll_record(2, second_window, second, TouchPhase::Started, 2.0);
    let mut translator = NativePointerTranslator::default();

    assert_eq!(
        translator.translate_for_test_device(&first_begin, 101, |_| None),
        NativePointerTranslation::Input(phaseful_scroll_input_for_device(
            NativePointerId::new(1),
            NativeScrollDeviceId::new(1),
            Some(first),
            NativeScrollPhase::Begin {
                sequence: NativeScrollSequenceId::new(1),
                delta: Some(NativeScrollDelta::Lines { x: 0.0, y: 1.0 }),
            },
        ))
    );
    assert_eq!(
        translator.translate_for_test_device(&second_begin, 202, |_| None),
        NativePointerTranslation::Input(phaseful_scroll_input_for_device(
            NativePointerId::new(2),
            NativeScrollDeviceId::new(2),
            Some(second),
            NativeScrollPhase::Begin {
                sequence: NativeScrollSequenceId::new(2),
                delta: Some(NativeScrollDelta::Lines { x: 0.0, y: 2.0 }),
            },
        ))
    );

    assert_eq!(
        translator.cancel_destroyed_binding(first),
        vec![phaseful_scroll_input_for_device(
            NativePointerId::new(1),
            NativeScrollDeviceId::new(1),
            None,
            NativeScrollPhase::Cancel {
                sequence: NativeScrollSequenceId::new(1),
                reason: NativeScrollCancelReason::BindingRetired,
            },
        )]
    );
    assert_eq!(
        translator.reset_destroyed_binding(first),
        vec![phaseful_scroll_input_for_device(
            NativePointerId::new(1),
            NativeScrollDeviceId::new(1),
            None,
            NativeScrollPhase::Cancel {
                sequence: NativeScrollSequenceId::new(1),
                reason: NativeScrollCancelReason::ProviderReset,
            },
        )]
    );
    assert!(!translator.references_binding(first));
    assert!(translator.references_binding(second));

    let second_update = phaseful_scroll_record(3, second_window, second, TouchPhase::Moved, 3.0);
    assert_eq!(
        translator.translate_for_test_device(&second_update, 202, |_| None),
        NativePointerTranslation::Input(phaseful_scroll_input_for_device(
            NativePointerId::new(2),
            NativeScrollDeviceId::new(2),
            Some(second),
            NativeScrollPhase::Update {
                sequence: NativeScrollSequenceId::new(2),
                delta: NativeScrollDelta::Lines { x: 0.0, y: 3.0 },
            },
        )),
        "destroying device A's binding must not reset device B's sequence"
    );
}

#[test]
fn destroyed_binding_retires_mouse_and_scroll_before_reuse() {
    let mut native = coordinator();
    let (binding, _) = register_roots(&mut native);
    let window = WindowId::from(61);
    let press = NativeWindowEventRecord::for_test(
        1,
        window,
        Some(ViewportId::ROOT),
        Some(binding),
        WindowEvent::MouseInput {
            device_id: DeviceId::dummy(),
            state: winit::event::ElementState::Pressed,
            button: winit::event::MouseButton::Left,
            facts: PointerEventFacts::default(),
        },
    );
    let scroll = phaseful_scroll_record(2, window, binding, TouchPhase::Started, 1.0);
    let mut translator = NativePointerTranslator::default();

    assert!(matches!(
        translator.translate(&press, |_| None),
        NativePointerTranslation::Input(_)
    ));
    assert!(matches!(
        translator.translate(&scroll, |_| None),
        NativePointerTranslation::Input(_)
    ));

    assert_eq!(
        translator.cancel_destroyed_binding(binding),
        vec![NativePointerInput::new(
            NativePointerId::new(1),
            NativePointerEvent::StreamCancelled(NativePointerCancelReason::BindingRetired),
            NativeDesktopPointerLocation::new(
                NativeDesktopPosition::Unknown,
                NativePointerHover::Unknown,
                None,
            ),
            NativePointerOwner::Unknown,
            NativePointerOwner::Unknown,
        )]
    );
    assert!(
        translator.cancel_destroyed_binding(binding).is_empty(),
        "the pointer cancellation already terminates its live scroll sequence"
    );
    assert!(
        translator.reset_destroyed_binding(binding).is_empty(),
        "the following boundary clears correlation without a duplicate scroll terminal"
    );
    assert!(!translator.has_pending_provider_tail());
    assert!(!translator.references_binding(binding));
}

#[test]
fn destroyed_scroll_reset_survives_a_later_close_correlation_failure() {
    let mut native = coordinator();
    let (binding, survivor) = register_roots(&mut native);
    let viewport = ViewportId::ROOT;
    let window = WindowId::from(81);
    super::close::observe_close(
        &mut native,
        binding,
        [binding, survivor],
        viewport,
        window,
        20,
    );
    assert!(
        native
            .drive_close_policy(NativeWindowClosePolicy::RetainLayout, binding.surface())
            .expect("the close request is accepted")
    );

    native
        .bridge
        .push_record(HostRecord::WindowEvent(phaseful_scroll_record(
            21,
            window,
            binding,
            TouchPhase::Started,
            1.0,
        )));
    assert!(
        native
            .reduce_callback_head()
            .expect("the scroll begin records")
    );
    commit_unpainted_frame(&mut native);

    let wrong_window = WindowId::from(82);
    native
        .bridge
        .push_record(HostRecord::WindowEvent(NativeWindowEventRecord::for_test(
            22,
            wrong_window,
            Some(viewport),
            Some(binding),
            WindowEvent::Destroyed,
        )));
    assert!(
        native
            .reduce_callback_head()
            .expect("the binding-retired boundary records first")
    );
    assert!(native.pointer_translator.has_pending_provider_tail());
    commit_unpainted_frame(&mut native);

    native
        .reduce_callback_head()
        .expect_err("the unrelated window cannot settle the accepted close");
    assert!(
        !native.pointer_translator.has_pending_provider_tail(),
        "the accepted provider reset publishes before the later lifecycle failure"
    );

    let matching_destroyed = NativeWindowEventRecord::for_test(
        22,
        window,
        Some(viewport),
        Some(binding),
        WindowEvent::Destroyed,
    );
    assert!(
        native
            .close_control
            .settle_accepted_destroyed(&matching_destroyed, binding),
        "the test clears the independent close correlation before retrying the raw event"
    );
    assert!(
        native
            .reduce_callback_head()
            .expect("the same raw destroyed callback retries without another cancel")
    );
    commit_unpainted_frame(&mut native);
}

fn phaseful_scroll_record(
    ordinal: u64,
    window: WindowId,
    binding: NativeSurfaceBinding,
    phase: TouchPhase,
    y: f32,
) -> NativeWindowEventRecord {
    NativeWindowEventRecord::for_test(
        ordinal,
        window,
        None,
        Some(binding),
        WindowEvent::MouseWheel {
            device_id: DeviceId::dummy(),
            delta: MouseScrollDelta::LineDelta(0.0, y),
            phase,
            facts: PointerEventFacts::default(),
        },
    )
}

fn phaseful_scroll_input(
    delivery: Option<NativeSurfaceBinding>,
    phase: NativeScrollPhase,
) -> NativePointerInput {
    phaseful_scroll_input_for_device(
        NativePointerId::new(1),
        NativeScrollDeviceId::new(1),
        delivery,
        phase,
    )
}

fn phaseful_scroll_input_for_device(
    pointer: NativePointerId,
    device: NativeScrollDeviceId,
    delivery: Option<NativeSurfaceBinding>,
    phase: NativeScrollPhase,
) -> NativePointerInput {
    NativePointerInput::new(
        pointer,
        NativePointerEvent::Scrolled(NativeScrollEvent::new(
            device,
            phase,
            NativeScrollMomentum::Unknown,
            NativeScrollModifiers::Unknown,
        )),
        NativeDesktopPointerLocation::new(
            NativeDesktopPosition::Unknown,
            NativePointerHover::Unknown,
            None,
        ),
        delivery.map_or(NativePointerOwner::Unknown, NativePointerOwner::Native),
        NativePointerOwner::Unknown,
    )
}

fn commit_unpainted_frame(native: &mut super::NativeCoordinator) {
    let mut frame = native
        .begin_host_frame(|_| NativeReceiverAnswer::Unknown)
        .expect("the pending callback frame begins");
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .expect("the pending callback frame settles every surface");
    frame.commit().expect("the pending callback frame commits");
}
