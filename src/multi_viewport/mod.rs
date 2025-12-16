use std::collections::{BTreeMap, VecDeque};

use egui::{Context, LayerId, Order, Pos2, Rect, ViewportId};
use egui_tiles::{Behavior, ContainerKind, InsertionPoint, Tile, TileId, Tree};

mod debug;
mod detached;
mod drop_apply;
mod drop_policy;
mod drop_queue;
mod drop_sanitize;
mod floating;
mod geometry;
mod ghost;
mod host;
mod integrity;
mod options;
mod overlay;
mod overlay_decision;
mod session;
mod surface;
mod title;
mod types;

#[cfg(test)]
mod model_tests;
#[cfg(test)]
mod world_model_tests;
#[cfg(test)]
mod overlay_decision_tests;

pub use options::DockingMultiViewportOptions;

use debug::{debug_clear_event_log_id, last_drop_debug_text_id, tiles_debug_visit_enabled_id};
use geometry::{pointer_pos_in_global, pointer_pos_in_viewport_space};
use overlay::{paint_outer_overlay, paint_overlay, pointer_in_outer_band};
use overlay_decision::{decide_overlay_for_tree, DragKind, OverlayPaint};
use session::DragSession;
use types::*;

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
    last_drag_modifiers: egui::Modifiers,

    pending_drop: Option<PendingDrop>,
    pending_internal_drop: Option<PendingInternalDrop>,
    pending_local_drop: Option<PendingLocalDrop>,

    floating: BTreeMap<ViewportId, FloatingManager<Pane>>,
    next_floating_serial: u64,
    last_floating_rects: BTreeMap<(ViewportId, FloatingId), Rect>,
    last_floating_content_rects: BTreeMap<(ViewportId, FloatingId), Rect>,

    ghost: Option<GhostDrag>,

    debug_log: VecDeque<String>,
    debug_frame: u64,
    debug_last_disable_drop_apply: BTreeMap<(u64, ViewportId), bool>,
    debug_last_integrity_hash: BTreeMap<(u64, ViewportId), u64>,

    drag_session: DragSession,

    detached_rendered_frame: BTreeMap<ViewportId, u64>,
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
            last_drag_modifiers: Default::default(),
            pending_drop: None,
            pending_internal_drop: None,
            pending_local_drop: None,
            floating: BTreeMap::new(),
            next_floating_serial: 1,
            last_floating_rects: BTreeMap::new(),
            last_floating_content_rects: BTreeMap::new(),
            ghost: None,
            debug_log: VecDeque::new(),
            debug_frame: 0,
            debug_last_disable_drop_apply: BTreeMap::new(),
            debug_last_integrity_hash: BTreeMap::new(),
            drag_session: DragSession::default(),
            detached_rendered_frame: BTreeMap::new(),
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
        self.drag_session.begin_frame();
        self.detached_rendered_frame
            .retain(|viewport_id, _| self.detached.contains_key(viewport_id));
        if self.options.debug_event_log || self.options.debug_integrity {
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
            self.observe_drag_sources_in_ctx(ui.ctx());

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
                                behavior,
                                dock_rect,
                                ViewportId::ROOT,
                                &self.tree,
                            )
                        } else {
                    None
                };
            let took_over_internal_drop = internal_drop.is_some();
            if let Some(pending) = internal_drop {
                if !self.try_take_release_action("internal_overlay_drop_root") {
                    // Another release handler already took ownership this frame.
                } else {
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
            }

            self.set_tiles_disable_drop_apply_if_taken_over(
                ui.ctx(),
                self.tree.id(),
                ViewportId::ROOT,
                took_over_internal_drop,
            );
            self.set_tiles_disable_drop_preview_if_overlay_hovered(
                ui.ctx(),
                behavior,
                dock_rect,
                ViewportId::ROOT,
                &self.tree,
            );

            if let Some(dragged_tile) = self.tree.dragged_id_including_root(ui.ctx()) {
                self.observe_tiles_drag_root();
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

        // 3) Late detached viewports: ghost tear-off can create new viewports during the root UI.
        //    Render newly-created viewports once in the same frame to reduce perceived latency.
        self.ui_detached_viewports(ctx, behavior);

        if self.options.debug_drop_targets
            || self.options.debug_event_log
            || self.options.debug_integrity
        {
            self.ui_debug_window(ctx, ViewportId::ROOT, self.tree.id());
        }

        // Apply after all viewports have had a chance to run `tree.ui` this frame so we can use
        // the computed rectangles for accurate docking.
        self.apply_pending_actions(ctx, behavior);
        self.clear_bridge_payload_on_release(ctx);
        self.finish_ghost_if_released_or_aborted(ctx);

        if self.options.debug_integrity {
            self.debug_check_integrity_all();
        }

        if let Some(msg) = self.drag_session.end_frame(self.debug_frame) {
            self.debug_log_event(msg);
        }
    }

    fn update_last_pointer_global_from_active_viewport(&mut self, ctx: &Context) {
        if let Some(pos) = pointer_pos_in_global(ctx) {
            self.last_pointer_global = Some(pos);
        }
    }

    fn window_move_docking_enabled_now(&self, ctx: &Context) -> bool {
        let shift = if egui::DragAndDrop::payload::<DockPayload>(ctx)
            .is_some_and(|p| p.bridge_id == self.tree.id() && p.tile_id.is_none())
        {
            self.last_drag_modifiers.shift
        } else {
            ctx.input(|i| i.modifiers.shift)
        };
        self.options.window_move_docking_enabled_by_shift(shift)
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
        if !(self.options.debug_drop_targets
            || self.options.debug_event_log
            || self.options.debug_integrity)
        {
            return;
        }
        ctx.data_mut(|d| {
            d.insert_temp(tiles_debug_visit_enabled_id(tree_id, viewport_id), true);
        });
    }

    fn observe_drag_sources_in_ctx(&mut self, ctx: &Context) {
        if self.ghost.is_some() {
            if let Some(msg) = self.drag_session.observe_active(self.debug_frame, "ghost") {
                self.debug_log_event(msg);
            }
        }

        let Some(payload) = egui::DragAndDrop::payload::<DockPayload>(ctx) else {
            return;
        };
        if payload.bridge_id != self.tree.id() {
            return;
        }
        if ctx.viewport_id() == payload.source_viewport {
            self.last_drag_modifiers = ctx.input(|i| i.modifiers);
        }
        if let Some(msg) = self
            .drag_session
            .observe_active(self.debug_frame, "payload")
        {
            self.debug_log_event(msg);
        }
    }

    fn observe_tiles_drag_root(&mut self) {
        if let Some(msg) = self
            .drag_session
            .observe_active(self.debug_frame, "tiles_drag_root")
        {
            self.debug_log_event(msg);
        }
    }

    fn observe_tiles_drag_detached(&mut self) {
        if let Some(msg) = self
            .drag_session
            .observe_active(self.debug_frame, "tiles_drag_detached")
        {
            self.debug_log_event(msg);
        }
    }

    fn try_take_release_action(&mut self, kind: &'static str) -> bool {
        let (ok, msg) = self
            .drag_session
            .take_release_action(self.debug_frame, kind);
        if let Some(msg) = msg {
            self.debug_log_event(msg);
        }
        ok
    }

    fn try_take_release_action_silent_if_taken(&mut self, kind: &'static str) -> bool {
        let (ok, msg) = self
            .drag_session
            .take_release_action(self.debug_frame, kind);
        if ok {
            if let Some(msg) = msg {
                self.debug_log_event(msg);
            }
        }
        ok
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
            let prev = self
                .debug_last_disable_drop_apply
                .insert(key, disable_drop_apply);
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
        behavior: &dyn Behavior<Pane>,
        dock_rect: Rect,
        viewport_id: ViewportId,
        tree: &Tree<Pane>,
    ) {
        let disable_preview = if !self.options.show_overlay_for_internal_drags {
            false
        } else if self.options.detach_on_alt_release_anywhere && ctx.input(|i| i.modifiers.alt) {
            false
        } else {
            let dragged_tile = tree.dragged_id_including_root(ctx);
            let pointer_local = ctx.input(|i| i.pointer.latest_pos());
            dragged_tile.zip(pointer_local).is_some_and(|(dragged_tile, pointer_local)| {
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

                let style = ctx.global_style();
                let decision = decide_overlay_for_tree(
                    tree,
                    behavior,
                    &style,
                    dock_rect,
                    pointer_local,
                    self.options.show_outer_overlay_targets,
                    DragKind::Subtree {
                        dragged_tile: Some(dragged_tile),
                        internal: true,
                    },
                );
                decision.disable_tiles_preview
            })
        };

        ctx.data_mut(|d| {
            d.insert_temp(
                tiles_disable_drop_preview_id(tree.id(), viewport_id),
                disable_preview,
            );
            d.insert_temp(
                tiles_disable_dragged_overlay_id(tree.id(), viewport_id),
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
        let is_window_move = payload.tile_id.is_none();
        let window_move_docking_enabled = !is_window_move || self.window_move_docking_enabled_now(ui.ctx());

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
        let excluded_floating = (payload.source_viewport == target_viewport)
            .then(|| payload.source_floating)
            .flatten();
        if self
            .floating_tree_id_under_pointer_excluding(
                target_viewport,
                pointer_local,
                excluded_floating,
            )
            .is_some_and(|floating_tree_id| floating_tree_id != tree.id())
        {
            return;
        }

        let internal_dragged_tile = (payload.source_viewport == target_viewport)
            .then(|| tree.dragged_id_including_root(ui.ctx()))
            .flatten();
        let drag_kind = if is_window_move {
            DragKind::WindowMove
        } else {
            DragKind::Subtree {
                dragged_tile: internal_dragged_tile,
                internal: internal_dragged_tile.is_some(),
            }
        };
        if matches!(drag_kind, DragKind::Subtree { internal: true, .. })
            && !self.options.show_overlay_for_internal_drags
        {
            return;
        }

        let style = ui.ctx().global_style();
        let decision = if window_move_docking_enabled {
            decide_overlay_for_tree(
                tree,
                behavior,
                &style,
                dock_rect,
                pointer_local,
                self.options.show_outer_overlay_targets,
                drag_kind,
            )
        } else {
            overlay_decision::OverlayDecision {
                paint: None,
                insertion_explicit: None,
                fallback_zone: None,
                insertion_final: None,
                disable_tiles_preview: false,
            }
        };

        if window_move_docking_enabled {
            if let Some(paint) = decision.paint {
            match paint {
                OverlayPaint::Inner(overlay) => {
                    let painter = ui.ctx().layer_painter(LayerId::new(
                        Order::Foreground,
                        egui::Id::new((tree.id(), target_viewport, "egui_docking_overlay")),
                    ));
                    paint_overlay(&painter, ui.visuals(), overlay);
                }
                OverlayPaint::Outer(overlay) => {
                    let painter = ui.ctx().layer_painter(LayerId::new(
                        Order::Foreground,
                        egui::Id::new((tree.id(), target_viewport, "egui_docking_outer_overlay")),
                    ));
                    paint_outer_overlay(&painter, ui.visuals(), overlay);
                }
            }
        }
        }

        // Subtree moves: if no explicit target is hit, fall back to `dock_zone_at` preview,
        // matching `egui_tiles` behavior. Window moves: fall back to "dock as tab" preview.
        if window_move_docking_enabled
            && matches!(drag_kind, DragKind::Subtree { internal: false, .. } | DragKind::WindowMove)
            && decision.insertion_explicit.is_none()
        {
            if let Some(zone) = decision.fallback_zone {
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
        }

        if self.options.debug_drop_targets {
            let mut lines: Vec<String> = Vec::new();
            lines.push(format!("viewport={target_viewport:?}"));
            lines.push(format!("tree_root={:?}", tree.root));
            lines.push(format!(
                "dragged_id={:?}",
                tree.dragged_id_including_root(ui.ctx())
            ));
            lines.push(format!(
                "payload_source_host={:?}",
                payload.source_host()
            ));
            lines.push(format!("payload_tile_id={:?}", payload.tile_id));
            let outer_mode =
                self.options.show_outer_overlay_targets && pointer_in_outer_band(dock_rect, pointer_local);
            lines.push(format!(
                "pointer_local=({:.1},{:.1}) outer_mode={outer_mode}",
                pointer_local.x, pointer_local.y
            ));
            let hovered_floating_raw = self.floating_under_pointer(target_viewport, pointer_local);
            let hovered_floating_excluding_source = self.floating_under_pointer_excluding(
                target_viewport,
                pointer_local,
                excluded_floating,
            );
            lines.push(format!(
                "hovered_floating_raw={hovered_floating_raw:?} hovered_floating_excluding_source={hovered_floating_excluding_source:?}"
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

            lines.push(format!("is_cross_viewport={is_cross_viewport}"));
            lines.push(format!("drag_kind={drag_kind:?}"));
            if is_window_move {
                lines.push(format!(
                    "window_move_docking_enabled={window_move_docking_enabled} config_docking_with_shift={} shift_held={}",
                    self.options.config_docking_with_shift,
                    ui.ctx().input(|i| i.modifiers.shift),
                ));
            }
            lines.push(format!(
                "overlay_paint={}",
                match decision.paint {
                    Some(OverlayPaint::Inner(_)) => "Inner",
                    Some(OverlayPaint::Outer(_)) => "Outer",
                    None => "None",
                }
            ));
            lines.push(format!(
                "overlay_hover={:?}",
                decision.paint.and_then(|p| p.hovered_target())
            ));
            lines.push(format!(
                "fallback_insertion={:?}",
                decision.fallback_zone.map(|z| z.insertion_point)
            ));
            lines.push(format!("insertion_explicit={:?}", decision.insertion_explicit));
            lines.push(format!("insertion_final={:?}", decision.insertion_final));
            lines.push(format!(
                "disable_tiles_preview={}",
                decision.disable_tiles_preview
            ));

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

            let debug_id = egui::Id::new((
                tree.id(),
                target_viewport,
                "egui_docking_debug_drop_targets",
            ));
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
                                .id_salt((tree.id(), target_viewport, "egui_docking_debug_drop_targets_log"))
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

fn tiles_disable_dragged_overlay_id(tree_id: egui::Id, _viewport_id: ViewportId) -> egui::Id {
    egui::Id::new((tree_id, "egui_docking_disable_dragged_overlay"))
}
