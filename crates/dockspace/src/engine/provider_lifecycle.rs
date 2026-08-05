//! Provider, pointer, and presentation-host lifecycle operations.

use super::*;

impl DockEngine {
    /// Creates one core-owned lease for an independent presentation host.
    ///
    /// The lease remains valid across reducer ticks and binds future
    /// presentation streams to this exact engine authority domain. It is not a
    /// native-window handle and cannot be reconstructed from an adapter
    /// context or viewport token.
    ///
    /// # Errors
    ///
    /// Returns a typed engine error only if the engine-local host identity
    /// counter cannot advance without wrapping.
    pub fn create_presentation_host(&mut self) -> Result<PresentationHostLease, EngineError> {
        self.presentation_authority
            .presentation
            .create_host()
            .map_err(presentation_ledger_error)
    }

    /// Atomically enrolls the sole native platform and desktop-pointer provider.
    ///
    /// The returned non-cloneable recorder is the only constructor of the
    /// provider's cross-lane capture order. Neither underlying provider becomes
    /// visible if any part of enrollment fails.
    pub fn create_backend_ingress_provider(
        &mut self,
        presentation_host: PresentationHostLease,
        pointer_committed_through: PointerEdgeSequence,
    ) -> Result<BackendIngressRecorder, EngineError> {
        if let Some(provider) = self.pointer_journal.abandoned_surface_local_provider() {
            return Err(EngineError::SurfaceLocalPointerProviderAbandoned { provider });
        }
        let mut candidate = self.candidate();
        candidate
            .presentation_authority
            .presentation
            .validate_lease(presentation_host)
            .map_err(presentation_ledger_error)?;
        let platform = candidate
            .viewport
            .create_platform_provider()
            .map_err(|source| EngineError::PlatformProvider { source })?;
        let pointer = candidate
            .pointer_journal
            .create_provider(
                PointerProviderScope::DesktopGlobal,
                pointer_committed_through,
            )
            .map_err(|source| EngineError::PointerJournal { source })?;
        let recorder = candidate
            .backend_ingress
            .enroll(
                platform,
                pointer,
                presentation_host,
                pointer_committed_through,
            )
            .map_err(|source| EngineError::BackendIngress { source })?;
        self.publish_candidate(candidate);
        Ok(recorder)
    }

    /// Returns the active joined native provider pair, when enrolled.
    #[must_use]
    pub const fn backend_ingress_provider(&self) -> Option<BackendIngressLease> {
        self.backend_ingress.active()
    }

    /// Returns the last backend ordinal published by a complete host frame.
    #[must_use]
    pub const fn backend_ingress_committed_through(&self) -> BackendIngressOrdinal {
        self.backend_ingress.committed_through()
    }

    /// Returns an exact provider-bound proof suitable for recorder prefix reclamation.
    #[must_use]
    pub fn backend_ingress_commit_watermark(&self) -> Option<BackendIngressCommitWatermark> {
        self.backend_ingress.commit_watermark()
    }

    /// Atomically settles one recorder-reclaimed prefix and its binding quiescence facts.
    ///
    /// The affine receipt proves that the sole backend recorder reclaimed only a
    /// core-committed prefix. Every binding named by that prefix must still have
    /// a destroyed-binding guard owned by the same platform provider. A failed
    /// validation publishes nothing and leaves the receipt available for an
    /// exact retry.
    ///
    /// # Errors
    ///
    /// Returns an error when the receipt is consumed, belongs to another active
    /// backend, no longer ends at the exact core commit boundary, or names a
    /// binding guard that is absent or owned by another platform provider.
    pub fn settle_backend_ingress_prefix_retirement(
        &mut self,
        receipt: &mut BackendIngressPrefixRetirementReceipt,
    ) -> Result<Vec<ViewportBinding>, EngineError> {
        let mut candidate = self.candidate();
        candidate
            .backend_ingress
            .commit_prefix_retirement(receipt)
            .map_err(|source| EngineError::BackendIngress { source })?;
        let provider = receipt
            .lease()
            .ok_or(EngineError::BackendIngress {
                source: BackendIngressError::PrefixRetirementReceiptConsumed,
            })?
            .platform_provider();
        let bindings = receipt.binding_quiescences().to_vec();
        for binding in &bindings {
            candidate
                .viewport
                .compact_quiesced_destroyed_binding_guard(*binding, provider)
                .map_err(|source| EngineError::Viewport {
                    input: candidate.last_input,
                    source,
                })?;
        }
        candidate.advance_runtime_retention_revision()?;
        receipt
            .consume()
            .map_err(|source| EngineError::BackendIngress { source })?;
        self.publish_candidate(candidate);
        Ok(bindings)
    }

