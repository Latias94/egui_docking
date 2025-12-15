use egui::emath::GuiRounding as _;
use egui::epaint::MarginF32;
use egui::{Context, Order, Pos2, Rect, Vec2, ViewportBuilder, ViewportId};
use egui_tiles::{Behavior, InsertionPoint, TileId, Tree};

use super::DockingMultiViewport;
use super::geometry::pointer_pos_in_viewport_space;
use super::title::title_for_detached_tree;
use super::types::{
    DockPayload, FloatingDockWindow, FloatingDragState, FloatingId, FloatingResizeState, GhostDrag,
    GhostDragMode,
};

impl<Pane> DockingMultiViewport<Pane> {
    pub(super) fn spawn_floating_subtree_in_viewport(
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

    pub(super) fn ui_floating_windows_in_viewport(
        &mut self,
        ui: &mut egui::Ui,
        behavior: &mut dyn Behavior<Pane>,
        dock_rect: Rect,
        viewport_id: ViewportId,
    ) {
        self.last_floating_rects
            .retain(|(vid, _fid), _| *vid != viewport_id);
        self.last_floating_content_rects
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
            mode: GhostDragMode::Contained { viewport, floating },
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
                                super::types::DetachedDock {
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
                        window.offset_in_dock = (pointer_local - dock_rect.min) - grab_offset;
                        manager.bring_to_front(floating);
                    }
                }
            }
        }

        let bridge_id = self.tree.id();

        let ids = manager.z_order.clone();
        let topmost_id = manager.z_order.last().copied();
        let mut bring_to_front: Vec<FloatingId> = Vec::new();
        let mut close_windows: Vec<FloatingId> = Vec::new();
        let mut dock_windows: Vec<FloatingId> = Vec::new();
        let mut ghost_from_floating: Option<(FloatingId, TileId, Pos2)> = None;

        for floating_id in ids {
            let Some(window) = manager.windows.get_mut(&floating_id) else {
                continue;
            };

            let title = title_for_detached_tree(&window.tree, behavior);

            let mut title_frame = egui::Frame::window(ui.style());
            // We control the outer rect explicitly via `Area` + `allocate_exact_size`.
            // Keep the frame outer margin at zero so it doesn't conceptually "expand" the window.
            title_frame.outer_margin = egui::Margin::ZERO;

            let title_widget_text =
                egui::WidgetText::from(title.clone()).fallback_text_style(egui::TextStyle::Heading);
            let title_galley = title_widget_text.clone().into_galley(
                ui,
                Some(egui::TextWrapMode::Extend),
                f32::INFINITY,
                egui::TextStyle::Heading,
            );
            let title_bar_metrics = egui::containers::window_chrome::title_bar_metrics(
                ui.ctx(),
                &title_widget_text,
                &mut title_frame,
                true,
                window.collapsed,
            );
            let title_height = title_bar_metrics.height_with_margin;
            let title_min_width = {
                let inner_height = (title_height - title_frame.inner_margin.sum().y).max(0.0);
                let item_spacing = ui.spacing().item_spacing;
                let button_size = Vec2::splat(ui.spacing().icon_width.min(inner_height));
                let left_pad = ((inner_height - button_size.y) / 2.0).round_ui();

                let content_min_width =
                    2.0 * (left_pad + button_size.x + item_spacing.x) + title_galley.size().x;

                (content_min_width
                    + title_frame.inner_margin.sum().x
                    + 2.0 * title_frame.stroke.width)
                    .max(96.0)
            };
            let min_size = Vec2::new(220.0, 120.0);
            window.size.x = window.size.x.max(min_size.x);
            window.size.y = window.size.y.max(min_size.y);

            let size = if window.collapsed {
                Vec2::new(title_min_width, title_height)
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

                    let frame = title_frame;

                    let stroke_margin = MarginF32::from(frame.stroke.width);
                    let inner_margin = MarginF32::from(frame.inner_margin);
                    let frame_content_rect = alloc_rect - inner_margin - stroke_margin;
                    ui.painter().add(frame.paint(frame_content_rect));
                    let header_background = ui.painter().add(egui::Shape::Noop);

                    if alloc_resp.clicked() {
                        bring_to_front.push(floating_id);
                    }

                    let title_rect = Rect::from_min_size(
                        frame_content_rect.min,
                        Vec2::new(frame_content_rect.width(), title_height),
                    );

                    let mut title_bar_rect = alloc_rect.shrink(frame.stroke.width);
                    title_bar_rect.max.y = title_bar_rect.min.y + title_height;

                    let title_bar_buttons =
                        egui::containers::window_chrome::title_bar_button_rects(ui, title_bar_rect);
                    let mut title_drag_rect = title_bar_rect;
                    title_drag_rect.min.x = title_bar_buttons.collapse.max.x + 4.0;
                    title_drag_rect.max.x = title_bar_buttons.close.min.x - 4.0;

                    egui::containers::window_chrome::paint_title_bar_background(
                        ui,
                        header_background,
                        title_bar_rect,
                        &frame,
                        frame.fill,
                        window.collapsed,
                        topmost_id == Some(floating_id),
                    );

                    let title_drag_resp = ui.interact(
                        title_drag_rect,
                        ui.id().with((floating_id, "floating_title_drag")),
                        egui::Sense::click_and_drag(),
                    );

                    if title_drag_resp.clicked() || title_drag_resp.drag_started() {
                        bring_to_front.push(floating_id);
                    }

                    if title_drag_resp.drag_started() {
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

                    title_drag_resp.context_menu(|ui| {
                        if ui.button("Dock").clicked() {
                            dock_windows.push(floating_id);
                            ui.close();
                        }
                    });

                    {
                        let collapse_id = ui.id().with((floating_id, "floating_collapse"));
                        let collapse_resp = ui.interact(
                            title_bar_buttons.collapse,
                            collapse_id,
                            egui::Sense::click(),
                        );
                        collapse_resp.widget_info(|| {
                            egui::WidgetInfo::labeled(
                                egui::WidgetType::Button,
                                ui.is_enabled(),
                                if window.collapsed { "Show" } else { "Hide" },
                            )
                        });
                        if collapse_resp.clicked() {
                            window.collapsed = !window.collapsed;
                        }
                        egui::containers::collapsing_header::paint_default_icon(
                            ui,
                            if window.collapsed { 0.0 } else { 1.0 },
                            &collapse_resp,
                        );

                        if egui::containers::window_chrome::window_close_button(
                            ui,
                            title_bar_buttons.close,
                        )
                        .clicked()
                        {
                            close_windows.push(floating_id);
                        }

                        if title_drag_resp.double_clicked() {
                            window.collapsed = !window.collapsed;
                        }

                        let text_pos = egui::emath::align::center_size_in_rect(
                            title_galley.size(),
                            title_bar_rect,
                        )
                        .left_top();
                        let text_pos = text_pos - title_galley.rect.min.to_vec2();
                        ui.painter().galley(
                            text_pos,
                            title_galley.clone(),
                            ui.visuals().text_color(),
                        );
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
                            Rect::from_min_max(title_rect.left_bottom(), frame_content_rect.max);
                        self.last_floating_content_rects
                            .insert((viewport_id, floating_id), content_rect);

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
                let detach_tile = super::pick_detach_tile_for_tree(
                    ui.ctx(),
                    &self.options,
                    &source_window.tree,
                    dragged_tile,
                );
                let pane_rect_last = source_window.tree.tiles.rect(detach_tile);
                let extracted = source_window.tree.extract_subtree(detach_tile);
                let source_empty = source_window.tree.root.is_none();

                if source_empty {
                    if self.options.debug_event_log {
                        self.debug_log_event(format!(
                            "floating_window EMPTY after extract -> remove viewport={viewport_id:?} floating={source_floating:?}"
                        ));
                    }
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

    pub(super) fn dock_subtree_into_dock_tree(
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

    pub(super) fn extract_subtree_from_floating(
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
                if self.options.debug_event_log {
                    self.debug_log_event(format!(
                        "floating_window EMPTY after extract_subtree -> remove viewport={viewport_id:?} floating={floating_id:?}"
                    ));
                }
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

    pub(super) fn take_whole_floating_tree(
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

    pub(super) fn floating_under_pointer(
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

    pub(super) fn floating_under_pointer_excluding(
        &self,
        viewport_id: ViewportId,
        pointer_local: Pos2,
        excluded: Option<FloatingId>,
    ) -> Option<FloatingId> {
        let manager = self.floating.get(&viewport_id)?;
        for id in manager.z_order.iter().rev().copied() {
            if excluded == Some(id) {
                continue;
            }
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

    pub(super) fn floating_content_under_pointer_excluding(
        &self,
        viewport_id: ViewportId,
        pointer_local: Pos2,
        excluded: Option<FloatingId>,
    ) -> Option<FloatingId> {
        let manager = self.floating.get(&viewport_id)?;
        for id in manager.z_order.iter().rev().copied() {
            if excluded == Some(id) {
                continue;
            }
            if self
                .last_floating_content_rects
                .get(&(viewport_id, id))
                .is_some_and(|r| r.contains(pointer_local))
            {
                return Some(id);
            }
        }
        None
    }

    pub(super) fn floating_tree_id_under_pointer(
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

    pub(super) fn floating_tree_id_under_pointer_excluding(
        &self,
        viewport_id: ViewportId,
        pointer_local: Pos2,
        excluded: Option<FloatingId>,
    ) -> Option<egui::Id> {
        let floating_id =
            self.floating_under_pointer_excluding(viewport_id, pointer_local, excluded)?;
        self.floating
            .get(&viewport_id)?
            .windows
            .get(&floating_id)
            .map(|w| w.tree.id())
    }
}
