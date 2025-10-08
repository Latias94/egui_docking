#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")] // hide console window on Windows in release

use eframe::egui;

#[derive(Clone, PartialEq)]
enum Pane {
    Scene,
    Inspector,
    Console,
}

struct Delegate;

impl egui_docking::Behavior<Pane> for Delegate {
    fn tab_title_for_pane(&mut self, pane: &Pane) -> egui::WidgetText {
        match pane {
            Pane::Scene => "Scene".into(),
            Pane::Inspector => "Inspector".into(),
            Pane::Console => "Console".into(),
        }
    }

    fn pane_ui(
        &mut self,
        ui: &mut egui::Ui,
        _tile_id: egui_docking::TileId,
        pane: &mut Pane,
    ) -> egui_docking::UiResponse {
        ui.label(match pane {
            Pane::Scene => "3D View (demo)",
            Pane::Inspector => "Properties",
            Pane::Console => "Logs",
        });
        ui.allocate_rect(ui.max_rect(), egui::Sense::hover());
        egui_docking::UiResponse::None
    }

    fn dock_indicator_style(&self) -> egui_docking::DockIndicatorStyle {
        egui_docking::DockIndicatorStyle::ImguiLike {
            size: -1.0,
            gap: -1.0,
            rounding: -1.0,
        }
    }

    fn is_tab_closable(
        &self,
        _tiles: &egui_docking::Tiles<Pane>,
        _tile_id: egui_docking::TileId,
    ) -> bool {
        true
    }
}

struct App {
    tree: egui_docking::Tree<Pane>,
    tabbed_pair: bool,
    normalize_shares: bool,
    equalize_shares_flag: bool,
    central_passthrough: bool,
    use_blueprint: bool,
    scene_first: bool,
    inspector_on_right: bool,
    inspector_ratio: f32,
    console_ratio: f32,
    console_on_bottom: bool,
    central_sel: CentralSel,
    show_export: bool,
    export_text: String,
    show_export_full: bool,
    export_full_text: String,
    show_import_full: bool,
    import_full_text: String,
}

#[derive(Clone, Copy, PartialEq)]
enum CentralSel {
    None,
    Scene,
    Inspector,
}

impl Default for App {
    fn default() -> Self {
        let mut app = Self {
            tree: egui_docking::Tree::empty("placeholder"),
            tabbed_pair: true,
            normalize_shares: false,
            equalize_shares_flag: false,
            central_passthrough: false,
            use_blueprint: false,
            scene_first: true,
            inspector_on_right: true,
            inspector_ratio: 0.30,
            console_ratio: 0.25,
            console_on_bottom: true,
            central_sel: CentralSel::Scene,
            show_export: false,
            export_text: String::new(),
            show_export_full: false,
            export_full_text: String::new(),
            show_import_full: false,
            import_full_text: String::new(),
        };
        app.rebuild();
        app
    }
}

