//! Application commands share the existing bounded native action slot.

use dockspace::close::{
    CloseDecision, CloseItemDecisionState, ClosePlanTarget, DeferredCloseDecision,
};
use dockspace::model::{
    DockAnchor, DockPlacement, DockspaceActionRejection, DockspaceLayout, DockspaceRootView,
    NativeTearOffSource, PreparedTabsGroup, RootId, SurfaceId,
};
use dockspace::runtime::{DockspaceClosePlan, NativeSurfaceBinding};
use eframe::egui;
use egui_dockspace::{DockInputConfig, DockStyle, PaneView};

use crate::application_action::{
    NativeActionRequestError, NativeActionTerminal, NativeApplicationAction,
    NativeApplicationCommand,
};

use super::NativeRuntimeState;
use crate::NativeLayoutRequestError;

#[cfg(test)]
mod tests;

impl<P: PaneView> NativeRuntimeState<P> {
    pub(crate) fn request_input(
        &mut self,
        input: DockInputConfig,
    ) -> Result<(), NativeActionRequestError> {
        self.ensure_application_slot_available()?;
        self.enqueue_application_action(NativeApplicationAction::new(
            self.coordinator.session().version(),
            NativeApplicationCommand::Input {
                input,
                changed: self.coordinator.input() != input,
            },
        ))
    }

    pub(crate) fn input(&self) -> DockInputConfig {
        self.coordinator.input()
    }

    pub(crate) fn set_initial_input(
        &mut self,
        input: DockInputConfig,
    ) -> Result<(), NativeActionRequestError> {
        self.ensure_application_slot_available()?;
        if self.root_context.is_some()
            || self.owned_update.is_some()
            || self.coordinator.has_pending_callback_records()
            || self.coordinator.session().has_active_gesture()
        {
            return Err(NativeActionRequestError::Busy);
        }
        self.coordinator.set_input(input);
        Ok(())
    }

    pub(crate) fn request_style(
        &mut self,
        style: DockStyle,
    ) -> Result<(), crate::NativeStyleRequestError> {
        self.ensure_application_slot_available()?;
        style.validate()?;
        let presentation =
            style.presentation_config_with(self.coordinator.session().presentation_config())?;
        let changed = self.style != style;
        let action = NativeApplicationAction::new(
            self.coordinator.session().version(),
            NativeApplicationCommand::Style {
                style: Box::new(style),
                presentation: Box::new(presentation),
                changed,
            },
        );
        self.enqueue_application_action(action)?;
        Ok(())
    }

    pub(crate) fn request_layout_reset_rebinding_root(
        &mut self,
        layout: DockspaceLayout,
        incoming_root: SurfaceId,
    ) -> Result<(), NativeLayoutRequestError> {
        self.ensure_application_slot_available()?;
        self.validate_replacement_host_at(layout.view(), incoming_root)?;
        let existing_root = self.existing_root_for_continuation()?;
        self.validate_root_continuation_target(existing_root, incoming_root)?;
        let reset = self.coordinator.session().prepare_layout_reset(layout)?;
        let action = NativeApplicationAction::new(
            self.coordinator.session().version(),
            NativeApplicationCommand::LayoutResetRebindingRoot {
                reset: Box::new(reset),
                existing_root,
                incoming_root,
            },
        );
        self.enqueue_application_action(action)?;
        Ok(())
    }

    fn existing_root_for_continuation(
        &self,
    ) -> Result<NativeSurfaceBinding, NativeLayoutRequestError> {
        let route = self
            .coordinator
            .viewport_binding(egui::ViewportId::ROOT)
            .filter(|binding| {
                self.coordinator.dispatch_bridge().with_viewports(|routes| {
                    routes.window(egui::ViewportId::ROOT).is_some()
                        && routes.has_window_receipt(egui::ViewportId::ROOT, *binding)
                })
            })
            .ok_or_else(|| {
                NativeLayoutRequestError::new_root_window_unavailable(self.root_surface)
            })?;
        self.coordinator
            .session()
            .native_surface_bindings_for_window(route.window())
            .find(|binding| binding.surface() == self.root_surface)
            .ok_or_else(|| NativeLayoutRequestError::new_root_window_unavailable(self.root_surface))
    }

