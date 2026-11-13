use super::*;
use winit::dpi::PhysicalPosition;
use winit::event::{PointerEventFacts, Touch};
use winit::keyboard::ModifiersState;
use winit::window::WindowId;

fn record(ordinal: u64, event: WindowEvent) -> NativeWindowEventRecord {
    NativeWindowEventRecord::for_test(ordinal, WindowId::from(11), None, None, event)
}

#[test]
fn configured_activation_uses_native_event_modifiers_and_platform_command_mapping() {
    use eframe::egui::Modifiers;
    use egui_dockspace::DockingActivation;

    let command = if cfg!(target_os = "macos") {
        ModifiersState::SUPER
    } else {
        ModifiersState::CONTROL
    };
    for (activation, observed, enabled) in [
        (DockingActivation::Always, ModifiersState::SHIFT, true),
        (
            DockingActivation::HoldToEnable(Modifiers::ALT),
            ModifiersState::empty(),
            false,
        ),
        (
            DockingActivation::HoldToEnable(Modifiers::ALT),
            ModifiersState::ALT | ModifiersState::SHIFT,
            true,
        ),
        (
            DockingActivation::HoldToDisable(Modifiers::COMMAND),
            command,
            false,
        ),
        (
            DockingActivation::HoldToEnable(Modifiers::COMMAND),
            command | ModifiersState::ALT,
            true,
        ),
    ] {
        let config = DockInputConfig {
            docking: activation,
            ..Default::default()
        };
        let record = record(
            1,
            WindowEvent::MouseInput {
                device_id: DeviceId::dummy(),
                primary_press: None,
                state: ElementState::Pressed,
                button: MouseButton::Left,
                facts: PointerEventFacts {
                    modifiers: Some(observed),
                    ..Default::default()
                },
            },
        );
        let mut translator = NativePointerTranslator::default();
        let NativePointerTranslation::Input(baseline) =
            translator.clone().translate(&record, |_| None)
        else {
            panic!("a real press translates");
        };
        let NativePointerTranslation::Input(actual) = translator.translate_captured_with_input(
            &record,
            Duration::from_millis(40),
            &config,
            |_| None,
        ) else {
            panic!("a configured real press translates");
        };
        assert_eq!(
            actual,
            baseline.with_metadata(
                PointerEventMetadata::new(PointerDeviceKind::Mouse, Duration::from_millis(40))
                    .with_docking_enabled(enabled)
            ),
            "{activation:?} with {observed:?}"
        );
    }
}

#[test]
fn mouse_press_move_and_release_keep_only_their_own_modifier_directive() {
    let captured_at = Duration::from_millis(40);
    for modifiers in [
        [Some(ModifiersState::SHIFT); 3],
        [Some(ModifiersState::SHIFT), None, None],
        [None, Some(ModifiersState::SHIFT), None],
        [None; 3],
        [Some(ModifiersState::ALT | ModifiersState::CONTROL); 3],
        [Some(ModifiersState::SHIFT | ModifiersState::SUPER); 3],
        [Some(ModifiersState::empty()); 3],
    ] {
        let mut translator = NativePointerTranslator::default();
        for (index, state) in [
            Some(ElementState::Pressed),
            None,
            Some(ElementState::Released),
        ]
        .into_iter()
        .enumerate()
        {
            let modifiers = modifiers[index];
            let position = PhysicalPosition::new(10.0 + index as f64, 20.0);
            let facts = PointerEventFacts {
                surface_position: Some(position),
                desktop_position: Some(position),
                modifiers,
                ..PointerEventFacts::default()
            };
            let event = match state {
                Some(state) => WindowEvent::MouseInput {
                    device_id: DeviceId::dummy(),
                    primary_press: None,
                    state,
                    button: MouseButton::Left,
                    facts,
                },
                None => WindowEvent::CursorMoved {
                    device_id: DeviceId::dummy(),
                    position,
                    facts,
                },
            };
            let record = record(index as u64 + 1, event.clone());
            let NativePointerTranslation::Input(untimed) =
                translator.clone().translate(&record, |_| None)
            else {
                panic!("a real mouse edge must translate");
            };
            let NativePointerTranslation::Input(input) =
                translator.translate_captured(&record, captured_at, |_| None)
            else {
                panic!("a real mouse edge must translate");
            };
            let mut expected = PointerEventMetadata::new(PointerDeviceKind::Mouse, captured_at);
            if let Some(modifiers) = modifiers {
                expected = expected.with_docking_enabled(!modifiers.shift_key());
            }
            assert_eq!(input, untimed.with_metadata(expected));
            assert_eq!(record.event(), &event);
        }
    }
}

