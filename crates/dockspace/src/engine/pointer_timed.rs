//! Stationary gesture work through the existing ordered pointer transaction.

use super::*;

impl DockEngine {
    pub(crate) fn pointer_continuation(&self) -> Option<PointerContinuationRequest> {
        let InteractionStatus::Dragging { session } = self.interaction.status() else {
            return None;
        };
        let drag = self.interaction.active_drag(session).ok()?;
        let source_current = self.presentation_authority_is_current(drag.presentation)
            || drag.continuation.as_ref().is_some_and(|continuation| {
                self.scene_gesture_continuation_is_current(continuation)
            });
        if self.pointer_provider() != Some(drag.owner.lease()) || !source_current {
            return None;
        }
        let pending = drag.hover_selection.as_ref()?;
        let current = self.interaction_projection(pending.request.surface)?;
        (pending.workspace == self.version
            && pending.authority.stream() == current.authority().stream()
            && pending
                .authority
                .same_interaction_semantics(current.authority()))
        .then_some(pending.request)
    }

    fn clear_hover_selection(&mut self, session: crate::interaction::DragSessionId) {
        if let Ok(drag) = self.interaction.active_drag_mut(session) {
            drag.hover_selection = None;
        }
    }

    fn hover_selection_target(
        &self,
        receipt: &ValidatedPointerReceiverReceipt,
        snapshot: &JournalPresentationSnapshot,
    ) -> Option<(
        crate::scene::TabSceneId,
        PresentedSurfaceAuthority,
        LogicalPoint,
    )> {
        let PointerReceiverObservation::Presented(observation) = receipt.observation() else {
            return None;
        };
        let hover = observation.probes().iter().find_map(|probe| match probe {
            PointerReceiverProbeReceipt::HoverHit(hover) => Some(*hover),
            PointerReceiverProbeReceipt::Delivery(_) => None,
        })?;
        if !matches!(
            hover.disposition(),
            PointerReceiverHoverHitDisposition::Dock(_)
                | PointerReceiverHoverHitDisposition::NoReceiver
        ) {
            return None;
        }
        let (output, authority, point) = (hover.output()?, hover.authority()?, hover.point()?);
        let presentation = snapshot.presentation(output, authority).ok()?;
        let current = self.interaction_projection(authority.surface())?;
        if current.authority() != authority {
            return None;
        }
        // The already validated hover receipt proves framework clearance at
        // this point. Click geometry identifies a tab without treating docking
        // target fields, tab-close buttons or strip controls as tab bodies.
        let region = presentation
            .hit_manifest()
            .resolve_exclusive_with_suppression(
                PresentationPointerLane::Click,
                point,
                self.hover_drop_suppression(),
            )
            .ok()??;
        let PresentationHitRegionKind::TabBody(tab) = region.id().kind() else {
            return None;
        };
        matches!(
            self.workspace.nodes.get(tab.tabs),
            Some(Node::Tabs { selected, .. }) if *selected != Some(tab.item)
        )
        .then_some((tab, authority, point))
    }

