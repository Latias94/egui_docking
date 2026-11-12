//! Application inventory, bounded presets and decisions from core close plans.

use dockspace::close::{
    CloseDecision, CloseItemDecisionState, CloseRequestId, DeferredCloseDecision,
};
use dockspace::model::{DockAnchor, DockPlacement, DockspaceNodeView, DockspaceView};
use dockspace::runtime::{DockspaceCloseItem, DockspaceCloseResolution};
use egui_dockspace_native::NativeCommand;

use super::*;

#[cfg(test)]
#[path = "layout_tests.rs"]
mod tests;

#[cfg(feature = "serde")]
const MAX_PRESETS: usize = 8;
#[cfg(feature = "serde")]
const MAX_PRESET_NAME_CHARS: usize = 64;

#[cfg(feature = "serde")]
pub(super) struct NamedPreset {
    json: Vec<u8>,
    root_surface: SurfaceId,
}

impl NativeExampleApp {
    pub(super) fn controls_available(&self) -> bool {
        !self.action.is_pending()
            && self.smoke.is_none()
            && self.command_smoke.is_none()
            && self.dockspace.error_kind().is_none()
    }

    pub(super) fn show_layout_controls(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_wrapped(|ui| {
            let committed_locked = self.dockspace.arrangement_locked();
            let mut locked = committed_locked;
            if ui
                .add_enabled(
                    self.controls_available(),
                    egui::Checkbox::new(&mut locked, "Lock arrangement"),
                )
                .changed()
            {
                let request = self
                    .dockspace
                    .request_command(NativeCommand::SetArrangementLocked { locked });
                self.begin_layout_action("Change arrangement lock", request);
            }
            if ui
                .add_enabled(
                    self.controls_available() && !committed_locked,
                    egui::Button::new("Default layout"),
                )
                .clicked()
            {
                let request = self.request_default_layout();
                self.begin_layout_action("Default layout", request);
            }
            ui.menu_button("Pane inventory (8)", |ui| self.show_pane_inventory(ui));
        });
        #[cfg(feature = "serde")]
        self.show_preset_controls(ui);
        self.show_close_decisions(ui);
    }

    pub(super) fn all_root_profile(&self) -> bool {
        let root_surface = self.dockspace.root_surface();
        let items = self
            .dockspace
            .with_panes(|panes| panes.panes.keys().copied().collect::<Vec<_>>());
        self.dockspace.with_view(|view| {
            items.len() == 8
                && view.surfaces().count() == 1
                && view.surface(root_surface).is_some()
                && items.iter().all(|item| {
                    view.item(*item)
                        .is_some_and(|pane| pane.surface() == root_surface)
                })
        })
    }

    pub(super) fn all_surfaces_presented(&self) -> bool {
        let root_surface = self.dockspace.root_surface();
        let surfaces = self.dockspace.with_view(|view| {
            view.surfaces()
                .map(|surface| surface.id())
                .collect::<Vec<_>>()
        });
        surfaces.contains(&root_surface)
            && surfaces
                .into_iter()
                .all(|surface| self.dockspace.is_surface_presented(surface))
    }

    pub(super) fn request_default_layout(&self) -> Result<(), String> {
        let layout = self.default_layout.clone();
        let request = if self.dockspace.root_surface() == SURFACE {
            self.dockspace.request_layout_reset(layout)
        } else {
            self.dockspace
                .request_layout_reset_rebinding_root(layout, SURFACE)
        };
        request.map_err(|error| error.to_string())
    }

    #[cfg(feature = "serde")]
    fn show_preset_controls(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_wrapped(|ui| {
            ui.label("Preset:");
            ui.add_enabled(
                self.controls_available(),
                egui::TextEdit::singleline(&mut self.preset_name)
                    .char_limit(MAX_PRESET_NAME_CHARS)
                    .desired_width(140.0),
            );
            if ui
                .add_enabled(
                    self.controls_available()
                        && self.all_surfaces_presented()
                        && self.dockspace.is_quiescent(),
                    egui::Button::new("Save JSON preset"),
                )
                .on_disabled_hover_text(
                    "Wait until the main window and every child window have displayed their panes and finished changing layout.",
                )
                .clicked()
                && let Err(error) = self.save_named_preset()
            {
                self.status = Some(error);
            }
            if ui
                .add_enabled(
                    self.controls_available()
                        && !self.dockspace.arrangement_locked()
                        && self.presets.contains_key(self.preset_name.trim()),
                    egui::Button::new("Restore JSON preset"),
                )
                .clicked()
            {
                let request = self.request_named_preset();
                self.begin_layout_action("Restore JSON preset", request);
            }
            ui.menu_button(format!("Saved ({}/{MAX_PRESETS})", self.presets.len()), |ui| {
                for name in self.presets.keys() {
                    if ui
                        .add_enabled(self.controls_available(), egui::Button::new(name))
                        .clicked()
                    {
                        self.preset_name.clone_from(name);
                        ui.close();
                    }
                }
            });
            if ui
                .add_enabled(
                    self.controls_available()
                        && self.presets.contains_key(self.preset_name.trim()),
                    egui::Button::new("Remove preset"),
                )
                .clicked()
            {
                self.presets.remove(self.preset_name.trim());
            }
        });
    }

