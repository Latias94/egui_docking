//! Real egui runs must publish only their intact terminal docking output.

use std::num::NonZeroUsize;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use dockspace::geometry::LogicalPoint;
use dockspace::model::{
    DockspaceLayout, DockspaceNode, DockspaceRootLayout, DockspaceSurfaceLayout, ItemId, RootId,
    SurfaceId,
};
use dockspace::runtime::{
    DockspaceContextMenuRequest, DockspaceReceiverRole, DockspaceTouchEvent, HostPointerInput,
    HostPointerRoute, PointerDeviceKind, PointerEventMetadata, SurfacePointerCapture,
    SurfacePointerEvent, SurfacePointerId, SurfacePointerReceiverFacts, SurfacePresentationResult,
    SurfaceUnavailableReason, SurfaceWork, SurfaceWorkDeferred,
};
use egui::accesskit::Role;
use egui::{
    Color32, Context, Event, FullOutput, PointerButton, Pos2, RawInput, Rect, RepaintCause, Shape,
    Ui,
};

use super::super::Dockspace;
use crate::DockspaceErrorKind;
use crate::pane::PaneView;
use crate::pass_settlement::{ProductPassSettlement, ReadyProductPassSettlementBatch};

const SURFACE: SurfaceId = SurfaceId::new(1);
const FIRST: ItemId = ItemId::new(1);
const SECOND: ItemId = ItemId::new(2);
const PANE_COLOR: Color32 = Color32::from_rgb(11, 97, 173);
const OUTPUT_HOOK_PANIC: &str = "owned run output hook panicked after pane painting";

#[derive(Default)]
struct RunPanes {
    calls: Vec<(ItemId, u64)>,
    bounds: Vec<Rect>,
}

impl PaneView for RunPanes {
    fn title(&self, item: ItemId) -> Option<egui::WidgetText> {
        match item {
            FIRST => Some("First".into()),
            SECOND => Some("Second".into()),
            _ => None,
        }
    }

    fn ui(&mut self, item: ItemId, ui: &mut Ui) -> crate::PaneResponse {
        self.calls.push((item, ui.ctx().cumulative_pass_nr()));
        self.bounds.push(ui.available_rect_before_wrap());
        ui.painter()
            .rect_filled(ui.available_rect_before_wrap().shrink(8.0), 0.0, PANE_COLOR);
        crate::PaneResponse::default()
    }
}

fn input(time: f64, events: Vec<Event>) -> RawInput {
    RawInput {
        screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(640.0, 480.0))),
        time: Some(time),
        events,
        ..RawInput::default()
    }
}

#[test]
fn measured_titles_are_painted_from_the_same_pass_and_refresh_next_pass() {
    struct ChangingTitles(std::cell::Cell<u32>);

    impl PaneView for ChangingTitles {
        fn title(&self, item: ItemId) -> Option<egui::WidgetText> {
            let revision = self.0.get() + 1;
            self.0.set(revision);
            Some(format!("Title {}:{revision}", item.get()).into())
        }

        fn ui(&mut self, _: ItemId, _: &mut Ui) -> crate::PaneResponse {
            crate::PaneResponse::default()
        }
    }

    for owned in [true, false] {
        let context = Context::default();
        context.enable_accesskit();
        context.options_mut(|options| options.max_passes = NonZeroUsize::new(1).unwrap());
        let layout = DockspaceLayout::new([DockspaceSurfaceLayout::new(
            SURFACE,
            DockspaceRootLayout::new(RootId::new(1), DockspaceNode::central_tabs([FIRST, SECOND])),
        )])
        .unwrap();
        let mut dockspace = Dockspace::builder(("pass-title-snapshot", owned), layout)
            .build()
            .unwrap();
        let mut panes = ChangingTitles(std::cell::Cell::new(0));
        for pass in 0..2 {
            let raw = input(f64::from(pass), Vec::new());
            let output = if owned {
                dockspace
                    .run_single_surface(&context, raw, SURFACE, &mut panes)
                    .unwrap()
                    .0
            } else {
                crate::run_ui(&context, raw, |ui| {
                    dockspace
                        .show_single_surface(SURFACE, ui, &mut panes)
                        .unwrap();
                })
            };
            let (mut output, receipt) = output.into_parts();
            output.textures_delta.clear();
            let mut labels = output
                .platform_output
                .accesskit_update
                .as_ref()
                .unwrap()
                .nodes
                .iter()
                .filter(|(_, node)| node.role() == Role::Tab)
                .map(|(_, node)| node.label().unwrap().to_owned())
                .collect::<Vec<_>>();
            // AccessKit update nodes are a set; their vector order is unspecified.
            labels.sort();
            assert_eq!(
                labels,
                [
                    format!("Title 1:{}", pass * 2 + 1),
                    format!("Title 2:{}", pass * 2 + 2),
                ],
                "measurement and painting must use one title snapshot per pass, owned={owned}"
            );
            output.drop_without_applying_deltas();
            receipt.report(SurfacePresentationResult::Presented);
        }
    }
}

fn press(position: Pos2) -> Vec<Event> {
    vec![
        Event::PointerMoved(position),
        Event::PointerButton {
            pos: position,
            button: PointerButton::Primary,
            pressed: true,
            modifiers: egui::Modifiers::NONE,
        },
    ]
}

#[allow(
    clippy::cast_possible_truncation,
    reason = "the product renderer supplies finite bounds inside the 640x480 viewport"
)]
fn inactive_tab_center(output: &FullOutput) -> Pos2 {
    let bounds = output
        .platform_output
        .accesskit_update
        .as_ref()
        .expect("the product renderer exports its actual tab geometry")
        .nodes
        .iter()
        .find_map(|(_, node)| {
            (node.role() == Role::Tab && node.label() == Some("Second"))
                .then(|| node.bounds().expect("the inactive tab has real bounds"))
        })
        .expect("the real inactive tab was painted");
    Rect::from_min_max(
        Pos2::new(bounds.x0 as f32, bounds.y0 as f32),
        Pos2::new(bounds.x1 as f32, bounds.y1 as f32),
    )
    .center()
}

fn presented_tabs(id: &'static str) -> (Context, Dockspace, RunPanes, Pos2) {
    let context = Context::default();
    context.enable_accesskit();
    context.options_mut(|options| options.max_passes = NonZeroUsize::new(1).unwrap());
    let layout = DockspaceLayout::new([DockspaceSurfaceLayout::new(
        SURFACE,
        DockspaceRootLayout::new(RootId::new(1), DockspaceNode::central_tabs([FIRST, SECOND])),
    )])
    .unwrap();
    let mut dockspace = Dockspace::builder(id, layout).build().unwrap();
    dockspace
        .session
        .configure_output_capacity(NonZeroUsize::new(1).unwrap())
        .unwrap();
    let mut panes = RunPanes::default();

    // Publish P0 before reading geometry. Consecutive real outputs must then
    // agree, so the next raw press uses the renderer's stable registrations.
    for time in [0.0, 0.1] {
        let (output, _) = dockspace
            .run_single_surface(&context, input(time, Vec::new()), SURFACE, &mut panes)
            .unwrap();
        let (output, receipt) = output.into_parts();
        output.drop_without_applying_deltas();
        receipt.report(SurfacePresentationResult::Presented);
    }
    assert!(dockspace.session.presented_surface(SURFACE).is_some());
    let mut position = None;
    for time in [0.2, 0.3] {
        let (output, _) = dockspace
            .run_single_surface(&context, input(time, Vec::new()), SURFACE, &mut panes)
            .unwrap();
        let (output, receipt) = output.into_parts();
        let current = inactive_tab_center(&output);
        if let Some(previous) = position {
            assert_eq!(current, previous, "P0 tab geometry must be stable");
        }
        position = Some(current);
        output.drop_without_applying_deltas();
        receipt.report(SurfacePresentationResult::Presented);
    }
    let position = position.unwrap();
    let hit = dockspace
        .session
        .host_pointer_hit(
            SURFACE,
            LogicalPoint::new(f64::from(position.x), f64::from(position.y)).unwrap(),
        )
        .and_then(|hit| hit.delivery())
        .expect("the stable rendered position hits a real presented receiver");
    assert_eq!(hit.descriptor().role(), DockspaceReceiverRole::TabBody);
    assert!(dockspace.view().item(FIRST).unwrap().is_selected());
    assert!(!dockspace.view().item(SECOND).unwrap().is_selected());
    assert!(!dockspace.session.has_active_gesture());
    assert!(!dockspace.measurement_deferred);
    panes.calls.clear();
    (context, dockspace, panes, position)
}