    fn current_root_binding(&self) -> Option<NativeSurfaceBinding> {
        let session = self.coordinator.session();
        session.native_window_bindings().find_map(|window| {
            session
                .native_surface_bindings_for_window(window)
                .find(|binding| binding.surface() == self.root_surface)
        })
    }

    fn validate_root_continuation_target(
        &self,
        existing: NativeSurfaceBinding,
        incoming: SurfaceId,
    ) -> Result<(), NativeLayoutRequestError> {
        if incoming != existing.surface()
            && self
                .coordinator
                .session()
                .view()
                .surface(incoming)
                .is_some()
            && self.coordinator.surface_viewport(incoming).is_some()
        {
            return Err(NativeLayoutRequestError::new_root_target_occupied(incoming));
        }
        Ok(())
    }

    #[cfg(feature = "serde")]
    pub(crate) fn request_document_restore_json_rebinding_root(
        &mut self,
        bytes: &[u8],
        incoming_root: SurfaceId,
        recognize: impl Fn(dockspace::runtime::DockspaceDocumentId, &str) -> bool,
    ) -> Result<(), NativeLayoutRequestError> {
        self.ensure_application_slot_available()?;
        if self.arrangement_locked() {
            return Err(NativeLayoutRequestError::new_arrangement_locked());
        }
        let existing_root = self.existing_root_for_continuation()?;
        self.validate_root_continuation_target(existing_root, incoming_root)?;
        let restore = self
            .coordinator
            .session()
            .prepare_document_restore_json(bytes, recognize)?;
        let root_mappings =
            self.prepare_document_root_mappings(&restore, Some(existing_root), incoming_root)?;
        let action = NativeApplicationAction::new(
            self.coordinator.session().version(),
            NativeApplicationCommand::DocumentRestore {
                restore: Box::new(restore),
                root_surface: incoming_root,
                root_mappings,
            },
        );
        self.enqueue_application_action(action)?;
        Ok(())
    }

    pub(crate) fn request_layout_reset(
        &mut self,
        layout: DockspaceLayout,
    ) -> Result<(), NativeLayoutRequestError> {
        self.ensure_application_slot_available()?;
        self.validate_replacement_host(layout.view())?;
        let reset = self.coordinator.session().prepare_layout_reset(layout)?;
        let action = NativeApplicationAction::new(
            self.coordinator.session().version(),
            NativeApplicationCommand::LayoutReset(Box::new(reset)),
        );
        self.enqueue_application_action(action)?;
        Ok(())
    }

    #[cfg(feature = "serde")]
    pub(crate) fn request_document_restore_json(
        &mut self,
        bytes: &[u8],
        recognize: impl Fn(dockspace::runtime::DockspaceDocumentId, &str) -> bool,
    ) -> Result<(), NativeLayoutRequestError> {
        self.ensure_application_slot_available()?;
        if self.arrangement_locked() {
            return Err(NativeLayoutRequestError::new_arrangement_locked());
        }
        let restore = self
            .coordinator
            .session()
            .prepare_document_restore_json(bytes, recognize)?;
        let root_mappings = self.prepare_document_root_mappings(
            &restore,
            self.current_root_binding(),
            self.root_surface,
        )?;
        let action = NativeApplicationAction::new(
            self.coordinator.session().version(),
            NativeApplicationCommand::DocumentRestore {
                restore: Box::new(restore),
                root_surface: self.root_surface,
                root_mappings,
            },
        );
        self.enqueue_application_action(action)?;
        Ok(())
    }

