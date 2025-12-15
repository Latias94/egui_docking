use egui::{Context, Pos2, Rect, ViewportId};
use egui_tiles::{Behavior, EditAction, InsertionPoint, TileId, Tree};

use super::geometry::{pointer_pos_in_global, pointer_pos_in_target_viewport_space};
use super::overlay::{
    overlay_insertion_for_tree_explicit_with_outer, overlay_insertion_for_tree_with_outer,
    tile_contains_descendant,
};
use super::title::title_for_detached_tree;
use super::types::{DetachedDock, DockPayload, DropAction, FloatingId, PendingDrop, PendingInternalDrop, PendingLocalDrop};
use super::DockingMultiViewport;

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

fn debug_tree_integrity_issues<Pane>(tree: &Tree<Pane>) -> Vec<String> {
    let Some(root) = tree.root else {
        return vec!["integrity: root=None".to_owned()];
    };

    let mut issues: Vec<String> = Vec::new();
    let mut seen: std::collections::HashSet<TileId> = std::collections::HashSet::new();
    let mut stack: Vec<TileId> = vec![root];

    while let Some(tile_id) = stack.pop() {
        if !seen.insert(tile_id) {
            continue;
        }

        let Some(tile) = tree.tiles.get(tile_id) else {
            issues.push(format!("integrity: missing tile {tile_id:?}"));
            continue;
        };

        if let egui_tiles::Tile::Container(container) = tile {
            for &child in container.children() {
                if tree.tiles.get(child).is_none() {
                    issues.push(format!(
                        "integrity: parent {tile_id:?} references missing child {child:?}"
                    ));
                } else {
                    stack.push(child);
                }
            }
        }
    }

    issues
}

impl<Pane> DockingMultiViewport<Pane> {
    pub(super) fn queue_pending_local_drop_on_release(
        &mut self,
        ctx: &Context,
        dock_rect: Rect,
        viewport_id: ViewportId,
    ) {
        if self.pending_drop.is_some()
            || self.pending_internal_drop.is_some()
            || self.pending_local_drop.is_some()
        {
            return;
        }
        if !ctx.input(|i| i.pointer.any_released()) {
            return;
        }

        let Some(payload) = egui::DragAndDrop::payload::<DockPayload>(ctx) else {
            return;
        };
        if payload.bridge_id != self.tree.id() {
            return;
        }
        if payload.source_viewport != viewport_id {
            return;
        }

        let Some(pointer_local) = ctx.input(|i| i.pointer.latest_pos()) else {
            return;
        };

        // Floating windows are top-most surfaces inside `dock_rect`, so check them first.
        let target_floating = self.floating_under_pointer(viewport_id, pointer_local);
        if target_floating.is_none() && !dock_rect.contains(pointer_local) {
            return;
        }

        if payload.source_floating == target_floating {
            return;
        }
        if payload.source_floating.is_none() && payload.tile_id.is_none() {
            // We don't support moving the whole dock tree within a viewport.
            return;
        }

        self.pending_local_drop = Some(PendingLocalDrop {
            payload: *payload,
            target_viewport: viewport_id,
            target_floating,
            pointer_local,
        });

        egui::DragAndDrop::clear_payload(ctx);
        ctx.stop_dragging();
        ctx.request_repaint_of(ViewportId::ROOT);
    }

    pub(super) fn queue_pending_local_drop_from_dragged_tile_on_release(
        &mut self,
        ctx: &Context,
        dock_rect: Rect,
        viewport_id: ViewportId,
        source_floating: Option<FloatingId>,
        dragged_tile: TileId,
    ) {
        if self.pending_drop.is_some()
            || self.pending_internal_drop.is_some()
            || self.pending_local_drop.is_some()
        {
            return;
        }
        if !ctx.input(|i| i.pointer.any_released()) {
            return;
        }

        let Some(pointer_local) = ctx.input(|i| i.pointer.latest_pos()) else {
            return;
        };

        let target_floating = self.floating_under_pointer(viewport_id, pointer_local);

        if target_floating.is_none() && !dock_rect.contains(pointer_local) {
            return;
        }

        // If you are still inside the same floating window, let `egui_tiles` handle internal drops/reorder.
        if target_floating == source_floating {
            return;
        }

        if source_floating.is_none() && target_floating.is_none() {
            return;
        }

        self.pending_local_drop = Some(PendingLocalDrop {
            payload: DockPayload {
                bridge_id: self.tree.id(),
                source_viewport: viewport_id,
                source_floating,
                tile_id: Some(dragged_tile),
            },
            target_viewport: viewport_id,
            target_floating,
            pointer_local,
        });

        egui::DragAndDrop::clear_payload(ctx);
        ctx.stop_dragging();
        ctx.request_repaint_of(ViewportId::ROOT);
    }

