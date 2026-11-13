//! Ordered modifier intent and mixed-source suppression through product frames.

use dockspace::model::{
    DockspaceLayout, DockspaceNode, DockspaceRootLayout, DockspaceSurfaceLayout, ItemId, RootId,
    SurfaceId,
};
use dockspace::runtime::{
    DockspaceManipulationCancelReason, DockspaceManipulationStatus, DockspaceSession,
    SurfacePointerEvent, SurfaceUnavailableReason,
};
use egui::accesskit::Role;
use egui::{
    Context, Event, FullOutput, Key, Modifiers, PointerButton, Pos2, RawInput, Rect, TouchPhase, Ui,
};

use crate::input::{DockInputConfig, DockingActivation};
use crate::{Dockspace, PaneView};

use super::{ProductPointer, continuation_sample, event_modifiers, update_docking_enabled};

const SURFACE: SurfaceId = SurfaceId::new(1);
const FIRST: ItemId = ItemId::new(1);
const SECOND: ItemId = ItemId::new(2);

struct Panes;

impl PaneView for Panes {
    fn title(&self, item: ItemId) -> Option<egui::WidgetText> {
        Some(if item == FIRST { "First" } else { "Second" }.into())
    }

    fn ui(&mut self, _item: ItemId, ui: &mut Ui) -> crate::PaneResponse {
        ui.label("Mixed pointer input");
        crate::PaneResponse::default()
    }
}

fn frame(
    context: &Context,
    dockspace: &mut Dockspace,
    time: f64,
    events: Vec<Event>,
) -> (FullOutput, Vec<DockspaceManipulationStatus>) {
    let input = RawInput {
        screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(800.0, 600.0))),
        time: Some(time),
        events,
        ..RawInput::default()
    };
    let mut statuses = Vec::new();
    let output = crate::run_ui(context, input, |ui| {
        let response = dockspace
            .show_single_surface(SURFACE, ui, &mut Panes)
            .expect("the ordered mixed-source frame commits");
        statuses.extend(
            response
                .manipulation_feedback()
                .iter()
                .map(|event| event.status()),
        );
    });
    let (mut output, receipt) = output.into_parts();
    receipt.report(crate::SurfacePresentationResult::Presented);
    output.textures_delta.clear();
    (output, statuses)
}

#[allow(
    clippy::cast_possible_truncation,
    reason = "the actual AccessKit bounds are inside the finite egui viewport"
)]
fn tab(output: &FullOutput, label: &str) -> Option<Pos2> {
    output
        .platform_output
        .accesskit_update
        .as_ref()?
        .nodes
        .iter()
        .find_map(|(_, node)| {
            (node.role() == Role::Tab && node.label() == Some(label)).then(|| {
                let bounds = node.bounds().unwrap();
                Rect::from_min_max(
                    Pos2::new(bounds.x0 as f32, bounds.y0 as f32),
                    Pos2::new(bounds.x1 as f32, bounds.y1 as f32),
                )
                .center()
            })
        })
}

fn button(pos: Pos2, pressed: bool) -> Event {
    Event::PointerButton {
        pos,
        button: PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::NONE,
    }
}

fn contact(phase: TouchPhase, pos: Pos2) -> Event {
    Event::Touch {
        device_id: egui::TouchDeviceId(1),
        id: egui::TouchId(7),
        phase,
        pos,
        force: None,
    }
}

