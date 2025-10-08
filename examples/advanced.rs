#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")] // hide console window on Windows in release

use eframe::egui;
use egui_docking::{Tile, TileId, Tiles};

fn main() -> Result<(), eframe::Error> {
    env_logger::init(); // Log to stderr (if you run with `RUST_LOG=debug`).
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([800.0, 600.0]),
        ..Default::default()
    };
    eframe::run_native(
        "egui_docking example",
        options,
        Box::new(|_cc| {
            #[cfg_attr(not(feature = "serde"), allow(unused_mut))]
            let mut app = MyApp::default();
            #[cfg(feature = "serde")]
            if let Some(storage) = _cc.storage {
                if let Some(state) = eframe::get_value(storage, eframe::APP_KEY) {
                    app = state;
                }
            }
            Ok(Box::new(app))
        }),
    )
}

#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
pub struct Pane {
    nr: usize,
}

impl std::fmt::Debug for Pane {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("View").field("nr", &self.nr).finish()
    }
}

impl Pane {
    pub fn with_nr(nr: usize) -> Self {
        Self { nr }
    }

    pub fn ui(&self, ui: &mut egui::Ui) -> egui_docking::UiResponse {
        let color = egui::epaint::Hsva::new(0.103 * self.nr as f32, 0.5, 0.5, 1.0);
        ui.painter().rect_filled(ui.max_rect(), 0.0, color);
        let dragged = ui
            .allocate_rect(ui.max_rect(), egui::Sense::click_and_drag())
            .on_hover_cursor(egui::CursorIcon::Grab)
            .dragged();
        if dragged {
            egui_docking::UiResponse::DragStarted
        } else {
            egui_docking::UiResponse::None
        }
    }
}

struct TreeBehavior {
    simplification_options: egui_docking::SimplificationOptions,
    tab_bar_height: f32,
    gap_width: f32,
    add_child_to: Option<egui_docking::TileId>,
    // Dock indicator tuning
    indicator_size: f32,
    indicator_gap: f32,
    indicator_rounding: f32,
    edge_frac: f32,
    mask_opacity: f32,
    // Demo policy toggles
    auto_hide_single_tab: bool,
    show_tab_bar: bool,
    allow_center_dock: bool,
    allow_side_splits: bool,
    // Dock interaction tuning
    snap_px: f32,
    activation_px: f32,
    hysteresis_px: f32,
    dock_requires_shift: bool,
}

impl Default for TreeBehavior {
    fn default() -> Self {
        Self {
            simplification_options: Default::default(),
            tab_bar_height: 24.0,
            gap_width: 2.0,
            add_child_to: None,
            indicator_size: -1.0,     // auto
            indicator_gap: -1.0,      // auto
            indicator_rounding: -1.0, // auto
            edge_frac: 0.35,
            mask_opacity: 0.08,
            auto_hide_single_tab: false,
            show_tab_bar: true,
            allow_center_dock: true,
            allow_side_splits: true,
            snap_px: 8.0,
            activation_px: 4.0,
            hysteresis_px: 6.0,
            dock_requires_shift: false,
        }
    }
}

