//! Native focus observations, activation and pane-intent lifecycle.

use super::*;

impl DockEngine {
    pub(in crate::engine) fn reduce_global_focus_fact(
        &mut self,
        input: InputSequence,
        provider: PlatformObservationLease,
        expected_epoch: crate::ids::WorkspaceEpoch,
        observation: crate::platform::focus::FocusObservationEnvelope,
        focus_causal: FocusCausalStamp,
        application_base: &mut crate::model::WorkspaceVersion,
        events: &mut Vec<WorkspaceEvent>,
    ) -> Result<InputOutcome, EngineError> {
        if let Some(rejected) = self.platform_provider_rejection(provider) {
            return Ok(rejected);
        }
        if expected_epoch != self.version.epoch() {
            return Ok(InputOutcome::PlatformSnapshotStale {
                expected_epoch,
                current_epoch: self.version.epoch(),
            });
        }
        self.reconcile_viewport_focus_authority();
        let transition = self.reduce_global_focus_observation(
            input,
            provider,
            observation,
            focus_causal,
            events,
        )?;
        *application_base = self.version;
        Ok(InputOutcome::GlobalFocusObservationPublished { transition })
    }

    pub(super) fn reduce_global_focus_observation(
        &mut self,
        input: InputSequence,
        provider: PlatformObservationLease,
        observation: crate::platform::focus::FocusObservationEnvelope,
        focus_causal: FocusCausalStamp,
        events: &mut Vec<WorkspaceEvent>,
    ) -> Result<FocusObservationTransition, EngineError> {
        let observed = self.focus_binding_snapshot(|record| record.can_observe_focus());
        let activatable = self.focus_binding_snapshot(|record| record.can_accept_activation());
        let reportable = self.focus_fact_window_snapshot(|record| record.can_report_focus_fact());
        let isolated = self.focus_fact_window_snapshot(|record| {
            record.admission() == crate::platform::registry::ViewportAdmission::Pending
                || self.native_source_surface_is_held(record.binding().surface())
        });
        let mut retired_binding = false;
        if let Authority::Known(crate::platform::focus::GlobalFocusedWindow::Dock(binding)) =
            observation.focused()
            && !reportable.contains(binding)
        {
            if self.viewport.recognizes_retired_native_window(*binding) {
                retired_binding = true;
            } else {
                return Err(EngineError::GlobalFocusBindingUnavailable {
                    input,
                    binding: *binding,
                });
            }
        }
        let acknowledged_effect = match *observation.acknowledged_effect() {
            Authority::Known(Some(effect))
                if self.focus_effect_acknowledgement_is_valid(provider, effect) =>
            {
                Authority::Known(Some(effect))
            }
            Authority::Known(Some(_) | None) => Authority::Known(None),
            Authority::Unknown(reason) => Authority::Unknown(reason),
        };
        let observation = crate::platform::focus::FocusObservationEnvelope::new(
            observation.generation(),
            *observation.focused(),
            acknowledged_effect,
        );
        let items = self.focus_item_snapshot();
        let mut transition = self
            .viewport_focus
            .publish_platform_focus_observation_with_retired_binding(
                observation,
                focus_causal,
                retired_binding,
                |binding| observed.contains(&binding),
                |binding| activatable.contains(&binding),
                |binding| isolated.contains(&binding),
                |surface, item| {
                    items
                        .get(&surface)
                        .is_some_and(|surface_items| surface_items.contains(&item))
                },
            )
            .map_err(|source| EngineError::ViewportFocus { input, source })?;
        if let FocusObservationTransition::Applied(applied) = &mut transition
            && let Some(intent) = applied.pane_intent()
            && let Some(reason) = self.reveal_pane_focus_intent(intent.causal(), intent, events)?
        {
            applied.reject_pane_reveal(intent, reason);
        }
        if let Some(applied) = transition.effect_settlement_mut() {
            let observed = applied.observed_effects().to_vec();
            for observed in observed {
                if !self.settle_observed_focus_effect(provider, observed) {
                    applied.discard_observed_effect(observed.effect());
                }
            }
            if let Some(observed) = self.settle_acknowledged_focus_effect(provider, observation) {
                applied.record_acknowledged_effect_settlement(observed);
            }
        }
        if let Some(stale_target) = self.viewport_focus.superseded_lifecycle_focus_target() {
            let _ =
                self.establish_winning_focus_lifecycle_barrier(stale_target, focus_causal, events)?;
        }
        Ok(transition)
    }

