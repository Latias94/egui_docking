use egui::Id;
use egui_tiles::{Container, Linear, LinearDir, TileId, Tiles, Tree};
use std::collections::BTreeMap;

/// Split direction with Dear ImGui `DockBuilder::SplitNode`-like semantics.
///
/// The direction indicates where the *side* node is placed relative to the *main* node.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SplitDirection {
    Left,
    Right,
    Up,
    Down,
}

/// A logical node id used by [`DockBuilder`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DockNodeId(u64);

/// A small convenience builder for constructing an `egui_tiles::Tree` from code.
///
/// This is intentionally lightweight: it doesn't attempt to replicate Dear ImGui's internal dock
/// node graph. It only provides an ergonomic way to express "Unity-like" split/tab layouts.
///
/// For full control, you can always use `egui_tiles::Tiles` + `egui_tiles::Tree::new` directly.
pub struct DockTreeBuilder<Pane> {
    id: Id,
    tiles: Tiles<Pane>,
}

impl<Pane> DockTreeBuilder<Pane> {
    /// Create a new builder with a globally unique tree id.
    pub fn new(id: impl Into<Id>) -> Self {
        Self {
            id: id.into(),
            tiles: Tiles::default(),
        }
    }

    /// Access the underlying `Tiles` for advanced customization.
    pub fn tiles_mut(&mut self) -> &mut Tiles<Pane> {
        &mut self.tiles
    }

    /// Insert a leaf pane.
    #[must_use]
    pub fn pane(&mut self, pane: Pane) -> TileId {
        self.tiles.insert_pane(pane)
    }

    /// Create a `Tabs` container.
    #[must_use]
    pub fn tabs(&mut self, children: Vec<TileId>) -> TileId {
        self.tiles.insert_tab_tile(children)
    }

    /// Create a binary split with Dear ImGui-like semantics.
    ///
    /// - `main`: the remainder node (the "existing dock")
    /// - `dir`: where to place `side` relative to `main`
    /// - `side_fraction`: fraction of the parent size given to `side` (0.0..=1.0)
    /// - `side`: the new side node
    ///
    /// Returns a new container tile that becomes the parent of both children.
    #[must_use]
    pub fn split(
        &mut self,
        main: TileId,
        dir: SplitDirection,
        side_fraction: f32,
        side: TileId,
    ) -> TileId {
        debug_assert!(
            (0.0..=1.0).contains(&side_fraction),
            "side_fraction must be in 0.0..=1.0"
        );

        let (linear_dir, first, second, first_fraction) = match dir {
            SplitDirection::Left => (LinearDir::Horizontal, side, main, side_fraction),
            SplitDirection::Right => (LinearDir::Horizontal, main, side, 1.0 - side_fraction),
            SplitDirection::Up => (LinearDir::Vertical, side, main, side_fraction),
            SplitDirection::Down => (LinearDir::Vertical, main, side, 1.0 - side_fraction),
        };

        let container = Container::from(Linear::new_binary(linear_dir, [first, second], first_fraction));
        self.tiles.insert_container(container)
    }

    /// Finish building, producing the `Tree`.
    pub fn build(self, root: TileId) -> Tree<Pane> {
        Tree::new(self.id, root, self.tiles)
    }
}

// ----------------------------------------------------------------------------
// ImGui-like API (node graph -> tiles)

#[derive(Clone, Debug)]
enum Node<Pane> {
    Tabs {
        panes: Vec<Pane>,
    },
    Split {
        dir: SplitDirection,
        side_fraction: f32,
        main: DockNodeId,
        side: DockNodeId,
    },
}

