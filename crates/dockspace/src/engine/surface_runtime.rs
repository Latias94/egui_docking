//! Native viewport, focus, destruction, and recovery runtime reduction.

use super::*;

impl DockEngine {
    pub(super) fn reduce_child_viewport_bootstrap_registration(
        &mut self,
        input: InputSequence,
        provider: PlatformObservationLease,
        expected: WorkspaceVersion,
        surface: crate::ids::SurfaceId,
        token: WindowToken,
        recovery: SurfaceRecoveryBootstrap,
    ) -> Result<InputOutcome, EngineError> {
        if let Some(rejected) = self.platform_provider_rejection(provider) {
            return Ok(rejected);
        }
        if expected != self.version {
            return Ok(InputOutcome::StaleRejected {
                expected,
                accepted_base: self.version,
            });
        }
        let Some(main_root) = self
            .workspace
            .surface(surface)
            .map(|presentation| presentation.main_root)
        else {
            return Ok(InputOutcome::ViewportRegistrationRejected { surface });
        };
        let host_surface = recovery.host_surface();
        let Some(anchor) = self.root_recovery_anchors.get(&host_surface).copied() else {
            return Ok(InputOutcome::ViewportRegistrationRejected { surface });
        };
        if host_surface == surface
            || self.workspace.surface(host_surface).is_none()
            || self.viewport.recovery_pending(surface).is_some()
            || self.viewport.viewport(surface).is_some()
            || self.root_recovery_anchors.contains_key(&surface)
            || self.bound_surface_recoveries.contains_key(&surface)
        {
            return Ok(InputOutcome::ViewportRegistrationRejected { surface });
        }

        let identity_before = self.presentation_identity;
        let target = if let Some(main_root) = main_root {
            let floating = self.reserve_presentation_floating_identity()?;
            SurfaceRecoveryTarget::with_converted_main(
                anchor,
                ConvertedMainRecovery::new(
                    main_root,
                    floating,
                    self.presentation_config().minimum_floating_size(),
                ),
            )
        } else {
            SurfaceRecoveryTarget::forest_only(anchor)
        };
        let outcome = self.reduce_viewport_registration(
            input,
            provider,
            expected,
            surface,
            token,
            ViewportRole::Child,
            Some(target),
            ViewportOwnership::RuntimeOwned,
        )?;
        if !matches!(outcome, InputOutcome::ViewportRegistered { .. }) {
            self.presentation_identity = identity_before;
        }
        Ok(outcome)
    }

    pub(super) fn reduce_viewport_registration(
        &mut self,
        input: InputSequence,
        provider: PlatformObservationLease,
        expected: WorkspaceVersion,
        surface: crate::ids::SurfaceId,
        token: WindowToken,
        role: ViewportRole,
        recovery_target: Option<SurfaceRecoveryTarget>,
        ownership: ViewportOwnership,
    ) -> Result<InputOutcome, EngineError> {
        if let Some(rejected) = self.platform_provider_rejection(provider) {
            return Ok(rejected);
        }
        if expected != self.version {
            return Ok(InputOutcome::StaleRejected {
                expected,
                accepted_base: self.version,
            });
        }
        let Some(presentation) = self.workspace.surface(surface) else {
            return Ok(InputOutcome::ViewportRegistrationRejected { surface });
        };
        if (role == ViewportRole::Child) != recovery_target.is_some() {
            return Ok(InputOutcome::ViewportRegistrationRejected { surface });
        }
        let pending_obligation = self
            .viewport
            .recovery_pending(surface)
            .map(crate::frame::RecoveryPending::recovery_obligation);
        if let Some(target) = recovery_target {
            if target.host_surface() == surface
                || self.workspace.surface(target.host_surface()).is_none()
            {
                return Ok(InputOutcome::ViewportRegistrationRejected { surface });
            }
            match (presentation.main_root, target.converted_main()) {
                (None, None) => {}
                (Some(main_root), Some(converted)) if converted.source_root() == main_root => {
                    let floating = converted.floating();
                    let reserved_elsewhere =
                        self.bound_surface_recoveries
                            .iter()
                            .any(|(reserved_surface, bound)| {
                                *reserved_surface != surface
                                    && bound
                                        .obligation
                                        .target()
                                        .converted_main()
                                        .is_some_and(|reserved| reserved.floating() == floating)
                            });
                    if self.workspace.contained_floating(floating).is_some()
                        || reserved_elsewhere
                        || self.native_create_reserves_floating(floating)
                    {
                        return Ok(InputOutcome::ViewportRegistrationRejected { surface });
                    }
                }
                (Some(_), None) | (None, Some(_)) | (Some(_), Some(_)) => {
                    return Ok(InputOutcome::ViewportRegistrationRejected { surface });
                }
            }
        }
        let mut new_obligation = None;
        let mut new_anchor = None;
        let recovery_obligation = if role == ViewportRole::Child {
            let target = recovery_target.ok_or(EngineError::ReductionCauseInvariant {
                detail: "child registration omitted its recovery target after validation",
            })?;
            if let Some(id) = pending_obligation {
                let Some(pending) = self.viewport.recovery_pending(surface) else {
                    return Err(EngineError::ReductionCauseInvariant {
                        detail: "pending recovery disappeared during registration",
                    });
                };
                let Some(bound) = self.bound_surface_recoveries.get(&surface) else {
                    return Ok(InputOutcome::ViewportRegistrationRejected { surface });
                };
                let obligation = &bound.obligation;
                let request = match Self::recovery_policy_request(
                    &self.workspace,
                    surface,
                    obligation.request().host_surface(),
                ) {
                    Ok(request) => request,
                    Err(_) => return Ok(InputOutcome::ViewportRegistrationRejected { surface }),
                };
                if pending.replacement_binding().is_some()
                    || pending.role() != role
                    || bound.binding != pending.destroyed_binding()
                    || obligation.id() != id
                    || obligation.authority_domain() != self.authority_domain
                    || target != obligation.target()
                    || request != *obligation.request()
                    || !self
                        .surface_recovery
                        .pending(surface)
                        .is_some_and(|roster| roster.matches_workspace(&self.workspace))
                {
                    return Ok(InputOutcome::ViewportRegistrationRejected { surface });
                }
                Some(id)
            } else {
                let anchor_is_current = target.anchor().authority_domain() == self.authority_domain
                    && self.root_recovery_anchors.get(&target.host_surface())
                        == Some(&target.anchor());
                if !anchor_is_current
                    || self.root_recovery_anchors.contains_key(&surface)
                    || self.bound_surface_recoveries.contains_key(&surface)
                {
                    return Ok(InputOutcome::ViewportRegistrationRejected { surface });
                }
                let id = self.next_surface_recovery_obligation_id(input)?;
                let obligation = match self.authorize_surface_recovery_obligation(
                    input,
                    id,
                    &self.workspace,
                    surface,
                    target,
                    &self.policy,
                ) {
                    Ok(obligation) => obligation,
                    Err(_) => return Ok(InputOutcome::ViewportRegistrationRejected { surface }),
                };
                new_obligation = Some(obligation);
                Some(id)
            }
        } else {
            if role == ViewportRole::Root && self.bound_surface_recoveries.contains_key(&surface) {
                return Ok(InputOutcome::ViewportRegistrationRejected { surface });
            }
            if role == ViewportRole::Root && !self.root_recovery_anchors.contains_key(&surface) {
                let id = self
                    .last_root_recovery_anchor
                    .checked_next()
                    .ok_or(EngineError::RootRecoveryAnchorExhausted { input })?;
                new_anchor = Some(RootRecoveryAnchor::new(self.authority_domain, id, surface));
            }
            None
        };
        let registration = match ownership {
            ViewportOwnership::External => self.viewport.register_existing(
                self.version.epoch(),
                surface,
                token,
                role,
                recovery_obligation,
            ),
            ViewportOwnership::RuntimeOwned => {
                if recovery_obligation.is_none() {
                    return Ok(InputOutcome::ViewportRegistrationRejected { surface });
                }
                if role != ViewportRole::Child {
                    return Ok(InputOutcome::ViewportRegistrationRejected { surface });
                }
                self.viewport
                    .register_owned_child(self.version.epoch(), surface, token)
            }
        };
        let binding = match registration {
            Ok(binding) => binding,
            Err(ViewportCoordinatorError::PendingRecoveryRegistrationMismatch { .. }) => {
                return Ok(InputOutcome::ViewportRegistrationRejected { surface });
            }
            Err(source) => return Err(EngineError::Viewport { input, source }),
        };
        if let Some(obligation) = new_obligation {
            self.last_surface_recovery_obligation = obligation.id();
            self.bound_surface_recoveries
                .insert(surface, BoundSurfaceRecovery::new(binding, obligation));
        }
        if let Some(anchor) = new_anchor {
            self.last_root_recovery_anchor = anchor.id();
            self.root_recovery_anchors.insert(surface, anchor);
        }
        let coordinate_result = self.invalidate_surface_coordinate_scene(surface);
        coordinate_result.map_err(|source| Self::scene_revision_error(input, source))?;
        Ok(InputOutcome::ViewportRegistered { binding })
    }

