use egui_tiles::{InsertionPoint, TileId};

pub(super) fn sanitize_insertion_for_subtree<Pane>(
    insertion: Option<InsertionPoint>,
    subtree: &egui_tiles::SubTree<Pane>,
    target_parent_exists: impl Fn(TileId) -> bool,
) -> Option<InsertionPoint> {
    let insertion = insertion.filter(|ins| {
        let parent_inside_subtree =
            ins.parent_id == subtree.root || subtree.tiles.get(ins.parent_id).is_some();
        !parent_inside_subtree
    });

    insertion.filter(|ins| target_parent_exists(ins.parent_id))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn subtree_with_root() -> egui_tiles::SubTree<()> {
        let mut tiles = egui_tiles::Tiles::default();
        let root = tiles.insert_pane(());
        egui_tiles::SubTree { root, tiles }
    }

    fn subtree_with_child() -> (egui_tiles::SubTree<()>, TileId) {
        let mut tiles = egui_tiles::Tiles::default();
        let a = tiles.insert_pane(());
        let b = tiles.insert_pane(());
        let root = tiles.insert_tab_tile(vec![a, b]);
        (egui_tiles::SubTree { root, tiles }, a)
    }

    #[test]
    fn drops_parent_inside_subtree() {
        let subtree = subtree_with_root();
        let insertion = Some(InsertionPoint::new(
            subtree.root,
            egui_tiles::ContainerInsertion::Horizontal(usize::MAX),
        ));
        let sanitized = sanitize_insertion_for_subtree(insertion, &subtree, |_| true);
        assert!(sanitized.is_none());
    }

    #[test]
    fn drops_parent_inside_subtree_child() {
        let (subtree, child) = subtree_with_child();
        let insertion = Some(InsertionPoint::new(
            child,
            egui_tiles::ContainerInsertion::Tabs(usize::MAX),
        ));
        let sanitized = sanitize_insertion_for_subtree(insertion, &subtree, |_| true);
        assert!(sanitized.is_none());
    }

    #[test]
    fn drops_missing_target_parent() {
        let subtree = subtree_with_root();
        let missing_parent = TileId::from_u64(9_999_999);
        let insertion = Some(InsertionPoint::new(
            missing_parent,
            egui_tiles::ContainerInsertion::Tabs(usize::MAX),
        ));
        let sanitized = sanitize_insertion_for_subtree(insertion, &subtree, |_| false);
        assert!(sanitized.is_none());
    }

    #[test]
    fn keeps_valid_target_parent() {
        let subtree = subtree_with_root();
        let good_parent = TileId::from_u64(123);
        let insertion = Some(InsertionPoint::new(
            good_parent,
            egui_tiles::ContainerInsertion::Tabs(usize::MAX),
        ));
        let sanitized = sanitize_insertion_for_subtree(insertion, &subtree, |id| id == good_parent);
        assert_eq!(sanitized, insertion);
    }
}
