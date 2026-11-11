//! Device-independent pointer interaction transaction.

use super::contained_geometry::{contained_manipulation_point, contained_move_geometry};
use super::*;
use crate::intent::PointerId;

mod click;

impl DockEngine {
    fn update_gesture_magnetic_targeting(
        &mut self,
        cause: ReductionCause,
        owner: GestureOwner,
        enabled: bool,
        interaction_events: &mut Vec<InteractionEvent>,
    ) -> Result<bool, EngineError> {
        if self.interaction.active_owner() != Some(owner) {
            return Ok(false);
        }
        let changed = self.interaction.set_magnetic_targeting(enabled);
        if let InteractionStatus::Dragging { session } = self.interaction.status() {
            let magnetic = self
                .interaction
                .active_drag(session)
                .ok()
                .and_then(|drag| drag.target_acquisition())
                .is_some_and(|target| target.is_magnetic());
            if !enabled && magnetic {
                self.interaction
                    .clear_preview(session, PreviewResolutionStatus::KnownNone)
                    .map_err(|source| EngineError::PointerInteractionInvariant {
                        cause,
                        detail: format!("{source:?}"),
                    })?;
                self.interaction
                    .set_drop_affordance(session, None)
                    .map_err(|source| EngineError::PointerInteractionInvariant {
                        cause,
                        detail: format!("{source:?}"),
                    })?;
                interaction_events.push(InteractionEvent::new_caused(
                    cause,
                    self.version,
                    InteractionEventKind::PreviewCleared { session },
                ));
            }
        }
        Ok(changed)
    }

    pub(super) fn reduce_pointer_magnetic_targeting(
        &mut self,
        cause: ReductionCause,
        provider: PointerInputLease,
        pointer: PointerId,
        enabled: bool,
        interaction_events: &mut Vec<InteractionEvent>,
    ) -> Result<InputOutcome, EngineError> {
        let owner = self.interaction.active_owner().filter(|owner| {
            self.pointer_provider() == Some(provider)
                && owner.lease() == provider
                && owner.pointer() == pointer
                && matches!(
                    provider.scope(),
                    PointerProviderScope::HostLocal(_) | PointerProviderScope::SurfaceLocal(_)
                )
        });
        let changed = if let Some(owner) = owner {
            self.update_gesture_magnetic_targeting(cause, owner, enabled, interaction_events)?
        } else {
            false
        };
        Ok(InputOutcome::MagneticTargetingUpdated { enabled, changed })
    }