#[test]
fn modifier_only_input_remembers_shift_without_enrolling_a_pointer() {
    let context = Context::default();
    context.options_mut(|options| options.max_passes = 1.try_into().unwrap());
    let layout = DockspaceLayout::new([DockspaceSurfaceLayout::new(
        SURFACE,
        DockspaceRootLayout::new(RootId::new(1), DockspaceNode::tabs([FIRST, SECOND])),
    )])
    .unwrap();
    let mut session = DockspaceSession::from_layout(layout, Default::default()).unwrap();
    session.enable_host_pointer().unwrap();
    let mut pointer = ProductPointer::default();
    let mut submissions = 0;
    let (mut output, receipt) = crate::run_ui(
        &context,
        RawInput {
            events: vec![Event::ModifiersChanged(Modifiers::SHIFT)],
            ..RawInput::default()
        },
        |ui| {
            let mut frame = session.begin_host_frame().unwrap();
            let mut draft = pointer.clone();
            let input = draft.prepare(&frame, SURFACE, ui);
            submissions += usize::from(!input.is_empty());
            input.submit_into(&mut frame).unwrap();
            frame
                .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
                .unwrap();
            frame.commit().unwrap();
            pointer = draft;
        },
    )
    .into_parts();
    output.textures_delta.clear();
    receipt.report(crate::SurfacePresentationResult::Presented);
    assert_eq!(submissions, 0, "idle modifiers need no host pointer batch");
    assert_eq!(pointer.docking_enabled, Some(false));
    assert_eq!(pointer.observed_modifiers, Some(Modifiers::SHIFT));
    assert!(!pointer.generic_live);
    assert!(pointer.touches.is_empty());
    assert_eq!(pointer.position, None);
}

#[test]
fn ordered_modifier_sources_preserve_intent_across_events_without_a_directive() {
    let point = Pos2::new(180.0, 160.0);
    let config = DockInputConfig::default();
    let mut observed = None;
    let mut current = None;
    for (event, changed, retained) in [
        (
            Event::PointerButton {
                pos: point,
                button: PointerButton::Primary,
                pressed: true,
                modifiers: Modifiers::SHIFT,
            },
            Some(false),
            false,
        ),
        (Event::PointerMoved(point), None, false),
        (Event::Text("held shift".into()), None, false),
        (
            Event::Key {
                key: Key::A,
                physical_key: Some(Key::A),
                pressed: false,
                repeat: false,
                modifiers: Modifiers::NONE,
            },
            Some(true),
            true,
        ),
        (
            Event::ModifiersChanged(Modifiers::SHIFT),
            Some(false),
            false,
        ),
        (Event::ModifiersChanged(Modifiers::SHIFT), None, false),
        (
            Event::PointerButton {
                pos: point,
                button: PointerButton::Secondary,
                pressed: false,
                modifiers: Modifiers::NONE,
            },
            Some(true),
            true,
        ),
    ] {
        let actual = event_modifiers(&event).and_then(|modifiers| {
            observed = Some(modifiers);
            update_docking_enabled(&mut current, config, modifiers)
        });
        assert_eq!(actual, changed);
        assert_eq!(current, Some(retained), "event-time intent for {event:?}");
    }
    assert_eq!(observed, Some(Modifiers::NONE));
}

#[test]
fn touch_samples_and_continuations_retain_the_observed_docking_intent() {
    let mut pointer = ProductPointer {
        docking_enabled: Some(false),
        ..ProductPointer::default()
    };
    let point = Pos2::new(180.0, 160.0);
    let sample = pointer
        .touches
        .observe(
            egui::TouchDeviceId(1),
            egui::TouchId(7),
            TouchPhase::Start,
            point,
            pointer.docking_enabled,
        )
        .unwrap()
        .unwrap();
    assert_eq!(sample.docking_enabled, Some(false));
    let touch = continuation_sample(
        &pointer.touches,
        Some(point),
        pointer.docking_enabled,
        sample.pointer,
        SurfacePointerEvent::Moved,
    )
    .unwrap();
    assert_eq!(touch.docking_enabled, Some(false));
    let generic = continuation_sample(
        &pointer.touches,
        Some(point),
        pointer.docking_enabled,
        dockspace::runtime::SurfacePointerId::new(1),
        SurfacePointerEvent::Moved,
    )
    .unwrap();
    pointer.docking_enabled = Some(true);
    assert_eq!(generic.docking_enabled, Some(false));
    assert_eq!(generic.position, Some(point));
    let changed = continuation_sample(
        &pointer.touches,
        Some(point),
        pointer.docking_enabled,
        sample.pointer,
        SurfacePointerEvent::Moved,
    )
    .unwrap();
    assert_eq!(changed.docking_enabled, Some(true));
    assert_eq!(changed.pointer, sample.pointer);
}

