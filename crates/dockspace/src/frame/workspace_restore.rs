//! Workspace epoch reconciliation and retained binding cleanup.

use crate::frame::*;

impl ViewportCoordinator {
    /// Prepares retention of an exact admitted external root on a replacement surface.
    ///
    /// The source surface may be absent from the replacement workspace. The target must
    /// belong to that workspace and cannot already belong to another native window.
    #[cfg(test)]
    pub(crate) fn prepare_native_root_continuation(
        &self,
        predecessor: ViewportBinding,
        target: SurfaceId,
        desired_surfaces: &BTreeSet<SurfaceId>,
    ) -> Result<PreparedNativeRootContinuation, ViewportCoordinatorError> {
        let continuation = PreparedNativeRootContinuation {
            predecessor,
            target,
        };
        self.analyze_restore(desired_surfaces, std::slice::from_ref(&continuation))?;
        Ok(continuation)
    }

    /// Prepares a complete set of external Root continuations in one transaction.
    ///
    /// Participating windows may exchange logical surfaces. Duplicate sources,
    /// duplicate targets and targets occupied by a nonparticipating window fail
    /// before any registry or cleanup state changes.
    pub(crate) fn prepare_native_root_continuations(
        &self,
        mappings: &[(ViewportBinding, SurfaceId)],
        desired_surfaces: &BTreeSet<SurfaceId>,
    ) -> Result<Vec<PreparedNativeRootContinuation>, ViewportCoordinatorError> {
        let continuations = mappings
            .iter()
            .map(|&(predecessor, target)| PreparedNativeRootContinuation {
                predecessor,
                target,
            })
            .collect::<Vec<_>>();
        self.analyze_restore(desired_surfaces, &continuations)?;
        Ok(continuations)
    }

    /// Atomically invalidates old-epoch platform state and reconciles safe current bindings.
    ///
    /// An explicit continuation overrides only its predecessor's default same-surface target.
    /// Its current eligibility is revalidated before any candidate state can publish.
    pub(crate) fn reconcile_workspace_epoch(
        &mut self,
        new_epoch: WorkspaceEpoch,
        desired_surfaces: &BTreeSet<SurfaceId>,
        continuations: &[PreparedNativeRootContinuation],
    ) -> Result<ViewportReconciliation, ViewportCoordinatorError> {
        let mut candidate = self.clone();
        let analysis = candidate.analyze_restore(desired_surfaces, continuations)?;
        let registry = candidate
            .registry
            .reconcile_restore(new_epoch, &analysis.retained_bindings)
            .map_err(ViewportCoordinatorError::Registry)?;
        let rebound_map: BTreeMap<ViewportBinding, ViewportBinding> =
            registry.retained().iter().copied().collect();
        // A retained window's old incarnation still owns its producer lane until
        // exact ingress quiescence, even though no physical destruction occurs.
        if let Some(provider) = candidate.platform_provider() {
            for &(predecessor, _) in registry.retained() {
                candidate
                    .binding_cleanup
                    .record_terminal_binding_guard(predecessor, provider);
            }
        }

        candidate
            .effects
            .invalidate_unemitted_for_workspace_replacement(new_epoch);
        candidate.workspace_epoch = new_epoch;
        candidate.pointer_passthrough.clear();
        let mut accumulated = RestoreAccumulation::default();
        candidate
            .migrate_retirement_cleanup_obligations(new_epoch, &mut accumulated.cleanup_effects)?;
        candidate.request_retirement_restore_cleanup(
            new_epoch,
            &analysis,
            &mut accumulated.cleanup_effects,
        )?;
        candidate.request_rebound_restore_cleanup(
            &analysis,
            &rebound_map,
            &mut accumulated.cleanup_effects,
        )?;
        candidate.retire_restored_bindings(registry.retired(), &analysis, &mut accumulated)?;
        candidate.finish_restore(desired_surfaces, &mut accumulated.unbound_surfaces);

        let reconciliation = ViewportReconciliation {
            rebound: registry.retained().to_vec(),
            retired: accumulated.retired,
            cleanup_effects: accumulated.cleanup_effects,
            unbound_surfaces: accumulated.unbound_surfaces,
        };
        *self = candidate;
        Ok(reconciliation)
    }

