//! Content and native-surface close planning, settlement, and commit.

use super::*;

impl DockEngine {
    pub(super) fn capture_content_close(
        &self,
        target: ContentCloseTarget,
        policy: &DockPolicySnapshot,
    ) -> Result<FrozenCloseClick, ContentCloseRequestRejection> {
        let expected_items = self.workspace.item_multiset();
        match target {
            ContentCloseTarget::Item(item) => {
                if !expected_items.contains_key(&item) {
                    return Err(ContentCloseRequestRejection::ItemUnavailable { item });
                }
                let capability = policy.pane_close_capability(item);
                if !capability.allows_close() {
                    return Err(ContentCloseRequestRejection::ItemCloseDisabled { item });
                }
                let source = self
                    .workspace
                    .capture_item_source_by_id(item)
                    .map_err(ContentCloseRequestRejection::SourceUnavailable)?
                    .ok_or(ContentCloseRequestRejection::ItemUnavailable { item })?;
                Ok(FrozenCloseClick {
                    target: ClosePlanTarget::Item { item },
                    requirements: vec![CloseItemRequirement::new(item, capability)],
                    prepared: PreparedContentClose::Item {
                        source,
                        expected_items,
                        requirement: CloseItemRequirement::new(item, capability),
                    },
                })
            }
            ContentCloseTarget::Root(root) => {
                let Some(record) = self.workspace.root(root) else {
                    return Err(ContentCloseRequestRejection::RootUnavailable { root });
                };
                let source = self
                    .workspace
                    .capture_node_source(root, record.node)
                    .map_err(ContentCloseRequestRejection::SourceUnavailable)?;
                let items = self.workspace.collect_items_in_subtree(record.node);
                if items.is_empty() {
                    return Err(ContentCloseRequestRejection::RootEmpty { root });
                }
                let mut requirements = Vec::with_capacity(items.len());
                for item in items.iter().copied() {
                    let capability = policy.pane_close_capability(item);
                    if !capability.allows_close() {
                        return Err(ContentCloseRequestRejection::ItemCloseDisabled { item });
                    }
                    requirements.push(CloseItemRequirement::new(item, capability));
                }
                Ok(FrozenCloseClick {
                    target: ClosePlanTarget::Root { root },
                    requirements: requirements.clone(),
                    prepared: PreparedContentClose::Root {
                        source,
                        items,
                        expected_items,
                        requirements,
                    },
                })
            }
        }
    }

    pub(super) fn content_close_rejection_to_interaction(
        target: CloseSceneTarget,
        rejection: ContentCloseRequestRejection,
    ) -> InteractionRejection {
        match rejection {
            ContentCloseRequestRejection::ItemCloseDisabled { .. }
            | ContentCloseRequestRejection::RootEmpty { .. } => {
                InteractionRejection::CloseControlUnavailable { target }
            }
            ContentCloseRequestRejection::SourceUnavailable(error) => {
                InteractionRejection::CloseSourceUnavailable(error)
            }
            ContentCloseRequestRejection::ItemUnavailable { .. }
            | ContentCloseRequestRejection::RootUnavailable { .. } => {
                InteractionRejection::CloseSceneTargetUnavailable { target }
            }
        }
    }

