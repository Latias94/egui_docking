//! Splitter and junction pointer-resize transactions.

use super::*;

impl DockEngine {
    pub(super) fn splitter_resize_delta_interval(
        &self,
        plan: &PresentationPlan,
        target: SplitterResizeTarget,
        axis: Axis,
    ) -> Option<crate::interaction::ResizeDeltaInterval> {
        let handles = match target {
            SplitterResizeTarget::Handle(_) => self.prepare_splitter_resize_handles(plan, target),
            SplitterResizeTarget::Junction(junction) => {
                self.prepare_splitter_junction_axis_handles(plan, junction, axis)
            }
        }
        .ok()?;
        let groups = self
            .prepare_checked_resize_axis_groups(plan, handles, self.policy_snapshot())
            .ok()?;
        groups
            .into_iter()
            .find(|group| group.axis == axis)
            .map(|group| group.common_delta)
    }

    pub(super) fn prepare_journal_splitter_gesture(
        &self,
        presentation: &JournalSurfacePresentation,
        region: PresentationHitRegionId,
        owner: GestureOwner,
        capture_authority: Authority<PointerCaptureOwner>,
        point: crate::geometry::LogicalPoint,
        device: crate::input::journal::PointerDeviceKind,
        policy: &DockPolicySnapshot,
    ) -> Result<ResizeStart, InteractionRejection> {
        let surface = presentation.surface();
        let plan = presentation.plan();
        let scene = presentation.scene();
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
        if scene.requirement().workspace_epoch() != self.version.epoch() {
            return Err(InteractionRejection::StaleScene);
        }
        if !plan.bounds().contains(point) {
            return Err(InteractionRejection::SplitterGesturePointerOutsideSurface { surface });
        }
        let winner = presentation
            .hit_manifest()
            .resolve_exclusive_for_device(
                PresentationPointerLane::Drag,
                point,
                Default::default(),
                device,
            )
            .ok()
            .flatten()
            .map(|winner| winner.id());
        if winner != Some(region) {
            return Err(InteractionRejection::SplitterGestureHitUnavailable { surface });
        }
        let handles = self.prepare_splitter_resize_handles(plan, target)?;
        let axis_groups = self.prepare_checked_resize_axis_groups(plan, handles, policy)?;
        Ok(ResizeStart {
            owner,
            journal_capture_authority: capture_authority,
            button: PointerButton::Primary,
            surface,
            scene,
            target,
            coordinate_capture: presentation.coordinate_capture(),
            authority: Self::freeze_journal_presentation(presentation),
            initial_pointer: point,
            axis_groups,
        })
    }

    fn prepare_splitter_resize_handles(
        &self,
        plan: &PresentationPlan,
        target: SplitterResizeTarget,
    ) -> Result<Vec<(NodeSource, crate::presentation::scene::SplitterRecord)>, InteractionRejection>
    {
        let surface = plan.surface();
        let ids = match target {
            SplitterResizeTarget::Handle(handle) => vec![handle],
            SplitterResizeTarget::Junction(junction) => junction.splitters(),
        };
        ids.into_iter()
            .map(|id| {
                if let Some(contained) = plan
                    .contained_records()
                    .iter()
                    .find(|record| record.root() == id.root)
                    && self
                        .workspace
                        .contained_floating(contained.floating())
                        .is_none_or(|durable| {
                            durable.root != id.root
                                || plan.unclipped_contained_bounds(contained.floating())
                                    != Some(durable.rect)
                        })
                {
                    return Err(InteractionRejection::StaleScene);
                }
                let record = plan
                    .splitter_record(id)
                    .filter(|record| record.operable())
                    .cloned()
                    .ok_or(InteractionRejection::SplitterGestureHitUnavailable { surface })?;
                let source = self
                    .workspace
                    .capture_node_source(id.root, id.split)
                    .map_err(InteractionRejection::ResizeRejected)?;
                Ok((source, record))
            })
            .collect()
    }

