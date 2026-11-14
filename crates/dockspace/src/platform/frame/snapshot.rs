//! Platform snapshot reduction and admission of ready native bindings.

use crate::platform::frame::*;

impl ViewportCoordinator {
    /// Atomically publishes one complete platform snapshot.
    pub(crate) fn publish_snapshot(
        &mut self,
        snapshot: &PlatformSnapshot,
    ) -> Result<ViewportFrameTransition, ViewportCoordinatorError> {
        let mut candidate = self.clone();
        let provider = candidate.active_platform_provider()?;
        if let Some(current) = candidate
            .inventory_observations
            .stale_against(snapshot.inventory_observation())
        {
            return Err(ViewportCoordinatorError::StaleInventoryObservation {
                submitted: snapshot.inventory_observation().generation(),
                current,
            });
        }
        if candidate
            .inventory_observations
            .conflicts_with(snapshot.inventory_observation())
        {
            return Err(ViewportCoordinatorError::ConflictingInventoryObservation {
                generation: snapshot.inventory_observation().generation(),
            });
        }
        if let Some(previous) = candidate
            .inventory_observations
            .generation_gap_before(snapshot.inventory_observation())
        {
            return Err(
                ViewportCoordinatorError::InventoryObservationGenerationGap {
                    previous,
                    submitted: snapshot.inventory_observation().generation(),
                },
            );
        }
        if let Some(previous) = candidate.last_platform_snapshot.as_ref() {
            if snapshot.generation() < previous.generation() {
                return Err(ViewportCoordinatorError::StalePlatformSnapshot {
                    submitted: snapshot.generation(),
                    current: previous.generation(),
                });
            }
            if snapshot.generation() == previous.generation() {
                if snapshot != previous {
                    return Err(ViewportCoordinatorError::ConflictingPlatformSnapshot {
                        generation: snapshot.generation(),
                    });
                }
                return Ok(ViewportFrameTransition {
                    capability_generation: candidate.capability_generation,
                    work_area_generation: candidate.work_area_generation,
                    capabilities_changed: false,
                    work_areas_changed: false,
                    native_window_placement_changed: false,
                    registry_events: Vec::new(),
                    consumed_pre_admission_close_requests: Vec::new(),
                    actions: Vec::new(),
                });
            }
        }
        candidate
            .capability_observations
            .observe(Some(snapshot.capability_observation().clone()));
        let capabilities = candidate
            .capability_observations
            .current()
            .and_then(|observation| observation.known_roster())
            .cloned()
            .unwrap_or_default();
        let capabilities_changed = candidate.capabilities != capabilities;
        if capabilities_changed {
            candidate.capability_generation = candidate
                .capability_generation
                .checked_next()
                .ok_or(ViewportCoordinatorError::CapabilityGenerationExhausted)?;
            candidate.capabilities = capabilities;
        }
        let capability_generation = candidate.capability_generation;
        candidate
            .inventory_observations
            .observe(Some(snapshot.inventory_observation().clone()));
        if !candidate
            .capabilities
            .authoritative_inventory()
            .is_supported()
        {
            // Retain the provider watermark, but revoke semantic authority. A
            // later capability recovery cannot resurrect inventory observed
            // while its prerequisite authority was unavailable.
            candidate.inventory_observations.observe(None);
        }
        let inventory = candidate
            .inventory_observations
            .current()
            .and_then(|observation| observation.known_roster());
        candidate
            .work_area_observations
            .observe(Some(snapshot.work_area_observation().clone()));
        if !candidate.capabilities.work_area().is_supported() {
            // Work-area geometry is meaningful only while the accepted
            // capability roster proves that this provider owns that fact.
            candidate.work_area_observations.observe(None);
        }
        let work_areas: BTreeMap<_, _> = candidate
            .work_area_observations
            .current()
            .and_then(|observation| observation.known_roster())
            .unwrap_or_default()
            .iter()
            .copied()
            .map(|work_area| (work_area.token(), work_area))
            .collect();
        let work_areas_changed = candidate.work_areas != work_areas;
        if work_areas_changed {
            candidate.work_area_generation = candidate
                .work_area_generation
                .checked_next()
                .ok_or(ViewportCoordinatorError::WorkAreaGenerationExhausted)?;
            candidate.work_areas = work_areas;
        }
        let recovery_coordinates: BTreeMap<_, _> = candidate
            .registry
            .records()
            .map(|(_, record)| (record.binding(), record.recovery_coordinates()))
            .collect();
        let retired_bindings = candidate
            .binding_cleanup
            .retired_iter()
            .map(|(binding, _)| *binding)
            .collect::<BTreeSet<_>>();
        let registry = candidate
            .registry
            .apply_snapshot_with_retired_bindings(
                snapshot,
                inventory,
                &retired_bindings,
                candidate.binding_cleanup.terminal_bindings(),
            )
            .map_err(ViewportCoordinatorError::Registry)?;
        for binding in registry.events().iter().filter_map(|event| match event {
            RegistryEvent::Destroyed { observation } => Some(observation.binding()),
            RegistryEvent::Ready { .. }
            | RegistryEvent::PresentationChanged { .. }
            | RegistryEvent::FactsUnavailable { .. }
            | RegistryEvent::BindingMissing { .. }
            | RegistryEvent::CloseRequested { .. }
            | RegistryEvent::CloseRequestCleared { .. } => None,
        }) {
            let members = candidate
                .registry
                .bindings_for_window(binding.window())
                .collect::<Vec<_>>();
            if members.is_empty() {
                candidate
                    .binding_cleanup
                    .record_terminal_binding_guard(binding, provider);
            } else {
                for member in members {
                    candidate
                        .binding_cleanup
                        .record_terminal_binding_guard(member, provider);
                }
            }
            candidate.terminate_pointer_passthrough_binding(binding);
        }
        candidate.reconcile_binding_cleanups(provider, snapshot)?;
        candidate.observe_pointer_passthrough_snapshot(provider);
        candidate.reconcile_pointer_passthrough_sagas()?;
        let retirement_bindings = candidate
            .binding_cleanup
            .keys()
            .copied()
            .collect::<Vec<_>>();
        for binding in retirement_bindings {
            let _ = candidate.drive_binding_cleanup(binding)?;
        }
        let registry_events = registry.events().to_vec();
        // A pre-admission replacement close belongs to the coordinator, not the
        // graph-level close/recovery reducer. Establish that ownership before
        // deriving lifecycle actions so this same snapshot cannot also enqueue
        // `RetryRecovery` and create a second compensating close.
        let mut consumed_pre_admission_close_requests = Vec::new();
        for requested in registry_events.iter().filter_map(|event| match event {
            RegistryEvent::CloseRequested { observation } => Some(*observation),
            RegistryEvent::Ready { .. }
            | RegistryEvent::PresentationChanged { .. }
            | RegistryEvent::FactsUnavailable { .. }
            | RegistryEvent::BindingMissing { .. }
            | RegistryEvent::CloseRequestCleared { .. }
            | RegistryEvent::Destroyed { .. } => None,
        }) {
            if candidate.consume_pre_admission_close(requested)? {
                consumed_pre_admission_close_requests.push(requested);
            }
        }
        let actions = candidate.reduce_registry_events(&registry_events, &recovery_coordinates)?;
        let transition = ViewportFrameTransition {
            capability_generation,
            work_area_generation: candidate.work_area_generation,
            capabilities_changed,
            work_areas_changed,
            native_window_placement_changed: registry.native_window_placement_changed(),
            registry_events,
            consumed_pre_admission_close_requests,
            actions,
        };
        candidate.last_platform_snapshot = Some(snapshot.clone());
        *self = candidate;
        Ok(transition)
    }