    pub(super) fn analyze_restore(
        &self,
        desired_surfaces: &BTreeSet<SurfaceId>,
        continuations: &[PreparedNativeRootContinuation],
    ) -> Result<RestoreAnalysis, ViewportCoordinatorError> {
        let create_by_binding = self.native_create_snapshot_by_binding();
        let mut staging_by_binding: BTreeMap<_, _> = create_by_binding
            .iter()
            .map(|(binding, saga)| {
                (
                    *binding,
                    RestoreStagingBinding {
                        creation_effect: Some(saga.create),
                        cleanup: Some(BindingCleanupAction::CompensateCreate {
                            create: saga.create,
                        }),
                        origin: BindingRetirementOrigin::NativeCreateAborted {
                            create: saga.create,
                        },
                        retained_resource: Some(saga.resource()),
                        resource_owner: Some(
                            if matches!(
                                saga.phase(),
                                NativeCreatePhase::AwaitingFirstLivePresentation { .. }
                            ) {
                                NativeStagingResourceOwner::NativeCreateFirstLive(*binding)
                            } else {
                                NativeStagingResourceOwner::NativeCreateSaga(saga.id())
                            },
                        ),
                    },
                )
            })
            .collect();
        let mut cleanup_by_binding = BTreeMap::new();
        for (effect, record) in self.effects.records() {
            if let PlatformEffect::ResolveNativeClose {
                edge,
                resolution: crate::effect::NativeCloseResolution::Accept,
                ..
            } = record.request().effect()
                && record.was_emitted()
                && !matches!(
                    record.phase(),
                    EffectPhase::DispatchFailed(_)
                        | EffectPhase::Unsupported(_)
                        | EffectPhase::Invalidated { .. }
                )
            {
                cleanup_by_binding.insert(edge.binding(), effect);
            }
        }
        let pending_recoveries = self
            .recovery_replacements
            .values()
            .cloned()
            .collect::<Vec<_>>();
        for pending in &pending_recoveries {
            if let Some(binding) = pending.replacement_binding() {
                let cleanup = pending
                    .replacement_effect()
                    .map(|create| BindingCleanupAction::CompensateCreate { create });
                staging_by_binding.insert(
                    binding,
                    RestoreStagingBinding {
                        creation_effect: pending.replacement_effect(),
                        cleanup,
                        origin: BindingRetirementOrigin::WorkspaceReplaced,
                        retained_resource: None,
                        resource_owner: None,
                    },
                );
                if let Some(cleanup) = self.binding_cleanup.recovery_cleanup(binding) {
                    if cleanup.recovery_cleanup_owner()
                        != Some((pending.source_binding(), pending.recovery_obligation()))
                    {
                        return Err(
                            ViewportCoordinatorError::PendingRecoveryRegistrationMismatch {
                                surface: pending.source_binding().surface(),
                            },
                        );
                    }
                    if let Some(effect) = cleanup.status().cleanup_effect() {
                        let replacement = pending.replacement_effect().ok_or(
                            ViewportCoordinatorError::MissingReplacementEffect {
                                surface: pending.source_binding().surface(),
                            },
                        )?;
                        let _ = self.validated_compensating_close(effect, binding, replacement)?;
                        cleanup_by_binding.insert(binding, effect);
                    }
                }
            }
        }
        let replacement_bindings: BTreeSet<_> = self
            .recovery_replacements
            .values()
            .filter_map(RecoveryPending::replacement_binding)
            .collect();
        let mut targets = BTreeSet::new();
        let mut continuing = BTreeMap::new();
        for &PreparedNativeRootContinuation {
            predecessor,
            target,
        } in continuations
        {
            if continuing.insert(predecessor, target).is_some() {
                return Err(
                    ViewportCoordinatorError::NativeRootContinuationDuplicatePredecessor {
                        predecessor,
                    },
                );
            }
            if !targets.insert(target) {
                return Err(
                    ViewportCoordinatorError::NativeRootContinuationDuplicateTarget { target },
                );
            }
        }
        for (&predecessor, &target) in &continuing {
            if !desired_surfaces.contains(&target) {
                return Err(
                    ViewportCoordinatorError::NativeRootContinuationTargetMissing { target },
                );
            }
            let record = self
                .registry
                .record(predecessor.surface())
                .filter(|record| record.binding() == predecessor)
                .ok_or(
                    ViewportCoordinatorError::NativeRootContinuationPredecessorUnavailable {
                        predecessor,
                    },
                )?;
            if record.role() != ViewportRole::Root
                || record.ownership() != ViewportOwnership::External
                || self.binding_cleanup.contains_key(&predecessor)
                || self.binding_cleanup.is_terminal(predecessor)
                || cleanup_by_binding.contains_key(&predecessor)
                || !Self::restore_binding_is_safe(
                    target,
                    record,
                    desired_surfaces,
                    &create_by_binding,
                    &replacement_bindings,
                )
            {
                return Err(
                    ViewportCoordinatorError::NativeRootContinuationPredecessorUnavailable {
                        predecessor,
                    },
                );
            }
            if let Some(occupied) = self.registry.record(target)
                && occupied.binding() != predecessor
                && !continuing.contains_key(&occupied.binding())
            {
                return Err(
                    ViewportCoordinatorError::NativeRootContinuationTargetOccupied {
                        target,
                        binding: occupied.binding(),
                    },
                );
            }
        }
        let retained_bindings = self
            .registry
            .records()
            .filter_map(|(surface, record)| {
                let target = continuing
                    .get(&record.binding())
                    .copied()
                    .unwrap_or(surface);
                Self::restore_binding_is_safe(
                    target,
                    record,
                    desired_surfaces,
                    &create_by_binding,
                    &replacement_bindings,
                )
                .then_some((record.binding(), target))
            })
            .collect();
        Ok(RestoreAnalysis {
            staging_by_binding,
            cleanup_by_binding,
            retained_bindings,
            passthrough_recoveries: self.pointer_passthrough.recovery_snapshots(),
        })
    }

