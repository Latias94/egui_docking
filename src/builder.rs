use crate::{Container, Linear, LinearDir, Tabs, Tile, TileId, Tiles, Tree};

/// Programmatic builder for docking layouts.
///
/// Build a tree of panes and containers, then call `build()` to get a `Tree`.
pub struct DockBuilder<Pane> {
    id: egui::Id,
    tiles: Tiles<Pane>,
    root: Option<TileId>,
    central: Option<TileId>,
    central_passthrough: bool,
}

impl<Pane> DockBuilder<Pane> {
    pub fn new(id: impl Into<egui::Id>) -> Self {
        Self {
            id: id.into(),
            tiles: Tiles::default(),
            root: None,
            central: None,
            central_passthrough: false,
        }
    }

    pub fn tiles(&self) -> &Tiles<Pane> {
        &self.tiles
    }
    pub fn tiles_mut(&mut self) -> &mut Tiles<Pane> {
        &mut self.tiles
    }

    pub fn set_root(&mut self, root: TileId) {
        self.root = Some(root);
    }

    /// Mark a tile as central (e.g. for passthrough).
    pub fn set_central(&mut self, tile: TileId) -> &mut Self {
        self.central = Some(tile);
        self
    }

    /// Enable/disable central passthrough overlay behavior.
    pub fn set_central_passthrough(&mut self, on: bool) -> &mut Self {
        self.central_passthrough = on;
        self
    }

    /// Add a new pane and return its tile id.
    pub fn add_pane(&mut self, pane: Pane) -> TileId {
        self.tiles.insert_pane(pane)
    }

    /// Create a tabs container from existing children.
    pub fn tabs(&mut self, children: Vec<TileId>) -> TileId {
        self.tiles
            .insert_new(Tile::Container(Container::Tabs(Tabs::new(children))))
    }

    /// Create a split container with direction and children (in order).
    fn split_with(&mut self, dir: LinearDir, children: Vec<TileId>) -> TileId {
        self.tiles
            .insert_new(Tile::Container(Container::Linear(Linear::new(
                dir, children,
            ))))
    }

    /// Insert `child` into an existing tabs container at the given index (clamped).
    pub fn dock_into_tabs(&mut self, tabs_id: TileId, child: TileId, index: usize) -> TileId {
        if let Some(Tile::Container(Container::Tabs(tabs))) = self.tiles.get_mut(tabs_id) {
            let idx = index.min(tabs.children.len());
            tabs.children.insert(idx, child);
            tabs.set_active(child);
            tabs_id
        } else {
            // If not tabs, wrap target into tabs with the child
            let tabs_id = self.tabs(vec![tabs_id, child]);
            if let Some(Tile::Container(Container::Tabs(tabs))) = self.tiles.get_mut(tabs_id) {
                tabs.set_active(child);
            }
            tabs_id
        }
    }

    /// Set the active child in a tabs container.
    pub fn set_active_in_tabs(&mut self, tabs_id: TileId, child: TileId) {
        if let Some(Tile::Container(Container::Tabs(tabs))) = self.tiles.get_mut(tabs_id) {
            tabs.set_active(child);
        }
    }

    /// Set the share value for a child in a linear container. Shares are relative.
    pub fn set_share(&mut self, linear_id: TileId, child: TileId, share: f32) {
        if let Some(Tile::Container(Container::Linear(linear))) = self.tiles.get_mut(linear_id) {
            linear.shares.set_share(child, share.max(0.0001));
        }
    }

    /// Reorder a tab within a tabs container.
    pub fn reorder_in_tabs(
        &mut self,
        tabs_id: TileId,
        from_idx: usize,
        to_idx: usize,
    ) -> Option<()> {
        if let Some(Tile::Container(Container::Tabs(tabs))) = self.tiles.get_mut(tabs_id) {
            if from_idx >= tabs.children.len() {
                return None;
            }
            let to_idx = to_idx.min(tabs.children.len().saturating_sub(1));
            if from_idx == to_idx {
                return Some(());
            }
            let child = tabs.children.remove(from_idx);
            tabs.children.insert(to_idx, child);
            if tabs.active == Some(child) {
                tabs.set_active(child);
            }
            return Some(());
        }
        None
    }

    /// Move a child within a linear container by indices.
    pub fn move_child_in_linear(
        &mut self,
        linear_id: TileId,
        from_idx: usize,
        to_idx: usize,
    ) -> Option<()> {
        if let Some(Tile::Container(Container::Linear(linear))) = self.tiles.get_mut(linear_id) {
            if from_idx >= linear.children.len() {
                return None;
            }
            let to_idx = to_idx.min(linear.children.len().saturating_sub(1));
            if from_idx == to_idx {
                return Some(());
            }
            let child = linear.children.remove(from_idx);
            linear.children.insert(to_idx, child);
            return Some(());
        }
        None
    }