    #[cfg(feature = "serde")]
    pub(super) fn save_named_preset(&mut self) -> Result<(), String> {
        let name = self.preset_name.trim();
        if name.is_empty() || self.preset_name.chars().count() > MAX_PRESET_NAME_CHARS {
            return Err("Choose a preset name of 1–64 characters.".to_owned());
        }
        if !self.presets.contains_key(name) && self.presets.len() >= MAX_PRESETS {
            return Err(
                "All eight preset slots are full; overwrite or remove a saved name.".to_owned(),
            );
        }
        if self.action.is_pending()
            || !self.all_surfaces_presented()
            || !self.dockspace.is_quiescent()
        {
            return Err(
                "Wait for the main window and every child window to be ready before saving."
                    .to_owned(),
            );
        }
        let json = self
            .dockspace
            .save_document_json()
            .map_err(|error| error.to_string())?;
        self.presets.insert(
            name.to_owned(),
            NamedPreset {
                json,
                root_surface: self.dockspace.root_surface(),
            },
        );
        self.status = Some(format!(
            "Saved JSON preset '{name}' with the complete window layout; pane data stays in the application."
        ));
        Ok(())
    }

    #[cfg(feature = "serde")]
    pub(super) fn request_named_preset(&self) -> Result<(), String> {
        let preset = self
            .presets
            .get(self.preset_name.trim())
            .ok_or("Choose a saved JSON preset.")?;
        // Recognition includes closed and unloaded panes, independently of content availability.
        let keys = self.dockspace.with_panes(|panes| {
            panes
                .panes
                .values()
                .map(|pane| pane.key)
                .collect::<Vec<_>>()
        });
        let recognizes = |document: DockspaceDocumentId, key: &str| {
            document == DOCUMENT_ID && keys.contains(&key)
        };
        let request = if self.dockspace.root_surface() == preset.root_surface {
            self.dockspace
                .request_document_restore_json(&preset.json, recognizes)
        } else {
            self.dockspace.request_document_restore_json_rebinding_root(
                &preset.json,
                preset.root_surface,
                recognizes,
            )
        };
        request.map_err(|error| error.to_string())
    }

    fn show_pane_inventory(&mut self, ui: &mut egui::Ui) {
        let inventory = self.dockspace.with_panes(|panes| {
            panes
                .panes
                .iter()
                .map(|(&item, pane)| (item, pane.title, pane.loaded))
                .collect::<Vec<_>>()
        });
        for (item, title, loaded) in inventory {
            let location = self.dockspace.with_view(|view| {
                view.item(item)
                    .map(|pane| (pane.root(), pane.is_selected()))
            });
            let state = match location {
                Some((_, true)) => "selected",
                Some((_, false)) => "open",
                None => "closed",
            };
            ui.push_id(item, |ui| {
                ui.menu_button(format!("{title} — {state}"), |ui| {
                    let can_request = self.controls_available();
                    let can_arrange = can_request && !self.dockspace.arrangement_locked();
                    if let Some((root, _)) = location {
                        if ui
                            .add_enabled(can_request, egui::Button::new("Select"))
                            .clicked()
                        {
                            let request = self
                                .dockspace
                                .request_command(NativeCommand::SelectItem { item });
                            self.begin_layout_action("Select pane", request);
                            ui.close();
                        }
                        ui.separator();
                        ui.add_enabled_ui(
                            self.controls_available() && !self.dockspace.arrangement_locked(),
                            |ui| self.show_pane_move_commands(ui, item),
                        );
                        ui.separator();
                        if ui
                            .add_enabled(
                                can_arrange && !self.action.is_pending(),
                                egui::Button::new("Close"),
                            )
                            .clicked()
                        {
                            let request = self
                                .dockspace
                                .request_command(NativeCommand::CloseItem { item });
                            self.begin_layout_action("Request pane close", request);
                            ui.close();
                        }
                        if ui
                            .add_enabled(
                                can_arrange && !self.action.is_pending(),
                                egui::Button::new("Close group"),
                            )
                            .on_hover_text("Request an atomic close of every pane in this root.")
                            .clicked()
                        {
                            let request = self
                                .dockspace
                                .request_command(NativeCommand::CloseRoot { root });
                            self.begin_layout_action("Request group close", request);
                            ui.close();
                        }
                    } else if ui
                        .add_enabled(can_arrange, egui::Button::new("Reopen"))
                        .clicked()
                    {
                        self.reopen_pane(item);
                        ui.close();
                    }
                    ui.separator();
                    if ui
                        .add_enabled(
                            self.controls_available(),
                            egui::Button::new(if loaded {
                                "Unload content"
                            } else {
                                "Reload content"
                            }),
                        )
                        .clicked()
                    {
                        self.dockspace.with_panes_mut(|panes| {
                            panes
                                .panes
                                .get_mut(&item)
                                .expect("inventory pane")
                                .set_loaded(!loaded);
                        });
                        self.status = Some(format!(
                            "{title}: content {}; key, title and data retained.",
                            if loaded { "unloaded" } else { "reloaded" },
                        ));
                        ui.close();
                    }
                });
            });
        }
        ui.small("Review close confirmations below the toolbar; pane content is retained.");
    }