#[test]
fn application_panels_leave_their_remaining_rectangle_for_docking_and_pane_content() {
    let (context, mut dockspace, mut panes, _) = presented_tabs("owned-run-application-panels");
    let before = dockspace.version();
    panes.bounds.clear();
    let mut panel_bounds = None;

    let (output, response) = dockspace
        .run_single_surface_with_ui(
            &context,
            input(1.0, Vec::new()),
            SURFACE,
            &mut panes,
            |ui| {
                let top = egui::Panel::top("owned-run-toolbar")
                    .show(ui, |ui| {
                        ui.set_min_height(36.0);
                        ui.label("Application toolbar");
                    })
                    .response
                    .rect;
                let bottom = egui::Panel::bottom("owned-run-status")
                    .show(ui, |ui| {
                        ui.set_min_height(24.0);
                        ui.label("Application status");
                    })
                    .response
                    .rect;
                panel_bounds = Some((top, bottom, ui.available_rect_before_wrap()));
            },
        )
        .expect("application panels must coexist with exact owned docking output");
    let (output, receipt) = output.into_parts();
    let (top, bottom, docking) = panel_bounds.unwrap();
    assert!(top.is_positive() && bottom.is_positive() && docking.is_positive());
    assert!(docking.top() >= top.bottom());
    assert!(docking.bottom() <= bottom.top());
    let workspace = output
        .platform_output
        .accesskit_update
        .as_ref()
        .unwrap()
        .nodes
        .iter()
        .find_map(|(_, node)| {
            (node.role() == Role::Pane && node.label() == Some("Dockspace workspace"))
                .then(|| node.bounds().unwrap())
        })
        .expect("the core workspace exposes its actual compiled bounds");
    assert_eq!(workspace.x0, f64::from(docking.left()));
    assert_eq!(workspace.y0, f64::from(docking.top()));
    assert_eq!(workspace.x1, f64::from(docking.right()));
    assert_eq!(workspace.y1, f64::from(docking.bottom()));
    assert_eq!(panes.calls.len(), 1);
    assert_eq!(panes.bounds.len(), 1);
    let pane = panes.bounds[0];
    assert!(pane.is_positive() && docking.contains_rect(pane));
    assert!(pane.top() >= top.bottom());
    assert!(pane.bottom() <= bottom.top());
    assert_eq!(dockspace.version(), before);
    assert!(!response.mutation().workspace_changed());
    assert_eq!(
        response.presentation_settlement_capability(),
        crate::DockspaceCapability::Supported
    );
    output.drop_without_applying_deltas();
    receipt.report(SurfacePresentationResult::Presented);
}

#[derive(Default)]
struct EditablePanes([String; 2]);

impl PaneView for EditablePanes {
    fn title(&self, item: ItemId) -> Option<egui::WidgetText> {
        match item {
            FIRST => Some("First".into()),
            SECOND => Some("Second".into()),
            _ => None,
        }
    }

    fn ui(&mut self, item: ItemId, ui: &mut Ui) -> crate::PaneResponse {
        let target = self.focus_target(item).unwrap();
        let text = &mut self.0[usize::from(item == SECOND)];
        let response = ui.add(egui::TextEdit::multiline(text).id(target));
        crate::PaneResponse::from(&response)
    }

    fn focus_target(&self, item: ItemId) -> Option<egui::Id> {
        Some(egui::Id::unique(("navigation-editable-pane", item.get())))
    }
}

#[test]
fn owned_run_output_backpressure_preserves_text_paste_and_ime_commit() {
    #[derive(Default)]
    struct Document {
        text: String,
        calls: usize,
    }

    impl PaneView for Document {
        fn title(&self, _: ItemId) -> Option<egui::WidgetText> {
            Some("Document".into())
        }

        fn ui(&mut self, item: ItemId, ui: &mut Ui) -> crate::PaneResponse {
            self.calls += 1;
            let target = self.focus_target(item).unwrap();
            crate::PaneResponse::from(&ui.add(egui::TextEdit::multiline(&mut self.text).id(target)))
        }

        fn focus_target(&self, _: ItemId) -> Option<egui::Id> {
            Some(egui::Id::unique("backpressured-document"))
        }
    }

    let context = Context::default();
    context.options_mut(|options| options.max_passes = NonZeroUsize::new(1).unwrap());
    let layout = DockspaceLayout::new([DockspaceSurfaceLayout::new(
        SURFACE,
        DockspaceRootLayout::new(RootId::new(1), DockspaceNode::central_tabs([FIRST])),
    )])
    .unwrap();
    let mut dockspace = Dockspace::builder("owned-backpressure-document", layout)
        .build()
        .unwrap();
    let mut document = Document::default();
    for step in 0..4 {
        let (output, _) = dockspace
            .run_single_surface(
                &context,
                input(f64::from(step) * 0.1, Vec::new()),
                SURFACE,
                &mut document,
            )
            .unwrap();
        let (output, receipt) = output.into_parts();
        output.drop_without_applying_deltas();
        receipt.report(SurfacePresentationResult::Presented);
    }
    let target = document.focus_target(FIRST).unwrap();
    context.memory_mut(|memory| memory.request_focus(target));
    let (output, _) = dockspace
        .run_single_surface(
            &context,
            input(0.5, vec![Event::Text("before|".into())]),
            SURFACE,
            &mut document,
        )
        .unwrap();
    let (output, receipt) = output.into_parts();
    assert_eq!(
        document.text, "before|",
        "the real editor accepts text before backpressure"
    );
    assert!(
        output.platform_output.ime.is_some(),
        "the real focused editor owns IME output"
    );
    output.drop_without_applying_deltas();
    receipt.report(SurfacePresentationResult::Presented);

    // Exercise the public facade's default 64 slots without changing its private session.
    // Retaining receipts is explicitly supported by DockspaceOutputReceipt's contract.
    let mut outstanding = Vec::new();
    for step in 0..64 {
        let (output, _) = dockspace
            .run_single_surface(
                &context,
                input(1.0 + f64::from(step) * 0.01, Vec::new()),
                SURFACE,
                &mut document,
            )
            .unwrap();
        let (output, receipt) = output.into_parts();
        output.drop_without_applying_deltas();
        outstanding.push(receipt);
    }
    assert_eq!(context.memory(egui::Memory::focused), Some(target));
    let calls_before_input = document.calls;
    let pass_before_input = context.cumulative_pass_nr();
    let frame_before_input = context.cumulative_frame_nr();
    let version_before_input = dockspace.version();
    let mut composition_calls = 0;
    let mut original = input(
        2.0,
        vec![
            Event::Text("text|".into()),
            Event::Paste("paste|".into()),
            Event::Ime(egui::ImeEvent::Commit("ime".into())),
        ],
    );
    original.predicted_dt = 1.0 / 120.0;
    original.max_texture_side = Some(4096);
    original.system_theme = Some(egui::Theme::Light);
    original
        .viewports
        .get_mut(&original.viewport_id)
        .unwrap()
        .title = Some("Deferred document input".into());
    let mut deferred = original.clone();
    for attempt in 0..2 {
        let error = dockspace
            .run_single_surface_with_ui(&context, deferred, SURFACE, &mut document, |_| {
                composition_calls += 1
            })
            .err()
            .expect("capacity must defer input before entering egui");
        assert_eq!(
            error.kind(),
            DockspaceErrorKind::OutputBackpressure,
            "retry {attempt} must preserve its recoverable error category"
        );
        deferred = error
            .into_deferred_input()
            .expect("backpressure returns the unconsumed input batch");
        assert_eq!(
            deferred.events, original.events,
            "retry {attempt} preserves text, paste and IME event order"
        );
        assert_eq!(deferred.viewports, original.viewports);
        assert_eq!(
            (
                deferred.viewport_id,
                deferred.time,
                deferred.screen_rect,
                deferred.predicted_dt,
                deferred.max_texture_side,
                deferred.focused,
                deferred.system_theme,
                deferred.safe_area_insets
            ),
            (
                original.viewport_id,
                original.time,
                original.screen_rect,
                original.predicted_dt,
                original.max_texture_side,
                original.focused,
                original.system_theme,
                original.safe_area_insets
            ),
            "retry {attempt} preserves the original input metadata",
        );
        assert_eq!(deferred.hovered_files, original.hovered_files);
        assert_eq!(deferred.dropped_files.len(), original.dropped_files.len());
        assert_eq!(context.cumulative_pass_nr(), pass_before_input);
        assert_eq!(context.cumulative_frame_nr(), frame_before_input);
        assert_eq!(composition_calls, 0);
        assert_eq!(document.calls, calls_before_input);
        assert_eq!(document.text, "before|");
        assert_eq!(dockspace.version(), version_before_input);
        assert_eq!(context.memory(egui::Memory::focused), Some(target));
    }
    for receipt in outstanding {
        receipt.report(SurfacePresentationResult::Presented);
    }
    let (output, _) = dockspace
        .run_single_surface_with_ui(&context, deferred, SURFACE, &mut document, |_| {
            composition_calls += 1
        })
        .expect("settling earlier outputs admits the exact deferred batch");
    assert_eq!(composition_calls, 1);
    assert_eq!(document.calls, calls_before_input + 1);
    assert_eq!(document.text, "before|text|paste|ime");
    let (output, receipt) = output.into_parts();
    assert!(
        output.platform_output.ime.is_some(),
        "the recovered content run keeps its focused editor's IME"
    );
    output.drop_without_applying_deltas();
    receipt.report(SurfacePresentationResult::Presented);
    for time in [3.0, 4.0] {
        let (output, _) = dockspace
            .run_single_surface(&context, input(time, Vec::new()), SURFACE, &mut document)
            .unwrap();
        let (output, receipt) = output.into_parts();
        output.drop_without_applying_deltas();
        receipt.report(SurfacePresentationResult::Presented);
    }
    assert_eq!(
        document.text, "before|text|paste|ime",
        "text, paste and IME input must survive backpressure exactly once in original order"
    );
}