    /// Move a specific child to a given index within a linear container.
    pub fn move_specific_child_in_linear(
        &mut self,
        linear_id: TileId,
        child_id: TileId,
        to_idx: usize,
    ) -> Option<()> {
        if let Some(Tile::Container(Container::Linear(linear))) = self.tiles.get_mut(linear_id) {
            if let Some(pos) = linear.children.iter().position(|&c| c == child_id) {
                let to_idx = to_idx.min(linear.children.len().saturating_sub(1));
                if pos == to_idx {
                    return Some(());
                }
                linear.children.remove(pos);
                linear.children.insert(to_idx, child_id);
                return Some(());
            }
            return None;
        }
        None
    }

    /// Split `target` horizontally, inserting `new_child` to the left of it.
    pub fn split_left(&mut self, target: TileId, new_child: TileId) -> TileId {
        self.split_horizontal_impl(target, new_child, true)
    }

    /// Split `target` horizontally, inserting `new_child` to the right of it.
    pub fn split_right(&mut self, target: TileId, new_child: TileId) -> TileId {
        self.split_horizontal_impl(target, new_child, false)
    }

    /// Split `target` vertically, inserting `new_child` above it.
    pub fn split_top(&mut self, target: TileId, new_child: TileId) -> TileId {
        self.split_vertical_impl(target, new_child, true)
    }

    /// Split `target` vertically, inserting `new_child` below it.
    pub fn split_bottom(&mut self, target: TileId, new_child: TileId) -> TileId {
        self.split_vertical_impl(target, new_child, false)
    }

    fn replace_in_parent(&mut self, target: TileId, replacement: TileId) {
        if let Some(parent) = self.tiles.parent_of(target) {
            if let Some(Tile::Container(container)) = self.tiles.get_mut(parent) {
                if let Some(idx) = container.remove_child(target) {
                    // Put back replacement where target was
                    match container {
                        Container::Tabs(tabs) => tabs.children.insert(idx, replacement),
                        Container::Linear(linear) => linear.children.insert(idx, replacement),
                        Container::Grid(grid) => grid.insert_at(idx, replacement),
                    }
                }
            }
        } else {
            // Target was root
            self.root = Some(replacement);
        }
    }

    /// Dock `new_child` into `target` on `side`. Returns the container id used.
    pub fn dock_into(
        &mut self,
        target: TileId,
        new_child: TileId,
        side: crate::DockSide,
    ) -> TileId {
        self.dock_with_ratio(target, new_child, side, 0.5)
    }

    /// Dock with a desired ratio (applies when introducing a new split or splitting an existing share).
    pub fn dock_with_ratio(
        &mut self,
        target: TileId,
        new_child: TileId,
        side: crate::DockSide,
        ratio: f32,
    ) -> TileId {
        let ratio = ratio.clamp(0.05, 0.95);

        match side {
            crate::DockSide::Center => {
                // Insert into tabs (wrap if needed)
                if let Some(Tile::Container(Container::Tabs(tabs))) = self.tiles.get_mut(target) {
                    tabs.add_child(new_child);
                    tabs.set_active(new_child);
                    target
                } else {
                    let tabs_id = self.tabs(vec![target, new_child]);
                    if let Some(Tile::Container(Container::Tabs(tabs))) =
                        self.tiles.get_mut(tabs_id)
                    {
                        tabs.set_active(new_child);
                    }
                    self.replace_in_parent(target, tabs_id);
                    tabs_id
                }
            }
            crate::DockSide::Left
            | crate::DockSide::Right
            | crate::DockSide::Top
            | crate::DockSide::Bottom => {
                let want_dir = match side {
                    crate::DockSide::Left | crate::DockSide::Right => LinearDir::Horizontal,
                    _ => LinearDir::Vertical,
                };

                if let Some(parent_id) = self.tiles.parent_of(target) {
                    if let Some(Tile::Container(Container::Linear(parent))) =
                        self.tiles.get_mut(parent_id)
                    {
                        if parent.dir == want_dir {
                            // insert into existing split
                            let idx = parent
                                .children
                                .iter()
                                .position(|&c| c == target)
                                .unwrap_or(0);
                            let insert_at = match side {
                                crate::DockSide::Left | crate::DockSide::Top => idx,
                                _ => idx + 1,
                            };
                            parent.children.insert(insert_at, new_child);

                            // reassign shares: split target's share
                            let old = parent.shares[target];
                            parent.shares[target] = old * (1.0 - ratio);
                            parent.shares.set_share(new_child, old * ratio);
                            return parent_id;
                        }
                    }
                }

                // create a new split around target
                let children = match side {
                    crate::DockSide::Left | crate::DockSide::Top => vec![new_child, target],
                    _ => vec![target, new_child],
                };
                let split_id = self.split_with(want_dir, children);
                if let Some(Tile::Container(Container::Linear(split))) =
                    self.tiles.get_mut(split_id)
                {
                    // assign shares based on ratio
                    let a = if matches!(side, crate::DockSide::Left | crate::DockSide::Top) {
                        new_child
                    } else {
                        target
                    };
                    let b = if a == new_child { target } else { new_child };
                    split.shares.set_share(a, ratio);
                    split.shares.set_share(b, 1.0 - ratio);
                }
                self.replace_in_parent(target, split_id);
                split_id
            }
        }
    }

