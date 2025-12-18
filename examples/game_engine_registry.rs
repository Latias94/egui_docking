#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")] // hide console window on Windows in release

use eframe::egui;
use std::collections::BTreeSet;

#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum ToolId {
    Hierarchy = 0,
    Inspector = 1,
    SceneView = 2,
    GameView = 3,
    Console = 4,
}

impl ToolId {
    fn title(self) -> &'static str {
        match self {
            ToolId::Hierarchy => "Hierarchy",
            ToolId::Inspector => "Inspector",
            ToolId::SceneView => "Scene View",
            ToolId::GameView => "Game View",
            ToolId::Console => "Console",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Pane {
    id: ToolId,
}

struct ToolRegistry {
    open: BTreeSet<ToolId>,
}

impl ToolRegistry {
    fn new_default_open() -> Self {
        Self {
            open: [ToolId::Hierarchy, ToolId::Inspector, ToolId::SceneView, ToolId::Console]
                .into_iter()
                .collect(),
        }
    }

    fn is_open(&self, id: ToolId) -> bool {
        self.open.contains(&id)
    }

    fn toggle(&mut self, id: ToolId) {
        if !self.open.remove(&id) {
            self.open.insert(id);
        }
    }
}

struct Behavior {
    hierarchy_filter: String,
    selected_entity: usize,
    console_lines: Vec<String>,
    console_last_time: f64,
    console_filter: String,
    console_autoscroll: bool,
}

impl Default for Behavior {
    fn default() -> Self {
        Self {
            hierarchy_filter: String::new(),
            selected_entity: 0,
            console_lines: vec![
                "Engine started.".to_owned(),
                "Registry-driven layout applied.".to_owned(),
            ],
            console_last_time: 0.0,
            console_filter: String::new(),
            console_autoscroll: true,
        }
    }
}

impl Behavior {
    fn entity_name(i: usize) -> &'static str {
        match i {
            0 => "Main Camera",
            1 => "Directional Light",
            2 => "Player",
            3 => "Enemy_01",
            4 => "Environment",
            _ => "Entity",
        }
    }

    fn maybe_push_console_lines(&mut self, ctx: &egui::Context) {
        let now = ctx.input(|i| i.time);
        if self.console_last_time == 0.0 {
            self.console_last_time = now;
            return;
        }
        if now - self.console_last_time < 2.0 {
            return;
        }
        self.console_last_time = now;
        let n = self.console_lines.len();
        self.console_lines
            .push(format!("[Info] background job tick #{n}"));
        if self.console_lines.len() > 200 {
            let drain = self.console_lines.len() - 200;
            self.console_lines.drain(0..drain);
        }
    }
}

impl egui_tiles::Behavior<Pane> for Behavior {
    fn tab_title_for_pane(&mut self, pane: &Pane) -> egui::WidgetText {
        pane.id.title().into()
    }

    fn pane_ui(
        &mut self,
        ui: &mut egui::Ui,
        _tile_id: egui_tiles::TileId,
        pane: &mut Pane,
    ) -> egui_tiles::UiResponse {
        ui.horizontal(|ui| ui.heading(pane.id.title()));
        ui.separator();
        match pane.id {
            ToolId::Hierarchy => {
                ui.horizontal(|ui| {
                    ui.label("Filter:");
                    ui.text_edit_singleline(&mut self.hierarchy_filter);
                });
                ui.separator();

                let filter = self.hierarchy_filter.to_lowercase();
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        for i in 0..18usize {
                            let name = format!("{} ({i})", Self::entity_name(i % 5));
                            if !filter.is_empty() && !name.to_lowercase().contains(&filter) {
                                continue;
                            }
                            let selected = self.selected_entity == i;
                            if ui.selectable_label(selected, name).clicked() {
                                self.selected_entity = i;
                            }
                        }
                    });
            }
            ToolId::Inspector => {
                ui.label(format!(
                    "Selected: {} ({})",
                    Self::entity_name(self.selected_entity % 5),
                    self.selected_entity
                ));
                ui.separator();
                egui::Grid::new("registry_inspector_grid")
                    .num_columns(2)
                    .spacing([12.0, 6.0])
                    .striped(true)
                    .show(ui, |ui| {
                        ui.label("Enabled");
                        let mut enabled = true;
                        ui.checkbox(&mut enabled, "");
                        ui.end_row();

                        ui.label("Tag");
                        let mut tag = "Player";
                        ui.text_edit_singleline(&mut tag);
                        ui.end_row();

                        ui.label("Color");
                        let mut col = egui::Color32::from_rgb(210, 240, 255);
                        ui.color_edit_button_srgba(&mut col);
                        ui.end_row();
                    });
            }
            ToolId::SceneView | ToolId::GameView => {
                let (rect, _resp) = ui.allocate_exact_size(
                    ui.available_size_before_wrap(),
                    egui::Sense::click_and_drag(),
                );
                let painter = ui.painter_at(rect);
                painter.rect_filled(rect, 0.0, ui.visuals().extreme_bg_color);
                painter.text(
                    rect.center(),
                    egui::Align2::CENTER_CENTER,
                    format!("{} viewport placeholder", pane.id.title()),
                    egui::TextStyle::Heading.resolve(ui.style()),
                    ui.visuals().text_color(),
                );
            }
            ToolId::Console => {
                self.maybe_push_console_lines(ui.ctx());

                ui.horizontal(|ui| {
                    ui.label("Filter:");
                    ui.text_edit_singleline(&mut self.console_filter);
                    ui.checkbox(&mut self.console_autoscroll, "Auto-scroll");
                    ui.separator();
                    if ui.button("Clear").clicked() {
                        self.console_lines.clear();
                    }
                });
                ui.separator();

                let filter = self.console_filter.to_lowercase();
                egui::ScrollArea::vertical()
                    .stick_to_bottom(self.console_autoscroll)
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        for line in self.console_lines.iter() {
                            if !filter.is_empty() && !line.to_lowercase().contains(&filter) {
                                continue;
                            }
                            ui.label(line);
                        }
                    });
            }
        }

        ui.allocate_rect(ui.available_rect_before_wrap(), egui::Sense::click_and_drag());
        egui_tiles::UiResponse::None
    }
}

