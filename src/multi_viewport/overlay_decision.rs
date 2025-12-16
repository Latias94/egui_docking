use egui::{NumExt as _, Pos2, Rect};
use egui_tiles::{Behavior, DockZone, InsertionPoint, TileId, Tree};

use super::overlay::{
    self, insertion_from_hovered_target, outer_overlay_for_dock_rect_explicit,
    overlay_for_tree_at_pointer_explicit, overlay_for_tree_at_pointer_explicit_considering_dragged,
    pointer_in_outer_band, tile_contains_descendant, DockingOverlay, OuterDockingOverlay,
    OverlayTarget,
};

fn best_tile_under_pointer<Pane>(tree: &Tree<Pane>, pointer_local: Pos2) -> Option<(TileId, Rect)> {
    let mut best: Option<(TileId, Rect)> = None;
    let mut best_area = f32::INFINITY;

    for tile_id in tree.active_tiles() {
        let Some(rect) = tree.tiles.rect(tile_id) else {
            continue;
        };
        if !rect.contains(pointer_local) {
            continue;
        }
        let area = rect.width() * rect.height();
        if area < best_area {
            best_area = area;
            best = Some((tile_id, rect));
        }
    }

    best
}

fn window_move_explicit_target_zone_at_pointer<Pane>(
    tree: &Tree<Pane>,
    behavior: &dyn Behavior<Pane>,
    style: &egui::Style,
    pointer_local: Pos2,
) -> Option<DockZone> {
    let (hit_tile, _rect) = best_tile_under_pointer(tree, pointer_local)?;

    // ImGui explicit target rect:
    // - for docked tabs: the tab bar rect
    // - otherwise: a title-bar band at the top of the hovered tile
    let header_tile = tree
        .tiles
        .parent_of(hit_tile)
        .filter(|&p| tree.tiles.get(p).and_then(|t| t.kind()) == Some(egui_tiles::ContainerKind::Tabs))
        .unwrap_or(hit_tile);

    let header_rect = tree.tiles.rect(header_tile)?;
    let title_bar_h = behavior.tab_bar_height(style).at_least(0.0);
    let y = (header_rect.top() + title_bar_h).at_most(header_rect.bottom());
    let title_bar_rect = header_rect.split_top_bottom_at_y(y).0;
    if !title_bar_rect.contains(pointer_local) {
        return None;
    }

    // If the explicit target is a Tabs container, leverage its richer `dock_zone_at` result
    // (insertion index based on pointer-x).
    if tree.tiles.get(header_tile).and_then(|t| t.kind()) == Some(egui_tiles::ContainerKind::Tabs)
    {
        if let Some(zone) = tree.dock_zone_at(behavior, style, pointer_local)
            && zone.insertion_point.insertion.kind() == egui_tiles::ContainerKind::Tabs
            && zone.insertion_point.parent_id == header_tile
        {
            return Some(zone);
        }
    }

    Some(DockZone {
        insertion_point: InsertionPoint::new(
            header_tile,
            egui_tiles::ContainerInsertion::Tabs(usize::MAX),
        ),
        preview_rect: title_bar_rect,
    })
}

#[derive(Clone, Copy, Debug)]
pub(super) enum DragKind {
    /// Moving a whole window host (native viewport title bar or contained floating header).
    /// Splits must be explicit-only; tab-dock requires hovering the target tab bar/title bar.
    WindowMove,
    /// Moving a subtree (tab/pane/container).
    Subtree {
        dragged_tile: Option<TileId>,
        /// Whether the dragged tile comes from this same tree (internal drag).
        internal: bool,
    },
}

#[derive(Clone, Copy, Debug)]
pub(super) enum OverlayPaint {
    Inner(DockingOverlay),
    Outer(OuterDockingOverlay),
}

