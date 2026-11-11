//! Checked workspace staging and publication under the sole engine owner.

mod commands;
mod configuration;

use crate::engine::*;

impl DockEngine {
    pub(super) fn stage_workspace_publication(
        &self,
        candidate: impl Into<crate::operation::WorkspaceSnapshot>,
        policy: &DockPolicySnapshot,
    ) -> Result<StagedWorkspacePublication, CommandError> {
        self.stage_workspace_publication_with_authority(
            candidate,
            policy,
            WorkspacePublicationAuthority::Ordinary,
        )
    }

    pub(super) fn stage_workspace_publication_with_authority(
        &self,
        candidate: impl Into<crate::operation::WorkspaceSnapshot>,
        policy: &DockPolicySnapshot,
        authority: WorkspacePublicationAuthority,
    ) -> Result<StagedWorkspacePublication, CommandError> {
        let candidate = candidate.into();
        let anchors = self.root_recovery_anchors.clone();
        let mut recoveries = self.bound_surface_recoveries.clone();
        let recovery_commit = match authority {
            WorkspacePublicationAuthority::RecoveryCommit {
                source_surface,
                obligation,
            } => Some((source_surface, obligation)),
            WorkspacePublicationAuthority::Ordinary
            | WorkspacePublicationAuthority::NativeCommit { .. }
            | WorkspacePublicationAuthority::WindowRecoveryCommit { .. }
            | WorkspacePublicationAuthority::PresentationRehome { .. }
            | WorkspacePublicationAuthority::PendingPresentationRehome { .. } => None,
        };
        let presentation_rehome = match authority {
            WorkspacePublicationAuthority::PresentationRehome {
                source_surface,
                obligation,
                root,
                floating,
            } => Some((source_surface, obligation, root, floating)),
            WorkspacePublicationAuthority::Ordinary
            | WorkspacePublicationAuthority::NativeCommit { .. }
            | WorkspacePublicationAuthority::RecoveryCommit { .. }
            | WorkspacePublicationAuthority::WindowRecoveryCommit { .. }
            | WorkspacePublicationAuthority::PendingPresentationRehome { .. } => None,
        };
        if let Some((source_surface, obligation, root, floating)) = presentation_rehome
            && !recoveries.get(&source_surface).is_some_and(|current| {
                current.obligation.id() == obligation
                    && current
                        .obligation
                        .target()
                        .converted_main()
                        .is_some_and(|converted| {
                            converted.source_root() == root && converted.floating() == floating
                        })
            })
        {
            return Err(CommandError::SurfaceLifecycleFrozen {
                surface: source_surface,
            });
        }
        if let Some((source_surface, obligation)) = recovery_commit
            && !recoveries
                .get(&source_surface)
                .is_some_and(|current| current.obligation.id() == obligation)
        {
            return Err(CommandError::SurfaceLifecycleFrozen {
                surface: source_surface,
            });
        }
        let window_recovery = match authority {
            WorkspacePublicationAuthority::WindowRecoveryCommit { window } => Some(window),
            _ => None,
        };
        if let Some(window) = window_recovery {
            for (surface, bound) in &recoveries {
                if bound.binding.window() == window && candidate.surface(*surface).is_some() {
                    return Err(CommandError::SurfaceLifecycleFrozen { surface: *surface });
                }
            }
        }
        let live_surfaces = recoveries.keys().copied().collect::<Vec<_>>();
        for surface in live_surfaces {
            let bound = recoveries
                .get(&surface)
                .cloned()
                .ok_or(CommandError::Invariant {
                    stage: "load recovery obligation",
                })?;
            if candidate.surface(surface).is_none() {
                continue;
            }
            let obligation = &bound.obligation;
            let host = obligation.request().host_surface();
            if candidate.surface(host).is_none()
                || anchors.get(&host) != Some(&obligation.target().anchor())
            {
                return Err(CommandError::SurfaceLifecycleFrozen { surface: host });
            }
            let request = Self::recovery_policy_request(&candidate, surface, host)?;
            if request != *obligation.request() {
                if let PolicyDecision::Reject(reason) =
                    policy.evaluate(&DockPolicyRequest::RecoverSurface(request.clone()))
                {
                    return Err(CommandError::Policy(reason));
                }
                let next = obligation
                    .reauthorized_for(request, policy.revision())
                    .map_err(|_| CommandError::SurfaceLifecycleFrozen { surface })?;
                recoveries.insert(surface, bound.reauthorize(next));
            }
        }

        for (surface, bound) in &recoveries {
            let obligation = &bound.obligation;
            if let Some(converted) = obligation.target().converted_main()
                && candidate.contained_floating(converted.floating()).is_some()
            {
                let exact_recovery_consumption = (recovery_commit
                    == Some((*surface, obligation.id()))
                    || window_recovery == Some(bound.binding.window()))
                    && candidate.presentation_for_root(converted.source_root())
                        == Some(crate::RootPresentationOwner::Contained {
                            surface: obligation.target().host_surface(),
                            floating: converted.floating(),
                        });
                let exact_presentation_rehome = matches!(
                    presentation_rehome,
                    Some((source_surface, obligation_id, root, floating))
                        if source_surface == *surface
                            && obligation_id == obligation.id()
                            && root == converted.source_root()
                            && floating == converted.floating()
                ) && candidate
                    .presentation_for_root(converted.source_root())
                    == Some(crate::RootPresentationOwner::Contained {
                        surface: obligation.target().host_surface(),
                        floating: converted.floating(),
                    });
                if !exact_recovery_consumption && !exact_presentation_rehome {
                    return Err(CommandError::SurfaceLifecycleFrozen { surface: *surface });
                }
            }
        }

        if let Some((source_surface, obligation)) = recovery_commit {
            if candidate.surface(source_surface).is_some() {
                return Err(CommandError::SurfaceLifecycleFrozen {
                    surface: source_surface,
                });
            }
            let removed =
                recoveries
                    .remove(&source_surface)
                    .ok_or(CommandError::SurfaceLifecycleFrozen {
                        surface: source_surface,
                    })?;
            if removed.obligation.id() != obligation {
                return Err(CommandError::SurfaceLifecycleFrozen {
                    surface: source_surface,
                });
            }
        }
        if let Some(window) = window_recovery {
            recoveries.retain(|_, bound| bound.binding.window() != window);
        }
        for (saga_id, saga) in self.viewport.native_create_sagas() {
            let prepared = saga.prepared();
            let obligation = prepared.recovery_obligation();
            let surface = obligation.request().source_surface();
            if let Some(converted) = obligation.target().converted_main()
                && candidate.contained_floating(converted.floating()).is_some()
            {
                // A contained source keeps this same floating identity until
                // transfer. Its future recovery reservation must not freeze an
                // unrelated create while that exact source is unchanged.
                let source_preserved = matches!(
                    self.workspace.presentation_for_root(converted.source_root()),
                    Some(crate::RootPresentationOwner::Contained { surface: source_surface, floating })
                        if source_surface == prepared.source_surface() && floating == converted.floating()
                ) && candidate
                    .presentation_for_root(converted.source_root())
                    == self
                        .workspace
                        .presentation_for_root(converted.source_root())
                    && candidate.contained_floating(converted.floating())
                        == self.workspace.contained_floating(converted.floating())
                    && matches!(prepared.command(), WorkspaceCommand::RehomeRoot { source, .. }
                        if candidate.capture_node_source(source.root(), source.node()).as_ref() == Ok(source));
                if !source_preserved {
                    return Err(CommandError::SurfaceLifecycleFrozen { surface });
                }
            }
            if candidate.surface(surface).is_some() {
                let transferring = matches!(
                    authority, WorkspacePublicationAuthority::NativeCommit { saga } if saga == saga_id
                );
                if !transferring
                    || Self::recovery_policy_request(
                        &candidate,
                        surface,
                        obligation.request().host_surface(),
                    )? != *obligation.request()
                {
                    return Err(CommandError::SurfaceLifecycleFrozen { surface });
                }
            }
        }

        let referenced_anchors = recoveries
            .iter()
            .filter(|(surface, _)| candidate.surface(**surface).is_some())
            .map(|(_, bound)| bound.obligation.target().anchor())
            .chain(
                self.viewport
                    .native_create_sagas()
                    .map(|(_, saga)| saga.prepared().recovery_obligation().target().anchor()),
            )
            .collect::<BTreeSet<_>>();
        for anchor in &referenced_anchors {
            if anchors.get(&anchor.surface()) != Some(anchor)
                || candidate.surface(anchor.surface()).is_none()
            {
                return Err(CommandError::SurfaceLifecycleFrozen {
                    surface: anchor.surface(),
                });
            }
        }
        Ok(StagedWorkspacePublication {
            workspace: candidate,
            root_recovery_anchors: anchors,
            bound_surface_recoveries: recoveries,
        })
    }