    pub(super) fn restore_binding_is_safe(
        target: SurfaceId,
        record: ViewportRecord<'_>,
        desired_surfaces: &BTreeSet<SurfaceId>,
        creates: &BTreeMap<ViewportBinding, NativeCreateSaga>,
        replacements: &BTreeSet<ViewportBinding>,
    ) -> bool {
        let binding = record.binding();
        // A surface identity alone cannot prove that a replacement workspace preserved the
        // child's exact root, recovery host, floating identities, ownership, and geometry
        // contract. Until restore publishes that semantic manifest, child bindings fail closed
        // and require a new explicit registration carrying a freshly validated recovery plan.
        desired_surfaces.contains(&target)
            && record.role() != ViewportRole::Child
            && record.admission() == ViewportAdmission::Admitted
            && !creates.contains_key(&binding)
            && !replacements.contains(&binding)
            && !matches!(
                record.lifecycle(),
                ViewportLifecycle::CloseRequested
                    | ViewportLifecycle::AwaitingDestroyed
                    | ViewportLifecycle::Missing
                    | ViewportLifecycle::Destroyed
            )
    }

    pub(super) fn migrate_retirement_cleanup_obligations(
        &mut self,
        new_epoch: WorkspaceEpoch,
        cleanup_effects: &mut Vec<EffectId>,
    ) -> Result<(), ViewportCoordinatorError> {
        let actions: Vec<_> = self
            .binding_cleanup
            .retired_iter()
            .map(|(_, retirement)| self.retirement_cleanup_restore_action(retirement))
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .flatten()
            .collect();

        self.apply_retirement_cleanup_actions(new_epoch, actions, cleanup_effects)
    }

    pub(super) fn migrate_retirement_cleanup_obligations_after_provider_replacement(
        &mut self,
    ) -> Result<(), ViewportCoordinatorError> {
        let actions = self
            .binding_cleanup
            .values()
            .map(|retirement| self.retirement_cleanup_provider_replacement_action(retirement))
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .flatten()
            .collect();
        self.apply_retirement_cleanup_actions(self.workspace_epoch, actions, &mut Vec::new())
    }

