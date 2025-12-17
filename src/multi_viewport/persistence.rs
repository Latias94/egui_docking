use std::collections::HashMap;
use std::path::Path;

use egui::{Context, Id, Pos2, Vec2, ViewportBuilder, ViewportId};
use egui_tiles::{Container, Grid, GridLayout, Linear, LinearDir, Tabs, Tile, Tree, Tiles};

use super::monitor_clamp::clamp_outer_pos_best_effort;
use super::PaneRegistry;

pub const LAYOUT_SNAPSHOT_VERSION: u32 = 2;

#[derive(Debug)]
pub enum LayoutPersistenceError {
    UnsupportedVersion { found: u32, expected: u32 },
    RonSerialize(ron::Error),
    RonDeserialize(ron::error::SpannedError),
    Io(std::io::Error),
}

impl std::fmt::Display for LayoutPersistenceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnsupportedVersion { found, expected } => {
                write!(
                    f,
                    "unsupported layout snapshot version: {found} (expected {expected})"
                )
            }
            Self::RonSerialize(err) => write!(f, "ron serialize error: {err}"),
            Self::RonDeserialize(err) => write!(f, "ron deserialize error: {err}"),
            Self::Io(err) => write!(f, "io error: {err}"),
        }
    }
}

impl std::error::Error for LayoutPersistenceError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::UnsupportedVersion { .. } => None,
            Self::RonSerialize(err) => Some(err),
            Self::RonDeserialize(err) => Some(err),
            Self::Io(err) => Some(err),
        }
    }
}

impl From<std::io::Error> for LayoutPersistenceError {
    fn from(err: std::io::Error) -> Self {
        Self::Io(err)
    }
}

impl From<ron::Error> for LayoutPersistenceError {
    fn from(err: ron::Error) -> Self {
        Self::RonSerialize(err)
    }
}

impl From<ron::error::SpannedError> for LayoutPersistenceError {
    fn from(err: ron::error::SpannedError) -> Self {
        Self::RonDeserialize(err)
    }
}

#[derive(Clone, Copy, Debug, Default, serde::Deserialize, serde::Serialize)]
pub struct ViewportSnapshot {
    pub outer_pos: Option<Pos2>,
    pub inner_size: Option<Vec2>,
    pub fullscreen: bool,
    pub maximized: bool,
    pub pixels_per_point: Option<f32>,
}

#[derive(Clone, Copy, Debug, Default)]
pub(super) struct ViewportRuntime {
    pub outer_pos: Option<Pos2>,
    pub inner_size: Option<Vec2>,
    pub fullscreen: bool,
    pub maximized: bool,
    pub pixels_per_point: f32,
}

#[derive(Clone, Copy, Debug, serde::Deserialize, serde::Serialize)]
pub enum HostSnapshot {
    Root,
    Detached { serial: u64 },
}

#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
pub struct FloatingWindowSnapshot<PaneId> {
    pub id: u64,
    pub tree: TreeSnapshot<PaneId>,
    pub offset_in_dock: Vec2,
    pub size: Vec2,
    pub collapsed: bool,
}

#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
pub struct FloatingManagerSnapshot<PaneId> {
    pub host: HostSnapshot,
    pub windows: Vec<FloatingWindowSnapshot<PaneId>>,
    pub z_order: Vec<u64>,
}

#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
pub struct DetachedSnapshot<PaneId> {
    pub serial: u64,
    pub viewport: ViewportSnapshot,
    pub tree: TreeSnapshot<PaneId>,
}

#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
pub struct LayoutSnapshot<PaneId> {
    pub version: u32,
    pub root: TreeSnapshot<PaneId>,
    pub detached: Vec<DetachedSnapshot<PaneId>>,
    pub floating: Vec<FloatingManagerSnapshot<PaneId>>,
    pub next_detached_serial: u64,
    pub next_floating_id: u64,
}

#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
pub struct TreeSnapshot<PaneId> {
    pub root: Option<usize>,
    pub nodes: Vec<NodeSnapshot<PaneId>>,
}

#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
pub enum NodeSnapshot<PaneId> {
    Pane {
        pane: PaneId,
        visible: bool,
    },
    Tabs {
        children: Vec<usize>,
        active: Option<usize>,
        visible: bool,
    },
    Linear {
        dir: LinearDir,
        children: Vec<usize>,
        shares: Vec<f32>,
        visible: bool,
    },
    Grid {
        layout: GridLayout,
        children: Vec<usize>,
        col_shares: Vec<f32>,
        row_shares: Vec<f32>,
        visible: bool,
    },
}

fn detached_viewport_id_from_serial(serial: u64) -> ViewportId {
    ViewportId::from_hash_of(("egui_docking_detached", serial))
}