    pub(super) fn request_close_plan(
        &mut self,
        input: InputSequence,
        scene: SurfaceSceneStamp,
        target: CloseSceneTarget,
        activation: CloseActivation,
        policy: &DockPolicySnapshot,
    ) -> Result<InteractionOutcome, EngineError> {
        let surface = scene.surface();
        let painted = match activation {
            CloseActivation::LocalResponse => self.local_response_candidate(scene),
            CloseActivation::Pointer { .. } | CloseActivation::Semantic => self
                .presentation_authority
                .scene
                .ready_surface(surface)
                .filter(|painted| {
                    painted.stamp() == scene
                        && scene.requirement().workspace_epoch() == self.version.epoch()
                })
                .ok_or(InteractionRejection::StaleScene),
        };
        let painted = match painted {
            Ok(painted) => painted,
            Err(error) => return Ok(InteractionOutcome::Rejected(error)),
        };
        if !Self::coordinate_capture_matches_current(
            painted.coordinate_capture(),
            self.viewport.viewport(surface),
            self.viewport.surface_coordinate_authority(surface),
        ) {
            return Ok(InteractionOutcome::Rejected(
                InteractionRejection::CloseCoordinateAuthorityUnavailable { surface },
            ));
        }

        let plan = painted.plan();
        let (close_bounds, layer, content_target) = match target {
            CloseSceneTarget::Tab(id) => {
                let Some(record) = plan.tab_records().iter().find(|record| *record.id() == id)
                else {
                    return Ok(InteractionOutcome::Rejected(
                        InteractionRejection::CloseSceneTargetUnavailable { target },
                    ));
                };
                let Some(close_bounds) = record.close_bounds() else {
                    return Ok(InteractionOutcome::Rejected(
                        InteractionRejection::CloseControlUnavailable { target },
                    ));
                };
                (
                    close_bounds,
                    record.layer(),
                    ContentCloseTarget::Item(id.item),
                )
            }
            CloseSceneTarget::Contained(floating) => {
                let Some(record) = plan.contained_record(floating) else {
                    return Ok(InteractionOutcome::Rejected(
                        InteractionRejection::CloseSceneTargetUnavailable { target },
                    ));
                };
                let Some(close_bounds) = record.close_bounds() else {
                    return Ok(InteractionOutcome::Rejected(
                        InteractionRejection::CloseControlUnavailable { target },
                    ));
                };
                (
                    close_bounds,
                    record.layer(),
                    ContentCloseTarget::Root(record.root()),
                )
            }
        };

        match activation {
            CloseActivation::Pointer {
                pointer: _,
                button,
                position,
            } => {
                if button != crate::intent::PointerButton::Primary {
                    return Ok(InteractionOutcome::Rejected(
                        InteractionRejection::CloseActivationButtonUnsupported { button },
                    ));
                }
                if position.surface() != surface {
                    return Ok(InteractionOutcome::Rejected(
                        InteractionRejection::CloseActivationSurfaceMismatch {
                            expected: surface,
                            actual: position.surface(),
                        },
                    ));
                }
                let point = position.position();
                if !close_bounds.contains(point) {
                    return Ok(InteractionOutcome::Rejected(
                        InteractionRejection::CloseActivationHitMismatch { target },
                    ));
                }
                if !plan.point_is_on_authoritative_layer(point, layer) {
                    return Ok(InteractionOutcome::Rejected(
                        InteractionRejection::CloseActivationOccluded { target },
                    ));
                }
            }
            CloseActivation::Semantic | CloseActivation::LocalResponse => {
                if !plan.region_is_operable(close_bounds, layer) {
                    return Ok(InteractionOutcome::Rejected(
                        InteractionRejection::CloseActivationOccluded { target },
                    ));
                }
            }
        }

        let capture = match self.capture_content_close(content_target, policy) {
            Ok(capture) => capture,
            Err(rejection) => {
                return Ok(InteractionOutcome::Rejected(
                    Self::content_close_rejection_to_interaction(target, rejection),
                ));
            }
        };
        let authority = self.close_authority();
        if let Some(existing) = self
            .close
            .active_plan_for_target(capture.target, authority)
            .cloned()
        {
            return Ok(InteractionOutcome::CloseRequested {
                plan: existing,
                reused: true,
            });
        }
        let plan = self
            .close
            .open(
                authority,
                capture.target,
                capture.requirements,
                PreparedCloseOperation::Content(capture.prepared),
            )
            .map_err(|source| EngineError::ClosePlanInvariant {
                input,
                detail: source.to_string(),
            })?;
        Ok(InteractionOutcome::CloseRequested {
            plan,
            reused: false,
        })
    }

    fn reconcile_noncurrent_native_surface_close_edges(&mut self) {
        let plans = self.close.active_plans().cloned().collect::<Vec<_>>();
        for plan in plans {
            let ClosePlanTarget::Surface { .. } = plan.target() else {
                continue;
            };
            let request = plan.request();
            let Some(edge) = self.close.native_edge(request) else {
                continue;
            };
            match self.viewport.native_close_edge_disposition(edge) {
                NativeCloseEdgeDisposition::ExactPending
                | NativeCloseEdgeDisposition::InventoryMissing
                | NativeCloseEdgeDisposition::BindingMissing
                | NativeCloseEdgeDisposition::AuthorityUnavailable => {}
                NativeCloseEdgeDisposition::DifferentPending { .. }
                | NativeCloseEdgeDisposition::LiveClear { .. }
                | NativeCloseEdgeDisposition::BindingRebound { .. } => {
                    let _ = self
                        .close
                        .mark_native_edge_indeterminate(request, self.close_authority());
                }
                NativeCloseEdgeDisposition::Destroyed { .. } => {
                    let _ = self
                        .close
                        .mark_externally_destroyed_unproved(request, edge.binding());
                }
            }
        }
    }

