//! Application data drag-and-drop remains separate from docking gestures.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use egui::{Context, Event, Modifiers, PointerButton, Pos2, RawInput, Rect, Sense, Ui, vec2};
use egui_dockspace::{
    Dockspace, DockspaceAxis, DockspaceCapability, DockspaceLayout, DockspaceNode,
    DockspaceRootLayout, DockspaceSurfaceLayout, ItemId, PaneResponse, PaneView, RootId, SurfaceId,
    SurfacePresentationResult, Tab, TabVisuals,
};

const SURFACE: SurfaceId = SurfaceId::new(1);
const ROOT: RootId = RootId::new(1);
const ASSETS: ItemId = ItemId::new(1);
const SETTINGS: ItemId = ItemId::new(2);
const INSPECTOR: ItemId = ItemId::new(3);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct AssetId(u64);

const ASSET: AssetId = AssetId(42);

#[derive(Default)]
struct Panes {
    high_level_source: bool,
    source: Option<Rect>,
    destination: Option<Rect>,
    settings_tab: Option<Rect>,
    drag_starts: usize,
    destination_contains_pointer: bool,
    received: Vec<AssetId>,
}

struct LateDiscard {
    remaining: usize,
}

impl egui::plugin::Plugin for LateDiscard {
    fn debug_name(&self) -> &'static str {
        "data_drag::late_discard"
    }

    fn output_hook(&mut self, _context: &Context, output: &mut egui::FullOutput) {
        if self.remaining > 0 {
            self.remaining -= 1;
            output
                .platform_output
                .request_discard_reasons
                .push(egui::RepaintCause::new_reason(
                    "retain a real data-drag release",
                ));
        }
    }
}

#[test]
fn high_level_drag_across_tabs_and_a_multipass_drop_delivers_one_payload() {
    for owned in [true, false] {
        let mut harness = Harness::new(owned, true);
        let source = harness.panes.source.unwrap().center();
        let destination = harness.panes.destination.unwrap().center();
        let settings = harness.panes.settings_tab.unwrap().center();
        let version = harness.dockspace.version();
        for events in [
            vec![Event::PointerMoved(source)],
            vec![button(source, true)],
            vec![Event::PointerMoved(source + vec2(40.0, 0.0))],
            vec![Event::PointerMoved(settings)],
            vec![Event::PointerMoved(destination)],
        ] {
            harness.frame(events);
            assert_eq!(harness.dockspace.version(), version);
            assert!(harness.dockspace.view().item(ASSETS).unwrap().is_selected());
            assert!(!harness.dockspace.has_active_gesture());
            harness.assert_layout();
        }
        harness.context.options_mut(|options| {
            options.max_passes = 3.try_into().unwrap();
        });
        harness.context.add_plugin(LateDiscard { remaining: 2 });
        harness.frame(vec![button(destination, false)]);
        assert_eq!(harness.passes, 3);
        assert_eq!(harness.panes.received, [ASSET], "owned={owned}");
        assert!(!egui::DragAndDrop::has_any_payload(&harness.context));
        assert_eq!(harness.dockspace.version(), version);
        assert!(!harness.dockspace.has_active_gesture());
        harness.frame(Vec::new());
        assert_eq!(harness.panes.received, [ASSET]);
    }
}

struct GhostTamper {
    delete: bool,
    changed: Arc<AtomicBool>,
}

impl egui::plugin::Plugin for GhostTamper {
    fn debug_name(&self) -> &'static str {
        "data_drag::tamper_ghost"
    }

    fn output_hook(&mut self, _context: &Context, output: &mut egui::FullOutput) {
        fn tamper(shape: &mut egui::Shape, delete: bool) -> bool {
            match shape {
                egui::Shape::Text(text) if text.galley.job.text == "Drag asset" => {
                    if delete {
                        *shape = egui::Shape::Noop;
                    } else {
                        text.pos.x += 3.0;
                    }
                    true
                }
                egui::Shape::Vec(shapes) => {
                    let mut changed = false;
                    for shape in shapes {
                        changed |= tamper(shape, delete);
                    }
                    changed
                }
                _ => false,
            }
        }
        for shape in &mut output.shapes {
            if tamper(&mut shape.shape, self.delete) {
                self.changed.store(true, Ordering::SeqCst);
            }
        }
    }
}

