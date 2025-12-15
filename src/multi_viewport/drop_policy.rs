use egui::ViewportId;

use super::surface::DockSurface;
use super::types::{DockPayload, FloatingId};

/// A small, testable policy helper: do we intentionally *skip* local-drop handling and let
/// `egui_tiles` handle the operation?
///
/// Rationale:
/// - When dragging from the dock tree (not floating) back onto the dock tree of the same viewport,
///   `egui_tiles` already provides a complete and well-tested internal DnD implementation.
/// - Re-applying a local drop by extracting+inserting can corrupt the tree if the insertion point
///   targets a tile inside the moved subtree (e.g. dropping back onto self), which can cause panes
///   or entire windows to disappear.
pub(super) fn should_skip_local_drop_internal_dock_to_dock(
    payload: &DockPayload,
    viewport_id: ViewportId,
    target_surface: DockSurface,
) -> bool {
    payload.source_viewport == viewport_id
        && payload.source_floating.is_none()
        && payload.tile_id.is_some()
        && matches!(target_surface, DockSurface::DockTree { .. })
}

pub(super) fn exclude_floating_for_hit_test(
    payload_source_floating: Option<FloatingId>,
    payload_tile_id: Option<egui_tiles::TileId>,
) -> Option<FloatingId> {
    let is_moving_floating_window = payload_source_floating.is_some() && payload_tile_id.is_none();
    is_moving_floating_window
        .then_some(payload_source_floating)
        .flatten()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn skips_internal_dock_to_dock() {
        let payload = DockPayload {
            bridge_id: egui::Id::new("x"),
            source_viewport: ViewportId::ROOT,
            source_floating: None,
            tile_id: Some(egui_tiles::TileId::from_u64(1)),
        };
        assert!(should_skip_local_drop_internal_dock_to_dock(
            &payload,
            ViewportId::ROOT,
            DockSurface::DockTree {
                viewport: ViewportId::ROOT
            }
        ));
    }

    #[test]
    fn does_not_skip_for_floating_source() {
        let payload = DockPayload {
            bridge_id: egui::Id::new("x"),
            source_viewport: ViewportId::ROOT,
            source_floating: Some(1),
            tile_id: Some(egui_tiles::TileId::from_u64(1)),
        };
        assert!(!should_skip_local_drop_internal_dock_to_dock(
            &payload,
            ViewportId::ROOT,
            DockSurface::DockTree {
                viewport: ViewportId::ROOT
            }
        ));
    }
}