    #[cfg(feature = "serde")]
    fn prepare_document_root_mappings(
        &self,
        restore: &dockspace::runtime::PreparedDockspaceDocumentRestore,
        existing_root: Option<NativeSurfaceBinding>,
        incoming_root: SurfaceId,
    ) -> Result<Vec<(NativeSurfaceBinding, SurfaceId)>, NativeLayoutRequestError> {
        let mut mappings = Vec::new();
        if let Some(existing_root) = existing_root {
            // Keep the representative first for an exact command rejection.
            // Siblings come only from the core's current window roster.
            mappings.push((existing_root, incoming_root));
            mappings.extend(
                self.coordinator
                    .session()
                    .native_surface_bindings_for_window(existing_root.window())
                    .filter(|binding| *binding != existing_root)
                    .filter(|binding| restore.view().surface(binding.surface()).is_some())
                    .map(|binding| (binding, binding.surface())),
            );
        }
        restore.validate_native_surface_hosting_for_roots(
            std::iter::once(incoming_root)
                .chain(mappings.iter().skip(1).map(|(_, target)| *target)),
        )?;
        Ok(mappings)
    }

    #[cfg(feature = "serde")]
    pub(crate) fn save_document_json(&mut self) -> Result<Vec<u8>, NativeLayoutRequestError> {
        self.ensure_application_slot_available()?;
        self.coordinator.save_document_json().map_err(Into::into)
    }

    fn ensure_application_slot_available(&self) -> Result<(), NativeActionRequestError> {
        if self.shutdown.is_some() {
            return Err(NativeActionRequestError::stopped());
        }
        if self.application_actions.is_occupied() {
            return Err(NativeActionRequestError::busy());
        }
        Ok(())
    }

    fn validate_replacement_host(
        &self,
        view: dockspace::model::DockspaceView<'_>,
    ) -> Result<(), NativeLayoutRequestError> {
        self.validate_replacement_host_at(view, self.root_surface)
    }

    fn validate_replacement_host_at(
        &self,
        view: dockspace::model::DockspaceView<'_>,
        root_surface: SurfaceId,
    ) -> Result<(), NativeLayoutRequestError> {
        if view.surface(root_surface).is_none() {
            return Err(NativeLayoutRequestError::new_host_surface_missing(
                root_surface,
            ));
        }
        let window = self
            .current_root_binding()
            .map(NativeSurfaceBinding::window);
        if let Some(unhosted) = view.surfaces().find(|surface| {
            let surface = surface.id();
            surface != root_surface
                && (surface == self.root_surface
                    || !window.is_some_and(|window| {
                        self.coordinator
                            .session()
                            .native_surface_bindings_for_window(window)
                            .any(|binding| binding.surface() == surface)
                    }))
        }) {
            return Err(NativeLayoutRequestError::new_unhosted_surface(
                unhosted.id(),
            ));
        }
        Ok(())
    }

    pub(crate) fn capture_tabs_group(
        &self,
        anchor: DockAnchor,
    ) -> Result<PreparedTabsGroup, DockspaceActionRejection> {
        self.coordinator.session().capture_tabs_group(anchor)
    }

