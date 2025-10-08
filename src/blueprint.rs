use crate::{Container, ContainerFlags, Grid, GridLayout, Linear, LinearDir, Tabs, Tile, TileId};

#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
#[derive(Clone, Debug)]
pub enum Node {
    /// A pane by an application-defined id.
    Pane(u64),

    /// A tab container with optional active index.
    Tabs {
        children: Vec<Node>,
        active: Option<usize>,
        flags: Option<ContainerFlags>,
    },

    /// A linear split with optional shares per child.
    Split {
        dir: LinearDir,
        children: Vec<Node>,
        shares: Option<Vec<f32>>,
        flags: Option<ContainerFlags>,
    },

    /// A grid with optional layout.
    Grid {
        children: Vec<Node>,
        layout: Option<GridLayout>,
        flags: Option<ContainerFlags>,
    },
}

impl Node {
    /// Recursively build tiles and return the root TileId.
    pub fn build<Pane, F: FnMut(u64) -> Pane>(
        &self,
        builder: &mut crate::DockBuilder<Pane>,
        make_pane: &mut F,
    ) -> TileId {
        match self {
            Node::Pane(id) => builder.add_pane(make_pane(*id)),

            Node::Tabs {
                children,
                active,
                flags,
            } => {
                let child_ids: Vec<_> = children
                    .iter()
                    .map(|c| c.build(builder, make_pane))
                    .collect();
                let tabs_id = builder
                    .tiles_mut()
                    .insert_new(Tile::Container(Container::Tabs(Tabs::new(child_ids))));
                if let Some(active) = *active {
                    let _ = builder.set_active_tab_index(tabs_id, active);
                }
                if let Some(flags) = flags {
                    if let Some(Tile::Container(Container::Tabs(t))) =
                        builder.tiles_mut().get_mut(tabs_id)
                    {
                        t.flags = *flags;
                    }
                }
                tabs_id
            }

            Node::Split {
                dir,
                children,
                shares,
                flags,
            } => {
                let child_ids: Vec<_> = children
                    .iter()
                    .map(|c| c.build(builder, make_pane))
                    .collect();
                let split_id = builder
                    .tiles_mut()
                    .insert_new(Tile::Container(Container::Linear(Linear::new(
                        *dir,
                        child_ids.clone(),
                    ))));
                if let Some(shares) = shares {
                    for (i, &child) in child_ids.iter().enumerate() {
                        if let Some(share) = shares.get(i) {
                            builder.set_share(split_id, child, *share);
                        }
                    }
                } else {
                    let _ = builder.equalize_shares(split_id);
                }
                if let Some(flags) = flags {
                    if let Some(Tile::Container(Container::Linear(l))) =
                        builder.tiles_mut().get_mut(split_id)
                    {
                        l.flags = *flags;
                    }
                }
                split_id
            }

            Node::Grid {
                children,
                layout,
                flags,
            } => {
                let child_ids: Vec<_> = children
                    .iter()
                    .map(|c| c.build(builder, make_pane))
                    .collect();
                let mut grid = Grid::new(child_ids);
                if let Some(layout) = layout {
                    grid.layout = *layout;
                }
                let id = builder
                    .tiles_mut()
                    .insert_new(Tile::Container(Container::Grid(grid)));
                if let Some(flags) = flags {
                    if let Some(Tile::Container(Container::Grid(g))) =
                        builder.tiles_mut().get_mut(id)
                    {
                        g.flags = *flags;
                    }
                }
                id
            }
        }
    }
}