impl TreeBehavior {
    fn ui(&mut self, ui: &mut egui::Ui) {
        let Self {
            simplification_options,
            tab_bar_height,
            gap_width,
            add_child_to: _,
            ..
        } = self;

        egui::Grid::new("behavior_ui")
            .num_columns(2)
            .show(ui, |ui| {
                ui.label("All panes must have tabs:");
                ui.checkbox(&mut simplification_options.all_panes_must_have_tabs, "");
                ui.end_row();

                ui.label("Join nested containers:");
                ui.checkbox(
                    &mut simplification_options.join_nested_linear_containers,
                    "",
                );
                ui.end_row();

                ui.label("Tab bar height:");
                ui.add(
                    egui::DragValue::new(tab_bar_height)
                        .range(0.0..=100.0)
                        .speed(1.0),
                );
                ui.end_row();

                ui.label("Gap width:");
                ui.add(egui::DragValue::new(gap_width).range(0.0..=20.0).speed(1.0));
                ui.end_row();

                ui.label("Auto-hide single-tab bar:");
                ui.checkbox(&mut self.auto_hide_single_tab, "");
                ui.end_row();

                ui.label("Show tab bar (when not auto-hidden):");
                ui.checkbox(&mut self.show_tab_bar, "");
                ui.end_row();

                ui.label("Allow center dock (tab-merge):");
                ui.checkbox(&mut self.allow_center_dock, "");
                ui.end_row();

                ui.label("Allow side splits (L/R/T/B):");
                ui.checkbox(&mut self.allow_side_splits, "");
                ui.end_row();

                ui.separator();
                ui.end_row();

                ui.label("Dock indicator size (auto<0):");
                ui.add(
                    egui::DragValue::new(&mut self.indicator_size)
                        .range(-1.0..=64.0)
                        .speed(1.0),
                );
                ui.end_row();

                ui.label("Dock indicator gap (auto<0):");
                ui.add(
                    egui::DragValue::new(&mut self.indicator_gap)
                        .range(-1.0..=64.0)
                        .speed(1.0),
                );
                ui.end_row();

                ui.label("Dock indicator rounding (auto<0):");
                ui.add(
                    egui::DragValue::new(&mut self.indicator_rounding)
                        .range(-1.0..=32.0)
                        .speed(1.0),
                );
                ui.end_row();

                ui.label("Dock edge fraction:");
                ui.add(
                    egui::DragValue::new(&mut self.edge_frac)
                        .range(0.1..=0.8)
                        .speed(0.01),
                );
                ui.end_row();

                ui.label("Dock mask opacity:");
                ui.add(
                    egui::DragValue::new(&mut self.mask_opacity)
                        .range(0.0..=0.5)
                        .speed(0.01),
                );
                ui.end_row();

                ui.separator();
                ui.end_row();

                ui.label("Require SHIFT to dock:");
                ui.checkbox(&mut self.dock_requires_shift, "");
                ui.end_row();

                ui.label("Dock snap distance (px):");
                ui.add(
                    egui::DragValue::new(&mut self.snap_px)
                        .range(0.0..=40.0)
                        .speed(0.5),
                );
                ui.end_row();

                ui.label("Dock activation distance (px):");
                ui.add(
                    egui::DragValue::new(&mut self.activation_px)
                        .range(0.0..=40.0)
                        .speed(0.5),
                );
                ui.end_row();

                ui.label("Dock hysteresis (px):");
                ui.add(
                    egui::DragValue::new(&mut self.hysteresis_px)
                        .range(0.0..=60.0)
                        .speed(0.5),
                );
                ui.end_row();
            });
    }
}

impl egui_docking::Behavior<Pane> for TreeBehavior {
    fn pane_ui(
        &mut self,
        ui: &mut egui::Ui,
        _tile_id: egui_docking::TileId,
        view: &mut Pane,
    ) -> egui_docking::UiResponse {
        view.ui(ui)
    }

    fn tab_title_for_pane(&mut self, view: &Pane) -> egui::WidgetText {
        format!("View {}", view.nr).into()
    }

    fn top_bar_right_ui(
        &mut self,
        _tiles: &egui_docking::Tiles<Pane>,
        ui: &mut egui::Ui,
        tile_id: egui_docking::TileId,
        _tabs: &egui_docking::Tabs,
        _scroll_offset: &mut f32,
    ) {
        if ui.button("➕").clicked() {
            self.add_child_to = Some(tile_id);
        }
    }

    // ---
    // Settings:

    fn tab_bar_height(&self, _style: &egui::Style) -> f32 {
        self.tab_bar_height
    }

    fn gap_width(&self, _style: &egui::Style) -> f32 {
        self.gap_width
    }

    fn simplification_options(&self) -> egui_docking::SimplificationOptions {
        self.simplification_options
    }

    fn show_tab_bar(
        &self,
        _tiles: &egui_docking::Tiles<Pane>,
        _tile_id: egui_docking::TileId,
    ) -> bool {
        self.show_tab_bar
    }

    fn auto_hide_single_tab(&self) -> bool {
        self.auto_hide_single_tab
    }

