//! Conservative egui pointer-edge staging for the single-surface facade.

use dockspace::engine::{CoreHostFrame, HostFrameView};
use dockspace::geometry::LogicalPoint;
use dockspace::ids::{SurfaceId, WorkspaceEpoch};
use dockspace::intent::{Authority, AuthorityUnavailableReason, PointerButton, PointerId};
use dockspace::pointer_journal::{
    PointerCaptureOwner, PointerEdge, PointerEdgeJournal, PointerEdgeKind, PointerEdgeLocation,
    PointerEdgeSequence, PointerInputLease,
};
#[cfg(test)]
use dockspace::pointer_journal::{
    PointerProviderScope, SurfaceLocalPointerEndpoint, SurfaceLocalPointerScope,
};
use dockspace::pointer_receiver::{
    PointerReceiverCandidateRoster, PointerReceiverDelivery, PointerReceiverDeliveryDisposition,
    PointerReceiverHoverHit, PointerReceiverHoverHitDisposition, PointerReceiverObservation,
    PointerReceiverProbe, PointerReceiverProbeReceipt, PointerReceiverReceiptBatch,
    PointerReceiverUnknownReason, PresentedPointerReceiverObservation,
};
use dockspace::presentation_hit::PresentationPointerLane;
#[cfg(test)]
use dockspace::presentation_observation::PresentationHostLease;
use egui::{Context, Event, PointerButton as EguiPointerButton, Pos2, ViewportId};

use crate::DockspaceError;
use crate::receiver::{
    PaintReceiverEvidence, PaintReceiverFingerprint, PaintReceiverLookup,
    PaintReceiverRegistrations,
};
use crate::render::EguiDockRenderer;

const EGUI_PRIMARY_POINTER: PointerId = PointerId::new(1);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct EguiPointerInputEpoch {
    cumulative_frame: u64,
    cumulative_pass: u64,
    viewport: ViewportId,
    incarnation: u64,
}

impl EguiPointerInputEpoch {
    pub(crate) const fn new(
        cumulative_frame: u64,
        cumulative_pass: u64,
        viewport: ViewportId,
        incarnation: u64,
    ) -> Self {
        Self {
            cumulative_frame,
            cumulative_pass,
            viewport,
            incarnation,
        }
    }
}

#[derive(Clone)]
struct PointerBinding {
    context: Option<Context>,
    viewport: Option<ViewportId>,
    surface: SurfaceId,
    workspace_epoch: WorkspaceEpoch,
    incarnation: u64,
}

#[derive(Clone, Debug)]
struct PendingPointerEpoch {
    epoch: EguiPointerInputEpoch,
    journal: PointerEdgeJournal,
    edge_raw_event_indices: Vec<usize>,
    delivery_correlation_available: bool,
    primary_capture_after: Authority<PointerCaptureOwner>,
}

#[derive(Debug)]
pub(crate) struct PreparedPointerInput {
    epoch: EguiPointerInputEpoch,
    provider: PointerInputLease,
    journal: PointerEdgeJournal,
    edge_raw_event_indices: Vec<usize>,
    delivery_correlation_available: bool,
    primary_capture_after: Authority<PointerCaptureOwner>,
}

impl PreparedPointerInput {
    pub(crate) const fn provider(&self) -> PointerInputLease {
        self.provider
    }

    pub(crate) fn journal_segments(&self) -> Result<Vec<PointerEdgeJournal>, DockspaceError> {
        if self.journal.edges().is_empty() {
            return Ok(vec![self.journal.clone()]);
        }
        let mut previous = self.journal.previous();
        self.journal
            .edges()
            .iter()
            .map(|edge| {
                let through = edge.sequence();
                let segment = PointerEdgeJournal::new(previous, through, vec![edge.clone()])?;
                previous = through;
                Ok(segment)
            })
            .collect()
    }

    pub(crate) fn segment_index_before_raw_event(&self, raw_event_index: usize) -> usize {
        self.edge_raw_event_indices
            .iter()
            .take_while(|edge_index| **edge_index < raw_event_index)
            .count()
    }