    /// Publishes one exact native-close fact without replacing the complete
    /// platform snapshot retained for every other authority lane.
    pub(crate) fn publish_close_observation(
        &mut self,
        observation: WindowCloseObservation,
    ) -> Result<ViewportFrameTransition, ViewportCoordinatorError> {
        let mut candidate = self.clone();
        candidate.active_platform_provider()?;
        let recovery_coordinates = candidate
            .registry
            .records()
            .map(|(_, record)| (record.binding(), record.recovery_coordinates()))
            .collect::<BTreeMap<_, _>>();
        let registry = candidate
            .registry
            .apply_close_observation(observation)
            .map_err(ViewportCoordinatorError::Registry)?;
        let registry_events = registry.events().to_vec();
        let mut consumed_pre_admission_close_requests = Vec::new();
        for requested in registry_events.iter().filter_map(|event| match event {
            RegistryEvent::CloseRequested { observation } => Some(*observation),
            RegistryEvent::Ready { .. }
            | RegistryEvent::PresentationChanged { .. }
            | RegistryEvent::FactsUnavailable { .. }
            | RegistryEvent::BindingMissing { .. }
            | RegistryEvent::CloseRequestCleared { .. }
            | RegistryEvent::Destroyed { .. } => None,
        }) {
            if candidate.consume_pre_admission_close(requested)? {
                consumed_pre_admission_close_requests.push(requested);
            }
        }
        let actions = candidate.reduce_registry_events(&registry_events, &recovery_coordinates)?;
        let transition = ViewportFrameTransition {
            capability_generation: candidate.capability_generation,
            work_area_generation: candidate.work_area_generation,
            capabilities_changed: false,
            work_areas_changed: false,
            native_window_placement_changed: false,
            registry_events,
            consumed_pre_admission_close_requests,
            actions,
        };
        *self = candidate;
        Ok(transition)
    }

