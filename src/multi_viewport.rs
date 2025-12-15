use std::collections::BTreeMap;

use egui::{Context, LayerId, Order, Pos2, Rect, Vec2, ViewportBuilder, ViewportClass, ViewportId};
use egui_tiles::{Behavior, ContainerKind, InsertionPoint, Tile, TileId, Tree};

/// Options for [`DockingMultiViewport`].
#[derive(Clone, Debug)]
pub struct DockingMultiViewportOptions {
    /// Fallback inner size (in points) when we can't infer a better size for a torn-off pane.
    pub default_detached_inner_size: Vec2,

    /// If true, holding SHIFT while tearing off a pane will instead tear off the closest parent `Tabs` container,
    /// preserving the whole tab-group (dear imgui style "dock node tear-off").
    pub detach_parent_tabs_on_shift: bool,

    /// If true, holding ALT while releasing a drag will force a tear-off into a new native viewport,
    /// even if the cursor is still inside the dock area.
    pub detach_on_alt_release_anywhere: bool,

    /// If true, show ImGui-style docking overlay targets even for drags that stay within the same viewport.
    pub show_overlay_for_internal_drags: bool,

    /// If true, show ImGui-style *outer* docking targets (dockspace edge markers),
    /// allowing quick splits at the dockspace boundary (dear imgui style outer docking).
    pub show_outer_overlay_targets: bool,

    /// If true, holding CTRL while tearing off will create a contained floating window (within the current viewport)
    /// instead of a native viewport window.
    pub tear_off_to_floating_on_ctrl: bool,

    /// If true, dragging a tab/pane outside the dock area will immediately create a "ghost" floating window
    /// that follows the pointer, and can be docked back before releasing (dear imgui style).
    pub ghost_tear_off: bool,

    /// Pointer distance (in points) outside the dock area required to trigger ghost tear-off.
    pub ghost_tear_off_threshold: f32,

    /// If true, a contained ghost window will be upgraded to a native viewport once the pointer leaves
    /// the source viewport's inner rectangle.
    pub ghost_upgrade_to_native_on_leave_viewport: bool,
}