/// A higher-level builder that feels closer to Dear ImGui's `DockBuilder`:
/// you create empty nodes, split them, then dock panes into leaf nodes, and finally `finish()`.
///
/// Differences vs Dear ImGui:
/// - We build an `egui_tiles::Tree<Pane>` (tiles graph), not a dock-node graph with window names.
/// - Leaves are modeled as `Tabs` containers, so "dock window" is "push pane into that Tabs node".
/// - Nodes are allowed to be empty (a Tabs container with 0 children). This is useful for "reserved"
///   areas in scripted layouts.
pub struct DockBuilder<Pane> {
    id: Id,
    next_node_id: u64,
    nodes: BTreeMap<DockNodeId, Node<Pane>>,
}

impl<Pane> DockBuilder<Pane> {
    pub fn new(id: impl Into<Id>) -> Self {
        Self {
            id: id.into(),
            next_node_id: 1,
            nodes: BTreeMap::new(),
        }
    }

    fn alloc_node_id(&mut self) -> DockNodeId {
        let id = DockNodeId(self.next_node_id);
        self.next_node_id = self.next_node_id.saturating_add(1);
        id
    }

    /// Create an empty leaf node (a `Tabs` container).
    #[must_use]
    pub fn add_node(&mut self) -> DockNodeId {
        let id = self.alloc_node_id();
        self.nodes.insert(id, Node::Tabs { panes: Vec::new() });
        id
    }

    /// Split an existing node and return `(side, main)` (Dear ImGui semantics).
    ///
    /// The `node` itself becomes the split container, and the original content is moved into the
    /// returned `main` child node.
    #[must_use]
    pub fn split_node(
        &mut self,
        node: DockNodeId,
        dir: SplitDirection,
        side_fraction: f32,
    ) -> (DockNodeId, DockNodeId) {
        debug_assert!(
            (0.0..=1.0).contains(&side_fraction),
            "side_fraction must be in 0.0..=1.0"
        );

        let old = self
            .nodes
            .remove(&node)
            .unwrap_or(Node::Tabs { panes: Vec::new() });
        let main = self.alloc_node_id();
        self.nodes.insert(main, old);

        let side = self.add_node();

        self.nodes.insert(
            node,
            Node::Split {
                dir,
                side_fraction,
                main,
                side,
            },
        );

        (side, main)
    }

    /// Dock a pane into a leaf `Tabs` node.
    pub fn dock_pane(&mut self, pane: Pane, node: DockNodeId) {
        match self.nodes.get_mut(&node) {
            Some(Node::Tabs { panes }) => panes.push(pane),
            Some(Node::Split { .. }) => {
                panic!("dock_pane: node {node:?} is not a leaf Tabs node");
            }
            None => {
                panic!("dock_pane: node {node:?} does not exist");
            }
        }
    }

    /// Dock multiple panes into a leaf `Tabs` node (tabbed together).
    pub fn dock_panes(&mut self, panes: impl IntoIterator<Item = Pane>, node: DockNodeId) {
        for pane in panes {
            self.dock_pane(pane, node);
        }
    }

    /// Alias for [`Self::dock_pane`], named after Dear ImGui's `DockBuilder::DockWindow`.
    pub fn dock_window(&mut self, pane: Pane, node: DockNodeId) {
        self.dock_pane(pane, node);
    }

    /// Alias for [`Self::dock_panes`], named after Dear ImGui's `DockBuilder::DockWindow`.
    pub fn dock_windows(&mut self, panes: impl IntoIterator<Item = Pane>, node: DockNodeId) {
        self.dock_panes(panes, node);
    }

    /// Finish building and produce the `egui_tiles::Tree`.
    ///
    /// `root` is typically the `DockNodeId` returned by the first `add_node()` and then mutated by splits.
    pub fn finish(self, root: DockNodeId) -> Tree<Pane> {
        self.finish_map(root, |pane| Some(pane))
    }