    /// Returns whether this binding has not yet acquired graph ownership.
    ///
    /// This intentionally includes retired native-create bindings: a late caller must not turn
    /// an old close edge into a new surface-close plan while its cleanup remains outstanding.
    pub(crate) fn is_staging_binding(&self, binding: ViewportBinding) -> bool {
        self.native_create_is_staging_binding(binding)
            || self.recovery_replacements.contains_replacement(binding)
            || self.binding_cleanup.contains_key(&binding)
    }

    /// Returns whether a retained recovery is exclusively owned by a pending
    /// pre-admission cleanup rather than by normal recovery retry.
    ///
    /// The binding is intentionally matched together with the exact source
    /// binding and obligation. This makes an old abort unable to suppress a
    /// later recovery incarnation on the same logical surface.
    pub(crate) fn recovery_retry_is_suppressed(
        &self,
        source_binding: ViewportBinding,
        recovery_obligation: SurfaceRecoveryObligationId,
    ) -> bool {
        let Some(pending) = self
            .recovery_replacements
            .pending(source_binding.surface())
            .filter(|pending| {
                pending.source_binding() == source_binding
                    && pending.recovery_obligation() == recovery_obligation
            })
        else {
            return false;
        };
        let Some(replacement) = pending.replacement_binding() else {
            return false;
        };
        self.binding_cleanup
            .recovery_cleanup(replacement)
            .and_then(BindingRetirement::recovery_cleanup_owner)
            == Some((source_binding, recovery_obligation))
    }

    /// Consumes one native close request which targets a pre-admission binding.
    ///
    /// The caller must invoke this before exposing a close edge to the application. A staging
    /// window owns no workspace topology, so it is aborted or compensated internally instead of
    /// entering the ordinary `ClosePlan` protocol.
    pub(super) fn consume_pre_admission_close(
        &mut self,
        requested: WindowCloseObservation,
    ) -> Result<bool, ViewportCoordinatorError> {
        if requested.known_state() != Some(WindowCloseState::LiveRequested) {
            return Ok(false);
        }
        let binding = requested.binding();
        if self
            .binding_cleanup
            .reduce_recovery_cleanup_close_request(requested)
            .was_consumed()
        {
            return Ok(true);
        }
        if let Some(saga) = self.abortable_native_create_for_binding(binding) {
            let _ = self.abort_native_create(saga)?;
            return Ok(true);
        }

        if self.recovery_replacements.contains_replacement(binding) {
            let mut candidate = self.clone();
            candidate.begin_pre_admission_recovery_cleanup(requested)?;
            *self = candidate;
            return Ok(true);
        }

        Ok(false)
    }