fn snapshot_tree<Pane, PaneId>(
    tree: &Tree<Pane>,
    mut pane_to_id: impl FnMut(&Pane) -> PaneId,
) -> TreeSnapshot<PaneId> {
    let mut ids: HashMap<egui_tiles::TileId, usize> = HashMap::new();
    let mut nodes: Vec<NodeSnapshot<PaneId>> = Vec::new();

    fn snapshot_node<Pane, PaneId>(
        tree: &Tree<Pane>,
        tile_id: egui_tiles::TileId,
        ids: &mut HashMap<egui_tiles::TileId, usize>,
        nodes: &mut Vec<NodeSnapshot<PaneId>>,
        pane_to_id: &mut dyn FnMut(&Pane) -> PaneId,
    ) -> usize {
        if let Some(&idx) = ids.get(&tile_id) {
            return idx;
        }

        let visible = tree.tiles.is_visible(tile_id);
        let tile = tree
            .tiles
            .get(tile_id)
            .expect("tree root references missing tile");

        // Important: reserve the index first (push placeholder), then fill it after recursing.
        // This guarantees that child references are stable indices into `nodes`.
        let idx = nodes.len();
        ids.insert(tile_id, idx);
        nodes.push(NodeSnapshot::Tabs {
            children: Vec::new(),
            active: None,
            visible,
        });

        let node = match tile {
            Tile::Pane(pane) => NodeSnapshot::Pane {
                pane: pane_to_id(pane),
                visible,
            },
            Tile::Container(container) => match container {
                Container::Tabs(tabs) => {
                    let children: Vec<usize> = tabs
                        .children
                        .iter()
                        .copied()
                        .map(|child| snapshot_node(tree, child, ids, nodes, pane_to_id))
                        .collect();
                    let active = tabs
                        .active
                        .and_then(|active_id| tabs.children.iter().position(|&c| c == active_id));
                    NodeSnapshot::Tabs {
                        children,
                        active,
                        visible,
                    }
                }
                Container::Linear(linear) => {
                    let children_tile_ids: Vec<_> = linear.children.clone();
                    let children: Vec<usize> = children_tile_ids
                        .iter()
                        .copied()
                        .map(|child| snapshot_node(tree, child, ids, nodes, pane_to_id))
                        .collect();
                    let shares: Vec<f32> = children_tile_ids
                        .iter()
                        .copied()
                        .map(|child| linear.shares[child])
                        .collect();
                    NodeSnapshot::Linear {
                        dir: linear.dir,
                        children,
                        shares,
                        visible,
                    }
                }
                Container::Grid(grid) => {
                    let children_tile_ids: Vec<_> = grid.children().copied().collect();
                    let children: Vec<usize> = children_tile_ids
                        .iter()
                        .copied()
                        .map(|child| snapshot_node(tree, child, ids, nodes, pane_to_id))
                        .collect();
                    NodeSnapshot::Grid {
                        layout: grid.layout,
                        children,
                        col_shares: grid.col_shares.clone(),
                        row_shares: grid.row_shares.clone(),
                        visible,
                    }
                }
            },
        };

        nodes[idx] = node;
        idx
    }

    let root = tree.root.map(|r| snapshot_node(tree, r, &mut ids, &mut nodes, &mut pane_to_id));
    TreeSnapshot { root, nodes }
}

fn restore_tree<Pane, PaneId>(
    tree_id: Id,
    snapshot: TreeSnapshot<PaneId>,
    mut pane_from_id: impl FnMut(PaneId) -> Pane,
) -> Tree<Pane>
where
    PaneId: Clone,
{
    let mut tiles: Tiles<Pane> = Tiles::default();
    let mut built: Vec<Option<egui_tiles::TileId>> = vec![None; snapshot.nodes.len()];

    fn build_node<Pane, PaneId>(
        snapshot: &TreeSnapshot<PaneId>,
        idx: usize,
        tiles: &mut Tiles<Pane>,
        built: &mut [Option<egui_tiles::TileId>],
        pane_from_id: &mut dyn FnMut(PaneId) -> Pane,
    ) -> egui_tiles::TileId
    where
        PaneId: Clone,
    {
        if let Some(id) = built[idx] {
            return id;
        }

        let (tile_id, visible) = match &snapshot.nodes[idx] {
            NodeSnapshot::Pane { pane, visible } => (tiles.insert_pane(pane_from_id(pane.clone())), *visible),
            NodeSnapshot::Tabs {
                children,
                active,
                visible,
            } => {
                let child_ids: Vec<_> = children
                    .iter()
                    .copied()
                    .map(|c| build_node(snapshot, c, tiles, built, pane_from_id))
                    .collect();
                let mut tabs = Tabs::new(child_ids.clone());
                if let Some(active_index) = *active {
                    if let Some(&active_child) = child_ids.get(active_index) {
                        tabs.active = Some(active_child);
                    }
                }
                (tiles.insert_container(tabs), *visible)
            }
            NodeSnapshot::Linear {
                dir,
                children,
                shares,
                visible,
            } => {
                let child_ids: Vec<_> = children
                    .iter()
                    .copied()
                    .map(|c| build_node(snapshot, c, tiles, built, pane_from_id))
                    .collect();
                let mut linear = Linear::new(*dir, child_ids.clone());
                for (child_id, share) in child_ids.iter().copied().zip(shares.iter().copied()) {
                    if share.is_finite() && share != 1.0 {
                        linear.shares.set_share(child_id, share);
                    }
                }
                (tiles.insert_container(linear), *visible)
            }
            NodeSnapshot::Grid {
                layout,
                children,
                col_shares,
                row_shares,
                visible,
            } => {
                let child_ids: Vec<_> = children
                    .iter()
                    .copied()
                    .map(|c| build_node(snapshot, c, tiles, built, pane_from_id))
                    .collect();
                let mut grid = Grid::new(child_ids);
                grid.layout = *layout;
                grid.col_shares = col_shares.clone();
                grid.row_shares = row_shares.clone();
                (tiles.insert_container(grid), *visible)
            }
        };

        tiles.set_visible(tile_id, visible);
        built[idx] = Some(tile_id);
        tile_id
    }

    let root = snapshot
        .root
        .map(|idx| build_node(&snapshot, idx, &mut tiles, &mut built, &mut pane_from_id));

    match root {
        Some(root) => Tree::new(tree_id, root, tiles),
        None => Tree::empty(tree_id),
    }
}