    fn focus_effect_acknowledgement_is_valid(
        &self,
        provider: PlatformObservationLease,
        effect: crate::platform::effect::EffectId,
    ) -> bool {
        let Some(binding) = self.focus_effect_binding(effect) else {
            return false;
        };
        self.viewport
            .viewport(binding.surface())
            .filter(|record| record.can_observe_focus())
            .map(crate::platform::registry::ViewportRecord::binding)
            == Some(binding)
            && self
                .viewport
                .focus_effect_observation_matches(provider, effect, binding)
    }

    fn settle_acknowledged_focus_effect(
        &mut self,
        provider: PlatformObservationLease,
        observation: crate::platform::focus::FocusObservationEnvelope,
    ) -> Option<ObservedPlatformFocusEffect> {
        let Authority::Known(Some(effect)) = *observation.acknowledged_effect() else {
            return None;
        };
        let binding = self.focus_effect_binding(effect)?;
        let observed = ObservedPlatformFocusEffect::new(
            effect,
            binding,
            observation.generation(),
            PlatformFocusEvidence::ExactEffectAcknowledgement,
        );
        self.settle_observed_focus_effect(provider, observed)
            .then_some(observed)
    }

    fn settle_observed_focus_effect(
        &mut self,
        provider: PlatformObservationLease,
        observed: ObservedPlatformFocusEffect,
    ) -> bool {
        let Some(binding) = self.focus_effect_binding(observed.effect()) else {
            return false;
        };
        if binding != observed.binding()
            || self
                .viewport
                .viewport(binding.surface())
                .filter(|record| record.can_observe_focus())
                .map(crate::platform::registry::ViewportRecord::binding)
                != Some(binding)
        {
            return false;
        }
        self.viewport
            .observe_focus_effect(provider, observed.effect(), binding)
            == EffectTransition::Applied
    }

    fn focus_effect_binding(
        &self,
        effect: crate::platform::effect::EffectId,
    ) -> Option<crate::platform::viewport::ViewportBinding> {
        let record = self.viewport.effects().record(effect)?;
        match record.request().effect() {
            crate::platform::effect::PlatformEffect::RequestFocus { binding, .. } => Some(*binding),
            _ => None,
        }
    }

    pub(in crate::engine) fn reduce_viewport_activation(
        &mut self,
        expected: WorkspaceVersion,
        request: ViewportActivationRequest,
        focus_causal: FocusCausalStamp,
        application_base: WorkspaceVersion,
        events: &mut Vec<WorkspaceEvent>,
    ) -> Result<InputOutcome, EngineError> {
        if expected != application_base {
            return Ok(InputOutcome::StaleRejected {
                expected,
                accepted_base: application_base,
            });
        }
        if request.cause() != crate::platform::focus::ViewportActivationCause::Explicit {
            return Ok(InputOutcome::ViewportActivationRejected { request });
        }
        let activation = self.start_viewport_activation(request, focus_causal, events)?;
        Ok(InputOutcome::ViewportActivationRequested { activation })
    }

    pub(in crate::engine) fn start_viewport_activation(
        &mut self,
        request: ViewportActivationRequest,
        focus_causal: FocusCausalStamp,
        events: &mut Vec<WorkspaceEvent>,
    ) -> Result<ActivationStart, EngineError> {
        self.start_viewport_activation_with_owner(None, request, focus_causal, events)
    }