    pub(super) fn schedule_native_surface_close_effects(
        &mut self,
        input: InputSequence,
    ) -> Result<(), EngineError> {
        self.reconcile_noncurrent_native_surface_close_edges();
        let plans = self.close.active_plans().cloned().collect::<Vec<_>>();
        for plan in plans {
            let ClosePlanTarget::Surface { .. } = plan.target() else {
                continue;
            };
            let request = plan.request();
            let Some(edge) = self.close.native_edge(request) else {
                return Err(EngineError::ClosePlanInvariant {
                    input,
                    detail: format!("surface close {request} has no native edge"),
                });
            };

            if !matches!(
                self.viewport.native_close_edge_disposition(edge),
                NativeCloseEdgeDisposition::ExactPending
            ) {
                continue;
            }

            match plan.phase() {
                ClosePlanPhase::Approved => {
                    if !self.preflight_approved_surface_close(request, input)? {
                        let cancelled = self.close.mark_stale(request, self.close_authority());
                        if !matches!(cancelled, CloseAdvanceOutcome::Advanced { .. }) {
                            return Err(EngineError::ClosePlanInvariant {
                                input,
                                detail: format!(
                                    "surface close {request} could not cancel after preflight failure: {cancelled:?}"
                                ),
                            });
                        }
                        continue;
                    }
                    let _ = self
                        .viewport
                        .request_native_close_resolution(
                            request,
                            edge,
                            NativeCloseResolution::Accept,
                            None,
                        )
                        .map_err(|source| EngineError::Viewport { input, source })?;
                }
                ClosePlanPhase::Vetoed => {
                    let cancelled = self.close.request_cancel(request, self.close_authority());
                    if !matches!(cancelled, CloseAdvanceOutcome::Advanced { .. }) {
                        return Err(EngineError::ClosePlanInvariant {
                            input,
                            detail: format!(
                                "surface close {request} veto retained an unresolved native edge: {cancelled:?}"
                            ),
                        });
                    }
                }
                ClosePlanPhase::CancelRequested | ClosePlanPhase::Indeterminate
                    if matches!(
                        plan.cancellation_state(),
                        crate::close_plan::CloseCancellationState::Requested
                            | crate::close_plan::CloseCancellationState::Indeterminate
                    ) && self.close.native_cancellation_effect(request).is_none() =>
                {
                    let _ = self
                        .viewport
                        .request_native_close_resolution(
                            request,
                            edge,
                            NativeCloseResolution::Cancel,
                            self.close.native_close_effect(request),
                        )
                        .map_err(|source| EngineError::Viewport { input, source })?;
                }
                ClosePlanPhase::Requested
                | ClosePlanPhase::Resolving
                | ClosePlanPhase::Deferred
                | ClosePlanPhase::EffectEmitted
                | ClosePlanPhase::AwaitingDestroyed
                | ClosePlanPhase::Applied
                | ClosePlanPhase::Stale
                | ClosePlanPhase::CancelRequested
                | ClosePlanPhase::Cancelled
                | ClosePlanPhase::ExternallyDestroyedUnproved
                | ClosePlanPhase::Indeterminate => {}
            }
        }
        Ok(())
    }

    fn preflight_approved_surface_close(
        &self,
        request: CloseRequestId,
        _input: InputSequence,
    ) -> Result<bool, EngineError> {
        let Some(prepared) = self.close.prepared(request) else {
            return Ok(false);
        };
        match prepared {
            PreparedCloseOperation::Content(_) => Ok(false),
            PreparedCloseOperation::SurfaceRetain { roster, .. } => Ok(roster
                .matches_workspace(&self.workspace)
                && self.retain_close_requirements_match_plan(request, roster)),
            PreparedCloseOperation::SurfaceRehome {
                roster,
                transaction,
                ..
            } => {
                if !roster.matches_workspace(&self.workspace) {
                    return Ok(false);
                }
                let mut candidate = self.clone_workspace_candidate();
                Ok(
                    WorkspaceTransaction::from_commands(transaction.commands().iter().cloned())
                        .apply(&mut candidate, &self.policy)
                        .is_ok(),
                )
            }
            PreparedCloseOperation::SurfaceContent {
                roster, prepared, ..
            } => {
                if !roster.matches_workspace(&self.workspace) {
                    return Ok(false);
                }
                Ok(prepare_surface_content_close(&self.workspace, &self.policy, prepared).is_ok())
            }
        }
    }

    fn retain_close_requirements_match_plan(
        &self,
        request: CloseRequestId,
        roster: &SurfaceRosterDisposition,
    ) -> bool {
        let Ok(requirements) = self.capture_surface_close_requirements(roster, &self.policy) else {
            return false;
        };
        let Some(plan) = self.close.plan(request) else {
            return false;
        };
        requirements.len() == plan.items().len()
            && requirements
                .iter()
                .zip(plan.items())
                .all(|(expected, actual)| {
                    expected.item() == actual.item() && expected.capability() == actual.capability()
                })
    }

    pub(super) fn record_emitted_native_surface_close_effects(
        &mut self,
        input: InputSequence,
        effects: &[PlatformEffectEmission],
    ) -> Result<(), EngineError> {
        for effect in effects {
            let PlatformEffect::ResolveNativeClose {
                request,
                edge,
                resolution,
            } = effect.effect()
            else {
                continue;
            };
            let fence = effect.native_close_emission_fence().ok_or_else(|| {
                EngineError::ClosePlanInvariant {
                    input,
                    detail: format!(
                        "native close effect {:?} was emitted without an authority fence",
                        effect.id()
                    ),
                }
            })?;
            if fence.provider() != effect.provider()
                || effect.native_close_edge() != Some(*edge)
                || fence.binding() != edge.binding()
                || fence.observed_through() != edge.observed_at()
                || fence.received_through() < edge.received_at()
            {
                return Err(EngineError::ClosePlanInvariant {
                    input,
                    detail: format!(
                        "native close effect {:?} fence differs from its exact close edge",
                        effect.id()
                    ),
                });
            }
            if self.close.native_edge(*request) != Some(*edge) {
                return Err(EngineError::ClosePlanInvariant {
                    input,
                    detail: format!(
                        "native close effect {:?} differs from close plan {request} edge",
                        effect.id()
                    ),
                });
            }
            let authority = self.close_authority();
            let advanced = match resolution {
                NativeCloseResolution::Accept => self.close.mark_effect_emitted(
                    *request,
                    authority,
                    edge.binding(),
                    effect.id(),
                    fence.observed_through(),
                    fence.received_through(),
                    fence.after_effect(),
                ),
                NativeCloseResolution::Cancel => self.close.mark_cancellation_effect_emitted(
                    *request,
                    authority,
                    edge.binding(),
                    effect.id(),
                    fence.observed_through(),
                    fence.received_through(),
                    fence.after_effect(),
                ),
            };
            if !matches!(advanced, CloseAdvanceOutcome::Advanced { .. }) {
                return Err(EngineError::ClosePlanInvariant {
                    input,
                    detail: format!(
                        "native close effect {:?} could not attach to plan {request}: {advanced:?}",
                        effect.id()
                    ),
                });
            }
            if *resolution == NativeCloseResolution::Accept {
                self.viewport
                    .mark_native_close_awaiting_destroyed(edge.binding())
                    .map_err(|source| EngineError::Viewport { input, source })?;
            }
        }
        Ok(())
    }

