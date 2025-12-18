#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")] // hide console window on Windows in release

use eframe::egui;
#[cfg(feature = "persistence")]
use std::path::PathBuf;
use std::time::Duration;

#[repr(u8)]
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

impl Pane {
    #[cfg(feature = "persistence")]
    fn from_id(id: u8) -> Option<Self> {
        match id {
            0 => Some(Self::Hierarchy),
            1 => Some(Self::Project),
            2 => Some(Self::AssetBrowser),
            3 => Some(Self::SceneView),
            4 => Some(Self::GameView),
            5 => Some(Self::Console),
            6 => Some(Self::Inspector),
            7 => Some(Self::Performance),
            _ => None,
        }
    }
}

struct App {
    docking: egui_docking::DockingMultiViewport<Pane>,
    behavior: Behavior,
    apply_workspace_once: bool,
    #[cfg(feature = "persistence")]
    layout_path: PathBuf,
    #[cfg(feature = "persistence")]
    last_layout_error: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GizmoMode {
    Move,
    Rotate,
    Scale,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ConsoleLevel {
    Info,
    Warn,
    Error,
}

#[derive(Debug)]
struct Behavior {
    hierarchy_filter: String,
    selected_entity: usize,
    gizmo: GizmoMode,

    console_filter: String,
    console_autoscroll: bool,
    console_level_info: bool,
    console_level_warn: bool,
    console_level_error: bool,
    console_lines: Vec<(ConsoleLevel, String)>,
    console_last_time: f64,

    perf_history_fps: Vec<f32>,
}

impl Default for Behavior {
    fn default() -> Self {
        Self {
            hierarchy_filter: String::new(),
            selected_entity: 0,
            gizmo: GizmoMode::Move,
            console_filter: String::new(),
            console_autoscroll: true,
            console_level_info: true,
            console_level_warn: true,
            console_level_error: true,
            console_lines: vec![
                (ConsoleLevel::Info, "Engine started.".to_owned()),
                (ConsoleLevel::Info, "Docking + multi-viewport ready.".to_owned()),
                (ConsoleLevel::Warn, "Shader hot-reload disabled (demo).".to_owned()),
            ],
            console_last_time: 0.0,
            perf_history_fps: Vec::new(),
        }
    }
}

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

    fn entity_name(i: usize) -> &'static str {
        match i {
            0 => "Main Camera",
            1 => "Directional Light",
            2 => "Player",
            3 => "Enemy_01",
            4 => "Enemy_02",
            5 => "Environment",
            _ => "Entity",
        }
    }

    fn update_perf(&mut self, ctx: &egui::Context) {
        let fps = ctx.input(|i| (1.0 / i.stable_dt.max(1e-6)).clamp(1.0, 1000.0));
        self.perf_history_fps.push(fps);
        if self.perf_history_fps.len() > 240 {
            let drain = self.perf_history_fps.len() - 240;
            self.perf_history_fps.drain(0..drain);
        }
    }

    fn maybe_push_console_lines(&mut self, ctx: &egui::Context) {
        let now = ctx.input(|i| i.time);
        if self.console_last_time == 0.0 {
            self.console_last_time = now;
            return;
        }
        if now - self.console_last_time < 1.5 {
            return;
        }
        self.console_last_time = now;

        let n = self.console_lines.len() as u64;
        let level = match n % 9 {
            0 => ConsoleLevel::Error,
            3 | 6 => ConsoleLevel::Warn,
            _ => ConsoleLevel::Info,
        };
        let msg = match level {
            ConsoleLevel::Info => format!("Loaded asset bundle #{n}."),
            ConsoleLevel::Warn => format!("Texture streaming fallback (id={n})."),
            ConsoleLevel::Error => format!("Physics step over budget (frame={n})."),
        };
        self.console_lines.push((level, msg));
        if self.console_lines.len() > 300 {
            let drain = self.console_lines.len() - 300;
            self.console_lines.drain(0..drain);
        }
    }

