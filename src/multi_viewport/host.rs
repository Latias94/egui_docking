use egui::ViewportId;

use egui_tiles::{Behavior, TileId, Tree};

use super::DockingMultiViewport;
use super::title::title_for_detached_tree;
use super::types::FloatingId;

fn collect_leaf_panes_and_containers_in_tree<Pane>(
    tree: &Tree<Pane>,
    root: TileId,
) -> (Vec<TileId>, Vec<TileId>) {
    let mut panes: Vec<TileId> = Vec::new();
    let mut containers: Vec<TileId> = Vec::new();

    let mut stack: Vec<TileId> = vec![root];
    while let Some(tile_id) = stack.pop() {
        let Some(tile) = tree.tiles.get(tile_id) else {
            continue;
        };
        match tile {
            egui_tiles::Tile::Pane(_) => panes.push(tile_id),
            egui_tiles::Tile::Container(container) => {
                containers.push(tile_id);
                let children: Vec<TileId> = container.children().copied().collect();
                for child in children.into_iter().rev() {
                    stack.push(child);
                }
            }
        }
    }

    (panes, containers)
}

fn insert_subtree_as_tabs_flattened_if_needed<Pane>(
    tree: &mut Tree<Pane>,
    subtree: egui_tiles::SubTree<Pane>,
    insertion: egui_tiles::InsertionPoint,
    allow_container_tabbing: bool,
) -> Result<(), egui_tiles::SubTree<Pane>> {
    let inserted_root = subtree.root;
    tree.insert_subtree_at(subtree, Some(insertion));

    if allow_container_tabbing {
        return Ok(());
    }

    if insertion.insertion.kind() != egui_tiles::ContainerKind::Tabs {
        return Ok(());
    }

    if matches!(tree.tiles.get(inserted_root), Some(egui_tiles::Tile::Pane(_))) {
        return Ok(());
    }

    let (panes, containers) = collect_leaf_panes_and_containers_in_tree(tree, inserted_root);
    if panes.is_empty() {
        return Ok(());
    }

    let Some(egui_tiles::Tile::Container(egui_tiles::Container::Tabs(parent_tabs))) =
        tree.tiles.get_mut(insertion.parent_id)
    else {
        return Ok(());
    };

    if let Some(pos) = parent_tabs.children.iter().position(|&id| id == inserted_root) {
        parent_tabs.children.remove(pos);
    }

    let mut index = match insertion.insertion {
        egui_tiles::ContainerInsertion::Tabs(i) if i != usize::MAX => i,
        _ => parent_tabs.children.len(),
    };
    index = index.min(parent_tabs.children.len());

    for (offset, pane_id) in panes.iter().copied().enumerate() {
        let at = (index + offset).min(parent_tabs.children.len());
        parent_tabs.children.insert(at, pane_id);
    }
    if let Some(&last) = panes.last() {
        parent_tabs.set_active(last);
    }

    let _ = parent_tabs;
    for container_id in containers {
        let _ = tree.tiles.remove(container_id);
    }

    Ok(())
}

/// A “window host” is where a dock tree (or a subtree) lives.
///
/// This is the core abstraction we want to converge on (ImGui mental model):
/// - docked tree inside a viewport
/// - contained floating window inside a viewport
/// - native viewport window (OS window) that owns a detached tree
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum WindowHost {
    DockTree { viewport: ViewportId },
    Floating { viewport: ViewportId, floating: FloatingId },
    NativeViewport { viewport: ViewportId },
}

impl WindowHost {
    pub(super) fn viewport(self) -> ViewportId {
        match self {
            Self::DockTree { viewport } => viewport,
            Self::Floating { viewport, .. } => viewport,
            Self::NativeViewport { viewport } => viewport,
        }
    }
}

impl<Pane> DockingMultiViewport<Pane> {
    pub(super) fn tree_for_host(&self, host: WindowHost) -> Option<&Tree<Pane>> {
        match host {
            WindowHost::DockTree { viewport } => {
                if viewport == ViewportId::ROOT {
                    Some(&self.tree)
                } else {
                    self.detached.get(&viewport).map(|d| &d.tree)
                }
            }
            WindowHost::Floating { viewport, floating } => self
                .floating
                .get(&viewport)?
                .windows
                .get(&floating)
                .map(|w| &w.tree),
            WindowHost::NativeViewport { viewport } => self.detached.get(&viewport).map(|d| &d.tree),
        }
    }