    pub(super) fn reconcile_after_provider_enrollment(
        &mut self,
    ) -> Result<(), ViewportCoordinatorError> {
        if !self.provider_reconciliation_pending {
            return Ok(());
        }
        self.migrate_retirement_cleanup_obligations_after_provider_replacement()?;
        let retirements = self.binding_cleanup.keys().copied().collect::<Vec<_>>();
        for binding in retirements {
            let _ = self.drive_binding_cleanup(binding)?;
        }
        self.provider_reconciliation_pending = false;
        Ok(())
    }

    pub(super) fn apply_retirement_cleanup_actions(
        &mut self,
        issuance_epoch: WorkspaceEpoch,
        actions: Vec<BindingCleanupRestoreAction>,
        cleanup_effects: &mut Vec<EffectId>,
    ) -> Result<(), ViewportCoordinatorError> {
        for action in actions {
            match action {
                BindingCleanupRestoreAction::Redispatch { binding } => {
                    let request = self
                        .binding_cleanup
                        .plan_cleanup_redispatch(binding)
                        .map_err(binding_cleanup_error)?;
                    if let Some(request) = request {
                        let effect = self.request_retirement_cleanup(request)?;
                        self.binding_cleanup
                            .accept_cleanup_effect(binding, effect)
                            .map_err(binding_cleanup_error)?;
                        cleanup_effects.push(effect);
                    }
                }
                BindingCleanupRestoreAction::ContinueObservation {
                    binding,
                    predecessor,
                    after,
                } => {
                    let superseded = self
                        .binding_cleanup
                        .get(&binding)
                        .and_then(|retirement| retirement.status().cleanup_effect())
                        .filter(|effect| *effect != predecessor);
                    let successor = self
                        .effects
                        .request_in(
                            issuance_epoch,
                            PlatformEffect::ContinueCleanup {
                                binding,
                                predecessor,
                                after,
                            },
                        )
                        .map_err(ViewportCoordinatorError::Effect)?;
                    self.binding_cleanup
                        .accept_cleanup_observation_effect(binding, successor, predecessor)
                        .map_err(binding_cleanup_error)?;
                    if let Some(effect) = superseded
                        && !matches!(
                            self.effects.supersede_cleanup_observation(
                                effect,
                                successor,
                                predecessor,
                                binding,
                            ),
                            EffectTransition::Applied | EffectTransition::Duplicate
                        )
                    {
                        return Err(ViewportCoordinatorError::InvalidCleanupContinuation {
                            effect: successor,
                            predecessor,
                        });
                    }
                    cleanup_effects.push(successor);
                }
            }
        }
        Ok(())
    }

    pub(super) fn retirement_cleanup_provider_replacement_action(
        &self,
        retirement: &BindingRetirement,
    ) -> Result<Option<BindingCleanupRestoreAction>, ViewportCoordinatorError> {
        let Some(effect) = retirement.status().cleanup_effect() else {
            return Ok(None);
        };
        let record = self
            .effects
            .record(effect)
            .ok_or(ViewportCoordinatorError::MissingCleanupEffect { effect })?;
        let observation_failed = matches!(
            record.phase(),
            EffectPhase::ObservationDispatchFailed(_) | EffectPhase::ObservationUnsupported(_)
        );
        let pending_delivery = matches!(
            retirement.status(),
            BindingRetirementStatus::CleanupRequested { .. }
                | BindingRetirementStatus::CleanupIndeterminate { .. }
        );
        if !pending_delivery
            && !(observation_failed
                && matches!(
                    record.request().effect(),
                    PlatformEffect::ContinueCleanup { .. }
                ))
        {
            return Ok(None);
        }
        match record.request().effect() {
            PlatformEffect::ContinueCleanup {
                binding,
                predecessor,
                ..
            } => {
                let predecessor_record = self.effects.record(*predecessor).ok_or(
                    ViewportCoordinatorError::InvalidCleanupContinuation {
                        effect,
                        predecessor: *predecessor,
                    },
                )?;
                if *binding != retirement.binding()
                    || predecessor_record.request().effect().binding() != retirement.binding()
                    || !predecessor_record.was_emitted()
                    || !is_destructive_cleanup(predecessor_record.request().effect())
                {
                    return Err(ViewportCoordinatorError::InvalidCleanupContinuation {
                        effect,
                        predecessor: *predecessor,
                    });
                }
                Ok((record.was_emitted()
                    || matches!(record.phase(), EffectPhase::Invalidated { .. }))
                .then_some(BindingCleanupRestoreAction::ContinueObservation {
                    binding: retirement.binding(),
                    predecessor: *predecessor,
                    after: None,
                }))
            }
            platform_effect if is_destructive_cleanup(platform_effect) => {
                if platform_effect.binding() != retirement.binding() {
                    return Err(ViewportCoordinatorError::InvalidCleanupContinuation {
                        effect,
                        predecessor: effect,
                    });
                }
                Ok(record.was_emitted().then_some(
                    BindingCleanupRestoreAction::ContinueObservation {
                        binding: retirement.binding(),
                        predecessor: effect,
                        after: None,
                    },
                ))
            }
            _ => Err(ViewportCoordinatorError::InvalidCleanupContinuation {
                effect,
                predecessor: effect,
            }),
        }
    }

