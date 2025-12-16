use egui::{Context, Rect, ViewportId};
use egui_tiles::{Behavior, TileId, Tree};

use super::DockingMultiViewport;
use super::drop_policy;
use super::geometry::viewport_under_pointer_global_excluding;
use super::overlay_decision::{decide_overlay_for_tree, DragKind};
use super::surface::DockSurface;
use super::host::WindowHost;
use super::types::{DockPayload, FloatingId, PendingDrop, PendingInternalDrop, PendingLocalDrop};

impl<Pane> DockingMultiViewport<Pane> {
    pub(super) fn queue_pending_local_drop_on_release(
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

        // Floating windows are top-most surfaces inside `dock_rect`.
        // When dragging a floating window, treat it as transparent so you can dock it back into
        // the underlying dock (ImGui behavior).
        let is_moving_floating_window =
            payload.source_floating.is_some() && payload.tile_id.is_none();
        let exclude_floating =
            drop_policy::exclude_floating_for_hit_test(payload.source_floating, payload.tile_id);
        let Some(target_surface) = self.surface_under_pointer_local(
            viewport_id,
            dock_rect,
            pointer_local,
            exclude_floating,
        ) else {
            if self.options.debug_event_log {
                self.debug_log_event(format!(
                    "local_drop_skip outside_dock viewport={viewport_id:?} source_floating={:?} tile_id={:?} pointer_local=({:.1},{:.1})",
                    payload.source_floating,
                    payload.tile_id,
                    pointer_local.x,
                    pointer_local.y,
                ));
            }
            return;
        };

        if !is_moving_floating_window
            && matches!(target_surface, DockSurface::Floating { floating, .. } if Some(floating) == payload.source_floating)
        {
            if self.options.debug_event_log {
                self.debug_log_event(format!(
                    "local_drop_skip same_floating viewport={viewport_id:?} floating={:?}",
                    payload.source_floating
                ));
            }
            return;
        }
        if payload.source_floating.is_none() && payload.tile_id.is_none() {
            // We don't support moving the whole dock tree within a viewport.
            if self.options.debug_event_log {
                self.debug_log_event(format!(
                    "local_drop_skip whole_tree_not_supported viewport={viewport_id:?}"
                ));
            }
            return;
        }

        if drop_policy::should_skip_local_drop_internal_dock_to_dock(
            payload.as_ref(),
            viewport_id,
            target_surface,
        ) {
            if self.options.debug_event_log {
                self.debug_log_event(format!(
                    "local_drop_skip internal_dock_to_dock viewport={viewport_id:?} payload={:?} target_surface={target_surface:?}",
                    *payload
                ));
            }
            return;
        }

        if !self.try_take_release_action("local_drop") {
            if self.options.debug_event_log {
                self.debug_log_event(format!(
                    "local_drop_skip release_taken viewport={viewport_id:?}"
                ));
            }
            return;
        }

        if self.options.debug_event_log {
            self.debug_log_event(format!(
                "queue_local_drop viewport={viewport_id:?} source_floating={:?} tile_id={:?} target_surface={target_surface:?} pointer_local=({:.1},{:.1})",
                payload.source_floating,
                payload.tile_id,
                pointer_local.x,
                pointer_local.y,
            ));
        }

        let target_host = match target_surface {
            DockSurface::DockTree { viewport } => WindowHost::DockTree { viewport },
            DockSurface::Floating { viewport, floating } => WindowHost::Floating { viewport, floating },
        };
        let modifiers = ctx.input(|i| i.modifiers);
        self.pending_local_drop = Some(PendingLocalDrop {
            payload: *payload,
            target_surface,
            target_host,
            pointer_local,
            modifiers,
        });

        egui::DragAndDrop::clear_payload(ctx);
        ctx.stop_dragging();
        ctx.request_repaint_of(ViewportId::ROOT);
    }

    pub(super) fn queue_pending_local_drop_from_dragged_tile_on_release(
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

        let Some(target_surface) =
            self.surface_under_pointer_local(viewport_id, dock_rect, pointer_local, None)
        else {
            return;
        };

        // If you are still inside the same floating window, let `egui_tiles` handle internal drops/reorder.
        if matches!(target_surface, DockSurface::Floating { floating, .. } if Some(floating) == source_floating)
        {
            return;
        }

        if source_floating.is_none() && matches!(target_surface, DockSurface::DockTree { .. }) {
            return;
        }

        if !self.try_take_release_action("local_drop_from_dragged_tile") {
            return;
        }

        self.pending_local_drop = Some(PendingLocalDrop {
            payload: DockPayload {
                bridge_id: self.tree.id(),
                source_viewport: viewport_id,
                source_floating,
                tile_id: Some(dragged_tile),
            },
            target_surface,
            target_host: match target_surface {
                DockSurface::DockTree { viewport } => WindowHost::DockTree { viewport },
                DockSurface::Floating { viewport, floating } => {
                    WindowHost::Floating { viewport, floating }
                }
            },
            pointer_local,
            modifiers: ctx.input(|i| i.modifiers),
        });

        egui::DragAndDrop::clear_payload(ctx);
        ctx.stop_dragging();
        ctx.request_repaint_of(ViewportId::ROOT);
    }

    pub(super) fn queue_pending_drop_on_release(&mut self, ctx: &Context) {
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
        let pointer_global = self.drag_state.pointer_global_fallback(ctx);
        let Some(pointer_global) = pointer_global else {
            return;
        };

        let exclude_viewport = payload.tile_id.is_none().then_some(payload.source_viewport);
        let Some(target_viewport) =
            viewport_under_pointer_global_excluding(ctx, pointer_global, exclude_viewport)
        else {
            return;
        };
        if target_viewport == payload.source_viewport {
            return;
        }

        if !self.try_take_release_action("cross_viewport_drop") {
            return;
        }

        let modifiers = ctx.input(|i| i.modifiers);
        self.pending_drop = Some(PendingDrop {
            payload: *payload,
            pointer_global,
            modifiers,
        });

        egui::DragAndDrop::clear_payload(ctx);
        ctx.stop_dragging();
        ctx.request_repaint_of(ViewportId::ROOT);
    }

    pub(super) fn pending_internal_overlay_drop_on_release(
        &self,
        ctx: &Context,
        behavior: &dyn Behavior<Pane>,
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
        let insertion = decision.insertion_final?;

        Some(PendingInternalDrop {
            viewport: viewport_id,
            tile_id: dragged_tile,
            insertion,
        })
    }
}
