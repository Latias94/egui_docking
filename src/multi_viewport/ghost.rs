use egui::{Context, Pos2, Rect, Vec2, ViewportBuilder, ViewportId};
use egui_tiles::{Behavior, Tree};

use super::DockingMultiViewport;
use super::geometry::{infer_detached_geometry, pointer_pos_in_global, root_inner_rect_in_global};
use super::title::title_for_detached_subtree;
use super::types::{DetachedDock, DockPayload, FloatingDockWindow, GhostDrag, GhostDragMode};

impl<Pane> DockingMultiViewport<Pane> {
    fn ghost_skip_window_move_logged_id(&self, viewport_id: ViewportId) -> egui::Id {
        egui::Id::new((
            self.tree.id(),
            viewport_id,
            "egui_docking_ghost_skip_window_move_logged",
        ))
    }

    fn clear_ghost_skip_window_move_log_flag(&self, ctx: &Context, viewport_id: ViewportId) {
        let id = self.ghost_skip_window_move_logged_id(viewport_id);
        ctx.data_mut(|d| d.remove::<bool>(id));
    }

    fn debug_log_ghost_skip_window_move_once(&mut self, ctx: &Context, viewport_id: ViewportId) {
        if !self.options.debug_event_log {
            return;
        }

        let id = self.ghost_skip_window_move_logged_id(viewport_id);
        let already = ctx.data(|d| d.get_temp::<bool>(id).unwrap_or(false));
        if already {
            return;
        }

        ctx.data_mut(|d| d.insert_temp(id, true));
        self.debug_log_event(format!(
            "ghost_skip (window_move payload) viewport={viewport_id:?}"
        ));
    }

    fn is_window_move_payload_active_in_viewport(
        &self,
        ctx: &Context,
        viewport_id: ViewportId,
    ) -> bool {
        egui::DragAndDrop::payload::<DockPayload>(ctx).is_some_and(|payload| {
            payload.bridge_id == self.tree.id()
                && payload.source_viewport == viewport_id
                && payload.tile_id.is_none()
        })
    }

    fn spawn_native_ghost_from_subtree(
        &mut self,
        ctx: &Context,
        behavior: &mut dyn Behavior<Pane>,
        subtree: egui_tiles::SubTree<Pane>,
        size: Vec2,
        grab_offset: Vec2,
        pointer_global_hint: Option<Pos2>,
    ) -> ViewportId {
        let title = title_for_detached_subtree(&subtree, behavior);

        let pointer_global = pointer_global_hint
            .or_else(|| pointer_pos_in_global(ctx))
            .or(self.drag_state.last_pointer_global());
        let pos = pointer_global
            .map(|p| p - grab_offset)
            .unwrap_or(Pos2::new(64.0, 64.0));

        let (viewport_id, serial) = self.allocate_detached_viewport_id();
        let builder = ViewportBuilder::default()
            .with_title(title)
            .with_position(pos)
            .with_inner_size(size)
            .with_decorations(self.options.detached_viewport_decorations);

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

        egui::DragAndDrop::set_payload(
            ctx,
            DockPayload {
                bridge_id: self.tree.id(),
                source_viewport: viewport_id,
                source_floating: None,
                tile_id: None,
            },
        );
        ctx.request_repaint_of(ViewportId::ROOT);
        self.ghost = Some(GhostDrag {
            mode: GhostDragMode::Native {
                viewport: viewport_id,
            },
            grab_offset,
        });

        if self.options.debug_event_log {
            self.debug_log_event(format!(
                "ghost_spawn_native viewport={viewport_id:?} size=({:.1},{:.1}) grab=({:.1},{:.1})",
                size.x, size.y, grab_offset.x, grab_offset.y
            ));
        }

        viewport_id
    }

