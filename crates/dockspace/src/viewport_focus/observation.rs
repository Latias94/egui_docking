use super::*;

impl FocusObservationStream {
    pub(super) fn observe(
        &mut self,
        observation: Option<FocusObservationEnvelope>,
    ) -> FocusObservationStreamTransition {
        let Some(observation) = observation else {
            self.current = None;
            self.requires_tombstone |= self.last_generation.is_some();
            return FocusObservationStreamTransition::AuthorityRevoked {
                generation: None,
                reason: FocusAuthorityRevocation::ObservationMissing,
            };
        };
        match self.last_generation {
            Some(generation) if observation.generation() < generation => {
                return FocusObservationStreamTransition::Stale {
                    current: generation,
                };
            }
            Some(generation) if observation.generation() == generation => {
                if self.last_envelope != Some(observation) {
                    self.current = None;
                    self.requires_tombstone = true;
                    return FocusObservationStreamTransition::AuthorityRevoked {
                        generation: Some(observation.generation()),
                        reason: FocusAuthorityRevocation::EqualGenerationConflict,
                    };
                }
                return FocusObservationStreamTransition::Duplicate;
            }
            Some(generation) if generation.checked_next() != Some(observation.generation()) => {
                self.last_generation = Some(observation.generation());
                self.last_envelope = Some(observation);
                self.current = None;
                self.requires_tombstone = true;
                return FocusObservationStreamTransition::AuthorityRevoked {
                    generation: Some(observation.generation()),
                    reason: FocusAuthorityRevocation::GenerationGap {
                        previous: generation,
                    },
                };
            }
            Some(_) | None => {}
        }
        self.last_generation = Some(observation.generation());
        self.last_envelope = Some(observation);
        if self.requires_tombstone && matches!(observation.focused(), Authority::Known(_)) {
            self.current = None;
            return FocusObservationStreamTransition::TombstoneRequired {
                generation: observation.generation(),
            };
        }
        if let Authority::Unknown(reason) = *observation.focused() {
            self.current = None;
            self.requires_tombstone = false;
            return FocusObservationStreamTransition::AuthorityRevoked {
                generation: Some(observation.generation()),
                reason: FocusAuthorityRevocation::Unknown(reason),
            };
        }
        self.current = Some(observation);
        self.requires_tombstone = false;
        FocusObservationStreamTransition::Current
    }

    #[cfg(test)]
    pub(super) fn force_current_for_test(&mut self, observation: FocusObservationEnvelope) {
        let transition = self.observe(Some(observation));
        assert!(
            matches!(transition, FocusObservationStreamTransition::Current),
            "a test baseline must become current"
        );
    }

    pub(crate) const fn current(self) -> Option<FocusObservationEnvelope> {
        self.current
    }

    #[cfg(test)]
    pub(crate) const fn generation_watermark(self) -> Option<FocusObservationGeneration> {
        self.last_generation
    }

    /// Starts a fresh observation namespace for a replacement provider.
    #[cfg(test)]
    pub(crate) fn reset_for_provider_replacement(&mut self) {
        *self = Self::default();
    }
}

impl ViewportFocusCoordinator {
    /// Publishes one provider-generated globally consistent native-focus envelope.
    ///
    /// The exact provider generation is compared before any state transition. A matching
    /// activation completes only after its `RequestFocus` effect identity is attached and
    /// the observation is newer than the frozen baseline.
    ///
    /// # Errors
    ///
    /// Returns [`ViewportFocusError::PaneFocusIntentIdExhausted`] if accepting the
    /// observation would require a pane intent after its identity domain is exhausted.
    #[cfg(test)]
    pub fn publish_focus_observation(
        &mut self,
        observation: FocusObservationEnvelope,
        causal: FocusCausalStamp,
        is_current_binding: impl Fn(ViewportBinding) -> bool + Copy,
        item_is_on_surface: impl Fn(SurfaceId, ItemId) -> bool + Copy,
    ) -> Result<FocusObservationTransition, ViewportFocusError> {
        let mut candidate = self.clone();
        let transition = candidate.publish_focus_observation_in_place(
            observation,
            causal,
            false,
            is_current_binding,
            is_current_binding,
            |_| false,
            item_is_on_surface,
        )?;
        *self = candidate;
        Ok(transition)
    }

