use egui::{Context, ViewportId};
use egui_tiles::{Behavior, EditAction, InsertionPoint, TileId, Tree};

use super::DockingMultiViewport;
use super::drop_sanitize;
use super::integrity;
use super::surface::DockSurface;
use super::title::title_for_detached_tree;
use super::types::{DetachedDock, DockPayload, DropAction, FloatingId};

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
    fn take_subtree_from_source_for_cross_viewport_drop(
        &mut self,
        ctx: &Context,
        behavior: &mut dyn Behavior<Pane>,
        source_viewport: ViewportId,
        source_floating: Option<FloatingId>,
        tile_id: TileId,
    ) -> Option<egui_tiles::SubTree<Pane>> {
        if source_viewport == ViewportId::ROOT && source_floating.is_none() {
            return self.tree.extract_subtree(tile_id);
        }

        if let Some(floating_id) = source_floating {
            return self.extract_subtree_from_floating(source_viewport, floating_id, tile_id);
        }

        let Some(mut source) = self.detached.remove(&source_viewport) else {
            return None;
        };

        let extracted = source.tree.extract_subtree(tile_id);
        if extracted.is_some() {
            if source.tree.root.is_some() {
                source.builder = source
                    .builder
                    .clone()
                    .with_title(title_for_detached_tree(&source.tree, behavior));
                self.detached.insert(source_viewport, source);
            } else {
                ctx.send_viewport_cmd_to(source_viewport, egui::ViewportCommand::Close);
            }
        } else {
            self.detached.insert(source_viewport, source);
        }

        extracted
    }

    fn take_whole_tree_from_source_for_cross_viewport_drop(
        &mut self,
        ctx: &Context,
        behavior: &mut dyn Behavior<Pane>,
        source_viewport: ViewportId,
        source_floating: Option<FloatingId>,
    ) -> Option<egui_tiles::SubTree<Pane>> {
        if source_viewport == ViewportId::ROOT {
            // Not supported: moving the whole root dock tree.
            return None;
        }

        if let Some(floating_id) = source_floating {
            return self.take_whole_floating_tree(source_viewport, floating_id);
        }

        let Some(mut source) = self.detached.remove(&source_viewport) else {
            return None;
        };

        let Some(root) = source.tree.root.take() else {
            // Should not happen, but keep the map consistent.
            if source.tree.root.is_some() {
                source.builder = source
                    .builder
                    .clone()
                    .with_title(title_for_detached_tree(&source.tree, behavior));
                self.detached.insert(source_viewport, source);
            }
            return None;
        };
        let tiles = std::mem::take(&mut source.tree.tiles);
        let subtree = egui_tiles::SubTree { root, tiles };

        ctx.send_viewport_cmd_to(source_viewport, egui::ViewportCommand::Close);
        Some(subtree)
    }

    pub(super) fn apply_pending_local_drop(
        &mut self,
        ctx: &Context,
        behavior: &mut dyn Behavior<Pane>,
    ) {
        let Some(pending) = self.pending_local_drop.take() else {
            return;
        };

        if pending.target_surface.viewport() != pending.payload.source_viewport {
            return;
        }

        let is_moving_floating_window =
            pending.payload.source_floating.is_some() && pending.payload.tile_id.is_none();
        let insertion = if is_moving_floating_window {
            self.explicit_insertion_at_pointer_local(pending.target_surface, pending.pointer_local)
        } else {
            self.insertion_at_pointer_local(
                behavior,
                ctx.global_style().as_ref(),
                pending.target_surface,
                pending.pointer_local,
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

        let subtree = match (pending.payload.source_floating, pending.payload.tile_id) {
            (Some(floating_id), Some(tile_id)) => self.extract_subtree_from_floating(
                pending.target_surface.viewport(),
                floating_id,
                tile_id,
            ),
            (Some(floating_id), None) => {
                self.take_whole_floating_tree(pending.target_surface.viewport(), floating_id)
            }
            (None, Some(tile_id)) => {
                if pending.target_surface.viewport() == ViewportId::ROOT {
                    self.tree.extract_subtree(tile_id)
                } else if let Some(detached) =
                    self.detached.get_mut(&pending.target_surface.viewport())
                {
                    detached.tree.extract_subtree(tile_id)
                } else {
                    None
                }
            }
            (None, None) => None,
        };

        let Some(mut subtree) = subtree else {
            if self.options.debug_event_log {
                self.debug_log_event(format!(
                    "apply_local_drop FAILED extract subtree viewport={:?} source_floating={:?} tile_id={:?}",
                    pending.target_surface.viewport(),
                    pending.payload.source_floating,
                    pending.payload.tile_id
                ));
            }
            return;
        };
        force_subtree_visible(&mut subtree);

        let target_parent_exists = |parent_id: TileId| match pending.target_surface {
            DockSurface::DockTree { viewport } => {
                if viewport == ViewportId::ROOT {
                    self.tree.tiles.get(parent_id).is_some()
                } else {
                    self.detached
                        .get(&viewport)
                        .is_some_and(|d| d.tree.tiles.get(parent_id).is_some())
                }
            }
            DockSurface::Floating { viewport, floating } => self
                .floating
                .get(&viewport)
                .and_then(|m| m.windows.get(&floating))
                .is_some_and(|w| w.tree.tiles.get(parent_id).is_some()),
        };
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

        match pending.target_surface {
            DockSurface::DockTree { viewport } => {
                self.dock_subtree_into_dock_tree(viewport, subtree, insertion_sanitized);
            }
            DockSurface::Floating { viewport, floating } => {
                let mut manager = self.floating.remove(&viewport).unwrap_or_default();
                if let Some(w) = manager.windows.get_mut(&floating) {
                    w.tree.insert_subtree_at(subtree, insertion_sanitized);
                    manager.bring_to_front(floating);
                    self.floating.insert(viewport, manager);
                } else {
                    // Target floating disappeared; fall back to dock tree.
                    self.dock_subtree_into_dock_tree(viewport, subtree, insertion_sanitized);
                }
            }
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

        let exclude_viewport = pending
            .payload
            .tile_id
            .is_none()
            .then_some(pending.payload.source_viewport);
        let Some((target_surface, pointer_local)) =
            self.surface_under_pointer_global(ctx, pending.pointer_global, exclude_viewport, None)
        else {
            return;
        };
        let target_viewport = target_surface.viewport();
        if target_viewport == pending.payload.source_viewport {
            return;
        }

        let style = ctx.global_style();
        let insertion =
            self.insertion_at_pointer_local(behavior, &style, target_surface, pointer_local);
        self.apply_drop_to_surface(ctx, behavior, target_surface, insertion, pending.payload);
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

    pub(super) fn apply_drop_to_surface(
        &mut self,
        ctx: &Context,
        behavior: &mut dyn Behavior<Pane>,
        target_surface: DockSurface,
        insertion: Option<InsertionPoint>,
        payload: DockPayload,
    ) {
        match target_surface {
            DockSurface::DockTree { viewport } => {
                if viewport == ViewportId::ROOT {
                    self.apply_drop_to_root(ctx, behavior, insertion, payload);
                } else {
                    self.apply_drop_to_detached(ctx, behavior, viewport, insertion, payload);
                }
            }
            DockSurface::Floating { viewport, floating } => {
                self.apply_drop_to_floating(ctx, behavior, viewport, floating, insertion, payload);
            }
        }
    }

    pub(super) fn apply_drop_to_floating(
        &mut self,
        ctx: &Context,
        behavior: &mut dyn Behavior<Pane>,
        target_viewport: ViewportId,
        target_floating: FloatingId,
        insertion: Option<InsertionPoint>,
        payload: DockPayload,
    ) {
        if payload.source_viewport == target_viewport
            && payload.source_floating == Some(target_floating)
        {
            return;
        }

        let Some(mut manager) = self.floating.remove(&target_viewport) else {
            // Target disappeared; fall back to the dock tree.
            self.apply_drop_to_surface(
                ctx,
                behavior,
                DockSurface::DockTree {
                    viewport: target_viewport,
                },
                insertion,
                payload,
            );
            return;
        };
        let Some(target_window) = manager.windows.get_mut(&target_floating) else {
            self.floating.insert(target_viewport, manager);
            self.apply_drop_to_surface(
                ctx,
                behavior,
                DockSurface::DockTree {
                    viewport: target_viewport,
                },
                insertion,
                payload,
            );
            return;
        };

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

        match action {
            DropAction::MoveSubtree {
                source_viewport,
                source_floating,
                tile_id,
                insertion,
            } => {
                let subtree = if source_viewport == ViewportId::ROOT && source_floating.is_none() {
                    self.tree.extract_subtree(tile_id)
                } else if source_viewport == target_viewport
                    && source_floating == Some(target_floating)
                {
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
                            ctx.send_viewport_cmd_to(source_viewport, egui::ViewportCommand::Close);
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
                    target_window.tree.insert_subtree_at(subtree, insertion);
                    manager.bring_to_front(target_floating);
                }
            }
            DropAction::MoveWholeTree {
                source_viewport,
                source_floating,
                insertion,
            } => {
                if source_viewport == ViewportId::ROOT || source_viewport == target_viewport {
                    self.floating.insert(target_viewport, manager);
                    return;
                }

                if let Some(floating_id) = source_floating {
                    if let Some(mut subtree) =
                        self.take_whole_floating_tree(source_viewport, floating_id)
                    {
                        force_subtree_visible(&mut subtree);
                        target_window.tree.insert_subtree_at(subtree, insertion);
                        manager.bring_to_front(target_floating);
                    }
                    self.floating.insert(target_viewport, manager);
                    return;
                }

                let Some(mut source) = self.detached.remove(&source_viewport) else {
                    self.floating.insert(target_viewport, manager);
                    return;
                };

                let Some(source_root) = source.tree.root.take() else {
                    self.floating.insert(target_viewport, manager);
                    return;
                };
                let source_tiles = std::mem::take(&mut source.tree.tiles);
                let subtree = egui_tiles::SubTree {
                    root: source_root,
                    tiles: source_tiles,
                };
                target_window.tree.insert_subtree_at(subtree, insertion);
                manager.bring_to_front(target_floating);

                ctx.send_viewport_cmd_to(source_viewport, egui::ViewportCommand::Close);
            }
        }

        self.floating.insert(target_viewport, manager);
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
        if let Some(tile_id) = payload.tile_id {
            if let Some(subtree) = self.take_subtree_from_source_for_cross_viewport_drop(
                ctx,
                behavior,
                payload.source_viewport,
                payload.source_floating,
                tile_id,
            ) {
                self.dock_subtree_into_root(subtree, insertion);
            }
        } else if let Some(subtree) = self.take_whole_tree_from_source_for_cross_viewport_drop(
            ctx,
            behavior,
            payload.source_viewport,
            payload.source_floating,
        ) {
            self.dock_subtree_into_root(subtree, insertion);
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
                let subtree = if source_viewport == target_viewport {
                    None
                } else {
                    self.take_subtree_from_source_for_cross_viewport_drop(
                        ctx,
                        behavior,
                        source_viewport,
                        source_floating,
                        tile_id,
                    )
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
                if let Some(mut subtree) = self.take_whole_tree_from_source_for_cross_viewport_drop(
                    ctx,
                    behavior,
                    source_viewport,
                    source_floating,
                ) {
                    force_subtree_visible(&mut subtree);
                    target.tree.insert_subtree_at(subtree, insertion);
                }
            }
        }
    }
}