#[test]
fn output_hooks_cannot_delete_or_modify_the_real_high_level_drag_ghost() {
    for delete in [false, true] {
        let mut harness = Harness::new(true, true);
        let source = harness.panes.source.unwrap().center();
        harness.frame(vec![Event::PointerMoved(source)]);
        harness.frame(vec![button(source, true)]);
        assert!(egui::DragAndDrop::has_payload_of_type::<AssetId>(
            &harness.context
        ));
        let version = harness.dockspace.version();
        let changed = Arc::new(AtomicBool::new(false));
        harness.context.add_plugin(GhostTamper {
            delete,
            changed: Arc::clone(&changed),
        });
        harness.time += 0.05;
        let result = harness.dockspace.run_single_surface(
            &harness.context,
            RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(900.0, 480.0))),
                time: Some(harness.time),
                events: vec![Event::PointerMoved(source + vec2(40.0, 0.0))],
                ..RawInput::default()
            },
            SURFACE,
            &mut harness.panes,
        );
        assert!(
            changed.load(Ordering::SeqCst),
            "the actual drag ghost must be modified"
        );
        let error = match result {
            Ok((output, _)) => {
                let (output, receipt) = output.into_parts();
                output.drop_without_applying_deltas();
                drop(receipt);
                panic!("a changed data-drag ghost must reject the complete output");
            }
            Err(error) => error,
        };
        assert_eq!(
            error.kind(),
            egui_dockspace::DockspaceErrorKind::OperationConflict
        );
        assert_eq!(harness.dockspace.version(), version);
        harness.assert_layout();
    }
}

impl PaneView for Panes {
    fn title(&self, item: ItemId) -> Option<egui::WidgetText> {
        Some(
            match item {
                ASSETS => "Assets",
                SETTINGS => "Settings",
                INSPECTOR => "Inspector",
                _ => return None,
            }
            .into(),
        )
    }

    fn tab_ui(&mut self, ui: &mut Ui, tab: &Tab<'_>) -> TabVisuals {
        if tab.item == SETTINGS {
            self.settings_tab = Some(tab.content_rect.intersect(tab.visible_rect));
        }
        tab.paint(ui)
    }

    fn ui(&mut self, item: ItemId, ui: &mut Ui) -> PaneResponse {
        match item {
            ASSETS => {
                if self.high_level_source {
                    let id = ui.make_persistent_id("asset-data-drag-source");
                    let source = ui.dnd_drag_source(id, ASSET, |ui| {
                        ui.add_sized(vec2(180.0, 80.0), egui::Label::new("Drag asset"))
                    });
                    self.source = Some(source.response.rect);
                    self.drag_starts += usize::from(ui.ctx().drag_started_id() == Some(id));
                    return PaneResponse::from(&source.response).with_content(&source.inner);
                }
                let response = ui.add_sized(
                    vec2(180.0, 80.0),
                    egui::Button::new("Drag asset").sense(Sense::click_and_drag()),
                );
                response.dnd_set_drag_payload(ASSET);
                self.source = Some(response.rect);
                self.drag_starts += usize::from(response.drag_started());
                PaneResponse::from(&response)
            }
            INSPECTOR => {
                let (zone, payload) =
                    ui.dnd_drop_zone::<AssetId, _>(egui::Frame::group(ui.style()), |ui| {
                        ui.set_min_size(vec2(240.0, 160.0));
                        ui.label("Drop asset here")
                    });
                self.destination = Some(zone.response.rect);
                self.destination_contains_pointer = zone.response.contains_pointer();
                if let Some(payload) = payload {
                    self.received.push(*payload);
                }
                PaneResponse::from(&zone.response).with_content(&zone.inner)
            }
            _ => PaneResponse::from(&ui.label("Settings content")),
        }
    }
}

struct Harness {
    context: Context,
    dockspace: Dockspace,
    panes: Panes,
    owned: bool,
    time: f64,
    passes: usize,
}

impl Harness {
    fn new(owned: bool, high_level_source: bool) -> Self {
        let context = Context::default();
        context.options_mut(|options| options.max_passes = 1.try_into().unwrap());
        let root = DockspaceNode::equal_split(
            DockspaceAxis::Horizontal,
            [
                DockspaceNode::tabs([ASSETS, SETTINGS]),
                DockspaceNode::central_tabs([INSPECTOR]),
            ],
        )
        .unwrap();
        let layout = DockspaceLayout::new([DockspaceSurfaceLayout::new(
            SURFACE,
            DockspaceRootLayout::new(ROOT, root),
        )])
        .unwrap();
        let mut harness = Self {
            context,
            dockspace: Dockspace::builder(("pane-data-drag", owned), layout)
                .build()
                .unwrap(),
            panes: Panes {
                high_level_source,
                ..Panes::default()
            },
            owned,
            time: 0.0,
            passes: 0,
        };
        for _ in 0..4 {
            harness.frame(Vec::new());
        }
        harness
    }