    fn show_pane_move_commands(&mut self, ui: &mut egui::Ui, item: ItemId) {
        let host = self.dockspace.root_surface();
        let Some((contained, can_float, placement)) = self.dockspace.with_view(|view| {
            let pane = view.item(item)?;
            let contained = pane.contained().is_some();
            let standalone = view
                .root(pane.root())?
                .content()?
                .tabs()
                .is_some_and(|tabs| tabs.items() == [item]);
            Some((
                contained,
                !contained || pane.surface() != host || !standalone,
                main_dock_placement(view, host, Some(item)),
            ))
        }) else {
            return;
        };
        for (label, command) in [
            (
                "Dock in main area",
                placement.map(|placement| NativeCommand::DockItem { item, placement }),
            ),
            (
                "Float in main window",
                can_float.then_some(NativeCommand::FloatItem {
                    item,
                    surface: host,
                }),
            ),
            (
                "Bring group to front",
                contained.then_some(NativeCommand::RaiseContained { item }),
            ),
            (
                "Move group into view",
                contained.then_some(NativeCommand::BringContainedIntoView { item }),
            ),
        ] {
            if ui
                .add_enabled(command.is_some(), egui::Button::new(label))
                .clicked()
                && let Some(command) = command
            {
                let request = self.dockspace.request_command(command);
                self.begin_layout_action(label, request);
                ui.close();
            }
        }
    }

    fn reopen_pane(&mut self, item: ItemId) {
        let host = self.dockspace.root_surface();
        let placement = self.dockspace.with_view(|view| {
            view.item(EDITOR)
                .map(|_| DockPlacement::Center(DockAnchor::Item(EDITOR)))
                .or_else(|| main_dock_placement(view, host, None))
        });
        let Some(placement) = placement else {
            self.status =
                Some("Could not reopen pane: no current Document or main-root anchor.".to_owned());
            return;
        };
        let request = self
            .dockspace
            .request_command(NativeCommand::OpenItem { item, placement });
        self.begin_layout_action("Reopen pane", request);
    }

    fn show_close_decisions(&mut self, ui: &mut egui::Ui) {
        // This projection is used only for this UI pass. Core owns plans and tokens.
        let plans = self.dockspace.pending_content_close_plans();
        if plans.is_empty() {
            return;
        }
        egui::CollapsingHeader::new(format!("Close confirmations ({})", plans.len()))
            .id_salt("native-pending-close-decisions")
            .default_open(true)
            .show(ui, |ui| {
                ui.small("Pane content is retained. Reopen closed panes from Pane inventory.");
                for plan in plans {
                    ui.push_id(plan.request(), |ui| {
                        ui.group(|ui| {
                            if let [item] = plan.items() {
                                let title = self.dockspace.with_panes(|panes| {
                                    panes.panes.get(&item.item()).map_or("Pane", |pane| pane.title)
                                });
                                ui.strong(format!("Close {title}?"));
                            } else {
                                ui.strong(format!("Close {} panes together?", plan.items().len()));
                                ui.small("All panes close together. Keep open cancels the complete request.");
                            }
                            for item in plan.items() {
                                self.show_close_decision(ui, plan.request(), *item);
                            }
                        });
                    });
                }
            });
    }