    #[cfg(test)]
    pub(crate) fn publish_platform_focus_observation(
        &mut self,
        observation: FocusObservationEnvelope,
        causal: FocusCausalStamp,
        can_observe_binding: impl Fn(ViewportBinding) -> bool + Copy,
        can_accept_activation: impl Fn(ViewportBinding) -> bool + Copy,
        is_isolated_window: impl Fn(WindowBinding) -> bool + Copy,
        item_is_on_surface: impl Fn(SurfaceId, ItemId) -> bool + Copy,
    ) -> Result<FocusObservationTransition, ViewportFocusError> {
        self.publish_platform_focus_observation_with_retired_binding(
            observation,
            causal,
            false,
            can_observe_binding,
            can_accept_activation,
            is_isolated_window,
            item_is_on_surface,
        )
    }

    pub(crate) fn publish_platform_focus_observation_with_retired_binding(
        &mut self,
        observation: FocusObservationEnvelope,
        causal: FocusCausalStamp,
        retired_binding: bool,
        can_observe_binding: impl Fn(ViewportBinding) -> bool + Copy,
        can_accept_activation: impl Fn(ViewportBinding) -> bool + Copy,
        is_isolated_window: impl Fn(WindowBinding) -> bool + Copy,
        item_is_on_surface: impl Fn(SurfaceId, ItemId) -> bool + Copy,
    ) -> Result<FocusObservationTransition, ViewportFocusError> {
        let mut candidate = self.clone();
        let transition = candidate.publish_focus_observation_in_place(
            observation,
            causal,
            retired_binding,
            can_observe_binding,
            can_accept_activation,
            is_isolated_window,
            item_is_on_surface,
        )?;
        *self = candidate;
        Ok(transition)
    }

    pub(super) fn known_focus_target(
        observation: FocusObservationEnvelope,
    ) -> Option<FocusClaimTarget> {
        match observation.focused {
            Authority::Known(focused) => Some(FocusClaimTarget::from_global(focused)),
            Authority::Unknown(_) => None,
        }
    }

    pub(super) fn target_is_pending_hazard(&self, target: FocusClaimTarget) -> bool {
        target.window().is_some_and(|window| {
            self.focus_lane
                .hazards
                .values()
                .any(|attempt| attempt.request.target.window() == window)
        })
    }

    pub(super) fn supersede_driving_with_platform_focus(
        &mut self,
        observation: FocusObservationEnvelope,
        focus_changed: bool,
        applied: &mut FocusObservationApplied,
    ) {
        if !focus_changed {
            return;
        }
        let Some(winner) = Self::known_focus_target(observation) else {
            return;
        };
        if winner
            .window()
            .is_some_and(|window| self.window_is_quarantined(window))
        {
            return;
        }
        let Some(driving) = self.focus_lane.driving else {
            if !self.focus_lane.hazards.is_empty() && !self.target_is_pending_hazard(winner) {
                self.focus_lane.barrier = FocusBarrier::Hazard(FocusHazardBarrier {
                    winner,
                    observed_at: observation.generation,
                    phase: FocusHazardBarrierPhase::AwaitingHazards,
                });
            }
            return;
        };
        if winner.window() == Some(driving.request.target.window())
            || observation.generation <= driving.observation_baseline
        {
            return;
        }
        let acknowledged = driving.platform_focus.effect().is_some_and(|effect| {
            matches!(
                observation.acknowledged_effect,
                Authority::Known(Some(acknowledged)) if acknowledged == effect
            )
        });
        if acknowledged
            || matches!(
                driving.platform_focus,
                PendingPlatformFocus::ObservedAwaitingTarget { .. }
            )
        {
            return;
        }

        self.focus_lane.driving = None;
        if let Some(effect) = driving.platform_focus.effect() {
            self.focus_lane.hazards.insert(effect, driving);
            self.focus_lane.quarantined.insert(driving.request.target);
            self.focus_lane.barrier = FocusBarrier::Hazard(FocusHazardBarrier {
                winner,
                observed_at: observation.generation,
                phase: FocusHazardBarrierPhase::AwaitingHazards,
            });
        }
        applied.cancelled_activation = Some((
            driving.generation,
            ActivationCancellation::SupersededByPlatformFocus {
                observation: observation.generation,
            },
        ));
    }

