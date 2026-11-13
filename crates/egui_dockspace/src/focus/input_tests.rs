//! Input ordering and IME composition through both public egui entry points.

use dockspace::model::{
    DockspaceLayout, DockspaceNode, DockspaceRootLayout, DockspaceSurfaceLayout, ItemId, RootId,
    SurfaceId,
};
use egui::accesskit::Role;
use egui::{
    Context, Event, FullOutput, Id, ImeEvent, Modifiers, PointerButton, Pos2, RawInput, Rect, Ui,
    ViewportId,
};

use crate::{Dockspace, DockspaceOutput, PaneResponse, PaneView, SurfacePresentationResult};

const SURFACE: SurfaceId = SurfaceId::new(1);
const EDITOR: ItemId = ItemId::new(1);
const OTHER: ItemId = ItemId::new(2);

struct EditorPane {
    target: Id,
    text: String,
    bounds: Option<Rect>,
    focus_trace: Vec<(u64, Option<Id>)>,
}

impl PaneView for EditorPane {
    fn title(&self, item: ItemId) -> Option<egui::WidgetText> {
        Some(if item == EDITOR { "Editor" } else { "Other" }.into())
    }

    fn ui(&mut self, item: ItemId, ui: &mut Ui) -> PaneResponse {
        if item != EDITOR {
            return PaneResponse::default();
        }
        self.focus_trace.push((
            ui.ctx().cumulative_pass_nr(),
            ui.memory(egui::Memory::focused),
        ));
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
    run_with_ui(owned, context, dockspace, pane, time, events, |_| {})
}

fn run_with_ui(
    owned: bool,
    context: &Context,
    dockspace: &mut Dockspace,
    pane: &mut EditorPane,
    time: f64,
    events: Vec<Event>,
    before_docking: impl FnMut(&mut Ui),
) -> DockspaceOutput {
    run_with_input(
        owned,
        context,
        dockspace,
        pane,
        input(ViewportId::ROOT, time, events),
        before_docking,
    )
}

fn input(viewport: ViewportId, time: f64, events: Vec<Event>) -> RawInput {
    let mut input = RawInput {
        viewport_id: viewport,
        screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(640.0, 480.0))),
        focused: true,
        time: Some(time),
        events,
        ..RawInput::default()
    };
    if viewport != ViewportId::ROOT {
        let info = input.viewports.entry(viewport).or_default();
        info.parent = Some(ViewportId::ROOT);
        info.focused = Some(true);
    }
    input
}

