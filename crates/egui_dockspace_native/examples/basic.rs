#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::collections::BTreeMap;

use dockspace::model::{
    DockspaceAxis, DockspaceLayout, DockspaceNode, DockspaceRootLayout, DockspaceSurfaceLayout,
    ItemId, RootId, SurfaceId,
};
use dockspace::policy::DockPolicy;
use dockspace::runtime::DockspaceSession;
use eframe::egui;
use egui_dockspace::{DockStyle, PaneView};
use egui_dockspace_native::NativeDockspaceApp;

const SURFACE: SurfaceId = SurfaceId::new(1);
const ROOT: RootId = RootId::new(1);
const OUTLINE: ItemId = ItemId::new(1);
const EDITOR: ItemId = ItemId::new(2);
const PREVIEW: ItemId = ItemId::new(3);

fn main() -> eframe::Result {
    let session = DockspaceSession::from_layout(example_layout(), DockPolicy::default())
        .expect("the static native example layout is valid");
    let app = NativeDockspaceApp::new(
        egui::Id::new("native-dockspace-example"),
        session,
        SURFACE,
        ExamplePanes::new(),
        DockStyle::default(),
    )
    .expect("the native coordinator initializes");
    let native_host = app.native_host_handler();
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([1100.0, 720.0]),
        native_host: Some(native_host),
        ..Default::default()
    };
    eframe::run_native(
        "egui_dockspace native",
        options,
        Box::new(|_| Ok(Box::new(app))),
    )
}

fn example_layout() -> DockspaceLayout {
    let root = DockspaceNode::split(
        DockspaceAxis::Horizontal,
        [
            (DockspaceNode::tabs([OUTLINE]), 0.24),
            (DockspaceNode::central_tabs([EDITOR, PREVIEW]), 0.76),
        ],
    )
    .expect("the static split is valid");
    DockspaceLayout::new([DockspaceSurfaceLayout::new(
        SURFACE,
        DockspaceRootLayout::new(ROOT, root),
    )])
    .expect("the static surface is valid")
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
                    OUTLINE,
                    ExamplePane {
                        title: "Outline",
                        text: "src\n  main.rs\n  docking.rs".to_owned(),
                    },
                ),
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
                        text: "Drag tabs or splitters in this native host.".to_owned(),
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
            ui.label("Missing pane");
            return;
        };
        ui.heading(pane.title);
        ui.add(
            egui::TextEdit::multiline(&mut pane.text)
                .desired_width(f32::INFINITY)
                .desired_rows(16),
        );
    }
}
