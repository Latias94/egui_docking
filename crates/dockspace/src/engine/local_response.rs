//! Current-frame framework response actions over an exact Ready candidate.

use super::*;

impl DockEngine {
    pub(super) fn local_response_candidate(
        &self,
        scene: SurfaceSceneStamp,
    ) -> Result<&crate::scene::SurfacePlanScene, InteractionRejection> {
        let candidate = self
            .presentation_authority
            .scene
            .surface(scene.surface())
            .and_then(SurfaceScene::ready)
            .map(crate::scene::ReadySurfaceScene::candidate)
            .filter(|candidate| candidate.stamp() == scene)
            .filter(|candidate| {
                candidate.stamp().requirement().workspace_epoch() == self.version.epoch()
            })
            .ok_or(InteractionRejection::StaleScene)?;
        Ok(candidate)
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn reduce_local_scene_tab_select(
        &mut self,
        input: InputSequence,
        expected: WorkspaceVersion,
        application_base: WorkspaceVersion,
        scene: SurfaceSceneStamp,
        tab: crate::scene::TabSceneId,
        policy: &DockPolicySnapshot,
        events: &mut Vec<WorkspaceEvent>,
        interaction_events: &mut Vec<InteractionEvent>,
    ) -> Result<InputOutcome, EngineError> {
        if expected != application_base {
            return Ok(InputOutcome::StaleRejected {
                expected,
                accepted_base: application_base,
            });
        }

        let source = {
            let candidate = match self.local_response_candidate(scene) {
                Ok(candidate) => candidate,
                Err(error) => return Ok(self.local_response_rejection(error)),
            };
            let Some(record) = candidate
                .plan()
                .tab_records()
                .iter()
                .find(|record| *record.id() == tab)
            else {
                return Ok(self.local_response_rejection(
                    InteractionRejection::TabGestureSourceUnavailable {
                        source: TabGestureSource::Item(tab),
                    },
                ));
            };
            if !candidate
                .plan()
                .region_is_operable(record.drag_hit().rect(), record.layer())
            {
                return Ok(self.local_response_rejection(
                    InteractionRejection::SemanticReceiverUnavailable {
                        target: PresentationHitRegionKind::TabBody(tab),
                    },
                ));
            }
            match self
                .workspace
                .capture_item_source(tab.root, tab.tabs, tab.item)
            {
                Ok(source) => source,
                Err(_) => {
                    return Ok(self.local_response_rejection(
                        InteractionRejection::TabGestureSourceUnavailable {
                            source: TabGestureSource::Item(tab),
                        },
                    ));
                }
            }
        };

        self.reduce_workspace_command(
            input,
            expected,
            application_base,
            &WorkspaceCommand::Select { source },
            policy,
            events,
            interaction_events,
        )
    }

    pub(super) fn reduce_local_scene_close_input(
        &mut self,
        input: InputSequence,
        expected: WorkspaceVersion,
        scene: SurfaceSceneStamp,
        target: CloseSceneTarget,
        policy: &DockPolicySnapshot,
    ) -> Result<InputOutcome, EngineError> {
        self.reduce_versioned_interaction(expected, |engine| {
            engine.request_close_plan(input, scene, target, CloseActivation::LocalResponse, policy)
        })
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn reduce_local_splitter_adjustment_input(
        &mut self,
        input: InputSequence,
        expected: WorkspaceVersion,
        application_base: WorkspaceVersion,
        scene: SurfaceSceneStamp,
        splitter: SplitterSceneId,
        delta: f64,
        policy: &DockPolicySnapshot,
        events: &mut Vec<WorkspaceEvent>,
        interaction_events: &mut Vec<InteractionEvent>,
    ) -> Result<InputOutcome, EngineError> {
        if expected != application_base {
            return Ok(InputOutcome::StaleRejected {
                expected,
                accepted_base: application_base,
            });
        }
        let outcome = self.adjust_splitter_resize(
            input,
            scene,
            splitter,
            delta,
            SplitterAdjustmentAuthority::LocalReady,
            policy,
            events,
            interaction_events,
        )?;
        Ok(InputOutcome::InteractionProcessed {
            outcome,
            version: self.version,
        })
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn reduce_local_tab_gesture(
        &mut self,
        input: InputSequence,
        cause: ReductionCause,
        focus_causal: FocusCausalStamp,
        expected: WorkspaceVersion,
        application_base: WorkspaceVersion,
        surface: SurfaceId,
        source: TabGestureSource,
        phase: LocalTabGesturePhase,
        policy: &DockPolicySnapshot,
        events: &mut Vec<WorkspaceEvent>,
        interaction_events: &mut Vec<InteractionEvent>,
    ) -> Result<InputOutcome, EngineError> {
        if expected != application_base {
            return Ok(InputOutcome::StaleRejected {
                expected,
                accepted_base: application_base,
            });
        }
        let owner = GestureOwner::LocalResponse { surface };
        let outcome = match phase {
            LocalTabGesturePhase::Begin {
                scene,
                initial,
                current,
            } => self.begin_local_tab_gesture(
                cause,
                focus_causal,
                owner,
                source,
                scene,
                initial,
                current,
                policy,
                events,
                interaction_events,
            )?,
            LocalTabGesturePhase::Move { scene, current } => self.update_local_tab_gesture(
                cause,
                owner,
                source,
                scene,
                current,
                policy,
                interaction_events,
            )?,
            LocalTabGesturePhase::Release { scene, current } => self.release_local_tab_gesture(
                cause,
                focus_causal,
                owner,
                source,
                scene,
                current,
                policy,
                events,
                interaction_events,
            )?,
            LocalTabGesturePhase::Cancel => {
                self.cancel_local_tab_gesture(input, owner, source, interaction_events)
            }
        };
        Ok(InputOutcome::InteractionProcessed {
            outcome,
            version: self.version,
        })
    }

    #[allow(clippy::too_many_arguments)]
    fn begin_local_tab_gesture(
        &mut self,
        cause: ReductionCause,
        focus_causal: FocusCausalStamp,
        owner: GestureOwner,
        source: TabGestureSource,
        scene: SurfaceSceneStamp,
        initial: LogicalPoint,
        current: LogicalPoint,
        policy: &DockPolicySnapshot,
        events: &mut Vec<WorkspaceEvent>,
        interaction_events: &mut Vec<InteractionEvent>,
    ) -> Result<InteractionOutcome, EngineError> {
        if self.interaction.status() != InteractionStatus::Idle {
            return Ok(InteractionOutcome::Rejected(
                InteractionRejection::SessionMismatch,
            ));
        }
        let prepared = match self
            .local_response_candidate(scene)
            .and_then(|candidate| self.prepare_local_tab_gesture(candidate, source, initial))
        {
            Ok(prepared) => prepared,
            Err(rejection) => return Ok(InteractionOutcome::Rejected(rejection)),
        };
        let armed = self.activate_prepared_tab_gesture(
            cause,
            focus_causal,
            owner,
            None,
            None,
            prepared,
            policy,
            events,
        )?;
        let InteractionOutcome::DragArmed { session, .. } = armed else {
            return Ok(armed);
        };
        if let Err(rejection) = self
            .interaction
            .begin_drag(session, owner, PointerButton::Primary)
        {
            return Ok(InteractionOutcome::Rejected(rejection));
        }
        if current != initial {
            let drag = self
                .interaction
                .active_drag(session)
                .map_err(|source| EngineError::PointerInteractionInvariant {
                    cause,
                    detail: format!("local tab drag session disappeared after begin: {source:?}"),
                })?
                .clone();
            let evaluation =
                self.resolve_local_tab_preview(cause, &drag, scene, current, policy)?;
            let _ = self.apply_preview_evaluation(
                cause,
                owner,
                session,
                evaluation,
                interaction_events,
            )?;
        }
        Ok(InteractionOutcome::DragBegan { session })
    }

    fn update_local_tab_gesture(
        &mut self,
        cause: ReductionCause,
        owner: GestureOwner,
        source: TabGestureSource,
        scene: SurfaceSceneStamp,
        current: LogicalPoint,
        policy: &DockPolicySnapshot,
        interaction_events: &mut Vec<InteractionEvent>,
    ) -> Result<InteractionOutcome, EngineError> {
        let session = match self.local_tab_session(owner, source) {
            Ok(session) => session,
            Err(rejection) => return Ok(InteractionOutcome::Rejected(rejection)),
        };
        let drag = self
            .interaction
            .active_drag(session)
            .map_err(|source| EngineError::PointerInteractionInvariant {
                cause,
                detail: format!("local tab drag session disappeared: {source:?}"),
            })?
            .clone();
        let evaluation = self.resolve_local_tab_preview(cause, &drag, scene, current, policy)?;
        self.apply_preview_evaluation(cause, owner, session, evaluation, interaction_events)?
            .ok_or(EngineError::ReductionCauseInvariant {
                detail: "local tab preview evaluation produced no outcome",
            })
    }

    #[allow(clippy::too_many_arguments)]
    fn release_local_tab_gesture(
        &mut self,
        cause: ReductionCause,
        focus_causal: FocusCausalStamp,
        owner: GestureOwner,
        source: TabGestureSource,
        scene: SurfaceSceneStamp,
        current: LogicalPoint,
        policy: &DockPolicySnapshot,
        events: &mut Vec<WorkspaceEvent>,
        interaction_events: &mut Vec<InteractionEvent>,
    ) -> Result<InteractionOutcome, EngineError> {
        let session = match self.local_tab_session(owner, source) {
            Ok(session) => session,
            Err(rejection) => return Ok(InteractionOutcome::Rejected(rejection)),
        };
        let drag = self
            .interaction
            .active_drag(session)
            .map_err(|source| EngineError::PointerInteractionInvariant {
                cause,
                detail: format!("local tab release session disappeared: {source:?}"),
            })?
            .clone();
        let evaluation = self.resolve_local_tab_preview(cause, &drag, scene, current, policy)?;
        if !matches!(evaluation.decision, PreviewDecision::Publish { .. }) {
            return self.cancel_local_tab_release(cause, session, interaction_events);
        }
        let release_decision = evaluation.decision.clone();
        let _ =
            self.apply_preview_evaluation(cause, owner, session, evaluation, interaction_events)?;
        let drag = self
            .interaction
            .take_drag_for_release(session, owner, PointerButton::Primary)
            .map_err(|source| EngineError::PointerInteractionInvariant {
                cause,
                detail: format!("local tab release could not consume drag: {source:?}"),
            })?;
        if drag.preview.as_ref().is_some_and(|published| {
            !published.painted()
                && Self::release_decision_matches_preview(published, &release_decision)
        }) {
            return self.defer_drag_release(
                cause,
                focus_causal,
                session,
                drag,
                release_decision,
                policy,
            );
        }
        self.finish_drag_release(
            cause,
            focus_causal,
            session,
            drag,
            release_decision,
            policy,
            events,
            interaction_events,
        )
    }

    fn cancel_local_tab_release(
        &mut self,
        cause: ReductionCause,
        session: crate::interaction::DragSessionId,
        interaction_events: &mut Vec<InteractionEvent>,
    ) -> Result<InteractionOutcome, EngineError> {
        let status = self.interaction.cancel_drag(session).map_err(|source| {
            EngineError::PointerInteractionInvariant {
                cause,
                detail: format!("local tab release cancellation failed: {source:?}"),
            }
        })?;
        let reason = InteractionCancelReason::LocalResponseCancelled;
        interaction_events.push(InteractionEvent::new_caused(
            cause,
            self.version,
            InteractionEventKind::Cancelled { status, reason },
        ));
        Ok(InteractionOutcome::Cancelled { status, reason })
    }

    fn cancel_local_tab_gesture(
        &mut self,
        input: InputSequence,
        owner: GestureOwner,
        source: TabGestureSource,
        interaction_events: &mut Vec<InteractionEvent>,
    ) -> InteractionOutcome {
        let Ok(session) = self.local_tab_session(owner, source) else {
            return InteractionOutcome::Rejected(InteractionRejection::NoActiveGesture);
        };
        match self.interaction.cancel_drag(session) {
            Ok(status) => {
                let reason = InteractionCancelReason::LocalResponseCancelled;
                interaction_events.push(InteractionEvent::new(
                    input,
                    self.version,
                    InteractionEventKind::Cancelled { status, reason },
                ));
                InteractionOutcome::Cancelled { status, reason }
            }
            Err(error) => InteractionOutcome::Rejected(error),
        }
    }

    fn local_tab_session(
        &self,
        owner: GestureOwner,
        source: TabGestureSource,
    ) -> Result<crate::interaction::DragSessionId, InteractionRejection> {
        let InteractionStatus::Dragging { session } = self.interaction.status() else {
            return Err(InteractionRejection::NoActiveGesture);
        };
        let drag = self.interaction.active_drag(session)?;
        if drag.owner != owner {
            return Err(InteractionRejection::SessionMismatch);
        }
        let GestureOwner::LocalResponse { surface } = owner else {
            return Err(InteractionRejection::SessionMismatch);
        };
        match (source, &drag.payload) {
            (TabGestureSource::Item(source), MovePayload::Item(item))
                if item.root() == source.root
                    && item.tabs() == source.tabs
                    && item.item() == source.item => {}
            (TabGestureSource::Group(source), MovePayload::Tabs(tabs))
                if tabs.root() == source.root && tabs.node() == source.tabs => {}
            (
                TabGestureSource::ContainedTitle { root, floating },
                MovePayload::Subtree(subtree),
            ) if subtree.root() == root
                && self.workspace.presentation_for_root(root)
                    == Some(crate::RootPresentationOwner::Contained { surface, floating }) => {}
            _ => return Err(InteractionRejection::SessionMismatch),
        }
        Ok(session)
    }

    fn resolve_local_tab_preview(
        &self,
        cause: ReductionCause,
        drag: &crate::interaction::ActiveDrag,
        scene: SurfaceSceneStamp,
        point: LogicalPoint,
        policy: &DockPolicySnapshot,
    ) -> Result<PreviewEvaluation, EngineError> {
        if !self.drag_source_presentation_allows_targeting(drag) {
            return Ok(PreviewEvaluation::without_affordance(
                PreviewDecision::Clear(PreviewResolutionStatus::Unavailable),
            ));
        }
        let candidate = match self.local_response_candidate(scene) {
            Ok(candidate) if candidate.stamp().surface() == drag.source_surface => candidate,
            Ok(_) | Err(_) => {
                return Ok(PreviewEvaluation::without_affordance(
                    PreviewDecision::Clear(PreviewResolutionStatus::Unavailable),
                ));
            }
        };
        let query = resolve_presented_drop(
            candidate.stamp(),
            candidate.plan(),
            drag.source_layout_facts.as_deref(),
            &self.workspace,
            self.version,
            self.presentation_authority
                .presentation_requirements
                .workspace_index(),
            policy,
            drag.session,
            drag.payload.clone(),
            drag.surface_background_offer,
            point,
        )
        .map_err(|source| EngineError::PointerInteractionInvariant {
            cause,
            detail: source.to_string(),
        })?;
        let (resolution, affordance) = query.into_parts();
        let decision = match resolution {
            DropResolution::Resolved(resolved) => {
                if resolved.session() != drag.session || resolved.source() != &drag.payload {
                    return Err(EngineError::PointerInteractionInvariant {
                        cause,
                        detail: "local tab resolver changed the frozen drag identity".to_owned(),
                    });
                }
                let target = resolved.target_id();
                PreviewDecision::Publish {
                    scene: resolved.scene_stamp(),
                    visual: PreviewVisual::Dock {
                        surface: candidate.stamp().surface(),
                        target,
                        rect: resolved.visual().rect(),
                    },
                    proof: Box::new(PreviewProof::Dock {
                        target,
                        command: resolved.into_command(),
                    }),
                }
            }
            DropResolution::KnownNone(_) => self
                .resolve_local_contained_move_preview(cause, drag, candidate, point, policy)?
                .unwrap_or(PreviewDecision::Clear(PreviewResolutionStatus::KnownNone)),
            DropResolution::Rejected(_) => {
                PreviewDecision::Clear(PreviewResolutionStatus::Rejected)
            }
            DropResolution::Unavailable(_) => {
                PreviewDecision::Clear(PreviewResolutionStatus::Unavailable)
            }
        };
        Ok(PreviewEvaluation::new(decision, affordance))
    }

    fn resolve_local_contained_move_preview(
        &self,
        cause: ReductionCause,
        drag: &crate::interaction::ActiveDrag,
        candidate: &crate::scene::SurfacePlanScene,
        point: LogicalPoint,
        policy: &DockPolicySnapshot,
    ) -> Result<Option<PreviewDecision>, EngineError> {
        let FrozenDragOrigin::Contained(origin) = &drag.origin else {
            return Ok(None);
        };
        let DragGestureAuthority::LocalReady {
            surface,
            coordinates,
        } = drag.presentation
        else {
            return Ok(Some(PreviewDecision::Clear(
                PreviewResolutionStatus::Unavailable,
            )));
        };
        if surface != drag.source_surface
            || surface != origin.surface
            || surface != candidate.stamp().surface()
            || coordinates != candidate.coordinate_capture()
        {
            return Ok(Some(PreviewDecision::Clear(
                PreviewResolutionStatus::Unavailable,
            )));
        }

        let requested =
            match translated_contained_rect(origin.source_rect, origin.initial_pointer, point) {
                Ok(requested) => requested,
                Err(()) => {
                    return Ok(Some(PreviewDecision::Clear(
                        PreviewResolutionStatus::Rejected,
                    )));
                }
            };
        let bounds = candidate.plan().bounds();
        let clamped = match clamp_contained_rect(surface, bounds, requested, origin.minimum_size) {
            Ok(clamped) => clamped,
            Err(_) => {
                return Ok(Some(PreviewDecision::Clear(
                    PreviewResolutionStatus::Rejected,
                )));
            }
        };
        let placement = ContainedPlacementProof::new(
            candidate.stamp(),
            surface,
            requested,
            origin.minimum_size,
            bounds,
            clamped,
        );
        let proposal = crate::intent::ContainedTearOffProposal::new(
            origin.root,
            origin.floating,
            placement,
            ContainedPosition::Front,
        );
        let Some(command) = self.contained_presentation_command(
            &drag.payload,
            proposal,
            drag.complete_root.clone(),
            Some(origin),
        ) else {
            return Ok(Some(PreviewDecision::Clear(
                PreviewResolutionStatus::Rejected,
            )));
        };
        if !self.valid_contained_command_structure(drag, proposal, &command)
            || self
                .stage_journal_workspace_command(cause, &command, policy)?
                .is_err()
        {
            return Ok(Some(PreviewDecision::Clear(
                PreviewResolutionStatus::Rejected,
            )));
        }

        Ok(Some(PreviewDecision::Publish {
            scene: placement.scene(),
            visual: PreviewVisual::Contained {
                surface,
                rect: proposal.rect(),
                fallback: false,
            },
            proof: Box::new(PreviewProof::Contained {
                command,
                proposal,
                fallback: false,
            }),
        }))
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn reduce_local_splitter_gesture(
        &mut self,
        input: InputSequence,
        cause: ReductionCause,
        expected: WorkspaceVersion,
        application_base: WorkspaceVersion,
        surface: SurfaceId,
        target: SplitterResizeTarget,
        phase: LocalSplitterGesturePhase,
        policy: &DockPolicySnapshot,
        events: &mut Vec<WorkspaceEvent>,
        interaction_events: &mut Vec<InteractionEvent>,
    ) -> Result<InputOutcome, EngineError> {
        if expected != application_base {
            return Ok(InputOutcome::StaleRejected {
                expected,
                accepted_base: application_base,
            });
        }

        let owner = GestureOwner::LocalResponse { surface };
        let outcome = match phase {
            LocalSplitterGesturePhase::Press {
                scene,
                initial,
                current,
            } => {
                if self.interaction.status() != InteractionStatus::Idle {
                    InteractionOutcome::Rejected(InteractionRejection::SessionMismatch)
                } else {
                    let candidate = match self.local_response_candidate(scene) {
                        Ok(candidate) if candidate.plan().surface() == surface => candidate,
                        Ok(_) => {
                            return Ok(self.local_response_rejection(
                                InteractionRejection::SplitterGestureSurfaceMismatch {
                                    expected: scene.surface(),
                                    actual: surface,
                                },
                            ));
                        }
                        Err(error) => return Ok(self.local_response_rejection(error)),
                    };
                    if !Self::coordinate_capture_matches_current(
                        candidate.coordinate_capture(),
                        self.viewport.viewport(surface),
                        self.viewport.surface_coordinate_authority(surface),
                    ) {
                        return Ok(self.local_response_rejection(
                            InteractionRejection::SplitterGestureCoordinateAuthorityUnavailable {
                                surface,
                            },
                        ));
                    }
                    let start = match self.prepare_splitter_resize(
                        candidate.plan(),
                        scene,
                        candidate.coordinate_capture(),
                        target,
                        owner,
                        None,
                        ResizeGestureAuthority::LocalReady,
                        initial,
                        policy,
                    ) {
                        Ok(start) => start,
                        Err(error) => return Ok(self.local_response_rejection(error)),
                    };
                    let (session, replaced) = self
                        .interaction
                        .begin_resize(self.version.epoch(), start)
                        .map_err(|source| EngineError::PointerInteractionInvariant {
                            cause,
                            detail: source.to_string(),
                        })?;
                    if replaced.is_some() {
                        return Err(EngineError::ReductionCauseInvariant {
                            detail: "local splitter press replaced an active gesture after idle validation",
                        });
                    }
                    if current != initial {
                        let update = self.update_resize_at_point(
                            cause, owner, session, target, current, policy,
                        )?;
                        if let InteractionOutcome::Rejected(rejection) = update {
                            let _ = self.interaction.cancel_resize(session);
                            return Ok(self.local_response_rejection(rejection));
                        }
                    }
                    InteractionOutcome::ResizeBegan { session, replaced }
                }
            }
            LocalSplitterGesturePhase::Move { current } => {
                let session = match self.local_resize_session(surface, target) {
                    Ok(session) => session,
                    Err(error) => return Ok(self.local_response_rejection(error)),
                };
                self.update_resize_at_point(cause, owner, session, target, current, policy)?
            }
            LocalSplitterGesturePhase::Release { current } => {
                let session = match self.local_resize_session(surface, target) {
                    Ok(session) => session,
                    Err(error) => return Ok(self.local_response_rejection(error)),
                };
                self.finish_resize_at_point(
                    cause,
                    owner,
                    session,
                    target,
                    current,
                    policy,
                    events,
                    interaction_events,
                )?
            }
            LocalSplitterGesturePhase::Cancel => {
                let session = match self.local_resize_session(surface, target) {
                    Ok(session) => session,
                    Err(error) => return Ok(self.local_response_rejection(error)),
                };
                self.cancel_resize(
                    input,
                    session,
                    InteractionCancelReason::LocalResponseCancelled,
                    interaction_events,
                )
            }
        };
        Ok(InputOutcome::InteractionProcessed {
            outcome,
            version: self.version,
        })
    }

    fn local_resize_session(
        &self,
        surface: SurfaceId,
        target: SplitterResizeTarget,
    ) -> Result<crate::interaction::ResizeSessionId, InteractionRejection> {
        let resize = self
            .interaction
            .active_resize_view()
            .ok_or(InteractionRejection::NoActiveGesture)?;
        if resize.local_response_surface() != Some(surface) || resize.target() != target {
            return Err(InteractionRejection::SessionMismatch);
        }
        Ok(resize.session())
    }

    pub(super) fn local_response_rejection(&self, outcome: InteractionRejection) -> InputOutcome {
        InputOutcome::InteractionProcessed {
            outcome: InteractionOutcome::Rejected(outcome),
            version: self.version,
        }
    }
}