impl App {
    fn rebuild(&mut self) {
        self.tree = if self.use_blueprint {
            // Build from blueprint
            use egui_docking::{DockBlueprintNode as N, DockBuilder, LinearDir};
            let pair = if self.tabbed_pair {
                let mut children = vec![N::Pane(1), N::Pane(2)];
                if !self.scene_first {
                    children.swap(0, 1);
                }
                N::Tabs {
                    children,
                    active: Some(if self.scene_first { 0 } else { 1 }),
                    flags: None,
                }
            } else {
                let children = if self.inspector_on_right {
                    vec![N::Pane(1), N::Pane(2)]
                } else {
                    vec![N::Pane(2), N::Pane(1)]
                };
                let shares = if self.inspector_on_right {
                    vec![1.0 - self.inspector_ratio, self.inspector_ratio]
                } else {
                    vec![self.inspector_ratio, 1.0 - self.inspector_ratio]
                };
                N::Split {
                    dir: LinearDir::Horizontal,
                    children,
                    shares: Some(shares),
                    flags: None,
                }
            };
            let shares_v = if self.equalize_shares_flag {
                vec![0.5, 0.5]
            } else {
                vec![1.0 - self.console_ratio, self.console_ratio]
            };
            let bp = if self.console_on_bottom {
                N::Split {
                    dir: LinearDir::Vertical,
                    children: vec![pair.clone(), N::Pane(3)],
                    shares: Some(shares_v),
                    flags: None,
                }
            } else {
                N::Split {
                    dir: LinearDir::Vertical,
                    children: vec![N::Pane(3), pair.clone()],
                    shares: Some(vec![self.console_ratio, 1.0 - self.console_ratio]),
                    flags: None,
                }
            };
            let mut tree = DockBuilder::from_blueprint("builder_demo", bp, |id| match id {
                1 => Pane::Scene,
                2 => Pane::Inspector,
                _ => Pane::Console,
            });
            // Mark central as the container (parent) of the selected pane, similar to ImGui's central node
            tree.central = match self.central_sel {
                CentralSel::Scene => tree
                    .tiles
                    .find_pane(&Pane::Scene)
                    .and_then(|pid| tree.tiles.parent_of(pid)),
                CentralSel::Inspector => tree
                    .tiles
                    .find_pane(&Pane::Inspector)
                    .and_then(|pid| tree.tiles.parent_of(pid)),
                CentralSel::None => None,
            };
            tree.central_passthrough = self.central_passthrough;
            tree
        } else {
            // Build via API
            use egui_docking::{DockBuilder, DockSide};
            let mut b = DockBuilder::new("builder_demo");
            let scene = b.add_pane(Pane::Scene);
            let inspector = b.add_pane(Pane::Inspector);
            let console = b.add_pane(Pane::Console);

            b.set_root(scene);
            let top = if self.tabbed_pair {
                let mut children = vec![scene, inspector];
                if !self.scene_first {
                    children.swap(0, 1);
                }
                let tabs = b.tabs(children);
                b.set_active_in_tabs(tabs, if self.scene_first { scene } else { inspector });
                tabs
            } else {
                let side = if self.inspector_on_right {
                    DockSide::Right
                } else {
                    DockSide::Left
                };
                b.dock_with_ratio(scene, inspector, side, self.inspector_ratio)
            };
            let split2 = b.dock_with_ratio(
                top,
                console,
                if self.console_on_bottom {
                    DockSide::Bottom
                } else {
                    DockSide::Top
                },
                self.console_ratio,
            );

            if self.normalize_shares {
                let _ = b.normalize_shares(split2);
            }
            if self.equalize_shares_flag {
                let _ = b.equalize_shares(split2);
            }

            match self.central_sel {
                CentralSel::Scene => {
                    b.set_central(scene);
                }
                CentralSel::Inspector => {
                    b.set_central(inspector);
                }
                CentralSel::None => {}
            }
            b.set_central_passthrough(self.central_passthrough);
            b.build()
        };
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::TopBottomPanel::top("top").show(ctx, |ui| {
            ui.label("DockBuilder demo: build layouts & tweak options.");
            ui.horizontal(|ui| {
                ui.checkbox(&mut self.tabbed_pair, "Tabs for Scene/Inspector");
                ui.checkbox(&mut self.normalize_shares, "Normalize shares");
                if ui.button("Equalize shares").clicked() {
                    self.equalize_shares_flag = true;
                    self.normalize_shares = false;
                    self.rebuild();
                    self.equalize_shares_flag = false;
                }
                ui.checkbox(&mut self.central_passthrough, "Central passthrough");
                ui.checkbox(&mut self.use_blueprint, "Build via Blueprint");
                if ui.button("Export Blueprint").clicked() {
                    let node = egui_docking::export_tree(&self.tree, |pane: &Pane| match pane {
                        Pane::Scene => 1,
                        Pane::Inspector => 2,
                        Pane::Console => 3,
                    });
                    #[cfg(feature = "serde")]
                    {
                        match serde_json::to_string_pretty(&node) {
                            Ok(s) => self.export_text = s,
                            Err(_) => {
                                self.export_text = format!("{node:#?}");
                            }
                        }
                    }
                    #[cfg(not(feature = "serde"))]
                    {
                        self.export_text = format!("{node:#?}");
                    }
                    self.show_export = true;
                }
                #[cfg(feature = "serde")]
                if ui.button("Export Full Blueprint").clicked() {
                    let fb = egui_docking::export_tree_full(&self.tree, |pane: &Pane| match pane {
                        Pane::Scene => 1,
                        Pane::Inspector => 2,
                        Pane::Console => 3,
                    });
                    match serde_json::to_string_pretty(&fb) {
                        Ok(s) => {
                            self.export_full_text = s;
                            self.show_export_full = true;
                        }
                        Err(e) => {
                            self.export_full_text = format!("Export error: {e}");
                            self.show_export_full = true;
                        }
                    }
                }
                #[cfg(feature = "serde")]
                if ui.button("Import Full Blueprint").clicked() {
                    self.import_full_text.clear();
                    self.show_import_full = true;
                }
                ui.separator();
                ui.label("Inspector on right:");
                ui.checkbox(&mut self.inspector_on_right, "");
                ui.label("Scene first (tabs):");
                ui.checkbox(&mut self.scene_first, "");
                ui.separator();
                ui.label("Console bottom:");
                ui.checkbox(&mut self.console_on_bottom, "");
            });
            ui.horizontal(|ui| {
                ui.label("Inspector ratio:");
                ui.add(egui::Slider::new(&mut self.inspector_ratio, 0.1..=0.9));
                if ui.small_button("30%").clicked() {
                    self.inspector_ratio = 0.30;
                }
                if ui.small_button("50%").clicked() {
                    self.inspector_ratio = 0.50;
                }
                if ui.small_button("70%").clicked() {
                    self.inspector_ratio = 0.70;
                }
                ui.label("Console ratio:");
                ui.add(egui::Slider::new(&mut self.console_ratio, 0.1..=0.9));
                if ui.small_button("20%").clicked() {
                    self.console_ratio = 0.20;
                }
                if ui.small_button("25%").clicked() {
                    self.console_ratio = 0.25;
                }
                if ui.small_button("33%").clicked() {
                    self.console_ratio = 1.0 / 3.0;
                }
                if ui.button("Rebuild").clicked() {
                    self.rebuild();
                }
            });
            ui.horizontal(|ui| {
                ui.label("Central:");
                ui.selectable_value(&mut self.central_sel, CentralSel::None, "None");
                ui.selectable_value(&mut self.central_sel, CentralSel::Scene, "Scene");
                ui.selectable_value(&mut self.central_sel, CentralSel::Inspector, "Inspector");
                if ui.button("Apply Central").clicked() {
                    self.rebuild();
                }
            });
            ui.small("Tip: try dragging tabs/panes; Dock indicators use Imgui-like style.");
        });

        egui::CentralPanel::default().show(ctx, |ui| {
            let mut delegate = Delegate;
            self.tree.ui(&mut delegate, ui);
        });

        if self.show_export {
            egui::Window::new("Blueprint Export")
                .open(&mut self.show_export)
                .resizable(true)
                .vscroll(true)
                .show(ctx, |ui| {
                    ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);
                    ui.monospace(&self.export_text);
                });
        }