    fn try_start_reserved_viewport_activation(
        &mut self,
        owner: NativeCreateSagaId,
        request: ViewportActivationRequest,
        focus_causal: FocusCausalStamp,
        events: &mut Vec<WorkspaceEvent>,
    ) -> Result<Option<ActivationStart>, EngineError> {
        let bindings = self.focus_binding_snapshot(|record| record.can_accept_activation());
        if !bindings.contains(&request.target()) {
            return Ok(None);
        }
        let owner_is_winner = self
            .viewport_focus
            .winning_activation_reservation()
            .is_some_and(|(winner, _, _)| winner == owner);
        if owner_is_winner {
            let items = self.focus_item_snapshot();
            let item_is_current = match request.pane() {
                PaneFocusDisposition::Set(item) => items
                    .get(&request.target().surface())
                    .is_some_and(|surface_items| surface_items.contains(&item)),
                PaneFocusDisposition::Preserve | PaneFocusDisposition::Clear => true,
            };
            if item_is_current {
                if self
                    .viewport_focus
                    .activation_observation_baseline()
                    .is_none()
                {
                    return Ok(None);
                }
                let target_is_focused =
                    self.viewport_focus
                        .focus_observation()
                        .is_some_and(|observation| {
                            matches!(
                                observation.focused(),
                                Authority::Known(
                                    crate::platform::focus::GlobalFocusedWindow::Dock(binding)
                                ) if *binding == request.target().window()
                            )
                        });
                if !target_is_focused
                    && !self
                        .viewport
                        .capabilities()
                        .window_activation_control()
                        .is_supported()
                {
                    return Ok(None);
                }
            }
        }
        self.start_viewport_activation_with_owner(Some(owner), request, focus_causal, events)
            .map(Some)
    }

    pub(in crate::engine) fn settle_native_activation_reservations(
        &mut self,
        events: &mut Vec<WorkspaceEvent>,
    ) -> Result<(), EngineError> {
        // First remove only reservations whose owner is terminal and whose exact
        // target disappeared in this tick. This must happen before winner
        // selection: a vanished newer reservation cannot suppress an older one.
        for (owner, _, request) in self.viewport_focus.activation_reservations() {
            if self.viewport.native_create_saga(owner).is_none()
                && !self.native_activation_target_is_current(request.target())
            {
                let _ = self.viewport_focus.cancel_activation_reservation(owner);
            }
        }

        // A newer reservation is authority even before its native window has
        // committed. Do not consume any older owner until that winner can be
        // settled, otherwise cancellation of the newer saga would lose the
        // older release-time claim.
        while let Some((owner, focus_causal, request)) =
            self.viewport_focus.winning_activation_reservation()
        {
            if self.viewport.native_create_saga(owner).is_some()
                || !self.native_activation_target_is_current(request.target())
            {
                return Ok(());
            }
            if self
                .try_start_reserved_viewport_activation(owner, request, focus_causal, events)?
                .is_none()
            {
                return Ok(());
            }
        }

        // Remaining owners are causally older than a settled winner. They may
        // be consumed only when the associated late-autofocus compensation can
        // be established atomically; otherwise leave the reservation available
        // for a future authoritative capability/focus snapshot.
        for (owner, focus_causal, request) in self.viewport_focus.activation_reservations() {
            if self.viewport.native_create_saga(owner).is_some() {
                break;
            }
            if !self.native_activation_target_is_current(request.target()) {
                let _ = self.viewport_focus.cancel_activation_reservation(owner);
                continue;
            }
            if !self.try_settle_superseded_native_activation_reservation(
                owner,
                request,
                focus_causal,
                events,
            )? {
                break;
            }
        }
        Ok(())
    }

    fn native_activation_target_is_current(&self, target: ViewportBinding) -> bool {
        self.workspace.surface(target.surface()).is_some()
            && self
                .viewport
                .viewport(target.surface())
                .is_some_and(|record| record.binding() == target)
    }

