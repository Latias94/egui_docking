//! Atomic workspace, policy and presentation-configuration replacement.

use crate::engine::*;

impl DockEngine {
    pub(in crate::engine) fn reduce_workspace_replacement(
        &mut self,
        input: InputSequence,
        focus_causal: FocusCausalStamp,
        workspace: &Workspace,
        restored_identity_frontier: Option<PresentationIdentityFrontier>,
        native_surfaces: Option<&crate::frame::NativeSurfaceRestorePlan>,
        root_continuations: &[crate::frame::PreparedNativeRootContinuation],
        application_base: &mut WorkspaceVersion,
        events: &mut Vec<WorkspaceEvent>,
        interaction_events: &mut Vec<InteractionEvent>,
    ) -> Result<InputOutcome, EngineError> {
        if self.policy.policy().arrangement_locked() {
            return Err(EngineError::WorkspaceReplacementPolicyDenied {
                input,
                reason: crate::policy::PolicyRejection::ArrangementLocked,
            });
        }
        let replacement = crate::operation::WorkspaceSnapshot::validated(workspace.clone())
            .map_err(EngineError::InvalidWorkspace)?;
        let previous_focus = self.viewport_focus.workspace_replacement_focus();
        let mut native_focus_surfaces = self
            .viewport
            .registry()
            .records()
            .map(|(surface, _)| surface)
            .collect::<BTreeSet<_>>();
        if let Some(intent) = self.viewport_focus.pending_pane_intent()
            && (intent.awaiting_native_binding() || intent.native_guard().is_some())
        {
            native_focus_surfaces.insert(intent.surface());
        }
        let before = self.version;
        let epoch = before
            .epoch()
            .checked_next()
            .ok_or(EngineError::WorkspaceEpochExhausted { input })?;
        let desired_surfaces = workspace
            .surfaces()
            .map(|(surface, _)| surface)
            .collect::<std::collections::BTreeSet<_>>();
        let reconciliation = self
            .viewport
            .reconcile_workspace_epoch(epoch, &desired_surfaces, root_continuations)
            .map_err(|source| EngineError::Viewport { input, source })?;
        let rebound_root_surfaces = reconciliation
            .rebound()
            .iter()
            .filter_map(|(_, binding)| {
                self.viewport
                    .viewport(binding.surface())
                    .filter(|record| {
                        record.binding() == *binding && record.role() == ViewportRole::Root
                    })
                    .map(|_| binding.surface())
            })
            .collect::<BTreeSet<_>>();
        match restored_identity_frontier {
            Some(restored) => self
                .presentation_identity
                .merge_restored(workspace, restored),
            None => self.presentation_identity.observe_workspace(workspace),
        }
        self.workspace = replacement;
        self.surface_recovery.clear();
        self.root_recovery_anchors.clear();
        self.bound_surface_recoveries.clear();
        self.native_admission.clear();
        self.issue_root_recovery_anchors(input, rebound_root_surfaces)?;
        if let Some(plan) = native_surfaces {
            for &root in &plan.roots {
                if !self.viewport.viewport(root).is_some_and(|record| {
                    record.role() == ViewportRole::Root
                        && record.ownership()
                            == crate::viewport_registry::ViewportOwnership::External
                }) || !self.root_recovery_anchors.contains_key(&root)
                {
                    return Err(EngineError::NativeWorkspaceRootUnavailable {
                        input,
                        surface: root,
                    });
                }
            }
            for child in &plan.children {
                if self.viewport.viewport(child.surface).is_some()
                    || self.root_recovery_anchors.contains_key(&child.surface)
                {
                    return Err(EngineError::NativeWorkspaceChildAlreadyHosted {
                        input,
                        surface: child.surface,
                    });
                }
            }
            self.viewport
                .enqueue_native_surface_restore(plan)
                .map_err(|source| EngineError::Viewport { input, source })?;
            for child in &plan.children {
                native_focus_surfaces.insert(child.surface);
                let roster = self
                    .capture_surface_roster(child.surface)
                    .map_err(|source| EngineError::SurfaceRoster { input, source })?;
                if !self.surface_recovery.defer(roster) {
                    return Err(EngineError::ConflictingSurfaceRecovery {
                        input,
                        surface: child.surface,
                    });
                }
            }
        }
        self.viewport_focus
            .reconcile_workspace_replacement(reconciliation.rebound());
        self.version = WorkspaceVersion::new(epoch, WorkspaceRevision::default());
        self.close.invalidate_stale(self.close_authority());
        self.rebuild_presentation_requirements(input)?;
        *application_base = self.version;
        self.invalidate_transient(
            input,
            InteractionCancelReason::WorkspaceRestored,
            interaction_events,
        )?;
        self.request_workspace_replacement_focus(
            previous_focus,
            &native_focus_surfaces,
            focus_causal,
        )?;
        events.push(WorkspaceEvent::new(
            input,
            self.version,
            WorkspaceEventKind::WorkspaceReplaced,
        ));
        Ok(InputOutcome::WorkspaceReplaced {
            before,
            after: self.version,
            restored_identity_frontier,
            reconciliation,
        })
    }

    #[allow(clippy::too_many_arguments)]
    pub(in crate::engine) fn reduce_policy_replacement(
        &mut self,
        input: InputSequence,
        expected: WorkspaceVersion,
        accepted_base: WorkspaceVersion,
        policy: &DockPolicy,
        events: &mut Vec<WorkspaceEvent>,
        interaction_events: &mut Vec<InteractionEvent>,
    ) -> Result<InputOutcome, EngineError> {
        if expected != accepted_base {
            return Ok(InputOutcome::StaleRejected {
                expected,
                accepted_base,
            });
        }
        let changed = !self.policy.has_same_rules(policy);
        if changed {
            let policy_revision = self
                .policy
                .revision()
                .checked_next()
                .ok_or(EngineError::PolicyRevisionExhausted { input })?;
            self.policy = policy.snapshot(policy_revision);
            self.advance_revision(input)?;
            self.invalidate_transient(
                input,
                InteractionCancelReason::PolicyChanged,
                interaction_events,
            )?;
            events.push(WorkspaceEvent::new(
                input,
                self.version,
                WorkspaceEventKind::PolicyReplaced,
            ));
        }
        Ok(InputOutcome::PolicyReplaced {
            changed,
            version: self.version,
        })
    }

    pub(in crate::engine) fn reduce_presentation_config_replacement(
        &mut self,
        input: InputSequence,
        expected: WorkspaceVersion,
        accepted_base: WorkspaceVersion,
        config: &DockPresentationConfig,
        interaction_events: &mut Vec<InteractionEvent>,
    ) -> Result<InputOutcome, EngineError> {
        if expected != accepted_base {
            return Ok(InputOutcome::StaleRejected {
                expected,
                accepted_base,
            });
        }
        let changed = self.presentation_authority.presentation_config != *config;
        if changed {
            let revision = self
                .presentation_authority
                .presentation_config_revision
                .checked_next()
                .ok_or(EngineError::PresentationConfigRevisionExhausted { input })?;
            self.presentation_authority.presentation_config = config.clone();
            self.presentation_authority.presentation_config_revision = revision;
            self.rebuild_presentation_requirements(input)?;
            self.terminate_all_scroll_sessions_input(
                input,
                ScrollTerminationReason::PresentationConfigChanged,
                interaction_events,
            );
            self.invalidate_transient(
                input,
                InteractionCancelReason::SceneUnavailable,
                interaction_events,
            )?;
        }
        Ok(InputOutcome::PresentationConfigReplaced {
            changed,
            revision: self.presentation_authority.presentation_config_revision,
        })
    }
}