    pub(super) fn close_authority(&self) -> CloseAuthority {
        CloseAuthority::new(self.authority_domain, self.version, self.policy.revision())
    }

    pub(super) fn reduce_close_decision(
        &mut self,
        input: InputSequence,
        request: CloseRequestId,
        token: CloseDecisionToken,
        decision: CloseDecision,
        events: &mut Vec<WorkspaceEvent>,
        interaction_events: &mut Vec<InteractionEvent>,
    ) -> Result<InputOutcome, EngineError> {
        let authority = self.close_authority();
        let resolution = self
            .close
            .resolve(request, token, authority, decision)
            .map_err(|source| EngineError::ClosePlanInvariant {
                input,
                detail: source.to_string(),
            })?;
        self.finish_close_resolution(input, request, resolution, events, interaction_events)
    }

    pub(super) fn reduce_deferred_close_decision(
        &mut self,
        input: InputSequence,
        request: CloseRequestId,
        token: DeferredCloseToken,
        decision: DeferredCloseDecision,
        events: &mut Vec<WorkspaceEvent>,
        interaction_events: &mut Vec<InteractionEvent>,
    ) -> Result<InputOutcome, EngineError> {
        let authority = self.close_authority();
        let resolution = self
            .close
            .continue_deferred(request, token, authority, decision);
        self.finish_close_resolution(input, request, resolution, events, interaction_events)
    }

    fn finish_close_resolution(
        &mut self,
        input: InputSequence,
        request: CloseRequestId,
        resolution: CloseResolutionOutcome,
        events: &mut Vec<WorkspaceEvent>,
        interaction_events: &mut Vec<InteractionEvent>,
    ) -> Result<InputOutcome, EngineError> {
        let mut application = None;
        let mut changed = false;
        match resolution {
            CloseResolutionOutcome::Approved { .. }
                if matches!(
                    self.close.plan(request).map(ClosePlan::target),
                    Some(ClosePlanTarget::Surface { .. })
                ) =>
            {
                // Native surface topology remains frozen until a later exact
                // destroyed observation. Tick-final scheduling emits the
                // accept effect after every input in this host frame settled.
            }
            CloseResolutionOutcome::Approved { .. } => {
                match self.apply_approved_content_close(
                    input,
                    request,
                    events,
                    interaction_events,
                )? {
                    ContentCloseApplication::Applied {
                        outcome,
                        changed: did_change,
                    } => {
                        changed = did_change;
                        application = Some(Ok(outcome));
                    }
                    ContentCloseApplication::Rejected(error) => {
                        let authority = self.close_authority();
                        let stale = self.close.mark_stale(request, authority);
                        if !matches!(stale, CloseAdvanceOutcome::Advanced { .. }) {
                            return Err(EngineError::ClosePlanInvariant {
                                input,
                                detail: format!(
                                    "approved close {request} could not become stale after commit rejection: {stale:?}"
                                ),
                            });
                        }
                        application = Some(Err(error));
                    }
                }
            }
            CloseResolutionOutcome::Vetoed { .. }
                if matches!(
                    self.close.plan(request).map(ClosePlan::target),
                    Some(ClosePlanTarget::Surface { .. })
                ) =>
            {
                let cancellation = self.close.request_cancel(request, self.close_authority());
                if !matches!(cancellation, CloseAdvanceOutcome::Advanced { .. }) {
                    return Err(EngineError::ClosePlanInvariant {
                        input,
                        detail: format!(
                            "vetoed surface close {request} could not begin cancellation: {cancellation:?}"
                        ),
                    });
                }
            }
            CloseResolutionOutcome::Recorded { .. }
            | CloseResolutionOutcome::Deferred { .. }
            | CloseResolutionOutcome::Vetoed { .. }
            | CloseResolutionOutcome::Inert(_) => {}
        }
        Ok(InputOutcome::CloseDecisionProcessed {
            resolution,
            plan: self.close.plan(request).cloned(),
            application,
            changed,
            version: self.version,
        })
    }

