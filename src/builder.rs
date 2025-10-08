use crate::{Container, Linear, LinearDir, Tabs, Tile, TileId, Tiles, Tree};

/// Programmatic builder for docking layouts.
///
/// Build a tree of panes and containers, then call `build()` to get a `Tree`.
pub struct DockBuilder<Pane> {
    id: egui::Id,
    tiles: Tiles<Pane>,
    root: Option<TileId>,
}

impl<Pane> DockBuilder<Pane> {
    pub fn new(id: impl Into<egui::Id>) -> Self {
        Self { id: id.into(), tiles: Tiles::default(), root: None }
    }

    pub fn tiles(&self) -> &Tiles<Pane> { &self.tiles }
    pub fn tiles_mut(&mut self) -> &mut Tiles<Pane> { &mut self.tiles }

    pub fn set_root(&mut self, root: TileId) { self.root = Some(root); }

    /// Add a new pane and return its tile id.
    pub fn add_pane(&mut self, pane: Pane) -> TileId { self.tiles.insert_pane(pane) }

    /// Create a tabs container from existing children.
    pub fn tabs(&mut self, children: Vec<TileId>) -> TileId {
        self.tiles.insert_new(Tile::Container(Container::Tabs(Tabs::new(children))))
    }

    /// Create a split container with direction and children (in order).
    fn split_with(&mut self, dir: LinearDir, children: Vec<TileId>) -> TileId {
        self.tiles
            .insert_new(Tile::Container(Container::Linear(Linear::new(dir, children))))
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

    fn split_horizontal_impl(
        &mut self,
        target: TileId,
        new_child: TileId,
        insert_left: bool,
    ) -> TileId {
        let children = if insert_left { vec![new_child, target] } else { vec![target, new_child] };
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
        let children = if insert_top { vec![new_child, target] } else { vec![target, new_child] };
        let split_id = self.split_with(LinearDir::Vertical, children);
        self.replace_in_parent(target, split_id);
        split_id
    }

    /// Finish building and return a Tree.
    pub fn build(self) -> Tree<Pane> {
        let root = self.root.expect("DockBuilder requires a root to be set");
        Tree::new(self.id, root, self.tiles)
    }
}
