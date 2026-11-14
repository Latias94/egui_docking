//! Stationary gesture work through the existing ordered pointer transaction.

use super::*;

impl DockEngine {
    pub(super) fn pointer_timing_supported(owner: GestureOwner) -> bool {
        matches!(
            (owner.lease().scope(), owner.lane()),
            (PointerProviderScope::HostLocal(_), GestureLane::HostLocal)
                | (
                    PointerProviderScope::SurfaceLocal(_),
                    GestureLane::SurfaceLocal
                )
                | (
                    PointerProviderScope::NativeJoined(_),
                    GestureLane::NativeLocal { .. } | GestureLane::Desktop
                )
        )
    }

    fn native_stationary_source_identity(&self, owner: GestureOwner) -> Option<ViewportBinding> {
        let PointerProviderScope::NativeJoined(host) = owner.lease().scope() else {
            return None;
        };
        if self.pointer_provider() != Some(owner.lease())
            || self.interaction.active_owner() != Some(owner)
        {
            return None;
        }
        let binding = self
            .interaction
            .active_presentation_authority()?
            .presented
            .binding()?;
        let coordinates = self.native_local_coordinate_snapshot(host, binding)?;
        if let GestureLane::NativeLocal {
            binding: local_binding,
            coordinate_generation,
        } = owner.lane()
            && (local_binding != binding.window()
                || coordinates.window_coordinates().coordinate_generation()
                    != coordinate_generation)
        {
            return None;
        }
        Some(binding)
    }

    fn native_stationary_source(&self, owner: GestureOwner) -> Option<ViewportBinding> {
        let binding = self.native_stationary_source_identity(owner)?;
        (self
            .pointer_journal
            .native_primary_press_continuity(owner.stream(), binding.window())
            && self.interaction.active_journal_capture_authority()
                == Some(Authority::Known(PointerCaptureOwner::Native(
                    binding.window(),
                ))))
        .then_some(binding)
    }

    pub(super) fn pointer_continuation_source_is_current(&self, owner: GestureOwner) -> bool {
        Self::pointer_timing_supported(owner)
            && self.pointer_provider() == Some(owner.lease())
            && (!matches!(owner.lease().scope(), PointerProviderScope::NativeJoined(_))
                || self.native_stationary_source(owner).is_some())
    }

    pub(super) fn stationary_surface_is_reachable(
        &self,
        owner: GestureOwner,
        surface: SurfaceId,
    ) -> bool {
        if !matches!(owner.lease().scope(), PointerProviderScope::NativeJoined(_)) {
            return true;
        }
        let Some(source) = self.native_stationary_source(owner) else {
            return false;
        };
        if source.surface() == surface {
            return true;
        }
        if self.viewport.viewport(surface).is_some_and(|target| {
            target.binding().window() == source.window() && target.is_routeable()
        }) {
            return true;
        }
        owner.is_desktop()
            && self
                .viewport
                .drag_source_input(owner.pointer())
                .is_some_and(|input| {
                    self.viewport
                        .physical_cross_surface_drag_capability(input)
                        .is_supported()
                })
            && self.viewport.confirmed_drag_source(owner.pointer()) == Some(source)
    }

    pub(super) fn stationary_edge_has_authority(
        &self,
        owner: GestureOwner,
        edge: &PointerEdge,
    ) -> bool {
        if !Self::pointer_timing_supported(owner) {
            return false;
        }
        if !matches!(owner.lease().scope(), PointerProviderScope::NativeJoined(_)) {
            return true;
        }
        self.native_stationary_source(owner).is_some_and(|binding| {
            edge.pointer() == owner.pointer()
                && edge.delivery_owner()
                    == Authority::Known(PointerEventDeliveryOwner::Native(binding.window()))
                && edge.capture_owner()
                    == Authority::Known(PointerCaptureOwner::Native(binding.window()))
        })
    }