    fn can_dock(
        &self,
        _tiles: &egui_docking::Tiles<Pane>,
        _src_tile: egui_docking::TileId,
        _dst_parent: egui_docking::TileId,
        side: egui_docking::DockSide,
    ) -> bool {
        match side {
            egui_docking::DockSide::Center => self.allow_center_dock,
            _ => self.allow_side_splits,
        }
    }

    fn can_split(
        &self,
        _tiles: &egui_docking::Tiles<Pane>,
        _dst_parent: egui_docking::TileId,
        side: egui_docking::DockSide,
    ) -> bool {
        match side {
            egui_docking::DockSide::Center => true,
            _ => self.allow_side_splits,
        }
    }

    fn dock_indicator_style(&self) -> egui_docking::DockIndicatorStyle {
        egui_docking::DockIndicatorStyle::ImguiLike {
            size: self.indicator_size,
            gap: self.indicator_gap,
            rounding: self.indicator_rounding,
        }
    }

    fn docking_edge_fraction(&self) -> f32 {
        self.edge_frac
    }
    fn docking_mask_opacity(&self) -> f32 {
        self.mask_opacity
    }

    fn dock_requires_modifier(&self) -> Option<egui::Modifiers> {
        if self.dock_requires_shift {
            let mut m = egui::Modifiers::default();
            m.shift = true;
            Some(m)
        } else {
            None
        }
    }

    fn docking_snap_distance(&self) -> f32 {
        self.snap_px
    }

    fn docking_activation_distance(&self) -> f32 {
        self.activation_px
    }

    fn docking_hysteresis_distance(&self) -> f32 {
        self.hysteresis_px
    }

    fn is_tab_closable(&self, _tiles: &Tiles<Pane>, _tile_id: TileId) -> bool {
        true
    }

    fn on_tab_close(&mut self, tiles: &mut Tiles<Pane>, tile_id: TileId) -> bool {
        if let Some(tile) = tiles.get(tile_id) {
            match tile {
                Tile::Pane(pane) => {
                    // Single pane removal
                    let tab_title = self.tab_title_for_pane(pane);
                    log::debug!("Closing tab: {}, tile ID: {tile_id:?}", tab_title.text());
                }
                Tile::Container(container) => {
                    // Container removal
                    log::debug!("Closing container: {:?}", container.kind());
                    let children_ids = container.children();
                    for child_id in children_ids {
                        if let Some(Tile::Pane(pane)) = tiles.get(*child_id) {
                            let tab_title = self.tab_title_for_pane(pane);
                            log::debug!("Closing tab: {}, tile ID: {tile_id:?}", tab_title.text());
                        }
                    }
                }
            }
        }

        // Proceed to removing the tab
        true
    }
}

#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
struct MyApp {
    tree: egui_docking::Tree<Pane>,

    #[cfg_attr(feature = "serde", serde(skip))]
    behavior: TreeBehavior,

    #[cfg_attr(feature = "serde", serde(skip))]
    selected_container: Option<egui_docking::TileId>,

    #[cfg_attr(feature = "serde", serde(skip))]
    apply_include_self: bool,

    #[cfg_attr(feature = "serde", serde(skip))]
    apply_kind_filter: Option<egui_docking::ContainerKind>,

    #[cfg_attr(feature = "serde", serde(skip))]
    apply_scope: ApplyScope,
}

#[derive(Clone, Copy, PartialEq)]
enum ApplyScope {
    Subtree,
    WholeTree,
    Siblings,
    Ancestors,
}

impl Default for ApplyScope {
    fn default() -> Self {
        ApplyScope::Subtree
    }
}

