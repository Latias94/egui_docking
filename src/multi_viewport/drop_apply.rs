use egui::{Context, ViewportId};
use egui_tiles::{Behavior, EditAction, TileId, Tree};

use super::DockingMultiViewport;
use super::drop_policy;
use super::drop_sanitize;
use super::host::WindowHost;
use super::integrity;
use super::overlay_decision::{decide_overlay_for_tree, DragKind};
use super::surface::DockSurface;
use super::title::title_for_detached_tree;
use super::types::{DockPayload, ResolvedDrop, ResolvedDropTarget};

fn force_subtree_visible<Pane>(subtree: &mut egui_tiles::SubTree<Pane>) {
    let ids: Vec<egui_tiles::TileId> = subtree.tiles.tile_ids().collect();
    for id in ids {
        subtree.tiles.set_visible(id, true);
    }
}

fn debug_tree_summary<Pane>(tree: &Tree<Pane>, max_nodes: usize) -> String {
    let Some(root) = tree.root else {
        return "root=None".to_owned();
    };

    let total_tiles = tree.tiles.iter().count();
    let mut seen: std::collections::HashSet<TileId> = std::collections::HashSet::new();
    let mut stack: Vec<TileId> = vec![root];
    let mut lines: Vec<String> = Vec::new();

    while let Some(tile_id) = stack.pop() {
        if !seen.insert(tile_id) {
            continue;
        }

        let visible = tree.is_visible(tile_id);
        let Some(tile) = tree.tiles.get(tile_id) else {
            lines.push(format!("{tile_id:?} MISSING visible={visible}"));
            continue;
        };

        match tile {
            egui_tiles::Tile::Pane(_) => {
                lines.push(format!("{tile_id:?} Pane visible={visible}"));
            }
            egui_tiles::Tile::Container(container) => {
                let kind = container.kind();
                let children: Vec<TileId> = container.children().copied().collect();
                lines.push(format!(
                    "{tile_id:?} Container({kind:?}) visible={visible} children={children:?}"
                ));
                stack.extend(children);
            }
        }

        if lines.len() >= max_nodes {
            break;
        }
    }

    format!(
        "root={root:?} reachable={} total={total_tiles}\n{}",
        seen.len(),
        lines.join("\n")
    )
}

