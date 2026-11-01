use super::*;
use winit::dpi::PhysicalPosition;
use winit::event::{PointerEventFacts, Touch};
use winit::keyboard::ModifiersState;
use winit::window::WindowId;

fn record(ordinal: u64, event: WindowEvent) -> NativeWindowEventRecord {
    NativeWindowEventRecord::for_test(ordinal, WindowId::from(11), None, None, event)
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
                expected = expected.with_magnetic_targeting(!modifiers.shift_key());
            }
            assert_eq!(input, untimed.with_metadata(expected));
            assert_eq!(record.event(), &event);
        }
    }
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
            metadata = metadata.with_magnetic_targeting(enabled);
        }
        assert_eq!(input, untimed.with_metadata(metadata));
    }
}