    fn try_settle_superseded_native_activation_reservation(
        &mut self,
        owner: NativeCreateSagaId,
        request: ViewportActivationRequest,
        focus_causal: FocusCausalStamp,
        events: &mut Vec<WorkspaceEvent>,
    ) -> Result<bool, EngineError> {
        let target = request.target();
        if self.viewport_focus.focus_observation().is_none() {
            return Ok(false);
        }
        if !self
            .viewport
            .capabilities()
            .window_activation_control()
            .is_supported()
        {
            return Ok(false);
        }

        let winner = self.viewport_focus.winning_focus_target();
        if let Some(winner) = winner {
            let activatable = self.focus_binding_snapshot(|record| record.can_accept_activation());
            if winner != target && !activatable.contains(&winner) {
                return Ok(false);
            }
        }

        let mut candidate = self.candidate();
        let mut candidate_events = Vec::new();
        let Some(_) = candidate.try_start_reserved_viewport_activation(
            owner,
            request,
            focus_causal,
            &mut candidate_events,
        )?
        else {
            return Ok(false);
        };

        if let Some(winner) = winner
            && winner != target
            && !candidate
                .viewport_focus
                .lifecycle_barrier_protects(target, winner)
        {
            return Ok(false);
        }

        *self = candidate;
        events.extend(candidate_events);
        Ok(true)
    }

    fn start_viewport_activation_with_owner(
        &mut self,
        owner: Option<NativeCreateSagaId>,
        request: ViewportActivationRequest,
        focus_causal: FocusCausalStamp,
        events: &mut Vec<WorkspaceEvent>,
    ) -> Result<ActivationStart, EngineError> {
        let bindings = self.focus_binding_snapshot(|record| record.can_accept_activation());
        let items = self.focus_item_snapshot();
        let control_supported = self
            .viewport
            .capabilities()
            .window_activation_control()
            .is_supported();
        let activation_result = match owner {
            None => self.viewport_focus.request_activation(
                request,
                focus_causal,
                control_supported,
                |binding| bindings.contains(&binding),
                |surface, item| {
                    items
                        .get(&surface)
                        .is_some_and(|surface_items| surface_items.contains(&item))
                },
            ),
            Some(owner) => self.viewport_focus.request_reserved_activation(
                owner,
                request,
                focus_causal,
                control_supported,
                |binding| bindings.contains(&binding),
                |surface, item| {
                    items
                        .get(&surface)
                        .is_some_and(|surface_items| surface_items.contains(&item))
                },
            ),
        };
        let mut activation =
            activation_result.map_err(|source| EngineError::CausedViewportFocus {
                cause: focus_causal.cause(),
                source,
            })?;
        let stale_tear_off_was_superseded = request.cause()
            == crate::platform::focus::ViewportActivationCause::TearOffCommitted
            && matches!(
                activation.outcome(),
                ActivationStartOutcome::Suppressed(
                    crate::platform::focus::ActivationSuppression::CausallySuperseded { .. }
                )
            );
        if stale_tear_off_was_superseded
            && self
                .viewport_focus
                .winning_focus_target()
                .is_some_and(|winner| winner != request.target())
        {
            if let Some(compensation) = self.establish_winning_focus_lifecycle_barrier(
                request.target(),
                focus_causal,
                events,
            )? {
                activation = compensation;
            }
        }
        if let ActivationStartOutcome::PaneFocusReady { intent } = activation.outcome()
            && let Some(reason) = self.reveal_pane_focus_intent(intent.causal(), intent, events)?
        {
            activation = activation.reject_pane_reveal(intent, reason);
        }
        Ok(activation)
    }

