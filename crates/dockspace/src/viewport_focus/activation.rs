use super::*;

impl ViewportFocusCoordinator {
    pub(super) fn window_matches_semantic_claim(&self, window: WindowBinding) -> bool {
        self.focus_lane.winner.is_some_and(|claim| {
            claim
                .target
                .dock()
                .is_some_and(|binding| binding.window() == window)
        })
    }

    pub(super) fn window_is_quarantined(&self, window: WindowBinding) -> bool {
        self.focus_lane
            .quarantined
            .iter()
            .any(|binding| binding.window() == window)
    }

    #[cfg(test)]
    pub(crate) fn suppressed_tear_off_bindings(&self) -> BTreeSet<ViewportBinding> {
        self.focus_lane.quarantined.clone()
    }

    pub(super) fn complete_lifecycle_focus_barrier(&mut self, generation: ActivationGeneration) {
        if matches!(
            self.focus_lane.barrier,
            FocusBarrier::Dock { activation } if activation == generation
        ) {
            self.focus_lane.barrier = FocusBarrier::None;
            self.focus_lane.quarantined.clear();
            self.focus_lane.hazards.clear();
        }
    }

    pub(super) fn cancel_lifecycle_focus_barrier(&mut self, generation: ActivationGeneration) {
        if matches!(
            self.focus_lane.barrier,
            FocusBarrier::Dock { activation } if activation == generation
        ) {
            self.focus_lane.barrier = FocusBarrier::ReconcileCurrent;
            self.focus_lane.quarantined.clear();
            self.focus_lane.hazards.clear();
        }
    }

    pub(super) fn pane_claim_order(claim: PaneCausalClaim, floor: PaneCausalClaim) -> Ordering {
        claim
            .causal
            .generation()
            .cmp(&floor.causal.generation())
            .then_with(|| claim.source.priority().cmp(&floor.source.priority()))
    }

    pub(super) fn pending_intent_claim(intent: PaneFocusIntent) -> PaneCausalClaim {
        PaneCausalClaim {
            causal: intent.causal,
            source: intent.source,
            target: intent.native_guard().map_or(
                FocusClaimTarget::Surface(intent.surface),
                FocusClaimTarget::Dock,
            ),
            pane: PaneFocusDisposition::from_focus(intent.focus()),
            cause: intent
                .cause
                .unwrap_or(ViewportActivationCause::PlatformObservation),
        }
    }

    pub(super) fn publish_focus_winner(&mut self, winner: Option<PaneCausalClaim>) {
        if self.focus_lane.winner == winner {
            return;
        }
        self.focus_lane.winner = winner;
        if let (Some(winner), Some(intent)) = (winner, self.pending_pane_intent)
            && Self::pane_claim_order(winner, Self::pending_intent_claim(intent))
                == Ordering::Greater
        {
            self.pending_pane_intent = None;
            self.clear_admitted_pane_intent(intent.id);
        }
    }

    pub(super) fn recompute_focus_winner(&mut self) {
        let winner = self
            .focus_lane
            .native_reservations
            .values()
            .map(|reservation| reservation.claim())
            .fold(self.focus_lane.settled_winner, |winner, claim| {
                winner.map_or(Some(claim), |current| {
                    if Self::pane_claim_order(claim, current) == Ordering::Greater {
                        Some(claim)
                    } else {
                        Some(current)
                    }
                })
            });
        self.publish_focus_winner(winner);
    }

    pub(super) fn admit_pane_causal_claim(&mut self, claim: PaneCausalClaim) -> bool {
        if let Some(floor) = self.focus_lane.settled_winner {
            match Self::pane_claim_order(claim, floor) {
                Ordering::Less => return false,
                Ordering::Equal if claim != floor => return false,
                Ordering::Equal | Ordering::Greater => {}
            }
        }
        self.focus_lane.settled_winner = Some(claim);
        self.recompute_focus_winner();
        self.focus_lane.winner == Some(claim)
    }

