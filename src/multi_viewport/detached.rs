use egui::{Context, ViewportClass, ViewportId};
use egui_tiles::Behavior;

use super::DockingMultiViewport;
use super::title::title_for_detached_tree;
use super::types::{GhostDrag, GhostDragMode};

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

                egui::Panel::top("egui_docking_detached_top_bar").show(ctx, |ui| {
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
                                super::types::DockPayload {
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
                        if !self.try_take_release_action("internal_overlay_drop_detached") {
                            // Another release handler already took ownership this frame.
                        } else {
                            self.debug_log_event(format!(
                                "queue_internal_drop viewport={:?} tile_id={:?} insertion={:?}",
                                pending.viewport, pending.tile_id, pending.insertion
                            ));
                            ctx.stop_dragging();
                            if let Some(payload) =
                                egui::DragAndDrop::payload::<super::types::DockPayload>(ctx)
                            {
                                if payload.bridge_id == self.tree.id()
                                    && payload.source_viewport == pending.viewport
                                {
                                    egui::DragAndDrop::clear_payload(ctx);
                                }
                            }

                            self.pending_internal_drop = Some(pending);
                            ctx.request_repaint_of(ViewportId::ROOT);
                        }
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

                    self.queue_pending_local_drop_on_release(ctx, dock_rect, viewport_id);
                    self.clear_bridge_payload_if_released_in_ctx(ctx);

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