    pub(super) fn journal_continuation_is_current(
        &self,
        stream: PointerStreamId,
        token: PointerContinuationToken,
    ) -> bool {
        if token.owner != stream || self.interaction.active_stream() != Some(stream) {
            return false;
        }
        match token.session {
            PointerContinuationSession::Click(session) => {
                self.interaction.active_click(session).is_ok_and(|click| {
                    click.touch.as_ref().is_some_and(|touch| {
                        !touch.armed && touch.request.is_some_and(|request| request.token == token)
                    })
                })
            }
            PointerContinuationSession::Drag(session) => {
                self.interaction.active_drag(session).is_ok_and(|drag| {
                    drag.timed
                        .as_ref()
                        .is_some_and(|timed| timed.request().token == token)
                })
            }
        }
    }

    pub(super) fn journal_capture_acquisition_is_current(
        &self,
        stream: PointerStreamId,
        token: PointerContinuationToken,
    ) -> bool {
        let PointerContinuationSession::Drag(session) = token.session else {
            return false;
        };
        // Target generations may advance while a native acquisition is in flight.
        // Only the original desktop gesture is matched; no old target is executed.
        self.interaction.active_drag(session).is_ok_and(|drag| {
            token.owner == stream
                && token.native_desktop
                && drag.owner.stream() == stream
                && drag.owner.is_desktop()
                && matches!(
                    drag.owner.lease().scope(),
                    PointerProviderScope::NativeJoined(_)
                )
                && self.native_stationary_source_identity(drag.owner).is_some()
                && (self.presentation_authority_is_current(drag.presentation)
                    || drag.continuation.as_ref().is_some_and(|continuation| {
                        self.scene_gesture_continuation_is_current(continuation)
                    }))
        })
    }

    pub(crate) fn native_pointer_capture_projection(
        &self,
    ) -> Option<(
        crate::interaction::DragSessionId,
        GestureOwner,
        crate::platform::NativeDragSourceInput,
    )> {
        let InteractionStatus::Dragging { session } = self.interaction.status() else {
            return None;
        };
        let drag = self.interaction.active_drag(session).ok()?;
        let owner = drag.owner;
        if !owner.is_desktop()
            || !matches!(owner.lease().scope(), PointerProviderScope::NativeJoined(_))
            || self.pointer_journal.stream_device(owner.stream())
                != Some(crate::pointer_journal::PointerDeviceKind::Mouse)
            || !self.pointer_continuation_source_is_current(owner)
            || !(self.presentation_authority_is_current(drag.presentation)
                || drag.continuation.as_ref().is_some_and(|continuation| {
                    self.scene_gesture_continuation_is_current(continuation)
                }))
        {
            return None;
        }
        Some((session, owner, self.native_drag_source_input(drag)))
    }

    pub(crate) fn native_drag_capture_projection(
        &self,
        token: PointerContinuationToken,
    ) -> Option<(crate::platform::NativeDragSourceInput, bool)> {
        if !self.journal_capture_acquisition_is_current(token.owner, token) {
            return None;
        }
        let PointerContinuationSession::Drag(session) = token.session else {
            return None;
        };
        let drag = self.interaction.active_drag(session).ok()?;
        if !self.pointer_continuation_source_is_current(drag.owner) {
            return None;
        }
        Some((
            self.native_drag_source_input(drag),
            drag.capture_acquired_at.is_some(),
        ))
    }