    pub(super) fn reduce_focus_hazards(
        &mut self,
        observation: FocusObservationEnvelope,
        is_isolated_window: impl Fn(WindowBinding) -> bool,
        applied: &mut FocusObservationApplied,
    ) {
        let focused_binding = match observation.focused {
            Authority::Known(GlobalFocusedWindow::Dock(binding)) => Some(binding),
            Authority::Known(GlobalFocusedWindow::Foreign | GlobalFocusedWindow::None)
            | Authority::Unknown(_) => None,
        };
        let acknowledged_effect = match observation.acknowledged_effect {
            Authority::Known(effect) => effect,
            Authority::Unknown(_) => None,
        };
        let settled = self
            .focus_lane
            .hazards
            .iter()
            .filter_map(|(effect, attempt)| {
                if observation.generation <= attempt.observation_baseline {
                    return None;
                }
                let evidence = if acknowledged_effect == Some(*effect) {
                    Some(PlatformFocusEvidence::ExactEffectAcknowledgement)
                } else if focused_binding == Some(attempt.request.target.window()) {
                    Some(PlatformFocusEvidence::NewerMatchingObservation)
                } else {
                    None
                }?;
                Some((*effect, attempt.request.target, evidence))
            })
            .collect::<Vec<_>>();

        let mut hazard_took_focus = false;
        let mut settled_any = false;
        for (effect, binding, evidence) in settled {
            settled_any = true;
            hazard_took_focus |= focused_binding == Some(binding.window());
            self.focus_lane.hazards.remove(&effect);
            applied.record_observed_effect(ObservedPlatformFocusEffect {
                effect,
                binding,
                generation: observation.generation,
                evidence,
            });
        }

        if let FocusBarrier::Hazard(mut barrier) = self.focus_lane.barrier {
            if self.focus_lane.hazards.is_empty() {
                let post_hazard_authority =
                    Self::known_focus_target(observation).is_some_and(|target| {
                        target.window().is_none_or(|window| {
                            !self.window_is_quarantined(window) && !is_isolated_window(window)
                        })
                    });
                if !hazard_took_focus
                    && post_hazard_authority
                    && (settled_any || observation.generation > barrier.observed_at)
                {
                    self.focus_lane.barrier = FocusBarrier::None;
                    self.focus_lane.quarantined.clear();
                } else {
                    barrier.phase = FocusHazardBarrierPhase::AwaitingPostHazardAuthority;
                    self.focus_lane.barrier = FocusBarrier::Hazard(barrier);
                }
            }
        }
    }