    pub(super) fn stage_workspace_command(
        &self,
        input: InputSequence,
        policy: &DockPolicySnapshot,
        command: &WorkspaceCommand,
        action_barrier: Option<&BTreeMap<crate::ids::SurfaceId, SurfaceRosterDisposition>>,
    ) -> Result<(Option<StagedWorkspacePublication>, CommandApplication), EngineError> {
        self.stage_workspace_command_with_authority(
            input,
            policy,
            command,
            action_barrier,
            WorkspacePublicationAuthority::Ordinary,
        )
    }

    pub(super) fn stage_workspace_command_for_native_commit(
        &self,
        input: InputSequence,
        policy: &DockPolicySnapshot,
        command: &WorkspaceCommand,
        action_barrier: Option<&BTreeMap<crate::ids::SurfaceId, SurfaceRosterDisposition>>,
        saga: NativeCreateSagaId,
    ) -> Result<(Option<StagedWorkspacePublication>, CommandApplication), EngineError> {
        self.stage_workspace_command_with_authority(
            input,
            policy,
            command,
            action_barrier,
            WorkspacePublicationAuthority::NativeCommit { saga },
        )
    }

    pub(super) fn stage_workspace_command_with_authority(
        &self,
        input: InputSequence,
        policy: &DockPolicySnapshot,
        command: &WorkspaceCommand,
        action_barrier: Option<&BTreeMap<crate::ids::SurfaceId, SurfaceRosterDisposition>>,
        authority: WorkspacePublicationAuthority,
    ) -> Result<(Option<StagedWorkspacePublication>, CommandApplication), EngineError> {
        if self.logical_surface_lifecycle_rejection(command).is_some() {
            let surface = match command {
                WorkspaceCommand::AttachEmptySurface { surface, .. }
                | WorkspaceCommand::RetireEmptySurface { surface } => *surface,
                _ => unreachable!("only membership commands have a lifecycle refusal"),
            };
            return Ok((
                None,
                CommandApplication::Rejected(CommandError::SurfaceLifecycleFrozen { surface }),
            ));
        }
        let prepared = match self
            .workspace
            .prepare_transaction(policy, std::slice::from_ref(command))
        {
            Ok(prepared) => prepared,
            Err(TransactionError::Command { index: 0, source })
                if source.is_expected_rejection() =>
            {
                return Ok((None, CommandApplication::Rejected(source)));
            }
            Err(source) => return Err(EngineError::Command { input, source }),
        };
        let (candidate, report) = prepared.into_snapshot_parts();
        if let Some(surface) =
            self.first_pending_presentation_source_mismatch(&candidate, authority)
        {
            return Ok((
                None,
                CommandApplication::Rejected(CommandError::SurfaceLifecycleFrozen { surface }),
            ));
        }
        if let Some(surface) =
            self.first_workspace_publication_mismatch(&candidate, action_barrier, None)
        {
            return Ok((
                None,
                CommandApplication::Rejected(crate::error::CommandError::SurfaceLifecycleFrozen {
                    surface,
                }),
            ));
        }
        let application = Self::command_application_from_report(input, report)?;
        let publication =
            match self.stage_workspace_publication_with_authority(candidate, policy, authority) {
                Ok(publication) => publication,
                Err(source) if source.is_expected_rejection() => {
                    return Ok((None, CommandApplication::Rejected(source)));
                }
                Err(source) => {
                    return Err(EngineError::Command {
                        input,
                        source: TransactionError::Command { index: 0, source },
                    });
                }
            };
        Ok((Some(publication), application))
    }