fn scripted_layout_from_registry(reg: &ToolRegistry) -> egui_tiles::Tree<Pane> {
    use egui_docking::{DockBuilder, SplitDirection};

    // The DockBuilder is expressed in terms of ids (ToolId), not actual Pane state.
    // `finish_map` materializes only the open tools into real panes.
    let mut b = DockBuilder::new("engine_registry_layout");
    let root = b.add_node();

    let (right, rest) = b.split_node(root, SplitDirection::Right, 0.25);
    let (bottom, top) = b.split_node(rest, SplitDirection::Down, 0.30);

    b.dock_window(ToolId::Inspector, right);
    b.dock_window(ToolId::Hierarchy, top);
    b.dock_windows([ToolId::SceneView, ToolId::GameView], top);
    b.dock_window(ToolId::Console, bottom);

    let mut tree = b.finish_map(root, |id| Some(Pane { id }));
    sync_visibility_in_tree(&mut tree, reg);
    tree
}

fn find_pane_tile_id(tree: &egui_tiles::Tree<Pane>, pane: ToolId) -> Option<egui_tiles::TileId> {
    tree.tiles.iter().find_map(|(tile_id, tile)| {
        match tile {
            egui_tiles::Tile::Pane(p) if p.id == pane => Some(*tile_id),
            _ => None,
        }
    })
}

fn sync_visibility_in_tree(tree: &mut egui_tiles::Tree<Pane>, reg: &ToolRegistry) {
    for id in [
        ToolId::Hierarchy,
        ToolId::Inspector,
        ToolId::SceneView,
        ToolId::GameView,
        ToolId::Console,
    ] {
        if let Some(tile_id) = find_pane_tile_id(tree, id) {
            tree.tiles.set_visible(tile_id, reg.is_open(id));
        }
    }
}

struct App {
    docking: egui_docking::DockingMultiViewport<Pane>,
    behavior: Behavior,
    reg: ToolRegistry,
    apply_once: bool,
}

impl Default for App {
    fn default() -> Self {
        let mut docking = egui_docking::DockingMultiViewport::new(egui_tiles::Tree::empty("init"));
        docking.options.debug_drop_targets = true;
        docking.options.debug_event_log = true;
        docking.options.debug_integrity = true;
        docking.options.debug_log_file_path = Some("target/egui_docking_debug.log".into());

        Self {
            docking,
            behavior: Behavior::default(),
            reg: ToolRegistry::new_default_open(),
            apply_once: true,
        }
    }
}

impl eframe::App for App {
    fn clear_color(&self, visuals: &egui::Visuals) -> [f32; 4] {
        let c = visuals.window_fill();
        egui::Color32::from_rgb(c.r(), c.g(), c.b()).to_normalized_gamma_f32()
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if self.apply_once {
            self.apply_once = false;
            self.docking
                .set_root_tree_in_ctx(ctx, scripted_layout_from_registry(&self.reg));
        }

        egui::Panel::top("top").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label("Tools:");
                for id in [
                    ToolId::Hierarchy,
                    ToolId::Inspector,
                    ToolId::SceneView,
                    ToolId::GameView,
                    ToolId::Console,
                ] {
                    let mut open = self.reg.is_open(id);
                    if ui.checkbox(&mut open, id.title()).changed() {
                        self.reg.toggle(id);
                        sync_visibility_in_tree(&mut self.docking.tree, &self.reg);
                        ctx.request_repaint();
                    }
                }
            });
        });

        self.docking.ui(ctx, &mut self.behavior);
    }
}

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1100.0, 700.0])
            .with_title("egui_docking: registry-based layout"),
        ..Default::default()
    };
    eframe::run_native(
        "egui_docking: registry-based layout",
        options,
        Box::new(|_cc| Ok(Box::new(App::default()))),
    )
}
