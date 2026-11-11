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

use crate::{Dockspace, PaneView};

use super::{ProductPointer, continuation_sample, update_magnetic_targeting};

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
    assert_eq!(pointer.magnetic_targeting, Some(false));
    assert!(!pointer.generic_live);
    assert!(pointer.touches.is_empty());
    assert_eq!(pointer.position, None);
}

#[test]
fn ordered_modifier_sources_preserve_intent_across_events_without_a_directive() {
    let point = Pos2::new(180.0, 160.0);
    let mut current = None;
    assert_eq!(
        update_magnetic_targeting(&mut current, &Event::PointerMoved(point)),
        None,
    );
    assert_eq!(current, None);
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
        assert_eq!(update_magnetic_targeting(&mut current, &event), changed);
        assert_eq!(current, Some(retained), "event-time intent for {event:?}");
    }
}

#[test]
fn touch_samples_and_continuations_keep_keyboard_targeting_unknown() {
    let mut pointer = ProductPointer {
        magnetic_targeting: Some(false),
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
        )
        .unwrap()
        .unwrap();
    assert_eq!(sample.magnetic_targeting, None);
    let touch = continuation_sample(
        &pointer.touches,
        Some(point),
        pointer.magnetic_targeting,
        sample.pointer,
        SurfacePointerEvent::Moved,
    )
    .unwrap();
    assert_eq!(touch.magnetic_targeting, None);
    let generic = continuation_sample(
        &pointer.touches,
        Some(point),
        pointer.magnetic_targeting,
        dockspace::runtime::SurfacePointerId::new(1),
        SurfacePointerEvent::Moved,
    )
    .unwrap();
    pointer.magnetic_targeting = Some(true);
    assert_eq!(generic.magnetic_targeting, Some(false));
    assert_eq!(generic.position, Some(point));
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
