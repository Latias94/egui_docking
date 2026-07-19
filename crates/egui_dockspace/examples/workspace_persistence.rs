#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::collections::{BTreeMap, BTreeSet};

use eframe::egui;
use egui_dockspace::dockspace::graph::{Axis, Node, RootRecord, SurfacePresentation, Workspace};
use egui_dockspace::dockspace::ids::{ItemId, RootId, SurfaceId};
use egui_dockspace::{Dockspace, PaneView};

const SURFACE: SurfaceId = SurfaceId::new(1);
const ROOT: RootId = RootId::new(1);
const DOCUMENT: ItemId = ItemId::new(1);
const CONSOLE: ItemId = ItemId::new(2);

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
        let dockspace = Dockspace::builder("persistence", example_workspace())
            .build()
            .expect("the static example workspace is valid");
        Self {
            dockspace,
            panes: PersistencePanes::new(),
            saved: None,
            status: "No snapshot".to_owned(),
        }
    }

    fn save(&mut self) {
        match self.dockspace.save_json() {
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
        let known_items = self.panes.items.keys().copied().collect::<BTreeSet<_>>();
        match self
            .dockspace
            .load_json(snapshot, |item| known_items.contains(&item))
        {
            Ok(_) => "Restore queued".clone_into(&mut self.status),
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
            if let Err(error) = self.dockspace.show(SURFACE, ui, &mut self.panes) {
                self.status = error.to_string();
            }
        });
    }
}

fn example_workspace() -> Workspace {
    let mut builder = Workspace::builder();
    let document = builder.insert_node(Node::tabs([DOCUMENT]));
    let console = builder.insert_node(Node::tabs([CONSOLE]));
    let root = builder.insert_node(
        Node::split(Axis::Vertical, [document, console], [0.72, 0.28])
            .expect("the static split is valid"),
    );
    builder.set_root(ROOT, RootRecord::new(root).with_central(document));
    builder.set_surface(SURFACE, SurfacePresentation::new(ROOT));
    builder.build().expect("the example workspace is valid")
}

struct PersistencePanes {
    items: BTreeMap<ItemId, (&'static str, String)>,
}

impl PersistencePanes {
    fn new() -> Self {
        Self {
            items: BTreeMap::from([
                (
                    DOCUMENT,
                    ("Document", "Persistent document state".to_owned()),
                ),
                (CONSOLE, ("Console", "Ready".to_owned())),
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
