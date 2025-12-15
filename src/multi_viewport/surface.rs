use egui::{Context, Pos2, Rect, ViewportId};
use egui_tiles::{Behavior, InsertionPoint, Tree};

use super::DockingMultiViewport;
use super::geometry::{
    pointer_pos_in_target_viewport_space, viewport_under_pointer_global,
    viewport_under_pointer_global_excluding,
};
use super::overlay::{
    overlay_insertion_for_tree_explicit_with_outer_considering_dragged,
    overlay_insertion_for_tree_with_outer,
};
use super::types::FloatingId;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum DockSurface {
    DockTree {
        viewport: ViewportId,
    },
    Floating {
        viewport: ViewportId,
        floating: FloatingId,
    },
}

impl DockSurface {
    pub(super) fn viewport(self) -> ViewportId {
        match self {
            DockSurface::DockTree { viewport } => viewport,
            DockSurface::Floating { viewport, .. } => viewport,
        }
    }
}

impl<Pane> DockingMultiViewport<Pane> {
    pub(super) fn dock_rect_for_surface(&self, surface: DockSurface) -> Option<Rect> {
        match surface {
            DockSurface::DockTree { viewport } => self.last_dock_rects.get(&viewport).copied(),
            DockSurface::Floating { viewport, floating } => self
                .last_floating_content_rects
                .get(&(viewport, floating))
                .copied(),
        }
    }

    pub(super) fn tree_for_surface(&self, surface: DockSurface) -> Option<&Tree<Pane>> {
        match surface {
            DockSurface::DockTree { viewport } => {
                if viewport == ViewportId::ROOT {
                    Some(&self.tree)
                } else {
                    self.detached.get(&viewport).map(|d| &d.tree)
                }
            }
            DockSurface::Floating { viewport, floating } => self
                .floating
                .get(&viewport)?
                .windows
                .get(&floating)
                .map(|w| &w.tree),
        }
    }

    pub(super) fn surface_under_pointer_local(
        &self,
        viewport_id: ViewportId,
        dock_rect: Rect,
        pointer_local: Pos2,
        exclude_floating: Option<FloatingId>,
    ) -> Option<DockSurface> {
        if let Some(floating) = self.floating_content_under_pointer_excluding(
            viewport_id,
            pointer_local,
            exclude_floating,
        ) {
            return Some(DockSurface::Floating {
                viewport: viewport_id,
                floating,
            });
        }

        dock_rect
            .contains(pointer_local)
            .then_some(DockSurface::DockTree {
                viewport: viewport_id,
            })
    }

    pub(super) fn surface_under_pointer_global(
        &self,
        ctx: &Context,
        pointer_global: Pos2,
        exclude_viewport: Option<ViewportId>,
        exclude_floating: Option<FloatingId>,
    ) -> Option<(DockSurface, Pos2)> {
        let viewport = if exclude_viewport.is_some() {
            viewport_under_pointer_global_excluding(ctx, pointer_global, exclude_viewport)
                .or_else(|| viewport_under_pointer_global(ctx, pointer_global))
        } else {
            viewport_under_pointer_global(ctx, pointer_global)
        }?;
        let pointer_local = pointer_pos_in_target_viewport_space(ctx, viewport, pointer_global)?;
        let dock_rect = self.last_dock_rects.get(&viewport).copied()?;
        let surface =
            self.surface_under_pointer_local(viewport, dock_rect, pointer_local, exclude_floating)?;
        Some((surface, pointer_local))
    }

    pub(super) fn insertion_at_pointer_local(
        &self,
        behavior: &dyn Behavior<Pane>,
        style: &egui::Style,
        surface: DockSurface,
        pointer_local: Pos2,
    ) -> Option<InsertionPoint> {
        let dock_rect = self.dock_rect_for_surface(surface)?;
        let tree = self.tree_for_surface(surface)?;

        overlay_insertion_for_tree_with_outer(tree, dock_rect, pointer_local).or_else(|| {
            tree.dock_zone_at(behavior, style, pointer_local)
                .map(|z| z.insertion_point)
        })
    }

    pub(super) fn explicit_insertion_at_pointer_local(
        &self,
        surface: DockSurface,
        pointer_local: Pos2,
    ) -> Option<InsertionPoint> {
        let dock_rect = self.dock_rect_for_surface(surface)?;
        let tree = self.tree_for_surface(surface)?;
        overlay_insertion_for_tree_explicit_with_outer_considering_dragged(
            tree,
            dock_rect,
            pointer_local,
            None,
        )
    }
}
