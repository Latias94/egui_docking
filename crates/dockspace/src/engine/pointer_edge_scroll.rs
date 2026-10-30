//! Bounded stationary tab-strip scrolling owned by the active drag.

use super::*;
use crate::scene_manifest::{TabStripControlPlacement, TabStripKey};

impl DockEngine {
    pub(crate) fn pointer_continuation_awaits_presentation(&self, surface: SurfaceId) -> bool {
        let InteractionStatus::Dragging { session } = self.interaction.status() else {
            return false;
        };
        let Ok(drag) = self.interaction.active_drag(session) else {
            return false;
        };
        let Some(DragTimedContinuation::TabStripEdgeScroll(pending)) = drag.timed.as_ref() else {
            return false;
        };
        pending.key.surface() == surface
            && pending.workspace == self.version
            && self.pointer_provider() == Some(drag.owner.lease())
            && self.pointer_continuation_source_is_current(drag.owner)
            && self.stationary_surface_is_reachable(drag.owner, surface)
            && self
                .scene()
                .ready_candidate(surface)
                .is_some_and(|candidate| candidate.output_ticket() == pending.output)
            && !self.edge_scroll_continuation_is_current(pending)
    }

    fn tab_strip_edge_scroll_target<'snapshot>(
        &self,
        receipt: &ValidatedPointerReceiverReceipt,
        snapshot: &'snapshot JournalPresentationSnapshot,
    ) -> Option<(
        &'snapshot JournalSurfacePresentation,
        &'snapshot TabBarRecord,
        LogicalPoint,
        TabStripEdgeDirection,
    )> {
        let config = &self.presentation_authority.presentation_config;
        config.tab_strip_edge_scroll_interval()?;
        if self
            .presentation_authority
            .tab_strip_states
            .active_menu()
            .is_some()
        {
            return None;
        }
        let (presentation, point) = self.stationary_hover_presentation(receipt, snapshot)?;
        let hit = presentation
            .hit_manifest()
            .resolve_exclusive(PresentationPointerLane::Scroll, point)
            .ok()??;
        let PresentationHitRegionKind::TabStripScroll(bar) = hit.id().kind() else {
            return None;
        };
        for lane in [
            PresentationPointerLane::Click,
            PresentationPointerLane::Drag,
        ] {
            let winner = presentation
                .hit_manifest()
                .resolve_exclusive(lane, point)
                .ok()?;
            let same_strip = winner.is_none_or(|winner| match winner.id().kind() {
                PresentationHitRegionKind::TabBody(tab) => {
                    tab.root == bar.root && tab.tabs == bar.tabs
                }
                PresentationHitRegionKind::TabGroupDrag { bar: owner, .. } => owner == bar,
                PresentationHitRegionKind::PaneBody(pane) => {
                    pane.root == bar.root && pane.tabs == bar.tabs
                }
                PresentationHitRegionKind::ContainedFrameBlocker(floating) => presentation
                    .plan()
                    .contained_record(floating)
                    .is_some_and(|contained| contained.root() == bar.root),
                _ => false,
            });
            if !same_strip {
                return None;
            }
        }
        let key = TabStripStateKey::new(presentation.surface(), bar);
        if self.tab_strip_has_manual_scroll_owner(key) {
            return None;
        }
        let record = presentation
            .plan()
            .tab_bar_records()
            .iter()
            .find(|record| *record.id() == bar)?;
        let viewport = edge_content_viewport(presentation.plan(), record)?;
        if record.interaction() != TabBarInteraction::Enabled || !viewport.contains(point) {
            return None;
        }
        let band = config
            .tab_strip_edge_scroll_band()
            .min(viewport.width() * 0.5);
        if band <= 0.0 {
            return None;
        }
        let left = point.x() - viewport.x();
        let right = viewport.max().x() - point.x();
        let direction = if left <= band && left < right && record.scroll_offset() > 0.0 {
            TabStripEdgeDirection::Backward
        } else if right <= band
            && right <= left
            && record.scroll_offset() < record.maximum_scroll_offset()
        {
            TabStripEdgeDirection::Forward
        } else {
            return None;
        };
        Some((presentation, record, point, direction))
    }

    pub(super) fn edge_scroll_continuation_is_current(
        &self,
        pending: &TabStripEdgeScrollState,
    ) -> bool {
        let Some(current) = self.interaction_projection(pending.key.surface()) else {
            return false;
        };
        let authority = current.authority();
        pending.workspace == self.version
            && self
                .presentation_authority
                .tab_strip_states
                .active_menu()
                .is_none()
            && !self.tab_strip_has_manual_scroll_owner(pending.key)
            && authority.stream() == pending.authority.stream()
            && authority.endpoint() == pending.authority.endpoint()
            && authority.coordinate_generation() == pending.authority.coordinate_generation()
            && authority.matches_output(pending.output)
            && current.plan().tab_bar_records().iter().any(|bar| {
                *bar.id() == pending.key.bar()
                    && bar.viewport() == pending.viewport
                    && bar.scroll_offset() == pending.offset
                    && bar.maximum_scroll_offset() == pending.maximum
                    && bar.interaction() == TabBarInteraction::Enabled
            })
    }

    pub(super) fn advance_tab_strip_edge_scroll(
        &mut self,
        cause: ReductionCause,
        owner: GestureOwner,
        session: crate::interaction::DragSessionId,
        edge: &PointerEdge,
        receipt: &ValidatedPointerReceiverReceipt,
        snapshot: &JournalPresentationSnapshot,
    ) -> Result<bool, EngineError> {
        let Some(now) = edge
            .metadata()
            .observed_at()
            .filter(|_| self.stationary_edge_has_authority(owner, edge))
        else {
            return Ok(false);
        };
        let Some((presentation, bar, point, direction)) =
            self.tab_strip_edge_scroll_target(receipt, snapshot)
        else {
            return Ok(false);
        };
        let authority = presentation.authority();
        let key = TabStripStateKey::new(presentation.surface(), *bar.id());
        let previous =
            self.interaction
                .active_drag(session)
                .ok()
                .and_then(|drag| match drag.timed.as_ref() {
                    Some(DragTimedContinuation::TabStripEdgeScroll(state))
                        if state.workspace == self.version
                            && state.key == key
                            && state.direction == direction
                            && state.output == authority.ticket()
                            && state.authority.stream() == authority.stream() =>
                    {
                        Some(state.started_at)
                    }
                    _ => None,
                });
        let started_at = previous.unwrap_or(now);
        let Some(deadline) = self
            .presentation_authority
            .presentation_config
            .tab_strip_edge_scroll_interval()
            .and_then(|interval| started_at.checked_add(interval))
        else {
            return Ok(false);
        };
        let token = self
            .interaction
            .mint_continuation(owner, PointerContinuationSession::Drag(session), deadline)
            .map_err(|source| edge_scroll_invariant(cause, source))?;
        self.interaction
            .active_drag_mut(session)
            .map_err(|error| edge_scroll_invariant(cause, format!("{error:?}")))?
            .timed = Some(DragTimedContinuation::TabStripEdgeScroll(
            TabStripEdgeScrollState {
                request: PointerContinuationRequest {
                    token,
                    surface: key.surface(),
                    point,
                },
                authority,
                output: authority.ticket(),
                workspace: self.version,
                key,
                direction,
                viewport: bar.viewport(),
                offset: bar.scroll_offset(),
                maximum: bar.maximum_scroll_offset(),
                started_at,
            },
        ));
        Ok(true)
    }

    pub(super) fn reduce_tab_strip_edge_scroll(
        &mut self,
        cause: ReductionCause,
        owner: GestureOwner,
        session: crate::interaction::DragSessionId,
        pending: TabStripEdgeScrollState,
        edge: &PointerEdge,
        receipt: &ValidatedPointerReceiverReceipt,
        snapshot: &JournalPresentationSnapshot,
        interaction_events: &mut Vec<InteractionEvent>,
    ) -> Result<Vec<InteractionOutcome>, EngineError> {
        if !self.edge_scroll_continuation_is_current(&pending) {
            self.clear_drag_continuation(session);
            return Ok(Vec::new());
        }
        let Some((presentation, bar, point, direction)) =
            self.tab_strip_edge_scroll_target(receipt, snapshot)
        else {
            self.clear_drag_continuation(session);
            return Ok(Vec::new());
        };
        if point != pending.request.point
            || *bar.id() != pending.key.bar()
            || presentation.surface() != pending.key.surface()
            || direction != pending.direction
        {
            self.clear_drag_continuation(session);
            return Ok(Vec::new());
        }
        let Some(now) = edge
            .metadata()
            .observed_at()
            .filter(|now| *now >= pending.request.token.deadline)
        else {
            return Ok(Vec::new());
        };
        if presentation.plan().layout_facts().is_none() {
            self.clear_drag_continuation(session);
            return Ok(Vec::new());
        }
        let step = self
            .presentation_authority
            .presentation_config
            .tab_strip_scroll_line_extent();
        let delta = match direction {
            TabStripEdgeDirection::Backward => -step,
            TabStripEdgeDirection::Forward => step,
        };
        let offset = (bar.scroll_offset() + delta).clamp(0.0, bar.maximum_scroll_offset());
        let preview_was_present = self
            .interaction
            .active_drag(session)
            .is_ok_and(|drag| drag.preview().is_some());
        self.clear_drag_continuation(session);
        let outcome = self.commit_tab_strip_scroll(cause, pending.key, offset)?;
        if preview_was_present {
            interaction_events.push(InteractionEvent::new_caused(
                cause,
                self.version,
                InteractionEventKind::PreviewCleared { session },
            ));
        }
        let candidate = self
            .scene()
            .ready_candidate(pending.key.surface())
            .ok_or_else(|| edge_scroll_invariant(cause, "edge scroll lost its ready candidate"))?;
        let bar = candidate
            .plan()
            .tab_bar_records()
            .iter()
            .find(|bar| *bar.id() == pending.key.bar())
            .ok_or_else(|| edge_scroll_invariant(cause, "edge scroll lost its tab strip"))?;
        let can_continue = match direction {
            TabStripEdgeDirection::Backward => bar.scroll_offset() > 0.0,
            TabStripEdgeDirection::Forward => bar.scroll_offset() < bar.maximum_scroll_offset(),
        };
        if can_continue
            && let Some(deadline) = self
                .presentation_authority
                .presentation_config
                .tab_strip_edge_scroll_interval()
                .and_then(|interval| now.checked_add(interval))
        {
            let output = candidate.output_ticket();
            let viewport = bar.viewport();
            let offset = bar.scroll_offset();
            let maximum = bar.maximum_scroll_offset();
            let token = self
                .interaction
                .mint_continuation(owner, PointerContinuationSession::Drag(session), deadline)
                .map_err(|error| edge_scroll_invariant(cause, error))?;
            self.interaction
                .active_drag_mut(session)
                .map_err(|error| edge_scroll_invariant(cause, format!("{error:?}")))?
                .timed = Some(DragTimedContinuation::TabStripEdgeScroll(
                TabStripEdgeScrollState {
                    request: PointerContinuationRequest {
                        token,
                        ..pending.request
                    },
                    output,
                    viewport,
                    offset,
                    maximum,
                    started_at: now,
                    ..pending
                },
            ));
        }
        Ok(vec![outcome])
    }
}