    pub(crate) fn request_command(
        &mut self,
        command: crate::NativeCommand,
    ) -> Result<(), NativeActionRequestError> {
        use crate::NativeCommand;

        self.ensure_application_slot_available()?;
        if self.command_removes_host_surface(&command) {
            return Err(NativeActionRequestError::HostSurfaceRequired {
                surface: self.root_surface,
            });
        }
        let session = self.coordinator.session();
        let action: NativeApplicationAction = match command {
            NativeCommand::SelectItem { item } => session.prepare_select_item(item).into(),
            NativeCommand::OpenItem { item, placement } => {
                session.prepare_open_item(item, placement).into()
            }
            NativeCommand::DockItem { item, placement } => {
                session.prepare_dock_item(item, placement).into()
            }
            NativeCommand::DockTabs { group, placement } => {
                session.prepare_dock_tabs(group, placement).into()
            }
            NativeCommand::DockRoot { root, placement } => {
                session.prepare_dock_root(root, placement).into()
            }
            NativeCommand::DockRootBack { root } => session.prepare_dock_root_back(root).into(),
            NativeCommand::FloatItem { item, surface } => {
                session.prepare_float_item(item, surface).into()
            }
            NativeCommand::FloatTabs { group, surface } => {
                session.prepare_float_tabs(group, surface).into()
            }
            NativeCommand::RaiseContained { item } => session.prepare_raise_contained(item).into(),
            NativeCommand::BringContainedIntoView { item } => {
                session.prepare_bring_contained_into_view(item).into()
            }
            NativeCommand::MoveToNewWindow { source, placement } => {
                session.prepare_move_to_new_window(source, placement).into()
            }
            NativeCommand::MoveToNewWindowDefault { source } => {
                session.prepare_move_to_new_window_default(source).into()
            }
            NativeCommand::MoveRootToNewWindow { root, placement } => session
                .prepare_move_to_new_window(NativeTearOffSource::Root(root), placement)
                .into(),
            NativeCommand::CloseItem { item } => session.prepare_close_item(item).into(),
            NativeCommand::CloseRoot { root } => session.prepare_close_root(root).into(),
            NativeCommand::DecideClose {
                request,
                token,
                decision,
            } => NativeApplicationAction::new(
                session.version(),
                NativeApplicationCommand::DecideClose {
                    request,
                    token,
                    decision,
                },
            ),
            NativeCommand::ContinueClose {
                request,
                token,
                decision,
            } => NativeApplicationAction::new(
                session.version(),
                NativeApplicationCommand::ContinueClose {
                    request,
                    token,
                    decision,
                },
            ),
            NativeCommand::SetPolicy { policy } => NativeApplicationAction::new(
                session.version(),
                NativeApplicationCommand::Policy {
                    policy,
                    arrangement_lock: None,
                },
            ),
            NativeCommand::SetArrangementLocked { locked } => {
                let mut policy = session.policy().clone();
                policy.set_arrangement_locked(locked);
                NativeApplicationAction::new(
                    session.version(),
                    NativeApplicationCommand::Policy {
                        policy: Box::new(policy),
                        arrangement_lock: Some(locked),
                    },
                )
            }
        };
        self.enqueue_application_action(action)
    }

