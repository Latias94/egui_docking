//! Small vector-only chrome primitives using the core's published rectangles.

use dockspace::runtime::DockspaceDropDirection;
use open_gpui::{
    Bounds, BoxShadow, Corners, Hsla, Pixels, Window, fill, point, px, quad, size,
    transparent_black,
};

use crate::DockspaceColors;

#[derive(Clone, Copy)]
pub(super) struct GuideState {
    pub(super) direction: DockspaceDropDirection,
    pub(super) active: bool,
    pub(super) eligible: bool,
}

pub(super) fn paint_guide(
    bounds: Bounds<Pixels>,
    state: GuideState,
    colors: &DockspaceColors,
    window: &mut Window,
) {
    let background = if !state.eligible {
        colors.disabled_guide
    } else if state.active {
        colors.active_guide
    } else {
        colors.guide
    };
    let border = if state.active {
        colors.accent
    } else {
        colors.border
    };
    let cue = if !state.eligible {
        colors.muted_text.opacity(0.45)
    } else if state.active {
        colors.accent
    } else {
        colors.muted_text
    };
    window.paint_quad(quad(
        bounds,
        px(4.0),
        background,
        px(1.0),
        border,
        Default::default(),
    ));

    let inset = px(5.0)
        .min(bounds.size.width / 4.0)
        .min(bounds.size.height / 4.0);
    let pane = Bounds::new(
        bounds.origin + point(inset, inset),
        size(
            bounds.size.width - inset * 2.0,
            bounds.size.height - inset * 2.0,
        ),
    );
    window.paint_quad(quad(
        pane,
        px(1.0),
        transparent_black(),
        px(1.0),
        cue,
        Default::default(),
    ));
    let interior = Bounds::new(
        pane.origin + point(px(1.0), px(1.0)),
        size(
            (pane.size.width - px(2.0)).max(px(0.0)),
            (pane.size.height - px(2.0)).max(px(0.0)),
        ),
    );
    let edge = px(4.0)
        .min(interior.size.width / 2.0)
        .min(interior.size.height / 2.0);
    let emphasis = match state.direction {
        DockspaceDropDirection::Center => interior,
        DockspaceDropDirection::Left => {
            Bounds::new(interior.origin, size(edge, interior.size.height))
        }
        DockspaceDropDirection::Right => Bounds::new(
            point(
                interior.origin.x + interior.size.width - edge,
                interior.origin.y,
            ),
            size(edge, interior.size.height),
        ),
        DockspaceDropDirection::Top => {
            Bounds::new(interior.origin, size(interior.size.width, edge))
        }
        DockspaceDropDirection::Bottom => Bounds::new(
            point(
                interior.origin.x,
                interior.origin.y + interior.size.height - edge,
            ),
            size(interior.size.width, edge),
        ),
    };
    window.paint_quad(fill(
        emphasis,
        cue.opacity(if state.direction == DockspaceDropDirection::Center {
            0.35
        } else {
            0.85
        }),
    ));
}

pub(super) fn paint_contained_frame(
    bounds: Bounds<Pixels>,
    colors: &DockspaceColors,
    window: &mut Window,
) {
    window.paint_drop_shadows(
        bounds,
        Corners::all(px(6.0)),
        &[BoxShadow {
            color: colors.shadow,
            offset: point(px(0.0), px(3.0)),
            blur_radius: px(12.0),
            spread_radius: px(0.0),
            inset: false,
        }],
    );
    window.paint_quad(quad(
        bounds,
        px(6.0),
        colors.pane,
        px(1.0),
        colors.border,
        Default::default(),
    ));
}

pub(super) fn paint_title(bounds: Bounds<Pixels>, color: Hsla, window: &mut Window) {
    window.paint_quad(quad(
        bounds,
        Corners {
            top_left: px(5.0),
            top_right: px(5.0),
            ..Default::default()
        },
        color,
        px(0.0),
        transparent_black(),
        Default::default(),
    ));
}

pub(super) fn paint_tab(
    bounds: Bounds<Pixels>,
    background: Hsla,
    selected: bool,
    colors: &DockspaceColors,
    window: &mut Window,
) {
    window.paint_quad(fill(bounds, background));
    if selected {
        window.paint_quad(fill(
            Bounds::new(bounds.origin, size(bounds.size.width, px(2.0))),
            colors.accent,
        ));
    } else {
        window.paint_quad(fill(
            Bounds::new(
                point(
                    bounds.origin.x + bounds.size.width - px(1.0),
                    bounds.origin.y + px(7.0),
                ),
                size(px(1.0), (bounds.size.height - px(14.0)).max(px(0.0))),
            ),
            colors.border,
        ));
    }
}