    pub(crate) fn prepare_receipts_for(
        &self,
        candidates: &PointerReceiverCandidateRoster,
        view: HostFrameView<'_>,
        surface: SurfaceId,
        receiver_store: &EguiDockRenderer,
        registrations: &PaintReceiverRegistrations,
        journal: &PointerEdgeJournal,
    ) -> Result<PointerReceiverReceiptBatch, DockspaceError> {
        let Some(projection) = view.interaction_projection(surface) else {
            return self.prepare_unavailable_receipts_for(candidates);
        };
        let receipts = candidates
            .candidates()
            .iter()
            .map(|candidate| -> Result<_, DockspaceError> {
                if !candidate.receiver_is_applicable() {
                    return Ok(candidate.receipt(PointerReceiverObservation::NotApplicable));
                }
                let edge = journal
                    .edges()
                    .iter()
                    .find(|edge| edge.sequence() == candidate.id().sequence())
                    .ok_or(DockspaceError::PointerReceiverEdgeMissing {
                        sequence: candidate.id().sequence(),
                    })?;
                let mut probes = Vec::new();
                if candidate.probes().requires(PointerReceiverProbe::Delivery) {
                    let correlation_unavailable = !self.delivery_correlation_available
                        && matches!(
                            edge.kind(),
                            PointerEdgeKind::ButtonPressed(PointerButton::Primary)
                                | PointerEdgeKind::ButtonReleased(PointerButton::Primary)
                        );
                    let (click, drag) = if correlation_unavailable {
                        let unavailable = PointerReceiverDeliveryDisposition::Unknown(
                            PointerReceiverUnknownReason::EventCorrelationUnavailable,
                        );
                        (unavailable, unavailable)
                    } else {
                        (
                            delivery_disposition(
                                registrations,
                                receiver_store,
                                projection,
                                edge.kind(),
                                false,
                                candidate.hover_point(),
                            ),
                            delivery_disposition(
                                registrations,
                                receiver_store,
                                projection,
                                edge.kind(),
                                true,
                                candidate.hover_point(),
                            ),
                        )
                    };
                    probes.push(PointerReceiverProbeReceipt::Delivery(
                        PointerReceiverDelivery::from_lanes(projection, click, drag)?,
                    ));
                }
                if candidate.probes().requires(PointerReceiverProbe::HoverHit) {
                    probes.push(PointerReceiverProbeReceipt::HoverHit(hover_hit(
                        registrations,
                        receiver_store,
                        view,
                        surface,
                        projection,
                        candidate.hover_point(),
                    )?));
                }
                Ok(candidate.receipt(PointerReceiverObservation::Presented(
                    PresentedPointerReceiverObservation::new(probes)?,
                )))
            })
            .collect::<Result<Vec<_>, DockspaceError>>()?;
        Ok(PointerReceiverReceiptBatch::new(receipts)?)
    }

    pub(crate) fn prepare_unavailable_receipts_for(
        &self,
        candidates: &PointerReceiverCandidateRoster,
    ) -> Result<PointerReceiverReceiptBatch, DockspaceError> {
        let receipts = candidates.candidates().iter().map(|candidate| {
            if candidate.receiver_is_applicable() {
                candidate.receipt(PointerReceiverObservation::Unknown(
                    PointerReceiverUnknownReason::PresentationAuthorityUnavailable,
                ))
            } else {
                candidate.receipt(PointerReceiverObservation::NotApplicable)
            }
        });
        Ok(PointerReceiverReceiptBatch::new(receipts)?)
    }
}

fn hover_hit(
    registrations: &PaintReceiverRegistrations,
    receiver_store: &EguiDockRenderer,
    view: HostFrameView<'_>,
    surface: SurfaceId,
    projection: dockspace::scene::SurfaceInteractionProjection<'_>,
    point: Option<LogicalPoint>,
) -> Result<PointerReceiverHoverHit, DockspaceError> {
    let Some(point) = point else {
        return Ok(PointerReceiverHoverHit::unknown(
            PointerReceiverUnknownReason::EventCorrelationUnavailable,
        ));
    };
    let core_winner = view.resolve_hover_drop_receiver(surface, point)?;
    let mut winner = None;
    for receiver in registrations.hover_receivers() {
        let lookup = receiver_store.resolve_receiver_for_latest_presented_pass(
            projection.output_ticket(),
            projection.authority(),
            registrations.viewport(),
            registrations.widget_pass_nr(),
            receiver,
        );
        let PaintReceiverLookup::Dock(kind) = lookup else {
            return Ok(PointerReceiverHoverHit::unknown(
                PointerReceiverUnknownReason::PresentationAuthorityUnavailable,
            ));
        };
        let mut regions = projection
            .hit_manifest()
            .regions()
            .iter()
            .filter(|region| region.id().kind() == kind);
        let Some(region) = regions.next() else {
            return Ok(PointerReceiverHoverHit::unknown(
                PointerReceiverUnknownReason::PresentationAuthorityUnavailable,
            ));
        };
        if regions.next().is_some() {
            return Ok(PointerReceiverHoverHit::unknown(
                PointerReceiverUnknownReason::EventCorrelationUnavailable,
            ));
        }
        if !region.lanes().contains(PresentationPointerLane::HoverDrop)
            || !region.hit().contains(point)
        {
            continue;
        }
        winner = match winner {
            None => Some(region),
            Some(current) => match region.stack().cmp(&current.stack()) {
                std::cmp::Ordering::Greater => Some(region),
                std::cmp::Ordering::Less => Some(current),
                std::cmp::Ordering::Equal if region.id() == current.id() => Some(current),
                std::cmp::Ordering::Equal => {
                    return Ok(PointerReceiverHoverHit::unknown(
                        PointerReceiverUnknownReason::LayerAuthorityUnavailable,
                    ));
                }
            },
        };
    }
    if let Some(region) = winner {
        if core_winner.disposition() == PointerReceiverHoverHitDisposition::Dock(region.id()) {
            return Ok(core_winner);
        }
    } else if core_winner.disposition() == PointerReceiverHoverHitDisposition::NoReceiver {
        #[allow(
            clippy::cast_possible_truncation,
            reason = "the event point originated as an egui f32 position"
        )]
        let egui_point = Pos2::new(point.x() as f32, point.y() as f32);
        if registrations
            .framework_top_layer_at(egui_point)
            .flatten()
            .is_some_and(|layer| registrations.owns_layer(layer))
        {
            return Ok(core_winner);
        }
    }
    Ok(PointerReceiverHoverHit::unknown(
        PointerReceiverUnknownReason::LayerAuthorityUnavailable,
    ))
}