    fn show_close_decision(
        &mut self,
        ui: &mut egui::Ui,
        request: CloseRequestId,
        item: DockspaceCloseItem,
    ) {
        let title = self.dockspace.with_panes(|panes| {
            panes
                .panes
                .get(&item.item())
                .map_or("Unknown pane", |pane| pane.title)
        });
        ui.push_id(item.item(), |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.label(title);
                let commands = match item.state() {
                    CloseItemDecisionState::Pending => [
                        Some((
                            "Close",
                            NativeCommand::DecideClose {
                                request,
                                token: item.token(),
                                decision: CloseDecision::Allow,
                            },
                        )),
                        Some((
                            "Keep open",
                            NativeCommand::DecideClose {
                                request,
                                token: item.token(),
                                decision: CloseDecision::Veto,
                            },
                        )),
                        item.capability().allows_deferred().then_some((
                            "Decide later",
                            NativeCommand::DecideClose {
                                request,
                                token: item.token(),
                                decision: CloseDecision::Deferred,
                            },
                        )),
                    ],
                    CloseItemDecisionState::Deferred { continuation } => {
                        ui.weak("Decision postponed.");
                        [
                            Some((
                                "Close",
                                NativeCommand::ContinueClose {
                                    request,
                                    token: continuation,
                                    decision: DeferredCloseDecision::Allow,
                                },
                            )),
                            Some((
                                "Keep open",
                                NativeCommand::ContinueClose {
                                    request,
                                    token: continuation,
                                    decision: DeferredCloseDecision::Veto,
                                },
                            )),
                            None,
                        ]
                    }
                    CloseItemDecisionState::Allowed => {
                        ui.weak("Ready to close; waiting for the other panes.");
                        return;
                    }
                    CloseItemDecisionState::Vetoed => {
                        ui.weak("Kept open; the close request is cancelled.");
                        return;
                    }
                };
                for (label, command) in commands.into_iter().flatten() {
                    let response =
                        ui.add_enabled(self.controls_available(), egui::Button::new(label));
                    ui.ctx().accesskit_node_builder(response.id, |node| {
                        node.set_label(format!("{title}: {label}"));
                    });
                    if response.clicked() {
                        let result = self.dockspace.request_command(command);
                        self.begin_layout_action("Close decision", result);
                    }
                }
            });
        });
    }

    pub(super) fn begin_layout_action<E: std::fmt::Display>(
        &mut self,
        operation: &'static str,
        request: Result<(), E>,
    ) {
        match request {
            Ok(()) => {
                self.action = ExampleActionState::AwaitingLayoutOutcome(operation);
                self.status = Some(format!("{operation}: request accepted."));
            }
            Err(error) => self.status = Some(format!("{operation}: {error}")),
        }
    }
}

fn main_dock_placement(
    view: DockspaceView<'_>,
    host: SurfaceId,
    exclude: Option<ItemId>,
) -> Option<DockPlacement> {
    let Some(root) = view.surface(host)?.main_root() else {
        return Some(DockPlacement::Main(host));
    };
    if root.central().is_some() {
        return Some(DockPlacement::Center(DockAnchor::Central(root.id())));
    }
    first_dock_anchor(root.content()?, exclude)
        .map(|item| DockPlacement::Center(DockAnchor::Item(item)))
}

fn first_dock_anchor(node: DockspaceNodeView<'_>, exclude: Option<ItemId>) -> Option<ItemId> {
    if let Some(tabs) = node.tabs() {
        return tabs
            .items()
            .iter()
            .copied()
            .find(|item| Some(*item) != exclude);
    }
    node.split()?
        .children()
        .find_map(|child| first_dock_anchor(child, exclude))
}