    pub(super) fn reduce_journal_capture_acquired(
        &mut self,
        cause: ReductionCause,
        owner: GestureOwner,
        token: PointerContinuationToken,
        edge: &PointerEdge,
        interaction_events: &mut Vec<InteractionEvent>,
    ) -> Result<Vec<InteractionOutcome>, EngineError> {
        if !self.journal_capture_acquisition_is_current(owner.stream(), token) {
            return Ok(Vec::new());
        }
        let PointerContinuationSession::Drag(session) = token.session else {
            return Ok(Vec::new());
        };
        let mut outcomes = Vec::new();
        if matches!(edge.delivery_owner(), Authority::Known(_)) {
            let (delivery, outcome) = self.gate_journal_delivery_action(
                cause,
                owner,
                edge.delivery_owner(),
                interaction_events,
            )?;
            outcomes.extend(outcome);
            if !delivery {
                return Ok(outcomes);
            }
        }
        outcomes.extend(self.reduce_journal_capture_change(
            cause,
            owner,
            edge.capture_owner(),
            interaction_events,
        )?);
        if self.interaction.active_owner() != Some(owner) {
            return Ok(outcomes);
        }
        if !self.stationary_edge_has_authority(owner, edge) {
            self.clear_drag_continuation(session);
            return Ok(outcomes);
        }
        if let Some(outcome) = self.cancel_revoked_presentation_caused(cause, interaction_events)? {
            outcomes.push(outcome);
            return Ok(outcomes);
        }
        let drag = self.interaction.active_drag(session).map_err(|_| {
            EngineError::ReductionCauseInvariant {
                detail: "capture acquisition lost its current drag",
            }
        })?;
        if drag.capture_acquired_at.is_some() {
            return Ok(outcomes);
        }
        let Some(now) = edge.metadata().observed_at() else {
            self.clear_drag_continuation(session);
            return Ok(outcomes);
        };
        let delay = match drag.timed.as_ref() {
            Some(DragTimedContinuation::HoverSelection(_)) => self
                .presentation_authority
                .presentation_config
                .hover_selection_delay(),
            Some(DragTimedContinuation::TabStripEdgeScroll(_)) => self
                .presentation_authority
                .presentation_config
                .tab_strip_edge_scroll_interval(),
            None => None,
        };
        let next = match delay.and_then(|delay| now.checked_add(delay)) {
            Some(deadline) => Some(
                self.interaction
                    .mint_continuation(owner, PointerContinuationSession::Drag(session), deadline)
                    .map_err(|source| EngineError::PointerInteractionInvariant {
                        cause,
                        detail: source.to_string(),
                    })?,
            ),
            None => None,
        };
        let drag = self.interaction.active_drag_mut(session).map_err(|_| {
            EngineError::ReductionCauseInvariant {
                detail: "capture acquisition lost its mutable drag",
            }
        })?;
        // Acquisition is accepted once per original drag. A late same-gesture result may
        // postpone its current target, but cannot reuse the pre-acquisition dwell interval.
        drag.capture_acquired_at = Some(now);
        match (drag.timed.as_mut(), next) {
            (Some(DragTimedContinuation::HoverSelection(state)), Some(token)) => {
                state.started_at = now;
                state.request.token = token;
            }
            (Some(DragTimedContinuation::TabStripEdgeScroll(state)), Some(token)) => {
                state.started_at = now;
                state.request.token = token;
            }
            _ => drag.timed = None,
        }
        Ok(outcomes)
    }

    pub(super) fn clear_pointer_continuation(&mut self, owner: GestureOwner) {
        if self.interaction.active_owner() != Some(owner) {
            return;
        }
        match self.interaction.status() {
            InteractionStatus::Pressed { session } => {
                if let Ok(click) = self.interaction.active_click_mut(session)
                    && let Some(touch) = click.touch.as_mut()
                {
                    touch.request = None;
                }
            }
            InteractionStatus::Dragging { session } => self.clear_drag_continuation(session),
            _ => {}
        }
    }

    pub(crate) fn pointer_continuation(&self) -> Option<PointerContinuationRequest> {
        let owner = self.interaction.active_owner()?;
        if !self.pointer_continuation_source_is_current(owner) {
            return None;
        }
        if let InteractionStatus::Pressed { session } = self.interaction.status() {
            let click = self.interaction.active_click(session).ok()?;
            let touch = click.touch.as_ref()?;
            if touch.armed
                || self.pointer_provider() != Some(click.owner.lease())
                || !self.presentation_authority_is_current(click.presentation)
            {
                return None;
            }
            let current = self.interaction_projection(click.region.surface())?;
            return (current.authority().stream() == click.presentation.presented.stream()
                && current
                    .authority()
                    .same_interaction_semantics(click.presentation.presented))
            .then_some(touch.request)
            .flatten();
        }
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
        let timed = drag.timed.as_ref()?;
        if !self.stationary_surface_is_reachable(owner, timed.request().surface) {
            return None;
        }
        match timed {
            DragTimedContinuation::TabStripEdgeScroll(pending) => self
                .edge_scroll_continuation_is_current(pending)
                .then_some(pending.request),
            DragTimedContinuation::HoverSelection(pending) => {
                let current = self.interaction_projection(pending.request.surface)?;
                (pending.workspace == self.version
                    && pending.authority.stream() == current.authority().stream()
                    && pending
                        .authority
                        .same_interaction_semantics(current.authority()))
                .then_some(pending.request)
            }
        }
    }

