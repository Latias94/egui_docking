//! Application inventory, bounded presets and decisions from core close plans.

use dockspace::close::{CloseDecision, CloseItemDecisionState, DeferredCloseDecision};
use dockspace::model::{DockAnchor, DockPlacement};
use dockspace::runtime::DockspaceCloseResolution;

use super::*;

#[cfg(feature = "serde")]
const MAX_PRESETS: usize = 8;
#[cfg(feature = "serde")]
const MAX_PRESET_NAME_CHARS: usize = 64;

impl NativeExampleApp {
    pub(super) fn controls_available(&self) -> bool {
        !self.action.is_pending() && self.smoke.is_none() && self.dockspace.error_kind().is_none()
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
                let request = self.dockspace.request_set_arrangement_locked(locked);
                self.begin_layout_action("Change arrangement lock", request);
            }
            if ui
                .add_enabled(
                    self.controls_available() && !committed_locked,
                    egui::Button::new("Default layout"),
                )
                .clicked()
            {
                let request = self
                    .dockspace
                    .request_layout_reset(self.default_layout.clone());
                self.begin_layout_action("Default layout", request);
            }
            ui.menu_button("Pane inventory (8)", |ui| self.show_pane_inventory(ui));
            ui.small(if committed_locked {
                "Arrangement locked; pane selection and content editing remain available."
            } else {
                "Pane data stays in the application registry when closed."
            });
        });
        #[cfg(feature = "serde")]
        self.show_preset_controls(ui);
        self.show_close_decisions(ui);
    }

    pub(super) fn all_root_profile(&self) -> bool {
        let items = self
            .dockspace
            .with_panes(|panes| panes.panes.keys().copied().collect::<Vec<_>>());
        self.dockspace.with_view(|view| {
            items.len() == 8
                && view.surfaces().count() == 1
                && view.surface(SURFACE).is_some()
                && items.iter().all(|item| {
                    view.item(*item)
                        .is_some_and(|pane| pane.surface() == SURFACE)
                })
        }) == Some(true)
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
                        && self.all_root_profile()
                        && self.dockspace.is_surface_presented(SURFACE)
                        && self.dockspace.is_quiescent(),
                    egui::Button::new("Save JSON preset"),
                )
                .on_disabled_hover_text(
                    "Bring all eight panes back to the root host and wait for presentation and native cleanup.",
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
        ui.small(
            "Session-only JSON · up to 8 names, 64 characters each; saving replaces that name's slot. \
             Profile: all eight panes in the root host, including contained windows. New logical surfaces do not create OS windows.",
        );
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
            || !self.all_root_profile()
            || !self.dockspace.is_surface_presented(SURFACE)
            || !self.dockspace.is_quiescent()
        {
            return Err(
                "Saving requires all eight panes in the presented, quiescent root host.".to_owned(),
            );
        }
        let json = self
            .dockspace
            .save_document_json()
            .map_err(|error| error.to_string())?;
        self.presets.insert(name.to_owned(), json);
        self.status = Some(format!(
            "Saved JSON preset '{name}' (all-root profile; pane data stays in the application)."
        ));
        Ok(())
    }

    #[cfg(feature = "serde")]
    pub(super) fn request_named_preset(&self) -> Result<(), String> {
        let json = self
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
        self.dockspace
            .request_document_restore_json(json, |document, key| {
                document == DOCUMENT_ID && keys.contains(&key)
            })
            .map_err(|error| error.to_string())
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
            let Some(location) = self.dockspace.with_view(|view| {
                view.item(item)
                    .map(|pane| (pane.root(), pane.is_selected()))
            }) else {
                ui.weak("Waiting for the current layout…");
                return;
            };
            ui.push_id(item, |ui| {
                ui.horizontal(|ui| {
                    let can_request = self.controls_available();
                    let can_arrange = can_request && !self.dockspace.arrangement_locked();
                    if let Some((root, selected)) = location {
                        ui.label(format!(
                            "{title} — {}",
                            if selected { "selected" } else { "open" }
                        ));
                        if ui
                            .add_enabled(can_request, egui::Button::new("Select"))
                            .clicked()
                        {
                            let request = self.dockspace.request_select_item(item);
                            self.begin_layout_action("Select pane", request);
                            ui.close();
                        }
                        if ui
                            .add_enabled(
                                can_arrange && !self.action.is_pending(),
                                egui::Button::new("Close"),
                            )
                            .clicked()
                        {
                            let request = self.dockspace.request_close_item(item);
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
                            let request = self.dockspace.request_close_root(root);
                            self.begin_layout_action("Request group close", request);
                            ui.close();
                        }
                    } else {
                        ui.label(format!("{title} — closed"));
                        if ui
                            .add_enabled(can_arrange, egui::Button::new("Reopen"))
                            .clicked()
                        {
                            self.reopen_pane(item);
                            ui.close();
                        }
                    }
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
        ui.small("Close requests need an explicit decision below the toolbar.");
    }

    fn reopen_pane(&mut self, item: ItemId) {
        let inventory = self
            .dockspace
            .with_panes(|panes| panes.panes.keys().copied().collect::<Vec<_>>());
        let placement = self
            .dockspace
            .with_view(|view| {
                if view.item(EDITOR).is_some() {
                    return Some(DockPlacement::Center(DockAnchor::Item(EDITOR)));
                }
                let surface = view.surface(SURFACE)?;
                Some(match surface.main_root() {
                    Some(root) if root.central().is_some() => {
                        DockPlacement::Center(DockAnchor::Central(root.id()))
                    }
                    Some(root) => {
                        let anchor = inventory.into_iter().find(|candidate| {
                            view.item(*candidate)
                                .is_some_and(|pane| pane.root() == root.id())
                        })?;
                        DockPlacement::Center(DockAnchor::Item(anchor))
                    }
                    None => DockPlacement::Main(surface.id()),
                })
            })
            .flatten();
        let Some(placement) = placement else {
            self.status =
                Some("Could not reopen pane: no current Document or main-root anchor.".to_owned());
            return;
        };
        let request = self.dockspace.request_open_item(item, placement);
        self.begin_layout_action("Reopen pane", request);
    }

    fn show_close_decisions(&mut self, ui: &mut egui::Ui) {
        // This projection is used only for this UI pass. Core owns plans and tokens.
        let plans = self.dockspace.pending_content_close_plans();
        if plans.is_empty() {
            return;
        }
        egui::CollapsingHeader::new(format!("Pending close decisions ({})", plans.len()))
            .id_salt("native-pending-close-decisions")
            .default_open(true)
            .show(ui, |ui| {
                ui.small("Allow every pane to close the target atomically; any Veto cancels it.");
                for plan in plans {
                    ui.label(format!(
                        "Close request {} — {:?}",
                        plan.request(),
                        plan.target()
                    ));
                    for item in plan.items() {
                        let title = self.dockspace.with_panes(|panes| {
                            panes
                                .panes
                                .get(&item.item())
                                .map_or("Unknown pane", |pane| pane.title)
                        });
                        ui.push_id((plan.request(), item.item()), |ui| {
                            ui.horizontal(|ui| {
                                ui.label(title);
                                match item.state() {
                                    CloseItemDecisionState::Pending => {
                                        for (label, decision) in [
                                            ("Allow", CloseDecision::Allow),
                                            ("Veto", CloseDecision::Veto),
                                            ("Defer", CloseDecision::Deferred),
                                        ] {
                                            let enabled = self.controls_available()
                                                && (decision != CloseDecision::Deferred
                                                    || item.capability().allows_deferred());
                                            if ui
                                                .add_enabled(enabled, egui::Button::new(label))
                                                .clicked()
                                            {
                                                let request =
                                                    self.dockspace.request_close_decision(
                                                        plan.request(),
                                                        item.token(),
                                                        decision,
                                                    );
                                                self.begin_layout_action(
                                                    "Resolve close decision",
                                                    request,
                                                );
                                            }
                                        }
                                    }
                                    CloseItemDecisionState::Deferred { continuation } => {
                                        ui.weak("Deferred — finish with the core continuation:");
                                        for (label, decision) in [
                                            ("Allow deferred", DeferredCloseDecision::Allow),
                                            ("Veto deferred", DeferredCloseDecision::Veto),
                                        ] {
                                            if ui
                                                .add_enabled(
                                                    self.controls_available(),
                                                    egui::Button::new(label),
                                                )
                                                .clicked()
                                            {
                                                let request =
                                                    self.dockspace.request_deferred_close_decision(
                                                        plan.request(),
                                                        continuation,
                                                        decision,
                                                    );
                                                self.begin_layout_action(
                                                    "Continue deferred close",
                                                    request,
                                                );
                                            }
                                        }
                                    }
                                    CloseItemDecisionState::Allowed => {
                                        ui.weak("Allowed; waiting for the other panes.");
                                    }
                                    CloseItemDecisionState::Vetoed => {
                                        ui.weak("Vetoed; the target stays open.");
                                    }
                                }
                            });
                        });
                    }
                }
            });
    }

    fn begin_layout_action<E: std::fmt::Display>(
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

pub(super) fn describe_status(status: &NativeActionStatus) -> String {
    match status {
        NativeActionStatus::Dock(DockspaceActionStatus::Applied(outcome)) => match outcome {
            DockspaceActionOutcome::Selected { item, changed } => {
                format!("Selected pane {} (changed: {changed}).", item.get())
            }
            DockspaceActionOutcome::Opened { item, .. } => format!(
                "Reopened pane {} with its retained application data.",
                item.get()
            ),
            DockspaceActionOutcome::Existing { item } => {
                format!("Pane {} is already open.", item.get())
            }
            outcome => format!("Presentation action completed: {outcome:?}"),
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
        NativeActionStatus::CloseRequested { reused, .. } => format!(
            "{} close request; choose Allow, Veto or Defer in Pending close decisions.",
            if *reused {
                "Reused the pending"
            } else {
                "Opened a new"
            }
        ),
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
                    Ok(outcome) => format!(
                        "Close applied (changed: {changed}): {outcome:?}. Pane data remains in the inventory."
                    ),
                    Err(reason) => format!("The approved close could not apply: {reason}"),
                };
            }
            match resolution {
                DockspaceCloseResolution::Recorded { .. } => {
                    "Allow recorded; the other panes still need a decision.".to_owned()
                }
                DockspaceCloseResolution::Deferred { .. } => {
                    "Close deferred; use Allow deferred or Veto deferred to continue.".to_owned()
                }
                DockspaceCloseResolution::Approved { .. } => {
                    "Close approved; waiting for the atomic close result.".to_owned()
                }
                DockspaceCloseResolution::Vetoed { .. } => {
                    "Close vetoed; the complete target stays open.".to_owned()
                }
                DockspaceCloseResolution::Inert(reason) => format!(
                    "Close decision had no effect: {reason:?}. Use the current pending decisions."
                ),
            }
        }
        NativeActionStatus::ArrangementLockChanged {
            locked, changed, ..
        } => format!(
            "Arrangement {} (changed: {changed}); pane selection and content editing remain available.",
            if *locked { "locked" } else { "unlocked" }
        ),
        NativeActionStatus::LayoutReset { previous, current } => format!(
            "Default layout logically committed ({previous:?} → {current:?}); waiting for fresh presented output and native quiescence."
        ),
        #[cfg(feature = "serde")]
        NativeActionStatus::DocumentRestored { previous, current } => format!(
            "Document logically restored ({previous:?} → {current:?}); waiting for fresh presented output and native quiescence."
        ),
    }
}
