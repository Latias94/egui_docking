#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")] // hide console window on Windows in release

use eframe::egui;

#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
#[derive(Clone)]
struct Pane(u32);

struct Delegate;

impl egui_docking::Behavior<Pane> for Delegate {
    fn tab_title_for_pane(&mut self, pane: &Pane) -> egui::WidgetText {
        format!("Pane {}", pane.0).into()
    }

    fn pane_ui(
        &mut self,
        ui: &mut egui::Ui,
        _tile_id: egui_docking::TileId,
        _pane: &mut Pane,
    ) -> egui_docking::UiResponse {
        // Whole pane is draggable as a demo
        let resp = ui
            .allocate_rect(ui.max_rect(), egui::Sense::click_and_drag())
            .on_hover_cursor(egui::CursorIcon::Grab);
        if resp.drag_started() {
            egui_docking::UiResponse::DragStarted
        } else {
            egui_docking::UiResponse::None
        }
    }
}

#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
struct App {
    tree: egui_docking::Tree<Pane>,
}

impl Default for App {
    fn default() -> Self {
        let mut tiles = egui_docking::Tiles::default();
        let a = tiles.insert_pane(Pane(1));
        let b = tiles.insert_pane(Pane(2));
        let c = tiles.insert_pane(Pane(3));
        let root = tiles.insert_tab_tile(vec![a, b, c]);
        Self { tree: egui_docking::Tree::new("demo_tree", root, tiles) }
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::TopBottomPanel::top("top").show(ctx, |ui| {
            ui.label("Tip: drag a tab outside to tear-off into a floating window. Drag it back to dock.");
        });

        egui::CentralPanel::default().show(ctx, |ui| {
            let mut delegate = Delegate;
            self.tree.ui(&mut delegate, ui);
        });
    }
}

fn main() -> Result<(), eframe::Error> {
    env_logger::init();
    let options = eframe::NativeOptions::default();
    eframe::run_native(
        "egui_docking - tear-off demo",
        options,
        Box::new(|_cc| Ok(Box::<App>::default())),
    )
}