    fn apply_approved_content_close(
        &mut self,
        input: InputSequence,
        request: CloseRequestId,
        events: &mut Vec<WorkspaceEvent>,
        interaction_events: &mut Vec<InteractionEvent>,
    ) -> Result<ContentCloseApplication, EngineError> {
        let authority = self.close_authority();
        let approved = self.close.approved(request, authority).map_err(|reason| {
            EngineError::ClosePlanInvariant {
                input,
                detail: format!("approved close {request} is unavailable: {reason:?}"),
            }
        })?;
        if matches!(approved.plan().target(), ClosePlanTarget::Surface { .. }) {
            return Err(EngineError::ClosePlanInvariant {
                input,
                detail: format!("surface close {request} reached the local commit path"),
            });
        }
        let prepared = approved.prepared().clone();
        let PreparedCloseOperation::Content(prepared) = prepared else {
            return Err(EngineError::ClosePlanInvariant {
                input,
                detail: format!("local close {request} has a non-content prepared payload"),
            });
        };
        let commit = match prepare_content_close(&self.workspace, &self.policy, &prepared) {
            Ok(commit) => commit,
            Err(TransactionError::Command { index: 0, source })
                if source.is_expected_rejection() =>
            {
                return Ok(ContentCloseApplication::Rejected(source));
            }
            Err(source) => return Err(EngineError::Command { input, source }),
        };
        if let Some(surface) =
            self.first_workspace_publication_mismatch(&commit.candidate, None, None)
        {
            return Ok(ContentCloseApplication::Rejected(
                CommandError::SurfaceLifecycleFrozen { surface },
            ));
        }
        let changed = commit.candidate != self.workspace;
        let outcome = commit.outcome;

        let publication = match self.stage_workspace_publication(commit.candidate, &self.policy) {
            Ok(publication) => publication,
            Err(source) if source.is_expected_rejection() => {
                return Ok(ContentCloseApplication::Rejected(source));
            }
            Err(source) => {
                return Err(EngineError::Command {
                    input,
                    source: TransactionError::Command { index: 0, source },
                });
            }
        };
        self.publish_workspace(publication);
        self.reconcile_viewport_focus_authority();
        let applied = self.close.mark_local_applied(request, authority);
        if !matches!(applied, CloseAdvanceOutcome::Advanced { .. }) {
            return Err(EngineError::ClosePlanInvariant {
                input,
                detail: format!(
                    "approved close {request} could not be marked applied: {applied:?}"
                ),
            });
        }
        if changed {
            self.advance_revision(input)?;
            self.reconcile_interaction_after_close(input, interaction_events)?;
            events.push(WorkspaceEvent::new(
                input,
                self.version,
                WorkspaceEventKind::CloseCommitted(outcome.clone()),
            ));
        }
        Ok(ContentCloseApplication::Applied { outcome, changed })
    }

    pub(super) fn reconcile_interaction_after_close(
        &mut self,
        input: InputSequence,
        interaction_events: &mut Vec<InteractionEvent>,
    ) -> Result<(), EngineError> {
        let status = self.interaction.status();
        match status {
            InteractionStatus::Idle => {}
            InteractionStatus::Pressed { .. } => {
                self.invalidate_transient(
                    input,
                    InteractionCancelReason::WorkspaceChanged,
                    interaction_events,
                )?;
            }
            InteractionStatus::Armed { session } | InteractionStatus::Dragging { session } => {
                let Some(active) = self.interaction.active_drag_view() else {
                    return Err(EngineError::Interaction {
                        input,
                        source: InteractionCounterError::StateInvariant,
                    });
                };
                let source_is_current = self.prepare_drag_source(active.payload()).is_ok();
                if !source_is_current {
                    self.viewport
                        .end_all_drag_routing()
                        .map_err(|source| EngineError::Viewport { input, source })?;
                    let cancelled = self.interaction.cancel_drag(session).map_err(|_| {
                        EngineError::Interaction {
                            input,
                            source: InteractionCounterError::StateInvariant,
                        }
                    })?;
                    interaction_events.push(InteractionEvent::new(
                        input,
                        self.version,
                        InteractionEventKind::Cancelled {
                            status: cancelled,
                            reason: InteractionCancelReason::SourceVanished,
                        },
                    ));
                } else if matches!(status, InteractionStatus::Dragging { .. })
                    && self.interaction.clear_drag_feedback(session).map_err(|_| {
                        EngineError::Interaction {
                            input,
                            source: InteractionCounterError::StateInvariant,
                        }
                    })?
                {
                    interaction_events.push(InteractionEvent::new(
                        input,
                        self.version,
                        InteractionEventKind::PreviewCleared { session },
                    ));
                }
            }
            InteractionStatus::Resizing { .. }
            | InteractionStatus::ContainedTransforming { .. } => {
                self.invalidate_transient(
                    input,
                    InteractionCancelReason::WorkspaceChanged,
                    interaction_events,
                )?;
            }
        }
        Ok(())
    }

    pub(super) fn reduce_content_close_request(
        &mut self,
        input: InputSequence,
        expected: WorkspaceVersion,
        target: ContentCloseTarget,
        application_base: WorkspaceVersion,
        policy: &DockPolicySnapshot,
    ) -> Result<InputOutcome, EngineError> {
        if expected != application_base {
            return Ok(InputOutcome::StaleRejected {
                expected,
                accepted_base: application_base,
            });
        }
        let capture = match self.capture_content_close(target, policy) {
            Ok(capture) => capture,
            Err(reason) => {
                return Ok(InputOutcome::ContentCloseRejected {
                    target,
                    reason,
                    version: self.version,
                });
            }
        };
        let authority = self.close_authority();
        if let Some(plan) = self
            .close
            .active_plan_for_target(capture.target, authority)
            .cloned()
        {
            return Ok(InputOutcome::ContentCloseRequested {
                target,
                plan,
                reused: true,
                version: self.version,
            });
        }
        let plan = self
            .close
            .open(
                authority,
                capture.target,
                capture.requirements,
                PreparedCloseOperation::Content(capture.prepared),
            )
            .map_err(|source| EngineError::ClosePlanInvariant {
                input,
                detail: source.to_string(),
            })?;
        Ok(InputOutcome::ContentCloseRequested {
            target,
            plan,
            reused: false,
            version: self.version,
        })
    }