impl<Pane> DockingMultiViewport<Pane> {
    pub(super) fn apply_pending_local_drop(
        &mut self,
        ctx: &Context,
        behavior: &mut dyn Behavior<Pane>,
    ) {
        let Some(pending) = self.pending_local_drop.take() else {
            return;
        };

        if pending.target_host.viewport() != pending.payload.source_viewport {
            return;
        }

        // Safety net: if this is an internal dock→dock drop inside the same viewport, we must not
        // re-apply extract+insert here. `egui_tiles` already handled it (or will handle it).
        if drop_policy::should_skip_local_drop_internal_dock_to_dock(
            &pending.payload,
            pending.target_surface.viewport(),
            pending.target_surface,
        ) {
            if self.options.debug_event_log {
                self.debug_log_event(format!(
                    "apply_local_drop SKIP internal_dock_to_dock viewport={:?} payload_tile_id={:?}",
                    pending.target_host.viewport(),
                    pending.payload.tile_id
                ));
            }
            return;
        }

        let is_moving_floating_window =
            pending.payload.source_floating.is_some() && pending.payload.tile_id.is_none();
        if is_moving_floating_window {
            if !self
                .options
                .window_move_docking_enabled_by_shift(pending.modifiers.shift)
            {
                if self.options.debug_event_log {
                    self.debug_log_event(format!(
                        "apply_local_drop SKIP (shift gating) viewport={:?} floating={:?} shift_held={} config_docking_with_shift={}",
                        pending.target_surface.viewport(),
                        pending.payload.source_floating,
                        pending.modifiers.shift,
                        self.options.config_docking_with_shift
                    ));
                }
                return;
            }
        }
        let insertion = if is_moving_floating_window {
            self.window_move_insertion_at_pointer_local(
                behavior,
                ctx.global_style().as_ref(),
                pending.target_surface,
                pending.pointer_local,
            )
        } else {
            self.insertion_at_pointer_local(
                behavior,
                ctx.global_style().as_ref(),
                pending.target_surface,
                pending.pointer_local,
                pending.payload.tile_id,
            )
        };
        if is_moving_floating_window && insertion.is_none() {
            if self.options.debug_event_log {
                self.debug_log_event(format!(
                    "apply_local_drop CANCEL (no overlay target) viewport={:?} floating={:?} pointer_local=({:.1},{:.1})",
                    pending.target_surface.viewport(),
                    pending.payload.source_floating,
                    pending.pointer_local.x,
                    pending.pointer_local.y,
                ));
            }
            return;
        }

        let source_host = pending.payload.source_host();
        let subtree = match pending.payload.tile_id {
            Some(tile_id) => {
                self.take_subtree_from_host_for_drop(ctx, behavior, source_host, tile_id)
            }
            None => self.take_whole_tree_from_host_for_drop(ctx, behavior, source_host),
        };

        let Some(mut subtree) = subtree else {
            if self.options.debug_event_log {
                self.debug_log_event(format!(
                    "apply_local_drop FAILED extract subtree viewport={:?} source_floating={:?} tile_id={:?}",
                    pending.target_host.viewport(),
                    pending.payload.source_floating,
                    pending.payload.tile_id
                ));
            }
            return;
        };
        force_subtree_visible(&mut subtree);

        let target_host = pending.target_host;
        let target_parent_exists = |parent_id: TileId| self
            .tree_for_host(target_host)
            .is_some_and(|t| t.tiles.get(parent_id).is_some());
        let insertion_sanitized = drop_sanitize::sanitize_insertion_for_subtree(
            insertion,
            &subtree,
            target_parent_exists,
        );
        if self.options.debug_event_log && insertion_sanitized != insertion {
            self.debug_log_event(format!(
                "apply_local_drop sanitize insertion viewport={:?} before={insertion:?} after={insertion_sanitized:?}",
                pending.target_surface.viewport()
            ));
        }

        let subtree = match self.insert_subtree_into_host(target_host, subtree, insertion_sanitized)
        {
            Ok(()) => None,
            Err(subtree) => Some(subtree),
        };
        if let Some(subtree) = subtree {
            // Target host disappeared; fall back to dock tree in the same viewport.
            let fallback = WindowHost::DockTree {
                viewport: pending.target_host.viewport(),
            };
            let _ = self.insert_subtree_into_host(fallback, subtree, insertion_sanitized);
        }
        behavior.on_edit(egui_tiles::EditAction::TileDropped);
        if self.options.debug_event_log {
            self.debug_log_event(format!(
                "apply_local_drop OK target_surface={:?} insertion={insertion_sanitized:?}",
                pending.target_surface
            ));
        }
    }

    pub(super) fn apply_pending_drop(&mut self, ctx: &Context, behavior: &mut dyn Behavior<Pane>) {
        let Some(pending) = self.pending_drop.take() else {
            return;
        };

        if pending.payload.tile_id.is_none()
            && !self
                .options
                .window_move_docking_enabled_by_shift(pending.modifiers.shift)
        {
            if self.options.debug_event_log {
                self.debug_log_event(format!(
                    "apply_pending_drop SKIP (shift gating) source_host={:?} source_viewport={:?} shift_held={} config_docking_with_shift={}",
                    pending.payload.source_host(),
                    pending.payload.source_viewport,
                    pending.modifiers.shift,
                    self.options.config_docking_with_shift
                ));
            }
            return;
        }

        let resolved = self.resolve_cross_viewport_drop(ctx, behavior, pending.payload, pending.pointer_global);
        let Some(resolved) = resolved else {
            return;
        };

        // ImGui parity: if we are moving a whole window host (native viewport / contained floating)
        // and we did not hover an explicit overlay target, releasing must *not* mutate the dock tree.
        if resolved.payload.tile_id.is_none() && resolved.target.insertion.is_none() {
            if self.options.debug_event_log {
                self.debug_log_event(format!(
                    "apply_pending_drop CANCEL (no overlay target) source_host={:?} target_surface={:?} pointer_local=({:.1},{:.1})",
                    resolved.payload.source_host(),
                    resolved.target.target_surface,
                    resolved.target.pointer_local.x,
                    resolved.target.pointer_local.y
                ));
            }
            return;
        }

        self.apply_resolved_cross_viewport_drop(ctx, behavior, resolved);
    }