    pub(super) fn reserve_native_activation(
        &mut self,
        owner: NativeCreateSagaId,
        reservation: NativeActivationReservation,
    ) -> bool {
        if let Some(existing) = self.focus_lane.native_reservations.get(&owner) {
            return *existing == reservation;
        }
        let claim = reservation.claim();
        if self
            .focus_lane
            .native_reservations
            .values()
            .any(|existing| existing.claim() == claim)
        {
            return false;
        }
        if let Some(floor) = self.focus_lane.winner {
            match Self::pane_claim_order(claim, floor) {
                Ordering::Less => return false,
                Ordering::Equal if claim != floor => return false,
                Ordering::Equal | Ordering::Greater => {}
            }
        }
        self.focus_lane
            .native_reservations
            .insert(owner, reservation);
        self.recompute_focus_winner();
        true
    }

    pub(super) fn retarget_lifecycle_focus_barrier(
        &mut self,
        superseded: Option<PendingViewportActivation>,
        replacement: ActivationGeneration,
    ) {
        let Some(superseded) = superseded else {
            return;
        };
        let inherited_barrier = matches!(
            self.focus_lane.barrier,
            FocusBarrier::Dock { activation } if activation == superseded.generation
        );
        let hazardous = if let Some(effect) = superseded.platform_focus.effect() {
            self.focus_lane.hazards.insert(effect, superseded);
            self.focus_lane
                .quarantined
                .insert(superseded.request.target);
            true
        } else {
            false
        };
        if !inherited_barrier && !hazardous {
            return;
        }
        self.focus_lane.barrier = FocusBarrier::Dock {
            activation: replacement,
        };
    }

    /// Reserves a semantic pane-focus claim before an asynchronous viewport can exist.
    ///
    /// A later completion may replay the exact claim. An older completion can
    /// still finish its topology transaction, but cannot regain focus authority.
    pub(crate) fn reserve_activation_causal(
        &mut self,
        owner: NativeCreateSagaId,
        causal: FocusCausalStamp,
        request: ViewportActivationRequest,
    ) -> bool {
        self.reserve_native_activation(owner, NativeActivationReservation { causal, request })
    }

    pub(crate) fn cancel_activation_reservation(&mut self, owner: NativeCreateSagaId) -> bool {
        let removed = self.focus_lane.native_reservations.remove(&owner).is_some();
        if removed {
            self.recompute_focus_winner();
        }
        removed
    }

    pub(crate) fn winning_activation_reservation(
        &self,
    ) -> Option<(
        NativeCreateSagaId,
        FocusCausalStamp,
        ViewportActivationRequest,
    )> {
        let winner = self.focus_lane.winner?;
        self.focus_lane
            .native_reservations
            .iter()
            .find_map(|(owner, reservation)| {
                (reservation.claim() == winner).then_some((
                    *owner,
                    reservation.causal,
                    reservation.request,
                ))
            })
    }

    pub(crate) fn activation_reservations(
        &self,
    ) -> Vec<(
        NativeCreateSagaId,
        FocusCausalStamp,
        ViewportActivationRequest,
    )> {
        let mut reservations = self
            .focus_lane
            .native_reservations
            .iter()
            .map(|(owner, reservation)| (*owner, reservation.causal, reservation.request))
            .collect::<Vec<_>>();
        reservations.sort_unstable_by(|left, right| {
            Self::pane_claim_order(
                PaneCausalClaim::from_request(right.1, right.2),
                PaneCausalClaim::from_request(left.1, left.2),
            )
            .then_with(|| right.0.cmp(&left.0))
        });
        reservations
    }

    pub(crate) fn lifecycle_barrier_protects(
        &self,
        stale_target: ViewportBinding,
        winner: ViewportBinding,
    ) -> bool {
        self.focus_lane.quarantined.contains(&stale_target)
            && self.focus_lane.driving.is_some_and(|pending| {
                pending.request.target == winner
                    && matches!(
                        self.focus_lane.barrier,
                        FocusBarrier::Dock { activation }
                            if activation == pending.generation
                    )
            })
    }

