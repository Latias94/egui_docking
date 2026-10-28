//! Mixed-source suppression through real product frames and pointer ownership.

use dockspace::model::{
    DockspaceLayout, DockspaceNode, DockspaceRootLayout, DockspaceSurfaceLayout, ItemId, RootId,
    SurfaceId,
};
use dockspace::runtime::{DockspaceManipulationCancelReason, DockspaceManipulationStatus};
use egui::accesskit::Role;
use egui::{Context, Event, FullOutput, PointerButton, Pos2, RawInput, Rect, TouchPhase, Ui};

use crate::{Dockspace, PaneView};

const SURFACE: SurfaceId = SurfaceId::new(1);
const FIRST: ItemId = ItemId::new(1);
const SECOND: ItemId = ItemId::new(2);

struct Panes;

impl PaneView for Panes {
    fn title(&self, item: ItemId) -> Option<egui::WidgetText> {
        Some(if item == FIRST { "First" } else { "Second" }.into())
    }

    fn ui(&mut self, _item: ItemId, ui: &mut Ui) {
        ui.label("Mixed pointer input");
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