    /// Ensure `target` is a tabs container. If it's not, wrap it into a new tabs container and replace in parent.
    pub fn ensure_tabs(&mut self, target: TileId) -> TileId {
        if let Some(Tile::Container(Container::Tabs(_))) = self.tiles.get(target) {
            return target;
        }
        let tabs_id = self.tabs(vec![target]);
        self.replace_in_parent(target, tabs_id);
        tabs_id
    }

    /// Set the active index of a tabs container (clamped). Returns the set tile id if any.
    pub fn set_active_tab_index(&mut self, tabs_id: TileId, index: usize) -> Option<TileId> {
        if let Some(Tile::Container(Container::Tabs(tabs))) = self.tiles.get_mut(tabs_id) {
            let idx = index.min(tabs.children.len().saturating_sub(1));
            let child = tabs.children.get(idx).copied();
            if let Some(child) = child {
                tabs.set_active(child);
            }
            return child;
        }
        None
    }

    /// Equalize all child shares to 1.0 in a linear container.
    pub fn equalize_shares(&mut self, linear_id: TileId) -> Option<()> {
        if let Some(Tile::Container(Container::Linear(linear))) = self.tiles.get_mut(linear_id) {
            for &child in &linear.children {
                linear.shares.set_share(child, 1.0);
            }
            return Some(());
        }
        None
    }

    /// Normalize shares in a linear container so they sum to the number of children.
    pub fn normalize_shares(&mut self, linear_id: TileId) -> Option<()> {
        if let Some(Tile::Container(Container::Linear(linear))) = self.tiles.get_mut(linear_id) {
            let mut sum = 0.0;
            for &child in &linear.children {
                sum += linear.shares[child];
            }
            if sum <= f32::EPSILON {
                return self.equalize_shares(linear_id);
            }
            let target_sum = linear.children.len() as f32;
            let k = target_sum / sum;
            for &child in &linear.children {
                let v = linear.shares[child] * k;
                linear.shares.set_share(child, v);
            }
            return Some(());
        }
        None
    }

    /// Bulk set shares for specific children in a linear container (values are clamped to >0).
    pub fn set_linear_children_shares(
        &mut self,
        linear_id: TileId,
        shares: &[(TileId, f32)],
    ) -> Option<()> {
        if let Some(Tile::Container(Container::Linear(linear))) = self.tiles.get_mut(linear_id) {
            for &(child, share) in shares {
                if linear.children.contains(&child) {
                    linear.shares.set_share(child, share.max(0.0001));
                }
            }
            return Some(());
        }
        None
    }

    fn split_horizontal_impl(
        &mut self,
        target: TileId,
        new_child: TileId,
        insert_left: bool,
    ) -> TileId {
        let children = if insert_left {
            vec![new_child, target]
        } else {
            vec![target, new_child]
        };
        let split_id = self.split_with(LinearDir::Horizontal, children);
        self.replace_in_parent(target, split_id);
        split_id
    }

    fn split_vertical_impl(
        &mut self,
        target: TileId,
        new_child: TileId,
        insert_top: bool,
    ) -> TileId {
        let children = if insert_top {
            vec![new_child, target]
        } else {
            vec![target, new_child]
        };
        let split_id = self.split_with(LinearDir::Vertical, children);
        self.replace_in_parent(target, split_id);
        split_id
    }

    /// Finish building and return a Tree.
    pub fn build(self) -> Tree<Pane> {
        let root = self.root.expect("DockBuilder requires a root to be set");
        let mut tree = Tree::new(self.id, root, self.tiles);
        tree.central = self.central;
        tree.central_passthrough = self.central_passthrough;
        tree
    }

    /// Build a tree from a blueprint node in one call.
    pub fn from_blueprint(
        id: impl Into<egui::Id>,
        blueprint: crate::DockBlueprintNode,
        mut make_pane: impl FnMut(u64) -> Pane,
    ) -> Tree<Pane> {
        let mut builder = DockBuilder::new(id);
        let root = blueprint.build(&mut builder, &mut make_pane);
        builder.set_root(root);
        builder.build()
    }

    /// Build a tree from a full blueprint (with central & flags) in one call.
    pub fn from_blueprint_full(
        id: impl Into<egui::Id>,
        blueprint: crate::FullBlueprint,
        mut make_pane: impl FnMut(u64) -> Pane,
    ) -> Tree<Pane>
    where
        Pane: PartialEq,
    {
        let mut builder = DockBuilder::new(id);
        let root = blueprint.root.build(&mut builder, &mut make_pane);
        builder.set_root(root);
        let mut tree = builder.build();

        // Try set central if it maps to a Pane
        if let Some(central_key) = blueprint.central {
            let sample = make_pane(central_key);
            if let Some(tile_id) = tree.tiles.find_pane(&sample) {
                tree.central = Some(tile_id);
            }
        }
        tree.central_passthrough = blueprint.central_passthrough;
        tree.central_no_docking = blueprint.central_no_docking;
        tree
    }
}
