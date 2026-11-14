//! Real-input regression over the actual eight-pane example and its ordinary content.
//!
//! The external input driver reads coordinates only. This observer accepts no
//! synthetic input, presentation, or programmatic docking commands.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use dockspace::model::ItemId;
use eframe::egui;

use super::{
    ASSETS, CONSOLE, EDITOR, HISTORY, INSPECTOR, NativeExampleApp, OUTLINE, PREVIEW, PROFILER,
    SURFACE,
};

mod drain;

#[derive(Default)]
struct EditorObservations {
    tabs: BTreeMap<String, egui::Rect>,
    splitters: Vec<(egui::accesskit::Orientation, egui::Rect)>,
    primary_down: bool,
    left_while_pressed: bool,
    pressed_reentries: usize,
    client_rect: Option<egui::Rect>,
}

struct EditorProbe(Arc<Mutex<EditorObservations>>);

impl egui::plugin::Plugin for EditorProbe {
    fn debug_name(&self) -> &'static str {
        "native-editor-docking-coordinates"
    }

    fn input_hook(&mut self, _: &egui::Context, input: &mut egui::RawInput) {
        let mut tabs = self.0.lock().unwrap();
        if let Some(rect) = input.screen_rect {
            tabs.client_rect = Some(rect);
        }
        for event in &input.events {
            match event {
                egui::Event::PointerButton {
                    button: egui::PointerButton::Primary,
                    pressed,
                    ..
                } => {
                    tabs.primary_down = *pressed;
                    tabs.left_while_pressed = false;
                }
                egui::Event::PointerGone if tabs.primary_down => {
                    tabs.left_while_pressed = true;
                }
                egui::Event::PointerMoved(point)
                    if tabs.left_while_pressed
                        && tabs.primary_down
                        && tabs.client_rect.is_some_and(|rect| rect.contains(*point)) =>
                {
                    tabs.pressed_reentries += 1;
                    tabs.left_while_pressed = false;
                }
                _ => {}
            }
        }
    }

    #[allow(clippy::cast_possible_truncation)]
    fn output_hook(&mut self, _: &egui::Context, output: &mut egui::FullOutput) {
        fn unaligned_labels(shape: &egui::Shape) -> usize {
            match shape {
                egui::Shape::Text(text) => usize::from(text.galley.job.text == "Unaligned"),
                egui::Shape::Vec(shapes) => shapes.iter().map(unaligned_labels).sum(),
                _ => 0,
            }
        }
        let unaligned: usize = output
            .shapes
            .iter()
            .map(|shape| unaligned_labels(&shape.shape))
            .sum();
        assert_eq!(unaligned, 0, "the actual editor emitted Unaligned overlays");

        let Some(update) = &output.platform_output.accesskit_update else {
            return;
        };
        let mut tabs = self.0.lock().unwrap();
        tabs.tabs.clear();
        tabs.splitters.clear();
        for (_, node) in &update.nodes {
            let Some(rect) = node.bounds() else { continue };
            let rect = egui::Rect::from_min_max(
                egui::pos2(rect.x0 as f32, rect.y0 as f32),
                egui::pos2(rect.x1 as f32, rect.y1 as f32),
            );
            match node.role() {
                egui::accesskit::Role::Tab => {
                    if let Some(title) = node.label() {
                        tabs.tabs.insert(title.to_owned(), rect);
                    }
                }
                egui::accesskit::Role::Splitter
                    if !node.is_disabled() && node.label() == Some("Resize panes") =>
                {
                    if let Some(orientation) = node.orientation() {
                        tabs.splitters.push((orientation, rect));
                    }
                }
                _ => {}
            }
        }
    }
}

pub(super) struct DockingSmoke {
    tabs: Arc<Mutex<EditorObservations>>,
    journey: Journey,
}

enum Journey {
    Merge(MergeSmoke),
    Drain(drain::DrainSmoke),
    Complete,
}

impl DockingSmoke {
    pub(super) fn new() -> Self {
        let journey = match std::env::var("EGUI_DOCKSPACE_DOCKING_CASE").as_deref() {
            Ok("console") => Journey::Merge(MergeSmoke::new(HISTORY, CONSOLE)),
            Ok("inspector") => Journey::Merge(MergeSmoke::new(HISTORY, INSPECTOR)),
            Ok("preview") => Journey::Merge(MergeSmoke::new(HISTORY, PREVIEW)),
            Ok("profiler") => Journey::Merge(MergeSmoke::new(ASSETS, PROFILER)),
            Ok("outline") | Err(_) => Journey::Merge(MergeSmoke::new(HISTORY, OUTLINE)),
            Ok("drain-inspector-first") => {
                Journey::Drain(drain::DrainSmoke::new([INSPECTOR, PREVIEW]))
            }
            Ok("drain-preview-first") => {
                Journey::Drain(drain::DrainSmoke::new([PREVIEW, INSPECTOR]))
            }
            Ok(value) => panic!("unknown editor docking case: {value}"),
        };
        Self {
            tabs: Arc::default(),
            journey,
        }
    }