fn delivery_disposition(
    registrations: &PaintReceiverRegistrations,
    receiver_store: &EguiDockRenderer,
    projection: dockspace::scene::SurfaceInteractionProjection<'_>,
    edge: PointerEdgeKind,
    drag_lane: bool,
    point: Option<LogicalPoint>,
) -> PointerReceiverDeliveryDisposition {
    let receivers: Box<dyn Iterator<Item = PaintReceiverFingerprint> + '_> = match edge {
        PointerEdgeKind::ButtonPressed(PointerButton::Primary) => {
            Box::new(registrations.primary_press_receivers(drag_lane))
        }
        PointerEdgeKind::ButtonReleased(PointerButton::Primary) => {
            Box::new(registrations.primary_release_receivers(drag_lane))
        }
        PointerEdgeKind::Moved
        | PointerEdgeKind::CaptureChanged
        | PointerEdgeKind::StreamCancelled(_)
        | PointerEdgeKind::Scrolled(_)
        | PointerEdgeKind::ButtonPressed(_)
        | PointerEdgeKind::ButtonReleased(_) => Box::new(std::iter::empty()),
    };
    let lane = if drag_lane {
        PresentationPointerLane::Drag
    } else {
        PresentationPointerLane::Click
    };
    let mut winner = None;
    for receiver in receivers {
        let lookup = receiver_store.resolve_receiver_for_latest_presented_pass(
            projection.output_ticket(),
            projection.authority(),
            registrations.viewport(),
            registrations.widget_pass_nr(),
            receiver,
        );
        let PaintReceiverLookup::Dock(kind) = lookup else {
            return PointerReceiverDeliveryDisposition::Unknown(
                PointerReceiverUnknownReason::PresentationAuthorityUnavailable,
            );
        };
        let mut regions = projection
            .hit_manifest()
            .regions()
            .iter()
            .filter(|region| region.id().kind() == kind && region.lanes().contains(lane))
            .filter(|region| point.is_none_or(|point| region.hit().contains(point)));
        let Some(region) = regions.next() else {
            return PointerReceiverDeliveryDisposition::Unknown(
                PointerReceiverUnknownReason::PresentationAuthorityUnavailable,
            );
        };
        if regions.next().is_some() {
            return PointerReceiverDeliveryDisposition::Unknown(
                PointerReceiverUnknownReason::EventCorrelationUnavailable,
            );
        }
        winner = match winner {
            None => Some((receiver, region)),
            Some((current_receiver, current)) => match region.stack().cmp(&current.stack()) {
                std::cmp::Ordering::Greater => Some((receiver, region)),
                std::cmp::Ordering::Less => Some((current_receiver, current)),
                std::cmp::Ordering::Equal if region.id() == current.id() => {
                    Some((current_receiver, current))
                }
                std::cmp::Ordering::Equal => {
                    return PointerReceiverDeliveryDisposition::Unknown(
                        PointerReceiverUnknownReason::LayerAuthorityUnavailable,
                    );
                }
            },
        };
    }
    let Some((receiver, region)) = winner else {
        return PointerReceiverDeliveryDisposition::Unknown(
            PointerReceiverUnknownReason::EventCorrelationUnavailable,
        );
    };
    if let Some(point) = point {
        #[allow(
            clippy::cast_possible_truncation,
            reason = "the event point originated as an egui f32 position"
        )]
        let egui_point = Pos2::new(point.x() as f32, point.y() as f32);
        if registrations.framework_top_layer_at(egui_point) != Some(Some(receiver.layer_id())) {
            return PointerReceiverDeliveryDisposition::Unknown(
                PointerReceiverUnknownReason::LayerAuthorityUnavailable,
            );
        }
    }
    PointerReceiverDeliveryDisposition::Dock(region.id())
}

pub(crate) struct EguiPointerInput {
    provider: Option<PointerInputLease>,
    committed_through: PointerEdgeSequence,
    pending: Option<PendingPointerEpoch>,
    delivered_epoch: Option<EguiPointerInputEpoch>,
    binding: Option<PointerBinding>,
    next_incarnation: u64,
    primary_capture: Authority<PointerCaptureOwner>,
}

impl Default for EguiPointerInput {
    fn default() -> Self {
        Self {
            provider: None,
            committed_through: PointerEdgeSequence::new(0),
            pending: None,
            delivered_epoch: None,
            binding: None,
            next_incarnation: 0,
            primary_capture: Authority::Unknown(AuthorityUnavailableReason::ProviderUnavailable),
        }
    }
}

impl EguiPointerInput {
    pub(crate) const fn provider(&self) -> Option<PointerInputLease> {
        self.provider
    }

    pub(crate) fn scope_changed(
        &self,
        context: &Context,
        viewport: ViewportId,
        surface: SurfaceId,
        workspace_epoch: WorkspaceEpoch,
    ) -> bool {
        let Some(binding) = self.binding.as_ref() else {
            return self.provider.is_some();
        };
        self.provider.is_some()
            && (binding.viewport.is_some_and(|bound| bound != viewport)
                || binding.surface != surface
                || binding.workspace_epoch != workspace_epoch
                || binding
                    .context
                    .as_ref()
                    .is_some_and(|bound| !bound.eq(context)))
    }

    pub(crate) fn scope_changed_unbound(
        &self,
        surface: SurfaceId,
        workspace_epoch: WorkspaceEpoch,
    ) -> bool {
        let Some(binding) = self.binding.as_ref() else {
            return self.provider.is_some();
        };
        self.provider.is_some()
            && (binding.surface != surface || binding.workspace_epoch != workspace_epoch)
    }

