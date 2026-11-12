//! Native Escape delivery crosses real egui painting and deferred core input.

use dockspace::runtime::{DockspaceManipulationCancelReason, NativeGlobalFocus};
use egui_dockspace::{DockStyle, PaneResponse, PaneView};

use super::*;

struct Panes;

impl PaneView for Panes {
    fn title(&self, _: ItemId) -> Option<egui::WidgetText> {
        Some("Native Escape regression".into())
    }

    fn ui(&mut self, _: ItemId, ui: &mut egui::Ui) -> PaneResponse {
        ui.label("Escape must cancel the source drag from the focused native viewport.");
        PaneResponse::default()
    }
}

#[test]
fn native_escape_cancel_from_source_viewport_reaches_the_reducer() {
    assert_native_escape_cancels_drag(SECOND_SURFACE);
}

#[test]
fn native_escape_cancel_from_other_viewport_reaches_the_reducer() {
    assert_native_escape_cancels_drag(FIRST_SURFACE);
}

fn assert_native_escape_cancels_drag(keyboard_surface: SurfaceId) {
    let (mut native, peer_binding, source_binding) = presented_routes();
    let (receiver, source, target) = source_and_target(&mut native);
    let version = native.session.version();
    for (ordinal, point, state, hover) in [
        (1, source, Some(ElementState::Pressed), 22),
        (2, target, None, 11),
    ] {
        record_pointer(
            &mut native,
            source_binding,
            ordinal,
            mouse_event(point, state, hover, None),
        );
    }
    finish(
        native
            .begin_host_frame(|query| resolve(query, &receiver))
            .unwrap(),
    );
    let capture = native
        .session
        .native_pointer_capture_request()
        .expect("the real native press and move must start a Desktop drag");
    assert_eq!(capture.source_binding(), source_binding);
    assert_eq!(native.session.version(), version);

    let keyboard_binding = if keyboard_surface == SECOND_SURFACE {
        source_binding
    } else {
        peer_binding
    };
    native
        .session
        .report_native_global_focus(NativeGlobalFocus::Dock(keyboard_binding.window()))
        .unwrap();
    commit_boundary(&mut native);
    assert_eq!(
        native.session.native_pointer_capture_request(),
        Some(capture)
    );

    let context = egui::Context::default();
    context.options_mut(|options| {
        options.max_passes = 1.try_into().unwrap();
    });
    let viewport = native.surface_viewport(keyboard_surface).unwrap();
    let mut input = egui::RawInput {
        viewport_id: viewport,
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(640.0, 480.0),
        )),
        focused: true,
        events: vec![egui::Event::Key {
            key: egui::Key::Escape,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        }],
        ..Default::default()
    };
    input.viewports.entry(viewport).or_default().focused = Some(true);
    let mut panes = Panes;
    let mut frame = native
        .begin_host_frame(|_| NativeReceiverAnswer::Unknown)
        .unwrap();
    let mut paint = None;
    let mut output = context.run_ui(input, |ui| {
        paint = Some(
            frame
                .paint_surface(
                    egui::Id::unique(("native-escape-cancel", keyboard_surface)),
                    keyboard_surface,
                    ui,
                    &mut panes,
                    &DockStyle::default(),
                )
                .unwrap(),
        );
    });
    let mut paint = paint.unwrap();
    assert!(paint.validate_final_output(&mut output));
    assert!(paint.confirm_into(&mut frame.frame).unwrap());
    let actions = paint.take_local_actions();
    let _painted = finish(frame);
    output.drop_without_applying_deltas();
    assert!(native.session.has_active_gesture());

    let mut apply = native
        .begin_host_frame(|_| NativeReceiverAnswer::Unknown)
        .unwrap();
    for action in actions {
        apply
            .frame
            .submit_deferred_surface_action(action.into_deferred())
            .unwrap();
    }
    let cancelled = finish(apply);
    assert!(
        !native.session.has_active_gesture(),
        "Escape delivered to native surface {keyboard_surface:?} must cancel the source drag"
    );
    assert_eq!(native.session.version(), version);
    assert!(matches!(
        cancelled.manipulation_feedback(),
        [feedback]
            if feedback.status() == Status::Cancelled(DockspaceManipulationCancelReason::ExplicitCancellation)
    ));
    assert!(native.session.native_pointer_capture_request().is_none());
    assert_eq!(
        native
            .session
            .view()
            .item(ItemId::new(2))
            .unwrap()
            .surface(),
        SECOND_SURFACE
    );
}