impl Default for MyApp {
    fn default() -> Self {
        let mut next_view_nr = 0;
        let mut gen_view = || {
            let view = Pane::with_nr(next_view_nr);
            next_view_nr += 1;
            view
        };

        let mut tiles = egui_docking::Tiles::default();

        let mut tabs = vec![];
        let tab_tile = {
            let children = (0..7).map(|_| tiles.insert_pane(gen_view())).collect();
            tiles.insert_tab_tile(children)
        };
        tabs.push(tab_tile);
        tabs.push({
            let children = (0..7).map(|_| tiles.insert_pane(gen_view())).collect();
            tiles.insert_horizontal_tile(children)
        });
        tabs.push({
            let children = (0..7).map(|_| tiles.insert_pane(gen_view())).collect();
            tiles.insert_vertical_tile(children)
        });
        tabs.push({
            let cells = (0..11).map(|_| tiles.insert_pane(gen_view())).collect();
            tiles.insert_grid_tile(cells)
        });
        tabs.push(tiles.insert_pane(gen_view()));

        let root = tiles.insert_tab_tile(tabs);

        let tree = egui_docking::Tree::new("my_tree", root, tiles);

        Self {
            tree,
            behavior: Default::default(),
            selected_container: None,
            apply_include_self: true,
            apply_kind_filter: None,
            apply_scope: ApplyScope::Subtree,
        }
    }
}

