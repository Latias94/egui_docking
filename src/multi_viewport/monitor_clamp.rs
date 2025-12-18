use egui::{Context, Pos2, Rect, Vec2};

use super::backend_hints::backend_monitors_outer_rects_points;

pub(super) fn clamp_outer_pos_if_monitors_available(ctx: &Context, pos: Pos2, size: Vec2) -> Pos2 {
    if let Some(monitors) = backend_monitors_outer_rects_points(ctx)
        && !monitors.is_empty()
    {
        return clamp_pos_to_monitors_best_effort(pos, size, &monitors);
    }

    // Fallback: clamp into the current monitor's coordinate space.
    // See `clamp_outer_pos_best_effort` for the rationale.
    if pos.x >= 0.0
        && pos.y >= 0.0
        && let Some(monitor_size) = ctx.input(|i| i.viewport().monitor_size)
        && size.x.is_finite()
        && size.y.is_finite()
    {
        let max = (monitor_size - size).max(Vec2::ZERO);
        return egui::pos2(pos.x.clamp(0.0, max.x), pos.y.clamp(0.0, max.y));
    }

    pos
}

#[cfg(feature = "persistence")]
pub(super) fn clamp_outer_pos_best_effort(ctx: &Context, pos: Pos2, size: Vec2) -> Pos2 {
    if let Some(monitors) = backend_monitors_outer_rects_points(ctx)
        && !monitors.is_empty()
    {
        return clamp_pos_to_monitors_best_effort(pos, size, &monitors);
    }

    // Fallback: clamp into the current monitor's coordinate space.
    // This matches egui's `ViewportCommand::center_on_screen` convention (origin at (0,0)).
    //
    // Important: without a global monitor list, we cannot know whether negative coordinates are
    // valid (e.g. a monitor placed left/up of the primary). To avoid breaking multi-monitor setups,
    // we only apply this fallback when the restored position looks "single-monitor-like".
    if pos.x >= 0.0
        && pos.y >= 0.0
        && let Some(monitor_size) = ctx.input(|i| i.viewport().monitor_size)
        && size.x.is_finite()
        && size.y.is_finite()
    {
        let max = (monitor_size - size).max(Vec2::ZERO);
        return egui::pos2(pos.x.clamp(0.0, max.x), pos.y.clamp(0.0, max.y));
    }

    pos
}

fn clamp_pos_to_monitors_best_effort(pos: Pos2, size: Vec2, monitors: &[Rect]) -> Pos2 {
    if monitors.is_empty() || !(size.x.is_finite() && size.y.is_finite()) {
        return pos;
    }

    let rect = Rect::from_min_size(pos, size);

    // Prefer the monitor with the largest intersection with the window rect.
    let mut best: Option<(Rect, f32)> = None;
    for &m in monitors {
        let inter = rect.intersect(m);
        if inter.is_positive() {
            let area = inter.width() * inter.height();
            match best {
                None => best = Some((m, area)),
                Some((_best_m, best_area)) if area > best_area => best = Some((m, area)),
                _ => {}
            }
        }
    }

    // If nothing intersects, pick the monitor whose clamped point is closest.
    let monitor = if let Some((m, _area)) = best {
        m
    } else {
        let mut best_m: Option<(Rect, f32)> = None;
        for &m in monitors {
            let clamped = egui::pos2(pos.x.clamp(m.min.x, m.max.x), pos.y.clamp(m.min.y, m.max.y));
            let d = clamped.distance(pos);
            match best_m {
                None => best_m = Some((m, d)),
                Some((_m0, d0)) if d < d0 => best_m = Some((m, d)),
                _ => {}
            }
        }
        best_m.map(|(m, _d)| m).unwrap_or_else(|| monitors[0])
    };

    let min = monitor.min;
    let max_unclamped = monitor.max - size;
    let max = egui::pos2(max_unclamped.x.max(min.x), max_unclamped.y.max(min.y));
    egui::pos2(pos.x.clamp(min.x, max.x), pos.y.clamp(min.y, max.y))
}
