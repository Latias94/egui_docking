//! Application commands share the existing bounded native action slot.

use dockspace::close::{
    CloseDecision, CloseDecisionToken, CloseItemDecisionState, ClosePlanTarget, CloseRequestId,
    DeferredCloseDecision, DeferredCloseToken,
};
use dockspace::model::{
    DockPlacement, DockspaceLayout, ItemId, NativeWindowPlacement, RootId, SurfaceId,
};
use dockspace::runtime::{DockspaceClosePlan, NativeSurfaceBinding};
use eframe::egui;
use egui_dockspace::PaneView;

use crate::application_action::{
    NativeActionRequestError, NativeActionTerminal, NativeApplicationAction,
    NativeApplicationCommand,
};

use super::NativeRuntimeState;
use crate::NativeLayoutRequestError;

#[cfg(test)]
mod tests;

impl<P: PaneView> NativeRuntimeState<P> {
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
        self.coordinator
            .viewport_binding(egui::ViewportId::ROOT)
            .filter(|binding| binding.surface() == self.root_surface)
            .filter(|binding| {
                self.coordinator.dispatch_bridge().with_viewports(|routes| {
                    routes.window(egui::ViewportId::ROOT).is_some()
                        && routes.has_window_receipt(egui::ViewportId::ROOT, *binding)
                })
            })
            .ok_or_else(|| NativeLayoutRequestError::new_root_window_unavailable(self.root_surface))
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
        restore.validate_native_surface_hosting(incoming_root)?;
        let action = NativeApplicationAction::new(
            self.coordinator.session().version(),
            NativeApplicationCommand::DocumentRestore {
                restore: Box::new(restore),
                root_surface: incoming_root,
                existing_root: Some(existing_root),
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
        restore.validate_native_surface_hosting(self.root_surface)?;
        let action = NativeApplicationAction::new(
            self.coordinator.session().version(),
            NativeApplicationCommand::DocumentRestore {
                restore: Box::new(restore),
                root_surface: self.root_surface,
                existing_root: None,
            },
        );
        self.enqueue_application_action(action)?;
        Ok(())
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
        if let Some(unhosted) = view.surfaces().find(|surface| surface.id() != root_surface) {
            return Err(NativeLayoutRequestError::new_unhosted_surface(
                unhosted.id(),
            ));
        }
        Ok(())
    }

    pub(crate) fn request_dock_root(
        &mut self,
        root: RootId,
        placement: DockPlacement,
    ) -> Result<(), NativeActionRequestError> {
        let action = self
            .coordinator
            .session()
            .prepare_dock_root(root, placement);
        self.enqueue_application_action(action)
    }

    pub(crate) fn request_select_item(
        &mut self,
        item: ItemId,
    ) -> Result<(), NativeActionRequestError> {
        let action = self.coordinator.session().prepare_select_item(item);
        self.enqueue_application_action(action)
    }

    pub(crate) fn request_open_item(
        &mut self,
        item: ItemId,
        placement: DockPlacement,
    ) -> Result<(), NativeActionRequestError> {
        let action = self
            .coordinator
            .session()
            .prepare_open_item(item, placement);
        self.enqueue_application_action(action)
    }

    pub(crate) fn request_close_item(
        &mut self,
        item: ItemId,
    ) -> Result<(), NativeActionRequestError> {
        let action = self.coordinator.session().prepare_close_item(item);
        self.enqueue_application_action(action)
    }

    pub(crate) fn request_close_root(
        &mut self,
        root: RootId,
    ) -> Result<(), NativeActionRequestError> {
        let action = self.coordinator.session().prepare_close_root(root);
        self.enqueue_application_action(action)
    }

    pub(crate) fn request_close_decision(
        &mut self,
        request: CloseRequestId,
        token: CloseDecisionToken,
        decision: CloseDecision,
    ) -> Result<(), NativeActionRequestError> {
        let action = NativeApplicationAction::new(
            self.coordinator.session().version(),
            NativeApplicationCommand::DecideClose {
                request,
                token,
                decision,
            },
        );
        self.enqueue_application_action(action)
    }

    pub(crate) fn request_deferred_close_decision(
        &mut self,
        request: CloseRequestId,
        token: DeferredCloseToken,
        decision: DeferredCloseDecision,
    ) -> Result<(), NativeActionRequestError> {
        let action = NativeApplicationAction::new(
            self.coordinator.session().version(),
            NativeApplicationCommand::ContinueClose {
                request,
                token,
                decision,
            },
        );
        self.enqueue_application_action(action)
    }

    pub(crate) fn request_set_arrangement_locked(
        &mut self,
        locked: bool,
    ) -> Result<(), NativeActionRequestError> {
        let mut policy = self.coordinator.session().policy().clone();
        policy.set_arrangement_locked(locked);
        let action = NativeApplicationAction::new(
            self.coordinator.session().version(),
            NativeApplicationCommand::ArrangementLock(Box::new(policy)),
        );
        self.enqueue_application_action(action)
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
        let Some(surface) = session.view().surface(self.root_surface) else {
            return false;
        };
        let main = surface.main_root();
        if usize::from(main.is_some()) + surface.contained_count() != 1 {
            return false;
        }
        let Some(root) = main.or_else(|| surface.contained().next()?.root()) else {
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

    pub(crate) fn request_dock_root_back(
        &mut self,
        root: RootId,
    ) -> Result<(), NativeActionRequestError> {
        let action = self.coordinator.session().prepare_dock_root_back(root);
        self.enqueue_application_action(action)
    }

    pub(crate) fn request_move_root_to_new_window(
        &mut self,
        root: RootId,
        placement: NativeWindowPlacement,
    ) -> Result<(), NativeActionRequestError> {
        let action = self
            .coordinator
            .session()
            .prepare_move_root_to_new_window(root, placement);
        self.enqueue_application_action(action)
    }

    pub(crate) fn take_action_status(&mut self) -> Option<NativeActionTerminal> {
        self.application_actions.take_result()
    }
}
#[cfg(test)]
mod layout_tests;
