use egui::{Context, Rect, ViewportId};
use egui_tiles::{Behavior, Tree};

use super::DockingMultiViewport;

impl<Pane> DockingMultiViewport<Pane> {
    /// Phase A (per-viewport, early): handle release-driven actions that depend on the dock tree
    /// geometry but should run *before* we consider ghost tear-off and payload re-seeding.
    ///
    /// Returns `true` if we took over an internal tiles drop (overlay hovered).
    pub(super) fn process_release_before_root_tree_ui(
        &mut self,
        ctx: &Context,
        behavior: &mut dyn Behavior<Pane>,
        dock_rect: Rect,
    ) -> bool {
        // Cross-viewport drop has priority: queue it as early as possible.
        self.queue_pending_drop_on_release(ctx);

        let internal_drop = if self.pending_drop.is_none() && self.pending_internal_drop.is_none() {
            self.pending_internal_overlay_drop_on_release(
                ctx,
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
            if self.try_take_release_action("internal_overlay_drop_root") {
                if self.options.debug_event_log {
                    self.debug_log_event(format!(
                        "queue_internal_drop viewport={:?} tile_id={:?} insertion={:?}",
                        pending.viewport, pending.tile_id, pending.insertion
                    ));
                }
                ctx.stop_dragging();
                if let Some(payload) = egui::DragAndDrop::payload::<super::types::DockPayload>(ctx) {
                    if payload.bridge_id == self.tree.id() && payload.source_viewport == pending.viewport {
                        egui::DragAndDrop::clear_payload(ctx);
                    }
                }
                self.pending_internal_drop = Some(pending);
                ctx.request_repaint_of(ViewportId::ROOT);
            }
        }

        took_over_internal_drop
    }

    pub(super) fn process_release_before_detached_tree_ui(
        &mut self,
        ctx: &Context,
        behavior: &mut dyn Behavior<Pane>,
        dock_rect: Rect,
        viewport_id: ViewportId,
        tree: &Tree<Pane>,
        internal_overlay_kind: &'static str,
    ) -> bool {
        // Cross-viewport drop has priority: queue it as early as possible.
        self.queue_pending_drop_on_release(ctx);

        let internal_drop = if self.pending_drop.is_none() && self.pending_internal_drop.is_none() {
            self.pending_internal_overlay_drop_on_release(
                ctx,
                behavior,
                dock_rect,
                viewport_id,
                tree,
            )
        } else {
            None
        };

        let took_over_internal_drop = internal_drop.is_some();
        if let Some(pending) = internal_drop {
            if self.try_take_release_action(internal_overlay_kind) {
                if self.options.debug_event_log {
                    self.debug_log_event(format!(
                        "queue_internal_drop viewport={:?} tile_id={:?} insertion={:?}",
                        pending.viewport, pending.tile_id, pending.insertion
                    ));
                }
                ctx.stop_dragging();
                if let Some(payload) = egui::DragAndDrop::payload::<super::types::DockPayload>(ctx) {
                    if payload.bridge_id == self.tree.id() && payload.source_viewport == pending.viewport {
                        egui::DragAndDrop::clear_payload(ctx);
                    }
                }
                self.pending_internal_drop = Some(pending);
                ctx.request_repaint_of(ViewportId::ROOT);
            }
        }

        took_over_internal_drop
    }

    /// Phase B (per-viewport, late): queue same-viewport "local" drops that require up-to-date
    /// floating-window hit-testing (since floating rects are tracked during their UI pass).
    pub(super) fn process_release_after_floating_ui(
        &mut self,
        ctx: &Context,
        dock_rect: Rect,
        viewport_id: ViewportId,
    ) {
        self.queue_pending_local_drop_on_release(ctx, dock_rect, viewport_id);
        self.clear_bridge_payload_if_released_in_ctx(ctx);
    }
}