    pub(super) fn publish_focus_observation_in_place(
        &mut self,
        mut observation: FocusObservationEnvelope,
        causal: FocusCausalStamp,
        retired_binding: bool,
        can_observe_binding: impl Fn(ViewportBinding) -> bool + Copy,
        can_accept_activation: impl Fn(ViewportBinding) -> bool + Copy,
        is_isolated_window: impl Fn(WindowBinding) -> bool + Copy,
        item_is_on_surface: impl Fn(SurfaceId, ItemId) -> bool + Copy,
    ) -> Result<FocusObservationTransition, ViewportFocusError> {
        let previous = self.focus_observations.current();
        let raw_transition = self.focus_observations.observe(Some(observation));
        let transition = if retired_binding
            && matches!(
                raw_transition,
                FocusObservationStreamTransition::Current
                    | FocusObservationStreamTransition::Duplicate
            ) {
            // Retirement revokes effective authority without rewriting provider history.
            self.focus_observations.current = None;
            let reason = AuthorityUnavailableReason::NotReported;
            observation = FocusObservationEnvelope::new(
                observation.generation(),
                Authority::Unknown(reason),
                *observation.acknowledged_effect(),
            );
            FocusObservationStreamTransition::AuthorityRevoked {
                generation: Some(observation.generation()),
                reason: FocusAuthorityRevocation::Unknown(reason),
            }
        } else {
            raw_transition
        };
        match transition {
            FocusObservationStreamTransition::Current => {
                let focused = match observation.focused {
                    Authority::Known(focused) => Some(focused),
                    Authority::Unknown(_) => None,
                };
                self.record_platform_ui_focus(focused);
            }
            FocusObservationStreamTransition::Duplicate => {
                return Ok(FocusObservationTransition::Duplicate);
            }
            FocusObservationStreamTransition::Stale { current } => {
                return Ok(FocusObservationTransition::Stale { current });
            }
            FocusObservationStreamTransition::AuthorityRevoked { generation, reason } => {
                self.record_platform_ui_focus(None);
                let mut effect_settlement = FocusObservationApplied::default();
                if matches!(reason, FocusAuthorityRevocation::Unknown(_)) {
                    self.reduce_focus_hazards(
                        observation,
                        is_isolated_window,
                        &mut effect_settlement,
                    );
                    self.reduce_pending_activation_observation(
                        observation,
                        can_observe_binding,
                        can_accept_activation,
                        item_is_on_surface,
                        &mut effect_settlement,
                    )?;
                    return Ok(FocusObservationTransition::AuthorityRevoked {
                        generation,
                        reason,
                        cleanup: ViewportFocusCleanup {
                            global_authority_removed: previous.is_some(),
                            ..ViewportFocusCleanup::default()
                        },
                        effect_settlement: Box::new(effect_settlement),
                    });
                }
                let cleanup = self.clear_focus_stream_authority(previous.is_some());
                return Ok(FocusObservationTransition::AuthorityRevoked {
                    generation,
                    reason,
                    cleanup,
                    effect_settlement: Box::new(effect_settlement),
                });
            }
            FocusObservationStreamTransition::TombstoneRequired { generation } => {
                self.record_platform_ui_focus(None);
                let cleanup = self.clear_focus_stream_authority(previous.is_some());
                return Ok(FocusObservationTransition::TombstoneRequired {
                    generation,
                    cleanup,
                });
            }
        }

        let mut applied = FocusObservationApplied::default();
        let focus_changed = previous.map(|previous| previous.focused) != Some(observation.focused);
        self.reduce_recorded_observe_only_activation(
            observation,
            can_observe_binding,
            can_accept_activation,
            item_is_on_surface,
            &mut applied,
        )?;
        self.supersede_driving_with_platform_focus(observation, focus_changed, &mut applied);
        self.reduce_focus_hazards(observation, is_isolated_window, &mut applied);
        self.reduce_pending_activation_observation(
            observation,
            can_observe_binding,
            can_accept_activation,
            item_is_on_surface,
            &mut applied,
        )?;
        let focus_requires_reconciliation =
            matches!(self.focus_lane.barrier, FocusBarrier::ReconcileCurrent);
        let focused_is_isolated = matches!(
            observation.focused,
            Authority::Known(GlobalFocusedWindow::Dock(binding))
                if is_isolated_window(binding)
                    || self.window_is_quarantined(binding)
                    || self
                        .focus_lane
                        .hazards
                        .values()
                        .any(|attempt| attempt.request.target.window() == binding)
        );
        if !focused_is_isolated
            && self.pending_pane_intent.is_some_and(|intent| {
                observation.generation > intent.focus_observation_baseline
                    && intent.native_guard().is_some()
                    && matches!(
                        observation.focused,
                        Authority::Known(focused)
                            if Some(focused)
                                != intent.native_guard().map(|binding| GlobalFocusedWindow::Dock(binding.window()))
                    )
            })
        {
            if let Some(intent) = self.pending_pane_intent.take() {
                self.clear_admitted_pane_intent(intent.id);
            }
        }

        // Readiness completes the replacement's existing obligation. Ordinary
        // platform observations advance facts, not its semantic causal owner.
        let preserves_replacement = self.pending_pane_intent.is_some_and(|intent| {
            intent.source == PaneFocusIntentSource::WorkspaceReplacement
                && (intent.awaiting_native_binding()
                    || intent
                        .native_guard()
                        .is_some_and(|binding| Self::observation_focuses(observation, binding)))
        });
        if preserves_replacement && let Some(intent) = self.pending_pane_intent.as_mut() {
            intent.focus_observation_baseline = observation.generation;
        }

        // A physical window can host several independently focused placements.
        // Its focus fact never selects a surface or restores a pane history.

        if ((focus_changed && (previous.is_some() || self.focus_lane.winner.is_none()))
            || focus_requires_reconciliation)
            && applied.completed_activation().is_none()
            && applied.pane_intent().is_none()
            && matches!(observation.focused, Authority::Known(_))
        {
            let Authority::Known(focused) = observation.focused else {
                unreachable!("known focus was checked above")
            };
            let target = FocusClaimTarget::from_global(focused);
            if target.window().is_none_or(|window| {
                !is_isolated_window(window) && !self.window_is_quarantined(window)
            }) {
                if preserves_replacement
                    || target
                        .window()
                        .is_some_and(|window| self.window_matches_semantic_claim(window))
                {
                    if focus_requires_reconciliation {
                        self.focus_lane.barrier = FocusBarrier::None;
                    }
                } else {
                    let _ = self.admit_pane_causal_claim(PaneCausalClaim::platform_observation(
                        causal, focused,
                    ));
                    if focus_requires_reconciliation {
                        self.focus_lane.barrier = FocusBarrier::None;
                    }
                }
            }
        }

        Ok(FocusObservationTransition::Applied(Box::new(applied)))
    }

