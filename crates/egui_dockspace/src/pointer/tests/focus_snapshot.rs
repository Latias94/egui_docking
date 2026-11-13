use super::*;

fn dragging(name: &str) -> (Context, Dockspace, Pos2, Pos2) {
    let (context, mut dockspace, source, target) = presented_tabs(name);
    frame(
        &context,
        &mut dockspace,
        1.0,
        vec![Event::PointerMoved(source), button(source, true)],
    );
    frame(
        &context,
        &mut dockspace,
        1.05,
        vec![Event::PointerMoved(source + egui::vec2(12.0, 30.0))],
    );
    assert!(dockspace.has_active_gesture());
    (context, dockspace, source, target)
}

fn focused_frame(
    context: &Context,
    dockspace: &mut Dockspace,
    time: f64,
    focused: bool,
    events: Vec<Event>,
) -> Vec<DockspaceManipulationStatus> {
    let mut statuses = Vec::new();
    let output = crate::run_ui(
        context,
        RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(800.0, 600.0))),
            time: Some(time),
            focused,
            events,
            ..RawInput::default()
        },
        |ui| {
            assert_eq!(ui.input(|input| input.focused), focused);
            let response = dockspace
                .show_single_surface(SURFACE, ui, &mut Panes)
                .unwrap();
            statuses.extend(
                response
                    .manipulation_feedback()
                    .iter()
                    .map(|feedback| feedback.status()),
            );
        },
    );
    let (mut output, receipt) = output.into_parts();
    receipt.report(crate::SurfacePresentationResult::Presented);
    output.textures_delta.clear();
    statuses
}

#[test]
fn an_unfocused_raw_input_snapshot_cancels_dragging_without_a_focus_event() {
    let (context, mut dockspace, _, _) = dragging("snapshot-focus-loss");
    let version = dockspace.version();
    let statuses = focused_frame(&context, &mut dockspace, 1.1, false, Vec::new());
    assert!(
        !dockspace.has_active_gesture(),
        "the actual host focus snapshot must retire docking even without an optional WindowFocused event"
    );
    assert_eq!(dockspace.version(), version);
    assert!(matches!(
        statuses.as_slice(),
        [DockspaceManipulationStatus::Cancelled(_)]
    ));
}

#[test]
fn an_unfocused_snapshot_rejects_new_mouse_edges_and_a_late_release() {
    let (context, mut dockspace, _, target) = dragging("unfocused-mouse-suffix");
    let version = dockspace.version();
    let statuses = focused_frame(
        &context,
        &mut dockspace,
        1.1,
        false,
        vec![
            Event::WindowFocused(true),
            Event::PointerMoved(target),
            button(target, true),
            Event::PointerMoved(target + egui::vec2(12.0, 30.0)),
            button(target, false),
        ],
    );
    assert!(matches!(
        statuses.as_slice(),
        [DockspaceManipulationStatus::Cancelled(_)]
    ));
    assert!(!dockspace.has_active_gesture());
    assert_eq!(dockspace.version(), version);
    assert!(dockspace.view().item(FIRST).unwrap().is_selected());

    let (_, statuses) = frame(&context, &mut dockspace, 1.2, vec![button(target, false)]);
    assert!(statuses.is_empty());
    assert!(!dockspace.has_active_gesture());
    assert_eq!(dockspace.version(), version);
    frame(&context, &mut dockspace, 1.3, vec![button(target, true)]);
    assert!(dockspace.has_active_gesture());
    assert!(dockspace.view().item(SECOND).unwrap().is_selected());
    frame(&context, &mut dockspace, 1.4, vec![button(target, false)]);
    assert!(!dockspace.has_active_gesture());
}

#[test]
fn a_focused_snapshot_preserves_ordered_focus_loss_and_gain() {
    for regain in [false, true] {
        let (context, mut dockspace, _, target) = dragging("ordered-focus-boundary");
        let version = dockspace.version();
        let mut events = vec![Event::WindowFocused(false), button(target, true)];
        if regain {
            events.extend([Event::WindowFocused(true), button(target, true)]);
        }
        let statuses = focused_frame(&context, &mut dockspace, 1.1, true, events);
        assert!(matches!(
            statuses.as_slice(),
            [DockspaceManipulationStatus::Cancelled(_)]
        ));
        assert_eq!(dockspace.has_active_gesture(), regain);
        assert_eq!(dockspace.view().item(SECOND).unwrap().is_selected(), regain);
        if !regain {
            assert_eq!(dockspace.version(), version);
        }
        frame(&context, &mut dockspace, 1.2, vec![button(target, false)]);
        assert!(!dockspace.has_active_gesture());
    }
}