    pub(super) fn reduce_native_magnetic_targeting(
        &mut self,
        cause: ReductionCause,
        provider: BackendIngressLease,
        binding: ViewportBinding,
        enabled: bool,
        interaction_events: &mut Vec<InteractionEvent>,
    ) -> Result<InputOutcome, EngineError> {
        self.require_managed_input_provider(provider)?;
        let window = binding.window();
        let current = self
            .viewport
            .registry()
            .window(window)
            .is_some_and(|record| record.is_routeable());
        let keyboard_focus = self
            .viewport_focus
            .focus_observation()
            .is_some_and(|observation| {
                observation.focused()
                    == &Authority::Known(crate::viewport_focus::GlobalFocusedWindow::Dock(window))
            });
        let source_matches = self
            .interaction
            .active_presentation_authority()
            .is_some_and(|authority| {
                matches!(
                    authority.presented.endpoint(),
                    crate::presentation_observation::HostPresentationEndpoint::Native(source)
                        if source.window() == window
                )
            });
        let owner = self.interaction.active_owner().filter(|owner| {
            current
                && owner.lease() == provider.pointer_provider()
                && (keyboard_focus || source_matches)
        });
        let changed = if let Some(owner) = owner {
            self.update_gesture_magnetic_targeting(cause, owner, enabled, interaction_events)?
        } else {
            false
        };
        Ok(InputOutcome::MagneticTargetingUpdated { enabled, changed })
    }

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
        let owner = self
            .interaction
            .active_owner()
            .filter(|owner| owner.stream() == stream)
            .or_else(|| self.journal_gesture_owner(stream, edge, desktop_delivery_route));
        let Some(owner) = owner else {
            return match edge.kind() {
                PointerEdgeKind::Scrolled(scroll) => {
                    self.reduce_scroll_edge(cause, stream, edge, scroll, receipt, snapshot)
                }
                PointerEdgeKind::StreamCancelled(reason) => self.terminate_journal_stream(
                    cause,
                    stream,
                    if reason == crate::pointer_journal::PointerStreamCancelReason::Escape {
                        InteractionCancelReason::Escape
                    } else {
                        InteractionCancelReason::PointerStreamCancelled
                    },
                    ScrollTerminationReason::StreamCancelled,
                    interaction_events,
                ),
                PointerEdgeKind::StreamEnded | PointerEdgeKind::ContactEnded(_) => self
                    .terminate_journal_stream(
                        cause,
                        stream,
                        InteractionCancelReason::PointerStreamEnded,
                        ScrollTerminationReason::StreamEnded,
                        interaction_events,
                    ),
                _ => Ok(Vec::new()),
            };
        };
        let status_before = self.interaction.status();
        let matching_owner = self.interaction.active_owner() == Some(owner);
        if let PointerEdgeKind::Continuation(token) = edge.kind()
            && !self.journal_continuation_is_current(stream, token)
        {
            // A revoked timer cannot reinterpret an old lane or ask receivers
            // about a newer gesture. Its real native capture loss still counts.
            if matching_owner
                && matches!(owner.lease().scope(), PointerProviderScope::NativeJoined(_))
            {
                return self.reduce_journal_capture_change(
                    cause,
                    owner,
                    capture_authority_after,
                    interaction_events,
                );
            }
            return Ok(Vec::new());
        }
        if let PointerEdgeKind::CaptureAcquired(token) = edge.kind()
            && !self.journal_capture_acquisition_is_current(stream, token)
        {
            if matching_owner
                && matches!(owner.lease().scope(), PointerProviderScope::NativeJoined(_))
            {
                return self.reduce_journal_capture_change(
                    cause,
                    owner,
                    capture_authority_after,
                    interaction_events,
                );
            }
            return Ok(Vec::new());
        }
        if matching_owner
            && matches!(
                edge.kind(),
                PointerEdgeKind::Moved
                    | PointerEdgeKind::Continuation(_)
                    | PointerEdgeKind::CaptureAcquired(_)
                    | PointerEdgeKind::ButtonReleased(PointerButton::Primary)
                    | PointerEdgeKind::ContactEnded(PointerButton::Primary)
            )
            && (!self.journal_edge_matches_gesture_lane(owner, edge)
                || (matches!(owner.lane(), GestureLane::NativeLocal { .. })
                    && super::pointer_proof::validated_native_local_route(self, edge).is_none()))
        {
            let mut outcomes = self
                .cancel_gesture_owner(
                    cause,
                    owner,
                    InteractionCancelReason::CoordinateAuthorityUnavailable,
                    interaction_events,
                )?
                .into_iter()
                .collect::<Vec<_>>();
            if matches!(edge.kind(), PointerEdgeKind::ContactEnded(_)) {
                outcomes.extend(
                    self.terminate_scroll_stream(stream, ScrollTerminationReason::StreamEnded),
                );
            }
            return Ok(outcomes);
        }
        if matching_owner
            && !matches!(
                edge.kind(),
                PointerEdgeKind::ButtonPressed(PointerButton::Primary)
            )
            && let Some(enabled) = edge.metadata().magnetic_targeting()
        {
            self.update_gesture_magnetic_targeting(cause, owner, enabled, interaction_events)?;
        }
        let event_start = interaction_events.len();
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
                focus_causal,
                owner,
                edge,
                receipt,
                snapshot,
                desktop_hover_route,
                policy,
                events,
                interaction_events,
            ),
            PointerEdgeKind::Continuation(token) => self.reduce_journal_continuation(
                cause,
                owner,
                token,
                edge,
                receipt,
                snapshot,
                policy,
                events,
                interaction_events,
            ),
            PointerEdgeKind::CaptureAcquired(token) => {
                self.reduce_journal_capture_acquired(cause, owner, token, edge, interaction_events)
            }
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
            PointerEdgeKind::StreamCancelled(reason) => self.terminate_journal_stream(
                cause,
                stream,
                if reason == crate::pointer_journal::PointerStreamCancelReason::Escape {
                    InteractionCancelReason::Escape
                } else {
                    InteractionCancelReason::PointerStreamCancelled
                },
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
                let mut outcomes = if matching_owner
                    && matches!(owner.lease().scope(), PointerProviderScope::NativeJoined(_))
                {
                    self.reduce_journal_capture_change(
                        cause,
                        owner,
                        capture_authority_after,
                        interaction_events,
                    )?
                } else {
                    Vec::new()
                };
                outcomes.extend(
                    self.reduce_scroll_edge(cause, stream, edge, scroll, receipt, snapshot)?,
                );
                Ok(outcomes)
            }
            PointerEdgeKind::ButtonPressed(_) | PointerEdgeKind::ButtonReleased(_)
                if matching_owner
                    && matches!(owner.lease().scope(), PointerProviderScope::NativeJoined(_)) =>
            {
                self.reduce_journal_capture_change(
                    cause,
                    owner,
                    capture_authority_after,
                    interaction_events,
                )
            }
            PointerEdgeKind::ButtonPressed(_)
            | PointerEdgeKind::ButtonReleased(_)
            | PointerEdgeKind::ContactEnded(_) => Ok(Vec::new()),
        }?;
        if status_before == InteractionStatus::Idle
            && matches!(
                edge.kind(),
                PointerEdgeKind::ButtonPressed(PointerButton::Primary)
            )
            && self.interaction.active_owner() == Some(owner)
        {
            self.update_gesture_magnetic_targeting(
                cause,
                owner,
                edge.metadata().magnetic_targeting().unwrap_or(true),
                interaction_events,
            )?;
        }
        if matches!(edge.kind(), PointerEdgeKind::ContactEnded(_)) {
            outcomes.extend(self.terminate_journal_stream(
                cause,
                stream,
                InteractionCancelReason::PointerStreamEnded,
                ScrollTerminationReason::StreamEnded,
                interaction_events,
            )?);
        }
        for outcome in &outcomes {
            let kind = match outcome {
                InteractionOutcome::ReleasePending { session, .. } => {
                    Some(InteractionEventKind::ReleasePending { session: *session })
                }
                _ => None,
            };
            if let Some(kind) = kind {
                interaction_events.push(InteractionEvent::new_caused(cause, self.version, kind));
            }
        }
        let consumed_release = matching_owner
            && matches!(
                edge.kind(),
                PointerEdgeKind::ButtonReleased(PointerButton::Primary)
                    | PointerEdgeKind::ContactEnded(PointerButton::Primary)
            )
            && self.interaction.status() == InteractionStatus::Idle;
        let unowned_press = status_before == InteractionStatus::Idle
            && matches!(
                edge.kind(),
                PointerEdgeKind::ButtonPressed(PointerButton::Primary)
            );
        let has_terminal = interaction_events[event_start..].iter().any(|event| {
            matches!(
                event.kind(),
                InteractionEventKind::Cancelled { .. }
                    | InteractionEventKind::Delivered { .. }
                    | InteractionEventKind::ResizeDelivered { .. }
                    | InteractionEventKind::ContainedTransformDelivered { .. }
                    | InteractionEventKind::ReleasePending { .. }
                    | InteractionEventKind::NativePresentationRequested(_)
            )
        });
        if (consumed_release || unowned_press) && !has_terminal {
            let rejection = outcomes.iter().rev().find_map(|outcome| match outcome {
                InteractionOutcome::Rejected(reason) => Some(
                    crate::interaction::ManipulationRejection::from_interaction(reason),
                ),
                InteractionOutcome::ProductActionRejected(reason) => Some(
                    crate::interaction::ManipulationRejection::from_product(*reason),
                ),
                _ => None,
            });
            if let Some(reason) = rejection {
                interaction_events.push(InteractionEvent::new_caused(
                    cause,
                    self.version,
                    InteractionEventKind::Rejected {
                        status: status_before,
                        reason,
                    },
                ));
            }
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
        if let Some(owner) = self
            .interaction
            .active_owner()
            .filter(|owner| owner.stream() == stream)
        {
            if let Some(outcome) =
                self.cancel_gesture_owner(cause, owner, interaction_reason, interaction_events)?
            {
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
        let Some(point) = self.journal_logical_point(edge, desktop_delivery_route) else {
            return Ok(None);
        };
        let stream = owner.stream();
        let expected_delivery =
            match stream.lease().scope() {
                PointerProviderScope::SurfaceLocal(_) | PointerProviderScope::HostLocal(_) => {
                    PointerEventDeliveryOwner::ProviderEndpoint
                }
                PointerProviderScope::DesktopGlobal => {
                    let binding = desktop_delivery_route
                        .and_then(DesktopRouteValidation::dock_route)
                        .map(|route| route.binding())
                        .ok_or(EngineError::ReductionCauseInvariant {
                            detail: "desktop primary press has no exact hovered source binding",
                        })?;
                    PointerEventDeliveryOwner::Native(binding.window())
                }
                PointerProviderScope::NativeJoined(_) => {
                    let binding = match owner.lane() {
                    GestureLane::NativeLocal { binding, .. } => binding,
                    GestureLane::Desktop => desktop_delivery_route
                        .and_then(DesktopRouteValidation::dock_route)
                        .map(|route| route.binding().window())
                        .ok_or(EngineError::ReductionCauseInvariant {
                            detail: "joined desktop primary press has no native coordinate route",
                        })?,
                    _ => return Err(EngineError::ReductionCauseInvariant {
                        detail: "joined primary press has a non-native lane",
                    }),
                };
                    PointerEventDeliveryOwner::Native(binding)
                }
            };
        if let Some(rejection) =
            self.journal_press_delivery_rejection(edge.delivery_owner(), expected_delivery)?
        {
            return Ok(Some(InteractionOutcome::Rejected(rejection)));
        }
        if let Some(outcome) = self.dismiss_popup_from_other_surface_press(
            cause,
            owner,
            capture_authority,
            receipt,
            snapshot,
        )? {
            return Ok(Some(outcome));
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
            let outcome = self.arm_journal_click(
                cause,
                owner,
                capture_authority,
                presentation,
                region,
                point,
                policy,
            )?;
            if edge.metadata().device() == crate::pointer_journal::PointerDeviceKind::Touch
                && Self::pointer_timing_supported(owner)
                && let InteractionStatus::Pressed { session } = self.interaction.status()
            {
                let threshold_origin =
                    self.journal_drag_threshold_origin(edge, desktop_delivery_route)?;
                self.interaction
                    .active_click_mut(session)
                    .map_err(|error| EngineError::PointerInteractionInvariant {
                        cause,
                        detail: format!("{error:?}"),
                    })?
                    .touch = Some(TouchPressState {
                    initial_pointer: point,
                    current_pointer: point,
                    threshold_origin,
                    request: None,
                    armed: false,
                });
            }
            return Ok(outcome);
        }
        let threshold_origin = self.journal_drag_threshold_origin(edge, desktop_delivery_route)?;
        if edge.metadata().device() == crate::pointer_journal::PointerDeviceKind::Touch
            && Self::pointer_timing_supported(owner)
            && Self::touch_wait_region(region.kind())
        {
            return self.arm_journal_touch_press(
                cause,
                owner,
                capture_authority,
                presentation,
                region,
                point,
                threshold_origin,
                edge.metadata(),
            );
        }
        self.activate_journal_source_press(
            cause,
            focus_causal,
            SourcePressIntent::Activate,
            owner,
            capture_authority,
            threshold_origin,
            presentation,
            region,
            point,
            self.journal_pointer_hit_device(edge),
            policy,
            events,
        )
    }

    pub(super) fn activate_journal_source_press(
        &mut self,
        cause: ReductionCause,
        focus_causal: FocusCausalStamp,
        intent: SourcePressIntent,
        owner: GestureOwner,
        capture_authority: Authority<PointerCaptureOwner>,
        threshold_origin: JournalDragThresholdOrigin,
        presentation: &JournalSurfacePresentation,
        region: PresentationHitRegionId,
        point: LogicalPoint,
        device: crate::pointer_journal::PointerDeviceKind,
        policy: &DockPolicySnapshot,
        events: &mut Vec<WorkspaceEvent>,
    ) -> Result<Option<InteractionOutcome>, EngineError> {
        if !self.presented_receiver_is_current(presentation.plan(), region) {
            return Ok(Some(InteractionOutcome::Rejected(
                InteractionRejection::StaleScene,
            )));
        }
        match region.kind() {
            PresentationHitRegionKind::TabBody(tab) => {
                let source = TabGestureSource::Item(tab);
                let prepared = match self.prepare_journal_tab_gesture(presentation, source, point) {
                    Ok(prepared) => prepared,
                    Err(rejection) => return Ok(Some(InteractionOutcome::Rejected(rejection))),
                };
                self.activate_prepared_journal_tab_gesture(
                    cause,
                    focus_causal,
                    intent,
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
                let source = TabGestureSource::Group(group);
                let prepared = match self.prepare_journal_tab_gesture(presentation, source, point) {
                    Ok(prepared) => prepared,
                    Err(rejection) => return Ok(Some(InteractionOutcome::Rejected(rejection))),
                };
                self.activate_prepared_journal_tab_gesture(
                    cause,
                    focus_causal,
                    intent,
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
                    device,
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
                let prepared = match self.prepare_journal_contained_gesture(
                    presentation,
                    floating,
                    ContainedGestureKind::TitleDrag,
                    point,
                    device,
                ) {
                    Ok(prepared) => prepared,
                    Err(rejection) => return Ok(Some(InteractionOutcome::Rejected(rejection))),
                };
                self.activate_prepared_journal_contained_gesture(
                    cause,
                    intent,
                    owner,
                    capture_authority,
                    threshold_origin,
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
                    device,
                ) {
                    Ok(prepared) => prepared,
                    Err(rejection) => return Ok(Some(InteractionOutcome::Rejected(rejection))),
                };
                self.activate_prepared_journal_contained_gesture(
                    cause,
                    intent,
                    owner,
                    capture_authority,
                    threshold_origin,
                    prepared,
                    policy,
                    events,
                )
                .map(Some)
            }
            PresentationHitRegionKind::PaneBody(_)
            | PresentationHitRegionKind::ContainedFrameBlocker(_) => self
                .activate_journal_contained_content(
                    cause,
                    focus_causal,
                    presentation,
                    region,
                    point,
                    policy,
                    events,
                ),
            _ => Ok(None),
        }
    }

    fn activate_journal_contained_content(
        &mut self,
        cause: ReductionCause,
        focus_causal: FocusCausalStamp,
        presentation: &JournalSurfacePresentation,
        region: PresentationHitRegionId,
        point: LogicalPoint,
        policy: &DockPolicySnapshot,
        events: &mut Vec<WorkspaceEvent>,
    ) -> Result<Option<InteractionOutcome>, EngineError> {
        let plan = presentation.plan();
        let (contained, focus_item) = match region.kind() {
            PresentationHitRegionKind::PaneBody(pane) => (
                plan.contained_records()
                    .iter()
                    .find(|record| record.root() == pane.root),
                plan.pane_records()
                    .iter()
                    .find(|record| record.id() == pane)
                    .and_then(|record| plan.pane_selected(record)),
            ),
            PresentationHitRegionKind::ContainedFrameBlocker(floating) => {
                (plan.contained_record(floating), None)
            }
            _ => return Ok(None),
        };
        let Some(contained) = contained else {
            return Ok(None);
        };
        if contained.is_frontmost() {
            return Ok(None);
        }
        if !contained.outer_bounds().contains(point)
            || !plan.point_is_on_authoritative_layer(point, contained.layer())
        {
            return Ok(Some(InteractionOutcome::Rejected(
                InteractionRejection::ActionReceiverUnavailable {
                    target: region.kind(),
                },
            )));
        }
        let surface = presentation.surface();
        let floating = contained.floating();
        let root = contained.root();
        if self.root_has_pending_presentation_transition(root) {
            return Ok(Some(InteractionOutcome::Rejected(
                InteractionRejection::PresentationTransitionPending,
            )));
        }
        if self.workspace.presentation_for_root(root)
            != Some(crate::RootPresentationOwner::Contained { surface, floating })
        {
            return Ok(Some(InteractionOutcome::Rejected(
                InteractionRejection::StaleScene,
            )));
        }
        let command = self
            .workspace
            .root(root)
            .ok_or(CommandError::MissingRoot { root })
            .and_then(|record| self.workspace.capture_node_source(root, record.node))
            .and_then(|source| {
                self.workspace
                    .capture_contained_roster(surface)
                    .map(|expected_roster| WorkspaceCommand::RaiseContained {
                        source,
                        floating,
                        expected_roster,
                    })
            });
        let command = match command {
            Ok(command) => command,
            Err(error) => {
                return Ok(Some(InteractionOutcome::Rejected(
                    InteractionRejection::CommandRejected(error),
                )));
            }
        };
        let (_, changed) =
            match self.apply_journal_workspace_command(cause, &command, policy, events)? {
                Ok(result) => result,
                Err(error) => {
                    return Ok(Some(InteractionOutcome::Rejected(
                        InteractionRejection::CommandRejected(error),
                    )));
                }
            };
        if let Some(item) = focus_item {
            let native_guard = self.viewport_focus_binding(surface);
            self.viewport_focus
                .request_local_pane_focus(surface, item, native_guard, focus_causal)
                .map_err(|source| EngineError::CausedViewportFocus { cause, source })?;
        }
        Ok(Some(InteractionOutcome::ProductActionApplied(
            crate::model::DockspaceActionOutcome::ContainedRaised {
                root,
                surface,
                floating,
                changed,
            },
        )))
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
            PointerEdgeLocation::HostLocal { .. } => {
                let route = self.journal_host_local_manipulation(edge).ok_or(
                    EngineError::ReductionCauseInvariant {
                        detail: "host-local press has no authoritative manipulation route",
                    },
                )?;
                Ok(JournalDragThresholdOrigin::HostLocal {
                    surface: route.surface(),
                    position: route.position(),
                })
            }
            PointerEdgeLocation::NativeLocal { .. } => {
                let route = super::pointer_proof::validated_native_local_route(self, edge).ok_or(
                    EngineError::ReductionCauseInvariant {
                        detail: "native-local press has no current coordinate route",
                    },
                )?;
                Ok(JournalDragThresholdOrigin::NativeLocal {
                    binding: route.binding(),
                    coordinate_generation: route.coordinate_generation(),
                    position: route.surface_position(),
                })
            }
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
            }
            | PointerEdgeLocation::NativeLocalToDesktop { .. } => {
                Err(EngineError::ReductionCauseInvariant {
                    detail: "journal press receiver has no authoritative local point",
                })
            }
        }
    }

    fn journal_drag_threshold_crossed(
        &self,
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
                JournalDragThresholdOrigin::HostLocal {
                    surface,
                    position: initial,
                },
                PointerEdgeLocation::HostLocal { .. },
            ) => {
                let Some(current) = self
                    .journal_host_local_manipulation(edge)
                    .filter(|route| route.surface() == surface)
                else {
                    return false;
                };
                (
                    current.position().x() - initial.x(),
                    current.position().y() - initial.y(),
                    source_logical_threshold,
                )
            }
            (
                JournalDragThresholdOrigin::NativeLocal {
                    binding,
                    coordinate_generation,
                    position: initial,
                },
                PointerEdgeLocation::NativeLocal { .. },
            ) => {
                let Some(current) = super::pointer_proof::validated_native_local_route(self, edge)
                    .filter(|route| {
                        route.binding() == binding
                            && route.coordinate_generation() == coordinate_generation
                    })
                else {
                    return false;
                };
                (
                    current.surface_position().x() - initial.x(),
                    current.surface_position().y() - initial.y(),
                    source_logical_threshold,
                )
            }
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

    pub(super) fn reduce_journal_move(
        &mut self,
        cause: ReductionCause,
        focus_causal: FocusCausalStamp,
        owner: GestureOwner,
        edge: &PointerEdge,
        receipt: &ValidatedPointerReceiverReceipt,
        snapshot: &JournalPresentationSnapshot,
        desktop_hover_route: Option<DesktopRouteValidation>,
        policy: &DockPolicySnapshot,
        events: &mut Vec<WorkspaceEvent>,
        interaction_events: &mut Vec<InteractionEvent>,
    ) -> Result<Vec<InteractionOutcome>, EngineError> {
        let mut outcomes = Vec::new();
        if self.interaction.active_owner() != Some(owner) {
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
        if matches!(owner.lease().scope(), PointerProviderScope::NativeJoined(_))
            && !self.stationary_edge_has_authority(owner, edge)
        {
            self.clear_pointer_continuation(owner);
        }
        if let Some(outcome) = self.cancel_revoked_presentation_caused(cause, interaction_events)? {
            outcomes.push(outcome);
            return Ok(outcomes);
        }
        if let Some(touch_outcomes) = self.reduce_touch_move(
            cause,
            focus_causal,
            owner,
            edge,
            receipt,
            snapshot,
            desktop_hover_route,
            policy,
            events,
            interaction_events,
        )? {
            outcomes.extend(touch_outcomes);
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
            let threshold_origin = armed.journal_threshold_origin;
            let threshold = self
                .presentation_authority
                .presentation_config
                .pointer_drag_start_distance();
            if !self.journal_drag_threshold_crossed(threshold_origin, edge, threshold) {
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
                    if owner.is_desktop() {
                        let drag = self.interaction.active_drag(session).map_err(|_| {
                            EngineError::ReductionCauseInvariant {
                                detail: "new native drag lost its active payload",
                            }
                        })?;
                        let input = self.native_drag_source_input(drag);
                        if self
                            .viewport
                            .physical_cross_surface_drag_capability(input)
                            .is_supported()
                        {
                            let _ = self
                                .viewport
                                .begin_drag_routing(owner.pointer(), source_surface, input)
                                .map_err(|source| EngineError::Viewport {
                                    input: self.last_input,
                                    source,
                                })?;
                        }
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
        let stream = owner.stream();
        let position = journal_position_key(self, stream, edge);
        let evaluation = self.resolve_journal_drag_preview(
            cause,
            &drag,
            position,
            edge,
            receipt,
            snapshot,
            desktop_hover_route,
            policy,
        )?;
        if let Some(outcome) = self.apply_position_evaluation(
            cause,
            owner,
            session,
            position,
            evaluation,
            interaction_events,
        )? {
            outcomes.push(outcome);
        }
        self.advance_drag_continuation(cause, owner, session, edge, receipt, snapshot)?;
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
        if self.interaction.active_owner() != Some(owner) {
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
                if let Some(outcome) = self.cancel_gesture_owner(
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
            InteractionStatus::Armed { .. } => {
                if let Some(outcome) = self.cancel_gesture_owner(
                    cause,
                    owner,
                    InteractionCancelReason::ReleasedBeforeDrag,
                    interaction_events,
                )? {
                    outcomes.push(outcome);
                }
            }
            InteractionStatus::Dragging { session } => {
                let drag = self
                    .interaction
                    .active_drag(session)
                    .map_err(|source| EngineError::PointerInteractionInvariant {
                        cause,
                        detail: format!("{source:?}"),
                    })?
                    .clone();
                let stream = owner.stream();
                let position = journal_position_key(self, stream, edge);
                let mut revalidated_preview = None;
                let prepared_mutation;
                // Release is always resolved against its own edge facts. This
                // keeps same-point blocker/receiver changes from authorizing a
                // cached preview, while the outside-all native route remains
                // independently resolvable without a hover receipt.
                {
                    let mut evaluation = self.resolve_journal_drag_preview(
                        cause,
                        &drag,
                        position,
                        edge,
                        receipt,
                        snapshot,
                        desktop_hover_route,
                        policy,
                    )?;
                    prepared_mutation = evaluation.prepared_mutation.take();
                    if let Some(outcome) = self.apply_position_evaluation(
                        cause,
                        owner,
                        session,
                        position,
                        evaluation,
                        interaction_events,
                    )? {
                        if matches!(outcome, InteractionOutcome::PreviewUpdated { .. }) {
                            // A release-time preview refresh is an internal
                            // authority check. It becomes observable only if
                            // the terminal command must wait for that freshly
                            // painted preview.
                            revalidated_preview = Some(outcome);
                        } else {
                            outcomes.push(outcome);
                        }
                    }
                    if self.interaction.status() != (InteractionStatus::Dragging { session }) {
                        return Ok(outcomes);
                    }
                }
                let outcome = self.finish_drag_release_with_prepared_mutation(
                    cause,
                    focus_causal,
                    owner,
                    session,
                    prepared_mutation,
                    policy,
                    events,
                    interaction_events,
                )?;
                if matches!(outcome, InteractionOutcome::ReleasePending { .. }) {
                    if let Some(preview) = revalidated_preview {
                        outcomes.push(preview);
                    }
                }
                outcomes.push(outcome);
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

    #[allow(clippy::too_many_arguments)]
    pub(super) fn defer_drag_release(
        &mut self,
        cause: ReductionCause,
        focus_causal: FocusCausalStamp,
        session: crate::interaction::DragSessionId,
        drag: crate::interaction::ActiveDrag,
        physical_route: Option<(
            crate::viewport::ViewportBinding,
            crate::platform::NativeDragSourceInput,
        )>,
        release_decision: PreviewDecision,
        policy: &DockPolicySnapshot,
    ) -> Result<InteractionOutcome, EngineError> {
        if self.pending_mutation.is_some() {
            return Err(EngineError::PointerInteractionInvariant {
                cause,
                detail: "a drag release presentation obligation is already pending".to_owned(),
            });
        }
        let preview = drag
            .preview()
            .expect("deferred release has an exact matching preview")
            .public()
            .token();
        self.pending_mutation = Some(PendingMutation::DragRelease(PendingDragRelease {
            source_version: self.version,
            policy_revision: policy.revision(),
            cause,
            focus_causal,
            session,
            drag,
            physical_route,
            release_decision,
            preview,
            presentation: PendingPresentationTracker::default(),
        }));
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

    pub(super) fn frozen_drag_release_decision(
        drag: &crate::interaction::ActiveDrag,
    ) -> Option<PreviewDecision> {
        let preview = drag.preview()?;
        Some(PreviewDecision::Publish {
            scene: preview.public().token().scene(),
            visual: preview.public().visual().clone(),
            proof: Box::new(preview.proof().clone()),
        })
    }

    pub(super) fn drag_preview_authority_is_current(
        &self,
        drag: &crate::interaction::ActiveDrag,
    ) -> bool {
        let Some(preview) = drag.preview() else {
            return false;
        };
        if matches!(preview.proof(), PreviewProof::Native { .. }) {
            return true;
        }
        let scene = preview.public().token().scene();
        if self
            .interaction_projection(scene.surface())
            .is_some_and(|projection| {
                projection.plan_stamp() == scene
                    && (preview.target_layout_basis().is_none()
                        || (preview.target_layout_basis() == Some(scene)
                            && projection.plan().contained_projection().is_none()))
            })
        {
            return true;
        }
        self.presentation_authority
            .scene
            .ready_candidate(scene.surface())
            .is_some_and(|candidate| {
                candidate.stamp() == scene
                    || (candidate
                        .plan()
                        .contained_projection()
                        .is_some_and(|projection| {
                            projection.owner
                                == crate::scene::ContainedLayoutOwner::Drag(drag.session)
                                && projection.target_basis.is_some()
                        })
                        && preview.target_layout_basis()
                            == self.drag_target_layout_basis(scene.surface(), drag.session))
            })
    }

    pub(super) fn settle_presented_pending_drag_release(
        &mut self,
        events: &mut Vec<WorkspaceEvent>,
        interaction_events: &mut Vec<InteractionEvent>,
    ) -> Result<bool, EngineError> {
        let Some(pending) = self
            .pending_mutation
            .as_ref()
            .and_then(PendingMutation::drag_release)
        else {
            return Ok(false);
        };
        if pending.presentation.presented.is_none() && !pending.presentation.failed {
            return Ok(false);
        }
        let Some(PendingMutation::DragRelease(pending)) = self.pending_mutation.take() else {
            unreachable!("checked pending release remains present");
        };
        let cancellation = if self.version != pending.source_version {
            Some(InteractionCancelReason::WorkspaceChanged)
        } else if self.policy.revision() != pending.policy_revision {
            Some(InteractionCancelReason::PolicyChanged)
        } else if pending.presentation.failed {
            Some(InteractionCancelReason::SceneUnavailable)
        } else {
            None
        };
        if let Some(reason) = cancellation {
            interaction_events.push(InteractionEvent::new_caused(
                pending.cause,
                self.version,
                InteractionEventKind::Cancelled {
                    status: InteractionStatus::Dragging {
                        session: pending.session,
                    },
                    reason,
                },
            ));
            return Ok(true);
        }

        if !self.viewport.native_binding_allocation_available()
            && matches!(&pending.release_decision,
                PreviewDecision::Publish { proof, .. } if matches!(proof.as_ref(), PreviewProof::Native { .. }))
        {
            // A presented preview reserves its exact release, not a future
            // native lifetime. Capacity rejection is one terminal outcome;
            // replaying the accepted release cannot create more storage.
            let rejection = InteractionRejection::CommandRejected(
                crate::error::CommandError::SurfaceLifecycleFrozen {
                    surface: pending.drag.source_surface,
                },
            );
            interaction_events.push(InteractionEvent::new_caused(
                pending.cause,
                self.version,
                InteractionEventKind::Rejected {
                    status: InteractionStatus::Dragging {
                        session: pending.session,
                    },
                    reason: crate::interaction::ManipulationRejection::from_interaction(&rejection),
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
            None,
            &policy,
            events,
            interaction_events,
        )?;
        match outcome {
            InteractionOutcome::DragDelivered { .. } => Ok(true),
            InteractionOutcome::Rejected(rejection) => {
                interaction_events.push(InteractionEvent::new_caused(
                    cause,
                    self.version,
                    InteractionEventKind::Rejected {
                        status: InteractionStatus::Dragging {
                            session: pending.session,
                        },
                        reason: crate::interaction::ManipulationRejection::from_interaction(
                            &rejection,
                        ),
                    },
                ));
                Ok(true)
            }
            outcome => Err(EngineError::PointerInteractionInvariant {
                cause,
                detail: format!(
                    "presented release obligation ended without delivery or rejection: {outcome:?}"
                ),
            }),
        }
    }

    #[cfg(test)]
    #[allow(clippy::too_many_arguments)]
    pub(super) fn finish_drag_release_after_evaluation(
        &mut self,
        cause: ReductionCause,
        focus_causal: FocusCausalStamp,
        owner: GestureOwner,
        session: crate::interaction::DragSessionId,
        policy: &DockPolicySnapshot,
        events: &mut Vec<WorkspaceEvent>,
        interaction_events: &mut Vec<InteractionEvent>,
    ) -> Result<InteractionOutcome, EngineError> {
        self.finish_drag_release_with_prepared_mutation(
            cause,
            focus_causal,
            owner,
            session,
            None,
            policy,
            events,
            interaction_events,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn finish_drag_release_with_prepared_mutation(
        &mut self,
        cause: ReductionCause,
        focus_causal: FocusCausalStamp,
        owner: GestureOwner,
        session: crate::interaction::DragSessionId,
        prepared_mutation: Option<PreparedDropMutation>,
        policy: &DockPolicySnapshot,
        events: &mut Vec<WorkspaceEvent>,
        interaction_events: &mut Vec<InteractionEvent>,
    ) -> Result<InteractionOutcome, EngineError> {
        let drag = self
            .interaction
            .active_drag(session)
            .map_err(|source| EngineError::PointerInteractionInvariant {
                cause,
                detail: format!("release lost its active drag: {source:?}"),
            })?
            .clone();
        if drag.owner != owner {
            return Ok(InteractionOutcome::Rejected(
                InteractionRejection::SessionMismatch,
            ));
        }
        let cancellation = if !self.drag_source_presentation_allows_targeting(&drag) {
            Some(InteractionCancelReason::SceneUnavailable)
        } else if drag.preview().is_some() && !self.drag_preview_authority_is_current(&drag) {
            Some(InteractionCancelReason::UnknownTargetAuthority)
        } else if drag.preview().is_none() {
            match drag.preview_status() {
                PreviewResolutionStatus::UnknownAuthority => {
                    Some(InteractionCancelReason::UnknownTargetAuthority)
                }
                PreviewResolutionStatus::OpaqueBlocker => {
                    Some(InteractionCancelReason::OpaquePointerBlocker)
                }
                _ => None,
            }
        } else {
            None
        };
        if let Some(reason) = cancellation {
            return self
                .cancel_gesture_owner(cause, owner, reason, interaction_events)?
                .ok_or(EngineError::ReductionCauseInvariant {
                    detail: "validated release failed to cancel its owner",
                });
        }
        let release_decision = Self::frozen_drag_release_decision(&drag);
        let drag =
            match self
                .interaction
                .take_drag_for_release(session, owner, PointerButton::Primary)
            {
                Ok(drag) => drag,
                Err(error) => return Ok(InteractionOutcome::Rejected(error)),
            };
        let physical_route = if owner.is_desktop() {
            let route = self
                .viewport
                .drag_source(owner.pointer())
                .filter(|binding| binding.surface() == drag.source_surface)
                .zip(self.viewport.drag_source_input(owner.pointer()));
            self.viewport
                .end_drag_routing(owner.pointer())
                .map_err(|source| EngineError::Viewport {
                    input: self.last_input,
                    source,
                })?;
            route
        } else {
            None
        };
        let Some(release_decision) = release_decision else {
            return Ok(InteractionOutcome::Rejected(
                InteractionRejection::PreviewMissing,
            ));
        };
        let preview = drag
            .preview()
            .expect("frozen release decision owns its preview");
        if !Self::release_decision_matches_preview(preview, &release_decision) {
            return Ok(InteractionOutcome::Rejected(
                InteractionRejection::TargetChanged,
            ));
        }
        if !preview.painted() && self.drag_release_has_native_resource_obligation(&drag) {
            return self.defer_drag_release(
                cause,
                focus_causal,
                session,
                drag,
                physical_route,
                release_decision,
                policy,
            );
        }
        self.deliver_drag_release(
            cause,
            focus_causal,
            session,
            drag,
            release_decision,
            prepared_mutation,
            policy,
            events,
            interaction_events,
        )
    }

    fn drag_release_has_native_resource_obligation(
        &self,
        drag: &crate::interaction::ActiveDrag,
    ) -> bool {
        let Some(preview) = drag.preview() else {
            return false;
        };
        if matches!(preview.proof(), PreviewProof::Native { .. }) {
            return true;
        }
        if preview.public().visual().surface() == drag.source_surface
            || !self
                .bound_surface_recoveries
                .contains_key(&drag.source_surface)
        {
            return false;
        }
        let source_items = self.surface_items(drag.source_surface);
        let moved_items = match &drag.payload {
            MovePayload::Item(source) => BTreeSet::from([source.item()]),
            MovePayload::Tabs(source) | MovePayload::Subtree(source) => self
                .workspace
                .collect_items_in_subtree(source.node())
                .into_iter()
                .collect(),
        };
        source_items == moved_items
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn deliver_drag_release(
        &mut self,
        cause: ReductionCause,
        focus_causal: FocusCausalStamp,
        session: crate::interaction::DragSessionId,
        drag: crate::interaction::ActiveDrag,
        release_decision: PreviewDecision,
        prepared_mutation: Option<PreparedDropMutation>,
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
            Ok(pane_focus) => match &drag.payload {
                MovePayload::Item(source) => PaneFocusDisposition::Set(source.item()),
                MovePayload::Tabs(_) | MovePayload::Subtree(_) => pane_focus,
            },
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
                let presentation = drag.presentation;
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
        let activation = match &drag.payload {
            MovePayload::Item(source) => JournalDeliveryActivation::SelectItem {
                item: source.item(),
                source_root: source.root(),
            },
            MovePayload::Tabs(_) | MovePayload::Subtree(_) => JournalDeliveryActivation::None,
        };
        let (outcome, changed) = match self.apply_journal_workspace_command_with_authority(
            cause,
            &command,
            policy,
            WorkspacePublicationAuthority::Ordinary,
            activation,
            prepared_mutation,
            events,
        )? {
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
        } else if let PaneFocusDisposition::Set(item) = pane_focus {
            self.request_product_item_focus(item, focus_causal)?;
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
        position: ManipulationPositionKey,
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
            return Ok(PreviewEvaluation::outside_all(decision));
        }
        let Some(edge_point) = position.logical_point() else {
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
        if presentation.surface() != drag.source_surface && drag.owner.is_desktop() {
            let capability = self
                .viewport
                .physical_cross_surface_drag_capability(self.native_drag_source_input(drag));
            if !capability.is_supported() || !self.physical_drag_has_route(drag, edge) {
                return Ok(PreviewEvaluation::without_affordance(
                    PreviewDecision::Clear(Self::physical_drag_preview_status(capability)),
                ));
            }
        }
        Self::validate_journal_desktop_presentation(edge, desktop_route, authority)?;
        self.validate_journal_native_local_presentation(edge, authority)?;
        let winner = presentation
            .hit_manifest()
            .resolve_exclusive_with_suppression(
                PresentationPointerLane::HoverDrop,
                point,
                self.hover_drop_suppression(),
            )
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
        if claimed
            .is_some_and(|region| !self.presented_receiver_is_current(presentation.plan(), region))
        {
            return Ok(PreviewEvaluation::without_affordance(
                PreviewDecision::Clear(PreviewResolutionStatus::Rejected),
            ));
        }
        // Framework clearance and the source-suppressed semantic winner are
        // proven independently. The existing solver remains the only target
        // policy; adapters never reconstruct payload membership.
        let query = self
            .resolve_presented_drop_with_current_index(
                &presentation,
                drag.source_layout_facts.as_deref(),
                policy,
                drag.session,
                drag.payload.clone(),
                drag.surface_background_offer,
                point,
                self.interaction.magnetic_targeting_enabled() && drag.magnetic_targeting_allowed(),
            )
            .map_err(|source| EngineError::PointerInteractionInvariant {
                cause,
                detail: source.to_string(),
            })?;
        let (resolution, affordance, observation) = query.into_evaluation_parts();
        let mut prepared_mutation = None;
        let decision = match resolution {
            DropResolution::Resolved(resolved) => {
                if resolved.session() != drag.session || resolved.source() != &drag.payload {
                    return Err(EngineError::PointerInteractionInvariant {
                        cause,
                        detail: "drop resolver changed the frozen journal drag identity".to_owned(),
                    });
                }
                let target = resolved.target_id();
                let valid = presentation
                    .hit_manifest()
                    .region_for_kind(PresentationHitRegionKind::DropTarget(target))
                    .is_some_and(|region| {
                        self.presented_receiver_is_current(presentation.plan(), region.id())
                    });
                if !valid {
                    return Ok(PreviewEvaluation::without_affordance(
                        PreviewDecision::Clear(PreviewResolutionStatus::Rejected),
                    ));
                }
                let scene = resolved.scene_stamp();
                let rect = resolved.visual().rect();
                let command = resolved.command().clone();
                prepared_mutation = Some(resolved.into_prepared_mutation());
                PreviewDecision::Publish {
                    scene,
                    visual: PreviewVisual::Dock {
                        surface: presentation.surface(),
                        target,
                        rect,
                    },
                    proof: Box::new(PreviewProof::Dock { target, command }),
                }
            }
            DropResolution::KnownNone(_) => {
                let (decision, mutation) = self.resolve_journal_local_contained_candidate(
                    cause,
                    drag,
                    position,
                    SurfacePointer::new(presentation.surface(), point),
                    policy,
                )?;
                prepared_mutation = mutation;
                decision
            }
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
        let target_layout_basis = presentation
            .plan()
            .contained_projection()
            .filter(|projection| {
                projection.owner == crate::scene::ContainedLayoutOwner::Drag(drag.session)
            })
            .and_then(|projection| projection.target_basis)
            .unwrap_or(presentation.scene());
        let affordance = affordance.map(|mut affordance| {
            affordance.bind_target_layout_basis(target_layout_basis);
            affordance
        });
        let mut evaluation =
            PreviewEvaluation::new(decision, affordance, observation, Some(target_layout_basis));
        evaluation.prepared_mutation = prepared_mutation;
        Ok(evaluation)
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
        allow_magnetic: bool,
    ) -> Result<crate::drop_resolver::DropQuery, DropResolutionError> {
        resolve_presented_drop_with_magnetic(
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
            allow_magnetic,
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
        let pointer_provider = drag.owner.lease();
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
        let source_input = self.native_drag_source_input(drag);
        let capability = self.viewport.physical_native_drag_capability(source_input);
        if !capability.is_supported() || !self.physical_drag_has_route(drag, edge) {
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
            source_input,
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

    fn physical_drag_has_route(
        &self,
        drag: &crate::interaction::ActiveDrag,
        edge: &PointerEdge,
    ) -> bool {
        let Some(binding) = self.viewport.confirmed_drag_source(drag.owner.pointer()) else {
            return false;
        };
        if binding.surface() != drag.source_surface {
            return false;
        }
        if self.viewport.drag_source_input(drag.owner.pointer())
            == Some(crate::platform::NativeDragSourceInput::Preserve)
        {
            // Input-state observation cannot replace the current edge's independent
            // delivery/capture facts. Release keeps its validated pre-edge capture.
            edge.delivery_owner()
                == Authority::Known(PointerEventDeliveryOwner::Native(binding.window()))
                && matches!(self.interaction.active_journal_capture_authority(),
                    Some(Authority::Known(PointerCaptureOwner::Native(current))) if current == binding.window())
        } else {
            true
        }
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
        position: ManipulationPositionKey,
        pointer: SurfacePointer,
        policy: &DockPolicySnapshot,
    ) -> Result<(PreviewDecision, Option<PreparedDropMutation>), EngineError> {
        let candidate = match &drag.origin {
            FrozenDragOrigin::Contained(origin) => {
                self.core_contained_move_candidate(drag, origin, position, pointer.surface())
            }
            FrozenDragOrigin::Workspace => self.core_contained_candidate(drag, pointer, None),
        };
        match candidate {
            CoreContainedCandidate::None => Ok((
                PreviewDecision::Clear(PreviewResolutionStatus::KnownNone),
                None,
            )),
            CoreContainedCandidate::Proposal(proposal) => self.resolve_contained_proposal(
                cause,
                drag,
                proposal,
                ContainedPlacementAuthority::PresentedCoordinates,
                policy,
            ),
            CoreContainedCandidate::Rejected => Ok((
                PreviewDecision::Clear(PreviewResolutionStatus::Rejected),
                None,
            )),
            CoreContainedCandidate::Cancel(reason) => Ok((PreviewDecision::Cancel(reason), None)),
        }
    }

    fn core_contained_move_candidate(
        &self,
        drag: &crate::interaction::ActiveDrag,
        origin: &FrozenContainedDragOrigin,
        position: ManipulationPositionKey,
        destination: SurfaceId,
    ) -> CoreContainedCandidate {
        if destination != origin.surface {
            return CoreContainedCandidate::None;
        }
        if !self.frozen_drag_origin_is_current(drag.origin.clone()) {
            return CoreContainedCandidate::Rejected;
        }
        let Some(point) = contained_manipulation_point(position, origin.surface) else {
            return CoreContainedCandidate::None;
        };
        let (scene, bounds) =
            match Self::ready_surface_bounds(self.scene(), self.version, origin.surface) {
                Ok(ready) => ready,
                Err(
                    ContainedPlacementUnavailable::StaleSurface { .. }
                    | ContainedPlacementUnavailable::PendingPaintSurface { .. },
                ) => return CoreContainedCandidate::None,
                Err(_) => {
                    return CoreContainedCandidate::Cancel(
                        InteractionCancelReason::SceneUnavailable,
                    );
                }
            };
        let previous = drag.preview().and_then(|preview| match preview.proof() {
            PreviewProof::Contained { proposal, .. }
                if proposal.surface() == origin.surface
                    && proposal.root() == origin.root
                    && proposal.floating() == origin.floating
                    && proposal.placement().surface_bounds() == bounds
                    && drag.last_advanced_position().and_then(|previous| {
                        contained_manipulation_point(previous, origin.surface)
                    }) == Some(point) =>
            {
                Some((proposal.placement().requested_rect(), proposal.rect()))
            }
            _ => None,
        });
        let (requested, rect) = match previous.map(Ok).unwrap_or_else(|| {
            contained_move_geometry(bounds, origin.source_rect, origin.initial_pointer, point)
        }) {
            Ok(geometry) => geometry,
            Err(()) => return CoreContainedCandidate::Rejected,
        };
        let placement = ContainedPlacementProof::new(
            scene,
            origin.surface,
            requested,
            origin.minimum_size,
            bounds,
            rect,
        );
        CoreContainedCandidate::Proposal(crate::intent::ContainedTearOffProposal::new(
            origin.root,
            origin.floating,
            placement,
            ContainedPosition::Front,
        ))
    }

    fn valid_contained_move_proposal(
        &self,
        origin: &FrozenContainedDragOrigin,
        proposal: crate::intent::ContainedTearOffProposal,
        authority: ContainedPlacementAuthority,
    ) -> bool {
        let proof = proposal.placement();
        let current = match authority {
            ContainedPlacementAuthority::PresentedCoordinates => {
                self.scene().ready_surface(proof.surface())
            }
            ContainedPlacementAuthority::CurrentCandidate => {
                self.semantic_candidate(proof.scene()).ok()
            }
        };
        let Some(current) = current else {
            return false;
        };
        // Existing movement has its own sizing rule, with the same exact scene,
        // epoch and bounds checks as placement. An oversized source stays intact.
        current.stamp() == proof.scene()
            && proof.scene().requirement().workspace_epoch() == self.version.epoch()
            && current.plan().bounds() == proof.surface_bounds()
            && proof.surface() == origin.surface
            && proof.minimum_size() == origin.minimum_size
            && proof.requested_rect().width() == origin.source_rect.width()
            && proof.requested_rect().height() == origin.source_rect.height()
            && clamp_moved_contained_rect(proof.surface_bounds(), proof.requested_rect())
                == Ok(proof.clamped_rect())
    }

    pub(super) fn resolve_contained_proposal(
        &self,
        cause: ReductionCause,
        drag: &crate::interaction::ActiveDrag,
        proposal: crate::intent::ContainedTearOffProposal,
        authority: ContainedPlacementAuthority,
        policy: &DockPolicySnapshot,
    ) -> Result<(PreviewDecision, Option<PreparedDropMutation>), EngineError> {
        let Some(command) = self.contained_presentation_command(
            &drag.payload,
            proposal,
            drag.complete_root.clone(),
            match &drag.origin {
                FrozenDragOrigin::Contained(origin) => Some(origin),
                FrozenDragOrigin::Workspace => None,
            },
        ) else {
            return Ok((
                PreviewDecision::Clear(PreviewResolutionStatus::Rejected),
                None,
            ));
        };
        let valid = match &drag.origin {
            FrozenDragOrigin::Contained(origin) => {
                self.valid_contained_move_proposal(origin, proposal, authority)
                    && self.valid_contained_command_structure(drag, proposal, &command)
            }
            FrozenDragOrigin::Workspace => {
                self.valid_core_contained_command(drag, proposal, &command, authority)
            }
        };
        if !valid {
            return Ok((
                PreviewDecision::Clear(PreviewResolutionStatus::Rejected),
                None,
            ));
        }
        let prepared = match crate::operation::prepare_transaction(
            self.workspace.as_ref(),
            policy,
            std::slice::from_ref(&command),
        ) {
            Ok(prepared) => prepared,
            Err(TransactionError::Command { index: 0, source })
                if source.is_expected_rejection() =>
            {
                return Ok((
                    PreviewDecision::Clear(PreviewResolutionStatus::Rejected),
                    None,
                ));
            }
            Err(source) => {
                return Err(EngineError::PointerInteractionInvariant {
                    cause,
                    detail: source.to_string(),
                });
            }
        };
        let (workspace, report) = prepared.into_parts();
        let staged = match self.stage_journal_workspace_draft(
            cause,
            policy,
            WorkspacePublicationAuthority::Ordinary,
            JournalDeliveryActivation::None,
            workspace,
            &report,
        )? {
            Ok(staged) => staged,
            Err(_) => {
                return Ok((
                    PreviewDecision::Clear(PreviewResolutionStatus::Rejected),
                    None,
                ));
            }
        };
        let mutation = PreparedDropMutation::new(
            self.version,
            policy.revision(),
            command.clone(),
            staged.publication.workspace.into_workspace(),
            report,
        );
        Ok((
            PreviewDecision::Publish {
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
            },
            Some(mutation),
        ))
    }

    pub(super) fn apply_preview_evaluation(
        &mut self,
        cause: ReductionCause,
        owner: GestureOwner,
        session: crate::interaction::DragSessionId,
        evaluation: PreviewEvaluation,
        interaction_events: &mut Vec<InteractionEvent>,
    ) -> Result<Option<InteractionOutcome>, EngineError> {
        let PreviewEvaluation {
            decision,
            affordance,
            observation,
            target_layout_basis,
            prepared_mutation: _,
        } = evaluation;
        let mut acquisition = observation.acquisition();
        let mut decision = decision;
        if let PreviewDecision::Publish { visual, proof, .. } = &decision
            && let Ok(drag) = self.interaction.active_drag(session)
            && self.current_contained_projection_is_owned(drag.source_surface)
            && self.drag_preview_authority_is_current(drag)
            && let Some(previous) = drag.preview()
            && previous.public().visual() == visual
            && previous.target_layout_basis() == target_layout_basis
        {
            let same_command = match (previous.proof(), proof.as_ref()) {
                (
                    PreviewProof::Dock {
                        target: old_target,
                        command: old,
                    },
                    PreviewProof::Dock { target, command },
                ) => old_target == target && old == command,
                (
                    PreviewProof::Contained {
                        command: old,
                        fallback: old_fallback,
                        ..
                    },
                    PreviewProof::Contained {
                        command, fallback, ..
                    },
                ) => old == command && old_fallback == fallback,
                _ => false,
            };
            if same_command {
                // Revalidate against the current hover winner, but retain the exact
                // already-presented token when only its source layout advanced.
                acquisition = drag.target.acquisition();
                decision = Self::frozen_drag_release_decision(drag)
                    .expect("the matching previous preview is present");
            }
        }
        self.interaction
            .set_drop_affordance(session, affordance)
            .map_err(|source| EngineError::PointerInteractionInvariant {
                cause,
                detail: format!("{source:?}"),
            })?;
        match decision {
            PreviewDecision::Publish {
                scene,
                visual,
                proof,
            } => {
                let (preview, changed) = self
                    .interaction
                    .publish_preview(session, scene, visual, *proof, acquisition)
                    .map_err(|source| EngineError::PointerInteractionInvariant {
                        cause,
                        detail: format!("{source:?}"),
                    })?;
                self.interaction
                    .bind_preview_layout_basis(session, target_layout_basis)
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
                let changed =
                    self.interaction
                        .clear_preview(session, status)
                        .map_err(|source| EngineError::PointerInteractionInvariant {
                            cause,
                            detail: format!("{source:?}"),
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
                self.cancel_gesture_owner(cause, owner, reason, interaction_events)
            }
        }
    }

    pub(super) fn apply_position_evaluation(
        &mut self,
        cause: ReductionCause,
        owner: GestureOwner,
        session: crate::interaction::DragSessionId,
        position: ManipulationPositionKey,
        evaluation: PreviewEvaluation,
        interaction_events: &mut Vec<InteractionEvent>,
    ) -> Result<Option<InteractionOutcome>, EngineError> {
        let PreviewEvaluation {
            decision,
            mut affordance,
            observation,
            target_layout_basis,
            prepared_mutation: _,
        } = evaluation;
        let latch_observation = match observation {
            crate::runtime::direct_manipulation::TargetObservation::Surface {
                layer,
                candidate,
                ..
            } if self.interaction.active_drag(session).is_ok_and(|drag| {
                self.current_contained_projection_is_owned(drag.source_surface)
                    && self.drag_preview_authority_is_current(drag)
                    && drag
                        .preview()
                        .is_some_and(|preview| preview.target_layout_basis() == target_layout_basis)
            }) =>
            {
                let drag = self
                    .interaction
                    .active_drag(session)
                    .expect("the exact source projection owner was checked");
                crate::runtime::direct_manipulation::TargetObservation::Surface {
                    scene: drag
                        .preview()
                        .expect("the target preview was checked")
                        .public()
                        .token()
                        .scene(),
                    layer,
                    candidate,
                }
            }
            _ => observation,
        };
        let latch = self
            .interaction
            .reconcile_drag_target(
                session,
                latch_observation,
                position.logical_point(),
                self.presentation_config().target_release_hysteresis(),
            )
            .map_err(|source| EngineError::PointerInteractionInvariant {
                cause,
                detail: format!("drag target latch rejected the position: {source:?}"),
            })?;
        let outcome = match latch {
            crate::runtime::direct_manipulation::TargetLatchDecision::AcceptCandidate => self
                .apply_preview_evaluation(
                    cause,
                    owner,
                    session,
                    PreviewEvaluation::new(decision, affordance, observation, target_layout_basis),
                    interaction_events,
                )?,
            crate::runtime::direct_manipulation::TargetLatchDecision::RetainCurrent => {
                let drag = self.interaction.active_drag(session).map_err(|source| {
                    EngineError::PointerInteractionInvariant {
                        cause,
                        detail: format!("latched drag target disappeared: {source:?}"),
                    }
                })?;
                let target = drag.target_acquisition().ok_or_else(|| {
                    EngineError::PointerInteractionInvariant {
                        cause,
                        detail: "latched drag retained no target acquisition".to_owned(),
                    }
                })?;
                let retained_target_is_visible = affordance
                    .as_mut()
                    .is_some_and(|next| next.activate_target(target.target()));
                if !retained_target_is_visible {
                    affordance = drag.drop_affordance().cloned();
                }
                let preview = drag.preview().map(|preview| preview.public().clone());
                let status = drag.preview_status();
                self.interaction
                    .set_drop_affordance(session, affordance)
                    .map_err(|source| EngineError::PointerInteractionInvariant {
                        cause,
                        detail: format!("latched drag lost its affordance: {source:?}"),
                    })?;
                Some(InteractionOutcome::PreviewUpdated {
                    session,
                    preview,
                    status,
                })
            }
        };
        if self.interaction.status() == (InteractionStatus::Dragging { session }) {
            self.interaction
                .set_drag_position(session, position)
                .map_err(|source| EngineError::PointerInteractionInvariant {
                    cause,
                    detail: format!("drag lost its exact advanced position: {source:?}"),
                })?;
        }
        Ok(outcome)
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
            Self::freeze_journal_presentation(presentation),
        )
    }

    fn prepare_tab_gesture(
        &self,
        surface: SurfaceId,
        scene: SurfaceSceneStamp,
        plan: &crate::scene::PresentationPlan,
        source: TabGestureSource,
        point: crate::geometry::LogicalPoint,
        presentation: FrozenPresentationAuthority,
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
                if plan.unclipped_contained_bounds(floating) != Some(durable.rect) {
                    return Err(InteractionRejection::TabGestureSourceUnavailable { source });
                }
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

    #[allow(clippy::too_many_arguments)]
    pub(super) fn activate_prepared_journal_tab_gesture(
        &mut self,
        cause: ReductionCause,
        focus_causal: FocusCausalStamp,
        intent: SourcePressIntent,
        owner: GestureOwner,
        capture_authority: Authority<PointerCaptureOwner>,
        threshold_origin: JournalDragThresholdOrigin,
        prepared: PreparedTabGesture,
        policy: &DockPolicySnapshot,
        events: &mut Vec<WorkspaceEvent>,
    ) -> Result<InteractionOutcome, EngineError> {
        if let Some(rejection) = self.pending_presentation_transition_gesture_rejection() {
            return Ok(InteractionOutcome::Rejected(rejection));
        }
        let source_payload = match prepared.source {
            TabGestureSource::Item(source) => match self.workspace.node(source.tabs) {
                Some(Node::Tabs { items, .. }) if items.contains(&source.item) => {
                    Ok(MovePayload::Item(ItemSource {
                        root: source.root,
                        tabs: source.tabs,
                        item: source.item,
                        reference: prepared.source_node.reference.clone(),
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
                if matches!(self.workspace.node(prepared.tabs), Some(Node::Tabs { .. })) {
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
        let can_arm = match authorize_captured_drag_source(&self.workspace, policy, &source_payload)
        {
            Ok(()) => true,
            // Selecting a tab and focusing its content do not require docking permission.
            Err(CommandError::Policy(_))
                if intent == SourcePressIntent::Activate
                    && matches!(prepared.source, TabGestureSource::Item(_)) =>
            {
                false
            }
            Err(source) => {
                return Ok(InteractionOutcome::Rejected(
                    InteractionRejection::CommandRejected(source),
                ));
            }
        };
        let mut candidate_events = Vec::new();
        let mut commands = Vec::with_capacity(2);
        let mut selection_changed = false;
        if let Some(contained) = &prepared.contained
            && contained.expected_roster.contained().last() != Some(&contained.floating)
        {
            let raise = WorkspaceCommand::RaiseContained {
                source: contained.root_source.clone(),
                floating: contained.floating,
                expected_roster: contained.expected_roster.clone(),
            };
            if intent == SourcePressIntent::Activate {
                commands.push(raise);
            } else if let Err(source) =
                crate::operation::authorize_workspace_command(&self.workspace, policy, &raise)
            {
                return Ok(InteractionOutcome::Rejected(
                    InteractionRejection::CommandRejected(source),
                ));
            }
        }
        if intent == SourcePressIntent::Activate
            && let TabGestureSource::Item(source) = prepared.source
        {
            let already_selected = matches!(
                self.workspace.node(source.tabs),
                Some(Node::Tabs {
                    selected: Some(selected),
                    ..
                }) if *selected == source.item
            );
            if !already_selected {
                selection_changed = true;
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
            let prepared = match self.workspace.prepare_transaction(policy, &commands) {
                Ok(prepared) => prepared,
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
            let (workspace, report) = prepared.into_snapshot_parts();
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
                if let [WorkspaceCommand::Select { source }] = commands.as_slice() {
                    self.advance_selection_revision_caused(cause, source)?;
                } else {
                    self.advance_revision_caused(cause)?;
                }
                candidate_events.extend(outcomes.into_iter().map(|outcome| {
                    WorkspaceEvent::new_caused(
                        cause,
                        self.version,
                        WorkspaceEventKind::CommandCommitted(outcome),
                    )
                }));
            }
        }
        if intent == SourcePressIntent::Activate
            && let TabGestureSource::Item(source) = prepared.source
        {
            if let Some(binding) = self
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
            } else {
                let native_guard = self.viewport_focus_binding(prepared.surface);
                self.viewport_focus
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
            }
            if !can_arm {
                events.extend(candidate_events);
                return Ok(InteractionOutcome::ProductActionApplied(
                    crate::model::DockspaceActionOutcome::Selected {
                        item: source.item,
                        changed: selection_changed,
                    },
                ));
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
                    intent: match prepared.source {
                        TabGestureSource::ContainedTitle { .. } => ContainedDragIntent::MoveWindow,
                        TabGestureSource::Item(_) | TabGestureSource::Group(_) => {
                            ContainedDragIntent::DockContent
                        }
                    },
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
        let continuation = self.scene_gesture_continuation_draft(
            cause,
            owner,
            prepared.presentation,
            SceneGestureContinuationSource::Drag {
                payload: payload.clone(),
                source_surface: drag_source.source_surface,
                complete_root: drag_source.complete_root.clone(),
                origin: origin.clone(),
                coordinates: self.capture_surface_coordinates(drag_source.source_surface),
            },
        );
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
        let (stamp, provider, journal, candidates, receipts, handoff) = protocol.into_parts();
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
        let edge_view = if let Some(edge) = prepared.journal().edges().first() {
            if let Some(projection) = handoff
                && self.prepare_native_handoff_projection(projection.stream(), edge) != handoff
            {
                return Err(EngineError::ReductionCauseInvariant {
                    detail: "native handoff preparation differs from its ordered segment",
                });
            }
            handoff.map(|projection| projection.edge_view(edge))
        } else {
            None
        };
        let validated_receipts = candidates
            .validate_against_presented_outputs(receipts, frozen_pointer_outputs)
            .map_err(|source| EngineError::PointerReceiverReceipt { source })?;
        let source_edge = edge_view
            .as_ref()
            .or_else(|| prepared.journal().edges().first());
        let native_view = source_edge
            .zip(validated_receipts.receipts().first())
            .map(|(edge, receipt)| resolve_native_pointer_edge(self, edge, receipt))
            .transpose()?;
        let effective_edges = native_view
            .as_ref()
            .map_or(prepared.journal().edges(), std::slice::from_ref);
        let desktop_hover_routes = effective_edges
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
        let desktop_delivery_routes = effective_edges
            .iter()
            .filter_map(|edge| {
                validated_desktop_delivery_route(self, edge).map(|route| (edge.sequence(), route))
            })
            .collect::<BTreeMap<_, _>>();
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
            effective_edges,
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
                || handoff.is_some_and(|projection| projection.stream() != accepted.stream())
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
            if let Some(projection) = handoff {
                self.apply_native_handoff_projection(projection, edge)?;
            }
            let effective = native_view.as_ref().unwrap_or(edge);
            let mut interaction = self.reduce_journal_pointer_edge(
                cause,
                focus_causal,
                accepted.stream(),
                accepted.capture_authority_for_reduction(),
                effective,
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