    pub(super) fn set_payload_from_root_drag_if_any(&mut self, ctx: &Context) {
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
        }
    }

    pub(super) fn try_tear_off_from_root(
        &mut self,
        ctx: &Context,
        behavior: &mut dyn Behavior<Pane>,
        dock_rect: Rect,
    ) {
        if self.is_window_move_payload_active_in_viewport(ctx, ViewportId::ROOT) {
            return;
        }
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
        let global_fallback_pos = self.drag_state.last_pointer_global();
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
            .with_inner_size(size)
            .with_decorations(self.options.detached_viewport_decorations);

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

    pub(super) fn try_tear_off_from_detached(
        &mut self,
        ctx: &Context,
        behavior: &mut dyn Behavior<Pane>,
        dock_rect: Rect,
        current_viewport: ViewportId,
        tree: &mut Tree<Pane>,
        did_tear_off: &mut bool,
    ) {
        if self.is_window_move_payload_active_in_viewport(ctx, current_viewport) {
            return;
        }
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
        let global_fallback_pos = self.drag_state.last_pointer_global();
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
            .with_inner_size(size)
            .with_decorations(self.options.detached_viewport_decorations);

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

    pub(super) fn maybe_start_ghost_from_root(
        &mut self,
        ctx: &Context,
        behavior: &mut dyn Behavior<Pane>,
        dock_rect: Rect,
    ) {
        if self.is_window_move_payload_active_in_viewport(ctx, ViewportId::ROOT) {
            // When moving a whole window (window-move docking), do not start ghost tear-off.
            self.clear_ghost_skip_window_move_log_flag(ctx, ViewportId::ROOT);
            return;
        }
        self.clear_ghost_skip_window_move_log_flag(ctx, ViewportId::ROOT);
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
        if self
            .floating_under_pointer(viewport_id, pointer_local)
            .is_some()
        {
            return;
        }

        let tree = &mut self.tree;

        let Some(dragged_tile) = tree.dragged_id_including_root(ctx) else {
            return;
        };
        let detach_tile = super::pick_detach_tile_for_tree(ctx, &self.options, tree, dragged_tile);

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
        let ctrl_floating =
            self.options.tear_off_to_floating_on_ctrl && ctx.input(|i| i.modifiers.ctrl);
        if self.options.ghost_spawn_native_on_leave_dock && !ctrl_floating {
            self.spawn_native_ghost_from_subtree(
                ctx,
                behavior,
                subtree,
                size,
                grab_offset,
                self.drag_state.last_pointer_global(),
            );
            ctx.request_repaint();
            return;
        }

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

    pub(super) fn maybe_start_ghost_from_tree_in_viewport(
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
        if self.is_window_move_payload_active_in_viewport(ctx, viewport_id) {
            // IMPORTANT: while we are moving a whole native/contained window host, starting a ghost
            // tear-off here can extract the whole tree, making the source viewport temporarily empty
            // (and thus close), which looks like the window "disappeared mid-drag".
            self.debug_log_ghost_skip_window_move_once(ctx, viewport_id);
            return;
        }
        self.clear_ghost_skip_window_move_log_flag(ctx, viewport_id);

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
        if self
            .floating_under_pointer(viewport_id, pointer_local)
            .is_some()
        {
            return;
        }

        let Some(dragged_tile) = tree.dragged_id_including_root(ctx) else {
            return;
        };
        let detach_tile = super::pick_detach_tile_for_tree(ctx, &self.options, tree, dragged_tile);

        ctx.stop_dragging();

        let pane_rect_last = tree.tiles.rect(detach_tile);
        let Some(subtree) = tree.extract_subtree(detach_tile) else {
            return;
        };

        let size = pane_rect_last
            .map(|r| Vec2::new(r.width().max(220.0), r.height().max(120.0)))
            .unwrap_or(self.options.default_detached_inner_size);

        let grab_offset = Vec2::new(20.0, 10.0);
        let ctrl_floating =
            self.options.tear_off_to_floating_on_ctrl && ctx.input(|i| i.modifiers.ctrl);
        if self.options.ghost_spawn_native_on_leave_dock && !ctrl_floating {
            self.spawn_native_ghost_from_subtree(
                ctx,
                behavior,
                subtree,
                size,
                grab_offset,
                self.drag_state.last_pointer_global(),
            );
            if tree.root.is_none() {
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
            ctx.request_repaint();
            ctx.request_repaint_of(ViewportId::ROOT);
            return;
        }

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

    pub(super) fn finish_ghost_if_released_or_aborted(&mut self, ctx: &Context) {
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
            // Drop has priority: if some other release handler already took ownership this frame,
            // we just stop tracking the ghost without performing any extra finalize logic here.
            let _took = self.try_take_release_action_silent_if_taken("ghost_finalize");
            self.ghost = None;
        }
    }
}