#[test]
fn keyboard_navigation_keeps_tab_focus_after_editable_pane_selection_commits() {
    let (context, mut dockspace, _, _) = presented_tabs("editable-tab-navigation");
    let mut panes = EditablePanes::default();
    let mut first_tab = None;
    let mut first_node = None;
    // Settle the initial activation and its content-focus observation before navigation.
    for step in 0..5 {
        let events = if step == 1 {
            vec![Event::AccessKitActionRequest(
                egui::accesskit::ActionRequest {
                    action: egui::accesskit::Action::Focus,
                    target_tree: egui::accesskit::TreeId::ROOT,
                    target_node: first_node.unwrap(),
                    data: None,
                },
            )]
        } else {
            Vec::new()
        };
        let (output, _) = dockspace
            .run_single_surface(
                &context,
                input(0.4 + f64::from(step) * 0.1, events),
                SURFACE,
                &mut panes,
            )
            .unwrap();
        let (output, receipt) = output.into_parts();
        let tabs = &output
            .platform_output
            .accesskit_update
            .as_ref()
            .unwrap()
            .nodes;
        let first = tabs
            .iter()
            .find(|(_, node)| node.role() == Role::Tab && node.label() == Some("First"))
            .unwrap()
            .0;
        first_node = Some(first);
        if let Some(focused) = context.memory(egui::Memory::focused)
            && focused.value() == first.0
        {
            first_tab = Some(focused);
        }
        output.drop_without_applying_deltas();
        receipt.report(SurfacePresentationResult::Presented);
    }
    let first_tab = first_tab.expect("the real tab must accept accessible framework focus");
    context.memory_mut(|memory| memory.request_focus(first_tab));

    for (step, key, item, title) in [
        (0, egui::Key::End, SECOND, "Second"),
        (1, egui::Key::Home, FIRST, "First"),
        (2, egui::Key::ArrowRight, SECOND, "Second"),
        (3, egui::Key::ArrowLeft, FIRST, "First"),
    ] {
        for pass in 0..3 {
            let events = if pass == 0 {
                vec![Event::Key {
                    key,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: egui::Modifiers::NONE,
                }]
            } else {
                Vec::new()
            };
            let (output, _) = dockspace
                .run_single_surface(
                    &context,
                    input(1.0 + f64::from(step * 3 + pass) * 0.1, events),
                    SURFACE,
                    &mut panes,
                )
                .unwrap();
            let (output, receipt) = output.into_parts();
            let target = output
                .platform_output
                .accesskit_update
                .as_ref()
                .unwrap()
                .nodes
                .iter()
                .find(|(_, node)| node.role() == Role::Tab && node.label() == Some(title))
                .unwrap()
                .0;
            assert_eq!(
                context.memory(egui::Memory::focused).map(|id| id.value()),
                Some(target.0),
                "{key:?} must retain {title} tab focus at step {step}, pass {pass}",
            );
            if pass > 0 {
                assert!(dockspace.view().item(item).unwrap().is_selected());
            }
            output.drop_without_applying_deltas();
            receipt.report(SurfacePresentationResult::Presented);
        }
    }
}

#[test]
fn activating_an_overflow_menu_row_focuses_its_editable_content() {
    let context = Context::default();
    context.enable_accesskit();
    let layout = DockspaceLayout::new([DockspaceSurfaceLayout::new(
        SURFACE,
        DockspaceRootLayout::new(RootId::new(1), DockspaceNode::central_tabs([FIRST, SECOND])),
    )])
    .unwrap();
    let mut dockspace = Dockspace::builder("editable-overflow-selection", layout)
        .style(crate::DockStyle {
            tab_min_width: 400.0,
            tab_max_width: 400.0,
            ..Default::default()
        })
        .build()
        .unwrap();
    let mut panes = EditablePanes::default();
    let mut menu_button = None;
    for pass in 0..24 {
        let phase = pass % 12;
        let events = match phase {
            4 => vec![Event::AccessKitActionRequest(
                egui::accesskit::ActionRequest {
                    action: egui::accesskit::Action::Click,
                    target_tree: egui::accesskit::TreeId::ROOT,
                    target_node: menu_button.unwrap(),
                    data: None,
                },
            )],
            6 | 8 => vec![Event::Key {
                key: if phase == 6 {
                    egui::Key::End
                } else if pass < 12 {
                    egui::Key::Enter
                } else {
                    egui::Key::Space
                },
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            }],
            11 => vec![Event::Text("typed after menu selection".to_owned())],
            _ => Vec::new(),
        };
        let (output, _) = dockspace
            .run_single_surface(
                &context,
                input(f64::from(pass) * 0.1, events),
                SURFACE,
                &mut panes,
            )
            .unwrap();
        let (output, receipt) = output.into_parts();
        if phase == 3 {
            menu_button = output
                .platform_output
                .accesskit_update
                .as_ref()
                .unwrap()
                .nodes
                .iter()
                .find(|(_, node)| node.label() == Some("Show hidden tabs"))
                .map(|(id, _)| *id);
            assert!(menu_button.is_some());
        }
        if phase == 10 {
            assert!(dockspace.view().item(SECOND).unwrap().is_selected());
            assert_eq!(
                context.memory(egui::Memory::focused),
                panes.focus_target(SECOND),
                "activating an overflow row must enter its content after deferred selection settles, pass {pass}"
            );
        }
        output.drop_without_applying_deltas();
        receipt.report(SurfacePresentationResult::Presented);
    }
    assert_eq!(
        panes.0[1],
        "typed after menu selectiontyped after menu selection"
    );
    assert!(panes.0[0].is_empty());
}

#[test]
fn escaping_an_overflow_menu_returns_focus_to_its_button_for_keyboard_reopening() {
    let context = Context::default();
    context.enable_accesskit();
    let layout = DockspaceLayout::new([DockspaceSurfaceLayout::new(
        SURFACE,
        DockspaceRootLayout::new(RootId::new(1), DockspaceNode::central_tabs([FIRST, SECOND])),
    )])
    .unwrap();
    let mut dockspace = Dockspace::builder("overflow-menu-keyboard-cancellation", layout)
        .style(crate::DockStyle {
            tab_min_width: 400.0,
            tab_max_width: 400.0,
            ..Default::default()
        })
        .build()
        .unwrap();
    let mut panes = EditablePanes::default();
    let mut menu_button = None;
    for pass in 0..24 {
        let phase = pass % 12;
        if pass == 8 {
            context.options_mut(|options| options.max_passes = NonZeroUsize::new(3).unwrap());
            context.add_plugin(LateDiscard { remaining: 2 });
        }
        let events = if pass == 4 {
            vec![Event::AccessKitActionRequest(
                egui::accesskit::ActionRequest {
                    action: egui::accesskit::Action::Click,
                    target_tree: egui::accesskit::TreeId::ROOT,
                    target_node: menu_button.unwrap(),
                    data: None,
                },
            )]
        } else if matches!(phase, 4 | 6 | 8) {
            vec![Event::Key {
                key: match phase {
                    4 => egui::Key::Enter,
                    6 => egui::Key::End,
                    _ => egui::Key::Escape,
                },
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            }]
        } else {
            Vec::new()
        };
        let (output, _) = dockspace
            .run_single_surface(
                &context,
                input(f64::from(pass) * 0.1, events),
                SURFACE,
                &mut panes,
            )
            .unwrap();
        let (output, receipt) = output.into_parts();
        if pass == 8 {
            assert_eq!(output.platform_output.num_completed_passes, 3);
        }
        let nodes = &output
            .platform_output
            .accesskit_update
            .as_ref()
            .unwrap()
            .nodes;
        if pass == 3 {
            menu_button = nodes
                .iter()
                .find(|(_, node)| node.label() == Some("Show hidden tabs"))
                .map(|(id, _)| *id);
        }
        let menu_visible = nodes.iter().any(|(_, node)| node.role() == Role::Menu);
        if phase == 7 {
            assert!(menu_visible, "the menu must open in cycle {}", pass / 12);
        }
        if phase == 10 {
            assert!(!menu_visible, "Escape must dismiss the menu");
            assert!(dockspace.view().item(FIRST).unwrap().is_selected());
            assert_eq!(
                context.memory(egui::Memory::focused).map(|id| id.value()),
                Some(menu_button.unwrap().0),
                "Escape must restore its button focus for reopening, pass {pass}"
            );
        }
        output.drop_without_applying_deltas();
        receipt.report(SurfacePresentationResult::Presented);
    }
    assert!(panes.0.iter().all(String::is_empty));
}