fn restore_tree_try<Pane, PaneId>(
    tree_id: Id,
    snapshot: TreeSnapshot<PaneId>,
    mut pane_from_id: impl FnMut(PaneId) -> Option<Pane>,
) -> Tree<Pane>
where
    PaneId: Clone,
{
    let mut tiles: Tiles<Pane> = Tiles::default();
    let mut built: Vec<Option<egui_tiles::TileId>> = vec![None; snapshot.nodes.len()];
    let mut missing: Vec<bool> = vec![false; snapshot.nodes.len()];

    fn build_node_try<Pane, PaneId>(
        snapshot: &TreeSnapshot<PaneId>,
        idx: usize,
        tiles: &mut Tiles<Pane>,
        built: &mut [Option<egui_tiles::TileId>],
        missing: &mut [bool],
        pane_from_id: &mut dyn FnMut(PaneId) -> Option<Pane>,
    ) -> Option<egui_tiles::TileId>
    where
        PaneId: Clone,
    {
        if missing[idx] {
            return None;
        }
        if let Some(id) = built[idx] {
            return Some(id);
        }

        let (tile_id, visible) = match &snapshot.nodes[idx] {
            NodeSnapshot::Pane { pane, visible } => {
                let Some(pane) = pane_from_id(pane.clone()) else {
                    missing[idx] = true;
                    return None;
                };
                (tiles.insert_pane(pane), *visible)
            }
            NodeSnapshot::Tabs {
                children,
                active,
                visible,
            } => {
                let mut child_ids: Vec<egui_tiles::TileId> = Vec::with_capacity(children.len());
                for c in children.iter().copied() {
                    if let Some(child_id) =
                        build_node_try(snapshot, c, tiles, built, missing, pane_from_id)
                    {
                        child_ids.push(child_id);
                    }
                }
                if child_ids.is_empty() {
                    missing[idx] = true;
                    return None;
                }
                let mut tabs = Tabs::new(child_ids.clone());
                if let Some(active_index) = *active {
                    if let Some(&active_child) = child_ids.get(active_index) {
                        tabs.active = Some(active_child);
                    }
                }
                (tiles.insert_container(tabs), *visible)
            }
            NodeSnapshot::Linear {
                dir,
                children,
                shares,
                visible,
            } => {
                let mut child_ids: Vec<egui_tiles::TileId> = Vec::with_capacity(children.len());
                let mut child_shares: Vec<f32> = Vec::with_capacity(children.len());
                for (c, share) in children.iter().copied().zip(shares.iter().copied()) {
                    if let Some(child_id) =
                        build_node_try(snapshot, c, tiles, built, missing, pane_from_id)
                    {
                        child_ids.push(child_id);
                        child_shares.push(share);
                    }
                }
                if child_ids.is_empty() {
                    missing[idx] = true;
                    return None;
                }

                let mut linear = Linear::new(*dir, child_ids.clone());
                for (child_id, share) in child_ids.iter().copied().zip(child_shares.iter().copied())
                {
                    if share.is_finite() && share != 1.0 {
                        linear.shares.set_share(child_id, share);
                    }
                }
                (tiles.insert_container(linear), *visible)
            }
            NodeSnapshot::Grid {
                layout,
                children,
                col_shares,
                row_shares,
                visible,
            } => {
                let mut child_ids: Vec<egui_tiles::TileId> = Vec::with_capacity(children.len());
                for c in children.iter().copied() {
                    if let Some(child_id) =
                        build_node_try(snapshot, c, tiles, built, missing, pane_from_id)
                    {
                        child_ids.push(child_id);
                    }
                }
                if child_ids.is_empty() {
                    missing[idx] = true;
                    return None;
                }

                let mut grid = Grid::new(child_ids);
                grid.layout = *layout;
                grid.col_shares = col_shares.clone();
                grid.row_shares = row_shares.clone();
                (tiles.insert_container(grid), *visible)
            }
        };

        tiles.set_visible(tile_id, visible);
        built[idx] = Some(tile_id);
        Some(tile_id)
    }

    let root = snapshot
        .root
        .and_then(|idx| build_node_try(&snapshot, idx, &mut tiles, &mut built, &mut missing, &mut pane_from_id));

    match root {
        Some(root) => Tree::new(tree_id, root, tiles),
        None => Tree::empty(tree_id),
    }
}

fn pretty_ron_config() -> ron::ser::PrettyConfig {
    ron::ser::PrettyConfig::new()
        .depth_limit(128)
        .separate_tuple_members(true)
        .enumerate_arrays(true)
}

