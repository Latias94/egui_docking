//! Local-touch waiting and activation owned by the existing click gesture.

use super::*;
use crate::interaction::ActiveClick;
use crate::pointer_journal::PointerEventMetadata;

impl DockEngine {
    pub(super) fn touch_complete_drag_root(
        &self,
        region: PresentationHitRegionKind,
    ) -> Option<RootId> {
        let payload = match region {
            PresentationHitRegionKind::TabBody(tab) => MovePayload::Item(
                self.workspace
                    .capture_item_source(tab.root, tab.tabs, tab.item)
                    .ok()?,
            ),
            PresentationHitRegionKind::TabGroupDrag { bar, .. } => MovePayload::Tabs(
                self.workspace
                    .capture_node_source(bar.root, bar.tabs)
                    .ok()?,
            ),
            PresentationHitRegionKind::ContainedTitle(floating) => {
                let contained = self.workspace.contained_floating(floating)?;
                let root = self.workspace.root(contained.root)?;
                MovePayload::Subtree(
                    self.workspace
                        .capture_node_source(contained.root, root.node)
                        .ok()?,
                )
            }
            _ => return None,
        };
        self.complete_root_source(&payload)
            .ok()
            .flatten()
            .map(|source| source.root())
    }

    pub(super) const fn touch_wait_region(kind: PresentationHitRegionKind) -> bool {
        matches!(
            kind,
            PresentationHitRegionKind::TabBody(_)
                | PresentationHitRegionKind::TabGroupDrag { .. }
                | PresentationHitRegionKind::ContainedTitle(_)
                | PresentationHitRegionKind::ContainedResize { .. }
                | PresentationHitRegionKind::SplitterHandle(_)
                | PresentationHitRegionKind::SplitterJunction(_)
        )
    }