    pub(super) fn reduce_surface_close_request(
        &mut self,
        input: InputSequence,
        expected: WorkspaceVersion,
        edge: NativeCloseEdge,
        request: SurfaceCloseRequest,
        application_base: WorkspaceVersion,
        policy: &DockPolicySnapshot,
    ) -> Result<InputOutcome, EngineError> {
        if expected != application_base {
            return Ok(InputOutcome::StaleRejected {
                expected,
                accepted_base: application_base,
            });
        }
        if self.viewport.is_staging_binding(edge.binding()) {
            return Ok(InputOutcome::SurfaceCloseRejected {
                edge,
                request,
                reason: SurfaceCloseRequestRejection::StagingBinding {
                    binding: edge.binding(),
                },
                version: self.version,
            });
        }
        if !self.native_close_edge_is_current(edge) {
            return Ok(InputOutcome::SurfaceCloseRejected {
                edge,
                request,
                reason: SurfaceCloseRequestRejection::EdgeUnavailable {
                    binding: edge.binding(),
                },
                version: self.version,
            });
        }
        let close_cancellation_supported = self
            .viewport
            .capabilities()
            .close_cancellation()
            .is_supported();
        let accept_only_candidate = matches!(&request, SurfaceCloseRequest::RehomeAll { .. });
        if !close_cancellation_supported && !accept_only_candidate {
            return Ok(self.reject_surface_close_without_cancellation(edge, request));
        }
        let surface = edge.binding().surface();
        if self.workspace.surface(surface).is_none() {
            return Ok(InputOutcome::SurfaceCloseRejected {
                edge,
                request,
                reason: SurfaceCloseRequestRejection::SurfaceUnavailable { surface },
                version: self.version,
            });
        }
        if let PolicyDecision::Reject(reason) =
            policy.evaluate(&DockPolicyRequest::CloseSurface { surface })
        {
            if !close_cancellation_supported {
                return Ok(self.reject_surface_close_without_cancellation(edge, request));
            }
            return self.reject_surface_close_with_cancellation(
                input,
                edge,
                request,
                SurfaceCloseRequestRejection::PolicyRejected(reason),
            );
        }
        if let Some(request_id) = self.close.surface_request_for_edge(edge) {
            return Ok(InputOutcome::SurfaceCloseRejected {
                edge,
                request,
                reason: SurfaceCloseRequestRejection::ActivePlan {
                    request: request_id,
                },
                version: self.version,
            });
        }

        let capture = match self.capture_surface_close(edge, &request, policy) {
            Ok(capture) => capture,
            Err(reason) => {
                if !close_cancellation_supported {
                    return Ok(self.reject_surface_close_without_cancellation(edge, request));
                }
                return self.reject_surface_close_with_cancellation(input, edge, request, reason);
            }
        };
        let accepts_without_cancellation = accept_only_candidate && capture.requirements.is_empty();
        if !close_cancellation_supported && !accepts_without_cancellation {
            return Ok(self.reject_surface_close_without_cancellation(edge, request));
        }
        let plan = self
            .close
            .open_surface(
                self.close_authority(),
                edge,
                request.clone(),
                capture.requirements,
                capture.prepared,
            )
            .map_err(|source| EngineError::ClosePlanInvariant {
                input,
                detail: source.to_string(),
            })?;
        Ok(InputOutcome::SurfaceCloseRequested {
            edge,
            request,
            plan,
            version: self.version,
        })
    }

    fn reject_surface_close_without_cancellation(
        &self,
        edge: NativeCloseEdge,
        request: SurfaceCloseRequest,
    ) -> InputOutcome {
        InputOutcome::SurfaceCloseRejected {
            edge,
            request,
            reason: SurfaceCloseRequestRejection::CancellationUnsupported,
            version: self.version,
        }
    }

    fn reject_surface_close_with_cancellation(
        &mut self,
        input: InputSequence,
        edge: NativeCloseEdge,
        request: SurfaceCloseRequest,
        reason: SurfaceCloseRequestRejection,
    ) -> Result<InputOutcome, EngineError> {
        let surface = edge.binding().surface();
        let roster = self
            .capture_surface_roster(surface)
            .map_err(|source| EngineError::SurfaceRoster { input, source })?;
        let authority = self.close_authority();
        let plan = self
            .close
            .open_surface(
                authority,
                edge,
                SurfaceCloseRequest::RetainLayout,
                [],
                PreparedCloseOperation::SurfaceRetain {
                    roster,
                    recovery_focus: self.frozen_surface_recovery_focus(surface),
                },
            )
            .map_err(|source| EngineError::ClosePlanInvariant {
                input,
                detail: source.to_string(),
            })?;
        let cancellation = self.close.request_cancel(plan.request(), authority);
        if !matches!(cancellation, CloseAdvanceOutcome::Advanced { .. }) {
            return Err(EngineError::ClosePlanInvariant {
                input,
                detail: format!(
                    "policy-rejected native close {} could not enter cancellation: {cancellation:?}",
                    plan.request()
                ),
            });
        }
        Ok(InputOutcome::SurfaceCloseRejected {
            edge,
            request,
            reason,
            version: self.version,
        })
    }

