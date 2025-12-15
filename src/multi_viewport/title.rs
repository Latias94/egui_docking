use egui_tiles::{Behavior, Tile, Tree};

pub(super) fn title_for_detached_subtree<Pane>(
    subtree: &egui_tiles::SubTree<Pane>,
    behavior: &mut dyn Behavior<Pane>,
) -> String {
    let mut stack = vec![subtree.root];
    while let Some(id) = stack.pop() {
        let Some(tile) = subtree.tiles.get(id) else {
            continue;
        };
        match tile {
            Tile::Pane(pane) => return behavior.tab_title_for_pane(pane).text().to_owned(),
            Tile::Container(container) => stack.extend(container.children().copied()),
        }
    }

    format!("{:?}", subtree.root)
}

pub(super) fn title_for_detached_tree<Pane>(
    tree: &Tree<Pane>,
    behavior: &mut dyn Behavior<Pane>,
) -> String {
    let Some(root) = tree.root else {
        return "Detached".to_owned();
    };

    let mut stack = vec![root];
    while let Some(id) = stack.pop() {
        let Some(tile) = tree.tiles.get(id) else {
            continue;
        };
        match tile {
            Tile::Pane(pane) => return behavior.tab_title_for_pane(pane).text().to_owned(),
            Tile::Container(container) => stack.extend(container.children().copied()),
        }
    }

    format!("{root:?}")
}