impl<Pane> super::DockingMultiViewport<Pane> {
    pub(super) fn capture_viewport_runtime(&mut self, ctx: &Context) {
        let viewport_id = ctx.viewport_id();
        let runtime = ctx.input(|i| {
            let outer_pos = i.viewport().outer_rect.map(|r| r.min);
            let inner_size = i.viewport().inner_rect.map(|r| r.size());
            ViewportRuntime {
                outer_pos,
                inner_size,
                fullscreen: i.viewport().fullscreen.unwrap_or(false),
                maximized: i.viewport().maximized.unwrap_or(false),
                pixels_per_point: ctx.pixels_per_point(),
            }
        });
        self.last_viewport_runtime.insert(viewport_id, runtime);
    }

    fn viewport_snapshot_for_detached(
        &self,
        viewport_id: ViewportId,
        detached: &super::types::DetachedDock<Pane>,
    ) -> ViewportSnapshot {
        let runtime = self.last_viewport_runtime.get(&viewport_id).copied();
        ViewportSnapshot {
            outer_pos: runtime
                .and_then(|r| r.outer_pos)
                .or(detached.builder.position),
            inner_size: runtime
                .and_then(|r| r.inner_size)
                .or(detached.builder.inner_size),
            fullscreen: runtime.map(|r| r.fullscreen).unwrap_or(false),
            maximized: runtime.map(|r| r.maximized).unwrap_or(false),
            pixels_per_point: runtime.map(|r| r.pixels_per_point),
        }
    }

    fn viewport_builder_from_snapshot(
        &self,
        snapshot: ViewportSnapshot,
        title_hint: &str,
    ) -> ViewportBuilder {
        let mut builder = ViewportBuilder::default()
            .with_title(title_hint)
            .with_decorations(self.options.detached_viewport_decorations)
            .with_fullscreen(snapshot.fullscreen)
            .with_maximized(snapshot.maximized);
        if let Some(pos) = snapshot.outer_pos {
            builder = builder.with_position(pos);
        }
        if let Some(size) = snapshot.inner_size {
            builder = builder.with_inner_size(size);
        }
        builder
    }

    fn clear_interaction_state_for_load(&mut self) {
        self.pending_drop = None;
        self.pending_internal_drop = None;
        self.pending_local_drop = None;
        self.ghost = None;
        self.drag_state = super::drag_state::DragState::default();

        self.last_root_dock_rect = None;
        self.last_dock_rects.clear();
        self.viewport_outer_from_inner_offset.clear();
        self.last_floating_rects.clear();
        self.last_floating_content_rects.clear();
        self.detached_rendered_frame.clear();
        self.last_viewport_runtime.clear();
    }

    fn snapshot_layout_impl<PaneId>(
        &self,
        mut pane_to_id: impl FnMut(&Pane) -> PaneId,
    ) -> LayoutSnapshot<PaneId> {
        let root = snapshot_tree(&self.tree, &mut pane_to_id);

        let detached: Vec<_> = self
            .detached
            .iter()
            .map(|(viewport_id, detached)| DetachedSnapshot {
                serial: detached.serial,
                viewport: self.viewport_snapshot_for_detached(*viewport_id, detached),
                tree: snapshot_tree(&detached.tree, &mut pane_to_id),
            })
            .collect();

        let floating: Vec<_> = self
            .floating
            .iter()
            .map(|(viewport_id, manager)| {
                let host = if *viewport_id == ViewportId::ROOT {
                    HostSnapshot::Root
                } else if let Some(detached) = self.detached.get(viewport_id) {
                    HostSnapshot::Detached {
                        serial: detached.serial,
                    }
                } else {
                    HostSnapshot::Root
                };

                let windows = manager
                    .windows
                    .iter()
                    .map(|(&id, w)| FloatingWindowSnapshot {
                        id,
                        tree: snapshot_tree(&w.tree, &mut pane_to_id),
                        offset_in_dock: w.offset_in_dock,
                        size: w.size,
                        collapsed: w.collapsed,
                    })
                    .collect();

                FloatingManagerSnapshot {
                    host,
                    windows,
                    z_order: manager.z_order.clone(),
                }
            })
            .collect();

        LayoutSnapshot {
            version: LAYOUT_SNAPSHOT_VERSION,
            root,
            detached,
            floating,
            next_detached_serial: self.next_viewport_serial,
            next_floating_id: self.next_floating_serial,
        }
    }

    pub fn snapshot_layout<PaneId>(
        &self,
        pane_to_id: impl FnMut(&Pane) -> PaneId,
    ) -> LayoutSnapshot<PaneId> {
        self.snapshot_layout_impl(pane_to_id)
    }

    pub fn snapshot_layout_with_registry<R>(&self, registry: &mut R) -> LayoutSnapshot<R::PaneId>
    where
        R: PaneRegistry<Pane>,
    {
        self.snapshot_layout_impl(|pane| registry.pane_id(pane))
    }

    pub fn snapshot_layout_to_ron_string_with_registry<R>(
        &self,
        registry: &mut R,
    ) -> Result<String, LayoutPersistenceError>
    where
        R: PaneRegistry<Pane>,
    {
        let snapshot = self.snapshot_layout_with_registry(registry);
        Ok(ron::ser::to_string_pretty(&snapshot, pretty_ron_config())?)
    }