    fn resolve_cross_viewport_drop(
        &mut self,
        ctx: &Context,
        behavior: &mut dyn Behavior<Pane>,
        payload: DockPayload,
        pointer_global: egui::Pos2,
    ) -> Option<ResolvedDrop> {
        let is_window_move = payload.tile_id.is_none();

        let exclude_viewport = payload.tile_id.is_none().then_some(payload.source_viewport);
        let Some((target_surface, pointer_local)) =
            self.surface_under_pointer_global(ctx, pointer_global, exclude_viewport, None)
        else {
            return None;
        };
        let target_viewport = target_surface.viewport();
        if target_viewport == payload.source_viewport {
            return None;
        }

        let dock_rect = self.dock_rect_for_surface(target_surface)?;
        let tree = self.tree_for_surface(target_surface)?;
        let style = ctx.global_style();
        let drag_kind = if is_window_move {
            DragKind::WindowMove {
                tab_dock_requires_explicit_target: self
                    .options
                    .window_move_tab_dock_requires_explicit_target,
            }
        } else {
            DragKind::Subtree {
                dragged_tile: None,
                internal: false,
            }
        };
        let decision = decide_overlay_for_tree(
            tree,
            behavior,
            &style,
            dock_rect,
            pointer_local,
            self.options.show_outer_overlay_targets,
            drag_kind,
        );
        let insertion = decision.insertion_final;

        let target_host = match target_surface {
            DockSurface::DockTree { viewport } => WindowHost::DockTree { viewport },
            DockSurface::Floating { viewport, floating } => WindowHost::Floating { viewport, floating },
        };
        let resolved = ResolvedDrop {
            payload,
            pointer_global,
            target: ResolvedDropTarget {
                target_surface,
                target_host,
                pointer_local,
                insertion,
            },
        };

        if self.options.debug_event_log {
            self.debug_log_event(format!(
                "resolve_cross_viewport_drop window_move={is_window_move} source_host={:?} payload_tile_id={:?} pointer_global=({:.1},{:.1}) target_host={:?} target_surface={:?} pointer_local=({:.1},{:.1}) insertion={:?}",
                resolved.payload.source_host(),
                resolved.payload.tile_id,
                resolved.pointer_global.x,
                resolved.pointer_global.y,
                resolved.target.target_host,
                resolved.target.target_surface,
                resolved.target.pointer_local.x,
                resolved.target.pointer_local.y,
                resolved.target.insertion,
            ));
        }

        Some(resolved)
    }