    pub(crate) fn request_winning_lifecycle_compensation(
        &mut self,
        can_request_platform_focus: bool,
        is_current_binding: impl FnOnce(ViewportBinding) -> bool,
        item_is_on_surface: impl FnOnce(SurfaceId, ItemId) -> bool,
    ) -> Result<Option<ActivationStart>, ViewportFocusError> {
        let Some(claim) = self.focus_lane.winner else {
            return Ok(None);
        };
        let Some(target) = claim.target.dock() else {
            return Ok(None);
        };
        let request = ViewportActivationRequest::with_disposition(target, claim.pane, claim.cause);
        let mut candidate = self.clone();
        let inherits_lifecycle_barrier =
            !matches!(candidate.focus_lane.barrier, FocusBarrier::None);
        let generation = candidate
            .last_activation
            .checked_next()
            .ok_or(ViewportFocusError::ActivationGenerationExhausted)?;
        candidate.last_activation = generation;
        if let Some(suppression) =
            Self::validate_focus_target(target, claim.pane, is_current_binding, item_is_on_surface)
        {
            if !inherits_lifecycle_barrier {
                candidate.focus_lane.quarantined.clear();
            }
            let activation = ActivationStart {
                generation,
                superseded: None,
                outcome: ActivationStartOutcome::Suppressed(suppression),
            };
            *self = candidate;
            return Ok(Some(activation));
        }
        let Some(observation_baseline) = candidate
            .focus_observations
            .current()
            .map(|observation| observation.generation())
        else {
            if !inherits_lifecycle_barrier {
                candidate.focus_lane.quarantined.clear();
            }
            *self = candidate;
            return Ok(Some(ActivationStart {
                generation,
                superseded: None,
                outcome: ActivationStartOutcome::Suppressed(
                    ActivationSuppression::FocusObservationBaselineUnavailable,
                ),
            }));
        };
        if !can_request_platform_focus {
            if !inherits_lifecycle_barrier {
                candidate.focus_lane.quarantined.clear();
            }
            *self = candidate;
            return Ok(Some(ActivationStart {
                generation,
                superseded: None,
                outcome: ActivationStartOutcome::Suppressed(
                    ActivationSuppression::PlatformFocusControlUnavailable,
                ),
            }));
        }
        // This is a post-lifecycle ordering barrier, not a replay of the
        // winner's original gesture. It therefore emits an exact platform
        // focus effect even when the latest observation already names the
        // winner: the effect must be ordered after the superseded window's
        // show/visible proof.
        let superseded = candidate.focus_lane.driving.take();
        if let Some(record) = candidate.recorded_observe_only_activation.take() {
            candidate.clear_admitted_observe_only(record.generation);
        }
        candidate.focus_lane.driving = Some(PendingViewportActivation {
            generation,
            causal: claim.causal,
            request,
            observation_baseline,
            platform_focus: PendingPlatformFocus::EffectRequired,
        });
        candidate.retarget_lifecycle_focus_barrier(superseded, generation);
        if matches!(candidate.focus_lane.barrier, FocusBarrier::None) {
            candidate.focus_lane.barrier = FocusBarrier::Dock {
                activation: generation,
            };
        }
        let activation = ActivationStart {
            generation,
            superseded,
            outcome: ActivationStartOutcome::RequestPlatformFocus { target },
        };
        *self = candidate;
        Ok(Some(activation))
    }

    pub(crate) fn winning_focus_target(&self) -> Option<ViewportBinding> {
        self.focus_lane.winner.and_then(|claim| claim.target.dock())
    }

    pub(crate) fn superseded_lifecycle_focus_target(&self) -> Option<ViewportBinding> {
        if self.focus_lane.driving.is_some_and(|pending| {
            matches!(
                self.focus_lane.barrier,
                FocusBarrier::Dock { activation } if activation == pending.generation
            )
        }) {
            return None;
        }
        if let Some(observation) = self.focus_observations.current()
            && let Authority::Known(GlobalFocusedWindow::Dock(binding)) = observation.focused
            && let Some(binding) = self
                .focus_lane
                .quarantined
                .iter()
                .copied()
                .find(|candidate| candidate.window() == binding)
        {
            return Some(binding);
        }
        let pending = self.focus_lane.driving?;
        let is_barrier = matches!(
            self.focus_lane.barrier,
            FocusBarrier::Dock { activation } if activation == pending.generation
        );
        if !is_barrier && pending.platform_focus.effect().is_none() {
            return None;
        }
        let winner = self.winning_focus_target()?;
        (pending.request.target != winner).then_some(pending.request.target)
    }