    /// Records one renderer-neutral presentation fact in the active backend order.
    ///
    /// The recorder's presentation host is core-bound at provider enrollment.
    /// Callers supply only a core-issued stream entry and cannot redirect it to
    /// another host or a superseded backend lifetime.
    pub fn record_backend_presentation_observation(
        &self,
        recorder: &mut BackendIngressRecorder,
        entry: HostPresentationObservationEntry,
    ) -> Result<BackendIngressOrdinal, EngineError> {
        let expected = self
            .backend_ingress
            .active()
            .ok_or(EngineError::BackendIngress {
                source: BackendIngressError::ProviderUnavailable,
            })?;
        let submitted = recorder.lease();
        if submitted != expected {
            return Err(EngineError::BackendIngress {
                source: BackendIngressError::ProviderLeaseMismatch {
                    expected,
                    submitted,
                },
            });
        }
        self.presentation_authority
            .presentation
            .validate_observation_entry(expected.presentation_host(), entry)
            .map_err(presentation_ledger_error)?;
        recorder
            .record_presentation_observation(entry)
            .map_err(|source| EngineError::BackendIngress { source })
    }

    /// Enrolls the sole live platform observation provider for this engine.
    ///
    /// The returned opaque lease must accompany native platform ingress. A
    /// delayed callback holding an older lease cannot regain authority after
    /// replacement or retirement.
    pub fn create_platform_provider(&mut self) -> Result<PlatformObservationLease, EngineError> {
        self.viewport
            .create_platform_provider()
            .map_err(|source| EngineError::PlatformProvider { source })
    }

    /// Returns the sole currently enrolled platform provider, when present.
    #[must_use]
    pub const fn platform_provider(&self) -> Option<PlatformObservationLease> {
        self.viewport.platform_provider()
    }

    /// Starts replacement after consuming the exact joined ingress producer.
    ///
    /// The affine drain receipt proves that the predecessor cannot mint another backend or
    /// pointer record. Any previously cloned batch remains replay-safe but loses authority in the
    /// same candidate which creates the replacement ticket.
    pub fn begin_backend_ingress_provider_replacement(
        &mut self,
        drained: &mut crate::backend_ingress::BackendIngressDrainReceipt,
    ) -> Result<BackendIngressProviderReplacementStart, EngineError> {
        let ingress = drained.lease();
        let (candidate, start) =
            self.prepare_platform_provider_replacement(ingress.platform_provider(), Some(drained))?;
        let predecessor = drained
            .transfer()
            .map_err(|source| EngineError::BackendIngress { source })?;
        let (ticket, transition) = start.into_parts();
        self.publish_candidate(candidate);
        Ok(BackendIngressProviderReplacementStart::new(
            BackendIngressProviderReplacementTicket::new(ticket, predecessor),
            transition,
        ))
    }

    /// Atomically revokes the active platform provider and starts a typed handoff.
    ///
    /// The returned ticket must not be completed until the runtime has stopped
    /// and joined every dispatch task owned by `provider`. Late callbacks are
    /// rejected immediately after this method publishes. A platform provider
    /// enrolled in the joined backend lane must instead use
    /// [`Self::begin_backend_ingress_provider_replacement`], whose affine drain
    /// receipt proves that both transport lanes are quiescent.
    pub fn begin_platform_provider_replacement(
        &mut self,
        provider: PlatformObservationLease,
    ) -> Result<PlatformProviderReplacementStart, EngineError> {
        let (candidate, start) = self.prepare_platform_provider_replacement(provider, None)?;
        self.publish_candidate(candidate);
        Ok(start)
    }