    pub(super) fn advance_hover_selection(
        &mut self,
        cause: ReductionCause,
        owner: GestureOwner,
        session: crate::interaction::DragSessionId,
        edge: &PointerEdge,
        receipt: &ValidatedPointerReceiverReceipt,
        snapshot: &JournalPresentationSnapshot,
    ) -> Result<(), EngineError> {
        // Local scheduling uses a host clock and logical receiver challenge.
        // A native desktop host needs independently observed stationary routing
        // before it can opt into the same contract.
        let delay = self
            .presentation_authority
            .presentation_config
            .hover_selection_delay();
        let (Some(delay), Some(now)) = (delay, edge.metadata().observed_at()) else {
            self.clear_hover_selection(session);
            return Ok(());
        };
        if !owner.lease().scope().is_local() {
            self.clear_hover_selection(session);
            return Ok(());
        }
        let Some((target, authority, point)) = self.hover_selection_target(receipt, snapshot)
        else {
            self.clear_hover_selection(session);
            return Ok(());
        };
        let previous = self
            .interaction
            .active_drag(session)
            .ok()
            .and_then(|drag| drag.hover_selection.as_ref())
            .cloned();
        let same_target = previous.as_ref().is_some_and(|previous| {
            previous.target == target
                && previous.workspace == self.version
                && previous.authority.stream() == authority.stream()
                && previous.authority.same_interaction_semantics(authority)
        });
        let started_at = previous
            .as_ref()
            .filter(|_| same_target)
            .map_or(now, |previous| previous.started_at);
        let Some(deadline) = started_at.checked_add(delay) else {
            self.clear_hover_selection(session);
            return Ok(());
        };
        // Motion updates the finite challenge even if it is already due.
        // Only an explicit continuation executes timed work, after the host
        // has drained its accepted motion/release batch in the same order.
        // Every newly observed motion revokes its previous challenge, even if
        // it remains over the same tab. Only the dwell origin is retained.
        let token = self
            .interaction
            .mint_continuation(owner, session, deadline)
            .map_err(|source| EngineError::PointerInteractionInvariant {
                cause,
                detail: source.to_string(),
            })?;
        let pending = HoverSelectionState {
            request: PointerContinuationRequest {
                token,
                surface: authority.surface(),
                point,
            },
            target,
            authority,
            workspace: self.version,
            started_at,
        };
        self.interaction
            .active_drag_mut(session)
            .map_err(|source| EngineError::PointerInteractionInvariant {
                cause,
                detail: format!("{source:?}"),
            })?
            .hover_selection = Some(pending);
        Ok(())
    }

    pub(super) fn reduce_journal_continuation(
        &mut self,
        cause: ReductionCause,
        owner: GestureOwner,
        token: PointerContinuationToken,
        edge: &PointerEdge,
        receipt: &ValidatedPointerReceiverReceipt,
        snapshot: &JournalPresentationSnapshot,
        policy: &DockPolicySnapshot,
        events: &mut Vec<WorkspaceEvent>,
        interaction_events: &mut Vec<InteractionEvent>,
    ) -> Result<Vec<InteractionOutcome>, EngineError> {
        let pending = self
            .interaction
            .active_drag(token.session)
            .ok()
            .filter(|drag| drag.owner == owner)
            .and_then(|drag| drag.hover_selection.as_ref())
            .cloned();
        let Some(pending) = pending.filter(|pending| pending.request.token == token) else {
            return Ok(Vec::new());
        };
        let (delivery, delivery_outcome) = self.gate_journal_delivery_action(
            cause,
            owner,
            edge.delivery_owner(),
            interaction_events,
        )?;
        let mut outcomes = delivery_outcome.into_iter().collect::<Vec<_>>();
        if !delivery {
            return Ok(outcomes);
        }
        let (capture, capture_outcome) = self.gate_journal_capture_action(
            cause,
            owner,
            edge.capture_owner(),
            false,
            interaction_events,
        )?;
        outcomes.extend(capture_outcome);
        if !capture {
            return Ok(outcomes);
        }
        if let Some(outcome) = self.cancel_revoked_presentation_caused(cause, interaction_events)? {
            outcomes.push(outcome);
            return Ok(outcomes);
        }
        let target = self.hover_selection_target(receipt, snapshot);
        if pending.workspace != self.version
            || target.is_none_or(|(target, authority, point)| {
                target != pending.target
                    || point != pending.request.point
                    || authority.stream() != pending.authority.stream()
                    || !authority.same_interaction_semantics(pending.authority)
            })
        {
            self.clear_hover_selection(token.session);
            return Ok(outcomes);
        }
        if edge
            .metadata()
            .observed_at()
            .is_none_or(|now| now < token.deadline)
        {
            return Ok(outcomes);
        }
        self.clear_hover_selection(token.session);
        outcomes.push(self.select_hovered_tab(
            cause,
            owner,
            token.session,
            pending.target,
            policy,
            events,
        )?);
        Ok(outcomes)
    }

