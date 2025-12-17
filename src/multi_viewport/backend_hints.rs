use egui::{Context, Id, Pos2, Rect, ViewportId};

/// Context data key (temp) set by the `egui-winit` backend to indicate which native viewport is
/// currently hovered by the mouse.
///
/// Used to make cross-viewport drags reliable (ImGui-style `io.MouseHoveredViewport`).
pub const BACKEND_MOUSE_HOVERED_VIEWPORT_ID_KEY: &str = "egui-winit::mouse_hovered_viewport_id";

/// Context data key (temp) set by the `egui-winit` backend with the best-effort global pointer
/// position in points.
///
/// Used as a fallback when a viewport stops receiving `CursorMoved` (e.g. during OS window moves).
pub const BACKEND_POINTER_GLOBAL_POINTS_KEY: &str = "egui-winit::pointer_global_points";

/// Context data key (temp) set by the backend with monitor outer rects in global (desktop)
/// coordinates, in points.
///
/// This is useful for clamping restored window positions across multiple monitors.
pub const BACKEND_MONITORS_OUTER_RECTS_POINTS_KEY: &str = "egui-winit::monitors_outer_rects_points";

#[inline]
pub fn set_backend_monitors_outer_rects_points(ctx: &Context, rects: Vec<Rect>) {
    let id = Id::new(BACKEND_MONITORS_OUTER_RECTS_POINTS_KEY);
    ctx.data_mut(|d| {
        d.insert_temp::<Vec<Rect>>(id, rects);
    });
}

#[inline]
pub fn clear_backend_monitors_outer_rects_points(ctx: &Context) {
    let id = Id::new(BACKEND_MONITORS_OUTER_RECTS_POINTS_KEY);
    ctx.data_mut(|d| {
        d.remove::<Vec<Rect>>(id);
        d.remove::<Option<Vec<Rect>>>(id);
    });
}

#[inline]
pub fn backend_mouse_hovered_viewport_id(ctx: &Context) -> Option<ViewportId> {
    let id = Id::new(BACKEND_MOUSE_HOVERED_VIEWPORT_ID_KEY);
    ctx.data(|d| {
        d.get_temp::<ViewportId>(id)
            .or_else(|| d.get_temp::<Option<ViewportId>>(id).flatten())
    })
}

#[inline]
pub fn backend_pointer_global_points(ctx: &Context) -> Option<Pos2> {
    let id = Id::new(BACKEND_POINTER_GLOBAL_POINTS_KEY);
    ctx.data(|d| {
        d.get_temp::<Pos2>(id)
            .or_else(|| d.get_temp::<Option<Pos2>>(id).flatten())
    })
}

#[inline]
pub fn backend_monitors_outer_rects_points(ctx: &Context) -> Option<Vec<Rect>> {
    let id = Id::new(BACKEND_MONITORS_OUTER_RECTS_POINTS_KEY);
    ctx.data(|d| {
        d.get_temp::<Vec<Rect>>(id)
            .or_else(|| d.get_temp::<Option<Vec<Rect>>>(id).flatten())
    })
}
