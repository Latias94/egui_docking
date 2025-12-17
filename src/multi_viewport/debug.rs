use egui::{Context, ViewportId};
use egui_tiles::{TileId, Tree};

use super::DockingMultiViewport;
use super::integrity;

use std::fs::{create_dir_all, File, OpenOptions};
use std::io::{BufWriter, Write as _};
use std::path::Path;

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
    pub(super) fn debug_log_file_prepare_if_needed(&mut self) {
        let configured_path = self.options.debug_log_file_path.as_deref();
        if self.debug_log_file_open_path.as_deref() != configured_path {
            self.debug_log_file_writer = None;
            self.debug_log_file_open_path = configured_path.map(ToOwned::to_owned);
            self.debug_log_file_inited_for_path = false;
            self.debug_log_file_last_error = None;
        }

        let Some(path) = self.debug_log_file_open_path.as_deref() else {
            return;
        };

        if self.debug_log_file_inited_for_path {
            return;
        }

        if self.options.debug_log_file_clear_on_start {
            if let Err(err) = self.truncate_debug_log_file(path) {
                self.debug_log_file_last_error =
                    Some(format!("truncate {} failed: {err}", path.display()));
            }
        }

        self.debug_log_file_inited_for_path = true;
    }

    pub(super) fn debug_log_file_truncate_now(&mut self) {
        let Some(path) = self.options.debug_log_file_path.as_deref() else {
            return;
        };

        self.debug_log_file_writer = None;
        if let Err(err) = self.truncate_debug_log_file(path) {
            self.debug_log_file_last_error = Some(format!("truncate {} failed: {err}", path.display()));
        } else {
            self.debug_log_file_last_error = None;
        }
        // After a manual clear, we consider the file "initialized" for this path.
        self.debug_log_file_open_path = Some(path.to_path_buf());
        self.debug_log_file_inited_for_path = true;
    }

    fn truncate_debug_log_file(&self, path: &Path) -> std::io::Result<()> {
        if let Some(parent) = path.parent() {
            create_dir_all(parent)?;
        }
        File::create(path).map(|_| ())
    }

    fn debug_log_file_writer(&mut self) -> Option<&mut BufWriter<File>> {
        let Some(path) = self.options.debug_log_file_path.as_deref() else {
            self.debug_log_file_writer = None;
            return None;
        };

        if self.debug_log_file_open_path.as_deref() != Some(path) {
            // Config changed without going through `debug_log_file_prepare_if_needed` yet.
            self.debug_log_file_open_path = Some(path.to_path_buf());
            self.debug_log_file_inited_for_path = false;
            self.debug_log_file_writer = None;
        }

        if self.debug_log_file_writer.is_none() {
            if let Some(parent) = path.parent() {
                if let Err(err) = create_dir_all(parent) {
                    self.debug_log_file_last_error =
                        Some(format!("create_dir_all {} failed: {err}", parent.display()));
                    return None;
                }
            }

            match OpenOptions::new().create(true).append(true).open(path) {
                Ok(file) => {
                    self.debug_log_file_writer = Some(BufWriter::new(file));
                    self.debug_log_file_last_error = None;
                }
                Err(err) => {
                    self.debug_log_file_last_error =
                        Some(format!("open {} failed: {err}", path.display()));
                    return None;
                }
            }
        }

        self.debug_log_file_writer.as_mut()
    }

    fn debug_log_file_append_line(&mut self, line: &str) {
        let flush_each_line = self.options.debug_log_file_flush_each_line;

        let Some(writer) = self.debug_log_file_writer() else {
            return;
        };

        if let Err(err) = writeln!(writer, "{line}") {
            self.debug_log_file_writer = None;
            if let Some(path) = self.options.debug_log_file_path.as_deref() {
                self.debug_log_file_last_error =
                    Some(format!("write {} failed: {err}", path.display()));
            } else {
                self.debug_log_file_last_error = Some(format!("write failed: {err}"));
            }
            return;
        }

        if flush_each_line {
            if let Err(err) = writer.flush() {
                self.debug_log_file_writer = None;
                if let Some(path) = self.options.debug_log_file_path.as_deref() {
                    self.debug_log_file_last_error =
                        Some(format!("flush {} failed: {err}", path.display()));
                } else {
                    self.debug_log_file_last_error = Some(format!("flush failed: {err}"));
                }
            }
        }
    }

    pub(super) fn debug_log_event(&mut self, message: impl Into<String>) {
        if !self.options.debug_event_log {
            return;
        }
        self.push_debug_log_line(message.into());
    }

    pub(super) fn debug_integrity_log_event(&mut self, message: impl Into<String>) {
        if !self.options.debug_integrity {
            return;
        }
        self.push_debug_log_line(message.into());
    }

    pub(super) fn push_debug_log_line(&mut self, message: String) {
        self.debug_log_file_prepare_if_needed();

        let cap = self.options.debug_event_log_capacity.max(1).min(10_000);
        while self.debug_log.len() >= cap {
            self.debug_log.pop_front();
        }
        let line = format!("[frame {}] {}", self.debug_frame, message);
        self.debug_log.push_back(line.clone());
        if self.options.debug_log_file_path.is_some() {
            self.debug_log_file_append_line(&line);
        }
    }

    pub(super) fn debug_log_clear(&mut self) {
        self.debug_log.clear();
    }

    pub(super) fn debug_log_text(&self) -> String {
        self.debug_log
            .iter()
            .cloned()
            .collect::<Vec<_>>()
            .join("\n")
    }

    pub(super) fn debug_check_integrity_all(&mut self) {
        let mut results: Vec<(ViewportId, egui::Id, Vec<String>, u64)> = Vec::new();

        {
            let tree = &self.tree;
            let issues = integrity::tree_integrity_issues(tree);
            let hash = integrity::hash_issues(&issues);
            results.push((ViewportId::ROOT, tree.id(), issues, hash));
        }

        results.extend(self.detached.iter().map(|(&viewport_id, detached)| {
            let issues = integrity::tree_integrity_issues(&detached.tree);
            let hash = integrity::hash_issues(&issues);
            (viewport_id, detached.tree.id(), issues, hash)
        }));

        for (&viewport_id, manager) in &self.floating {
            for window in manager.windows.values() {
                let issues = integrity::tree_integrity_issues(&window.tree);
                let hash = integrity::hash_issues(&issues);
                results.push((viewport_id, window.tree.id(), issues, hash));
            }
        }

        for (viewport_id, tree_id, issues, hash) in results {
            self.debug_handle_integrity_result(viewport_id, tree_id, issues, hash);
        }
    }

    pub(super) fn debug_handle_integrity_result(
        &mut self,
        viewport_id: ViewportId,
        tree_id: egui::Id,
        issues: Vec<String>,
        hash: u64,
    ) {
        let key = (tree_id.value(), viewport_id);

        let prev = self.debug_last_integrity_hash.insert(key, hash);
        if prev == Some(hash) {
            return;
        }

        if issues.is_empty() {
            if prev.unwrap_or(0) != 0 {
                self.debug_integrity_log_event(format!(
                    "integrity OK viewport={viewport_id:?} tree={:04X}",
                    tree_id.value() as u16
                ));
            }
            return;
        }

        self.debug_integrity_log_event(format!(
            "integrity FAIL viewport={viewport_id:?} tree={:04X} issues={}",
            tree_id.value() as u16,
            issues.len()
        ));
        for issue in &issues {
            self.debug_integrity_log_event(issue.clone());
        }
        // Include a short tree summary to make copy-paste debugging self contained.
        let summary = if viewport_id == ViewportId::ROOT && tree_id == self.tree.id() {
            debug_tree_summary(&self.tree, 48)
        } else if let Some(detached) = self.detached.get(&viewport_id)
            && detached.tree.id() == tree_id
        {
            debug_tree_summary(&detached.tree, 48)
        } else if let Some(manager) = self.floating.get(&viewport_id) {
            manager
                .windows
                .values()
                .find(|w| w.tree.id() == tree_id)
                .map(|w| debug_tree_summary(&w.tree, 48))
                .unwrap_or_else(|| "(tree summary unavailable)".to_owned())
        } else {
            "(tree summary unavailable)".to_owned()
        };
        self.debug_integrity_log_event(format!("integrity tree_summary:\n{summary}"));

        if self.options.debug_integrity_panic && cfg!(debug_assertions) {
            panic!(
                "egui_docking integrity failure viewport={viewport_id:?} tree={:04X}\n{}",
                tree_id.value() as u16,
                issues.join("\n")
            );
        }
    }

    pub(super) fn ui_debug_window(
        &self,
        ctx: &Context,
        viewport_id: ViewportId,
        tree_id: egui::Id,
    ) {
        // UX: while dragging, the debug window frequently steals hover/click and makes docking feel broken
        // (especially when every viewport has a debug window). During an active drag session for this bridge,
        // hide the debug window and rely on hotkeys for copy-to-clipboard.
        if egui::DragAndDrop::payload::<super::types::DockPayload>(ctx)
            .is_some_and(|p| p.bridge_id == self.tree.id())
        {
            return;
        }

        let last_drop_debug =
            ctx.data(|d| d.get_temp::<String>(last_drop_debug_text_id(tree_id, viewport_id)));
        let tiles_last_ui =
            ctx.data(|d| d.get_temp::<String>(tiles_debug_visit_last_id(tree_id, viewport_id)));
        let log_text = self.debug_log_text();

        egui::Window::new("Dock Debug")
            .id(egui::Id::new((tree_id, viewport_id, "egui_docking_debug_window")))
            .frame(egui::Frame::window(ctx.global_style().as_ref()))
            .default_pos(egui::Pos2::new(12.0, 12.0))
            .resizable(true)
            .show(ctx, |ui| {
                ui.label("Shortcuts: Cmd/Ctrl+Shift+D 复制 drop debug；Cmd/Ctrl+Shift+L 复制 event/integrity log。");
                ui.separator();

                ui.horizontal(|ui| {
                    if ui.button("Copy drop debug").clicked() {
                        if let Some(text) = &last_drop_debug {
                            ctx.copy_text(text.clone());
                        } else {
                            ctx.copy_text("(no drop debug captured yet)".to_owned());
                        }
                    }
                    if ui.button("Copy tiles ui").clicked() {
                        if let Some(text) = &tiles_last_ui {
                            ctx.copy_text(text.clone());
                        } else {
                            ctx.copy_text("(no tiles ui captured yet)".to_owned());
                        }
                    }
                    if ui.button("Copy event log").clicked() {
                        ctx.copy_text(log_text.clone());
                    }
                    if ui.button("Clear event log").clicked() {
                        ctx.data_mut(|d| {
                            d.insert_temp(debug_clear_event_log_id(self.tree.id()), true);
                        });
                    }
                });

                if let Some(path) = self.options.debug_log_file_path.as_deref() {
                    ui.separator();
                    ui.horizontal(|ui| {
                        ui.label(format!("Log file: {}", path.display()));
                        if ui.button("Copy path").clicked() {
                            ctx.copy_text(path.display().to_string());
                        }
                        if ui.button("Clear log file").clicked() {
                            ctx.data_mut(|d| {
                                d.insert_temp(debug_clear_log_file_id(self.tree.id()), true);
                            });
                        }
                    });
                    if let Some(err) = self.debug_log_file_last_error.as_deref() {
                        ui.colored_label(egui::Color32::LIGHT_RED, format!("Log file error: {err}"));
                    }
                }

                if let Some(text) = last_drop_debug {
                    ui.separator();
                    egui::ScrollArea::vertical()
                        .id_salt("drop_debug")
                        .max_height(240.0)
                        .show(ui, |ui| {
                            ui.label(text);
                        });
                }

                if let Some(text) = tiles_last_ui {
                    ui.separator();
                    egui::ScrollArea::vertical()
                        .id_salt("tiles_ui")
                        .max_height(240.0)
                        .show(ui, |ui| {
                            ui.label(text);
                        });
                }

                if self.options.debug_event_log || self.options.debug_integrity {
                    ui.separator();
                    egui::ScrollArea::vertical()
                        .id_salt("event_log")
                        .max_height(240.0)
                        .show(ui, |ui| {
                            ui.label(log_text);
                        });
                }

                // Parity warnings are not structural integrity failures, but they explain
                // "why this feels unlike ImGui" when container tabbing sneaks in.
                if self.options.debug_integrity {
                    let parity = if viewport_id == ViewportId::ROOT && tree_id == self.tree.id() {
                        integrity::tree_parity_warnings(&self.tree)
                    } else if let Some(detached) = self.detached.get(&viewport_id)
                        && detached.tree.id() == tree_id
                    {
                        integrity::tree_parity_warnings(&detached.tree)
                    } else if let Some(manager) = self.floating.get(&viewport_id) {
                        manager
                            .windows
                            .values()
                            .find(|w| w.tree.id() == tree_id)
                            .map(|w| integrity::tree_parity_warnings(&w.tree))
                            .unwrap_or_default()
                    } else {
                        Vec::new()
                    };

                    if !parity.is_empty() {
                        ui.separator();
                        ui.heading("ImGui parity warnings");
                        ui.label("These are valid in egui_tiles, but differ from ImGui DockSpace.");
                        egui::ScrollArea::vertical()
                            .id_salt("parity_warnings")
                            .max_height(160.0)
                            .show(ui, |ui| {
                                for line in parity {
                                    ui.label(line);
                                }
                            });
                    }
                }
            });
    }
}

pub(super) fn debug_clear_event_log_id(bridge_id: egui::Id) -> egui::Id {
    egui::Id::new((bridge_id, "egui_docking_clear_event_log"))
}

pub(super) fn debug_clear_log_file_id(bridge_id: egui::Id) -> egui::Id {
    egui::Id::new((bridge_id, "egui_docking_clear_log_file"))
}

pub(super) fn last_drop_debug_text_id(tree_id: egui::Id, viewport_id: ViewportId) -> egui::Id {
    egui::Id::new((tree_id, viewport_id, "egui_docking_last_drop_debug_text"))
}

pub(super) fn tiles_debug_visit_enabled_id(tree_id: egui::Id, viewport_id: ViewportId) -> egui::Id {
    egui::Id::new((tree_id, viewport_id, "egui_docking_debug_visit_enabled"))
}

pub(super) fn tiles_debug_visit_last_id(tree_id: egui::Id, viewport_id: ViewportId) -> egui::Id {
    egui::Id::new((tree_id, viewport_id, "egui_docking_debug_visit_last"))
}
