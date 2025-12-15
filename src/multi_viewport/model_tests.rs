use super::integrity;

fn assert_tree_ok(tree: &egui_tiles::Tree<()>) {
    let issues = integrity::tree_integrity_issues(tree);
    assert!(
        issues.is_empty(),
        "tree integrity failed:\n{}",
        issues.join("\n")
    );
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

#[derive(Default)]
struct DropAllPanesBehavior;

impl egui_tiles::Behavior<()> for DropAllPanesBehavior {
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
        false
    }
}

#[derive(Clone)]
struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Self {
        Self(seed ^ 0xD0C3_D0C3_D0C3_D0C3)
    }

    fn next_u64(&mut self) -> u64 {
        // Simple LCG: deterministic, fast, no dependency.
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

fn new_pane_subtree() -> egui_tiles::SubTree<()> {
    let mut tiles = egui_tiles::Tiles::default();
    let root = tiles.insert_pane(());
    egui_tiles::SubTree { root, tiles }
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

fn new_tree_random(id: egui::Id, rng: &mut Rng, panes: usize) -> egui_tiles::Tree<()> {
    let panes = panes.max(1);
    let mut tiles = egui_tiles::Tiles::default();
    let mut nodes: Vec<egui_tiles::TileId> = (0..panes).map(|_| tiles.insert_pane(())).collect();

    // Randomly combine nodes into containers until we have a single root.
    while nodes.len() > 1 {
        let take = (2 + rng.next_usize(3)).min(nodes.len()); // 2..=4
        let start = rng.next_usize(nodes.len() - take + 1);
        let chunk: Vec<_> = nodes.drain(start..start + take).collect();

        let kind = rng.next_usize(4);
        let new_id = match kind {
            0 => tiles.insert_tab_tile(chunk),
            1 => tiles.insert_horizontal_tile(chunk),
            2 => tiles.insert_vertical_tile(chunk),
            _ => tiles.insert_grid_tile(chunk),
        };
        nodes.push(new_id);
    }

    egui_tiles::Tree::new(id, nodes[0], tiles)
}

fn random_insertion_point(
    rng: &mut Rng,
    tree: &egui_tiles::Tree<()>,
    prefer_container_parent: bool,
) -> Option<egui_tiles::InsertionPoint> {
    let all_tile_ids: Vec<egui_tiles::TileId> = tree.tiles.tile_ids().collect();
    if all_tile_ids.is_empty() {
        return None;
    }

    let container_tile_ids: Vec<egui_tiles::TileId> = all_tile_ids
        .iter()
        .copied()
        .filter(|id| tree.tiles.get(*id).is_some_and(|t| t.is_container()))
        .collect();

    let candidates = if prefer_container_parent && !container_tile_ids.is_empty() {
        &container_tile_ids
    } else {
        &all_tile_ids
    };

    let parent_id = candidates[rng.next_usize(candidates.len())];

    let kind = rng.next_usize(3);
    let index = if rng.next_bool() { 0 } else { usize::MAX };
    let insertion = match kind {
        0 => egui_tiles::ContainerInsertion::Tabs(index),
        1 => egui_tiles::ContainerInsertion::Horizontal(index),
        _ => egui_tiles::ContainerInsertion::Vertical(index),
    };

    Some(egui_tiles::InsertionPoint::new(parent_id, insertion))
}

fn sanitize_insertion(
    insertion: Option<egui_tiles::InsertionPoint>,
    subtree: &egui_tiles::SubTree<()>,
    target_tree: &egui_tiles::Tree<()>,
) -> Option<egui_tiles::InsertionPoint> {
    super::drop_sanitize::sanitize_insertion_for_subtree(insertion, subtree, |parent_id| {
        target_tree.tiles.get(parent_id).is_some()
    })
}

fn subtree_contains_tile(
    tree: &egui_tiles::Tree<()>,
    root: egui_tiles::TileId,
    needle: egui_tiles::TileId,
) -> bool {
    if root == needle {
        return true;
    }
    let mut stack = vec![root];
    while let Some(id) = stack.pop() {
        if id == needle {
            return true;
        }
        if let Some(egui_tiles::Tile::Container(container)) = tree.tiles.get(id) {
            stack.extend(container.children().copied());
        }
    }
    false
}

fn random_simplify_options(rng: &mut Rng) -> egui_tiles::SimplificationOptions {
    // Keep defaults as the “likely real world” baseline, but flip a few knobs to stress invariants.
    let mut opt = egui_tiles::SimplificationOptions::default();
    if rng.next_u64() % 3 == 0 {
        opt.all_panes_must_have_tabs = true;
    }
    if rng.next_u64() % 4 == 0 {
        opt.join_nested_linear_containers = false;
    }
    if rng.next_u64() % 5 == 0 {
        opt.prune_single_child_tabs = false;
    }
    if rng.next_u64() % 7 == 0 {
        opt.prune_single_child_containers = false;
    }
    opt
}

#[test]
fn model_random_extract_insert_stays_integrity_ok() {
    for seed in 1u64..=12u64 {
        let mut rng = Rng::new(seed);

        let mut a = if rng.next_bool() {
            new_tree_tabs(egui::Id::new(format!("model_a_{seed}")), 5)
        } else {
            new_tree_random(egui::Id::new(format!("model_a_{seed}")), &mut rng, 5)
        };
        let mut b = if rng.next_bool() {
            new_tree_tabs(egui::Id::new(format!("model_b_{seed}")), 4)
        } else {
            new_tree_random(egui::Id::new(format!("model_b_{seed}")), &mut rng, 4)
        };

        assert_tree_ok(&a);
        assert_tree_ok(&b);

        let mut dummy_behavior = DummyBehavior::default();

        for _step in 0..400 {
            let do_cross = rng.next_bool();
            let do_new_pane = (rng.next_u64() % 13) == 0;
            let do_remove = (rng.next_u64() % 19) == 0;
            let do_move = (rng.next_u64() % 7) == 0;
            let do_simplify_children = (rng.next_u64() % 23) == 0;
            let do_simplify = (rng.next_u64() % 5) == 0;
            let do_gc = (rng.next_u64() % 17) == 0;

            if do_remove {
                let tree = if rng.next_bool() { &mut a } else { &mut b };
                let Some(root) = tree.root else {
                    continue;
                };
                let ids: Vec<egui_tiles::TileId> = tree.tiles.tile_ids().collect();
                if ids.len() <= 1 {
                    continue;
                }
                let candidates: Vec<_> = ids.into_iter().filter(|id| *id != root).collect();
                if candidates.is_empty() {
                    continue;
                }
                let victim = candidates[rng.next_usize(candidates.len())];
                let _removed = tree.remove_recursively(victim);
            } else if do_move {
                let tree = if rng.next_bool() { &mut a } else { &mut b };
                let Some(root) = tree.root else {
                    continue;
                };
                let ids: Vec<egui_tiles::TileId> = tree.tiles.tile_ids().collect();
                if ids.len() <= 1 {
                    continue;
                }

                let moved_candidates: Vec<_> =
                    ids.iter().copied().filter(|id| *id != root).collect();
                if moved_candidates.is_empty() {
                    continue;
                }
                let moved_tile = moved_candidates[rng.next_usize(moved_candidates.len())];

                let container_candidates: Vec<_> = ids
                    .iter()
                    .copied()
                    .filter(|id| tree.tiles.get(*id).is_some_and(|t| t.is_container()))
                    .collect();
                if container_candidates.is_empty() {
                    continue;
                }
                let destination_container =
                    container_candidates[rng.next_usize(container_candidates.len())];

                // Avoid cycles: do not move a tile into one of its descendants.
                if subtree_contains_tile(tree, moved_tile, destination_container) {
                    continue;
                }

                let insertion_index = if rng.next_bool() { 0 } else { usize::MAX };
                let reflow_grid = rng.next_u64() % 3 == 0;
                tree.move_tile_to_container(
                    moved_tile,
                    destination_container,
                    insertion_index,
                    reflow_grid,
                );
            } else if do_new_pane {
                let target = if rng.next_bool() { &mut a } else { &mut b };
                let subtree = new_pane_subtree();
                let prefer_container_parent = rng.next_u64() % 2 == 0;
                let insertion = random_insertion_point(&mut rng, target, prefer_container_parent);
                let insertion = sanitize_insertion(insertion, &subtree, target);
                target.insert_subtree_at(subtree, insertion);
            } else if do_cross {
                // Cross-tree: use `extract_subtree` to avoid id collisions.
                let (src, dst) = if rng.next_bool() {
                    (&mut a, &mut b)
                } else {
                    (&mut b, &mut a)
                };

                let ids: Vec<egui_tiles::TileId> = src.tiles.tile_ids().collect();
                if ids.is_empty() {
                    continue;
                }
                let picked = ids[rng.next_usize(ids.len())];
                let Some(subtree) = src.extract_subtree(picked) else {
                    continue;
                };

                let prefer_container_parent = rng.next_u64() % 2 == 0;
                let mut insertion = random_insertion_point(&mut rng, dst, prefer_container_parent);

                // Intentionally generate hostile insertion parents sometimes to validate sanitization.
                if (rng.next_u64() % 7) == 0 {
                    insertion = Some(egui_tiles::InsertionPoint::new(
                        subtree.root,
                        egui_tiles::ContainerInsertion::Horizontal(usize::MAX),
                    ));
                }
                if (rng.next_u64() % 11) == 0 {
                    insertion = Some(egui_tiles::InsertionPoint::new(
                        egui_tiles::TileId::from_u64(9_999_999),
                        egui_tiles::ContainerInsertion::Tabs(usize::MAX),
                    ));
                }

                let insertion = sanitize_insertion(insertion, &subtree, dst);
                dst.insert_subtree_at(subtree, insertion);
            } else {
                // Same-tree move: use `extract_subtree_no_reserve`.
                let tree = if rng.next_bool() { &mut a } else { &mut b };

                let ids: Vec<egui_tiles::TileId> = tree.tiles.tile_ids().collect();
                if ids.is_empty() {
                    continue;
                }
                let picked = ids[rng.next_usize(ids.len())];

                // Sometimes compute insertion *before* extraction to emulate the real flow
                // (insertion computed from hover rects, then we extract the subtree).
                let insertion_pre = if rng.next_u64() % 3 == 0 {
                    let prefer_container_parent = rng.next_u64() % 2 == 0;
                    random_insertion_point(&mut rng, tree, prefer_container_parent)
                } else {
                    None
                };
                let Some(subtree) = tree.extract_subtree_no_reserve(picked) else {
                    continue;
                };

                let mut insertion = insertion_pre.or_else(|| {
                    let prefer_container_parent = rng.next_u64() % 2 == 0;
                    random_insertion_point(&mut rng, tree, prefer_container_parent)
                });
                if (rng.next_u64() % 9) == 0 {
                    // Try to create the exact “self-parent insertion” scenario.
                    insertion = Some(egui_tiles::InsertionPoint::new(
                        subtree.root,
                        egui_tiles::ContainerInsertion::Vertical(usize::MAX),
                    ));
                }

                let insertion = sanitize_insertion(insertion, &subtree, tree);
                tree.insert_subtree_at(subtree, insertion);
            }

            if do_simplify {
                let opt_a = random_simplify_options(&mut rng);
                let opt_b = random_simplify_options(&mut rng);
                a.simplify(&opt_a);
                b.simplify(&opt_b);
            }

            if do_gc {
                a.gc(&mut dummy_behavior);
                b.gc(&mut dummy_behavior);
            }

            if do_simplify_children {
                let tree = if rng.next_bool() { &mut a } else { &mut b };
                let ids: Vec<egui_tiles::TileId> = tree.tiles.tile_ids().collect();
                let container_candidates: Vec<_> = ids
                    .iter()
                    .copied()
                    .filter(|id| tree.tiles.get(*id).is_some_and(|t| t.is_container()))
                    .collect();
                if !container_candidates.is_empty() {
                    let tile_id = container_candidates[rng.next_usize(container_candidates.len())];
                    let opt = random_simplify_options(&mut rng);
                    tree.simplify_children_of_tile(tile_id, &opt);
                }
            }

            assert_tree_ok(&a);
            assert_tree_ok(&b);
        }
    }
}

#[test]
fn tiles_gc_can_remove_root_pane() {
    let mut tiles = egui_tiles::Tiles::default();
    let root = tiles.insert_pane(());
    let mut tree = egui_tiles::Tree::new(egui::Id::new("gc_root_pane"), root, tiles);

    let mut behavior = DropAllPanesBehavior::default();
    tree.gc(&mut behavior);

    assert!(tree.root.is_none());
    assert!(tree.tiles.tile_ids().next().is_none());
    assert_tree_ok(&tree);
}