    pub(super) fn begin_pre_admission_recovery_cleanup(
        &mut self,
        requested: WindowCloseObservation,
    ) -> Result<(), ViewportCoordinatorError> {
        let binding = requested.binding();
        if self.binding_cleanup.recovery_cleanup(binding).is_some() {
            return Ok(());
        }
        let surface = self
            .recovery_replacements
            .replacement_surface(binding)
            .ok_or(ViewportCoordinatorError::MissingRecoveryPending {
                surface: binding.surface(),
            })?;
        let pending = self
            .recovery_replacements
            .pending(surface)
            .cloned()
            .ok_or(ViewportCoordinatorError::MissingRecoveryPending { surface })?;
        let replacement = pending
            .replacement_effect()
            .ok_or(ViewportCoordinatorError::MissingReplacementEffect { surface })?;
        let (admission, role, ownership, input_observations, close_observations) =
            match self.registry.record(binding.surface()) {
                Some(record) if record.binding() == binding => (
                    record.admission(),
                    record.role(),
                    record.ownership(),
                    record.input_observations(),
                    record.close_observations(),
                ),
                Some(_) => {
                    return Err(ViewportCoordinatorError::Registry(
                        ViewportRegistryError::StaleBinding { binding },
                    ));
                }
                None => {
                    return Err(ViewportCoordinatorError::Registry(
                        ViewportRegistryError::MissingSurface {
                            surface: binding.surface(),
                        },
                    ));
                }
            };
        if admission != ViewportAdmission::Pending {
            return Err(
                ViewportCoordinatorError::StagingBindingUnexpectedAdmission { binding, admission },
            );
        }
        let cleanup = self
            .effects
            .request(PlatformEffect::CompensatingClose {
                binding,
                compensates: replacement,
            })
            .map_err(ViewportCoordinatorError::Effect)?;
        self.registry
            .mark_awaiting_destroyed(binding)
            .map_err(ViewportCoordinatorError::Registry)?;
        self.pointer_passthrough.terminate_binding(binding);
        self.begin_binding_cleanup(BindingCleanupRequest {
            binding,
            role,
            ownership,
            origin: BindingRetirementOrigin::RecoveryReplacementCleanup { replacement },
            status: BindingRetirementStatus::CleanupRequested { effect: cleanup },
            observed: true,
            input_observations,
            close_observations,
            may_reappear: true,
            cleanup: BindingCleanupAction::CompensateCreate {
                create: replacement,
            },
            retained_staging_resource: None,
            purpose: BindingCleanupPurpose::RecoveryReplacement {
                destroyed_binding: pending.source_binding(),
                recovery_obligation: pending.recovery_obligation(),
                requirement: RecoveryReplacementCleanupRequirement::ConditionalClose {
                    requested,
                    clear: None,
                },
            },
        })?;
        Ok(())
    }

    pub(super) fn reduce_pre_admission_close_clear(
        &mut self,
        requested: WindowCloseObservation,
        observation: WindowCloseObservation,
    ) -> Result<(), ViewportCoordinatorError> {
        let binding = observation.binding();
        if requested.binding() != binding
            || requested.known_state() != Some(WindowCloseState::LiveRequested)
            || observation.known_state() != Some(WindowCloseState::LiveClear)
        {
            return Ok(());
        }
        if !self
            .binding_cleanup
            .record_recovery_cleanup_clear(requested, observation)
        {
            return Ok(());
        }
        self.try_resume_pre_admission_cleanup(binding)
    }

    pub(super) fn reduce_pre_admission_cleanup_result(
        &mut self,
        effect: EffectId,
    ) -> Result<(), ViewportCoordinatorError> {
        let bindings = self
            .binding_cleanup
            .recovery_cleanup_bindings_for_effect(effect);
        for binding in bindings {
            self.try_resume_pre_admission_cleanup(binding)?;
        }
        Ok(())
    }

    pub(super) fn try_resume_pre_admission_cleanup(
        &mut self,
        binding: ViewportBinding,
    ) -> Result<(), ViewportCoordinatorError> {
        let Some((cleanup, clear)) = self.binding_cleanup.recovery_cleanup_resume(binding) else {
            return Ok(());
        };
        self.invalidate_unemitted_pre_admission_cleanup(cleanup);
        if !self.pre_admission_cleanup_is_proven_safe(cleanup, clear) {
            return Ok(());
        }
        if !self
            .registry
            .resume_pending_after_pre_admission_close_clear(binding)
            .map_err(ViewportCoordinatorError::Registry)?
        {
            return Ok(());
        }
        self.recovery_replacements
            .mark_admission_fence(binding, clear.inventory_generation())
            .map_err(recovery_replacement_error)?;
        let _ = self
            .binding_cleanup
            .finish_recovery_cleanup_resume(binding)
            .map_err(binding_cleanup_error)?;
        Ok(())
    }

    /// Drops a private abort cleanup before restoring its replacement to pending admission.
    ///
    /// The cleanup is queued while reducing `LiveRequested`, but platform effects are extracted
    /// only after the entire host frame settles. An exact `LiveClear` in that interval revokes
    /// the reason for the close; leaving the request pending would let it close a replacement
    /// after the coordinator has already restored it.
    pub(super) fn invalidate_unemitted_pre_admission_cleanup(&mut self, cleanup: EffectId) {
        let should_invalidate = self.effects.record(cleanup).is_some_and(|record| {
            !record.was_emitted() && matches!(record.phase(), EffectPhase::Requested)
        });
        if should_invalidate {
            let _ = self
                .effects
                .invalidate_unemitted(cleanup, EffectInvalidation::PreAdmissionCloseCleared);
        }
    }