    pub(crate) fn current_epoch(
        &self,
        cumulative_frame: u64,
        cumulative_pass: u64,
        viewport: ViewportId,
    ) -> Option<EguiPointerInputEpoch> {
        self.binding.as_ref().and_then(|binding| {
            (binding.viewport == Some(viewport)).then(|| {
                EguiPointerInputEpoch::new(
                    cumulative_frame,
                    cumulative_pass,
                    viewport,
                    binding.incarnation,
                )
            })
        })
    }

    pub(crate) fn bind_context(
        &mut self,
        context: &Context,
        viewport: ViewportId,
        surface: SurfaceId,
        workspace_epoch: WorkspaceEpoch,
    ) -> Result<(), DockspaceError> {
        let Some(binding) = self.binding.as_mut() else {
            return Err(DockspaceError::PointerInputBindingMissing);
        };
        if binding.surface != surface || binding.workspace_epoch != workspace_epoch {
            return Err(DockspaceError::PointerInputBindingStale);
        }
        if binding
            .context
            .as_ref()
            .is_some_and(|bound| !bound.eq(context))
            || binding.viewport.is_some_and(|bound| bound != viewport)
        {
            return Err(DockspaceError::PointerInputBindingStale);
        }
        binding.context = Some(context.clone());
        binding.viewport = Some(viewport);
        Ok(())
    }

    /// Drops an uncommitted input epoch after its enclosing host frame aborts.
    ///
    /// The core lease is retired by the owning `Dockspace`; this method only clears
    /// adapter-side replay state so a later egui epoch can enroll a fresh lease.
    pub(crate) fn clear_after_abort(&mut self) {
        self.provider = None;
        self.committed_through = PointerEdgeSequence::new(0);
        self.pending = None;
        self.delivered_epoch = None;
        self.binding = None;
        self.primary_capture = Authority::Unknown(AuthorityUnavailableReason::ProviderUnavailable);
    }

    /// Discards input staged by an uncommitted host-frame attempt.
    ///
    /// The provider lease and committed watermark stay live, so aborting an
    /// adapter transaction does not publish an unrelated core transition.
    pub(crate) fn discard_pending_epoch(&mut self) {
        self.pending = None;
        self.delivered_epoch = None;
    }

    pub(crate) fn submit_empty_interval(
        &self,
        frame: &mut CoreHostFrame,
    ) -> Result<(), DockspaceError> {
        let Some(provider) = self.provider else {
            return Ok(());
        };
        if self.pending.is_some() {
            return Err(DockspaceError::PointerInputControlDuringPendingEpoch);
        }
        frame.submit_pointer_journal(
            provider,
            PointerEdgeJournal::new(self.committed_through, self.committed_through, Vec::new())?,
        )?;
        debug_assert!(
            frame
                .pointer_receiver_candidates()
                .is_some_and(|roster| roster.candidates().is_empty()),
            "an empty pointer interval freezes an empty candidate roster",
        );
        frame.submit_pointer_receiver_receipts(PointerReceiverReceiptBatch::new([])?)?;
        Ok(())
    }

    pub(crate) fn install_unbound(
        &mut self,
        provider: PointerInputLease,
        committed_through: PointerEdgeSequence,
        surface: SurfaceId,
        workspace_epoch: WorkspaceEpoch,
    ) -> Result<(), DockspaceError> {
        let incarnation = self
            .next_incarnation
            .checked_add(1)
            .ok_or(DockspaceError::PointerAdapterIncarnationExhausted)?;
        self.next_incarnation = incarnation;
        self.provider = Some(provider);
        self.committed_through = committed_through;
        self.pending = None;
        self.delivered_epoch = None;
        self.binding = Some(PointerBinding {
            context: None,
            viewport: None,
            surface,
            workspace_epoch,
            incarnation,
        });
        self.primary_capture = Authority::Unknown(AuthorityUnavailableReason::ProviderUnavailable);
        Ok(())
    }

    pub(crate) fn prepare(
        &mut self,
        context: &Context,
        epoch: EguiPointerInputEpoch,
        registrations: Option<&PaintReceiverRegistrations>,
    ) -> Result<Option<PreparedPointerInput>, DockspaceError> {
        self.prepare_inner(context, epoch, registrations, None)
    }

    pub(crate) fn prepare_captured_events(
        &mut self,
        context: &Context,
        epoch: EguiPointerInputEpoch,
        registrations: Option<&PaintReceiverRegistrations>,
        events: &[Event],
    ) -> Result<Option<PreparedPointerInput>, DockspaceError> {
        self.prepare_inner(context, epoch, registrations, Some(events))
    }

