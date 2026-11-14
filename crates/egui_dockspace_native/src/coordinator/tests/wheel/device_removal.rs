//! Anonymous device resets must not require phases that ordinary wheels never send.

use super::*;

mod overflow;

#[test]
fn device_removal_reopens_an_idle_discrete_wheel_with_a_fresh_identity() {
    for click_before_scroll in [false, true] {
        let mut native = coordinator();
        let (binding, _) = register_roots(&mut native);
        let window = WindowId::from(91);
        let wheel = phaseful_scroll_record(1, window, binding, TouchPhase::Moved, -1.0);
        let mut translator = NativePointerTranslator::default();
        let discrete = NativeScrollPhase::Discrete {
            delta: NativeScrollDelta::Lines { x: 0.0, y: -1.0 },
        };
        assert_eq!(
            translator.translate(&wheel, |_| None),
            NativePointerTranslation::Input(phaseful_scroll_input(Some(binding), discrete)),
        );
        translator.forget_removed_devices();
        translator.forget_removed_devices();
        assert!(translator.can_coalesce_idle_cursor_moves());
        if click_before_scroll {
            assert!(matches!(
                translator.translate(&mouse_press_record(2, window, binding), |_| None),
                NativePointerTranslation::Input(_)
            ));
        }
        let identity = if click_before_scroll { 3 } else { 2 };
        assert_eq!(
            translator.translate(&wheel, |_| None),
            NativePointerTranslation::Input(phaseful_scroll_input_for_device(
                NativePointerId::new(identity),
                NativeScrollDeviceId::new(identity),
                Some(binding),
                discrete,
            )),
            "a wheel-only Moved stream must resume after removal; click={click_before_scroll}",
        );
    }
}

#[test]
fn device_removal_reopens_discrete_wheel_after_a_completed_phaseful_sequence() {
    for terminal in [TouchPhase::Ended, TouchPhase::Cancelled] {
        let mut native = coordinator();
        let (binding, _) = register_roots(&mut native);
        let window = WindowId::from(92);
        let mut translator = NativePointerTranslator::default();
        for (ordinal, phase) in [(1, TouchPhase::Started), (2, terminal)] {
            assert!(matches!(
                translator.translate(
                    &phaseful_scroll_record(ordinal, window, binding, phase, 0.0),
                    |_| None,
                ),
                NativePointerTranslation::Input(_)
            ));
        }
        assert!(translator.can_coalesce_idle_cursor_moves());
        translator.forget_removed_devices();
        assert_eq!(
            translator.translate(
                &phaseful_scroll_record(3, window, binding, TouchPhase::Moved, -1.0),
                |_| None,
            ),
            NativePointerTranslation::Input(phaseful_scroll_input_for_device(
                NativePointerId::new(2),
                NativeScrollDeviceId::new(2),
                Some(binding),
                NativeScrollPhase::Discrete {
                    delta: NativeScrollDelta::Lines { x: 0.0, y: -1.0 },
                },
            )),
            "completed {terminal:?} has no phaseful tail to quarantine",
        );
    }
}
