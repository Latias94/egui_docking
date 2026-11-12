#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::collections::BTreeMap;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use eframe::egui;
use egui_dockspace::{
    CloseDecision, CloseItemDecisionState, DockAnchor, DockPlacement, Dockspace,
    DockspaceActionStatus, DockspaceAxis, DockspaceDocumentBootstrap, DockspaceDocumentId,
    DockspaceLayout, DockspaceNode, DockspaceRootLayout, DockspaceSurfaceLayout, ItemId, PaneView,
    RootId, SurfaceId,
};

const SURFACE: SurfaceId = SurfaceId::new(1);
const ROOT: RootId = RootId::new(1);
const PERSISTENCE_DOCUMENT_ID: DockspaceDocumentId =
    DockspaceDocumentId::from_bytes(*b"egui-example-doc");
const DEFAULT_PRESET: &str = "Default";
const MAX_USER_PRESETS: usize = 8;
const PANES: [(&str, &str); 8] = [
    ("pane/document", "Document"),
    ("pane/console", "Console"),
    ("pane/scene", "Scene"),
    ("pane/outline", "Outline"),
    ("pane/inspector", "Inspector"),
    ("pane/history", "History"),
    ("pane/profiler", "Profiler"),
    ("pane/assets", "Assets"),
];

fn main() -> eframe::Result {
    let layout_file = std::env::args_os()
        .nth(1)
        .map_or_else(|| PathBuf::from("dockspace-layout.json"), PathBuf::from);
    eframe::run_native(
        "egui_dockspace editor layouts",
        eframe::NativeOptions {
            viewport: egui::ViewportBuilder::default().with_inner_size([1200.0, 780.0]),
            ..Default::default()
        },
        Box::new(move |creation| {
            creation.egui_ctx.set_theme(egui::Theme::Light);
            Ok(Box::new(PersistenceApp::open(layout_file)))
        }),
    )
}

struct PersistenceApp {
    dockspace: Dockspace,
    panes: PersistencePanes,
    default_layout: DockspaceLayout,
    // Names, storage and closed-pane data belong to the application, not the layout engine.
    presets: BTreeMap<String, Vec<u8>>,
    preset_name: String,
    status: String,
    layout_file: Option<PathBuf>,
    save_on_exit: bool,
}

impl PersistenceApp {
    fn new() -> Self {
        let mut bootstrap = DockspaceDocumentBootstrap::new(PERSISTENCE_DOCUMENT_ID);
        let items: Vec<_> = PANES
            .iter()
            .map(|(key, _)| {
                bootstrap
                    .ensure_item(*key)
                    .expect("the bounded pane identities fit")
            })
            .collect();
        let default_layout = example_layout(&items);
        let mut dockspace = Dockspace::builder("persistence", default_layout.clone())
            .persistence(bootstrap)
            .build()
            .expect("the static editor layout is valid");
        let default = dockspace
            .save_document_json()
            .expect("the default layout encodes");
        Self {
            dockspace,
            panes: PersistencePanes::new(&items),
            default_layout,
            presets: BTreeMap::from([(DEFAULT_PRESET.to_owned(), default)]),
            preset_name: "Working".to_owned(),
            status: "Layouts stay in application memory; pane data survives every layout change"
                .to_owned(),
            layout_file: None,
            save_on_exit: false,
        }
    }