    pub fn snapshot_layout_to_ron_string<PaneId>(
        &self,
        pane_to_id: impl FnMut(&Pane) -> PaneId,
    ) -> Result<String, LayoutPersistenceError>
    where
        PaneId: serde::Serialize,
    {
        let snapshot = self.snapshot_layout_impl(pane_to_id);
        Ok(ron::ser::to_string_pretty(&snapshot, pretty_ron_config())?)
    }

    pub fn save_layout_to_ron_file_with_registry<R>(
        &self,
        path: impl AsRef<Path>,
        registry: &mut R,
    ) -> Result<(), LayoutPersistenceError>
    where
        R: PaneRegistry<Pane>,
    {
        let ron = self.snapshot_layout_to_ron_string_with_registry(registry)?;
        std::fs::write(path, ron)?;
        Ok(())
    }

    pub fn save_layout_to_ron_file<PaneId>(
        &self,
        path: impl AsRef<Path>,
        pane_to_id: impl FnMut(&Pane) -> PaneId,
    ) -> Result<(), LayoutPersistenceError>
    where
        PaneId: serde::Serialize,
    {
        let ron = self.snapshot_layout_to_ron_string(pane_to_id)?;
        std::fs::write(path, ron)?;
        Ok(())
    }

    fn load_layout_snapshot_impl<PaneId>(
        &mut self,
        snapshot: LayoutSnapshot<PaneId>,
        mut pane_from_id: impl FnMut(PaneId) -> Pane,
    ) -> Result<(), LayoutPersistenceError>
    where
        PaneId: Clone,
    {
        if snapshot.version != LAYOUT_SNAPSHOT_VERSION {
            return Err(LayoutPersistenceError::UnsupportedVersion {
                found: snapshot.version,
                expected: LAYOUT_SNAPSHOT_VERSION,
            });
        }

        self.clear_interaction_state_for_load();

        let bridge_id = self.tree.id();

        self.tree = restore_tree(bridge_id, snapshot.root, &mut pane_from_id);

        self.detached.clear();
        self.floating.clear();

        let mut max_detached_serial = 0u64;
        for detached in snapshot.detached {
            max_detached_serial = max_detached_serial.max(detached.serial);

            let viewport_id = detached_viewport_id_from_serial(detached.serial);
            let detached_tree_id =
                Id::new((bridge_id, "egui_docking_detached_tree", detached.serial));
            let tree = restore_tree(detached_tree_id, detached.tree, &mut pane_from_id);

            let builder = self.viewport_builder_from_snapshot(detached.viewport, "detached");

            self.detached.insert(
                viewport_id,
                super::types::DetachedDock {
                    serial: detached.serial,
                    tree,
                    builder,
                },
            );
        }

        let mut max_floating_id = 0u64;
        for manager in snapshot.floating {
            let viewport_id = match manager.host {
                HostSnapshot::Root => ViewportId::ROOT,
                HostSnapshot::Detached { serial } => detached_viewport_id_from_serial(serial),
            };

            let mut restored = super::types::FloatingManager::default();
            for w in manager.windows {
                max_floating_id = max_floating_id.max(w.id);

                let floating_tree_id = Id::new((bridge_id, "egui_docking_floating_tree", w.id));
                let tree = restore_tree(floating_tree_id, w.tree, &mut pane_from_id);

                restored.windows.insert(
                    w.id,
                    super::types::FloatingDockWindow {
                        tree,
                        offset_in_dock: w.offset_in_dock,
                        size: w.size,
                        collapsed: w.collapsed,
                        drag: None,
                        resize: None,
                    },
                );
            }
            restored.z_order = manager.z_order;
            restored
                .z_order
                .retain(|id| restored.windows.contains_key(id));
            for id in restored.windows.keys().copied().collect::<Vec<_>>() {
                if !restored.z_order.contains(&id) {
                    restored.z_order.push(id);
                }
            }

            self.floating.insert(viewport_id, restored);
        }

        self.next_viewport_serial = snapshot
            .next_detached_serial
            .max(max_detached_serial.saturating_add(1))
            .max(1);
        self.next_floating_serial = snapshot
            .next_floating_id
            .max(max_floating_id.saturating_add(1))
            .max(1);

        Ok(())
    }