    pub(super) fn retirement_cleanup_restore_action(
        &self,
        retirement: &BindingRetirement,
    ) -> Result<Option<BindingCleanupRestoreAction>, ViewportCoordinatorError> {
        let Some(effect) = retirement.status().cleanup_effect() else {
            return Ok(None);
        };
        let record = self
            .effects
            .record(effect)
            .ok_or(ViewportCoordinatorError::MissingCleanupEffect { effect })?;
        match record.request().effect() {
            PlatformEffect::ContinueCleanup {
                binding,
                predecessor,
                ..
            } => {
                let predecessor_record = self.effects.record(*predecessor).ok_or(
                    ViewportCoordinatorError::InvalidCleanupContinuation {
                        effect,
                        predecessor: *predecessor,
                    },
                )?;
                if *binding != retirement.binding()
                    || predecessor_record.request().effect().binding() != retirement.binding()
                    || !predecessor_record.was_emitted()
                    || !is_destructive_cleanup(predecessor_record.request().effect())
                {
                    return Err(ViewportCoordinatorError::InvalidCleanupContinuation {
                        effect,
                        predecessor: *predecessor,
                    });
                }
                if !matches!(
                    predecessor_record.phase(),
                    EffectPhase::Requested | EffectPhase::Indeterminate(_)
                ) {
                    return Ok(None);
                }
                Ok(Some(BindingCleanupRestoreAction::ContinueObservation {
                    binding: retirement.binding(),
                    predecessor: *predecessor,
                    after: self.cleanup_observation_lane_predecessor(effect)?,
                }))
            }
            platform_effect if is_destructive_cleanup(platform_effect) => {
                if platform_effect.binding() != retirement.binding() {
                    return Err(ViewportCoordinatorError::InvalidCleanupContinuation {
                        effect,
                        predecessor: effect,
                    });
                }
                if record.was_emitted()
                    && matches!(
                        record.phase(),
                        EffectPhase::Requested | EffectPhase::Indeterminate(_)
                    )
                {
                    return Ok(Some(BindingCleanupRestoreAction::ContinueObservation {
                        binding: retirement.binding(),
                        predecessor: effect,
                        after: None,
                    }));
                }
                Ok((!record.was_emitted()
                    && matches!(record.phase(), EffectPhase::Invalidated { .. }))
                .then_some(BindingCleanupRestoreAction::Redispatch {
                    binding: retirement.binding(),
                }))
            }
            _ => Err(ViewportCoordinatorError::InvalidCleanupContinuation {
                effect,
                predecessor: effect,
            }),
        }
    }

