use egui::{Pos2, Rect};
use egui_tiles::{Behavior, DockZone, InsertionPoint, TileId, Tree};

use super::overlay::{
    self, insertion_from_hovered_target, outer_overlay_for_dock_rect_explicit,
    overlay_for_tree_at_pointer_explicit, overlay_for_tree_at_pointer_explicit_considering_dragged,
    pointer_in_outer_band, tile_contains_descendant, DockingOverlay, OuterDockingOverlay,
    OverlayTarget,
};

#[derive(Clone, Copy, Debug)]
pub(super) enum DragKind {
    /// Moving a whole window host (native viewport title bar or contained floating header).
    /// Docking must be explicit-only.
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
    let outer_mode = show_outer_overlay_targets && pointer_in_outer_band(dock_rect, pointer_local);

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
        DragKind::Subtree { internal: false, .. } => tree.dock_zone_at(behavior, style, pointer_local),
        _ => None,
    };

    let insertion_final = match drag_kind {
        DragKind::WindowMove => insertion_explicit,
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