    fn prepare_platform_provider_replacement(
        &self,
        provider: PlatformObservationLease,
        drained_backend: Option<&crate::backend_ingress::BackendIngressDrainReceipt>,
    ) -> Result<(DockEngine, PlatformProviderReplacementStart), EngineError> {
        if let Some(drained) = drained_backend {
            drained
                .validate_active()
                .map_err(|source| EngineError::BackendIngress { source })?;
        }
        let drained_ingress = drained_backend.map(|receipt| receipt.lease());
        match (self.backend_ingress.active(), drained_ingress) {
            (Some(active), None) => {
                return Err(EngineError::BackendIngress {
                    source: BackendIngressError::PlatformReplacementRequiresDrain { active },
                });
            }
            (Some(active), Some(submitted)) if active != submitted => {
                return Err(EngineError::BackendIngress {
                    source: BackendIngressError::ProviderLeaseMismatch {
                        expected: active,
                        submitted,
                    },
                });
            }
            (None, Some(_)) => {
                return Err(EngineError::BackendIngress {
                    source: BackendIngressError::ProviderUnavailable,
                });
            }
            (Some(_), Some(_)) | (None, None) => {}
        }
        if let Some(drained) = drained_backend {
            let committed = self.backend_ingress.committed_through();
            if drained.recorded_through() < committed {
                return Err(EngineError::BackendIngress {
                    source: BackendIngressError::CommittedWatermarkAhead {
                        committed,
                        recorded: drained.recorded_through(),
                    },
                });
            }
        }

        let before = self.version;
        let before_scene = self.presentation_authority.scene.clone();
        let before_viewport = self.viewport.clone();
        let before_viewport_focus = self.viewport_focus.clone();
        let mut candidate = self.candidate();
        let tick = candidate
            .last_reducer_tick
            .checked_next()
            .ok_or(EngineError::ReducerTickExhausted)?;
        candidate.last_reducer_tick = tick;
        let cause = ReductionCause::PlatformProviderReplacement { tick, provider };
        let mut interaction_events = Vec::new();

        if let Some(ingress) = drained_ingress {
            candidate
                .backend_ingress
                .revoke(ingress)
                .map_err(|source| EngineError::BackendIngress { source })?;
        }

        if let Some(pointer_provider) =
            candidate
                .pointer_journal
                .active_lease()
                .filter(|lease| match lease.scope() {
                    PointerProviderScope::DesktopGlobal => true,
                    PointerProviderScope::SurfaceLocal(local) => {
                        matches!(local.endpoint(), SurfaceLocalPointerEndpoint::Native(_))
                    }
                })
        {
            candidate
                .pointer_journal
                .retire_provider(pointer_provider)
                .map_err(|source| EngineError::PointerJournal { source })?;
            candidate.cancel_retired_pointer_owner(
                cause,
                pointer_provider,
                InteractionCancelReason::PointerProviderRetired,
                &mut interaction_events,
            )?;
        }

        let ticket = candidate
            .viewport
            .begin_platform_provider_replacement(provider)
            .map_err(|source| EngineError::Viewport {
                input: candidate.last_input,
                source,
            })?;
        let _ = candidate
            .viewport_focus
            .revoke_platform_provider_authority();

        let native_close_plans = candidate
            .close
            .active_plans()
            .filter_map(|plan| {
                candidate
                    .close
                    .native_edge(plan.request())
                    .map(|_| (plan.request(), plan.authority()))
            })
            .collect::<Vec<_>>();
        for (request, authority) in native_close_plans {
            let _ = candidate
                .close
                .mark_native_edge_indeterminate(request, authority);
        }

        let invalidated_scene_authorities = candidate.invalidate_changed_surface_scene_authority();
        candidate.demote_invalidated_surface_scenes(
            candidate.last_input,
            &invalidated_scene_authorities,
        )?;
        let _ = candidate.cancel_revoked_presentation_caused(cause, &mut interaction_events)?;
        candidate.reconcile_viewport_focus_authority();
        candidate.settle_retired_presentation_hosts()?;

        let platform_effects = candidate
            .viewport
            .try_take_new_effects()
            .map_err(|source| EngineError::Viewport {
                input: candidate.last_input,
                source,
            })?;
        debug_assert!(platform_effects.is_empty());
        let focus_delta = FocusDelta::between(
            &before_viewport_focus,
            &candidate.viewport_focus,
            before_viewport.effects(),
            candidate.viewport.effects(),
            &[],
        );
        let surface_scene_deltas =
            Self::surface_scene_deltas(&before_scene, &candidate.presentation_authority.scene);
        let transition = EngineTransition::new(EngineTransitionParts {
            tick,
            before,
            after: candidate.version,
            reduced: Vec::new(),
            reduced_pointer_edges: Vec::new(),
            events: Vec::new(),
            interaction_events,
            platform_effects,
            focus_delta,
            presentation_observations: Vec::new(),
            presentation_emissions: Vec::new(),
            presentation_dispositions: Vec::new(),
            surface_contributions: Vec::new(),
            surface_scene_deltas,
            published_state_changed: true,
        });
        Ok((
            candidate,
            PlatformProviderReplacementStart::new(ticket, transition),
        ))
    }