#[test]
fn configured_activation_rechecks_observations_without_modifier_events() {
    let mut current = None;
    let mut pointer = ProductPointer::default();
    pointer.set_input(DockInputConfig {
        docking: DockingActivation::HoldToEnable(Modifiers::ALT),
        ..Default::default()
    });
    assert_eq!(
        update_docking_enabled(&mut current, pointer.input(), Modifiers::NONE),
        Some(false),
        "hold-to-enable starts disabled when no modifiers were observed",
    );
    pointer.observed_modifiers = Some(Modifiers::ALT | Modifiers::SHIFT);
    assert_eq!(
        update_docking_enabled(
            &mut current,
            pointer.input(),
            pointer.observed_modifiers.unwrap(),
        ),
        Some(true),
    );
    pointer.set_input(DockInputConfig {
        docking: DockingActivation::HoldToDisable(Modifiers::ALT),
        ..pointer.input()
    });
    assert_eq!(
        update_docking_enabled(
            &mut current,
            pointer.input(),
            pointer.observed_modifiers.unwrap(),
        ),
        Some(false),
        "changing the policy recomputes the same actual observation",
    );
    let config = pointer.input();
    pointer.clear();
    assert_eq!(pointer.input(), config);
    assert_eq!(pointer.observed_modifiers, None);
    assert_eq!(pointer.docking_enabled, None);
}

#[test]
fn modifier_markers_only_name_existing_generic_and_touch_streams() {
    let mut pointer = ProductPointer::default();
    assert_eq!(
        super::docking_change_samples(&pointer.touches, false, false).count(),
        0,
    );
    let first = pointer
        .touches
        .observe(
            egui::TouchDeviceId(1),
            egui::TouchId(7),
            TouchPhase::Start,
            Pos2::new(180.0, 160.0),
            Some(true),
        )
        .unwrap()
        .unwrap();
    let second = pointer
        .touches
        .observe(
            egui::TouchDeviceId(1),
            egui::TouchId(8),
            TouchPhase::Start,
            Pos2::new(240.0, 160.0),
            Some(true),
        )
        .unwrap()
        .unwrap();
    let markers = super::docking_change_samples(&pointer.touches, true, false).collect::<Vec<_>>();
    assert_eq!(
        markers
            .iter()
            .map(|marker| marker.pointer)
            .collect::<Vec<_>>(),
        [
            dockspace::runtime::SurfacePointerId::new(1),
            first.pointer,
            second.pointer
        ],
    );
    assert!(markers.iter().all(|marker| {
        marker.position.is_none()
            && marker.docking_enabled == Some(false)
            && matches!(
                marker.event,
                SurfacePointerEvent::DockingEnabledChanged { enabled: false }
            )
    }));
}

#[test]
fn configured_drag_cancellation_replaces_escape_and_allows_extra_modifiers() {
    let context = Context::default();
    context.enable_accesskit();
    context.options_mut(|options| options.max_passes = 1.try_into().unwrap());
    let layout = DockspaceLayout::new([DockspaceSurfaceLayout::new(
        SURFACE,
        DockspaceRootLayout::new(RootId::new(1), DockspaceNode::tabs([FIRST, SECOND])),
    )])
    .unwrap();
    let mut dockspace = Dockspace::builder("configured-drag-cancel", layout)
        .input(DockInputConfig {
            cancel_drag: Some(egui::KeyboardShortcut::new(Modifiers::CTRL, Key::Q)),
            ..Default::default()
        })
        .build()
        .unwrap();
    let mut source = None;
    for index in 0..8 {
        let (output, _) = frame(
            &context,
            &mut dockspace,
            f64::from(index) * 0.05,
            Vec::new(),
        );
        source = tab(&output, "First").or(source);
    }
    let source = source.unwrap();
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
        vec![Event::PointerMoved(source + egui::vec2(12.0, 20.0))],
    );
    let key = |key, modifiers| Event::Key {
        key,
        physical_key: Some(key),
        pressed: true,
        repeat: false,
        modifiers,
    };
    let (_, ignored) = frame(
        &context,
        &mut dockspace,
        1.1,
        vec![key(Key::Escape, Modifiers::NONE)],
    );
    assert!(
        ignored.is_empty(),
        "the replaced default binding must not cancel the drag"
    );
    let (_, cancelled) = frame(
        &context,
        &mut dockspace,
        1.2,
        vec![key(
            Key::Q,
            Modifiers::CTRL | Modifiers::COMMAND | Modifiers::SHIFT,
        )],
    );
    assert_eq!(
        cancelled,
        [DockspaceManipulationStatus::Cancelled(
            DockspaceManipulationCancelReason::ExplicitCancellation
        )],
    );
    assert_eq!(
        dockspace.view().item(FIRST).unwrap().tabs().items(),
        &[FIRST, SECOND]
    );
}