    fn open(path: PathBuf) -> Self {
        let mut app = Self::new();
        match std::fs::read(&path) {
            Ok(bytes) => match Self::decode(&bytes) {
                Ok((dockspace, items)) => {
                    app.dockspace = dockspace;
                    app.default_layout = example_layout(&items);
                    app.panes = PersistencePanes::new(&items);
                    app.status = format!("Restored {}", path.display());
                    app.save_on_exit = true;
                }
                Err(error) => {
                    app.status = format!(
                        "Could not restore {}: {error}. Save layout file to replace it.",
                        path.display()
                    );
                }
            },
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                app.status = format!("Layout saves to {} on exit", path.display());
                app.save_on_exit = true;
            }
            Err(error) => app.status = format!("Could not read {}: {error}", path.display()),
        }
        app.layout_file = Some(path);
        app
    }

    fn decode(bytes: &[u8]) -> Result<(Dockspace, Vec<ItemId>), Box<dyn std::error::Error>> {
        let dockspace = Dockspace::from_document_json(
            "persistence",
            bytes,
            Default::default(),
            Default::default(),
            |document, key| {
                document == PERSISTENCE_DOCUMENT_ID && PANES.iter().any(|(known, _)| *known == key)
            },
        )?;
        // Reconnect the application registry through stable keys, not saved numeric IDs.
        let items = PANES
            .iter()
            .map(|(key, _)| {
                dockspace.item_id_for_external_key(key).ok_or_else(|| {
                    io::Error::new(
                        io::ErrorKind::InvalidData,
                        format!("missing pane key: {key}"),
                    )
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok((dockspace, items))
    }

    fn write_layout(&mut self, path: &Path) -> Result<(), Box<dyn std::error::Error>> {
        let bytes = self.dockspace.save_document_json()?;
        let parent = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        let mut file = tempfile::NamedTempFile::new_in(parent)?;
        file.write_all(&bytes)?;
        file.as_file().sync_all()?;
        file.persist(path)?;
        Ok(())
    }

    fn save_layout_file(&mut self) {
        let Some(path) = self.layout_file.clone() else {
            return;
        };
        match self.write_layout(&path) {
            Ok(()) => {
                self.status = format!("Saved {}", path.display());
                self.save_on_exit = true;
            }
            Err(error) => self.status = format!("Could not save {}: {error}", path.display()),
        }
    }

    fn save(&mut self) {
        let name = self.preset_name.trim();
        if name.is_empty() || name == DEFAULT_PRESET {
            "Choose a non-empty name other than Default".clone_into(&mut self.status);
            return;
        }
        if !self.presets.contains_key(name) && self.presets.len() > MAX_USER_PRESETS {
            "The eight user layout slots are full; replace or remove one"
                .clone_into(&mut self.status);
            return;
        }
        match self.dockspace.save_document_json() {
            Ok(snapshot) => {
                self.presets.insert(name.to_owned(), snapshot);
                self.status = format!("Saved layout: {name}");
            }
            Err(error) => self.status = error.to_string(),
        }
    }

    fn restore(&mut self) {
        let name = self.preset_name.trim().to_owned();
        self.restore_named(&name);
    }

    fn restore_named(&mut self, name: &str) {
        if name == DEFAULT_PRESET {
            // Defaults are ordinary layouts; applying one does not need JSON or document authority.
            match self.dockspace.reset_layout(self.default_layout.clone()) {
                Ok(_) => self.status = format!("Applied layout: {name}"),
                Err(error) => self.status = error.to_string(),
            }
            return;
        }
        let Some(snapshot) = self.presets.get(name) else {
            "Choose a saved layout".clone_into(&mut self.status);
            return;
        };
        match self
            .dockspace
            .restore_document_json(snapshot, |document_id, key| {
                document_id == PERSISTENCE_DOCUMENT_ID
                    && PANES.iter().any(|(known, _)| *known == key)
            }) {
            Ok(_) => self.status = format!("Applied layout: {name}"),
            Err(error) => self.status = error.to_string(),
        }
    }

    fn reopen(&mut self, item: ItemId) {
        if !self.panes.items.contains_key(&item) {
            return;
        }
        // Resolve the current presentation: restoring an old preset can remap retired IDs.
        let view = self.dockspace.view();
        let Some(surface) = view.surfaces().next() else {
            "No logical surface is available".clone_into(&mut self.status);
            return;
        };
        let placement = match surface.main_root() {
            None => DockPlacement::Main(surface.id()),
            Some(root) if root.central().is_some() => {
                DockPlacement::Center(DockAnchor::Central(root.id()))
            }
            Some(root) => {
                let anchor = self.panes.items.keys().copied().find(|item| {
                    view.item(*item)
                        .is_some_and(|open| open.root() == root.id())
                });
                let Some(anchor) = anchor else {
                    "The current layout has no available docking anchor"
                        .clone_into(&mut self.status);
                    return;
                };
                DockPlacement::Center(DockAnchor::Item(anchor))
            }
        };
        match self.dockspace.open_item_current(item, placement) {
            Ok(result) => match result.status() {
                DockspaceActionStatus::Applied(_) => "Pane reopened".clone_into(&mut self.status),
                status => self.status = format!("Open pane: {status:?}"),
            },
            Err(error) => self.status = error.to_string(),
        }
    }

    fn set_locked(&mut self, locked: bool) {
        let mut policy = self.dockspace.policy().clone();
        policy.set_arrangement_locked(locked);
        match self.dockspace.set_policy(policy) {
            Ok(_) => {
                self.status = if locked {
                    "Layout locked"
                } else {
                    "Layout unlocked"
                }
                .to_owned()
            }
            Err(error) => self.status = error.to_string(),
        }
    }
}

impl eframe::App for PersistenceApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        egui::Panel::top("layout-controls").show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.strong("Editor layouts");
                if ui.button("Save layout file").clicked() {
                    self.save_layout_file();
                }
                let mut locked = self.dockspace.policy().arrangement_locked();
                if ui.checkbox(&mut locked, "Lock layout").changed() {
                    self.set_locked(locked);
                }
                ui.label("Name:");
                ui.add(egui::TextEdit::singleline(&mut self.preset_name).desired_width(130.0));
                if ui.button("Save preset").clicked() {
                    self.save();
                }
                if ui
                    .add_enabled(
                        !locked && self.presets.contains_key(self.preset_name.trim()),
                        egui::Button::new("Apply preset"),
                    )
                    .clicked()
                {
                    self.restore();
                }
                if ui
                    .add_enabled(!locked, egui::Button::new("Reset to default"))
                    .clicked()
                {
                    self.restore_named(DEFAULT_PRESET);
                }
                ui.menu_button("Saved layouts", |ui| {
                    let names: Vec<_> = self.presets.keys().cloned().collect();
                    for name in names {
                        ui.horizontal(|ui| {
                            if ui.add_enabled(!locked, egui::Button::new(&name)).clicked() {
                                self.preset_name = name.clone();
                                self.restore_named(&name);
                                ui.close();
                            }
                            if name != DEFAULT_PRESET && ui.small_button("Remove").clicked() {
                                self.presets.remove(&name);
                            }
                        });
                    }
                });
                ui.menu_button("Closed panes", |ui| {
                    let closed: Vec<_> = self
                        .panes
                        .items
                        .iter()
                        .filter_map(|(item, pane)| {
                            self.dockspace
                                .view()
                                .item(*item)
                                .is_none()
                                .then_some((*item, pane.title))
                        })
                        .collect();
                    if closed.is_empty() {
                        ui.weak("Every pane is open");
                    }
                    for (item, title) in closed {
                        if ui
                            .add_enabled(!locked, egui::Button::new(format!("Reopen {title}")))
                            .clicked()
                        {
                            self.reopen(item);
                            ui.close();
                        }
                    }
                });
            });
            ui.label(&self.status);
            ui.weak("For drag docking, run the workbench example.");
        });
        egui::CentralPanel::default().show(ui, |ui| {
            let surface = self
                .dockspace
                .view()
                .surfaces()
                .next()
                .map(|surface| surface.id());
            let Some(surface) = surface else {
                ui.weak("No logical surface is available");
                return;
            };
            match self
                .dockspace
                .show_single_surface(surface, ui, &mut self.panes)
            {
                Ok(response) => {
                    for request in response.close_request_events() {
                        if request.reused() {
                            continue;
                        }
                        let plan = request.plan();
                        for item in plan.items() {
                            if item.state() != CloseItemDecisionState::Pending {
                                continue;
                            }
                            // Only layout ownership closes. Application content remains in the catalog.
                            match self.dockspace.resolve_close(
                                plan.request(),
                                item.token(),
                                CloseDecision::Allow,
                            ) {
                                Ok(result) => {
                                    if result.mutation().workspace_changed() {
                                        ui.ctx().request_repaint();
                                    }
                                }
                                Err(error) => self.status = error.to_string(),
                            }
                        }
                    }
                }
                Err(error) => self.status = error.to_string(),
            }
        });
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        if self.save_on_exit {
            self.save_layout_file();
            eprintln!("{}", self.status);
        }
    }
}