    pub fn load_layout_snapshot_with_registry<R>(
        &mut self,
        snapshot: LayoutSnapshot<R::PaneId>,
        registry: &mut R,
    ) -> Result<(), LayoutPersistenceError>
    where
        R: PaneRegistry<Pane>,
    {
        if snapshot.version != LAYOUT_SNAPSHOT_VERSION {
            return Err(LayoutPersistenceError::UnsupportedVersion {
                found: snapshot.version,
                expected: LAYOUT_SNAPSHOT_VERSION,
            });
        }

        self.clear_interaction_state_for_load();

        let bridge_id = self.tree.id();

        self.tree = restore_tree_try(bridge_id, snapshot.root, |id| registry.try_pane_from_id(id));

        self.detached.clear();
        self.floating.clear();

        let mut max_detached_serial = 0u64;
        for detached in snapshot.detached {
            max_detached_serial = max_detached_serial.max(detached.serial);

            let viewport_id = detached_viewport_id_from_serial(detached.serial);
            let detached_tree_id =
                Id::new((bridge_id, "egui_docking_detached_tree", detached.serial));
            let tree = restore_tree_try(detached_tree_id, detached.tree, |id| {
                registry.try_pane_from_id(id)
            });
            if tree.root.is_none() {
                continue;
            }

            let builder = self.viewport_builder_from_snapshot(detached.viewport, "detached");

            self.detached.insert(
                viewport_id,
                super::types::DetachedDock {
                    serial: detached.serial,
                    tree,
                    builder,
                },
            );
        }

        let mut max_floating_id = 0u64;
        for manager in snapshot.floating {
            let viewport_id = match manager.host {
                HostSnapshot::Root => ViewportId::ROOT,
                HostSnapshot::Detached { serial } => detached_viewport_id_from_serial(serial),
            };

            let mut restored = super::types::FloatingManager::default();
            for w in manager.windows {
                max_floating_id = max_floating_id.max(w.id);

                let floating_tree_id = Id::new((bridge_id, "egui_docking_floating_tree", w.id));
                let tree = restore_tree_try(floating_tree_id, w.tree, |id| {
                    registry.try_pane_from_id(id)
                });

                if tree.root.is_none() {
                    continue;
                }

                restored.windows.insert(
                    w.id,
                    super::types::FloatingDockWindow {
                        tree,
                        offset_in_dock: w.offset_in_dock,
                        size: w.size,
                        collapsed: w.collapsed,
                        drag: None,
                        resize: None,
                    },
                );
            }
            restored.z_order = manager.z_order;
            restored
                .z_order
                .retain(|id| restored.windows.contains_key(id));
            for id in restored.windows.keys().copied().collect::<Vec<_>>() {
                if !restored.z_order.contains(&id) {
                    restored.z_order.push(id);
                }
            }

            if !restored.windows.is_empty() {
                self.floating.insert(viewport_id, restored);
            }
        }

        self.next_viewport_serial = snapshot
            .next_detached_serial
            .max(max_detached_serial.saturating_add(1))
            .max(1);
        self.next_floating_serial = snapshot
            .next_floating_id
            .max(max_floating_id.saturating_add(1))
            .max(1);

        Ok(())
    }

    pub fn load_layout_snapshot<PaneId>(
        &mut self,
        snapshot: LayoutSnapshot<PaneId>,
        pane_from_id: impl FnMut(PaneId) -> Pane,
    ) -> Result<(), LayoutPersistenceError>
    where
        PaneId: Clone,
    {
        self.load_layout_snapshot_impl(snapshot, pane_from_id)
    }

    /// Like [`Self::load_layout_snapshot`], but clears any active `egui::DragAndDrop` payload first.
    ///
    /// This is a practical safety net: loading a layout while a drag session is still active can
    /// leave stale payloads that interfere with subsequent docking interactions.
    pub fn load_layout_snapshot_in_ctx_with_registry<R>(
        &mut self,
        ctx: &Context,
        snapshot: LayoutSnapshot<R::PaneId>,
        registry: &mut R,
    ) -> Result<(), LayoutPersistenceError>
    where
        R: PaneRegistry<Pane>,
    {
        egui::DragAndDrop::clear_payload(ctx);
        self.load_layout_snapshot_with_registry(snapshot, registry)?;

        // Apply geometry to already-existing native viewports immediately.
        // For viewports that are created on the next frame, `ViewportBuilder` will take effect.
        let mut clamp_builder_pos: Vec<(ViewportId, Pos2)> = Vec::new();
        for (&viewport_id, detached) in &self.detached {
            if detached.builder.fullscreen == Some(true) || detached.builder.maximized == Some(true)
            {
                continue;
            }

            if let Some(pos) = detached.builder.position {
                let pos = if let Some(size) = detached.builder.inner_size {
                    clamp_outer_pos_best_effort(ctx, pos, size)
                } else {
                    pos
                };
                ctx.send_viewport_cmd_to(viewport_id, egui::ViewportCommand::OuterPosition(pos));
                clamp_builder_pos.push((viewport_id, pos));
            }
            if let Some(size) = detached.builder.inner_size {
                ctx.send_viewport_cmd_to(viewport_id, egui::ViewportCommand::InnerSize(size));
            }
            if let Some(fullscreen) = detached.builder.fullscreen {
                ctx.send_viewport_cmd_to(viewport_id, egui::ViewportCommand::Fullscreen(fullscreen));
            }
            if let Some(maximized) = detached.builder.maximized {
                ctx.send_viewport_cmd_to(viewport_id, egui::ViewportCommand::Maximized(maximized));
            }
        }

        // Keep the builder in sync so the next `show_viewport_immediate` also uses the clamped position.
        for (viewport_id, pos) in clamp_builder_pos {
            if let Some(detached) = self.detached.get_mut(&viewport_id) {
                detached.builder = detached.builder.clone().with_position(pos);
            }
        }

        ctx.request_repaint();
        Ok(())
    }

