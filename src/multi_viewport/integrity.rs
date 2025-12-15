use std::collections::{HashMap, HashSet};
use std::hash::{Hash as _, Hasher as _};

use egui_tiles::{Container, Tile, TileId, Tree};

pub(super) fn tree_integrity_issues<Pane>(tree: &Tree<Pane>) -> Vec<String> {
    let mut issues: Vec<String> = Vec::new();

    let Some(root) = tree.root else {
        if tree.tiles.tile_ids().next().is_some() {
            issues.push("integrity: root=None but tiles non-empty".to_owned());
        }
        return issues;
    };

    if tree.tiles.get(root).is_none() {
        issues.push(format!("integrity: root {root:?} missing"));
        return issues;
    }

    let mut visited: HashSet<TileId> = HashSet::new();
    let mut parent_of: HashMap<TileId, TileId> = HashMap::new();
    let mut stack: Vec<TileId> = vec![root];

    while let Some(tile_id) = stack.pop() {
        if !visited.insert(tile_id) {
            continue;
        }

        let Some(tile) = tree.tiles.get(tile_id) else {
            issues.push(format!("integrity: missing tile {tile_id:?} (reachable)"));
            continue;
        };

        let Tile::Container(container) = tile else {
            continue;
        };

        match container {
            Container::Tabs(tabs) => {
                if let Some(active) = tabs.active {
                    if !tabs.children.contains(&active) {
                        issues.push(format!(
                            "integrity: tabs {tile_id:?} active {active:?} not in children={:?}",
                            tabs.children
                        ));
                    }
                    if tree.tiles.get(active).is_none() {
                        issues.push(format!(
                            "integrity: tabs {tile_id:?} active {active:?} missing tile"
                        ));
                    }
                }
            }
            Container::Linear(_linear) => {}
            Container::Grid(_grid) => {}
        }

        let children: Vec<TileId> = container.children().copied().collect();

        let mut local_set: HashSet<TileId> = HashSet::new();
        for child in &children {
            if !local_set.insert(*child) {
                issues.push(format!(
                    "integrity: parent {tile_id:?} contains duplicate child {child:?}"
                ));
            }
        }

        for child in children {
            if tree.tiles.get(child).is_none() {
                issues.push(format!(
                    "integrity: parent {tile_id:?} references missing child {child:?}"
                ));
                continue;
            }

            if let Some(prev_parent) = parent_of.insert(child, tile_id) {
                issues.push(format!(
                    "integrity: child {child:?} has multiple parents {prev_parent:?} and {tile_id:?}"
                ));
            }

            stack.push(child);
        }
    }

    let total = tree.tiles.tile_ids().count();
    if visited.len() != total {
        issues.push(format!(
            "integrity: unreachable tiles {} of {}",
            total.saturating_sub(visited.len()),
            total
        ));
    }

    issues
}

pub(super) fn hash_issues(lines: &[String]) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    for line in lines {
        line.hash(&mut hasher);
    }
    hasher.finish()
}