    /// Activates the exact successor reserved by a completed provider handoff.
    ///
    /// This method changes transport authority only. Queued effects remain
    /// undispatched until a later reducer boundary establishes the successor's
    /// exact observations and extracts them as provider-bound emissions.
    pub fn finish_platform_provider_replacement(
        &mut self,
        ticket: PlatformProviderReplacementTicket,
    ) -> Result<PlatformObservationLease, EngineError> {
        let mut candidate = self.candidate();
        let provider = candidate
            .viewport
            .finish_platform_provider_replacement(ticket)
            .map_err(|source| EngineError::Viewport {
                input: candidate.last_input,
                source,
            })?;
        self.publish_candidate(candidate);
        Ok(provider)
    }

    /// Activates a platform successor together with a fresh desktop pointer and
    /// joined backend-order namespace in one atomic authority update.
    ///
    /// The successor pointer watermark is derived from the consumed predecessor recorder rather
    /// than supplied by the caller. The replacement host is explicit so a runtime can recover
    /// when the predecessor presentation host retires during handoff. The affine ticket remains
    /// reusable after every failure and becomes consumed only after the complete successor
    /// authority publishes. The same candidate compacts its detailed retired pointer tombstone
    /// only after that producer-quiescence proof is validated.
    pub fn finish_backend_ingress_provider_replacement(
        &mut self,
        ticket: &mut BackendIngressProviderReplacementTicket,
        presentation_host: PresentationHostLease,
    ) -> Result<BackendIngressRecorder, EngineError> {
        let mut candidate = self.candidate();
        let (platform_ticket, predecessor_drain) =
            ticket.authority().ok_or(EngineError::BackendIngress {
                source: BackendIngressError::ProviderReplacementTicketConsumed,
            })?;
        let predecessor = predecessor_drain.lease();
        candidate
            .presentation_authority
            .presentation
            .validate_lease(presentation_host)
            .map_err(presentation_ledger_error)?;
        let platform = candidate
            .viewport
            .finish_platform_provider_replacement(platform_ticket)
            .map_err(|source| EngineError::Viewport {
                input: candidate.last_input,
                source,
            })?;
        let _ = candidate
            .viewport
            .compact_quiesced_destroyed_binding_guards(predecessor.platform_provider());
        let _ = candidate
            .viewport
            .compact_quiesced_backend_effect_provider(&predecessor_drain);
        candidate
            .pointer_journal
            .compact_quiesced_backend(&predecessor_drain)
            .map_err(|source| EngineError::PointerJournal { source })?;
        let pointer_committed_through = predecessor_drain.pointer_through();
        let pointer = candidate
            .pointer_journal
            .create_provider(
                PointerProviderScope::DesktopGlobal,
                pointer_committed_through,
            )
            .map_err(|source| EngineError::PointerJournal { source })?;
        let recorder = candidate
            .backend_ingress
            .enroll(
                platform,
                pointer,
                presentation_host,
                pointer_committed_through,
            )
            .map_err(|source| EngineError::BackendIngress { source })?;
        let consumed = ticket.consume();
        debug_assert!(consumed);
        self.publish_candidate(candidate);
        Ok(recorder)
    }