    pub fn load_layout_snapshot_in_ctx<PaneId>(
        &mut self,
        ctx: &Context,
        snapshot: LayoutSnapshot<PaneId>,
        pane_from_id: impl FnMut(PaneId) -> Pane,
    ) -> Result<(), LayoutPersistenceError>
    where
        PaneId: Clone,
    {
        egui::DragAndDrop::clear_payload(ctx);
        self.load_layout_snapshot_impl(snapshot, pane_from_id)?;

        // Apply geometry to already-existing native viewports immediately.
        // For viewports that are created on the next frame, `ViewportBuilder` will take effect.
        let mut clamp_builder_pos: Vec<(ViewportId, Pos2)> = Vec::new();
        for (&viewport_id, detached) in &self.detached {
            if detached.builder.fullscreen == Some(true) || detached.builder.maximized == Some(true)
            {
                continue;
            }

            if let Some(pos) = detached.builder.position {
                let pos = if let Some(size) = detached.builder.inner_size {
                    clamp_outer_pos_best_effort(ctx, pos, size)
                } else {
                    pos
                };
                ctx.send_viewport_cmd_to(viewport_id, egui::ViewportCommand::OuterPosition(pos));
                clamp_builder_pos.push((viewport_id, pos));
            }
            if let Some(size) = detached.builder.inner_size {
                ctx.send_viewport_cmd_to(viewport_id, egui::ViewportCommand::InnerSize(size));
            }
            if let Some(fullscreen) = detached.builder.fullscreen {
                ctx.send_viewport_cmd_to(
                    viewport_id,
                    egui::ViewportCommand::Fullscreen(fullscreen),
                );
            }
            if let Some(maximized) = detached.builder.maximized {
                ctx.send_viewport_cmd_to(
                    viewport_id,
                    egui::ViewportCommand::Maximized(maximized),
                );
            }
        }

        for (viewport_id, pos) in clamp_builder_pos {
            if let Some(detached) = self.detached.get_mut(&viewport_id) {
                detached.builder = detached.builder.clone().with_position(pos);
            }
        }

        ctx.request_repaint();
        Ok(())
    }

    pub fn load_layout_from_ron_str<PaneId>(
        &mut self,
        ron_str: &str,
        pane_from_id: impl FnMut(PaneId) -> Pane,
    ) -> Result<(), LayoutPersistenceError>
    where
        PaneId: Clone + for<'de> serde::Deserialize<'de>,
    {
        let snapshot: LayoutSnapshot<PaneId> = ron::from_str(ron_str)?;
        self.load_layout_snapshot(snapshot, pane_from_id)
    }

    pub fn load_layout_from_ron_str_with_registry<R>(
        &mut self,
        ron_str: &str,
        registry: &mut R,
    ) -> Result<(), LayoutPersistenceError>
    where
        R: PaneRegistry<Pane>,
    {
        let snapshot: LayoutSnapshot<R::PaneId> = ron::from_str(ron_str)?;
        self.load_layout_snapshot_with_registry(snapshot, registry)
    }

    pub fn load_layout_from_ron_str_in_ctx<PaneId>(
        &mut self,
        ctx: &Context,
        ron_str: &str,
        pane_from_id: impl FnMut(PaneId) -> Pane,
    ) -> Result<(), LayoutPersistenceError>
    where
        PaneId: Clone + for<'de> serde::Deserialize<'de>,
    {
        let snapshot: LayoutSnapshot<PaneId> = ron::from_str(ron_str)?;
        self.load_layout_snapshot_in_ctx(ctx, snapshot, pane_from_id)
    }

    pub fn load_layout_from_ron_str_in_ctx_with_registry<R>(
        &mut self,
        ctx: &Context,
        ron_str: &str,
        registry: &mut R,
    ) -> Result<(), LayoutPersistenceError>
    where
        R: PaneRegistry<Pane>,
    {
        let snapshot: LayoutSnapshot<R::PaneId> = ron::from_str(ron_str)?;
        self.load_layout_snapshot_in_ctx_with_registry(ctx, snapshot, registry)
    }

    pub fn load_layout_from_ron_file<PaneId>(
        &mut self,
        path: impl AsRef<Path>,
        pane_from_id: impl FnMut(PaneId) -> Pane,
    ) -> Result<(), LayoutPersistenceError>
    where
        PaneId: Clone + for<'de> serde::Deserialize<'de>,
    {
        let ron_str = std::fs::read_to_string(path)?;
        self.load_layout_from_ron_str(&ron_str, pane_from_id)
    }

    pub fn load_layout_from_ron_file_with_registry<R>(
        &mut self,
        path: impl AsRef<Path>,
        registry: &mut R,
    ) -> Result<(), LayoutPersistenceError>
    where
        R: PaneRegistry<Pane>,
    {
        let ron_str = std::fs::read_to_string(path)?;
        self.load_layout_from_ron_str_with_registry(&ron_str, registry)
    }

    pub fn load_layout_from_ron_file_in_ctx<PaneId>(
        &mut self,
        ctx: &Context,
        path: impl AsRef<Path>,
        pane_from_id: impl FnMut(PaneId) -> Pane,
    ) -> Result<(), LayoutPersistenceError>
    where
        PaneId: Clone + for<'de> serde::Deserialize<'de>,
    {
        let ron_str = std::fs::read_to_string(path)?;
        self.load_layout_from_ron_str_in_ctx(ctx, &ron_str, pane_from_id)
    }