    pub(super) fn drop_insertion_at_pointer_local(
        &self,
        behavior: &dyn Behavior<Pane>,
        style: &egui::Style,
        viewport_id: ViewportId,
        target_floating: Option<FloatingId>,
        pointer_local: Pos2,
    ) -> Option<InsertionPoint> {
        if let Some(floating_id) = target_floating {
            let dock_rect = self
                .last_floating_rects
                .get(&(viewport_id, floating_id))
                .copied()?;
            let tree = self
                .floating
                .get(&viewport_id)?
                .windows
                .get(&floating_id)
                .map(|w| &w.tree)?;
            return overlay_insertion_for_tree_with_outer(tree, dock_rect, pointer_local).or_else(
                || {
                    tree.dock_zone_at(behavior, style, pointer_local)
                        .map(|z| z.insertion_point)
                },
            );
        }

        if viewport_id == ViewportId::ROOT {
            let dock_rect = self.last_dock_rects.get(&ViewportId::ROOT).copied()?;
            return overlay_insertion_for_tree_with_outer(&self.tree, dock_rect, pointer_local)
                .or_else(|| {
                    self.tree
                        .dock_zone_at(behavior, style, pointer_local)
                        .map(|z| z.insertion_point)
                });
        }

        let tree = self.detached.get(&viewport_id).map(|d| &d.tree)?;
        let dock_rect = self.last_dock_rects.get(&viewport_id).copied()?;
        overlay_insertion_for_tree_with_outer(tree, dock_rect, pointer_local).or_else(|| {
            tree.dock_zone_at(behavior, style, pointer_local)
                .map(|z| z.insertion_point)
        })
    }

    pub(super) fn apply_pending_local_drop(&mut self, ctx: &Context, behavior: &mut dyn Behavior<Pane>) {
        let Some(pending) = self.pending_local_drop.take() else {
            return;
        };

        if pending.target_viewport != pending.payload.source_viewport {
            return;
        }

        let subtree = match (pending.payload.source_floating, pending.payload.tile_id) {
            (Some(floating_id), Some(tile_id)) => {
                self.extract_subtree_from_floating(pending.target_viewport, floating_id, tile_id)
            }
            (Some(floating_id), None) => {
                self.take_whole_floating_tree(pending.target_viewport, floating_id)
            }
            (None, Some(tile_id)) => {
                if pending.target_viewport == ViewportId::ROOT {
                    self.tree.extract_subtree(tile_id)
                } else if let Some(detached) = self.detached.get_mut(&pending.target_viewport) {
                    detached.tree.extract_subtree(tile_id)
                } else {
                    None
                }
            }
            (None, None) => None,
        };

        let Some(mut subtree) = subtree else {
            return;
        };
        force_subtree_visible(&mut subtree);

        let insertion = self.drop_insertion_at_pointer_local(
            behavior,
            ctx.style().as_ref(),
            pending.target_viewport,
            pending.target_floating,
            pending.pointer_local,
        );

        if let Some(target_floating) = pending.target_floating {
            let mut manager = self
                .floating
                .remove(&pending.target_viewport)
                .unwrap_or_default();
            if let Some(w) = manager.windows.get_mut(&target_floating) {
                w.tree.insert_subtree_at(subtree, insertion);
                manager.bring_to_front(target_floating);
                self.floating.insert(pending.target_viewport, manager);
            } else if pending.target_viewport == ViewportId::ROOT {
                self.dock_subtree_into_root(subtree, insertion);
            } else if let Some(detached) = self.detached.get_mut(&pending.target_viewport) {
                detached.tree.insert_subtree_at(subtree, insertion);
            }
            return;
        }

        self.dock_subtree_into_dock_tree(pending.target_viewport, subtree, insertion);
        behavior.on_edit(egui_tiles::EditAction::TileDropped);
    }

    pub(super) fn queue_pending_drop_on_release(&mut self, ctx: &Context) {
        if self.pending_drop.is_some() {
            return;
        }
        if !ctx.input(|i| i.pointer.any_released()) {
            return;
        }

        let Some(payload) = egui::DragAndDrop::payload::<DockPayload>(ctx) else {
            return;
        };
        if payload.bridge_id != self.tree.id() {
            return;
        }

        // Prefer the active viewport's computed global pointer, but fall back to the last known
        // global pointer from any viewport if needed.
        let pointer_global = pointer_pos_in_global(ctx).or(self.last_pointer_global);
        let Some(pointer_global) = pointer_global else {
            return;
        };

        let Some(target_viewport) = super::geometry::viewport_under_pointer_global(ctx, pointer_global)
        else {
            return;
        };
        if target_viewport == payload.source_viewport {
            return;
        }

        self.pending_drop = Some(PendingDrop {
            payload: *payload,
            pointer_global,
        });

        egui::DragAndDrop::clear_payload(ctx);
        ctx.stop_dragging();
        ctx.request_repaint_of(ViewportId::ROOT);
    }

