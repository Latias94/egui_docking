//! Completed buttons retain their own tail gate without inventing press state.

use super::*;
use dockspace::model::{
    DockspaceLayout, DockspaceNode, DockspaceRootLayout, DockspaceSurfaceLayout, ItemId, RootId,
    SurfaceId,
};
use dockspace::policy::DockPolicy;
use dockspace::runtime::{
    DockspaceSession, HostWindowToken, NativePointerRoster, NativePointerState,
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