#[test]
fn unknown_mouse_and_touch_start_use_the_configured_baseline() {
    let config = DockInputConfig {
        docking: egui_dockspace::DockingActivation::HoldToEnable(eframe::egui::Modifiers::ALT),
        ..Default::default()
    };
    for (event, device) in [
        (
            WindowEvent::MouseInput {
                device_id: DeviceId::dummy(),
                primary_press: None,
                state: ElementState::Pressed,
                button: MouseButton::Left,
                facts: PointerEventFacts::default(),
            },
            PointerDeviceKind::Mouse,
        ),
        (
            WindowEvent::Touch(Touch {
                device_id: DeviceId::dummy(),
                phase: TouchPhase::Started,
                location: PhysicalPosition::new(10.0, 20.0),
                facts: PointerEventFacts::default(),
                force: None,
                id: 7,
            }),
            PointerDeviceKind::Touch,
        ),
    ] {
        let record = record(1, event.clone());
        let mut translator = NativePointerTranslator::default();
        let NativePointerTranslation::Input(baseline) =
            translator.clone().translate(&record, |_| None)
        else {
            panic!("the initial pointer edge translates");
        };
        let captured_at = Duration::from_millis(40);
        assert_eq!(
            translator.translate_captured_with_input(&record, captured_at, &config, |_| None),
            NativePointerTranslation::Input(baseline.with_metadata(
                PointerEventMetadata::new(device, captured_at).with_docking_enabled(false)
            ))
        );
        assert_eq!(record.event(), &event);
    }
}

#[test]
fn a_new_mouse_press_reseeds_activation_after_a_missing_release() {
    let config = DockInputConfig {
        docking: egui_dockspace::DockingActivation::HoldToEnable(eframe::egui::Modifiers::ALT),
        ..Default::default()
    };
    let mut translator = NativePointerTranslator::default();
    let captured_at = Duration::from_millis(40);
    for (index, (state, modifiers, directive)) in [
        (Some(ElementState::Pressed), None, Some(false)),
        (None, Some(ModifiersState::ALT), Some(true)),
        (Some(ElementState::Pressed), None, Some(false)),
        (None, None, None),
        (Some(ElementState::Released), None, None),
    ]
    .into_iter()
    .enumerate()
    {
        let position = PhysicalPosition::new(10.0 + index as f64, 20.0);
        let facts = PointerEventFacts {
            surface_position: Some(position),
            desktop_position: Some(position),
            modifiers,
            ..Default::default()
        };
        let event = match state {
            Some(state) => WindowEvent::MouseInput {
                device_id: DeviceId::dummy(),
                primary_press: None,
                state,
                button: MouseButton::Left,
                facts,
            },
            None => WindowEvent::CursorMoved {
                device_id: DeviceId::dummy(),
                position,
                facts,
            },
        };
        let record = record(index as u64 + 1, event);
        let baseline = translator.clone().translate(&record, |_| None);
        let mut metadata = PointerEventMetadata::new(PointerDeviceKind::Mouse, captured_at);
        if let Some(enabled) = directive {
            metadata = metadata.with_docking_enabled(enabled);
        }
        let expected = match baseline {
            NativePointerTranslation::Input(input) => {
                NativePointerTranslation::Input(input.with_metadata(metadata))
            }
            NativePointerTranslation::Restart { cancelled, pressed } => {
                assert_eq!(index, 2, "only the replacement down restarts the stream");
                NativePointerTranslation::Restart {
                    cancelled: cancelled.with_metadata(PointerEventMetadata::new(
                        PointerDeviceKind::Mouse,
                        captured_at,
                    )),
                    pressed: pressed.with_metadata(metadata),
                }
            }
            other => panic!("the real mouse edge translates: {other:?}"),
        };
        assert_eq!(
            translator.translate_captured_with_input(&record, captured_at, &config, |_| None),
            expected,
            "edge {index} with {state:?}"
        );
    }
}

