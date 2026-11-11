//! Workspace command delivery over the current ordered input and frozen authority.

use crate::engine::*;

impl DockEngine {
    pub(in crate::engine) fn apply_journal_workspace_command(
        &mut self,
        cause: ReductionCause,
        command: &WorkspaceCommand,
        policy: &DockPolicySnapshot,
        events: &mut Vec<WorkspaceEvent>,
    ) -> Result<Result<(CommandOutcome, bool), CommandError>, EngineError> {
        self.apply_journal_workspace_command_with_authority(
            cause,
            command,
            policy,
            WorkspacePublicationAuthority::Ordinary,
            JournalDeliveryActivation::None,
            None,
            events,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub(in crate::engine) fn apply_journal_workspace_command_with_authority(
        &mut self,
        cause: ReductionCause,
        command: &WorkspaceCommand,
        policy: &DockPolicySnapshot,
        authority: WorkspacePublicationAuthority,
        activation: JournalDeliveryActivation,
        prepared: Option<PreparedDropMutation>,
        events: &mut Vec<WorkspaceEvent>,
    ) -> Result<Result<(CommandOutcome, bool), CommandError>, EngineError> {
        let prepared = prepared.filter(|prepared| prepared.matches(self.version, policy, command));
        let staged = if let Some(prepared) = prepared {
            // Release still checks the exact current command and policy. Only
            // its already validated, uniquely owned graph draft is reused.
            if let Err(error) =
                crate::operation::authorize_workspace_command(&self.workspace, policy, command)
            {
                return Ok(Err(error));
            }
            let (workspace, report) =
                prepared
                    .into_owned_draft()
                    .ok_or(EngineError::ReductionCauseInvariant {
                        detail: "a release preflight still has another graph draft owner",
                    })?;
            self.stage_journal_workspace_draft(
                cause, policy, authority, activation, workspace, &report,
            )?
        } else {
            self.stage_journal_workspace_command_with_authority(
                cause, command, policy, authority, activation,
            )?
        };
        let staged = match staged {
            Ok(staged) => staged,
            Err(source) => return Ok(Err(source)),
        };
        self.publish_workspace(staged.publication);
        self.reconcile_viewport_focus_authority();
        if staged.changed {
            if let WorkspaceCommand::Select { source } = command {
                self.advance_selection_revision_caused(cause, source)?;
            } else {
                self.advance_revision_caused(cause)?;
            }
            events.push(WorkspaceEvent::new_caused(
                cause,
                self.version,
                WorkspaceEventKind::CommandCommitted(staged.outcome.clone()),
            ));
            for activation in staged.activation {
                events.push(WorkspaceEvent::new_caused(
                    cause,
                    self.version,
                    WorkspaceEventKind::CommandCommitted(activation),
                ));
            }
        }
        Ok(Ok((staged.outcome, staged.changed)))
    }

    pub(in crate::engine) fn stage_journal_workspace_command(
        &self,
        cause: ReductionCause,
        command: &WorkspaceCommand,
        policy: &DockPolicySnapshot,
    ) -> Result<Result<StagedJournalWorkspaceCommand, CommandError>, EngineError> {
        self.stage_journal_workspace_command_with_authority(
            cause,
            command,
            policy,
            WorkspacePublicationAuthority::Ordinary,
            JournalDeliveryActivation::None,
        )
    }

    pub(in crate::engine) fn stage_journal_workspace_command_with_authority(
        &self,
        cause: ReductionCause,
        command: &WorkspaceCommand,
        policy: &DockPolicySnapshot,
        authority: WorkspacePublicationAuthority,
        activation: JournalDeliveryActivation,
    ) -> Result<Result<StagedJournalWorkspaceCommand, CommandError>, EngineError> {
        let prepared = match self
            .workspace
            .prepare_transaction(policy, std::slice::from_ref(command))
        {
            Ok(prepared) => prepared,
            Err(TransactionError::Command { index: 0, source })
                if source.is_expected_rejection() =>
            {
                return Ok(Err(source));
            }
            Err(source) => {
                return Err(EngineError::PointerInteractionInvariant {
                    cause,
                    detail: source.to_string(),
                });
            }
        };
        let (workspace, report) = prepared.into_snapshot_parts();
        self.stage_journal_workspace_draft(cause, policy, authority, activation, workspace, &report)
    }

    pub(in crate::engine) fn stage_journal_workspace_draft(
        &self,
        cause: ReductionCause,
        policy: &DockPolicySnapshot,
        authority: WorkspacePublicationAuthority,
        activation: JournalDeliveryActivation,
        workspace: impl Into<crate::operation::WorkspaceSnapshot>,
        report: &crate::transaction::TransactionReport,
    ) -> Result<Result<StagedJournalWorkspaceCommand, CommandError>, EngineError> {
        let mut workspace = workspace.into();
        let mut changed = report.changed();
        // A successful manipulation may activate its delivered content. Stage
        // that command against the post-move source and publish both together;
        // failure cannot leave a move without its required activation.
        let raise = match activation {
            JournalDeliveryActivation::SelectItem { item, source_root } => workspace
                .locate_item_by_id(item)
                .filter(|(root, _, _)| *root == source_root)
                .and_then(|(root, _, _)| workspace.presentation_for_root(root))
                .and_then(|owner| match owner {
                    crate::RootPresentationOwner::Contained { floating, .. } => {
                        Some(JournalDeliveryActivation::RaiseContained(floating))
                    }
                    crate::RootPresentationOwner::Main { .. } => None,
                }),
            JournalDeliveryActivation::None | JournalDeliveryActivation::RaiseContained(_) => None,
        };
        let mut activation_outcomes = Vec::new();
        for activation in [Some(activation), raise].into_iter().flatten() {
            let command = match Self::journal_delivery_activation_command(&workspace, activation) {
                Ok(Some(command)) => command,
                Ok(None) => continue,
                Err(source) => return Ok(Err(source)),
            };
            let activated = match workspace.apply_draft_command(policy, &command) {
                Ok(outcome) => outcome,
                Err(TransactionError::Command { index: 0, source })
                    if source.is_expected_rejection() =>
                {
                    return Ok(Err(source));
                }
                Err(source) => {
                    return Err(EngineError::PointerInteractionInvariant {
                        cause,
                        detail: source.to_string(),
                    });
                }
            };
            changed = *workspace != *self.workspace;
            activation_outcomes.push(activated);
        }
        if let Some(surface) =
            self.first_pending_presentation_source_mismatch(&workspace, authority)
        {
            return Ok(Err(CommandError::SurfaceLifecycleFrozen { surface }));
        }
        if let Some(surface) = self.first_workspace_publication_mismatch(&workspace, None, None) {
            return Ok(Err(CommandError::SurfaceLifecycleFrozen { surface }));
        }
        let [outcome] = report.outcomes() else {
            return Err(EngineError::PointerInteractionInvariant {
                cause,
                detail: "one journal drop command produced a non-unit outcome roster".to_owned(),
            });
        };
        let outcome = outcome.clone();
        let publication =
            match self.stage_workspace_publication_with_authority(workspace, policy, authority) {
                Ok(publication) => publication,
                Err(source) if source.is_expected_rejection() => return Ok(Err(source)),
                Err(source) => {
                    return Err(EngineError::PointerInteractionInvariant {
                        cause,
                        detail: source.to_string(),
                    });
                }
            };
        Ok(Ok(StagedJournalWorkspaceCommand {
            publication,
            outcome,
            activation: activation_outcomes,
            changed,
        }))
    }

    pub(in crate::engine) fn journal_delivery_activation_command(
        workspace: &Workspace,
        activation: JournalDeliveryActivation,
    ) -> Result<Option<WorkspaceCommand>, CommandError> {
        match activation {
            JournalDeliveryActivation::None => Ok(None),
            JournalDeliveryActivation::SelectItem { item, .. } => {
                let source =
                    workspace
                        .capture_item_source_by_id(item)?
                        .ok_or(CommandError::Invariant {
                            stage: "select delivered item after manipulation",
                        })?;
                if matches!(
                    workspace.node(source.tabs()),
                    Some(Node::Tabs { selected: Some(selected), .. }) if *selected == item
                ) {
                    Ok(None)
                } else {
                    Ok(Some(WorkspaceCommand::Select { source }))
                }
            }
            JournalDeliveryActivation::RaiseContained(floating) => {
                let contained = workspace
                    .contained_floating(floating)
                    .ok_or(CommandError::MissingFloating { floating })?;
                let Some(crate::RootPresentationOwner::Contained { surface, .. }) =
                    workspace.presentation_for_root(contained.root)
                else {
                    return Err(CommandError::Invariant {
                        stage: "raise delivered contained manipulation",
                    });
                };
                let expected_roster = workspace.capture_contained_roster(surface)?;
                if expected_roster.contained().last() == Some(&floating) {
                    return Ok(None);
                }
                let root = workspace
                    .root(contained.root)
                    .ok_or(CommandError::MissingRoot {
                        root: contained.root,
                    })?;
                let source = workspace.capture_node_source(contained.root, root.node)?;
                Ok(Some(WorkspaceCommand::RaiseContained {
                    source,
                    floating,
                    expected_roster,
                }))
            }
        }
    }

    pub(in crate::engine) fn capture_surface_coordinates(
        &self,
        surface: crate::ids::SurfaceId,
    ) -> SurfaceCoordinateCapture {
        let authority_generation = self.viewport.surface_coordinate_authority(surface);
        match self.viewport.viewport(surface) {
            None => SurfaceCoordinateCapture::Headless {
                authority_generation,
            },
            Some(record) if record.has_coordinate_authority() => match record.coordinates() {
                Some(coordinates) => SurfaceCoordinateCapture::NativeReady {
                    coordinates,
                    authority_generation,
                },
                None => SurfaceCoordinateCapture::NativeUnavailable {
                    binding: record.binding(),
                    lifecycle: record.lifecycle(),
                    authority_generation,
                },
            },
            Some(record) => SurfaceCoordinateCapture::NativeUnavailable {
                binding: record.binding(),
                lifecycle: record.lifecycle(),
                authority_generation,
            },
        }
    }

    pub(in crate::engine) fn coordinate_capture_matches_current(
        capture: SurfaceCoordinateCapture,
        current: Option<crate::viewport_registry::ViewportRecord<'_>>,
        current_authority: crate::viewport::CoordinateGeneration,
    ) -> bool {
        match capture {
            SurfaceCoordinateCapture::Headless {
                authority_generation,
            } => current.is_none() && authority_generation == current_authority,
            SurfaceCoordinateCapture::NativeUnavailable {
                binding,
                lifecycle,
                authority_generation,
            } => {
                authority_generation == current_authority
                    && current.is_some_and(|record| {
                        !record.has_coordinate_authority()
                            && record.binding() == binding
                            && record.lifecycle() == lifecycle
                            && record.coordinate_generation() == authority_generation
                    })
            }
            SurfaceCoordinateCapture::NativeReady {
                coordinates,
                authority_generation,
            } => {
                authority_generation == current_authority
                    && current.is_some_and(|record| {
                        record.has_coordinate_authority()
                            && record.coordinate_generation() == authority_generation
                            && record.coordinates().is_some_and(|current| {
                                current.binding() == coordinates.binding()
                                    && current.coordinate_generation()
                                        == coordinates.coordinate_generation()
                                    && current.content_size() == coordinates.content_size()
                                    && current.content_bounds() == coordinates.content_bounds()
                                    && current.native_scale_factor()
                                        == coordinates.native_scale_factor()
                                    && current.presentation_scale_factor()
                                        == coordinates.presentation_scale_factor()
                                    && current.surface_client_geometry()
                                        == coordinates.surface_client_geometry()
                            })
                    })
            }
        }
    }

    pub(in crate::engine) fn coordinate_capture_incarnation_is_current(
        capture: SurfaceCoordinateCapture,
        current: Option<crate::viewport_registry::ViewportRecord<'_>>,
        current_authority: crate::viewport::CoordinateGeneration,
    ) -> bool {
        match Self::coordinate_capture_binding(capture) {
            Some(binding) => current.is_some_and(|record| record.binding() == binding),
            None => Self::coordinate_capture_matches_current(capture, current, current_authority),
        }
    }

    pub(in crate::engine) const fn coordinate_capture_binding(
        capture: SurfaceCoordinateCapture,
    ) -> Option<crate::viewport::ViewportBinding> {
        match capture {
            SurfaceCoordinateCapture::Headless { .. } => None,
            SurfaceCoordinateCapture::NativeUnavailable { binding, .. } => Some(binding),
            SurfaceCoordinateCapture::NativeReady { coordinates, .. } => {
                Some(coordinates.binding())
            }
        }
    }

    /// Finishes one sealed core-minted host frame and publishes it atomically.
    ///
    /// The capability must come from [`Self::begin_host_frame`] on this exact
    /// engine. Its frozen domain, authority frontiers, workspace version,
    /// requirement revision, and complete roster are revalidated before any
    /// tick or source watermark can advance. Semantic and configuration inputs
    /// reduce in append order in the private candidate before its contributions.
    pub(in crate::engine) fn platform_provider_rejection(
        &self,
        provider: PlatformObservationLease,
    ) -> Option<InputOutcome> {
        self.viewport
            .require_platform_provider(provider)
            .err()
            .map(|error| InputOutcome::PlatformProviderRejected { provider, error })
    }
}

impl DockEngine {
    pub(in crate::engine) fn apply_interaction_command(
        &mut self,
        input: InputSequence,
        command: &WorkspaceCommand,
        events: &mut Vec<WorkspaceEvent>,
    ) -> Result<CommandApplication, EngineError> {
        let policy = self.policy.clone();
        self.apply_interaction_command_with_policy(input, &policy, command, events)
    }

    pub(in crate::engine) fn apply_interaction_command_with_policy(
        &mut self,
        input: InputSequence,
        policy: &DockPolicySnapshot,
        command: &WorkspaceCommand,
        events: &mut Vec<WorkspaceEvent>,
    ) -> Result<CommandApplication, EngineError> {
        self.apply_interaction_command_with_policy_and_barrier(input, policy, command, None, events)
    }

    #[cfg(test)]
    pub(in crate::engine) fn apply_interaction_command_with_barrier(
        &mut self,
        input: InputSequence,
        command: &WorkspaceCommand,
        action_barrier: Option<&BTreeMap<crate::ids::SurfaceId, SurfaceRosterDisposition>>,
        events: &mut Vec<WorkspaceEvent>,
    ) -> Result<CommandApplication, EngineError> {
        let policy = self.policy.clone();
        self.apply_interaction_command_with_policy_and_barrier(
            input,
            &policy,
            command,
            action_barrier,
            events,
        )
    }

    pub(in crate::engine) fn apply_interaction_command_with_policy_and_barrier(
        &mut self,
        input: InputSequence,
        policy: &DockPolicySnapshot,
        command: &WorkspaceCommand,
        action_barrier: Option<&BTreeMap<crate::ids::SurfaceId, SurfaceRosterDisposition>>,
        events: &mut Vec<WorkspaceEvent>,
    ) -> Result<CommandApplication, EngineError> {
        let (candidate, application) =
            self.stage_workspace_command(input, policy, command, action_barrier)?;
        if let Some(candidate) = candidate {
            self.publish_workspace(candidate);
            self.reconcile_viewport_focus_authority();
        }
        self.record_interaction_command_application(input, application, events)
    }

    pub(in crate::engine) fn record_interaction_command_application(
        &mut self,
        input: InputSequence,
        application: CommandApplication,
        events: &mut Vec<WorkspaceEvent>,
    ) -> Result<CommandApplication, EngineError> {
        if let CommandApplication::Applied { outcome, changed } = &application
            && *changed
        {
            self.advance_revision(input)?;
            events.push(WorkspaceEvent::new(
                input,
                self.version,
                WorkspaceEventKind::CommandCommitted(outcome.clone()),
            ));
        }
        Ok(application)
    }
}