    pub(super) fn pre_admission_cleanup_is_proven_safe(
        &self,
        cleanup: EffectId,
        clear: WindowCloseObservation,
    ) -> bool {
        if clear.acknowledges(cleanup) {
            return true;
        }
        let Some(record) = self.effects.record(cleanup) else {
            return false;
        };
        !record.was_emitted()
            || matches!(
                record.phase(),
                EffectPhase::DispatchFailed(_) | EffectPhase::Unsupported(_)
            )
    }

    pub(super) fn pre_admission_cleanup_allows_replacement_admission(
        &self,
        binding: ViewportBinding,
        presentation: WindowPresentationObservation,
    ) -> bool {
        self.recovery_replacements
            .admission_allows(binding, presentation.inventory_generation())
    }

    pub(super) fn reconcile_binding_cleanups(
        &mut self,
        provider: PlatformObservationLease,
        snapshot: &PlatformSnapshot,
    ) -> Result<(), ViewportCoordinatorError> {
        let directives = self.binding_cleanup.observe_snapshot(provider, snapshot);
        for directive in directives {
            let _ = self.apply_binding_cleanup_directive(directive)?;
        }
        Ok(())
    }

    pub(super) fn request_retirement_cleanup(
        &mut self,
        effect: PlatformEffect,
    ) -> Result<EffectId, ViewportCoordinatorError> {
        self.effects
            .request_in(self.workspace_epoch, effect)
            .map_err(ViewportCoordinatorError::Effect)
    }

    pub(super) fn reduce_registry_events(
        &mut self,
        events: &[RegistryEvent],
        recovery_coordinates: &BTreeMap<ViewportBinding, Option<RecoveryCoordinateSnapshot>>,
    ) -> Result<Vec<ViewportLifecycleAction>, ViewportCoordinatorError> {
        let mut actions = Vec::new();
        let mut presentation_reduced = BTreeSet::new();
        let mut suppress_recovery_retry = BTreeSet::new();
        for event in events {
            match *event {
                RegistryEvent::Ready { binding } => {
                    if presentation_reduced.insert(binding) {
                        actions.extend(self.reduce_ready_binding(binding)?);
                    }
                }
                RegistryEvent::PresentationChanged { binding } => {
                    if presentation_reduced.insert(binding) {
                        actions.extend(self.reduce_ready_binding(binding)?);
                    }
                }
                RegistryEvent::CloseRequested { observation } => {
                    if let Some(surface) =
                        self.recovery_surface_owned_by_pre_admission_cleanup(observation.binding())
                    {
                        suppress_recovery_retry.insert(surface);
                    }
                }
                RegistryEvent::CloseRequestCleared {
                    requested,
                    observation,
                } => {
                    self.reduce_pre_admission_close_clear(requested, observation)?;
                }
                RegistryEvent::Destroyed { observation } => {
                    let binding = observation.binding();
                    let disposition = self.reduce_destroyed_binding(
                        observation,
                        recovery_coordinates.get(&binding).copied().flatten(),
                        &mut actions,
                    )?;
                    if let DestroyedBindingDisposition::Handled {
                        suppress_recovery_retry: Some(surface),
                    } = disposition
                    {
                        suppress_recovery_retry.insert(surface);
                    }
                }
                RegistryEvent::FactsUnavailable { .. } | RegistryEvent::BindingMissing { .. } => {}
            }
        }

        // A registry event can arrive in the same reducer batch that creates an
        // effect.  In that case the observation is deliberately rejected by
        // the ledger's emission fence.  Revisit the durable facts at every
        // snapshot boundary so a causal barrier cannot strand a replacement or
        // close request after its one-shot edge event has been consumed.
        let ready_bindings: Vec<ViewportBinding> = self
            .registry
            .records()
            .map(|(_, record)| record.binding())
            .collect();
        for binding in ready_bindings {
            if presentation_reduced.insert(binding) {
                actions.extend(self.reduce_ready_binding(binding)?);
            }
        }
        let mut queued = BTreeSet::new();
        for pending in self.recovery_replacements.values() {
            if pending.permits_recovery_retry()
                && !suppress_recovery_retry.contains(&pending.source_binding().surface())
                && !self.recovery_retry_is_suppressed(
                    pending.source_binding(),
                    pending.recovery_obligation(),
                )
                && queued.insert(pending.source_binding().surface())
            {
                actions.push(ViewportLifecycleAction::RetryRecovery {
                    source_binding: pending.source_binding(),
                    recovery_obligation: pending.recovery_obligation(),
                });
            }
        }
        Ok(actions)
    }

