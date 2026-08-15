#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::collections::BTreeMap;

use eframe::egui;
use egui_dockspace::{
    Dockspace, DockspaceAxis, DockspaceDocumentBootstrap, DockspaceDocumentId, DockspaceLayout,
    DockspaceNode, DockspaceRootLayout, DockspaceSurfaceLayout, ItemId, PaneView, RootId,
    SurfaceId,
};

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
    saved: Option<Vec<u8>>,
    status: String,
}

impl PersistenceApp {
    fn new() -> Self {
        let mut bootstrap = DockspaceDocumentBootstrap::new(PERSISTENCE_DOCUMENT_ID);
        let document = bootstrap
            .ensure_item("pane/document")
            .expect("the document identity must fit");
        let console = bootstrap
            .ensure_item("pane/console")
            .expect("the console identity must fit");
        let dockspace = Dockspace::builder("persistence", example_layout(document, console))
            .persistence(bootstrap)
            .build()
            .expect("the static example layout is valid");
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
            .restore_document_json(snapshot, |document_id, key| {
                document_id == PERSISTENCE_DOCUMENT_ID
                    && matches!(key, "pane/document" | "pane/console")
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

fn example_layout(document_item: ItemId, console_item: ItemId) -> DockspaceLayout {
    let root = DockspaceNode::split(
        DockspaceAxis::Vertical,
        [
            (DockspaceNode::central_tabs([document_item]), 0.72),
            (DockspaceNode::tabs([console_item]), 0.28),
        ],
    )
    .expect("the static split is valid");
    DockspaceLayout::new([DockspaceSurfaceLayout::new(
        SURFACE,
        DockspaceRootLayout::new(ROOT, root),
    )])
    .expect("the example layout is valid")
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
