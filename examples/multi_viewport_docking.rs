#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")] // hide console window on Windows in release

use eframe::egui;
#[cfg(feature = "persistence")]
use std::path::PathBuf;

#[derive(Clone, Debug)]
struct Pane {
    id: usize,
}

struct App {
    docking: egui_docking::DockingMultiViewport<Pane>,
    behavior: DemoBehavior,
    #[cfg(feature = "persistence")]
    layout_path: PathBuf,
    #[cfg(feature = "persistence")]
    last_layout_error: Option<String>,
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
                     To dock back, drag any tab (or tab-bar background) into another window and release. \
                     Dragging a tab-bar background moves the whole dock node (ImGui-style).",
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

    fn is_tab_closable(
        &self,
        _tiles: &egui_tiles::Tiles<Pane>,
        _tile_id: egui_tiles::TileId,
    ) -> bool {
        true
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
        docking.options.debug_log_file_path = Some("target/egui_docking_debug.log".into());
        docking.options.debug_log_window_move_every_send = true;

        Self {
            docking,
            behavior: DemoBehavior,
            #[cfg(feature = "persistence")]
            layout_path: PathBuf::from("target/egui_docking_layout.ron"),
            #[cfg(feature = "persistence")]
            last_layout_error: None,
        }
    }
}

impl eframe::App for App {
    fn clear_color(&self, visuals: &egui::Visuals) -> [f32; 4] {
        // Make the OS window background match egui's theme (less "black frame" around panels).
        let c = visuals.window_fill();
        egui::Color32::from_rgb(c.r(), c.g(), c.b()).to_normalized_gamma_f32()
    }

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
            ui.horizontal(|ui| {
                ui.checkbox(
                    &mut self.docking.options.config_docking_with_shift,
                    "ImGui: io.ConfigDockingWithShift",
                );
                ui.checkbox(
                    &mut self
                        .docking
                        .options
                        .window_move_tab_dock_requires_explicit_target,
                    "ImGui: window-move tab dock requires target title/tab-bar",
                );
                ui.checkbox(
                    &mut self.docking.options.focus_detached_on_custom_title_drag,
                    "Focus detached while window-move dragging",
                );
                ui.checkbox(
                    &mut self.docking.options.allow_container_tabbing,
                    "Allow container tabbing (Unity-like, non-ImGui)",
                );
                ui.checkbox(
                    &mut self.docking.options.detached_viewport_decorations,
                    "Detached native decorations (OS title bar)",
                );
                ui.checkbox(
                    &mut self.docking.options.detached_csd_window_controls,
                    "CSD window controls (close/min/max) for detached",
                );
                ui.checkbox(
                    &mut self.docking.options.debug_log_file_flush_each_line,
                    "Debug log file: flush each line",
                );
                ui.checkbox(
                    &mut self.docking.options.debug_log_window_move_every_send,
                    "Debug log: window-move every send (verbose)",
                );
            });
            ui.add(
                egui::Label::new(
                    "Tip: release outside dock = new native window. SHIFT detaches whole tab-group; ALT forces tear-off. \
                     Hold CTRL while tearing off to create a contained floating window. \
                     While dragging, use the overlay targets for center/left/right/top/bottom docking. \
                     Window-move docking matches ImGui: hold SHIFT to temporarily disable docking while moving (unless ConfigDockingWithShift is enabled). \
                     In a detached viewport, drag the tab-bar background to move the native window. \
                     ImGui-like: double-click the detached window's tab-bar background to toggle maximize. \
                     Disabling OS decorations (CSD) can improve docking drags (no native title bar/menu bar intercept); enable CSD window controls for close/min/max. \
                     For window-move tab docking, hover the target window's title/tab bar (or use overlay targets). \
                     Drag into any other window to dock back.",
                )
                .selectable(false),
            );
            if let Some(path) = self.docking.options.debug_log_file_path.as_deref() {
                ui.separator();
                ui.label(format!(
                    "Debug log file: {} (auto-truncate on start: {})",
                    path.display(),
                    self.docking.options.debug_log_file_clear_on_start
                ));
            }

            ui.separator();
            ui.collapsing("Backend hints (for cross-viewport UX)", |ui| {
                let hovered = egui_docking::backend_mouse_hovered_viewport_id(ctx);
                let pointer = egui_docking::backend_pointer_global_points(ctx);
                let monitors = egui_docking::backend_monitors_outer_rects_points(ctx);

                ui.label(format!("mouse_hovered_viewport_id: {hovered:?}"));
                ui.label(format!("pointer_global_points: {pointer:?}"));

                match monitors {
                    Some(ref m) if !m.is_empty() => {
                        ui.label(format!("monitors_outer_rects_points: {} monitor(s)", m.len()));
                        for (idx, r) in m.iter().enumerate() {
                            ui.label(format!("  {idx}: min={:?} max={:?}", r.min, r.max));
                        }
                    }
                    _ => {
                        ui.label("monitors_outer_rects_points: <missing>");
                        ui.label(
                            "Note: Dear ImGui-style multi-viewport backends typically provide a monitor list. \
                             If missing, layout restore can only do best-effort clamping to the current monitor.",
                        );
                    }
                }
            });

            #[cfg(feature = "persistence")]
            {
                ui.separator();
                ui.horizontal(|ui| {
                    ui.label(format!("Layout: {}", self.layout_path.display()));

                    if ui.button("Save layout").clicked() {
                        let mut registry = egui_docking::SimplePaneRegistry::new(
                            |pane: &Pane| pane.id,
                            |id| Pane { id },
                        );
                        self.last_layout_error = self
                            .docking
                            .save_layout_to_ron_file_with_registry(&self.layout_path, &mut registry)
                            .err()
                            .map(|e| e.to_string());
                    }

                    if ui.button("Load layout").clicked() {
                        let mut registry = egui_docking::SimplePaneRegistry::new(
                            |pane: &Pane| pane.id,
                            |id| Pane { id },
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
                });

                if let Some(err) = self.last_layout_error.as_deref() {
                    ui.colored_label(ui.visuals().error_fg_color, err);
                }
            }
        });
        self.docking.ui(ctx, &mut self.behavior);
    }
}
