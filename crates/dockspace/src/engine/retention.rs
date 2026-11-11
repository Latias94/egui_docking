//! Core-owned settlement of runtime retention obligations.

use super::*;

impl DockEngine {
    pub(super) fn retained_presentation_streams(&self) -> BTreeSet<HostPresentationStreamId> {
        self.retained_presentation_emissions()
            .into_iter()
            .map(HostFrameKey::stream)
            .collect()
    }

    /// Derives every concrete emission still referenced by core presentation authority.
    pub(super) fn retained_presentation_emissions(&self) -> BTreeSet<HostFrameKey> {
        let mut emissions = self
            .presentation_authority
            .presentation
            .pending_output_keys()
            .chain(
                self.presentation_authority
                    .scene
                    .retained_interaction_emissions(),
            )
            .collect::<BTreeSet<_>>();

        if let Some(active) = self.interaction.active_presentation_authority() {
            emissions.insert(active.presented.emission());
        }
        if let Some(pending) = &self.pending_mutation {
            emissions.insert(pending.source_presentation());
            pending.presentation().retain_emissions(&mut emissions);
        }
        emissions.extend(
            self.viewport
                .retained_native_staging_resources()
                .map(|resource| resource.source_presentation().emission()),
        );
        emissions
    }

    /// Reclaims detailed retired-host state only after every exact core reference disappears.
    ///
    /// Frozen host-frame capabilities are intentionally not blockers. Their core-minted serials
    /// remain represented by the compact retirement ranges and therefore continue to fail closed.
    pub(super) fn settle_retired_presentation_hosts(&mut self) -> Result<usize, EngineError> {
        let retained_streams = self.retained_presentation_streams();
        let backend_host = self
            .backend_ingress
            .active()
            .map(BackendIngressLease::presentation_host);
        let pointer_host = self
            .pointer_journal
            .active_lease()
            .and_then(|lease| lease.scope().local_host());
        let candidates = self
            .presentation_authority
            .presentation
            .detailed_retired_host_rosters()
            .map(|(host, streams)| (host, streams.clone()))
            .collect::<Vec<_>>();
        let mut compacted = 0;

        for (host, streams) in candidates {
            if backend_host == Some(host)
                || pointer_host == Some(host)
                || streams
                    .iter()
                    .any(|stream| retained_streams.contains(stream))
            {
                continue;
            }
            compacted += usize::from(
                self.presentation_authority
                    .presentation
                    .compact_retired_host(host)
                    .map_err(presentation_ledger_error)?,
            );
        }
        Ok(compacted)
    }
}

impl DockEngine {
    /// Returns count-only presentation-ledger diagnostics for conformance tests.
    #[doc(hidden)]
    #[must_use]
    #[cfg(test)]
    pub fn presentation_ledger_diagnostics(&self) -> PresentationLedgerDiagnostics {
        self.presentation_authority.presentation.diagnostics()
    }

    pub(crate) fn is_presentation_endpoint_handoff(
        &self,
        predecessor: HostPresentationStreamId,
        successor: HostPresentationStreamId,
    ) -> bool {
        self.presentation_authority
            .presentation
            .is_endpoint_handoff(predecessor, successor)
    }

    /// Derives the exact concrete presentation emissions whose adapter resources remain live.
    ///
    /// This is the sole reclamation authority for renderer receiver sidecars. In particular, a
    /// frame age, a newer semantic ticket, or a viewport callback disappearing does not make an
    /// older emission reclaimable while core still retains it for settlement or interaction.
    #[must_use]
    pub fn presentation_retention_manifest(&self) -> PresentationRetentionManifest {
        PresentationRetentionManifest::from_resources(
            self.retained_presentation_emissions(),
            self.presentation_authority
                .presentation
                .retained_stream_ids(),
        )
    }

    /// Returns complete core-owned accounting for resources retained across host frames.
    ///
    /// The presentation subset is an executable adapter reclamation allow-list. The remaining
    /// subsets expose every retained structure and the protocol barrier which still prevents its
    /// safe compaction; no entry is hidden behind frame age or a fixed cache capacity.
    #[must_use]
    #[cfg(test)]
    pub fn runtime_retention_manifest(&self) -> RuntimeRetentionManifest {
        RuntimeRetentionManifest::new(
            self.presentation_retention_manifest(),
            self.presentation_authority
                .presentation
                .retention_manifest(),
            self.viewport.effect_retention_manifest(),
            self.close.retention_manifest(),
            self.pointer_journal.retention_manifest(),
            self.viewport.binding_retention_manifest(),
            InputSourceRetentionManifest::new(usize::from(self.semantic_input_watermark.is_some())),
            self.scroll_interaction.retention_manifest(),
        )
    }