pub(super) fn describe_status(status: &NativeActionStatus, panes: &ExamplePanes) -> String {
    let title = |item| panes.panes.get(&item).map_or("Pane", |pane| pane.title);
    match status {
        NativeActionStatus::RootContinuationRejected { kind, .. } => {
            format!(
                "The main-window layout could not be replaced: {kind:?}. The current layout remains available."
            )
        }
        NativeActionStatus::Dock(DockspaceActionStatus::Applied(outcome)) => match outcome {
            DockspaceActionOutcome::Selected { item, .. } => {
                format!("Selected {}.", title(*item))
            }
            DockspaceActionOutcome::Opened { item, .. } => {
                format!("Reopened {} with its retained content.", title(*item))
            }
            DockspaceActionOutcome::Existing { item } => {
                format!("{} is already open.", title(*item))
            }
            DockspaceActionOutcome::Docked { item, changed, .. } => {
                if *changed {
                    format!("Docked {}; its content is retained.", title(*item))
                } else {
                    format!("{} is already at that docking position.", title(*item))
                }
            }
            DockspaceActionOutcome::Floated { item, changed, .. } => {
                if *changed {
                    format!("{} is floating; its content is retained.", title(*item))
                } else {
                    format!("{} is already floating at that position.", title(*item))
                }
            }
            DockspaceActionOutcome::ContainedRaised { changed, .. } => {
                if *changed {
                    "Brought the floating group to the front.".to_owned()
                } else {
                    "The floating group is already in front.".to_owned()
                }
            }
            DockspaceActionOutcome::ContainedBoundsUpdated { changed, .. } => {
                if *changed {
                    "Moved the floating group into view.".to_owned()
                } else {
                    "The floating group is already in view.".to_owned()
                }
            }
            DockspaceActionOutcome::RootDocked { changed, .. } => {
                if *changed {
                    "Docked the group; pane content is retained.".to_owned()
                } else {
                    "The group is already at that docking position.".to_owned()
                }
            }
            DockspaceActionOutcome::RootFloated { changed, .. } => {
                if *changed {
                    "The group is floating; pane content is retained.".to_owned()
                } else {
                    "The group is already floating at that position.".to_owned()
                }
            }
            DockspaceActionOutcome::RootFloatRequested { .. }
            | DockspaceActionOutcome::RootDockRequested { .. }
            | DockspaceActionOutcome::NativeRootTearOffRequested { .. } => {
                "Moving the group; waiting for the destination window to display it.".to_owned()
            }
        },
        NativeActionStatus::Dock(DockspaceActionStatus::Rejected(reason)) => {
            format!("The action was rejected: {reason}")
        }
        NativeActionStatus::Dock(DockspaceActionStatus::Stale { .. })
        | NativeActionStatus::Stale { .. } => {
            "The layout changed before the request committed; try again using the current controls."
                .to_owned()
        }
        NativeActionStatus::Dock(DockspaceActionStatus::PresentationFailed { reason, .. }) => {
            format!("The target presentation did not complete: {reason:?}")
        }
        NativeActionStatus::CloseRequested { .. } => {
            "Review the close confirmation below. Pane content is retained.".to_owned()
        }
        NativeActionStatus::CloseRejected(reason) => {
            format!("The close request was rejected: {reason}")
        }
        NativeActionStatus::CloseDecisionProcessed {
            resolution,
            application,
            changed,
            ..
        } => {
            if let Some(application) = application {
                return match application {
                    Ok(_) if *changed => "Closed the requested panes. Reopen them from Pane inventory; their content is retained.".to_owned(),
                    Ok(_) => "The close request completed without changing the layout.".to_owned(),
                    Err(reason) => format!("The approved close could not apply: {reason}"),
                };
            }
            match resolution {
                DockspaceCloseResolution::Recorded { .. } => {
                    "Close confirmed for this pane; waiting for the other panes.".to_owned()
                }
                DockspaceCloseResolution::Deferred { .. } => {
                    "Decision postponed. Use Close or Keep open when ready.".to_owned()
                }
                DockspaceCloseResolution::Approved { .. } => {
                    "Close confirmed; waiting for the panes to finish closing.".to_owned()
                }
                DockspaceCloseResolution::Vetoed { .. } => {
                    "Kept open; the complete close request was cancelled.".to_owned()
                }
                DockspaceCloseResolution::Inert(_) => {
                    "This close decision no longer applies. Use the current confirmation."
                        .to_owned()
                }
            }
        }
        NativeActionStatus::PolicyChanged { changed, .. } => {
            format!("Docking policy committed; changed={changed}.")
        }
        NativeActionStatus::ArrangementLockChanged {
            locked, changed, ..
        } => format!(
            "Arrangement {} (changed: {changed}); pane selection and content editing remain available.",
            if *locked { "locked" } else { "unlocked" }
        ),
        NativeActionStatus::StyleChanged {
            changed,
            geometry_changed,
        } => {
            format!("Dock style committed; changed={changed}, geometry changed={geometry_changed}.")
        }
        NativeActionStatus::InputChanged { changed } => {
            format!("Docking input bindings applied; changed={changed}.")
        }
        NativeActionStatus::LayoutReset { previous, current } => format!(
            "Default layout applied ({previous:?} → {current:?}); waiting for the windows to display their panes and be ready for editing."
        ),
        #[cfg(feature = "serde")]
        NativeActionStatus::DocumentRestored { previous, current } => format!(
            "Saved layout applied ({previous:?} → {current:?}); waiting for the main window and each replacement child window to display their panes and be ready for editing."
        ),
    }
}