    fn console_level_enabled(&self, level: ConsoleLevel) -> bool {
        match level {
            ConsoleLevel::Info => self.console_level_info,
            ConsoleLevel::Warn => self.console_level_warn,
            ConsoleLevel::Error => self.console_level_error,
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
        let title = Self::title(pane);
        ui.horizontal(|ui| {
            ui.heading(title);
            ui.add_space(8.0);
            if matches!(pane, Pane::SceneView | Pane::GameView) {
                ui.separator();
                ui.selectable_value(&mut self.gizmo, GizmoMode::Move, "Move");
                ui.selectable_value(&mut self.gizmo, GizmoMode::Rotate, "Rotate");
                ui.selectable_value(&mut self.gizmo, GizmoMode::Scale, "Scale");
            }
        });
        ui.separator();

        match pane {
            Pane::Hierarchy => {
                ui.horizontal(|ui| {
                    ui.label("Filter:");
                    ui.text_edit_singleline(&mut self.hierarchy_filter);
                });
                ui.separator();

                let filter = self.hierarchy_filter.to_lowercase();
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        for i in 0..24usize {
                            let name = format!("{} ({i})", Self::entity_name(i % 6));
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

            Pane::Project | Pane::AssetBrowser => {
                ui.horizontal(|ui| {
                    ui.label("Path:");
                    ui.monospace("Assets/");
                    ui.add_space(8.0);
                    let _ = ui.button("Import…");
                    let _ = ui.button("Reimport");
                });
                ui.separator();

                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        egui::CollapsingHeader::new("Materials")
                            .default_open(true)
                            .show(ui, |ui| {
                                for name in ["M_Default", "M_Player", "M_Enemy", "M_Env"] {
                                    ui.label(name);
                                }
                            });
                        egui::CollapsingHeader::new("Meshes")
                            .default_open(true)
                            .show(ui, |ui| {
                                for name in ["SM_Cube", "SM_Sphere", "SM_Tree", "SM_Rock"] {
                                    ui.label(name);
                                }
                            });
                        egui::CollapsingHeader::new("Textures")
                            .default_open(true)
                            .show(ui, |ui| {
                                for name in ["T_Albedo", "T_Normal", "T_Roughness"] {
                                    ui.label(name);
                                }
                            });
                    });
            }

            Pane::SceneView | Pane::GameView => {
                let (rect, _resp) = ui.allocate_exact_size(
                    ui.available_size_before_wrap(),
                    egui::Sense::click_and_drag(),
                );
                let painter = ui.painter_at(rect);
                painter.rect_filled(rect, 0.0, ui.visuals().extreme_bg_color);

                let stroke = egui::Stroke::new(
                    1.0,
                    ui.visuals().widgets.noninteractive.bg_stroke.color,
                );
                let step = 32.0;
                let mut x = rect.left();
                while x < rect.right() {
                    painter.line_segment(
                        [egui::pos2(x, rect.top()), egui::pos2(x, rect.bottom())],
                        stroke,
                    );
                    x += step;
                }
                let mut y = rect.top();
                while y < rect.bottom() {
                    painter.line_segment(
                        [egui::pos2(rect.left(), y), egui::pos2(rect.right(), y)],
                        stroke,
                    );
                    y += step;
                }

                painter.text(
                    rect.left_top() + egui::vec2(8.0, 8.0),
                    egui::Align2::LEFT_TOP,
                    format!(
                        "{} (gizmo: {:?}, selected: {})",
                        title,
                        self.gizmo,
                        Self::entity_name(self.selected_entity % 6)
                    ),
                    egui::TextStyle::Monospace.resolve(ui.style()),
                    ui.visuals().text_color(),
                );
            }

            Pane::Console => {
                self.maybe_push_console_lines(ui.ctx());

                ui.horizontal(|ui| {
                    ui.label("Filter:");
                    ui.text_edit_singleline(&mut self.console_filter);
                    ui.checkbox(&mut self.console_autoscroll, "Auto-scroll");
                    ui.separator();
                    ui.checkbox(&mut self.console_level_info, "Info");
                    ui.checkbox(&mut self.console_level_warn, "Warn");
                    ui.checkbox(&mut self.console_level_error, "Error");
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
                        for (level, line) in self.console_lines.iter() {
                            if !self.console_level_enabled(*level) {
                                continue;
                            }
                            if !filter.is_empty() && !line.to_lowercase().contains(&filter) {
                                continue;
                            }
                            let prefix = match level {
                                ConsoleLevel::Info => "[Info] ",
                                ConsoleLevel::Warn => "[Warn] ",
                                ConsoleLevel::Error => "[Error] ",
                            };
                            let color = match level {
                                ConsoleLevel::Info => ui.visuals().text_color(),
                                ConsoleLevel::Warn => ui.visuals().warn_fg_color,
                                ConsoleLevel::Error => ui.visuals().error_fg_color,
                            };
                            ui.label(egui::RichText::new(format!("{prefix}{line}")).color(color));
                        }
                    });
            }

            Pane::Inspector => {
                ui.label(format!(
                    "Selected: {} ({})",
                    Self::entity_name(self.selected_entity % 6),
                    self.selected_entity
                ));
                ui.separator();

                egui::Grid::new("inspector_grid")
                    .num_columns(2)
                    .spacing([12.0, 6.0])
                    .striped(true)
                    .show(ui, |ui| {
                        ui.label("Enabled");
                        let mut enabled = true;
                        ui.checkbox(&mut enabled, "");
                        ui.end_row();

                        ui.label("Layer");
                        let mut layer = 0i32;
                        ui.add(egui::DragValue::new(&mut layer).speed(1));
                        ui.end_row();

                        ui.label("Material");
                        let mut col = egui::Color32::from_rgb(170, 200, 255);
                        ui.color_edit_button_srgba(&mut col);
                        ui.end_row();
                    });
            }

            Pane::Performance => {
                // eframe is reactive by default; keep the performance graphs updating even
                // without user input.
                ui.ctx().request_repaint_after(Duration::from_millis(16));

                self.update_perf(ui.ctx());
                let fps = *self.perf_history_fps.last().unwrap_or(&0.0);
                ui.label(format!("FPS (stable): {fps:.0}"));
                ui.separator();

                let (rect, _resp) = ui.allocate_exact_size(
                    egui::vec2(ui.available_width(), 120.0),
                    egui::Sense::hover(),
                );
                let painter = ui.painter_at(rect);
                painter.rect_filled(rect, 0.0, ui.visuals().extreme_bg_color);

                let max_fps = 240.0f32;
                let stroke = egui::Stroke::new(1.0, ui.visuals().hyperlink_color);
                let n = self.perf_history_fps.len();
                if n >= 2 {
                    let points: Vec<egui::Pos2> = self
                        .perf_history_fps
                        .iter()
                        .enumerate()
                        .map(|(i, &v)| {
                            let t = i as f32 / ((n - 1) as f32);
                            let x = egui::lerp(rect.x_range(), t);
                            let y = egui::lerp(
                                rect.y_range(),
                                1.0 - (v / max_fps).clamp(0.0, 1.0),
                            );
                            egui::pos2(x, y)
                        })
                        .collect();
                    painter.add(egui::Shape::line(points, stroke));
                }
            }
        }

        egui_tiles::UiResponse::None
    }
}

fn unity_like_root_tree() -> egui_tiles::Tree<Pane> {
    use egui_docking::{DockBuilder, SplitDirection};

    let mut b = DockBuilder::new("unity_root");
    let dockspace = b.add_node();

    let (inspector_panel, after_right) = b.split_node(dockspace, SplitDirection::Right, 0.251);
    let (left_panel, center_area) = b.split_node(after_right, SplitDirection::Left, 0.2896);

    let (asset_id, top_left_stack) = b.split_node(left_panel, SplitDirection::Down, 0.313);
    let (project_id, hierarchy_id) = b.split_node(top_left_stack, SplitDirection::Down, 0.439);

    let (performance_id, inspector_id) = b.split_node(inspector_panel, SplitDirection::Down, 0.2);

    // Center: SceneView on top, Console on bottom (GameView will be in a detached window).
    let (console_id, scene_id) = b.split_node(center_area, SplitDirection::Down, 0.313);

    b.dock_window(Pane::Hierarchy, hierarchy_id);
    b.dock_window(Pane::Project, project_id);
    b.dock_window(Pane::AssetBrowser, asset_id);
    b.dock_window(Pane::SceneView, scene_id);
    b.dock_window(Pane::Console, console_id);
    b.dock_window(Pane::Inspector, inspector_id);
    b.dock_window(Pane::Performance, performance_id);

    b.finish(dockspace)
}

fn game_view_detached_tree() -> egui_tiles::Tree<Pane> {
    use egui_docking::DockBuilder;

    let mut b = DockBuilder::new("game_view_detached");
    let node = b.add_node();
    b.dock_window(Pane::GameView, node);
    b.finish(node)
}

fn unity_like_workspace() -> egui_docking::WorkspaceLayout<Pane> {
    let mut ws = egui_docking::WorkspaceLayout::new(unity_like_root_tree());

    let builder = egui::ViewportBuilder::default()
        .with_title("Game View")
        .with_inner_size([720.0, 480.0]);
    ws.detached
        .push(egui_docking::DetachedViewportLayout::new(
            builder,
            game_view_detached_tree(),
        ));
    ws
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
            apply_workspace_once: true,
            #[cfg(feature = "persistence")]
            layout_path: PathBuf::from("target/egui_docking_workspace.ron"),
            #[cfg(feature = "persistence")]
            last_layout_error: None,
        }
    }
}