    pub(super) fn cleanup_observation_lane_predecessor(
        &self,
        tail: EffectId,
    ) -> Result<Option<EffectId>, ViewportCoordinatorError> {
        let mut current = Some(tail);
        while let Some(effect) = current {
            let record = self
                .effects
                .record(effect)
                .ok_or(ViewportCoordinatorError::MissingCleanupEffect { effect })?;
            let invalidated_unemitted =
                !record.was_emitted() && matches!(record.phase(), EffectPhase::Invalidated { .. });
            if !invalidated_unemitted {
                return Ok(Some(effect));
            }
            current = match record.request().effect() {
                PlatformEffect::ContinueCleanup { after, .. }
                    if after.is_none_or(|predecessor| predecessor < effect) =>
                {
                    *after
                }
                _ => {
                    return Err(ViewportCoordinatorError::InvalidCleanupContinuation {
                        effect,
                        predecessor: effect,
                    });
                }
            };
        }
        Ok(None)
    }

    pub(super) fn request_retirement_restore_cleanup(
        &mut self,
        new_epoch: WorkspaceEpoch,
        analysis: &RestoreAnalysis,
        cleanup_effects: &mut Vec<EffectId>,
    ) -> Result<(), ViewportCoordinatorError> {
        for (binding, old_saga) in &analysis.passthrough_recoveries {
            if !self.binding_cleanup.contains_key(binding) {
                continue;
            }
            self.pointer_passthrough
                .install_epoch_recovery(*binding, old_saga);
            let effect = self.request_pointer_input_restore_in(new_epoch, *binding)?;
            cleanup_effects.push(effect);
        }
        Ok(())
    }

    pub(super) fn request_rebound_restore_cleanup(
        &mut self,
        analysis: &RestoreAnalysis,
        rebound: &BTreeMap<ViewportBinding, ViewportBinding>,
        cleanup_effects: &mut Vec<EffectId>,
    ) -> Result<(), ViewportCoordinatorError> {
        for (old_binding, old_saga) in &analysis.passthrough_recoveries {
            let Some(binding) = rebound.get(old_binding).copied() else {
                continue;
            };
            self.pointer_passthrough
                .install_epoch_recovery(binding, old_saga);
            if let Some(effect) = self.reconcile_pointer_passthrough_saga(binding)? {
                cleanup_effects.push(effect);
            }
        }
        Ok(())
    }

