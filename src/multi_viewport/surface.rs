use egui::{Context, Pos2, Rect, ViewportId};
use egui_tiles::{Behavior, InsertionPoint, Tree};

use super::DockingMultiViewport;
use super::geometry::{
    pointer_pos_in_target_viewport_space, viewport_under_pointer_global,
    viewport_under_pointer_global_excluding,
};
use super::overlay::overlay_insertion_for_tree_explicit_with_outer_considering_dragged;
use super::overlay_decision::{decide_overlay_for_tree, DragKind};
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
        dragged_tile: Option<egui_tiles::TileId>,
    ) -> Option<InsertionPoint> {
        let dock_rect = self.dock_rect_for_surface(surface)?;
        let tree = self.tree_for_surface(surface)?;
        let decision = decide_overlay_for_tree(
            tree,
            behavior,
            style,
            dock_rect,
            pointer_local,
            self.options.show_outer_overlay_targets,
            DragKind::Subtree {
                dragged_tile: None,
                internal: false,
            },
        );
        let insertion = decision.insertion_final;
        let Some(dragged_tile) = dragged_tile else {
            return insertion;
        };
        let Some(ins) = insertion else {
            return None;
        };

        // If we are dropping a subtree back into the same tree (e.g. floating → dock),
        // never allow an insertion that targets the subtree itself.
        if ins.parent_id == dragged_tile
            || super::overlay::tile_contains_descendant(tree, dragged_tile, ins.parent_id)
        {
            return None;
        }
        Some(ins)
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

    pub(super) fn window_move_insertion_at_pointer_local(
        &self,
        behavior: &dyn Behavior<Pane>,
        style: &egui::Style,
        surface: DockSurface,
        pointer_local: Pos2,
    ) -> Option<InsertionPoint> {
        let dock_rect = self.dock_rect_for_surface(surface)?;
        let tree = self.tree_for_surface(surface)?;
        let decision = decide_overlay_for_tree(
            tree,
            behavior,
            style,
            dock_rect,
            pointer_local,
            self.options.show_outer_overlay_targets,
            DragKind::WindowMove {
                tab_dock_requires_explicit_target: self
                    .options
                    .window_move_tab_dock_requires_explicit_target,
            },
        );
        decision.insertion_final
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct DummyBehavior;

    impl egui_tiles::Behavior<()> for DummyBehavior {
        fn pane_ui(
            &mut self,
            _ui: &mut egui::Ui,
            _tile_id: egui_tiles::TileId,
            _pane: &mut (),
        ) -> egui_tiles::UiResponse {
            Default::default()
        }

        fn tab_title_for_pane(&mut self, _pane: &()) -> egui::WidgetText {
            "pane".into()
        }
    }

    fn layout_tree(tree: &mut egui_tiles::Tree<()>, behavior: &mut dyn egui_tiles::Behavior<()>) -> egui::Rect {
        let ctx = egui::Context::default();
        let raw = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::Vec2::new(800.0, 600.0),
            )),
            ..Default::default()
        };
        ctx.begin_pass(raw);
        let mut dock_rect = egui::Rect::NOTHING;
        egui::CentralPanel::default().show(&ctx, |ui| {
            dock_rect = ui.available_rect_before_wrap();
            tree.ui(behavior, ui);
        });
        let _ = ctx.end_pass();
        dock_rect
    }

    #[test]
    fn insertion_filters_self_target_when_dragged_tile_provided() {
        let mut tiles: egui_tiles::Tiles<()> = egui_tiles::Tiles::default();
        let a = tiles.insert_pane(());
        let b = tiles.insert_pane(());
        let root = tiles.insert_tab_tile(vec![a, b]);
        let tree = egui_tiles::Tree::new(egui::Id::new("tree"), root, tiles);
        let mut docking = DockingMultiViewport::new(tree);

        let mut behavior = DummyBehavior::default();
        let dock_rect = layout_tree(&mut docking.tree, &mut behavior);
        docking
            .last_dock_rects
            .insert(egui::ViewportId::ROOT, dock_rect);

        let root_rect = docking
            .tree
            .tiles
            .rect(root)
            .expect("root must have rect");
        let pointer_over_self = root_rect.center();

        let surface = DockSurface::DockTree {
            viewport: egui::ViewportId::ROOT,
        };
        let style = egui::Style::default();

        let insertion_without_filter =
            docking.insertion_at_pointer_local(&behavior, &style, surface, pointer_over_self, None);
        assert!(insertion_without_filter.is_some());

        let insertion_with_filter = docking.insertion_at_pointer_local(
            &behavior,
            &style,
            surface,
            pointer_over_self,
            Some(root),
        );
        assert!(insertion_with_filter.is_none());
    }
}
