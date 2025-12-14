use std::collections::BTreeMap;

use egui::{Context, Pos2, Rect, Vec2, ViewportBuilder, ViewportClass, ViewportId};
use egui_tiles::{Behavior, ContainerKind, InsertionPoint, Tile, TileId, Tree};

/// Options for [`DockingMultiViewport`].
#[derive(Clone, Debug)]
pub struct DockingMultiViewportOptions {
    /// Fallback inner size (in points) when we can't infer a better size for a torn-off pane.
    pub default_detached_inner_size: Vec2,

    /// If true, holding SHIFT while tearing off a pane will instead tear off the closest parent `Tabs` container,
    /// preserving the whole tab-group (dear imgui style "dock node tear-off").
    pub detach_parent_tabs_on_shift: bool,
}

impl Default for DockingMultiViewportOptions {
    fn default() -> Self {
        Self {
            default_detached_inner_size: Vec2::new(480.0, 360.0),
            detach_parent_tabs_on_shift: true,
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct DockPayload {
    bridge_id: egui::Id,
    source_viewport: ViewportId,
    tile_id: Option<TileId>,
}

#[derive(Clone, Copy, Debug)]
struct PendingDrop {
    payload: DockPayload,
    pointer_global: Pos2,
}

#[derive(Debug)]
struct DetachedDock<Pane> {
    tree: Tree<Pane>,
    builder: ViewportBuilder,
}

#[derive(Clone, Copy, Debug)]
enum DropAction {
    MoveSubtree {
        source_viewport: ViewportId,
        tile_id: TileId,
        insertion: Option<InsertionPoint>,
    },
    MoveWholeTree {
        source_viewport: ViewportId,
        insertion: Option<InsertionPoint>,
    },
}

/// Bridge `egui_tiles` docking with `egui` multi-viewports.
///
/// Current scope:
/// - Tear-off: drag a pane and release outside the dock → new native viewport window.
/// - Re-dock: drag a detached window's header back into the root dock and release.
/// - Cross-window tab move: drag a tab/pane inside a detached window back into the root dock and release.
/// - Viewport↔viewport move: drop onto any detached window's dock.
///
/// Notes:
/// - The root dock drop preview/targeting uses `egui_tiles::Tree::dock_zone_at` (same heuristic as internal drag-drop).
/// - Holding SHIFT while tearing off a pane can detach the whole parent `Tabs` container (see options).
#[derive(Debug)]
pub struct DockingMultiViewport<Pane> {
    pub options: DockingMultiViewportOptions,
    pub tree: Tree<Pane>,

    detached: BTreeMap<ViewportId, DetachedDock<Pane>>,
    next_viewport_serial: u64,

    last_root_dock_rect: Option<Rect>,
    last_dock_rects: BTreeMap<ViewportId, Rect>,

    last_pointer_global: Option<Pos2>,

    pending_drop: Option<PendingDrop>,
}

impl<Pane> DockingMultiViewport<Pane> {
    pub fn new(tree: Tree<Pane>) -> Self {
        Self::new_with_options(tree, DockingMultiViewportOptions::default())
    }

    pub fn new_with_options(tree: Tree<Pane>, options: DockingMultiViewportOptions) -> Self {
        Self {
            options,
            tree,
            detached: BTreeMap::new(),
            next_viewport_serial: 1,
            last_root_dock_rect: None,
            last_dock_rects: BTreeMap::new(),
            last_pointer_global: None,
            pending_drop: None,
        }
    }

    /// Show detached viewports + the root dock in the current (root) viewport.
    ///
    /// Call this from your `eframe::App::update` (or equivalent).
    pub fn ui(&mut self, ctx: &Context, behavior: &mut dyn Behavior<Pane>) {
        // 1) Detached viewports first: they can re-dock into the root tree, and we want the root
        //    dock to reflect that immediately within the same frame.
        self.ui_detached_viewports(ctx, behavior);

        // 2) Root dock (ViewportId::ROOT).
        egui::CentralPanel::default().show(ctx, |ui| {
            self.update_last_pointer_global_from_active_viewport(ui.ctx());

            let dock_rect = ui.available_rect_before_wrap();
            self.last_root_dock_rect = Some(dock_rect);
            self.last_dock_rects.insert(ViewportId::ROOT, dock_rect);

            // Queue cross-viewport drops first so we don't accidentally tear-off when the release
            // is captured by the source window while the pointer is over a different viewport.
            self.queue_pending_drop_on_release(ui.ctx());

            // Tear-off detection must happen before `tree.ui`, otherwise egui_tiles will interpret
            // every drop as "somewhere" inside the tree.
            self.try_tear_off_from_root(ui.ctx(), behavior, dock_rect);

            self.tree.ui(behavior, ui);

            self.set_payload_from_root_drag_if_any(ui.ctx());
            self.paint_drop_preview_if_any_for_tree(
                ui,
                behavior,
                &self.tree,
                dock_rect,
                ViewportId::ROOT,
            );
        });

        // Apply after all viewports have had a chance to run `tree.ui` this frame so we can use
        // the computed rectangles for accurate docking.
        self.apply_pending_drop(ctx, behavior);
    }

    fn ui_detached_viewports(&mut self, ctx: &Context, behavior: &mut dyn Behavior<Pane>) {
        let viewport_ids: Vec<ViewportId> = self.detached.keys().copied().collect();
        let bridge_id = self.tree.id();

        for viewport_id in viewport_ids {
            let Some(mut detached) = self.detached.remove(&viewport_id) else {
                continue;
            };

            let builder = detached.builder.clone();
            let mut should_redock_to_root = false;

            ctx.show_viewport_immediate(viewport_id, builder, |ctx, class| {
                self.update_last_pointer_global_from_active_viewport(ctx);

                let title = title_for_detached_tree(&detached.tree, behavior);

                match class {
                    ViewportClass::Immediate | ViewportClass::Deferred | ViewportClass::Root => {
                        // For a child viewport created with `show_viewport_immediate` we expect
                        // `Immediate` (or `Embedded` below).
                    }
                    ViewportClass::Embedded => {
                        egui::Window::new(title.clone())
                            .default_size(
                                detached
                                    .builder
                                    .inner_size
                                    .unwrap_or(self.options.default_detached_inner_size),
                            )
                            .show(ctx, |ui| detached.tree.ui(behavior, ui));
                        return;
                    }
                }

                egui::TopBottomPanel::top("egui_docking_detached_top_bar").show(ctx, |ui| {
                    ui.horizontal(|ui| {
                        let response = ui
                            .add(
                                egui::Label::new(title)
                                    .selectable(false)
                                    .sense(egui::Sense::click_and_drag()),
                            )
                            .on_hover_cursor(egui::CursorIcon::Grab);

                        if response.drag_started() {
                            egui::DragAndDrop::set_payload(
                                ctx,
                                DockPayload {
                                    bridge_id,
                                    source_viewport: viewport_id,
                                    tile_id: None,
                                },
                            );
                            ctx.request_repaint_of(ViewportId::ROOT);
                        }
                    });
                });

                egui::CentralPanel::default().show(ctx, |ui| {
                    let dock_rect = ui.available_rect_before_wrap();
                    self.last_dock_rects.insert(viewport_id, dock_rect);

                    // Same as root: queue cross-viewport drops before we consider tearing off.
                    self.queue_pending_drop_on_release(ctx);

                    let mut did_tear_off = false;
                    self.try_tear_off_from_detached(
                        ctx,
                        behavior,
                        dock_rect,
                        viewport_id,
                        &mut detached.tree,
                        &mut did_tear_off,
                    );

                    detached.tree.ui(behavior, ui);

                    self.paint_drop_preview_if_any_for_tree(
                        ui,
                        behavior,
                        &detached.tree,
                        dock_rect,
                        viewport_id,
                    );
                    if did_tear_off {
                        ctx.request_repaint_of(ViewportId::ROOT);
                    }
                });

                if let Some(dragged_tile) = detached.tree.dragged_id_including_root(ctx) {
                    egui::DragAndDrop::set_payload(
                        ctx,
                        DockPayload {
                            bridge_id,
                            source_viewport: viewport_id,
                            tile_id: Some(dragged_tile),
                        },
                    );
                    ctx.request_repaint_of(ViewportId::ROOT);
                } else if let Some(payload) = egui::DragAndDrop::payload::<DockPayload>(ctx) {
                    if payload.bridge_id == bridge_id
                        && payload.source_viewport == viewport_id
                        && ctx.input(|i| i.pointer.any_released())
                    {
                        egui::DragAndDrop::clear_payload(ctx);
                    }
                }

                if ctx.input(|i| i.viewport().close_requested()) {
                    // Safe default: closing the native window re-docks it to the root.
                    should_redock_to_root = true;
                }
            });

            if should_redock_to_root {
                self.dock_tree_into_root(detached.tree, None);
                continue;
            }

            // Keep detached.
            detached.builder = detached
                .builder
                .clone()
                .with_title(title_for_detached_tree(&detached.tree, behavior));
            self.detached.insert(viewport_id, detached);
        }
    }

    fn set_payload_from_root_drag_if_any(&mut self, ctx: &Context) {
        let bridge_id = self.tree.id();

        if let Some(dragged_tile) = self.tree.dragged_id_including_root(ctx) {
            egui::DragAndDrop::set_payload(
                ctx,
                DockPayload {
                    bridge_id,
                    source_viewport: ViewportId::ROOT,
                    tile_id: Some(dragged_tile),
                },
            );
            return;
        }

        if ctx.input(|i| i.pointer.any_released()) {
            if let Some(payload) = egui::DragAndDrop::payload::<DockPayload>(ctx) {
                if payload.bridge_id == bridge_id && payload.source_viewport == ViewportId::ROOT {
                    egui::DragAndDrop::clear_payload(ctx);
                }
            }
        }
    }

    fn try_tear_off_from_root(
        &mut self,
        ctx: &Context,
        behavior: &mut dyn Behavior<Pane>,
        dock_rect: Rect,
    ) {
        let did_release = ctx.input(|i| i.pointer.any_released());
        if !did_release {
            return;
        }

        let Some(dragged_tile) = self.tree.dragged_id_including_root(ctx) else {
            return;
        };

        let detach_tile = self.pick_detach_tile(ctx, dragged_tile);

        let pointer_pos = ctx.input(|i| i.pointer.latest_pos());
        let dropped_inside_dock = pointer_pos.is_some_and(|p| dock_rect.contains(p));
        if dropped_inside_dock {
            return;
        }

        // Prevent egui_tiles from applying an internal drop this frame.
        ctx.stop_dragging();
        if let Some(payload) = egui::DragAndDrop::payload::<DockPayload>(ctx) {
            if payload.bridge_id == self.tree.id() && payload.source_viewport == ViewportId::ROOT {
                egui::DragAndDrop::clear_payload(ctx);
            }
        }

        let pane_rect_last = self.tree.tiles.rect(detach_tile);
        let global_fallback_pos = self.last_pointer_global;
        let root_inner_rect = root_inner_rect_in_global(ctx);

        let Some(subtree) = self.tree.extract_subtree(detach_tile) else {
            return;
        };

        let title = title_for_detached_subtree(&subtree, behavior);
        let (pos, size) = infer_detached_geometry(
            pane_rect_last,
            global_fallback_pos,
            root_inner_rect,
            self.options.default_detached_inner_size,
        );

        let (viewport_id, serial) = self.allocate_detached_viewport_id();
        let builder = ViewportBuilder::default()
            .with_title(title)
            .with_position(pos)
            .with_inner_size(size);

        let detached_tree_id =
            egui::Id::new((self.tree.id(), "egui_docking_detached_tree", serial));
        let detached_tree = Tree::new(detached_tree_id, subtree.root, subtree.tiles);

        self.detached.insert(
            viewport_id,
            DetachedDock {
                tree: detached_tree,
                builder,
            },
        );

        ctx.request_repaint();
    }

    fn try_tear_off_from_detached(
        &mut self,
        ctx: &Context,
        behavior: &mut dyn Behavior<Pane>,
        dock_rect: Rect,
        current_viewport: ViewportId,
        tree: &mut Tree<Pane>,
        did_tear_off: &mut bool,
    ) {
        let did_release = ctx.input(|i| i.pointer.any_released());
        if !did_release {
            return;
        }

        let Some(dragged_tile) = tree.dragged_id_including_root(ctx) else {
            return;
        };

        let pointer_pos = ctx.input(|i| i.pointer.latest_pos());
        let dropped_inside_dock = pointer_pos.is_some_and(|p| dock_rect.contains(p));
        if dropped_inside_dock {
            return;
        }

        ctx.stop_dragging();
        if let Some(payload) = egui::DragAndDrop::payload::<DockPayload>(ctx) {
            if payload.bridge_id == self.tree.id() && payload.source_viewport == current_viewport {
                egui::DragAndDrop::clear_payload(ctx);
            }
        }

        let pane_rect_last = tree.tiles.rect(dragged_tile);
        let global_fallback_pos = self.last_pointer_global;
        let inner_rect = ctx.input(|i| i.viewport().inner_rect);

        let Some(subtree) = tree.extract_subtree(dragged_tile) else {
            return;
        };

        let title = title_for_detached_subtree(&subtree, behavior);
        let (pos, size) = infer_detached_geometry(
            pane_rect_last,
            global_fallback_pos,
            inner_rect,
            self.options.default_detached_inner_size,
        );

        let (viewport_id, serial) = self.allocate_detached_viewport_id();
        let builder = ViewportBuilder::default()
            .with_title(title)
            .with_position(pos)
            .with_inner_size(size);

        let detached_tree_id =
            egui::Id::new((self.tree.id(), "egui_docking_detached_tree", serial));
        let detached_tree = Tree::new(detached_tree_id, subtree.root, subtree.tiles);

        self.detached.insert(
            viewport_id,
            DetachedDock {
                tree: detached_tree,
                builder,
            },
        );

        if tree.root.is_none() {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }

        *did_tear_off = true;
        ctx.request_repaint();
        ctx.request_repaint_of(ViewportId::ROOT);
    }

    fn allocate_detached_viewport_id(&mut self) -> (ViewportId, u64) {
        let serial = self.next_viewport_serial;
        self.next_viewport_serial = self.next_viewport_serial.saturating_add(1);
        (
            ViewportId::from_hash_of(("egui_docking_detached", serial)),
            serial,
        )
    }

    fn update_last_pointer_global_from_active_viewport(&mut self, ctx: &Context) {
        if let Some(pos) = pointer_pos_in_global(ctx) {
            self.last_pointer_global = Some(pos);
        }
    }

    fn queue_pending_drop_on_release(&mut self, ctx: &Context) {
        if self.pending_drop.is_some() {
            return;
        }
        if !ctx.input(|i| i.pointer.any_released()) {
            return;
        }

        let Some(payload) = egui::DragAndDrop::payload::<DockPayload>(ctx) else {
            return;
        };
        if payload.bridge_id != self.tree.id() {
            return;
        }

        // Prefer the active viewport's computed global pointer, but fall back to the last known
        // global pointer from any viewport if needed.
        let pointer_global = pointer_pos_in_global(ctx).or(self.last_pointer_global);
        let Some(pointer_global) = pointer_global else {
            return;
        };

        let Some(target_viewport) = viewport_under_pointer_global(ctx, pointer_global) else {
            return;
        };
        if target_viewport == payload.source_viewport {
            return;
        }

        self.pending_drop = Some(PendingDrop {
            payload: *payload,
            pointer_global,
        });

        egui::DragAndDrop::clear_payload(ctx);
        ctx.stop_dragging();
        ctx.request_repaint_of(ViewportId::ROOT);
    }

    fn apply_pending_drop(&mut self, ctx: &Context, behavior: &mut dyn Behavior<Pane>) {
        let Some(pending) = self.pending_drop.take() else {
            return;
        };

        let Some(target_viewport) = viewport_under_pointer_global(ctx, pending.pointer_global)
        else {
            return;
        };
        if target_viewport == pending.payload.source_viewport {
            return;
        }

        let style = ctx.style();
        let insertion = self.drop_insertion_at_pointer_global(
            ctx,
            behavior,
            &style,
            target_viewport,
            pending.pointer_global,
        );

        if target_viewport == ViewportId::ROOT {
            self.apply_drop_to_root(ctx, behavior, insertion, pending.payload);
        } else {
            self.apply_drop_to_detached(ctx, behavior, target_viewport, insertion, pending.payload);
        }
    }

    fn drop_insertion_at_pointer_global(
        &self,
        ctx: &Context,
        behavior: &dyn Behavior<Pane>,
        style: &egui::Style,
        target_viewport: ViewportId,
        pointer_global: Pos2,
    ) -> Option<InsertionPoint> {
        let Some(pointer_local) =
            pointer_pos_in_target_viewport_space(ctx, target_viewport, pointer_global)
        else {
            return None;
        };

        let dock_rect = self.last_dock_rects.get(&target_viewport).copied();
        if dock_rect.is_some_and(|r| !r.contains(pointer_local)) {
            return None;
        }

        if target_viewport == ViewportId::ROOT {
            return self
                .tree
                .dock_zone_at(behavior, style, pointer_local)
                .map(|z| z.insertion_point);
        }

        let Some(detached) = self.detached.get(&target_viewport) else {
            return None;
        };
        detached
            .tree
            .dock_zone_at(behavior, style, pointer_local)
            .map(|z| z.insertion_point)
    }

    fn apply_drop_to_root(
        &mut self,
        ctx: &Context,
        behavior: &mut dyn Behavior<Pane>,
        insertion: Option<InsertionPoint>,
        payload: DockPayload,
    ) {
        if payload.source_viewport == ViewportId::ROOT {
            return;
        }

        let Some(mut detached) = self.detached.remove(&payload.source_viewport) else {
            return;
        };

        if let Some(tile_id) = payload.tile_id {
            let Some(subtree) = detached.tree.extract_subtree(tile_id) else {
                self.detached.insert(payload.source_viewport, detached);
                return;
            };

            self.dock_subtree_into_root(subtree, insertion);

            if detached.tree.root.is_some() {
                detached.builder = detached
                    .builder
                    .clone()
                    .with_title(title_for_detached_tree(&detached.tree, behavior));
                self.detached.insert(payload.source_viewport, detached);
            } else {
                ctx.send_viewport_cmd_to(payload.source_viewport, egui::ViewportCommand::Close);
            }
        } else {
            self.dock_tree_into_root(detached.tree, insertion);
            ctx.send_viewport_cmd_to(payload.source_viewport, egui::ViewportCommand::Close);
        }
    }

    fn apply_drop_to_detached(
        &mut self,
        ctx: &Context,
        behavior: &mut dyn Behavior<Pane>,
        target_viewport: ViewportId,
        insertion: Option<InsertionPoint>,
        payload: DockPayload,
    ) {
        let Some(mut target) = self.detached.remove(&target_viewport) else {
            return;
        };

        if payload.source_viewport == target_viewport {
            self.detached.insert(target_viewport, target);
            return;
        }

        let action = match payload.tile_id {
            Some(tile_id) => DropAction::MoveSubtree {
                source_viewport: payload.source_viewport,
                tile_id,
                insertion,
            },
            None => DropAction::MoveWholeTree {
                source_viewport: payload.source_viewport,
                insertion,
            },
        };

        self.apply_drop_action_to_detached_target(
            ctx,
            target_viewport,
            &mut target,
            action,
            behavior,
        );

        target.builder = target
            .builder
            .clone()
            .with_title(title_for_detached_tree(&target.tree, behavior));
        self.detached.insert(target_viewport, target);
    }

    fn dock_tree_into_root(
        &mut self,
        mut detached_tree: Tree<Pane>,
        insertion: Option<InsertionPoint>,
    ) {
        let Some(detached_root) = detached_tree.root.take() else {
            return;
        };

        let detached_tiles = std::mem::take(&mut detached_tree.tiles);

        self.tree.insert_subtree_at(
            egui_tiles::SubTree {
                root: detached_root,
                tiles: detached_tiles,
            },
            insertion,
        );
    }

    fn dock_subtree_into_root(
        &mut self,
        subtree: egui_tiles::SubTree<Pane>,
        insertion: Option<InsertionPoint>,
    ) {
        self.tree.insert_subtree_at(subtree, insertion);
    }

    fn apply_drop_action_to_detached_target(
        &mut self,
        ctx: &Context,
        target_viewport: ViewportId,
        target: &mut DetachedDock<Pane>,
        action: DropAction,
        behavior: &mut dyn Behavior<Pane>,
    ) {
        match action {
            DropAction::MoveSubtree {
                source_viewport,
                tile_id,
                insertion,
            } => {
                let subtree = if source_viewport == ViewportId::ROOT {
                    self.tree.extract_subtree(tile_id)
                } else if source_viewport == target_viewport {
                    None
                } else if let Some(mut source) = self.detached.remove(&source_viewport) {
                    let extracted = source.tree.extract_subtree(tile_id);
                    if extracted.is_some() {
                        if source.tree.root.is_some() {
                            source.builder = source
                                .builder
                                .clone()
                                .with_title(title_for_detached_tree(&source.tree, behavior));
                            self.detached.insert(source_viewport, source);
                        } else {
                            ctx.send_viewport_cmd_to(source_viewport, egui::ViewportCommand::Close);
                        }
                    } else {
                        self.detached.insert(source_viewport, source);
                    }
                    extracted
                } else {
                    None
                };

                if let Some(subtree) = subtree {
                    target.tree.insert_subtree_at(subtree, insertion);
                }
            }

            DropAction::MoveWholeTree {
                source_viewport,
                insertion,
            } => {
                if source_viewport == ViewportId::ROOT || source_viewport == target_viewport {
                    return;
                }

                let Some(mut source) = self.detached.remove(&source_viewport) else {
                    return;
                };

                let Some(source_root) = source.tree.root.take() else {
                    return;
                };

                let source_tiles = std::mem::take(&mut source.tree.tiles);
                target.tree.insert_subtree_at(
                    egui_tiles::SubTree {
                        root: source_root,
                        tiles: source_tiles,
                    },
                    insertion,
                );

                ctx.send_viewport_cmd_to(source_viewport, egui::ViewportCommand::Close);
            }
        }
    }

    fn paint_drop_preview_if_any_for_tree(
        &self,
        ui: &egui::Ui,
        behavior: &dyn Behavior<Pane>,
        tree: &Tree<Pane>,
        dock_rect: Rect,
        target_viewport: ViewportId,
    ) {
        let Some(payload) = egui::DragAndDrop::payload::<DockPayload>(ui.ctx()) else {
            return;
        };
        if payload.bridge_id != self.tree.id() {
            return;
        }
        if payload.source_viewport == target_viewport {
            return;
        }

        let is_fresh = if payload.source_viewport == ViewportId::ROOT {
            payload
                .tile_id
                .is_some_and(|tile_id| self.tree.tiles.get(tile_id).is_some())
        } else {
            self.detached.contains_key(&payload.source_viewport)
        };
        if !is_fresh {
            return;
        }

        let Some(pointer_local) = pointer_pos_in_viewport_space(ui.ctx(), self.last_pointer_global)
        else {
            return;
        };
        if !dock_rect.contains(pointer_local) {
            return;
        }

        let Some(zone) = tree.dock_zone_at(behavior, ui.style(), pointer_local) else {
            return;
        };

        let stroke = ui.visuals().selection.stroke;
        let fill = stroke.color.gamma_multiply(0.25);
        ui.painter().rect(
            zone.preview_rect,
            1.0,
            fill,
            stroke,
            egui::StrokeKind::Inside,
        );

        ui.ctx().request_repaint();
    }

    fn pick_detach_tile(&self, ctx: &Context, dragged_tile: TileId) -> TileId {
        if !self.options.detach_parent_tabs_on_shift {
            return dragged_tile;
        }

        let shift = ctx.input(|i| i.modifiers.shift);
        if !shift {
            return dragged_tile;
        }

        if !matches!(self.tree.tiles.get(dragged_tile), Some(Tile::Pane(_))) {
            return dragged_tile;
        }

        let Some(parent) = self.tree.tiles.parent_of(dragged_tile) else {
            return dragged_tile;
        };

        let parent_kind = self.tree.tiles.get(parent).and_then(|t| t.kind());
        if parent_kind == Some(ContainerKind::Tabs) {
            parent
        } else {
            dragged_tile
        }
    }

    // `paint_root_drop_preview_if_any` replaced by `paint_drop_preview_if_any_for_tree`.
}

fn title_for_detached_subtree<Pane>(
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

fn title_for_detached_tree<Pane>(tree: &Tree<Pane>, behavior: &mut dyn Behavior<Pane>) -> String {
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

fn pointer_pos_in_global(ctx: &Context) -> Option<Pos2> {
    ctx.input(|i| {
        let local = i.pointer.interact_pos()?;
        let inner = i.viewport().inner_rect?;
        Some(inner.min + local.to_vec2())
    })
}

fn pointer_pos_in_viewport_space(ctx: &Context, pointer_global: Option<Pos2>) -> Option<Pos2> {
    let pointer_global = pointer_global?;
    let inner = ctx.input(|i| i.viewport().inner_rect)?;
    if !inner.contains(pointer_global) {
        return None;
    }

    let delta: Vec2 = pointer_global - inner.min;
    Some(Pos2::new(delta.x, delta.y))
}

fn pointer_pos_in_target_viewport_space(
    ctx: &Context,
    target_viewport: ViewportId,
    pointer_global: Pos2,
) -> Option<Pos2> {
    ctx.input(|i| {
        let inner = i.raw.viewports.get(&target_viewport)?.inner_rect?;
        if !inner.contains(pointer_global) {
            return None;
        }
        let delta: Vec2 = pointer_global - inner.min;
        Some(Pos2::new(delta.x, delta.y))
    })
}

fn viewport_under_pointer_global(ctx: &Context, pointer_global: Pos2) -> Option<ViewportId> {
    fn area(rect: Rect) -> f32 {
        rect.width() * rect.height()
    }

    ctx.input(|i| {
        i.raw
            .viewports
            .iter()
            .filter_map(|(id, info)| {
                let rect = info.inner_rect?;
                rect.contains(pointer_global).then_some((*id, rect))
            })
            .min_by(|a, b| area(a.1).total_cmp(&area(b.1)))
            .map(|(id, _rect)| id)
    })
}

fn root_inner_rect_in_global(ctx: &Context) -> Option<Rect> {
    ctx.input(|i| i.raw.viewports.get(&ViewportId::ROOT)?.inner_rect)
}

fn infer_detached_geometry(
    pane_rect_in_root: Option<Rect>,
    pointer_global_fallback: Option<Pos2>,
    root_inner_rect_global: Option<Rect>,
    default_size: Vec2,
) -> (Pos2, Vec2) {
    let size = pane_rect_in_root
        .map(|r| Vec2::new(r.width().max(200.0), r.height().max(120.0)))
        .unwrap_or(default_size);

    let pos = if let Some(pointer_global) = pointer_global_fallback {
        pointer_global - Vec2::new(20.0, 10.0)
    } else if let (Some(root_inner_rect), Some(pane_rect)) =
        (root_inner_rect_global, pane_rect_in_root)
    {
        root_inner_rect.min + pane_rect.min.to_vec2()
    } else {
        Pos2::new(64.0, 64.0)
    };

    (pos, size)
}
