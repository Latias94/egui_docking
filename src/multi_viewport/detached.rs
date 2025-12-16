use egui::{Context, ViewportClass, ViewportId};
use egui_tiles::Behavior;

use super::DockingMultiViewport;
use super::title::title_for_detached_tree;
use super::types::{GhostDrag, GhostDragMode};
use super::types::DockPayload;

impl<Pane> DockingMultiViewport<Pane> {
    pub(super) fn ui_detached_viewports(
        &mut self,
        ctx: &Context,
        behavior: &mut dyn Behavior<Pane>,
    ) {
        let viewport_ids: Vec<ViewportId> = self.detached.keys().copied().collect();
        let bridge_id = self.tree.id();

        for viewport_id in viewport_ids {
            if self
                .detached_rendered_frame
                .get(&viewport_id)
                .is_some_and(|&f| f == self.debug_frame)
            {
                continue;
            }
            self.detached_rendered_frame
                .insert(viewport_id, self.debug_frame);

            let Some(mut detached) = self.detached.remove(&viewport_id) else {
                continue;
            };

            let builder = detached.builder.clone();
            let mut should_redock_to_root = false;

            ctx.show_viewport_immediate(viewport_id, builder, |ctx, class| {
                self.update_last_pointer_global_from_active_viewport(ctx);
                self.observe_drag_sources_in_ctx(ctx);

                if let Some(GhostDrag {
                    mode: GhostDragMode::Native { viewport },
                    grab_offset,
                }) = self.ghost
                {
                    if viewport == viewport_id {
                        if let Some(pointer_global) = self.drag_state.last_pointer_global() {
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

                egui::CentralPanel::default().show(ctx, |ui| {
                    let dock_rect = ui.available_rect_before_wrap();
                    self.last_dock_rects.insert(viewport_id, dock_rect);
                    // Hit-testing and drop preview must not depend on draw order. Rebuild the floating
                    // rect cache before any release/overlay logic runs for this viewport.
                    self.rebuild_floating_rect_cache_for_viewport(
                        ctx,
                        behavior,
                        dock_rect,
                        viewport_id,
                    );

                    // ImGui-like: dragging the tab-bar background of a detached window should move the
                    // native viewport itself (window-move docking), without adding a second custom title bar.
                    //
                    // We only enable this when the detached tree is hosted as a single Tabs root node.
                    // More complex split layouts should be redocked by dragging individual tabs/panes.
                    let move_active_id =
                        egui::Id::new((bridge_id, viewport_id, "egui_docking_detached_window_move_active"));
                    let grab_id =
                        egui::Id::new((bridge_id, viewport_id, "egui_docking_detached_window_move_grab"));
                    let last_local_id = egui::Id::new((
                        bridge_id,
                        viewport_id,
                        "egui_docking_detached_window_move_last_pointer_local",
                    ));

                    let root_is_tabs = detached
                        .tree
                        .root
                        .and_then(|r| detached.tree.tiles.get(r).and_then(|t| t.kind()))
                        == Some(egui_tiles::ContainerKind::Tabs);
                    let dragged_tile = detached.tree.dragged_id_including_root(ctx);
                    let should_start_window_move =
                        root_is_tabs && dragged_tile == detached.tree.root && ctx.data(|d| d.get_temp::<bool>(move_active_id)).unwrap_or(false) == false;

                    if should_start_window_move {
                        self.observe_tiles_drag_detached();
                        ctx.data_mut(|d| d.insert_temp(move_active_id, true));

                        if self.options.focus_detached_on_custom_title_drag {
                            ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
                        }

                        if let Some(pointer_global) = self.drag_state.pointer_global_fallback(ctx) {
                            let window_rect =
                                ctx.input(|i| i.viewport().outer_rect.or(i.viewport().inner_rect));
                            if let Some(window_rect) = window_rect {
                                let grab_offset = pointer_global - window_rect.min;
                                ctx.data_mut(|d| d.insert_temp(grab_id, grab_offset));
                            }
                        }

                        egui::DragAndDrop::set_payload(
                            ctx,
                            DockPayload {
                                bridge_id,
                                source_viewport: viewport_id,
                                source_floating: None,
                                tile_id: None,
                            },
                        );

                        // Transfer authority away from egui_tiles; we are now in "window move" mode.
                        ctx.stop_dragging();
                        ctx.request_repaint_of(ViewportId::ROOT);

                        if self.options.debug_event_log {
                            self.debug_log_event(format!(
                                "detached_window_move START viewport={viewport_id:?} root={:?}",
                                detached.tree.root
                            ));
                        }
                    }

                    let window_move_active =
                        ctx.data(|d| d.get_temp::<bool>(move_active_id)).unwrap_or(false);
                    if window_move_active {
                        // Keep the viewport following the pointer while the payload is active.
                        //
                        // IMPORTANT: some platforms stop delivering cursor updates while the native window is moving.
                        // If we keep recomputing pointer_global from a stale local pointer, we can drift forever.
                        let pointer_local_now = ctx.input(|i| i.pointer.latest_pos());
                        let pointer_local_prev = ctx.data(|d| d.get_temp::<egui::Pos2>(last_local_id));
                        let local_is_fresh = match (pointer_local_now, pointer_local_prev) {
                            (Some(now), Some(prev)) => (now - prev).length_sq() > 0.25, // ~0.5px threshold
                            (Some(_), None) => true,
                            _ => false,
                        };
                        if let Some(now) = pointer_local_now {
                            ctx.data_mut(|d| d.insert_temp(last_local_id, now));
                        }

                        if local_is_fresh {
                            if let Some(grab_offset) = ctx.data(|d| d.get_temp::<egui::Vec2>(grab_id))
                                && let Some(pointer_global) = self.drag_state.pointer_global_fallback(ctx)
                            {
                                ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(
                                    pointer_global - grab_offset,
                                ));
                            }
                        } else if self.options.debug_event_log {
                            self.debug_log_event(format!(
                                "detached_window_move SKIP (stale pointer_local) viewport={viewport_id:?} local_now={pointer_local_now:?} local_prev={pointer_local_prev:?}"
                            ));
                        }

                        // End on any release.
                        if ctx.input(|i| i.pointer.any_released()) {
                            ctx.data_mut(|d| {
                                d.remove::<bool>(move_active_id);
                                d.remove::<egui::Vec2>(grab_id);
                                d.remove::<egui::Pos2>(last_local_id);
                            });
                            if self.options.debug_event_log {
                                self.debug_log_event(format!(
                                    "detached_window_move END viewport={viewport_id:?}"
                                ));
                            }
                        } else {
                            ctx.request_repaint_of(ViewportId::ROOT);
                        }
                    }

                    let took_over_internal_drop = self.process_release_before_detached_tree_ui(
                        ctx,
                        behavior,
                        dock_rect,
                        viewport_id,
                        &detached.tree,
                        "internal_overlay_drop_detached",
                    );

                    self.set_tiles_disable_drop_apply_if_taken_over(
                        ctx,
                        detached.tree.id(),
                        viewport_id,
                        took_over_internal_drop,
                    );
                    self.set_tiles_disable_drop_preview_if_overlay_hovered(
                        ctx,
                        behavior,
                        dock_rect,
                        viewport_id,
                        &detached.tree,
                    );

                    if let Some(dragged_tile) = detached.tree.dragged_id_including_root(ctx) {
                        self.observe_tiles_drag_detached();
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
                                super::types::DockPayload {
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

                    self.process_release_after_floating_ui(ctx, dock_rect, viewport_id);

                    if self.options.debug_drop_targets
                        || self.options.debug_event_log
                        || self.options.debug_integrity
                    {
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

            if detached.tree.root.is_none() {
                // Empty: stop calling `show_viewport_immediate` so the native viewport closes.
                if self.options.debug_event_log {
                    self.debug_log_event(format!(
                        "detached_viewport EMPTY -> close viewport={viewport_id:?}"
                    ));
                }
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

    pub(super) fn allocate_detached_viewport_id(&mut self) -> (ViewportId, u64) {
        let serial = self.next_viewport_serial;
        self.next_viewport_serial = self.next_viewport_serial.saturating_add(1);
        (
            ViewportId::from_hash_of(("egui_docking_detached", serial)),
            serial,
        )
    }
}
