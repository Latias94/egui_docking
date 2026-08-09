//! Current-frame contained-floating transforms over an exact Ready candidate.

use super::*;

impl DockEngine {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn reduce_local_contained_gesture(
        &mut self,
        input: InputSequence,
        cause: ReductionCause,
        expected: WorkspaceVersion,
        application_base: WorkspaceVersion,
        surface: SurfaceId,
        floating: FloatingPresentationId,
        kind: ContainedGestureKind,
        phase: LocalContainedGesturePhase,
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
        if matches!(kind, ContainedGestureKind::TitleDrag) {
            return Ok(self.local_response_rejection(
                InteractionRejection::ContainedGestureHitMismatch { floating, kind },
            ));
        }

        let owner = GestureOwner::LocalResponse { surface };
        let outcome = match phase {
            LocalContainedGesturePhase::Begin {
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
                                InteractionRejection::ContainedTransformPresentationUnavailable {
                                    surface,
                                    floating,
                                },
                            ));
                        }
                        Err(error) => return Ok(self.local_response_rejection(error)),
                    };
                    let prepared = match self
                        .prepare_local_contained_gesture(candidate, floating, kind, initial)
                    {
                        Ok(prepared) => prepared,
                        Err(error) => return Ok(self.local_response_rejection(error)),
                    };
                    let began = self.activate_prepared_contained_gesture(
                        cause, owner, None, None, prepared, policy, events,
                    )?;
                    let InteractionOutcome::ContainedTransformBegan { session, .. } = began else {
                        return Ok(InputOutcome::InteractionProcessed {
                            outcome: began,
                            version: self.version,
                        });
                    };
                    if current != initial {
                        let update = self.update_local_contained_transform_at_point(
                            cause,
                            owner,
                            session,
                            floating,
                            kind,
                            scene,
                            current,
                            interaction_events,
                        )?;
                        if let InteractionOutcome::Rejected(rejection) = update {
                            let _ = self.cancel_contained_transform(
                                input,
                                session,
                                InteractionCancelReason::LocalResponseCancelled,
                                interaction_events,
                            );
                            return Ok(self.local_response_rejection(rejection));
                        }
                    }
                    began
                }
            }
            LocalContainedGesturePhase::Move { scene, current } => {
                let session =
                    match self.local_contained_transform_session(surface, floating, kind, scene) {
                        Ok(session) => session,
                        Err(error) => return Ok(self.local_response_rejection(error)),
                    };
                self.update_local_contained_transform_at_point(
                    cause,
                    owner,
                    session,
                    floating,
                    kind,
                    scene,
                    current,
                    interaction_events,
                )?
            }
            LocalContainedGesturePhase::Release { scene, current } => {
                let session =
                    match self.local_contained_transform_session(surface, floating, kind, scene) {
                        Ok(session) => session,
                        Err(error) => return Ok(self.local_response_rejection(error)),
                    };
                self.finish_local_contained_transform_at_point(
                    cause,
                    owner,
                    session,
                    scene,
                    current,
                    policy,
                    events,
                    interaction_events,
                )?
            }
            LocalContainedGesturePhase::Cancel => {
                let session = match self
                    .local_contained_transform_session_without_scene(surface, floating, kind)
                {
                    Ok(session) => session,
                    Err(error) => return Ok(self.local_response_rejection(error)),
                };
                self.cancel_contained_transform(
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

    fn update_local_contained_transform_at_point(
        &mut self,
        cause: ReductionCause,
        owner: GestureOwner,
        session: ContainedTransformSessionId,
        floating: FloatingPresentationId,
        kind: ContainedGestureKind,
        scene: SurfaceSceneStamp,
        current: LogicalPoint,
        interaction_events: &mut Vec<InteractionEvent>,
    ) -> Result<InteractionOutcome, EngineError> {
        let GestureOwner::LocalResponse { surface } = owner else {
            return Ok(InteractionOutcome::Rejected(
                InteractionRejection::SessionMismatch,
            ));
        };
        if self.local_contained_transform_session(surface, floating, kind, scene) != Ok(session) {
            return Ok(InteractionOutcome::Rejected(
                InteractionRejection::SessionMismatch,
            ));
        }
        match self.publish_contained_transform_preview_at_point(
            cause,
            owner,
            session,
            current,
            interaction_events,
        )? {
            Ok((_, preview)) => {
                Ok(InteractionOutcome::ContainedTransformPreviewUpdated { session, preview })
            }
            Err(rejection) => Ok(InteractionOutcome::Rejected(rejection)),
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn finish_local_contained_transform_at_point(
        &mut self,
        cause: ReductionCause,
        owner: GestureOwner,
        session: ContainedTransformSessionId,
        scene: SurfaceSceneStamp,
        current: LogicalPoint,
        policy: &DockPolicySnapshot,
        events: &mut Vec<WorkspaceEvent>,
        interaction_events: &mut Vec<InteractionEvent>,
    ) -> Result<InteractionOutcome, EngineError> {
        let placement = match self
            .resolve_local_contained_transform_placement(owner, session, scene, current)
        {
            Ok(placement) => placement,
            Err(rejection) => {
                let _ = self.interaction.take_contained_transform_for_release(
                    session,
                    owner,
                    PointerButton::Primary,
                );
                return Ok(InteractionOutcome::Rejected(rejection));
            }
        };
        let transform = self
            .interaction
            .take_contained_transform_for_release(session, owner, PointerButton::Primary)
            .map_err(|source| EngineError::PointerInteractionInvariant {
                cause,
                detail: format!("local contained release could not consume transform: {source:?}"),
            })?;
        let Some(painted) = transform.preview.as_ref() else {
            return Ok(InteractionOutcome::Rejected(
                InteractionRejection::PreviewMissing,
            ));
        };
        if painted.placement() != placement {
            return Ok(InteractionOutcome::Rejected(
                InteractionRejection::ContainedTransformChanged,
            ));
        }
        if !painted.painted() {
            return Ok(InteractionOutcome::Rejected(
                InteractionRejection::PreviewNotPainted,
            ));
        }
        self.deliver_contained_transform_release(
            cause,
            session,
            transform,
            placement,
            policy,
            events,
            interaction_events,
        )
    }

    fn local_contained_transform_session(
        &self,
        surface: SurfaceId,
        floating: FloatingPresentationId,
        kind: ContainedGestureKind,
        scene: SurfaceSceneStamp,
    ) -> Result<ContainedTransformSessionId, InteractionRejection> {
        let session =
            self.local_contained_transform_session_without_scene(surface, floating, kind)?;
        self.validate_local_contained_scene(scene, session)?;
        Ok(session)
    }

    fn local_contained_transform_session_without_scene(
        &self,
        surface: SurfaceId,
        floating: FloatingPresentationId,
        kind: ContainedGestureKind,
    ) -> Result<ContainedTransformSessionId, InteractionRejection> {
        let expected_kind = match kind {
            ContainedGestureKind::Resize(direction) => {
                ContainedTransformKind::Resize(contained_resize_edges(direction))
            }
            ContainedGestureKind::TitleDrag => {
                return Err(InteractionRejection::SessionMismatch);
            }
        };
        let transform = self
            .interaction
            .active_contained_transform_view()
            .ok_or(InteractionRejection::NoActiveGesture)?;
        if transform.journal_stream().is_some()
            || transform.surface() != surface
            || transform.floating() != floating
            || transform.kind() != expected_kind
        {
            return Err(InteractionRejection::SessionMismatch);
        }
        Ok(transform.session())
    }

    fn validate_local_contained_scene(
        &self,
        scene: SurfaceSceneStamp,
        session: ContainedTransformSessionId,
    ) -> Result<(), InteractionRejection> {
        let transform = self
            .interaction
            .active_contained_transform(session)
            .map_err(|_| InteractionRejection::NoActiveGesture)?;
        let candidate = self.local_response_candidate(scene)?;
        if candidate.stamp().surface() != transform.surface
            || candidate.coordinate_capture() != transform.coordinate_capture
            || candidate.plan().bounds() != transform.surface_bounds
            || candidate
                .plan()
                .contained_record(transform.floating)
                .is_none_or(|record| {
                    record.root() != transform.root
                        || record.minimum_size() != transform.minimum_size
                })
        {
            return Err(InteractionRejection::StaleScene);
        }
        Ok(())
    }

    fn resolve_local_contained_transform_placement(
        &self,
        owner: GestureOwner,
        session: ContainedTransformSessionId,
        scene: SurfaceSceneStamp,
        current: LogicalPoint,
    ) -> Result<ContainedTransformPlacement, InteractionRejection> {
        let transform = self
            .interaction
            .active_contained_transform(session)
            .map_err(|_| InteractionRejection::NoActiveGesture)?
            .clone();
        if transform.owner != owner {
            return Err(InteractionRejection::SessionMismatch);
        }
        self.validate_local_contained_scene(scene, session)?;
        let requested =
            contained_transform_requested_rect(&transform, current, transform.surface_bounds)
                .map_err(|()| InteractionRejection::ContainedTransformGeometryUnavailable)?;
        let rect = match transform.kind {
            ContainedTransformKind::Move => {
                clamp_moved_contained_rect(transform.surface_bounds, requested)
                    .map_err(|()| InteractionRejection::ContainedTransformGeometryUnavailable)?
            }
            ContainedTransformKind::Resize(_) => requested,
        };
        Ok(ContainedTransformPlacement::new(
            scene,
            transform.surface,
            transform.surface_bounds,
            rect,
        ))
    }
}
