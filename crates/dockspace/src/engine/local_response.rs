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