fn example_layout(items: &[ItemId]) -> DockspaceLayout {
    let center = DockspaceNode::split(
        DockspaceAxis::Vertical,
        [
            (
                DockspaceNode::central_tabs([items[0], items[2], items[5], items[6], items[7]]),
                0.75,
            ),
            (DockspaceNode::tabs([items[1]]), 0.25),
        ],
    )
    .expect("the center split is valid");
    let root = DockspaceNode::split(
        DockspaceAxis::Horizontal,
        [
            (DockspaceNode::tabs([items[3]]), 0.18),
            (center, 0.62),
            (DockspaceNode::tabs([items[4]]), 0.20),
        ],
    )
    .expect("the editor split is valid");
    DockspaceLayout::new([DockspaceSurfaceLayout::new(
        SURFACE,
        DockspaceRootLayout::new(ROOT, root),
    )])
    .expect("the editor layout is valid")
}

struct EditorPane {
    title: &'static str,
    text: String,
    canvas: bool,
}

struct PersistencePanes {
    items: BTreeMap<ItemId, EditorPane>,
}

impl PersistencePanes {
    fn new(items: &[ItemId]) -> Self {
        Self {
            items: items.iter().copied().zip(PANES).map(|(item, (key, title))| {
                (item, EditorPane { title, text: format!("{title} application data\n{key}\n\nEdits survive closing, reopening and layout presets."), canvas: key == "pane/scene" })
            }).collect(),
        }
    }
}