fn edge_scroll_invariant(cause: ReductionCause, detail: impl std::fmt::Display) -> EngineError {
    EngineError::PointerInteractionInvariant {
        cause,
        detail: detail.to_string(),
    }
}

fn edge_content_viewport(plan: &PresentationPlan, bar: &TabBarRecord) -> Option<LogicalRect> {
    let viewport = bar.viewport();
    let metrics = plan
        .layout_facts()?
        .tab_strip(TabStripKey::new(plan.surface(), *bar.id()))?
        .controls();
    let mut left = viewport.x();
    let mut right = viewport.max().x();
    for control in plan
        .tab_strip_control_records()
        .iter()
        .filter(|control| control.id().bar() == *bar.id())
    {
        let hit = control.hit().rect();
        if hit.max().x() <= left
            || hit.x() >= right
            || hit.max().y() <= viewport.y()
            || hit.y() >= viewport.max().y()
        {
            continue;
        }
        let metric = match control.id() {
            TabStripControlId::ScrollBackward(_) => metrics?.scroll_backward(),
            TabStripControlId::ScrollForward(_) => metrics?.scroll_forward(),
            TabStripControlId::TabListMenu(_) => metrics?.tab_list_menu(),
        }?;
        match metric.placement() {
            TabStripControlPlacement::OverlayLeading => left = left.max(hit.max().x()),
            TabStripControlPlacement::OverlayTrailing => right = right.min(hit.x()),
            TabStripControlPlacement::ReservedLeading
            | TabStripControlPlacement::ReservedTrailing => {}
        }
    }
    (right > left)
        .then(|| LogicalRect::new(left, viewport.y(), right - left, viewport.height()).ok())
        .flatten()
}