#[test]
#[allow(
    clippy::too_many_lines,
    reason = "the regression opens a real menu, transfers focus to a modal and verifies text input and selection"
)]
fn an_overflow_menu_yields_focus_and_keyboard_activation_to_an_application_modal() {
    let context = Context::default();
    context.enable_accesskit();
    let layout = DockspaceLayout::new([DockspaceSurfaceLayout::new(
        SURFACE,
        DockspaceRootLayout::new(RootId::new(1), DockspaceNode::central_tabs([FIRST, SECOND])),
    )])
    .unwrap();
    let mut dockspace = Dockspace::builder("overflow-menu-application-modal", layout)
        .style(crate::DockStyle {
            tab_min_width: 400.0,
            tab_max_width: 400.0,
            ..Default::default()
        })
        .build()
        .unwrap();
    let mut panes = EditablePanes(["first content".to_owned(), "second content".to_owned()]);
    let mut menu_button = None;
    let modal_editor = egui::Id::unique("overflow-menu-modal-editor");
    let mut modal_focus_requested = false;
    let mut modal_text = String::new();
    for pass in 0..16 {
        let events = match pass {
            4 => vec![Event::AccessKitActionRequest(
                egui::accesskit::ActionRequest {
                    action: egui::accesskit::Action::Click,
                    target_tree: egui::accesskit::TreeId::ROOT,
                    target_node: menu_button.unwrap(),
                    data: None,
                },
            )],
            6 | 12 => vec![Event::Key {
                key: if pass == 6 {
                    egui::Key::End
                } else {
                    egui::Key::Enter
                },
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            }],
            10 => vec![Event::Text("modal".to_owned())],
            14 => vec![Event::Text(" typing".to_owned())],
            _ => Vec::new(),
        };
        let (output, _) = dockspace
            .run_single_surface_with_ui(
                &context,
                input(f64::from(pass) * 0.1, events),
                SURFACE,
                &mut panes,
                |ui| {
                    if pass >= 8 {
                        egui::Modal::new(egui::Id::unique("overflow-menu-modal")).show(
                            ui.ctx(),
                            |ui| {
                                // Leave Enter to the modal layer instead of consuming it here.
                                let response = ui.add(
                                    egui::TextEdit::singleline(&mut modal_text)
                                        .id(modal_editor)
                                        .return_key(None),
                                );
                                if !modal_focus_requested && !ui.is_sizing_pass() {
                                    response.request_focus();
                                    modal_focus_requested = true;
                                }
                            },
                        );
                    }
                },
            )
            .unwrap();
        let (mut output, receipt) = output.into_parts();
        output.textures_delta.clear();
        let nodes = &output
            .platform_output
            .accesskit_update
            .as_ref()
            .unwrap()
            .nodes;
        if pass == 3 {
            menu_button = nodes
                .iter()
                .find(|(_, node)| node.label() == Some("Show hidden tabs"))
                .map(|(id, _)| *id);
        }
        if pass == 7 {
            let (row_id, _) = nodes
                .iter()
                .find(|(_, node)| node.role() == Role::MenuItem && node.label() == Some("Second"))
                .expect("End must focus a real unselected overflow row before the modal opens");
            assert_eq!(
                context.memory(egui::Memory::focused).map(|id| id.value()),
                Some(row_id.0)
            );
        }
        if pass >= 9 {
            assert_eq!(
                context.memory(egui::Memory::focused),
                Some(modal_editor),
                "the modal editor must retain focus without another request, pass {pass}"
            );
        }
        assert!(dockspace.view().item(FIRST).unwrap().is_selected());
        assert!(!dockspace.view().item(SECOND).unwrap().is_selected());
        output.drop_without_applying_deltas();
        receipt.report(SurfacePresentationResult::Presented);
    }
    assert_eq!(modal_text, "modal typing");
    assert_eq!(panes.0, ["first content", "second content"]);
}

#[test]
fn an_application_modal_blocks_accessible_tab_activation() {
    for action in [
        egui::accesskit::Action::Click,
        egui::accesskit::Action::Focus,
    ] {
        let (context, mut dockspace, _, _) = presented_tabs("modal-accessible-tab-activation");
        let mut panes = EditablePanes(["first content".to_owned(), "second content".to_owned()]);
        let mut second_tab = None;
        let modal_editor = egui::Id::unique("accessible-tab-modal-editor");
        let mut modal_focus_requested = false;
        let mut modal_text = String::new();
        for pass in 0..10 {
            let events = match pass {
                6 => vec![Event::AccessKitActionRequest(
                    egui::accesskit::ActionRequest {
                        action,
                        target_tree: egui::accesskit::TreeId::ROOT,
                        target_node: second_tab.unwrap(),
                        data: None,
                    },
                )],
                8 => vec![Event::Text("modal input".to_owned())],
                _ => Vec::new(),
            };
            let (output, _) = dockspace
                .run_single_surface_with_ui(
                    &context,
                    input(1.0 + f64::from(pass) * 0.1, events),
                    SURFACE,
                    &mut panes,
                    |ui| {
                        if pass >= 2 {
                            egui::Modal::new(egui::Id::unique("accessible-tab-modal")).show(
                                ui.ctx(),
                                |ui| {
                                    let response = ui.add(
                                        egui::TextEdit::singleline(&mut modal_text)
                                            .id(modal_editor),
                                    );
                                    if !modal_focus_requested && !ui.is_sizing_pass() {
                                        response.request_focus();
                                        modal_focus_requested = true;
                                    }
                                },
                            );
                        }
                    },
                )
                .unwrap();
            let (mut output, receipt) = output.into_parts();
            output.textures_delta.clear();
            if pass == 1 {
                let (id, node) = output
                    .platform_output
                    .accesskit_update
                    .as_ref()
                    .unwrap()
                    .nodes
                    .iter()
                    .find(|(_, node)| node.role() == Role::Tab && node.label() == Some("Second"))
                    .expect("the inactive tab must expose a real accessible activation target");
                assert!(node.supports_action(action));
                second_tab = Some(*id);
            }
            assert!(
                dockspace.view().item(FIRST).unwrap().is_selected(),
                "{action:?} must not select a tab behind the modal, pass {pass}"
            );
            assert!(!dockspace.view().item(SECOND).unwrap().is_selected());
            if pass == 5 || pass >= 7 {
                assert_eq!(
                    context.memory(egui::Memory::focused),
                    Some(modal_editor),
                    "the modal must retain keyboard focus across {action:?}, pass {pass}"
                );
            }
            output.drop_without_applying_deltas();
            receipt.report(SurfacePresentationResult::Presented);
        }
        assert_eq!(modal_text, "modal input");
        assert_eq!(panes.0, ["first content", "second content"]);
    }
}

#[test]
fn a_pending_pane_focus_request_does_not_steal_focus_from_an_application_modal() {
    let (context, mut dockspace, _, _) = presented_tabs("modal-pending-pane-focus");
    let mut panes = EditablePanes(["first content".to_owned(), "second content".to_owned()]);
    let modal_editor = egui::Id::unique("pending-pane-modal-editor");
    let mut modal_focus_requested = false;
    let mut modal_text = String::new();
    for pass in 0..8 {
        if pass == 4 {
            // Selecting the current item still publishes a real content-focus request.
            dockspace.select_item_current(FIRST).unwrap();
        }
        let events = if pass == 6 {
            vec![Event::Text("modal input".to_owned())]
        } else {
            Vec::new()
        };
        let (output, _) = dockspace
            .run_single_surface_with_ui(
                &context,
                input(1.0 + f64::from(pass) * 0.1, events),
                SURFACE,
                &mut panes,
                |ui| {
                    egui::Modal::new(egui::Id::unique("pending-pane-modal")).show(ui.ctx(), |ui| {
                        let response =
                            ui.add(egui::TextEdit::singleline(&mut modal_text).id(modal_editor));
                        if !modal_focus_requested && !ui.is_sizing_pass() {
                            response.request_focus();
                            modal_focus_requested = true;
                        }
                    });
                },
            )
            .unwrap();
        let (mut output, receipt) = output.into_parts();
        output.textures_delta.clear();
        if pass >= 3 {
            assert_eq!(
                context.memory(egui::Memory::focused),
                Some(modal_editor),
                "the modal must retain focus before and after the pane request, pass {pass}"
            );
        }
        assert!(dockspace.view().item(FIRST).unwrap().is_selected());
        assert!(!dockspace.view().item(SECOND).unwrap().is_selected());
        output.drop_without_applying_deltas();
        receipt.report(SurfacePresentationResult::Presented);
    }
    assert_eq!(modal_text, "modal input");
    assert_eq!(panes.0, ["first content", "second content"]);
}