    /// Prepares an affine acknowledgement for a renderer that has externally quiesced a retiring
    /// presentation stream.
    ///
    /// The host must stop every path that could submit another observation for `stream` before
    /// calling this method. The returned acknowledgement is consumed by
    /// [`Self::confirm_presentation_stream_quiescence`], which revalidates core retention before
    /// reclaiming the stream record.
    ///
    /// # Errors
    ///
    /// Returns an error when the stream is foreign, still active, unsettled, or lacks a live
    /// same-host successor for its surface.
    #[cfg(test)]
    pub fn prepare_presentation_stream_quiescence(
        &self,
        host: PresentationHostLease,
        stream: HostPresentationStreamId,
    ) -> Result<PresentationStreamQuiescence, EngineError> {
        self.presentation_authority
            .presentation
            .prepare_stream_quiescence(host, stream)
            .map_err(presentation_ledger_error)
    }

    /// Tries to prepare stream reclamation without treating normal lifecycle progress as failure.
    ///
    /// `Ok(None)` means the exact stream is still active, has unsettled output, lacks its
    /// same-host successor, remains referenced by another core authority, or belongs to a host
    /// whose whole retirement path now owns cleanup. Those conditions are expected to change at a
    /// later host boundary. Foreign identities and host mismatches remain typed errors rather than
    /// being hidden as retryable state.
    ///
    /// # Errors
    ///
    /// Returns an error when `host` or `stream` is foreign, unknown, or belongs to a different
    /// host.
    pub fn try_prepare_presentation_stream_quiescence(
        &self,
        host: PresentationHostLease,
        stream: HostPresentationStreamId,
    ) -> Result<Option<PresentationStreamQuiescence>, EngineError> {
        let quiescence = match self
            .presentation_authority
            .presentation
            .prepare_stream_quiescence(host, stream)
        {
            Ok(quiescence) => quiescence,
            Err(
                PresentationLedgerError::HostRetired { .. }
                | PresentationLedgerError::StreamNotRetiring { .. }
                | PresentationLedgerError::StreamHasPendingOutputs { .. }
                | PresentationLedgerError::StreamQuiescenceRequiresSameHostSuccessor { .. },
            ) => return Ok(None),
            Err(source) => return Err(presentation_ledger_error(source)),
        };
        if self.retained_presentation_streams().contains(&stream) {
            return Ok(None);
        }
        Ok(Some(quiescence))
    }

    /// Consumes a renderer's affine quiescence acknowledgement for one retiring stream.
    ///
    /// No age or capacity policy participates in this reclamation. The exact stream remains in
    /// the retention manifest until this acknowledgement is accepted and every core reference has
    /// disappeared.
    ///
    /// # Errors
    ///
    /// Returns an error when core still retains the stream or its ledger state changed after the
    /// acknowledgement was prepared.
    #[cfg(test)]
    pub fn confirm_presentation_stream_quiescence(
        &mut self,
        quiescence: PresentationStreamQuiescence,
    ) -> Result<(), EngineError> {
        self.confirm_presentation_stream_quiescence_batch([quiescence])
    }

    /// Atomically consumes renderer quiescence acknowledgements for retiring streams.
    ///
    /// Every acknowledgement is revalidated against one candidate engine. If any stream changed
    /// after preparation, none of the stream records are reclaimed.
    ///
    /// # Errors
    ///
    /// Returns an error when core still retains any stream, an acknowledgement is duplicated, or
    /// a ledger state changed after the acknowledgement was prepared.
    #[doc(hidden)]
    pub fn confirm_presentation_stream_quiescence_batch(
        &mut self,
        quiescences: impl IntoIterator<Item = PresentationStreamQuiescence>,
    ) -> Result<(), EngineError> {
        let mut candidate = self.candidate();
        let retained_streams = candidate.retained_presentation_streams();
        for quiescence in quiescences {
            candidate
                .presentation_authority
                .presentation
                .compact_quiesced_retiring_stream(quiescence, &retained_streams)
                .map_err(presentation_ledger_error)?;
        }
        candidate.advance_runtime_retention_revision()?;
        self.publish_candidate(candidate);
        Ok(())
    }