impl OverlayPaint {
    pub(super) fn hovered_target(self) -> Option<OverlayTarget> {
        match self {
            Self::Inner(o) => o.hovered_target(),
            Self::Outer(o) => o.hovered_target(),
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub(super) struct OverlayDecision {
    pub(super) paint: Option<OverlayPaint>,
    pub(super) insertion_explicit: Option<InsertionPoint>,
    pub(super) fallback_zone: Option<DockZone>,
    pub(super) insertion_final: Option<InsertionPoint>,
    pub(super) disable_tiles_preview: bool,
}

pub(super) fn decide_overlay_for_tree<Pane>(
    tree: &Tree<Pane>,
    behavior: &dyn Behavior<Pane>,
    style: &egui::Style,
    dock_rect: Rect,
    pointer_local: Pos2,
    show_outer_overlay_targets: bool,
    drag_kind: DragKind,
) -> OverlayDecision {
    let window_move_explicit_zone =
        window_move_explicit_target_zone_at_pointer(tree, behavior, style, pointer_local);

    // Prefer tab-bar docking over outer overlay mode so we can still dock as a tab when the
    // pointer is near the edge (tab bars often live at the top edge, which is inside the band).
    let outer_mode = show_outer_overlay_targets
        && pointer_in_outer_band(dock_rect, pointer_local)
        && window_move_explicit_zone.is_none();

    let (paint_candidate, insertion_explicit) = if outer_mode {
        let overlay = outer_overlay_for_dock_rect_explicit(dock_rect, pointer_local);
        let insertion = overlay
            .and_then(|o| o.hovered_target())
            .and_then(|t| {
                let root = tree.root?;
                Some(overlay::insertion_from_hovered_target(root, t))
            });
        (overlay.map(OverlayPaint::Outer), insertion)
    } else {
        let (overlay, insertion) = match drag_kind {
            DragKind::Subtree { dragged_tile, internal: true } => {
                let o = overlay_for_tree_at_pointer_explicit_considering_dragged(
                    tree,
                    pointer_local,
                    dragged_tile,
                );
                let insertion = o
                    .and_then(|o| o.hovered_target().map(|t| (o.tile_id(), t)))
                    .map(|(tile_id, t)| insertion_from_hovered_target(tile_id, t));
                (o, insertion)
            }
            _ => {
                let o = overlay_for_tree_at_pointer_explicit(tree, pointer_local);
                let insertion = o
                    .and_then(|o| o.hovered_target().map(|t| (o.tile_id(), t)))
                    .map(|(tile_id, t)| insertion_from_hovered_target(tile_id, t));
                (o, insertion)
            }
        };
        (overlay.map(OverlayPaint::Inner), insertion)
    };

    // Filter illegal insertion for internal subtree moves (self-parent/cycle).
    let insertion_explicit = match drag_kind {
        DragKind::Subtree {
            dragged_tile: Some(dragged),
            internal: true,
        } => insertion_explicit.filter(|ins| !tile_contains_descendant(tree, dragged, ins.parent_id)),
        _ => insertion_explicit,
    };

    // UI principle (ImGui parity): only show *one* preview system at a time.
    // - External drags (and window moves) always show overlay targets (discoverable docking).
    // - Internal drags only show overlay when it is authoritative (explicit hit); otherwise keep
    //   `egui_tiles`' built-in internal preview.
    let should_paint_overlay = match drag_kind {
        DragKind::Subtree { internal: true, .. } => insertion_explicit.is_some(),
        _ => true,
    };
    let paint = should_paint_overlay.then_some(paint_candidate).flatten();

    let fallback_zone = match drag_kind {
        // Subtree moves: fall back to tiles' heuristic when the overlay isn't explicitly hit.
        DragKind::Subtree { internal: false, .. } => tree.dock_zone_at(behavior, style, pointer_local),

        // Window moves (ImGui-like): docking is only allowed when hovering either:
        // - an explicit docking target (overlay button / drop-rect), or
        // - the target tab bar / title bar (explicit target rect).
        //
        // If neither is true, we do not provide a fallback insertion (dropping does nothing).
        DragKind::WindowMove => window_move_explicit_zone,

        _ => None,
    };

    let insertion_final = match drag_kind {
        DragKind::WindowMove => insertion_explicit.or_else(|| fallback_zone.map(|z| z.insertion_point)),
        DragKind::Subtree { internal: true, .. } => insertion_explicit,
        DragKind::Subtree { internal: false, .. } => {
            insertion_explicit.or_else(|| fallback_zone.map(|z| z.insertion_point))
        }
    };

    // Disable tiles preview only when we are taking over an internal drag with an explicit target.
    let disable_tiles_preview = matches!(drag_kind, DragKind::Subtree { internal: true, .. })
        && insertion_explicit.is_some();

    OverlayDecision {
        paint,
        insertion_explicit,
        fallback_zone,
        insertion_final,
        disable_tiles_preview,
    }
}