impl eframe::App for App {
    fn clear_color(&self, visuals: &egui::Visuals) -> [f32; 4] {
        let c = visuals.window_fill();
        egui::Color32::from_rgb(c.r(), c.g(), c.b()).to_normalized_gamma_f32()
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if self.apply_workspace_once {
            self.apply_workspace_once = false;
            #[cfg(feature = "persistence")]
            {
                let mut registry = egui_docking::SimplePaneRegistry::new(
                    |pane: &Pane| *pane as u8,
                    |id: u8| Pane::from_id(id).unwrap_or(Pane::Hierarchy),
                );

                self.last_layout_error = self
                    .docking
                    .load_layout_from_ron_file_in_ctx_with_registry_or_apply_workspace(
                        ctx,
                        &self.layout_path,
                        &mut registry,
                        unity_like_workspace,
                    )
                    .err()
                    .map(|e| e.to_string());
            }

            #[cfg(not(feature = "persistence"))]
            {
                self.docking.set_workspace_layout_in_ctx(ctx, unity_like_workspace());
            }
        }

        egui::Panel::top("top").show(ctx, |ui| {
            ui.horizontal(|ui| {
                if ui.button("Reset workspace").clicked() {
                    self.docking
                        .set_workspace_layout_in_ctx(ctx, unity_like_workspace());
                }
                #[cfg(feature = "persistence")]
                {
                    if ui.button("Save layout").clicked() {
                        let mut registry = egui_docking::SimplePaneRegistry::new(
                            |pane: &Pane| *pane as u8,
                            |id: u8| Pane::from_id(id).unwrap_or(Pane::Hierarchy),
                        );
                        self.last_layout_error = self
                            .docking
                            .save_layout_to_ron_file_with_registry(&self.layout_path, &mut registry)
                            .err()
                            .map(|e| e.to_string());
                    }
                    if ui.button("Load layout").clicked() {
                        let mut registry = egui_docking::SimplePaneRegistry::new(
                            |pane: &Pane| *pane as u8,
                            |id: u8| Pane::from_id(id).unwrap_or(Pane::Hierarchy),
                        );
                        self.last_layout_error = self
                            .docking
                            .load_layout_from_ron_file_in_ctx_with_registry(
                                ctx,
                                &self.layout_path,
                                &mut registry,
                            )
                            .err()
                            .map(|e| e.to_string());
                    }

                    if let Some(err) = self.last_layout_error.as_deref() {
                        ui.label(format!("layout error: {err}"));
                    } else {
                        ui.label(format!("layout: {}", self.layout_path.display()));
                    }
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
            .with_title("egui_docking: game engine workspace"),
        ..Default::default()
    };
    eframe::run_native(
        "egui_docking: game engine workspace",
        options,
        Box::new(|_cc| Ok(Box::new(App::default()))),
    )
}