/// Export a subtree rooted at `root` into a DockBlueprint `Node`.
pub fn to_blueprint<Pane, F: FnMut(&Pane) -> u64>(
    tiles: &crate::Tiles<Pane>,
    root: TileId,
    pane_id_of: &mut F,
) -> Node {
    fn export_tabs<Pane, F: FnMut(&Pane) -> u64>(
        tiles: &crate::Tiles<Pane>,
        tabs: &Tabs,
        pane_id_of: &mut F,
    ) -> Node {
        // Keep original child order; drop invisible children
        let mut pairs: Vec<(TileId, Node)> = vec![];
        for &child in &tabs.children {
            if !tiles.is_visible(child) {
                continue;
            }
            let node = to_blueprint(tiles, child, pane_id_of);
            pairs.push((child, node));
        }
        let active = tabs
            .active
            .and_then(|a| pairs.iter().position(|(id, _)| *id == a));
        let children = pairs.into_iter().map(|(_, n)| n).collect();
        Node::Tabs {
            children,
            active,
            flags: Some(tabs.flags),
        }
    }

    fn export_linear<Pane, F: FnMut(&Pane) -> u64>(
        tiles: &crate::Tiles<Pane>,
        lin: &Linear,
        pane_id_of: &mut F,
    ) -> Node {
        let mut children_ids: Vec<TileId> = vec![];
        for &child in &lin.children {
            if tiles.is_visible(child) {
                children_ids.push(child);
            }
        }
        let mut children_nodes = Vec::with_capacity(children_ids.len());
        let mut shares = Vec::with_capacity(children_ids.len());
        for &child in &children_ids {
            children_nodes.push(to_blueprint(tiles, child, pane_id_of));
            shares.push(lin.shares[child]);
        }
        Node::Split {
            dir: lin.dir,
            children: children_nodes,
            shares: Some(shares),
            flags: Some(lin.flags),
        }
    }

    fn export_grid<Pane, F: FnMut(&Pane) -> u64>(
        tiles: &crate::Tiles<Pane>,
        grid: &Grid,
        pane_id_of: &mut F,
    ) -> Node {
        let mut children_nodes = Vec::new();
        for child in grid.children() {
            if tiles.is_visible(*child) {
                children_nodes.push(to_blueprint(tiles, *child, pane_id_of));
            }
        }
        Node::Grid {
            children: children_nodes,
            layout: Some(grid.layout),
            flags: Some(grid.flags),
        }
    }

    match tiles.get(root) {
        Some(Tile::Pane(p)) => Node::Pane(pane_id_of(p)),
        Some(Tile::Container(Container::Tabs(t))) => export_tabs(tiles, t, pane_id_of),
        Some(Tile::Container(Container::Linear(l))) => export_linear(tiles, l, pane_id_of),
        Some(Tile::Container(Container::Grid(g))) => export_grid(tiles, g, pane_id_of),
        None => Node::Tabs {
            children: vec![],
            active: None,
            flags: Some(ContainerFlags::default()),
        },
    }
}

/// Convenience: export the entire tree.
pub fn export_tree<Pane, F: FnMut(&Pane) -> u64>(
    tree: &crate::Tree<Pane>,
    mut pane_id_of: F,
) -> Node {
    if let Some(root) = tree.root() {
        to_blueprint(&tree.tiles, root, &mut pane_id_of)
    } else {
        Node::Tabs {
            children: vec![],
            active: None,
            flags: Some(ContainerFlags::default()),
        }
    }
}

#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
#[derive(Clone, Debug)]
pub struct FullBlueprint {
    #[cfg_attr(feature = "serde", serde(default))]
    pub version: u32,
    #[cfg_attr(feature = "serde", serde(default))]
    pub schema: Option<String>,
    #[cfg_attr(feature = "serde", serde(default))]
    pub description: Option<String>,
    pub root: Node,
    #[cfg_attr(feature = "serde", serde(default))]
    pub central: Option<u64>,
    #[cfg_attr(feature = "serde", serde(default))]
    pub central_passthrough: bool,
    #[cfg_attr(feature = "serde", serde(default))]
    pub central_no_docking: bool,
}

pub fn export_tree_full<Pane, F: FnMut(&Pane) -> u64>(
    tree: &crate::Tree<Pane>,
    mut pane_id_of: F,
) -> FullBlueprint {
    let root = export_tree(tree, |p| pane_id_of(p));
    let mut central = None;
    if let Some(c_id) = tree.central {
        if let Some(p) = tree.tiles.get_pane(&c_id) {
            central = Some(pane_id_of(p));
        }
    }
    FullBlueprint {
        version: 1,
        schema: Some("egui_docking.full_blueprint".to_string()),
        description: None,
        root,
        central,
        central_passthrough: tree.central_passthrough,
        central_no_docking: tree.central_no_docking,
    }
}