    pub(super) fn prepare_checked_resize_axis_groups(
        &self,
        plan: &PresentationPlan,
        handles: Vec<(NodeSource, crate::presentation::scene::SplitterRecord)>,
        policy: &DockPolicySnapshot,
    ) -> Result<Vec<crate::interaction::FrozenResizeAxisGroup>, InteractionRejection> {
        let groups = prepare_resize_axis_groups(&self.workspace, plan, handles)
            .ok_or(InteractionRejection::SplitterResizeGeometryUnavailable)?;
        for source in groups.iter().flat_map(|group| group.resized_sources()) {
            self.check_resize_policy(source, policy)
                .map_err(InteractionRejection::ResizeRejected)?;
        }
        Ok(groups)
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
            crate::graph::operation::validate_split_resizes(&self.workspace, policy, &updates)
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
            Err(rejection) => {
                self.interaction
                    .take_resize_for_release(session, owner, PointerButton::Primary)
                    .map_err(|source| EngineError::PointerInteractionInvariant {
                        cause,
                        detail: format!("{source:?}"),
                    })?;
                return Ok(InteractionOutcome::Rejected(rejection));
            }
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
        #[cfg(feature = "motion")]
        if changed {
            self.settle_resize_motion(resize.surface);
        }
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
    ) -> Result<Vec<crate::graph::command::SplitResize>, InteractionRejection> {
        self.validate_resize_coordinate_authority(resize)?;
        let updates = split_resize_updates(resize, point)
            .ok_or(InteractionRejection::SplitterResizeGeometryUnavailable)?;
        crate::graph::operation::validate_split_resizes(&self.workspace, policy, &updates)
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
        let stream = resize.owner;
        match (stream.lease().scope(), edge.location()) {
            (PointerProviderScope::HostLocal(_), PointerEdgeLocation::HostLocal { .. }) => {
                let route = self.journal_host_local_manipulation(edge).ok_or(
                    InteractionRejection::SplitterGestureCoordinateAuthorityUnavailable {
                        surface: resize.surface,
                    },
                )?;
                if route.surface() != resize.surface {
                    return Err(InteractionRejection::SplitterGestureSurfaceMismatch {
                        expected: resize.surface,
                        actual: route.surface(),
                    });
                }
                Ok(route.position())
            }
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
            (PointerProviderScope::NativeJoined(_), PointerEdgeLocation::NativeLocal { .. }) => {
                super::pointer_proof::validated_native_local_route(self, edge)
                    .filter(|route| route.binding().surface() == resize.surface)
                    .map(|route| route.surface_position())
                    .ok_or(
                        InteractionRejection::SplitterGestureCoordinateAuthorityUnavailable {
                            surface: resize.surface,
                        },
                    )
            }
            (
                PointerProviderScope::DesktopGlobal | PointerProviderScope::NativeJoined(_),
                PointerEdgeLocation::Desktop { route },
            ) => {
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

impl DockEngine {
    pub(super) fn check_resize_policy(
        &self,
        split: &NodeSource,
        policy: &DockPolicySnapshot,
    ) -> Result<(), CommandError> {
        self.validate_node_source(split)?;
        let axis = match self.workspace.node(split.node()) {
            Some(Node::Split { axis, .. }) => *axis,
            Some(Node::Tabs { .. }) => {
                return Err(CommandError::NodeIsNotSplit { node: split.node() });
            }
            None => {
                return Err(CommandError::MissingNode {
                    role: ReferenceRole::Source,
                    node: split.node(),
                });
            }
        };
        let surface = self.root_surface(split.root());
        match policy.evaluate(&DockPolicyRequest::Resize(DockResizePolicyRequest::new(
            axis, surface,
        ))) {
            PolicyDecision::Allow => Ok(()),
            PolicyDecision::Reject(reason) => Err(reason.into()),
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn adjust_splitter_resize(
        &mut self,
        input: InputSequence,
        scene: SurfaceSceneStamp,
        splitter: SplitterSceneId,
        delta: f64,
        authority: SplitterAdjustmentAuthority,
        policy: &DockPolicySnapshot,
        events: &mut Vec<WorkspaceEvent>,
        interaction_events: &mut Vec<InteractionEvent>,
    ) -> Result<InteractionOutcome, EngineError> {
        let surface = scene.surface();
        let painted = match authority {
            SplitterAdjustmentAuthority::PresentedCoordinates => self
                .presentation_authority
                .scene
                .ready_surface(surface)
                .filter(|painted| {
                    painted.stamp() == scene
                        && scene.requirement().workspace_epoch() == self.version.epoch()
                })
                .ok_or(InteractionRejection::StaleScene),
            SplitterAdjustmentAuthority::CurrentCandidate => self.semantic_candidate(scene),
        };
        let painted = match painted {
            Ok(painted) => painted,
            Err(error) => return Ok(InteractionOutcome::Rejected(error)),
        };
        if authority == SplitterAdjustmentAuthority::PresentedCoordinates
            && !Self::coordinate_capture_matches_current(
                painted.coordinate_capture(),
                self.viewport.viewport(surface),
                self.viewport.surface_coordinate_authority(surface),
            )
        {
            return Ok(InteractionOutcome::Rejected(
                InteractionRejection::SplitterGestureCoordinateAuthorityUnavailable { surface },
            ));
        }
        let painted = painted.frozen_witness();
        self.adjust_splitter_from_candidate(
            input,
            &painted,
            splitter,
            delta,
            policy,
            events,
            interaction_events,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn adjust_splitter_from_candidate(
        &mut self,
        input: InputSequence,
        painted: &crate::presentation::scene::SurfacePlanScene,
        splitter: SplitterSceneId,
        delta: f64,
        policy: &DockPolicySnapshot,
        events: &mut Vec<WorkspaceEvent>,
        interaction_events: &mut Vec<InteractionEvent>,
    ) -> Result<InteractionOutcome, EngineError> {
        let surface = painted.stamp().surface();
        let record = match painted.plan().splitter_record(splitter).cloned() {
            Some(record) if record.operable() => record,
            None => {
                return Ok(InteractionOutcome::Rejected(
                    InteractionRejection::SplitterGestureHitUnavailable { surface },
                ));
            }
            Some(_) => {
                return Ok(InteractionOutcome::Rejected(
                    InteractionRejection::SplitterGestureHitUnavailable { surface },
                ));
            }
        };
        let source = match self
            .workspace
            .capture_node_source(splitter.root, splitter.split)
        {
            Ok(source) => source,
            Err(error) => {
                return Ok(InteractionOutcome::Rejected(
                    InteractionRejection::ResizeRejected(error),
                ));
            }
        };
        let axis = record.axis();
        let groups = match self.prepare_checked_resize_axis_groups(
            painted.plan(),
            vec![(source, record)],
            policy,
        ) {
            Ok(groups) => groups,
            Err(rejection) => return Ok(InteractionOutcome::Rejected(rejection)),
        };
        let delta = match axis {
            Axis::Horizontal => [delta, 0.0],
            Axis::Vertical => [0.0, delta],
        };
        let Some(updates) = resize_axis_group_updates(&groups, delta) else {
            return Ok(InteractionOutcome::Rejected(
                InteractionRejection::SplitterResizeGeometryUnavailable,
            ));
        };
        let command = WorkspaceCommand::ResizeSplits { splits: updates };
        match self.apply_interaction_command_with_policy(input, policy, &command, events)? {
            CommandApplication::Applied { outcome, changed } => {
                if changed {
                    self.invalidate_transient(
                        input,
                        InteractionCancelReason::WorkspaceChanged,
                        interaction_events,
                    )?;
                    #[cfg(feature = "motion")]
                    self.settle_resize_motion(surface);
                }
                Ok(InteractionOutcome::SplitterAdjusted { outcome, changed })
            }
            CommandApplication::Rejected(error) => Ok(InteractionOutcome::Rejected(
                InteractionRejection::ResizeRejected(error),
            )),
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn adjust_splitter_junction_resize(
        &mut self,
        input: InputSequence,
        scene: SurfaceSceneStamp,
        junction: crate::presentation::scene::SplitterJunctionId,
        axis: crate::model::DockspaceAxis,
        delta: f64,
        policy: &DockPolicySnapshot,
        events: &mut Vec<WorkspaceEvent>,
        interaction_events: &mut Vec<InteractionEvent>,
    ) -> Result<InteractionOutcome, EngineError> {
        let painted = match self.semantic_candidate(scene) {
            Ok(painted) => painted.frozen_witness(),
            Err(error) => return Ok(InteractionOutcome::Rejected(error)),
        };
        self.adjust_splitter_junction_from_candidate(
            input,
            &painted,
            junction,
            axis,
            delta,
            policy,
            events,
            interaction_events,
        )
    }

    fn prepare_splitter_junction_axis_handles(
        &self,
        plan: &PresentationPlan,
        junction: crate::presentation::scene::SplitterJunctionId,
        axis: Axis,
    ) -> Result<Vec<(NodeSource, crate::presentation::scene::SplitterRecord)>, InteractionRejection>
    {
        let surface = plan.surface();
        let Some(record) = plan
            .splitter_junction_records()
            .iter()
            .find(|record| record.id() == junction)
        else {
            return Err(InteractionRejection::SplitterGestureHitUnavailable { surface });
        };

        let directions = match axis {
            crate::graph::Axis::Horizontal => [
                crate::presentation::scene::SplitterJunctionDirection::North,
                crate::presentation::scene::SplitterJunctionDirection::South,
            ],
            crate::graph::Axis::Vertical => [
                crate::presentation::scene::SplitterJunctionDirection::East,
                crate::presentation::scene::SplitterJunctionDirection::West,
            ],
        };
        let mut ids = directions
            .into_iter()
            .filter_map(|direction| record.id().arm(direction))
            .collect::<Vec<_>>();
        ids.sort_unstable();
        ids.dedup();
        if ids.is_empty() {
            return Err(InteractionRejection::SplitterGestureHitUnavailable { surface });
        }
        let mut handles = Vec::with_capacity(ids.len());
        for id in ids {
            let Some(handle) = plan
                .splitter_record(id)
                .filter(|splitter| splitter.axis() == axis && splitter.operable())
            else {
                return Err(InteractionRejection::SplitterGestureHitUnavailable { surface });
            };
            handles.push(handle.clone());
        }

        let mut sources = Vec::with_capacity(handles.len());
        for record in handles {
            let source = match self
                .workspace
                .capture_node_source(record.id().root, record.id().split)
            {
                Ok(source) => source,
                Err(error) => {
                    return Err(InteractionRejection::ResizeRejected(error));
                }
            };
            sources.push((source, record));
        }

        Ok(sources)
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn adjust_splitter_junction_from_candidate(
        &mut self,
        input: InputSequence,
        painted: &crate::presentation::scene::SurfacePlanScene,
        junction: crate::presentation::scene::SplitterJunctionId,
        axis: crate::model::DockspaceAxis,
        delta: f64,
        policy: &DockPolicySnapshot,
        events: &mut Vec<WorkspaceEvent>,
        interaction_events: &mut Vec<InteractionEvent>,
    ) -> Result<InteractionOutcome, EngineError> {
        let axis = axis.into();
        let sources =
            match self.prepare_splitter_junction_axis_handles(painted.plan(), junction, axis) {
                Ok(sources) => sources,
                Err(rejection) => return Ok(InteractionOutcome::Rejected(rejection)),
            };

        let groups = match self.prepare_checked_resize_axis_groups(painted.plan(), sources, policy)
        {
            Ok(groups) => groups,
            Err(rejection) => return Ok(InteractionOutcome::Rejected(rejection)),
        };
        let delta = match axis {
            Axis::Horizontal => [delta, 0.0],
            Axis::Vertical => [0.0, delta],
        };
        let Some(updates) = resize_axis_group_updates(&groups, delta) else {
            return Ok(InteractionOutcome::Rejected(
                InteractionRejection::SplitterResizeGeometryUnavailable,
            ));
        };
        let command = WorkspaceCommand::ResizeSplits { splits: updates };
        match self.apply_interaction_command_with_policy(input, policy, &command, events)? {
            CommandApplication::Applied { outcome, changed } => {
                if changed {
                    self.invalidate_transient(
                        input,
                        InteractionCancelReason::WorkspaceChanged,
                        interaction_events,
                    )?;
                    #[cfg(feature = "motion")]
                    self.settle_resize_motion(painted.stamp().surface());
                }
                Ok(InteractionOutcome::SplitterAdjusted { outcome, changed })
            }
            CommandApplication::Rejected(error) => Ok(InteractionOutcome::Rejected(
                InteractionRejection::ResizeRejected(error),
            )),
        }
    }

    pub(super) fn cancel_resize(
        &mut self,
        input: InputSequence,
        session: crate::interaction::ResizeSessionId,
        reason: InteractionCancelReason,
        interaction_events: &mut Vec<InteractionEvent>,
    ) -> InteractionOutcome {
        let owner = match self.interaction.active_resize(session) {
            Ok(resize) => resize.owner,
            Err(error) => return InteractionOutcome::Rejected(error),
        };
        let Some(status) = self.cancel_interaction_state(Some(owner)) else {
            return InteractionOutcome::Rejected(InteractionRejection::NoActiveGesture);
        };
        interaction_events.push(InteractionEvent::new(
            input,
            self.version,
            InteractionEventKind::Cancelled { status, reason },
        ));
        InteractionOutcome::Cancelled { status, reason }
    }
}
