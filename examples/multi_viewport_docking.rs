#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")] // hide console window on Windows in release

use eframe::egui;

#[derive(Clone, Debug)]
struct Pane {
    id: usize,
}

struct App {
    docking: egui_docking::DockingMultiViewport<Pane>,
    behavior: DemoBehavior,
}

#[derive(Default)]
struct DemoBehavior;

impl egui_tiles::Behavior<Pane> for DemoBehavior {
    fn tab_title_for_pane(&mut self, pane: &Pane) -> egui::WidgetText {
        format!("Pane {}", pane.id).into()
    }

    fn pane_ui(
        &mut self,
        ui: &mut egui::Ui,
        tile_id: egui_tiles::TileId,
        pane: &mut Pane,
    ) -> egui_tiles::UiResponse {
        let color = egui::epaint::Hsva::new(0.13 * pane.id as f32, 0.5, 0.6, 1.0);
        ui.painter().rect_filled(ui.max_rect(), 0.0, color);

        ui.horizontal(|ui| {
            ui.add(egui::Label::new(format!("tile: {tile_id:?}")).selectable(false));
            ui.add(
                    egui::Label::new(
                        "Tear-off: drag a tab/pane and release outside the dock. \
                     Live tear-off (ghost): drag outside the dock to spawn a floating ghost window before release (leaving the native window upgrades it to a new native window). \
                     SHIFT: detach the whole tab-group (parent Tabs). \
                     ALT: force tear-off on release even inside the dock. \
                     CTRL: tear-off into a contained floating window (instead of a native window). \
                     Docking overlay targets show while dragging; hover to choose split direction (inner 5-way + outer edge markers; outer shows near dock edges). \
                     To dock back, drag any tab (or tab-bar background) into another window and release.",
                )
                .selectable(false),
            );
        });

        // Make the whole pane draggable:
        if ui
            .allocate_rect(ui.max_rect(), egui::Sense::click_and_drag())
            .drag_started()
        {
            egui_tiles::UiResponse::DragStarted
        } else {
            egui_tiles::UiResponse::None
        }
    }

    fn on_tab_button(
        &mut self,
        _tiles: &egui_tiles::Tiles<Pane>,
        _tile_id: egui_tiles::TileId,
        button_response: egui::Response,
    ) -> egui::Response {
        // Enable dragging by tab title:
        if button_response.drag_started() {
            button_response.ctx.set_dragged_id(button_response.id);
        }
        button_response
    }
}

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([960.0, 640.0])
            .with_title("egui_docking demo"),
        ..Default::default()
    };

    eframe::run_native(
        "egui_docking demo",
        options,
        Box::new(|_cc| Ok(Box::new(App::default()))),
    )
}

impl Default for App {
    fn default() -> Self {
        let mut tiles = egui_tiles::Tiles::default();
        let mut next_id = 0usize;
        let mut panes = Vec::new();
        for _ in 0..3 {
            panes.push(tiles.insert_pane(Pane { id: next_id }));
            next_id += 1;
        }

        let root = tiles.insert_tab_tile(panes);
        let tree = egui_tiles::Tree::new("root_dock", root, tiles);

        let mut docking = egui_docking::DockingMultiViewport::new(tree);
        docking.options.debug_drop_targets = true;
        docking.options.debug_event_log = true;
        docking.options.debug_integrity = true;

        Self {
            docking,
            behavior: DemoBehavior,
        }
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::Panel::top("egui_docking_demo_help").show(ctx, |ui| {
            let modifiers = ctx.input(|i| i.modifiers);
            ui.horizontal(|ui| {
                ui.add(
                    egui::Label::new(format!(
                        "detached: {} | floating: {} | modifiers: ctrl={} shift={} alt={}",
                        self.docking.detached_viewport_count(),
                        self.docking.floating_window_count(),
                        modifiers.ctrl,
                        modifiers.shift,
                        modifiers.alt,
                    ))
                    .selectable(false),
                );
            });
            ui.add(
                egui::Label::new(
                    "Tip: release outside dock = new native window. SHIFT detaches whole tab-group; ALT forces tear-off. \
                     Hold CTRL while tearing off to create a contained floating window. \
                     While dragging, use the overlay targets for center/left/right/top/bottom docking. \
                     Drag into any other window to dock back.",
                )
                .selectable(false),
            );
        });
        self.docking.ui(ctx, &mut self.behavior);
    }
}