    pub(super) fn first_workspace_publication_mismatch(
        &self,
        candidate: &Workspace,
        action_barrier: Option<&BTreeMap<crate::ids::SurfaceId, SurfaceRosterDisposition>>,
        excluded: Option<crate::ids::SurfaceId>,
    ) -> Option<crate::ids::SurfaceId> {
        let persistent = match excluded {
            Some(surface) => self
                .surface_recovery
                .first_workspace_mismatch_excluding(candidate, surface),
            None => self.surface_recovery.first_workspace_mismatch(candidate),
        };
        let transient = action_barrier.and_then(|barrier| {
            barrier.values().find_map(|roster| {
                (Some(roster.surface()) != excluded && !roster.matches_workspace(candidate))
                    .then_some(roster.surface())
            })
        });
        persistent.or(transient)
    }

    pub(super) fn first_workspace_publication_mismatch_excluding(
        &self,
        candidate: &Workspace,
        action_barrier: Option<&BTreeMap<SurfaceId, SurfaceRosterDisposition>>,
        excluded: &BTreeSet<SurfaceId>,
    ) -> Option<SurfaceId> {
        self.surface_recovery
            .first_workspace_mismatch_excluding_surfaces(candidate, excluded)
            .or_else(|| {
                action_barrier.and_then(|barrier| {
                    barrier.values().find_map(|roster| {
                        (!excluded.contains(&roster.surface())
                            && !roster.matches_workspace(candidate))
                        .then_some(roster.surface())
                    })
                })
            })
    }

