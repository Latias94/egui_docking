#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")] // hide console window on Windows in release

use eframe::egui;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Pane {
    Hierarchy,
    Project,
    AssetBrowser,
    SceneView,
    GameView,
    Console,
    Inspector,
    Performance,
}

struct App {
    docking: egui_docking::DockingMultiViewport<Pane>,
    behavior: Behavior,
}

#[derive(Default)]
struct Behavior;

impl Behavior {
    fn title(pane: &Pane) -> &'static str {
        match pane {
            Pane::Hierarchy => "Hierarchy",
            Pane::Project => "Project",
            Pane::AssetBrowser => "Asset Browser",
            Pane::SceneView => "Scene View",
            Pane::GameView => "Game View",
            Pane::Console => "Console",
            Pane::Inspector => "Inspector",
            Pane::Performance => "Performance",
        }
    }
}

impl egui_tiles::Behavior<Pane> for Behavior {
    fn tab_title_for_pane(&mut self, pane: &Pane) -> egui::WidgetText {
        Behavior::title(pane).into()
    }

    fn pane_ui(
        &mut self,
        ui: &mut egui::Ui,
        _tile_id: egui_tiles::TileId,
        pane: &mut Pane,
    ) -> egui_tiles::UiResponse {
        ui.painter()
            .rect_filled(ui.max_rect(), 0.0, ui.visuals().panel_fill);

        ui.horizontal(|ui| {
            ui.heading(Behavior::title(pane));
        });
        ui.separator();

        match pane {
            Pane::SceneView | Pane::GameView => {
                ui.label("Render target placeholder (Scene/Game).");
                ui.allocate_rect(ui.available_rect_before_wrap(), egui::Sense::click_and_drag());
            }
            Pane::Console => {
                ui.label("[Info] Hello docking + multi-viewport.");
                ui.allocate_rect(ui.available_rect_before_wrap(), egui::Sense::click_and_drag());
            }
            _ => {
                ui.label("Panel placeholder.");
                ui.allocate_rect(ui.available_rect_before_wrap(), egui::Sense::click_and_drag());
            }
        }

        egui_tiles::UiResponse::None
    }
}

fn unity_like_tree() -> egui_tiles::Tree<Pane> {
    use egui_docking::{DockBuilder, SplitDirection};

    let mut b = DockBuilder::new("unity_layout");

    // Unity-style Professional Layout (rough ratios copied from the Dear ImGui DockBuilder example):
    //
    // +-------------------+---------------------------+-------------------+
    // |                   |        Scene View         |                   |
    // |   Hierarchy       |---------------------------|    Inspector      |
    // |                   |        Game View          |                   |
    // +-------------------+---------------------------+-------------------+
    // |      Project      |         Console           |   Performance     |
    // +-------------------+---------------------------+-------------------+

    let dockspace = b.add_node();

    // First split right from root:
    let (inspector_panel, after_right) = b.split_node(dockspace, SplitDirection::Right, 0.251);
    // Then split left from the remaining:
    let (left_panel, center_area) = b.split_node(after_right, SplitDirection::Left, 0.2896);

    // Left panel vertical stack:
    let (asset_id, top_left_stack) = b.split_node(left_panel, SplitDirection::Down, 0.313);
    let (project_id, hierarchy_id) = b.split_node(top_left_stack, SplitDirection::Down, 0.439);

    // Right panel vertical:
    let (performance_id, inspector_id) = b.split_node(inspector_panel, SplitDirection::Down, 0.2);

    // Center vertical:
    let (console_id, scene_game_id) = b.split_node(center_area, SplitDirection::Down, 0.313);

    // Dock (tab) panes into leaf nodes:
    b.dock_window(Pane::Hierarchy, hierarchy_id);
    b.dock_window(Pane::Project, project_id);
    b.dock_window(Pane::AssetBrowser, asset_id);
    b.dock_windows([Pane::SceneView, Pane::GameView], scene_game_id);
    b.dock_window(Pane::Console, console_id);
    b.dock_window(Pane::Inspector, inspector_id);
    b.dock_window(Pane::Performance, performance_id);

    b.finish(dockspace)
}

impl Default for App {
    fn default() -> Self {
        let tree = unity_like_tree();
        let mut docking = egui_docking::DockingMultiViewport::new(tree);

        Self {
            docking,
            behavior: Behavior,
        }
    }
}

impl eframe::App for App {
    fn clear_color(&self, visuals: &egui::Visuals) -> [f32; 4] {
        let c = visuals.window_fill();
        egui::Color32::from_rgb(c.r(), c.g(), c.b()).to_normalized_gamma_f32()
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::Panel::top("top").show(ctx, |ui| {
            ui.horizontal(|ui| {
                if ui.button("Reset (Unity-like)").clicked() {
                    self.docking.set_root_tree_in_ctx(ctx, unity_like_tree());
                }

                ui.checkbox(
                    &mut self.docking.options.detached_viewport_decorations,
                    "Detached OS decorations",
                );
                ui.checkbox(
                    &mut self.docking.options.detached_csd_window_controls,
                    "CSD window controls",
                );
            });
        });

        self.docking.ui(ctx, &mut self.behavior);
    }
}

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1200.0, 800.0])
            .with_title("egui_docking: game engine layout"),
        ..Default::default()
    };
    eframe::run_native(
        "egui_docking: game engine layout",
        options,
        Box::new(|_cc| Ok(Box::new(App::default()))),
    )
}