        #[cfg(feature = "serde")]
        if self.show_export_full {
            egui::Window::new("Full Blueprint Export")
                .open(&mut self.show_export_full)
                .resizable(true)
                .vscroll(true)
                .show(ctx, |ui| {
                    ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);
                    ui.monospace(&self.export_full_text);
                });
        }

        #[cfg(feature = "serde")]
        if self.show_import_full {
            let mut open = true;
            let mut to_apply: Option<egui_docking::FullBlueprint> = None;
            egui::Window::new("Full Blueprint Import")
                .open(&mut open)
                .resizable(true)
                .vscroll(true)
                .show(ctx, |ui| {
                    ui.label("Paste Full Blueprint JSON and click Apply");
                    ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);
                    ui.add_sized(
                        ui.available_size(),
                        egui::TextEdit::multiline(&mut self.import_full_text),
                    );
                    if ui.button("Apply Import").clicked() {
                        match serde_json::from_str::<egui_docking::FullBlueprint>(
                            &self.import_full_text,
                        ) {
                            Ok(bp) => {
                                // Accept version 0 (missing) or 1; otherwise, show error
                                if bp.version != 0 && bp.version != 1 {
                                    self.import_full_text = format!(
                                        "Unsupported blueprint version: {} (supported: 0/1)\n\n{}",
                                        bp.version, self.import_full_text
                                    );
                                } else if let Some(schema) = &bp.schema {
                                    if schema != "egui_docking.full_blueprint" {
                                        self.import_full_text = format!(
                                            "Unsupported blueprint schema: {} (expected: egui_docking.full_blueprint)\n\n{}",
                                            schema, self.import_full_text
                                        );
                                    } else {
                                        to_apply = Some(bp);
                                    }
                                } else {
                                    to_apply = Some(bp);
                                }
                            }
                            Err(e) => {
                                self.import_full_text =
                                    format!("Parse error: {e}\n\n{}", self.import_full_text);
                            }
                        }
                    }
                });

            if let Some(bp) = to_apply {
                // Rebuild from full blueprint; use same mapping 1->Scene, 2->Inspector, 3->Console
                self.tree = egui_docking::DockBuilder::from_blueprint_full(
                    "builder_demo",
                    bp,
                    |id| match id {
                        1 => Pane::Scene,
                        2 => Pane::Inspector,
                        _ => Pane::Console,
                    },
                );
                self.show_import_full = false;
            } else {
                self.show_import_full = open;
            }
        }
    }
}

fn main() -> Result<(), eframe::Error> {
    env_logger::init();
    let options = eframe::NativeOptions::default();
    eframe::run_native(
        "egui_docking - builder",
        options,
        Box::new(|_cc| Ok(Box::<App>::default())),
    )
}
