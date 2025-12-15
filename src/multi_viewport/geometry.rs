use egui::{Context, Pos2, Rect, Vec2, ViewportId};

pub(super) fn pointer_pos_in_global(ctx: &Context) -> Option<Pos2> {
    ctx.input(|i| {
        let local = i.pointer.interact_pos()?;
        let inner = i.viewport().inner_rect?;
        Some(inner.min + local.to_vec2())
    })
}

pub(super) fn pointer_pos_in_viewport_space(
    ctx: &Context,
    pointer_global: Option<Pos2>,
) -> Option<Pos2> {
    let pointer_global = pointer_global?;
    let inner = ctx.input(|i| i.viewport().inner_rect)?;
    if !inner.contains(pointer_global) {
        return None;
    }

    let delta: Vec2 = pointer_global - inner.min;
    Some(Pos2::new(delta.x, delta.y))
}

pub(super) fn pointer_pos_in_target_viewport_space(
    ctx: &Context,
    target_viewport: ViewportId,
    pointer_global: Pos2,
) -> Option<Pos2> {
    ctx.input(|i| {
        let inner = i.raw.viewports.get(&target_viewport)?.inner_rect?;
        if !inner.contains(pointer_global) {
            return None;
        }
        let delta: Vec2 = pointer_global - inner.min;
        Some(Pos2::new(delta.x, delta.y))
    })
}

pub(super) fn viewport_under_pointer_global(
    ctx: &Context,
    pointer_global: Pos2,
) -> Option<ViewportId> {
    fn area(rect: Rect) -> f32 {
        rect.width() * rect.height()
    }

    ctx.input(|i| {
        i.raw
            .viewports
            .iter()
            .filter_map(|(id, info)| {
                let rect = info.inner_rect?;
                rect.contains(pointer_global).then_some((*id, rect))
            })
            .min_by(|a, b| area(a.1).total_cmp(&area(b.1)))
            .map(|(id, _rect)| id)
    })
}

pub(super) fn viewport_under_pointer_global_excluding(
    ctx: &Context,
    pointer_global: Pos2,
    excluded: Option<ViewportId>,
) -> Option<ViewportId> {
    fn area(rect: Rect) -> f32 {
        rect.width() * rect.height()
    }

    ctx.input(|i| {
        i.raw
            .viewports
            .iter()
            .filter_map(|(id, info)| {
                if excluded == Some(*id) {
                    return None;
                }
                let rect = info.inner_rect?;
                rect.contains(pointer_global).then_some((*id, rect))
            })
            .min_by(|a, b| area(a.1).total_cmp(&area(b.1)))
            .map(|(id, _rect)| id)
    })
}

pub(super) fn root_inner_rect_in_global(ctx: &Context) -> Option<Rect> {
    ctx.input(|i| i.raw.viewports.get(&ViewportId::ROOT)?.inner_rect)
}

pub(super) fn infer_detached_geometry(
    pane_rect_in_root: Option<Rect>,
    pointer_global_fallback: Option<Pos2>,
    root_inner_rect_global: Option<Rect>,
    default_size: Vec2,
) -> (Pos2, Vec2) {
    let size = pane_rect_in_root
        .map(|r| Vec2::new(r.width().max(200.0), r.height().max(120.0)))
        .unwrap_or(default_size);

    let pos = if let Some(pointer_global) = pointer_global_fallback {
        pointer_global - Vec2::new(20.0, 10.0)
    } else if let (Some(root_inner_rect), Some(pane_rect)) =
        (root_inner_rect_global, pane_rect_in_root)
    {
        root_inner_rect.min + pane_rect.min.to_vec2()
    } else {
        Pos2::new(64.0, 64.0)
    };

    (pos, size)
}
