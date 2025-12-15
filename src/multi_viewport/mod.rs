use std::collections::{BTreeMap, VecDeque};

use egui::{Context, LayerId, Order, Pos2, Rect, ViewportClass, ViewportId};
use egui_tiles::{Behavior, ContainerKind, InsertionPoint, Tile, TileId, Tree};

mod options;
mod types;
mod overlay;
mod geometry;
mod title;
mod drop;
mod floating;
mod ghost;

pub use options::DockingMultiViewportOptions;

use types::*;
use overlay::{
    outer_overlay_for_dock_rect, overlay_for_tree_at_pointer,
    paint_outer_overlay, paint_overlay, pointer_in_outer_band,
};
use geometry::{
    pointer_pos_in_global, pointer_pos_in_viewport_space,
};
use title::title_for_detached_tree;

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
    pending_internal_drop: Option<PendingInternalDrop>,
    pending_local_drop: Option<PendingLocalDrop>,

    floating: BTreeMap<ViewportId, FloatingManager<Pane>>,
    next_floating_serial: u64,
    last_floating_rects: BTreeMap<(ViewportId, FloatingId), Rect>,

    ghost: Option<GhostDrag>,

    debug_log: VecDeque<String>,
    debug_frame: u64,
    debug_last_disable_drop_apply: BTreeMap<(u64, ViewportId), bool>,
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
            pending_internal_drop: None,
            pending_local_drop: None,
            floating: BTreeMap::new(),
            next_floating_serial: 1,
            last_floating_rects: BTreeMap::new(),
            ghost: None,
            debug_log: VecDeque::new(),
            debug_frame: 0,
            debug_last_disable_drop_apply: BTreeMap::new(),
        }
    }

    /// Total number of detached native viewports currently alive.
    pub fn detached_viewport_count(&self) -> usize {
        self.detached.len()
    }

    /// Total number of contained floating windows across all viewports.
    pub fn floating_window_count(&self) -> usize {
        self.floating.values().map(|m| m.windows.len()).sum()
    }

    /// Show detached viewports + the root dock in the current (root) viewport.
    ///
    /// Call this from your `eframe::App::update` (or equivalent).
    pub fn ui(&mut self, ctx: &Context, behavior: &mut dyn Behavior<Pane>) {
        self.debug_frame = self.debug_frame.wrapping_add(1);
        if self.options.debug_event_log {
            let clear_id = debug_clear_event_log_id(self.tree.id());
            let should_clear = ctx.data(|d| d.get_temp::<bool>(clear_id).unwrap_or(false));
            if should_clear {
                self.debug_log_clear();
                ctx.data_mut(|d| {
                    d.remove::<bool>(clear_id);
                });
            }
        }

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
            let internal_drop =
                if self.pending_drop.is_none() && self.pending_internal_drop.is_none() {
                    self.pending_internal_overlay_drop_on_release(
                        ui.ctx(),
                        dock_rect,
                        ViewportId::ROOT,
                        &self.tree,
                    )
                } else {
                    None
                };
            let took_over_internal_drop = internal_drop.is_some();
            if let Some(pending) = internal_drop {
                self.debug_log_event(format!(
                    "queue_internal_drop viewport={:?} tile_id={:?} insertion={:?}",
                    pending.viewport, pending.tile_id, pending.insertion
                ));
                ui.ctx().stop_dragging();
                if let Some(payload) = egui::DragAndDrop::payload::<DockPayload>(ui.ctx()) {
                    if payload.bridge_id == self.tree.id()
                        && payload.source_viewport == pending.viewport
                    {
                        egui::DragAndDrop::clear_payload(ui.ctx());
                    }
                }

                self.pending_internal_drop = Some(pending);
                ui.ctx().request_repaint_of(ViewportId::ROOT);
            }

            self.set_tiles_disable_drop_apply_if_taken_over(
                ui.ctx(),
                self.tree.id(),
                ViewportId::ROOT,
                took_over_internal_drop,
            );
            self.set_tiles_disable_drop_preview_if_overlay_hovered(
                ui.ctx(),
                dock_rect,
                ViewportId::ROOT,
                &self.tree,
            );

            if let Some(dragged_tile) = self.tree.dragged_id_including_root(ui.ctx()) {
                self.queue_pending_local_drop_from_dragged_tile_on_release(
                    ui.ctx(),
                    dock_rect,
                    ViewportId::ROOT,
                    None,
                    dragged_tile,
                );
            }

            // Tear-off detection must happen before `tree.ui`, otherwise egui_tiles will interpret
            // every drop as "somewhere" inside the tree.
            self.try_tear_off_from_root(ui.ctx(), behavior, dock_rect);

            self.maybe_start_ghost_from_root(ui.ctx(), behavior, dock_rect);

            self.set_tiles_debug_visit_enabled(ui.ctx(), self.tree.id(), ViewportId::ROOT);
            self.tree.ui(behavior, ui);

            self.set_payload_from_root_drag_if_any(ui.ctx());
            self.paint_drop_preview_if_any_for_tree(
                ui,
                behavior,
                &self.tree,
                dock_rect,
                ViewportId::ROOT,
            );

            self.ui_floating_windows_in_viewport(ui, behavior, dock_rect, ViewportId::ROOT);

            self.queue_pending_local_drop_on_release(ui.ctx(), dock_rect, ViewportId::ROOT);

            self.clear_bridge_payload_if_released_in_ctx(ui.ctx());
        });

        if self.options.debug_drop_targets || self.options.debug_event_log {
            self.ui_debug_window(ctx, ViewportId::ROOT, self.tree.id());
        }

        // Apply after all viewports have had a chance to run `tree.ui` this frame so we can use
        // the computed rectangles for accurate docking.
        self.apply_pending_drop(ctx, behavior);
        self.apply_pending_internal_drop(behavior);
        self.apply_pending_local_drop(ctx, behavior);
        self.clear_bridge_payload_on_release(ctx);
        self.finish_ghost_if_released_or_aborted(ctx);
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

                if let Some(GhostDrag {
                    mode: GhostDragMode::Native { viewport },
                    grab_offset,
                }) = self.ghost
                {
                    if viewport == viewport_id {
                        if let Some(pointer_global) = self.last_pointer_global {
                            ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(
                                pointer_global - grab_offset,
                            ));
                        }
                    }
                }

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
                                    source_floating: None,
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
                    let internal_drop =
                        if self.pending_drop.is_none() && self.pending_internal_drop.is_none() {
                            self.pending_internal_overlay_drop_on_release(
                                ctx,
                                dock_rect,
                                viewport_id,
                                &detached.tree,
                            )
                        } else {
                            None
                        };
                    let took_over_internal_drop = internal_drop.is_some();
                    if let Some(pending) = internal_drop {
                        self.debug_log_event(format!(
                            "queue_internal_drop viewport={:?} tile_id={:?} insertion={:?}",
                            pending.viewport, pending.tile_id, pending.insertion
                        ));
                        ctx.stop_dragging();
                        if let Some(payload) = egui::DragAndDrop::payload::<DockPayload>(ctx) {
                            if payload.bridge_id == self.tree.id()
                                && payload.source_viewport == pending.viewport
                            {
                                egui::DragAndDrop::clear_payload(ctx);
                            }
                        }

                        self.pending_internal_drop = Some(pending);
                        ctx.request_repaint_of(ViewportId::ROOT);
                    }

                    self.set_tiles_disable_drop_apply_if_taken_over(
                        ctx,
                        detached.tree.id(),
                        viewport_id,
                        took_over_internal_drop,
                    );
                    self.set_tiles_disable_drop_preview_if_overlay_hovered(
                        ctx,
                        dock_rect,
                        viewport_id,
                        &detached.tree,
                    );

                    if let Some(dragged_tile) = detached.tree.dragged_id_including_root(ctx) {
                        self.queue_pending_local_drop_from_dragged_tile_on_release(
                            ctx,
                            dock_rect,
                            viewport_id,
                            None,
                            dragged_tile,
                        );
                    }

                    let mut did_tear_off = false;
                    self.try_tear_off_from_detached(
                        ctx,
                        behavior,
                        dock_rect,
                        viewport_id,
                        &mut detached.tree,
                        &mut did_tear_off,
                    );

                    self.maybe_start_ghost_from_tree_in_viewport(
                        ctx,
                        behavior,
                        dock_rect,
                        viewport_id,
                        &mut detached.tree,
                    );

                    self.set_tiles_debug_visit_enabled(ctx, detached.tree.id(), viewport_id);
                    detached.tree.ui(behavior, ui);

                    if self.pending_drop.is_none()
                        && self.pending_local_drop.is_none()
                        && self.ghost.is_none()
                    {
                        if let Some(dragged_tile) = detached.tree.dragged_id_including_root(ctx) {
                            egui::DragAndDrop::set_payload(
                                ctx,
                                DockPayload {
                                    bridge_id,
                                    source_viewport: viewport_id,
                                    source_floating: None,
                                    tile_id: Some(dragged_tile),
                                },
                            );
                            ctx.request_repaint_of(ViewportId::ROOT);
                        }
                    }

                    self.paint_drop_preview_if_any_for_tree(
                        ui,
                        behavior,
                        &detached.tree,
                        dock_rect,
                        viewport_id,
                    );

                    self.ui_floating_windows_in_viewport(ui, behavior, dock_rect, viewport_id);

                    self.queue_pending_local_drop_on_release(ctx, dock_rect, viewport_id);
                    self.clear_bridge_payload_if_released_in_ctx(ctx);

                    if self.options.debug_drop_targets || self.options.debug_event_log {
                        self.ui_debug_window(ctx, viewport_id, detached.tree.id());
                    }
                    if did_tear_off {
                        ctx.request_repaint_of(ViewportId::ROOT);
                    }
                });

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

    fn allocate_floating_id(&mut self) -> FloatingId {
        let serial = self.next_floating_serial;
        self.next_floating_serial = self.next_floating_serial.saturating_add(1);
        serial
    }


    fn clear_bridge_payload_if_released_in_ctx(&self, ctx: &Context) {
        if !ctx.input(|i| i.pointer.any_released()) {
            return;
        }
        let Some(payload) = egui::DragAndDrop::payload::<DockPayload>(ctx) else {
            return;
        };
        if payload.bridge_id != self.tree.id() {
            return;
        }
        egui::DragAndDrop::clear_payload(ctx);
    }

    fn clear_bridge_payload_on_release(&self, ctx: &Context) {
        self.clear_bridge_payload_if_released_in_ctx(ctx);
    }

    fn set_tiles_debug_visit_enabled(
        &self,
        ctx: &Context,
        tree_id: egui::Id,
        viewport_id: ViewportId,
    ) {
        if !(self.options.debug_drop_targets || self.options.debug_event_log) {
            return;
        }
        ctx.data_mut(|d| {
            d.insert_temp(
                tiles_debug_visit_enabled_id(tree_id, viewport_id),
                true,
            );
        });
    }

    fn debug_log_event(&mut self, message: impl Into<String>) {
        if !self.options.debug_event_log {
            return;
        }
        let cap = self.options.debug_event_log_capacity.max(1).min(10_000);
        while self.debug_log.len() >= cap {
            self.debug_log.pop_front();
        }
        self.debug_log
            .push_back(format!("[frame {}] {}", self.debug_frame, message.into()));
    }

    fn debug_log_clear(&mut self) {
        self.debug_log.clear();
    }

    fn debug_log_text(&self) -> String {
        self.debug_log.iter().cloned().collect::<Vec<_>>().join("\n")
    }

    fn ui_debug_window(&self, ctx: &Context, viewport_id: ViewportId, tree_id: egui::Id) {
        let last_drop_debug =
            ctx.data(|d| d.get_temp::<String>(last_drop_debug_text_id(tree_id, viewport_id)));
        let tiles_last_ui = ctx.data(|d| d.get_temp::<String>(tiles_debug_visit_last_id(tree_id, viewport_id)));
        let log_text = self.debug_log_text();

        egui::Window::new("Dock Debug")
            .id(egui::Id::new((tree_id, viewport_id, "egui_docking_debug_window")))
            .default_pos(egui::Pos2::new(12.0, 12.0))
            .resizable(true)
            .show(ctx, |ui| {
                ui.label("Shortcuts: Cmd/Ctrl+Shift+D 复制 drop debug；Cmd/Ctrl+Shift+L 复制 event log。");
                ui.separator();

                ui.horizontal(|ui| {
                    if ui.button("Copy drop debug").clicked() {
                        if let Some(text) = &last_drop_debug {
                            ctx.copy_text(text.clone());
                        } else {
                            ctx.copy_text("(no drop debug captured yet)".to_owned());
                        }
                    }
                    if ui.button("Copy tiles ui").clicked() {
                        if let Some(text) = &tiles_last_ui {
                            ctx.copy_text(text.clone());
                        } else {
                            ctx.copy_text("(no tiles ui captured yet)".to_owned());
                        }
                    }
                    if ui.button("Copy event log").clicked() {
                        ctx.copy_text(log_text.clone());
                    }
                    if ui.button("Clear event log").clicked() {
                        ctx.data_mut(|d| {
                            d.insert_temp(debug_clear_event_log_id(self.tree.id()), true);
                        });
                    }
                });

                if let Some(text) = last_drop_debug {
                    ui.separator();
                    egui::ScrollArea::vertical()
                        .id_salt("drop_debug")
                        .max_height(240.0)
                        .show(ui, |ui| {
                        ui.label(text);
                    });
                }

                if let Some(text) = tiles_last_ui {
                    ui.separator();
                    egui::ScrollArea::vertical()
                        .id_salt("tiles_ui")
                        .max_height(240.0)
                        .show(ui, |ui| {
                        ui.label(text);
                    });
                }

                if self.options.debug_event_log {
                    ui.separator();
                    egui::ScrollArea::vertical()
                        .id_salt("event_log")
                        .max_height(240.0)
                        .show(ui, |ui| {
                        ui.label(log_text);
                    });
                }
            });
    }

    fn set_tiles_disable_drop_apply_if_taken_over(
        &mut self,
        ctx: &Context,
        tree_id: egui::Id,
        viewport_id: ViewportId,
        disable_drop_apply: bool,
    ) {
        if self.options.debug_event_log {
            let key = (tree_id.value(), viewport_id);
            let prev = self.debug_last_disable_drop_apply.insert(key, disable_drop_apply);
            if prev != Some(disable_drop_apply) {
                self.debug_log_event(format!(
                    "tiles_disable_drop_apply viewport={viewport_id:?} tree={:04X} -> {disable_drop_apply}",
                    tree_id.value() as u16
                ));
            }
        }
        ctx.data_mut(|d| {
            d.insert_temp(
                tiles_disable_drop_apply_id(tree_id, viewport_id),
                disable_drop_apply,
            );
        });
    }

    fn set_tiles_disable_drop_preview_if_overlay_hovered(
        &self,
        ctx: &Context,
        dock_rect: Rect,
        viewport_id: ViewportId,
        tree: &Tree<Pane>,
    ) {
        let disable_preview = self.options.show_overlay_for_internal_drags
            && tree.dragged_id_including_root(ctx).is_some()
            && !(self.options.detach_on_alt_release_anywhere && ctx.input(|i| i.modifiers.alt))
            && ctx
                .input(|i| i.pointer.latest_pos())
                .is_some_and(|pointer_local| {
                    if !dock_rect.contains(pointer_local) {
                        return false;
                    }

                    if let Some(floating_tree_id) =
                        self.floating_tree_id_under_pointer(viewport_id, pointer_local)
                    {
                        if floating_tree_id != tree.id() {
                            return false;
                        }
                    }

                    let outer_mode =
                        self.options.show_outer_overlay_targets && pointer_in_outer_band(dock_rect, pointer_local);

                    if outer_mode {
                        outer_overlay_for_dock_rect(dock_rect, pointer_local)
                            .is_some_and(|o| o.is_hovered())
                    } else {
                        overlay_for_tree_at_pointer(tree, pointer_local)
                            .is_some_and(|o| o.is_hovered())
                    }
                });

        ctx.data_mut(|d| {
            d.insert_temp(
                tiles_disable_drop_preview_id(tree.id(), viewport_id),
                disable_preview,
            );
        });
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
        let is_cross_viewport = payload.source_viewport != target_viewport;
        if !is_cross_viewport && !self.options.show_overlay_for_internal_drags {
            return;
        }

        let is_fresh = if let Some(floating_id) = payload.source_floating {
            self.floating
                .get(&payload.source_viewport)
                .and_then(|m| m.windows.get(&floating_id))
                .is_some_and(|w| {
                    payload
                        .tile_id
                        .map(|tile_id| w.tree.tiles.get(tile_id).is_some())
                        .unwrap_or(true)
                })
        } else if payload.source_viewport == ViewportId::ROOT {
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
        if self
            .floating_tree_id_under_pointer(target_viewport, pointer_local)
            .is_some_and(|floating_tree_id| floating_tree_id != tree.id())
        {
            return;
        }

        let outer_mode =
            self.options.show_outer_overlay_targets && pointer_in_outer_band(dock_rect, pointer_local);
        if outer_mode {
            if let Some(overlay) = outer_overlay_for_dock_rect(dock_rect, pointer_local) {
                let painter = ui.ctx().layer_painter(LayerId::new(
                    Order::Foreground,
                    egui::Id::new((tree.id(), target_viewport, "egui_docking_outer_overlay")),
                ));
                paint_outer_overlay(&painter, ui.visuals(), overlay);
            }
        } else if let Some(overlay) = overlay_for_tree_at_pointer(tree, pointer_local) {
            let painter = ui.ctx().layer_painter(LayerId::new(
                Order::Foreground,
                egui::Id::new((tree.id(), target_viewport, "egui_docking_overlay")),
            ));
            paint_overlay(&painter, ui.visuals(), overlay);
        } else if let Some(zone) = tree.dock_zone_at(behavior, ui.style(), pointer_local) {
            let stroke = ui.visuals().selection.stroke;
            let fill = stroke.color.gamma_multiply(0.25);
            ui.painter().rect(
                zone.preview_rect,
                1.0,
                fill,
                stroke,
                egui::StrokeKind::Inside,
            );
        }

        if self.options.debug_drop_targets {
            let mut lines: Vec<String> = Vec::new();
            lines.push(format!("viewport={target_viewport:?}"));
            lines.push(format!("tree_root={:?}", tree.root));
            lines.push(format!(
                "dragged_id={:?}",
                tree.dragged_id_including_root(ui.ctx())
            ));
            lines.push(format!("payload_tile_id={:?}", payload.tile_id));
            lines.push(format!(
                "pointer_local=({:.1},{:.1}) outer_mode={outer_mode}",
                pointer_local.x, pointer_local.y
            ));
            let tiles_disable_drop_preview = ui.ctx().data(|d| {
                d.get_temp::<bool>(tiles_disable_drop_preview_id(tree.id(), target_viewport))
                    .unwrap_or(false)
            });
            let tiles_disable_drop_apply = ui.ctx().data(|d| {
                d.get_temp::<bool>(tiles_disable_drop_apply_id(tree.id(), target_viewport))
                    .unwrap_or(false)
            });
            lines.push(format!(
                "tiles_disable_drop_preview={tiles_disable_drop_preview} tiles_disable_drop_apply={tiles_disable_drop_apply}"
            ));
            let tiles_last_release = ui.ctx().data(|d| {
                d.get_temp::<String>(egui::Id::new((
                    tree.id(),
                    "egui_docking_last_drop_release_debug",
                )))
            });
            if let Some(s) = tiles_last_release.as_deref() {
                lines.push(s.to_owned());
            }

            if outer_mode {
                if let Some(overlay) = outer_overlay_for_dock_rect(dock_rect, pointer_local) {
                    lines.push(format!(
                        "outer_hover={:?}",
                        overlay.hovered_target()
                    ));
                    lines.push(format!(
                        "outer_insertion={:?}",
                        overlay::overlay_insertion_for_tree_with_outer(tree, dock_rect, pointer_local)
                    ));
                } else {
                    lines.push("outer_hover=None".to_owned());
                }
            } else if let Some(overlay) = overlay_for_tree_at_pointer(tree, pointer_local) {
                lines.push(format!("inner_hover={:?}", overlay.hovered_target()));
                lines.push(format!(
                    "inner_insertion={:?}",
                    overlay::overlay_insertion_for_tree_with_outer(tree, dock_rect, pointer_local)
                ));
            } else if let Some(zone) = tree.dock_zone_at(behavior, ui.style(), pointer_local) {
                lines.push("inner_hover=None".to_owned());
                lines.push(format!("tiles_zone_insertion={:?}", zone.insertion_point));
            } else {
                lines.push("no_target".to_owned());
            }

            let debug_text = lines.join("\n");
            let log_text = self.debug_log_text();
            let ctx = ui.ctx();
            ctx.data_mut(|d| {
                d.insert_temp(
                    last_drop_debug_text_id(tree.id(), target_viewport),
                    debug_text.clone(),
                );
            });

            let (copy_drop_debug, copy_event_log) = ctx.input(|i| {
                let primary = i.modifiers.command || i.modifiers.ctrl;
                let shift = i.modifiers.shift;
                (
                    primary && shift && i.key_pressed(egui::Key::D),
                    primary && shift && i.key_pressed(egui::Key::L),
                )
            });
            if copy_drop_debug {
                ctx.copy_text(debug_text.clone());
            }
            if copy_event_log {
                ctx.copy_text(log_text.clone());
            }

            let debug_id = egui::Id::new((tree.id(), target_viewport, "egui_docking_debug_drop_targets"));
            egui::Area::new(debug_id)
                .order(Order::Foreground)
                .fixed_pos(dock_rect.left_top() + egui::Vec2::new(8.0, 8.0))
                .interactable(false)
                .show(ctx, |ui| {
                    ui.set_clip_rect(ui.clip_rect().intersect(dock_rect));
                    egui::Frame::popup(ui.style()).show(ui, |ui| {
                        ui.set_max_width((dock_rect.width() * 0.75).max(240.0));
                        if self.options.debug_event_log {
                            ui.label(
                                "提示：拖拽中无法点击按钮；请用快捷键 Cmd/Ctrl+Shift+D/L 复制，或松手后在 Dock Debug 窗口点击按钮。",
                            );
                            ui.separator();
                        }

                        ui.label(debug_text);

                        if self.options.debug_event_log {
                            ui.separator();
                            egui::ScrollArea::vertical()
                                .max_height(180.0)
                                .show(ui, |ui| {
                                    ui.label(log_text);
                                });
                        }
                    });
                });
        }

        ui.ctx().request_repaint();
    }

    fn pick_detach_tile(&self, ctx: &Context, dragged_tile: TileId) -> TileId {
        pick_detach_tile_for_tree(ctx, &self.options, &self.tree, dragged_tile)
    }

    // `paint_root_drop_preview_if_any` replaced by `paint_drop_preview_if_any_for_tree`.
}