    /// Creates the sole live desktop-global pointer-input provider for this
    /// engine authority domain.
    ///
    /// The provider owns a complete, strictly ordered edge journal across
    /// future host frames. Surface-local producers must use
    /// [`Self::create_surface_local_pointer_provider`] so producer shutdown can
    /// be proved before retirement. Creating a second live provider is
    /// rejected.
    ///
    /// This API allocates transport authority only; a journal still becomes
    /// effective exclusively through a completed [`CoreHostFrame`].
    ///
    /// # Errors
    ///
    /// Returns a typed error when `scope` is surface-local or another provider
    /// is already active.
    #[doc(hidden)]
    pub fn create_pointer_provider(
        &mut self,
        scope: PointerProviderScope,
        committed_through: crate::pointer_journal::PointerEdgeSequence,
    ) -> Result<PointerInputLease, EngineError> {
        if scope.surface_local().is_some() {
            return Err(EngineError::SurfaceLocalPointerProducerRequired);
        }
        self.create_pointer_provider_inner(scope, committed_through)
    }

    fn create_pointer_provider_inner(
        &mut self,
        scope: PointerProviderScope,
        committed_through: PointerEdgeSequence,
    ) -> Result<PointerInputLease, EngineError> {
        if let Some(provider) = self.pointer_journal.abandoned_surface_local_provider() {
            return Err(EngineError::SurfaceLocalPointerProviderAbandoned { provider });
        }
        self.validate_pointer_provider_scope(scope)?;
        self.pointer_journal
            .create_provider(scope, committed_through)
            .map_err(|source| EngineError::PointerJournal { source })
    }

    /// Creates one affine surface-local pointer producer.
    ///
    /// The returned producer owns the adapter-side lifetime of this provider.
    /// It must be drained before quiesced retirement can compact the exact
    /// provider tombstone into the monotonic incarnation frontier.
    ///
    /// # Errors
    ///
    /// Returns a typed error when the local scope is stale or another pointer
    /// provider is already active.
    pub fn create_surface_local_pointer_provider(
        &mut self,
        scope: SurfaceLocalPointerScope,
        committed_through: PointerEdgeSequence,
    ) -> Result<SurfaceLocalPointerProvider, EngineError> {
        let lease = self.create_pointer_provider_inner(
            PointerProviderScope::SurfaceLocal(scope),
            committed_through,
        )?;
        let provider = SurfaceLocalPointerProvider::new(lease, committed_through);
        self.pointer_journal
            .bind_surface_local_producer(lease, provider.monitor());
        Ok(provider)
    }

    /// Returns the current sole pointer provider, if the runtime has enrolled
    /// one.
    #[must_use]
    pub const fn pointer_provider(&self) -> Option<PointerInputLease> {
        self.pointer_journal.active_lease()
    }

    /// Returns the active provider's scope-relative aggregate button authority.
    ///
    /// A newly created or retired provider reports `Unknown`. Only a complete
    /// checkpoint can establish that all buttons are released.
    #[must_use]
    pub fn pointer_button_authority(&self) -> crate::pointer_journal::AnyButtonDownAuthority {
        self.pointer_journal.button_authority()
    }