    fn establish_winning_focus_lifecycle_barrier(
        &mut self,
        stale_target: crate::platform::viewport::ViewportBinding,
        focus_causal: FocusCausalStamp,
        events: &mut Vec<WorkspaceEvent>,
    ) -> Result<Option<ActivationStart>, EngineError> {
        let Some(winner) = self.viewport_focus.winning_focus_target() else {
            return Ok(None);
        };
        if winner == stale_target {
            return Ok(None);
        }

        let bindings = self.focus_binding_snapshot(|record| record.can_accept_activation());
        if !bindings.contains(&winner) {
            return Ok(None);
        }
        let items = self.focus_item_snapshot();
        let control_supported = self
            .viewport
            .capabilities()
            .window_activation_control()
            .is_supported();
        let Some(mut compensation) = self
            .viewport_focus
            .request_winning_lifecycle_compensation(
                control_supported,
                |binding| bindings.contains(&binding),
                |surface, item| {
                    items
                        .get(&surface)
                        .is_some_and(|surface_items| surface_items.contains(&item))
                },
            )
            .map_err(|source| EngineError::CausedViewportFocus {
                cause: focus_causal.cause(),
                source,
            })?
        else {
            return Ok(None);
        };
        if let ActivationStartOutcome::PaneFocusReady { intent } = compensation.outcome()
            && let Some(reason) = self.reveal_pane_focus_intent(intent.causal(), intent, events)?
        {
            compensation = compensation.reject_pane_reveal(intent, reason);
        }
        Ok(Some(compensation))
    }

    /// Allocates the one surviving pending native-focus request only after all
    /// host-frame workspace and viewport lifecycle changes have settled.
    ///
    /// An activation can be valid at the instant a native surface becomes
    /// visible yet become invalid before the same host frame commits, for
    /// example when a later command vacates that surface. Allocating here
    /// prevents an already-detached binding from being emitted to the adapter.
    pub(in crate::engine) fn emit_pending_platform_focus_effect(
        &mut self,
        input: InputSequence,
    ) -> Result<(), EngineError> {
        let Some(pending) = self.viewport_focus.pending_activation() else {
            return Ok(());
        };
        if pending.platform_focus() != PendingPlatformFocus::EffectRequired {
            return Ok(());
        }

        let effect = self
            .viewport
            .request_focus_binding(pending.request().target())
            .map_err(|source| EngineError::Viewport { input, source })?;
        if self
            .viewport_focus
            .attach_platform_focus_effect(pending.generation(), effect)
            != crate::platform::focus::FocusEffectAttachment::Applied
        {
            return Err(EngineError::ViewportFocus {
                input,
                source: ViewportFocusError::EffectAttachmentInvariant,
            });
        }
        Ok(())
    }

    fn reveal_pane_focus_intent(
        &mut self,
        focus_causal: FocusCausalStamp,
        intent: PaneFocusIntent,
        events: &mut Vec<WorkspaceEvent>,
    ) -> Result<Option<PaneFocusRevealRejection>, EngineError> {
        let PanelFocus::Item(item) = intent.focus() else {
            return Ok(None);
        };
        let Some(source) =
            Self::capture_surface_item_source(&self.workspace, intent.surface(), item)
        else {
            return self.reject_pane_focus_reveal(
                focus_causal,
                intent,
                PaneFocusRevealRejection::ItemUnavailable { item },
            );
        };
        let command = WorkspaceCommand::Select { source };
        let application = match focus_causal.cause() {
            ReductionCause::Input { input, .. } => {
                self.apply_interaction_command(input, &command, events)?
            }
            cause => match self.apply_journal_workspace_command(
                cause,
                &command,
                &self.policy.clone(),
                events,
            )? {
                Ok((outcome, changed)) => CommandApplication::Applied { outcome, changed },
                Err(error) => CommandApplication::Rejected(error),
            },
        };
        match application {
            CommandApplication::Applied { .. } => Ok(None),
            CommandApplication::Rejected(error) => {
                let reason = match error {
                    crate::graph::error::CommandError::SurfaceLifecycleFrozen { surface } => {
                        PaneFocusRevealRejection::SurfaceLifecycleFrozen { surface }
                    }
                    crate::graph::error::CommandError::Policy(_) => {
                        PaneFocusRevealRejection::PolicyRejected
                    }
                    crate::graph::error::CommandError::MissingRoot { .. }
                    | crate::graph::error::CommandError::MissingNode { .. }
                    | crate::graph::error::CommandError::NodeOutsideRoot { .. }
                    | crate::graph::error::CommandError::StaleNode { .. }
                    | crate::graph::error::CommandError::NodeIsNotTabs { .. }
                    | crate::graph::error::CommandError::ItemNotInTabs { .. } => {
                        PaneFocusRevealRejection::ItemUnavailable { item }
                    }
                    _ => PaneFocusRevealRejection::WorkspaceRejected,
                };
                self.reject_pane_focus_reveal(focus_causal, intent, reason)
            }
        }
    }

