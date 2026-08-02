#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::collections::BTreeMap;

use dockspace::geometry::LogicalRect;
use dockspace::graph::{Axis, ContainedFloating, Node, RootRecord, SurfacePresentation, Workspace};
use dockspace::ids::{FloatingPresentationId, ItemId, RootId, SurfaceId};
use eframe::egui;
use egui_dockspace::{Dockspace, PaneView};

const SURFACE: SurfaceId = SurfaceId::new(1);
const MAIN_ROOT: RootId = RootId::new(1);
const FLOATING_ROOT: RootId = RootId::new(2);
const FLOATING: FloatingPresentationId = FloatingPresentationId::new(1);

const EDITOR: ItemId = ItemId::new(1);
const PREVIEW: ItemId = ItemId::new(2);
const OUTLINE: ItemId = ItemId::new(3);
const INSPECTOR: ItemId = ItemId::new(4);

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([1100.0, 720.0]),
        ..Default::default()
    };
    eframe::run_native(
        "egui_dockspace",
        options,
        Box::new(|_| Ok(Box::new(DockspaceApp::new()))),
    )
}

struct DockspaceApp {
    dockspace: Dockspace,
    panes: ExamplePanes,
    error: Option<String>,
}

impl DockspaceApp {
    fn new() -> Self {
        let workspace = example_workspace();
        let dockspace = Dockspace::builder("basic", workspace)
            .build()
            .expect("the static example workspace is valid");
        Self {
            dockspace,
            panes: ExamplePanes::new(),
            error: None,
        }
    }
}

impl eframe::App for DockspaceApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        if let Some(error) = &self.error {
            egui::Panel::bottom("dockspace-error").show(ui, |ui| {
                ui.colored_label(ui.visuals().error_fg_color, error);
            });
        }
        egui::CentralPanel::default().show(ui, |ui| {
            match self
                .dockspace
                .show_single_surface(SURFACE, ui, &mut self.panes)
            {
                Ok(_) => self.error = None,
                Err(error) => self.error = Some(error.to_string()),
            }
        });
    }
}

fn example_workspace() -> Workspace {
    let mut builder = Workspace::builder();
    let editors = builder.insert_node(Node::tabs([EDITOR, PREVIEW]));
    let outline = builder.insert_node(Node::tabs([OUTLINE]));
    let main = builder.insert_node(
        Node::split(Axis::Horizontal, [outline, editors], [0.24, 0.76])
            .expect("the static split is valid"),
    );
    let inspector = builder.insert_node(Node::tabs([INSPECTOR]));

    builder.set_root(MAIN_ROOT, RootRecord::new(main).with_central(editors));
    builder.set_root(FLOATING_ROOT, RootRecord::new(inspector));
    builder.set_surface(SURFACE, SurfacePresentation::with_main(MAIN_ROOT));
    builder.set_contained_floating(
        FLOATING,
        ContainedFloating::new(
            FLOATING_ROOT,
            LogicalRect::new(690.0, 90.0, 310.0, 300.0).expect("the static rect is valid"),
        ),
    );
    builder
        .attach_contained(SURFACE, FLOATING)
        .expect("the example surface exists");
    builder.build().expect("the example workspace is valid")
}

struct ExamplePane {
    title: &'static str,
    text: String,
}

struct ExamplePanes {
    panes: BTreeMap<ItemId, ExamplePane>,
}

impl ExamplePanes {
    fn new() -> Self {
        Self {
            panes: BTreeMap::from([
                (
                    EDITOR,
                    ExamplePane {
                        title: "Editor",
                        text: "fn main() {\n    println!(\"dockspace\");\n}".to_owned(),
                    },
                ),
                (
                    PREVIEW,
                    ExamplePane {
                        title: "Preview",
                        text: "Rendered output".to_owned(),
                    },
                ),
                (
                    OUTLINE,
                    ExamplePane {
                        title: "Outline",
                        text: "main\n  editor\n  preview".to_owned(),
                    },
                ),
                (
                    INSPECTOR,
                    ExamplePane {
                        title: "Inspector",
                        text: "Selection: Editor".to_owned(),
                    },
                ),
            ]),
        }
    }
}

impl PaneView for ExamplePanes {
    fn title(&self, item: ItemId) -> Option<egui::WidgetText> {
        self.panes.get(&item).map(|pane| pane.title.into())
    }

    fn ui(&mut self, item: ItemId, ui: &mut egui::Ui) {
        let Some(pane) = self.panes.get_mut(&item) else {
            return;
        };
        ui.add(
            egui::TextEdit::multiline(&mut pane.text)
                .code_editor()
                .desired_width(f32::INFINITY),
        );
    }
}