#[test]
fn touch_suppression_cancels_held_generic_once_and_allows_fresh_generic_press() {
    // The ambiguous generic terminal may precede or follow the explicit touch.
    // Neither order can leave pointer one's old button/gesture owner alive.
    for release_first in [false, true] {
        let context = Context::default();
        context.enable_accesskit();
        context.options_mut(|options| options.max_passes = 1.try_into().unwrap());
        let layout = DockspaceLayout::new([DockspaceSurfaceLayout::new(
            SURFACE,
            DockspaceRootLayout::new(RootId::new(1), DockspaceNode::tabs([FIRST, SECOND])),
        )])
        .unwrap();
        let mut dockspace = Dockspace::builder("mixed-source-suppression", layout)
            .build()
            .unwrap();
        let mut points = None;
        for index in 0..8 {
            let (output, _) = frame(
                &context,
                &mut dockspace,
                f64::from(index) * 0.05,
                Vec::new(),
            );
            if let Some(pair) = tab(&output, "First").zip(tab(&output, "Second")) {
                points = Some(pair);
            }
        }
        let (first, second) = points.expect("both real tab controls are presented");
        frame(
            &context,
            &mut dockspace,
            1.0,
            vec![Event::PointerMoved(first), button(first, true)],
        );
        let mut events = vec![contact(TouchPhase::Start, second), button(first, false)];
        if release_first {
            events.reverse();
        }
        let (_, statuses) = frame(&context, &mut dockspace, 1.1, events);
        assert_eq!(
            statuses,
            [DockspaceManipulationStatus::Cancelled(
                DockspaceManipulationCancelReason::PointerStreamCancelled,
            )],
            "the real generic owner terminates once before touch can acquire it",
        );
        assert!(!dockspace.view().item(SECOND).unwrap().is_selected());
        frame(
            &context,
            &mut dockspace,
            1.2,
            vec![contact(TouchPhase::End, second), Event::PointerGone],
        );
        assert!(dockspace.view().item(SECOND).unwrap().is_selected());
        // Let the selected output settle before the next independent press.
        for index in 0..3 {
            frame(
                &context,
                &mut dockspace,
                1.3 + f64::from(index) * 0.05,
                Vec::new(),
            );
        }
        frame(
            &context,
            &mut dockspace,
            1.5,
            vec![Event::PointerMoved(first), button(first, true)],
        );
        assert!(dockspace.view().item(FIRST).unwrap().is_selected());
        frame(&context, &mut dockspace, 1.6, vec![button(first, false)]);
    }
}

fn presented_tabs(name: &str) -> (Context, Dockspace, Pos2, Pos2) {
    let context = Context::default();
    context.enable_accesskit();
    context.options_mut(|options| options.max_passes = 1.try_into().unwrap());
    let layout = DockspaceLayout::new([DockspaceSurfaceLayout::new(
        SURFACE,
        DockspaceRootLayout::new(RootId::new(1), DockspaceNode::tabs([FIRST, SECOND])),
    )])
    .unwrap();
    let mut dockspace = Dockspace::builder(name, layout).build().unwrap();
    let mut points = None;
    for index in 0..6 {
        let (output, _) = frame(
            &context,
            &mut dockspace,
            f64::from(index) * 0.05,
            Vec::new(),
        );
        points = tab(&output, "First").zip(tab(&output, "Second")).or(points);
    }
    let (first, second) = points.expect("both real tab controls are presented");
    (context, dockspace, first, second)
}