    /// Records an activation after validating its exact binding and explicit pane focus.
    ///
    /// # Errors
    ///
    /// Returns [`ViewportFocusError::ActivationGenerationExhausted`] when the activation
    /// identity domain can no longer advance, or a pane-intent allocation error when an
    /// already-focused activation becomes immediately actionable.
    pub fn request_activation(
        &mut self,
        request: ViewportActivationRequest,
        causal: FocusCausalStamp,
        can_request_platform_focus: bool,
        is_current_binding: impl FnOnce(ViewportBinding) -> bool,
        item_is_on_surface: impl FnOnce(SurfaceId, ItemId) -> bool,
    ) -> Result<ActivationStart, ViewportFocusError> {
        let mut candidate = self.clone();
        let start = candidate.request_activation_in_place(
            request,
            causal,
            can_request_platform_focus,
            is_current_binding,
            item_is_on_surface,
        )?;
        *self = candidate;
        Ok(start)
    }

    pub(crate) fn request_reserved_activation(
        &mut self,
        owner: NativeCreateSagaId,
        request: ViewportActivationRequest,
        causal: FocusCausalStamp,
        can_request_platform_focus: bool,
        is_current_binding: impl FnOnce(ViewportBinding) -> bool,
        item_is_on_surface: impl FnOnce(SurfaceId, ItemId) -> bool,
    ) -> Result<ActivationStart, ViewportFocusError> {
        let expected = NativeActivationReservation { causal, request };
        if self.focus_lane.native_reservations.get(&owner) != Some(&expected) {
            return Err(ViewportFocusError::ActivationReservationMismatch { owner });
        }

        let mut candidate = self.clone();
        let removed = candidate.cancel_activation_reservation(owner);
        debug_assert!(removed, "the reservation was validated before cloning");
        let start = candidate.request_activation_in_place(
            request,
            causal,
            can_request_platform_focus,
            is_current_binding,
            item_is_on_surface,
        )?;
        *self = candidate;
        Ok(start)
    }

    pub(super) fn request_activation_in_place(
        &mut self,
        request: ViewportActivationRequest,
        causal: FocusCausalStamp,
        can_request_platform_focus: bool,
        is_current_binding: impl FnOnce(ViewportBinding) -> bool,
        item_is_on_surface: impl FnOnce(SurfaceId, ItemId) -> bool,
    ) -> Result<ActivationStart, ViewportFocusError> {
        let generation = self
            .last_activation
            .checked_next()
            .ok_or(ViewportFocusError::ActivationGenerationExhausted)?;
        self.last_activation = generation;

        let request_validity = Self::validate_focus_target(
            request.target,
            request.pane,
            is_current_binding,
            item_is_on_surface,
        );
        if let Some(suppression) = request_validity {
            return Ok(ActivationStart {
                generation,
                superseded: None,
                outcome: ActivationStartOutcome::Suppressed(suppression),
            });
        }
        let observation = self.focus_observations.current();
        if request.cause.requests_platform_focus() {
            if self.activation_observation_baseline().is_none() {
                return Ok(ActivationStart {
                    generation,
                    superseded: None,
                    outcome: ActivationStartOutcome::Suppressed(
                        ActivationSuppression::FocusObservationBaselineUnavailable,
                    ),
                });
            }
            let requires_effect = observation
                .is_none_or(|observation| !Self::observation_focuses(observation, request.target))
                || self.focus_lane.driving.is_some();
            if requires_effect && !can_request_platform_focus {
                return Ok(ActivationStart {
                    generation,
                    superseded: None,
                    outcome: ActivationStartOutcome::Suppressed(
                        ActivationSuppression::PlatformFocusControlUnavailable,
                    ),
                });
            }
        }
        if !self.admit_pane_causal_claim(PaneCausalClaim::from_request(causal, request)) {
            if request.cause == ViewportActivationCause::TearOffCommitted
                && self
                    .focus_lane
                    .winner
                    .and_then(|claim| claim.target.dock())
                    .is_some_and(|winner| winner != request.target)
            {
                self.focus_lane.quarantined.insert(request.target);
            }
            return Ok(ActivationStart {
                generation,
                superseded: None,
                outcome: ActivationStartOutcome::Suppressed(
                    ActivationSuppression::CausallySuperseded {
                        current: self
                            .focus_lane
                            .winner
                            .map_or_else(PaneFocusIntentGeneration::default, |floor| {
                                floor.causal.generation()
                            }),
                    },
                ),
            });
        }
        let requires_ordered_compensation = self.focus_lane.driving.is_some_and(|pending| {
            matches!(
                self.focus_lane.barrier,
                FocusBarrier::Dock { activation } if activation == pending.generation
            ) || pending.platform_focus.effect().is_some()
        });
        if !request.cause.requests_platform_focus() && !requires_ordered_compensation {
            let superseded = self.focus_lane.driving.take();
            return self.request_observe_only_activation(
                generation,
                request,
                causal,
                observation,
                superseded,
            );
        }
        if requires_ordered_compensation && !can_request_platform_focus {
            return Ok(ActivationStart {
                generation,
                superseded: None,
                outcome: ActivationStartOutcome::Suppressed(
                    ActivationSuppression::PlatformFocusControlUnavailable,
                ),
            });
        }

        let superseded = self.focus_lane.driving.take();
        self.recorded_observe_only_activation = None;
        let activation = self.request_platform_activation(
            generation,
            request,
            causal,
            observation,
            superseded,
            can_request_platform_focus,
        )?;
        if matches!(
            activation.outcome,
            ActivationStartOutcome::RequestPlatformFocus { .. }
        ) {
            self.retarget_lifecycle_focus_barrier(superseded, generation);
        }
        Ok(activation)
    }