#[test]
fn a_tab_press_does_not_replace_a_new_application_modals_focus_after_publication() {
    let (context, mut dockspace, _, position) = presented_tabs("modal-deferred-tab-focus");
    context.options_mut(|options| options.max_passes = NonZeroUsize::new(2).unwrap());
    // Let the new modal finish sizing inside the same run as the qualified tab press.
    context.add_plugin(LateDiscard { remaining: 1 });
    let mut panes = EditablePanes(["first content".to_owned(), "second content".to_owned()]);
    let modal_editor = egui::Id::unique("deferred-tab-modal-editor");
    let mut modal_focus_requested = false;
    let mut modal_text = String::new();
    for step in 0..5 {
        // Keep the press held: releasing outside the modal legitimately blurs TextEdit.
        let events = match step {
            0 => press(position),
            3 => vec![Event::Text("modal input".to_owned())],
            _ => Vec::new(),
        };
        let (output, response) = dockspace
            .run_single_surface_with_ui(
                &context,
                input(1.0 + f64::from(step) * 0.1, events),
                SURFACE,
                &mut panes,
                |ui| {
                    egui::Modal::new(egui::Id::unique("deferred-tab-modal")).show(ui.ctx(), |ui| {
                        let response =
                            ui.add(egui::TextEdit::singleline(&mut modal_text).id(modal_editor));
                        if !modal_focus_requested && !ui.is_sizing_pass() {
                            response.request_focus();
                            modal_focus_requested = true;
                        }
                        if modal_focus_requested {
                            assert_eq!(
                                ui.memory(egui::Memory::focused),
                                Some(modal_editor),
                                "the real modal editor must own focus before docking paints"
                            );
                        }
                    });
                },
            )
            .unwrap();
        let (mut output, receipt) = output.into_parts();
        output.textures_delta.clear();
        if step == 0 {
            assert_eq!(output.platform_output.num_completed_passes, 2);
            assert!(modal_focus_requested);
            assert!(
                response.mutation().workspace_changed(),
                "the original presented tab press must actually reach the core"
            );
        }
        assert!(dockspace.view().item(SECOND).unwrap().is_selected());
        assert!(!dockspace.view().item(FIRST).unwrap().is_selected());
        assert_eq!(
            context.memory(egui::Memory::focused),
            Some(modal_editor),
            "publishing the tab press must preserve the newer modal focus, step {step}"
        );
        output.drop_without_applying_deltas();
        receipt.report(SurfacePresentationResult::Presented);
    }
    assert_eq!(modal_text, "modal input");
    assert_eq!(panes.0, ["first content", "second content"]);
}

#[test]
fn a_tab_press_does_not_focus_a_tab_disabled_by_the_terminal_pass() {
    let (context, mut dockspace, _, position) = presented_tabs("terminal-disabled-tab-focus");
    context.options_mut(|options| options.max_passes = NonZeroUsize::new(2).unwrap());
    context.add_plugin(LateDiscard { remaining: 1 });
    let mut panes = EditablePanes(["first content".to_owned(), "second content".to_owned()]);
    let editor = egui::Id::unique("terminal-disabled-tab-application-editor");
    let mut editor_focus_requested = false;
    let mut text = String::new();
    for step in 0..4 {
        let events = match step {
            0 => press(position),
            2 => vec![Event::Text("application input".to_owned())],
            _ => Vec::new(),
        };
        let (output, response) = dockspace
            .run_single_surface_with_ui(
                &context,
                input(1.0 + f64::from(step) * 0.1, events),
                SURFACE,
                &mut panes,
                |ui| {
                    if step > 0 || ui.ctx().current_pass_index() > 0 {
                        let response = ui.add(egui::TextEdit::singleline(&mut text).id(editor));
                        if !editor_focus_requested {
                            response.request_focus();
                            editor_focus_requested = true;
                        }
                        assert!(response.enabled());
                        // Disable only the UI built after the application editor.
                        ui.disable();
                    }
                },
            )
            .unwrap();
        let (mut output, receipt) = output.into_parts();
        output.textures_delta.clear();
        if step == 0 {
            assert_eq!(output.platform_output.num_completed_passes, 2);
            assert!(response.mutation().workspace_changed());
            assert!(editor_focus_requested);
        }
        let (_, tab) = output
            .platform_output
            .accesskit_update
            .as_ref()
            .unwrap()
            .nodes
            .iter()
            .find(|(_, node)| node.role() == Role::Tab && node.label() == Some("Second"))
            .expect("the terminal docking scope must still render the selected tab");
        assert!(
            tab.is_disabled(),
            "the docking scope must inherit ui.disable()"
        );
        assert!(dockspace.view().item(SECOND).unwrap().is_selected());
        assert!(!dockspace.view().item(FIRST).unwrap().is_selected());
        assert_eq!(
            context.memory(egui::Memory::focused),
            Some(editor),
            "the disabled terminal tab must not replace application focus, step {step}"
        );
        output.drop_without_applying_deltas();
        receipt.report(SurfacePresentationResult::Presented);
    }
    assert_eq!(text, "application input");
    assert_eq!(panes.0, ["first content", "second content"]);
}

#[test]
fn escaping_a_presentation_menu_returns_focus_to_its_button_for_keyboard_reopening() {
    let (context, mut dockspace, _, _) = presented_tabs("presentation-menu-keyboard-cancellation");
    let mut panes = EditablePanes(["first content".to_owned(), "second content".to_owned()]);
    let mut menu_button = None;
    for pass in 0..24 {
        let phase = pass % 12;
        let events = if pass == 2 {
            vec![Event::AccessKitActionRequest(
                egui::accesskit::ActionRequest {
                    action: egui::accesskit::Action::Focus,
                    target_tree: egui::accesskit::TreeId::ROOT,
                    target_node: menu_button.unwrap(),
                    data: None,
                },
            )]
        } else if matches!(phase, 4 | 6 | 8) {
            vec![Event::Key {
                key: match phase {
                    4 => egui::Key::Enter,
                    6 => egui::Key::ArrowDown,
                    _ => egui::Key::Escape,
                },
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            }]
        } else {
            Vec::new()
        };
        let (output, _) = dockspace
            .run_single_surface(
                &context,
                input(1.0 + f64::from(pass) * 0.1, events),
                SURFACE,
                &mut panes,
            )
            .unwrap();
        let (mut output, receipt) = output.into_parts();
        output.textures_delta.clear();
        let nodes = &output
            .platform_output
            .accesskit_update
            .as_ref()
            .unwrap()
            .nodes;
        if pass == 1 {
            menu_button = nodes
                .iter()
                .find(|(_, node)| {
                    node.role() == Role::Button && node.label() == Some("Presentation commands")
                })
                .map(|(id, _)| *id);
        }
        if pass == 3 || phase == 10 {
            assert_eq!(
                context.memory(egui::Memory::focused).map(|id| id.value()),
                Some(menu_button.unwrap().0),
                "the presentation button must own focus before keyboard opening, pass {pass}"
            );
        }
        let menu_visible = nodes.iter().any(|(_, node)| {
            node.role() == Role::Menu && node.label() == Some("Presentation commands")
        });
        if phase == 7 {
            assert!(
                menu_visible,
                "Enter must open the menu in cycle {}",
                pass / 12
            );
            let focused = context.memory(egui::Memory::focused).map(|id| id.value());
            assert!(
                nodes
                    .iter()
                    .any(|(id, node)| { node.role() == Role::MenuItem && Some(id.0) == focused }),
                "ArrowDown must enter a real presentation command before cancellation"
            );
        }
        if phase == 10 {
            assert!(!menu_visible, "Escape must dismiss the presentation menu");
        }
        assert!(dockspace.view().item(FIRST).unwrap().is_selected());
        assert!(!dockspace.view().item(SECOND).unwrap().is_selected());
        assert!(dockspace.view().item(FIRST).unwrap().contained().is_none());
        output.drop_without_applying_deltas();
        receipt.report(SurfacePresentationResult::Presented);
    }
    assert_eq!(panes.0, ["first content", "second content"]);
}

