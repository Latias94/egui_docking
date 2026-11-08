//! Real-input regression over the actual eight-pane example and its ordinary content.
//!
//! The external input driver reads coordinates only. This observer accepts no
//! synthetic input, presentation, or programmatic docking commands.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use super::*;

#[derive(Default)]
struct Tabs(BTreeMap<String, egui::Rect>);

struct TabProbe(Arc<Mutex<Tabs>>);

impl egui::plugin::Plugin for TabProbe {
    fn debug_name(&self) -> &'static str {
        "native-editor-docking-coordinates"
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
        tabs.0.clear();
        for (_, node) in &update.nodes {
            if node.role() == egui::accesskit::Role::Tab
                && let (Some(title), Some(rect)) = (node.label(), node.bounds())
            {
                tabs.0.insert(
                    title.to_owned(),
                    egui::Rect::from_min_max(
                        egui::pos2(rect.x0 as f32, rect.y0 as f32),
                        egui::pos2(rect.x1 as f32, rect.y1 as f32),
                    ),
                );
            }
        }
    }
}

pub(super) struct DockingSmoke {
    started: Instant,
    tabs: Arc<Mutex<Tabs>>,
    announced: bool,
    complete: bool,
    source: ItemId,
    target: ItemId,
    selection_announced: bool,
    expected_target: Vec<ItemId>,
    remaining_source: Vec<ItemId>,
}

impl DockingSmoke {
    pub(super) fn new() -> Self {
        let (source, target) = match std::env::var("EGUI_DOCKSPACE_DOCKING_CASE").as_deref() {
            Ok("console") => (HISTORY, CONSOLE),
            Ok("inspector") => (HISTORY, INSPECTOR),
            Ok("preview") => (HISTORY, PREVIEW),
            Ok("profiler") => (ASSETS, PROFILER),
            Ok("outline") | Err(_) => (HISTORY, OUTLINE),
            Ok(value) => panic!("unknown editor docking case: {value}"),
        };
        Self {
            started: Instant::now(),
            tabs: Arc::default(),
            announced: false,
            complete: false,
            source,
            target,
            selection_announced: false,
            expected_target: Vec::new(),
            remaining_source: Vec::new(),
        }
    }

    pub(super) fn install(&self, context: &egui::Context) {
        context.enable_accesskit();
        context.add_plugin(TabProbe(Arc::clone(&self.tabs)));
    }

    pub(super) fn advance(&mut self, app: &NativeExampleApp, context: &egui::Context) {
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
            && let Some(rect) = self.tabs.lock().unwrap().0.get(target_title).copied()
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
        if !self.announced
            && ready
            && target_selected
            && let Some(source) = self.tabs.lock().unwrap().0.get(source_title).copied()
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
            self.announced = true;
        }
        let docked = app.dockspace.with_view(|view| {
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
        });
        if self.announced && docked && app.dockspace.is_quiescent() {
            if !self.complete {
                eprintln!(
                    "editor-docking-complete: {source_title} docked into the actual {target_title} content"
                );
                self.complete = true;
            }
            context.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        context.request_repaint_after(Duration::from_millis(16));
    }
}
