use egui::emath::GuiRounding as _;
use egui::{Context, Order, Rect, ResizeDirection, ViewportClass, ViewportCommand, ViewportId};
use egui_tiles::Behavior;

use super::DockingMultiViewport;
use super::geometry::outer_position_for_window_move;
use super::title::title_for_detached_tree;
use super::types::{GhostDrag, GhostDragMode};
use super::types::DockPayload;

impl<Pane> DockingMultiViewport<Pane> {
    fn start_detached_window_move(&mut self, ctx: &Context, viewport_id: ViewportId) {
        let bridge_id = self.tree.id();
        let move_active_id = self.detached_window_move_active_id(viewport_id);

        let already_active = ctx
            .data(|d| d.get_temp::<bool>(move_active_id))
            .unwrap_or(false);
        if already_active {
            return;
        }

        self.observe_tiles_drag_detached();
        ctx.data_mut(|d| d.insert_temp(move_active_id, true));

        if self.options.focus_detached_on_custom_title_drag {
            ctx.send_viewport_cmd(ViewportCommand::Focus);
        }

        // Prefer OS/native window dragging for detached viewport moves.
        //
        // This avoids the self-referential coordinate feedback that happens when we drive
        // `OuterPosition` from window-local pointer deltas (and it works even on platforms where
        // raw device motion isn't available).
        ctx.send_viewport_cmd(ViewportCommand::StartDrag);

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
                "detached_window_move START viewport={viewport_id:?}"
            ));
        }
    }

    fn ui_borderless_detached_chrome(
        &mut self,
        ctx: &Context,
        behavior: &mut dyn Behavior<Pane>,
        viewport_id: ViewportId,
        title: &str,
        should_redock_to_root: &mut bool,
    ) {
        let style = ctx.global_style();
        let bar_height = behavior.tab_bar_height(&style).max(24.0);
        let bar_id = egui::Id::new((self.tree.id(), viewport_id, "borderless_title_bar"));

        egui::Panel::top(bar_id)
            .exact_size(bar_height)
            .frame(egui::Frame::new().fill(style.visuals.window_fill()))
            .show(ctx, |ui| {
                let rect = ui.max_rect();
                let drag_id = ui.id().with("drag");
                let drag = ui.interact(rect, drag_id, egui::Sense::click_and_drag());
                if drag.drag_started() {
                    self.start_detached_window_move(ctx, viewport_id);
                }

                let button_rects =
                    egui::containers::window_chrome::title_bar_button_rects(ui, rect);
                let close = egui::containers::window_chrome::window_close_button(
                    ui,
                    button_rects.close,
                );
                if close.clicked() {
                    *should_redock_to_root = true;
                }

                let text_rect = Rect::from_min_max(
                    rect.min + egui::vec2(8.0, 0.0),
                    rect.max - egui::vec2(ui.spacing().icon_width + 12.0, 0.0),
                );
                ui.scope_builder(egui::UiBuilder::new().max_rect(text_rect), |ui| {
                    ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                        ui.label(egui::RichText::new(title).strong());
                    });
                })
                .response
            });
    }

    fn ui_borderless_resize_handles(&self, ctx: &Context, viewport_id: ViewportId) {
        let thickness = 6.0;
        let corner = 14.0;
        let screen_rect = ctx.input(|i| i.content_rect());

        fn dir_key(dir: ResizeDirection) -> u8 {
            match dir {
                ResizeDirection::North => 0,
                ResizeDirection::South => 1,
                ResizeDirection::East => 2,
                ResizeDirection::West => 3,
                ResizeDirection::NorthEast => 4,
                ResizeDirection::SouthEast => 5,
                ResizeDirection::NorthWest => 6,
                ResizeDirection::SouthWest => 7,
            }
        }

        let layer_id = egui::Id::new((self.tree.id(), viewport_id, "borderless_resize_area"));
        egui::Area::new(layer_id)
            .order(Order::Foreground)
            .fixed_pos(screen_rect.min)
            .show(ctx, |ui| {
                ui.set_clip_rect(screen_rect);
                let ui = ui.new_child(egui::UiBuilder::new().max_rect(screen_rect));

                let make = |rect: Rect, dir: ResizeDirection, cursor: egui::CursorIcon| {
                    let id = egui::Id::new((
                        self.tree.id(),
                        viewport_id,
                        "borderless_resize",
                        dir_key(dir),
                    ));
                    let resp = ui
                        .interact(rect, id, egui::Sense::click_and_drag())
                        .on_hover_cursor(cursor);
                    if resp.drag_started() {
                        ctx.send_viewport_cmd(ViewportCommand::BeginResize(dir));
                    }
                };

                let r = screen_rect;
                let left = Rect::from_min_max(
                    r.min,
                    egui::pos2(r.min.x + thickness, r.max.y),
                );
                let right = Rect::from_min_max(
                    egui::pos2(r.max.x - thickness, r.min.y),
                    r.max,
                );
                let top = Rect::from_min_max(
                    r.min,
                    egui::pos2(r.max.x, r.min.y + thickness),
                );
                let bottom = Rect::from_min_max(
                    egui::pos2(r.min.x, r.max.y - thickness),
                    r.max,
                );

                let nw = Rect::from_min_max(r.min, r.min + egui::vec2(corner, corner));
                let ne = Rect::from_min_max(
                    egui::pos2(r.max.x - corner, r.min.y),
                    egui::pos2(r.max.x, r.min.y + corner),
                );
                let sw = Rect::from_min_max(
                    egui::pos2(r.min.x, r.max.y - corner),
                    egui::pos2(r.min.x + corner, r.max.y),
                );
                let se = Rect::from_min_max(r.max - egui::vec2(corner, corner), r.max);

                make(nw, ResizeDirection::NorthWest, egui::CursorIcon::ResizeNwSe);
                make(ne, ResizeDirection::NorthEast, egui::CursorIcon::ResizeNeSw);
                make(sw, ResizeDirection::SouthWest, egui::CursorIcon::ResizeNeSw);
                make(se, ResizeDirection::SouthEast, egui::CursorIcon::ResizeNwSe);
                make(left, ResizeDirection::West, egui::CursorIcon::ResizeHorizontal);
                make(right, ResizeDirection::East, egui::CursorIcon::ResizeHorizontal);
                make(top, ResizeDirection::North, egui::CursorIcon::ResizeVertical);
                make(bottom, ResizeDirection::South, egui::CursorIcon::ResizeVertical);
            });
    }

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

            let builder = detached
                .builder
                .clone()
                .with_decorations(self.options.detached_viewport_decorations);
            let mut should_redock_to_root = false;

            ctx.show_viewport_immediate(viewport_id, builder, |ctx, class| {
                self.update_last_pointer_global_from_active_viewport(ctx);
                self.update_viewport_outer_from_inner_offset(ctx);
                self.observe_drag_sources_in_ctx(ctx);

                if let Some(GhostDrag {
                    mode: GhostDragMode::Native { viewport },
                    grab_offset,
                }) = self.ghost
                {
                    if viewport == viewport_id {
                        if let Some(pointer_global) = self.drag_state.last_pointer_global() {
                            let outer_from_inner =
                                self.viewport_outer_from_inner_offset(viewport_id);
                            let desired_outer = outer_position_for_window_move(
                                pointer_global,
                                grab_offset,
                                outer_from_inner,
                            )
                            .round_to_pixels(ctx.pixels_per_point())
                            .round_ui();

                            let last_id = egui::Id::new((
                                bridge_id,
                                viewport_id,
                                "egui_docking_ghost_native_last_sent_outer",
                            ));
                            let prev = ctx.data(|d| d.get_temp::<egui::Pos2>(last_id));
                            let should_send = match prev {
                                Some(prev) => desired_outer != prev,
                                None => true,
                            };
                            if should_send {
                                ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(
                                    desired_outer,
                                ));
                                ctx.data_mut(|d| d.insert_temp(last_id, desired_outer));

                                if self.options.debug_event_log {
                                    let step_id = egui::Id::new((
                                        bridge_id,
                                        viewport_id,
                                        "egui_docking_ghost_native_move_step_count",
                                    ));
                                    let step = ctx
                                        .data(|d| d.get_temp::<u64>(step_id).unwrap_or(0))
                                        .saturating_add(1);
                                    ctx.data_mut(|d| d.insert_temp(step_id, step));

                                    let log_every_send = self.options.debug_log_window_move_every_send
                                        && self.options.debug_log_file_path.is_some();
                                    if log_every_send || step == 1 || step % 20 == 0 {
                                        self.debug_log_event(format!(
                                            "ghost_window_move step={step} viewport={:?} pointer_global=({:.1},{:.1}) grab_in_inner=({:.1},{:.1}) outer_from_inner=({:.1},{:.1}) desired_outer=({:.1},{:.1})",
                                            viewport_id,
                                            pointer_global.x,
                                            pointer_global.y,
                                            grab_offset.x,
                                            grab_offset.y,
                                            outer_from_inner.x,
                                            outer_from_inner.y,
                                            desired_outer.x,
                                            desired_outer.y
                                        ));
                                    }
                                }
                            }
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

                if !self.options.detached_viewport_decorations {
                    self.ui_borderless_detached_chrome(
                        ctx,
                        behavior,
                        viewport_id,
                        &title,
                        &mut should_redock_to_root,
                    );
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
                    let move_active_id = self.detached_window_move_active_id(viewport_id);

                    let (root_is_tabs, root_tabs_single_child) = detached
                        .tree
                        .root
                        .and_then(|root| detached.tree.tiles.get(root).map(|t| (root, t)))
                        .and_then(|(_root, tile)| match tile {
                            egui_tiles::Tile::Container(container)
                                if container.kind() == egui_tiles::ContainerKind::Tabs =>
                            {
                                let children: Vec<egui_tiles::TileId> =
                                    container.children().copied().collect();
                                let single_child = (children.len() == 1).then_some(children[0]);
                                Some((true, single_child))
                            }
                            _ => Some((false, None)),
                        })
                        .unwrap_or((false, None));
                    let dragged_tile = detached.tree.dragged_id_including_root(ctx);
                    let should_start_window_move =
                        root_is_tabs && dragged_tile == detached.tree.root && ctx.data(|d| d.get_temp::<bool>(move_active_id)).unwrap_or(false) == false;

                    if should_start_window_move {
                        self.start_detached_window_move(ctx, viewport_id);
                    }

                    let window_move_active =
                        ctx.data(|d| d.get_temp::<bool>(move_active_id)).unwrap_or(false);
                    if window_move_active {
                        // End on any release.
                        if ctx.input(|i| i.pointer.any_released()) {
                            self.clear_detached_window_move_state(ctx, viewport_id);
                            if self.options.debug_event_log {
                                self.debug_log_event(format!(
                                    "detached_window_move END viewport={viewport_id:?}"
                                ));
                            }
                        } else {
                            // Keep both the moving window and the root preview alive while dragging.
                            ctx.request_repaint();
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

                    // If the detached viewport is a single-tab host, treat dragging that lone tab
                    // as a window-move drag (ImGui-style), so the native viewport follows the cursor
                    // while docking back.
                    let window_move_active =
                        ctx.data(|d| d.get_temp::<bool>(move_active_id)).unwrap_or(false);
                    let dragged_tile_after_ui = detached.tree.dragged_id_including_root(ctx);
                    let should_upgrade_single_tab_to_window_move = !window_move_active
                        && root_tabs_single_child.is_some_and(|child| Some(child) == dragged_tile_after_ui);
                    let should_start_window_move_late =
                        !window_move_active && root_is_tabs && dragged_tile_after_ui == detached.tree.root;
                    if should_upgrade_single_tab_to_window_move || should_start_window_move_late {
                        self.start_detached_window_move(ctx, viewport_id);
                    }

                    if self.pending_drop.is_none()
                        && self.pending_local_drop.is_none()
                        && self.ghost.is_none()
                    {
                        // If we just upgraded to window-move mode above, do not re-install a tile payload.
                        let window_move_active =
                            ctx.data(|d| d.get_temp::<bool>(move_active_id)).unwrap_or(false);
                        if !window_move_active
                            && let Some(dragged_tile) = detached.tree.dragged_id_including_root(ctx)
                        {
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

                if !self.options.detached_viewport_decorations {
                    self.ui_borderless_resize_handles(ctx, viewport_id);
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
                .with_title(title_for_detached_tree(&detached.tree, behavior))
                .with_decorations(self.options.detached_viewport_decorations);
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