    pub(super) fn reduce_surface_close_cancellation(
        &mut self,
        input: InputSequence,
        expected: WorkspaceVersion,
        edge: NativeCloseEdge,
        application_base: WorkspaceVersion,
    ) -> Result<InputOutcome, EngineError> {
        if expected != application_base {
            return Ok(InputOutcome::StaleRejected {
                expected,
                accepted_base: application_base,
            });
        }
        let request = SurfaceCloseRequest::RetainLayout;
        if self.viewport.is_staging_binding(edge.binding()) {
            return Ok(InputOutcome::SurfaceCloseRejected {
                edge,
                request,
                reason: SurfaceCloseRequestRejection::StagingBinding {
                    binding: edge.binding(),
                },
                version: self.version,
            });
        }
        if !self.native_close_edge_is_current(edge) {
            return Ok(InputOutcome::SurfaceCloseRejected {
                edge,
                request,
                reason: SurfaceCloseRequestRejection::EdgeUnavailable {
                    binding: edge.binding(),
                },
                version: self.version,
            });
        }
        if !self
            .viewport
            .capabilities()
            .close_cancellation()
            .is_supported()
        {
            return Ok(InputOutcome::SurfaceCloseRejected {
                edge,
                request,
                reason: SurfaceCloseRequestRejection::CancellationUnsupported,
                version: self.version,
            });
        }

        let authority = self.close_authority();
        let request_id = match self.close.surface_request_for_edge(edge) {
            Some(request_id) => request_id,
            None => {
                let surface = edge.binding().surface();
                let roster = match self.capture_surface_roster(surface) {
                    Ok(roster) => roster,
                    Err(_) => {
                        return Ok(InputOutcome::SurfaceCloseRejected {
                            edge,
                            request,
                            reason: SurfaceCloseRequestRejection::SurfaceUnavailable { surface },
                            version: self.version,
                        });
                    }
                };
                let recovery_focus = self.frozen_surface_recovery_focus(surface);
                let plan = self
                    .close
                    .open_surface(
                        authority,
                        edge,
                        SurfaceCloseRequest::RetainLayout,
                        [],
                        PreparedCloseOperation::SurfaceRetain {
                            roster,
                            recovery_focus,
                        },
                    )
                    .map_err(|source| EngineError::ClosePlanInvariant {
                        input,
                        detail: source.to_string(),
                    })?;
                plan.request()
            }
        };
        let advanced = self.close.request_cancel(request_id, authority);
        if !matches!(advanced, CloseAdvanceOutcome::Advanced { .. })
            && !matches!(
                self.close.plan(request_id).map(ClosePlan::phase),
                Some(ClosePlanPhase::CancelRequested | ClosePlanPhase::Indeterminate)
            )
        {
            return Ok(InputOutcome::SurfaceCloseRejected {
                edge,
                request,
                reason: SurfaceCloseRequestRejection::ActivePlan {
                    request: request_id,
                },
                version: self.version,
            });
        }
        let plan = self
            .close
            .plan(request_id)
            .cloned()
            .ok_or(EngineError::ClosePlanInvariant {
                input,
                detail: format!("surface close {request_id} disappeared during cancellation"),
            })?;
        Ok(InputOutcome::SurfaceCloseCancellationRequested {
            edge,
            plan,
            version: self.version,
        })
    }

    pub(super) fn native_close_edge_is_current(&self, edge: NativeCloseEdge) -> bool {
        edge.domain() == self.authority_domain
            && matches!(
                self.viewport.native_close_edge_disposition(edge),
                NativeCloseEdgeDisposition::ExactPending
            )
    }

    fn frozen_surface_recovery_focus(
        &self,
        surface: crate::ids::SurfaceId,
    ) -> PaneFocusDisposition {
        PaneFocusDisposition::from_record(self.viewport_focus.panel_focus(surface))
    }