    pub(super) fn publish_workspace(&mut self, publication: StagedWorkspacePublication) {
        self.presentation_identity
            .observe_workspace(&publication.workspace);
        self.workspace = publication.workspace;
        self.root_recovery_anchors = publication.root_recovery_anchors;
        self.bound_surface_recoveries = publication.bound_surface_recoveries;
    }

    pub(super) fn settle_semantic_surface_vacancies(
        &mut self,
        _input: InputSequence,
        authorities: &[SurfaceVacancyAuthority],
    ) -> Result<(), EngineError> {
        for authority in authorities {
            let surface = authority.surface();
            if self
                .bound_surface_recoveries
                .get(&surface)
                .is_some_and(|bound| Some(bound.binding) == authority.binding())
            {
                self.bound_surface_recoveries.remove(&surface);
            }
        }
        let referenced_anchors = self
            .bound_surface_recoveries
            .values()
            .map(|bound| bound.obligation.target().anchor())
            .chain(
                self.viewport
                    .native_create_sagas()
                    .map(|(_, saga)| saga.prepared().recovery_obligation().target().anchor()),
            )
            .collect::<BTreeSet<_>>();
        for authority in authorities {
            let surface = authority.surface();
            let Some(anchor) = self.root_recovery_anchors.get(&surface).copied() else {
                continue;
            };
            if referenced_anchors.contains(&anchor) {
                return Err(EngineError::ReductionCauseInvariant {
                    detail: "tick-final vacancy retained a referenced root recovery anchor",
                });
            }
            self.root_recovery_anchors.remove(&surface);
        }
        Ok(())
    }

    pub(super) fn command_application_from_report(
        input: InputSequence,
        report: crate::transaction::TransactionReport,
    ) -> Result<CommandApplication, EngineError> {
        let changed = report.changed();
        let outcome = report
            .into_outcomes()
            .into_iter()
            .next()
            .ok_or(EngineError::MissingCommandOutcome { input })?;
        Ok(CommandApplication::Applied { outcome, changed })
    }

    pub(super) fn reduce_workspace_command(
        &mut self,
        input: InputSequence,
        expected: WorkspaceVersion,
        application_base: WorkspaceVersion,
        command: &WorkspaceCommand,
        policy: &DockPolicySnapshot,
        events: &mut Vec<WorkspaceEvent>,
        interaction_events: &mut Vec<InteractionEvent>,
    ) -> Result<InputOutcome, EngineError> {
        if expected != application_base {
            return Ok(InputOutcome::StaleRejected {
                expected,
                accepted_base: application_base,
            });
        }

        let (candidate, application) =
            self.stage_workspace_command(input, policy, command, None)?;
        let CommandApplication::Applied { outcome, changed } = application else {
            let CommandApplication::Rejected(error) = application else {
                unreachable!("command application variants are exhaustive")
            };
            return Ok(InputOutcome::CommandRejected {
                error,
                version: self.version,
            });
        };
        if let Err(error) = self.validate_application_command_identity_freshness(command) {
            return Ok(InputOutcome::CommandRejected {
                error,
                version: self.version,
            });
        }
        self.adopt_application_command_identities(command);
        self.publish_workspace(candidate.ok_or(EngineError::MissingCommandOutcome { input })?);
        self.reconcile_viewport_focus_authority();
        if changed {
            if let WorkspaceCommand::Select { source } = command {
                self.advance_selection_revision(input, source)?;
            } else {
                self.advance_revision(input)?;
            }
            if !matches!(
                command,
                WorkspaceCommand::AttachEmptySurface { .. }
                    | WorkspaceCommand::RetireEmptySurface { .. }
            ) {
                self.invalidate_transient(
                    input,
                    InteractionCancelReason::WorkspaceChanged,
                    interaction_events,
                )?;
            }
        }
        if changed {
            events.push(WorkspaceEvent::new(
                input,
                self.version,
                WorkspaceEventKind::CommandCommitted(outcome.clone()),
            ));
        }
        Ok(InputOutcome::CommandProcessed {
            outcome,
            changed,
            version: self.version,
        })
    }
}