impl Default for DockingMultiViewportOptions {
    fn default() -> Self {
        Self {
            default_detached_inner_size: Vec2::new(480.0, 360.0),
            detach_parent_tabs_on_shift: true,
            detach_on_alt_release_anywhere: true,
            show_overlay_for_internal_drags: true,
            show_outer_overlay_targets: true,
            tear_off_to_floating_on_ctrl: true,
            ghost_tear_off: true,
            ghost_tear_off_threshold: 8.0,
            ghost_upgrade_to_native_on_leave_viewport: true,
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct DockPayload {
    bridge_id: egui::Id,
    source_viewport: ViewportId,
    source_floating: Option<u64>,
    tile_id: Option<TileId>,
}

#[derive(Clone, Copy, Debug)]
struct PendingDrop {
    payload: DockPayload,
    pointer_global: Pos2,
}

#[derive(Clone, Copy, Debug)]
struct PendingInternalDrop {
    viewport: ViewportId,
    tile_id: TileId,
    insertion: InsertionPoint,
}

type FloatingId = u64;

#[derive(Clone, Copy, Debug)]
struct PendingLocalDrop {
    payload: DockPayload,
    target_viewport: ViewportId,
    target_floating: Option<FloatingId>,
    pointer_local: Pos2,
}

#[derive(Clone, Copy, Debug)]
enum GhostDragMode {
    Contained {
        viewport: ViewportId,
        floating: FloatingId,
    },
    Native {
        viewport: ViewportId,
    },
}

#[derive(Clone, Copy, Debug)]
struct GhostDrag {
    mode: GhostDragMode,
    grab_offset: Vec2,
}

#[derive(Debug)]
struct DetachedDock<Pane> {
    tree: Tree<Pane>,
    builder: ViewportBuilder,
}

#[derive(Clone, Copy, Debug)]
struct FloatingDragState {
    pointer_start: Pos2,
    offset_start: Vec2,
}

#[derive(Clone, Copy, Debug)]
struct FloatingResizeState {
    pointer_start: Pos2,
    size_start: Vec2,
}

#[derive(Debug)]
struct FloatingDockWindow<Pane> {
    tree: Tree<Pane>,
    offset_in_dock: Vec2,
    size: Vec2,
    collapsed: bool,
    drag: Option<FloatingDragState>,
    resize: Option<FloatingResizeState>,
}

#[derive(Debug)]
struct FloatingManager<Pane> {
    windows: BTreeMap<FloatingId, FloatingDockWindow<Pane>>,
    z_order: Vec<FloatingId>,
}

impl<Pane> Default for FloatingManager<Pane> {
    fn default() -> Self {
        Self {
            windows: BTreeMap::new(),
            z_order: Vec::new(),
        }
    }
}

impl<Pane> FloatingManager<Pane> {
    fn bring_to_front(&mut self, id: FloatingId) {
        self.z_order.retain(|&x| x != id);
        self.z_order.push(id);
    }
}

#[derive(Clone, Copy, Debug)]
enum DropAction {
    MoveSubtree {
        source_viewport: ViewportId,
        source_floating: Option<FloatingId>,
        tile_id: TileId,
        insertion: Option<InsertionPoint>,
    },
    MoveWholeTree {
        source_viewport: ViewportId,
        source_floating: Option<FloatingId>,
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
    pending_internal_drop: Option<PendingInternalDrop>,
    pending_local_drop: Option<PendingLocalDrop>,

    floating: BTreeMap<ViewportId, FloatingManager<Pane>>,
    next_floating_serial: u64,
    last_floating_rects: BTreeMap<(ViewportId, FloatingId), Rect>,

    ghost: Option<GhostDrag>,
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
            if let Some(pending) = internal_drop {
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
                    if let Some(pending) = internal_drop {
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

    fn set_payload_from_root_drag_if_any(&mut self, ctx: &Context) {
        if self.pending_drop.is_some() || self.pending_local_drop.is_some() {
            return;
        }
        if self.ghost.is_some() {
            return;
        }

        let bridge_id = self.tree.id();

        if let Some(dragged_tile) = self.tree.dragged_id_including_root(ctx) {
            egui::DragAndDrop::set_payload(
                ctx,
                DockPayload {
                    bridge_id,
                    source_viewport: ViewportId::ROOT,
                    source_floating: None,
                    tile_id: Some(dragged_tile),
                },
            );
            return;
        }
    }

    fn try_tear_off_from_root(
        &mut self,
        ctx: &Context,
        behavior: &mut dyn Behavior<Pane>,
        dock_rect: Rect,
    ) {
        if self.pending_drop.is_some()
            || self.pending_internal_drop.is_some()
            || self.pending_local_drop.is_some()
        {
            return;
        }

        let did_release = ctx.input(|i| i.pointer.any_released());
        if !did_release {
            return;
        }

        let Some(dragged_tile) = self.tree.dragged_id_including_root(ctx) else {
            return;
        };

        let detach_tile = self.pick_detach_tile(ctx, dragged_tile);

        let force_detach =
            self.options.detach_on_alt_release_anywhere && ctx.input(|i| i.modifiers.alt);
        if !force_detach {
            let pointer_pos = ctx.input(|i| i.pointer.latest_pos());
            let dropped_inside_any_surface = pointer_pos.is_some_and(|p| {
                dock_rect.contains(p) || self.floating_under_pointer(ViewportId::ROOT, p).is_some()
            });
            if dropped_inside_any_surface {
                return;
            }
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

        let ctrl_floating =
            self.options.tear_off_to_floating_on_ctrl && ctx.input(|i| i.modifiers.ctrl);
        if ctrl_floating {
            self.spawn_floating_subtree_in_viewport(
                ctx,
                ViewportId::ROOT,
                dock_rect,
                title,
                subtree,
                pane_rect_last,
                size,
            );
            return;
        }

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
        if self.pending_drop.is_some()
            || self.pending_internal_drop.is_some()
            || self.pending_local_drop.is_some()
        {
            return;
        }

        let did_release = ctx.input(|i| i.pointer.any_released());
        if !did_release {
            return;
        }

        let Some(dragged_tile) = tree.dragged_id_including_root(ctx) else {
            return;
        };

        let force_detach =
            self.options.detach_on_alt_release_anywhere && ctx.input(|i| i.modifiers.alt);
        if !force_detach {
            let pointer_pos = ctx.input(|i| i.pointer.latest_pos());
            let dropped_inside_any_surface = pointer_pos.is_some_and(|p| {
                dock_rect.contains(p) || self.floating_under_pointer(current_viewport, p).is_some()
            });
            if dropped_inside_any_surface {
                return;
            }
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

        let ctrl_floating =
            self.options.tear_off_to_floating_on_ctrl && ctx.input(|i| i.modifiers.ctrl);
        if ctrl_floating {
            self.spawn_floating_subtree_in_viewport(
                ctx,
                current_viewport,
                dock_rect,
                title,
                subtree,
                pane_rect_last,
                size,
            );
            *did_tear_off = true;
            ctx.request_repaint();
            ctx.request_repaint_of(ViewportId::ROOT);
            return;
        }

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

    fn allocate_floating_id(&mut self) -> FloatingId {
        let serial = self.next_floating_serial;
        self.next_floating_serial = self.next_floating_serial.saturating_add(1);
        serial
    }

    fn maybe_start_ghost_from_root(
        &mut self,
        ctx: &Context,
        behavior: &mut dyn Behavior<Pane>,
        dock_rect: Rect,
    ) {
        if !self.options.ghost_tear_off {
            return;
        }
        if self.ghost.is_some()
            || self.pending_drop.is_some()
            || self.pending_internal_drop.is_some()
            || self.pending_local_drop.is_some()
        {
            return;
        }
        if ctx.input(|i| i.pointer.any_released()) {
            return;
        }

        let Some(pointer_local) = ctx.input(|i| i.pointer.latest_pos()) else {
            return;
        };

        if dock_rect
            .expand(self.options.ghost_tear_off_threshold)
            .contains(pointer_local)
        {
            return;
        }
        let viewport_id = ViewportId::ROOT;
        if self.floating_under_pointer(viewport_id, pointer_local).is_some() {
            return;
        }

        let tree = &mut self.tree;

        let Some(dragged_tile) = tree.dragged_id_including_root(ctx) else {
            return;
        };
        let detach_tile = pick_detach_tile_for_tree(ctx, &self.options, tree, dragged_tile);

        // Transfer authority away from egui_tiles internal drag-drop as soon as we switch
        // to a cross-surface "ghost" payload.
        ctx.stop_dragging();

        let pane_rect_last = tree.tiles.rect(detach_tile);
        let Some(subtree) = tree.extract_subtree(detach_tile) else {
            return;
        };

        let size = pane_rect_last
            .map(|r| Vec2::new(r.width().max(220.0), r.height().max(120.0)))
            .unwrap_or(self.options.default_detached_inner_size);

        let grab_offset = Vec2::new(20.0, 10.0);
        let mut offset_in_dock = (pointer_local - dock_rect.min) - grab_offset;
        offset_in_dock.x = offset_in_dock
            .x
            .clamp(0.0, (dock_rect.width() - size.x).max(0.0));
        offset_in_dock.y = offset_in_dock
            .y
            .clamp(0.0, (dock_rect.height() - size.y).max(0.0));

        // Title derived from the tree each frame; keep it for future customization.
        let _title = title_for_detached_subtree(&subtree, behavior);

        let floating_id = self.allocate_floating_id();
        let floating_tree_id =
            egui::Id::new((self.tree.id(), viewport_id, "egui_docking_floating_tree", floating_id));
        let floating_tree = Tree::new(floating_tree_id, subtree.root, subtree.tiles);

        let mut manager = self.floating.remove(&viewport_id).unwrap_or_default();
        manager.windows.insert(
            floating_id,
            FloatingDockWindow {
                tree: floating_tree,
                offset_in_dock,
                size,
                collapsed: false,
                drag: None,
                resize: None,
            },
        );
        manager.bring_to_front(floating_id);
        self.floating.insert(viewport_id, manager);

        // Use a "whole tree" payload while dragging the ghost surface around.
        egui::DragAndDrop::set_payload(
            ctx,
            DockPayload {
                bridge_id: self.tree.id(),
                source_viewport: viewport_id,
                source_floating: Some(floating_id),
                tile_id: None,
            },
        );

        ctx.request_repaint_of(ViewportId::ROOT);
        self.ghost = Some(GhostDrag {
            mode: GhostDragMode::Contained {
                viewport: viewport_id,
                floating: floating_id,
            },
            grab_offset,
        });
    }

    fn maybe_start_ghost_from_tree_in_viewport(
        &mut self,
        ctx: &Context,
        behavior: &mut dyn Behavior<Pane>,
        dock_rect: Rect,
        viewport_id: ViewportId,
        tree: &mut Tree<Pane>,
    ) {
        if viewport_id == ViewportId::ROOT {
            // Root uses a specialized implementation to avoid borrowing conflicts.
            return;
        }

        if !self.options.ghost_tear_off {
            return;
        }
        if self.ghost.is_some()
            || self.pending_drop.is_some()
            || self.pending_internal_drop.is_some()
            || self.pending_local_drop.is_some()
        {
            return;
        }
        if ctx.input(|i| i.pointer.any_released()) {
            return;
        }

        let Some(pointer_local) = ctx.input(|i| i.pointer.latest_pos()) else {
            return;
        };

        if dock_rect
            .expand(self.options.ghost_tear_off_threshold)
            .contains(pointer_local)
        {
            return;
        }
        if self.floating_under_pointer(viewport_id, pointer_local).is_some() {
            return;
        }

        let Some(dragged_tile) = tree.dragged_id_including_root(ctx) else {
            return;
        };
        let detach_tile = pick_detach_tile_for_tree(ctx, &self.options, tree, dragged_tile);

        ctx.stop_dragging();

        let pane_rect_last = tree.tiles.rect(detach_tile);
        let Some(subtree) = tree.extract_subtree(detach_tile) else {
            return;
        };

        let size = pane_rect_last
            .map(|r| Vec2::new(r.width().max(220.0), r.height().max(120.0)))
            .unwrap_or(self.options.default_detached_inner_size);

        let grab_offset = Vec2::new(20.0, 10.0);
        let mut offset_in_dock = (pointer_local - dock_rect.min) - grab_offset;
        offset_in_dock.x = offset_in_dock
            .x
            .clamp(0.0, (dock_rect.width() - size.x).max(0.0));
        offset_in_dock.y = offset_in_dock
            .y
            .clamp(0.0, (dock_rect.height() - size.y).max(0.0));

        let _title = title_for_detached_subtree(&subtree, behavior);

        let floating_id = self.allocate_floating_id();
        let floating_tree_id = egui::Id::new((
            self.tree.id(),
            viewport_id,
            "egui_docking_floating_tree",
            floating_id,
        ));
        let floating_tree = Tree::new(floating_tree_id, subtree.root, subtree.tiles);

        let mut manager = self.floating.remove(&viewport_id).unwrap_or_default();
        manager.windows.insert(
            floating_id,
            FloatingDockWindow {
                tree: floating_tree,
                offset_in_dock,
                size,
                collapsed: false,
                drag: None,
                resize: None,
            },
        );
        manager.bring_to_front(floating_id);
        self.floating.insert(viewport_id, manager);

        egui::DragAndDrop::set_payload(
            ctx,
            DockPayload {
                bridge_id: self.tree.id(),
                source_viewport: viewport_id,
                source_floating: Some(floating_id),
                tile_id: None,
            },
        );

        ctx.request_repaint_of(ViewportId::ROOT);
        self.ghost = Some(GhostDrag {
            mode: GhostDragMode::Contained {
                viewport: viewport_id,
                floating: floating_id,
            },
            grab_offset,
        });
    }

    fn finish_ghost_if_released_or_aborted(&mut self, ctx: &Context) {
        let Some(ghost) = self.ghost else {
            return;
        };

        // If the payload is already gone (cleared by another viewport), stop tracking the ghost.
        if egui::DragAndDrop::payload::<DockPayload>(ctx).is_none() {
            self.ghost = None;
            return;
        }

        let abort = ctx.input(|i| i.key_pressed(egui::Key::Escape));
        if abort {
            self.ghost = None;
            match ghost.mode {
                GhostDragMode::Contained { viewport, floating } => {
                    if let Some(subtree) = self.take_whole_floating_tree(viewport, floating) {
                        self.dock_subtree_into_root(subtree, None);
                    }
                }
                GhostDragMode::Native { viewport } => {
                    if let Some(detached) = self.detached.remove(&viewport) {
                        self.dock_tree_into_root(detached.tree, None);
                        ctx.send_viewport_cmd_to(viewport, egui::ViewportCommand::Close);
                    }
                }
            }
            return;
        }

        if ctx.input(|i| i.pointer.any_released()) {
            self.ghost = None;
        }
    }

    fn spawn_floating_subtree_in_viewport(
        &mut self,
        ctx: &Context,
        viewport_id: ViewportId,
        dock_rect: Rect,
        title: String,
        subtree: egui_tiles::SubTree<Pane>,
        pane_rect_last: Option<Rect>,
        size_hint: Vec2,
    ) {
        let size = Vec2::new(
            size_hint.x.max(200.0).min(dock_rect.width().max(200.0)),
            size_hint.y.max(120.0).min(dock_rect.height().max(120.0)),
        );

        let pointer_local = ctx.input(|i| i.pointer.latest_pos());
        let pos = if let Some(pointer_local) = pointer_local {
            pointer_local - Vec2::new(20.0, 10.0)
        } else if let Some(pane_rect_last) = pane_rect_last {
            dock_rect.min + pane_rect_last.min.to_vec2()
        } else {
            dock_rect.min + Vec2::splat(32.0)
        };

        let mut offset_in_dock = pos - dock_rect.min;
        offset_in_dock.x = offset_in_dock
            .x
            .clamp(0.0, (dock_rect.width() - size.x).max(0.0));
        offset_in_dock.y = offset_in_dock
            .y
            .clamp(0.0, (dock_rect.height() - size.y).max(0.0));

        let floating_id = self.allocate_floating_id();
        let floating_tree_id =
            egui::Id::new((self.tree.id(), "egui_docking_floating_tree", floating_id));
        let floating_tree = Tree::new(floating_tree_id, subtree.root, subtree.tiles);

        let mut manager = self.floating.remove(&viewport_id).unwrap_or_default();
        manager.windows.insert(
            floating_id,
            FloatingDockWindow {
                tree: floating_tree,
                offset_in_dock,
                size,
                collapsed: false,
                drag: None,
                resize: None,
            },
        );
        manager.bring_to_front(floating_id);
        self.floating.insert(viewport_id, manager);

        let _ = title; // title currently derived from the tree each frame; keep the param for future customization.
        ctx.request_repaint_of(ViewportId::ROOT);
    }

    fn ui_floating_windows_in_viewport(
        &mut self,
        ui: &mut egui::Ui,
        behavior: &mut dyn Behavior<Pane>,
        dock_rect: Rect,
        viewport_id: ViewportId,
    ) {
        self.last_floating_rects
            .retain(|(vid, _fid), _| *vid != viewport_id);

        let mut manager = self.floating.remove(&viewport_id).unwrap_or_default();
        manager
            .z_order
            .retain(|id| manager.windows.contains_key(id));
        for id in manager.windows.keys().copied().collect::<Vec<_>>() {
            if !manager.z_order.contains(&id) {
                manager.z_order.push(id);
            }
        }

        if let Some(GhostDrag {
            mode:
                GhostDragMode::Contained {
                    viewport,
                    floating,
                },
            grab_offset,
        }) = self.ghost
        {
            if viewport == viewport_id {
                let ctx = ui.ctx();
                let pointer_global = self.last_pointer_global;

                let should_upgrade = self.options.ghost_upgrade_to_native_on_leave_viewport
                    && pointer_global.is_some()
                    && pointer_pos_in_viewport_space(ctx, pointer_global).is_none();

                if should_upgrade {
                    if let (Some(pointer_global), Some(mut window)) =
                        (pointer_global, manager.windows.remove(&floating))
                    {
                        manager.z_order.retain(|&id| id != floating);

                        if let Some(root) = window.tree.root.take() {
                            let (ghost_viewport_id, serial) = self.allocate_detached_viewport_id();
                            let title = title_for_detached_tree(&window.tree, behavior);
                            let builder = ViewportBuilder::default()
                                .with_title(title)
                                .with_position(pointer_global - grab_offset)
                                .with_inner_size(window.size);

                            let tiles = std::mem::take(&mut window.tree.tiles);

                            let detached_tree_id = egui::Id::new((
                                self.tree.id(),
                                "egui_docking_detached_tree",
                                serial,
                            ));
                            let detached_tree = Tree::new(detached_tree_id, root, tiles);

                            self.detached.insert(
                                ghost_viewport_id,
                                DetachedDock {
                                    tree: detached_tree,
                                    builder,
                                },
                            );

                            egui::DragAndDrop::set_payload(
                                ctx,
                                DockPayload {
                                    bridge_id: self.tree.id(),
                                    source_viewport: ghost_viewport_id,
                                    source_floating: None,
                                    tile_id: None,
                                },
                            );

                            self.ghost = Some(GhostDrag {
                                mode: GhostDragMode::Native {
                                    viewport: ghost_viewport_id,
                                },
                                grab_offset,
                            });
                            ctx.request_repaint_of(ViewportId::ROOT);
                        } else {
                            // Empty tree; drop the ghost.
                            self.ghost = None;
                        }
                    }
                } else if let Some(pointer_local) = ui.ctx().input(|i| i.pointer.latest_pos()) {
                    if let Some(window) = manager.windows.get_mut(&floating) {
                        window.offset_in_dock =
                            (pointer_local - dock_rect.min) - grab_offset;
                        manager.bring_to_front(floating);
                    }
                }
            }
        }

        let bridge_id = self.tree.id();

        let ids = manager.z_order.clone();
        let mut bring_to_front: Vec<FloatingId> = Vec::new();
        let mut close_windows: Vec<FloatingId> = Vec::new();
        let mut dock_windows: Vec<FloatingId> = Vec::new();
        let mut ghost_from_floating: Option<(FloatingId, TileId, Pos2)> = None;

        for floating_id in ids {
            let Some(window) = manager.windows.get_mut(&floating_id) else {
                continue;
            };

            let title = title_for_detached_tree(&window.tree, behavior);

            let title_height = 24.0;
            let min_size = Vec2::new(220.0, 120.0);
            window.size.x = window.size.x.max(min_size.x);
            window.size.y = window.size.y.max(min_size.y);

            let size = if window.collapsed {
                Vec2::new(window.size.x, title_height)
            } else {
                window.size
            };

            window.offset_in_dock.x = window
                .offset_in_dock
                .x
                .clamp(0.0, (dock_rect.width() - size.x).max(0.0));
            window.offset_in_dock.y = window
                .offset_in_dock
                .y
                .clamp(0.0, (dock_rect.height() - size.y).max(0.0));

            let rect = Rect::from_min_size(dock_rect.min + window.offset_in_dock, size);
            self.last_floating_rects
                .insert((viewport_id, floating_id), rect);

            let area_id = egui::Id::new((
                bridge_id,
                viewport_id,
                "egui_docking_floating_window",
                floating_id,
            ));
            let ctx = ui.ctx().clone();

            egui::Area::new(area_id)
                .order(Order::Foreground)
                .fixed_pos(rect.min)
                .interactable(true)
                .show(&ctx, |ui| {
                    ui.set_clip_rect(ui.clip_rect().intersect(dock_rect));

                    let (alloc_rect, alloc_resp) =
                        ui.allocate_exact_size(rect.size(), egui::Sense::hover());

                    let visuals = ui.visuals();
                    ui.painter()
                        .rect_filled(alloc_rect, 6.0, visuals.window_fill());
                    ui.painter().rect_stroke(
                        alloc_rect,
                        6.0,
                        visuals.widgets.noninteractive.bg_stroke,
                        egui::StrokeKind::Inside,
                    );

                    if alloc_resp.clicked() {
                        bring_to_front.push(floating_id);
                    }

                    let title_rect = Rect::from_min_size(
                        alloc_rect.min,
                        Vec2::new(alloc_rect.width(), title_height),
                    );

                    let title_resp = ui.interact(
                        title_rect,
                        ui.id().with((floating_id, "floating_title_bar")),
                        egui::Sense::click_and_drag(),
                    );

                    if title_resp.clicked() || title_resp.drag_started() {
                        bring_to_front.push(floating_id);
                    }

                    if title_resp.drag_started() {
                        if let Some(pointer_start) = ctx.input(|i| i.pointer.latest_pos()) {
                            window.drag = Some(FloatingDragState {
                                pointer_start,
                                offset_start: window.offset_in_dock,
                            });
                        }

                        egui::DragAndDrop::set_payload(
                            &ctx,
                            DockPayload {
                                bridge_id,
                                source_viewport: viewport_id,
                                source_floating: Some(floating_id),
                                tile_id: None,
                            },
                        );
                        ctx.request_repaint_of(ViewportId::ROOT);
                    }

                    if let Some(drag) = window.drag {
                        if let Some(pointer) = ctx.input(|i| i.pointer.latest_pos()) {
                            window.offset_in_dock =
                                drag.offset_start + (pointer - drag.pointer_start);
                        }
                        if ctx.input(|i| i.pointer.any_released()) {
                            window.drag = None;
                        }
                    }

                    {
                        let mut title_ui =
                            ui.new_child(egui::UiBuilder::new().max_rect(title_rect));
                        title_ui.style_mut().interaction.selectable_labels = false;
                        title_ui.horizontal(|ui| {
                            let collapse_label = if window.collapsed { "▸" } else { "▾" };
                            if ui.button(collapse_label).clicked() {
                                window.collapsed = !window.collapsed;
                            }

                            ui.label(title);

                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    if ui.button("✕").clicked() {
                                        close_windows.push(floating_id);
                                    }
                                    if ui.button("Dock").clicked() {
                                        dock_windows.push(floating_id);
                                    }
                                },
                            );
                        });
                    }

                    if !window.collapsed {
                        let resize_handle_size = 14.0;
                        let resize_rect = Rect::from_min_size(
                            alloc_rect.right_bottom() - Vec2::splat(resize_handle_size),
                            Vec2::splat(resize_handle_size),
                        );

                        let resize_resp = ui.interact(
                            resize_rect,
                            ui.id().with((floating_id, "floating_resize")),
                            egui::Sense::drag(),
                        );
                        if resize_resp.hovered() || resize_resp.dragged() {
                            ctx.set_cursor_icon(egui::CursorIcon::ResizeNwSe);
                        }
                        if resize_resp.drag_started() {
                            if let Some(pointer_start) = ctx.input(|i| i.pointer.latest_pos()) {
                                window.resize = Some(FloatingResizeState {
                                    pointer_start,
                                    size_start: window.size,
                                });
                            }
                        }
                        if let Some(resize) = window.resize {
                            if let Some(pointer) = ctx.input(|i| i.pointer.latest_pos()) {
                                let delta = pointer - resize.pointer_start;
                                window.size = (resize.size_start + delta).max(min_size);
                            }
                            if ctx.input(|i| i.pointer.any_released()) {
                                window.resize = None;
                            }
                        }

                        let content_rect =
                            Rect::from_min_max(title_rect.left_bottom(), alloc_rect.right_bottom());

                        if ghost_from_floating.is_none()
                            && self.options.ghost_tear_off
                            && self.ghost.is_none()
                            && self.pending_drop.is_none()
                            && self.pending_internal_drop.is_none()
                            && self.pending_local_drop.is_none()
                            && !ctx.input(|i| i.pointer.any_released())
                        {
                            if let (Some(pointer_local), Some(dragged_tile)) = (
                                ctx.input(|i| i.pointer.latest_pos()),
                                window.tree.dragged_id_including_root(&ctx),
                            ) {
                                if !alloc_rect
                                    .expand(self.options.ghost_tear_off_threshold)
                                    .contains(pointer_local)
                                {
                                    ghost_from_floating =
                                        Some((floating_id, dragged_tile, pointer_local));
                                    ctx.stop_dragging();
                                }
                            }
                        }

                        if let (Some(pointer_local), Some(dragged_tile)) = (
                            ctx.input(|i| i.pointer.latest_pos()),
                            window.tree.dragged_id_including_root(&ctx),
                        ) {
                            if !alloc_rect.contains(pointer_local) {
                                self.queue_pending_local_drop_from_dragged_tile_on_release(
                                    &ctx,
                                    dock_rect,
                                    viewport_id,
                                    Some(floating_id),
                                    dragged_tile,
                                );
                            }
                        }

                        self.set_tiles_disable_drop_preview_if_overlay_hovered(
                            &ctx,
                            content_rect,
                            viewport_id,
                            &window.tree,
                        );

                        {
                            let mut content_ui =
                                ui.new_child(egui::UiBuilder::new().max_rect(content_rect));
                            content_ui
                                .set_clip_rect(content_ui.clip_rect().intersect(content_rect));
                            window.tree.ui(behavior, &mut content_ui);
                        }

                        if self.pending_drop.is_none()
                            && self.pending_local_drop.is_none()
                            && self.ghost.is_none()
                        {
                            if let Some(dragged_tile) = window.tree.dragged_id_including_root(&ctx)
                            {
                                egui::DragAndDrop::set_payload(
                                    &ctx,
                                    DockPayload {
                                        bridge_id,
                                        source_viewport: viewport_id,
                                        source_floating: Some(floating_id),
                                        tile_id: Some(dragged_tile),
                                    },
                                );
                                ctx.request_repaint_of(ViewportId::ROOT);
                            }
                        }

                        self.paint_drop_preview_if_any_for_tree(
                            ui,
                            behavior,
                            &window.tree,
                            content_rect,
                            viewport_id,
                        );
                    }
                });
        }

        if let Some((source_floating, dragged_tile, pointer_local)) = ghost_from_floating {
            if let Some(source_window) = manager.windows.get_mut(&source_floating) {
                let detach_tile = pick_detach_tile_for_tree(
                    ui.ctx(),
                    &self.options,
                    &source_window.tree,
                    dragged_tile,
                );
                let pane_rect_last = source_window.tree.tiles.rect(detach_tile);
                let extracted = source_window.tree.extract_subtree(detach_tile);
                let source_empty = source_window.tree.root.is_none();

                if source_empty {
                    manager.windows.remove(&source_floating);
                    manager.z_order.retain(|&id| id != source_floating);
                }

                if let Some(subtree) = extracted {
                    let size = pane_rect_last
                        .map(|r| Vec2::new(r.width().max(220.0), r.height().max(120.0)))
                        .unwrap_or(self.options.default_detached_inner_size);

                    let grab_offset = Vec2::new(20.0, 10.0);
                    let mut offset_in_dock = (pointer_local - dock_rect.min) - grab_offset;
                    offset_in_dock.x = offset_in_dock
                        .x
                        .clamp(0.0, (dock_rect.width() - size.x).max(0.0));
                    offset_in_dock.y = offset_in_dock
                        .y
                        .clamp(0.0, (dock_rect.height() - size.y).max(0.0));

                    let floating_id = self.allocate_floating_id();
                    let floating_tree_id = egui::Id::new((
                        self.tree.id(),
                        viewport_id,
                        "egui_docking_floating_tree",
                        floating_id,
                    ));
                    let floating_tree = Tree::new(floating_tree_id, subtree.root, subtree.tiles);

                    manager.windows.insert(
                        floating_id,
                        FloatingDockWindow {
                            tree: floating_tree,
                            offset_in_dock,
                            size,
                            collapsed: false,
                            drag: None,
                            resize: None,
                        },
                    );
                    manager.bring_to_front(floating_id);

                    egui::DragAndDrop::set_payload(
                        ui.ctx(),
                        DockPayload {
                            bridge_id: self.tree.id(),
                            source_viewport: viewport_id,
                            source_floating: Some(floating_id),
                            tile_id: None,
                        },
                    );

                    ui.ctx().request_repaint_of(ViewportId::ROOT);
                    self.ghost = Some(GhostDrag {
                        mode: GhostDragMode::Contained {
                            viewport: viewport_id,
                            floating: floating_id,
                        },
                        grab_offset,
                    });
                }
            }
        }

        for id in bring_to_front {
            manager.bring_to_front(id);
        }
        for id in dock_windows {
            let Some(mut window) = manager.windows.remove(&id) else {
                continue;
            };
            let Some(root) = window.tree.root.take() else {
                continue;
            };
            let tiles = std::mem::take(&mut window.tree.tiles);
            self.dock_subtree_into_dock_tree(
                viewport_id,
                egui_tiles::SubTree { root, tiles },
                None,
            );
        }
        for id in close_windows {
            manager.windows.remove(&id);
        }
        manager
            .z_order
            .retain(|id| manager.windows.contains_key(id));

        if !manager.windows.is_empty() {
            self.floating.insert(viewport_id, manager);
        }
    }

    fn dock_subtree_into_dock_tree(
        &mut self,
        viewport_id: ViewportId,
        subtree: egui_tiles::SubTree<Pane>,
        insertion: Option<InsertionPoint>,
    ) {
        if viewport_id == ViewportId::ROOT {
            self.dock_subtree_into_root(subtree, insertion);
            return;
        }

        if let Some(detached) = self.detached.get_mut(&viewport_id) {
            detached.tree.insert_subtree_at(subtree, insertion);
        }
    }

    fn extract_subtree_from_floating(
        &mut self,
        viewport_id: ViewportId,
        floating_id: FloatingId,
        tile_id: TileId,
    ) -> Option<egui_tiles::SubTree<Pane>> {
        let mut manager = self.floating.remove(&viewport_id)?;
        let extracted = manager
            .windows
            .get_mut(&floating_id)
            .and_then(|w| w.tree.extract_subtree(tile_id));

        if let Some(w) = manager.windows.get(&floating_id) {
            if w.tree.root.is_none() {
                manager.windows.remove(&floating_id);
            }
        }

        manager
            .z_order
            .retain(|id| manager.windows.contains_key(id));
        if !manager.windows.is_empty() {
            self.floating.insert(viewport_id, manager);
        }

        extracted
    }

    fn take_whole_floating_tree(
        &mut self,
        viewport_id: ViewportId,
        floating_id: FloatingId,
    ) -> Option<egui_tiles::SubTree<Pane>> {
        let mut manager = self.floating.remove(&viewport_id)?;
        let mut window = manager.windows.remove(&floating_id)?;
        manager
            .z_order
            .retain(|id| manager.windows.contains_key(id));

        if !manager.windows.is_empty() {
            self.floating.insert(viewport_id, manager);
        }

        let root = window.tree.root.take()?;
        let tiles = std::mem::take(&mut window.tree.tiles);
        Some(egui_tiles::SubTree { root, tiles })
    }

    fn floating_under_pointer(
        &self,
        viewport_id: ViewportId,
        pointer_local: Pos2,
    ) -> Option<FloatingId> {
        let manager = self.floating.get(&viewport_id)?;
        for id in manager.z_order.iter().rev().copied() {
            if self
                .last_floating_rects
                .get(&(viewport_id, id))
                .is_some_and(|r| r.contains(pointer_local))
            {
                return Some(id);
            }
        }
        None
    }

    fn floating_tree_id_under_pointer(
        &self,
        viewport_id: ViewportId,
        pointer_local: Pos2,
    ) -> Option<egui::Id> {
        let floating_id = self.floating_under_pointer(viewport_id, pointer_local)?;
        self.floating
            .get(&viewport_id)?
            .windows
            .get(&floating_id)
            .map(|w| w.tree.id())
    }

    fn queue_pending_local_drop_on_release(
        &mut self,
        ctx: &Context,
        dock_rect: Rect,
        viewport_id: ViewportId,
    ) {
        if self.pending_drop.is_some()
            || self.pending_internal_drop.is_some()
            || self.pending_local_drop.is_some()
        {
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
        if payload.source_viewport != viewport_id {
            return;
        }

        let Some(pointer_local) = ctx.input(|i| i.pointer.latest_pos()) else {
            return;
        };

        // Floating windows are top-most surfaces inside `dock_rect`, so check them first.
        let target_floating = self.floating_under_pointer(viewport_id, pointer_local);
        if target_floating.is_none() && !dock_rect.contains(pointer_local) {
            return;
        }

        if payload.source_floating == target_floating {
            return;
        }
        if payload.source_floating.is_none() && payload.tile_id.is_none() {
            // We don't support moving the whole dock tree within a viewport.
            return;
        }

        self.pending_local_drop = Some(PendingLocalDrop {
            payload: *payload,
            target_viewport: viewport_id,
            target_floating,
            pointer_local,
        });

        egui::DragAndDrop::clear_payload(ctx);
        ctx.stop_dragging();
        ctx.request_repaint_of(ViewportId::ROOT);
    }

    fn queue_pending_local_drop_from_dragged_tile_on_release(
        &mut self,
        ctx: &Context,
        dock_rect: Rect,
        viewport_id: ViewportId,
        source_floating: Option<FloatingId>,
        dragged_tile: TileId,
    ) {
        if self.pending_drop.is_some()
            || self.pending_internal_drop.is_some()
            || self.pending_local_drop.is_some()
        {
            return;
        }
        if !ctx.input(|i| i.pointer.any_released()) {
            return;
        }

        let Some(pointer_local) = ctx.input(|i| i.pointer.latest_pos()) else {
            return;
        };

        let target_floating = self.floating_under_pointer(viewport_id, pointer_local);

        if target_floating.is_none() && !dock_rect.contains(pointer_local) {
            return;
        }

        // If you are still inside the same floating window, let `egui_tiles` handle internal drops/reorder.
        if target_floating == source_floating {
            return;
        }

        if source_floating.is_none() && target_floating.is_none() {
            return;
        }

        self.pending_local_drop = Some(PendingLocalDrop {
            payload: DockPayload {
                bridge_id: self.tree.id(),
                source_viewport: viewport_id,
                source_floating,
                tile_id: Some(dragged_tile),
            },
            target_viewport: viewport_id,
            target_floating,
            pointer_local,
        });

        egui::DragAndDrop::clear_payload(ctx);
        ctx.stop_dragging();
        ctx.request_repaint_of(ViewportId::ROOT);
    }

    fn drop_insertion_at_pointer_local(
        &self,
        behavior: &dyn Behavior<Pane>,
        style: &egui::Style,
        viewport_id: ViewportId,
        target_floating: Option<FloatingId>,
        pointer_local: Pos2,
    ) -> Option<InsertionPoint> {
        if let Some(floating_id) = target_floating {
            let dock_rect = self
                .last_floating_rects
                .get(&(viewport_id, floating_id))
                .copied()?;
            let tree = self
                .floating
                .get(&viewport_id)?
                .windows
                .get(&floating_id)
                .map(|w| &w.tree)?;
            return overlay_insertion_for_tree_with_outer(tree, dock_rect, pointer_local).or_else(
                || {
                tree.dock_zone_at(behavior, style, pointer_local)
                    .map(|z| z.insertion_point)
            },
            );
        }

        if viewport_id == ViewportId::ROOT {
            let dock_rect = self.last_dock_rects.get(&ViewportId::ROOT).copied()?;
            return overlay_insertion_for_tree_with_outer(&self.tree, dock_rect, pointer_local)
                .or_else(|| {
                self.tree
                    .dock_zone_at(behavior, style, pointer_local)
                    .map(|z| z.insertion_point)
            });
        }

        let tree = self.detached.get(&viewport_id).map(|d| &d.tree)?;
        let dock_rect = self.last_dock_rects.get(&viewport_id).copied()?;
        overlay_insertion_for_tree_with_outer(tree, dock_rect, pointer_local).or_else(|| {
            tree.dock_zone_at(behavior, style, pointer_local)
                .map(|z| z.insertion_point)
        })
    }

    fn apply_pending_local_drop(&mut self, ctx: &Context, behavior: &mut dyn Behavior<Pane>) {
        let Some(pending) = self.pending_local_drop.take() else {
            return;
        };

        if pending.target_viewport != pending.payload.source_viewport {
            return;
        }

        let subtree = match (pending.payload.source_floating, pending.payload.tile_id) {
            (Some(floating_id), Some(tile_id)) => {
                self.extract_subtree_from_floating(pending.target_viewport, floating_id, tile_id)
            }
            (Some(floating_id), None) => {
                self.take_whole_floating_tree(pending.target_viewport, floating_id)
            }
            (None, Some(tile_id)) => {
                if pending.target_viewport == ViewportId::ROOT {
                    self.tree.extract_subtree(tile_id)
                } else if let Some(detached) = self.detached.get_mut(&pending.target_viewport) {
                    detached.tree.extract_subtree(tile_id)
                } else {
                    None
                }
            }
            (None, None) => None,
        };

        let Some(subtree) = subtree else {
            return;
        };

        let insertion = self.drop_insertion_at_pointer_local(
            behavior,
            ctx.style().as_ref(),
            pending.target_viewport,
            pending.target_floating,
            pending.pointer_local,
        );

        if let Some(target_floating) = pending.target_floating {
            let mut manager = self
                .floating
                .remove(&pending.target_viewport)
                .unwrap_or_default();
            if let Some(w) = manager.windows.get_mut(&target_floating) {
                w.tree.insert_subtree_at(subtree, insertion);
                manager.bring_to_front(target_floating);
                self.floating.insert(pending.target_viewport, manager);
            } else if pending.target_viewport == ViewportId::ROOT {
                self.dock_subtree_into_root(subtree, insertion);
            } else if let Some(detached) = self.detached.get_mut(&pending.target_viewport) {
                detached.tree.insert_subtree_at(subtree, insertion);
            }
            return;
        }

        self.dock_subtree_into_dock_tree(pending.target_viewport, subtree, insertion);
        behavior.on_edit(egui_tiles::EditAction::TileDropped);
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
                            .is_some_and(|o| o.hovered.is_some())
                    } else {
                        overlay_for_tree_at_pointer(tree, pointer_local)
                            .is_some_and(|o| o.hovered.is_some())
                    }
                });

        ctx.data_mut(|d| {
            d.insert_temp(
                tiles_disable_drop_preview_id(tree.id(), viewport_id),
                disable_preview,
            );
        });
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

    fn pending_internal_overlay_drop_on_release(
        &self,
        ctx: &Context,
        dock_rect: Rect,
        viewport_id: ViewportId,
        tree: &Tree<Pane>,
    ) -> Option<PendingInternalDrop> {
        if !self.options.show_overlay_for_internal_drags {
            return None;
        }
        if self.options.detach_on_alt_release_anywhere && ctx.input(|i| i.modifiers.alt) {
            return None;
        }
        if !ctx.input(|i| i.pointer.any_released()) {
            return None;
        }

        let dragged_tile = tree.dragged_id_including_root(ctx)?;
        let pointer_local = ctx.input(|i| i.pointer.latest_pos())?;
        if !dock_rect.contains(pointer_local) {
            return None;
        }
        if self
            .floating_tree_id_under_pointer(viewport_id, pointer_local)
            .is_some_and(|floating_tree_id| floating_tree_id != tree.id())
        {
            return None;
        }

        let insertion =
            overlay_insertion_for_tree_explicit_with_outer(tree, dock_rect, pointer_local)?;
        if tile_contains_descendant(tree, dragged_tile, insertion.parent_id) {
            return None;
        }

        Some(PendingInternalDrop {
            viewport: viewport_id,
            tile_id: dragged_tile,
            insertion,
        })
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

    fn apply_pending_internal_drop(&mut self, behavior: &mut dyn Behavior<Pane>) {
        let Some(pending) = self.pending_internal_drop.take() else {
            return;
        };

        if pending.viewport == ViewportId::ROOT {
            let Some(subtree) = self.tree.extract_subtree_no_reserve(pending.tile_id) else {
                return;
            };

            let insertion = self
                .tree
                .tiles
                .get(pending.insertion.parent_id)
                .is_some()
                .then_some(pending.insertion);
            self.tree.insert_subtree_at(subtree, insertion);
            return;
        }

        let Some(mut detached) = self.detached.remove(&pending.viewport) else {
            return;
        };

        let Some(subtree) = detached.tree.extract_subtree_no_reserve(pending.tile_id) else {
            self.detached.insert(pending.viewport, detached);
            return;
        };

        let insertion = detached
            .tree
            .tiles
            .get(pending.insertion.parent_id)
            .is_some()
            .then_some(pending.insertion);
        detached.tree.insert_subtree_at(subtree, insertion);

        detached.builder = detached
            .builder
            .clone()
            .with_title(title_for_detached_tree(&detached.tree, behavior));
        self.detached.insert(pending.viewport, detached);
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
        let dock_rect = dock_rect?;

        if target_viewport == ViewportId::ROOT {
            if let Some(insertion) =
                overlay_insertion_for_tree_with_outer(&self.tree, dock_rect, pointer_local)
            {
                return Some(insertion);
            }
            return self
                .tree
                .dock_zone_at(behavior, style, pointer_local)
                .map(|z| z.insertion_point);
        }

        let Some(detached) = self.detached.get(&target_viewport) else {
            return None;
        };
        if let Some(insertion) =
            overlay_insertion_for_tree_with_outer(&detached.tree, dock_rect, pointer_local)
        {
            return Some(insertion);
        }
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

        if let Some(floating_id) = payload.source_floating {
            if let Some(tile_id) = payload.tile_id {
                if let Some(subtree) = self.extract_subtree_from_floating(
                    payload.source_viewport,
                    floating_id,
                    tile_id,
                ) {
                    self.dock_subtree_into_root(subtree, insertion);
                }
            } else if let Some(subtree) =
                self.take_whole_floating_tree(payload.source_viewport, floating_id)
            {
                self.dock_subtree_into_root(subtree, insertion);
            }

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
                source_floating: payload.source_floating,
                tile_id,
                insertion,
            },
            None => DropAction::MoveWholeTree {
                source_viewport: payload.source_viewport,
                source_floating: payload.source_floating,
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
                source_floating,
                tile_id,
                insertion,
            } => {
                let subtree = if source_viewport == ViewportId::ROOT && source_floating.is_none() {
                    self.tree.extract_subtree(tile_id)
                } else if source_viewport == target_viewport {
                    None
                } else if let Some(floating_id) = source_floating {
                    self.extract_subtree_from_floating(source_viewport, floating_id, tile_id)
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
                source_floating,
                insertion,
            } => {
                if source_viewport == ViewportId::ROOT || source_viewport == target_viewport {
                    return;
                }

                if let Some(floating_id) = source_floating {
                    if let Some(subtree) =
                        self.take_whole_floating_tree(source_viewport, floating_id)
                    {
                        target.tree.insert_subtree_at(subtree, insertion);
                    }
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum OverlayTarget {
    Center,
    Left,
    Right,
    Top,
    Bottom,
}

#[derive(Clone, Copy, Debug)]
struct OverlayTargets {
    center: Rect,
    left: Option<Rect>,
    right: Option<Rect>,
    top: Option<Rect>,
    bottom: Option<Rect>,
}

impl OverlayTargets {
    fn iter(self) -> impl Iterator<Item = (OverlayTarget, Rect)> {
        [
            Some((OverlayTarget::Center, self.center)),
            self.left.map(|r| (OverlayTarget::Left, r)),
            self.right.map(|r| (OverlayTarget::Right, r)),
            self.top.map(|r| (OverlayTarget::Top, r)),
            self.bottom.map(|r| (OverlayTarget::Bottom, r)),
        ]
        .into_iter()
        .flatten()
    }

    fn hit_test(self, pointer: Pos2, parent_center: Pos2) -> Option<(OverlayTarget, Rect)> {
        let hs_w = self.center.width() * 0.5;
        if hs_w > 0.0 {
            let delta = pointer - parent_center;
            let len2 = delta.x * delta.x + delta.y * delta.y;

            let r_threshold_center = hs_w * 1.4;
            let r_threshold_sides = hs_w * (1.4 + 1.2);

            if len2 < r_threshold_center * r_threshold_center {
                return Some((OverlayTarget::Center, self.center));
            }

            if len2 < r_threshold_sides * r_threshold_sides {
                let prefer_horizontal = delta.x.abs() >= delta.y.abs();
                if prefer_horizontal {
                    if delta.x < 0.0 {
                        if let Some(r) = self.left {
                            return Some((OverlayTarget::Left, r));
                        }
                    } else if let Some(r) = self.right {
                        return Some((OverlayTarget::Right, r));
                    }
                } else if delta.y < 0.0 {
                    if let Some(r) = self.top {
                        return Some((OverlayTarget::Top, r));
                    }
                } else if let Some(r) = self.bottom {
                    return Some((OverlayTarget::Bottom, r));
                }
            }

            let expand = (hs_w * 0.30).round();
            if let Some(hit) = self
                .iter()
                .find(|(_t, rect)| rect.expand(expand).contains(pointer))
            {
                return Some(hit);
            }
        }

        self.iter().find(|(_t, rect)| rect.contains(pointer))
    }

    fn hit_test_boxes(self, pointer: Pos2) -> Option<(OverlayTarget, Rect)> {
        let hs_w = self.center.width() * 0.5;
        let expand = if hs_w > 0.0 {
            (hs_w * 0.30).round()
        } else {
            0.0
        };
        self.iter()
            .find(|(_t, rect)| rect.expand(expand).contains(pointer))
    }
}

#[derive(Clone, Copy, Debug)]
struct DockingOverlay {
    tile_rect: Rect,
    targets: OverlayTargets,
    hovered: Option<(OverlayTarget, Rect)>,
}

#[derive(Clone, Copy, Debug)]
struct OuterOverlayTargets {
    left: Rect,
    right: Rect,
    top: Rect,
    bottom: Rect,
}

impl OuterOverlayTargets {
    fn iter(self) -> impl Iterator<Item = (OverlayTarget, Rect)> {
        [
            (OverlayTarget::Left, self.left),
            (OverlayTarget::Right, self.right),
            (OverlayTarget::Top, self.top),
            (OverlayTarget::Bottom, self.bottom),
        ]
        .into_iter()
    }

    fn hit_test(self, pointer: Pos2, parent_center: Pos2) -> Option<(OverlayTarget, Rect)> {
        let hs_w = self.left.width() * 0.5;
        if hs_w > 0.0 {
            let delta = pointer - parent_center;
            let len2 = delta.x * delta.x + delta.y * delta.y;

            let r_threshold_sides = hs_w * (1.4 + 1.2);
            if len2 < r_threshold_sides * r_threshold_sides {
                let prefer_horizontal = delta.x.abs() >= delta.y.abs();
                if prefer_horizontal {
                    if delta.x < 0.0 {
                        return Some((OverlayTarget::Left, self.left));
                    }
                    return Some((OverlayTarget::Right, self.right));
                }

                if delta.y < 0.0 {
                    return Some((OverlayTarget::Top, self.top));
                }
                return Some((OverlayTarget::Bottom, self.bottom));
            }

            let expand = (hs_w * 0.30).round();
            if let Some(hit) = self
                .iter()
                .find(|(_t, rect)| rect.expand(expand).contains(pointer))
            {
                return Some(hit);
            }
        }

        self.iter().find(|(_t, rect)| rect.contains(pointer))
    }

    fn hit_test_boxes(self, pointer: Pos2) -> Option<(OverlayTarget, Rect)> {
        let hs_w = self.left.width() * 0.5;
        let expand = if hs_w > 0.0 {
            (hs_w * 0.30).round()
        } else {
            0.0
        };
        self.iter()
            .find(|(_t, rect)| rect.expand(expand).contains(pointer))
    }
}

#[derive(Clone, Copy, Debug)]
struct OuterDockingOverlay {
    dock_rect: Rect,
    targets: OuterOverlayTargets,
    hovered: Option<(OverlayTarget, Rect)>,
}

fn pointer_in_outer_band(dock_rect: Rect, pointer: Pos2) -> bool {
    if !dock_rect.contains(pointer) {
        return false;
    }

    let min_dim = dock_rect.width().min(dock_rect.height());
    if min_dim <= 0.0 {
        return false;
    }

    let band = (min_dim * 0.22).clamp(32.0, 80.0);
    let dx = (pointer.x - dock_rect.left()).min(dock_rect.right() - pointer.x);
    let dy = (pointer.y - dock_rect.top()).min(dock_rect.bottom() - pointer.y);
    dx.min(dy) <= band
}

fn outer_overlay_targets_in_rect(dock_rect: Rect) -> Option<OuterOverlayTargets> {
    let min_dim = dock_rect.width().min(dock_rect.height());
    if min_dim <= 0.0 {
        return None;
    }

    let size = (min_dim * 0.12).clamp(22.0, 56.0);
    let hs = size * 0.5;
    let margin = (size * 0.35).clamp(6.0, 18.0);

    let center = dock_rect.center();
    let left_center = Pos2::new(dock_rect.left() + margin + hs, center.y);
    let right_center = Pos2::new(dock_rect.right() - margin - hs, center.y);
    let top_center = Pos2::new(center.x, dock_rect.top() + margin + hs);
    let bottom_center = Pos2::new(center.x, dock_rect.bottom() - margin - hs);

    // If there isn't enough room to place non-overlapping targets near edges, skip outer overlay.
    if left_center.x + hs >= right_center.x - hs || top_center.y + hs >= bottom_center.y - hs {
        return None;
    }

    let left = Rect::from_center_size(left_center, Vec2::splat(size)).intersect(dock_rect);
    let right = Rect::from_center_size(right_center, Vec2::splat(size)).intersect(dock_rect);
    let top = Rect::from_center_size(top_center, Vec2::splat(size)).intersect(dock_rect);
    let bottom = Rect::from_center_size(bottom_center, Vec2::splat(size)).intersect(dock_rect);

    (left.is_positive() && right.is_positive() && top.is_positive() && bottom.is_positive())
        .then_some(OuterOverlayTargets {
            left,
            right,
            top,
            bottom,
        })
}

fn outer_overlay_for_dock_rect(dock_rect: Rect, pointer: Pos2) -> Option<OuterDockingOverlay> {
    if !pointer_in_outer_band(dock_rect, pointer) {
        return None;
    }

    let targets = outer_overlay_targets_in_rect(dock_rect)?;
    let hovered = targets.hit_test(pointer, dock_rect.center());
    Some(OuterDockingOverlay {
        dock_rect,
        targets,
        hovered,
    })
}

fn outer_insertion_for_tree<Pane>(
    tree: &Tree<Pane>,
    dock_rect: Rect,
    pointer: Pos2,
) -> Option<InsertionPoint> {
    let root = tree.root?;
    if !pointer_in_outer_band(dock_rect, pointer) {
        return None;
    }
    let overlay = outer_overlay_for_dock_rect(dock_rect, pointer)?;
    let (target, _rect) = overlay.hovered?;

    Some(match target {
        OverlayTarget::Left => InsertionPoint::new(
            root,
            egui_tiles::ContainerInsertion::Horizontal(0),
        ),
        OverlayTarget::Right => InsertionPoint::new(
            root,
            egui_tiles::ContainerInsertion::Horizontal(usize::MAX),
        ),
        OverlayTarget::Top => InsertionPoint::new(root, egui_tiles::ContainerInsertion::Vertical(0)),
        OverlayTarget::Bottom => InsertionPoint::new(
            root,
            egui_tiles::ContainerInsertion::Vertical(usize::MAX),
        ),
        OverlayTarget::Center => return None,
    })
}

fn outer_insertion_for_tree_explicit<Pane>(
    tree: &Tree<Pane>,
    dock_rect: Rect,
    pointer: Pos2,
) -> Option<InsertionPoint> {
    let root = tree.root?;
    if !pointer_in_outer_band(dock_rect, pointer) {
        return None;
    }
    let targets = outer_overlay_targets_in_rect(dock_rect)?;
    let (target, _rect) = targets.hit_test_boxes(pointer)?;
    Some(match target {
        OverlayTarget::Left => InsertionPoint::new(
            root,
            egui_tiles::ContainerInsertion::Horizontal(0),
        ),
        OverlayTarget::Right => InsertionPoint::new(
            root,
            egui_tiles::ContainerInsertion::Horizontal(usize::MAX),
        ),
        OverlayTarget::Top => InsertionPoint::new(root, egui_tiles::ContainerInsertion::Vertical(0)),
        OverlayTarget::Bottom => InsertionPoint::new(
            root,
            egui_tiles::ContainerInsertion::Vertical(usize::MAX),
        ),
        OverlayTarget::Center => return None,
    })
}

fn overlay_insertion_for_tree_with_outer<Pane>(
    tree: &Tree<Pane>,
    dock_rect: Rect,
    pointer: Pos2,
) -> Option<InsertionPoint> {
    // Only consider "outer" docking when the pointer is near the dockspace edge, otherwise it
    // competes visually with the inner 5-way overlay.
    if pointer_in_outer_band(dock_rect, pointer) {
        outer_insertion_for_tree(tree, dock_rect, pointer)
    } else {
        overlay_insertion_for_tree(tree, pointer)
    }
}

fn overlay_insertion_for_tree_explicit_with_outer<Pane>(
    tree: &Tree<Pane>,
    dock_rect: Rect,
    pointer: Pos2,
) -> Option<InsertionPoint> {
    if pointer_in_outer_band(dock_rect, pointer) {
        outer_insertion_for_tree_explicit(tree, dock_rect, pointer)
    } else {
        overlay_insertion_for_tree_explicit(tree, pointer)
    }
}

fn overlay_insertion_for_tree<Pane>(tree: &Tree<Pane>, pointer: Pos2) -> Option<InsertionPoint> {
    let overlay = overlay_for_tree_at_pointer(tree, pointer)?;
    let (target, _rect) = overlay.hovered?;

    let tile_id = best_tile_under_pointer(tree, pointer)?.0;

    Some(match target {
        OverlayTarget::Center => {
            InsertionPoint::new(tile_id, egui_tiles::ContainerInsertion::Tabs(usize::MAX))
        }
        OverlayTarget::Left => {
            InsertionPoint::new(tile_id, egui_tiles::ContainerInsertion::Horizontal(0))
        }
        OverlayTarget::Right => InsertionPoint::new(
            tile_id,
            egui_tiles::ContainerInsertion::Horizontal(usize::MAX),
        ),
        OverlayTarget::Top => {
            InsertionPoint::new(tile_id, egui_tiles::ContainerInsertion::Vertical(0))
        }
        OverlayTarget::Bottom => InsertionPoint::new(
            tile_id,
            egui_tiles::ContainerInsertion::Vertical(usize::MAX),
        ),
    })
}

fn overlay_insertion_for_tree_explicit<Pane>(
    tree: &Tree<Pane>,
    pointer: Pos2,
) -> Option<InsertionPoint> {
    let (tile_id, tile_rect) = best_tile_under_pointer(tree, pointer)?;

    let kind = tree.tiles.get(tile_id).and_then(|t| t.kind());
    let allow_lr = kind != Some(ContainerKind::Horizontal);
    let allow_tb = kind != Some(ContainerKind::Vertical);

    let targets = overlay_targets_in_rect(tile_rect, allow_lr, allow_tb);
    let (target, _rect) = targets.hit_test_boxes(pointer)?;

    Some(match target {
        OverlayTarget::Center => {
            InsertionPoint::new(tile_id, egui_tiles::ContainerInsertion::Tabs(usize::MAX))
        }
        OverlayTarget::Left => {
            InsertionPoint::new(tile_id, egui_tiles::ContainerInsertion::Horizontal(0))
        }
        OverlayTarget::Right => InsertionPoint::new(
            tile_id,
            egui_tiles::ContainerInsertion::Horizontal(usize::MAX),
        ),
        OverlayTarget::Top => {
            InsertionPoint::new(tile_id, egui_tiles::ContainerInsertion::Vertical(0))
        }
        OverlayTarget::Bottom => InsertionPoint::new(
            tile_id,
            egui_tiles::ContainerInsertion::Vertical(usize::MAX),
        ),
    })
}

fn overlay_for_tree_at_pointer<Pane>(tree: &Tree<Pane>, pointer: Pos2) -> Option<DockingOverlay> {
    let (tile_id, tile_rect) = best_tile_under_pointer(tree, pointer)?;

    let kind = tree.tiles.get(tile_id).and_then(|t| t.kind());
    let allow_lr = kind != Some(ContainerKind::Horizontal);
    let allow_tb = kind != Some(ContainerKind::Vertical);

    let targets = overlay_targets_in_rect(tile_rect, allow_lr, allow_tb);
    let hovered = targets.hit_test(pointer, tile_rect.center());

    Some(DockingOverlay {
        tile_rect,
        targets,
        hovered,
    })
}

fn overlay_targets_in_rect(tile_rect: Rect, allow_lr: bool, allow_tb: bool) -> OverlayTargets {
    let min_dim = tile_rect.width().min(tile_rect.height());
    let size = (min_dim * 0.16).clamp(24.0, 56.0);
    let gap = (size * 0.25).clamp(6.0, 18.0);

    let center = Rect::from_center_size(tile_rect.center(), Vec2::splat(size)).intersect(tile_rect);
    let left = allow_lr
        .then(|| {
            Rect::from_center_size(
                tile_rect.center() - Vec2::new(size + gap, 0.0),
                Vec2::splat(size),
            )
        })
        .map(|r| r.intersect(tile_rect))
        .filter(|r| r.is_positive());
    let right = allow_lr
        .then(|| {
            Rect::from_center_size(
                tile_rect.center() + Vec2::new(size + gap, 0.0),
                Vec2::splat(size),
            )
        })
        .map(|r| r.intersect(tile_rect))
        .filter(|r| r.is_positive());
    let top = allow_tb
        .then(|| {
            Rect::from_center_size(
                tile_rect.center() - Vec2::new(0.0, size + gap),
                Vec2::splat(size),
            )
        })
        .map(|r| r.intersect(tile_rect))
        .filter(|r| r.is_positive());
    let bottom = allow_tb
        .then(|| {
            Rect::from_center_size(
                tile_rect.center() + Vec2::new(0.0, size + gap),
                Vec2::splat(size),
            )
        })
        .map(|r| r.intersect(tile_rect))
        .filter(|r| r.is_positive());

    OverlayTargets {
        center,
        left,
        right,
        top,
        bottom,
    }
}

fn tile_contains_descendant<Pane>(tree: &Tree<Pane>, root: TileId, candidate: TileId) -> bool {
    if root == candidate {
        return true;
    }

    let mut stack = vec![root];
    while let Some(tile_id) = stack.pop() {
        let Some(tile) = tree.tiles.get(tile_id) else {
            continue;
        };
        match tile {
            Tile::Pane(_) => {}
            Tile::Container(container) => {
                for &child in container.children() {
                    if child == candidate {
                        return true;
                    }
                    stack.push(child);
                }
            }
        }
    }

    false
}

fn best_tile_under_pointer<Pane>(tree: &Tree<Pane>, pointer: Pos2) -> Option<(TileId, Rect)> {
    let mut best: Option<(TileId, Rect)> = None;
    let mut best_area = f32::INFINITY;

    for tile_id in tree.active_tiles() {
        let Some(rect) = tree.tiles.rect(tile_id) else {
            continue;
        };
        if !rect.contains(pointer) {
            continue;
        }
        let area = rect.width() * rect.height();
        if area < best_area {
            best_area = area;
            best = Some((tile_id, rect));
        }
    }

    best
}

fn paint_overlay(painter: &egui::Painter, visuals: &egui::Visuals, overlay: DockingOverlay) {
    if let Some((target, _rect)) = overlay.hovered {
        let split_frac = 0.5;
        let preview_rect = match target {
            OverlayTarget::Center => overlay.tile_rect.shrink(1.0),
            OverlayTarget::Left => overlay
                .tile_rect
                .split_left_right_at_fraction(split_frac)
                .0
                .shrink(1.0),
            OverlayTarget::Right => overlay
                .tile_rect
                .split_left_right_at_fraction(split_frac)
                .1
                .shrink(1.0),
            OverlayTarget::Top => overlay
                .tile_rect
                .split_top_bottom_at_fraction(split_frac)
                .0
                .shrink(1.0),
            OverlayTarget::Bottom => overlay
                .tile_rect
                .split_top_bottom_at_fraction(split_frac)
                .1
                .shrink(1.0),
        };

        let stroke = visuals.selection.stroke;
        let base = visuals.selection.bg_fill;
        let fill = with_alpha(base, ((base.a() as f32) * 0.45) as u8);
        painter.rect(preview_rect, 1.0, fill, stroke, egui::StrokeKind::Inside);
    }

    let panel_fill = visuals.window_fill().gamma_multiply(0.75);
    let panel_stroke = visuals.widgets.inactive.bg_stroke;
    let active_fill = visuals.selection.bg_fill.gamma_multiply(0.85);
    let active_stroke = visuals.selection.stroke;
    let inactive_icon = visuals.widgets.inactive.fg_stroke.color;
    let active_icon = visuals.selection.stroke.color;

    for (t, rect) in overlay.targets.iter() {
        let hovered = overlay.hovered.is_some_and(|(ht, _)| ht == t);
        let (fill, stroke) = if hovered {
            (active_fill, active_stroke)
        } else {
            (panel_fill, panel_stroke)
        };

        painter.rect(rect, 4.0, fill, stroke, egui::StrokeKind::Inside);

        let icon_color = if hovered { active_icon } else { inactive_icon };
        paint_overlay_icon(painter, rect, t, icon_color);
    }
}

fn paint_outer_overlay(
    painter: &egui::Painter,
    visuals: &egui::Visuals,
    overlay: OuterDockingOverlay,
) {
    if let Some((target, _rect)) = overlay.hovered {
        let split_frac = 0.5;
        let preview_rect = match target {
            OverlayTarget::Left => overlay
                .dock_rect
                .split_left_right_at_fraction(split_frac)
                .0
                .shrink(1.0),
            OverlayTarget::Right => overlay
                .dock_rect
                .split_left_right_at_fraction(split_frac)
                .1
                .shrink(1.0),
            OverlayTarget::Top => overlay
                .dock_rect
                .split_top_bottom_at_fraction(split_frac)
                .0
                .shrink(1.0),
            OverlayTarget::Bottom => overlay
                .dock_rect
                .split_top_bottom_at_fraction(split_frac)
                .1
                .shrink(1.0),
            OverlayTarget::Center => overlay.dock_rect.shrink(1.0),
        };

        let stroke = visuals.selection.stroke;
        let base = visuals.selection.bg_fill;
        let fill = with_alpha(base, ((base.a() as f32) * 0.45) as u8);
        painter.rect(preview_rect, 1.0, fill, stroke, egui::StrokeKind::Inside);
    }

    let panel_fill = visuals.window_fill().gamma_multiply(0.75);
    let panel_stroke = visuals.widgets.inactive.bg_stroke;
    let active_fill = visuals.selection.bg_fill.gamma_multiply(0.85);
    let active_stroke = visuals.selection.stroke;
    let inactive_icon = visuals.widgets.inactive.fg_stroke.color;
    let active_icon = visuals.selection.stroke.color;

    for (t, rect) in overlay.targets.iter() {
        let hovered = overlay.hovered.is_some_and(|(ht, _)| ht == t);
        let (fill, stroke) = if hovered {
            (active_fill, active_stroke)
        } else {
            (panel_fill, panel_stroke)
        };

        painter.rect(rect, 4.0, fill, stroke, egui::StrokeKind::Inside);

        let icon_color = if hovered { active_icon } else { inactive_icon };
        paint_overlay_icon(painter, rect, t, icon_color);
    }
}

fn with_alpha(color: egui::Color32, alpha: u8) -> egui::Color32 {
    egui::Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), alpha)
}

fn paint_overlay_icon(
    painter: &egui::Painter,
    rect: Rect,
    target: OverlayTarget,
    color: egui::Color32,
) {
    let icon_rect = Rect::from_center_size(rect.center(), rect.size() * 0.62);
    let stroke = egui::Stroke::new(1.5, color.gamma_multiply(0.9));

    painter.rect_stroke(icon_rect, 2.0, stroke, egui::StrokeKind::Inside);

    match target {
        OverlayTarget::Center => {
            let mid = icon_rect.center();
            painter.line_segment(
                [
                    Pos2::new(icon_rect.left(), mid.y),
                    Pos2::new(icon_rect.right(), mid.y),
                ],
                stroke,
            );
            painter.line_segment(
                [
                    Pos2::new(mid.x, icon_rect.top()),
                    Pos2::new(mid.x, icon_rect.bottom()),
                ],
                stroke,
            );
        }
        OverlayTarget::Left => {
            let split_x = icon_rect.left() + icon_rect.width() * 0.38;
            let fill = color.gamma_multiply(0.25);
            painter.rect_filled(
                Rect::from_min_max(icon_rect.min, Pos2::new(split_x, icon_rect.max.y)),
                0.0,
                fill,
            );
            painter.line_segment(
                [
                    Pos2::new(split_x, icon_rect.top()),
                    Pos2::new(split_x, icon_rect.bottom()),
                ],
                stroke,
            );
        }
        OverlayTarget::Right => {
            let split_x = icon_rect.right() - icon_rect.width() * 0.38;
            let fill = color.gamma_multiply(0.25);
            painter.rect_filled(
                Rect::from_min_max(Pos2::new(split_x, icon_rect.min.y), icon_rect.max),
                0.0,
                fill,
            );
            painter.line_segment(
                [
                    Pos2::new(split_x, icon_rect.top()),
                    Pos2::new(split_x, icon_rect.bottom()),
                ],
                stroke,
            );
        }
        OverlayTarget::Top => {
            let split_y = icon_rect.top() + icon_rect.height() * 0.38;
            let fill = color.gamma_multiply(0.25);
            painter.rect_filled(
                Rect::from_min_max(icon_rect.min, Pos2::new(icon_rect.max.x, split_y)),
                0.0,
                fill,
            );
            painter.line_segment(
                [
                    Pos2::new(icon_rect.left(), split_y),
                    Pos2::new(icon_rect.right(), split_y),
                ],
                stroke,
            );
        }
        OverlayTarget::Bottom => {
            let split_y = icon_rect.bottom() - icon_rect.height() * 0.38;
            let fill = color.gamma_multiply(0.25);
            painter.rect_filled(
                Rect::from_min_max(Pos2::new(icon_rect.min.x, split_y), icon_rect.max),
                0.0,
                fill,
            );
            painter.line_segment(
                [
                    Pos2::new(icon_rect.left(), split_y),
                    Pos2::new(icon_rect.right(), split_y),
                ],
                stroke,
            );
        }
    }
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

fn tiles_disable_drop_preview_id(tree_id: egui::Id, _viewport_id: ViewportId) -> egui::Id {
    egui::Id::new((tree_id, "egui_docking_disable_drop_preview"))
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