    pub fn load_layout_from_ron_file_in_ctx_with_registry<R>(
        &mut self,
        ctx: &Context,
        path: impl AsRef<Path>,
        registry: &mut R,
    ) -> Result<(), LayoutPersistenceError>
    where
        R: PaneRegistry<Pane>,
    {
        let ron_str = std::fs::read_to_string(path)?;
        self.load_layout_from_ron_str_in_ctx_with_registry(ctx, &ron_str, registry)
    }
}

#[cfg(all(test, feature = "persistence"))]
mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use crate::multi_viewport::types::{DetachedDock, FloatingDockWindow, FloatingManager};
    use crate::multi_viewport::PaneRegistry;

    #[derive(Clone, Debug)]
    struct Pane {
        id: usize,
    }

    fn new_tree_tabs(id: Id, pane_ids: &[usize]) -> Tree<Pane> {
        let mut tiles = egui_tiles::Tiles::default();
        let children: Vec<_> = pane_ids
            .iter()
            .copied()
            .map(|id| tiles.insert_pane(Pane { id }))
            .collect();
        let root = tiles.insert_tab_tile(children);
        Tree::new(id, root, tiles)
    }

    #[test]
    fn persistence_roundtrip_restores_layout() {
        let root_tree = new_tree_tabs(Id::new("root"), &[1, 2, 3]);
        let mut docking = crate::multi_viewport::DockingMultiViewport::new(root_tree);

        // Detached viewport:
        let serial = 42u64;
        let viewport_id = ViewportId::from_hash_of(("egui_docking_detached", serial));
        let detached_tree = new_tree_tabs(Id::new(("detached_tree", serial)), &[10, 11]);
        docking.detached.insert(
            viewport_id,
            DetachedDock {
                serial,
                tree: detached_tree,
                builder: ViewportBuilder::default()
                    .with_title("detached")
                    .with_position(Pos2::new(123.0, 456.0))
                    .with_inner_size(Vec2::new(640.0, 480.0)),
            },
        );

        // Floating window in root:
        let floating_id = 7u64;
        let floating_tree = new_tree_tabs(Id::new(("floating_tree", floating_id)), &[100]);
        docking.floating.insert(
            ViewportId::ROOT,
            FloatingManager {
                windows: BTreeMap::from([(
                    floating_id,
                    FloatingDockWindow {
                        tree: floating_tree,
                        offset_in_dock: Vec2::new(12.0, 34.0),
                        size: Vec2::new(320.0, 200.0),
                        collapsed: true,
                        drag: None,
                        resize: None,
                    },
                )]),
                z_order: vec![floating_id],
            },
        );

        let ron = docking
            .snapshot_layout_to_ron_string::<usize>(|pane| pane.id)
            .unwrap();

        let mut restored = crate::multi_viewport::DockingMultiViewport::new(Tree::empty("restored"));
        restored
            .load_layout_from_ron_str::<usize>(&ron, |id| Pane { id })
            .unwrap();

        // Detached restored:
        assert_eq!(restored.detached.len(), 1);
        let detached = restored.detached.get(&viewport_id).unwrap();
        assert_eq!(detached.serial, serial);

        // Floating restored:
        let root_manager = restored.floating.get(&ViewportId::ROOT).unwrap();
        assert_eq!(root_manager.windows.len(), 1);
        assert_eq!(root_manager.z_order, vec![floating_id]);
        let w = root_manager.windows.get(&floating_id).unwrap();
        assert_eq!(w.offset_in_dock, Vec2::new(12.0, 34.0));
        assert_eq!(w.size, Vec2::new(320.0, 200.0));
        assert!(w.collapsed);
    }

    #[test]
    fn missing_panes_are_dropped_on_load() {
        let root_tree = new_tree_tabs(Id::new("root"), &[1, 2, 3]);
        let docking = crate::multi_viewport::DockingMultiViewport::new(root_tree);

        let ron = docking
            .snapshot_layout_to_ron_string::<usize>(|pane| pane.id)
            .unwrap();

        struct DroppingRegistry {
            drop_id: usize,
        }

        impl PaneRegistry<Pane> for DroppingRegistry {
            type PaneId = usize;

            fn pane_id(&mut self, pane: &Pane) -> Self::PaneId {
                pane.id
            }

            fn pane_from_id(&mut self, id: Self::PaneId) -> Pane {
                Pane { id }
            }

            fn try_pane_from_id(&mut self, id: Self::PaneId) -> Option<Pane> {
                (id != self.drop_id).then_some(Pane { id })
            }
        }

        let mut restored = crate::multi_viewport::DockingMultiViewport::new(Tree::empty("restored"));
        let mut registry = DroppingRegistry { drop_id: 2 };

        restored
            .load_layout_from_ron_str_with_registry(&ron, &mut registry)
            .unwrap();

        // Pane 2 dropped, but the tree remains non-empty.
        let root = restored.tree.root.unwrap();
        let panes: Vec<_> = restored
            .tree
            .tiles
            .tile_ids()
            .filter_map(|id| restored.tree.tiles.get(id))
            .filter_map(|t| match t {
                Tile::Pane(p) => Some(p.id),
                Tile::Container(_) => None,
            })
            .collect();
        assert!(panes.contains(&1));
        assert!(!panes.contains(&2));
        assert!(panes.contains(&3));

        // Root still exists.
        assert!(restored.tree.tiles.get(root).is_some());
    }
}