#[test]
fn replacement_mouse_press_uses_current_configuration_instead_of_the_old_gesture_policy() {
    let mut translator = NativePointerTranslator::default();
    let captured_at = Duration::from_millis(40);
    let down = |ordinal| {
        record(
            ordinal,
            WindowEvent::MouseInput {
                device_id: DeviceId::dummy(),
                primary_press: None,
                state: ElementState::Pressed,
                button: MouseButton::Left,
                facts: PointerEventFacts::default(),
            },
        )
    };
    assert!(matches!(
        translator.translate_captured(&down(1), captured_at, |_| None),
        NativePointerTranslation::Input(_)
    ));
    let config = DockInputConfig {
        docking: egui_dockspace::DockingActivation::HoldToEnable(eframe::egui::Modifiers::ALT),
        ..Default::default()
    };
    let NativePointerTranslation::Restart {
        cancelled: raw_cancelled,
        pressed: raw_pressed,
    } = translator.clone().translate(&down(2), |_| None)
    else {
        panic!("a second tokenless down must replace the old stream");
    };
    let NativePointerTranslation::Restart { cancelled, pressed } =
        translator.translate_captured_with_input(&down(2), captured_at, &config, |_| None)
    else {
        panic!("the tokenless down must replace the lost-release stream");
    };
    assert_eq!(
        cancelled,
        raw_cancelled.with_metadata(PointerEventMetadata::new(
            PointerDeviceKind::Mouse,
            captured_at,
        )),
    );
    assert_eq!(
        pressed,
        raw_pressed.with_metadata(
            PointerEventMetadata::new(PointerDeviceKind::Mouse, captured_at)
                .with_docking_enabled(false),
        ),
    );
}

#[test]
fn replacement_mouse_press_suppresses_held_secondary_button_release_tails() {
    let mut translator = NativePointerTranslator::default();
    let event = |ordinal, state, button| {
        record(
            ordinal,
            WindowEvent::MouseInput {
                device_id: DeviceId::dummy(),
                primary_press: None,
                state,
                button,
                facts: PointerEventFacts::default(),
            },
        )
    };
    for (ordinal, button) in [(1, MouseButton::Left), (2, MouseButton::Right)] {
        assert!(matches!(
            translator.translate(&event(ordinal, ElementState::Pressed, button), |_| None),
            NativePointerTranslation::Input(_)
        ));
    }
    assert!(matches!(
        translator.translate(&event(3, ElementState::Pressed, MouseButton::Left), |_| {
            None
        }),
        NativePointerTranslation::Restart { .. }
    ));
    assert_eq!(
        translator.translate(
            &event(4, ElementState::Released, MouseButton::Right),
            |_| None
        ),
        NativePointerTranslation::Ignored,
    );
    assert!(matches!(
        translator.translate(&event(5, ElementState::Released, MouseButton::Left), |_| {
            None
        }),
        NativePointerTranslation::Input(_)
    ));
    assert!(translator.can_coalesce_idle_cursor_moves());
}

#[test]
fn modifier_control_never_enrolls_or_mutates_a_pointer_device() {
    let mut translator = NativePointerTranslator::default();
    for device in 1..=4 {
        translator
            .device_mut(NativeInputDeviceKey::Mouse(NativePhysicalDeviceKey::Test(
                device,
            )))
            .unwrap();
    }
    let before = format!("{translator:?}");
    let control = record(
        5,
        WindowEvent::ModifiersChanged(ModifiersState::SHIFT.into()),
    );
    assert_eq!(
        translator.translate_captured(&control, Duration::from_millis(40), |_| {
            panic!("a modifier control cannot resolve pointer coordinates")
        }),
        NativePointerTranslation::NotPointer
    );
    assert_eq!(format!("{translator:?}"), before);
}

#[test]
fn touch_edges_preserve_event_time_modifiers_and_missing_facts() {
    let mut translator = NativePointerTranslator::default();
    for (index, (phase, modifiers, expected)) in [
        (
            TouchPhase::Started,
            Some(ModifiersState::SHIFT),
            Some(false),
        ),
        (TouchPhase::Moved, None, None),
        (TouchPhase::Ended, Some(ModifiersState::empty()), Some(true)),
    ]
    .into_iter()
    .enumerate()
    {
        let record = record(
            index as u64 + 1,
            WindowEvent::Touch(Touch {
                device_id: DeviceId::dummy(),
                phase,
                location: PhysicalPosition::new(10.0, 20.0),
                facts: PointerEventFacts {
                    modifiers,
                    ..PointerEventFacts::default()
                },
                force: None,
                id: 7,
            }),
        );
        let NativePointerTranslation::Input(untimed) =
            translator.clone().translate(&record, |_| None)
        else {
            panic!("a real touch edge must translate");
        };
        let NativePointerTranslation::Input(input) =
            translator.translate_captured(&record, Duration::from_millis(40), |_| None)
        else {
            panic!("a real touch edge must translate");
        };
        let mut metadata =
            PointerEventMetadata::new(PointerDeviceKind::Touch, Duration::from_millis(40));
        if let Some(enabled) = expected {
            metadata = metadata.with_docking_enabled(enabled);
        }
        assert_eq!(input, untimed.with_metadata(metadata));
    }
}
