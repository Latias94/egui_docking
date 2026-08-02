#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::collections::BTreeMap;

use dockspace::document::{DockspaceDocumentBootstrap, DockspaceDocumentId};
use dockspace::graph::{Axis, Node, RootRecord, SurfacePresentation, Workspace};
use dockspace::ids::{ItemId, RootId, SurfaceId};
use eframe::egui;
use egui_dockspace::{Dockspace, PaneView};

const SURFACE: SurfaceId = SurfaceId::new(1);
const ROOT: RootId = RootId::new(1);
const PERSISTENCE_DOCUMENT_ID: DockspaceDocumentId =
    DockspaceDocumentId::from_bytes(*b"egui-example-doc");

fn main() -> eframe::Result {
    eframe::run_native(
        "egui_dockspace persistence",
        eframe::NativeOptions::default(),
        Box::new(|_| Ok(Box::new(PersistenceApp::new()))),
    )
}

struct PersistenceApp {
    dockspace: Dockspace,
    panes: PersistencePanes,
    saved: Option<String>,
    status: String,
}

impl PersistenceApp {
    fn new() -> Self {
        let mut bootstrap = DockspaceDocumentBootstrap::new(PERSISTENCE_DOCUMENT_ID, 0);
        let document = bootstrap
            .ensure_external_item_key("pane/document")
            .expect("the document identity must fit");
        let console = bootstrap
            .ensure_external_item_key("pane/console")
            .expect("the console identity must fit");
        let mut dockspace = Dockspace::builder("persistence", example_workspace(document, console))
            .build()
            .expect("the static example workspace is valid");
        dockspace
            .bind_document_persistence(bootstrap)
            .expect("the static persistence identity must bind");
        Self {
            dockspace,
            panes: PersistencePanes::new(document, console),
            saved: None,
            status: "No snapshot".to_owned(),
        }
    }

    fn save(&mut self) {
        match self.dockspace.save_document_json() {
            Ok(snapshot) => {
                self.saved = Some(snapshot);
                "Snapshot saved".clone_into(&mut self.status);
            }
            Err(error) => self.status = error.to_string(),
        }
    }

    fn restore(&mut self) {
        let Some(snapshot) = self.saved.as_deref() else {
            "No snapshot".clone_into(&mut self.status);
            return;
        };
        match self
            .dockspace
            .load_document_json(snapshot, |document_id, item, key| {
                document_id == PERSISTENCE_DOCUMENT_ID
                    && ((item == ItemId::new(1) && key == "pane/document")
                        || (item == ItemId::new(2) && key == "pane/console"))
            }) {
            Ok(_) => "Snapshot restored".clone_into(&mut self.status),
            Err(error) => self.status = error.to_string(),
        }
    }
}

impl eframe::App for PersistenceApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        egui::Panel::top("snapshot-controls").show(ui, |ui| {
            ui.horizontal(|ui| {
                if ui.button("Save snapshot").clicked() {
                    self.save();
                }
                if ui
                    .add_enabled(self.saved.is_some(), egui::Button::new("Restore snapshot"))
                    .clicked()
                {
                    self.restore();
                }
                ui.label(&self.status);
            });
        });
        egui::CentralPanel::default().show(ui, |ui| {
            if let Err(error) = self
                .dockspace
                .show_single_surface(SURFACE, ui, &mut self.panes)
            {
                self.status = error.to_string();
            }
        });
    }
}

fn example_workspace(document_item: ItemId, console_item: ItemId) -> Workspace {
    let mut builder = Workspace::builder();
    let document = builder.insert_node(Node::tabs([document_item]));
    let console = builder.insert_node(Node::tabs([console_item]));
    let root = builder.insert_node(
        Node::split(Axis::Vertical, [document, console], [0.72, 0.28])
            .expect("the static split is valid"),
    );
    builder.set_root(ROOT, RootRecord::new(root).with_central(document));
    builder.set_surface(SURFACE, SurfacePresentation::with_main(ROOT));
    builder.build().expect("the example workspace is valid")
}

struct PersistencePanes {
    items: BTreeMap<ItemId, (&'static str, String)>,
}

impl PersistencePanes {
    fn new(document: ItemId, console: ItemId) -> Self {
        Self {
            items: BTreeMap::from([
                (
                    document,
                    ("Document", "Persistent document state".to_owned()),
                ),
                (console, ("Console", "Ready".to_owned())),
            ]),
        }
    }
}

impl PaneView for PersistencePanes {
    fn title(&self, item: ItemId) -> Option<egui::WidgetText> {
        self.items.get(&item).map(|(title, _)| (*title).into())
    }

    fn ui(&mut self, item: ItemId, ui: &mut egui::Ui) {
        if let Some((_, text)) = self.items.get_mut(&item) {
            ui.add(egui::TextEdit::multiline(text).desired_width(f32::INFINITY));
        }
    }
}