    /// Permanently retires one presentation host and all streams it ever owned.
    ///
    /// The core enumerates the host's complete stream roster, discards every
    /// pending output without promotion, releases only active ownership still
    /// pointing at those streams, and revokes matching scene interaction
    /// authority atomically. No rendering host or contribution roster is
    /// required because this is a terminal control boundary rather than a host
    /// frame.
    ///
    /// Repeating retirement is explicitly idempotent: the original tombstone is
    /// returned and the reducer tick does not advance.
    ///
    /// # Errors
    ///
    /// Returns a typed ledger or reducer error when the lease is foreign,
    /// unknown, or the atomic candidate cannot be published.
    #[cfg(test)]
    pub fn retire_presentation_host(
        &mut self,
        host: PresentationHostLease,
        reason: PresentationHostRetirementReason,
    ) -> Result<PresentationHostRetirementOutcome, EngineError> {
        match self
            .presentation_authority
            .presentation
            .retirement_status(host)
            .map_err(presentation_ledger_error)?
        {
            PresentationHostRetirementStatus::Detailed(tombstone) => {
                return Ok(PresentationHostRetirementOutcome::AlreadyRetired { host, tombstone });
            }
            PresentationHostRetirementStatus::Compacted => {
                return Ok(PresentationHostRetirementOutcome::Compacted { host });
            }
            PresentationHostRetirementStatus::Live => {}
        }

        let before = self.version;
        let before_scene = self.presentation_authority.scene.clone();
        let before_viewport = self.viewport.clone();
        let before_viewport_focus = self.viewport_focus.clone();
        let effect_boundary = before_viewport.latest_effect_id();
        let mut candidate = self.candidate();
        let tick = candidate
            .last_reducer_tick
            .checked_next()
            .ok_or(EngineError::ReducerTickExhausted)?;
        candidate.last_reducer_tick = tick;

        let retirement = candidate
            .presentation_authority
            .presentation
            .retire_host(host, tick, reason)
            .map_err(presentation_ledger_error)?;
        candidate.observe_pending_mutation_host_retirement(retirement.retired_outputs());
        let retired_pointer_provider = candidate
            .pointer_journal
            .active_lease()
            .filter(|provider| provider.scope().local_host() == Some(host));
        if let Some(provider) = retired_pointer_provider {
            candidate
                .pointer_journal
                .retire_provider(provider)
                .map_err(|source| EngineError::PointerJournal { source })?;
        }
        let affected_surfaces = candidate
            .presentation_authority
            .scene
            .revoke_interaction_authority_for_streams(retirement.streams());
        let mut interaction_events = Vec::new();
        candidate.reconcile_interaction_after_host_retirement(
            tick,
            host,
            &affected_surfaces,
            &mut interaction_events,
        )?;
        if let Some(provider) = retired_pointer_provider {
            candidate.cancel_retired_pointer_owner(
                ReductionCause::PresentationHostRetirement { tick, host },
                provider,
                InteractionCancelReason::SceneUnavailable,
                &mut interaction_events,
            )?;
        }
        let mut events = Vec::new();
        if candidate.settle_presented_pending_mutation(&mut events, &mut interaction_events)? {
            candidate.rebuild_presentation_requirements(candidate.last_input)?;
        }
        candidate.settle_retired_presentation_hosts()?;
        candidate.reconcile_viewport_focus_authority();

        let platform_effects = candidate
            .viewport
            .try_take_new_effects_after(effect_boundary)
            .map_err(|source| EngineError::Viewport {
                input: candidate.last_input,
                source,
            })?;
        candidate
            .record_emitted_native_surface_close_effects(candidate.last_input, &platform_effects)?;
        let focus_delta = FocusDelta::between(
            &before_viewport_focus,
            &candidate.viewport_focus,
            before_viewport.effects(),
            candidate.viewport.effects(),
            &[],
        );
        let surface_scene_deltas =
            Self::surface_scene_deltas(&before_scene, &candidate.presentation_authority.scene);
        // The durable host tombstone always changes published core state, even
        // when the host never emitted a stream and no scene delta is visible.
        let published_state_changed = true;
        let transition = EngineTransition::new(EngineTransitionParts {
            authority_domain: candidate.authority_domain,
            tick,
            before,
            after: candidate.version,
            reduced: Vec::new(),
            reduced_pointer_edges: Vec::new(),
            events,
            interaction_events,
            platform_effects,
            focus_delta,
            presentation_observations: Vec::new(),
            presentation_emissions: Vec::new(),
            presentation_dispositions: Vec::new(),
            surface_contributions: Vec::new(),
            surface_scene_deltas,
            published_state_changed,
        });
        let outcome = PresentationHostRetirementOutcome::Retired {
            host: retirement.host(),
            reason: retirement.tombstone().reason(),
            retired_stream_count: retirement.streams().len(),
            retired_output_count: retirement.retired_output_count(),
            released_active_surfaces: retirement
                .released_active_surfaces()
                .iter()
                .copied()
                .collect(),
            affected_surfaces: affected_surfaces.iter().copied().collect(),
            transition,
        };
        self.publish_candidate(candidate);
        Ok(outcome)
    }
}