    pub(super) fn reduce_recorded_observe_only_activation(
        &mut self,
        observation: FocusObservationEnvelope,
        can_observe_binding: impl Fn(ViewportBinding) -> bool,
        can_accept_activation: impl Fn(ViewportBinding) -> bool,
        item_is_on_surface: impl Fn(SurfaceId, ItemId) -> bool,
        applied: &mut FocusObservationApplied,
    ) -> Result<(), ViewportFocusError> {
        let Some(record) = self.recorded_observe_only_activation else {
            return Ok(());
        };
        if record
            .observation_baseline
            .is_some_and(|baseline| observation.generation <= baseline)
        {
            return Ok(());
        }

        let request = record.request;
        let target_is_current = if self.admitted_observe_only_activation == Some(record.generation)
        {
            can_observe_binding(request.target)
        } else {
            can_accept_activation(request.target)
        };
        let item_is_current = match request.pane {
            PaneFocusDisposition::Set(item) => item_is_on_surface(request.target.surface(), item),
            PaneFocusDisposition::Preserve | PaneFocusDisposition::Clear => true,
        };
        let target_focus = matches!(
            observation.focused,
            Authority::Known(GlobalFocusedWindow::Dock(binding)) if binding == request.target.window()
        );
        let authoritative_non_target = matches!(
            observation.focused,
            Authority::Known(focused) if focused != GlobalFocusedWindow::Dock(request.target.window())
        );

        if request.cause.awaits_observed_focus()
            && target_is_current
            && item_is_current
            && target_focus
        {
            let was_admitted = self.admitted_observe_only_activation == Some(record.generation);
            self.recorded_observe_only_activation = None;
            self.clear_admitted_observe_only(record.generation);
            applied.cleared_observe_only_activation = Some(record.generation);
            if let Some(focus) = request.focus() {
                let intent = self.install_pane_intent(PaneIntentInstall {
                    causal: record.causal,
                    activation: Some(record.generation),
                    surface: request.target.surface(),
                    native_guard: Some(request.target),
                    focus,
                    source: request.cause.pane_source(),
                    cause: Some(request.cause),
                    focus_observation_baseline: observation.generation,
                })?;
                if was_admitted {
                    self.admitted_pane_intent = intent.map(|intent| intent.id);
                }
                applied.set_pane_intent(intent);
            }
        } else if !request.cause.awaits_observed_focus()
            || !target_is_current
            || !item_is_current
            || authoritative_non_target
        {
            self.recorded_observe_only_activation = None;
            self.clear_admitted_observe_only(record.generation);
            applied.cleared_observe_only_activation = Some(record.generation);
        }
        Ok(())
    }