    fn prepare_inner(
        &mut self,
        context: &Context,
        epoch: EguiPointerInputEpoch,
        registrations: Option<&PaintReceiverRegistrations>,
        captured_events: Option<&[Event]>,
    ) -> Result<Option<PreparedPointerInput>, DockspaceError> {
        let Some(provider) = self.provider else {
            return Ok(None);
        };
        let Some(binding) = self.binding.as_ref() else {
            return Err(DockspaceError::PointerInputBindingMissing);
        };
        if binding.incarnation != epoch.incarnation {
            return Err(DockspaceError::PointerInputBindingStale);
        }
        if binding.viewport != Some(epoch.viewport)
            || binding
                .context
                .as_ref()
                .is_none_or(|bound| !bound.eq(context))
        {
            return Err(DockspaceError::PointerInputBindingStale);
        }
        if self
            .pending
            .as_ref()
            .is_some_and(|pending| pending.epoch != epoch)
        {
            return Err(DockspaceError::PointerInputEpochAdvancedBeforeCommit);
        }
        if self.pending.is_none() && self.delivered_epoch != Some(epoch) {
            let mut cursor = self.committed_through;
            let events = captured_events.map_or_else(
                || context.input(|input| input.events.clone()),
                <[Event]>::to_vec,
            );
            let primary_event_count = events
                .iter()
                .filter(|event| {
                    matches!(
                        event,
                        Event::PointerButton {
                            button: EguiPointerButton::Primary,
                            ..
                        }
                    )
                })
                .count();
            let any_button_down_after_epoch = context.input(|input| input.pointer.any_down());
            let mut primary_capture = self.primary_capture;
            let mut edges = Vec::new();
            let mut edge_raw_event_indices = Vec::new();
            for (raw_event_index, event) in events.iter().enumerate() {
                let capture = capture_after_event(
                    event,
                    registrations,
                    primary_event_count,
                    any_button_down_after_epoch,
                    primary_capture,
                );
                if matches!(
                    event,
                    Event::PointerButton {
                        button: EguiPointerButton::Primary,
                        ..
                    }
                ) {
                    primary_capture = capture;
                }
                if let Some(edge) = pointer_edge(event, &mut cursor, capture)? {
                    edge_raw_event_indices.push(raw_event_index);
                    edges.push(edge);
                }
            }
            let journal = PointerEdgeJournal::new(self.committed_through, cursor, edges)?;
            self.pending = Some(PendingPointerEpoch {
                epoch,
                journal,
                edge_raw_event_indices,
                delivery_correlation_available: primary_event_count <= 1,
                primary_capture_after: primary_capture,
            });
        }
        let (journal, edge_raw_event_indices) = if let Some(pending) = self.pending.as_ref() {
            (
                pending.journal.clone(),
                pending.edge_raw_event_indices.clone(),
            )
        } else {
            (
                PointerEdgeJournal::new(
                    self.committed_through,
                    self.committed_through,
                    Vec::new(),
                )?,
                Vec::new(),
            )
        };
        Ok(Some(PreparedPointerInput {
            epoch,
            provider,
            journal,
            edge_raw_event_indices,
            delivery_correlation_available: self
                .pending
                .as_ref()
                .is_none_or(|pending| pending.delivery_correlation_available),
            primary_capture_after: self
                .pending
                .as_ref()
                .map_or(self.primary_capture, |pending| {
                    pending.primary_capture_after
                }),
        }))
    }

    pub(crate) fn commit(&mut self, prepared: &PreparedPointerInput) {
        if self
            .pending
            .as_ref()
            .is_some_and(|pending| pending.epoch == prepared.epoch)
        {
            self.committed_through = prepared.journal.through();
            self.pending = None;
            self.delivered_epoch = Some(prepared.epoch);
            self.primary_capture = prepared.primary_capture_after;
        }
    }
}

#[cfg(test)]
pub(crate) fn enroll_surface_local_provider(
    engine: &mut dockspace::engine::DockEngine,
    state: &mut EguiPointerInput,
    host: PresentationHostLease,
    surface: SurfaceId,
    workspace_epoch: WorkspaceEpoch,
) -> Result<(), DockspaceError> {
    let watermark = PointerEdgeSequence::new(0);
    let provider = engine.create_pointer_provider(
        PointerProviderScope::SurfaceLocal(SurfaceLocalPointerScope::new(
            host,
            SurfaceLocalPointerEndpoint::Logical(surface),
        )),
        watermark,
    )?;
    state.install_unbound(provider, watermark, surface, workspace_epoch)
}

#[cfg(test)]
pub(crate) fn install_surface_local_provider(
    engine: &mut dockspace::engine::DockEngine,
    state: &mut EguiPointerInput,
    host: PresentationHostLease,
    surface: SurfaceId,
    context: &Context,
    viewport: ViewportId,
    workspace_epoch: WorkspaceEpoch,
) -> Result<(), DockspaceError> {
    enroll_surface_local_provider(engine, state, host, surface, workspace_epoch)?;
    state.bind_context(context, viewport, surface, workspace_epoch)
}

fn pointer_edge(
    event: &Event,
    cursor: &mut PointerEdgeSequence,
    capture: Authority<PointerCaptureOwner>,
) -> Result<Option<PointerEdge>, DockspaceError> {
    let (kind, position) = match event {
        Event::PointerMoved(position) => (PointerEdgeKind::Moved, Some(*position)),
        Event::PointerButton {
            pos,
            button,
            pressed,
            ..
        } => {
            let button = pointer_button(*button);
            let kind = if *pressed {
                PointerEdgeKind::ButtonPressed(button)
            } else {
                PointerEdgeKind::ButtonReleased(button)
            };
            (kind, Some(*pos))
        }
        // A viewport-local leave does not prove global capture or stream termination.
        Event::PointerGone => return Ok(None),
        _ => return Ok(None),
    };
    let sequence = cursor
        .checked_next()
        .ok_or(DockspaceError::PointerEdgeSequenceExhausted { after: *cursor })?;
    *cursor = sequence;
    Ok(Some(PointerEdge::new(
        sequence,
        EGUI_PRIMARY_POINTER,
        kind,
        PointerEdgeLocation::SurfaceLocal {
            position: position.and_then(to_logical_point).map_or(
                Authority::Unknown(AuthorityUnavailableReason::CoordinateUnavailable),
                Authority::Known,
            ),
        },
        capture,
    )))
}

