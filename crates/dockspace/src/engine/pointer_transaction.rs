//! Device-independent pointer interaction transaction.

use super::*;

impl DockEngine {
    fn reduce_journal_pointer_edge(
        &mut self,
        cause: ReductionCause,
        focus_causal: FocusCausalStamp,
        stream: PointerStreamId,
        capture_authority_after: Authority<PointerCaptureOwner>,
        edge: &PointerEdge,
        receipt: &ValidatedPointerReceiverReceipt,
        snapshot: &JournalPresentationSnapshot,
        desktop_delivery_route: Option<DesktopRouteValidation>,
        desktop_hover_route: Option<DesktopRouteValidation>,
        policy: &DockPolicySnapshot,
        events: &mut Vec<WorkspaceEvent>,
        interaction_events: &mut Vec<InteractionEvent>,
    ) -> Result<Vec<InteractionOutcome>, EngineError> {
        if stream.pointer() != edge.pointer() || receipt.candidate().sequence() != edge.sequence() {
            return Err(EngineError::ReductionCauseInvariant {
                detail: "journal edge, stream, and receipt sequence do not agree",
            });
        }
        let owner = GestureOwner::Stream(stream);
        let mut outcomes = match edge.kind() {
            PointerEdgeKind::ButtonPressed(PointerButton::Primary) => {
                let press = self.reduce_journal_primary_press(
                    cause,
                    focus_causal,
                    owner,
                    capture_authority_after,
                    edge,
                    receipt,
                    snapshot,
                    desktop_delivery_route,
                    policy,
                    events,
                )?;
                let mut outcomes = if self.interaction.status() == InteractionStatus::Idle {
                    Vec::new()
                } else {
                    self.cancel_pending_release_obligations_caused(
                        cause,
                        InteractionCancelReason::ReplacedByNewGesture,
                        interaction_events,
                    )
                };
                outcomes.extend(press);
                Ok(outcomes)
            }
            PointerEdgeKind::Moved => self.reduce_journal_move(
                cause,
                owner,
                edge,
                receipt,
                snapshot,
                desktop_hover_route,
                policy,
                interaction_events,
            ),
            PointerEdgeKind::ButtonReleased(PointerButton::Primary)
            | PointerEdgeKind::ContactEnded(PointerButton::Primary) => self
                .reduce_journal_primary_release(
                    cause,
                    focus_causal,
                    owner,
                    edge,
                    receipt,
                    snapshot,
                    desktop_delivery_route,
                    desktop_hover_route,
                    policy,
                    events,
                    interaction_events,
                ),
            PointerEdgeKind::CaptureChanged => self.reduce_journal_capture_change(
                cause,
                owner,
                capture_authority_after,
                interaction_events,
            ),
            PointerEdgeKind::StreamCancelled(_) => self.terminate_journal_stream(
                cause,
                stream,
                InteractionCancelReason::PointerStreamCancelled,
                ScrollTerminationReason::StreamCancelled,
                interaction_events,
            ),
            PointerEdgeKind::StreamEnded => self.terminate_journal_stream(
                cause,
                stream,
                InteractionCancelReason::PointerStreamEnded,
                ScrollTerminationReason::StreamEnded,
                interaction_events,
            ),
            PointerEdgeKind::Scrolled(scroll) => {
                self.reduce_scroll_edge(cause, stream, edge, scroll, receipt, snapshot)
            }
            PointerEdgeKind::ButtonPressed(_)
            | PointerEdgeKind::ButtonReleased(_)
            | PointerEdgeKind::ContactEnded(_) => Ok(Vec::new()),
        }?;
        if matches!(edge.kind(), PointerEdgeKind::ContactEnded(_)) {
            outcomes.extend(self.terminate_journal_stream(
                cause,
                stream,
                InteractionCancelReason::PointerStreamEnded,
                ScrollTerminationReason::StreamEnded,
                interaction_events,
            )?);
        }
        Ok(outcomes)
    }

    fn terminate_journal_stream(
        &mut self,
        cause: ReductionCause,
        stream: PointerStreamId,
        interaction_reason: InteractionCancelReason,
        scroll_reason: ScrollTerminationReason,
        interaction_events: &mut Vec<InteractionEvent>,
    ) -> Result<Vec<InteractionOutcome>, EngineError> {
        let mut outcomes = self.terminate_scroll_stream(stream, scroll_reason);
        if self.interaction.active_stream() == Some(stream) {
            self.viewport
                .end_all_drag_routing()
                .map_err(|source| EngineError::Viewport {
                    input: self.last_input,
                    source,
                })?;
            if let Some(outcome) = self.cancel_journal_owner(
                cause,
                GestureOwner::Stream(stream),
                interaction_reason,
                interaction_events,
            )? {
                outcomes.push(outcome);
            }
        }
        Ok(outcomes)
    }