fn owned_touch_frame(
    context: &Context,
    dockspace: &mut Dockspace,
    panes: &mut RunPanes,
    time: f64,
    events: Vec<Event>,
) -> (FullOutput, Vec<DockspaceTouchEvent>) {
    let (output, response) = dockspace
        .run_single_surface(context, input(time, events), SURFACE, panes)
        .unwrap();
    let (mut output, receipt) = output.into_parts();
    output.textures_delta.clear();
    receipt.report(SurfacePresentationResult::Presented);
    (output, response.touch_events().to_vec())
}

fn request_owned_touch_menu(
    context: &Context,
    dockspace: &mut Dockspace,
    panes: &mut RunPanes,
    position: Pos2,
    time: f64,
) -> DockspaceContextMenuRequest {
    let contact = |phase| Event::Touch {
        device_id: egui::TouchDeviceId(1),
        id: egui::TouchId(7),
        phase,
        pos: position,
        force: None,
    };
    let (_, events) = owned_touch_frame(
        context,
        dockspace,
        panes,
        time,
        vec![contact(egui::TouchPhase::Start)],
    );
    assert!(events.is_empty());
    let (_, events) = owned_touch_frame(context, dockspace, panes, time + 0.35, Vec::new());
    assert!(matches!(
        events.as_slice(),
        [DockspaceTouchEvent::Armed { .. }]
    ));
    let (_, events) = owned_touch_frame(
        context,
        dockspace,
        panes,
        time + 0.36,
        vec![contact(egui::TouchPhase::End)],
    );
    let [DockspaceTouchEvent::ContextMenuRequested(request)] = events.as_slice() else {
        panic!("a stationary armed release must request its real menu: {events:?}");
    };
    assert_eq!(request.item(), Some(SECOND));
    *request
}

fn presentation_menu_count(output: &FullOutput) -> usize {
    output
        .platform_output
        .accesskit_update
        .as_ref()
        .unwrap()
        .nodes
        .iter()
        .filter(|(_, node)| {
            node.role() == Role::Menu && node.label() == Some("Presentation commands")
        })
        .count()
}

#[test]
fn owned_touch_menu_escape_survives_late_discards_and_a_new_long_press_can_reopen() {
    for discarded_passes in [0, 2] {
        let (context, mut dockspace, mut panes, position) =
            presented_tabs("owned-touch-menu-cancellation");
        let before = dockspace.version();
        let first = request_owned_touch_menu(&context, &mut dockspace, &mut panes, position, 1.0);
        assert_eq!(first.version(), before);
        context.options_mut(|options| {
            options.max_passes = NonZeroUsize::new(discarded_passes + 1).unwrap();
        });
        context.add_plugin(LateDiscard {
            remaining: discarded_passes,
        });
        let (output, events) = owned_touch_frame(
            &context,
            &mut dockspace,
            &mut panes,
            1.4,
            vec![Event::Key {
                key: egui::Key::Escape,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            }],
        );
        assert!(events.is_empty());
        assert_eq!(
            output.platform_output.num_completed_passes,
            discarded_passes + 1
        );
        // A closing pass may still contain the menu's final accessibility nodes.
        // Later outputs must stay closed, including after a discarded close pass.
        for (time, events) in [
            (
                1.5,
                vec![Event::Key {
                    key: egui::Key::Escape,
                    physical_key: None,
                    pressed: false,
                    repeat: false,
                    modifiers: egui::Modifiers::NONE,
                }],
            ),
            (1.6, Vec::new()),
        ] {
            let (output, events) =
                owned_touch_frame(&context, &mut dockspace, &mut panes, time, events);
            assert!(events.is_empty());
            assert_eq!(
                presentation_menu_count(&output),
                0,
                "Escape must remain effective after {discarded_passes} late discards"
            );
        }

        let second = request_owned_touch_menu(&context, &mut dockspace, &mut panes, position, 2.0);
        assert_ne!(second.pointer(), first.pointer());
        // egui sizes a newly opened popup invisibly before exposing its widgets.
        let (_, events) = owned_touch_frame(&context, &mut dockspace, &mut panes, 2.4, Vec::new());
        assert!(events.is_empty());
        assert!(egui::Popup::is_any_open(&context));
        for time in [2.5, 2.6] {
            let (output, events) =
                owned_touch_frame(&context, &mut dockspace, &mut panes, time, Vec::new());
            assert!(events.is_empty());
            assert_eq!(
                presentation_menu_count(&output),
                1,
                "a later contact must open a fresh presentation menu"
            );
        }
        assert_eq!(dockspace.version(), before);
        assert!(dockspace.view().item(FIRST).unwrap().is_selected());
        assert!(!dockspace.view().item(SECOND).unwrap().is_selected());
    }
}

#[test]
fn owned_touch_menu_request_survives_an_output_hook_failure() {
    let (context, mut dockspace, mut panes, position) =
        presented_tabs("owned-touch-menu-failed-publication");
    let before = dockspace.version();
    request_owned_touch_menu(&context, &mut dockspace, &mut panes, position, 1.0);
    context.add_plugin(PanicAfterPaint { armed: true });
    let interrupted = catch_unwind(AssertUnwindSafe(|| {
        let _ = dockspace.run_single_surface(&context, input(1.4, Vec::new()), SURFACE, &mut panes);
    }));
    assert!(interrupted.is_err());
    assert!(egui::Popup::is_any_open(&context));
    // Remove transient egui state so the retry must use the unpublished request.
    egui::Popup::close_all(&context);
    let (_, events) = owned_touch_frame(&context, &mut dockspace, &mut panes, 1.5, Vec::new());
    assert!(events.is_empty());
    assert!(egui::Popup::is_any_open(&context));
    for time in [1.6, 1.7] {
        let (output, events) =
            owned_touch_frame(&context, &mut dockspace, &mut panes, time, Vec::new());
        assert!(events.is_empty());
        assert_eq!(presentation_menu_count(&output), 1);
    }
    assert_eq!(dockspace.version(), before);
    assert!(dockspace.view().item(FIRST).unwrap().is_selected());
    assert!(!dockspace.view().item(SECOND).unwrap().is_selected());
}

#[test]
#[allow(
    clippy::cast_possible_truncation,
    reason = "AccessKit bounds come from the finite logical egui menu geometry"
)]
fn overflow_menu_titles_fit_their_text_clip_after_ui_zoom() {
    for zoom in [1.0, 1.5] {
        let context = Context::default();
        context.enable_accesskit();
        context.set_zoom_factor(zoom);
        let layout = DockspaceLayout::new([DockspaceSurfaceLayout::new(
            SURFACE,
            DockspaceRootLayout::new(RootId::new(1), DockspaceNode::central_tabs([FIRST, SECOND])),
        )])
        .unwrap();
        let mut dockspace = Dockspace::builder("overflow-menu-label-clip", layout)
            .style(crate::DockStyle {
                tab_min_width: 400.0,
                tab_max_width: 400.0,
                ..Default::default()
            })
            .build()
            .unwrap();
        let mut panes = EditablePanes::default();
        let mut menu_button = None;
        for pass in 0..8 {
            let events = if pass == 4 {
                vec![Event::AccessKitActionRequest(
                    egui::accesskit::ActionRequest {
                        action: egui::accesskit::Action::Click,
                        target_tree: egui::accesskit::TreeId::ROOT,
                        target_node: menu_button.unwrap(),
                        data: None,
                    },
                )]
            } else {
                Vec::new()
            };
            let (output, _) = dockspace
                .run_single_surface(
                    &context,
                    input(f64::from(pass) * 0.1, events),
                    SURFACE,
                    &mut panes,
                )
                .unwrap();
            let (output, receipt) = output.into_parts();
            let nodes = &output
                .platform_output
                .accesskit_update
                .as_ref()
                .unwrap()
                .nodes;
            if pass == 3 {
                menu_button = nodes
                    .iter()
                    .find(|(_, node)| node.label() == Some("Show hidden tabs"))
                    .map(|(id, _)| *id);
            }
            if pass == 7 {
                assert!((output.pixels_per_point - zoom).abs() < f32::EPSILON);
                let bounds = nodes
                    .iter()
                    .find(|(_, node)| node.role() == Role::Menu)
                    .unwrap()
                    .1
                    .bounds()
                    .unwrap();
                let menu = Rect::from_min_max(
                    egui::pos2(bounds.x0 as f32, bounds.y0 as f32),
                    egui::pos2(bounds.x1 as f32, bounds.y1 as f32),
                );
                let mut titles = 0;
                for clipped in &output.shapes {
                    if let Shape::Text(text) = &clipped.shape
                        && menu.contains(text.pos)
                    {
                        let full_title = text.galley.rect.translate(text.pos.to_vec2());
                        assert!(
                            clipped.clip_rect.expand(0.5).contains_rect(full_title),
                            "menu title {:?} is clipped at zoom {zoom}: {full_title:?} outside {:?}",
                            text.galley.text(),
                            clipped.clip_rect
                        );
                        titles += 1;
                    }
                }
                assert_eq!(titles, 2, "both menu row titles must actually be painted");
            }
            output.drop_without_applying_deltas();
            receipt.report(SurfacePresentationResult::Presented);
        }
    }
}