    pub(super) fn recovery_surface_owned_by_pre_admission_cleanup(
        &self,
        binding: ViewportBinding,
    ) -> Option<SurfaceId> {
        self.binding_cleanup
            .recovery_cleanup(binding)
            .and_then(BindingRetirement::recovery_cleanup_owner)
            .map(|(source_binding, _)| source_binding.surface())
    }

    pub(super) fn reduce_ready_binding(
        &mut self,
        binding: ViewportBinding,
    ) -> Result<Vec<ViewportLifecycleAction>, ViewportCoordinatorError> {
        let provider = self.active_platform_provider()?;
        let mut actions = Vec::new();
        actions.extend(self.reduce_ready_native_surface(provider, binding)?);
        actions.extend(self.reduce_ready_native_create(provider, binding)?);
        Ok(actions)
    }

    pub(super) fn reduce_ready_native_surface(
        &mut self,
        provider: PlatformObservationLease,
        binding: ViewportBinding,
    ) -> Result<Vec<ViewportLifecycleAction>, ViewportCoordinatorError> {
        let mut actions = Vec::new();
        if let Some(surface) = self.recovery_replacements.replacement_surface(binding) {
            let proof = self
                .recovery_replacements
                .pending(surface)
                .and_then(|pending| pending.first_live_proof());
            if let Some(proof) = proof {
                if self.native_ready_proof_is_current(proof) {
                    return Ok(actions);
                }
                let pending = self
                    .recovery_replacements
                    .pending(surface)
                    .ok_or(ViewportCoordinatorError::MissingRecoveryPending { surface })?;
                actions.push(ViewportLifecycleAction::NativeSurfaceLost {
                    origin: pending.origin(),
                    binding,
                    recovery_obligation: pending.recovery_obligation(),
                });
                self.recovery_replacements
                    .resume_preparation(binding, proof)
                    .map_err(recovery_replacement_error)?;
            }
        }
        let Some((surface, phase)) = self.recovery_replacements.bringup(binding) else {
            return Ok(actions);
        };
        let pending = self
            .recovery_replacements
            .pending(surface)
            .ok_or(ViewportCoordinatorError::MissingRecoveryPending { surface })?;
        let origin = pending.origin();
        let recovery_obligation = pending.recovery_obligation();
        let owner = pending.staging_owner();
        let failed = self
            .registry
            .record(binding.surface())
            .and_then(ViewportRecord::create_preparation)
            .is_some_and(|preparation| {
                preparation.known_readiness()
                    == Some(crate::platform::NativeCreateReadiness::Failed)
                    && self
                        .effect_emission_fence(phase.create_effect(), binding)
                        .is_some_and(|emitted| preparation.inventory_generation() > emitted)
            });
        if failed {
            self.recovery_replacements
                .fail_preparation(binding, phase)
                .map_err(recovery_replacement_error)?;
            self.retire_abandoned_recovery_replacement_in_place(
                surface,
                binding,
                AbandonedReplacementRetirementMode::DriveImmediately,
            )?;
            return Ok(actions);
        }
        match self.reduce_ready_native_preparation(provider, binding, owner, phase)? {
            native_bringup::NativePreparationOutcome::Ignored => Ok(actions),
            native_bringup::NativePreparationOutcome::Ready(proof) => {
                if !self.replacement_is_admissible(binding) {
                    return Ok(actions);
                }
                self.recovery_replacements
                    .mark_awaiting_first_live(binding, phase, proof)
                    .map_err(recovery_replacement_error)?;
                actions.push(ViewportLifecycleAction::NativeSurfaceReady {
                    origin,
                    binding,
                    recovery_obligation,
                });
                Ok(actions)
            }
        }
    }

    pub(super) fn replacement_is_admissible(&self, binding: ViewportBinding) -> bool {
        self.registry
            .record(binding.surface())
            .is_some_and(|record| {
                record.binding() == binding
                    && record.admission() != ViewportAdmission::Retiring
                    && record.is_ready()
                    && record
                        .presentation_observation()
                        .is_some_and(|observation| {
                            observation.known_state() == Some(WindowPresentationState::Visible)
                                && self.pre_admission_cleanup_allows_replacement_admission(
                                    binding,
                                    observation,
                                )
                        })
            })
    }
}