    pub(super) fn retire_restored_bindings(
        &mut self,
        retired: &[DetachedViewportFacts],
        analysis: &RestoreAnalysis,
        accumulated: &mut RestoreAccumulation,
    ) -> Result<(), ViewportCoordinatorError> {
        for facts in retired {
            let binding = facts.binding();
            accumulated.retired.push(binding);
            if !facts.window_retired() {
                let provider = self.active_platform_provider()?;
                self.binding_cleanup
                    .record_terminal_binding_guard(binding, provider);
                continue;
            }
            let recovery_cleanup_exists = self.binding_cleanup.recovery_cleanup(binding).is_some();
            let staging = analysis.staging_by_binding.get(&binding).copied();
            let cleanup =
                staging
                    .and_then(|staging| staging.cleanup)
                    .unwrap_or_else(|| match facts.ownership() {
                        ViewportOwnership::External => BindingCleanupAction::KeepExternal,
                        ViewportOwnership::RuntimeOwned => BindingCleanupAction::ReleaseChild,
                    });
            let origin = staging.map_or(BindingRetirementOrigin::WorkspaceReplaced, |staging| {
                staging.origin
            });
            let existing_effect = match analysis.cleanup_by_binding.get(&binding).copied() {
                Some(effect) => {
                    let record = self
                        .effects
                        .record(effect)
                        .ok_or(ViewportCoordinatorError::MissingCleanupEffect { effect })?;
                    (record.was_emitted()
                        && !matches!(record.phase(), EffectPhase::Invalidated { .. }))
                    .then_some(effect)
                }
                None => None,
            };
            let creation_may_reappear = staging
                .and_then(|staging| staging.creation_effect)
                .is_some_and(|effect| {
                    self.effects.record(effect).is_some_and(|record| {
                        record.was_emitted()
                            && matches!(
                                record.phase(),
                                EffectPhase::Requested
                                    | EffectPhase::Indeterminate(_)
                                    | EffectPhase::ObservedApplied { .. }
                            )
                    })
                });
            let observed =
                facts.ever_observed() && !matches!(facts.lifecycle(), ViewportLifecycle::Missing);
            let may_reappear = creation_may_reappear
                || (staging
                    .and_then(|staging| staging.creation_effect)
                    .is_none()
                    && facts.ever_observed()
                    && !matches!(facts.lifecycle(), ViewportLifecycle::Destroyed));
            // An external binding has no cleanup effect that can prove destruction.
            // Its token remains quarantined until this exact incarnation is observed
            // as destroyed, including when no inventory snapshot ever saw it live.
            let external_requires_exact_destruction = facts.ownership()
                == ViewportOwnership::External
                && !matches!(facts.lifecycle(), ViewportLifecycle::Destroyed);
            if !external_requires_exact_destruction && !observed && !may_reappear {
                if let Some((resource, owner)) = staging
                    .and_then(|staging| staging.retained_resource.zip(staging.resource_owner))
                {
                    self.release_native_staging_resource(resource, owner)?;
                }
                if let Some(create) = staging.and_then(|staging| staging.creation_effect) {
                    self.retain_uncreated_binding_guard(binding, create);
                }
                continue;
            }
            let status = existing_effect
                .map_or(BindingRetirementStatus::AwaitingAppearance, |effect| {
                    self.retirement_status_for_effect(effect)
                });
            let retained_staging_resource = staging.and_then(|staging| staging.retained_resource);
            if let Some((resource, owner)) =
                staging.and_then(|staging| staging.retained_resource.zip(staging.resource_owner))
            {
                self.transition_native_staging_resource(
                    resource,
                    owner,
                    NativeStagingResourceOwner::BindingCleanup(binding),
                )?;
            }
            let request = BindingCleanupRequest {
                binding,
                role: facts.role(),
                ownership: facts.ownership(),
                origin,
                status,
                observed,
                input_observations: facts.input_observations(),
                close_observations: facts.close_observations(),
                may_reappear,
                cleanup,
                retained_staging_resource,
                purpose: BindingCleanupPurpose::Retired,
            };
            if recovery_cleanup_exists {
                self.binding_cleanup
                    .promote_recovery_cleanup_to_retirement(request)
                    .map_err(binding_cleanup_error)?;
            } else {
                self.begin_binding_cleanup(request)?;
            }
            if let Some(old_saga) = analysis.passthrough_recoveries.get(&binding) {
                self.pointer_passthrough
                    .install_retired_recovery(binding, old_saga);
                if let Some(effect) = self.reconcile_pointer_passthrough_saga(binding)? {
                    accumulated.cleanup_effects.push(effect);
                }
            }
            if existing_effect.is_some() {
                let retirement = self
                    .binding_cleanup
                    .get(&binding)
                    .ok_or(ViewportCoordinatorError::MissingBindingCleanup { binding })?;
                let active_record = existing_effect.and_then(|effect| self.effects.record(effect));
                let can_migrate = active_record.is_some_and(|record| {
                    is_destructive_cleanup(record.request().effect())
                        || matches!(
                            record.request().effect(),
                            PlatformEffect::ContinueCleanup { .. }
                        )
                });
                if can_migrate
                    && let Some(action) = self.retirement_cleanup_restore_action(retirement)?
                {
                    self.apply_retirement_cleanup_actions(
                        self.workspace_epoch,
                        vec![action],
                        &mut accumulated.cleanup_effects,
                    )?;
                }
            } else if let Some(effect) = self.drive_binding_cleanup(binding)? {
                accumulated.cleanup_effects.push(effect);
            }
        }
        Ok(())
    }

    pub(super) fn retirement_status_for_effect(&self, effect: EffectId) -> BindingRetirementStatus {
        match self
            .effects
            .record(effect)
            .map(crate::effect::EffectRecord::phase)
        {
            Some(EffectPhase::Indeterminate(_)) => {
                BindingRetirementStatus::CleanupIndeterminate { effect }
            }
            Some(EffectPhase::ObservationDispatchFailed(_)) => {
                BindingRetirementStatus::CleanupObservationFailed { effect }
            }
            Some(EffectPhase::DispatchFailed(_)) => {
                BindingRetirementStatus::CleanupFailed { effect }
            }
            Some(
                EffectPhase::Unsupported(reason) | EffectPhase::ObservationUnsupported(reason),
            ) => BindingRetirementStatus::CleanupBlocked { effect, reason },
            _ => BindingRetirementStatus::CleanupRequested { effect },
        }
    }

