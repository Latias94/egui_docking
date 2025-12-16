use egui::{Context, Modifiers, Pos2};

use super::geometry::pointer_pos_in_global;
use super::session::DragSession;
use super::types::DockPayload;

#[derive(Debug, Default)]
pub(super) struct DragState {
    session: DragSession,
    last_pointer_global: Option<Pos2>,
    last_modifiers: Modifiers,
}

impl DragState {
    fn pointer_pos_in_global_from_latest(ctx: &Context) -> Option<Pos2> {
        ctx.input(|i| {
            let local = i.pointer.latest_pos()?;
            let inner = i.viewport().inner_rect?;
            Some(inner.min + local.to_vec2())
        })
    }

    pub(super) fn begin_frame(&mut self) {
        self.session.begin_frame();
    }

    pub(super) fn end_frame(&mut self, frame: u64) -> Option<String> {
        self.session.end_frame(frame)
    }

    pub(super) fn update_pointer_global_from_ctx(&mut self, ctx: &Context) {
        if let Some(pos) = pointer_pos_in_global(ctx)
            .or_else(|| Self::pointer_pos_in_global_from_latest(ctx))
        {
            self.last_pointer_global = Some(pos);
            return;
        }

        // Fallback: during OS-level window moves, we may stop receiving `CursorMoved` updates in the
        // target viewport, but we can still get reliable pointer deltas (e.g. synthesized from raw
        // device events in the backend). Integrate the delta into the last known global position.
        if let Some(prev) = self.last_pointer_global {
            let delta = ctx.input(|i| i.pointer.delta());
            if delta != egui::Vec2::ZERO {
                self.last_pointer_global = Some(prev + delta);
            }
        }
    }

    pub(super) fn last_pointer_global(&self) -> Option<Pos2> {
        self.last_pointer_global
    }

    pub(super) fn pointer_global_fallback(&self, ctx: &Context) -> Option<Pos2> {
        pointer_pos_in_global(ctx)
            .or_else(|| Self::pointer_pos_in_global_from_latest(ctx))
            .or(self.last_pointer_global)
    }

    pub(super) fn observe_source(
        &mut self,
        frame: u64,
        source: &'static str,
    ) -> Option<String> {
        self.session.observe_active(frame, source)
    }

    pub(super) fn observe_payload(
        &mut self,
        frame: u64,
        ctx: &Context,
        bridge_id: egui::Id,
    ) -> Option<DockPayload> {
        let Some(payload) = egui::DragAndDrop::payload::<DockPayload>(ctx) else {
            return None;
        };
        if payload.bridge_id != bridge_id {
            return None;
        }

        if ctx.viewport_id() == payload.source_viewport {
            self.last_modifiers = ctx.input(|i| i.modifiers);
        }
        let _ = frame;

        Some(*payload)
    }

    pub(super) fn window_move_shift_held_now(&self, ctx: &Context, bridge_id: egui::Id) -> bool {
        if egui::DragAndDrop::payload::<DockPayload>(ctx)
            .is_some_and(|p| p.bridge_id == bridge_id && p.tile_id.is_none())
        {
            self.last_modifiers.shift
        } else {
            ctx.input(|i| i.modifiers.shift)
        }
    }

    pub(super) fn take_release_action(
        &mut self,
        frame: u64,
        kind: &'static str,
    ) -> (bool, Option<String>) {
        self.session.take_release_action(frame, kind)
    }
}