    /// Finish building and produce the `egui_tiles::Tree`, mapping docked items along the way.
    ///
    /// This is useful when your scripted layout is expressed in terms of stable ids (e.g. `PaneId`
    /// or tool names), and you want to:
    /// - lazily skip closed panes, or
    /// - map ids into real pane state structs.
    ///
    /// Returning `None` drops that item from the output tree.
    pub fn finish_map<OutPane>(
        self,
        root: DockNodeId,
        mut map: impl FnMut(Pane) -> Option<OutPane>,
    ) -> Tree<OutPane> {
        fn build_tile<Pane, OutPane>(
            node_id: DockNodeId,
            nodes: &mut BTreeMap<DockNodeId, Node<Pane>>,
            tiles: &mut Tiles<OutPane>,
            map: &mut impl FnMut(Pane) -> Option<OutPane>,
        ) -> TileId {
            match nodes.remove(&node_id) {
                Some(Node::Tabs { panes }) => {
                    let children: Vec<TileId> = panes
                        .into_iter()
                        .filter_map(|p| map(p))
                        .map(|p| tiles.insert_pane(p))
                        .collect();
                    tiles.insert_tab_tile(children)
                }
                Some(Node::Split {
                    dir,
                    side_fraction,
                    main,
                    side,
                }) => {
                    let main_tile = build_tile(main, nodes, tiles, map);
                    let side_tile = build_tile(side, nodes, tiles, map);

                    let (linear_dir, first, second, first_fraction) = match dir {
                        SplitDirection::Left => (LinearDir::Horizontal, side_tile, main_tile, side_fraction),
                        SplitDirection::Right => {
                            (LinearDir::Horizontal, main_tile, side_tile, 1.0 - side_fraction)
                        }
                        SplitDirection::Up => (LinearDir::Vertical, side_tile, main_tile, side_fraction),
                        SplitDirection::Down => {
                            (LinearDir::Vertical, main_tile, side_tile, 1.0 - side_fraction)
                        }
                    };

                    let container =
                        Container::from(Linear::new_binary(linear_dir, [first, second], first_fraction));
                    tiles.insert_container(container)
                }
                None => tiles.insert_tab_tile(Vec::new()),
            }
        }

        let mut nodes = self.nodes;
        let mut tiles: Tiles<OutPane> = Tiles::default();
        let root_tile = build_tile(root, &mut nodes, &mut tiles, &mut map);
        Tree::new(self.id, root_tile, tiles)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui_tiles::{ContainerKind, Tile};

    #[test]
    fn split_node_semantics_match_imgui() {
        let mut b = DockBuilder::new("dock_builder_test");
        let dockspace = b.add_node();

        let (right, main) = b.split_node(dockspace, SplitDirection::Right, 0.25);
        b.dock_window(1u8, main);
        b.dock_window(2u8, right);

        let tree = b.finish(dockspace);
        let root = tree.root.unwrap();

        let Tile::Container(root_container) = tree.tiles.get(root).unwrap() else {
            panic!("root should be a container");
        };
        assert_eq!(root_container.kind(), ContainerKind::Horizontal);

        let egui_tiles::Container::Linear(linear) = root_container else {
            panic!("root should be a Linear container");
        };
        assert_eq!(linear.children.len(), 2);

        // Right split: main is first (left), side is second (right).
        let left_child = linear.children[0];
        let right_child = linear.children[1];

        let left_tabs = match tree.tiles.get(left_child).unwrap() {
            Tile::Container(egui_tiles::Container::Tabs(tabs)) => tabs,
            other => panic!("left child should be Tabs container, got: {other:?}"),
        };
        let right_tabs = match tree.tiles.get(right_child).unwrap() {
            Tile::Container(egui_tiles::Container::Tabs(tabs)) => tabs,
            other => panic!("right child should be Tabs container, got: {other:?}"),
        };

        assert_eq!(left_tabs.children.len(), 1);
        assert_eq!(right_tabs.children.len(), 1);
    }

    #[test]
    #[should_panic]
    fn dock_pane_into_non_leaf_panics() {
        let mut b = DockBuilder::new("dock_builder_panic_test");
        let dockspace = b.add_node();
        let (_side, _main) = b.split_node(dockspace, SplitDirection::Left, 0.5);
        b.dock_window(1u8, dockspace);
    }
}