    pub(super) fn begin_binding_cleanup(
        &mut self,
        request: BindingCleanupRequest,
    ) -> Result<(), ViewportCoordinatorError> {
        self.binding_cleanup
            .begin(request)
            .map_err(binding_cleanup_error)
    }

    pub(super) fn finish_binding_cleanup(
        &mut self,
        terminal: BindingCleanupTerminal,
    ) -> Result<(), ViewportCoordinatorError> {
        if let Some(resource) = terminal.retained_staging_resource() {
            self.release_native_staging_resource(
                resource,
                NativeStagingResourceOwner::BindingCleanup(terminal.binding()),
            )?;
        }
        Ok(())
    }

    pub(super) fn drive_binding_cleanup(
        &mut self,
        binding: ViewportBinding,
    ) -> Result<Option<EffectId>, ViewportCoordinatorError> {
        if !self.binding_cleanup.contains_key(&binding) {
            return Ok(None);
        }
        let pointer_restore_pending = self.pointer_passthrough.contains_binding(binding);
        if !pointer_restore_pending
            && self
                .binding_cleanup
                .get(&binding)
                .is_some_and(BindingRetirement::is_retired)
            && let Some(record) = self.registry.record(binding.surface())
            && record.binding() == binding
        {
            if record.admission() != ViewportAdmission::Retiring {
                return Err(
                    ViewportCoordinatorError::RetiringBindingUnexpectedAdmission {
                        binding,
                        admission: record.admission(),
                    },
                );
            }
            let _ = self
                .registry
                .detach(binding)
                .map_err(ViewportCoordinatorError::Registry)?;
        }
        let directive = self
            .binding_cleanup
            .plan_drive(binding, pointer_restore_pending)
            .map_err(binding_cleanup_error)?;
        directive.map_or(Ok(None), |directive| {
            self.apply_binding_cleanup_directive(directive)
        })
    }

    pub(super) fn apply_binding_cleanup_directive(
        &mut self,
        directive: BindingCleanupDirective,
    ) -> Result<Option<EffectId>, ViewportCoordinatorError> {
        match directive {
            BindingCleanupDirective::Drive { binding } => self.drive_binding_cleanup(binding),
            BindingCleanupDirective::RequestCleanup(request) => {
                let binding = request.binding();
                let effect = self.request_retirement_cleanup(request)?;
                self.binding_cleanup
                    .accept_cleanup_effect(binding, effect)
                    .map_err(binding_cleanup_error)?;
                Ok(Some(effect))
            }
            BindingCleanupDirective::Finish(terminal) => {
                if let Some(create) = terminal.uncreated_effect() {
                    self.retain_uncreated_binding_guard(terminal.binding(), create);
                }
                self.finish_binding_cleanup(terminal)?;
                Ok(None)
            }
            BindingCleanupDirective::ExactDestroyed(terminal) => {
                self.mark_binding_effects_destroyed(terminal.binding());
                self.pointer_passthrough
                    .terminate_binding(terminal.binding());
                self.finish_binding_cleanup(terminal)?;
                Ok(None)
            }
        }
    }

    pub(super) fn finish_restore(
        &mut self,
        desired_surfaces: &BTreeSet<SurfaceId>,
        unbound_surfaces: &mut Vec<SurfaceId>,
    ) {
        self.clear_active_native_creates();
        let _ = self.recovery_replacements.clear();
        self.capabilities = PlatformCapabilities::default();
        self.work_area_observations = WorkAreaRosterObservationStream::default();
        self.work_areas.clear();
        let bound_surfaces: BTreeSet<_> = self
            .registry
            .records()
            .map(|(surface, _)| surface)
            .collect();
        let additionally_unbound: Vec<_> = desired_surfaces
            .iter()
            .filter(|surface| {
                !bound_surfaces.contains(surface) && !unbound_surfaces.contains(surface)
            })
            .copied()
            .collect();
        unbound_surfaces.extend(additionally_unbound);
        unbound_surfaces.sort_unstable();
    }
}
