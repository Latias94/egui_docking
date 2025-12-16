use egui::{Pos2, Rect, Vec2};
use egui_tiles::{Behavior, Container, Tile, TileId, Tiles, Tree};

use super::overlay_decision::{decide_overlay_for_tree, DragKind, OverlayPaint};

#[derive(Default)]
struct DummyBehavior;

impl Behavior<()> for DummyBehavior {
    fn pane_ui(
        &mut self,
        _ui: &mut egui::Ui,
        _tile_id: TileId,
        _pane: &mut (),
    ) -> egui_tiles::UiResponse {
        Default::default()
    }

    fn tab_title_for_pane(&mut self, _pane: &()) -> egui::WidgetText {
        "pane".into()
    }
}

fn layout_tree(tree: &mut Tree<()>, behavior: &mut dyn Behavior<()>) -> (Rect, egui::Style) {
    let ctx = egui::Context::default();
    let raw = egui::RawInput {
        screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(800.0, 600.0))),
        ..Default::default()
    };

    ctx.begin_pass(raw);
    let mut dock_rect = Rect::NOTHING;
    egui::CentralPanel::default().show(&ctx, |ui| {
        dock_rect = ui.available_rect_before_wrap();
        tree.ui(behavior, ui);
    });
    let _ = ctx.end_pass();

    (dock_rect, (*ctx.global_style()).clone())
}

fn tabs_tree_two_panes_active(active_pane: usize) -> (Tree<()>, TileId, TileId) {
    let mut tiles: Tiles<()> = Tiles::default();
    let a = tiles.insert_pane(());
    let b = tiles.insert_pane(());
    let root = tiles.insert_tab_tile(vec![a, b]);

    let mut tree = Tree::new(egui::Id::new("tree"), root, tiles);
    let active = if active_pane == 0 { a } else { b };
    if let Some(Tile::Container(Container::Tabs(tabs))) = tree.tiles.get_mut(root) {
        tabs.set_active(active);
    }

    (tree, a, b)
}

#[test]
fn internal_drag_only_paints_on_explicit_hit() {
    let mut behavior = DummyBehavior::default();
    let (mut tree, dragged, other) = tabs_tree_two_panes_active(1);
    let (dock_rect, style) = layout_tree(&mut tree, &mut behavior);

    let other_rect = tree.tiles.rect(other).expect("active pane must have rect");

    let pointer_no_hit = other_rect.min + Vec2::new(2.0, 2.0);
    let decision = decide_overlay_for_tree(
        &tree,
        &behavior,
        &style,
        dock_rect,
        pointer_no_hit,
        true,
        DragKind::Subtree {
            dragged_tile: Some(dragged),
            internal: true,
        },
    );
    assert!(decision.paint.is_none());
    assert!(decision.insertion_final.is_none());
    assert!(!decision.disable_tiles_preview);

    let pointer_hit = other_rect.center();
    let decision = decide_overlay_for_tree(
        &tree,
        &behavior,
        &style,
        dock_rect,
        pointer_hit,
        true,
        DragKind::Subtree {
            dragged_tile: Some(dragged),
            internal: true,
        },
    );
    assert!(matches!(decision.paint, Some(OverlayPaint::Inner(_))));
    assert!(decision.insertion_final.is_some());
    assert!(decision.disable_tiles_preview);
}

#[test]
fn window_move_is_explicit_only_but_overlay_is_discoverable() {
    let mut behavior = DummyBehavior::default();
    let (mut tree, _a, b) = tabs_tree_two_panes_active(1);
    let (dock_rect, style) = layout_tree(&mut tree, &mut behavior);

    let b_rect = tree.tiles.rect(b).expect("active pane must have rect");

    let pointer_no_hit = b_rect.min + Vec2::new(2.0, 2.0);
    let decision = decide_overlay_for_tree(
        &tree,
        &behavior,
        &style,
        dock_rect,
        pointer_no_hit,
        true,
        DragKind::WindowMove,
    );
    assert!(decision.paint.is_some());
    assert!(decision.insertion_final.is_none());

    let pointer_hit = b_rect.center();
    let decision = decide_overlay_for_tree(
        &tree,
        &behavior,
        &style,
        dock_rect,
        pointer_hit,
        true,
        DragKind::WindowMove,
    );
    assert!(decision.insertion_final.is_some());
}

#[test]
fn external_subtree_falls_back_to_dock_zone_when_not_explicit() {
    let mut behavior = DummyBehavior::default();
    let (mut tree, _a, b) = tabs_tree_two_panes_active(1);
    let (dock_rect, style) = layout_tree(&mut tree, &mut behavior);

    let b_rect = tree.tiles.rect(b).expect("active pane must have rect");
    let pointer_no_hit = b_rect.min + Vec2::new(2.0, 2.0);
    let decision = decide_overlay_for_tree(
        &tree,
        &behavior,
        &style,
        dock_rect,
        pointer_no_hit,
        true,
        DragKind::Subtree {
            dragged_tile: None,
            internal: false,
        },
    );

    assert!(decision.paint.is_some());
    assert!(decision.insertion_explicit.is_none());
    assert!(decision.fallback_zone.is_some());
    assert_eq!(
        decision.insertion_final,
        decision.fallback_zone.map(|z| z.insertion_point)
    );
}

#[test]
fn outer_overlay_is_mutually_exclusive_and_respects_internal_policy() {
    let mut behavior = DummyBehavior::default();
    let (mut tree, dragged, _other) = tabs_tree_two_panes_active(1);
    let (dock_rect, style) = layout_tree(&mut tree, &mut behavior);

    let pointer_outer_band = Pos2::new(dock_rect.left() + 2.0, dock_rect.center().y);

    let internal = decide_overlay_for_tree(
        &tree,
        &behavior,
        &style,
        dock_rect,
        pointer_outer_band,
        true,
        DragKind::Subtree {
            dragged_tile: Some(dragged),
            internal: true,
        },
    );
    assert!(internal.paint.is_none());

    let external = decide_overlay_for_tree(
        &tree,
        &behavior,
        &style,
        dock_rect,
        pointer_outer_band,
        true,
        DragKind::Subtree {
            dragged_tile: None,
            internal: false,
        },
    );
    assert!(matches!(external.paint, Some(OverlayPaint::Outer(_))));
}
