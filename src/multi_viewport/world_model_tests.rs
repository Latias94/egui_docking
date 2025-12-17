use super::drop_sanitize;
use super::host::WindowHost;
use super::integrity;
use super::{DockingMultiViewport, FloatingDockWindow, FloatingManager};

use egui::ViewportId;
use egui_tiles::{ContainerInsertion, InsertionPoint, TileId};

fn assert_all_trees_ok(docking: &DockingMultiViewport<()>) {
    for issue in integrity::tree_integrity_issues(&docking.tree) {
        panic!("root integrity failed: {issue}");
    }

    for (viewport, detached) in &docking.detached {
        for issue in integrity::tree_integrity_issues(&detached.tree) {
            panic!("detached {viewport:?} integrity failed: {issue}");
        }
    }

    for (viewport, manager) in &docking.floating {
        for (floating_id, window) in &manager.windows {
            for issue in integrity::tree_integrity_issues(&window.tree) {
                panic!("floating {viewport:?}/{floating_id:?} integrity failed: {issue}");
            }
        }
    }
}

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

    fn retain_pane(&mut self, _pane: &()) -> bool {
        true
    }
}

#[derive(Clone)]
struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Self {
        Self(seed ^ 0xA11C_E5E7_0000_0001)
    }

    fn next_u64(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005u64)
            .wrapping_add(1442695040888963407u64);
        self.0
    }

    fn next_usize(&mut self, upper: usize) -> usize {
        if upper == 0 {
            return 0;
        }
        (self.next_u64() as usize) % upper
    }

    fn next_bool(&mut self) -> bool {
        (self.next_u64() & 1) != 0
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

fn sorted_tile_ids(tree: &egui_tiles::Tree<()>) -> Vec<TileId> {
    let mut ids: Vec<TileId> = tree.tiles.tile_ids().collect();
    ids.sort_by_key(|id| id.0);
    ids
}

fn random_pane_id(rng: &mut Rng, tree: &egui_tiles::Tree<()>) -> Option<TileId> {
    let candidates: Vec<_> = sorted_tile_ids(tree)
        .into_iter()
        .filter(|id| tree.tiles.get(*id).is_some_and(|t| t.is_pane()))
        .collect();
    if candidates.is_empty() {
        None
    } else {
        Some(candidates[rng.next_usize(candidates.len())])
    }
}

fn random_insertion_point(rng: &mut Rng, tree: &egui_tiles::Tree<()>) -> Option<InsertionPoint> {
    if tree.root.is_none() {
        return None;
    }
    if rng.next_u64() % 4 == 0 {
        return None;
    }

    let ids = sorted_tile_ids(tree);
    if ids.is_empty() {
        return None;
    }
    let parent_id = ids[rng.next_usize(ids.len())];
    let index = if rng.next_bool() { 0 } else { usize::MAX };
    let kind = match rng.next_usize(3) {
        0 => ContainerInsertion::Tabs(index),
        1 => ContainerInsertion::Horizontal(index),
        _ => ContainerInsertion::Vertical(index),
    };
    Some(InsertionPoint::new(parent_id, kind))
}

fn current_hosts(docking: &DockingMultiViewport<()>) -> Vec<WindowHost> {
    let mut hosts = Vec::new();
    hosts.push(WindowHost::DockTree {
        viewport: ViewportId::ROOT,
    });
    for viewport in docking.detached.keys().copied() {
        hosts.push(WindowHost::NativeViewport { viewport });
    }
    for (viewport, manager) in &docking.floating {
        for floating in manager.windows.keys().copied() {
            hosts.push(WindowHost::Floating { viewport: *viewport, floating });
        }
    }
    hosts
}

#[test]
fn world_model_cross_host_moves_keep_integrity() {
    for seed in 1u64..=8 {
        let mut rng = Rng::new(seed);
        let mut docking = DockingMultiViewport::new(new_tree_tabs(egui::Id::new(("root", seed)), 4));

        let detached_viewport = ViewportId::from_hash_of(("detached", seed));
        docking.detached.insert(
            detached_viewport,
            super::types::DetachedDock {
                serial: 1,
                tree: new_tree_tabs(egui::Id::new(("detached_tree", seed)), 3),
                builder: egui::ViewportBuilder::default(),
            },
        );

        let floating_id = 1u64;
        let floating_window = FloatingDockWindow {
            tree: new_tree_tabs(egui::Id::new(("floating_tree", seed)), 2),
            offset_in_dock: egui::Vec2::new(20.0, 20.0),
            size: egui::Vec2::new(320.0, 200.0),
            collapsed: false,
            drag: None,
            resize: None,
        };
        docking.floating.insert(
            ViewportId::ROOT,
            FloatingManager {
                windows: std::collections::BTreeMap::from([(floating_id, floating_window)]),
                z_order: vec![floating_id],
            },
        );

        let ctx = egui::Context::default();
        let mut behavior = DummyBehavior::default();

        for _step in 0..250 {
            let hosts = current_hosts(&docking);
            assert!(!hosts.is_empty());

            let do_whole_tree = rng.next_u64() % 5 == 0;
            let eligible_whole: Vec<_> = hosts
                .iter()
                .copied()
                .filter(|h| matches!(h, WindowHost::NativeViewport { .. } | WindowHost::Floating { .. }))
                .collect();

            if do_whole_tree && !eligible_whole.is_empty() {
                let source_host = eligible_whole[rng.next_usize(eligible_whole.len())];
                let Some(subtree) =
                    docking.take_whole_tree_from_host_for_drop(&ctx, &mut behavior, source_host)
                else {
                    assert_all_trees_ok(&docking);
                    continue;
                };

                let mut targets: Vec<_> = current_hosts(&docking)
                    .into_iter()
                    .filter(|h| *h != source_host)
                    .collect();
                if targets.is_empty() {
                    targets.push(WindowHost::DockTree {
                        viewport: ViewportId::ROOT,
                    });
                }
                let target_host = targets[rng.next_usize(targets.len())];
                let insertion = docking
                    .tree_for_host(target_host)
                    .and_then(|t| random_insertion_point(&mut rng, t));
                let insertion = drop_sanitize::sanitize_insertion_for_subtree(
                    insertion,
                    &subtree,
                    |parent_id| docking
                        .tree_for_host(target_host)
                        .is_some_and(|t| t.tiles.get(parent_id).is_some()),
                );
                let _ = docking.insert_subtree_into_host(target_host, subtree, insertion);
            } else {
                let source_host = hosts[rng.next_usize(hosts.len())];
                let Some(source_tree) = docking.tree_for_host(source_host) else {
                    assert_all_trees_ok(&docking);
                    continue;
                };
                let Some(tile_id) = random_pane_id(&mut rng, source_tree) else {
                    assert_all_trees_ok(&docking);
                    continue;
                };
                let Some(subtree) =
                    docking.take_subtree_from_host_for_drop(&ctx, &mut behavior, source_host, tile_id)
                else {
                    assert_all_trees_ok(&docking);
                    continue;
                };

                let mut targets: Vec<_> = current_hosts(&docking)
                    .into_iter()
                    .filter(|h| *h != source_host)
                    .collect();
                if targets.is_empty() {
                    targets.push(WindowHost::DockTree {
                        viewport: ViewportId::ROOT,
                    });
                }
                let target_host = targets[rng.next_usize(targets.len())];

                let insertion = docking
                    .tree_for_host(target_host)
                    .and_then(|t| random_insertion_point(&mut rng, t));
                let insertion = drop_sanitize::sanitize_insertion_for_subtree(
                    insertion,
                    &subtree,
                    |parent_id| docking
                        .tree_for_host(target_host)
                        .is_some_and(|t| t.tiles.get(parent_id).is_some()),
                );
                if let Err(subtree_back) =
                    docking.insert_subtree_into_host(target_host, subtree, insertion)
                {
                    let _ = docking.insert_subtree_into_host(
                        WindowHost::DockTree {
                            viewport: ViewportId::ROOT,
                        },
                        subtree_back,
                        None,
                    );
                }
            }

            assert_all_trees_ok(&docking);
        }
    }
}