impl eframe::App for MyApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::SidePanel::left("tree").show(ctx, |ui| {
            if ui.button("Reset").clicked() {
                *self = Default::default();
            }
            self.behavior.ui(ui);

            ui.separator();
            ui.horizontal(|ui| {
                ui.label("Central: No docking");
                ui.checkbox(&mut self.tree.central_no_docking, "");
                if ui.small_button("Use root as central").clicked() {
                    self.tree.central = self.tree.root();
                }
                if ui.small_button("Clear central").clicked() {
                    self.tree.central = None;
                }
            });

            ui.separator();

            ui.collapsing("Tree", |ui| {
                ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);
                let tree_debug = format!("{:#?}", self.tree);
                ui.monospace(&tree_debug);
            });

            ui.separator();

            ui.collapsing("Active tiles", |ui| {
                let active = self.tree.active_tiles();
                for tile_id in active {
                    use egui_docking::Behavior as _;
                    let name = self.behavior.tab_title_for_tile(&self.tree.tiles, tile_id);
                    ui.label(format!("{} - {tile_id:?}", name.text()));
                }
            });

            ui.separator();
            ui.collapsing("Container Flags", |ui| {
                if ui.button("Reset flags (whole tree)").clicked() {
                    // Set all container flags back to default
                    let ids: Vec<_> = self
                        .tree
                        .tiles
                        .iter()
                        .filter_map(|(tid, tile)| match tile {
                            egui_docking::Tile::Container(_) => Some(*tid),
                            _ => None,
                        })
                        .collect();
                    for tid in ids {
                        if let Some(egui_docking::Tile::Container(c)) = self.tree.tiles.get_mut(tid)
                        {
                            *c.flags_mut() = egui_docking::ContainerFlags::default();
                        }
                    }
                }
                // Collect all containers
                let mut containers: Vec<(egui_docking::TileId, egui_docking::ContainerKind)> =
                    Vec::new();
                for (id, tile) in self.tree.tiles.iter() {
                    if let egui_docking::Tile::Container(c) = tile {
                        containers.push((*id, c.kind()));
                    }
                }

                // Selection UI
                let mut sel = self.selected_container;
                egui::ComboBox::from_label("Select container")
                    .selected_text(
                        sel.map(|t| format!("{t:?}"))
                            .unwrap_or_else(|| "<none>".into()),
                    )
                    .show_ui(ui, |ui| {
                        for (id, kind) in &containers {
                            let text = format!("{id:?} - {:?}", kind);
                            if ui.selectable_label(sel == Some(*id), text).clicked() {
                                sel = Some(*id);
                            }
                        }
                    });
                self.selected_container = sel;

                if let Some(id) = self.selected_container {
                    if let Some(egui_docking::Tile::Container(c)) = self.tree.tiles.get_mut(id) {
                        let flags = c.flags_mut();
                        ui.checkbox(&mut flags.no_split, "No split");
                        ui.checkbox(&mut flags.no_tabs, "No tabs (center merge)");
                        ui.checkbox(&mut flags.lock_layout, "Lock layout");
                        ui.horizontal(|ui| {
                            ui.checkbox(&mut self.apply_include_self, "Include self");
                            egui::ComboBox::from_label("Filter kind")
                                .selected_text(match self.apply_kind_filter {
                                    None => "All".to_owned(),
                                    Some(k) => format!("{:?}", k),
                                })
                                .show_ui(ui, |ui| {
                                    if ui
                                        .selectable_label(self.apply_kind_filter.is_none(), "All")
                                        .clicked()
                                    {
                                        self.apply_kind_filter = None;
                                    }
                                    for k in egui_docking::ContainerKind::ALL {
                                        if ui
                                            .selectable_label(
                                                self.apply_kind_filter == Some(k),
                                                format!("{:?}", k),
                                            )
                                            .clicked()
                                        {
                                            self.apply_kind_filter = Some(k);
                                        }
                                    }
                                });
                        });

                        ui.horizontal(|ui| {
                            ui.label("Scope:");
                            ui.selectable_value(
                                &mut self.apply_scope,
                                ApplyScope::Subtree,
                                "Subtree",
                            );
                            ui.selectable_value(
                                &mut self.apply_scope,
                                ApplyScope::WholeTree,
                                "Whole tree",
                            );
                            ui.selectable_value(
                                &mut self.apply_scope,
                                ApplyScope::Siblings,
                                "Siblings",
                            );
                            ui.selectable_value(
                                &mut self.apply_scope,
                                ApplyScope::Ancestors,
                                "Ancestors",
                            );
                        });

                        if ui.button("Apply").clicked() {
                            let target_flags = *flags;
                            match self.apply_scope {
                                ApplyScope::Subtree => {
                                    let mut stack = vec![id];
                                    while let Some(tid) = stack.pop() {
                                        let mut children: Vec<egui_docking::TileId> = Vec::new();
                                        if let Some(tile) = self.tree.tiles.get_mut(tid) {
                                            if let egui_docking::Tile::Container(container) = tile {
                                                let kind = container.kind();
                                                let pass_kind = self
                                                    .apply_kind_filter
                                                    .map_or(true, |k| k == kind);
                                                let pass_self =
                                                    self.apply_include_self || tid != id;
                                                if pass_kind && pass_self {
                                                    *container.flags_mut() = target_flags;
                                                }
                                                children = container.children_vec();
                                            }
                                        }
                                        stack.extend(children);
                                    }
                                }
                                ApplyScope::WholeTree => {
                                    let ids: Vec<_> = self
                                        .tree
                                        .tiles
                                        .iter()
                                        .filter_map(|(tid, tile)| match tile {
                                            egui_docking::Tile::Container(c) => {
                                                Some((*tid, c.kind()))
                                            }
                                            _ => None,
                                        })
                                        .collect();
                                    for (tid, kind) in ids {
                                        if self.apply_kind_filter.map_or(true, |k| k == kind) {
                                            if let Some(egui_docking::Tile::Container(c)) =
                                                self.tree.tiles.get_mut(tid)
                                            {
                                                *c.flags_mut() = target_flags;
                                            }
                                        }
                                    }
                                }
                                ApplyScope::Siblings => {
                                    if let Some(parent_id) = self.tree.tiles.parent_of(id) {
                                        // Optionally include self, then siblings
                                        if self.apply_include_self {
                                            if let Some(egui_docking::Tile::Container(c)) =
                                                self.tree.tiles.get_mut(id)
                                            {
                                                if self
                                                    .apply_kind_filter
                                                    .map_or(true, |k| k == c.kind())
                                                {
                                                    *c.flags_mut() = target_flags;
                                                }
                                            }
                                        }
                                        // Apply to siblings
                                        if let Some(egui_docking::Tile::Container(parent)) =
                                            self.tree.tiles.get(parent_id)
                                        {
                                            let child_ids: Vec<_> =
                                                parent.children().copied().collect();
                                            for child in child_ids {
                                                if child == id {
                                                    continue;
                                                }
                                                if let Some(egui_docking::Tile::Container(c)) =
                                                    self.tree.tiles.get_mut(child)
                                                {
                                                    if self
                                                        .apply_kind_filter
                                                        .map_or(true, |k| k == c.kind())
                                                    {
                                                        *c.flags_mut() = target_flags;
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                                ApplyScope::Ancestors => {
                                    // Walk up to root and apply to ancestor containers (respect filter); include self flag controls first step
                                    let mut cur = id;
                                    let mut first = true;
                                    while let Some(parent_id) = self.tree.tiles.parent_of(cur) {
                                        if first {
                                            if self.apply_include_self {
                                                if let Some(egui_docking::Tile::Container(c)) =
                                                    self.tree.tiles.get_mut(cur)
                                                {
                                                    if self
                                                        .apply_kind_filter
                                                        .map_or(true, |k| k == c.kind())
                                                    {
                                                        *c.flags_mut() = target_flags;
                                                    }
                                                }
                                            }
                                            first = false;
                                        }
                                        if let Some(egui_docking::Tile::Container(c)) =
                                            self.tree.tiles.get_mut(parent_id)
                                        {
                                            if self
                                                .apply_kind_filter
                                                .map_or(true, |k| k == c.kind())
                                            {
                                                *c.flags_mut() = target_flags;
                                            }
                                        }
                                        cur = parent_id;
                                    }
                                }
                            }
                        }
                    } else {
                        ui.label("Selected tile is not a container (or missing)");
                    }
                } else {
                    ui.small("Select a container to edit flags");
                }
            });

            ui.separator();

            if let Some(root) = self.tree.root() {
                tree_ui(ui, &mut self.behavior, &mut self.tree.tiles, root);
            }

            if let Some(parent) = self.behavior.add_child_to.take() {
                let new_child = self.tree.tiles.insert_pane(Pane::with_nr(100));
                if let Some(egui_docking::Tile::Container(egui_docking::Container::Tabs(tabs))) =
                    self.tree.tiles.get_mut(parent)
                {
                    tabs.add_child(new_child);
                    tabs.set_active(new_child);
                }
            }
        });

        egui::CentralPanel::default().show(ctx, |ui| {
            self.tree.ui(&mut self.behavior, ui);
        });
    }

    fn save(&mut self, _storage: &mut dyn eframe::Storage) {
        #[cfg(feature = "serde")]
        eframe::set_value(_storage, eframe::APP_KEY, &self);
    }
}

fn tree_ui(
    ui: &mut egui::Ui,
    behavior: &mut dyn egui_docking::Behavior<Pane>,
    tiles: &mut egui_docking::Tiles<Pane>,
    tile_id: egui_docking::TileId,
) {
    // Get the name BEFORE we remove the tile below!
    let text = format!(
        "{} - {tile_id:?}",
        behavior.tab_title_for_tile(tiles, tile_id).text()
    );

    // Temporarily remove the tile to circumvent the borrowchecker
    let Some(mut tile) = tiles.remove(tile_id) else {
        log::debug!("Missing tile {tile_id:?}");
        return;
    };

    let default_open = true;
    egui::collapsing_header::CollapsingState::load_with_default_open(
        ui.ctx(),
        ui.id().with((tile_id, "tree")),
        default_open,
    )
    .show_header(ui, |ui| {
        ui.label(text);
        let mut visible = tiles.is_visible(tile_id);
        ui.checkbox(&mut visible, "Visible");
        tiles.set_visible(tile_id, visible);
    })
    .body(|ui| match &mut tile {
        egui_docking::Tile::Pane(_) => {}
        egui_docking::Tile::Container(container) => {
            let mut kind = container.kind();
            egui::ComboBox::from_label("Kind")
                .selected_text(format!("{kind:?}"))
                .show_ui(ui, |ui| {
                    for alternative in egui_docking::ContainerKind::ALL {
                        ui.selectable_value(&mut kind, alternative, format!("{alternative:?}"))
                            .clicked();
                    }
                });
            if kind != container.kind() {
                container.set_kind(kind);
            }

            for &child in container.children() {
                tree_ui(ui, behavior, tiles, child);
            }
        }
    });

    // Put the tile back
    tiles.insert(tile_id, tile);
}