    pub(super) fn arm_journal_touch_press(
        &mut self,
        cause: ReductionCause,
        owner: GestureOwner,
        capture_authority: Authority<PointerCaptureOwner>,
        presentation: &JournalSurfacePresentation,
        region: PresentationHitRegionId,
        point: LogicalPoint,
        threshold_origin: JournalDragThresholdOrigin,
        metadata: PointerEventMetadata,
    ) -> Result<Option<InteractionOutcome>, EngineError> {
        if !self.presented_receiver_is_current(presentation.plan(), region) {
            return Ok(Some(InteractionOutcome::Rejected(
                InteractionRejection::StaleScene,
            )));
        }
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
                action: Arc::new(FrozenClickAction::Touch),
            })
            .map_err(|error| touch_invariant(cause, error))?;
        if replaced.is_some() {
            return Err(touch_invariant(cause, "touch press replaced another owner"));
        }
        let deadline = metadata
            .observed_at()
            .filter(|_| {
                !matches!(owner.lease().scope(), PointerProviderScope::NativeJoined(_))
                    || self.pointer_continuation_source_is_current(owner)
            })
            .and_then(|now| {
                now.checked_add(
                    self.presentation_authority
                        .presentation_config
                        .touch_long_press_delay(),
                )
            });
        let request = deadline
            .map(|deadline| {
                self.interaction
                    .mint_continuation(owner, PointerContinuationSession::Click(session), deadline)
                    .map(|token| PointerContinuationRequest {
                        token,
                        surface: presentation.surface(),
                        point,
                    })
            })
            .transpose()
            .map_err(|error| touch_invariant(cause, error))?;
        self.interaction
            .active_click_mut(session)
            .map_err(|error| touch_invariant(cause, format!("{error:?}")))?
            .touch = Some(TouchPressState {
            initial_pointer: point,
            current_pointer: point,
            threshold_origin,
            request,
            armed: false,
        });
        Ok(None)
    }

    pub(super) fn reduce_touch_continuation(
        &mut self,
        cause: ReductionCause,
        owner: GestureOwner,
        token: PointerContinuationToken,
        edge: &PointerEdge,
        receipt: &ValidatedPointerReceiverReceipt,
        snapshot: &JournalPresentationSnapshot,
        interaction_events: &mut Vec<InteractionEvent>,
    ) -> Result<Vec<InteractionOutcome>, EngineError> {
        let PointerContinuationSession::Click(session) = token.session else {
            return Ok(Vec::new());
        };
        let Some(click) = self
            .interaction
            .active_click(session)
            .ok()
            .filter(|click| click.owner == owner)
            .cloned()
        else {
            return Ok(Vec::new());
        };
        let Some(touch) = click.touch.as_ref().filter(|touch| {
            !touch.armed && touch.request.is_some_and(|request| request.token == token)
        }) else {
            return Ok(Vec::new());
        };
        let (delivery, outcome) = self.gate_journal_delivery_action(
            cause,
            owner,
            edge.delivery_owner(),
            interaction_events,
        )?;
        let mut outcomes = outcome.into_iter().collect::<Vec<_>>();
        if !delivery {
            return Ok(outcomes);
        }
        let (capture, outcome) = self.gate_journal_capture_action(
            cause,
            owner,
            edge.capture_owner(),
            false,
            interaction_events,
        )?;
        outcomes.extend(outcome);
        if !capture {
            return Ok(outcomes);
        }
        if let Some(outcome) = self.cancel_revoked_presentation_caused(cause, interaction_events)? {
            outcomes.push(outcome);
            return Ok(outcomes);
        }
        let observed = Self::journal_semantic_press_delivery(cause, receipt, snapshot)?;
        let valid = observed.is_some_and(|(region, presentation)| {
            region == click.region && self.touch_source_presentation_matches(&click, presentation)
        }) && self
            .journal_logical_point(edge, validated_desktop_delivery_route(self, edge))
            == Some(touch.current_pointer);
        if !valid {
            if let Some(outcome) = self.cancel_gesture_owner(
                cause,
                owner,
                InteractionCancelReason::UnknownTargetAuthority,
                interaction_events,
            )? {
                outcomes.push(outcome);
            }
            return Ok(outcomes);
        }
        if edge
            .metadata()
            .observed_at()
            .is_none_or(|now| now < token.deadline)
        {
            return Ok(outcomes);
        }
        let state = self
            .interaction
            .active_click_mut(session)
            .map_err(|error| touch_invariant(cause, format!("{error:?}")))?
            .touch
            .as_mut()
            .ok_or_else(|| touch_invariant(cause, "touch deadline lost its pressed state"))?;
        state.request = None;
        state.armed = true;
        outcomes.push(InteractionOutcome::TouchArmed {
            pointer: owner.pointer(),
            surface: click.region.surface(),
        });
        Ok(outcomes)
    }

    fn touch_source_presentation_matches(
        &self,
        click: &ActiveClick,
        presentation: &JournalSurfacePresentation,
    ) -> bool {
        click.region.surface() == presentation.surface()
            && self.presented_receiver_is_current(presentation.plan(), click.region)
            && click.measurement == presentation.scene().requirement()
            && click
                .coordinates
                .same_projection_authority(presentation.coordinate_capture())
            && click.presentation.presented.stream() == presentation.authority().stream()
            && click
                .presentation
                .presented
                .same_interaction_semantics(presentation.authority())
            && self
                .interaction_projection(presentation.surface())
                .is_some_and(|current| current.authority() == presentation.authority())
    }

    pub(super) fn reduce_touch_move(
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
    ) -> Result<Option<Vec<InteractionOutcome>>, EngineError> {
        let InteractionStatus::Pressed { session } = self.interaction.status() else {
            return Ok(None);
        };
        let Some(click) = self
            .interaction
            .active_click(session)
            .ok()
            .filter(|click| click.owner == owner && click.touch.is_some())
            .cloned()
        else {
            return Ok(None);
        };
        let touch = click
            .touch
            .as_ref()
            .expect("the pressed touch state was checked");
        let point = self.journal_logical_point(edge, validated_desktop_delivery_route(self, edge));
        if point.is_none() {
            // Missing coordinates do not prove movement and cannot activate
            // the frozen source. Stop scheduling the old stationary challenge.
            if let Ok(click) = self.interaction.active_click_mut(session)
                && let Some(touch) = click.touch.as_mut()
            {
                touch.request = None;
            }
            return Ok(Some(Vec::new()));
        }
        let slop = self
            .presentation_authority
            .presentation_config
            .touch_press_slop_distance();
        let outside =
            point.is_none_or(|point| exceeds_touch_slop(touch.initial_pointer, point, slop));
        if !touch.armed && outside {
            self.interaction
                .take_click_for_release(session, owner, PointerButton::Primary)
                .map_err(|error| touch_invariant(cause, format!("{error:?}")))?;
            return Ok(Some(vec![InteractionOutcome::TouchYieldedToContent {
                pointer: owner.pointer(),
                surface: click.region.surface(),
            }]));
        }
        if !outside {
            let point = point.expect("a point inside the touch slop is known");
            let request = touch
                .request
                .map(|request| {
                    self.interaction
                        .mint_continuation(
                            owner,
                            PointerContinuationSession::Click(session),
                            request.token.deadline,
                        )
                        .map(|token| PointerContinuationRequest {
                            token,
                            point,
                            ..request
                        })
                })
                .transpose()
                .map_err(|error| touch_invariant(cause, error))?;
            let state = self
                .interaction
                .active_click_mut(session)
                .map_err(|error| touch_invariant(cause, format!("{error:?}")))?
                .touch
                .as_mut()
                .expect("touch owner remains pressed");
            state.current_pointer = point;
            state.request = request;
            return Ok(Some(Vec::new()));
        }
        // Only an already armed contact can turn movement into docking. The
        // frozen source proof survives equivalent repaints, not layout changes.
        let presentation = self
            .interaction_projection(click.region.surface())
            .map(JournalSurfacePresentation::from_interaction);
        let Some(presentation) =
            presentation.filter(|current| self.touch_source_presentation_matches(&click, current))
        else {
            return Ok(Some(
                self.cancel_gesture_owner(
                    cause,
                    owner,
                    InteractionCancelReason::SceneUnavailable,
                    interaction_events,
                )?
                .into_iter()
                .collect(),
            ));
        };
        self.interaction
            .take_click_for_release(session, owner, PointerButton::Primary)
            .map_err(|error| touch_invariant(cause, format!("{error:?}")))?;
        // This motion starts manipulation rather than activating a tap. Keep
        // the displayed selection and contained order intact until delivery;
        // eager Select/Raise would revoke same-batch target authority.
        let mut outcomes = self
            .activate_journal_source_press(
                cause,
                focus_causal,
                SourcePressIntent::Manipulate,
                owner,
                edge.capture_owner(),
                touch.threshold_origin,
                &presentation,
                click.region,
                touch.initial_pointer,
                crate::pointer_journal::PointerDeviceKind::Touch,
                policy,
                events,
            )?
            .into_iter()
            .collect::<Vec<_>>();
        outcomes.extend(self.reduce_journal_move(
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
        )?);
        Ok(Some(outcomes))
    }

    pub(super) fn finish_journal_touch_release(
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
        let click = self
            .interaction
            .take_click_for_release(session, owner, PointerButton::Primary)
            .map_err(|error| touch_invariant(cause, format!("{error:?}")))?;
        let touch = click
            .touch
            .as_ref()
            .ok_or_else(|| touch_invariant(cause, "touch release has no press state"))?;
        let delivered = Self::journal_semantic_press_delivery(cause, receipt, snapshot)?;
        let Some((region, presentation)) = delivered.filter(|(region, presentation)| {
            *region == click.region && self.touch_source_presentation_matches(&click, presentation)
        }) else {
            return Ok(touch_release_cancelled(
                cause,
                self.version,
                session,
                InteractionCancelReason::ClickReceiverMismatch,
                interaction_events,
            ));
        };
        let point = self.journal_logical_point(edge, desktop_route);
        let slop = self
            .presentation_authority
            .presentation_config
            .touch_press_slop_distance();
        let Some(point) =
            point.filter(|point| !exceeds_touch_slop(touch.initial_pointer, *point, slop))
        else {
            return Ok(InteractionOutcome::TouchYieldedToContent {
                pointer: owner.pointer(),
                surface: click.region.surface(),
            });
        };
        if touch.armed {
            let (root, item) = touch_menu_target(region.kind(), presentation.plan())
                .ok_or_else(|| touch_invariant(cause, "touch menu has no semantic root"))?;
            return Ok(InteractionOutcome::ContextMenuRequested {
                pointer: owner.pointer(),
                surface: region.surface(),
                root,
                item,
                position: point,
            });
        }
        let before = self.version;
        let outcome = self.activate_journal_source_press(
            cause,
            focus_causal,
            SourcePressIntent::Activate,
            owner,
            edge.capture_owner(),
            touch.threshold_origin,
            presentation,
            region,
            touch.initial_pointer,
            crate::pointer_journal::PointerDeviceKind::Touch,
            policy,
            events,
        )?;
        if matches!(
            outcome,
            Some(InteractionOutcome::Rejected(_))
                | Some(InteractionOutcome::ProductActionRejected(_))
        ) {
            return Ok(outcome.expect("the rejected touch activation was matched"));
        }
        // A tap consumes its newly prepared arm without synthesizing release or
        // delivery. All selection/raise/focus semantics came from the shared path.
        match self.interaction.status() {
            InteractionStatus::Armed { session } => {
                self.interaction
                    .cancel_drag(session)
                    .map_err(|error| touch_invariant(cause, format!("{error:?}")))?;
            }
            InteractionStatus::Resizing { session } => {
                self.interaction
                    .cancel_resize(session)
                    .map_err(|error| touch_invariant(cause, format!("{error:?}")))?;
            }
            InteractionStatus::ContainedTransforming { session } => {
                self.interaction
                    .cancel_contained_transform(session)
                    .map_err(|error| touch_invariant(cause, format!("{error:?}")))?;
            }
            InteractionStatus::Idle => {}
            _ => {
                return Err(touch_invariant(
                    cause,
                    "a touch tap created an unexpected owner",
                ));
            }
        }
        if let PresentationHitRegionKind::TabBody(tab) = region.kind() {
            return Ok(InteractionOutcome::ProductActionApplied(
                crate::model::DockspaceActionOutcome::Selected {
                    item: tab.item,
                    changed: self.version != before,
                },
            ));
        }
        Ok(InteractionOutcome::TouchActivated {
            pointer: owner.pointer(),
            surface: region.surface(),
        })
    }
}