    pub(super) fn reduce_platform_snapshot(
        &mut self,
        input: InputSequence,
        provider: PlatformObservationLease,
        expected_epoch: crate::ids::WorkspaceEpoch,
        snapshot: &PlatformSnapshot,
        focus_causal: FocusCausalStamp,
        context: PlatformSnapshotReductionContext<'_>,
    ) -> Result<InputOutcome, EngineError> {
        let PlatformSnapshotReductionContext {
            application_base,
            events,
            interaction_events,
        } = context;
        if let Some(rejected) = self.platform_provider_rejection(provider) {
            return Ok(rejected);
        }
        if expected_epoch != self.version.epoch() {
            return Ok(InputOutcome::PlatformSnapshotStale {
                expected_epoch,
                current_epoch: self.version.epoch(),
            });
        }
        let previous_physical_native_drag = self.viewport.physical_native_drag_capability();
        let previous_physical_cross_surface_drag =
            self.viewport.physical_cross_surface_drag_capability();
        let interaction_dependencies = self.platform_interaction_dependencies();
        let version_before_actions = self.version;
        let transition = self
            .viewport
            .publish_snapshot(snapshot)
            .map_err(|source| EngineError::Viewport { input, source })?;
        let destroyed_bindings = transition
            .registry_events()
            .iter()
            .filter_map(|event| match event {
                crate::viewport_registry::RegistryEvent::Destroyed { observation } => {
                    Some(observation.binding())
                }
                crate::viewport_registry::RegistryEvent::Ready { .. }
                | crate::viewport_registry::RegistryEvent::PresentationChanged { .. }
                | crate::viewport_registry::RegistryEvent::FactsUnavailable { .. }
                | crate::viewport_registry::RegistryEvent::BindingMissing { .. }
                | crate::viewport_registry::RegistryEvent::CloseRequested { .. }
                | crate::viewport_registry::RegistryEvent::CloseRequestCleared { .. } => None,
            })
            .collect::<BTreeSet<_>>();
        let mut native_close_edges = Vec::new();
        for event in transition.registry_events() {
            match event {
                crate::viewport_registry::RegistryEvent::CloseRequested { observation } => {
                    if transition.pre_admission_close_was_consumed(*observation) {
                        continue;
                    }
                    native_close_edges.push(NativeCloseEdge::from_authoritative_requested(
                        self.authority_domain,
                        observation.binding(),
                        observation.generation(),
                        observation.inventory_generation(),
                    ));
                }
                crate::viewport_registry::RegistryEvent::CloseRequestCleared {
                    requested,
                    observation,
                } if !destroyed_bindings.contains(&observation.binding()) => {
                    self.settle_cleared_native_surface_close(provider, *requested, *observation);
                }
                crate::viewport_registry::RegistryEvent::Destroyed { observation } => {
                    let _ = self
                        .viewport_focus
                        .observe_destroyed_binding(observation.binding());
                }
                crate::viewport_registry::RegistryEvent::Ready { .. }
                | crate::viewport_registry::RegistryEvent::PresentationChanged { .. }
                | crate::viewport_registry::RegistryEvent::FactsUnavailable { .. }
                | crate::viewport_registry::RegistryEvent::BindingMissing { .. }
                | crate::viewport_registry::RegistryEvent::CloseRequestCleared { .. } => {}
            }
        }
        self.reconcile_viewport_focus_authority();
        let focus = self.reduce_global_focus_observation(
            input,
            provider,
            snapshot.focus(),
            focus_causal,
            self.platform_focus_restore_gate(),
            events,
        )?;
        let actions = transition.actions().to_vec();
        let recovery_batch = self.freeze_surface_recovery_batch(input, &actions)?;
        let invalidated_scene_authorities = self.invalidate_changed_surface_scene_authority();
        self.demote_invalidated_surface_scenes(input, &invalidated_scene_authorities)?;
        let mut activations = Vec::new();
        self.reduce_viewport_actions(
            input,
            provider,
            focus_causal,
            &actions,
            &recovery_batch,
            &mut activations,
            events,
            interaction_events,
        )?;
        self.issue_admitted_root_recovery_anchors(input)?;
        self.surface_recovery
            .retain_pending(|surface| self.viewport.recovery_pending(surface).is_some());
        *application_base = self.version;
        match self.platform_interaction_reconciliation(
            &interaction_dependencies,
            version_before_actions,
            &transition,
            previous_physical_native_drag,
            previous_physical_cross_surface_drag,
            &invalidated_scene_authorities.bindings,
        ) {
            PlatformInteractionReconciliation::Preserve => {}
            PlatformInteractionReconciliation::ClearDragFeedback {
                workspace_changed,
                end_routing,
            } => {
                self.clear_platform_drag_feedback(
                    input,
                    workspace_changed,
                    end_routing,
                    interaction_events,
                )?;
            }
            PlatformInteractionReconciliation::Cancel(
                InteractionCancelReason::SceneUnavailable,
            ) => {
                self.cancel_active_interaction_preserving_scene(
                    input,
                    InteractionCancelReason::SceneUnavailable,
                    interaction_events,
                )?;
            }
            PlatformInteractionReconciliation::Cancel(reason) => {
                self.invalidate_transient(input, reason, interaction_events)?;
            }
        }
        Ok(InputOutcome::PlatformSnapshotPublished {
            transition,
            focus,
            activations,
            native_close_edges,
        })
    }

    pub(super) fn reduce_native_close_observation(
        &mut self,
        input: InputSequence,
        provider: PlatformObservationLease,
        expected_epoch: crate::ids::WorkspaceEpoch,
        observation: WindowCloseObservation,
        focus_causal: FocusCausalStamp,
        context: PlatformSnapshotReductionContext<'_>,
    ) -> Result<InputOutcome, EngineError> {
        let PlatformSnapshotReductionContext {
            application_base,
            events,
            interaction_events,
        } = context;
        if let Some(rejected) = self.platform_provider_rejection(provider) {
            return Ok(rejected);
        }
        if expected_epoch != self.version.epoch() {
            return Ok(InputOutcome::PlatformSnapshotStale {
                expected_epoch,
                current_epoch: self.version.epoch(),
            });
        }

        let previous_physical_native_drag = self.viewport.physical_native_drag_capability();
        let previous_physical_cross_surface_drag =
            self.viewport.physical_cross_surface_drag_capability();
        let interaction_dependencies = self.platform_interaction_dependencies();
        let version_before_actions = self.version;
        let transition = self
            .viewport
            .publish_close_observation(observation)
            .map_err(|source| EngineError::Viewport { input, source })?;
        let mut native_close_edges = Vec::new();
        for event in transition.registry_events() {
            match event {
                crate::viewport_registry::RegistryEvent::CloseRequested { observation } => {
                    if !transition.pre_admission_close_was_consumed(*observation) {
                        native_close_edges.push(NativeCloseEdge::from_authoritative_requested(
                            self.authority_domain,
                            observation.binding(),
                            observation.generation(),
                            observation.inventory_generation(),
                        ));
                    }
                }
                crate::viewport_registry::RegistryEvent::CloseRequestCleared {
                    requested,
                    observation,
                } => {
                    self.settle_cleared_native_surface_close(provider, *requested, *observation);
                }
                crate::viewport_registry::RegistryEvent::Ready { .. }
                | crate::viewport_registry::RegistryEvent::PresentationChanged { .. }
                | crate::viewport_registry::RegistryEvent::FactsUnavailable { .. }
                | crate::viewport_registry::RegistryEvent::BindingMissing { .. }
                | crate::viewport_registry::RegistryEvent::Destroyed { .. } => {}
            }
        }

        self.reconcile_viewport_focus_authority();
        let actions = transition.actions().to_vec();
        let recovery_batch = self.freeze_surface_recovery_batch(input, &actions)?;
        let invalidated_scene_authorities = self.invalidate_changed_surface_scene_authority();
        self.demote_invalidated_surface_scenes(input, &invalidated_scene_authorities)?;
        let mut activations = Vec::new();
        self.reduce_viewport_actions(
            input,
            provider,
            focus_causal,
            &actions,
            &recovery_batch,
            &mut activations,
            events,
            interaction_events,
        )?;
        self.issue_admitted_root_recovery_anchors(input)?;
        self.surface_recovery
            .retain_pending(|surface| self.viewport.recovery_pending(surface).is_some());
        *application_base = self.version;
        match self.platform_interaction_reconciliation(
            &interaction_dependencies,
            version_before_actions,
            &transition,
            previous_physical_native_drag,
            previous_physical_cross_surface_drag,
            &invalidated_scene_authorities.bindings,
        ) {
            PlatformInteractionReconciliation::Preserve => {}
            PlatformInteractionReconciliation::ClearDragFeedback {
                workspace_changed,
                end_routing,
            } => self.clear_platform_drag_feedback(
                input,
                workspace_changed,
                end_routing,
                interaction_events,
            )?,
            PlatformInteractionReconciliation::Cancel(
                InteractionCancelReason::SceneUnavailable,
            ) => self.cancel_active_interaction_preserving_scene(
                input,
                InteractionCancelReason::SceneUnavailable,
                interaction_events,
            )?,
            PlatformInteractionReconciliation::Cancel(reason) => {
                self.invalidate_transient(input, reason, interaction_events)?;
            }
        }
        Ok(InputOutcome::NativeCloseObservationPublished {
            transition,
            native_close_edges,
        })
    }