    pub(super) fn clear_drag_continuation(&mut self, session: crate::interaction::DragSessionId) {
        if let Ok(drag) = self.interaction.active_drag_mut(session) {
            drag.timed = None;
        }
    }

    pub(super) fn pause_drag_continuation_for_scroll(&mut self, receiver: PresentationHitRegionId) {
        let key = match receiver.kind() {
            PresentationHitRegionKind::TabStripScroll(bar) => {
                TabStripStateKey::new(receiver.surface(), bar)
            }
            PresentationHitRegionKind::TabListMenuScroll(menu) => menu.key(),
            _ => return,
        };
        let InteractionStatus::Dragging { session } = self.interaction.status() else {
            return;
        };
        let affected = self.interaction.active_drag(session).is_ok_and(|drag| {
            drag.timed.as_ref().is_some_and(|timed| match timed {
                DragTimedContinuation::TabStripEdgeScroll(pending) => pending.key == key,
                DragTimedContinuation::HoverSelection(pending) => {
                    pending.request.surface == key.surface()
                        && pending.target.root == key.bar().root
                        && pending.target.tabs == key.bar().tabs
                }
            })
        });
        if affected {
            self.clear_drag_continuation(session);
        }
    }

    pub(super) fn stationary_hover_presentation<'snapshot>(
        &self,
        receipt: &ValidatedPointerReceiverReceipt,
        snapshot: &'snapshot JournalPresentationSnapshot,
    ) -> Option<(&'snapshot JournalSurfacePresentation, LogicalPoint)> {
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
                | PointerReceiverHoverHitDisposition::EmptyLane
        ) {
            return None;
        }
        let (output, authority, point) = (hover.output()?, hover.authority()?, hover.point()?);
        let presentation = snapshot.presentation(output, authority).ok()?;
        if !self
            .stationary_surface_is_reachable(self.interaction.active_owner()?, authority.surface())
        {
            return None;
        }
        let current = self.interaction_projection(authority.surface())?;
        (current.authority() == authority).then_some((presentation, point))
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
        let (presentation, point) = self.stationary_hover_presentation(receipt, snapshot)?;
        let authority = presentation.authority();
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
        if !self.presented_receiver_is_current(presentation.plan(), region.id()) {
            return None;
        }
        matches!(
            self.workspace.node(tab.tabs),
            Some(Node::Tabs { items, selected, .. })
                if items.contains(&tab.item) && *selected != Some(tab.item)
        )
        .then_some((tab, authority, point))
    }

    pub(super) fn advance_drag_continuation(
        &mut self,
        cause: ReductionCause,
        owner: GestureOwner,
        session: crate::interaction::DragSessionId,
        edge: &PointerEdge,
        receipt: &ValidatedPointerReceiverReceipt,
        snapshot: &JournalPresentationSnapshot,
    ) -> Result<(), EngineError> {
        if self.advance_tab_strip_edge_scroll(cause, owner, session, edge, receipt, snapshot)? {
            return Ok(());
        }
        let delay = self
            .presentation_authority
            .presentation_config
            .hover_selection_delay();
        let (Some(delay), Some(now)) = (delay, edge.metadata().observed_at()) else {
            self.clear_drag_continuation(session);
            return Ok(());
        };
        if !self.stationary_edge_has_authority(owner, edge) {
            self.clear_drag_continuation(session);
            return Ok(());
        }
        let Some((target, authority, point)) = self.hover_selection_target(receipt, snapshot)
        else {
            self.clear_drag_continuation(session);
            return Ok(());
        };
        let previous = self
            .interaction
            .active_drag(session)
            .ok()
            .and_then(|drag| match drag.timed.as_ref() {
                Some(DragTimedContinuation::HoverSelection(state)) => Some(state),
                _ => None,
            })
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
            self.clear_drag_continuation(session);
            return Ok(());
        };
        // Motion updates the finite challenge even if it is already due.
        // Only an explicit continuation executes timed work, after the host
        // has drained its accepted motion/release batch in the same order.
        // Every newly observed motion revokes its previous challenge, even if
        // it remains over the same tab. Only the dwell origin is retained.
        let token = self
            .interaction
            .mint_continuation(owner, PointerContinuationSession::Drag(session), deadline)
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
            .timed = Some(DragTimedContinuation::HoverSelection(pending));
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
        if !self.journal_continuation_is_current(owner.stream(), token) {
            return Ok(Vec::new());
        }
        if !self.stationary_edge_has_authority(owner, edge) {
            self.clear_pointer_continuation(owner);
            // An optional stationary observation cannot infer owner loss from
            // missing facts. Explicit delivery and capture loss still count.
            let mut outcomes = Vec::new();
            if matches!(edge.delivery_owner(), Authority::Known(_)) {
                let (delivery, outcome) = self.gate_journal_delivery_action(
                    cause,
                    owner,
                    edge.delivery_owner(),
                    interaction_events,
                )?;
                outcomes.extend(outcome);
                if !delivery {
                    return Ok(outcomes);
                }
            }
            if matches!(edge.capture_owner(), Authority::Known(_)) {
                let (_, outcome) = self.gate_journal_capture_action(
                    cause,
                    owner,
                    edge.capture_owner(),
                    false,
                    interaction_events,
                )?;
                outcomes.extend(outcome);
            }
            return Ok(outcomes);
        }
        let PointerContinuationSession::Drag(session) = token.session else {
            return self.reduce_touch_continuation(
                cause,
                owner,
                token,
                edge,
                receipt,
                snapshot,
                interaction_events,
            );
        };
        let pending = self
            .interaction
            .active_drag(session)
            .ok()
            .filter(|drag| drag.owner == owner)
            .and_then(|drag| drag.timed.as_ref())
            .cloned();
        let Some(pending) = pending.filter(|pending| pending.request().token == token) else {
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
        let pending = match pending {
            DragTimedContinuation::TabStripEdgeScroll(pending) => {
                outcomes.extend(self.reduce_tab_strip_edge_scroll(
                    cause,
                    owner,
                    session,
                    pending,
                    edge,
                    receipt,
                    snapshot,
                    interaction_events,
                )?);
                return Ok(outcomes);
            }
            DragTimedContinuation::HoverSelection(pending) => pending,
        };
        let target = self.hover_selection_target(receipt, snapshot);
        if pending.workspace != self.version
            || target.is_none_or(|(target, authority, point)| {
                target != pending.target
                    || point != pending.request.point
                    || authority.stream() != pending.authority.stream()
                    || !authority.same_interaction_semantics(pending.authority)
            })
        {
            self.clear_drag_continuation(session);
            return Ok(outcomes);
        }
        if edge
            .metadata()
            .observed_at()
            .is_none_or(|now| now < token.deadline)
        {
            return Ok(outcomes);
        }
        self.clear_drag_continuation(session);
        outcomes.push(self.select_hovered_tab(
            cause,
            owner,
            session,
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
        let prepared = match self
            .workspace
            .prepare_transaction(policy, &[WorkspaceCommand::Select { source }])
        {
            Ok(prepared) => prepared,
            Err(TransactionError::Command { source, .. }) if source.is_expected_rejection() => {
                return Ok(InteractionOutcome::Rejected(
                    InteractionRejection::CommandRejected(source),
                ));
            }
            Err(error) => return Err(invariant(error.to_string())),
        };
        let (candidate, report) = prepared.into_snapshot_parts();
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
