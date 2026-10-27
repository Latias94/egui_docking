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
            Self::freeze_journal_presentation(presentation),
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
        presentation: FrozenPresentationAuthority,
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

    #[allow(clippy::too_many_arguments)]
    pub(super) fn activate_prepared_journal_contained_gesture(
        &mut self,
        cause: ReductionCause,
        owner: GestureOwner,
        capture_authority: Authority<PointerCaptureOwner>,
        threshold_origin: JournalDragThresholdOrigin,
        prepared: PreparedContainedGesture,
        policy: &DockPolicySnapshot,
        events: &mut Vec<WorkspaceEvent>,
    ) -> Result<InteractionOutcome, EngineError> {
        if let Some(rejection) = self.pending_presentation_transition_gesture_rejection() {
            return Ok(InteractionOutcome::Rejected(rejection));
        }
        let mut activation_changed = false;
        let needs_raise = prepared.expected_roster.contained().last() != Some(&prepared.floating);
        if needs_raise {
            let raise = WorkspaceCommand::RaiseContained {
                source: prepared.source.clone(),
                floating: prepared.floating,
                expected_roster: prepared.expected_roster.clone(),
            };
            match self.apply_journal_workspace_command(cause, &raise, policy, events)? {
                Ok((_, changed)) => activation_changed = changed,
                Err(source) => {
                    return Ok(InteractionOutcome::Rejected(
                        InteractionRejection::CommandRejected(source),
                    ));
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
                    intent: ContainedDragIntent::MoveWindow,
                    source_rect: prepared.source_rect,
                    source_roster,
                    initial_pointer: prepared.initial_pointer,
                    minimum_size: prepared.minimum_size,
                });
                let continuation = if activation_changed {
                    self.scene_gesture_continuation_draft(
                        cause,
                        owner,
                        prepared.presentation,
                        SceneGestureContinuationSource::Drag {
                            payload: payload.clone(),
                            source_surface: drag_source.source_surface,
                            complete_root: drag_source.complete_root.clone(),
                            origin: origin.clone(),
                            coordinates: self
                                .capture_surface_coordinates(drag_source.source_surface),
                        },
                    )
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
                        presentation: prepared.presentation,
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
                let continuation = if activation_changed {
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
                        prepared.presentation,
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
        let stream = transform.owner;
        match (stream.lease().scope(), edge.location()) {
            (PointerProviderScope::HostLocal(_), PointerEdgeLocation::HostLocal { .. }) => self
                .journal_host_local_manipulation(edge)
                .filter(|route| route.surface() == transform.surface)
                .map(|route| route.position())
                .ok_or(
                    InteractionRejection::ContainedGestureCoordinateAuthorityUnavailable {
                        surface: transform.surface,
                    },
                ),
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
                    .cancel_gesture_owner(cause, owner, reason, interaction_events)?
                    .into_iter()
                    .collect());
            }
        };
        self.finish_contained_transform_at_point(
            cause,
            owner,
            session,
            point,
            policy,
            events,
            interaction_events,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn finish_contained_transform_at_point(
        &mut self,
        cause: ReductionCause,
        owner: GestureOwner,
        session: ContainedTransformSessionId,
        point: crate::geometry::LogicalPoint,
        policy: &DockPolicySnapshot,
        events: &mut Vec<WorkspaceEvent>,
        interaction_events: &mut Vec<InteractionEvent>,
    ) -> Result<Vec<InteractionOutcome>, EngineError> {
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
        if self.pending_drag_release.is_some()
            || self.pending_contained_transform_release.is_some()
            || self.pending_presentation_rehome.is_some()
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
        ) {
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

#[cfg(test)]
mod tests {
    use super::super::test_support::{
        TestPresentationHost, complete_host_frame_with_retained_or_unavailable, publish_surface,
    };
    use super::*;
    use crate::graph::{ContainedFloating, RootRecord, SurfacePresentation};
    use crate::pointer_journal::HostLocalPointerRoute;
    use crate::pointer_receiver::{
        PointerReceiverDelivery, PointerReceiverDeliveryDisposition, PointerReceiverObservation,
        PointerReceiverProbeReceipt, PointerReceiverReceiptBatch,
        PresentedPointerReceiverObservation,
    };
    use crate::scene::ContainedResizeDirection;
    #[test]
    fn host_local_rear_contained_resize_keeps_exact_source_continuation_without_hover_authority() {
        let surface = SurfaceId::new(1);
        let rear = FloatingPresentationId::new(1);
        let front = FloatingPresentationId::new(2);
        let rect = |x, y, width, height| {
            crate::geometry::LogicalRect::new(x, y, width, height).expect("finite test bounds")
        };
        let initial_rect = rect(40.0, 40.0, 220.0, 180.0);
        let mut builder = Workspace::builder();
        for (root, item) in [
            (RootId::new(1), ItemId::new(1)),
            (RootId::new(2), ItemId::new(2)),
            (RootId::new(3), ItemId::new(3)),
        ] {
            let tabs = builder.insert_node(Node::tabs([item]));
            builder.set_root(root, RootRecord::new(tabs));
        }
        builder.set_surface(surface, SurfacePresentation::with_main(RootId::new(1)));
        for (floating, root, bounds) in [
            (rear, RootId::new(2), initial_rect),
            (front, RootId::new(3), rect(400.0, 300.0, 220.0, 180.0)),
        ] {
            builder.set_contained_floating(floating, ContainedFloating::new(root, bounds));
            builder
                .attach_contained(surface, floating)
                .expect("contained root attaches");
        }
        let mut engine = DockEngine::new(
            builder.build().expect("valid test workspace"),
            DockPolicy::default(),
        )
        .expect("valid test engine");
        let mut host = TestPresentationHost::new(&mut engine);
        publish_surface(
            &mut engine,
            &mut host,
            surface,
            rect(0.0, 0.0, 800.0, 600.0),
        );
        let projection = engine
            .interaction_projection(surface)
            .expect("presented source");
        let authority = projection.authority();
        let region = projection
            .hit_manifest()
            .regions()
            .iter()
            .find(|region| {
                matches!(region.id().kind(), PresentationHitRegionKind::ContainedResize {
                floating, direction: ContainedResizeDirection::East,
            } if floating == rear)
            })
            .expect("rear floating has an unobscured east resize receiver");
        let bounds = region.hit().rect();
        let press = crate::geometry::LogicalPoint::new(
            bounds.x() + bounds.width() * 0.5,
            bounds.y() + bounds.height() * 0.5,
        )
        .expect("finite resize press");
        let moved = crate::geometry::LogicalPoint::new(press.x() + 30.0, press.y())
            .expect("finite resize movement");
        let delivery = PointerReceiverDelivery::new(
            projection,
            PointerReceiverDeliveryDisposition::Dock(region.id()),
        )
        .expect("exact presented resize receiver");
        let provider = engine
            .create_host_local_pointer_provider(host.lease(), PointerEdgeSequence::new(0))
            .expect("host-local producer");
        let before_version = engine.version();
        let mut frame = host.begin(&engine);
        for (index, kind, point) in [
            (
                0,
                PointerEdgeKind::ButtonPressed(PointerButton::Primary),
                press,
            ),
            (1, PointerEdgeKind::Moved, moved),
            (
                2,
                PointerEdgeKind::ButtonReleased(PointerButton::Primary),
                moved,
            ),
        ] {
            let route = HostLocalPointerRoute::new(authority, point);
            let edge = PointerEdge::new(
                PointerEdgeSequence::new(index + 1),
                crate::intent::PointerId::new(7),
                kind,
                PointerEdgeLocation::HostLocal {
                    manipulation: Authority::Known(route),
                    hover: Authority::Known(route),
                },
                Authority::Known(PointerCaptureOwner::ProviderEndpoint),
            );
            if index != 0 {
                assert!(
                    frame.candidate.interaction_projection(surface).is_none(),
                    "Raise invalidates the source's old general interaction authority"
                );
                assert_eq!(
                    frame.candidate.journal_host_local_manipulation(&edge),
                    Some(route),
                    "the exact active source retains its press-bound coordinate proof"
                );
                let stream = frame
                    .candidate
                    .interaction
                    .active_stream()
                    .expect("same stream");
                let position = journal_position_key(&frame.candidate, stream, &edge);
                assert_eq!(
                    position.hover_probe(),
                    None,
                    "source continuation must not refresh the stale hover authority"
                );
                let foreign_pointer = PointerEdge::new(
                    edge.sequence(),
                    crate::intent::PointerId::new(8),
                    kind,
                    edge.location(),
                    Authority::Known(PointerCaptureOwner::ProviderEndpoint),
                );
                assert_eq!(
                    frame
                        .candidate
                        .journal_host_local_manipulation(&foreign_pointer),
                    None,
                    "another pointer cannot borrow the active source continuation"
                );
                let mut transferred = frame.candidate.candidate();
                let new_host = transferred
                    .presentation_authority
                    .presentation
                    .create_host()
                    .expect("replacement presentation host");
                let replacement = transferred
                    .presentation_authority
                    .presentation
                    .emit(
                        new_host,
                        surface,
                        HostPresentationEndpoint::Headless,
                        HostPresentationOutputPayload::Bootstrap,
                    )
                    .expect("replacement host takes the surface stream");
                assert_eq!(
                    transferred
                        .presentation_authority
                        .presentation
                        .host_for_stream(authority.stream()),
                    Some(host.lease()),
                    "historical stream ownership still names the original host"
                );
                assert_eq!(
                    transferred
                        .presentation_authority
                        .presentation
                        .active_surface_scope(surface),
                    Some((
                        replacement.stream(),
                        new_host,
                        HostPresentationEndpoint::Headless,
                    )),
                    "the surface now belongs to the replacement host's active stream"
                );
                assert_eq!(
                    transferred.journal_host_local_manipulation(&edge),
                    None,
                    "historical ownership cannot preserve source continuation after host takeover"
                );
            }
            frame
                .submit_surface_pointer_journal(
                    &provider,
                    PointerEdgeJournal::new(
                        PointerEdgeSequence::new(index),
                        PointerEdgeSequence::new(index + 1),
                        vec![edge],
                    )
                    .expect("contiguous journal edge"),
                )
                .expect("host-local edge stages");
            let candidate = frame
                .pointer_receiver_candidates()
                .expect("receiver roster")
                .candidates()[0]
                .clone();
            let observation = if index == 0 {
                PointerReceiverObservation::Presented(
                    PresentedPointerReceiverObservation::new([
                        PointerReceiverProbeReceipt::Delivery(delivery),
                    ])
                    .expect("exact press receiver"),
                )
            } else {
                assert!(
                    candidate.probes().is_not_applicable(),
                    "contained resize continuation needs no fresh receiver probe"
                );
                PointerReceiverObservation::NotApplicable
            };
            frame
                .submit_pointer_receiver_receipts(
                    PointerReceiverReceiptBatch::new([candidate.receipt(observation)])
                        .expect("exact receipt roster"),
                )
                .expect("resize edge reduces");
            if index == 0 {
                assert_eq!(
                    frame
                        .candidate
                        .workspace
                        .surface(surface)
                        .expect("surface")
                        .contained,
                    [front, rear],
                    "press must actually raise the rear floating"
                );
                assert_ne!(frame.candidate.version(), before_version);
            } else if index == 1 {
                let preview = frame
                    .candidate
                    .interaction
                    .contained_transform_preview()
                    .expect("movement publishes a real resize preview");
                assert_eq!(preview.rect(), rect(40.0, 40.0, 250.0, 180.0));
            }
        }
        assert_eq!(
            engine.version(),
            before_version,
            "the batch remains private until commit"
        );
        assert_eq!(
            frame.candidate.interaction.status(),
            InteractionStatus::Idle
        );
        assert!(
            frame
                .candidate
                .pending_contained_transform_release
                .is_some(),
            "release retains its resized rectangle for actual presentation"
        );
        complete_host_frame_with_retained_or_unavailable(&engine, &mut frame);
        let transition = host.finish(frame, &mut engine);
        assert!(matches!(
            transition.reduced_pointer_edges()[2]
                .interaction_outcomes()
                .last(),
            Some(InteractionOutcome::ContainedTransformReleasePending { .. })
        ));
        assert_eq!(provider.committed_through(), PointerEdgeSequence::new(3));
        assert_eq!(
            engine
                .workspace
                .contained_floating(rear)
                .expect("durable contained root")
                .rect,
            initial_rect,
            "unpresented release must not commit the resized rectangle"
        );
    }
}
