//! Ordered host-frame input reduction and rollback-candidate publication.

use super::*;

/// Input after assignment by the engine's sole sequence writer.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct SequencedInput {
    stamp: CoreCausalStamp,
    sequence: InputSequence,
    source: StableInputSourceId,
    source_sequence: SourceSequence,
    input: EngineInput,
}

impl SequencedInput {
    pub(super) const fn new(
        stamp: CoreCausalStamp,
        sequence: InputSequence,
        source: StableInputSourceId,
        source_sequence: SourceSequence,
        input: EngineInput,
    ) -> Self {
        Self {
            stamp,
            sequence,
            source,
            source_sequence,
            input,
        }
    }

    pub(super) const fn cause(&self, tick: ReducerTickId) -> ReductionCause {
        ReductionCause::Input {
            tick,
            ordinal: self.stamp.ordinal(),
            input: self.sequence,
            source: self.source,
            source_sequence: self.source_sequence,
        }
    }
}

#[derive(Debug)]
pub(super) struct PreparedHostPointerProtocol {
    stamp: CoreCausalStamp,
    provider: PointerInputLease,
    journal: PointerEdgeJournal,
    candidates: PointerReceiverCandidateRoster,
    receipts: PointerReceiverReceiptBatch,
}

impl PreparedHostPointerProtocol {
    #[allow(clippy::type_complexity)]
    pub(super) fn into_parts(
        self,
    ) -> (
        CoreCausalStamp,
        PointerInputLease,
        PointerEdgeJournal,
        PointerReceiverCandidateRoster,
        PointerReceiverReceiptBatch,
    ) {
        (
            self.stamp,
            self.provider,
            self.journal,
            self.candidates,
            self.receipts,
        )
    }
}

#[derive(Debug)]
pub(super) struct HostPointerProtocolSegment {
    stamp: CoreCausalStamp,
    provider: PointerInputLease,
    journal: PointerEdgeJournal,
    candidates: PointerReceiverCandidateRoster,
}

impl HostPointerProtocolSegment {
    pub(super) const fn new(
        stamp: CoreCausalStamp,
        provider: PointerInputLease,
        journal: PointerEdgeJournal,
        candidates: PointerReceiverCandidateRoster,
    ) -> Self {
        Self {
            stamp,
            provider,
            journal,
            candidates,
        }
    }

    pub(super) const fn candidates(&self) -> &PointerReceiverCandidateRoster {
        &self.candidates
    }

    pub(super) fn into_prepared(
        self,
        receipts: PointerReceiverReceiptBatch,
    ) -> PreparedHostPointerProtocol {
        PreparedHostPointerProtocol {
            stamp: self.stamp,
            provider: self.provider,
            journal: self.journal,
            candidates: self.candidates,
            receipts,
        }
    }
}

#[derive(Debug)]
pub(super) struct HostBackendIngressCursor {
    batch: BackendIngressBatch,
    next_record: usize,
}

impl HostBackendIngressCursor {
    pub(super) const fn new(batch: BackendIngressBatch) -> Self {
        Self {
            batch,
            next_record: 0,
        }
    }

    pub(super) fn next(&mut self) -> Option<(BackendIngressOrdinal, BackendIngressPayload)> {
        let record = self.batch.records().get(self.next_record)?;
        self.next_record += 1;
        Some((record.ordinal(), record.payload().clone()))
    }

    pub(super) fn into_batch(self) -> BackendIngressBatch {
        self.batch
    }
}