    pub(super) fn reduce_global_focus_fact(
        &mut self,
        input: InputSequence,
        provider: PlatformObservationLease,
        expected_epoch: crate::ids::WorkspaceEpoch,
        observation: crate::viewport_focus::FocusObservationEnvelope,
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
            self.platform_focus_restore_gate(),
            events,
        )?;
        *application_base = self.version;
        Ok(InputOutcome::GlobalFocusObservationPublished { transition })
    }

    fn settle_cleared_native_surface_close(
        &mut self,
        provider: PlatformObservationLease,
        requested: WindowCloseObservation,
        observation: WindowCloseObservation,
    ) {
        let edge = NativeCloseEdge::from_authoritative_requested(
            self.authority_domain,
            requested.binding(),
            requested.generation(),
            requested.inventory_generation(),
        );
        let binding = edge.binding();
        let Some(request) = self.close.surface_request_for_edge(edge) else {
            return;
        };
        let authority = self.close_authority();
        let _ = self.close.request_cancel(request, authority);
        let settled = match observation.acknowledged_effect() {
            CloseEffectAcknowledgement::Known(frontier) => {
                if let Some(effect) = frontier {
                    let transition = self.viewport.observe_effect_applied(
                        provider,
                        effect,
                        binding,
                        observation.inventory_generation(),
                    );
                    if !matches!(
                        transition,
                        EffectTransition::Applied | EffectTransition::Duplicate
                    ) {
                        let _ = self
                            .close
                            .mark_native_edge_indeterminate(request, authority);
                        return;
                    }
                }
                let proof = CloseCancellationProof::from_authoritative_live_close_cleared(
                    request,
                    binding,
                    self.close.native_close_effect(request),
                    self.close.native_cancellation_effect(request),
                    frontier,
                    observation.generation(),
                    observation.inventory_generation(),
                );
                self.close.settle_native(
                    request,
                    authority,
                    CloseNativeSettlement::CancellationProved(proof),
                )
            }
            CloseEffectAcknowledgement::Unknown(_) => self
                .close
                .mark_native_edge_indeterminate(request, authority),
        };
        if !matches!(settled, CloseAdvanceOutcome::Advanced { .. }) {
            let _ = self
                .close
                .mark_native_edge_indeterminate(request, authority);
        }
    }

    fn reduce_global_focus_observation(
        &mut self,
        input: InputSequence,
        provider: PlatformObservationLease,
        observation: crate::viewport_focus::FocusObservationEnvelope,
        focus_causal: FocusCausalStamp,
        restore_gate: PlatformFocusRestoreGate,
        events: &mut Vec<WorkspaceEvent>,
    ) -> Result<FocusObservationTransition, EngineError> {
        let observed = self.focus_binding_snapshot(|record| record.can_observe_focus());
        let activatable = self.focus_binding_snapshot(|record| record.can_accept_activation());
        let reportable = self.focus_fact_binding_snapshot(|record| record.can_report_focus_fact());
        let isolated = self.focus_fact_binding_snapshot(|record| {
            record.admission() == crate::viewport_registry::ViewportAdmission::Pending
                || self.native_source_surface_is_held(record.binding().surface())
        });
        if let Authority::Known(crate::viewport_focus::GlobalFocusedWindow::Dock(binding)) =
            observation.focused()
            && !reportable.contains(binding)
        {
            return Err(EngineError::GlobalFocusBindingUnavailable {
                input,
                binding: *binding,
            });
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
        let observation = crate::viewport_focus::FocusObservationEnvelope::new(
            observation.generation(),
            *observation.focused(),
            acknowledged_effect,
        );
        let items = self.focus_item_snapshot();
        let mut transition = self
            .viewport_focus
            .publish_platform_focus_observation(
                observation,
                focus_causal,
                restore_gate,
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
        effect: crate::effect::EffectId,
    ) -> bool {
        let Some(binding) = self.focus_effect_binding(effect) else {
            return false;
        };
        self.viewport
            .viewport(binding.surface())
            .filter(|record| record.can_observe_focus())
            .map(crate::viewport_registry::ViewportRecord::binding)
            == Some(binding)
            && self
                .viewport
                .focus_effect_observation_matches(provider, effect, binding)
    }

    fn settle_acknowledged_focus_effect(
        &mut self,
        provider: PlatformObservationLease,
        observation: crate::viewport_focus::FocusObservationEnvelope,
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
                .map(crate::viewport_registry::ViewportRecord::binding)
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
        effect: crate::effect::EffectId,
    ) -> Option<crate::viewport::ViewportBinding> {
        let record = self.viewport.effects().record(effect)?;
        match record.request().effect() {
            crate::effect::PlatformEffect::RequestFocus { binding, .. } => Some(*binding),
            _ => None,
        }
    }

    fn platform_focus_restore_gate(&self) -> PlatformFocusRestoreGate {
        match self.pointer_button_authority() {
            crate::pointer_journal::AnyButtonDownAuthority::KnownDown => {
                PlatformFocusRestoreGate::KnownDown
            }
            crate::pointer_journal::AnyButtonDownAuthority::KnownAllReleased
                if matches!(
                    self.pointer_provider().map(PointerInputLease::scope),
                    Some(PointerProviderScope::DesktopGlobal)
                ) =>
            {
                PlatformFocusRestoreGate::KnownAllReleased
            }
            crate::pointer_journal::AnyButtonDownAuthority::KnownAllReleased
            | crate::pointer_journal::AnyButtonDownAuthority::Unknown(_) => {
                PlatformFocusRestoreGate::Unknown
            }
        }
    }

    pub(super) fn reduce_viewport_activation(
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
        if request.cause() != crate::viewport_focus::ViewportActivationCause::Explicit {
            return Ok(InputOutcome::ViewportActivationRejected { request });
        }
        let activation = self.start_viewport_activation(request, focus_causal, events)?;
        Ok(InputOutcome::ViewportActivationRequested { activation })
    }

    pub(super) fn start_viewport_activation(
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
                let Some(observation) = self.viewport_focus.focus_observation() else {
                    return Ok(None);
                };
                let target_is_focused = matches!(
                    observation.focused(),
                    Authority::Known(
                        crate::viewport_focus::GlobalFocusedWindow::Dock(binding)
                    ) if *binding == request.target()
                );
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

    pub(super) fn settle_native_activation_reservations(
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
            == crate::viewport_focus::ViewportActivationCause::TearOffCommitted
            && matches!(
                activation.outcome(),
                ActivationStartOutcome::Suppressed(
                    crate::viewport_focus::ActivationSuppression::CausallySuperseded { .. }
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
        stale_target: crate::viewport::ViewportBinding,
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
    pub(super) fn emit_pending_platform_focus_effect(
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
            != crate::viewport_focus::FocusEffectAttachment::Applied
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
                    crate::error::CommandError::SurfaceLifecycleFrozen { surface } => {
                        PaneFocusRevealRejection::SurfaceLifecycleFrozen { surface }
                    }
                    crate::error::CommandError::Policy(_) => {
                        PaneFocusRevealRejection::PolicyRejected
                    }
                    crate::error::CommandError::MissingRoot { .. }
                    | crate::error::CommandError::MissingNode { .. }
                    | crate::error::CommandError::NodeOutsideRoot { .. }
                    | crate::error::CommandError::StaleNode { .. }
                    | crate::error::CommandError::NodeIsNotTabs { .. }
                    | crate::error::CommandError::ItemNotInTabs { .. } => {
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

    pub(super) fn capture_surface_item_source(
        workspace: &Workspace,
        surface: crate::ids::SurfaceId,
        item: crate::ids::ItemId,
    ) -> Option<crate::command::ItemSource> {
        let presentation = workspace.surface(surface)?;
        let mut roots = Vec::with_capacity(presentation.contained.len().saturating_add(1));
        roots.extend(presentation.main_root);
        roots.extend(presentation.contained.iter().filter_map(|floating| {
            workspace
                .contained_floating(*floating)
                .map(|record| record.root)
        }));
        for root in roots {
            for (tabs, node) in workspace.nodes() {
                if !matches!(node, Node::Tabs { items, .. } if items.contains(&item)) {
                    continue;
                }
                if let Ok(source) = workspace.capture_item_source(root, tabs, item) {
                    return Some(source);
                }
            }
        }
        None
    }

    pub(super) fn reduce_pane_focus_observation(
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
        include: impl Fn(&crate::viewport_registry::ViewportRecord) -> bool,
    ) -> BTreeSet<crate::viewport::ViewportBinding> {
        self.viewport
            .registry()
            .records()
            .filter_map(|(surface, record)| {
                (self.workspace.surface(surface).is_some() && include(record))
                    .then_some(record.binding())
            })
            .collect()
    }

    fn focus_fact_binding_snapshot(
        &self,
        include: impl Fn(&crate::viewport_registry::ViewportRecord) -> bool,
    ) -> BTreeSet<crate::viewport::ViewportBinding> {
        self.viewport
            .registry()
            .records()
            .filter_map(|(_, record)| include(record).then_some(record.binding()))
            .collect()
    }

    fn focus_item_snapshot(&self) -> BTreeMap<crate::ids::SurfaceId, BTreeSet<crate::ids::ItemId>> {
        self.workspace
            .surfaces()
            .map(|(surface, _)| (surface, self.surface_items(surface)))
            .collect()
    }

    pub(super) fn reconcile_viewport_focus_authority(&mut self) {
        let observed = self.focus_binding_snapshot(|record| record.can_observe_focus());
        let activatable = self.focus_binding_snapshot(|record| record.can_accept_activation());
        let items = self.focus_item_snapshot();
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

    pub(super) fn surface_items(
        &self,
        surface: crate::ids::SurfaceId,
    ) -> BTreeSet<crate::ids::ItemId> {
        let Some(presentation) = self.workspace.surface(surface) else {
            return BTreeSet::new();
        };
        presentation
            .main_root
            .into_iter()
            .chain(presentation.contained.iter().filter_map(|floating| {
                self.workspace
                    .contained_floating(*floating)
                    .map(|floating| floating.root)
            }))
            .filter_map(|root| self.workspace.root(root).map(|record| record.node))
            .flat_map(|node| self.workspace.collect_items_in_subtree(node))
            .collect()
    }

    pub(super) fn reduce_platform_effect(
        &mut self,
        input: InputSequence,
        provider: PlatformObservationLease,
        expected_epoch: crate::ids::WorkspaceEpoch,
        result: EffectResult,
        interaction_events: &mut Vec<InteractionEvent>,
    ) -> Result<InputOutcome, EngineError> {
        if let Some(rejected) = self.platform_provider_rejection(provider) {
            return Ok(rejected);
        }
        let native_close = self
            .viewport
            .effects()
            .record(result.effect())
            .and_then(|record| match record.request().effect() {
                PlatformEffect::ResolveNativeClose {
                    request,
                    edge,
                    resolution,
                } => Some((*request, *edge, *resolution)),
                _ => None,
            });
        let is_focus_effect =
            self.viewport
                .effects()
                .record(result.effect())
                .is_some_and(|record| {
                    matches!(
                        record.request().effect(),
                        PlatformEffect::RequestFocus { .. }
                    )
                });
        let active_drag_route_failed = matches!(
            result.result(),
            EffectDispatchResult::DispatchFailed(_) | EffectDispatchResult::Unsupported(_)
        ) && match self.interaction.status() {
            InteractionStatus::Dragging { session } => {
                self.interaction
                    .active_drag(session)
                    .ok()
                    .and_then(|drag| drag.owner.pointer_if_physical())
                    .and_then(|pointer| self.viewport.drag_route_effect(pointer))
                    == Some(result.effect())
            }
            InteractionStatus::Idle
            | InteractionStatus::Pressed { .. }
            | InteractionStatus::Armed { .. }
            | InteractionStatus::Resizing { .. }
            | InteractionStatus::ContainedTransforming { .. } => false,
        };
        let transition = if expected_epoch == self.version.epoch() {
            self.viewport
                .report_effect_from(provider, expected_epoch, result)
                .map_err(|source| EngineError::Viewport { input, source })?
        } else {
            EffectTransition::StaleEpoch
        };
        let focus = (transition == EffectTransition::Applied && is_focus_effect).then(|| {
            self.viewport_focus
                .report_platform_focus_effect(result.effect(), result.result())
        });
        if transition == EffectTransition::Applied && active_drag_route_failed {
            self.cancel_active_interaction_preserving_scene(
                input,
                InteractionCancelReason::PointerRoutingUnavailable,
                interaction_events,
            )?;
        }
        if transition == EffectTransition::Applied
            && let Some((request, edge, resolution)) = native_close
        {
            let plan_is_settled = match self.close.lookup(request) {
                ClosePlanLookup::Detailed(plan) => {
                    if self.close.native_edge(request) != Some(edge) {
                        return Err(EngineError::ClosePlanInvariant {
                            input,
                            detail: format!(
                                "native close effect {:?} does not match close plan {request} edge",
                                result.effect()
                            ),
                        });
                    }
                    plan.is_terminal() || plan.phase() == ClosePlanPhase::Indeterminate
                }
                ClosePlanLookup::RetiredTerminal => true,
                ClosePlanLookup::Unknown => {
                    return Err(EngineError::ClosePlanInvariant {
                        input,
                        detail: format!(
                            "native close effect {:?} names unknown close plan {request}",
                            result.effect()
                        ),
                    });
                }
            };
            if !plan_is_settled {
                let indeterminate = self
                    .close
                    .mark_indeterminate(request, self.close_authority());
                if matches!(indeterminate, CloseAdvanceOutcome::Inert(_)) {
                    return Err(EngineError::ClosePlanInvariant {
                        input,
                        detail: format!(
                            "native close effect {:?} settled without an active close obligation: {indeterminate:?}",
                            result.effect()
                        ),
                    });
                }
                if matches!(
                    result.result(),
                    EffectDispatchResult::DispatchFailed(_) | EffectDispatchResult::Unsupported(_)
                ) && resolution == NativeCloseResolution::Accept
                    && self.native_close_edge_is_current(edge)
                {
                    self.viewport
                        .resume_after_failed_native_close(edge.binding())
                        .map_err(|source| EngineError::Viewport { input, source })?;
                }
            }
        }
        Ok(InputOutcome::PlatformEffectReported {
            effect: result.effect(),
            transition,
            focus,
        })
    }

    pub(super) fn reduce_native_create_cancellation(
        &mut self,
        input: InputSequence,
        expected: WorkspaceVersion,
        saga: NativeCreateSagaId,
    ) -> Result<InputOutcome, EngineError> {
        if expected != self.version {
            return Ok(InputOutcome::StaleRejected {
                expected,
                accepted_base: self.version,
            });
        }
        let compensation = self
            .viewport
            .cancel_native_create(saga)
            .map_err(|source| EngineError::Viewport { input, source })?;
        let _ = self.viewport_focus.cancel_activation_reservation(saga);
        Ok(InputOutcome::NativeCreateCancelled { saga, compensation })
    }

    pub(super) fn reduce_viewport_cleanup_retry(
        &mut self,
        input: InputSequence,
        expected: WorkspaceVersion,
        failed_effect: crate::effect::EffectId,
    ) -> Result<InputOutcome, EngineError> {
        if expected != self.version {
            return Ok(InputOutcome::StaleRejected {
                expected,
                accepted_base: self.version,
            });
        }
        let retry = self
            .viewport
            .retry_cleanup(failed_effect)
            .map_err(|source| EngineError::Viewport { input, source })?;
        Ok(InputOutcome::ViewportCleanupRetried {
            failed_effect,
            retry,
        })
    }

    // Lifecycle actions share one batch-frozen roster/scene barrier and remain ordered in one
    // reduction loop. Native-close semantics are resolved by CloseCoordinator, not by frame.
    pub(super) fn reduce_viewport_actions(
        &mut self,
        input: InputSequence,
        provider: PlatformObservationLease,
        focus_causal: FocusCausalStamp,
        actions: &[crate::frame::ViewportLifecycleAction],
        recovery_batch: &SurfaceRecoveryBatchContext,
        activations: &mut Vec<ActivationStart>,
        events: &mut Vec<WorkspaceEvent>,
        interaction_events: &mut Vec<InteractionEvent>,
    ) -> Result<(), EngineError> {
        let mut active_barrier = recovery_batch.active_rosters.clone();
        for action in actions {
            match action {
                crate::frame::ViewportLifecycleAction::TransferNativeCreate {
                    saga,
                    prepared,
                    proof,
                } => {
                    let policy = self.policy.clone();
                    let (publication, application) = self
                        .stage_workspace_command_for_native_commit(
                            input,
                            &policy,
                            prepared.command(),
                            Some(&active_barrier),
                            *saga,
                        )?;
                    match application {
                        application @ CommandApplication::Applied { .. } => {
                            let Some(mut publication) = publication else {
                                return Err(EngineError::MissingCommandOutcome { input });
                            };
                            let obligation = prepared.recovery_obligation();
                            let surface = obligation.request().source_surface();
                            let valid = obligation.authority_domain() == self.authority_domain
                                && surface == prepared.proposal().surface()
                                && !self.bound_surface_recoveries.contains_key(&surface)
                                && Self::recovery_policy_request(
                                    &publication.workspace,
                                    surface,
                                    obligation.request().host_surface(),
                                ) == Ok(obligation.request().clone());
                            if !valid {
                                self.viewport
                                    .reject_native_create(*saga)
                                    .map_err(|source| EngineError::Viewport { input, source })?;
                                let _ = self.viewport_focus.cancel_activation_reservation(*saga);
                                continue;
                            }
                            let source_surface = prepared.source_surface();
                            let source_vacancy = publication
                                .workspace
                                .surface(source_surface)
                                .is_none()
                                .then(|| {
                                    self.viewport
                                        .capture_surface_vacancy_authority(source_surface)
                                });
                            let mut viewport = self.viewport.clone();
                            let binding = match viewport.transfer_native_create(*saga, *proof) {
                                Ok(binding) => binding,
                                Err(_) => {
                                    self.viewport.reject_native_create(*saga).map_err(
                                        |source| EngineError::Viewport { input, source },
                                    )?;
                                    let _ =
                                        self.viewport_focus.cancel_activation_reservation(*saga);
                                    continue;
                                }
                            };
                            let Some(resource) = proof.retained_resource() else {
                                return Err(EngineError::ReductionCauseInvariant {
                                    detail: "native create proof lost its retained staging resource",
                                });
                            };
                            publication.bound_surface_recoveries.insert(
                                surface,
                                BoundSurfaceRecovery::new(binding, obligation.clone())
                                    .retaining_staging_resource(resource),
                            );
                            self.native_admission
                                .register_transfer(
                                    *saga,
                                    binding,
                                    resource,
                                    self.presentation_authority.last_presentation_output_serial,
                                    source_vacancy,
                                )
                                .map_err(|source| EngineError::ReductionCauseInvariant {
                                    detail: source.detail(),
                                })?;
                            self.publish_workspace(publication);
                            self.viewport = viewport;
                            self.reconcile_viewport_focus_authority();
                            self.record_interaction_command_application(
                                input,
                                application,
                                events,
                            )?;
                        }
                        CommandApplication::Rejected(_) => {
                            self.viewport
                                .reject_native_create(*saga)
                                .map_err(|source| EngineError::Viewport { input, source })?;
                            let _ = self.viewport_focus.cancel_activation_reservation(*saga);
                        }
                    }
                }
                crate::frame::ViewportLifecycleAction::SurfaceDestroyed { observation, .. } => {
                    let binding = observation.binding();
                    self.mark_native_source_transition_target_destroyed(binding);
                    if self.reduce_planned_destroyed_surface(
                        input,
                        provider,
                        focus_causal,
                        *observation,
                        &active_barrier,
                        activations,
                        events,
                        interaction_events,
                    )? {
                        active_barrier.remove(&binding.surface());
                        continue;
                    }
                    let mut context = DestroyedSurfaceContext {
                        roster: active_barrier.get(&binding.surface()),
                        action_barrier: &active_barrier,
                        recovery_targets: &recovery_batch.targets,
                        events,
                    };
                    self.reduce_destroyed_surface(
                        input,
                        focus_causal,
                        *observation,
                        activations,
                        &mut context,
                    )?;
                    active_barrier.remove(&binding.surface());
                }
                crate::frame::ViewportLifecycleAction::RecoveryReplacementReady {
                    destroyed_binding,
                    replacement_binding,
                    recovery_obligation,
                } => {
                    let surface = destroyed_binding.surface();
                    let resource = self
                        .bound_surface_recoveries
                        .get(&surface)
                        .filter(|bound| {
                            bound.binding == *destroyed_binding
                                && bound.obligation.id() == *recovery_obligation
                        })
                        .map(|bound| bound.retained_staging_resource)
                        .ok_or(EngineError::ConflictingSurfaceRecovery { input, surface })?;
                    let replacement_is_focused = self
                        .viewport_focus
                        .focus_observation()
                        .is_some_and(|observation| {
                            matches!(
                                observation.focused(),
                                Authority::Known(
                                    crate::viewport_focus::GlobalFocusedWindow::Dock(binding)
                                ) if *binding == *replacement_binding
                            )
                        });
                    let focus_is_already_pending = self
                        .viewport_focus
                        .pending_pane_intent()
                        .is_some_and(|intent| intent.native_guard() == Some(*replacement_binding));
                    let recovery_activation = (replacement_is_focused && !focus_is_already_pending)
                        .then(|| {
                            (
                                focus_causal,
                                PaneFocusDisposition::from_record(
                                    self.viewport_focus.panel_focus(surface),
                                ),
                            )
                        });
                    self.native_admission
                        .register_recovery_replacement(
                            *destroyed_binding,
                            *replacement_binding,
                            *recovery_obligation,
                            resource,
                            self.presentation_authority.last_presentation_output_serial,
                            recovery_activation,
                        )
                        .map_err(|source| EngineError::ReductionCauseInvariant {
                            detail: source.detail(),
                        })?;
                }
                crate::frame::ViewportLifecycleAction::RecoveryReplacementLost {
                    destroyed_binding,
                    replacement_binding,
                    recovery_obligation,
                } => {
                    self.native_admission
                        .cancel_recovery_replacement(
                            *destroyed_binding,
                            *replacement_binding,
                            *recovery_obligation,
                        )
                        .map_err(|source| EngineError::ReductionCauseInvariant {
                            detail: source.detail(),
                        })?;
                }
                crate::frame::ViewportLifecycleAction::RetryRecovery {
                    destroyed_binding,
                    recovery_obligation,
                } => {
                    // The frame coordinator owns a replacement while its
                    // pre-admission cleanup is unresolved. A queued retry
                    // must not race that cleanup and issue another close for
                    // the same native binding.
                    if self
                        .viewport
                        .recovery_retry_is_suppressed(*destroyed_binding, *recovery_obligation)
                    {
                        continue;
                    }
                    let surface = destroyed_binding.surface();
                    let obligation = self
                        .bound_surface_recoveries
                        .get(&surface)
                        .filter(|bound| {
                            bound.binding == *destroyed_binding
                                && bound.obligation.id() == *recovery_obligation
                        })
                        .map(|bound| bound.obligation.clone())
                        .ok_or(EngineError::ConflictingSurfaceRecovery { input, surface })?;
                    let mut roster = self
                        .surface_recovery
                        .pending(surface)
                        .cloned()
                        .ok_or(EngineError::MissingSurfaceRoster { input, surface })?;
                    let _ = self.freeze_surface_roster_minimums(&mut roster);
                    if !self.surface_recovery.update_pending_roster(roster.clone()) {
                        return Err(EngineError::ConflictingSurfaceRecovery { input, surface });
                    }
                    let application = self.apply_destroyed_surface_recovery(
                        input,
                        &roster,
                        &obligation,
                        SurfaceRecoveryTargetContext::new(
                            &active_barrier,
                            recovery_batch
                                .targets
                                .get(&obligation.target().host_surface()),
                        ),
                        events,
                    )?;
                    match application {
                        DestroyedSurfaceRecoveryApplication::Applied => {
                            self.viewport
                                .complete_pending_recovery(surface)
                                .map_err(|source| EngineError::Viewport { input, source })?;
                            self.surface_recovery.complete_pending(surface);
                        }
                        DestroyedSurfaceRecoveryApplication::Blocked(reason) => {
                            self.surface_recovery.mark_blocked(surface, reason);
                        }
                    }
                }
            }
        }
        self.viewport
            .validate_native_staging_resource_conservation()
            .map_err(|source| EngineError::Viewport { input, source })?;
        Ok(())
    }

    pub(super) fn freeze_surface_recovery_batch(
        &self,
        input: InputSequence,
        actions: &[crate::frame::ViewportLifecycleAction],
    ) -> Result<SurfaceRecoveryBatchContext, EngineError> {
        let active_rosters = self.freeze_destroyed_surface_rosters(input, actions)?;
        let mut targets = BTreeMap::new();
        for action in actions {
            let target_surface = match action {
                crate::frame::ViewportLifecycleAction::TransferNativeCreate { .. }
                | crate::frame::ViewportLifecycleAction::RecoveryReplacementReady { .. }
                | crate::frame::ViewportLifecycleAction::RecoveryReplacementLost { .. } => None,
                crate::frame::ViewportLifecycleAction::SurfaceDestroyed { observation, .. } => self
                    .bound_surface_recoveries
                    .get(&observation.binding().surface())
                    .filter(|bound| bound.binding == observation.binding())
                    .map(|bound| bound.obligation.target().host_surface()),
                crate::frame::ViewportLifecycleAction::RetryRecovery {
                    destroyed_binding,
                    recovery_obligation,
                } => self
                    .bound_surface_recoveries
                    .get(&destroyed_binding.surface())
                    .filter(|bound| {
                        bound.binding == *destroyed_binding
                            && bound.obligation.id() == *recovery_obligation
                    })
                    .map(|bound| bound.obligation.target().host_surface()),
            };
            let Some(target_surface) = target_surface else {
                continue;
            };
            if targets.contains_key(&target_surface) {
                continue;
            }
            if let Some(facts) = self.freeze_surface_recovery_target(target_surface) {
                targets.insert(target_surface, facts);
            }
        }
        Ok(SurfaceRecoveryBatchContext {
            active_rosters,
            targets,
        })
    }

    pub(super) fn freeze_surface_recovery_target(
        &self,
        surface: crate::ids::SurfaceId,
    ) -> Option<SurfaceRecoveryHostFacts> {
        let coordinates = self
            .viewport
            .viewport(surface)
            .filter(|record| record.is_routeable())?
            .coordinates()?;
        let ready = self.presentation_authority.scene.ready_candidate(surface)?;
        if ready.stamp().requirement().workspace_epoch() != self.version.epoch()
            || !Self::coordinate_capture_matches_current(
                ready.coordinate_capture(),
                self.viewport.viewport(surface),
                self.viewport.surface_coordinate_authority(surface),
            )
        {
            return None;
        }
        SurfaceRecoveryHostFacts::new(surface, ready.stamp(), coordinates, ready.plan().bounds())
            .ok()
    }

    pub(super) fn invalidate_changed_surface_scene_authority(
        &self,
    ) -> InvalidatedSurfaceSceneAuthorities {
        let mut invalidated = InvalidatedSurfaceSceneAuthorities::default();
        for (surface, state) in self.presentation_authority.scene.surfaces() {
            let SurfaceScene::Ready(ready) = state else {
                continue;
            };
            let capture = ready.coordinate_capture();
            if Self::coordinate_capture_matches_current(
                capture,
                self.viewport.viewport(*surface),
                self.viewport.surface_coordinate_authority(*surface),
            ) {
                continue;
            }
            invalidated.surfaces.insert(*surface);
            if let Some(binding) = Self::coordinate_capture_binding(capture) {
                invalidated.bindings.insert(binding);
            }
        }
        invalidated
    }

    pub(super) fn demote_invalidated_surface_scenes(
        &mut self,
        input: InputSequence,
        invalidated: &InvalidatedSurfaceSceneAuthorities,
    ) -> Result<(), EngineError> {
        for surface in &invalidated.surfaces {
            self.invalidate_surface_coordinate_scene(*surface)
                .map_err(|source| Self::scene_revision_error(input, source))?;
        }
        Ok(())
    }

    fn invalidate_surface_coordinate_scene(
        &mut self,
        surface: crate::ids::SurfaceId,
    ) -> Result<SurfaceSceneStamp, SceneBuildError> {
        let has_paint_fallback = match self.presentation_authority.scene.surface(surface) {
            Some(SurfaceScene::Ready(ready)) => ready.paint_fallback().is_some(),
            Some(SurfaceScene::Stale(_)) => true,
            Some(SurfaceScene::Bootstrap(_)) => false,
            None => return Err(SceneBuildError::SurfaceOutsideRoster { surface }),
        };
        if has_paint_fallback {
            self.presentation_authority
                .scene
                .demote_to_stale(surface, StaleSurfaceSceneReason::CoordinateAuthorityChanged)
        } else {
            self.presentation_authority.scene.replace_with_bootstrap(
                surface,
                BootstrapSurfaceSceneReason::CoordinateAuthorityChanged,
            )
        }
    }

    fn freeze_destroyed_surface_rosters(
        &self,
        input: InputSequence,
        actions: &[crate::frame::ViewportLifecycleAction],
    ) -> Result<BTreeMap<crate::ids::SurfaceId, SurfaceRosterDisposition>, EngineError> {
        let mut rosters = BTreeMap::new();
        for action in actions {
            let crate::frame::ViewportLifecycleAction::SurfaceDestroyed {
                observation,
                source_coordinates,
                ..
            } = action
            else {
                continue;
            };
            let binding = observation.binding();
            let surface = binding.surface();
            if self.workspace.surface(surface).is_none() {
                continue;
            }
            let mut roster = self
                .capture_surface_roster_with_coordinates(surface, *source_coordinates)
                .map_err(|source| EngineError::SurfaceRoster { input, source })?;
            let _ = self.freeze_surface_roster_minimums(&mut roster);
            rosters.insert(surface, roster);
        }
        Ok(rosters)
    }

    pub(super) fn capture_surface_roster(
        &self,
        surface: crate::ids::SurfaceId,
    ) -> Result<SurfaceRosterDisposition, SurfaceRosterCaptureError> {
        let source_coordinates = self
            .viewport
            .viewport(surface)
            .and_then(crate::viewport_registry::ViewportRecord::recovery_coordinates);
        self.capture_surface_roster_with_coordinates(surface, source_coordinates)
    }

    fn capture_surface_roster_with_coordinates(
        &self,
        surface: crate::ids::SurfaceId,
        source_coordinates: Option<RecoveryCoordinateSnapshot>,
    ) -> Result<SurfaceRosterDisposition, SurfaceRosterCaptureError> {
        SurfaceRosterDisposition::capture(&self.workspace, surface, source_coordinates)
    }

    fn freeze_surface_roster_minimums(&self, roster: &mut SurfaceRosterDisposition) -> bool {
        if roster.has_contained_minimum_authority() {
            return true;
        }
        let Some(ready) = self
            .presentation_authority
            .scene
            .ready_candidate(roster.surface())
        else {
            return false;
        };
        if ready.stamp().requirement().workspace_epoch() != self.version.epoch() {
            return false;
        }
        if let Some(coordinates) = roster.source_coordinates()
            && !matches!(
                ready.coordinate_capture(),
                SurfaceCoordinateCapture::NativeReady { coordinates: captured, .. }
                    if captured == coordinates.content()
            )
        {
            return false;
        }
        roster.freeze_contained_minimums(ready.stamp(), ready.plan().contained_minimums())
    }

    #[allow(clippy::too_many_arguments)]
    fn reduce_planned_destroyed_surface(
        &mut self,
        input: InputSequence,
        provider: PlatformObservationLease,
        focus_causal: FocusCausalStamp,
        observation: WindowCloseObservation,
        action_barrier: &BTreeMap<crate::ids::SurfaceId, SurfaceRosterDisposition>,
        activations: &mut Vec<ActivationStart>,
        events: &mut Vec<WorkspaceEvent>,
        interaction_events: &mut Vec<InteractionEvent>,
    ) -> Result<bool, EngineError> {
        let binding = observation.binding();
        let request = match observation.acknowledged_effect() {
            CloseEffectAcknowledgement::Known(Some(effect))
                if self
                    .viewport
                    .effects()
                    .record(effect)
                    .is_some_and(|record| record.provider() == Some(provider)) =>
            {
                self.close.surface_request_for_effect(binding, effect)
            }
            CloseEffectAcknowledgement::Known(Some(_))
            | CloseEffectAcknowledgement::Known(None)
            | CloseEffectAcknowledgement::Unknown(_) => None,
        };
        let Some(request) = request else {
            self.mark_active_surface_close_plans_externally_destroyed_unproved(binding, None);
            return Ok(false);
        };
        let CloseEffectAcknowledgement::Known(Some(acknowledged_effect)) =
            observation.acknowledged_effect()
        else {
            self.mark_active_surface_close_plans_externally_destroyed_unproved(binding, None);
            return Ok(false);
        };
        let Some(prepared) = self.close.prepared(request).cloned() else {
            self.mark_active_surface_close_plans_externally_destroyed_unproved(binding, None);
            return Ok(false);
        };
        let Some(commit) = self.stage_prepared_surface_close(input, &prepared, action_barrier)?
        else {
            self.mark_active_surface_close_plans_externally_destroyed_unproved(binding, None);
            return Ok(false);
        };
        let publication =
            match self.stage_workspace_publication(commit.candidate.clone(), &self.policy) {
                Ok(publication) => publication,
                Err(source) if source.is_expected_rejection() => return Ok(false),
                Err(source) => {
                    return Err(EngineError::Command {
                        input,
                        source: TransactionError::Command { index: 0, source },
                    });
                }
            };

        // Validate on a coordinator copy. If either proof or transaction is
        // invalid, neither close phase nor workspace publication changes.
        let proof = CloseDestroyedProof::from_authoritative_destroyed(
            request,
            binding,
            observation.generation(),
            observation.inventory_generation(),
            acknowledged_effect,
        );
        let mut close = self.close.clone();
        let settlement = close.settle_native(
            request,
            self.close_authority(),
            CloseNativeSettlement::DestroyedProved(proof),
        );
        if !matches!(settlement, CloseAdvanceOutcome::Advanced { .. }) {
            self.mark_active_surface_close_plans_externally_destroyed_unproved(binding, None);
            return Ok(false);
        }

        for stale_request in close.active_surface_requests_for_binding(binding) {
            let _ = close.mark_externally_destroyed_unproved(stale_request, binding);
        }

        let changed = commit.candidate != self.workspace;
        self.publish_workspace(publication);
        self.close = close;
        self.reconcile_viewport_focus_authority();
        if changed {
            self.advance_revision(input)?;
            self.reconcile_interaction_after_close(input, interaction_events)?;
        }
        for kind in commit.events {
            events.push(WorkspaceEvent::new(input, self.version, kind));
        }
        self.complete_destroyed_surface(input, binding)?;
        self.surface_recovery.complete_pending(binding.surface());

        if let Some(surface) = commit.focus_target
            && let Some(target) = self
                .viewport
                .viewport(surface)
                .filter(|record| record.can_accept_activation())
                .map(crate::viewport_registry::ViewportRecord::binding)
        {
            activations.push(self.start_viewport_activation(
                ViewportActivationRequest::close_recovery(target, commit.recovery_focus, request),
                focus_causal,
                events,
            )?);
        }
        Ok(true)
    }

    fn mark_active_surface_close_plans_externally_destroyed_unproved(
        &mut self,
        binding: crate::viewport::ViewportBinding,
        except: Option<CloseRequestId>,
    ) {
        for request in self.close.active_surface_requests_for_binding(binding) {
            if Some(request) != except {
                let _ = self
                    .close
                    .mark_externally_destroyed_unproved(request, binding);
            }
        }
    }

    fn stage_prepared_surface_close(
        &self,
        input: InputSequence,
        prepared: &PreparedCloseOperation,
        action_barrier: &BTreeMap<crate::ids::SurfaceId, SurfaceRosterDisposition>,
    ) -> Result<Option<PreparedSurfaceCloseCommit>, EngineError> {
        match prepared {
            PreparedCloseOperation::Content(_) => Ok(None),
            PreparedCloseOperation::SurfaceRetain {
                roster,
                recovery_focus,
            } => {
                if !roster.matches_workspace(&self.workspace)
                    || self
                        .first_workspace_publication_mismatch(
                            &self.workspace,
                            Some(action_barrier),
                            None,
                        )
                        .is_some()
                {
                    return Ok(None);
                }
                Ok(Some(PreparedSurfaceCloseCommit {
                    candidate: self.clone_workspace_candidate(),
                    events: Vec::new(),
                    focus_target: None,
                    recovery_focus: *recovery_focus,
                }))
            }
            PreparedCloseOperation::SurfaceRehome {
                roster,
                transaction,
                recovery_focus,
            } => {
                if !roster.matches_workspace(&self.workspace) {
                    return Ok(None);
                }
                let mut candidate = self.clone_workspace_candidate();
                let report = match transaction.apply(&mut candidate) {
                    Ok(report) => report,
                    Err(source) if source.is_expected_rejection() => return Ok(None),
                    Err(source) => return Err(EngineError::Command { input, source }),
                };
                if candidate.surface(roster.surface()).is_some()
                    || self
                        .first_workspace_publication_mismatch(
                            &candidate,
                            Some(action_barrier),
                            Some(roster.surface()),
                        )
                        .is_some()
                {
                    return Ok(None);
                }
                Ok(Some(PreparedSurfaceCloseCommit {
                    candidate,
                    events: report
                        .into_outcomes()
                        .into_iter()
                        .map(WorkspaceEventKind::CommandCommitted)
                        .collect(),
                    focus_target: Some(transaction.target_surface()),
                    recovery_focus: *recovery_focus,
                }))
            }
            PreparedCloseOperation::SurfaceContent {
                roster,
                prepared,
                recovery_focus,
            } => {
                if !roster.matches_workspace(&self.workspace) {
                    return Ok(None);
                }
                let commit =
                    match prepare_surface_content_close(&self.workspace, &self.policy, prepared) {
                        Ok(commit) => commit,
                        Err(source) if source.is_expected_rejection() => return Ok(None),
                        Err(source) => return Err(EngineError::Command { input, source }),
                    };
                if commit.candidate.surface(roster.surface()).is_some()
                    || self
                        .first_workspace_publication_mismatch(
                            &commit.candidate,
                            Some(action_barrier),
                            Some(roster.surface()),
                        )
                        .is_some()
                {
                    return Ok(None);
                }
                Ok(Some(PreparedSurfaceCloseCommit {
                    candidate: commit.candidate,
                    events: vec![WorkspaceEventKind::CloseCommitted(commit.outcome)],
                    focus_target: None,
                    recovery_focus: *recovery_focus,
                }))
            }
        }
    }

    fn reduce_destroyed_surface(
        &mut self,
        input: InputSequence,
        _focus_causal: FocusCausalStamp,
        observation: crate::platform::WindowCloseObservation,
        _activations: &mut Vec<ActivationStart>,
        context: &mut DestroyedSurfaceContext<'_>,
    ) -> Result<(), EngineError> {
        let binding = observation.binding();
        let surface = binding.surface();
        if self.workspace.surface(surface).is_none() {
            self.complete_destroyed_surface(input, binding)?;
            self.surface_recovery.complete_pending(surface);
            if self
                .bound_surface_recoveries
                .get(&surface)
                .is_some_and(|bound| bound.binding == binding)
            {
                if let Some(bound) = self.bound_surface_recoveries.remove(&surface)
                    && let Some(resource) = bound.retained_staging_resource
                {
                    self.viewport
                        .release_transferred_native_staging_resource(resource, binding)
                        .map_err(|source| EngineError::Viewport { input, source })?;
                }
            }
            return Ok(());
        }

        // Until a CloseCoordinator proof selects a prepared surface plan, destruction is
        // unplanned. Root windows retain their logical roster; child windows recover the complete
        // roster through the explicit contained recovery plan supplied at registration.
        let Some(bound) = self
            .bound_surface_recoveries
            .get(&surface)
            .filter(|bound| bound.binding == binding)
            .cloned()
        else {
            self.complete_destroyed_surface(input, binding)?;
            return Ok(());
        };
        let recovery_obligation = bound.obligation.id();
        let retained_staging_resource = bound.retained_staging_resource;
        let obligation = bound.obligation;
        let roster = context
            .roster
            .ok_or(EngineError::MissingSurfaceRoster { input, surface })?;
        let current_request = Self::recovery_policy_request(
            &self.workspace,
            surface,
            obligation.request().host_surface(),
        );
        let application = if obligation.authority_domain() != self.authority_domain
            || current_request.as_ref() != Ok(obligation.request())
        {
            DestroyedSurfaceRecoveryApplication::Blocked(
                SurfaceRecoveryBlockedReason::ObligationMismatch,
            )
        } else {
            let recovery_target = obligation.target();
            self.apply_destroyed_surface_recovery(
                input,
                roster,
                &obligation,
                SurfaceRecoveryTargetContext::new(
                    context.action_barrier,
                    context
                        .recovery_targets
                        .get(&recovery_target.host_surface()),
                ),
                context.events,
            )?
        };
        if matches!(application, DestroyedSurfaceRecoveryApplication::Applied)
            && self.workspace.surface(surface).is_none()
        {
            self.complete_destroyed_surface(input, binding)?;
            self.surface_recovery.complete_pending(surface);
            if let Some(resource) = retained_staging_resource {
                self.viewport
                    .release_transferred_native_staging_resource(resource, binding)
                    .map_err(|source| EngineError::Viewport { input, source })?;
            }
        } else if let DestroyedSurfaceRecoveryApplication::Blocked(reason) = application {
            self.viewport
                .defer_destroyed_surface_recovery(
                    binding,
                    recovery_obligation,
                    retained_staging_resource,
                )
                .map_err(|source| EngineError::Viewport { input, source })?;
            if !self.surface_recovery.defer(roster.clone()) {
                return Err(EngineError::ConflictingSurfaceRecovery { input, surface });
            }
            self.surface_recovery.mark_blocked(surface, reason);
        }
        Ok(())
    }

    fn complete_destroyed_surface(
        &mut self,
        input: InputSequence,
        binding: crate::viewport::ViewportBinding,
    ) -> Result<(), EngineError> {
        self.viewport
            .complete_destroyed_surface(binding)
            .map_err(|source| EngineError::Viewport { input, source })?;
        let _ = self.viewport_focus.clear_binding(binding);
        if self.workspace.surface(binding.surface()).is_none() {
            let _ = self.viewport_focus.clear_surface(binding.surface());
        }
        Ok(())
    }

    fn apply_destroyed_surface_recovery(
        &mut self,
        input: InputSequence,
        roster: &SurfaceRosterDisposition,
        obligation: &SurfaceRecoveryObligation,
        context: SurfaceRecoveryTargetContext<'_>,
        events: &mut Vec<WorkspaceEvent>,
    ) -> Result<DestroyedSurfaceRecoveryApplication, EngineError> {
        let recovery_target = obligation.target();
        let Some(target_facts) = context.target_facts else {
            return Ok(DestroyedSurfaceRecoveryApplication::Blocked(
                SurfaceRecoveryBlockedReason::HostAuthorityUnavailable {
                    surface: recovery_target.host_surface(),
                },
            ));
        };
        let transaction = match roster.compile_recovery_transaction(
            &self.workspace,
            recovery_target,
            *target_facts,
        ) {
            Ok(transaction) => transaction,
            Err(source) => {
                return Ok(DestroyedSurfaceRecoveryApplication::Blocked(
                    SurfaceRecoveryBlockedReason::Compile(source),
                ));
            }
        };
        let applied = self.apply_surface_roster_transaction(
            input,
            roster,
            &transaction,
            None,
            context.action_barrier,
            events,
            WorkspacePublicationAuthority::RecoveryCommit {
                source_surface: roster.surface(),
                obligation: obligation.id(),
            },
        )?;
        Ok(if applied {
            DestroyedSurfaceRecoveryApplication::Applied
        } else {
            DestroyedSurfaceRecoveryApplication::Blocked(
                SurfaceRecoveryBlockedReason::ProgramRejected,
            )
        })
    }

    pub(super) fn apply_surface_roster_transaction(
        &mut self,
        input: InputSequence,
        roster: &SurfaceRosterDisposition,
        transaction: &SurfaceRecoveryTransaction,
        selection: Option<CandidatePaneSelection>,
        action_barrier: &BTreeMap<crate::ids::SurfaceId, SurfaceRosterDisposition>,
        events: &mut Vec<WorkspaceEvent>,
        publication_authority: WorkspacePublicationAuthority,
    ) -> Result<bool, EngineError> {
        let mut candidate = self.clone_workspace_candidate();
        let report = match transaction.apply(&mut candidate) {
            Ok(report) => report,
            Err(source) if source.is_expected_rejection() => return Ok(false),
            Err(source) => return Err(EngineError::Command { input, source }),
        };
        let mut changed = report.changed();
        let mut outcomes = report.into_outcomes();
        if let Some(selection) = selection {
            let Some(source) =
                Self::capture_surface_item_source(&candidate, selection.surface, selection.item)
            else {
                return Ok(false);
            };
            let selection_report =
                match WorkspaceTransaction::from_commands([WorkspaceCommand::Select { source }])
                    .apply(&mut candidate, &self.policy)
                {
                    Ok(report) => report,
                    Err(TransactionError::Command { source, .. })
                        if source.is_expected_rejection() =>
                    {
                        return Ok(false);
                    }
                    Err(source) => return Err(EngineError::Command { input, source }),
                };
            changed |= selection_report.changed();
            outcomes.extend(selection_report.into_outcomes());
        }
        if candidate.surface(roster.surface()).is_some()
            || self
                .first_workspace_publication_mismatch(
                    &candidate,
                    Some(action_barrier),
                    Some(roster.surface()),
                )
                .is_some()
        {
            return Ok(false);
        }

        let publication = match self.stage_workspace_publication_with_authority(
            candidate,
            &self.policy,
            publication_authority,
        ) {
            Ok(publication) => publication,
            Err(source) if source.is_expected_rejection() => return Ok(false),
            Err(source) => {
                return Err(EngineError::Command {
                    input,
                    source: TransactionError::Command { index: 0, source },
                });
            }
        };
        self.publish_workspace(publication);
        self.reconcile_viewport_focus_authority();
        if changed {
            self.advance_revision(input)?;
            events.extend(outcomes.into_iter().map(|outcome| {
                WorkspaceEvent::new(
                    input,
                    self.version,
                    WorkspaceEventKind::CommandCommitted(outcome),
                )
            }));
        }
        Ok(true)
    }

    pub(super) fn issue_root_recovery_anchors(
        &mut self,
        input: InputSequence,
        surfaces: impl IntoIterator<Item = crate::ids::SurfaceId>,
    ) -> Result<(), EngineError> {
        for surface in surfaces {
            if self.root_recovery_anchors.contains_key(&surface) {
                continue;
            }
            let id = self
                .last_root_recovery_anchor
                .checked_next()
                .ok_or(EngineError::RootRecoveryAnchorExhausted { input })?;
            self.last_root_recovery_anchor = id;
            self.root_recovery_anchors.insert(
                surface,
                RootRecoveryAnchor::new(self.authority_domain, id, surface),
            );
        }
        Ok(())
    }

    fn issue_admitted_root_recovery_anchors(
        &mut self,
        input: InputSequence,
    ) -> Result<(), EngineError> {
        let surfaces = self
            .viewport
            .registry()
            .records()
            .filter_map(|(surface, record)| {
                (record.role() == ViewportRole::Root
                    && record.admission() == ViewportAdmission::Admitted
                    && self.workspace.surface(surface).is_some())
                .then_some(surface)
            })
            .collect::<Vec<_>>();
        self.issue_root_recovery_anchors(input, surfaces)
    }
}