    pub(super) fn take_subtree_from_host_for_drop(
        &mut self,
        ctx: &egui::Context,
        behavior: &mut dyn Behavior<Pane>,
        host: WindowHost,
        tile_id: TileId,
    ) -> Option<egui_tiles::SubTree<Pane>> {
        match host {
            WindowHost::DockTree { viewport } => {
                if viewport == ViewportId::ROOT {
                    self.tree.extract_subtree(tile_id)
                } else {
                    let Some(mut source) = self.detached.remove(&viewport) else {
                        return None;
                    };
                    let extracted = source.tree.extract_subtree(tile_id);
                    if extracted.is_some() {
                        if source.tree.root.is_some() {
                            source.builder = source
                                .builder
                                .clone()
                                .with_title(title_for_detached_tree(&source.tree, behavior));
                            self.detached.insert(viewport, source);
                        } else {
                            if self.options.debug_event_log {
                                self.debug_log_event(format!(
                                    "take_subtree_from_detached CLOSE viewport={viewport:?} (tree became empty)"
                                ));
                            }
                            ctx.send_viewport_cmd_to(viewport, egui::ViewportCommand::Close);
                        }
                    } else {
                        self.detached.insert(viewport, source);
                    }
                    extracted
                }
            }

            WindowHost::Floating { viewport, floating } => {
                self.extract_subtree_from_floating(viewport, floating, tile_id)
            }

            WindowHost::NativeViewport { .. } => {
                // A native viewport host represents “whole tree move” by title-bar; it does not
                // support subtree extraction via tile_id.
                None
            }
        }
    }

    pub(super) fn take_whole_tree_from_host_for_drop(
        &mut self,
        ctx: &egui::Context,
        _behavior: &mut dyn Behavior<Pane>,
        host: WindowHost,
    ) -> Option<egui_tiles::SubTree<Pane>> {
        match host {
            WindowHost::Floating { viewport, floating } => self.take_whole_floating_tree(viewport, floating),

            WindowHost::NativeViewport { viewport } => {
                if viewport == ViewportId::ROOT {
                    return None;
                }

                let Some(mut source) = self.detached.remove(&viewport) else {
                    return None;
                };
                let Some(root) = source.tree.root.take() else {
                    return None;
                };
                let tiles = std::mem::take(&mut source.tree.tiles);
                if self.options.debug_event_log {
                    self.debug_log_event(format!(
                        "take_whole_detached_tree CLOSE viewport={viewport:?} (moved whole host)"
                    ));
                }
                ctx.send_viewport_cmd_to(viewport, egui::ViewportCommand::Close);
                Some(egui_tiles::SubTree { root, tiles })
            }

            WindowHost::DockTree { .. } => {
                // Not supported: “whole tree move” is only meaningful for native viewport title-bar
                // moves or contained floating window moves.
                None
            }
        }
    }