fn capture_after_event(
    event: &Event,
    registrations: Option<&PaintReceiverRegistrations>,
    primary_event_count: usize,
    any_button_down_after_epoch: bool,
    current: Authority<PointerCaptureOwner>,
) -> Authority<PointerCaptureOwner> {
    match event {
        Event::PointerButton {
            button: EguiPointerButton::Primary,
            pressed: true,
            ..
        } if primary_event_count == 1
            && registrations.is_some_and(primary_press_has_exact_receiver) =>
        {
            Authority::Known(PointerCaptureOwner::ProviderEndpoint)
        }
        Event::PointerButton {
            button: EguiPointerButton::Primary,
            pressed: false,
            ..
        } if primary_event_count == 1 && !any_button_down_after_epoch => {
            Authority::Known(PointerCaptureOwner::None)
        }
        Event::PointerButton {
            button: EguiPointerButton::Primary,
            ..
        } => Authority::Unknown(AuthorityUnavailableReason::ProviderUnavailable),
        Event::PointerMoved(_) => current,
        _ => current,
    }
}

fn primary_press_has_exact_receiver(registrations: &PaintReceiverRegistrations) -> bool {
    let click = registrations.primary_press_receiver(false);
    let drag = registrations.primary_press_receiver(true);
    match (click, drag) {
        (PaintReceiverEvidence::Exact(click), PaintReceiverEvidence::Exact(drag)) => click == drag,
        (PaintReceiverEvidence::Exact(_), PaintReceiverEvidence::Absent)
        | (PaintReceiverEvidence::Absent, PaintReceiverEvidence::Exact(_)) => true,
        (PaintReceiverEvidence::Exact(_), PaintReceiverEvidence::Ambiguous)
        | (PaintReceiverEvidence::Ambiguous, PaintReceiverEvidence::Exact(_))
        | (PaintReceiverEvidence::Absent, PaintReceiverEvidence::Absent)
        | (PaintReceiverEvidence::Absent, PaintReceiverEvidence::Ambiguous)
        | (PaintReceiverEvidence::Ambiguous, PaintReceiverEvidence::Absent)
        | (PaintReceiverEvidence::Ambiguous, PaintReceiverEvidence::Ambiguous) => false,
    }
}

fn pointer_button(button: EguiPointerButton) -> PointerButton {
    match button {
        EguiPointerButton::Primary => PointerButton::Primary,
        EguiPointerButton::Secondary => PointerButton::Secondary,
        EguiPointerButton::Middle => PointerButton::Middle,
        EguiPointerButton::Extra1 => PointerButton::Other(1),
        EguiPointerButton::Extra2 => PointerButton::Other(2),
    }
}

fn to_logical_point(point: Pos2) -> Option<LogicalPoint> {
    LogicalPoint::new(f64::from(point.x), f64::from(point.y)).ok()
}

#[cfg(test)]
mod tests {
    use dockspace::graph::{Node, RootRecord, SurfacePresentation, Workspace};
    use dockspace::ids::{ItemId, RootId};
    use dockspace::presentation_observation::HostPresentationObservation;
    use egui::{Modifiers, RawInput, Rect, pos2, vec2};

    use super::*;

    const SURFACE: SurfaceId = SurfaceId::new(1);
    const ROOT: RootId = RootId::new(2);
    const ITEM: ItemId = ItemId::new(3);

    fn state(context: &Context) -> EguiPointerInput {
        let mut builder = Workspace::builder();
        let tabs = builder.insert_node(Node::tabs([ITEM]));
        builder.set_root(ROOT, RootRecord::new(tabs));
        builder.set_surface(SURFACE, SurfacePresentation::with_main(ROOT));
        let workspace = builder.build().expect("fixture workspace is valid");
        let mut engine = dockspace::engine::DockEngine::new(workspace, Default::default())
            .expect("fixture engine builds");
        let host = engine
            .create_presentation_host()
            .expect("fixture presentation host is minted");
        let mut state = EguiPointerInput::default();
        install_surface_local_provider(
            &mut engine,
            &mut state,
            host,
            SURFACE,
            context,
            ViewportId::ROOT,
            WorkspaceEpoch::new(0),
        )
        .expect("surface-local provider is admitted");
        state
    }