fn pick_detach_tile_for_tree<Pane>(
    ctx: &Context,
    options: &DockingMultiViewportOptions,
    tree: &Tree<Pane>,
    dragged_tile: TileId,
) -> TileId {
    if !options.detach_parent_tabs_on_shift {
        return dragged_tile;
    }

    let shift = ctx.input(|i| i.modifiers.shift);
    if !shift {
        return dragged_tile;
    }

    if !matches!(tree.tiles.get(dragged_tile), Some(Tile::Pane(_))) {
        return dragged_tile;
    }

    let Some(parent) = tree.tiles.parent_of(dragged_tile) else {
        return dragged_tile;
    };

    let parent_kind = tree.tiles.get(parent).and_then(|t| t.kind());
    if parent_kind == Some(ContainerKind::Tabs) {
        parent
    } else {
        dragged_tile
    }
}


fn tiles_disable_drop_preview_id(tree_id: egui::Id, _viewport_id: ViewportId) -> egui::Id {
    egui::Id::new((tree_id, "egui_docking_disable_drop_preview"))
}

fn tiles_disable_drop_apply_id(tree_id: egui::Id, _viewport_id: ViewportId) -> egui::Id {
    egui::Id::new((tree_id, "egui_docking_disable_drop_apply"))
}

fn debug_clear_event_log_id(bridge_id: egui::Id) -> egui::Id {
    egui::Id::new((bridge_id, "egui_docking_clear_event_log"))
}

fn last_drop_debug_text_id(tree_id: egui::Id, viewport_id: ViewportId) -> egui::Id {
    egui::Id::new((tree_id, viewport_id, "egui_docking_last_drop_debug_text"))
}

fn tiles_debug_visit_enabled_id(tree_id: egui::Id, viewport_id: ViewportId) -> egui::Id {
    egui::Id::new((tree_id, viewport_id, "egui_docking_debug_visit_enabled"))
}

fn tiles_debug_visit_last_id(tree_id: egui::Id, viewport_id: ViewportId) -> egui::Id {
    egui::Id::new((tree_id, viewport_id, "egui_docking_debug_visit_last"))
}
