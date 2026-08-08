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
    pub(super) fn reduce_local_tab_gesture(
        &mut self,
        input: InputSequence,
        cause: ReductionCause,
        focus_causal: FocusCausalStamp,
        expected: WorkspaceVersion,
        application_base: WorkspaceVersion,
        surface: SurfaceId,
        source: crate::scene::TabSceneId,
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
            LocalTabGesturePhase::Move { current } => self.update_local_tab_gesture(
                cause,
                owner,
                source,
                current,
                policy,
                interaction_events,
            )?,
            LocalTabGesturePhase::Release { current } => self.release_local_tab_gesture(
                cause,
                focus_causal,
                owner,
                source,
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
        source: crate::scene::TabSceneId,
        scene: SurfaceSceneStamp,
        initial: LogicalPoint,
        _current: LogicalPoint,
        policy: &DockPolicySnapshot,
        events: &mut Vec<WorkspaceEvent>,
        _interaction_events: &mut Vec<InteractionEvent>,
    ) -> Result<InteractionOutcome, EngineError> {
        if self.interaction.status() != InteractionStatus::Idle {
            return Ok(InteractionOutcome::Rejected(
                InteractionRejection::SessionMismatch,
            ));
        }
        let prepared = match self.local_response_candidate(scene).and_then(|candidate| {
            self.prepare_local_tab_gesture(candidate, TabGestureSource::Item(source), initial)
        }) {
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
        Ok(InteractionOutcome::DragBegan { session })
    }

    fn update_local_tab_gesture(
        &mut self,
        cause: ReductionCause,
        owner: GestureOwner,
        source: crate::scene::TabSceneId,
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
        let evaluation = self.resolve_local_tab_preview(cause, &drag, current, policy)?;
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
        source: crate::scene::TabSceneId,
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
        let evaluation = self.resolve_local_tab_preview(cause, &drag, current, policy)?;
        let PreviewDecision::Publish { proof, .. } = evaluation.decision else {
            return self.cancel_local_tab_release(cause, session, interaction_events);
        };
        let PreviewProof::Dock { target, command } = *proof else {
            return self.cancel_local_tab_release(cause, session, interaction_events);
        };
        let pane_focus = match self.freeze_payload_focus(&drag.payload) {
            Ok(focus) => focus,
            Err(error) => {
                let _ = self.cancel_local_tab_release(cause, session, interaction_events)?;
                return Ok(InteractionOutcome::Rejected(
                    InteractionRejection::CommandRejected(error),
                ));
            }
        };
        let released_status = self.interaction.status();
        let _ = self
            .interaction
            .take_drag_for_release(session, owner, PointerButton::Primary)
            .map_err(|source| EngineError::PointerInteractionInvariant {
                cause,
                detail: format!("local tab release could not consume drag: {source:?}"),
            })?;
        let (outcome, changed) =
            match self.apply_journal_workspace_command(cause, &command, policy, events)? {
                Ok(result) => result,
                Err(error) => {
                    let reason = InteractionCancelReason::LocalResponseCancelled;
                    interaction_events.push(InteractionEvent::new_caused(
                        cause,
                        self.version,
                        InteractionEventKind::Cancelled {
                            status: released_status,
                            reason,
                        },
                    ));
                    return Ok(InteractionOutcome::Rejected(
                        InteractionRejection::CommandRejected(error),
                    ));
                }
            };
        if let Some(binding) = self
            .viewport
            .viewport(target.surface())
            .filter(|record| record.can_accept_activation())
            .map(crate::viewport_registry::ViewportRecord::binding)
        {
            let _ = self.start_viewport_activation(
                ViewportActivationRequest::drop_committed(binding, pane_focus),
                focus_causal,
                events,
            )?;
        }
        interaction_events.push(InteractionEvent::new_caused(
            cause,
            self.version,
            InteractionEventKind::Delivered {
                session,
                kind: WorkspaceDeliveryKind::Dock,
            },
        ));
        Ok(InteractionOutcome::DragDelivered {
            session,
            delivery: InteractionDelivery::Workspace {
                kind: WorkspaceDeliveryKind::Dock,
                outcome,
                changed,
            },
        })
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
        source: crate::scene::TabSceneId,
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
        source: crate::scene::TabSceneId,
    ) -> Result<crate::interaction::DragSessionId, InteractionRejection> {
        let InteractionStatus::Dragging { session } = self.interaction.status() else {
            return Err(InteractionRejection::NoActiveGesture);
        };
        let drag = self.interaction.active_drag(session)?;
        if drag.owner != owner {
            return Err(InteractionRejection::SessionMismatch);
        }
        let MovePayload::Item(item) = &drag.payload else {
            return Err(InteractionRejection::SessionMismatch);
        };
        if item.root() != source.root || item.tabs() != source.tabs || item.item() != source.item {
            return Err(InteractionRejection::SessionMismatch);
        }
        Ok(session)
    }

    fn resolve_local_tab_preview(
        &self,
        cause: ReductionCause,
        drag: &crate::interaction::ActiveDrag,
        point: LogicalPoint,
        policy: &DockPolicySnapshot,
    ) -> Result<PreviewEvaluation, EngineError> {
        if !self.drag_source_presentation_allows_targeting(drag) {
            return Ok(PreviewEvaluation::without_affordance(
                PreviewDecision::Clear(PreviewResolutionStatus::Unavailable),
            ));
        }
        let Some(candidate) = self
            .presentation_authority
            .scene
            .surface(drag.source_surface)
            .and_then(SurfaceScene::ready)
            .map(crate::scene::ReadySurfaceScene::candidate)
            .filter(|candidate| {
                candidate.stamp().requirement().workspace_epoch() == self.version.epoch()
            })
        else {
            return Ok(PreviewEvaluation::without_affordance(
                PreviewDecision::Clear(PreviewResolutionStatus::Unavailable),
            ));
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
            DropResolution::KnownNone(_) => {
                PreviewDecision::Clear(PreviewResolutionStatus::KnownNone)
            }
            DropResolution::Rejected(_) => {
                PreviewDecision::Clear(PreviewResolutionStatus::Rejected)
            }
            DropResolution::Unavailable(_) => {
                PreviewDecision::Clear(PreviewResolutionStatus::Unavailable)
            }
        };
        Ok(PreviewEvaluation::new(decision, affordance))
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

    fn local_response_rejection(&self, outcome: InteractionRejection) -> InputOutcome {
        InputOutcome::InteractionProcessed {
            outcome: InteractionOutcome::Rejected(outcome),
            version: self.version,
        }
    }
}