fn run_with_input(
    owned: bool,
    context: &Context,
    dockspace: &mut Dockspace,
    pane: &mut EditorPane,
    input: RawInput,
    mut before_docking: impl FnMut(&mut Ui),
) -> DockspaceOutput {
    if owned {
        dockspace
            .run_single_surface_with_ui(context, input, SURFACE, pane, before_docking)
            .unwrap()
            .0
    } else {
        crate::run_ui(context, input, |ui| {
            before_docking(ui);
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
    presented_editor_with_ui(owned, |_| {})
}

fn presented_editor_with_ui(
    owned: bool,
    before_docking: impl FnMut(&mut Ui),
) -> (Context, Dockspace, EditorPane, Pos2) {
    presented_editor_in_viewport(owned, ViewportId::ROOT, before_docking)
}

fn presented_editor_in_viewport(
    owned: bool,
    viewport: ViewportId,
    mut before_docking: impl FnMut(&mut Ui),
) -> (Context, Dockspace, EditorPane, Pos2) {
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
        focus_trace: Vec::new(),
    };
    let mut tab = None;
    for pass in 0..4 {
        let (mut output, receipt) = run_with_input(
            owned,
            &context,
            &mut dockspace,
            &mut pane,
            input(viewport, f64::from(pass) * 0.1, Vec::new()),
            &mut before_docking,
        )
        .into_parts();
        output.textures_delta.clear();
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
        let (mut output, receipt) =
            run(owned, &context, &mut dockspace, &mut pane, 1.0, events).into_parts();
        output.textures_delta.clear();
        assert_eq!(
            context.memory(egui::Memory::focused),
            Some(pane.target),
            "the last click must focus its editor before the run returns, owned={owned}"
        );
        output.drop_without_applying_deltas();
        receipt.report(SurfacePresentationResult::Presented);

        let (mut output, receipt) = run(
            owned,
            &context,
            &mut dockspace,
            &mut pane,
            1.1,
            vec![Event::Text("typed after the last click".into())],
        )
        .into_parts();
        output.textures_delta.clear();
        assert_eq!(pane.text, "typed after the last click", "owned={owned}");
        assert_eq!(context.memory(egui::Memory::focused), Some(pane.target));
        output.drop_without_applying_deltas();
        receipt.report(SurfacePresentationResult::Presented);
    }
}

#[test]
fn an_unsuperseded_tab_press_hands_focus_to_its_content_once() {
    for owned in [true, false] {
        let (context, mut dockspace, mut pane, tab) = presented_editor(owned);
        let (mut output, receipt) = run(
            owned,
            &context,
            &mut dockspace,
            &mut pane,
            1.0,
            click(tab).into(),
        )
        .into_parts();
        output.textures_delta.clear();
        output.drop_without_applying_deltas();
        receipt.report(SurfacePresentationResult::Presented);

        for time in [1.1, 1.2] {
            let (mut output, receipt) =
                run(owned, &context, &mut dockspace, &mut pane, time, Vec::new()).into_parts();
            output.textures_delta.clear();
            assert_eq!(
                context.memory(egui::Memory::focused),
                Some(pane.target),
                "the tab's core focus intent must reach its content, owned={owned}"
            );
            output.drop_without_applying_deltas();
            receipt.report(SurfacePresentationResult::Presented);
        }
        context.memory_mut(|memory| memory.surrender_focus(pane.target));
        let (mut output, receipt) =
            run(owned, &context, &mut dockspace, &mut pane, 1.3, Vec::new()).into_parts();
        output.textures_delta.clear();
        assert!(!context.memory(|memory| memory.has_focus(pane.target)));
        output.drop_without_applying_deltas();
        receipt.report(SurfacePresentationResult::Presented);
    }
}

#[test]
fn pending_pane_focus_keeps_ime_composition_while_receipts_are_held() {
    for owned in [true, false] {
        let (context, mut dockspace, mut pane, _) = presented_editor(owned);
        dockspace.select_item_current(EDITOR).unwrap();
        let (mut output, request_receipt) =
            run(owned, &context, &mut dockspace, &mut pane, 1.0, Vec::new()).into_parts();
        output.textures_delta.clear();
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
            let (mut output, receipt) = run(
                owned,
                &context,
                &mut dockspace,
                &mut pane,
                1.1 + f64::from(step) * 0.1,
                vec![Event::Ime(event)],
            )
            .into_parts();
            output.textures_delta.clear();
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
        let (mut output, receipt) =
            run(owned, &context, &mut dockspace, &mut pane, 2.0, Vec::new()).into_parts();
        output.textures_delta.clear();
        assert_eq!(pane.text, "你");
        assert_eq!(context.memory(egui::Memory::focused), Some(pane.target));
        output.drop_without_applying_deltas();
        receipt.report(SurfacePresentationResult::Presented);
    }
}

struct SearchField {
    target: Id,
    text: String,
    bounds: Option<Rect>,
    focused_during_ui: bool,
}

impl SearchField {
    fn new() -> Self {
        Self {
            target: Id::unique("focus-input-host-search"),
            text: String::new(),
            bounds: None,
            focused_during_ui: false,
        }
    }

    fn ui(&mut self, ui: &mut Ui) {
        egui::Panel::top("focus-input-search-panel").show(ui, |ui| {
            let response = ui.add(
                egui::TextEdit::singleline(&mut self.text)
                    .id(self.target)
                    .hint_text("Search"),
            );
            self.bounds = Some(response.rect);
            self.focused_during_ui = response.has_focus();
        });
    }
}

struct FocusSearchAtPassEnd {
    target: Id,
    armed: bool,
    discard_once: bool,
}

impl egui::plugin::Plugin for FocusSearchAtPassEnd {
    fn debug_name(&self) -> &'static str {
        "product_focus_input_tests::focus_search_at_pass_end"
    }

    fn on_end_pass(&mut self, ui: &mut Ui) {
        if std::mem::take(&mut self.armed) {
            assert!(ui.ctx().viewport(|viewport| {
                viewport
                    .this_pass
                    .widgets
                    .get(self.target)
                    .is_some_and(|widget| {
                        widget.enabled && widget.visible && widget.sense.is_focusable()
                    })
            }));
            ui.memory_mut(|memory| memory.request_focus(self.target));
        }
    }

    fn output_hook(&mut self, _context: &Context, output: &mut FullOutput) {
        if std::mem::take(&mut self.discard_once) {
            output
                .platform_output
                .request_discard_reasons
                .push(egui::RepaintCause::new_reason(
                    "retry after the plugin transfers focus",
                ));
        }
    }
}

fn assert_owned_tab_press_yields_to_plugin_search(discard_once: bool, viewport: ViewportId) {
    let mut search = SearchField::new();
    let (context, mut dockspace, mut pane, tab) =
        presented_editor_in_viewport(true, viewport, |ui| search.ui(ui));
    if discard_once {
        context.options_mut(|options| options.max_passes = 2.try_into().unwrap());
    }
    context.add_plugin(FocusSearchAtPassEnd {
        target: search.target,
        armed: true,
        discard_once,
    });
    pane.focus_trace.clear();
    let (mut output, receipt) = run_with_input(
        true,
        &context,
        &mut dockspace,
        &mut pane,
        input(viewport, 1.0, click(tab).into()),
        |ui| search.ui(ui),
    )
    .into_parts();
    output.textures_delta.clear();
    assert_eq!(
        output.platform_output.num_completed_passes,
        if discard_once { 2 } else { 1 },
        "the fixture must exercise the requested number of real passes"
    );
    let update = output.platform_output.accesskit_update.as_ref().unwrap();
    assert!(
        update
            .nodes
            .iter()
            .any(|(id, _)| *id == search.target.accesskit_id())
    );
    assert_eq!(
        update.focus,
        search.target.accesskit_id(),
        "the one-shot plugin must retain its real search focus through final output, viewport={viewport:?}, discard_once={discard_once}, pane_focus_trace={:?}",
        pane.focus_trace,
    );
    if viewport == ViewportId::ROOT {
        assert_eq!(
            context.memory(egui::Memory::focused),
            Some(search.target),
            "publishing the earlier tab press must preserve the plugin's later focus, discard_once={discard_once}"
        );
    }
    output.drop_without_applying_deltas();
    receipt.report(SurfacePresentationResult::Presented);

    pane.focus_trace.clear();
    let (mut output, receipt) = run_with_input(
        true,
        &context,
        &mut dockspace,
        &mut pane,
        input(
            viewport,
            1.1,
            vec![Event::Text("plugin search input".into())],
        ),
        |ui| search.ui(ui),
    )
    .into_parts();
    output.textures_delta.clear();
    assert!(
        search.focused_during_ui,
        "the real search must still own focus while it handles the text, viewport={viewport:?}, discard_once={discard_once}"
    );
    assert_eq!(search.text, "plugin search input");
    let final_focus = output
        .platform_output
        .accesskit_update
        .as_ref()
        .unwrap()
        .focus;
    assert!(
        pane.text.is_empty(),
        "the same Text must not be handled by the pane after search: viewport={viewport:?}, discard_once={discard_once}, search_text={:?}, pane_text={:?}, search_id={:?}, pane_id={:?}, pane_focus_trace={:?}, final_tree_focus={final_focus:?}",
        search.text,
        pane.text,
        search.target,
        pane.target,
        pane.focus_trace,
    );
    assert_eq!(final_focus, search.target.accesskit_id());
    if viewport == ViewportId::ROOT {
        assert_eq!(context.memory(egui::Memory::focused), Some(search.target));
    }
    output.drop_without_applying_deltas();
    receipt.report(SurfacePresentationResult::Presented);
}

#[test]
fn an_owned_tab_press_yields_to_a_plugins_later_search_focus() {
    assert_owned_tab_press_yields_to_plugin_search(false, ViewportId::ROOT);
}

#[test]
fn an_owned_tab_press_yields_to_a_plugins_later_search_focus_after_a_discard() {
    assert_owned_tab_press_yields_to_plugin_search(true, ViewportId::ROOT);
}

#[test]
fn an_owned_tab_press_yields_to_a_plugins_later_search_focus_in_a_child_viewport() {
    assert_owned_tab_press_yields_to_plugin_search(
        false,
        ViewportId::from_hash_of("focus-input-child"),
    );
}

#[test]
fn an_owned_tab_press_yields_to_a_plugins_later_search_focus_in_a_child_viewport_after_a_discard() {
    assert_owned_tab_press_yields_to_plugin_search(
        true,
        ViewportId::from_hash_of("focus-input-child"),
    );
}

#[allow(
    clippy::too_many_lines,
    reason = "the regression follows a real search click, IME composition and final receipt settlement"
)]
fn assert_pending_pane_focus_yields_to_search(owned: bool, hold_receipts: bool) {
    let mut search = SearchField::new();
    let (context, mut dockspace, mut pane, _) = presented_editor_with_ui(owned, |ui| search.ui(ui));
    dockspace.select_item_current(EDITOR).unwrap();
    let (mut output, receipt) = run_with_ui(
        owned,
        &context,
        &mut dockspace,
        &mut pane,
        1.0,
        Vec::new(),
        |ui| search.ui(ui),
    )
    .into_parts();
    output.textures_delta.clear();
    assert_eq!(context.memory(egui::Memory::focused), Some(pane.target));
    output.drop_without_applying_deltas();
    let mut receipts = Vec::new();
    if hold_receipts {
        receipts.push(receipt);
    } else {
        receipt.report(SurfacePresentationResult::Dropped);
    }

    let position = search.bounds.unwrap().center();
    let (mut output, receipt) = run_with_ui(
        owned,
        &context,
        &mut dockspace,
        &mut pane,
        1.1,
        click(position).into(),
        |ui| search.ui(ui),
    )
    .into_parts();
    output.textures_delta.clear();
    assert!(
        search.focused_during_ui,
        "the real search field must receive its pointer click"
    );
    assert_eq!(
        context.memory(egui::Memory::focused),
        Some(search.target),
        "the later application click supersedes the already applied request, owned={owned}, hold_receipts={hold_receipts}"
    );
    output.drop_without_applying_deltas();
    if hold_receipts {
        receipts.push(receipt);
    } else {
        receipt.report(SurfacePresentationResult::Dropped);
    }

    for (step, event, expected) in [
        (
            0,
            Event::Ime(ImeEvent::Preedit {
                text: "ni".into(),
                active_range_chars: Some(0..2),
            }),
            "ni",
        ),
        (1, Event::Ime(ImeEvent::Commit("你".into())), "你"),
        (2, Event::Text(" found".into()), "你 found"),
    ] {
        let (mut output, receipt) = run_with_ui(
            owned,
            &context,
            &mut dockspace,
            &mut pane,
            1.2 + f64::from(step) * 0.1,
            vec![event],
            |ui| search.ui(ui),
        )
        .into_parts();
        output.textures_delta.clear();
        assert_eq!(
            search.text, expected,
            "owned={owned}, hold_receipts={hold_receipts}, step={step}"
        );
        assert!(
            pane.text.is_empty(),
            "search input must never enter the pane editor"
        );
        assert_eq!(context.memory(egui::Memory::focused), Some(search.target));
        let ime = output
            .platform_output
            .ime
            .as_ref()
            .expect("the search editor owns IME output");
        assert!(search.bounds.unwrap().contains_rect(ime.rect));
        assert!(
            !ime.should_interrupt_composition,
            "the old request must not interrupt the search composition"
        );
        output.drop_without_applying_deltas();
        if hold_receipts {
            receipts.push(receipt);
        } else {
            receipt.report(SurfacePresentationResult::Dropped);
        }
    }
    for receipt in receipts {
        receipt.report(SurfacePresentationResult::Presented);
    }
    let (mut output, receipt) = run_with_ui(
        owned,
        &context,
        &mut dockspace,
        &mut pane,
        2.0,
        Vec::new(),
        |ui| search.ui(ui),
    )
    .into_parts();
    output.textures_delta.clear();
    assert_eq!(search.text, "你 found");
    assert_eq!(context.memory(egui::Memory::focused), Some(search.target));
    output.drop_without_applying_deltas();
    receipt.report(SurfacePresentationResult::Presented);
}