#[test]
fn a_foreign_payload_takes_capture_without_motion_and_cannot_finish_a_docking_drag() {
    let (context, mut dockspace, first, second) = presented_tabs("foreign-drag-capture");
    frame(
        &context,
        &mut dockspace,
        1.0,
        vec![Event::PointerMoved(first), button(first, true)],
    );
    frame(
        &context,
        &mut dockspace,
        1.1,
        vec![Event::PointerMoved(first + egui::vec2(12.0, 30.0))],
    );
    assert!(dockspace.has_active_gesture());
    egui::DragAndDrop::set_payload(&context, "application data");
    let (_, statuses) = frame(&context, &mut dockspace, 1.2, Vec::new());
    assert_eq!(
        statuses,
        [DockspaceManipulationStatus::Cancelled(
            DockspaceManipulationCancelReason::CaptureLost,
        )],
        "foreign capture cancels the gesture even without a mouse event"
    );
    assert!(!dockspace.has_active_gesture());
    assert!(egui::DragAndDrop::has_payload_of_type::<&str>(&context));
    frame(
        &context,
        &mut dockspace,
        1.3,
        vec![Event::PointerMoved(second), button(second, false)],
    );
    assert_eq!(
        dockspace.view().item(FIRST).unwrap().tabs().items(),
        &[FIRST, SECOND]
    );
    assert_eq!(
        dockspace
            .view()
            .surfaces()
            .next()
            .unwrap()
            .contained()
            .count(),
        0
    );
    frame(
        &context,
        &mut dockspace,
        1.4,
        vec![
            Event::PointerMoved(second),
            button(second, true),
            button(second, false),
        ],
    );
    assert!(dockspace.view().item(SECOND).unwrap().is_selected());
}

#[test]
fn original_run_foreign_drag_ownership_survives_application_consumption() {
    #[derive(Debug)]
    struct FileDrop;

    impl egui::DroppedFile for FileDrop {
        fn path(&self) -> &std::path::Path {
            std::path::Path::new("/document.txt")
        }

        #[cfg(not(target_arch = "wasm32"))]
        fn bytes(&self) -> Result<Vec<u8>, String> {
            Ok(Vec::new())
        }

        #[cfg(target_arch = "wasm32")]
        fn bytes_async(
            &self,
        ) -> std::pin::Pin<Box<dyn Future<Output = Result<Vec<u8>, String>> + '_>> {
            Box::pin(async { Ok(Vec::new()) })
        }
    }

    for files in [false, true] {
        let (context, mut dockspace, _, second) = presented_tabs("foreign-drag-snapshot");
        if !files {
            egui::DragAndDrop::set_payload(&context, 7_u32);
        }
        let input = RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(800.0, 600.0))),
            time: Some(1.0),
            events: vec![
                Event::PointerMoved(second),
                button(second, true),
                button(second, false),
            ],
            dropped_files: if files {
                vec![std::sync::Arc::new(FileDrop)]
            } else {
                Vec::new()
            },
            ..Default::default()
        };
        let (mut output, receipt) = crate::run_ui(&context, input, |ui| {
            if files {
                ui.input_mut(|input| input.raw.dropped_files.clear());
            } else {
                assert_eq!(
                    egui::DragAndDrop::take_payload::<u32>(ui.ctx()).as_deref(),
                    Some(&7)
                );
            }
            dockspace
                .show_single_surface(SURFACE, ui, &mut Panes)
                .unwrap();
        })
        .into_parts();
        output.textures_delta.clear();
        output.drop_without_applying_deltas();
        receipt.report(crate::SurfacePresentationResult::Presented);
        assert!(
            dockspace.view().item(FIRST).unwrap().is_selected(),
            "files={files}"
        );
        assert!(!dockspace.has_active_gesture());
    }
}