    fn apply_resolved_cross_viewport_drop(
        &mut self,
        ctx: &Context,
        behavior: &mut dyn Behavior<Pane>,
        resolved: ResolvedDrop,
    ) {
        // Defensive guard: some callers may bypass `apply_pending_drop` and call this directly.
        // If we are moving a whole window host and there is no valid insertion, this must be a no-op.
        if resolved.payload.tile_id.is_none() && resolved.target.insertion.is_none() {
            if self.options.debug_event_log {
                self.debug_log_event(format!(
                    "apply_cross_viewport_drop CANCEL (no overlay target) source_host={:?} target_surface={:?}",
                    resolved.payload.source_host(),
                    resolved.target.target_surface
                ));
            }
            return;
        }

        let source_host = resolved.payload.source_host();
        let target_host = resolved.target.target_host;
        if self.options.debug_event_log {
            let source_before = self
                .tree_for_host(source_host)
                .map(|t| debug_tree_summary(t, 40))
                .unwrap_or_else(|| "missing".to_owned());
            let target_before = self
                .tree_for_host(target_host)
                .map(|t| debug_tree_summary(t, 40))
                .unwrap_or_else(|| "missing".to_owned());
            self.debug_log_event(format!(
                "apply_cross_viewport_drop BEGIN source_host={source_host:?} payload_tile_id={:?} target_host={target_host:?} target_surface={:?} insertion={:?}",
                resolved.payload.tile_id,
                resolved.target.target_surface,
                resolved.target.insertion
            ));
            self.debug_log_event(format!("source_before:\n{source_before}"));
            self.debug_log_event(format!("target_before:\n{target_before}"));
        }

        let subtree = match resolved.payload.tile_id {
            Some(tile_id) => self.take_subtree_from_host_for_drop(ctx, behavior, source_host, tile_id),
            None => self.take_whole_tree_from_host_for_drop(ctx, behavior, source_host),
        };
        let Some(mut subtree) = subtree else {
            return;
        };
        force_subtree_visible(&mut subtree);

        let target_parent_exists = |parent_id: TileId| self
            .tree_for_host(target_host)
            .is_some_and(|t| t.tiles.get(parent_id).is_some());
        let insertion_sanitized = drop_sanitize::sanitize_insertion_for_subtree(
            resolved.target.insertion,
            &subtree,
            target_parent_exists,
        );

        let subtree = match self.insert_subtree_into_host(target_host, subtree, insertion_sanitized)
        {
            Ok(()) => None,
            Err(subtree) => Some(subtree),
        };
        if let Some(subtree) = subtree {
            // Target host disappeared; fall back to dock tree in the same viewport.
            let fallback = WindowHost::DockTree {
                viewport: target_host.viewport(),
            };
            let _ = self.insert_subtree_into_host(fallback, subtree, insertion_sanitized);
        }

        // Keep detached window title in sync to avoid one-frame mismatch after drops.
        if let WindowHost::DockTree { viewport } = target_host
            && viewport != ViewportId::ROOT
            && let Some(detached) = self.detached.get_mut(&viewport)
        {
            detached.builder = detached
                .builder
                .clone()
                .with_title(title_for_detached_tree(&detached.tree, behavior));
        }

        behavior.on_edit(egui_tiles::EditAction::TileDropped);
        if self.options.debug_event_log {
            for issue in integrity::tree_integrity_issues(&self.tree) {
                self.debug_log_event(issue);
            }

            let source_after = self
                .tree_for_host(source_host)
                .map(|t| debug_tree_summary(t, 40))
                .unwrap_or_else(|| "missing".to_owned());
            let target_after = self
                .tree_for_host(target_host)
                .map(|t| debug_tree_summary(t, 40))
                .unwrap_or_else(|| "missing".to_owned());
            self.debug_log_event(format!(
                "apply_cross_viewport_drop END source_host={source_host:?} target_host={target_host:?} insertion_sanitized={insertion_sanitized:?}",
            ));
            self.debug_log_event(format!("source_after:\n{source_after}"));
            self.debug_log_event(format!("target_after:\n{target_after}"));
        }
    }

    pub(super) fn apply_pending_actions(
        &mut self,
        ctx: &Context,
        behavior: &mut dyn Behavior<Pane>,
    ) {
        self.apply_pending_drop(ctx, behavior);
        self.apply_pending_internal_drop(behavior);
        self.apply_pending_local_drop(ctx, behavior);
    }