    pub(super) fn insert_subtree_into_host(
        &mut self,
        host: WindowHost,
        subtree: egui_tiles::SubTree<Pane>,
        insertion: Option<egui_tiles::InsertionPoint>,
    ) -> Result<(), egui_tiles::SubTree<Pane>> {
        match host {
            WindowHost::DockTree { viewport } => {
                if viewport == ViewportId::ROOT {
                    if let Some(ins) = insertion
                        && ins.insertion.kind() == egui_tiles::ContainerKind::Tabs
                    {
                        return insert_subtree_as_tabs_flattened_if_needed(
                            &mut self.tree,
                            subtree,
                            ins,
                            self.options.allow_container_tabbing,
                        );
                    }
                    self.dock_subtree_into_root(subtree, insertion);
                    Ok(())
                } else if let Some(detached) = self.detached.get_mut(&viewport) {
                    if let Some(ins) = insertion
                        && ins.insertion.kind() == egui_tiles::ContainerKind::Tabs
                    {
                        return insert_subtree_as_tabs_flattened_if_needed(
                            &mut detached.tree,
                            subtree,
                            ins,
                            self.options.allow_container_tabbing,
                        );
                    }
                    detached.tree.insert_subtree_at(subtree, insertion);
                    Ok(())
                } else {
                    Err(subtree)
                }
            }

            WindowHost::NativeViewport { viewport } => {
                if viewport == ViewportId::ROOT {
                    return Err(subtree);
                }
                if let Some(detached) = self.detached.get_mut(&viewport) {
                    if let Some(ins) = insertion
                        && ins.insertion.kind() == egui_tiles::ContainerKind::Tabs
                    {
                        return insert_subtree_as_tabs_flattened_if_needed(
                            &mut detached.tree,
                            subtree,
                            ins,
                            self.options.allow_container_tabbing,
                        );
                    }
                    detached.tree.insert_subtree_at(subtree, insertion);
                    Ok(())
                } else {
                    Err(subtree)
                }
            }

            WindowHost::Floating { viewport, floating } => {
                let Some(manager) = self.floating.get_mut(&viewport) else {
                    return Err(subtree);
                };
                let Some(window) = manager.windows.get_mut(&floating) else {
                    return Err(subtree);
                };
                if let Some(ins) = insertion
                    && ins.insertion.kind() == egui_tiles::ContainerKind::Tabs
                {
                    return insert_subtree_as_tabs_flattened_if_needed(
                        &mut window.tree,
                        subtree,
                        ins,
                        self.options.allow_container_tabbing,
                    );
                }
                window.tree.insert_subtree_at(subtree, insertion);
                manager.bring_to_front(floating);
                Ok(())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct DummyBehavior;

    impl egui_tiles::Behavior<()> for DummyBehavior {
        fn pane_ui(
            &mut self,
            _ui: &mut egui::Ui,
            _tile_id: egui_tiles::TileId,
            _pane: &mut (),
        ) -> egui_tiles::UiResponse {
            Default::default()
        }

        fn tab_title_for_pane(&mut self, _pane: &()) -> egui::WidgetText {
            egui::WidgetText::from("pane")
        }
    }

    fn new_tree_tabs(id: egui::Id, panes: usize) -> egui_tiles::Tree<()> {
        let panes = panes.max(1);
        let mut tiles = egui_tiles::Tiles::default();
        let mut ids = Vec::with_capacity(panes);
        for _ in 0..panes {
            ids.push(tiles.insert_pane(()));
        }
        let root = tiles.insert_tab_tile(ids);
        egui_tiles::Tree::new(id, root, tiles)
    }

    #[test]
    fn take_whole_detached_tree_removes_viewport() {
        let root_tree = new_tree_tabs(egui::Id::new("root"), 2);
        let mut docking = DockingMultiViewport::new(root_tree);

        let viewport = ViewportId::from_hash_of("detached");
        let detached_tree = new_tree_tabs(egui::Id::new("detached_tree"), 3);
        docking.detached.insert(
            viewport,
            super::super::types::DetachedDock {
                serial: 1,
                tree: detached_tree,
                builder: egui::ViewportBuilder::default(),
            },
        );

        let ctx = egui::Context::default();
        let mut behavior = DummyBehavior::default();
        let subtree = docking.take_whole_tree_from_host_for_drop(
            &ctx,
            &mut behavior,
            WindowHost::NativeViewport { viewport },
        );
        assert!(subtree.is_some());
        assert!(!docking.detached.contains_key(&viewport));
    }

    #[test]
    fn take_subtree_from_detached_keeps_viewport_if_non_empty() {
        let root_tree = new_tree_tabs(egui::Id::new("root"), 2);
        let mut docking = DockingMultiViewport::new(root_tree);

        let viewport = ViewportId::from_hash_of("detached");
        let detached_tree = new_tree_tabs(egui::Id::new("detached_tree"), 3);
        let pane_id = detached_tree
            .tiles
            .tile_ids()
            .find(|id| detached_tree.tiles.get(*id).is_some_and(|t| t.is_pane()))
            .unwrap();

        docking.detached.insert(
            viewport,
            super::super::types::DetachedDock {
                serial: 1,
                tree: detached_tree,
                builder: egui::ViewportBuilder::default(),
            },
        );

        let ctx = egui::Context::default();
        let mut behavior = DummyBehavior::default();
        let subtree = docking.take_subtree_from_host_for_drop(
            &ctx,
            &mut behavior,
            WindowHost::DockTree { viewport },
            pane_id,
        );
        assert!(subtree.is_some());
        assert!(docking.detached.contains_key(&viewport));
    }
}