    pub(super) fn request_observe_only_activation(
        &mut self,
        generation: ActivationGeneration,
        request: ViewportActivationRequest,
        causal: FocusCausalStamp,
        observation: Option<FocusObservationEnvelope>,
        superseded: Option<PendingViewportActivation>,
    ) -> Result<ActivationStart, ViewportFocusError> {
        if request.pane == PaneFocusDisposition::Preserve {
            return Ok(ActivationStart {
                generation,
                superseded,
                outcome: ActivationStartOutcome::PaneFocusPreserved {
                    target: request.target,
                },
            });
        }
        if let Some(observation) = observation
            && Self::observation_focuses(observation, request.target)
        {
            self.recorded_observe_only_activation = None;
            let Some(focus) = request.focus() else {
                return Ok(ActivationStart {
                    generation,
                    superseded,
                    outcome: ActivationStartOutcome::PaneFocusPreserved {
                        target: request.target,
                    },
                });
            };
            let intent = self.install_pane_intent(PaneIntentInstall {
                causal,
                activation: Some(generation),
                surface: request.target.surface(),
                native_guard: Some(request.target),
                focus,
                source: request.cause.pane_source(),
                cause: Some(request.cause),
                focus_observation_baseline: observation.generation,
            })?;
            return Ok(ActivationStart {
                generation,
                superseded,
                outcome: Self::pane_intent_outcome(intent),
            });
        }

        let record = RecordedObserveOnlyActivation {
            generation,
            causal,
            request,
            observation_baseline: observation.map(|observation| observation.generation),
        };
        self.recorded_observe_only_activation = Some(record);
        Ok(ActivationStart {
            generation,
            superseded,
            outcome: ActivationStartOutcome::ObserveOnlyRecorded { record },
        })
    }

    pub(super) fn request_platform_activation(
        &mut self,
        generation: ActivationGeneration,
        request: ViewportActivationRequest,
        causal: FocusCausalStamp,
        observation: Option<FocusObservationEnvelope>,
        superseded: Option<PendingViewportActivation>,
        can_request_platform_focus: bool,
    ) -> Result<ActivationStart, ViewportFocusError> {
        let Some(observation_baseline) = self.activation_observation_baseline() else {
            return Ok(ActivationStart {
                generation,
                superseded,
                outcome: ActivationStartOutcome::Suppressed(
                    ActivationSuppression::FocusObservationBaselineUnavailable,
                ),
            });
        };
        if let Some(observation) = observation
            && Self::observation_focuses(observation, request.target)
            && superseded.is_none()
        {
            let Some(focus) = request.focus() else {
                return Ok(ActivationStart {
                    generation,
                    superseded,
                    outcome: ActivationStartOutcome::PaneFocusPreserved {
                        target: request.target,
                    },
                });
            };
            let intent = self.install_pane_intent(PaneIntentInstall {
                causal,
                activation: Some(generation),
                surface: request.target.surface(),
                native_guard: Some(request.target),
                focus,
                source: request.cause.pane_source(),
                cause: Some(request.cause),
                focus_observation_baseline: observation.generation,
            })?;
            return Ok(ActivationStart {
                generation,
                superseded,
                outcome: Self::pane_intent_outcome(intent),
            });
        }

        if !can_request_platform_focus {
            return Ok(ActivationStart {
                generation,
                superseded,
                outcome: ActivationStartOutcome::Suppressed(
                    ActivationSuppression::PlatformFocusControlUnavailable,
                ),
            });
        }

        self.focus_lane.driving = Some(PendingViewportActivation {
            generation,
            causal,
            request,
            observation_baseline,
            platform_focus: PendingPlatformFocus::EffectRequired,
        });
        Ok(ActivationStart {
            generation,
            superseded,
            outcome: ActivationStartOutcome::RequestPlatformFocus {
                target: request.target,
            },
        })
    }