struct LateDiscard {
    remaining: usize,
}

impl egui::plugin::Plugin for LateDiscard {
    fn debug_name(&self) -> &'static str {
        "product_owned_run_tests::late_discard"
    }

    fn output_hook(&mut self, _context: &Context, output: &mut FullOutput) {
        if self.remaining > 0 {
            self.remaining -= 1;
            output
                .platform_output
                .request_discard_reasons
                .push(RepaintCause::new_reason("discard the owned encoding pass"));
        }
    }
}

struct PanicAfterPaint {
    armed: bool,
}

impl egui::plugin::Plugin for PanicAfterPaint {
    fn debug_name(&self) -> &'static str {
        "product_owned_run_tests::panic_after_paint"
    }

    fn output_hook(&mut self, _context: &Context, output: &mut FullOutput) {
        if std::mem::take(&mut self.armed) {
            // The fixture abandons this output, including newly rasterized menu glyphs.
            output.textures_delta.clear();
            panic!("{OUTPUT_HOOK_PANIC}");
        }
    }
}

struct RemovePaneGeometry {
    removed: Arc<AtomicUsize>,
}

impl egui::plugin::Plugin for RemovePaneGeometry {
    fn debug_name(&self) -> &'static str {
        "product_owned_run_tests::remove_pane_geometry"
    }

    fn output_hook(&mut self, _context: &Context, output: &mut FullOutput) {
        output.shapes.retain(|clipped| {
            let remove = matches!(&clipped.shape, Shape::Rect(rect) if rect.fill == PANE_COLOR);
            if remove {
                self.removed.fetch_add(1, Ordering::Relaxed);
            }
            !remove
        });
    }
}

fn take_ready(
    settlement: &ProductPassSettlement,
    context: &Context,
) -> ReadyProductPassSettlementBatch {
    let mut ready = None;
    context
        .run_ui(input(4.0, Vec::new()), |ui| {
            ready = Some(settlement.take_ready(ui));
        })
        .drop_without_applying_deltas();
    ready.expect("the renderer report queue is read through a real egui UI")
}

#[test]
fn late_discards_paint_once_per_actual_pass_and_publish_one_selection_and_output() {
    let (context, mut dockspace, mut panes, position) = presented_tabs("owned-run-late-discards");
    context.options_mut(|options| options.max_passes = NonZeroUsize::new(3).unwrap());
    context.add_plugin(LateDiscard { remaining: 2 });
    let before = dockspace.version();
    let first_pass = context.cumulative_pass_nr();
    let mut composition_passes = Vec::new();

    let (output, response) = dockspace
        .run_single_surface_with_ui(
            &context,
            input(3.0, press(position)),
            SURFACE,
            &mut panes,
            |ui| composition_passes.push(ui.ctx().cumulative_pass_nr()),
        )
        .expect("discarded encoding must not consume the only output slot");
    let (output, receipt) = output.into_parts();
    assert_eq!(output.platform_output.num_completed_passes, 3);
    assert_eq!(context.cumulative_pass_nr(), first_pass + 3);
    assert_eq!(
        composition_passes,
        [first_pass, first_pass + 1, first_pass + 2],
        "application composition must run once in each actual pass"
    );
    assert_eq!(
        panes.calls,
        [
            (SECOND, first_pass),
            (SECOND, first_pass + 1),
            (SECOND, first_pass + 2),
        ],
        "pane UI must run exactly once in each actual pass, including both discards"
    );
    output.drop_without_applying_deltas();
    assert_eq!(response.mutation().before(), before);
    assert_eq!(response.mutation().after(), dockspace.version());
    assert!(response.mutation().workspace_changed());
    assert_ne!(dockspace.version(), before);
    assert!(dockspace.view().item(SECOND).unwrap().is_selected());
    assert!(dockspace.session.has_active_gesture());
    {
        let mut probe = dockspace.session.begin_host_frame().unwrap();
        assert!(matches!(
            probe.surface_work(SURFACE).unwrap(),
            SurfaceWork::Deferred(SurfaceWorkDeferred::OutputCapacity)
        ));
    }

    receipt.report(SurfacePresentationResult::Presented);
    let mut ready = take_ready(&dockspace.pass_settlement, &context);
    assert!(!ready.lane_changed);
    assert_eq!(ready.presentation_failure, None);
    assert_eq!(
        ready.presentations.len(),
        1,
        "only the terminal output exists"
    );
    let presentation = ready.presentations.pop().unwrap();
    assert_eq!(presentation.result, SurfacePresentationResult::Presented);
    let artifact = presentation.output.presentation_artifact();
    assert!(
        !dockspace
            .session
            .presented_surface(SURFACE)
            .unwrap()
            .matches_presentation_artifact(artifact),
        "a renderer report alone cannot publish presentation authority"
    );
    dockspace
        .session
        .report_surface_presentation(presentation.output, presentation.result)
        .unwrap();
    let after = dockspace.version();
    let mut frame = dockspace.session.begin_host_frame().unwrap();
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    let mut report = frame.commit().unwrap();
    assert_eq!(report.before(), after);
    assert_eq!(report.after(), after);
    assert!(!report.workspace_changed());
    assert!(report.take_painted_outputs().is_empty());
    assert_eq!(report.surface_presentation_commits().len(), 1);
    assert!(report.surface_presentation_commits()[0].presents(artifact));
    assert!(
        dockspace
            .session
            .presented_surface(SURFACE)
            .unwrap()
            .matches_presentation_artifact(artifact)
    );
    assert!(
        take_ready(&dockspace.pass_settlement, &context)
            .presentations
            .is_empty()
    );
}

#[test]
fn queued_accessibility_event_survives_a_later_update_hook_panic_until_publication() {
    let (context, mut dockspace, mut panes, _) = presented_tabs("owned-run-queued-event-retry");
    let (output, _) = dockspace
        .run_single_surface(&context, input(1.0, Vec::new()), SURFACE, &mut panes)
        .unwrap();
    let (output, receipt) = output.into_parts();
    let target = output
        .platform_output
        .accesskit_update
        .as_ref()
        .unwrap()
        .nodes
        .iter()
        .find_map(|(id, node)| {
            (node.role() == Role::Tab && node.label() == Some("Second")).then_some(*id)
        })
        .unwrap();
    output.drop_without_applying_deltas();
    receipt.report(SurfacePresentationResult::Presented);
    let event = Event::AccessKitActionRequest(egui::accesskit::ActionRequest {
        action: egui::accesskit::Action::Click,
        target_tree: egui::accesskit::TreeId::ROOT,
        target_node: target,
        data: None,
    });
    let (output, _) = dockspace
        .run_single_surface(&context, input(2.0, vec![event]), SURFACE, &mut panes)
        .unwrap();
    let (output, receipt) = output.into_parts();
    output.drop_without_applying_deltas();
    receipt.report(SurfacePresentationResult::Presented);
    assert!(dockspace.view().item(FIRST).unwrap().is_selected());
    assert!(
        !dockspace.ui_actions.is_empty(),
        "the real UI event entered deferred ingress"
    );
    let before = dockspace.version();
    panes.calls.clear();
    context.add_plugin(PanicAfterPaint { armed: true });
    assert!(
        catch_unwind(AssertUnwindSafe(|| {
            let _ =
                dockspace.run_single_surface(&context, input(3.0, Vec::new()), SURFACE, &mut panes);
        }))
        .is_err()
    );
    assert!(panes.calls.iter().any(|(item, _)| *item == SECOND));
    assert_eq!(dockspace.version(), before);
    assert!(dockspace.view().item(FIRST).unwrap().is_selected());
    assert!(
        !dockspace.ui_actions.is_empty(),
        "an unpublished attempt cannot acknowledge the queued event"
    );
    let (output, response) = dockspace
        .run_single_surface(&context, input(4.0, Vec::new()), SURFACE, &mut panes)
        .unwrap();
    let (output, receipt) = output.into_parts();
    output.drop_without_applying_deltas();
    receipt.report(SurfacePresentationResult::Presented);
    assert!(response.mutation().workspace_changed());
    assert!(dockspace.view().item(SECOND).unwrap().is_selected());
    assert!(dockspace.ui_actions.is_empty());
    let selected = dockspace.version();
    let (output, response) = dockspace
        .run_single_surface(&context, input(5.0, Vec::new()), SURFACE, &mut panes)
        .unwrap();
    output.into_parts().0.drop_without_applying_deltas();
    assert_eq!(dockspace.version(), selected);
    assert!(!response.mutation().workspace_changed());
}