fn touch_menu_target(
    kind: PresentationHitRegionKind,
    plan: &PresentationPlan,
) -> Option<(RootId, Option<crate::ids::ItemId>)> {
    match kind {
        PresentationHitRegionKind::TabBody(tab) => Some((tab.root, Some(tab.item))),
        PresentationHitRegionKind::TabGroupDrag { bar, .. } => Some((bar.root, None)),
        PresentationHitRegionKind::ContainedTitle(floating)
        | PresentationHitRegionKind::ContainedResize { floating, .. } => plan
            .contained_record(floating)
            .map(|record| (record.root(), None)),
        PresentationHitRegionKind::SplitterHandle(split) => Some((split.root, None)),
        PresentationHitRegionKind::SplitterJunction(junction) => [
            crate::scene::SplitterJunctionDirection::North,
            crate::scene::SplitterJunctionDirection::East,
            crate::scene::SplitterJunctionDirection::South,
            crate::scene::SplitterJunctionDirection::West,
        ]
        .into_iter()
        .find_map(|direction| junction.arm(direction).map(|split| (split.root, None))),
        _ => None,
    }
}

pub(super) fn exceeds_touch_slop(origin: LogicalPoint, point: LogicalPoint, slop: f64) -> bool {
    (point.x() - origin.x()).hypot(point.y() - origin.y()) > slop
}

fn touch_invariant(cause: ReductionCause, detail: impl std::fmt::Display) -> EngineError {
    EngineError::PointerInteractionInvariant {
        cause,
        detail: detail.to_string(),
    }
}

fn touch_release_cancelled(
    cause: ReductionCause,
    version: WorkspaceVersion,
    session: ClickSessionId,
    reason: InteractionCancelReason,
    events: &mut Vec<InteractionEvent>,
) -> InteractionOutcome {
    let status = InteractionStatus::Pressed { session };
    events.push(InteractionEvent::new_caused(
        cause,
        version,
        InteractionEventKind::Cancelled { status, reason },
    ));
    InteractionOutcome::Cancelled { status, reason }
}