    pub(super) fn pending_internal_overlay_drop_on_release(
        &self,
        ctx: &Context,
        dock_rect: Rect,
        viewport_id: ViewportId,
        tree: &Tree<Pane>,
    ) -> Option<PendingInternalDrop> {
        if !self.options.show_overlay_for_internal_drags {
            return None;
        }
        if self.options.detach_on_alt_release_anywhere && ctx.input(|i| i.modifiers.alt) {
            return None;
        }
        if !ctx.input(|i| i.pointer.any_released()) {
            return None;
        }

        let dragged_tile = tree.dragged_id_including_root(ctx)?;
        let pointer_local = ctx.input(|i| i.pointer.latest_pos())?;
        if !dock_rect.contains(pointer_local) {
            return None;
        }
        if self
            .floating_tree_id_under_pointer(viewport_id, pointer_local)
            .is_some_and(|floating_tree_id| floating_tree_id != tree.id())
        {
            return None;
        }

        let insertion =
            overlay_insertion_for_tree_explicit_with_outer(tree, dock_rect, pointer_local)?;
        if tile_contains_descendant(tree, dragged_tile, insertion.parent_id) {
            return None;
        }

        Some(PendingInternalDrop {
            viewport: viewport_id,
            tile_id: dragged_tile,
            insertion,
        })
    }