    fn sole_host_root(&self) -> Option<DockspaceRootView<'_>> {
        let surface = self
            .coordinator
            .session()
            .view()
            .surface(self.root_surface)?;
        let main = surface.main_root();
        if usize::from(main.is_some()) + surface.contained_count() != 1 {
            return None;
        }
        main.or_else(|| surface.contained().next()?.root())
    }

    fn command_removes_host_surface(&self, command: &crate::NativeCommand) -> bool {
        use crate::NativeCommand;

        // A locked move cannot remove the host; retain its core-owned rejection.
        if self.arrangement_locked() {
            return false;
        }
        let Some(root) = self.sole_host_root() else {
            return false;
        };
        let complete_item = |item| {
            root.central().is_none()
                && root
                    .content()
                    .and_then(|node| node.tabs())
                    .is_some_and(|tabs| tabs.items() == [item])
        };
        let complete_tabs = |group: &PreparedTabsGroup| {
            root.content()
                .and_then(|node| node.tabs())
                .and_then(|tabs| tabs.selected())
                .and_then(|item| {
                    self.coordinator
                        .session()
                        .capture_tabs_group(DockAnchor::Item(item))
                        .ok()
                })
                .is_some_and(|current| current == *group)
        };
        let view = self.coordinator.session().view();
        let target = match command {
            NativeCommand::DockItem { item, placement } if complete_item(*item) => {
                self.placement_surface(*placement)
            }
            NativeCommand::DockTabs { group, placement } if complete_tabs(group) => {
                self.placement_surface(*placement)
            }
            NativeCommand::DockRoot {
                root: source,
                placement,
            } if *source == root.id() => self.placement_surface(*placement),
            NativeCommand::FloatItem { item, surface } if complete_item(*item) => {
                view.surface(*surface).map(|surface| surface.id())
            }
            NativeCommand::FloatTabs { group, surface } if complete_tabs(group) => {
                view.surface(*surface).map(|surface| surface.id())
            }
            _ => None,
        };
        target.is_some_and(|target| target != self.root_surface)
    }

    fn placement_surface(&self, placement: DockPlacement) -> Option<SurfaceId> {
        let view = self.coordinator.session().view();
        let root_surface = |root: RootId| {
            view.surfaces()
                .find(|surface| {
                    surface.main_root().is_some_and(|main| main.id() == root)
                        || surface.contained().any(|contained| {
                            contained.root().is_some_and(|owner| owner.id() == root)
                        })
                })
                .map(|surface| surface.id())
        };
        match placement {
            DockPlacement::Center(DockAnchor::Item(item))
            | DockPlacement::InnerEdge {
                anchor: DockAnchor::Item(item),
                ..
            }
            | DockPlacement::Before(item)
            | DockPlacement::After(item) => view.item(item).map(|item| item.surface()),
            DockPlacement::Center(DockAnchor::Central(root))
            | DockPlacement::InnerEdge {
                anchor: DockAnchor::Central(root),
                ..
            } => {
                view.root(root)?.central()?;
                root_surface(root)
            }
            DockPlacement::OuterEdge { root, .. } => root_surface(root),
            DockPlacement::Main(surface) => view
                .surface(surface)
                .filter(|surface| surface.is_rootless())
                .map(|surface| surface.id()),
        }
    }

    pub(crate) fn arrangement_locked(&self) -> bool {
        self.coordinator.session().policy().arrangement_locked()
    }

    pub(crate) fn pending_content_close_plans(&self) -> Vec<DockspaceClosePlan> {
        self.coordinator
            .session()
            .pending_content_close_plans()
            .collect()
    }

    fn enqueue_application_action(
        &mut self,
        action: impl Into<NativeApplicationAction>,
    ) -> Result<(), NativeActionRequestError> {
        self.ensure_application_slot_available()?;
        let action = action.into();
        if self.content_close_removes_host_surface(&action.command) {
            return Err(NativeActionRequestError::HostSurfaceRequired {
                surface: self.root_surface,
            });
        }
        self.application_actions.enqueue(action)?;
        self.request_root_repaint();
        Ok(())
    }

    fn content_close_removes_host_surface(&self, command: &NativeApplicationCommand) -> bool {
        let session = self.coordinator.session();
        let target = match command {
            NativeApplicationCommand::Close(request) => request.target(),
            NativeApplicationCommand::DecideClose {
                request,
                token,
                decision: CloseDecision::Allow,
            } => {
                let Some(plan) = session.pending_content_close_plans().find(|plan| {
                    plan.request() == *request
                        && plan.items().iter().any(|item| {
                            item.state() == CloseItemDecisionState::Pending
                                && item.token() == *token
                        })
                }) else {
                    return false;
                };
                plan.target()
            }
            NativeApplicationCommand::ContinueClose {
                request,
                token,
                decision: DeferredCloseDecision::Allow,
            } => {
                let Some(plan) = session.pending_content_close_plans().find(|plan| {
                    plan.request() == *request
                        && plan
                            .items()
                            .iter()
                            .any(|item| item.deferred_token() == Some(*token))
                }) else {
                    return false;
                };
                plan.target()
            }
            _ => return false,
        };
        let Some(root) = self.sole_host_root() else {
            return false;
        };
        match target {
            ClosePlanTarget::Root { root: target } => root.id() == target,
            ClosePlanTarget::Item { item } => {
                root.central().is_none()
                    && root
                        .content()
                        .and_then(|node| node.tabs())
                        .is_some_and(|tabs| tabs.items() == [item])
            }
            ClosePlanTarget::Surface { .. } => false,
        }
    }

    pub(crate) fn take_action_status(&mut self) -> Option<NativeActionTerminal> {
        self.application_actions.take_result()
    }
}
#[cfg(test)]
mod layout_tests;