#[test]
fn an_unfocused_snapshot_retires_touch_contacts_and_cannot_rearm_them() {
    let (context, mut dockspace, source, target) = presented_tabs("unfocused-touch-suffix");
    let version = dockspace.version();
    frame(
        &context,
        &mut dockspace,
        1.0,
        vec![contact(TouchPhase::Start, target)],
    );
    assert!(dockspace.has_active_gesture());
    let mut another_contact = contact(TouchPhase::Start, source);
    if let Event::Touch { id, .. } = &mut another_contact {
        *id = egui::TouchId(8);
    }
    focused_frame(
        &context,
        &mut dockspace,
        1.1,
        false,
        vec![Event::WindowFocused(true), another_contact],
    );
    assert!(!dockspace.has_active_gesture());
    frame(
        &context,
        &mut dockspace,
        1.2,
        vec![contact(TouchPhase::End, target)],
    );
    assert_eq!(dockspace.version(), version);
    assert!(dockspace.view().item(FIRST).unwrap().is_selected());
    frame(
        &context,
        &mut dockspace,
        1.3,
        vec![contact(TouchPhase::Start, target)],
    );
    frame(
        &context,
        &mut dockspace,
        1.4,
        vec![contact(TouchPhase::End, target)],
    );
    assert!(dockspace.view().item(SECOND).unwrap().is_selected());
    assert!(!dockspace.has_active_gesture());
}

#[test]
fn a_discarded_touch_in_an_unfocused_interval_does_not_suppress_the_focused_mouse_suffix() {
    for touch_before_loss in [false, true] {
        let (context, mut dockspace, _, target) = if touch_before_loss {
            presented_tabs("cancelled-touch-focused-mouse")
        } else {
            dragging("discarded-touch-focused-mouse")
        };
        let mut events = if touch_before_loss {
            vec![
                contact(TouchPhase::Start, target),
                Event::WindowFocused(false),
            ]
        } else {
            vec![
                Event::WindowFocused(false),
                contact(TouchPhase::Start, target),
            ]
        };
        events.extend([
            Event::WindowFocused(true),
            Event::PointerMoved(target),
            button(target, true),
        ]);
        let statuses = focused_frame(&context, &mut dockspace, 1.1, true, events);
        if !touch_before_loss {
            assert!(matches!(
                statuses.as_slice(),
                [DockspaceManipulationStatus::Cancelled(_)]
            ));
        }
        assert!(
            dockspace.has_active_gesture(),
            "touch before loss={touch_before_loss}: a retired or discarded contact cannot suppress a real press after focus returns"
        );
        assert!(dockspace.view().item(SECOND).unwrap().is_selected());
        let selected = dockspace.version();
        frame(&context, &mut dockspace, 1.2, vec![button(target, false)]);
        assert!(!dockspace.has_active_gesture());
        assert_eq!(dockspace.version(), selected);
        assert_eq!(
            dockspace.view().item(SECOND).unwrap().tabs().items(),
            [FIRST, SECOND],
            "the focused mouse release completes its own click without moving the old drag"
        );
    }
}

struct DiscardOnce(bool);

impl egui::plugin::Plugin for DiscardOnce {
    fn debug_name(&self) -> &'static str {
        "focus_snapshot::discard_once"
    }

    fn output_hook(&mut self, _: &Context, output: &mut FullOutput) {
        if !self.0 {
            self.0 = true;
            output
                .platform_output
                .request_discard_reasons
                .push(egui::RepaintCause::new_reason(
                    "verify frozen focus on a repeated pass",
                ));
        }
    }
}

#[test]
fn raw_focus_is_frozen_before_application_edits_and_reduced_once_across_passes() {
    for focused in [false, true] {
        let (context, mut dockspace, _, _) = dragging("frozen-focus-multipass");
        let version = dockspace.version();
        context.options_mut(|options| options.max_passes = 2.try_into().unwrap());
        context.add_plugin(DiscardOnce(false));
        let mut passes = 0;
        let (output, response) = dockspace
            .run_single_surface_with_ui(
                &context,
                RawInput {
                    screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(800.0, 600.0))),
                    time: Some(1.1),
                    focused,
                    ..RawInput::default()
                },
                SURFACE,
                &mut Panes,
                |ui| {
                    passes += 1;
                    ui.input_mut(|input| {
                        input.focused = !focused;
                        input.raw.focused = !focused;
                    });
                },
            )
            .unwrap();
        let (mut output, receipt) = output.into_parts();
        assert_eq!(output.platform_output.num_completed_passes, 2);
        assert_eq!(passes, 2);
        assert_eq!(dockspace.has_active_gesture(), focused);
        assert_eq!(dockspace.version(), version);
        if focused {
            assert!(response.manipulation_feedback().is_empty());
        } else {
            assert!(matches!(
                response.manipulation_feedback(),
                [feedback] if matches!(feedback.status(), DockspaceManipulationStatus::Cancelled(_))
            ));
        }
        receipt.report(crate::SurfacePresentationResult::Presented);
        output.textures_delta.clear();
        let (_, statuses) = frame(&context, &mut dockspace, 1.2, Vec::new());
        assert!(
            statuses.is_empty(),
            "a settled run must not replay its cancellation"
        );
    }
}