    fn capture_surface_close(
        &self,
        edge: NativeCloseEdge,
        request: &SurfaceCloseRequest,
        policy: &DockPolicySnapshot,
    ) -> Result<SurfaceCloseCapture, SurfaceCloseRequestRejection> {
        let surface = edge.binding().surface();
        let roster = self
            .capture_surface_roster(surface)
            .map_err(|_| SurfaceCloseRequestRejection::SurfaceUnavailable { surface })?;
        let recovery_focus = self.frozen_surface_recovery_focus(surface);
        match request {
            SurfaceCloseRequest::RetainLayout => {
                let requirements = self.capture_surface_close_requirements(&roster, policy)?;
                Ok(SurfaceCloseCapture {
                    requirements,
                    prepared: PreparedCloseOperation::SurfaceRetain {
                        roster,
                        recovery_focus,
                    },
                })
            }
            SurfaceCloseRequest::RehomeAll { target } => {
                let transaction = self.compile_surface_rehome(&roster, target, policy)?;
                Ok(SurfaceCloseCapture {
                    requirements: Vec::new(),
                    prepared: PreparedCloseOperation::SurfaceRehome {
                        roster,
                        transaction,
                        recovery_focus,
                    },
                })
            }
            SurfaceCloseRequest::CloseContent => {
                let mut roots = Vec::new();
                let requirements = self.capture_surface_close_requirements(&roster, policy)?;
                for source in roster.roster().main_source().into_iter().chain(
                    roster
                        .contained()
                        .iter()
                        .map(|contained| contained.source()),
                ) {
                    let root = self
                        .workspace
                        .root(source.root())
                        .ok_or(SurfaceCloseRequestRejection::CloseContentUnavailable)?;
                    let items = self.workspace.collect_items_in_subtree(root.node);
                    if items.is_empty() {
                        return Err(SurfaceCloseRequestRejection::CloseContentUnavailable);
                    }
                    roots.push((source.clone(), items));
                }
                if roots.is_empty() {
                    return Err(SurfaceCloseRequestRejection::CloseContentUnavailable);
                }
                let prepared = PreparedSurfaceContentClose::new(
                    roster.roster().clone(),
                    roots,
                    requirements.clone(),
                );
                Ok(SurfaceCloseCapture {
                    requirements,
                    prepared: PreparedCloseOperation::SurfaceContent {
                        roster,
                        prepared,
                        recovery_focus,
                    },
                })
            }
        }
    }

    fn capture_surface_close_requirements(
        &self,
        roster: &SurfaceRosterDisposition,
        policy: &DockPolicySnapshot,
    ) -> Result<Vec<CloseItemRequirement>, SurfaceCloseRequestRejection> {
        let mut requirements = Vec::new();
        for source in roster.roster().main_source().into_iter().chain(
            roster
                .contained()
                .iter()
                .map(|contained| contained.source()),
        ) {
            let root = self.workspace.root(source.root()).ok_or(
                SurfaceCloseRequestRejection::SurfaceUnavailable {
                    surface: roster.surface(),
                },
            )?;
            for item in self.workspace.collect_items_in_subtree(root.node) {
                let capability = policy.pane_close_capability(item);
                if !capability.allows_close() {
                    return Err(SurfaceCloseRequestRejection::PaneCloseDisabled { item });
                }
                requirements.push(CloseItemRequirement::new(item, capability));
            }
        }
        Ok(requirements)
    }

    fn compile_surface_rehome(
        &self,
        roster: &SurfaceRosterDisposition,
        target: &SurfaceRehomeTarget,
        policy: &DockPolicySnapshot,
    ) -> Result<SurfaceRecoveryTransaction, SurfaceCloseRequestRejection> {
        if target.surface() == roster.surface()
            || target.contained().len() != roster.contained().len()
        {
            return Err(SurfaceCloseRequestRejection::RehomeProgramUnavailable);
        }
        let main = match (roster.roster().main_source(), target.main()) {
            (None, None) => None,
            (Some(_), Some(SurfaceMainRehomeTarget::Dock(target))) => {
                Some(SurfaceMainRehome::Dock(target.clone()))
            }
            (Some(_), Some(SurfaceMainRehomeTarget::Main)) => {
                Some(SurfaceMainRehome::Present(RootPresentationTarget::Main {
                    surface: target.surface(),
                }))
            }
            (
                Some(_),
                Some(SurfaceMainRehomeTarget::Contained {
                    floating,
                    rect,
                    position,
                }),
            ) => Some(SurfaceMainRehome::Present(
                RootPresentationTarget::Contained {
                    surface: target.surface(),
                    floating: *floating,
                    rect: *rect,
                    position: *position,
                },
            )),
            (Some(_), None) | (None, Some(_)) => {
                return Err(SurfaceCloseRequestRejection::RehomeProgramUnavailable);
            }
        };
        let contained = roster
            .contained()
            .iter()
            .zip(target.contained())
            .map(|(source, destination)| {
                (source.floating() == destination.floating()).then_some(
                    ContainedRootPlacement::new(
                        source.floating(),
                        source.root(),
                        destination.rect(),
                    ),
                )
            })
            .collect::<Option<Vec<_>>>()
            .ok_or(SurfaceCloseRequestRejection::RehomeProgramUnavailable)?;
        let placement = SurfaceRehomePlacement::new(target.surface(), main, contained);
        let transaction = roster
            .compile_rehome_transaction(&self.workspace, &placement)
            .ok_or(SurfaceCloseRequestRejection::RehomeProgramUnavailable)?;

        // A native close effect is only allowed after the complete frozen
        // transaction is accepted by the same policy snapshot that accepted
        // the surface request. Recovery application itself is intentionally
        // policy-independent because an already-destroyed window must not be
        // left with a half-applied topology.
        let mut candidate = self.clone_workspace_candidate();
        match WorkspaceTransaction::from_commands(transaction.commands().iter().cloned())
            .apply(&mut candidate, policy)
        {
            Ok(_) => {}
            Err(TransactionError::Command {
                source: CommandError::Policy(reason),
                ..
            }) => return Err(SurfaceCloseRequestRejection::RehomePolicyRejected(reason)),
            Err(_) => return Err(SurfaceCloseRequestRejection::RehomeProgramUnavailable),
        }
        Ok(transaction)
    }
}