    pub(super) fn pane_intent_outcome(intent: Option<PaneFocusIntent>) -> ActivationStartOutcome {
        intent.map_or(ActivationStartOutcome::PaneIntentSuperseded, |intent| {
            ActivationStartOutcome::PaneFocusReady { intent }
        })
    }

    /// Correlates the effect-ledger identity allocated for a pending activation.
    pub fn attach_platform_focus_effect(
        &mut self,
        activation: ActivationGeneration,
        effect: EffectId,
    ) -> FocusEffectAttachment {
        let Some(pending) = self.focus_lane.driving.as_mut() else {
            return FocusEffectAttachment::StaleActivation;
        };
        if pending.generation != activation {
            return FocusEffectAttachment::StaleActivation;
        }
        match pending.platform_focus {
            PendingPlatformFocus::EffectRequired => {
                pending.platform_focus = PendingPlatformFocus::Requested { effect };
                FocusEffectAttachment::Applied
            }
            PendingPlatformFocus::Requested { effect: existing }
            | PendingPlatformFocus::Indeterminate {
                effect: existing, ..
            }
            | PendingPlatformFocus::ObservedAwaitingTarget {
                effect: existing, ..
            } if existing == effect => FocusEffectAttachment::Duplicate,
            PendingPlatformFocus::Requested { effect: existing }
            | PendingPlatformFocus::Indeterminate {
                effect: existing, ..
            }
            | PendingPlatformFocus::ObservedAwaitingTarget {
                effect: existing, ..
            } => FocusEffectAttachment::EffectAlreadyAttached { existing },
        }
    }

    /// Reduces a platform result which cannot itself claim that focus changed.
    pub fn report_platform_focus_effect(
        &mut self,
        effect: EffectId,
        result: EffectDispatchResult,
    ) -> FocusEffectReportTransition {
        let Some(pending) = self.focus_lane.driving else {
            return self.report_superseded_focus_effect(effect, result);
        };
        let Some(expected) = pending.platform_focus.effect() else {
            return self.report_superseded_focus_effect(effect, result);
        };
        if expected != effect {
            return self.report_superseded_focus_effect(effect, result);
        }
        if matches!(
            pending.platform_focus,
            PendingPlatformFocus::ObservedAwaitingTarget { .. }
        ) {
            return FocusEffectReportTransition::Duplicate;
        }
        match result {
            EffectDispatchResult::Indeterminate(reason) => {
                if matches!(
                    pending.platform_focus,
                    PendingPlatformFocus::Indeterminate {
                        effect: current,
                        reason: current_reason,
                    } if current == effect && current_reason == reason
                ) {
                    return FocusEffectReportTransition::Duplicate;
                }
                self.focus_lane.driving = Some(PendingViewportActivation {
                    platform_focus: PendingPlatformFocus::Indeterminate { effect, reason },
                    ..pending
                });
                FocusEffectReportTransition::Indeterminate {
                    activation: pending.generation,
                }
            }
            EffectDispatchResult::DispatchFailed(_) => {
                self.focus_lane.driving = None;
                self.cancel_lifecycle_focus_barrier(pending.generation);
                FocusEffectReportTransition::Cancelled {
                    activation: pending.generation,
                    reason: ActivationCancellation::EffectFailed,
                }
            }
            EffectDispatchResult::Unsupported(_) => {
                self.focus_lane.driving = None;
                self.cancel_lifecycle_focus_barrier(pending.generation);
                FocusEffectReportTransition::Cancelled {
                    activation: pending.generation,
                    reason: ActivationCancellation::EffectUnsupported,
                }
            }
        }
    }

