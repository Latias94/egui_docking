//! Contained floating pointer gestures and transform transactions.

use super::*;

impl DockEngine {
    pub(super) fn prepare_journal_contained_gesture(
        &self,
        presentation: &JournalSurfacePresentation,
        floating: crate::ids::FloatingPresentationId,
        kind: ContainedGestureKind,
        point: crate::geometry::LogicalPoint,
    ) -> Result<PreparedContainedGesture, InteractionRejection> {
        self.prepare_contained_gesture(
            presentation.surface(),
            presentation.scene(),
            presentation.plan(),
            presentation.coordinate_capture(),
            floating,
            kind,
            point,
            ContainedTransformGestureAuthority::Presented(Self::freeze_journal_presentation(
                presentation,
            )),
        )
    }

    pub(super) fn prepare_local_contained_gesture(
        &self,
        candidate: &crate::scene::SurfacePlanScene,
        floating: crate::ids::FloatingPresentationId,
        kind: ContainedGestureKind,
        point: crate::geometry::LogicalPoint,
    ) -> Result<PreparedContainedGesture, InteractionRejection> {
        let surface = candidate.stamp().surface();
        self.prepare_contained_gesture(
            surface,
            candidate.stamp(),
            candidate.plan(),
            candidate.coordinate_capture(),
            floating,
            kind,
            point,
            ContainedTransformGestureAuthority::LocalReady {
                surface,
                coordinates: candidate.coordinate_capture(),
            },
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn prepare_contained_gesture(
        &self,
        surface: SurfaceId,
        scene: SurfaceSceneStamp,
        plan: &PresentationPlan,
        coordinate_capture: SurfaceCoordinateCapture,
        floating: crate::ids::FloatingPresentationId,
        kind: ContainedGestureKind,
        point: crate::geometry::LogicalPoint,
        presentation: ContainedTransformGestureAuthority,
    ) -> Result<PreparedContainedGesture, InteractionRejection> {
        if scene.requirement().workspace_epoch() != self.version.epoch() {
            return Err(InteractionRejection::StaleScene);
        }
        if !plan.bounds().contains(point) {
            return Err(InteractionRejection::ContainedGesturePointerOutsideSurface { surface });
        }
        let contained = plan.contained_record(floating).ok_or(
            InteractionRejection::ContainedTransformPresentationUnavailable { surface, floating },
        )?;
        let hit = match kind {
            ContainedGestureKind::TitleDrag => contained.title_drag_hit().contains(point),
            ContainedGestureKind::Resize(direction) => contained
                .resize()
                .iter()
                .find(|record| record.direction() == direction)
                .is_some_and(|record| record.hit().contains(point)),
        };
        if !hit {
            return Err(InteractionRejection::ContainedGestureHitMismatch { floating, kind });
        }
        if let Some(occluding) = plan
            .drop_occlusions()
            .iter()
            .filter(|occlusion| {
                occlusion.layer() > contained.layer() && occlusion.region().contains(point)
            })
            .max_by_key(|occlusion| occlusion.layer())
        {
            return Err(InteractionRejection::ContainedGestureOccluded {
                floating,
                occluding: occluding.floating(),
            });
        }
        let root = contained.root();
        if self.workspace.presentation_for_root(root)
            != Some(crate::RootPresentationOwner::Contained { surface, floating })
        {
            return Err(
                InteractionRejection::ContainedTransformPresentationUnavailable {
                    surface,
                    floating,
                },
            );
        }
        let durable = self.workspace.contained_floating(floating).ok_or(
            InteractionRejection::ContainedTransformPresentationUnavailable { surface, floating },
        )?;
        if durable.root != root {
            return Err(
                InteractionRejection::ContainedTransformPresentationUnavailable {
                    surface,
                    floating,
                },
            );
        }
        let root_record = self.workspace.root(root).ok_or(
            InteractionRejection::ContainedTransformPresentationUnavailable { surface, floating },
        )?;
        let source = self
            .workspace
            .capture_node_source(root, root_record.node)
            .map_err(InteractionRejection::CommandRejected)?;
        let expected_roster = self
            .workspace
            .capture_contained_roster(surface)
            .map_err(InteractionRejection::CommandRejected)?;
        Ok(PreparedContainedGesture {
            surface,
            root,
            floating,
            button: PointerButton::Primary,
            initial_pointer: point,
            minimum_size: contained.minimum_size(),
            source_rect: durable.rect,
            source,
            expected_roster,
            kind,
            scene,
            surface_bounds: plan.bounds(),
            coordinate_capture,
            presentation,
            source_layout_facts: plan.layout_facts().cloned().map(std::sync::Arc::new),
        })
    }

    pub(super) fn activate_prepared_journal_contained_gesture(
        &mut self,
        cause: ReductionCause,
        owner: GestureOwner,
        capture_authority: Authority<PointerCaptureOwner>,
        threshold_origin: Option<JournalDragThresholdOrigin>,
        prepared: PreparedContainedGesture,
        policy: &DockPolicySnapshot,
        events: &mut Vec<WorkspaceEvent>,
    ) -> Result<InteractionOutcome, EngineError> {
        self.activate_prepared_contained_gesture(
            cause,
            owner,
            Some(capture_authority),
            threshold_origin,
            prepared,
            policy,
            events,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn activate_prepared_contained_gesture(
        &mut self,
        cause: ReductionCause,
        owner: GestureOwner,
        capture_authority: Option<Authority<PointerCaptureOwner>>,
        threshold_origin: Option<JournalDragThresholdOrigin>,
        prepared: PreparedContainedGesture,
        policy: &DockPolicySnapshot,
        events: &mut Vec<WorkspaceEvent>,
    ) -> Result<InteractionOutcome, EngineError> {
        let mut candidate_events = Vec::new();
        let mut activation_changed = false;
        // A local egui response already owns the current painted chrome. Do
        // not enqueue a second RaiseContained transaction here: changing the
        // roster during begin would invalidate the very scene which supplied
        // the response before the first local preview can be painted. The
        // journal/native path still raises through its retained transaction.
        if !matches!(owner, GestureOwner::LocalResponse { .. }) {
            let raise = WorkspaceCommand::RaiseContained {
                source: prepared.source.clone(),
                floating: prepared.floating,
                expected_roster: prepared.expected_roster.clone(),
            };
            let mut workspace = self.clone_workspace_candidate();
            match WorkspaceTransaction::from_commands([raise]).apply(&mut workspace, policy) {
                Ok(report) => {
                    if let Some(surface) =
                        self.first_workspace_publication_mismatch(&workspace, None, None)
                    {
                        return Ok(InteractionOutcome::Rejected(
                            InteractionRejection::CommandRejected(
                                CommandError::SurfaceLifecycleFrozen { surface },
                            ),
                        ));
                    }
                    let changed = report.changed();
                    let outcomes = report.into_outcomes();
                    let publication = match self.stage_workspace_publication(workspace, policy) {
                        Ok(publication) => publication,
                        Err(source) if source.is_expected_rejection() => {
                            return Ok(InteractionOutcome::Rejected(
                                InteractionRejection::CommandRejected(source),
                            ));
                        }
                        Err(source) => {
                            return Err(EngineError::PointerInteractionInvariant {
                                cause,
                                detail: source.to_string(),
                            });
                        }
                    };
                    self.publish_workspace(publication);
                    self.reconcile_viewport_focus_authority();
                    activation_changed = changed;
                    if changed {
                        self.advance_revision_caused(cause)?;
                        candidate_events.extend(outcomes.into_iter().map(|outcome| {
                            WorkspaceEvent::new_caused(
                                cause,
                                self.version,
                                WorkspaceEventKind::CommandCommitted(outcome),
                            )
                        }));
                    }
                }
                Err(TransactionError::Command { source, .. }) if source.is_expected_rejection() => {
                    let title_drag_policy_rejection =
                        matches!(prepared.kind, ContainedGestureKind::TitleDrag)
                            && matches!(&source, CommandError::Policy(_));
                    if !title_drag_policy_rejection {
                        return Ok(InteractionOutcome::Rejected(
                            InteractionRejection::CommandRejected(source),
                        ));
                    }
                }
                Err(source) => {
                    return Err(EngineError::PointerInteractionInvariant {
                        cause,
                        detail: source.to_string(),
                    });
                }
            }
        }

        let outcome = match prepared.kind {
            ContainedGestureKind::TitleDrag => {
                let source = self
                    .workspace
                    .capture_node_source(prepared.root, prepared.source.node())
                    .map_err(|source| EngineError::PointerInteractionInvariant {
                        cause,
                        detail: format!("staged contained title gesture lost its source: {source}"),
                    })?;
                let payload = MovePayload::Subtree(source);
                let drag_source = self.prepare_drag_source(&payload).map_err(|source| {
                    EngineError::PointerInteractionInvariant {
                        cause,
                        detail: format!(
                            "staged contained title gesture lost drag authority: {source:?}"
                        ),
                    }
                })?;
                let source_roster = self
                    .workspace
                    .capture_contained_roster(prepared.surface)
                    .map_err(|source| EngineError::PointerInteractionInvariant {
                        cause,
                        detail: format!(
                            "staged contained title gesture lost its source roster: {source}"
                        ),
                    })?;
                let origin = FrozenDragOrigin::Contained(FrozenContainedDragOrigin {
                    surface: prepared.surface,
                    root: prepared.root,
                    floating: prepared.floating,
                    source_rect: prepared.source_rect,
                    source_roster,
                    initial_pointer: prepared.initial_pointer,
                    minimum_size: prepared.minimum_size,
                });
                let continuation = if activation_changed {
                    prepared.presentation.presented().and_then(|presentation| {
                        self.scene_gesture_continuation_draft(
                            cause,
                            owner,
                            presentation,
                            SceneGestureContinuationSource::Drag {
                                payload: payload.clone(),
                                source_surface: drag_source.source_surface,
                                complete_root: drag_source.complete_root.clone(),
                                origin: origin.clone(),
                                coordinates: self
                                    .capture_surface_coordinates(drag_source.source_surface),
                            },
                        )
                    })
                } else {
                    None
                };
                let (session, replaced) = self
                    .interaction
                    .arm_drag(DragArmStart {
                        epoch: self.version.epoch(),
                        owner,
                        journal_capture_authority: capture_authority,
                        button: prepared.button,
                        payload,
                        source_surface: drag_source.source_surface,
                        initial_pointer: Some(prepared.initial_pointer),
                        journal_threshold_origin: threshold_origin,
                        complete_root: drag_source.complete_root,
                        partial_detachable: drag_source.partial_detachable,
                        origin,
                        source_validated_at: self.version,
                        presentation: prepared.presentation.into_drag(),
                        source_layout_facts: prepared.source_layout_facts,
                        journal_source_geometry: None,
                        continuation,
                    })
                    .map_err(|source| EngineError::PointerInteractionInvariant {
                        cause,
                        detail: source.to_string(),
                    })?;
                if replaced.is_some() {
                    return Err(EngineError::ReductionCauseInvariant {
                        detail: "journal contained title press replaced an active gesture after busy validation",
                    });
                }
                InteractionOutcome::DragArmed { session, replaced }
            }
            ContainedGestureKind::Resize(direction) => {
                let continuation = if activation_changed
                    && prepared.presentation.presented().is_some()
                {
                    let source = self
                        .workspace
                        .capture_node_source(prepared.root, prepared.source.node())
                        .map_err(|source| EngineError::PointerInteractionInvariant {
                            cause,
                            detail: format!("staged contained resize lost its source: {source}"),
                        })?;
                    let expected_roster = self
                        .workspace
                        .capture_contained_roster(prepared.surface)
                        .map_err(|source| EngineError::PointerInteractionInvariant {
                            cause,
                            detail: format!(
                                "staged contained resize lost its source roster: {source}"
                            ),
                        })?;
                    self.scene_gesture_continuation_draft(
                        cause,
                        owner,
                        prepared
                            .presentation
                            .presented()
                            .expect("checked contained transform has presented authority"),
                        SceneGestureContinuationSource::ContainedTransform {
                            source,
                            surface: prepared.surface,
                            root: prepared.root,
                            floating: prepared.floating,
                            source_rect: prepared.source_rect,
                            expected_roster,
                            coordinates: self.capture_surface_coordinates(prepared.surface),
                        },
                    )
                } else {
                    None
                };
                let (session, replaced) = self
                    .interaction
                    .begin_contained_transform(
                        self.version.epoch(),
                        ContainedTransformStart {
                            owner,
                            journal_capture_authority: capture_authority,
                            button: prepared.button,
                            surface: prepared.surface,
                            root: prepared.root,
                            floating: prepared.floating,
                            source_rect: prepared.source_rect,
                            initial_pointer: prepared.initial_pointer,
                            kind: ContainedTransformKind::Resize(contained_resize_edges(direction)),
                            minimum_size: prepared.minimum_size,
                            scene: prepared.scene,
                            surface_bounds: prepared.surface_bounds,
                            coordinate_capture: prepared.coordinate_capture,
                            presentation: prepared.presentation,
                            continuation,
                        },
                    )
                    .map_err(|source| EngineError::PointerInteractionInvariant {
                        cause,
                        detail: source.to_string(),
                    })?;
                if replaced.is_some() {
                    return Err(EngineError::ReductionCauseInvariant {
                        detail: "journal contained resize press replaced an active gesture after busy validation",
                    });
                }
                InteractionOutcome::ContainedTransformBegan { session, replaced }
            }
        };

        events.extend(candidate_events);
        Ok(outcome)
    }

    fn journal_contained_transform_point(
        &self,
        transform: &ActiveContainedTransform,
        edge: &PointerEdge,
    ) -> Result<crate::geometry::LogicalPoint, InteractionRejection> {
        if !Self::coordinate_capture_matches_current(
            transform.coordinate_capture,
            self.viewport.viewport(transform.surface),
            self.viewport
                .surface_coordinate_authority(transform.surface),
        ) {
            return Err(
                InteractionRejection::ContainedGestureCoordinateAuthorityUnavailable {
                    surface: transform.surface,
                },
            );
        }
        let stream = transform
            .owner
            .stream()
            .ok_or(InteractionRejection::SessionMismatch)?;
        match (stream.lease().scope(), edge.location()) {
            (
                PointerProviderScope::SurfaceLocal(scope),
                PointerEdgeLocation::SurfaceLocal {
                    position: Authority::Known(point),
                },
            ) if scope.surface() == transform.surface => Ok(point),
            (PointerProviderScope::DesktopGlobal, PointerEdgeLocation::Desktop { route }) => {
                let Authority::Known(position) = route.position() else {
                    return Err(
                        InteractionRejection::ContainedGestureCoordinateAuthorityUnavailable {
                            surface: transform.surface,
                        },
                    );
                };
                let SurfaceCoordinateCapture::NativeReady { coordinates, .. } =
                    transform.coordinate_capture
                else {
                    return Err(
                        InteractionRejection::ContainedGestureCoordinateAuthorityUnavailable {
                            surface: transform.surface,
                        },
                    );
                };
                coordinates.desktop_to_surface(position).map_err(|_| {
                    InteractionRejection::ContainedGestureCoordinateAuthorityUnavailable {
                        surface: transform.surface,
                    }
                })
            }
            _ => Err(
                InteractionRejection::ContainedGestureCoordinateAuthorityUnavailable {
                    surface: transform.surface,
                },
            ),
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn update_journal_contained_transform(
        &mut self,
        cause: ReductionCause,
        owner: GestureOwner,
        session: ContainedTransformSessionId,
        edge: &PointerEdge,
        interaction_events: &mut Vec<InteractionEvent>,
    ) -> Result<InteractionOutcome, EngineError> {
        let transform = self
            .interaction
            .active_contained_transform(session)
            .map_err(|source| EngineError::PointerInteractionInvariant {
                cause,
                detail: format!("{source:?}"),
            })?
            .clone();
        if transform.owner != owner {
            return Ok(InteractionOutcome::Rejected(
                InteractionRejection::SessionMismatch,
            ));
        }
        let point = match self.journal_contained_transform_point(&transform, edge) {
            Ok(point) => point,
            Err(rejection) => return Ok(InteractionOutcome::Rejected(rejection)),
        };
        match self.publish_contained_transform_preview_at_point(
            cause,
            owner,
            session,
            point,
            interaction_events,
        )? {
            Ok((_, preview)) => {
                Ok(InteractionOutcome::ContainedTransformPreviewUpdated { session, preview })
            }
            Err(rejection) => Ok(InteractionOutcome::Rejected(rejection)),
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn finish_journal_contained_transform_release(
        &mut self,
        cause: ReductionCause,
        owner: GestureOwner,
        session: ContainedTransformSessionId,
        edge: &PointerEdge,
        policy: &DockPolicySnapshot,
        events: &mut Vec<WorkspaceEvent>,
        interaction_events: &mut Vec<InteractionEvent>,
    ) -> Result<Vec<InteractionOutcome>, EngineError> {
        let transform = self
            .interaction
            .active_contained_transform(session)
            .map_err(|source| EngineError::PointerInteractionInvariant {
                cause,
                detail: format!("{source:?}"),
            })?
            .clone();
        if transform.owner != owner {
            return Ok(vec![InteractionOutcome::Rejected(
                InteractionRejection::SessionMismatch,
            )]);
        }
        let point = match self.journal_contained_transform_point(&transform, edge) {
            Ok(point) => point,
            Err(_) => {
                let reason = InteractionCancelReason::UnknownTargetAuthority;
                return Ok(self
                    .cancel_journal_owner(cause, owner, reason, interaction_events)
                    .into_iter()
                    .collect());
            }
        };
        let (placement, preview) = match self.publish_contained_transform_preview_at_point(
            cause,
            owner,
            session,
            point,
            interaction_events,
        )? {
            Ok(publication) => publication,
            Err(rejection) => {
                let _ = self.interaction.take_contained_transform_for_release(
                    session,
                    owner,
                    PointerButton::Primary,
                );
                return Ok(vec![InteractionOutcome::Rejected(rejection)]);
            }
        };
        let mut outcomes =
            vec![InteractionOutcome::ContainedTransformPreviewUpdated { session, preview }];
        let transform = match self.interaction.take_contained_transform_for_release(
            session,
            owner,
            PointerButton::Primary,
        ) {
            Ok(transform) => transform,
            Err(error) => {
                outcomes.push(InteractionOutcome::Rejected(error));
                return Ok(outcomes);
            }
        };
        let Some(painted) = transform.preview.as_ref() else {
            outcomes.push(InteractionOutcome::Rejected(
                InteractionRejection::PreviewMissing,
            ));
            return Ok(outcomes);
        };
        if painted.placement() != placement || painted.public() != &preview {
            outcomes.push(InteractionOutcome::Rejected(
                InteractionRejection::ContainedTransformChanged,
            ));
            return Ok(outcomes);
        }
        if !painted.painted() {
            outcomes.push(
                self.defer_contained_transform_release(
                    cause, session, transform, placement, policy,
                )?,
            );
            return Ok(outcomes);
        }
        outcomes.push(self.deliver_contained_transform_release(
            cause,
            session,
            transform,
            placement,
            policy,
            events,
            interaction_events,
        )?);
        Ok(outcomes)
    }

    pub(super) fn defer_contained_transform_release(
        &mut self,
        cause: ReductionCause,
        session: ContainedTransformSessionId,
        transform: ActiveContainedTransform,
        placement: ContainedTransformPlacement,
        policy: &DockPolicySnapshot,
    ) -> Result<InteractionOutcome, EngineError> {
        if self.pending_drag_release.is_some() || self.pending_contained_transform_release.is_some()
        {
            return Err(EngineError::PointerInteractionInvariant {
                cause,
                detail: "a contained-transform release obligation is already pending".to_owned(),
            });
        }
        let preview = transform
            .preview
            .as_ref()
            .expect("deferred contained release has an exact preview")
            .public()
            .token();
        self.pending_contained_transform_release = Some(PendingContainedTransformRelease {
            source_version: self.version,
            policy_revision: policy.revision(),
            cause,
            session,
            transform,
            placement,
            preview,
            presentation_outputs: BTreeSet::new(),
            presented_output: None,
            presentation_failed: false,
        });
        Ok(InteractionOutcome::ContainedTransformReleasePending { session, preview })
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn deliver_contained_transform_release(
        &mut self,
        cause: ReductionCause,
        session: ContainedTransformSessionId,
        transform: ActiveContainedTransform,
        placement: ContainedTransformPlacement,
        policy: &DockPolicySnapshot,
        events: &mut Vec<WorkspaceEvent>,
        interaction_events: &mut Vec<InteractionEvent>,
    ) -> Result<InteractionOutcome, EngineError> {
        let command = WorkspaceCommand::UpdateContainedRect {
            surface: transform.surface,
            root: transform.root,
            floating: transform.floating,
            expected_rect: transform.source_rect,
            rect: placement.rect(),
        };
        let (outcome, changed) =
            match self.apply_journal_workspace_command(cause, &command, policy, events)? {
                Ok(result) => result,
                Err(error) => {
                    return Ok(InteractionOutcome::Rejected(
                        InteractionRejection::CommandRejected(error),
                    ));
                }
            };
        interaction_events.push(InteractionEvent::new_caused(
            cause,
            self.version,
            InteractionEventKind::ContainedTransformDelivered { session },
        ));
        Ok(InteractionOutcome::ContainedTransformDelivered {
            session,
            outcome,
            changed,
        })
    }

    pub(super) fn publish_contained_transform_preview_at_point(
        &mut self,
        cause: ReductionCause,
        owner: GestureOwner,
        session: ContainedTransformSessionId,
        point: crate::geometry::LogicalPoint,
        interaction_events: &mut Vec<InteractionEvent>,
    ) -> Result<
        Result<(ContainedTransformPlacement, ContainedTransformPreview), InteractionRejection>,
        EngineError,
    > {
        let transform = match self.interaction.active_contained_transform(session) {
            Ok(transform) => transform.clone(),
            Err(error) => return Ok(Err(error)),
        };
        if transform.owner != owner {
            return Ok(Err(InteractionRejection::SessionMismatch));
        }
        let placement = match self.resolve_contained_transform_placement(&transform, point) {
            Ok(placement) => placement,
            Err(error) => return Ok(Err(error)),
        };
        self.interaction
            .set_contained_transform_pointer(session, point)
            .map_err(|source| EngineError::PointerInteractionInvariant {
                cause,
                detail: format!("{source:?}"),
            })?;
        let (preview, changed) = self
            .interaction
            .publish_contained_transform_preview(session, placement)
            .map_err(|source| EngineError::PointerInteractionInvariant {
                cause,
                detail: source.to_string(),
            })?;
        if changed {
            interaction_events.push(InteractionEvent::new_caused(
                cause,
                self.version,
                InteractionEventKind::ContainedTransformPreviewPublished { preview },
            ));
        }
        Ok(Ok((placement, preview)))
    }

    pub(super) fn resolve_contained_transform_placement(
        &self,
        transform: &ActiveContainedTransform,
        current_pointer: crate::geometry::LogicalPoint,
    ) -> Result<ContainedTransformPlacement, InteractionRejection> {
        if !Self::coordinate_capture_matches_current(
            transform.coordinate_capture,
            self.viewport.viewport(transform.surface),
            self.viewport
                .surface_coordinate_authority(transform.surface),
        ) || transform
            .presentation
            .local_coordinates()
            .is_some_and(|coordinates| coordinates != transform.coordinate_capture)
        {
            return Err(
                InteractionRejection::ContainedGestureCoordinateAuthorityUnavailable {
                    surface: transform.surface,
                },
            );
        }
        if transform.scene.requirement().workspace_epoch() != self.version.epoch() {
            return Err(InteractionRejection::StaleScene);
        }
        let Some(contained) = self.workspace.contained_floating(transform.floating) else {
            return Err(
                InteractionRejection::ContainedTransformPresentationUnavailable {
                    surface: transform.surface,
                    floating: transform.floating,
                },
            );
        };
        if contained.root != transform.root
            || self.workspace.presentation_for_root(transform.root)
                != Some(crate::RootPresentationOwner::Contained {
                    surface: transform.surface,
                    floating: transform.floating,
                })
        {
            return Err(
                InteractionRejection::ContainedTransformPresentationUnavailable {
                    surface: transform.surface,
                    floating: transform.floating,
                },
            );
        }
        let bounds = transform.surface_bounds;
        let requested = contained_transform_requested_rect(transform, current_pointer, bounds)
            .map_err(|()| InteractionRejection::ContainedTransformGeometryUnavailable)?;
        let rect = match transform.kind {
            ContainedTransformKind::Move => clamp_moved_contained_rect(bounds, requested)
                .map_err(|()| InteractionRejection::ContainedTransformGeometryUnavailable)?,
            ContainedTransformKind::Resize(_) => requested,
        };
        Ok(ContainedTransformPlacement::new(
            transform.scene,
            transform.surface,
            bounds,
            rect,
        ))
    }

    pub(super) fn settle_presented_pending_contained_transform_release(
        &mut self,
        events: &mut Vec<WorkspaceEvent>,
        interaction_events: &mut Vec<InteractionEvent>,
    ) -> Result<bool, EngineError> {
        let Some(pending) = self.pending_contained_transform_release.as_ref() else {
            return Ok(false);
        };
        if pending.presented_output.is_none() && !pending.presentation_failed {
            return Ok(false);
        }
        let pending = self
            .pending_contained_transform_release
            .take()
            .expect("checked contained release remains present");
        let cancellation = if self.version != pending.source_version {
            Some(InteractionCancelReason::WorkspaceChanged)
        } else if self.policy.revision() != pending.policy_revision {
            Some(InteractionCancelReason::PolicyChanged)
        } else if pending.presentation_failed {
            Some(InteractionCancelReason::SceneUnavailable)
        } else {
            None
        };
        if let Some(reason) = cancellation {
            interaction_events.push(InteractionEvent::new_caused(
                pending.cause,
                self.version,
                InteractionEventKind::Cancelled {
                    status: InteractionStatus::Idle,
                    reason,
                },
            ));
            return Ok(true);
        }

        let cause = pending.cause;
        let policy = self.policy.clone();
        let outcome = self.deliver_contained_transform_release(
            cause,
            pending.session,
            pending.transform,
            pending.placement,
            &policy,
            events,
            interaction_events,
        )?;
        if !matches!(
            outcome,
            InteractionOutcome::ContainedTransformDelivered { .. }
        ) {
            return Err(EngineError::PointerInteractionInvariant {
                cause,
                detail: format!(
                    "presented contained release did not deliver its frozen command: {outcome:?}"
                ),
            });
        }
        Ok(true)
    }
}
