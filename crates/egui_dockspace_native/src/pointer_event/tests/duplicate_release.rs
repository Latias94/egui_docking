//! Completed buttons retain their own tail gate without inventing press state.

use super::*;
use dockspace::model::{
    DockspaceLayout, DockspaceNode, DockspaceRootLayout, DockspaceSurfaceLayout, ItemId, RootId,
    SurfaceId,
};
use dockspace::policy::DockPolicy;
use dockspace::runtime::{
    DockspaceSession, HostInputOutcome, HostWindowToken, NativePointerRoster, NativePointerState,
    NativeSurfaceBinding, SurfaceUnavailableReason,
};
use winit::event::{PointerEventFacts, PointerWindowRoute};
use winit::window::WindowId;

fn record(
    ordinal: u64,
    binding: Option<NativeSurfaceBinding>,
    button: MouseButton,
    state: ElementState,
) -> NativeWindowEventRecord {
    NativeWindowEventRecord::for_test(
        ordinal,
        WindowId::from(11),
        None,
        binding,
        WindowEvent::MouseInput {
            device_id: DeviceId::dummy(),
            primary_press: None,
            state,
            button,
            facts: PointerEventFacts::default(),
        },
    )
}

fn expected(event: NativePointerEvent) -> NativePointerTranslation {
    NativePointerTranslation::Input(NativePointerInput::new(
        NativePointerId::new(1),
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

fn commit(session: &mut DockspaceSession) {
    let mut frame = session.begin_host_frame().unwrap();
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    frame.commit().unwrap();
}

fn session(roster: NativePointerRoster) -> (DockspaceSession, NativeSurfaceBinding) {
    let surface = SurfaceId::new(1);
    let layout = DockspaceLayout::new([DockspaceSurfaceLayout::new(
        surface,
        DockspaceRootLayout::new(
            RootId::new(1),
            DockspaceNode::central_tabs([ItemId::new(1)]),
        ),
    )])
    .unwrap();
    let mut session = DockspaceSession::from_layout(layout, DockPolicy::default()).unwrap();
    session.enable_managed_native_host(roster).unwrap();
    session
        .register_native_root(surface, HostWindowToken::new(11))
        .unwrap();
    commit(&mut session);
    let binding = session
        .native_surface_bindings_for_window(session.native_window_bindings().next().unwrap())
        .next()
        .unwrap();
    (session, binding)
}

#[test]
fn completed_mouse_buttons_ignore_every_late_release_and_accept_a_fresh_press() {
    for (button, native_button) in [
        (MouseButton::Left, NativePointerButton::Primary),
        (MouseButton::Right, NativePointerButton::Secondary),
        (MouseButton::Other(7), NativePointerButton::Other(7)),
    ] {
        let mut translator = NativePointerTranslator::default();
        for cycle in 0..2 {
            assert_eq!(
                translator.translate(&record(1, None, button, ElementState::Pressed), |_| None),
                expected(NativePointerEvent::ButtonPressed(native_button)),
                "cycle {cycle}: a completed press must not become a restart"
            );
            assert!(!translator.can_coalesce_idle_cursor_moves());
            assert_eq!(
                translator.translate(&record(2, None, button, ElementState::Released), |_| None),
                expected(NativePointerEvent::ButtonReleased(native_button))
            );
            let retained = translator.owned_data_capacity_bytes();
            for ordinal in 3..=5 {
                assert_eq!(
                    translator.translate(
                        &record(ordinal, None, button, ElementState::Released),
                        |_| None
                    ),
                    NativePointerTranslation::Ignored
                );
                assert_eq!(translator.owned_data_capacity_bytes(), retained);
                assert!(translator.can_coalesce_idle_cursor_moves());
            }
        }
    }
}

#[test]
fn the_first_release_of_a_button_held_at_host_enrollment_still_reaches_core() {
    let (mut session, binding) =
        session(NativePointerRoster::Exact(vec![NativePointerState::new(
            NativePointerId::new(1),
            [NativePointerButton::Primary],
            NativePointerOwner::None,
        )]));
    let mut translator = NativePointerTranslator::default();
    let release = record(1, Some(binding), MouseButton::Left, ElementState::Released);
    let NativePointerTranslation::Input(input) = translator.translate(&release, |_| None) else {
        panic!("a release with no local press may settle the exact enrollment roster");
    };
    session.record_native_pointer(input).unwrap();
    commit(&mut session);
    for _ in 0..2 {
        assert_eq!(
            translator.translate(&release, |_| None),
            NativePointerTranslation::Ignored
        );
    }
    for state in [ElementState::Pressed, ElementState::Released] {
        let NativePointerTranslation::Input(input) = translator
            .translate(&record(2, Some(binding), MouseButton::Left, state), |_| {
                None
            })
        else {
            panic!("a new press and its release must remain ordinary input");
        };
        session.record_native_pointer(input).unwrap();
        commit(&mut session);
    }
    assert!(translator.can_coalesce_idle_cursor_moves());
    assert!(!session.has_pending_native_input());
}

#[test]
fn retired_button_tails_cannot_reacquire_capture_or_revive_a_second_held_button() {
    let (_, binding) = session(NativePointerRoster::Exact(Vec::new()));
    let mut translator = NativePointerTranslator::default();
    let mut routes = crate::viewport_map::NativeViewportMap::default();
    routes
        .bind(eframe::egui::ViewportId::ROOT, WindowId::from(11), binding)
        .unwrap();
    for button in [MouseButton::Left, MouseButton::Right] {
        assert!(matches!(
            translator.translate(
                &record(1, Some(binding), button, ElementState::Pressed),
                |_| None
            ),
            NativePointerTranslation::Input(_)
        ));
    }
    translator.retire_binding(binding.window());
    let retained = translator.owned_data_capacity_bytes();
    for _ in 0..3 {
        for button in [MouseButton::Left, MouseButton::Right] {
            let mut event = record(2, Some(binding), button, ElementState::Released)
                .event()
                .clone();
            let WindowEvent::MouseInput { facts, .. } = &mut event else {
                unreachable!()
            };
            // The ignored callback's capture facts must not become new ownership.
            facts.capture = PointerWindowRoute::Window(WindowId::from(11));
            let late = NativeWindowEventRecord::for_test_snapshot(
                2,
                WindowId::from(11),
                Some(eframe::egui::ViewportId::ROOT),
                event,
                &routes,
            );
            assert_eq!(
                translator.translate(&late, |_| None),
                NativePointerTranslation::Ignored
            );
        }
        assert!(translator.can_coalesce_idle_cursor_moves());
        assert!(!translator.references_binding(binding.window()));
        assert_eq!(translator.owned_data_capacity_bytes(), retained);
    }
    assert!(matches!(
        translator.translate(
            &record(2, Some(binding), MouseButton::Left, ElementState::Pressed),
            |_| None
        ),
        NativePointerTranslation::Input(_)
    ));
    assert!(!translator.can_coalesce_idle_cursor_moves());
    assert!(matches!(
        translator.translate(
            &record(3, Some(binding), MouseButton::Left, ElementState::Released),
            |_| None
        ),
        NativePointerTranslation::Input(_)
    ));
    assert!(translator.can_coalesce_idle_cursor_moves());
}

#[test]
fn overload_reset_late_release_settles_unknown_core_authority_once_then_accepts_a_fresh_press() {
    let (mut session, binding) = session(NativePointerRoster::Exact(Vec::new()));
    let mut translator = NativePointerTranslator::default();
    let NativePointerTranslation::Input(pressed) = translator.translate(
        &record(1, Some(binding), MouseButton::Left, ElementState::Pressed),
        |_| None,
    ) else {
        panic!("a real press must reach core before reset");
    };
    session.record_native_pointer(pressed).unwrap();
    commit(&mut session);
    assert!(
        translator
            .mouse_press_for_pointer(NativePointerId::new(1))
            .is_some()
    );

    let mut frame = session.begin_host_frame().unwrap();
    let cancelled = frame.cancel_native_input_for_overload().unwrap();
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    let report = frame.commit().unwrap();
    assert_eq!(
        report.submitted_action_outcome(cancelled),
        Some(&HostInputOutcome::NativeInputCancelledForOverload)
    );
    translator.finish_overload_cancellation();
    assert!(
        translator
            .mouse_press_for_pointer(NativePointerId::new(1))
            .is_none()
    );

    // The reset leaves the core roster unknown. Its first late release remains
    // valid input, just like a release from an initially held host button.
    let release = record(2, Some(binding), MouseButton::Left, ElementState::Released);
    let NativePointerTranslation::Input(input) = translator.translate(&release, |_| None) else {
        panic!("the first release after an unknown reset must still reach core");
    };
    session.record_native_pointer(input).unwrap();
    commit(&mut session);
    assert_eq!(translator.next_device_identity, 2);
    let recorded = session.native_input_recorded_count();
    for _ in 0..3 {
        assert_eq!(
            translator.translate(&release, |_| None),
            NativePointerTranslation::Ignored
        );
    }
    assert_eq!(session.native_input_recorded_count(), recorded);
    assert!(translator.can_coalesce_idle_cursor_moves());

    session
        .report_native_pointer_roster(NativePointerRoster::Exact(Vec::new()))
        .unwrap();
    commit(&mut session);
    assert_eq!(
        translator.translate(&release, |_| None),
        NativePointerTranslation::Ignored,
        "complete roster recovery must not make a completed release legal again"
    );

    for (ordinal, state) in [(3, ElementState::Pressed), (4, ElementState::Released)] {
        let NativePointerTranslation::Input(input) = translator.translate(
            &record(ordinal, Some(binding), MouseButton::Left, state),
            |_| None,
        ) else {
            panic!("a new press and release must reach exact core authority without a restart");
        };
        session.record_native_pointer(input).unwrap();
        commit(&mut session);
    }
    assert_eq!(session.native_input_recorded_count(), recorded + 3);
    assert_eq!(translator.next_device_identity, 2);
    assert!(translator.can_coalesce_idle_cursor_moves());
    assert!(!session.has_active_gesture());
    assert!(!session.has_pending_native_input());
}

#[test]
fn restarting_one_button_ends_other_button_tails_and_preserves_the_next_real_press() {
    let (mut session, binding) = session(NativePointerRoster::Exact(Vec::new()));
    let mut translator = NativePointerTranslator::default();
    for (ordinal, button) in [(1, MouseButton::Left), (2, MouseButton::Right)] {
        let NativePointerTranslation::Input(input) = translator.translate(
            &record(ordinal, Some(binding), button, ElementState::Pressed),
            |_| None,
        ) else {
            panic!("each initially released button must have one normal press");
        };
        session.record_native_pointer(input).unwrap();
        commit(&mut session);
    }

    let NativePointerTranslation::Restart { cancelled, pressed } = translator.translate(
        &record(3, Some(binding), MouseButton::Left, ElementState::Pressed),
        |_| None,
    ) else {
        panic!("a new unqualified down must atomically replace the old stream");
    };
    session
        .record_native_pointer_batch([cancelled, pressed])
        .unwrap();
    commit(&mut session);
    let retained = translator.owned_data_capacity_bytes();
    let recorded = session.native_input_recorded_count();
    for ordinal in 4..=6 {
        assert_eq!(
            translator.translate(
                &record(
                    ordinal,
                    Some(binding),
                    MouseButton::Right,
                    ElementState::Released
                ),
                |_| None,
            ),
            NativePointerTranslation::Ignored,
            "restarting Primary already ended the predecessor's Secondary button"
        );
        assert_eq!(translator.owned_data_capacity_bytes(), retained);
        assert!(!translator.can_coalesce_idle_cursor_moves());
    }
    assert_eq!(session.native_input_recorded_count(), recorded);

    for (ordinal, button, state) in [
        (7, MouseButton::Left, ElementState::Released),
        (8, MouseButton::Right, ElementState::Pressed),
        (9, MouseButton::Right, ElementState::Released),
    ] {
        let NativePointerTranslation::Input(input) =
            translator.translate(&record(ordinal, Some(binding), button, state), |_| None)
        else {
            panic!("the current release and the next real button sequence must remain valid");
        };
        session.record_native_pointer(input).unwrap();
        commit(&mut session);
    }
    assert_eq!(session.native_input_recorded_count(), recorded + 3);
    assert_eq!(translator.next_device_identity, 1);
    assert_eq!(translator.owned_data_capacity_bytes(), retained);
    assert!(translator.can_coalesce_idle_cursor_moves());
    assert!(!session.has_active_gesture());
    assert!(!session.has_pending_native_input());
}
