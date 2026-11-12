//! Input ordering and IME composition through both public egui entry points.

use dockspace::model::{
    DockspaceLayout, DockspaceNode, DockspaceRootLayout, DockspaceSurfaceLayout, ItemId, RootId,
    SurfaceId,
};
use egui::accesskit::Role;
use egui::{
    Context, Event, FullOutput, Id, ImeEvent, Modifiers, PointerButton, Pos2, RawInput, Rect, Ui,
};

use crate::{Dockspace, DockspaceOutput, PaneResponse, PaneView, SurfacePresentationResult};

const SURFACE: SurfaceId = SurfaceId::new(1);
const EDITOR: ItemId = ItemId::new(1);
const OTHER: ItemId = ItemId::new(2);

struct EditorPane {
    target: Id,
    text: String,
    bounds: Option<Rect>,
}

impl PaneView for EditorPane {
    fn title(&self, item: ItemId) -> Option<egui::WidgetText> {
        Some(if item == EDITOR { "Editor" } else { "Other" }.into())
    }

    fn ui(&mut self, item: ItemId, ui: &mut Ui) -> PaneResponse {
        if item != EDITOR {
            return PaneResponse::default();
        }
        let response = ui.add_sized(
            ui.available_size(),
            egui::TextEdit::multiline(&mut self.text).id(self.target),
        );
        self.bounds = Some(response.rect);
        PaneResponse::from(&response)
    }

    fn focus_target(&self, item: ItemId) -> Option<Id> {
        (item == EDITOR).then_some(self.target)
    }
}

fn run(
    owned: bool,
    context: &Context,
    dockspace: &mut Dockspace,
    pane: &mut EditorPane,
    time: f64,
    events: Vec<Event>,
) -> DockspaceOutput {
    let input = RawInput {
        screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(640.0, 480.0))),
        focused: true,
        time: Some(time),
        events,
        ..RawInput::default()
    };
    if owned {
        dockspace
            .run_single_surface(context, input, SURFACE, pane)
            .unwrap()
            .0
    } else {
        crate::run_ui(context, input, |ui| {
            dockspace.show_single_surface(SURFACE, ui, pane).unwrap();
        })
    }
}

#[allow(
    clippy::cast_possible_truncation,
    reason = "the renderer supplies finite tab bounds inside the 640x480 viewport"
)]
fn editor_tab_position(output: &FullOutput) -> Pos2 {
    let bounds = output
        .platform_output
        .accesskit_update
        .as_ref()
        .unwrap()
        .nodes
        .iter()
        .find_map(|(_, node)| {
            (node.role() == Role::Tab && node.label() == Some("Editor"))
                .then(|| node.bounds().unwrap())
        })
        .expect("the selected editor tab must have real painted geometry");
    Rect::from_min_max(
        Pos2::new(bounds.x0 as f32, bounds.y0 as f32),
        Pos2::new(bounds.x1 as f32, bounds.y1 as f32),
    )
    .center()
}

fn presented_editor(owned: bool) -> (Context, Dockspace, EditorPane, Pos2) {
    let context = Context::default();
    context.enable_accesskit();
    context.options_mut(|options| options.max_passes = 1.try_into().unwrap());
    let layout = DockspaceLayout::new([DockspaceSurfaceLayout::new(
        SURFACE,
        DockspaceRootLayout::new(RootId::new(1), DockspaceNode::central_tabs([EDITOR, OTHER])),
    )])
    .unwrap();
    let mut dockspace = Dockspace::builder(("focus-input-order", owned), layout)
        .build()
        .unwrap();
    let mut pane = EditorPane {
        target: Id::unique(("focus-input-editor", owned)),
        text: String::new(),
        bounds: None,
    };
    let mut tab = None;
    for pass in 0..4 {
        let (output, receipt) = run(
            owned,
            &context,
            &mut dockspace,
            &mut pane,
            f64::from(pass) * 0.1,
            Vec::new(),
        )
        .into_parts();
        if pass >= 2 {
            let current = editor_tab_position(&output);
            if let Some(previous) = tab {
                assert_eq!(current, previous, "presented tab geometry must be stable");
            }
            tab = Some(current);
        }
        output.drop_without_applying_deltas();
        receipt.report(SurfacePresentationResult::Presented);
    }
    assert!(dockspace.view().item(EDITOR).unwrap().is_selected());
    assert!(pane.bounds.is_some());
    (context, dockspace, pane, tab.unwrap())
}