impl PaneView for PersistencePanes {
    fn title(&self, item: ItemId) -> Option<egui::WidgetText> {
        self.items.get(&item).map(|pane| pane.title.into())
    }

    fn ui(&mut self, item: ItemId, ui: &mut egui::Ui) -> egui_dockspace::PaneResponse {
        let Some(pane) = self.items.get_mut(&item) else {
            return egui_dockspace::PaneResponse::default();
        };
        if pane.canvas {
            let (response, painter) =
                ui.allocate_painter(ui.available_size(), egui::Sense::click());
            let rect = response.rect;
            painter.rect_filled(rect, 0.0, ui.visuals().extreme_bg_color);
            for index in 1..12 {
                let x = egui::lerp(rect.x_range(), index as f32 / 12.0);
                painter.line_segment(
                    [egui::pos2(x, rect.top()), egui::pos2(x, rect.bottom())],
                    ui.visuals().widgets.noninteractive.bg_stroke,
                );
            }
            painter.circle_filled(rect.center(), 32.0, ui.visuals().selection.bg_fill);
            painter.text(
                rect.center() + egui::vec2(0.0, 52.0),
                egui::Align2::CENTER_CENTER,
                "Application-drawn scene",
                egui::FontId::proportional(16.0),
                ui.visuals().text_color(),
            );
        } else {
            egui::ScrollArea::both()
                .id_salt(("editor-pane-scroll", item.get()))
                .show(ui, |ui| {
                    ui.add(
                        egui::TextEdit::multiline(&mut pane.text)
                            .id(egui::Id::unique(("editor-pane-focus", item.get())))
                            .desired_width(f32::INFINITY),
                    );
                    if pane.title == "Console" {
                        for line in 0..128 {
                            ui.monospace(format!("[{line:03}] Application event log"));
                        }
                    }
                });
        }
        egui_dockspace::PaneResponse::default()
    }

    fn focus_target(&self, item: ItemId) -> Option<egui::Id> {
        self.items
            .get(&item)
            .filter(|pane| !pane.canvas)
            .map(|_| egui::Id::unique(("editor-pane-focus", item.get())))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::accesskit::{Action, ActionRequest, Role, TreeId};

    #[test]
    fn layout_file_round_trip_preserves_tab_order_and_reconnects_application_keys() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("layout.json");
        let mut app = PersistenceApp::open(path.clone());
        let document = app
            .dockspace
            .item_id_for_external_key("pane/document")
            .unwrap();
        let inspector = app
            .dockspace
            .item_id_for_external_key("pane/inspector")
            .unwrap();
        app.dockspace
            .dock_item_current(document, DockPlacement::Center(DockAnchor::Item(inspector)))
            .unwrap();
        app.save_layout_file();
        assert!(path.is_file());
        let restored = PersistenceApp::open(path.clone());
        assert_eq!(
            restored
                .dockspace
                .view()
                .item(document)
                .unwrap()
                .tabs()
                .items(),
            &[inspector, document]
        );
        assert_eq!(restored.panes.items[&document].title, "Document");
        assert_eq!(restored.status, format!("Restored {}", path.display()));

        // A second save replaces the existing file atomically on supported platforms.
        app.dockspace
            .dock_item_current(inspector, DockPlacement::Center(DockAnchor::Item(document)))
            .unwrap();
        app.save_layout_file();
        assert!(app.status.starts_with("Saved "));
        assert!(PersistenceApp::open(path).save_on_exit);
    }