#[test]
fn real_ui_action_ingress_accepts_64_events_and_rejects_65_without_partial_publication() {
    struct ManyPanes;
    impl PaneView for ManyPanes {
        fn title(&self, item: ItemId) -> Option<egui::WidgetText> {
            Some(format!("Pane {}", item.get()).into())
        }
        fn ui(&mut self, _item: ItemId, ui: &mut Ui) -> crate::PaneResponse {
            ui.label("Bounded action ingress");
            crate::PaneResponse::default()
        }
    }
    for count in [64, 65] {
        let context = Context::default();
        context.enable_accesskit();
        context.options_mut(|options| options.max_passes = NonZeroUsize::new(1).unwrap());
        let layout = DockspaceLayout::new([DockspaceSurfaceLayout::new(
            SURFACE,
            DockspaceRootLayout::new(
                RootId::new(1),
                DockspaceNode::central_tabs((1..=count).map(ItemId::new)),
            ),
        )])
        .unwrap();
        let mut dockspace = Dockspace::builder(("bounded-owned-actions", count), layout)
            .build()
            .unwrap();
        let mut panes = ManyPanes;
        let wide_input = |time, events| {
            let mut raw = input(time, events);
            raw.screen_rect = Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(16_384.0, 480.0)));
            raw
        };
        let mut events = Vec::new();
        for time in [0.0, 0.1, 0.2] {
            let (output, _) = dockspace
                .run_single_surface(&context, wide_input(time, Vec::new()), SURFACE, &mut panes)
                .unwrap();
            let (output, receipt) = output.into_parts();
            events = output
                .platform_output
                .accesskit_update
                .as_ref()
                .unwrap()
                .nodes
                .iter()
                .filter(|(_, node)| node.role() == Role::Tab)
                .map(|(id, _)| {
                    Event::AccessKitActionRequest(egui::accesskit::ActionRequest {
                        action: egui::accesskit::Action::Click,
                        target_tree: egui::accesskit::TreeId::ROOT,
                        target_node: *id,
                        data: None,
                    })
                })
                .collect();
            output.drop_without_applying_deltas();
            receipt.report(SurfacePresentationResult::Presented);
        }
        assert_eq!(events.len(), usize::try_from(count).unwrap());
        let before = dockspace.version();
        let result =
            dockspace.run_single_surface(&context, wide_input(1.0, events), SURFACE, &mut panes);
        assert_eq!(dockspace.version(), before);
        if count == 65 {
            assert!(
                result.is_err(),
                "overflow is rejected before any new action batch publishes"
            );
            assert!(dockspace.ui_actions.is_empty());
        } else {
            let (output, _) = result.unwrap();
            let (output, receipt) = output.into_parts();
            output.drop_without_applying_deltas();
            receipt.report(SurfacePresentationResult::Presented);
            assert_eq!(dockspace.ui_actions.before(u64::MAX).count(), 64);
            let (output, _) = dockspace
                .run_single_surface(&context, wide_input(2.0, Vec::new()), SURFACE, &mut panes)
                .unwrap();
            output.into_parts().0.drop_without_applying_deltas();
            assert!(
                dockspace
                    .view()
                    .item(ItemId::new(64))
                    .unwrap()
                    .is_selected()
            );
            assert!(dockspace.ui_actions.is_empty());
        }
    }
}

#[test]
fn output_hook_panic_rolls_back_a_real_selection_and_never_replays_its_raw_press() {
    let (context, mut dockspace, mut panes, position) =
        presented_tabs("owned-run-output-hook-panic");
    let before = dockspace.version();
    let presented = dockspace.session.presented_surface(SURFACE);
    let continuation = dockspace.session.pointer_continuation();
    let first_pass = context.cumulative_pass_nr();
    context.add_plugin(PanicAfterPaint { armed: true });

    let interrupted = catch_unwind(AssertUnwindSafe(|| {
        let _ = dockspace.run_single_surface(
            &context,
            input(3.0, press(position)),
            SURFACE,
            &mut panes,
        );
    }));
    let panic = interrupted.expect_err("the terminal output hook must unwind the owned run");
    let message = panic
        .downcast_ref::<&str>()
        .copied()
        .or_else(|| panic.downcast_ref::<String>().map(String::as_str));
    assert_eq!(message, Some(OUTPUT_HOOK_PANIC));
    assert_eq!(
        panes.calls,
        [(SECOND, first_pass)],
        "the real inactive-tab press must select and paint before the hook panics"
    );
    assert_eq!(dockspace.version(), before);
    assert!(dockspace.view().item(FIRST).unwrap().is_selected());
    assert!(!dockspace.view().item(SECOND).unwrap().is_selected());
    assert!(!dockspace.session.has_active_gesture());
    assert!(presented.is_some());
    assert!(
        dockspace.session.presented_surface(SURFACE).is_none(),
        "the prior renderer report still withholds old authority after candidate rollback"
    );
    assert_eq!(dockspace.session.pointer_continuation(), continuation);

    // Probe core's producer clock directly: egui's clock itself cannot rewind.
    // This earlier event must remain admissible after discarding the 3s batch.
    {
        let mut probe = dockspace.session.begin_host_frame().unwrap();
        probe
            .submit_host_pointer(
                HostPointerInput::new(
                    SurfacePointerId::new(1),
                    SurfacePointerEvent::Moved,
                    HostPointerRoute::Unknown,
                    HostPointerRoute::Unknown,
                    SurfacePointerCapture::Unknown,
                    SurfacePointerReceiverFacts::unknown(),
                )
                .with_metadata(PointerEventMetadata::new(
                    PointerDeviceKind::Unknown,
                    Duration::from_millis(2500),
                )),
            )
            .expect("the failed run must not advance the producer watermark");
        assert!(matches!(
            probe.surface_work(SURFACE).unwrap(),
            SurfaceWork::Ready(_)
        ));
    }

    panes.calls.clear();
    let next_pass = context.cumulative_pass_nr();
    let (output, response) = dockspace
        .run_single_surface(&context, input(4.0, Vec::new()), SURFACE, &mut panes)
        .expect("unwinding must release the candidate and its encoding slot");
    let (output, receipt) = output.into_parts();
    output.drop_without_applying_deltas();
    assert_eq!(panes.calls, [(FIRST, next_pass)]);
    assert_eq!(response.mutation().before(), before);
    assert_eq!(response.mutation().after(), before);
    assert!(!response.mutation().workspace_changed());
    assert_eq!(dockspace.version(), before);
    assert!(dockspace.view().item(FIRST).unwrap().is_selected());
    assert!(!dockspace.session.has_active_gesture());
    assert!(dockspace.ui_actions.is_empty());
    receipt.report(SurfacePresentationResult::Presented);
}

#[test]
fn removing_real_pane_geometry_rejects_the_run_before_core_publication() {
    let (context, mut dockspace, mut panes, position) =
        presented_tabs("owned-run-damaged-pane-output");
    let before = dockspace.version();
    let presented = dockspace.session.presented_surface(SURFACE);
    let first_pass = context.cumulative_pass_nr();
    let removed = Arc::new(AtomicUsize::new(0));
    context.add_plugin(RemovePaneGeometry {
        removed: Arc::clone(&removed),
    });
    let mut composition_calls = 0;

    let error = dockspace
        .run_single_surface_with_ui(
            &context,
            input(3.0, press(position)),
            SURFACE,
            &mut panes,
            |_| composition_calls += 1,
        )
        .err()
        .expect("an output hook cannot remove pane pixels and still publish selection");
    assert_eq!(error.kind(), DockspaceErrorKind::OperationConflict);
    assert_eq!(
        composition_calls, 1,
        "rejecting docking output does not roll back external callback state"
    );
    assert_eq!(removed.load(Ordering::Relaxed), 1);
    assert_eq!(
        panes.calls,
        [(SECOND, first_pass)],
        "the failed output must contain the real newly selected pane"
    );
    assert_eq!(dockspace.version(), before);
    assert!(dockspace.view().item(FIRST).unwrap().is_selected());
    assert!(!dockspace.view().item(SECOND).unwrap().is_selected());
    assert!(!dockspace.session.has_active_gesture());
    assert!(presented.is_some());
    assert!(
        dockspace.session.presented_surface(SURFACE).is_none(),
        "the prior renderer report still withholds old authority after candidate rollback"
    );
    assert!(dockspace.ui_actions.is_empty());
    let mut probe = dockspace.session.begin_host_frame().unwrap();
    assert!(matches!(
        probe.surface_work(SURFACE).unwrap(),
        SurfaceWork::Ready(_)
    ));
}