    pub(super) fn report_superseded_focus_effect(
        &mut self,
        effect: EffectId,
        result: EffectDispatchResult,
    ) -> FocusEffectReportTransition {
        let Some(attempt) = self.focus_lane.hazards.get_mut(&effect) else {
            return FocusEffectReportTransition::StaleEffect;
        };
        match result {
            EffectDispatchResult::Indeterminate(reason) => {
                if matches!(
                    attempt.platform_focus,
                    PendingPlatformFocus::Indeterminate {
                        effect: current,
                        reason: current_reason,
                    } if current == effect && current_reason == reason
                ) {
                    return FocusEffectReportTransition::Duplicate;
                }
                attempt.platform_focus = PendingPlatformFocus::Indeterminate { effect, reason };
                FocusEffectReportTransition::Indeterminate {
                    activation: attempt.generation,
                }
            }
            EffectDispatchResult::DispatchFailed(_) => {
                let activation = attempt.generation;
                self.focus_lane.hazards.remove(&effect);
                self.reconcile_hazard_barrier_after_terminal_effect();
                FocusEffectReportTransition::Cancelled {
                    activation,
                    reason: ActivationCancellation::EffectFailed,
                }
            }
            EffectDispatchResult::Unsupported(_) => {
                let activation = attempt.generation;
                self.focus_lane.hazards.remove(&effect);
                self.reconcile_hazard_barrier_after_terminal_effect();
                FocusEffectReportTransition::Cancelled {
                    activation,
                    reason: ActivationCancellation::EffectUnsupported,
                }
            }
        }
    }

    pub(super) fn reconcile_hazard_barrier_after_terminal_effect(&mut self) {
        let FocusBarrier::Hazard(mut barrier) = self.focus_lane.barrier else {
            return;
        };
        if !self.focus_lane.hazards.is_empty() {
            return;
        }

        let retained_authority = self
            .focus_observations
            .current()
            .and_then(Self::known_focus_target)
            .is_some_and(|target| {
                target
                    .window()
                    .is_none_or(|window| !self.window_is_quarantined(window))
            });
        if retained_authority {
            self.focus_lane.barrier = FocusBarrier::None;
            self.focus_lane.quarantined.clear();
        } else {
            barrier.phase = FocusHazardBarrierPhase::AwaitingPostHazardAuthority;
            self.focus_lane.barrier = FocusBarrier::Hazard(barrier);
        }
    }

    pub(super) fn validate_focus_target(
        target: ViewportBinding,
        pane: PaneFocusDisposition,
        is_current_binding: impl FnOnce(ViewportBinding) -> bool,
        item_is_on_surface: impl FnOnce(SurfaceId, ItemId) -> bool,
    ) -> Option<ActivationSuppression> {
        if !is_current_binding(target) {
            return Some(ActivationSuppression::StaleBinding);
        }
        if let PaneFocusDisposition::Set(item) = pane
            && !item_is_on_surface(target.surface(), item)
        {
            return Some(ActivationSuppression::ItemUnavailable { item });
        }
        None
    }

    pub(super) fn stale_activation_reason(
        request: ViewportActivationRequest,
        surface_exists: impl FnOnce(SurfaceId) -> bool,
        is_current_binding: impl FnOnce(ViewportBinding) -> bool,
        item_is_on_surface: impl FnOnce(SurfaceId, ItemId) -> bool,
    ) -> Option<ActivationCancellation> {
        if !surface_exists(request.target.surface()) {
            return Some(ActivationCancellation::SurfaceRemoved);
        }
        if !is_current_binding(request.target) {
            return Some(ActivationCancellation::StaleBinding);
        }
        if let PaneFocusDisposition::Set(item) = request.pane
            && !item_is_on_surface(request.target.surface(), item)
        {
            return Some(ActivationCancellation::ItemUnavailable { item });
        }
        None
    }
}