    #[test]
    fn invalid_layout_file_is_kept_until_an_explicit_save() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("layout.json");
        let original = b"an invalid or newer document";
        std::fs::write(&path, original).unwrap();
        let mut app = PersistenceApp::open(path.clone());
        assert!(!app.save_on_exit);
        assert!(app.status.starts_with("Could not restore "));
        eframe::App::on_exit(&mut app, None);
        assert_eq!(std::fs::read(&path).unwrap(), original);
        app.save_layout_file();
        assert!(app.save_on_exit);
        assert!(PersistenceApp::decode(&std::fs::read(path).unwrap()).is_ok());
    }

    fn run_frame(
        context: &egui::Context,
        app: &mut PersistenceApp,
        events: Vec<egui::Event>,
    ) -> egui::FullOutput {
        let mut frame = eframe::Frame::_new_kittest();
        let mut output = context.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1100.0, 720.0),
                )),
                events,
                ..Default::default()
            },
            |ui| eframe::App::ui(app, ui, &mut frame),
        );
        output.textures_delta.clear();
        output
    }

    #[test]
    fn closed_inventory_reopens_stable_items_and_presets_preserve_application_data() {
        let context = egui::Context::default();
        context.enable_accesskit();
        let mut app = PersistenceApp::new();
        let document = app
            .dockspace
            .item_id_for_external_key("pane/document")
            .expect("the document has a persistent identity");
        let text = "Unsaved application text survives closing and layout restore";
        text.clone_into(&mut app.panes.items.get_mut(&document).expect("pane data").text);
        app.save();
        assert_eq!(app.status, "Saved layout: Working");
        let _ = run_frame(&context, &mut app, Vec::new());
        let ready = run_frame(&context, &mut app, Vec::new());
        let close = ready
            .platform_output
            .accesskit_update
            .as_ref()
            .expect("the example exposes its controls")
            .nodes
            .iter()
            .find_map(|(id, node)| {
                (node.role() == Role::Button
                    && node.label() == Some("Close Document")
                    && node.supports_action(Action::Click))
                .then_some(*id)
            })
            .expect("the document has an enabled close button");
        assert!(app.dockspace.view().item(document).is_some());

        let _ = run_frame(
            &context,
            &mut app,
            vec![egui::Event::AccessKitActionRequest(ActionRequest {
                action: Action::Click,
                target_tree: TreeId::ROOT,
                target_node: close,
                data: None,
            })],
        );

        // The encoded close action is consumed by the next update.
        assert!(app.dockspace.view().item(document).is_some());
        let _ = run_frame(&context, &mut app, Vec::new());
        assert!(app.dockspace.view().item(document).is_none());
        assert_eq!(
            app.panes
                .items
                .get(&document)
                .expect("closed pane data")
                .text,
            text
        );
        "Closed document".clone_into(&mut app.preset_name);
        app.save();
        app.reopen(document);
        assert!(app.dockspace.view().item(document).unwrap().is_selected());
        assert_eq!(app.panes.items[&document].text, text);
        let _ = run_frame(&context, &mut app, Vec::new());
        let reopened = app.dockspace.version();
        app.reopen(document);
        assert_eq!(
            app.dockspace.version(),
            reopened,
            "duplicate reopen is a no-op"
        );

        app.restore();
        assert_eq!(app.status, "Applied layout: Closed document");
        assert!(app.dockspace.view().item(document).is_none());
        app.restore_named(DEFAULT_PRESET);
        assert!(app.dockspace.view().item(document).is_some());
        assert_eq!(app.panes.items[&document].text, text);
        "Working".clone_into(&mut app.preset_name);
        app.restore();
        assert_eq!(app.status, "Applied layout: Working");
        assert!(app.dockspace.view().item(document).is_some());
        assert_eq!(
            app.panes
                .items
                .get(&document)
                .expect("restored pane data")
                .text,
            text
        );
        let restored = run_frame(&context, &mut app, Vec::new());
        assert_eq!(app.status, "Applied layout: Working");
        assert!(
            restored
                .platform_output
                .accesskit_update
                .as_ref()
                .expect("the restored example exposes its panes")
                .nodes
                .iter()
                .any(|(_, node)| node.role() == Role::Tab && node.label() == Some("Document")),
            "the restored layout renders the retained application pane"
        );
    }

    #[test]
    fn named_layout_storage_is_bounded_and_default_is_immutable() {
        let mut app = PersistenceApp::new();
        let default = app.presets[DEFAULT_PRESET].clone();
        for index in 0..MAX_USER_PRESETS {
            app.preset_name = format!("Layout {index}");
            app.save();
        }
        assert_eq!(app.presets.len(), MAX_USER_PRESETS + 1);
        let generation = app.dockspace.next_document_generation();
        "Extra".clone_into(&mut app.preset_name);
        app.save();
        assert_eq!(app.presets.len(), MAX_USER_PRESETS + 1);
        assert_eq!(app.dockspace.next_document_generation(), generation);
        "Layout 0".clone_into(&mut app.preset_name);
        app.save();
        assert_eq!(app.status, "Saved layout: Layout 0");
        assert_eq!(app.presets.len(), MAX_USER_PRESETS + 1);
        DEFAULT_PRESET.clone_into(&mut app.preset_name);
        app.save();
        assert_eq!(app.presets[DEFAULT_PRESET], default);
        app.set_locked(true);
        let version = app.dockspace.version();
        app.restore_named(DEFAULT_PRESET);
        assert!(app.dockspace.policy().arrangement_locked());
        assert_eq!(app.dockspace.version(), version);
        app.set_locked(false);
        app.restore_named(DEFAULT_PRESET);
        assert_eq!(app.status, "Applied layout: Default");
    }

    #[test]
    fn root_retirement_and_default_restore_use_current_presentation_identities() {
        use egui_dockspace::DockspaceCloseRequestStatus;

        let context = egui::Context::default();
        context.enable_accesskit();
        let mut app = PersistenceApp::new();
        let _ = run_frame(&context, &mut app, Vec::new());
        let document = app
            .dockspace
            .item_id_for_external_key("pane/document")
            .unwrap();
        let close = app.dockspace.request_close_root_current(ROOT).unwrap();
        let DockspaceCloseRequestStatus::Requested(request) = close.status() else {
            panic!("the editor root must offer an ordinary application close plan");
        };
        let plan = request.plan();
        for item in plan.items() {
            app.dockspace
                .resolve_close(plan.request(), item.token(), CloseDecision::Allow)
                .unwrap();
        }
        assert!(app.dockspace.view().root(ROOT).is_none());
        assert!(app.dockspace.view().surfaces().next().is_none());
        app.reopen(document);
        assert_eq!(app.status, "No logical surface is available");

        app.restore_named(DEFAULT_PRESET);
        assert_eq!(app.status, "Applied layout: Default");
        let surface = app.dockspace.view().surfaces().next().unwrap();
        assert_eq!(surface.id(), SURFACE);
        let root = surface.main_root().unwrap().id();
        assert_eq!(root, ROOT);
        let close = app.dockspace.request_close_item_current(document).unwrap();
        let DockspaceCloseRequestStatus::Requested(request) = close.status() else {
            panic!("the restored document must remain closeable");
        };
        let plan = request.plan();
        app.dockspace
            .resolve_close(
                plan.request(),
                plan.items()[0].token(),
                CloseDecision::Allow,
            )
            .unwrap();
        app.reopen(document);
        assert_eq!(app.status, "Pane reopened");
        assert_eq!(app.dockspace.view().item(document).unwrap().root(), root);
        let output = run_frame(&context, &mut app, Vec::new());
        assert!(
            output
                .platform_output
                .accesskit_update
                .as_ref()
                .unwrap()
                .nodes
                .iter()
                .any(|(_, node)| node.role() == Role::Tab && node.label() == Some("Document"))
        );
    }
}