    fn select_hovered_tab(
        &mut self,
        cause: ReductionCause,
        owner: GestureOwner,
        session: crate::interaction::DragSessionId,
        tab: crate::scene::TabSceneId,
        policy: &DockPolicySnapshot,
        events: &mut Vec<WorkspaceEvent>,
    ) -> Result<InteractionOutcome, EngineError> {
        let invariant = |detail: String| EngineError::PointerInteractionInvariant { cause, detail };
        let drag = self
            .interaction
            .active_drag(session)
            .map_err(|error| invariant(format!("{error:?}")))?
            .clone();
        let source = self
            .workspace
            .capture_item_source(tab.root, tab.tabs, tab.item)
            .map_err(|error| invariant(error.to_string()))?;
        let mut candidate = self.clone_workspace_candidate();
        let report =
            match WorkspaceTransaction::from_commands([WorkspaceCommand::Select { source }])
                .apply(&mut candidate, policy)
            {
                Ok(report) => report,
                Err(TransactionError::Command { source, .. }) if source.is_expected_rejection() => {
                    return Ok(InteractionOutcome::Rejected(
                        InteractionRejection::CommandRejected(source),
                    ));
                }
                Err(error) => return Err(invariant(error.to_string())),
            };
        if let Some(surface) = self
            .first_pending_presentation_source_mismatch(
                &candidate,
                WorkspacePublicationAuthority::Ordinary,
            )
            .or_else(|| self.first_workspace_publication_mismatch(&candidate, None, None))
        {
            return Ok(InteractionOutcome::Rejected(
                InteractionRejection::CommandRejected(CommandError::SurfaceLifecycleFrozen {
                    surface,
                }),
            ));
        }
        let changed = report.changed();
        let committed = report.into_outcomes();
        let publication = match self.stage_workspace_publication(candidate, policy) {
            Ok(publication) => publication,
            Err(error) if error.is_expected_rejection() => {
                return Ok(InteractionOutcome::Rejected(
                    InteractionRejection::CommandRejected(error),
                ));
            }
            Err(error) => return Err(invariant(error.to_string())),
        };
        self.publish_workspace(publication);
        self.reconcile_viewport_focus_authority();
        if changed {
            self.advance_revision_caused(cause)?;
            events.extend(committed.into_iter().map(|outcome| {
                WorkspaceEvent::new_caused(
                    cause,
                    self.version,
                    WorkspaceEventKind::CommandCommitted(outcome),
                )
            }));
        }
        // Select changes fingerprints but cannot change topology, placement or
        // payload membership. Recapture only those same structural identities;
        // ordinary application mutations still cancel the gesture normally.
        let payload = match &drag.payload {
            MovePayload::Item(source) => self
                .workspace
                .capture_item_source(source.root(), source.tabs(), source.item())
                .map(MovePayload::Item),
            MovePayload::Tabs(source) => self
                .workspace
                .capture_node_source(source.root(), source.node())
                .map(MovePayload::Tabs),
            MovePayload::Subtree(source) => self
                .workspace
                .capture_node_source(source.root(), source.node())
                .map(MovePayload::Subtree),
        }
        .map_err(|error| invariant(error.to_string()))?;
        let prepared = self
            .prepare_drag_source(&payload)
            .map_err(|error| invariant(format!("{error:?}")))?;
        if prepared.source_surface != drag.source_surface {
            return Err(invariant(
                "hover selection changed its source owner".to_owned(),
            ));
        }
        let coordinates = drag
            .continuation
            .as_ref()
            .and_then(|continuation| match &continuation.draft.source {
                SceneGestureContinuationSource::Drag { coordinates, .. } => Some(*coordinates),
                SceneGestureContinuationSource::ContainedTransform { .. } => None,
            })
            .unwrap_or_else(|| self.capture_surface_coordinates(drag.source_surface));
        let continuation = self
            .scene_gesture_continuation_draft(
                cause,
                owner,
                drag.presentation,
                SceneGestureContinuationSource::Drag {
                    payload: payload.clone(),
                    source_surface: drag.source_surface,
                    complete_root: prepared.complete_root.clone(),
                    origin: drag.origin.clone(),
                    coordinates,
                },
            )
            .map(|draft| draft.bind(SceneGestureSession::Drag(session)));
        let active = self
            .interaction
            .active_drag_mut(session)
            .map_err(|error| invariant(format!("{error:?}")))?;
        active.payload = payload;
        active.complete_root = prepared.complete_root;
        active.partial_detachable = prepared.partial_detachable;
        active.source_validated_at = self.version;
        active.continuation = continuation;
        self.interaction
            .clear_drag_feedback(session)
            .map_err(|error| invariant(format!("{error:?}")))?;
        Ok(InteractionOutcome::ProductActionApplied(
            crate::model::DockspaceActionOutcome::Selected {
                item: tab.item,
                changed,
            },
        ))
    }
}
