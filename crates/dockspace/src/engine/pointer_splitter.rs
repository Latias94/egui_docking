//! Splitter and junction pointer-resize transactions.

use super::*;

impl DockEngine {
    pub(super) fn prepare_journal_splitter_gesture(
        &self,
        presentation: &JournalSurfacePresentation,
        region: PresentationHitRegionId,
        owner: GestureOwner,
        capture_authority: Authority<PointerCaptureOwner>,
        point: crate::geometry::LogicalPoint,
        policy: &DockPolicySnapshot,
    ) -> Result<ResizeStart, InteractionRejection> {
        let surface = presentation.surface();
        let target = match region.kind() {
            PresentationHitRegionKind::SplitterHandle(splitter) => {
                SplitterResizeTarget::Handle(splitter)
            }
            PresentationHitRegionKind::SplitterJunction(junction) => {
                SplitterResizeTarget::Junction(junction)
            }
            _ => {
                return Err(InteractionRejection::SplitterGestureHitUnavailable { surface });
            }
        };
        self.prepare_splitter_resize(
            presentation.plan(),
            presentation.scene(),
            presentation.coordinate_capture(),
            target,
            owner,
            Some(capture_authority),
            ResizeGestureAuthority::Presented(Self::freeze_journal_presentation(presentation)),
            point,
            policy,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn prepare_splitter_resize(
        &self,
        plan: &PresentationPlan,
        scene: SurfaceSceneStamp,
        coordinate_capture: SurfaceCoordinateCapture,
        target: SplitterResizeTarget,
        owner: GestureOwner,
        journal_capture_authority: Option<Authority<PointerCaptureOwner>>,
        authority: ResizeGestureAuthority,
        point: crate::geometry::LogicalPoint,
        policy: &DockPolicySnapshot,
    ) -> Result<ResizeStart, InteractionRejection> {
        let surface = plan.surface();
        if scene.requirement().workspace_epoch() != self.version.epoch() {
            return Err(InteractionRejection::StaleScene);
        }
        if !plan.bounds().contains(point) {
            return Err(InteractionRejection::SplitterGesturePointerOutsideSurface { surface });
        }
        let resolved = plan
            .splitter_resize_target_at(point)
            .map_err(InteractionRejection::SplitterGestureHitAmbiguous)?
            .ok_or(InteractionRejection::SplitterGestureHitUnavailable { surface })?;
        if resolved != target {
            return Err(InteractionRejection::SplitterGestureHitUnavailable { surface });
        }
        let handles = self.prepare_splitter_resize_handles(plan, target, point, policy)?;
        let axis_groups = prepare_resize_axis_groups(handles)
            .ok_or(InteractionRejection::SplitterResizeGeometryUnavailable)?;
        Ok(ResizeStart {
            owner,
            journal_capture_authority,
            button: PointerButton::Primary,
            surface,
            scene,
            target,
            coordinate_capture,
            authority,
            initial_pointer: point,
            axis_groups,
        })
    }

    fn prepare_splitter_resize_handles(
        &self,
        plan: &PresentationPlan,
        target: SplitterResizeTarget,
        point: crate::geometry::LogicalPoint,
        policy: &DockPolicySnapshot,
    ) -> Result<Vec<(NodeSource, crate::scene::SplitterRecord)>, InteractionRejection> {
        let surface = plan.surface();
        let (ids, require_direct_hit) = match target {
            SplitterResizeTarget::Handle(handle) => (vec![handle], true),
            SplitterResizeTarget::Junction(junction) => (junction.splitters(), false),
        };
        ids.into_iter()
            .map(|id| {
                let record = plan
                    .splitter_record(id)
                    .filter(|record| {
                        record.operable() && (!require_direct_hit || record.hit().contains(point))
                    })
                    .cloned()
                    .ok_or(InteractionRejection::SplitterGestureHitUnavailable { surface })?;
                let source = self
                    .workspace
                    .capture_node_source(id.root, id.split)
                    .map_err(InteractionRejection::ResizeRejected)?;
                self.check_resize_policy(&source, policy)
                    .map_err(InteractionRejection::ResizeRejected)?;
                Ok((source, record))
            })
            .collect()
    }

    pub(super) fn update_journal_resize(
        &mut self,
        cause: ReductionCause,
        owner: GestureOwner,
        session: crate::interaction::ResizeSessionId,
        edge: &PointerEdge,
        policy: &DockPolicySnapshot,
    ) -> Result<InteractionOutcome, EngineError> {
        let resize = self
            .interaction
            .active_resize(session)
            .map_err(|source| EngineError::PointerInteractionInvariant {
                cause,
                detail: format!("{source:?}"),
            })?
            .clone();
        if resize.owner != owner {
            return Ok(InteractionOutcome::Rejected(
                InteractionRejection::SessionMismatch,
            ));
        }
        let point = match self.journal_resize_point(&resize, edge) {
            Ok(point) => point,
            Err(rejection) => return Ok(InteractionOutcome::Rejected(rejection)),
        };
        self.update_resize_at_point(cause, owner, session, resize.target, point, policy)
    }

    pub(super) fn update_resize_at_point(
        &mut self,
        cause: ReductionCause,
        owner: GestureOwner,
        session: crate::interaction::ResizeSessionId,
        target: SplitterResizeTarget,
        point: crate::geometry::LogicalPoint,
        policy: &DockPolicySnapshot,
    ) -> Result<InteractionOutcome, EngineError> {
        let resize = self
            .interaction
            .active_resize(session)
            .map_err(|source| EngineError::PointerInteractionInvariant {
                cause,
                detail: format!("{source:?}"),
            })?
            .clone();
        if resize.owner != owner || resize.target != target {
            return Ok(InteractionOutcome::Rejected(
                InteractionRejection::SessionMismatch,
            ));
        }
        if let Err(rejection) = self.validate_resize_coordinate_authority(&resize) {
            return Ok(InteractionOutcome::Rejected(rejection));
        }
        let updates = match split_resize_updates(&resize, point) {
            Some(updates) => updates,
            None => {
                return Ok(InteractionOutcome::Rejected(
                    InteractionRejection::SplitterResizeGeometryUnavailable,
                ));
            }
        };
        if let Err(error) =
            crate::operation::validate_split_resizes(&self.workspace, policy, &updates)
        {
            return Ok(InteractionOutcome::Rejected(
                InteractionRejection::ResizeRejected(error),
            ));
        }
        self.interaction
            .set_resize_updates(session, point, updates.clone())
            .map_err(|source| EngineError::PointerInteractionInvariant {
                cause,
                detail: format!("{source:?}"),
            })?;
        Ok(InteractionOutcome::ResizeUpdated {
            session,
            splits: updates,
        })
    }

    pub(super) fn finish_journal_resize_release(
        &mut self,
        cause: ReductionCause,
        owner: GestureOwner,
        session: crate::interaction::ResizeSessionId,
        edge: &PointerEdge,
        policy: &DockPolicySnapshot,
        events: &mut Vec<WorkspaceEvent>,
        interaction_events: &mut Vec<InteractionEvent>,
    ) -> Result<InteractionOutcome, EngineError> {
        let resize = self
            .interaction
            .active_resize(session)
            .map_err(|source| EngineError::PointerInteractionInvariant {
                cause,
                detail: format!("{source:?}"),
            })?
            .clone();
        if resize.owner != owner {
            return Ok(InteractionOutcome::Rejected(
                InteractionRejection::SessionMismatch,
            ));
        }
        let point = match self.journal_resize_point(&resize, edge) {
            Ok(point) => point,
            Err(rejection) => return Ok(InteractionOutcome::Rejected(rejection)),
        };
        self.finish_resize_at_point(
            cause,
            owner,
            session,
            resize.target,
            point,
            policy,
            events,
            interaction_events,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn finish_resize_at_point(
        &mut self,
        cause: ReductionCause,
        owner: GestureOwner,
        session: crate::interaction::ResizeSessionId,
        target: SplitterResizeTarget,
        point: crate::geometry::LogicalPoint,
        policy: &DockPolicySnapshot,
        events: &mut Vec<WorkspaceEvent>,
        interaction_events: &mut Vec<InteractionEvent>,
    ) -> Result<InteractionOutcome, EngineError> {
        let resize = self
            .interaction
            .active_resize(session)
            .map_err(|source| EngineError::PointerInteractionInvariant {
                cause,
                detail: format!("{source:?}"),
            })?
            .clone();
        if resize.owner != owner || resize.target != target {
            return Ok(InteractionOutcome::Rejected(
                InteractionRejection::SessionMismatch,
            ));
        }
        let proposal = self.resize_updates_at_point(&resize, point, policy);
        self.interaction
            .take_resize_for_release(session, owner, PointerButton::Primary)
            .map_err(|source| EngineError::PointerInteractionInvariant {
                cause,
                detail: format!("{source:?}"),
            })?;
        let updates = match proposal {
            Ok(updates) => updates,
            Err(rejection) => return Ok(InteractionOutcome::Rejected(rejection)),
        };
        let command = WorkspaceCommand::ResizeSplits { splits: updates };
        let (outcome, changed) =
            match self.apply_journal_workspace_command(cause, &command, policy, events)? {
                Ok(result) => result,
                Err(error) => {
                    return Ok(InteractionOutcome::Rejected(
                        InteractionRejection::ResizeRejected(error),
                    ));
                }
            };
        interaction_events.push(InteractionEvent::new_caused(
            cause,
            self.version,
            InteractionEventKind::ResizeDelivered { session },
        ));
        Ok(InteractionOutcome::ResizeDelivered {
            session,
            outcome,
            changed,
        })
    }

    fn resize_updates_at_point(
        &self,
        resize: &ActiveResize,
        point: crate::geometry::LogicalPoint,
        policy: &DockPolicySnapshot,
    ) -> Result<Vec<crate::command::SplitResize>, InteractionRejection> {
        self.validate_resize_coordinate_authority(resize)?;
        let updates = split_resize_updates(resize, point)
            .ok_or(InteractionRejection::SplitterResizeGeometryUnavailable)?;
        crate::operation::validate_split_resizes(&self.workspace, policy, &updates)
            .map_err(InteractionRejection::ResizeRejected)?;
        Ok(updates)
    }

    fn validate_resize_coordinate_authority(
        &self,
        resize: &ActiveResize,
    ) -> Result<(), InteractionRejection> {
        if Self::coordinate_capture_matches_current(
            resize.coordinate_capture,
            self.viewport.viewport(resize.surface),
            self.viewport.surface_coordinate_authority(resize.surface),
        ) {
            Ok(())
        } else {
            Err(
                InteractionRejection::SplitterGestureCoordinateAuthorityUnavailable {
                    surface: resize.surface,
                },
            )
        }
    }

    fn journal_resize_point(
        &self,
        resize: &ActiveResize,
        edge: &PointerEdge,
    ) -> Result<crate::geometry::LogicalPoint, InteractionRejection> {
        self.validate_resize_coordinate_authority(resize)?;
        let stream = resize
            .owner
            .stream()
            .ok_or(InteractionRejection::SessionMismatch)?;
        match (stream.lease().scope(), edge.location()) {
            (
                PointerProviderScope::SurfaceLocal(scope),
                PointerEdgeLocation::SurfaceLocal {
                    position: Authority::Known(point),
                },
            ) => {
                if scope.surface() != resize.surface {
                    return Err(InteractionRejection::SplitterGestureSurfaceMismatch {
                        expected: resize.surface,
                        actual: scope.surface(),
                    });
                }
                Ok(point)
            }
            (PointerProviderScope::DesktopGlobal, PointerEdgeLocation::Desktop { route }) => {
                let Authority::Known(position) = route.position() else {
                    return Err(
                        InteractionRejection::SplitterGestureCoordinateAuthorityUnavailable {
                            surface: resize.surface,
                        },
                    );
                };
                let SurfaceCoordinateCapture::NativeReady { coordinates, .. } =
                    resize.coordinate_capture
                else {
                    return Err(
                        InteractionRejection::SplitterGestureCoordinateAuthorityUnavailable {
                            surface: resize.surface,
                        },
                    );
                };
                coordinates.desktop_to_surface(position).map_err(|_| {
                    InteractionRejection::SplitterGestureCoordinateAuthorityUnavailable {
                        surface: resize.surface,
                    }
                })
            }
            (
                PointerProviderScope::SurfaceLocal(scope),
                PointerEdgeLocation::SurfaceLocal {
                    position: Authority::Unknown(_),
                },
            ) if scope.surface() == resize.surface => Err(
                InteractionRejection::SplitterGestureCoordinateAuthorityUnavailable {
                    surface: resize.surface,
                },
            ),
            _ => Err(
                InteractionRejection::SplitterGestureCoordinateAuthorityUnavailable {
                    surface: resize.surface,
                },
            ),
        }
    }
}