    /// Permanently retires the exact live desktop-global provider incarnation.
    ///
    /// Retirement, exact-owner gesture cancellation, drag-route cleanup, and
    /// the durable lease tombstone publish in one reducer boundary. A retired
    /// lease cannot submit another journal or reuse a receiver candidate.
    /// Surface-local producers must drain and use
    /// [`Self::retire_quiesced_surface_local_pointer_provider`].
    #[doc(hidden)]
    pub fn retire_pointer_provider(
        &mut self,
        provider: PointerInputLease,
    ) -> Result<EngineTransition, EngineError> {
        if provider.scope().surface_local().is_some() {
            return Err(EngineError::SurfaceLocalPointerProducerRequired);
        }
        let before = self.version;
        let effect_boundary = self.viewport.latest_effect_id();
        let mut candidate = self.candidate();
        let tick = candidate
            .last_reducer_tick
            .checked_next()
            .ok_or(EngineError::ReducerTickExhausted)?;
        candidate.last_reducer_tick = tick;
        if let Some(ingress) = candidate
            .backend_ingress
            .active()
            .filter(|ingress| ingress.pointer_provider() == provider)
        {
            candidate
                .backend_ingress
                .revoke(ingress)
                .map_err(|source| EngineError::BackendIngress { source })?;
        }
        candidate
            .pointer_journal
            .retire_provider(provider)
            .map_err(|source| EngineError::PointerJournal { source })?;

        let cause = ReductionCause::PointerProviderRetirement { tick, provider };
        let mut interaction_events = Vec::new();
        candidate.cancel_retired_pointer_owner(
            cause,
            provider,
            InteractionCancelReason::PointerProviderRetired,
            &mut interaction_events,
        )?;
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
            &self.viewport_focus,
            &candidate.viewport_focus,
            self.viewport.effects(),
            candidate.viewport.effects(),
            &[],
        );
        let transition = EngineTransition::new(EngineTransitionParts {
            tick,
            before,
            after: candidate.version,
            reduced: Vec::new(),
            reduced_pointer_edges: Vec::new(),
            events: Vec::new(),
            interaction_events,
            platform_effects,
            focus_delta,
            presentation_observations: Vec::new(),
            presentation_emissions: Vec::new(),
            presentation_dispositions: Vec::new(),
            surface_contributions: Vec::new(),
            surface_scene_deltas: Vec::new(),
            published_state_changed: true,
        });
        self.publish_candidate(candidate);
        Ok(transition)
    }

    /// Retires and compacts one surface-local pointer provider after its sole
    /// producer has stopped.
    ///
    /// The receipt remains retryable on every rejected candidate. Successful
    /// publication consumes it only after the core validates the exact lease
    /// and final committed watermark.
    pub fn retire_quiesced_surface_local_pointer_provider(
        &mut self,
        receipt: &mut SurfaceLocalPointerDrainReceipt,
    ) -> Result<SurfaceLocalPointerRetirementOutcome, EngineError> {
        let provider = receipt.lease();
        let effect_boundary = self.viewport.latest_effect_id();
        let mut candidate = self.candidate();
        let disposition = candidate
            .pointer_journal
            .retire_quiesced_surface_local(receipt)
            .map_err(|source| EngineError::PointerJournal { source })?;
        candidate.advance_runtime_retention_revision()?;

        if disposition == SurfaceLocalPointerQuiescenceDisposition::CompactedPreviouslyRetired {
            let consumed = receipt.consume();
            debug_assert!(consumed, "validated surface-local drain proof is affine");
            self.publish_candidate(candidate);
            return Ok(SurfaceLocalPointerRetirementOutcome::compacted_previously_retired());
        }

        let (candidate, outcome) = self.prepare_surface_local_pointer_retirement_candidate(
            candidate,
            provider,
            effect_boundary,
        )?;
        let consumed = receipt.consume();
        debug_assert!(consumed, "validated surface-local drain proof is affine");
        self.publish_candidate(candidate);
        Ok(outcome)
    }

    /// Reclaims one surface-local lane or tombstone after every producer-side
    /// owner vanished.
    ///
    /// A live provider, an in-flight frame guard, and a drain receipt all retain
    /// the same producer state. Reclamation is therefore admitted only when the
    /// core's weak monitor proves that none of those capabilities still exist.
    /// Active authority is retired first; later calls compact abandoned
    /// detailed tombstones without manufacturing another reducer transition.
    ///
    /// # Errors
    ///
    /// Returns a lifecycle error without mutation if local cancellation would
    /// create platform or focus obligations.
    pub fn reap_abandoned_surface_local_pointer_provider(
        &mut self,
    ) -> Result<Option<SurfaceLocalPointerRetirementOutcome>, EngineError> {
        if let Some(provider) = self.pointer_journal.abandoned_surface_local_provider() {
            let effect_boundary = self.viewport.latest_effect_id();
            let mut candidate = self.candidate();
            candidate
                .pointer_journal
                .retire_abandoned_active_surface_local(provider)
                .map_err(|source| EngineError::PointerJournal { source })?;
            candidate.advance_runtime_retention_revision()?;
            let (candidate, outcome) = self.prepare_surface_local_pointer_retirement_candidate(
                candidate,
                provider,
                effect_boundary,
            )?;
            self.publish_candidate(candidate);
            return Ok(Some(outcome));
        }
        let Some(provider) = self
            .pointer_journal
            .abandoned_retired_surface_local_provider()
        else {
            return Ok(None);
        };
        let mut candidate = self.candidate();
        candidate
            .pointer_journal
            .compact_abandoned_retired_surface_local(provider)
            .map_err(|source| EngineError::PointerJournal { source })?;
        candidate.advance_runtime_retention_revision()?;
        self.publish_candidate(candidate);
        Ok(Some(
            SurfaceLocalPointerRetirementOutcome::compacted_previously_retired(),
        ))
    }

    fn prepare_surface_local_pointer_retirement_candidate(
        &self,
        mut candidate: Self,
        provider: PointerInputLease,
        effect_boundary: EffectId,
    ) -> Result<(Self, SurfaceLocalPointerRetirementOutcome), EngineError> {
        let tick = candidate
            .last_reducer_tick
            .checked_next()
            .ok_or(EngineError::ReducerTickExhausted)?;
        candidate.last_reducer_tick = tick;
        let cause = ReductionCause::PointerProviderRetirement { tick, provider };
        let mut interaction_events = Vec::new();
        candidate.cancel_retired_pointer_owner(
            cause,
            provider,
            InteractionCancelReason::PointerProviderRetired,
            &mut interaction_events,
        )?;
        let platform_effects = candidate
            .viewport
            .try_take_new_effects_after(effect_boundary)
            .map_err(|source| EngineError::Viewport {
                input: candidate.last_input,
                source,
            })?;
        let focus_delta = FocusDelta::between(
            &self.viewport_focus,
            &candidate.viewport_focus,
            self.viewport.effects(),
            candidate.viewport.effects(),
            &[],
        );
        if !platform_effects.is_empty() || !focus_delta.is_empty() {
            return Err(
                EngineError::SurfaceLocalPointerRetirementExternalObligation {
                    platform_effect_count: platform_effects.len(),
                    focus_changed: !focus_delta.is_empty(),
                },
            );
        }
        Ok((
            candidate,
            SurfaceLocalPointerRetirementOutcome::retired_active(!interaction_events.is_empty()),
        ))
    }

    pub(super) fn validate_pointer_provider_scope(
        &self,
        scope: PointerProviderScope,
    ) -> Result<(), EngineError> {
        let Some(local) = scope.surface_local() else {
            return Ok(());
        };
        self.presentation_authority
            .presentation
            .validate_lease(local.host())
            .map_err(presentation_ledger_error)?;
        let surface = local.surface();
        if self
            .presentation_authority
            .presentation_requirements
            .surface(surface)
            .is_none()
        {
            return Err(EngineError::PointerProviderScope {
                detail: format!("surface {surface:?} is absent from the current semantic roster"),
            });
        }
        if let Some(owner) = self
            .presentation_authority
            .presentation
            .active_surface_host(surface)
            && owner != local.host()
        {
            return Err(EngineError::PointerProviderSurfaceHostMismatch {
                host: local.host(),
                surface,
                owner,
            });
        }
        if let SurfaceLocalPointerEndpoint::Native(binding) = local.endpoint() {
            let current = self
                .viewport
                .viewport(surface)
                .map(|record| record.binding());
            if current != Some(binding) {
                return Err(EngineError::PointerProviderScope {
                    detail: format!(
                        "native endpoint {binding:?} is not the current binding for surface {surface:?}"
                    ),
                });
            }
        }
        Ok(())
    }

    pub(super) fn reconcile_pointer_provider_scope(
        &mut self,
        cause: ReductionCause,
        interaction_events: &mut Vec<InteractionEvent>,
    ) -> Result<Option<PointerInputLease>, EngineError> {
        let Some(provider) = self.pointer_journal.active_lease() else {
            return Ok(None);
        };
        if self
            .validate_pointer_provider_scope(provider.scope())
            .is_ok()
        {
            return Ok(None);
        }

        // The lease was valid when this engine minted it, so any later scope
        // rejection is a lifecycle revocation rather than malformed adapter
        // input. Publish the tombstone in the same candidate that changed the
        // binding, surface roster, or presentation-host authority.
        self.pointer_journal
            .retire_provider(provider)
            .map_err(|source| EngineError::PointerJournal { source })?;
        self.cancel_retired_pointer_owner(
            cause,
            provider,
            InteractionCancelReason::PointerProviderRetired,
            interaction_events,
        )?;
        Ok(Some(provider))
    }
}