    pub(super) fn reduce_pending_activation_observation(
        &mut self,
        observation: FocusObservationEnvelope,
        can_observe_binding: impl Fn(ViewportBinding) -> bool,
        can_accept_activation: impl Fn(ViewportBinding) -> bool,
        item_is_on_surface: impl Fn(SurfaceId, ItemId) -> bool,
        applied: &mut FocusObservationApplied,
    ) -> Result<(), ViewportFocusError> {
        let Some(pending) = self.focus_lane.driving else {
            return Ok(());
        };
        let target_is_current = match pending.platform_focus {
            PendingPlatformFocus::EffectRequired => can_accept_activation(pending.request.target),
            PendingPlatformFocus::Requested { .. }
            | PendingPlatformFocus::Indeterminate { .. }
            | PendingPlatformFocus::ObservedAwaitingTarget { .. } => {
                can_observe_binding(pending.request.target)
            }
        };
        if !target_is_current {
            self.cancel_pending_activation(pending, ActivationCancellation::StaleBinding, applied);
            return Ok(());
        }
        if let PaneFocusDisposition::Set(item) = pending.request.pane
            && !item_is_on_surface(pending.request.target.surface(), item)
        {
            self.cancel_pending_activation(
                pending,
                ActivationCancellation::ItemUnavailable { item },
                applied,
            );
            return Ok(());
        }
        if observation.generation <= pending.observation_baseline {
            return Ok(());
        }
        let Some(effect) = pending.platform_focus.effect() else {
            return Ok(());
        };

        let acknowledged = matches!(
            observation.acknowledged_effect,
            Authority::Known(Some(acknowledged)) if acknowledged == effect
        );
        let target_focused = Self::observation_focuses(observation, pending.request.target);
        if acknowledged || target_focused {
            applied.record_observed_effect(ObservedPlatformFocusEffect {
                effect,
                binding: pending.request.target,
                generation: observation.generation,
                evidence: if acknowledged {
                    PlatformFocusEvidence::ExactEffectAcknowledgement
                } else {
                    PlatformFocusEvidence::NewerMatchingObservation
                },
            });
        }
        if target_focused {
            self.complete_pending_activation(pending, observation, applied)?;
        } else if acknowledged {
            match observation.focused {
                Authority::Known(GlobalFocusedWindow::Dock(binding))
                    if self.window_is_quarantined(binding) =>
                {
                    self.reassert_after_quarantined_focus(pending, observation, applied)?;
                }
                Authority::Known(_) => self.cancel_pending_activation(
                    pending,
                    ActivationCancellation::EffectObservedWithoutTargetFocus,
                    applied,
                ),
                Authority::Unknown(_) => {
                    self.focus_lane.driving = Some(PendingViewportActivation {
                        platform_focus: PendingPlatformFocus::ObservedAwaitingTarget {
                            effect,
                            acknowledged_at: observation.generation,
                        },
                        ..pending
                    });
                }
            }
        } else if matches!(
            pending.platform_focus,
            PendingPlatformFocus::ObservedAwaitingTarget { .. }
        ) && matches!(observation.focused, Authority::Known(_))
        {
            if matches!(
                observation.focused,
                Authority::Known(GlobalFocusedWindow::Dock(binding))
                    if self.window_is_quarantined(binding)
            ) {
                self.reassert_after_quarantined_focus(pending, observation, applied)?;
            } else {
                self.cancel_pending_activation(
                    pending,
                    ActivationCancellation::EffectObservedWithoutTargetFocus,
                    applied,
                );
            }
        }
        Ok(())
    }

    pub(super) fn reassert_after_quarantined_focus(
        &mut self,
        pending: PendingViewportActivation,
        observation: FocusObservationEnvelope,
        applied: &mut FocusObservationApplied,
    ) -> Result<(), ViewportFocusError> {
        let replacement = self
            .last_activation
            .checked_next()
            .ok_or(ViewportFocusError::ActivationGenerationExhausted)?;
        self.last_activation = replacement;
        self.focus_lane.driving = Some(PendingViewportActivation {
            generation: replacement,
            observation_baseline: observation.generation,
            platform_focus: PendingPlatformFocus::EffectRequired,
            ..pending
        });
        self.retarget_lifecycle_focus_barrier(Some(pending), replacement);
        applied.cancelled_activation = Some((
            pending.generation,
            ActivationCancellation::EffectObservedWithoutTargetFocus,
        ));
        Ok(())
    }

    pub(super) fn complete_pending_activation(
        &mut self,
        pending: PendingViewportActivation,
        observation: FocusObservationEnvelope,
        applied: &mut FocusObservationApplied,
    ) -> Result<(), ViewportFocusError> {
        self.focus_lane.driving = None;
        self.complete_lifecycle_focus_barrier(pending.generation);
        applied.completed_activation = Some(pending.generation);
        if let Some(focus) = pending.request.focus() {
            let intent = self.install_pane_intent(PaneIntentInstall {
                causal: pending.causal,
                activation: Some(pending.generation),
                surface: pending.request.target.surface(),
                native_guard: Some(pending.request.target),
                focus,
                source: pending.request.cause.pane_source(),
                cause: Some(pending.request.cause),
                focus_observation_baseline: observation.generation,
            })?;
            self.admitted_pane_intent = intent.map(|intent| intent.id);
            applied.set_pane_intent(intent);
        }
        Ok(())
    }

    pub(super) fn cancel_pending_activation(
        &mut self,
        pending: PendingViewportActivation,
        reason: ActivationCancellation,
        applied: &mut FocusObservationApplied,
    ) {
        self.focus_lane.driving = None;
        self.cancel_lifecycle_focus_barrier(pending.generation);
        applied.cancelled_activation = Some((pending.generation, reason));
    }

    pub(super) fn observation_focuses(
        observation: FocusObservationEnvelope,
        target: ViewportBinding,
    ) -> bool {
        matches!(
            observation.focused,
            Authority::Known(GlobalFocusedWindow::Dock(binding)) if binding == target.window()
        )
    }
}
