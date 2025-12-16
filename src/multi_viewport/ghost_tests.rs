use egui::{Context, Event, Pos2, Rect, Vec2, ViewportId, ViewportInfo};
use egui_tiles::{Behavior, TileId, Tiles, Tree};

use super::types::DockPayload;
use super::DockingMultiViewport;

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

fn begin_pass_for_viewport(ctx: &Context, viewport_id: ViewportId, pointer_local: Pos2) {
    let inner_rect = Rect::from_min_size(Pos2::ZERO, Vec2::new(800.0, 600.0));

    let mut viewports: egui::ViewportIdMap<ViewportInfo> = Default::default();
    viewports.insert(ViewportId::ROOT, ViewportInfo::default());
    viewports.insert(
        viewport_id,
        ViewportInfo {
            inner_rect: Some(inner_rect),
            outer_rect: Some(inner_rect),
            ..Default::default()
        },
    );

    let raw = egui::RawInput {
        viewport_id,
        viewports,
        screen_rect: Some(inner_rect),
        events: vec![Event::PointerMoved(pointer_local)],
        ..Default::default()
    };

    ctx.begin_pass(raw);
}

fn new_tabs_tree() -> Tree<()> {
    let mut tiles: Tiles<()> = Tiles::default();
    let a = tiles.insert_pane(());
    let b = tiles.insert_pane(());
    let root = tiles.insert_tab_tile(vec![a, b]);
    Tree::new(egui::Id::new("ghost_test_tree"), root, tiles)
}

fn new_bridge_tree() -> Tree<()> {
    let mut tiles: Tiles<()> = Tiles::default();
    let root = tiles.insert_pane(());
    Tree::new(egui::Id::new("ghost_test_bridge"), root, tiles)
}

#[test]
fn ghost_spawns_when_dragging_tree_outside_dock_rect() {
    let viewport_id = ViewportId::from_hash_of("ghost_test_viewport");
    let ctx = Context::default();
    begin_pass_for_viewport(&ctx, viewport_id, Pos2::new(250.0, 50.0));

    let mut behavior = DummyBehavior::default();
    let mut docking = DockingMultiViewport::new(new_bridge_tree());
    docking.options.debug_event_log = true;
    docking.options.ghost_tear_off = true;
    docking.options.ghost_spawn_native_on_leave_dock = true;

    let mut tree = new_tabs_tree();
    let root = tree.root.expect("tree must have root");
    ctx.set_dragged_id(root.egui_id(tree.id()));

    let dock_rect = Rect::from_min_size(Pos2::ZERO, Vec2::new(100.0, 100.0));
    docking.maybe_start_ghost_from_tree_in_viewport(
        &ctx,
        &mut behavior,
        dock_rect,
        viewport_id,
        &mut tree,
    );

    assert!(docking.ghost.is_some(), "ghost must be spawned");
    assert_eq!(docking.detached_viewport_count(), 1);
    assert!(tree.root.is_none(), "tree must be extracted into ghost");
    let _ = ctx.end_pass();
}

#[test]
fn ghost_does_not_spawn_during_window_move_payload() {
    let viewport_id = ViewportId::from_hash_of("ghost_test_viewport_window_move");
    let ctx = Context::default();
    begin_pass_for_viewport(&ctx, viewport_id, Pos2::new(250.0, 50.0));

    let mut behavior = DummyBehavior::default();
    let mut docking = DockingMultiViewport::new(new_bridge_tree());
    docking.options.debug_event_log = true;
    docking.options.ghost_tear_off = true;
    docking.options.ghost_spawn_native_on_leave_dock = true;

    let mut tree = new_tabs_tree();
    let root = tree.root.expect("tree must have root");
    ctx.set_dragged_id(root.egui_id(tree.id()));

    egui::DragAndDrop::set_payload(
        &ctx,
        DockPayload {
            bridge_id: docking.tree.id(),
            source_viewport: viewport_id,
            source_floating: None,
            tile_id: None,
        },
    );

    let dock_rect = Rect::from_min_size(Pos2::ZERO, Vec2::new(100.0, 100.0));
    docking.maybe_start_ghost_from_tree_in_viewport(
        &ctx,
        &mut behavior,
        dock_rect,
        viewport_id,
        &mut tree,
    );

    assert!(docking.ghost.is_none(), "window-move must not start ghost tear-off");
    assert_eq!(docking.detached_viewport_count(), 0);
    assert!(tree.root.is_some(), "window-move must not extract the tree");
    let _ = ctx.end_pass();
}