    fn reject_pane_focus_reveal(
        &mut self,
        focus_causal: FocusCausalStamp,
        intent: PaneFocusIntent,
        reason: PaneFocusRevealRejection,
    ) -> Result<Option<PaneFocusRevealRejection>, EngineError> {
        if !self.viewport_focus.reject_pane_reveal(intent.id()) {
            return Err(EngineError::CausedViewportFocus {
                cause: focus_causal.cause(),
                source: ViewportFocusError::PaneRevealInvariant,
            });
        }
        Ok(Some(reason))
    }

    pub(in crate::engine) fn reduce_pane_focus_observation(
        &mut self,
        expected_epoch: crate::ids::WorkspaceEpoch,
        observation: PaneFocusObservation,
    ) -> InputOutcome {
        if expected_epoch != self.version.epoch() {
            return InputOutcome::PaneFocusObservationStale {
                expected_epoch,
                current_epoch: self.version.epoch(),
            };
        }
        let bindings = self.focus_binding_snapshot(|record| record.can_observe_focus());
        let items = self.focus_item_snapshot();
        let transition = self.viewport_focus.publish_pane_focus_observation(
            observation,
            |binding| bindings.contains(&binding),
            |surface, item| {
                items
                    .get(&surface)
                    .is_some_and(|surface_items| surface_items.contains(&item))
            },
        );
        InputOutcome::PaneFocusObservationPublished { transition }
    }

    fn focus_binding_snapshot(
        &self,
        include: impl Fn(crate::platform::registry::ViewportRecord<'_>) -> bool,
    ) -> BTreeSet<crate::platform::viewport::ViewportBinding> {
        self.viewport
            .registry()
            .records()
            .filter_map(|(surface, record)| {
                (self.workspace.surface(surface).is_some() && include(record))
                    .then_some(record.binding())
            })
            .collect()
    }

    fn focus_fact_window_snapshot(
        &self,
        include: impl Fn(crate::platform::registry::ViewportRecord<'_>) -> bool,
    ) -> BTreeSet<crate::platform::viewport::WindowBinding> {
        self.viewport
            .registry()
            .records()
            .filter_map(|(_, record)| include(record).then_some(record.binding().window()))
            .collect()
    }

    fn focus_item_snapshot(&self) -> BTreeMap<crate::ids::SurfaceId, BTreeSet<crate::ids::ItemId>> {
        self.workspace
            .surfaces()
            .map(|(surface, _)| (surface, self.surface_items(surface)))
            .collect()
    }

    pub(in crate::engine) fn reconcile_viewport_focus_authority(&mut self) {
        let presentation = &self.presentation_authority.presentation;
        let workspace = &self.workspace;
        let _ = self
            .viewport_focus
            .reconcile_host_focus_scopes(|surface, host, stream| {
                workspace.surface(surface).is_some()
                    && presentation.active_surface_scope(surface).is_some_and(
                        |(active_stream, active_host, _)| {
                            active_stream == stream && active_host == host
                        },
                    )
            });
        let observed = self.focus_binding_snapshot(|record| record.can_observe_focus());
        let activatable = self.focus_binding_snapshot(|record| record.can_accept_activation());
        let items = self.focus_item_snapshot();
        self.viewport_focus
            .resume_workspace_replacement_focus(|surface| {
                activatable
                    .iter()
                    .find(|binding| binding.surface() == surface)
                    .copied()
            });
        let _ = self.viewport_focus.reconcile_authority(
            |surface| items.contains_key(&surface),
            |binding| observed.contains(&binding),
            |binding| activatable.contains(&binding),
            |surface, item| {
                items
                    .get(&surface)
                    .is_some_and(|surface_items| surface_items.contains(&item))
            },
        );
    }
}