#[test]
fn an_owned_held_pane_focus_request_yields_to_later_search_input() {
    assert_pending_pane_focus_yields_to_search(true, true);
}

#[test]
fn a_borrowed_held_pane_focus_request_yields_to_later_search_input() {
    assert_pending_pane_focus_yields_to_search(false, true);
}

#[test]
fn an_owned_dropped_pane_focus_request_yields_to_later_search_input() {
    assert_pending_pane_focus_yields_to_search(true, false);
}

#[test]
fn a_borrowed_dropped_pane_focus_request_yields_to_later_search_input() {
    assert_pending_pane_focus_yields_to_search(false, false);
}

#[test]
fn a_stable_unfocused_request_idles_and_a_new_selection_can_request_focus_again() {
    for owned in [true, false] {
        let mut search = SearchField::new();
        let (context, mut dockspace, mut pane, _) =
            presented_editor_with_ui(owned, |ui| search.ui(ui));
        dockspace.select_item_current(EDITOR).unwrap();
        let (mut output, receipt) = run_with_ui(
            owned,
            &context,
            &mut dockspace,
            &mut pane,
            1.0,
            Vec::new(),
            |ui| search.ui(ui),
        )
        .into_parts();
        output.textures_delta.clear();
        output.drop_without_applying_deltas();
        receipt.report(SurfacePresentationResult::Dropped);

        let search_position = search.bounds.unwrap().center();
        let mut repaint_delay = std::time::Duration::ZERO;
        for step in 0..10 {
            let (mut output, receipt) = run_with_ui(
                owned,
                &context,
                &mut dockspace,
                &mut pane,
                1.1 + f64::from(step) * 0.1,
                if step == 0 {
                    click(search_position).into()
                } else {
                    Vec::new()
                },
                |ui| search.ui(ui),
            )
            .into_parts();
            output.textures_delta.clear();
            repaint_delay = output.viewport_output[&egui::ViewportId::ROOT].repaint_delay;
            assert_eq!(context.memory(egui::Memory::focused), Some(search.target));
            output.drop_without_applying_deltas();
            receipt.report(SurfacePresentationResult::Dropped);
        }
        assert!(
            !repaint_delay.is_zero(),
            "an unchanged NotFocused observation must not request another immediate paint, owned={owned}"
        );

        dockspace.select_item_current(EDITOR).unwrap();
        let (mut output, receipt) = run_with_ui(
            owned,
            &context,
            &mut dockspace,
            &mut pane,
            3.0,
            Vec::new(),
            |ui| search.ui(ui),
        )
        .into_parts();
        output.textures_delta.clear();
        assert_eq!(context.memory(egui::Memory::focused), Some(pane.target));
        output.drop_without_applying_deltas();
        receipt.report(SurfacePresentationResult::Presented);
    }
}