    pub(super) fn apply_pending_internal_drop(&mut self, behavior: &mut dyn Behavior<Pane>) {
        let Some(pending) = self.pending_internal_drop.take() else {
            return;
        };

        if pending.viewport == ViewportId::ROOT {
            if self.options.debug_event_log {
                self.debug_log_event(format!(
                    "apply_internal_drop BEGIN viewport=ROOT tile_id={:?} insertion={:?}",
                    pending.tile_id, pending.insertion
                ));
                self.debug_log_event(format!(
                    "tree_before:\n{}",
                    debug_tree_summary(&self.tree, 80)
                ));
            }
            let Some(mut subtree) = self.tree.extract_subtree_no_reserve(pending.tile_id) else {
                self.debug_log_event(
                    "apply_internal_drop FAILED extract_subtree_no_reserve returned None"
                        .to_owned(),
                );
                return;
            };
            force_subtree_visible(&mut subtree);

            let insertion = self
                .tree
                .tiles
                .get(pending.insertion.parent_id)
                .is_some()
                .then_some(pending.insertion);
            behavior.on_edit(EditAction::TileDropped);
            self.tree.insert_subtree_at(subtree, insertion);
            if self.options.debug_event_log {
                for issue in integrity::tree_integrity_issues(&self.tree) {
                    self.debug_log_event(issue);
                }
                self.debug_log_event(format!(
                    "apply_internal_drop END viewport=ROOT\n{}",
                    debug_tree_summary(&self.tree, 80)
                ));
            }
            return;
        }

        let Some(mut detached) = self.detached.remove(&pending.viewport) else {
            self.debug_log_event(format!(
                "apply_internal_drop FAILED: missing detached viewport={:?}",
                pending.viewport
            ));
            return;
        };

        if self.options.debug_event_log {
            self.debug_log_event(format!(
                "apply_internal_drop BEGIN viewport={:?} tile_id={:?} insertion={:?}",
                pending.viewport, pending.tile_id, pending.insertion
            ));
            self.debug_log_event(format!(
                "detached_tree_before:\n{}",
                debug_tree_summary(&detached.tree, 80)
            ));
        }

        let Some(mut subtree) = detached.tree.extract_subtree_no_reserve(pending.tile_id) else {
            self.detached.insert(pending.viewport, detached);
            self.debug_log_event(
                "apply_internal_drop FAILED extract_subtree_no_reserve returned None (detached)"
                    .to_owned(),
            );
            return;
        };
        force_subtree_visible(&mut subtree);

        let insertion = detached
            .tree
            .tiles
            .get(pending.insertion.parent_id)
            .is_some()
            .then_some(pending.insertion);
        behavior.on_edit(EditAction::TileDropped);
        detached.tree.insert_subtree_at(subtree, insertion);
        if self.options.debug_event_log {
            for issue in integrity::tree_integrity_issues(&detached.tree) {
                self.debug_log_event(issue);
            }
            self.debug_log_event(format!(
                "apply_internal_drop END viewport={:?}\n{}",
                pending.viewport,
                debug_tree_summary(&detached.tree, 80)
            ));
        }

        detached.builder = detached
            .builder
            .clone()
            .with_title(title_for_detached_tree(&detached.tree, behavior));
        self.detached.insert(pending.viewport, detached);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::multi_viewport::surface::DockSurface;
    use crate::multi_viewport::types::{DockPayload, ResolvedDrop, ResolvedDropTarget};
    use crate::multi_viewport::host::WindowHost;

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
            egui::WidgetText::from("pane")
        }
    }

    fn new_tree_tabs(id: egui::Id, panes: usize) -> egui_tiles::Tree<()> {
        let panes = panes.max(1);
        let mut tiles = egui_tiles::Tiles::default();
        let mut ids = Vec::with_capacity(panes);
        for _ in 0..panes {
            ids.push(tiles.insert_pane(()));
        }
        let root = tiles.insert_tab_tile(ids);
        egui_tiles::Tree::new(id, root, tiles)
    }

    #[test]
    fn cross_viewport_window_move_without_target_is_noop() {
        let root_tree = new_tree_tabs(egui::Id::new("root"), 2);
        let mut docking = DockingMultiViewport::new(root_tree);

        let detached_viewport = ViewportId::from_hash_of("detached");
        let detached_tree = new_tree_tabs(egui::Id::new("detached_tree"), 3);
        let detached_tiles_before = detached_tree.tiles.tile_ids().count();
        docking.detached.insert(
            detached_viewport,
            crate::multi_viewport::types::DetachedDock {
                tree: detached_tree,
                builder: egui::ViewportBuilder::default(),
            },
        );
        let root_tiles_before = docking.tree.tiles.tile_ids().count();

        let ctx = egui::Context::default();
        let mut behavior = DummyBehavior::default();
        let resolved = ResolvedDrop {
            payload: DockPayload {
                bridge_id: docking.tree.id(),
                source_viewport: detached_viewport,
                source_floating: None,
                tile_id: None,
            },
            pointer_global: egui::Pos2::new(10.0, 10.0),
            target: ResolvedDropTarget {
                target_surface: DockSurface::DockTree {
                    viewport: ViewportId::ROOT,
                },
                target_host: WindowHost::DockTree {
                    viewport: ViewportId::ROOT,
                },
                pointer_local: egui::Pos2::new(100.0, 100.0),
                insertion: None,
            },
        };

        docking.apply_resolved_cross_viewport_drop(&ctx, &mut behavior, resolved);

        assert!(docking.detached.contains_key(&detached_viewport));
        assert_eq!(docking.tree.tiles.tile_ids().count(), root_tiles_before);
        assert_eq!(
            docking
                .detached
                .get(&detached_viewport)
                .unwrap()
                .tree
                .tiles
                .tile_ids()
                .count(),
            detached_tiles_before
        );
    }
}