    fn input() -> RawInput {
        RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(200.0, 120.0))),
            events: vec![
                Event::PointerMoved(pos2(20.0, 10.0)),
                Event::PointerButton {
                    pos: pos2(20.0, 10.0),
                    button: EguiPointerButton::Primary,
                    pressed: true,
                    modifiers: Modifiers::NONE,
                },
            ]
            .into_iter()
            .map(Into::into)
            .collect(),
            ..RawInput::default()
        }
    }

    #[test]
    fn failed_epoch_retry_reuses_exact_journal_and_watermark() {
        let context = Context::default();
        let mut state = state(&context);
        let epoch = EguiPointerInputEpoch::new(7, 0, ViewportId::ROOT, 1);
        let mut first = None;
        let mut retry = None;
        let _ = context.run_ui(input(), |_ui| {
            first = state
                .prepare(&context, epoch, None)
                .expect("first staging succeeds");
            retry = state
                .prepare(&context, epoch, None)
                .expect("same epoch retry succeeds");
        });
        let first = first.expect("provider produces a journal");
        let retry = retry.expect("provider produces a retry journal");
        assert_eq!(first.journal, retry.journal);
        assert_eq!(first.journal.previous(), PointerEdgeSequence::new(0));
        assert_eq!(first.journal.through(), PointerEdgeSequence::new(2));
    }

    #[test]
    fn committed_epoch_is_not_redelivered_on_a_later_pass() {
        let context = Context::default();
        let mut state = state(&context);
        let epoch = EguiPointerInputEpoch::new(11, 0, ViewportId::ROOT, 1);
        let mut first = None;
        let _ = context.run_ui(input(), |_ui| {
            first = state
                .prepare(&context, epoch, None)
                .expect("first pass stages");
        });
        let first = first.expect("provider produces a journal");
        state.commit(&first);

        let mut repeated = None;
        let _ = context.run_ui(input(), |_ui| {
            repeated = state
                .prepare(&context, epoch, None)
                .expect("later pass stages its mandatory empty journal");
        });
        let repeated = repeated.expect("provider remains active");
        assert!(repeated.journal.is_empty());
        assert_eq!(repeated.journal.previous(), PointerEdgeSequence::new(2));
        assert_eq!(repeated.journal.through(), PointerEdgeSequence::new(2));
    }

    #[test]
    fn captured_outer_events_survive_an_empty_final_pass_and_commit_once() {
        let context = Context::default();
        let mut state = state(&context);
        let epoch = EguiPointerInputEpoch::new(12, 0, ViewportId::ROOT, 1);
        #[cfg(egui_backend_event_envelope)]
        let captured = input()
            .events
            .into_iter()
            .map(egui::EventEnvelope::into_event)
            .collect::<Vec<_>>();
        #[cfg(not(egui_backend_event_envelope))]
        let captured = input().events;
        let mut first = None;
        let _ = context.run_ui(RawInput::default(), |_ui| {
            first = state
                .prepare_captured_events(&context, epoch, None, &captured)
                .expect("the outer-host event snapshot stages");
        });
        let first = first.expect("the provider produces a journal");
        assert_eq!(first.journal.edges().len(), 2);
        state.commit(&first);

        let mut repeated = None;
        let _ = context.run_ui(RawInput::default(), |_ui| {
            repeated = state
                .prepare_captured_events(&context, epoch, None, &captured)
                .expect("a repeated render pass remains valid");
        });
        let repeated = repeated.expect("the provider remains active");
        assert!(repeated.journal.is_empty());
        assert_eq!(repeated.journal.previous(), PointerEdgeSequence::new(2));
        assert_eq!(repeated.journal.through(), PointerEdgeSequence::new(2));
    }

    #[test]
    fn a_new_input_epoch_cannot_replace_an_uncommitted_epoch() {
        let context = Context::default();
        let mut state = state(&context);
        let first_epoch = EguiPointerInputEpoch::new(13, 0, ViewportId::ROOT, 1);
        let mut result = None;
        let _ = context.run_ui(input(), |_ui| {
            let _ = state
                .prepare(&context, first_epoch, None)
                .expect("first epoch stages");
            result = Some(state.prepare(
                &context,
                EguiPointerInputEpoch::new(14, 0, ViewportId::ROOT, 1),
                None,
            ));
        });
        assert!(matches!(
            result.expect("second attempt ran"),
            Err(DockspaceError::PointerInputEpochAdvancedBeforeCommit)
        ));
    }

    #[test]
    fn application_control_empty_interval_can_retry_without_advancing_watermark() {
        let mut builder = Workspace::builder();
        let tabs = builder.insert_node(Node::tabs([ITEM]));
        builder.set_root(ROOT, RootRecord::new(tabs));
        builder.set_surface(SURFACE, SurfacePresentation::with_main(ROOT));
        let workspace = builder.build().expect("fixture workspace is valid");
        let mut engine = dockspace::engine::DockEngine::new(workspace, Default::default())
            .expect("fixture engine builds");
        let host = engine
            .create_presentation_host()
            .expect("fixture presentation host is minted");
        let mut state = EguiPointerInput::default();
        install_surface_local_provider(
            &mut engine,
            &mut state,
            host,
            SURFACE,
            &Context::default(),
            ViewportId::ROOT,
            WorkspaceEpoch::new(0),
        )
        .expect("surface-local provider is admitted");

        for _ in 0..2 {
            let mut prelude = engine
                .begin_host_frame(host)
                .expect("retry frame begins from unchanged core watermark");
            prelude
                .submit_presentation_observation(HostPresentationObservation::NoUpdate)
                .expect("observation phase is satisfied");
            let mut frame = prelude.seal(&engine).expect("retry frame seals");
            state
                .submit_empty_interval(&mut frame)
                .expect("the same empty interval stages on every failed attempt");
            assert!(
                frame.pointer_receiver_candidates().is_none(),
                "the empty candidate roster is consumed with its exact empty receipt"
            );
        }
        assert_eq!(state.committed_through, PointerEdgeSequence::new(0));
    }

    #[test]
    fn sequence_exhaustion_rejects_the_complete_epoch_without_staging_or_advancing() {
        let context = Context::default();
        let mut state = state(&context);
        state.committed_through = PointerEdgeSequence::new(u64::MAX);
        let epoch = EguiPointerInputEpoch::new(17, 0, ViewportId::ROOT, 1);
        let mut result = None;
        let _ = context.run_ui(input(), |_ui| {
            result = Some(state.prepare(&context, epoch, None));
        });
        assert!(matches!(
            result.expect("staging was attempted"),
            Err(DockspaceError::PointerEdgeSequenceExhausted { after })
                if after == PointerEdgeSequence::new(u64::MAX)
        ));
        assert_eq!(state.committed_through, PointerEdgeSequence::new(u64::MAX));
        assert!(state.pending.is_none());
        assert!(state.delivered_epoch.is_none());
    }

    #[test]
    fn pointer_gone_does_not_invent_global_stream_termination() {
        let mut cursor = PointerEdgeSequence::new(0);
        let edge = pointer_edge(
            &Event::PointerGone,
            &mut cursor,
            Authority::Unknown(AuthorityUnavailableReason::ProviderUnavailable),
        )
        .expect("pointer-gone filtering is infallible");
        assert!(edge.is_none());
        assert_eq!(cursor, PointerEdgeSequence::new(0));
    }

    #[test]
    fn official_egui_wheel_does_not_invent_a_scrolled_edge() {
        let mut cursor = PointerEdgeSequence::new(0);
        let edge = pointer_edge(
            &Event::MouseWheel {
                unit: egui::MouseWheelUnit::Point,
                delta: vec2(0.0, -24.0),
                phase: egui::TouchPhase::Move,
                modifiers: Modifiers::NONE,
            },
            &mut cursor,
            Authority::Unknown(AuthorityUnavailableReason::ProviderUnavailable),
        )
        .expect("filtering an authority-incomplete wheel event is infallible");

        assert!(edge.is_none());
        assert_eq!(cursor, PointerEdgeSequence::new(0));
    }

    #[test]
    fn raw_event_position_places_escape_between_pointer_edges() {
        let context = Context::default();
        let mut state = state(&context);
        let events = vec![
            Event::PointerMoved(pos2(20.0, 10.0)),
            Event::Key {
                key: egui::Key::Escape,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: Modifiers::NONE,
            },
            Event::PointerButton {
                pos: pos2(20.0, 10.0),
                button: EguiPointerButton::Primary,
                pressed: false,
                modifiers: Modifiers::NONE,
            },
        ];
        let mut prepared = None;
        let _ = context.run_ui(RawInput::default(), |_ui| {
            prepared = state
                .prepare_captured_events(
                    &context,
                    EguiPointerInputEpoch::new(19, 0, ViewportId::ROOT, 1),
                    None,
                    &events,
                )
                .expect("captured events stage");
        });
        let prepared = prepared.expect("the provider produces a journal");
        assert_eq!(prepared.journal.edges().len(), 2);
        assert_eq!(prepared.segment_index_before_raw_event(1), 1);
    }

    #[test]
    fn raw_event_position_places_escape_after_an_earlier_release() {
        let context = Context::default();
        let mut state = state(&context);
        let events = vec![
            Event::PointerButton {
                pos: pos2(20.0, 10.0),
                button: EguiPointerButton::Primary,
                pressed: false,
                modifiers: Modifiers::NONE,
            },
            Event::Key {
                key: egui::Key::Escape,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: Modifiers::NONE,
            },
        ];
        let mut prepared = None;
        let _ = context.run_ui(RawInput::default(), |_ui| {
            prepared = state
                .prepare_captured_events(
                    &context,
                    EguiPointerInputEpoch::new(20, 0, ViewportId::ROOT, 1),
                    None,
                    &events,
                )
                .expect("captured events stage");
        });
        let prepared = prepared.expect("the provider produces a journal");
        assert_eq!(prepared.journal.edges().len(), 1);
        assert_eq!(prepared.segment_index_before_raw_event(1), 1);
    }

    #[test]
    fn same_frame_primary_edges_preserve_order_but_fail_closed_delivery_correlation() {
        let context = Context::default();
        let mut state = state(&context);
        let events = vec![
            Event::PointerButton {
                pos: pos2(20.0, 10.0),
                button: EguiPointerButton::Primary,
                pressed: true,
                modifiers: Modifiers::NONE,
            },
            Event::PointerButton {
                pos: pos2(20.0, 10.0),
                button: EguiPointerButton::Primary,
                pressed: false,
                modifiers: Modifiers::NONE,
            },
        ];
        let mut prepared = None;
        let _ = context.run_ui(RawInput::default(), |_ui| {
            prepared = state
                .prepare_captured_events(
                    &context,
                    EguiPointerInputEpoch::new(21, 0, ViewportId::ROOT, 1),
                    None,
                    &events,
                )
                .expect("captured same-frame edges stage");
        });
        let prepared = prepared.expect("the provider preserves the physical journal");

        assert_eq!(prepared.journal.edges().len(), 2);
        assert!(matches!(
            prepared.journal.edges()[0].kind(),
            PointerEdgeKind::ButtonPressed(PointerButton::Primary)
        ));
        assert!(matches!(
            prepared.journal.edges()[1].kind(),
            PointerEdgeKind::ButtonReleased(PointerButton::Primary)
        ));
        assert!(
            !prepared.delivery_correlation_available,
            "official egui cannot prove an event-time receiver for both edges"
        );
    }
}