impl DockEngine {
    pub(super) fn prepare_host_frame<'a>(
        &'a mut self,
        frame: CoreHostFrame,
    ) -> Result<PreparedHostFrameCommit<'a>, EngineError> {
        let prepared = self.prepare_host_frame_owned(frame)?;
        let OwnedPreparedHostFrameCommit {
            fence: _,
            candidate,
            transition,
        } = prepared;
        Ok(PreparedHostFrameCommit {
            engine: self,
            candidate,
            transition,
        })
    }

    pub(super) fn prepare_host_frame_owned(
        &self,
        frame: CoreHostFrame,
    ) -> Result<OwnedPreparedHostFrameCommit, EngineError> {
        let CoreHostFrame {
            authority_domain,
            presentation_host,
            workspace,
            requirements,
            predecessor_tick,
            presentation_host_frontier,
            platform_provider_frontier,
            tick,
            admission_surface_scope,
            frozen_presentation_roster,
            presentation_obligations,
            backend_ingress,
            backend_ingress_batch_submitted,
            backend_ingress_complete,
            pending_backend_ingress,
            pointer_provider,
            staged_pointer_journal: _,
            frozen_pointer_outputs: _,
            frozen_pointer_presentations: _,
            frozen_semantic_presentations: _,
            pointer_receiver_attempt_issuer: _,
            pending_pointer_segment,
            pointer_segment_submitted,
            presentation_scopes,
            item_identity_scope,
            presentation_snapshot_changed: _,
            presentation_projection_changed: _,
            candidate,
            tick_policy,
            vacancy_ledger,
            application_base,
            presentation_observation_outcomes,
            reduced_inputs,
            reduced_pointer_edges,
            events,
            interaction_events,
            last_reduced_input,
            last_causal_cause,
            configuration_inputs,
            staged_presentation_outputs,
            staged_presentation_output_surfaces: _,
            surface_contributions,
            presentation_phase_started: _,
            configuration_phase_started: _,
            next_causal_ordinal: _,
            poison,
            input_prefix_error,
        } = frame;
        let presentation_surface_scope = frozen_presentation_roster.surface_scope();
        if let Some(source) = input_prefix_error {
            return Err(source);
        }
        if let Some(source) = poison {
            return Err(EngineError::HostFramePoisoned { source });
        }
        if let Some(scope) = &item_identity_scope
            && let Some(item) = candidate
                .workspace()
                .item_multiset()
                .into_keys()
                .find(|item| !scope.contains(item))
        {
            return Err(EngineError::HostFramePoisoned {
                source: CoreHostFrameError::ItemIdentityOutsideScope { item },
            });
        }
        if authority_domain != self.authority_domain {
            return Err(EngineError::HostFrameAuthorityDomainMismatch {
                expected: self.authority_domain,
                submitted: authority_domain,
            });
        }
        let current_host_frontier = self.presentation_authority.presentation.host_frontier();
        if presentation_host_frontier != current_host_frontier {
            return Err(EngineError::HostFramePresentationHostFrontierStale {
                submitted: presentation_host_frontier,
                current: current_host_frontier,
            });
        }
        let current_platform_provider_frontier = self.viewport.platform_provider_frontier();
        if platform_provider_frontier != current_platform_provider_frontier {
            return Err(EngineError::HostFramePlatformProviderFrontierStale {
                submitted: platform_provider_frontier.get(),
                current: current_platform_provider_frontier.get(),
            });
        }
        let active_pointer_provider = self.pointer_journal.active_lease();
        if pointer_provider != active_pointer_provider {
            return Err(EngineError::HostFramePointerProviderStale {
                submitted: pointer_provider,
                current: active_pointer_provider,
            });
        }
        if let Some(provider) = pointer_provider {
            self.validate_pointer_provider_scope(provider.scope())?;
        }
        let active_backend_ingress = self.backend_ingress.active();
        if backend_ingress != active_backend_ingress {
            let ingress = backend_ingress
                .or(active_backend_ingress)
                .expect("backend ingress mismatch requires one provider");
            return Err(EngineError::BackendIngressProviderMismatch {
                ingress,
                platform: self.platform_provider(),
                pointer: active_pointer_provider,
            });
        }
        match backend_ingress {
            Some(_) => {
                if !backend_ingress_batch_submitted
                    || !backend_ingress_complete
                    || pending_backend_ingress.is_some()
                {
                    return Err(EngineError::HostFrameBackendIngressIncomplete);
                }
            }
            None => {
                if backend_ingress_batch_submitted
                    || backend_ingress_complete
                    || pending_backend_ingress.is_some()
                {
                    return Err(EngineError::HostFrameBackendIngressUnexpected);
                }
            }
        }
        match pointer_provider {
            None => {
                if pointer_segment_submitted || pending_pointer_segment.is_some() {
                    return Err(EngineError::ReductionCauseInvariant {
                        detail: "host frame staged pointer protocol state without a frozen provider",
                    });
                }
            }
            Some(provider) => {
                if !pointer_segment_submitted {
                    return Err(EngineError::HostFramePointerJournalMissing { provider });
                }
                if pending_pointer_segment.is_some() {
                    return Err(EngineError::HostFramePointerReceiverReceiptsMissing { provider });
                }
            }
        }
        // Terminal host identity is stronger than ordinary frame staleness. A
        // capability frozen before destruction must diagnose the destroyed
        // participant rather than merely report that some later tick exists.
        for host in presentation_scopes.keys().copied() {
            self.presentation_authority
                .presentation
                .validate_lease(host)
                .map_err(presentation_ledger_error)?;
        }
        if predecessor_tick != self.last_reducer_tick {
            return Err(EngineError::HostFramePredecessorStale {
                submitted: predecessor_tick,
                current: self.last_reducer_tick,
            });
        }
        if candidate.authority_domain != authority_domain
            || candidate.last_reducer_tick != tick
            || predecessor_tick.checked_next() != Some(tick)
        {
            return Err(EngineError::ReductionCauseInvariant {
                detail: "sealed host-frame candidate does not match its predecessor",
            });
        }
        let current_requirements = self
            .presentation_authority
            .presentation_requirements
            .revision();
        if workspace != self.version || requirements != current_requirements {
            return Err(EngineError::HostFrameStale {
                submitted_workspace: workspace,
                current_workspace: self.version,
                submitted_requirements: requirements,
                current_requirements,
            });
        }
        let current_scope = self
            .presentation_authority
            .presentation_requirements
            .surfaces()
            .map(|(surface, _)| surface)
            .collect::<BTreeSet<_>>();
        if admission_surface_scope != current_scope {
            return Err(EngineError::HostFrameRosterStale {
                submitted: admission_surface_scope.into_iter().collect(),
                current: current_scope.into_iter().collect(),
            });
        }
        for (host, scope) in presentation_scopes {
            let current_scope = self
                .presentation_authority
                .presentation
                .pending_stream_scope(host)
                .map_err(presentation_ledger_error)?;
            if scope != current_scope {
                if host == presentation_host {
                    return Err(EngineError::HostFramePresentationScopeStale {
                        submitted: scope.into_iter().collect(),
                        current: current_scope.into_iter().collect(),
                    });
                }
                return Err(EngineError::HostFrameSupplementaryPresentationScopeStale {
                    host,
                    submitted: scope.into_iter().collect(),
                    current: current_scope.into_iter().collect(),
                });
            }
        }
        if let Some((expected, resolved)) = presentation_obligations.completion() {
            return Err(
                EngineError::HostFramePresentationObligationRosterIncomplete { expected, resolved },
            );
        }
        let presentation_attempt = presentation_obligations.attempt();
        let presentation_dispositions = presentation_obligations.dispositions();
        let submitted_contribution_surfaces = surface_contributions
            .iter()
            .map(PreparedSurfaceContribution::surface)
            .collect::<BTreeSet<_>>();
        if submitted_contribution_surfaces != presentation_surface_scope {
            return Err(EngineError::HostFrameContributionRosterIncomplete {
                expected: presentation_surface_scope.into_iter().collect(),
                submitted: submitted_contribution_surfaces.into_iter().collect(),
            });
        }
        let fence = self.host_frame_commit_fence();
        let (candidate, transition) = self.reduce_tick_candidate(
            candidate,
            tick,
            presentation_host,
            predecessor_tick,
            presentation_observation_outcomes,
            interaction_events,
            tick_policy,
            vacancy_ledger,
            application_base,
            reduced_inputs,
            reduced_pointer_edges,
            events,
            last_reduced_input,
            last_causal_cause,
            configuration_inputs,
            presentation_attempt,
            presentation_dispositions,
            staged_presentation_outputs,
            surface_contributions,
        )?;
        Ok(OwnedPreparedHostFrameCommit {
            fence,
            candidate,
            transition,
        })
    }

    fn host_frame_commit_fence(&self) -> HostFrameCommitFence {
        HostFrameCommitFence {
            authority_domain: self.authority_domain,
            predecessor_tick: self.last_reducer_tick,
            workspace: self.version,
            requirements: self
                .presentation_authority
                .presentation_requirements
                .revision(),
            presentation_host_frontier: self.presentation_authority.presentation.host_frontier(),
            platform_provider_frontier: self.viewport.platform_provider_frontier(),
            pointer_provider: self.pointer_journal.active_lease(),
            backend_ingress: self.backend_ingress.active(),
        }
    }

    pub(super) fn validate_host_frame_commit_fence(
        &self,
        fence: HostFrameCommitFence,
    ) -> Result<(), EngineError> {
        if fence.authority_domain != self.authority_domain {
            return Err(EngineError::HostFrameAuthorityDomainMismatch {
                expected: self.authority_domain,
                submitted: fence.authority_domain,
            });
        }
        if fence.predecessor_tick != self.last_reducer_tick {
            return Err(EngineError::HostFramePredecessorStale {
                submitted: fence.predecessor_tick,
                current: self.last_reducer_tick,
            });
        }
        let current_host_frontier = self.presentation_authority.presentation.host_frontier();
        if fence.presentation_host_frontier != current_host_frontier {
            return Err(EngineError::HostFramePresentationHostFrontierStale {
                submitted: fence.presentation_host_frontier,
                current: current_host_frontier,
            });
        }
        let current_platform_provider_frontier = self.viewport.platform_provider_frontier();
        if fence.platform_provider_frontier != current_platform_provider_frontier {
            return Err(EngineError::HostFramePlatformProviderFrontierStale {
                submitted: fence.platform_provider_frontier.get(),
                current: current_platform_provider_frontier.get(),
            });
        }
        let current_pointer_provider = self.pointer_journal.active_lease();
        if fence.pointer_provider != current_pointer_provider {
            return Err(EngineError::HostFramePointerProviderStale {
                submitted: fence.pointer_provider,
                current: current_pointer_provider,
            });
        }
        let current_backend_ingress = self.backend_ingress.active();
        if fence.backend_ingress != current_backend_ingress {
            let ingress = fence
                .backend_ingress
                .or(current_backend_ingress)
                .expect("backend ingress mismatch requires one provider");
            return Err(EngineError::BackendIngressProviderMismatch {
                ingress,
                platform: self.platform_provider(),
                pointer: current_pointer_provider,
            });
        }
        let current_requirements = self
            .presentation_authority
            .presentation_requirements
            .revision();
        if fence.workspace != self.version || fence.requirements != current_requirements {
            return Err(EngineError::HostFrameStale {
                submitted_workspace: fence.workspace,
                current_workspace: self.version,
                submitted_requirements: fence.requirements,
                current_requirements,
            });
        }
        Ok(())
    }

    pub(super) fn validate_and_advance_source_watermarks(
        &mut self,
        inputs: impl IntoIterator<Item = (StableInputSourceId, SourceSequence)>,
    ) -> Result<(), EngineError> {
        let mut watermarks = self.source_watermarks.clone();
        for (input_source, source_sequence) in inputs {
            if let Some(previous) = watermarks.get(&input_source).copied()
                && source_sequence <= previous
            {
                return Err(EngineError::SourceSequenceNotIncreasing {
                    input_source,
                    previous,
                    submitted: source_sequence,
                });
            }
            watermarks.insert(input_source, source_sequence);
        }
        self.source_watermarks = watermarks;
        Ok(())
    }

    fn reduce_tick_candidate(
        &self,
        mut candidate: Self,
        tick: ReducerTickId,
        presentation_host: PresentationHostLease,
        presentation_predecessor_tick: ReducerTickId,
        presentation_observations: Vec<HostPresentationObservationOutcome>,
        mut interaction_events: Vec<InteractionEvent>,
        tick_policy: DockPolicySnapshot,
        mut vacancy_ledger: TickVacancyLedger,
        mut application_base: WorkspaceVersion,
        mut reduced: Vec<ReducedInput>,
        reduced_pointer_edges: Vec<crate::transition::ReducedPointerEdge>,
        mut events: Vec<WorkspaceEvent>,
        last_reduced_input: Option<InputSequence>,
        last_causal_cause: Option<ReductionCause>,
        configuration_inputs: Vec<SequencedInput>,
        presentation_attempt: HostPresentationAttemptId,
        presentation_dispositions: Vec<HostPresentationDispositionOutcome>,
        staged_presentation_outputs: Vec<StagedPresentationOutput>,
        surface_contributions: Vec<PreparedSurfaceContribution>,
    ) -> Result<(DockEngine, EngineTransition), EngineError> {
        let before = self.version;
        let before_presentation_identity = self.presentation_identity;
        let before_scene = self.presentation_authority.scene.clone();
        let before_interaction = self.interaction.clone();
        let before_pending_drag_release = self.pending_drag_release.clone();
        let before_pending_contained_transform_release =
            self.pending_contained_transform_release.clone();
        let before_close = self.close.clone();
        let before_viewport = self.viewport.clone();
        let before_viewport_focus = self.viewport_focus.clone();
        let tick_start = TickStartAuthority {
            policy: &tick_policy,
            semantic_presentations: None,
        };
        if let Some(input) = last_reduced_input {
            candidate.rebuild_presentation_requirements(input)?;
        }
        if let Some(cause) = last_causal_cause {
            let _ = candidate.reconcile_pointer_provider_scope(cause, &mut interaction_events)?;
            let _ = candidate.cancel_revoked_presentation_caused(cause, &mut interaction_events)?;
            candidate.reconcile_scroll_lifecycle(cause, &mut interaction_events);
        }
        candidate.invalidate_changed_resize_presentation(
            &before_interaction,
            last_reduced_input.unwrap_or(candidate.last_input),
        )?;
        if let Some(cause) = last_causal_cause {
            let _ = candidate.cancel_revoked_presentation_caused(cause, &mut interaction_events)?;
        }

        let before_contribution_interaction = candidate.interaction.clone();
        let before_contribution_authorities = candidate
            .presentation_authority
            .scene
            .interaction_authorities();
        let mut changed_contribution_surfaces = BTreeSet::new();
        let mut contribution_outcomes = Vec::with_capacity(surface_contributions.len());
        for contribution in surface_contributions {
            let outcome =
                candidate.reduce_surface_contribution(tick, contribution, tick_start.policy)?;
            if outcome.changes_surface_authority() {
                changed_contribution_surfaces.insert(outcome.surface());
            }
            contribution_outcomes.push(outcome);
        }
        let after_contribution_authorities = candidate
            .presentation_authority
            .scene
            .interaction_authorities();
        changed_contribution_surfaces.extend(
            before_contribution_authorities
                .keys()
                .chain(after_contribution_authorities.keys())
                .copied()
                .collect::<BTreeSet<_>>()
                .into_iter()
                .filter(|surface| {
                    before_contribution_authorities.get(surface)
                        != after_contribution_authorities.get(surface)
                }),
        );
        candidate.settle_popup_geometry_unavailability_after_surface_contributions(
            tick,
            &contribution_outcomes,
            &mut changed_contribution_surfaces,
        )?;
        candidate.reconcile_interaction_after_surface_contributions(
            tick,
            &changed_contribution_surfaces,
            &mut interaction_events,
        )?;
        candidate.invalidate_changed_resize_presentation(
            &before_contribution_interaction,
            candidate.last_input,
        )?;
        let _ = candidate.cancel_revoked_presentation_caused(
            ReductionCause::SurfaceContributionBatch { tick },
            &mut interaction_events,
        )?;

        let before_configuration_interaction = candidate.interaction.clone();
        let mut last_configuration_cause = None;
        for input in configuration_inputs {
            let cause = input.cause(tick);
            last_configuration_cause = Some(cause);
            let outcome = candidate.reduce_sequenced_input(
                input,
                tick,
                tick_start,
                &mut application_base,
                &mut events,
                &mut interaction_events,
            )?;
            let _ = candidate.reconcile_pointer_provider_scope(cause, &mut interaction_events)?;
            vacancy_ledger.observe_workspace_reconciliation(outcome.outcome());
            vacancy_ledger.observe_bindings(&candidate);
            reduced.push(outcome);
        }
        candidate.invalidate_changed_resize_presentation(
            &before_configuration_interaction,
            candidate.last_input,
        )?;
        if let Some(cause) = last_configuration_cause {
            let _ = candidate.cancel_revoked_presentation_caused(cause, &mut interaction_events)?;
        }

        // Native close decisions are deliberately emitted only after every
        // application, renderer, and configuration input in this host frame
        // has settled. This makes one reducer tick the causal boundary rather
        // than whichever viewport callback happened to run first.
        candidate.schedule_native_surface_close_effects(candidate.last_input)?;

        let held_source_surfaces = candidate.native_admission.held_source_surfaces();
        let mut vacant_surfaces = vacancy_ledger.vacant_authorities(&candidate);
        vacant_surfaces.retain(|authority| !held_source_surfaces.contains(&authority.surface()));
        vacant_surfaces.extend(candidate.take_settleable_native_source_vacancies());
        let final_live_surfaces = candidate
            .workspace
            .surfaces()
            .map(|(surface, _)| surface)
            .collect::<BTreeSet<_>>();
        let vacancy_interaction_dependencies = candidate.platform_interaction_dependencies();
        let mut vacancy_settlement = crate::frame::SurfaceVacancySettlement::default();
        let mut pending_vacancies = vacant_surfaces.clone();
        while !pending_vacancies.is_empty() {
            let settled = candidate
                .viewport
                .settle_surface_vacancies(&pending_vacancies, &final_live_surfaces)
                .map_err(|source| EngineError::Viewport {
                    input: candidate.last_input,
                    source,
                })?;
            candidate
                .settle_vacated_native_first_live_barriers(settled.retired_native_creates())?;
            vacancy_settlement.extend(settled);
            pending_vacancies = candidate.take_settleable_native_source_vacancies();
            vacant_surfaces.extend(pending_vacancies.iter().copied());
        }
        candidate.settle_semantic_surface_vacancies(candidate.last_input, &vacant_surfaces)?;
        let owner_vacated = vacancy_settlement
            .logical_vacated_bindings()
            .iter()
            .any(|binding| {
                vacancy_interaction_dependencies
                    .owner_bindings
                    .contains(binding)
            });
        let target_vacated = vacancy_settlement
            .logical_vacated_bindings()
            .iter()
            .any(|binding| {
                vacancy_interaction_dependencies
                    .target_bindings
                    .contains(binding)
            });
        if owner_vacated {
            candidate.invalidate_transient(
                candidate.last_input,
                InteractionCancelReason::SurfaceClosed,
                &mut interaction_events,
            )?;
        } else if target_vacated {
            candidate.clear_platform_drag_target(
                candidate.last_input,
                before != candidate.version,
                false,
                &mut interaction_events,
            )?;
        }
        if let Some(cause) = last_configuration_cause.or(last_causal_cause) {
            let _ = candidate.reconcile_pointer_provider_scope(cause, &mut interaction_events)?;
        }
        candidate.reconcile_viewport_focus_authority();
        candidate.settle_native_activation_reservations(&mut events)?;
        candidate.emit_pending_platform_focus_effect(candidate.last_input)?;

        let platform_effects = candidate
            .viewport
            .try_take_new_effects()
            .map_err(|source| EngineError::Viewport {
                input: candidate.last_input,
                source,
            })?;
        candidate
            .record_emitted_native_surface_close_effects(candidate.last_input, &platform_effects)?;
        debug_assert!(vacancy_settlement.effects().iter().all(|effect| {
            platform_effects
                .iter()
                .any(|request| request.id() == *effect)
        }));
        let presentation_emissions = candidate.emit_staged_presentation_outputs(
            presentation_host,
            presentation_predecessor_tick,
            presentation_attempt,
            staged_presentation_outputs,
        )?;
        candidate.settle_retired_presentation_hosts()?;
        let observed_focus_effects = Self::observed_focus_effects(&reduced);
        let focus_delta = FocusDelta::between(
            &before_viewport_focus,
            &candidate.viewport_focus,
            before_viewport.effects(),
            candidate.viewport.effects(),
            &observed_focus_effects,
        );
        candidate.viewport_focus.mark_boundary_published();
        let published_state_changed = before != candidate.version
            || before_presentation_identity != candidate.presentation_identity
            || before_scene != candidate.presentation_authority.scene
            || before_interaction != candidate.interaction
            || before_pending_drag_release != candidate.pending_drag_release
            || before_pending_contained_transform_release
                != candidate.pending_contained_transform_release
            || before_close != candidate.close
            || before_viewport != candidate.viewport
            || !focus_delta.is_empty();
        let surface_scene_deltas =
            Self::surface_scene_deltas(&before_scene, &candidate.presentation_authority.scene);
        let transition = EngineTransition::new(EngineTransitionParts {
            tick,
            before,
            after: candidate.version,
            reduced,
            reduced_pointer_edges,
            events,
            interaction_events,
            platform_effects,
            focus_delta,
            presentation_observations,
            presentation_emissions,
            presentation_dispositions,
            surface_contributions: contribution_outcomes,
            surface_scene_deltas,
            published_state_changed,
        });
        Ok((candidate, transition))
    }

    fn emit_staged_presentation_outputs(
        &mut self,
        presentation_host: PresentationHostLease,
        predecessor_tick: ReducerTickId,
        expected_attempt: HostPresentationAttemptId,
        mut staged_outputs: Vec<StagedPresentationOutput>,
    ) -> Result<Vec<HostPresentationEmission>, EngineError> {
        staged_outputs.sort_by_key(|staged| staged.request.ordinal());
        let mut emissions = Vec::with_capacity(staged_outputs.len());
        for staged in staged_outputs {
            if staged.request.host() != presentation_host
                || staged.request.predecessor_tick() != predecessor_tick
                || staged.request.attempt() != expected_attempt
            {
                return Err(EngineError::PresentationLedger {
                    detail: "presentation output request belongs to another host frame".to_owned(),
                });
            }
            let output = self
                .presentation_authority
                .presentation
                .emit(
                    presentation_host,
                    staged.surface,
                    staged.endpoint,
                    staged.payload,
                )
                .map_err(presentation_ledger_error)?;
            self.bind_pending_release_outputs(output)?;
            emissions.push(HostPresentationEmission::new(staged.request, output));
        }
        Ok(emissions)
    }

    fn bind_pending_release_outputs(
        &mut self,
        output: crate::presentation_observation::HostPresentationOutput,
    ) -> Result<(), EngineError> {
        let Some(interaction) = output.payload().interaction() else {
            return Ok(());
        };
        if let Some(pending) = self.pending_drag_release.as_mut()
            && interaction.drag_preview() == Some(pending.preview)
        {
            let preview_surface = pending
                .drag
                .preview
                .as_ref()
                .expect("pending release retains its exact preview")
                .public()
                .visual()
                .surface();
            if output.surface() != preview_surface {
                return Err(EngineError::PresentationLedger {
                    detail: "release preview output was emitted on the wrong surface".to_owned(),
                });
            }
            pending.presentation_outputs.insert(output.key());
        }
        if let Some(pending) = self.pending_contained_transform_release.as_mut()
            && interaction.contained_transform_preview() == Some(pending.preview)
        {
            let preview_surface = pending
                .transform
                .preview
                .as_ref()
                .expect("pending contained release retains its exact preview")
                .public()
                .surface();
            if output.surface() != preview_surface {
                return Err(EngineError::PresentationLedger {
                    detail: "contained release preview output was emitted on the wrong surface"
                        .to_owned(),
                });
            }
            pending.presentation_outputs.insert(output.key());
        }
        Ok(())
    }

    pub(super) fn reduce_sequenced_input(
        &mut self,
        input: SequencedInput,
        tick: ReducerTickId,
        tick_start: TickStartAuthority<'_>,
        application_base: &mut WorkspaceVersion,
        events: &mut Vec<WorkspaceEvent>,
        interaction_events: &mut Vec<InteractionEvent>,
    ) -> Result<ReducedInput, EngineError> {
        let priority = input.input.priority();
        let cause = input.cause(tick);
        let workspace_event_start = events.len();
        let interaction_event_start = interaction_events.len();
        let outcome = self.reduce_one(
            &input,
            cause,
            tick_start,
            application_base,
            events,
            interaction_events,
        )?;
        self.cancel_invalid_pending_release_obligations_caused(cause, interaction_events);
        if matches!(&input.input, EngineInput::ReplacePresentationConfig { .. }) {
            let _ = self.cancel_pending_release_obligations_caused(
                cause,
                InteractionCancelReason::SceneUnavailable,
                interaction_events,
            );
        }
        let _ = self.cancel_revoked_presentation_input(input.sequence, interaction_events)?;
        if events[workspace_event_start..]
            .iter_mut()
            .any(|event| !event.bind_input(cause))
        {
            return Err(EngineError::ReductionCauseInvariant {
                detail: "workspace input event did not match its reducer input",
            });
        }
        if interaction_events[interaction_event_start..]
            .iter_mut()
            .any(|event| !event.bind_input(cause))
        {
            return Err(EngineError::ReductionCauseInvariant {
                detail: "interaction input event did not match its reducer input",
            });
        }
        Ok(ReducedInput::new(
            tick,
            input.stamp.ordinal(),
            input.sequence,
            input.source,
            input.source_sequence,
            priority,
            outcome,
        ))
    }

    fn observed_focus_effects(reduced: &[ReducedInput]) -> Vec<ObservedPlatformFocusEffect> {
        reduced
            .iter()
            .flat_map(|input| {
                match input.outcome() {
                    InputOutcome::PlatformSnapshotPublished { focus, .. } => focus
                        .effect_settlement()
                        .map_or(&[][..], |settlement| settlement.observed_effects()),
                    InputOutcome::ViewportRegistered { .. }
                    | InputOutcome::ViewportRegistrationRejected { .. }
                    | InputOutcome::NativeCloseObservationPublished { .. }
                    | InputOutcome::PlatformSnapshotStale { .. }
                    | InputOutcome::PlatformEffectReported { .. }
                    | InputOutcome::ViewportActivationRequested { .. }
                    | InputOutcome::ViewportActivationRejected { .. }
                    | InputOutcome::PaneFocusObservationPublished { .. }
                    | InputOutcome::PaneFocusObservationStale { .. }
                    | InputOutcome::NativeCreateCancelled { .. }
                    | InputOutcome::ViewportCleanupRetried { .. }
                    | InputOutcome::WorkspaceReplaced { .. }
                    | InputOutcome::CommandProcessed { .. }
                    | InputOutcome::CommandRejected { .. }
                    | InputOutcome::ContentCloseRequested { .. }
                    | InputOutcome::ContentCloseRejected { .. }
                    | InputOutcome::SurfaceCloseRequested { .. }
                    | InputOutcome::SurfaceCloseRejected { .. }
                    | InputOutcome::SurfaceCloseCancellationRequested { .. }
                    | InputOutcome::CloseDecisionProcessed { .. }
                    | InputOutcome::PolicyReplaced { .. }
                    | InputOutcome::PresentationConfigReplaced { .. }
                    | InputOutcome::InteractionProcessed { .. }
                    | InputOutcome::WorkspaceValidated { .. }
                    | InputOutcome::PlatformProviderRejected { .. }
                    | InputOutcome::StaleRejected { .. } => &[],
                }
                .iter()
                .copied()
            })
            .collect()
    }

    // This is the sole sorted-input dispatch table; keeping every input variant visible here
    // makes reducer ordering auditable and prevents hidden secondary dispatch.
    #[allow(clippy::too_many_lines)]
    fn reduce_one(
        &mut self,
        input: &SequencedInput,
        cause: ReductionCause,
        tick_start: TickStartAuthority<'_>,
        application_base: &mut WorkspaceVersion,
        events: &mut Vec<WorkspaceEvent>,
        interaction_events: &mut Vec<InteractionEvent>,
    ) -> Result<InputOutcome, EngineError> {
        let focus_generation = self.last_focus_reducer_generation.checked_next().ok_or(
            EngineError::ViewportFocus {
                input: input.sequence,
                source: ViewportFocusError::ReducerGenerationExhausted,
            },
        )?;
        self.last_focus_reducer_generation = focus_generation;
        let focus_causal = FocusCausalStamp::new(focus_generation, cause);
        match &input.input {
            EngineInput::RegisterViewport {
                provider,
                expected,
                surface,
                token,
                role,
                recovery_target,
            } => self.reduce_viewport_registration(
                input.sequence,
                *provider,
                *expected,
                *surface,
                *token,
                *role,
                *recovery_target,
            ),
            EngineInput::BootstrapChildViewport {
                provider,
                expected,
                surface,
                token,
                recovery,
            } => self.reduce_child_viewport_bootstrap_registration(
                input.sequence,
                *provider,
                *expected,
                *surface,
                *token,
                *recovery,
            ),
            EngineInput::PublishPlatformSnapshot {
                provider,
                expected_epoch,
                snapshot,
            } => self.reduce_platform_snapshot(
                input.sequence,
                *provider,
                *expected_epoch,
                snapshot,
                focus_causal,
                PlatformSnapshotReductionContext {
                    application_base,
                    events,
                    interaction_events,
                },
            ),
            EngineInput::PublishNativeCloseObservation {
                provider,
                expected_epoch,
                observation,
            } => self.reduce_native_close_observation(
                input.sequence,
                *provider,
                *expected_epoch,
                *observation,
                focus_causal,
                PlatformSnapshotReductionContext {
                    application_base,
                    events,
                    interaction_events,
                },
            ),
            EngineInput::ReportPlatformEffect {
                provider,
                expected_epoch,
                result,
            } => self.reduce_platform_effect(input.sequence, *provider, *expected_epoch, *result),
            EngineInput::ActivateViewport { expected, request } => self.reduce_viewport_activation(
                *expected,
                *request,
                focus_causal,
                *application_base,
                events,
            ),
            EngineInput::PublishPaneFocusObservation {
                expected_epoch,
                observation,
            } => Ok(self.reduce_pane_focus_observation(*expected_epoch, *observation)),
            EngineInput::CancelNativeCreate { expected, saga } => {
                self.reduce_native_create_cancellation(input.sequence, *expected, *saga)
            }
            EngineInput::RetryViewportCleanup {
                expected,
                failed_effect,
            } => self.reduce_viewport_cleanup_retry(input.sequence, *expected, *failed_effect),
            EngineInput::ReplaceWorkspace(workspace) => self.reduce_workspace_replacement(
                input.sequence,
                workspace,
                None,
                application_base,
                events,
                interaction_events,
            ),
            EngineInput::RestoreWorkspace(restore) => self.reduce_workspace_replacement(
                input.sequence,
                restore.workspace(),
                Some(restore.presentation_identity_frontier()),
                application_base,
                events,
                interaction_events,
            ),
            EngineInput::WorkspaceCommand { expected, command } => self.reduce_workspace_command(
                input.sequence,
                *expected,
                *application_base,
                command,
                tick_start.policy,
                events,
                interaction_events,
            ),
            EngineInput::RequestContentClose { expected, target } => self
                .reduce_content_close_request(
                    input.sequence,
                    *expected,
                    *target,
                    *application_base,
                    tick_start.policy,
                ),
            EngineInput::RequestSceneClose {
                expected,
                scene,
                target,
            } => self.reduce_scene_close_input(
                input.sequence,
                *expected,
                *scene,
                *target,
                tick_start.policy,
            ),
            EngineInput::ActivateSemanticReceiver { expected, event } => self
                .reduce_semantic_receiver_input(
                    input.sequence,
                    *expected,
                    *application_base,
                    *event,
                    tick_start.semantic_presentations,
                    tick_start.policy,
                    events,
                    interaction_events,
                ),
            EngineInput::AdjustSplitterResize {
                expected,
                scene,
                splitter,
                delta,
            } => self.reduce_splitter_adjustment_input(
                input.sequence,
                *expected,
                *scene,
                *splitter,
                *delta,
                tick_start.policy,
                events,
                interaction_events,
            ),
            EngineInput::ActivateTabStripControl { prepared } => {
                self.reduce_prepared_tab_strip_control(cause, prepared)
            }
            EngineInput::ActivateTabListMenuRow { prepared } => self
                .reduce_prepared_tab_list_menu_row(
                    cause,
                    focus_causal,
                    prepared,
                    tick_start.policy,
                    events,
                ),
            EngineInput::DismissTabListMenu { prepared } => {
                self.reduce_prepared_tab_list_menu_dismiss(cause, prepared)
            }
            EngineInput::AdjustTabStripScroll { prepared } => {
                self.reduce_prepared_tab_strip_scroll(cause, prepared)
            }
            EngineInput::AdjustTabListMenuScroll { prepared } => {
                self.reduce_prepared_tab_list_menu_scroll(cause, prepared)
            }
            EngineInput::NavigateTabListMenu { prepared } => {
                self.reduce_prepared_tab_list_menu_navigation(cause, prepared)
            }
            EngineInput::ApplyContainedPlacement {
                expected,
                root,
                floating,
                expected_rect,
                placement,
            } => self.reduce_contained_placement_input(
                input.sequence,
                *expected,
                ContainedPlacementInput {
                    root: *root,
                    floating: *floating,
                    expected_rect: *expected_rect,
                    placement: *placement,
                },
                tick_start.policy,
                events,
                interaction_events,
            ),
            EngineInput::CancelActiveInteractionWithEscape { expected, delivery } => {
                self.reduce_escape_input(input.sequence, *expected, *delivery, interaction_events)
            }
            EngineInput::AcknowledgePreview {
                expected,
                acknowledgement,
            } => self.reduce_preview_acknowledgement(*expected, acknowledgement),
            EngineInput::AcknowledgeContainedTransformPreview {
                expected,
                acknowledgement,
            } => {
                self.reduce_contained_transform_preview_acknowledgement(*expected, *acknowledgement)
            }
            EngineInput::RequestSurfaceClose {
                expected,
                edge,
                request,
            } => self.reduce_surface_close_request(
                input.sequence,
                *expected,
                *edge,
                request.clone(),
                *application_base,
                tick_start.policy,
            ),
            EngineInput::CancelSurfaceClose { expected, edge } => self
                .reduce_surface_close_cancellation(
                    input.sequence,
                    *expected,
                    *edge,
                    *application_base,
                ),
            EngineInput::ResolveClose {
                request,
                token,
                decision,
            } => self.reduce_close_decision(
                input.sequence,
                *request,
                *token,
                *decision,
                events,
                interaction_events,
            ),
            EngineInput::ContinueDeferredClose {
                request,
                token,
                decision,
            } => self.reduce_deferred_close_decision(
                input.sequence,
                *request,
                *token,
                *decision,
                events,
                interaction_events,
            ),
            EngineInput::ReplacePolicy { expected, policy } => self.reduce_policy_replacement(
                input.sequence,
                *expected,
                *application_base,
                policy,
                events,
                interaction_events,
            ),
            EngineInput::ReplacePresentationConfig { expected, config } => self
                .reduce_presentation_config_replacement(
                    input.sequence,
                    *expected,
                    *application_base,
                    config,
                    interaction_events,
                ),
            EngineInput::ValidateWorkspace => {
                self.workspace
                    .validate()
                    .map_err(EngineError::InvalidWorkspace)?;
                Ok(InputOutcome::WorkspaceValidated {
                    version: self.version,
                })
            }
        }
    }
}