    fn reduce_journal_primary_press(
        &mut self,
        cause: ReductionCause,
        focus_causal: FocusCausalStamp,
        owner: GestureOwner,
        capture_authority: Authority<PointerCaptureOwner>,
        edge: &PointerEdge,
        receipt: &ValidatedPointerReceiverReceipt,
        snapshot: &JournalPresentationSnapshot,
        desktop_delivery_route: Option<DesktopRouteValidation>,
        policy: &DockPolicySnapshot,
        events: &mut Vec<WorkspaceEvent>,
    ) -> Result<Option<InteractionOutcome>, EngineError> {
        if self.interaction.status() != InteractionStatus::Idle {
            return Ok(Some(InteractionOutcome::Rejected(
                InteractionRejection::GestureBusy {
                    status: self.interaction.status(),
                },
            )));
        }
        let Some(point) = Self::journal_logical_point(edge, desktop_delivery_route) else {
            return Ok(None);
        };
        let stream = owner.stream().ok_or(EngineError::ReductionCauseInvariant {
            detail: "journal primary press was reduced for a legacy owner",
        })?;
        let expected_delivery = match stream.lease().scope() {
            PointerProviderScope::SurfaceLocal(_) => PointerEventDeliveryOwner::ProviderEndpoint,
            PointerProviderScope::DesktopGlobal => {
                let binding = desktop_delivery_route
                    .and_then(DesktopRouteValidation::dock_route)
                    .map(|route| route.binding())
                    .ok_or(EngineError::ReductionCauseInvariant {
                        detail: "desktop primary press has no exact hovered source binding",
                    })?;
                PointerEventDeliveryOwner::Native(binding)
            }
        };
        if let Some(rejection) =
            self.journal_press_delivery_rejection(edge.delivery_owner(), expected_delivery)?
        {
            return Ok(Some(InteractionOutcome::Rejected(rejection)));
        }
        let Some((region, presentation)) =
            Self::journal_semantic_press_delivery(cause, receipt, snapshot)?
        else {
            return Ok(None);
        };
        if let Some(rejection) =
            self.journal_press_capture_rejection(owner, capture_authority, presentation)?
        {
            return Ok(Some(InteractionOutcome::Rejected(rejection)));
        }
        if matches!(
            region.kind(),
            PresentationHitRegionKind::TabClose(_)
                | PresentationHitRegionKind::ContainedClose(_)
                | PresentationHitRegionKind::TabStripControl(_)
                | PresentationHitRegionKind::TabListMenuRow { .. }
                | PresentationHitRegionKind::TabListMenuBlocker(_)
                | PresentationHitRegionKind::TabListMenuBackdrop(_)
        ) {
            return self.arm_journal_click(
                cause,
                owner,
                capture_authority,
                presentation,
                region,
                point,
                policy,
            );
        }
        match region.kind() {
            PresentationHitRegionKind::TabBody(tab) => {
                let threshold_origin =
                    self.journal_drag_threshold_origin(edge, desktop_delivery_route)?;
                let source = TabGestureSource::Item(tab);
                let prepared = match self.prepare_journal_tab_gesture(presentation, source, point) {
                    Ok(prepared) => prepared,
                    Err(rejection) => return Ok(Some(InteractionOutcome::Rejected(rejection))),
                };
                self.activate_prepared_journal_tab_gesture(
                    cause,
                    focus_causal,
                    owner,
                    capture_authority,
                    threshold_origin,
                    prepared,
                    policy,
                    events,
                )
                .map(Some)
            }
            PresentationHitRegionKind::TabGroupDrag { bar: group, .. } => {
                let threshold_origin =
                    self.journal_drag_threshold_origin(edge, desktop_delivery_route)?;
                let source = TabGestureSource::Group(group);
                let prepared = match self.prepare_journal_tab_gesture(presentation, source, point) {
                    Ok(prepared) => prepared,
                    Err(rejection) => return Ok(Some(InteractionOutcome::Rejected(rejection))),
                };
                self.activate_prepared_journal_tab_gesture(
                    cause,
                    focus_causal,
                    owner,
                    capture_authority,
                    threshold_origin,
                    prepared,
                    policy,
                    events,
                )
                .map(Some)
            }
            PresentationHitRegionKind::SplitterHandle(_)
            | PresentationHitRegionKind::SplitterJunction(_) => {
                if let Some(rejection) = self.pending_presentation_transition_gesture_rejection() {
                    return Ok(Some(InteractionOutcome::Rejected(rejection)));
                }
                let start = match self.prepare_journal_splitter_gesture(
                    presentation,
                    region,
                    owner,
                    capture_authority,
                    point,
                    policy,
                ) {
                    Ok(start) => start,
                    Err(rejection) => return Ok(Some(InteractionOutcome::Rejected(rejection))),
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
                        detail: "journal splitter press replaced an active gesture after idle validation",
                    });
                }
                Ok(Some(InteractionOutcome::ResizeBegan { session, replaced }))
            }
            PresentationHitRegionKind::ContainedTitle(floating) => {
                let threshold_origin =
                    self.journal_drag_threshold_origin(edge, desktop_delivery_route)?;
                let prepared = match self.prepare_journal_contained_gesture(
                    presentation,
                    floating,
                    ContainedGestureKind::TitleDrag,
                    point,
                ) {
                    Ok(prepared) => prepared,
                    Err(rejection) => return Ok(Some(InteractionOutcome::Rejected(rejection))),
                };
                self.activate_prepared_journal_contained_gesture(
                    cause,
                    owner,
                    capture_authority,
                    Some(threshold_origin),
                    prepared,
                    policy,
                    events,
                )
                .map(Some)
            }
            PresentationHitRegionKind::ContainedResize {
                floating,
                direction,
            } => {
                let prepared = match self.prepare_journal_contained_gesture(
                    presentation,
                    floating,
                    ContainedGestureKind::Resize(direction),
                    point,
                ) {
                    Ok(prepared) => prepared,
                    Err(rejection) => return Ok(Some(InteractionOutcome::Rejected(rejection))),
                };
                self.activate_prepared_journal_contained_gesture(
                    cause,
                    owner,
                    capture_authority,
                    None,
                    prepared,
                    policy,
                    events,
                )
                .map(Some)
            }
            _ => Ok(None),
        }
    }

    fn journal_drag_threshold_origin(
        &self,
        edge: &PointerEdge,
        desktop_route: Option<DesktopRouteValidation>,
    ) -> Result<JournalDragThresholdOrigin, EngineError> {
        match edge.location() {
            PointerEdgeLocation::SurfaceLocal {
                position: Authority::Known(position),
            } => Ok(JournalDragThresholdOrigin::SurfaceLocal(position)),
            PointerEdgeLocation::Desktop { .. } => {
                let route = desktop_route
                    .and_then(DesktopRouteValidation::dock_route)
                    .ok_or(EngineError::ReductionCauseInvariant {
                        detail: "desktop journal press has no validated coordinate route",
                    })?;
                let source_scale = self
                    .viewport
                    .viewport(route.binding().surface())
                    .filter(|record| record.binding() == route.binding())
                    .and_then(crate::viewport_registry::ViewportRecord::coordinates)
                    .filter(|coordinates| {
                        coordinates.coordinate_generation() == route.coordinate_generation()
                    })
                    .map(CoordinateSnapshot::presentation_scale_factor)
                    .ok_or(EngineError::ReductionCauseInvariant {
                        detail: "desktop journal press lost its validated source scale",
                    })?;
                Ok(JournalDragThresholdOrigin::DesktopPhysical {
                    position: route.desktop_position(),
                    source_scale,
                })
            }
            PointerEdgeLocation::SurfaceLocal {
                position: Authority::Unknown(_),
            } => Err(EngineError::ReductionCauseInvariant {
                detail: "journal press receiver has no authoritative local point",
            }),
        }
    }

    fn journal_drag_threshold_crossed(
        origin: JournalDragThresholdOrigin,
        edge: &PointerEdge,
        source_logical_threshold: f64,
    ) -> bool {
        let (delta_x, delta_y, threshold) = match (origin, edge.location()) {
            (
                JournalDragThresholdOrigin::SurfaceLocal(initial),
                PointerEdgeLocation::SurfaceLocal {
                    position: Authority::Known(current),
                },
            ) => (
                current.x() - initial.x(),
                current.y() - initial.y(),
                source_logical_threshold,
            ),
            (
                JournalDragThresholdOrigin::DesktopPhysical {
                    position: initial,
                    source_scale,
                },
                PointerEdgeLocation::Desktop { route },
            ) => {
                let Authority::Known(current) = route.position() else {
                    return false;
                };
                (
                    current.x() - initial.x(),
                    current.y() - initial.y(),
                    source_logical_threshold * source_scale.get(),
                )
            }
            _ => return false,
        };
        let distance_squared = delta_x * delta_x + delta_y * delta_y;
        let threshold_squared = threshold * threshold;
        distance_squared.is_finite()
            && threshold_squared.is_finite()
            && distance_squared >= threshold_squared
    }

    fn reduce_journal_move(
        &mut self,
        cause: ReductionCause,
        owner: GestureOwner,
        edge: &PointerEdge,
        receipt: &ValidatedPointerReceiverReceipt,
        snapshot: &JournalPresentationSnapshot,
        desktop_hover_route: Option<DesktopRouteValidation>,
        policy: &DockPolicySnapshot,
        interaction_events: &mut Vec<InteractionEvent>,
    ) -> Result<Vec<InteractionOutcome>, EngineError> {
        let mut outcomes = Vec::new();
        if self.interaction.active_stream() != owner.stream() {
            return Ok(outcomes);
        }
        let (delivery_authorized, delivery_outcome) = self.gate_journal_delivery_action(
            cause,
            owner,
            edge.delivery_owner(),
            interaction_events,
        )?;
        if let Some(outcome) = delivery_outcome {
            outcomes.push(outcome);
        }
        if !delivery_authorized {
            return Ok(outcomes);
        }
        let (capture_authorized, capture_outcome) = self.gate_journal_capture_action(
            cause,
            owner,
            edge.capture_owner(),
            false,
            interaction_events,
        )?;
        if let Some(outcome) = capture_outcome {
            outcomes.push(outcome);
        }
        if !capture_authorized {
            return Ok(outcomes);
        }
        if let Some(outcome) = self.cancel_revoked_presentation_caused(cause, interaction_events)? {
            outcomes.push(outcome);
            return Ok(outcomes);
        }
        if let InteractionStatus::Armed { session } = self.interaction.status() {
            let armed = match self.interaction.armed_drag(session) {
                Ok(armed) => armed,
                Err(error) => {
                    outcomes.push(InteractionOutcome::Rejected(error));
                    return Ok(outcomes);
                }
            };
            if armed.initial_pointer.is_none() {
                return Err(EngineError::ReductionCauseInvariant {
                    detail: "journal-owned armed drag has no initial pointer",
                });
            }
            let Some(threshold_origin) = armed.journal_threshold_origin else {
                return Err(EngineError::ReductionCauseInvariant {
                    detail: "journal-owned armed drag has no threshold coordinate origin",
                });
            };
            let threshold = self
                .presentation_authority
                .presentation_config
                .pointer_drag_start_distance();
            if !Self::journal_drag_threshold_crossed(threshold_origin, edge, threshold) {
                return Ok(outcomes);
            }
            if let Some(rejection) = self.pending_presentation_transition_gesture_rejection() {
                outcomes.push(InteractionOutcome::Rejected(rejection));
                return Ok(outcomes);
            }
            let button = armed.button;
            let source_surface = armed.source_surface;
            match self.interaction.begin_drag(session, owner, button) {
                Ok(()) => {
                    let provider_scope = owner
                        .stream()
                        .expect("journal gestures retain their pointer stream")
                        .lease()
                        .scope();
                    if provider_scope == PointerProviderScope::DesktopGlobal
                        && self
                            .viewport
                            .physical_cross_surface_drag_capability()
                            .is_supported()
                    {
                        let _ = self
                            .viewport
                            .begin_drag_routing(owner.pointer(), source_surface)
                            .map_err(|source| EngineError::Viewport {
                                input: self.last_input,
                                source,
                            })?;
                    }
                    self.freeze_journal_drag_presentation_reservation(cause, session, owner)?;
                    outcomes.push(InteractionOutcome::DragBegan { session });
                }
                Err(error) => {
                    outcomes.push(InteractionOutcome::Rejected(error));
                    return Ok(outcomes);
                }
            }
        }

        let status = self.interaction.status();
        if let InteractionStatus::Resizing { session } = status {
            outcomes.push(self.update_journal_resize(cause, owner, session, edge, policy)?);
            return Ok(outcomes);
        }
        if let InteractionStatus::ContainedTransforming { session } = status {
            outcomes.push(self.update_journal_contained_transform(
                cause,
                owner,
                session,
                edge,
                interaction_events,
            )?);
            return Ok(outcomes);
        }
        let InteractionStatus::Dragging { session } = status else {
            return Ok(outcomes);
        };
        let drag = self
            .interaction
            .active_drag(session)
            .map_err(|source| EngineError::PointerInteractionInvariant {
                cause,
                detail: format!("{source:?}"),
            })?
            .clone();
        if !self.drag_source_presentation_allows_targeting(&drag) {
            outcomes.push(InteractionOutcome::Rejected(
                InteractionRejection::StaleScene,
            ));
            return Ok(outcomes);
        }
        let evaluation = self.resolve_journal_drag_preview(
            cause,
            &drag,
            edge,
            receipt,
            snapshot,
            desktop_hover_route,
            policy,
        )?;
        if let Some(outcome) =
            self.apply_preview_evaluation(cause, owner, session, evaluation, interaction_events)?
        {
            outcomes.push(outcome);
        }
        Ok(outcomes)
    }

    fn freeze_journal_drag_presentation_reservation(
        &mut self,
        cause: ReductionCause,
        session: crate::interaction::DragSessionId,
        owner: GestureOwner,
    ) -> Result<(), EngineError> {
        let drag = self
            .interaction
            .active_drag(session)
            .map_err(|source| EngineError::PointerInteractionInvariant {
                cause,
                detail: format!("{source:?}"),
            })?
            .clone();
        let (
            initial_pointer,
            source_geometry,
            existing_root,
            existing_floating,
            reserve_background_root,
        ) = match &drag.origin {
            FrozenDragOrigin::Workspace => {
                let initial_pointer = drag.initial_pointer.ok_or_else(|| {
                    EngineError::PointerInteractionInvariant {
                        cause,
                        detail: "journal drag activation has no frozen initial pointer".to_owned(),
                    }
                })?;
                let source_geometry = drag.journal_source_geometry.ok_or_else(|| {
                    EngineError::PointerInteractionInvariant {
                        cause,
                        detail: "journal drag activation has no press-bound source geometry"
                            .to_owned(),
                    }
                })?;
                (
                    initial_pointer,
                    source_geometry,
                    drag.complete_root.as_ref().map(NodeSource::root),
                    None,
                    drag.complete_root.is_none(),
                )
            }
            FrozenDragOrigin::Contained(origin) => (
                origin.initial_pointer,
                JournalDragSourceGeometry {
                    source_rect: origin.source_rect,
                    minimum_size: origin.minimum_size,
                },
                Some(origin.root),
                Some(origin.floating),
                false,
            ),
        };
        let reservation = self.reserve_drag_presentation_identities(
            existing_root.is_none(),
            existing_floating.is_none(),
        )?;
        let root = existing_root.or(reservation.root()).ok_or_else(|| {
            EngineError::PointerInteractionInvariant {
                cause,
                detail: "drag presentation reservation did not provide a root".to_owned(),
            }
        })?;
        let floating = existing_floating
            .or(reservation.floating())
            .ok_or_else(|| EngineError::PointerInteractionInvariant {
                cause,
                detail: "drag presentation reservation did not provide a floating identity"
                    .to_owned(),
            })?;
        self.interaction
            .freeze_journal_presentation_reservation(
                session,
                owner,
                reservation.surface(),
                root,
                floating,
                SurfacePointer::new(drag.source_surface, initial_pointer),
                source_geometry.source_rect,
                source_geometry.minimum_size,
                reserve_background_root,
            )
            .map_err(|source| EngineError::PointerInteractionInvariant {
                cause,
                detail: source.to_string(),
            })
    }

    fn reduce_journal_primary_release(
        &mut self,
        cause: ReductionCause,
        focus_causal: FocusCausalStamp,
        owner: GestureOwner,
        edge: &PointerEdge,
        receipt: &ValidatedPointerReceiverReceipt,
        snapshot: &JournalPresentationSnapshot,
        desktop_delivery_route: Option<DesktopRouteValidation>,
        desktop_hover_route: Option<DesktopRouteValidation>,
        policy: &DockPolicySnapshot,
        events: &mut Vec<WorkspaceEvent>,
        interaction_events: &mut Vec<InteractionEvent>,
    ) -> Result<Vec<InteractionOutcome>, EngineError> {
        let mut outcomes = Vec::new();
        if self.interaction.active_stream() != owner.stream() {
            return Ok(outcomes);
        }
        let (delivery_authorized, delivery_outcome) = self.gate_journal_delivery_action(
            cause,
            owner,
            edge.delivery_owner(),
            interaction_events,
        )?;
        if let Some(outcome) = delivery_outcome {
            outcomes.push(outcome);
        }
        if !delivery_authorized {
            return Ok(outcomes);
        }
        let (capture_authorized, capture_outcome) = self.gate_journal_capture_action(
            cause,
            owner,
            edge.capture_owner(),
            true,
            interaction_events,
        )?;
        let capture_unavailable = capture_outcome.is_none();
        if let Some(outcome) = capture_outcome {
            outcomes.push(outcome);
        }
        if !capture_authorized {
            if capture_unavailable {
                if let Some(outcome) = self.cancel_journal_owner(
                    cause,
                    owner,
                    InteractionCancelReason::CaptureAuthorityUnavailable,
                    interaction_events,
                )? {
                    outcomes.push(outcome);
                }
            }
            return Ok(outcomes);
        }
        if let Some(outcome) = self.cancel_revoked_presentation_caused(cause, interaction_events)? {
            outcomes.push(outcome);
            return Ok(outcomes);
        }
        match self.interaction.status() {
            InteractionStatus::Pressed { session } => {
                outcomes.push(self.finish_journal_click_release(
                    cause,
                    focus_causal,
                    owner,
                    session,
                    edge,
                    receipt,
                    snapshot,
                    desktop_delivery_route,
                    policy,
                    events,
                    interaction_events,
                )?);
            }
            InteractionStatus::Armed { session } => match self.interaction.cancel_drag(session) {
                Ok(status) => {
                    let reason = InteractionCancelReason::ReleasedBeforeDrag;
                    interaction_events.push(InteractionEvent::new_caused(
                        cause,
                        self.version,
                        InteractionEventKind::Cancelled { status, reason },
                    ));
                    outcomes.push(InteractionOutcome::Cancelled { status, reason });
                }
                Err(error) => outcomes.push(InteractionOutcome::Rejected(error)),
            },
            InteractionStatus::Dragging { session } => {
                let drag = self
                    .interaction
                    .active_drag(session)
                    .map_err(|source| EngineError::PointerInteractionInvariant {
                        cause,
                        detail: format!("{source:?}"),
                    })?
                    .clone();
                if !self.drag_source_presentation_allows_targeting(&drag) {
                    if let Some(outcome) = self.cancel_journal_owner(
                        cause,
                        owner,
                        InteractionCancelReason::SceneUnavailable,
                        interaction_events,
                    )? {
                        outcomes.push(outcome);
                    }
                    return Ok(outcomes);
                }
                let evaluation = self.resolve_journal_drag_preview(
                    cause,
                    &drag,
                    edge,
                    receipt,
                    snapshot,
                    desktop_hover_route,
                    policy,
                )?;
                let release_decision = evaluation.decision.clone();
                let terminal_reason = match &evaluation.decision {
                    PreviewDecision::Clear(PreviewResolutionStatus::UnknownAuthority) => {
                        Some(InteractionCancelReason::UnknownTargetAuthority)
                    }
                    PreviewDecision::Clear(PreviewResolutionStatus::OpaqueBlocker) => {
                        Some(InteractionCancelReason::OpaquePointerBlocker)
                    }
                    _ => None,
                };
                if let Some(outcome) = self.apply_preview_evaluation(
                    cause,
                    owner,
                    session,
                    evaluation,
                    interaction_events,
                )? {
                    outcomes.push(outcome);
                }
                if let Some(reason) = terminal_reason {
                    if let Some(outcome) =
                        self.cancel_journal_owner(cause, owner, reason, interaction_events)?
                    {
                        outcomes.push(outcome);
                    }
                } else {
                    let drag = match self.interaction.take_drag_for_release(
                        session,
                        owner,
                        PointerButton::Primary,
                    ) {
                        Ok(drag) => drag,
                        Err(error) => {
                            outcomes.push(InteractionOutcome::Rejected(error));
                            return Ok(outcomes);
                        }
                    };
                    let physical_route = drag
                        .owner
                        .pointer_if_physical()
                        .and_then(|pointer| self.viewport.drag_source(pointer))
                        .filter(|binding| binding.surface() == drag.source_surface);
                    let _ = self
                        .viewport
                        .end_drag_routing(owner.pointer())
                        .map_err(|source| EngineError::Viewport {
                            input: self.last_input,
                            source,
                        })?;
                    if drag.preview.as_ref().is_some_and(|published| {
                        !published.painted()
                            && Self::release_decision_matches_preview(published, &release_decision)
                    }) {
                        outcomes.push(self.defer_drag_release(
                            cause,
                            focus_causal,
                            session,
                            drag,
                            physical_route,
                            release_decision,
                            policy,
                        )?);
                    } else {
                        outcomes.push(self.finish_drag_release(
                            cause,
                            focus_causal,
                            session,
                            drag,
                            release_decision,
                            policy,
                            events,
                            interaction_events,
                        )?);
                    }
                }
            }
            InteractionStatus::Resizing { session } => {
                outcomes.push(self.finish_journal_resize_release(
                    cause,
                    owner,
                    session,
                    edge,
                    policy,
                    events,
                    interaction_events,
                )?);
            }
            InteractionStatus::ContainedTransforming { session } => {
                outcomes.extend(self.finish_journal_contained_transform_release(
                    cause,
                    owner,
                    session,
                    edge,
                    policy,
                    events,
                    interaction_events,
                )?);
            }
            InteractionStatus::Idle => {}
        }
        Ok(outcomes)
    }

    fn finish_journal_click_release(
        &mut self,
        cause: ReductionCause,
        focus_causal: FocusCausalStamp,
        owner: GestureOwner,
        session: ClickSessionId,
        edge: &PointerEdge,
        receipt: &ValidatedPointerReceiverReceipt,
        snapshot: &JournalPresentationSnapshot,
        desktop_route: Option<DesktopRouteValidation>,
        policy: &DockPolicySnapshot,
        events: &mut Vec<WorkspaceEvent>,
        interaction_events: &mut Vec<InteractionEvent>,
    ) -> Result<InteractionOutcome, EngineError> {
        let click =
            match self
                .interaction
                .take_click_for_release(session, owner, PointerButton::Primary)
            {
                Ok(click) => click,
                Err(rejection) => return Ok(InteractionOutcome::Rejected(rejection)),
            };
        let pressed = InteractionStatus::Pressed { session };
        let (region, presentation) =
            match Self::journal_click_delivery(cause, edge, receipt, snapshot, desktop_route)? {
                JournalClickDelivery::Dock(region, presentation) => (region, presentation),
                JournalClickDelivery::KnownMismatch => {
                    return Ok(Self::record_terminal_click_cancellation(
                        cause,
                        self.version,
                        pressed,
                        InteractionCancelReason::ClickReceiverMismatch,
                        interaction_events,
                    ));
                }
                JournalClickDelivery::Blocked => {
                    return Ok(Self::record_terminal_click_cancellation(
                        cause,
                        self.version,
                        pressed,
                        InteractionCancelReason::OpaquePointerBlocker,
                        interaction_events,
                    ));
                }
                JournalClickDelivery::Unknown => {
                    return Ok(Self::record_terminal_click_cancellation(
                        cause,
                        self.version,
                        pressed,
                        InteractionCancelReason::UnknownTargetAuthority,
                        interaction_events,
                    ));
                }
            };
        if region != click.region
            || presentation.scene().requirement() != click.measurement
            || !presentation
                .coordinate_capture()
                .same_projection_authority(click.coordinates)
        {
            return Ok(Self::record_terminal_click_cancellation(
                cause,
                self.version,
                pressed,
                InteractionCancelReason::ClickReceiverMismatch,
                interaction_events,
            ));
        }
        if matches!(
            click.action.as_ref(),
            FrozenClickAction::TabListMenuRow(_)
                | FrozenClickAction::TabListMenuBlocker(_)
                | FrozenClickAction::TabListMenuBackdrop(_)
        ) && (!click
            .presentation
            .presented
            .same_interaction_semantics(presentation.authority())
            || click.presentation.popup_gate_revision != presentation.popup_gate_revision()
            || !self.journal_popup_presentation_is_exact_current(presentation))
        {
            return Ok(Self::record_terminal_click_cancellation(
                cause,
                self.version,
                pressed,
                InteractionCancelReason::ClickReceiverMismatch,
                interaction_events,
            ));
        }

        match click.action.as_ref() {
            FrozenClickAction::Close(close) => {
                self.finish_journal_close_click(cause, pressed, close, policy, interaction_events)
            }
            FrozenClickAction::DockBack(dock_back) => self.finish_journal_contained_dock_back(
                cause,
                dock_back,
                presentation.plan(),
                policy,
                events,
            ),
            FrozenClickAction::TabStripControl(control) => {
                self.finish_journal_tab_strip_control(cause, control, presentation.plan())
            }
            FrozenClickAction::TabListMenuRow(row) => self.finish_journal_tab_list_menu_row(
                cause,
                focus_causal,
                row,
                presentation.plan(),
                policy,
                events,
            ),
            FrozenClickAction::TabListMenuBlocker(blocker) => {
                self.finish_journal_tab_list_menu_blocker(blocker, presentation.plan())
            }
            FrozenClickAction::TabListMenuBackdrop(backdrop) => {
                self.finish_journal_tab_list_menu_backdrop(cause, backdrop, presentation.plan())
            }
        }
    }

    fn finish_journal_close_click(
        &mut self,
        cause: ReductionCause,
        pressed: InteractionStatus,
        close: &FrozenCloseClick,
        policy: &DockPolicySnapshot,
        interaction_events: &mut Vec<InteractionEvent>,
    ) -> Result<InteractionOutcome, EngineError> {
        let current_target = match close.target {
            ClosePlanTarget::Item { item } => ContentCloseTarget::Item(item),
            ClosePlanTarget::Root { root } => ContentCloseTarget::Root(root),
            ClosePlanTarget::Surface { .. } => {
                return Err(EngineError::ReductionCauseInvariant {
                    detail: "journal content click froze a surface close target",
                });
            }
        };
        let current = match self.capture_content_close(current_target, policy) {
            Ok(current) => current,
            Err(_) => {
                return Ok(Self::record_terminal_click_cancellation(
                    cause,
                    self.version,
                    pressed,
                    InteractionCancelReason::SourceVanished,
                    interaction_events,
                ));
            }
        };
        if current.target != close.target
            || current.requirements != close.requirements
            || current.prepared != close.prepared
            || prepare_content_close(&self.workspace, policy, &close.prepared).is_err()
        {
            return Ok(Self::record_terminal_click_cancellation(
                cause,
                self.version,
                pressed,
                InteractionCancelReason::SourceVanished,
                interaction_events,
            ));
        }

        let authority = self.close_authority();
        if let Some(existing) = self
            .close
            .active_plan_for_target(close.target, authority)
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
                close.target,
                close.requirements.clone(),
                PreparedCloseOperation::Content(close.prepared.clone()),
            )
            .map_err(|source| EngineError::PointerInteractionInvariant {
                cause,
                detail: source.to_string(),
            })?;
        Ok(InteractionOutcome::CloseRequested {
            plan,
            reused: false,
        })
    }

    fn finish_journal_contained_dock_back(
        &mut self,
        cause: ReductionCause,
        frozen: &FrozenContainedDockBackClick,
        plan: &PresentationPlan,
        policy: &DockPolicySnapshot,
        events: &mut Vec<WorkspaceEvent>,
    ) -> Result<InteractionOutcome, EngineError> {
        let target = CloseSceneTarget::Contained(frozen.floating);
        let Some(record) = plan.contained_record(frozen.floating) else {
            return Ok(InteractionOutcome::Rejected(
                InteractionRejection::CloseSceneTargetUnavailable { target },
            ));
        };
        if record.root() != frozen.root || record.close_bounds().is_none() {
            return Ok(InteractionOutcome::Rejected(
                InteractionRejection::CloseSceneTargetUnavailable { target },
            ));
        }
        match self.apply_journal_product_action(
            cause,
            ProductAction::DockBackRoot { root: frozen.root },
            policy,
            events,
        )? {
            Ok(outcome) => Ok(InteractionOutcome::ProductActionApplied(outcome)),
            Err(reason) => Ok(InteractionOutcome::ProductActionRejected(reason)),
        }
    }

    fn finish_journal_tab_list_menu_blocker(
        &self,
        frozen: &FrozenTabListMenuBlockerClick,
        plan: &PresentationPlan,
    ) -> Result<InteractionOutcome, EngineError> {
        let session = frozen.session;
        if let Err(rejection) = self.validate_tab_list_menu_popup(plan, session, frozen.revision) {
            return Ok(InteractionOutcome::Rejected(rejection));
        }
        let Some(current) = plan
            .tab_list_menu_records()
            .iter()
            .find(|record| record.session() == session)
        else {
            return Ok(InteractionOutcome::Rejected(
                InteractionRejection::TabListMenuBlockerUnavailable { session },
            ));
        };
        if current != &frozen.record {
            return Ok(InteractionOutcome::Rejected(
                InteractionRejection::TabListMenuBlockerRecordChanged { session },
            ));
        }
        Ok(InteractionOutcome::TabListMenuFrameConsumed { session })
    }

    pub(super) fn finish_journal_tab_list_menu_backdrop(
        &mut self,
        cause: ReductionCause,
        frozen: &FrozenTabListMenuBackdropClick,
        plan: &PresentationPlan,
    ) -> Result<InteractionOutcome, EngineError> {
        let session = frozen.session;
        if let Err(rejection) = self.validate_tab_list_menu_popup(plan, session, frozen.revision) {
            return Ok(InteractionOutcome::Rejected(rejection));
        }
        let Some(current) = plan
            .tab_list_menu_backdrop_records()
            .iter()
            .copied()
            .find(|record| record.session() == session)
        else {
            return Ok(InteractionOutcome::Rejected(
                InteractionRejection::TabListMenuBackdropUnavailable { session },
            ));
        };
        if current != frozen.record {
            return Ok(InteractionOutcome::Rejected(
                InteractionRejection::TabListMenuBackdropRecordChanged { session },
            ));
        }

        let mut tab_strip_states = self.presentation_authority.tab_strip_states.clone();
        let delta = tab_strip_states
            .close_tab_list_menu(session)
            .map_err(|source| Self::tab_strip_state_invariant(cause, source))?;
        self.presentation_authority.tab_strip_states = tab_strip_states;
        self.consume_tab_strip_state_delta(cause, &delta)?;
        Ok(InteractionOutcome::TabListMenuDismissed { session })
    }

    fn record_terminal_click_cancellation(
        cause: ReductionCause,
        version: WorkspaceVersion,
        status: InteractionStatus,
        reason: InteractionCancelReason,
        interaction_events: &mut Vec<InteractionEvent>,
    ) -> InteractionOutcome {
        interaction_events.push(InteractionEvent::new_caused(
            cause,
            version,
            InteractionEventKind::Cancelled { status, reason },
        ));
        InteractionOutcome::Cancelled { status, reason }
    }

    fn arm_journal_click(
        &mut self,
        cause: ReductionCause,
        owner: GestureOwner,
        capture_authority: Authority<PointerCaptureOwner>,
        presentation: &JournalSurfacePresentation,
        region: PresentationHitRegionId,
        point: crate::geometry::LogicalPoint,
        policy: &DockPolicySnapshot,
    ) -> Result<Option<InteractionOutcome>, EngineError> {
        if presentation.scene().requirement().workspace_epoch() != self.version.epoch() {
            return Ok(Some(InteractionOutcome::Rejected(
                InteractionRejection::StaleScene,
            )));
        }
        if is_tab_list_menu_click(region.kind())
            && !self.journal_popup_presentation_is_exact_current(presentation)
        {
            return Ok(Some(InteractionOutcome::Rejected(
                InteractionRejection::StaleScene,
            )));
        }
        let action = match self.freeze_journal_click_action(
            presentation.plan(),
            presentation.surface(),
            region,
            point,
            policy,
        ) {
            Ok(action) => action,
            Err(rejection) => return Ok(Some(InteractionOutcome::Rejected(rejection))),
        };
        let (session, replaced) = self
            .interaction
            .begin_click(ClickStart {
                epoch: self.version.epoch(),
                owner,
                capture_authority,
                button: PointerButton::Primary,
                region,
                measurement: presentation.scene().requirement(),
                coordinates: presentation.coordinate_capture(),
                presentation: Self::freeze_journal_presentation(presentation),
                action: Arc::new(action),
            })
            .map_err(|source| EngineError::PointerInteractionInvariant {
                cause,
                detail: source.to_string(),
            })?;
        if replaced.is_some() {
            return Err(EngineError::ReductionCauseInvariant {
                detail: "journal click press replaced an active gesture after idle validation",
            });
        }
        debug_assert_eq!(
            self.interaction.status(),
            InteractionStatus::Pressed { session }
        );
        Ok(None)
    }

    pub(super) fn freeze_journal_click_action(
        &self,
        plan: &PresentationPlan,
        surface: SurfaceId,
        region: PresentationHitRegionId,
        point: crate::geometry::LogicalPoint,
        policy: &DockPolicySnapshot,
    ) -> Result<FrozenClickAction, InteractionRejection> {
        match region.kind() {
            PresentationHitRegionKind::TabClose(tab) => self
                .freeze_journal_close_click(plan, point, policy, CloseSceneTarget::Tab(tab))
                .map(FrozenClickAction::Close),
            PresentationHitRegionKind::ContainedClose(floating) => self
                .freeze_journal_contained_dock_back(plan, point, floating)
                .map(FrozenClickAction::DockBack),
            PresentationHitRegionKind::TabStripControl(control) => {
                let record = plan
                    .tab_strip_control_records()
                    .iter()
                    .copied()
                    .find(|record| record.id() == control)
                    .ok_or(InteractionRejection::TabStripControlUnavailable { control })?;
                if !record.hit().contains(point) {
                    return Err(InteractionRejection::TabStripControlHitMismatch { control });
                }
                if !record.enabled() {
                    return Err(InteractionRejection::TabStripControlDisabled { control });
                }
                let key = TabStripStateKey::new(surface, control.bar());
                if self
                    .presentation_authority
                    .tab_strip_states
                    .state(key)
                    .is_none()
                    || !plan
                        .tab_bar_records()
                        .iter()
                        .any(|bar| *bar.id() == control.bar())
                {
                    return Err(InteractionRejection::TabStripSourceUnavailable { key });
                }
                Ok(FrozenClickAction::TabStripControl(
                    FrozenTabStripControlClick { key, record },
                ))
            }
            PresentationHitRegionKind::TabListMenuRow { menu, tab } => {
                let revision = plan.popup().revision();
                self.validate_tab_list_menu_popup(plan, menu, revision)?;
                let menu_record = plan
                    .tab_list_menu_records()
                    .iter()
                    .find(|record| record.session() == menu)
                    .ok_or(InteractionRejection::TabListMenuSessionUnavailable { session: menu })?;
                let record = menu_record
                    .rows()
                    .iter()
                    .copied()
                    .find(|record| record.tab() == tab)
                    .ok_or(InteractionRejection::TabListMenuRowUnavailable {
                        session: menu,
                        tab,
                    })?;
                if !record.hit().is_some_and(|hit| hit.contains(point)) {
                    return Err(InteractionRejection::TabListMenuRowHitMismatch {
                        session: menu,
                        tab,
                    });
                }
                let active = self
                    .presentation_authority
                    .tab_strip_states
                    .active_menu_for(menu.key())
                    .ok_or(InteractionRejection::TabListMenuSessionUnavailable { session: menu })?;
                if active.session() != menu || !active.items().contains(&tab.item) {
                    return Err(InteractionRejection::TabListMenuSessionUnavailable {
                        session: menu,
                    });
                }
                self.workspace
                    .capture_item_source(tab.root, tab.tabs, tab.item)
                    .map_err(InteractionRejection::CommandRejected)?;
                Ok(FrozenClickAction::TabListMenuRow(
                    FrozenTabListMenuRowClick {
                        session: menu,
                        record,
                        revision,
                    },
                ))
            }
            PresentationHitRegionKind::TabListMenuBlocker(session) => {
                let revision = plan.popup().revision();
                self.validate_tab_list_menu_popup(plan, session, revision)?;
                let record = plan
                    .tab_list_menu_records()
                    .iter()
                    .find(|record| record.session() == session)
                    .cloned()
                    .ok_or(InteractionRejection::TabListMenuBlockerUnavailable { session })?;
                if !record.bounds().contains(point) {
                    return Err(InteractionRejection::TabListMenuBlockerHitMismatch { session });
                }
                Ok(FrozenClickAction::TabListMenuBlocker(
                    FrozenTabListMenuBlockerClick {
                        session,
                        record,
                        revision,
                    },
                ))
            }
            PresentationHitRegionKind::TabListMenuBackdrop(session) => {
                let revision = plan.popup().revision();
                self.validate_tab_list_menu_popup(plan, session, revision)?;
                let record = plan
                    .tab_list_menu_backdrop_records()
                    .iter()
                    .copied()
                    .find(|record| record.session() == session)
                    .ok_or(InteractionRejection::TabListMenuBackdropUnavailable { session })?;
                if record.revision() != revision {
                    return Err(InteractionRejection::TabListMenuBackdropRecordChanged { session });
                }
                if !record.bounds().contains(point) {
                    return Err(InteractionRejection::TabListMenuBackdropHitMismatch { session });
                }
                Ok(FrozenClickAction::TabListMenuBackdrop(
                    FrozenTabListMenuBackdropClick {
                        session,
                        record,
                        revision,
                    },
                ))
            }
            _ => Err(InteractionRejection::TargetAuthorityInvalid),
        }
    }

    fn journal_popup_presentation_is_exact_current(
        &self,
        presentation: &JournalSurfacePresentation,
    ) -> bool {
        self.presentation_authority
            .scene
            .interaction_projection(presentation.surface())
            .is_some_and(|current| {
                current.plan_stamp() == presentation.scene()
                    && current.output_ticket() == presentation.output_ticket()
                    && current.authority() == presentation.authority()
                    && current.popup_gate_revision() == presentation.popup_gate_revision()
                    && current
                        .output()
                        .coordinate_capture()
                        .same_projection_authority(presentation.coordinate_capture())
                    && Self::coordinate_capture_matches_current(
                        presentation.coordinate_capture(),
                        self.viewport.viewport(presentation.surface()),
                        self.viewport
                            .surface_coordinate_authority(presentation.surface()),
                    )
            })
    }

    fn freeze_journal_close_click(
        &self,
        plan: &PresentationPlan,
        point: crate::geometry::LogicalPoint,
        policy: &DockPolicySnapshot,
        target: CloseSceneTarget,
    ) -> Result<FrozenCloseClick, InteractionRejection> {
        let (close_bounds, layer, content_target) = match target {
            CloseSceneTarget::Tab(id) => {
                let Some(record) = plan.tab_records().iter().find(|record| *record.id() == id)
                else {
                    return Err(InteractionRejection::CloseSceneTargetUnavailable { target });
                };
                let Some(close_bounds) = record.close_bounds() else {
                    return Err(InteractionRejection::CloseControlUnavailable { target });
                };
                (
                    close_bounds,
                    record.layer(),
                    ContentCloseTarget::Item(id.item),
                )
            }
            CloseSceneTarget::Contained(floating) => {
                let Some(record) = plan.contained_record(floating) else {
                    return Err(InteractionRejection::CloseSceneTargetUnavailable { target });
                };
                let Some(close_bounds) = record.close_bounds() else {
                    return Err(InteractionRejection::CloseControlUnavailable { target });
                };
                (
                    close_bounds,
                    record.layer(),
                    ContentCloseTarget::Root(record.root()),
                )
            }
        };
        if !close_bounds.contains(point) {
            return Err(InteractionRejection::CloseActivationHitMismatch { target });
        }
        if !plan.point_is_on_authoritative_layer(point, layer) {
            return Err(InteractionRejection::CloseActivationOccluded { target });
        }
        let capture = match self.capture_content_close(content_target, policy) {
            Ok(capture) => capture,
            Err(rejection) => {
                return Err(Self::content_close_rejection_to_interaction(
                    target, rejection,
                ));
            }
        };
        Ok(capture)
    }

    fn freeze_journal_contained_dock_back(
        &self,
        plan: &PresentationPlan,
        point: crate::geometry::LogicalPoint,
        floating: FloatingPresentationId,
    ) -> Result<FrozenContainedDockBackClick, InteractionRejection> {
        let target = CloseSceneTarget::Contained(floating);
        let Some(record) = plan.contained_record(floating) else {
            return Err(InteractionRejection::CloseSceneTargetUnavailable { target });
        };
        let Some(close_bounds) = record.close_bounds() else {
            return Err(InteractionRejection::CloseControlUnavailable { target });
        };
        if !close_bounds.contains(point) {
            return Err(InteractionRejection::CloseActivationHitMismatch { target });
        }
        if !plan.point_is_on_authoritative_layer(point, record.layer()) {
            return Err(InteractionRejection::CloseActivationOccluded { target });
        }
        Ok(FrozenContainedDockBackClick {
            floating,
            root: record.root(),
        })
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn defer_drag_release(
        &mut self,
        cause: ReductionCause,
        focus_causal: FocusCausalStamp,
        session: crate::interaction::DragSessionId,
        drag: crate::interaction::ActiveDrag,
        physical_route: Option<crate::viewport::ViewportBinding>,
        release_decision: PreviewDecision,
        policy: &DockPolicySnapshot,
    ) -> Result<InteractionOutcome, EngineError> {
        if self.pending_drag_release.is_some()
            || self.pending_contained_transform_release.is_some()
            || self.pending_presentation_rehome.is_some()
        {
            return Err(EngineError::PointerInteractionInvariant {
                cause,
                detail: "a drag release presentation obligation is already pending".to_owned(),
            });
        }
        let preview = drag
            .preview
            .as_ref()
            .expect("deferred release has an exact matching preview")
            .public()
            .token();
        self.pending_drag_release = Some(PendingDragRelease {
            source_version: self.version,
            policy_revision: policy.revision(),
            cause,
            focus_causal,
            session,
            drag,
            physical_route,
            release_decision,
            preview,
            presentation_outputs: BTreeSet::new(),
            presented_output: None,
            presentation_failed: false,
        });
        Ok(InteractionOutcome::ReleasePending { session, preview })
    }

    pub(super) fn release_decision_matches_preview(
        preview: &crate::interaction::PublishedPreview,
        release_decision: &PreviewDecision,
    ) -> bool {
        let PreviewDecision::Publish {
            scene,
            visual,
            proof,
        } = release_decision
        else {
            return false;
        };
        preview.public().token().scene() == *scene
            && preview.public().visual() == visual
            && preview.proof() == proof.as_ref()
    }

    pub(super) fn settle_presented_pending_drag_release(
        &mut self,
        events: &mut Vec<WorkspaceEvent>,
        interaction_events: &mut Vec<InteractionEvent>,
    ) -> Result<bool, EngineError> {
        let Some(pending) = self.pending_drag_release.as_ref() else {
            return Ok(false);
        };
        if pending.presented_output.is_none() && !pending.presentation_failed {
            return Ok(false);
        }
        let pending = self
            .pending_drag_release
            .take()
            .expect("checked pending release remains present");
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
        let outcome = self.deliver_drag_release(
            cause,
            pending.focus_causal,
            pending.session,
            pending.drag,
            pending.release_decision,
            &policy,
            events,
            interaction_events,
        )?;
        if !matches!(outcome, InteractionOutcome::DragDelivered { .. }) {
            return Err(EngineError::PointerInteractionInvariant {
                cause,
                detail: format!(
                    "presented release obligation did not deliver its frozen command: {outcome:?}"
                ),
            });
        }
        Ok(true)
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn finish_drag_release(
        &mut self,
        cause: ReductionCause,
        focus_causal: FocusCausalStamp,
        session: crate::interaction::DragSessionId,
        drag: crate::interaction::ActiveDrag,
        release_decision: PreviewDecision,
        policy: &DockPolicySnapshot,
        events: &mut Vec<WorkspaceEvent>,
        interaction_events: &mut Vec<InteractionEvent>,
    ) -> Result<InteractionOutcome, EngineError> {
        let Some(preview) = drag.preview.as_ref() else {
            return Ok(InteractionOutcome::Rejected(
                InteractionRejection::PreviewMissing,
            ));
        };
        if !preview.painted() {
            return Ok(InteractionOutcome::Rejected(
                InteractionRejection::PreviewNotPainted,
            ));
        }
        if !Self::release_decision_matches_preview(preview, &release_decision) {
            return Ok(InteractionOutcome::Rejected(
                InteractionRejection::TargetChanged,
            ));
        }
        self.deliver_drag_release(
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

    #[allow(clippy::too_many_arguments)]
    pub(super) fn deliver_drag_release(
        &mut self,
        cause: ReductionCause,
        focus_causal: FocusCausalStamp,
        session: crate::interaction::DragSessionId,
        drag: crate::interaction::ActiveDrag,
        release_decision: PreviewDecision,
        policy: &DockPolicySnapshot,
        events: &mut Vec<WorkspaceEvent>,
        interaction_events: &mut Vec<InteractionEvent>,
    ) -> Result<InteractionOutcome, EngineError> {
        let PreviewDecision::Publish { proof, .. } = release_decision else {
            return Err(EngineError::PointerInteractionInvariant {
                cause,
                detail: "validated drag release lost its publish decision".to_owned(),
            });
        };
        let pane_focus = match self.freeze_payload_focus(&drag.payload) {
            Ok(pane_focus) => pane_focus,
            Err(error) => {
                return Ok(InteractionOutcome::Rejected(
                    InteractionRejection::CommandRejected(error),
                ));
            }
        };
        let (command, kind, focus_surface) = match *proof {
            PreviewProof::Dock { target, command } => {
                (command, WorkspaceDeliveryKind::Dock, target.surface())
            }
            PreviewProof::Contained {
                command,
                proposal,
                fallback,
            } => (
                command,
                if fallback {
                    WorkspaceDeliveryKind::ContainedFallback
                } else {
                    WorkspaceDeliveryKind::Contained
                },
                proposal.surface(),
            ),
            PreviewProof::Native { command, offer } => {
                let Some(presentation) = drag.presentation.presented() else {
                    return Err(EngineError::ReductionCauseInvariant {
                        detail: "local drag produced a native delivery proof",
                    });
                };
                return self.finish_journal_native_delivery(
                    cause,
                    focus_causal,
                    session,
                    presentation.presented,
                    drag.payload.clone(),
                    pane_focus,
                    command,
                    offer,
                    policy,
                    interaction_events,
                );
            }
            PreviewProof::PresentationRehome { .. } => {
                return Err(EngineError::ReductionCauseInvariant {
                    detail: "programmatic presentation rehome proof entered pointer delivery",
                });
            }
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
        if let Some(binding) = self
            .viewport
            .viewport(focus_surface)
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
            InteractionEventKind::Delivered { session, kind },
        ));
        Ok(InteractionOutcome::DragDelivered {
            session,
            delivery: InteractionDelivery::Workspace {
                kind,
                outcome,
                changed,
            },
        })
    }

    #[allow(clippy::too_many_arguments)]
    fn finish_journal_native_delivery(
        &mut self,
        cause: ReductionCause,
        focus_causal: FocusCausalStamp,
        session: crate::interaction::DragSessionId,
        source_presentation: PresentedSurfaceAuthority,
        payload: MovePayload,
        pane_focus: PaneFocusDisposition,
        command: WorkspaceCommand,
        offer: NativePresentationOffer,
        policy: &DockPolicySnapshot,
        interaction_events: &mut Vec<InteractionEvent>,
    ) -> Result<InteractionOutcome, EngineError> {
        let proposal = offer.proposal();
        if !matches!(proposal.placement(), NativePlacementProof::TearOff(_)) {
            return Ok(InteractionOutcome::Rejected(
                InteractionRejection::TargetChanged,
            ));
        }
        if !self
            .viewport
            .native_placement_is_current(proposal.placement())
        {
            return Ok(InteractionOutcome::Rejected(
                InteractionRejection::TargetChanged,
            ));
        }
        let normalized = crate::frame::NativeCreateProposal::from_tear_off(proposal);
        let request = match self.start_native_root_create(
            cause,
            focus_causal,
            source_presentation,
            payload,
            command,
            normalized,
            pane_focus,
            policy,
        )? {
            Ok(request) => request,
            Err(rejection) => {
                return Ok(InteractionOutcome::Rejected(
                    InteractionRejection::CommandRejected(
                        rejection.into_interaction_command_error(),
                    ),
                ));
            }
        };
        interaction_events.push(InteractionEvent::new_caused(
            cause,
            self.version,
            InteractionEventKind::NativePresentationRequested(request),
        ));
        Ok(InteractionOutcome::DragDelivered {
            session,
            delivery: InteractionDelivery::NativeRequested(request),
        })
    }

    fn resolve_journal_drag_preview(
        &self,
        cause: ReductionCause,
        drag: &crate::interaction::ActiveDrag,
        edge: &PointerEdge,
        receipt: &ValidatedPointerReceiverReceipt,
        snapshot: &JournalPresentationSnapshot,
        desktop_route: Option<DesktopRouteValidation>,
        policy: &DockPolicySnapshot,
    ) -> Result<PreviewEvaluation, EngineError> {
        if matches!(
            desktop_route,
            Some(DesktopRouteValidation::Known(
                ValidatedDesktopRoute::NoWindow { .. }
            ))
        ) {
            let decision =
                self.resolve_journal_native_preview(cause, drag, edge, desktop_route, policy)?;
            return Ok(PreviewEvaluation::without_affordance(decision));
        }
        let Some(edge_point) = Self::journal_logical_point(edge, desktop_route) else {
            return Ok(PreviewEvaluation::without_affordance(
                PreviewDecision::Clear(Self::journal_route_preview_status(edge, desktop_route)),
            ));
        };
        let hover = match receipt.observation() {
            PointerReceiverObservation::Unknown(_) => {
                return Ok(PreviewEvaluation::without_affordance(
                    PreviewDecision::Clear(PreviewResolutionStatus::UnknownAuthority),
                ));
            }
            PointerReceiverObservation::NotApplicable => {
                return Err(EngineError::PointerInteractionInvariant {
                    cause,
                    detail: "active drag edge has no hover receiver observation".to_owned(),
                });
            }
            PointerReceiverObservation::Presented(observation) => observation
                .probes()
                .iter()
                .find_map(|probe| match probe {
                    PointerReceiverProbeReceipt::HoverHit(hover) => Some(*hover),
                    PointerReceiverProbeReceipt::Delivery(_) => None,
                })
                .ok_or_else(|| EngineError::PointerInteractionInvariant {
                    cause,
                    detail: "active drag edge receipt has no hover probe".to_owned(),
                })?,
        };
        match hover.disposition() {
            PointerReceiverHoverHitDisposition::Unknown(_) => {
                return Ok(PreviewEvaluation::without_affordance(
                    PreviewDecision::Clear(PreviewResolutionStatus::UnknownAuthority),
                ));
            }
            PointerReceiverHoverHitDisposition::Blocked => {
                return Ok(PreviewEvaluation::without_affordance(
                    PreviewDecision::Clear(PreviewResolutionStatus::OpaqueBlocker),
                ));
            }
            PointerReceiverHoverHitDisposition::Dock(_)
            | PointerReceiverHoverHitDisposition::NoReceiver => {}
        }
        let (Some(output), Some(authority), Some(point)) =
            (hover.output(), hover.authority(), hover.point())
        else {
            return Err(EngineError::PointerInteractionInvariant {
                cause,
                detail: "known hover receipt has no atomic presentation binding".to_owned(),
            });
        };
        if point != edge_point {
            return Err(EngineError::PointerInteractionInvariant {
                cause,
                detail: "known hover receipt point differs from its journal edge".to_owned(),
            });
        }
        let presentation = snapshot.presentation(output, authority).map_err(|source| {
            EngineError::PointerInteractionInvariant {
                cause,
                detail: format!("{source:?}"),
            }
        })?;
        if !self.presentation_authority_is_current(Self::freeze_journal_presentation(presentation))
        {
            return Ok(PreviewEvaluation::without_affordance(
                PreviewDecision::Clear(PreviewResolutionStatus::UnknownAuthority),
            ));
        }
        if presentation.surface() != drag.source_surface {
            let capability = self.viewport.physical_cross_surface_drag_capability();
            if !capability.is_supported() || !self.physical_drag_has_route(drag) {
                return Ok(PreviewEvaluation::without_affordance(
                    PreviewDecision::Clear(Self::physical_drag_preview_status(capability)),
                ));
            }
        }
        Self::validate_journal_desktop_presentation(edge, desktop_route, authority)?;
        let winner = presentation
            .hit_manifest()
            .resolve_exclusive(PresentationPointerLane::HoverDrop, point)
            .map_err(|_| EngineError::PointerReceiverGeometry {
                source: PointerReceiverGeometryError::ReceiverWinnerAmbiguous {
                    sequence: edge.sequence(),
                    lane: PresentationPointerLane::HoverDrop,
                },
            })?
            .map(|region| region.id());
        let claimed = match hover.disposition() {
            PointerReceiverHoverHitDisposition::Dock(region) => Some(region),
            PointerReceiverHoverHitDisposition::NoReceiver => None,
            PointerReceiverHoverHitDisposition::Blocked
            | PointerReceiverHoverHitDisposition::Unknown(_) => {
                unreachable!("blocked and unknown hover receipts return before winner validation")
            }
        };
        if winner != claimed {
            return Err(EngineError::PointerReceiverGeometry {
                source: PointerReceiverGeometryError::ReceiverWinnerMismatch {
                    sequence: edge.sequence(),
                    lane: PresentationPointerLane::HoverDrop,
                    claimed,
                    winner,
                },
            });
        }
        // The raw framework winner is now proven. Source and payload
        // suppression are core semantics, so only the drop resolver applies
        // them; the adapter never needs to know the active drag payload.
        let query = self
            .resolve_presented_drop_with_current_index(
                &presentation,
                drag.source_layout_facts.as_deref(),
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
                        detail: "drop resolver changed the frozen journal drag identity".to_owned(),
                    });
                }
                let target = resolved.target_id();
                PreviewDecision::Publish {
                    scene: resolved.scene_stamp(),
                    visual: PreviewVisual::Dock {
                        surface: presentation.surface(),
                        target,
                        rect: resolved.visual().rect(),
                    },
                    proof: Box::new(PreviewProof::Dock {
                        target,
                        command: resolved.into_command(),
                    }),
                }
            }
            DropResolution::KnownNone(_) => self.resolve_journal_local_contained_candidate(
                cause,
                drag,
                SurfacePointer::new(presentation.surface(), point),
                policy,
            )?,
            DropResolution::Rejected(_) => {
                PreviewDecision::Clear(PreviewResolutionStatus::Rejected)
            }
            DropResolution::Unavailable(_) => {
                return Err(EngineError::PointerInteractionInvariant {
                    cause,
                    detail: "frozen presented drop resolver returned unavailable".to_owned(),
                });
            }
        };
        Ok(PreviewEvaluation::new(decision, affordance))
    }

    pub(super) fn resolve_presented_drop_with_current_index(
        &self,
        presentation: &JournalSurfacePresentation,
        source_layout_facts: Option<&crate::scene::PresentationLayoutFacts>,
        policy: &DockPolicySnapshot,
        session: crate::interaction::DragSessionId,
        source: MovePayload,
        surface_background_offer: Option<crate::intent::SurfaceBackgroundRootOffer>,
        point: crate::geometry::LogicalPoint,
    ) -> Result<crate::drop_resolver::DropQuery, DropResolutionError> {
        resolve_presented_drop(
            presentation.scene(),
            presentation.plan(),
            source_layout_facts,
            &self.workspace,
            self.version,
            self.presentation_authority
                .presentation_requirements
                .workspace_index(),
            policy,
            session,
            source,
            surface_background_offer,
            point,
        )
    }

    /// Resolves the canonical desktop-global outside-all path from the same
    /// edge facts which drove pointer capture. No legacy platform route or
    /// adapter-owned native offer is consulted here.
    fn resolve_journal_native_preview(
        &self,
        cause: ReductionCause,
        drag: &crate::interaction::ActiveDrag,
        edge: &PointerEdge,
        desktop_route: Option<DesktopRouteValidation>,
        policy: &DockPolicySnapshot,
    ) -> Result<PreviewDecision, EngineError> {
        let Some(DesktopRouteValidation::Known(ValidatedDesktopRoute::NoWindow {
            position: Authority::Known(desktop_position),
            work_area: Authority::Known(work_area_route),
        })) = desktop_route
        else {
            return Ok(PreviewDecision::Clear(
                PreviewResolutionStatus::NativePlacementUnavailable,
            ));
        };
        let Some(pointer_provider) = drag.owner.stream().map(|stream| stream.lease()) else {
            return Ok(PreviewDecision::Clear(PreviewResolutionStatus::Rejected));
        };
        if self.platform_provider() != Some(work_area_route.provider())
            || self.viewport.work_area_generation() != work_area_route.generation()
        {
            return Ok(PreviewDecision::Clear(
                PreviewResolutionStatus::NativePlacementUnavailable,
            ));
        }
        let Some(work_area) = self.viewport.work_area(work_area_route.token()) else {
            return Ok(PreviewDecision::Clear(
                PreviewResolutionStatus::NativePlacementUnavailable,
            ));
        };
        if policy.check_tear_off(TearOffPresentation::Native).is_err() {
            return Ok(PreviewDecision::Clear(PreviewResolutionStatus::Rejected));
        }
        let capability = self.viewport.physical_native_drag_capability();
        if !capability.is_supported() || !self.physical_drag_has_route(drag) {
            let status = Self::physical_drag_preview_status(capability);
            return Ok(PreviewDecision::Clear(status));
        }
        let Some(reservation) = drag.journal_presentation_reservation else {
            return Ok(PreviewDecision::Clear(
                PreviewResolutionStatus::NativePlacementUnavailable,
            ));
        };
        let (source_geometry, initial_pointer) = match &drag.origin {
            FrozenDragOrigin::Workspace => {
                let (Some(source_geometry), Some(initial_pointer)) =
                    (drag.journal_source_geometry, drag.initial_pointer)
                else {
                    return Ok(PreviewDecision::Clear(PreviewResolutionStatus::Unavailable));
                };
                (source_geometry, initial_pointer)
            }
            FrozenDragOrigin::Contained(origin) => (
                JournalDragSourceGeometry {
                    source_rect: origin.source_rect,
                    minimum_size: origin.minimum_size,
                },
                origin.initial_pointer,
            ),
        };
        let cursor_offset = match LogicalSize::new(
            initial_pointer.x() - source_geometry.source_rect.x(),
            initial_pointer.y() - source_geometry.source_rect.y(),
        ) {
            Ok(offset) => offset,
            Err(_) => return Ok(PreviewDecision::Clear(PreviewResolutionStatus::Rejected)),
        };
        let request = TearOffPlacementRequest::new(
            cursor_offset,
            source_geometry.source_rect.size(),
            source_geometry.minimum_size,
            work_area_route.token(),
        );
        let placement = match solve_tear_off_placement(
            pointer_provider,
            work_area_route.provider(),
            edge.pointer(),
            desktop_position,
            work_area,
            work_area_route.generation(),
            request,
        ) {
            Ok(placement) => placement,
            Err(_) => {
                return Ok(PreviewDecision::Clear(
                    PreviewResolutionStatus::NativePlacementUnavailable,
                ));
            }
        };
        if placement.pointer() != drag.owner.pointer()
            || placement.desktop_position() != desktop_position
        {
            return Ok(PreviewDecision::Clear(
                PreviewResolutionStatus::NativePlacementUnavailable,
            ));
        }
        let source_scene = self
            .presentation_authority
            .scene
            .ready_surface(drag.source_surface)
            .map(|ready| ready.stamp());
        let Some(source_scene) = source_scene else {
            return Ok(PreviewDecision::Clear(PreviewResolutionStatus::Unavailable));
        };
        let command = Self::tear_off_command_from_complete_root(
            &drag.payload,
            RootPresentationTarget::NewSurface {
                surface: reservation.surface,
            },
            reservation.root,
            drag.complete_root.clone(),
        );
        let staged = match self.stage_journal_workspace_command(cause, &command, policy)? {
            Ok(staged) => staged,
            Err(_) => return Ok(PreviewDecision::Clear(PreviewResolutionStatus::Rejected)),
        };
        let recovery_anchor = self
            .surface_recovery_target(drag.source_surface)
            .map(|target| target.anchor())
            .or_else(|| {
                self.root_recovery_anchors
                    .get(&drag.source_surface)
                    .copied()
            });
        let Some(recovery_anchor) = recovery_anchor else {
            return Ok(PreviewDecision::Clear(PreviewResolutionStatus::Rejected));
        };
        let recovery_target = SurfaceRecoveryTarget::with_converted_main(
            recovery_anchor,
            ConvertedMainRecovery::new(
                reservation.root,
                reservation.floating,
                source_geometry.minimum_size,
            ),
        );
        let obligation = self.authorize_surface_recovery_obligation(
            self.last_input,
            SurfaceRecoveryObligationId::default(),
            &staged.publication.workspace,
            reservation.surface,
            recovery_target,
            policy,
        );
        if obligation.is_err() {
            return Ok(PreviewDecision::Clear(PreviewResolutionStatus::Rejected));
        }
        let offer = NativePresentationOffer::new(
            NativeTearOffProposal::new(
                reservation.surface,
                reservation.root,
                placement,
                ConvertedMainRecovery::new(
                    reservation.root,
                    reservation.floating,
                    source_geometry.minimum_size,
                ),
            )
            .map_err(|error| EngineError::PointerInteractionInvariant {
                cause,
                detail: error.to_string(),
            })?,
            None,
        );
        Ok(PreviewDecision::Publish {
            scene: source_scene,
            visual: PreviewVisual::Native {
                host_surface: drag.source_surface,
                target_surface: reservation.surface,
                placement: offer.proposal().physical_placement(),
            },
            proof: Box::new(PreviewProof::Native { command, offer }),
        })
    }

    fn physical_drag_has_route(&self, drag: &crate::interaction::ActiveDrag) -> bool {
        drag.owner
            .pointer_if_physical()
            .and_then(|pointer| self.viewport.confirmed_drag_source(pointer))
            .is_some_and(|binding| binding.surface() == drag.source_surface)
    }

    fn physical_drag_preview_status(capability: PlatformCapability) -> PreviewResolutionStatus {
        match capability {
            PlatformCapability::Unknown(_) => PreviewResolutionStatus::NativeCapabilityUnknown,
            PlatformCapability::Unsupported(_) => {
                PreviewResolutionStatus::NativeCapabilityUnavailable
            }
            PlatformCapability::Supported => PreviewResolutionStatus::Rejected,
        }
    }

    fn resolve_journal_local_contained_candidate(
        &self,
        cause: ReductionCause,
        drag: &crate::interaction::ActiveDrag,
        pointer: SurfacePointer,
        policy: &DockPolicySnapshot,
    ) -> Result<PreviewDecision, EngineError> {
        match self.core_contained_candidate(drag, pointer) {
            CoreContainedCandidate::None => {
                Ok(PreviewDecision::Clear(PreviewResolutionStatus::KnownNone))
            }
            CoreContainedCandidate::Proposal(proposal) => {
                self.resolve_journal_contained_proposal(cause, drag, proposal, policy)
            }
            CoreContainedCandidate::Rejected => {
                Ok(PreviewDecision::Clear(PreviewResolutionStatus::Rejected))
            }
            CoreContainedCandidate::Cancel(reason) => Ok(PreviewDecision::Cancel(reason)),
        }
    }

    fn resolve_journal_contained_proposal(
        &self,
        cause: ReductionCause,
        drag: &crate::interaction::ActiveDrag,
        proposal: crate::intent::ContainedTearOffProposal,
        policy: &DockPolicySnapshot,
    ) -> Result<PreviewDecision, EngineError> {
        if self
            .validate_contained_placement(proposal.placement())
            .is_err()
        {
            return Ok(PreviewDecision::Clear(PreviewResolutionStatus::Rejected));
        }
        let Some(command) = self.contained_presentation_command(
            &drag.payload,
            proposal,
            drag.complete_root.clone(),
            match &drag.origin {
                FrozenDragOrigin::Contained(origin) => Some(origin),
                FrozenDragOrigin::Workspace => None,
            },
        ) else {
            return Ok(PreviewDecision::Clear(PreviewResolutionStatus::Rejected));
        };
        if !self.valid_core_contained_command(drag, proposal, &command)
            || self
                .stage_journal_workspace_command(cause, &command, policy)?
                .is_err()
        {
            return Ok(PreviewDecision::Clear(PreviewResolutionStatus::Rejected));
        }
        Ok(PreviewDecision::Publish {
            scene: proposal.placement().scene(),
            visual: PreviewVisual::Contained {
                surface: proposal.surface(),
                rect: proposal.rect(),
                fallback: false,
            },
            proof: Box::new(PreviewProof::Contained {
                command,
                proposal,
                fallback: false,
            }),
        })
    }

    pub(super) fn apply_preview_evaluation(
        &mut self,
        cause: ReductionCause,
        owner: GestureOwner,
        session: crate::interaction::DragSessionId,
        evaluation: PreviewEvaluation,
        interaction_events: &mut Vec<InteractionEvent>,
    ) -> Result<Option<InteractionOutcome>, EngineError> {
        self.interaction
            .set_drop_affordance(session, evaluation.affordance)
            .map_err(|source| EngineError::PointerInteractionInvariant {
                cause,
                detail: format!("{source:?}"),
            })?;
        match evaluation.decision {
            PreviewDecision::Publish {
                scene,
                visual,
                proof,
            } => {
                let (preview, changed) = self
                    .interaction
                    .publish_preview(session, scene, visual, *proof)
                    .map_err(|source| EngineError::PointerInteractionInvariant {
                        cause,
                        detail: format!("{source:?}"),
                    })?;
                if changed {
                    interaction_events.push(InteractionEvent::new_caused(
                        cause,
                        self.version,
                        InteractionEventKind::PreviewPublished {
                            preview: preview.clone(),
                        },
                    ));
                }
                Ok(Some(InteractionOutcome::PreviewUpdated {
                    session,
                    preview: Some(preview),
                    status: PreviewResolutionStatus::Resolved,
                }))
            }
            PreviewDecision::Clear(status) => {
                let changed = self.interaction.clear_preview(session).map_err(|source| {
                    EngineError::PointerInteractionInvariant {
                        cause,
                        detail: format!("{source:?}"),
                    }
                })?;
                if changed {
                    interaction_events.push(InteractionEvent::new_caused(
                        cause,
                        self.version,
                        InteractionEventKind::PreviewCleared { session },
                    ));
                }
                Ok(Some(InteractionOutcome::PreviewUpdated {
                    session,
                    preview: None,
                    status,
                }))
            }
            PreviewDecision::Cancel(reason) => {
                self.cancel_journal_owner(cause, owner, reason, interaction_events)
            }
        }
    }

    pub(super) fn prepare_journal_tab_gesture(
        &self,
        presentation: &JournalSurfacePresentation,
        source: TabGestureSource,
        point: crate::geometry::LogicalPoint,
    ) -> Result<PreparedTabGesture, InteractionRejection> {
        let surface = presentation.surface();
        let plan = presentation.plan();
        self.prepare_tab_gesture(
            surface,
            presentation.scene(),
            plan,
            source,
            point,
            DragGestureAuthority::Presented(Self::freeze_journal_presentation(presentation)),
        )
    }

    pub(super) fn prepare_local_tab_gesture(
        &self,
        candidate: &crate::scene::SurfacePlanScene,
        source: TabGestureSource,
        point: crate::geometry::LogicalPoint,
    ) -> Result<PreparedTabGesture, InteractionRejection> {
        let surface = candidate.stamp().surface();
        self.prepare_tab_gesture(
            surface,
            candidate.stamp(),
            candidate.plan(),
            source,
            point,
            DragGestureAuthority::LocalReady {
                surface,
                coordinates: candidate.coordinate_capture(),
            },
        )
    }

    fn prepare_tab_gesture(
        &self,
        surface: SurfaceId,
        scene: SurfaceSceneStamp,
        plan: &crate::scene::PresentationPlan,
        source: TabGestureSource,
        point: crate::geometry::LogicalPoint,
        presentation: DragGestureAuthority,
    ) -> Result<PreparedTabGesture, InteractionRejection> {
        if scene.requirement().workspace_epoch() != self.version.epoch() {
            return Err(InteractionRejection::StaleScene);
        }
        if !plan.bounds().contains(point) {
            return Err(InteractionRejection::TabGesturePointerOutsideSurface { surface });
        }
        let (root, tabs, layer) = match source {
            TabGestureSource::Item(source) => {
                let tab = plan
                    .tab_records()
                    .iter()
                    .find(|record| *record.id() == source)
                    .ok_or(InteractionRejection::TabGestureSourceUnavailable {
                        source: TabGestureSource::Item(source),
                    })?;
                if !tab.drag_hit().contains(point)
                    || tab
                        .close_bounds()
                        .is_some_and(|close| close.contains(point))
                {
                    return Err(InteractionRejection::TabGestureHitMismatch {
                        source: TabGestureSource::Item(source),
                    });
                }
                (source.root, source.tabs, tab.layer())
            }
            TabGestureSource::Group(source) => {
                let bar = plan
                    .tab_bar_records()
                    .iter()
                    .find(|record| *record.id() == source)
                    .ok_or(InteractionRejection::TabGestureSourceUnavailable {
                        source: TabGestureSource::Group(source),
                    })?;
                let group =
                    bar.group_drag()
                        .ok_or(InteractionRejection::TabGestureSourceUnavailable {
                            source: TabGestureSource::Group(source),
                        })?;
                if !group.contains(point) {
                    return Err(InteractionRejection::TabGestureHitMismatch {
                        source: TabGestureSource::Group(source),
                    });
                }
                (source.root, source.tabs, bar.layer())
            }
            TabGestureSource::ContainedTitle { root, floating } => {
                let contained = plan
                    .contained_record(floating)
                    .filter(|record| record.root() == root)
                    .ok_or(InteractionRejection::TabGestureSourceUnavailable { source })?;
                if !contained.title_drag_hit().contains(point) {
                    return Err(InteractionRejection::TabGestureHitMismatch { source });
                }
                let node = self
                    .workspace
                    .root(root)
                    .map(|record| record.node)
                    .ok_or(InteractionRejection::TabGestureSourceUnavailable { source })?;
                (root, node, contained.layer())
            }
        };
        if let Some(occluding) = plan
            .drop_occlusions()
            .iter()
            .filter(|occlusion| occlusion.layer() > layer && occlusion.region().contains(point))
            .max_by_key(|occlusion| occlusion.layer())
        {
            return Err(InteractionRejection::TabGestureOccluded {
                source,
                occluding: occluding.floating(),
            });
        }
        let source_node = self
            .workspace
            .capture_node_source(root, tabs)
            .map_err(InteractionRejection::CommandRejected)?;
        let contained = match self.workspace.presentation_for_root(root) {
            Some(crate::RootPresentationOwner::Main {
                surface: owner_surface,
            }) if owner_surface == surface => None,
            Some(crate::RootPresentationOwner::Contained {
                surface: owner_surface,
                floating,
            }) if owner_surface == surface => {
                let presented = plan
                    .contained_record(floating)
                    .filter(|record| record.root() == root && record.layer() == layer);
                let durable = self
                    .workspace
                    .contained_floating(floating)
                    .filter(|record| record.root == root);
                let (Some(presented), Some(durable)) = (presented, durable) else {
                    return Err(InteractionRejection::TabGestureSourceUnavailable { source });
                };
                let root_node = self
                    .workspace
                    .root(root)
                    .map(|record| record.node)
                    .ok_or(InteractionRejection::TabGestureSourceUnavailable { source })?;
                Some(PreparedContainedTabOrigin {
                    surface,
                    floating,
                    root_source: self
                        .workspace
                        .capture_node_source(root, root_node)
                        .map_err(InteractionRejection::CommandRejected)?,
                    source_rect: durable.rect,
                    minimum_size: presented.minimum_size(),
                    expected_roster: self
                        .workspace
                        .capture_contained_roster(surface)
                        .map_err(InteractionRejection::CommandRejected)?,
                })
            }
            _ => return Err(InteractionRejection::TabGestureSourceUnavailable { source }),
        };
        let source_geometry = contained.as_ref().map_or_else(
            || {
                plan.pane_records()
                    .iter()
                    .find(|pane| pane.id() == (crate::scene::PaneSceneId { root, tabs }))
                    .map(|pane| JournalDragSourceGeometry {
                        source_rect: pane.bounds(),
                        minimum_size: self
                            .presentation_authority
                            .presentation_config
                            .minimum_floating_size(),
                    })
            },
            |contained| {
                Some(JournalDragSourceGeometry {
                    source_rect: contained.source_rect,
                    minimum_size: contained.minimum_size,
                })
            },
        );
        let Some(source_geometry) = source_geometry else {
            return Err(InteractionRejection::TabGestureSourceUnavailable { source });
        };
        Ok(PreparedTabGesture {
            source,
            surface,
            root,
            tabs,
            source_node,
            source_geometry,
            contained,
            initial_pointer: point,
            presentation,
            source_layout_facts: plan.layout_facts().cloned().map(std::sync::Arc::new),
        })
    }

    pub(super) fn activate_prepared_journal_tab_gesture(
        &mut self,
        cause: ReductionCause,
        focus_causal: FocusCausalStamp,
        owner: GestureOwner,
        capture_authority: Authority<PointerCaptureOwner>,
        threshold_origin: JournalDragThresholdOrigin,
        prepared: PreparedTabGesture,
        policy: &DockPolicySnapshot,
        events: &mut Vec<WorkspaceEvent>,
    ) -> Result<InteractionOutcome, EngineError> {
        self.activate_prepared_tab_gesture(
            cause,
            focus_causal,
            owner,
            Some(capture_authority),
            Some(threshold_origin),
            prepared,
            policy,
            events,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn activate_prepared_tab_gesture(
        &mut self,
        cause: ReductionCause,
        focus_causal: FocusCausalStamp,
        owner: GestureOwner,
        capture_authority: Option<Authority<PointerCaptureOwner>>,
        threshold_origin: Option<JournalDragThresholdOrigin>,
        prepared: PreparedTabGesture,
        policy: &DockPolicySnapshot,
        events: &mut Vec<WorkspaceEvent>,
    ) -> Result<InteractionOutcome, EngineError> {
        if let Some(rejection) = self.pending_presentation_transition_gesture_rejection() {
            return Ok(InteractionOutcome::Rejected(rejection));
        }
        let source_payload = match prepared.source {
            TabGestureSource::Item(source) => match self.workspace.nodes.get(source.tabs) {
                Some(Node::Tabs { items, .. }) if items.contains(&source.item) => {
                    Ok(MovePayload::Item(ItemSource {
                        root: source.root,
                        tabs: source.tabs,
                        item: source.item,
                        fingerprint: prepared.source_node.fingerprint.clone(),
                    }))
                }
                Some(Node::Tabs { .. }) => Err(CommandError::ItemNotInTabs {
                    tabs: source.tabs,
                    item: source.item,
                }),
                Some(Node::Split { .. }) => Err(CommandError::NodeIsNotTabs { node: source.tabs }),
                None => Err(CommandError::MissingNode {
                    role: ReferenceRole::Source,
                    node: source.tabs,
                }),
            },
            TabGestureSource::Group(_) => {
                if matches!(
                    self.workspace.nodes.get(prepared.tabs),
                    Some(Node::Tabs { .. })
                ) {
                    Ok(MovePayload::Tabs(prepared.source_node.clone()))
                } else {
                    Err(CommandError::NodeIsNotTabs {
                        node: prepared.tabs,
                    })
                }
            }
            TabGestureSource::ContainedTitle { .. } => {
                Ok(MovePayload::Subtree(prepared.source_node.clone()))
            }
        };
        let source_payload = match source_payload {
            Ok(payload) => payload,
            Err(source) => {
                return Ok(InteractionOutcome::Rejected(
                    InteractionRejection::CommandRejected(source),
                ));
            }
        };
        if let Err(source) =
            authorize_captured_drag_source(&self.workspace, policy, &source_payload)
        {
            return Ok(InteractionOutcome::Rejected(
                InteractionRejection::CommandRejected(source),
            ));
        }
        let mut candidate_events = Vec::new();
        let mut commands = Vec::with_capacity(2);
        if let Some(contained) = &prepared.contained
            && contained.expected_roster.contained().last() != Some(&contained.floating)
        {
            commands.push(WorkspaceCommand::RaiseContained {
                source: contained.root_source.clone(),
                floating: contained.floating,
                expected_roster: contained.expected_roster.clone(),
            });
        }
        if let TabGestureSource::Item(source) = prepared.source {
            let already_selected = matches!(
                self.workspace.nodes.get(source.tabs),
                Some(Node::Tabs {
                    selected: Some(selected),
                    ..
                }) if *selected == source.item
            );
            if !already_selected {
                let selection =
                    match self
                        .workspace
                        .capture_item_source(source.root, source.tabs, source.item)
                    {
                        Ok(source) => source,
                        Err(error) => {
                            return Ok(InteractionOutcome::Rejected(
                                InteractionRejection::CommandRejected(error),
                            ));
                        }
                    };
                commands.push(WorkspaceCommand::Select { source: selection });
            }
        }
        if !commands.is_empty() {
            let mut workspace = self.clone_workspace_candidate();
            let report = match WorkspaceTransaction::from_commands(commands)
                .apply(&mut workspace, policy)
            {
                Ok(report) => report,
                Err(TransactionError::Command { source, .. }) if source.is_expected_rejection() => {
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
            if let Some(surface) = self.first_pending_presentation_source_mismatch(
                &workspace,
                WorkspacePublicationAuthority::Ordinary,
            ) {
                return Ok(InteractionOutcome::Rejected(
                    InteractionRejection::CommandRejected(
                        crate::error::CommandError::SurfaceLifecycleFrozen { surface },
                    ),
                ));
            }
            if let Some(surface) = self.first_workspace_publication_mismatch(&workspace, None, None)
            {
                return Ok(InteractionOutcome::Rejected(
                    InteractionRejection::CommandRejected(
                        crate::error::CommandError::SurfaceLifecycleFrozen { surface },
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
        if let TabGestureSource::Item(source) = prepared.source {
            if matches!(owner, GestureOwner::LocalResponse { .. }) {
                let native_guard = self.viewport_focus_binding(prepared.surface);
                let _ = self
                    .viewport_focus
                    .request_local_pane_focus(
                        prepared.surface,
                        source.item,
                        native_guard,
                        focus_causal,
                    )
                    .map_err(|source| EngineError::CausedViewportFocus {
                        cause: focus_causal.cause(),
                        source,
                    })?;
            } else if let Some(binding) = self
                .viewport
                .viewport(prepared.surface)
                .filter(|record| record.can_accept_activation())
                .map(crate::viewport_registry::ViewportRecord::binding)
            {
                let _ = self.start_viewport_activation(
                    ViewportActivationRequest::pointer_tab_gesture(
                        binding,
                        PanelFocus::Item(source.item),
                    ),
                    focus_causal,
                    &mut candidate_events,
                )?;
            }
        }
        let payload = match prepared.source {
            TabGestureSource::Item(source) => self
                .workspace
                .capture_item_source(source.root, source.tabs, source.item)
                .map(MovePayload::Item),
            TabGestureSource::Group(_) => self
                .workspace
                .capture_node_source(prepared.root, prepared.tabs)
                .map(MovePayload::Tabs),
            TabGestureSource::ContainedTitle { .. } => self
                .workspace
                .capture_node_source(prepared.root, prepared.tabs)
                .map(MovePayload::Subtree),
        };
        let payload = payload.map_err(|source| EngineError::PointerInteractionInvariant {
            cause,
            detail: format!("staged tab gesture lost its payload source: {source}"),
        })?;
        let drag_source = self.prepare_drag_source(&payload).map_err(|source| {
            EngineError::PointerInteractionInvariant {
                cause,
                detail: format!("staged tab gesture lost drag authority: {source:?}"),
            }
        })?;
        let origin = if drag_source.complete_root.is_some() {
            if let Some(contained) = &prepared.contained {
                let source_roster = self
                    .workspace
                    .capture_contained_roster(contained.surface)
                    .map_err(|source| EngineError::PointerInteractionInvariant {
                        cause,
                        detail: format!(
                            "staged contained tab gesture lost its source roster: {source}"
                        ),
                    })?;
                FrozenDragOrigin::Contained(FrozenContainedDragOrigin {
                    surface: contained.surface,
                    root: prepared.root,
                    floating: contained.floating,
                    source_rect: contained.source_rect,
                    source_roster,
                    initial_pointer: prepared.initial_pointer,
                    minimum_size: contained.minimum_size,
                })
            } else {
                FrozenDragOrigin::Workspace
            }
        } else {
            FrozenDragOrigin::Workspace
        };
        let continuation = prepared.presentation.presented().and_then(|presentation| {
            self.scene_gesture_continuation_draft(
                cause,
                owner,
                presentation,
                SceneGestureContinuationSource::Drag {
                    payload: payload.clone(),
                    source_surface: drag_source.source_surface,
                    complete_root: drag_source.complete_root.clone(),
                    origin: origin.clone(),
                    coordinates: self.capture_surface_coordinates(drag_source.source_surface),
                },
            )
        });
        let (session, replaced) = self
            .interaction
            .arm_drag(DragArmStart {
                epoch: self.version.epoch(),
                owner,
                journal_capture_authority: capture_authority,
                button: PointerButton::Primary,
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
                journal_source_geometry: Some(prepared.source_geometry),
                continuation,
            })
            .map_err(|source| EngineError::PointerInteractionInvariant {
                cause,
                detail: source.to_string(),
            })?;
        if replaced.is_some() {
            return Err(EngineError::ReductionCauseInvariant {
                detail: "tab gesture replaced an active gesture after busy validation",
            });
        }
        events.extend(candidate_events);
        Ok(InteractionOutcome::DragArmed { session, replaced })
    }

    pub(super) fn reduce_host_pointer_protocol(
        &mut self,
        tick: ReducerTickId,
        protocol: PreparedHostPointerProtocol,
        frozen_pointer_outputs: &BTreeMap<SurfaceId, PointerReceiverPresentedOutput>,
        frozen_pointer_presentations: &BTreeMap<SurfaceId, JournalSurfacePresentation>,
        policy: &DockPolicySnapshot,
        events: &mut Vec<WorkspaceEvent>,
        interaction_events: &mut Vec<InteractionEvent>,
    ) -> Result<Vec<crate::transition::ReducedPointerEdge>, EngineError> {
        let (stamp, provider, journal, candidates, receipts) = protocol.into_parts();
        // A SurfaceLocal lease freezes a native endpoint, including its window
        // incarnation. The host frame can span a platform or workspace
        // transition, so creation-time validation is not sufficient here.
        // Revalidate at every causal journal segment before any current output
        // or receipt can be joined to the old lease.
        self.validate_pointer_provider_scope(provider.scope())?;
        let prepared = self
            .pointer_journal
            .prepare_candidate(provider, journal)
            .map_err(|source| EngineError::PointerJournal { source })?;
        let desktop_hover_routes = prepared
            .journal()
            .edges()
            .iter()
            .filter_map(|edge| {
                edge.desktop_route().map(|route| {
                    (
                        edge.sequence(),
                        route.validate_against_registry(
                            self.authority_domain,
                            self.viewport.registry(),
                        ),
                    )
                })
            })
            .collect::<BTreeMap<_, _>>();
        let desktop_delivery_routes = prepared
            .journal()
            .edges()
            .iter()
            .filter_map(|edge| {
                validated_desktop_delivery_route(self, edge).map(|route| (edge.sequence(), route))
            })
            .collect::<BTreeMap<_, _>>();
        let validated_receipts = candidates
            .validate_against_presented_outputs(receipts, frozen_pointer_outputs)
            .map_err(|source| EngineError::PointerReceiverReceipt { source })?;
        let snapshot = JournalPresentationSnapshot::from_validated_receipts(
            provider,
            frozen_pointer_presentations,
            &validated_receipts,
        )
        .map_err(|source| EngineError::JournalPresentationSnapshot {
            detail: source.to_string(),
        })?;
        self.validate_pointer_receiver_geometry(
            provider,
            prepared.journal(),
            &validated_receipts,
            &snapshot,
            &desktop_hover_routes,
            &desktop_delivery_routes,
        )?;
        let commit = self
            .pointer_journal
            .commit_prepared(prepared)
            .map_err(|source| EngineError::PointerJournal { source })?;
        let receipts_by_sequence = validated_receipts
            .receipts()
            .iter()
            .map(|receipt| (receipt.candidate().sequence(), receipt))
            .collect::<BTreeMap<_, _>>();
        let accepted_by_sequence = commit
            .accepted_edges()
            .iter()
            .map(|accepted| (accepted.ticket().sequence(), accepted))
            .collect::<BTreeMap<_, _>>();
        let ordinal = stamp.ordinal();
        let mut reduced = Vec::with_capacity(commit.journal().len());
        for edge in commit.journal().edges() {
            let accepted = accepted_by_sequence.get(&edge.sequence()).copied().ok_or(
                EngineError::ReductionCauseInvariant {
                    detail: "committed journal edge has no accepted stream ticket",
                },
            )?;
            let receipt = receipts_by_sequence.get(&edge.sequence()).copied().ok_or(
                EngineError::ReductionCauseInvariant {
                    detail: "committed journal edge has no validated receiver receipt",
                },
            )?;
            if accepted.ticket().sequence() != edge.sequence()
                || accepted.stream().pointer() != edge.pointer()
            {
                return Err(EngineError::ReductionCauseInvariant {
                    detail: "accepted pointer identity does not match committed journal edge",
                });
            }
            let cause = ReductionCause::PointerEdge {
                tick,
                ordinal,
                stream: accepted.stream(),
                ticket: accepted.ticket(),
            };
            let focus_generation = self.last_focus_reducer_generation.checked_next().ok_or(
                EngineError::ReductionCauseInvariant {
                    detail: "pointer focus causal generation is exhausted",
                },
            )?;
            self.last_focus_reducer_generation = focus_generation;
            let focus_causal = FocusCausalStamp::new(focus_generation, cause);
            let mut interaction = self.reduce_journal_pointer_edge(
                cause,
                focus_causal,
                accepted.stream(),
                accepted.capture_authority_for_reduction(),
                edge,
                receipt,
                &snapshot,
                desktop_delivery_routes.get(&edge.sequence()).copied(),
                desktop_hover_routes.get(&edge.sequence()).copied(),
                policy,
                events,
                interaction_events,
            )?;
            if let Some(outcome) =
                self.cancel_revoked_presentation_caused(cause, interaction_events)?
            {
                interaction.push(outcome);
            }
            reduced.push(crate::transition::ReducedPointerEdge::new(
                tick,
                ordinal,
                accepted.stream(),
                accepted.ticket(),
                edge.clone(),
                accepted.button_authority_after(),
                accepted.capture_authority_after(),
                interaction,
            ));
        }
        Ok(reduced)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::{PlatformCapabilityReason, PlatformRequirement};

    #[test]
    fn unsupported_physical_drag_capability_has_a_typed_preview_status() {
        let capability = PlatformCapability::unsupported(
            PlatformRequirement::PointerHitTestControl,
            PlatformCapabilityReason::BackendUnsupported,
        );

        assert_eq!(
            DockEngine::physical_drag_preview_status(capability),
            PreviewResolutionStatus::NativeCapabilityUnavailable
        );
    }
}