    fn frame(&mut self, events: Vec<Event>) {
        self.time += 0.05;
        let input = RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(900.0, 480.0))),
            time: Some(self.time),
            events,
            ..RawInput::default()
        };
        let (output, response) = if self.owned {
            self.dockspace
                .run_single_surface(&self.context, input, SURFACE, &mut self.panes)
                .expect("ordinary pane DnD must not cause PassSettlementConflict")
        } else {
            let mut response = None;
            let output = egui_dockspace::run_ui(&self.context, input, |ui| {
                response = Some(
                    self.dockspace
                        .show_single_surface(SURFACE, ui, &mut self.panes)
                        .expect("ordinary pane DnD must not cause PassSettlementConflict"),
                );
            });
            (output, response.unwrap())
        };
        let (mut output, receipt) = output.into_parts();
        self.passes = output.platform_output.num_completed_passes;
        output.textures_delta.clear();
        output.drop_without_applying_deltas();
        receipt.report(SurfacePresentationResult::Presented);
        assert_eq!(
            response.presentation_settlement_capability(),
            DockspaceCapability::Supported,
            "each ordinary pane output must retain exact settlement, owned={}",
            self.owned,
        );
    }

    fn assert_layout(&self) {
        let view = self.dockspace.view();
        assert_eq!(view.surfaces().count(), 1);
        let surface = view.surface(SURFACE).unwrap();
        assert_eq!(surface.contained_count(), 0);
        let root = surface.main_root().unwrap();
        assert_eq!(root.id(), ROOT);
        let split = root.content().unwrap().split().unwrap();
        assert_eq!(split.axis(), DockspaceAxis::Horizontal);
        let children = split.children().collect::<Vec<_>>();
        assert_eq!(children.len(), 2);
        assert_eq!(children[0].tabs().unwrap().items(), [ASSETS, SETTINGS]);
        let inspector = children[1].tabs().unwrap();
        assert_eq!(inspector.items(), [INSPECTOR]);
        assert_eq!(inspector.selected(), Some(INSPECTOR));
        assert!(inspector.is_central());
    }
}

fn button(position: Pos2, pressed: bool) -> Event {
    Event::PointerButton {
        pos: position,
        button: PointerButton::Primary,
        pressed,
        modifiers: Modifiers::NONE,
    }
}

#[test]
fn pane_data_drag_delivers_once_without_docking_and_then_allows_a_new_tab_press() {
    for owned in [true, false] {
        for high_level_source in [false, true] {
            let mut harness = Harness::new(owned, high_level_source);
            let version = harness.dockspace.version();
            let source = harness.panes.source.unwrap().center();
            let destination = harness.panes.destination.unwrap().center();
            assert!(destination.x - source.x > 200.0);
            assert!(harness.dockspace.view().item(ASSETS).unwrap().is_selected());

            for events in [
                vec![Event::PointerMoved(source)],
                vec![button(source, true)],
                vec![Event::PointerMoved(source + vec2(40.0, 0.0))],
            ] {
                harness.frame(events);
                assert_eq!(harness.dockspace.version(), version, "owned={owned}");
                harness.assert_layout();
            }
            assert_eq!(harness.panes.drag_starts, 1, "owned={owned}");
            assert_eq!(
                egui::DragAndDrop::payload::<AssetId>(&harness.context).as_deref(),
                Some(&ASSET),
                "the real source response must start a typed data drag, owned={owned}",
            );

            harness.frame(vec![Event::PointerMoved(destination)]);
            assert!(harness.panes.destination_contains_pointer, "owned={owned}");
            assert!(harness.panes.received.is_empty());
            assert!(egui::DragAndDrop::has_payload_of_type::<AssetId>(
                &harness.context
            ));
            assert_eq!(harness.dockspace.version(), version);
            harness.assert_layout();

            for events in [vec![button(destination, false)], Vec::new(), Vec::new()] {
                harness.frame(events);
                assert_eq!(harness.panes.received, [ASSET], "owned={owned}");
                assert_eq!(harness.dockspace.version(), version, "owned={owned}");
                harness.assert_layout();
            }
            assert!(!egui::DragAndDrop::has_any_payload(&harness.context));
            assert!(!harness.dockspace.has_active_gesture());
            assert!(harness.dockspace.view().item(ASSETS).unwrap().is_selected());

            let settings = harness.panes.settings_tab.unwrap().center();
            for events in [
                vec![Event::PointerMoved(settings)],
                vec![button(settings, true)],
                vec![button(settings, false)],
                Vec::new(),
            ] {
                harness.frame(events);
            }
            assert!(
                harness
                    .dockspace
                    .view()
                    .item(SETTINGS)
                    .unwrap()
                    .is_selected()
            );
            assert!(!harness.dockspace.view().item(ASSETS).unwrap().is_selected());
            assert_ne!(harness.dockspace.version(), version);
            assert!(!harness.dockspace.has_active_gesture());
            assert_eq!(harness.panes.drag_starts, 1);
            assert_eq!(harness.panes.received, [ASSET]);
            harness.assert_layout();
        }
    }
}
