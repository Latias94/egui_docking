use egui::{Context, Modifiers, Pos2, ViewportId};
use std::collections::BTreeMap;

use super::geometry::pointer_pos_in_global;
use super::session::DragSession;
use super::types::DockPayload;

// Keep in sync with `egui-winit` (repo-ref) backend key:
// `repo-ref/egui/crates/egui-winit/src/lib.rs`.
const BACKEND_MOUSE_HOVERED_VIEWPORT_ID_KEY: &str = "egui-winit::mouse_hovered_viewport_id";
const BACKEND_POINTER_GLOBAL_POINTS_KEY: &str = "egui-winit::pointer_global_points";

#[inline]
fn backend_mouse_hovered_viewport(ctx: &Context) -> Option<ViewportId> {
    let id = egui::Id::new(BACKEND_MOUSE_HOVERED_VIEWPORT_ID_KEY);
    ctx.data(|d| d.get_temp::<Option<ViewportId>>(id).flatten())
}

#[inline]
fn backend_pointer_global_points(ctx: &Context) -> Option<Pos2> {
    let id = egui::Id::new(BACKEND_POINTER_GLOBAL_POINTS_KEY);
    ctx.data(|d| d.get_temp::<Option<Pos2>>(id).flatten())
}

#[derive(Debug, Default)]
pub(super) struct DragState {
    session: DragSession,
    last_pointer_global: Option<Pos2>,
    last_modifiers: Modifiers,
    any_pointer_down_this_frame: bool,
    any_pointer_released_this_frame: bool,
    last_interact_update_frame: u64,
    last_delta_integrated_frame: u64,
    last_hovered_viewport: Option<ViewportId>,
    last_viewport_inner_min: BTreeMap<ViewportId, Pos2>,
}

impl DragState {
    pub(super) fn begin_frame(&mut self) {
        self.session.begin_frame();
        self.any_pointer_down_this_frame = false;
        self.any_pointer_released_this_frame = false;
    }

    pub(super) fn end_frame(&mut self, frame: u64) -> Option<String> {
        self.session.end_frame(frame)
    }

    pub(super) fn update_pointer_global_from_ctx(
        &mut self,
        frame: u64,
        ctx: &Context,
        allow_interact_pos: bool,
    ) {
        if let Some(pos) = backend_pointer_global_points(ctx) {
            self.last_pointer_global = Some(pos);
            self.last_interact_update_frame = frame;
        }

        if let Some(vp) = backend_mouse_hovered_viewport(ctx) {
            self.last_hovered_viewport = Some(vp);
        }

        let (any_down, any_released) = ctx.input(|i| (i.pointer.any_down(), i.pointer.any_released()));
        self.any_pointer_down_this_frame |= any_down;
        self.any_pointer_released_this_frame |= any_released;

        let viewport_moved_this_frame = ctx
            .input(|i| i.viewport().inner_rect.map(|r| r.min))
            .is_some_and(|inner_min| {
                match self.last_viewport_inner_min.insert(ctx.viewport_id(), inner_min) {
                    Some(prev) => prev != inner_min,
                    None => false,
                }
            });

        let moved_this_frame = viewport_moved_this_frame
            || ctx.input(|i| {
            i.pointer.delta() != egui::Vec2::ZERO
                || i
                    .pointer
                    .motion()
                    .is_some_and(|m| m != egui::Vec2::ZERO)
        });

        // Only accept an interact-based position update if we believe it's fresh this frame.
        // During OS-level window moves, viewports that don't receive cursor updates may keep a stale
        // `interact_pos` from earlier frames, and using it would overwrite the integrated global
        // pointer and make cross-viewport docking impossible.
        if allow_interact_pos
            && (moved_this_frame || self.last_pointer_global.is_none())
            && let Some(pos) = pointer_pos_in_global(ctx)
        {
            self.last_pointer_global = Some(pos);
            self.last_interact_update_frame = frame;
            self.last_hovered_viewport.get_or_insert(ctx.viewport_id());
            return;
        }

        // Don't overwrite the global pointer with stale data from other viewports in the same frame.
        if self.last_interact_update_frame == frame {
            return;
        }

        // Fallback: during OS-level window moves, we may stop receiving `CursorMoved` updates in the
        // target viewport, but we can still get reliable pointer deltas (e.g. synthesized from raw
        // device events in the backend). Integrate the delta into the last known global position.
        if let Some(prev) = self.last_pointer_global {
            // Prefer raw mouse motion (unaccelerated) when available: it stays stable even if the
            // window itself is moving under the cursor (which can make `pointer.delta()` appear to
            // drift in window-local coordinates).
            let delta_points = ctx.input(|i| {
                if let Some(motion) = i.pointer.motion() {
                    let ppp = ctx.pixels_per_point();
                    if ppp > 0.0 && ppp.is_finite() {
                        motion / ppp
                    } else {
                        egui::Vec2::ZERO
                    }
                } else if allow_interact_pos {
                    i.pointer.delta()
                } else {
                    // During native viewport window-move drags, the window itself is moving under the
                    // cursor. In that mode `pointer.delta()` is in *window-local* space and can reflect
                    // the window motion even when the user doesn't move the mouse, causing severe jitter.
                    //
                    // Only integrate raw device motion (`pointer.motion`) here.
                    egui::Vec2::ZERO
                }
            });
            if delta_points != egui::Vec2::ZERO {
                // Only mark the frame as integrated when we actually applied a non-zero delta.
                // This avoids "stealing" the per-frame integration slot in a viewport that doesn't
                // receive mouse motion (e.g. the root viewport during cross-window drags), which
                // would otherwise prevent the real source viewport from updating the global pointer.
                if self.last_delta_integrated_frame != frame {
                    self.last_delta_integrated_frame = frame;
                    self.last_pointer_global = Some(prev + delta_points);
                }
            }
        }
    }

    pub(super) fn last_pointer_global(&self) -> Option<Pos2> {
        self.last_pointer_global
    }

    pub(super) fn last_hovered_viewport(&self) -> Option<ViewportId> {
        self.last_hovered_viewport
    }

    pub(super) fn any_pointer_down_this_frame(&self) -> bool {
        self.any_pointer_down_this_frame
    }

    pub(super) fn any_pointer_released_this_frame(&self) -> bool {
        self.any_pointer_released_this_frame
    }

    pub(super) fn pointer_global_fallback(&self, ctx: &Context) -> Option<Pos2> {
        pointer_pos_in_global(ctx)
            .or(self.last_pointer_global)
    }

    /// Like [`Self::pointer_global_fallback`], but prefer the integrated global pointer position.
    ///
    /// This is important during OS-level window moves: the cursor local position in the moving window
    /// can remain constant, making `(inner_rect + local)` self-referential and stale, while raw mouse
    /// motion integration continues to reflect the real cursor position.
    pub(super) fn pointer_global_prefer_integrated(&self, ctx: &Context) -> Option<Pos2> {
        self.last_pointer_global
            .or_else(|| pointer_pos_in_global(ctx))
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