    pub(super) fn apply_pending_drop(&mut self, ctx: &Context, behavior: &mut dyn Behavior<Pane>) {
        let Some(pending) = self.pending_drop.take() else {
            return;
        };

        let Some(target_viewport) =
            super::geometry::viewport_under_pointer_global(ctx, pending.pointer_global)
        else {
            return;
        };
        if target_viewport == pending.payload.source_viewport {
            return;
        }

        let style = ctx.style();
        let insertion = self.drop_insertion_at_pointer_global(
            ctx,
            behavior,
            &style,
            target_viewport,
            pending.pointer_global,
        );

        if target_viewport == ViewportId::ROOT {
            self.apply_drop_to_root(ctx, behavior, insertion, pending.payload);
        } else {
            self.apply_drop_to_detached(ctx, behavior, target_viewport, insertion, pending.payload);
        }
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
                self.debug_log_event("apply_internal_drop FAILED extract_subtree_no_reserve returned None".to_owned());
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
                for issue in debug_tree_integrity_issues(&self.tree) {
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
            self.debug_log_event("apply_internal_drop FAILED extract_subtree_no_reserve returned None (detached)".to_owned());
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
            for issue in debug_tree_integrity_issues(&detached.tree) {
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

    pub(super) fn drop_insertion_at_pointer_global(
        &self,
        ctx: &Context,
        behavior: &dyn Behavior<Pane>,
        style: &egui::Style,
        target_viewport: ViewportId,
        pointer_global: Pos2,
    ) -> Option<InsertionPoint> {
        let Some(pointer_local) =
            pointer_pos_in_target_viewport_space(ctx, target_viewport, pointer_global)
        else {
            return None;
        };

        let dock_rect = self.last_dock_rects.get(&target_viewport).copied();
        if dock_rect.is_some_and(|r| !r.contains(pointer_local)) {
            return None;
        }
        let dock_rect = dock_rect?;

        if target_viewport == ViewportId::ROOT {
            if let Some(insertion) =
                overlay_insertion_for_tree_with_outer(&self.tree, dock_rect, pointer_local)
            {
                return Some(insertion);
            }
            return self
                .tree
                .dock_zone_at(behavior, style, pointer_local)
                .map(|z| z.insertion_point);
        }

        let Some(detached) = self.detached.get(&target_viewport) else {
            return None;
        };
        if let Some(insertion) =
            overlay_insertion_for_tree_with_outer(&detached.tree, dock_rect, pointer_local)
        {
            return Some(insertion);
        }
        detached
            .tree
            .dock_zone_at(behavior, style, pointer_local)
            .map(|z| z.insertion_point)
    }

    pub(super) fn apply_drop_to_root(
        &mut self,
        ctx: &Context,
        behavior: &mut dyn Behavior<Pane>,
        insertion: Option<InsertionPoint>,
        payload: DockPayload,
    ) {
        if payload.source_viewport == ViewportId::ROOT {
            return;
        }

        if let Some(floating_id) = payload.source_floating {
            if let Some(tile_id) = payload.tile_id {
                if let Some(subtree) = self.extract_subtree_from_floating(
                    payload.source_viewport,
                    floating_id,
                    tile_id,
                ) {
                    self.dock_subtree_into_root(subtree, insertion);
                }
            } else if let Some(subtree) =
                self.take_whole_floating_tree(payload.source_viewport, floating_id)
            {
                self.dock_subtree_into_root(subtree, insertion);
            }

            return;
        }

        let Some(mut detached) = self.detached.remove(&payload.source_viewport) else {
            return;
        };

        if let Some(tile_id) = payload.tile_id {
            let Some(subtree) = detached.tree.extract_subtree(tile_id) else {
                self.detached.insert(payload.source_viewport, detached);
                return;
            };

            self.dock_subtree_into_root(subtree, insertion);

            if detached.tree.root.is_some() {
                detached.builder = detached
                    .builder
                    .clone()
                    .with_title(title_for_detached_tree(&detached.tree, behavior));
                self.detached.insert(payload.source_viewport, detached);
            } else {
                ctx.send_viewport_cmd_to(payload.source_viewport, egui::ViewportCommand::Close);
            }
        } else {
            self.dock_tree_into_root(detached.tree, insertion);
            ctx.send_viewport_cmd_to(payload.source_viewport, egui::ViewportCommand::Close);
        }
    }

    pub(super) fn apply_drop_to_detached(
        &mut self,
        ctx: &Context,
        behavior: &mut dyn Behavior<Pane>,
        target_viewport: ViewportId,
        insertion: Option<InsertionPoint>,
        payload: DockPayload,
    ) {
        let Some(mut target) = self.detached.remove(&target_viewport) else {
            return;
        };

        if payload.source_viewport == target_viewport {
            self.detached.insert(target_viewport, target);
            return;
        }

        let action = match payload.tile_id {
            Some(tile_id) => DropAction::MoveSubtree {
                source_viewport: payload.source_viewport,
                source_floating: payload.source_floating,
                tile_id,
                insertion,
            },
            None => DropAction::MoveWholeTree {
                source_viewport: payload.source_viewport,
                source_floating: payload.source_floating,
                insertion,
            },
        };

        self.apply_drop_action_to_detached_target(
            ctx,
            target_viewport,
            &mut target,
            action,
            behavior,
        );

        target.builder = target
            .builder
            .clone()
            .with_title(title_for_detached_tree(&target.tree, behavior));
        self.detached.insert(target_viewport, target);
    }

    pub(super) fn apply_drop_action_to_detached_target(
        &mut self,
        ctx: &Context,
        target_viewport: ViewportId,
        target: &mut DetachedDock<Pane>,
        action: DropAction,
        behavior: &mut dyn Behavior<Pane>,
    ) {
        match action {
            DropAction::MoveSubtree {
                source_viewport,
                source_floating,
                tile_id,
                insertion,
            } => {
                let subtree = if source_viewport == ViewportId::ROOT && source_floating.is_none() {
                    self.tree.extract_subtree(tile_id)
                } else if source_viewport == target_viewport {
                    None
                } else if let Some(floating_id) = source_floating {
                    self.extract_subtree_from_floating(source_viewport, floating_id, tile_id)
                } else if let Some(mut source) = self.detached.remove(&source_viewport) {
                    let extracted = source.tree.extract_subtree(tile_id);
                    if extracted.is_some() {
                        if source.tree.root.is_some() {
                            source.builder = source
                                .builder
                                .clone()
                                .with_title(title_for_detached_tree(&source.tree, behavior));
                            self.detached.insert(source_viewport, source);
                        } else {
                            ctx.send_viewport_cmd_to(
                                source_viewport,
                                egui::ViewportCommand::Close,
                            );
                        }
                    } else {
                        self.detached.insert(source_viewport, source);
                    }
                    extracted
                } else {
                    None
                };

                if let Some(mut subtree) = subtree {
                    force_subtree_visible(&mut subtree);
                    target.tree.insert_subtree_at(subtree, insertion);
                }
            }

            DropAction::MoveWholeTree {
                source_viewport,
                source_floating,
                insertion,
            } => {
                if source_viewport == ViewportId::ROOT || source_viewport == target_viewport {
                    return;
                }

                if let Some(floating_id) = source_floating {
                    if let Some(subtree) = self.take_whole_floating_tree(source_viewport, floating_id)
                    {
                        target.tree.insert_subtree_at(subtree, insertion);
                    }
                    return;
                }

                let Some(mut source) = self.detached.remove(&source_viewport) else {
                    return;
                };

                let Some(source_root) = source.tree.root.take() else {
                    return;
                };

                let source_tiles = std::mem::take(&mut source.tree.tiles);
                target.tree.insert_subtree_at(
                    egui_tiles::SubTree {
                        root: source_root,
                        tiles: source_tiles,
                    },
                    insertion,
                );

                ctx.send_viewport_cmd_to(source_viewport, egui::ViewportCommand::Close);
            }
        }
    }
}