fn click(position: Pos2) -> [Event; 3] {
    [
        Event::PointerMoved(position),
        Event::PointerButton {
            pos: position,
            button: PointerButton::Primary,
            pressed: true,
            modifiers: Modifiers::NONE,
        },
        Event::PointerButton {
            pos: position,
            button: PointerButton::Primary,
            pressed: false,
            modifiers: Modifiers::NONE,
        },
    ]
}

#[test]
fn a_later_editor_click_replaces_the_same_packets_deferred_tab_focus() {
    for owned in [true, false] {
        let (context, mut dockspace, mut pane, tab) = presented_editor(owned);
        let editor = pane.bounds.unwrap().center();
        let events = click(tab).into_iter().chain(click(editor)).collect();
        let (output, receipt) =
            run(owned, &context, &mut dockspace, &mut pane, 1.0, events).into_parts();
        assert_eq!(
            context.memory(egui::Memory::focused),
            Some(pane.target),
            "the last click must focus its editor before the run returns, owned={owned}"
        );
        output.drop_without_applying_deltas();
        receipt.report(SurfacePresentationResult::Presented);

        let (output, receipt) = run(
            owned,
            &context,
            &mut dockspace,
            &mut pane,
            1.1,
            vec![Event::Text("typed after the last click".into())],
        )
        .into_parts();
        assert_eq!(pane.text, "typed after the last click", "owned={owned}");
        assert_eq!(context.memory(egui::Memory::focused), Some(pane.target));
        output.drop_without_applying_deltas();
        receipt.report(SurfacePresentationResult::Presented);
    }
}

#[test]
fn pending_pane_focus_keeps_ime_composition_while_receipts_are_held() {
    for owned in [true, false] {
        let (context, mut dockspace, mut pane, _) = presented_editor(owned);
        dockspace.select_item_current(EDITOR).unwrap();
        let (output, request_receipt) =
            run(owned, &context, &mut dockspace, &mut pane, 1.0, Vec::new()).into_parts();
        assert_eq!(context.memory(egui::Memory::focused), Some(pane.target));
        output.drop_without_applying_deltas();

        let mut receipts = vec![request_receipt];
        for (step, event, expected) in [
            (
                0,
                ImeEvent::Preedit {
                    text: "n".into(),
                    active_range_chars: Some(0..1),
                },
                "n",
            ),
            (
                1,
                ImeEvent::Preedit {
                    text: "ni".into(),
                    active_range_chars: Some(0..2),
                },
                "ni",
            ),
            (2, ImeEvent::Commit("你".into()), "你"),
        ] {
            let (output, receipt) = run(
                owned,
                &context,
                &mut dockspace,
                &mut pane,
                1.1 + f64::from(step) * 0.1,
                vec![Event::Ime(event)],
            )
            .into_parts();
            let ime = output
                .platform_output
                .ime
                .as_ref()
                .expect("the real focused editor must continue emitting IME output");
            assert!(
                !ime.should_interrupt_composition,
                "an unacknowledged request must not refocus its existing owner, owned={owned}, step={step}"
            );
            assert_eq!(context.memory(egui::Memory::focused), Some(pane.target));
            assert_eq!(pane.text, expected, "owned={owned}, step={step}");
            output.drop_without_applying_deltas();
            receipts.push(receipt);
        }
        for receipt in receipts {
            receipt.report(SurfacePresentationResult::Presented);
        }
        let (output, receipt) =
            run(owned, &context, &mut dockspace, &mut pane, 2.0, Vec::new()).into_parts();
        assert_eq!(pane.text, "你");
        assert_eq!(context.memory(egui::Memory::focused), Some(pane.target));
        output.drop_without_applying_deltas();
        receipt.report(SurfacePresentationResult::Presented);
    }
}