    pub(super) fn install(&self, context: &egui::Context) {
        context.enable_accesskit();
        context.add_plugin(EditorProbe(Arc::clone(&self.tabs)));
    }

    pub(super) fn advance(&mut self, app: &NativeExampleApp, context: &egui::Context) {
        let complete = match &mut self.journey {
            Journey::Merge(smoke) => smoke.advance(app, context, &self.tabs),
            Journey::Drain(smoke) => smoke.advance(app, context, &self.tabs),
            Journey::Complete => return,
        };
        if complete {
            self.journey = Journey::Complete;
            eprintln!("editor-docking-complete: actual editor journey settled");
            context.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        context.request_repaint_after(Duration::from_millis(16));
    }
}

struct MergeSmoke {
    started: Instant,
    announced: bool,
    source: ItemId,
    target: ItemId,
    selection_announced: bool,
    expected_target: Vec<ItemId>,
    remaining_source: Vec<ItemId>,
    reentries_before: usize,
}

impl MergeSmoke {
    fn new(source: ItemId, target: ItemId) -> Self {
        Self {
            started: Instant::now(),
            announced: false,
            source,
            target,
            selection_announced: false,
            expected_target: Vec::new(),
            remaining_source: Vec::new(),
            reentries_before: 0,
        }
    }

    fn advance(
        &mut self,
        app: &NativeExampleApp,
        context: &egui::Context,
        tabs: &Arc<Mutex<EditorObservations>>,
    ) -> bool {
        assert!(app.dockspace.error_kind().is_none());
        assert!(
            self.started.elapsed() < Duration::from_secs(15),
            "actual editor docking failed; view={:?}",
            app.dockspace.with_view(|view| {
                [self.source, self.target, EDITOR].map(|id| {
                    let item = view.item(id).unwrap();
                    (
                        id,
                        item.root(),
                        item.tabs().items().to_vec(),
                        item.is_selected(),
                    )
                })
            }),
        );
        let ready = app.dockspace.is_surface_presented(SURFACE)
            && app.dockspace.is_quiescent()
            && !app.dockspace.current_output_is_retained();
        let target_selected = app
            .dockspace
            .with_view(|view| view.item(self.target).unwrap().is_selected());
        let target_title = app
            .dockspace
            .with_panes(|panes| panes.panes[&self.target].title);
        let source_title = app
            .dockspace
            .with_panes(|panes| panes.panes[&self.source].title);
        if ready
            && !self.announced
            && !target_selected
            && !self.selection_announced
            && let Some(rect) = tabs.lock().unwrap().tabs.get(target_title).copied()
        {
            let point = rect.min + rect.size() * 0.35;
            let scale = context.pixels_per_point();
            let token = eframe::current_native_output_token().unwrap();
            eprintln!(
                "editor-docking-select-target: {} {} {}",
                u64::from(token.window_id()),
                (point.x * scale).round(),
                (point.y * scale).round()
            );
            self.selection_announced = true;
        }
        let (source, reentries_before) = {
            let tabs = tabs.lock().unwrap();
            (tabs.tabs.get(source_title).copied(), tabs.pressed_reentries)
        };
        if !self.announced
            && ready
            && target_selected
            && let Some(source) = source
            && let Some((target, scale)) = app
                .dockspace
                .with_panes(|panes| panes.panes.get(&self.target).unwrap().content)
        {
            let source = (source.min + source.size() * 0.35) * scale;
            let target = target.center() * scale;
            let (mut target_items, source_items) = app.dockspace.with_view(|view| {
                (
                    view.item(self.target).unwrap().tabs().items().to_vec(),
                    view.item(self.source).unwrap().tabs().items().to_vec(),
                )
            });
            assert!(!target_items.contains(&self.source));
            target_items.push(self.source);
            self.expected_target = target_items;
            self.remaining_source = source_items
                .into_iter()
                .filter(|item| *item != self.source)
                .collect();
            let token = eframe::current_native_output_token().unwrap();
            eprintln!(
                "editor-docking-ready: {} {} {} {} {}",
                u64::from(token.window_id()),
                source.x.round(),
                source.y.round(),
                target.x.round(),
                target.y.round(),
            );
            self.reentries_before = reentries_before;
            self.announced = true;
        }
        if self.announced && self.docked(app) && app.dockspace.is_quiescent() {
            eprintln!(
                "editor-docking-merged: {source_title} docked into the actual {target_title} content"
            );
            return true;
        }
        false
    }

    fn docked(&self, app: &NativeExampleApp) -> bool {
        app.dockspace.with_view(|view| {
            let source = view.item(self.source).unwrap();
            let target = view.item(self.target).unwrap();
            source.tabs().items() == self.expected_target
                && target.tabs().items() == self.expected_target
                && source.root() == target.root()
                && source.contained() == target.contained()
                && source.is_selected()
                && self.remaining_source.first().is_none_or(|item| {
                    view.item(*item).unwrap().tabs().items() == self.remaining_source
                })
        })
    }
}
