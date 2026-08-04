//! Ordered, device-independent scroll interaction state.

use super::*;
use crate::pointer_journal::ScrollModifiers;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct ScrollSequenceKey {
    provider: PointerInputLease,
    stream: PointerStreamId,
    device: ScrollDeviceId,
    token: ScrollSequenceToken,
}

impl ScrollSequenceKey {
    const fn new(
        provider: PointerInputLease,
        stream: PointerStreamId,
        device: ScrollDeviceId,
        token: ScrollSequenceToken,
    ) -> Self {
        Self {
            provider,
            stream,
            device,
            token,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct ScrollDeviceKey {
    provider: PointerInputLease,
    stream: PointerStreamId,
    device: ScrollDeviceId,
}

impl ScrollDeviceKey {
    const fn from_sequence(key: ScrollSequenceKey) -> Self {
        Self {
            provider: key.provider,
            stream: key.stream,
            device: key.device,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum ScrollReceiverGeometry {
    TabStrip {
        key: TabStripStateKey,
        viewport_extent: f64,
        line_extent: f64,
        maximum_offset: f64,
    },
    TabListMenu {
        session: TabListMenuSessionId,
        viewport_extent: f64,
        line_extent: f64,
        maximum_offset: f64,
    },
}

impl ScrollReceiverGeometry {
    const fn maximum_offset(self) -> f64 {
        match self {
            Self::TabStrip { maximum_offset, .. } | Self::TabListMenu { maximum_offset, .. } => {
                maximum_offset
            }
        }
    }

    const fn line_extent(self) -> f64 {
        match self {
            Self::TabStrip { line_extent, .. } | Self::TabListMenu { line_extent, .. } => {
                line_extent
            }
        }
    }

    const fn viewport_extent(self) -> f64 {
        match self {
            Self::TabStrip {
                viewport_extent, ..
            }
            | Self::TabListMenu {
                viewport_extent, ..
            } => viewport_extent,
        }
    }

    const fn primary_component(self, vector: crate::pointer_journal::FiniteScrollVector) -> f64 {
        match self {
            Self::TabStrip { .. } => {
                if vector.x() != 0.0 {
                    vector.x()
                } else {
                    vector.y()
                }
            }
            Self::TabListMenu { .. } => {
                if vector.y() != 0.0 {
                    vector.y()
                } else {
                    vector.x()
                }
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct ScrollOwner {
    receiver: PresentationHitRegionId,
    hit: crate::presentation_hit::PresentationHitRegion,
    endpoint: ScrollDeliveryEndpoint,
    popup_routing: PopupRoutingRevision,
    policy: PolicyRevision,
    config: PresentationConfigRevision,
    geometry: ScrollReceiverGeometry,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct SuppressedScrollSession {
    reason: ScrollSuppressionReason,
    endpoint: Option<ScrollDeliveryEndpoint>,
    popup_routing: PopupRoutingRevision,
    policy: PolicyRevision,
    config: PresentationConfigRevision,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct AwaitingScrollProviderTerminal {
    receiver: Option<PresentationHitRegionId>,
    claim_derivative: bool,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum ActiveScrollDisposition {
    Owned(ScrollOwner),
    Suppressed(SuppressedScrollSession),
    AwaitingProviderTerminal(AwaitingScrollProviderTerminal),
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct ActiveScrollSession {
    id: ScrollSessionId,
    disposition: ActiveScrollDisposition,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub(super) struct ScrollInteractionState {
    last_session: ScrollSessionId,
    active: BTreeMap<ScrollSequenceKey, ActiveScrollSession>,
    token_watermarks: BTreeMap<ScrollDeviceKey, ScrollSequenceToken>,
}

impl ScrollInteractionState {
    pub(super) fn retention_manifest(&self) -> crate::retention::ScrollRetentionManifest {
        crate::retention::ScrollRetentionManifest::new(
            self.active.len(),
            self.token_watermarks.len(),
        )
    }

    fn issue_session(&mut self) -> Option<ScrollSessionId> {
        let next = self.last_session.checked_next()?;
        self.last_session = next;
        Some(next)
    }

    fn key_conflicts(&self, key: ScrollSequenceKey) -> bool {
        let device = ScrollDeviceKey::from_sequence(key);
        self.active
            .keys()
            .any(|active| ScrollDeviceKey::from_sequence(*active) == device)
            || self
                .token_watermarks
                .get(&device)
                .is_some_and(|watermark| key.token <= *watermark)
    }

    fn advance_token_watermark(&mut self, key: ScrollSequenceKey) {
        self.token_watermarks
            .insert(ScrollDeviceKey::from_sequence(key), key.token);
    }

    fn retire(&mut self, key: ScrollSequenceKey) -> Result<ActiveScrollSession, &'static str> {
        let session = self
            .active
            .remove(&key)
            .ok_or("smooth scroll edge has no matching active sequence")?;
        Ok(session)
    }

    fn mark_semantically_terminated(
        &mut self,
        key: ScrollSequenceKey,
        release_derivative_to_framework: bool,
    ) -> Result<ActiveScrollSession, &'static str> {
        let session = self
            .active
            .get(&key)
            .copied()
            .ok_or("smooth scroll edge has no matching active sequence")?;
        if matches!(
            session.disposition,
            ActiveScrollDisposition::AwaitingProviderTerminal(_)
        ) {
            return Err("smooth scroll sequence already terminated semantically");
        }
        self.active
            .get_mut(&key)
            .expect("the scroll session was read from the same active map")
            .disposition =
            ActiveScrollDisposition::AwaitingProviderTerminal(AwaitingScrollProviderTerminal {
                receiver: scroll_session_receiver(session),
                claim_derivative: !release_derivative_to_framework
                    && matches!(session.disposition, ActiveScrollDisposition::Owned(_)),
            });
        Ok(session)
    }

    pub(super) fn retire_provider(
        &mut self,
        provider: PointerInputLease,
    ) -> Vec<(ScrollSessionId, Option<PresentationHitRegionId>)> {
        let keys = self
            .active
            .keys()
            .copied()
            .filter(|key| key.provider == provider)
            .collect::<Vec<_>>();
        let sessions = keys
            .into_iter()
            .filter_map(|key| self.active.remove(&key))
            .filter(scroll_session_is_semantically_active)
            .map(|session| (session.id, scroll_session_receiver(session)))
            .collect();
        self.token_watermarks
            .retain(|key, _| key.provider != provider);
        sessions
    }

    fn retire_stream(&mut self, stream: PointerStreamId) -> Vec<ActiveScrollSession> {
        let keys = self
            .active
            .keys()
            .copied()
            .filter(|key| key.stream == stream)
            .collect::<Vec<_>>();
        let sessions = keys
            .into_iter()
            .filter_map(|key| self.active.remove(&key))
            .filter(scroll_session_is_semantically_active)
            .collect();
        // The pointer stream incarnation is itself terminal authority. No
        // scroll-token tombstone from that stream is needed after retirement.
        self.token_watermarks.retain(|key, _| key.stream != stream);
        sessions
    }

    fn retire_all(&mut self) -> Vec<ActiveScrollSession> {
        std::mem::take(&mut self.active)
            .into_values()
            .filter(scroll_session_is_semantically_active)
            .collect()
    }
}

#[derive(Clone, Copy)]
enum ObservedScrollReceiver<'a> {
    Unknown,
    FrameworkBlocked,
    Known {
        disposition: PointerReceiverDeliveryDisposition,
        presentation: &'a JournalSurfacePresentation,
    },
}

impl DockEngine {
    pub(super) fn scroll_receiver_challenge(
        &self,
        stream: PointerStreamId,
        scroll: ScrollEdge,
    ) -> ScrollReceiverChallenge {
        if scroll.delta().is_some()
            && matches!(
                scroll.modifiers(),
                Authority::Known(modifiers) if modifiers.control() || modifiers.command()
            )
        {
            return ScrollReceiverChallenge::FrameworkReserved;
        }
        let projected_delta = scroll_receiver_probe_vector(scroll);
        let Some(token) = scroll.sequence() else {
            return ScrollReceiverChallenge::Spatial { projected_delta };
        };
        let key = ScrollSequenceKey::new(stream.lease(), stream, scroll.device(), token);
        let Some(session) = self.scroll_interaction.active.get(&key) else {
            return ScrollReceiverChallenge::Spatial { projected_delta };
        };
        match session.disposition {
            ActiveScrollDisposition::Owned(owner) => ScrollReceiverChallenge::Locked {
                receiver: owner.receiver,
                probe_point: scroll_owner_probe_point(owner),
                projected_delta,
            },
            ActiveScrollDisposition::Suppressed(_) => ScrollReceiverChallenge::Unavailable,
            ActiveScrollDisposition::AwaitingProviderTerminal(terminal) => {
                if terminal.claim_derivative {
                    ScrollReceiverChallenge::OwnedTerminal
                } else {
                    ScrollReceiverChallenge::Unavailable
                }
            }
        }
    }

    pub(super) fn reduce_scroll_edge(
        &mut self,
        cause: ReductionCause,
        stream: PointerStreamId,
        edge: &PointerEdge,
        scroll: ScrollEdge,
        receipt: &ValidatedPointerReceiverReceipt,
        snapshot: &JournalPresentationSnapshot,
    ) -> Result<Vec<InteractionOutcome>, EngineError> {
        let observed = match (scroll.delta(), scroll.modifiers()) {
            (
                Some(ScrollDelta::PhysicalPixels {
                    coordinates: Authority::Unknown(_),
                    ..
                }),
                _,
            ) => ObservedScrollReceiver::Unknown,
            (Some(_), Authority::Unknown(_)) => ObservedScrollReceiver::Unknown,
            (Some(_), Authority::Known(modifiers))
                if modifiers.control() || modifiers.command() =>
            {
                ObservedScrollReceiver::FrameworkBlocked
            }
            _ => Self::observed_scroll_receiver(cause, receipt, snapshot)?,
        };
        if let ObservedScrollReceiver::Known { presentation, .. } = &observed
            && self
                .validate_scroll_delivery_endpoint(cause, edge, presentation)?
                .is_none()
        {
            return Err(scroll_invariant(
                cause,
                "known scroll receiver receipt has no exact delivery endpoint",
            ));
        }
        match scroll.phase() {
            ScrollPhase::Discrete => self.reduce_discrete_scroll(cause, edge, scroll, observed),
            ScrollPhase::Begin => self.begin_smooth_scroll(cause, stream, edge, scroll, observed),
            ScrollPhase::Update => {
                self.update_smooth_scroll(cause, stream, edge, scroll, observed, false)
            }
            ScrollPhase::End => {
                self.update_smooth_scroll(cause, stream, edge, scroll, observed, true)
            }
            ScrollPhase::Cancel(reason) => self.cancel_smooth_scroll(cause, stream, scroll, reason),
        }
    }

    pub(super) fn cancel_scroll_stream(
        &mut self,
        stream: PointerStreamId,
    ) -> Vec<InteractionOutcome> {
        self.scroll_interaction
            .retire_stream(stream)
            .into_iter()
            .map(|session| {
                InteractionOutcome::Scroll(ScrollReductionOutcome::Terminated {
                    session: session.id,
                    receiver: scroll_session_receiver(session),
                    reason: ScrollTerminationReason::StreamCancelled,
                })
            })
            .collect()
    }

    pub(super) fn terminate_all_scroll_sessions_input(
        &mut self,
        input: InputSequence,
        reason: ScrollTerminationReason,
        interaction_events: &mut Vec<InteractionEvent>,
    ) {
        for session in self.scroll_interaction.retire_all() {
            interaction_events.push(InteractionEvent::new(
                input,
                self.version,
                InteractionEventKind::ScrollTerminated {
                    session: session.id,
                    receiver: scroll_session_receiver(session),
                    reason,
                },
            ));
        }
    }

    pub(super) fn reconcile_scroll_lifecycle(
        &mut self,
        cause: ReductionCause,
        interaction_events: &mut Vec<InteractionEvent>,
    ) {
        let losses = self
            .scroll_interaction
            .active
            .iter()
            .filter_map(|(key, session)| match session.disposition {
                ActiveScrollDisposition::Owned(owner) => self
                    .scroll_owner_lifecycle_loss(owner)
                    .map(|reason| (*key, reason)),
                ActiveScrollDisposition::Suppressed(suppressed) => self
                    .suppressed_scroll_lifecycle_loss(suppressed)
                    .map(|reason| (*key, reason)),
                ActiveScrollDisposition::AwaitingProviderTerminal(_) => None,
            })
            .collect::<Vec<_>>();
        for (key, reason) in losses {
            let session = self
                .scroll_interaction
                .mark_semantically_terminated(key, false)
                .expect("the lifecycle loss was derived from the same active map");
            interaction_events.push(InteractionEvent::new_caused(
                cause,
                self.version,
                InteractionEventKind::ScrollTerminated {
                    session: session.id,
                    receiver: scroll_session_receiver(session),
                    reason,
                },
            ));
        }
    }

    fn reduce_discrete_scroll(
        &mut self,
        cause: ReductionCause,
        edge: &PointerEdge,
        scroll: ScrollEdge,
        observed: ObservedScrollReceiver<'_>,
    ) -> Result<Vec<InteractionOutcome>, EngineError> {
        let phase = ScrollPhase::Discrete;
        match self.resolve_initial_scroll_owner(cause, edge, observed)? {
            Ok(owner) => self
                .apply_scroll_delta(cause, None, owner, scroll)
                .map(|outcome| outcome.into_iter().collect()),
            Err(reason) => Ok(vec![scroll_suppressed(None, None, phase, reason)]),
        }
    }

    fn begin_smooth_scroll(
        &mut self,
        cause: ReductionCause,
        stream: PointerStreamId,
        edge: &PointerEdge,
        scroll: ScrollEdge,
        observed: ObservedScrollReceiver<'_>,
    ) -> Result<Vec<InteractionOutcome>, EngineError> {
        let token = scroll
            .sequence()
            .ok_or_else(|| scroll_invariant(cause, "smooth scroll begin has no sequence token"))?;
        let key = ScrollSequenceKey::new(stream.lease(), stream, scroll.device(), token);
        if self.scroll_interaction.key_conflicts(key) {
            return Err(scroll_invariant(
                cause,
                "smooth scroll begin reuses an active device lane or does not advance its token watermark",
            ));
        }
        let session = self
            .scroll_interaction
            .issue_session()
            .ok_or_else(|| scroll_invariant(cause, "scroll session identity space is exhausted"))?;
        self.scroll_interaction.advance_token_watermark(key);

        match self.resolve_initial_scroll_owner(cause, edge, observed)? {
            Ok(owner) => {
                self.scroll_interaction.active.insert(
                    key,
                    ActiveScrollSession {
                        id: session,
                        disposition: ActiveScrollDisposition::Owned(owner),
                    },
                );
                let mut outcomes =
                    vec![InteractionOutcome::Scroll(ScrollReductionOutcome::Began {
                        session,
                        receiver: owner.receiver,
                    })];
                if let Some(applied) =
                    self.apply_scroll_delta(cause, Some(session), owner, scroll)?
                {
                    outcomes.push(applied);
                }
                Ok(outcomes)
            }
            Err(reason) => {
                let suppressed = self.capture_suppressed_scroll(edge, reason);
                self.scroll_interaction.active.insert(
                    key,
                    ActiveScrollSession {
                        id: session,
                        disposition: ActiveScrollDisposition::Suppressed(suppressed),
                    },
                );
                Ok(vec![scroll_suppressed(
                    Some(session),
                    Some(token),
                    scroll.phase(),
                    reason,
                )])
            }
        }
    }

    fn update_smooth_scroll(
        &mut self,
        cause: ReductionCause,
        stream: PointerStreamId,
        edge: &PointerEdge,
        scroll: ScrollEdge,
        observed: ObservedScrollReceiver<'_>,
        terminal: bool,
    ) -> Result<Vec<InteractionOutcome>, EngineError> {
        let token = scroll.sequence().ok_or_else(|| {
            scroll_invariant(cause, "smooth scroll continuation has no sequence token")
        })?;
        let key = ScrollSequenceKey::new(stream.lease(), stream, scroll.device(), token);
        let active = self
            .scroll_interaction
            .active
            .get(&key)
            .copied()
            .ok_or_else(|| {
                scroll_invariant(
                    cause,
                    "smooth scroll continuation has no matching active sequence",
                )
            })?;

        let outcomes = match active.disposition {
            ActiveScrollDisposition::Suppressed(suppressed) => {
                if terminal {
                    self.terminate_scroll_session(cause, key, ScrollTerminationReason::Completed)?
                } else {
                    vec![scroll_suppressed(
                        Some(active.id),
                        Some(token),
                        scroll.phase(),
                        suppressed.reason,
                    )]
                }
            }
            ActiveScrollDisposition::Owned(owner) => self.reduce_owned_scroll_continuation(
                cause, key, active.id, edge, scroll, observed, owner, terminal,
            )?,
            ActiveScrollDisposition::AwaitingProviderTerminal(terminal) => {
                if matches!(observed, ObservedScrollReceiver::FrameworkBlocked)
                    && terminal.claim_derivative
                {
                    self.scroll_interaction
                        .active
                        .get_mut(&key)
                        .expect("the provider-terminal session remains active")
                        .disposition = ActiveScrollDisposition::AwaitingProviderTerminal(
                        AwaitingScrollProviderTerminal {
                            claim_derivative: false,
                            ..terminal
                        },
                    );
                }
                Vec::new()
            }
        };
        if terminal {
            self.finish_scroll_provider_terminal(cause, key)?;
        }
        Ok(outcomes)
    }

    #[allow(clippy::too_many_arguments)]
    fn reduce_owned_scroll_continuation(
        &mut self,
        cause: ReductionCause,
        key: ScrollSequenceKey,
        session: ScrollSessionId,
        edge: &PointerEdge,
        scroll: ScrollEdge,
        observed: ObservedScrollReceiver<'_>,
        owner: ScrollOwner,
        terminal: bool,
    ) -> Result<Vec<InteractionOutcome>, EngineError> {
        if let Some(reason) = self.scroll_owner_lifecycle_loss(owner) {
            return self.terminate_scroll_session(cause, key, reason);
        }
        if let Authority::Known(endpoint) = scroll.delivery()
            && endpoint != owner.endpoint
        {
            return self.terminate_scroll_session(
                cause,
                key,
                ScrollTerminationReason::DeliveryEndpointChanged,
            );
        }

        let mut outcomes = Vec::new();
        match observed {
            ObservedScrollReceiver::Unknown => {
                if terminal {
                    return self.terminate_scroll_session(
                        cause,
                        key,
                        ScrollTerminationReason::Completed,
                    );
                }
                outcomes.push(scroll_suppressed(
                    Some(session),
                    Some(key.token),
                    scroll.phase(),
                    ScrollSuppressionReason::ReceiverUnknown,
                ));
                return Ok(outcomes);
            }
            ObservedScrollReceiver::FrameworkBlocked => {
                return self.terminate_scroll_session_releasing_derivative(
                    cause,
                    key,
                    ScrollTerminationReason::ReceiverLost,
                );
            }
            ObservedScrollReceiver::Known {
                disposition: PointerReceiverDeliveryDisposition::Dock(region),
                presentation,
            } => {
                let Some(endpoint) =
                    self.validate_scroll_delivery_endpoint(cause, edge, presentation)?
                else {
                    if terminal {
                        return self.terminate_scroll_session(
                            cause,
                            key,
                            ScrollTerminationReason::Completed,
                        );
                    }
                    outcomes.push(scroll_suppressed(
                        Some(session),
                        Some(key.token),
                        scroll.phase(),
                        ScrollSuppressionReason::ReceiverUnknown,
                    ));
                    return Ok(outcomes);
                };
                if endpoint != owner.endpoint {
                    return self.terminate_scroll_session(
                        cause,
                        key,
                        ScrollTerminationReason::DeliveryEndpointChanged,
                    );
                }
                let Some(current) =
                    self.scroll_owner_from_region(cause, region, presentation, endpoint)?
                else {
                    return self.terminate_scroll_session(
                        cause,
                        key,
                        ScrollTerminationReason::ReceiverLost,
                    );
                };
                if current.receiver != owner.receiver
                    || current.hit != owner.hit
                    || current.geometry != owner.geometry
                {
                    return self.terminate_scroll_session(
                        cause,
                        key,
                        ScrollTerminationReason::ReceiverLost,
                    );
                }
                if current.popup_routing != owner.popup_routing {
                    return self.terminate_scroll_session(
                        cause,
                        key,
                        ScrollTerminationReason::PopupRoutingChanged,
                    );
                }
                if current.policy != owner.policy || current.config != owner.config {
                    return self.terminate_scroll_session(
                        cause,
                        key,
                        ScrollTerminationReason::PolicyChanged,
                    );
                }
                if let Some(applied) =
                    self.apply_scroll_delta(cause, Some(session), owner, scroll)?
                {
                    outcomes.push(applied);
                }
            }
            ObservedScrollReceiver::Known { .. } => {
                return self.terminate_scroll_session(
                    cause,
                    key,
                    ScrollTerminationReason::ReceiverLost,
                );
            }
        }

        if terminal {
            outcomes.extend(self.terminate_scroll_session(
                cause,
                key,
                ScrollTerminationReason::Completed,
            )?);
        }
        Ok(outcomes)
    }

    fn cancel_smooth_scroll(
        &mut self,
        cause: ReductionCause,
        stream: PointerStreamId,
        scroll: ScrollEdge,
        reason: crate::pointer_journal::ScrollCancelReason,
    ) -> Result<Vec<InteractionOutcome>, EngineError> {
        let token = scroll
            .sequence()
            .ok_or_else(|| scroll_invariant(cause, "smooth scroll cancel has no sequence token"))?;
        let key = ScrollSequenceKey::new(stream.lease(), stream, scroll.device(), token);
        let already_terminated = self
            .scroll_interaction
            .active
            .get(&key)
            .is_some_and(|session| {
                matches!(
                    session.disposition,
                    ActiveScrollDisposition::AwaitingProviderTerminal(_)
                )
            });
        let outcomes = if already_terminated {
            Vec::new()
        } else {
            self.terminate_scroll_session(cause, key, ScrollTerminationReason::Cancelled(reason))?
        };
        if reason != crate::pointer_journal::ScrollCancelReason::BindingRetired {
            self.finish_scroll_provider_terminal(cause, key)?;
        }
        Ok(outcomes)
    }

    fn terminate_scroll_session(
        &mut self,
        cause: ReductionCause,
        key: ScrollSequenceKey,
        reason: ScrollTerminationReason,
    ) -> Result<Vec<InteractionOutcome>, EngineError> {
        let session = self
            .scroll_interaction
            .mark_semantically_terminated(key, false)
            .map_err(|detail| scroll_invariant(cause, detail))?;
        Ok(vec![InteractionOutcome::Scroll(
            ScrollReductionOutcome::Terminated {
                session: session.id,
                receiver: scroll_session_receiver(session),
                reason,
            },
        )])
    }

    fn terminate_scroll_session_releasing_derivative(
        &mut self,
        cause: ReductionCause,
        key: ScrollSequenceKey,
        reason: ScrollTerminationReason,
    ) -> Result<Vec<InteractionOutcome>, EngineError> {
        let session = self
            .scroll_interaction
            .mark_semantically_terminated(key, true)
            .map_err(|detail| scroll_invariant(cause, detail))?;
        Ok(vec![InteractionOutcome::Scroll(
            ScrollReductionOutcome::Terminated {
                session: session.id,
                receiver: scroll_session_receiver(session),
                reason,
            },
        )])
    }

    fn finish_scroll_provider_terminal(
        &mut self,
        cause: ReductionCause,
        key: ScrollSequenceKey,
    ) -> Result<(), EngineError> {
        let session = self
            .scroll_interaction
            .active
            .get(&key)
            .copied()
            .ok_or_else(|| {
                scroll_invariant(cause, "provider terminal has no matching scroll sequence")
            })?;
        if !matches!(
            session.disposition,
            ActiveScrollDisposition::AwaitingProviderTerminal(_)
        ) {
            return Err(scroll_invariant(
                cause,
                "provider terminal arrived before semantic scroll termination",
            ));
        }
        self.scroll_interaction
            .retire(key)
            .map_err(|detail| scroll_invariant(cause, detail))?;
        Ok(())
    }

    fn resolve_initial_scroll_owner(
        &self,
        cause: ReductionCause,
        edge: &PointerEdge,
        observed: ObservedScrollReceiver<'_>,
    ) -> Result<Result<ScrollOwner, ScrollSuppressionReason>, EngineError> {
        if matches!(observed, ObservedScrollReceiver::FrameworkBlocked) {
            return Ok(Err(ScrollSuppressionReason::FrameworkBlocked));
        }
        let ObservedScrollReceiver::Known {
            disposition,
            presentation,
        } = observed
        else {
            return Ok(Err(ScrollSuppressionReason::ReceiverUnknown));
        };
        match disposition {
            PointerReceiverDeliveryDisposition::Dock(region) => {
                let Some(endpoint) =
                    self.validate_scroll_delivery_endpoint(cause, edge, presentation)?
                else {
                    return Ok(Err(ScrollSuppressionReason::ReceiverUnknown));
                };
                Ok(
                    match self.scroll_owner_from_region(cause, region, presentation, endpoint)? {
                        Some(owner) => Ok(owner),
                        None => Err(ScrollSuppressionReason::DockBlocker(region)),
                    },
                )
            }
            PointerReceiverDeliveryDisposition::DockCanvas => {
                Ok(Err(ScrollSuppressionReason::DockCanvas))
            }
            PointerReceiverDeliveryDisposition::Blocked => {
                Ok(Err(ScrollSuppressionReason::FrameworkBlocked))
            }
            PointerReceiverDeliveryDisposition::NoReceiver => {
                Ok(Err(ScrollSuppressionReason::NoReceiver))
            }
            PointerReceiverDeliveryDisposition::Unknown(_) => {
                Ok(Err(ScrollSuppressionReason::ReceiverUnknown))
            }
        }
    }

    fn capture_suppressed_scroll(
        &self,
        edge: &PointerEdge,
        reason: ScrollSuppressionReason,
    ) -> SuppressedScrollSession {
        let endpoint = match edge.kind() {
            PointerEdgeKind::Scrolled(scroll) => match scroll.delivery() {
                Authority::Known(endpoint) => Some(endpoint),
                Authority::Unknown(_) => None,
            },
            _ => None,
        };
        SuppressedScrollSession {
            reason,
            endpoint,
            popup_routing: self
                .presentation_authority
                .tab_strip_states
                .popup_requirement()
                .revision(),
            policy: self.policy.revision(),
            config: self.presentation_authority.presentation_config_revision,
        }
    }

    fn observed_scroll_receiver<'snapshot>(
        cause: ReductionCause,
        receipt: &ValidatedPointerReceiverReceipt,
        snapshot: &'snapshot JournalPresentationSnapshot,
    ) -> Result<ObservedScrollReceiver<'snapshot>, EngineError> {
        let PointerReceiverObservation::Presented(observation) = receipt.observation() else {
            return Ok(ObservedScrollReceiver::Unknown);
        };
        let delivery = observation
            .probes()
            .iter()
            .find_map(|probe| match probe {
                PointerReceiverProbeReceipt::Delivery(delivery) => Some(*delivery),
                PointerReceiverProbeReceipt::HoverHit(_) => None,
            })
            .ok_or_else(|| {
                scroll_invariant(cause, "scroll receiver receipt has no delivery probe")
            })?;
        let disposition = delivery.scroll();
        if matches!(disposition, PointerReceiverDeliveryDisposition::Unknown(_)) {
            return Ok(ObservedScrollReceiver::Unknown);
        }
        let (Some(output), Some(authority)) = (delivery.output(), delivery.authority()) else {
            return Err(scroll_invariant(
                cause,
                "known scroll receiver has no atomic presentation binding",
            ));
        };
        let presentation = snapshot.presentation(output, authority).map_err(|source| {
            EngineError::JournalPresentationSnapshot {
                detail: source.to_string(),
            }
        })?;
        Ok(ObservedScrollReceiver::Known {
            disposition,
            presentation,
        })
    }

    fn validate_scroll_delivery_endpoint(
        &self,
        cause: ReductionCause,
        edge: &PointerEdge,
        presentation: &JournalSurfacePresentation,
    ) -> Result<Option<ScrollDeliveryEndpoint>, EngineError> {
        let Authority::Known(endpoint) = (match edge.kind() {
            PointerEdgeKind::Scrolled(scroll) => scroll.delivery(),
            _ => {
                return Err(scroll_invariant(
                    cause,
                    "scroll reducer received a non-scroll pointer edge",
                ));
            }
        }) else {
            return Ok(None);
        };
        let authority = presentation.authority();
        if endpoint.surface() != presentation.surface()
            || endpoint.binding() != authority.binding()
            || endpoint.coordinate_generation() != authority.coordinate_generation()
            || self
                .presentation_authority
                .presentation
                .host_for_stream(authority.stream())
                != Some(endpoint.host())
        {
            return Err(scroll_invariant(
                cause,
                "scroll delivery endpoint differs from final-presentation authority",
            ));
        }
        Ok(Some(endpoint))
    }

    fn scroll_owner_from_region(
        &self,
        cause: ReductionCause,
        region: PresentationHitRegionId,
        presentation: &JournalSurfacePresentation,
        endpoint: ScrollDeliveryEndpoint,
    ) -> Result<Option<ScrollOwner>, EngineError> {
        let plan = presentation.plan();
        let geometry = match region.kind() {
            PresentationHitRegionKind::TabStripScroll(_)
            | PresentationHitRegionKind::TabListMenuScroll(_) => {
                self.scroll_receiver_geometry(region, plan).ok_or_else(|| {
                    scroll_invariant(cause, "scroll receiver has no matching plan geometry")
                })?
            }
            PresentationHitRegionKind::TabListMenuBlocker(_)
            | PresentationHitRegionKind::TabListMenuBackdrop(_)
            | PresentationHitRegionKind::ContainedFrameBlocker(_) => return Ok(None),
            _ => {
                return Err(scroll_invariant(
                    cause,
                    "scroll receipt names a non-scroll semantic receiver",
                ));
            }
        };
        let hit = presentation
            .hit_manifest()
            .region(region)
            .filter(|hit| hit.lanes().contains(PresentationPointerLane::Scroll))
            .copied()
            .ok_or_else(|| {
                scroll_invariant(
                    cause,
                    "scroll receiver is absent from the exact hit manifest",
                )
            })?;
        let requirement = presentation.scene().requirement();
        Ok(Some(ScrollOwner {
            receiver: region,
            hit,
            endpoint,
            popup_routing: plan.popup().revision(),
            policy: requirement.policy(),
            config: requirement.config(),
            geometry,
        }))
    }

    fn scroll_receiver_geometry(
        &self,
        region: PresentationHitRegionId,
        plan: &PresentationPlan,
    ) -> Option<ScrollReceiverGeometry> {
        match region.kind() {
            PresentationHitRegionKind::TabStripScroll(bar) => {
                let record = plan
                    .tab_bar_records()
                    .iter()
                    .find(|record| *record.id() == bar)?;
                Some(ScrollReceiverGeometry::TabStrip {
                    key: TabStripStateKey::new(region.surface(), bar),
                    viewport_extent: record.viewport().width(),
                    line_extent: self
                        .presentation_authority
                        .presentation_config
                        .tab_strip_scroll_line_extent(),
                    maximum_offset: record.maximum_scroll_offset(),
                })
            }
            PresentationHitRegionKind::TabListMenuScroll(session) => {
                let record = plan
                    .tab_list_menu_records()
                    .iter()
                    .find(|record| record.session() == session)?;
                Some(ScrollReceiverGeometry::TabListMenu {
                    session,
                    viewport_extent: record.viewport().height(),
                    line_extent: menu_line_extent(record)?,
                    maximum_offset: record.maximum_scroll_offset(),
                })
            }
            _ => None,
        }
    }

    fn apply_scroll_delta(
        &mut self,
        cause: ReductionCause,
        session: Option<ScrollSessionId>,
        owner: ScrollOwner,
        scroll: ScrollEdge,
    ) -> Result<Option<InteractionOutcome>, EngineError> {
        let Some(delta) = scroll.delta() else {
            return Ok(None);
        };
        let Authority::Known(modifiers) = scroll.modifiers() else {
            return Err(scroll_invariant(
                cause,
                "scroll application reached core without authoritative modifiers",
            ));
        };
        if modifiers.control() || modifiers.command() {
            return Err(scroll_invariant(
                cause,
                "framework-reserved modified scroll reached core application",
            ));
        }
        let requested_delta = self.convert_scroll_delta(cause, owner, delta, modifiers)?;
        let old_offset = match owner.geometry {
            ScrollReceiverGeometry::TabStrip { key, .. } => self
                .presentation_authority
                .tab_strip_states
                .state(key)
                .and_then(|state| state.scroll_offset())
                .ok_or_else(|| scroll_invariant(cause, "tab-strip scroll state is unavailable"))?,
            ScrollReceiverGeometry::TabListMenu { session, .. } => self
                .presentation_authority
                .tab_strip_states
                .active_menu()
                .filter(|menu| menu.session() == session)
                .map(|menu| menu.scroll_offset())
                .ok_or_else(|| scroll_invariant(cause, "tab-list scroll state is unavailable"))?,
        };
        let offset = (old_offset + requested_delta).clamp(0.0, owner.geometry.maximum_offset());
        let applied_delta = offset - old_offset;
        let unapplied_delta = requested_delta - applied_delta;

        let state_delta = match owner.geometry {
            ScrollReceiverGeometry::TabStrip { key, .. } => self
                .presentation_authority
                .tab_strip_states
                .set_tab_strip_scroll_offset(key, offset)
                .map_err(|source| Self::tab_strip_state_invariant(cause, source))?,
            ScrollReceiverGeometry::TabListMenu { session, .. } => self
                .presentation_authority
                .tab_strip_states
                .set_active_menu_scroll_offset(session, offset)
                .map_err(|source| Self::tab_strip_state_invariant(cause, source))?,
        };
        self.consume_tab_strip_state_delta(cause, &state_delta)?;

        Ok(Some(InteractionOutcome::Scroll(
            ScrollReductionOutcome::Applied(ScrollApplication::new(
                session,
                owner.receiver,
                scroll.phase(),
                requested_delta,
                applied_delta,
                unapplied_delta,
                offset,
            )),
        )))
    }

    fn convert_scroll_delta(
        &self,
        cause: ReductionCause,
        owner: ScrollOwner,
        delta: ScrollDelta,
        modifiers: ScrollModifiers,
    ) -> Result<f64, EngineError> {
        let vector = core_scroll_vector(delta.vector(), modifiers).map_err(|_| {
            scroll_invariant(
                cause,
                "shift-axis scroll projection produced a non-finite delta",
            )
        })?;
        let component = owner.geometry.primary_component(vector);
        let content_delta = match delta {
            ScrollDelta::LogicalPoints(_) => component,
            ScrollDelta::Lines(_) => component * owner.geometry.line_extent(),
            ScrollDelta::Pages(_) => component * owner.geometry.viewport_extent(),
            ScrollDelta::PhysicalPixels {
                coordinates: Authority::Known(coordinates),
                ..
            } => {
                let binding = coordinates.binding();
                let coordinate_generation = coordinates.coordinate_generation();
                if owner.endpoint.binding() != Some(binding)
                    || owner.endpoint.coordinate_generation() != coordinate_generation
                {
                    return Err(scroll_invariant(
                        cause,
                        "physical scroll delta differs from the locked delivery endpoint",
                    ));
                }
                let coordinates = self
                    .viewport
                    .viewport(binding.surface())
                    .filter(|record| {
                        record.binding() == binding
                            && record.coordinate_generation() == coordinate_generation
                            && record.has_coordinate_authority()
                    })
                    .and_then(|record| record.coordinates())
                    .ok_or_else(|| {
                        scroll_invariant(
                            cause,
                            "physical scroll delta has no current exact coordinate authority",
                        )
                    })?;
                component / coordinates.presentation_scale_factor().get()
            }
            ScrollDelta::PhysicalPixels {
                coordinates: Authority::Unknown(_),
                ..
            } => {
                return Err(scroll_invariant(
                    cause,
                    "physical scroll application reached core without coordinate authority",
                ));
            }
        };
        let requested = -content_delta;
        if !requested.is_finite() {
            return Err(scroll_invariant(
                cause,
                "scroll unit conversion produced a non-finite offset delta",
            ));
        }
        Ok(if requested == 0.0 { 0.0 } else { requested })
    }

    fn scroll_owner_lifecycle_loss(&self, owner: ScrollOwner) -> Option<ScrollTerminationReason> {
        if self.workspace.surface(owner.receiver.surface()).is_none() {
            return Some(ScrollTerminationReason::SurfaceRemoved);
        }
        if self.policy.revision() != owner.policy {
            return Some(ScrollTerminationReason::PolicyChanged);
        }
        if self.presentation_authority.presentation_config_revision != owner.config {
            return Some(ScrollTerminationReason::PresentationConfigChanged);
        }
        if self
            .presentation_authority
            .tab_strip_states
            .popup_requirement()
            .revision()
            != owner.popup_routing
        {
            return Some(ScrollTerminationReason::PopupRoutingChanged);
        }
        if let Some(reason) = self.scroll_endpoint_lifecycle_loss(owner.endpoint) {
            return Some(reason);
        }
        if let Some(projection) = self
            .presentation_authority
            .scene
            .interaction_projection(owner.receiver.surface())
        {
            let Some(current_hit) = projection
                .hit_manifest()
                .region(owner.receiver)
                .filter(|hit| hit.lanes().contains(PresentationPointerLane::Scroll))
            else {
                return Some(ScrollTerminationReason::ReceiverLost);
            };
            let Some(current_geometry) =
                self.scroll_receiver_geometry(owner.receiver, projection.plan())
            else {
                return Some(ScrollTerminationReason::ReceiverLost);
            };
            if *current_hit != owner.hit || current_geometry != owner.geometry {
                return Some(ScrollTerminationReason::ReceiverLost);
            }
        }
        match owner.geometry {
            ScrollReceiverGeometry::TabStrip { key, .. } => {
                if self
                    .presentation_authority
                    .tab_strip_states
                    .state(key)
                    .is_none()
                {
                    return Some(ScrollTerminationReason::ReceiverLost);
                }
            }
            ScrollReceiverGeometry::TabListMenu { session, .. } => {
                if !self
                    .presentation_authority
                    .tab_strip_states
                    .active_menu()
                    .is_some_and(|menu| menu.session() == session)
                {
                    return Some(ScrollTerminationReason::ReceiverLost);
                }
            }
        }
        None
    }

    fn suppressed_scroll_lifecycle_loss(
        &self,
        suppressed: SuppressedScrollSession,
    ) -> Option<ScrollTerminationReason> {
        if self.policy.revision() != suppressed.policy {
            return Some(ScrollTerminationReason::PolicyChanged);
        }
        if self.presentation_authority.presentation_config_revision != suppressed.config {
            return Some(ScrollTerminationReason::PresentationConfigChanged);
        }
        if self
            .presentation_authority
            .tab_strip_states
            .popup_requirement()
            .revision()
            != suppressed.popup_routing
        {
            return Some(ScrollTerminationReason::PopupRoutingChanged);
        }
        suppressed
            .endpoint
            .and_then(|endpoint| self.scroll_endpoint_lifecycle_loss(endpoint))
    }

    fn scroll_endpoint_lifecycle_loss(
        &self,
        endpoint: ScrollDeliveryEndpoint,
    ) -> Option<ScrollTerminationReason> {
        if self.workspace.surface(endpoint.surface()).is_none() {
            return Some(ScrollTerminationReason::SurfaceRemoved);
        }
        if !matches!(
            self.presentation_authority
                .presentation
                .retirement_status(endpoint.host()),
            Ok(PresentationHostRetirementStatus::Live)
        ) {
            return Some(ScrollTerminationReason::PresentationHostRetired);
        }
        match endpoint.binding() {
            Some(binding) => {
                let current = self.viewport.viewport(binding.surface());
                if !current.is_some_and(|record| {
                    record.binding() == binding
                        && record.coordinate_generation() == endpoint.coordinate_generation()
                        && record.has_coordinate_authority()
                }) {
                    return Some(ScrollTerminationReason::BindingRetired);
                }
            }
            None => {
                if self.viewport.viewport(endpoint.surface()).is_some() {
                    return Some(ScrollTerminationReason::DeliveryEndpointChanged);
                }
            }
        }
        None
    }
}

fn menu_line_extent(record: &crate::scene::TabListMenuRecord) -> Option<f64> {
    match record.rows() {
        [first, second, ..] => {
            let pitch = (second.bounds().y() - first.bounds().y()).abs();
            (pitch > 0.0 && pitch.is_finite()).then_some(pitch)
        }
        [only] => {
            let height = only.bounds().height();
            (height > 0.0 && height.is_finite()).then_some(height)
        }
        [] => None,
    }
}

pub(super) fn scroll_receiver_probe_vector(
    scroll: ScrollEdge,
) -> Option<crate::pointer_journal::FiniteScrollVector> {
    let delta = scroll.delta()?;
    if matches!(
        delta,
        ScrollDelta::PhysicalPixels {
            coordinates: Authority::Unknown(_),
            ..
        }
    ) {
        return None;
    }
    let Authority::Known(modifiers) = scroll.modifiers() else {
        return None;
    };
    if modifiers.control() || modifiers.command() {
        return None;
    }
    core_scroll_vector(delta.vector(), modifiers).ok()
}

fn core_scroll_vector(
    vector: crate::pointer_journal::FiniteScrollVector,
    modifiers: ScrollModifiers,
) -> Result<crate::pointer_journal::FiniteScrollVector, crate::pointer_journal::ScrollEdgeError> {
    if !modifiers.shift() {
        return Ok(vector);
    }
    crate::pointer_journal::FiniteScrollVector::new(vector.x() + vector.y(), 0.0)
}

const fn scroll_session_receiver(session: ActiveScrollSession) -> Option<PresentationHitRegionId> {
    match session.disposition {
        ActiveScrollDisposition::Owned(owner) => Some(owner.receiver),
        ActiveScrollDisposition::Suppressed(_) => None,
        ActiveScrollDisposition::AwaitingProviderTerminal(terminal) => terminal.receiver,
    }
}

fn scroll_owner_probe_point(owner: ScrollOwner) -> LogicalPoint {
    let rect = owner.hit.hit().rect();
    let min = rect.min();
    let max = rect.max();
    LogicalPoint::new(
        min.x() + (max.x() - min.x()) / 2.0,
        min.y() + (max.y() - min.y()) / 2.0,
    )
    .expect("a validated hit rectangle has a finite midpoint")
}

const fn scroll_session_is_semantically_active(session: &ActiveScrollSession) -> bool {
    !matches!(
        session.disposition,
        ActiveScrollDisposition::AwaitingProviderTerminal(_)
    )
}

const fn scroll_suppressed(
    session: Option<ScrollSessionId>,
    sequence: Option<ScrollSequenceToken>,
    phase: ScrollPhase,
    reason: ScrollSuppressionReason,
) -> InteractionOutcome {
    InteractionOutcome::Scroll(ScrollReductionOutcome::Suppressed {
        session,
        sequence,
        phase,
        reason,
    })
}

fn scroll_invariant(cause: ReductionCause, detail: impl Into<String>) -> EngineError {
    EngineError::PointerInteractionInvariant {
        cause,
        detail: detail.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::intent::PointerId;

    #[test]
    fn shift_projection_keeps_raw_f64_precision_in_core() {
        let raw = crate::pointer_journal::FiniteScrollVector::new(16_777_217.25, -16_777_216.0)
            .expect("the raw scroll vector is finite");
        let projected = core_scroll_vector(raw, ScrollModifiers::new(true, false, false, false))
            .expect("the projected scroll vector is finite");

        assert_eq!(projected.x(), 1.25);
        assert_eq!(projected.y(), 0.0);
    }

    #[test]
    fn retention_manifest_accounts_for_sessions_and_live_stream_watermarks() {
        let lease = PointerInputLease::new(
            EngineAuthorityDomainId::new_for_test(91),
            1,
            PointerProviderScope::DesktopGlobal,
        );
        let stream = PointerStreamId::new(lease, PointerId::new(7), 1);
        let key = ScrollSequenceKey::new(
            lease,
            stream,
            ScrollDeviceId::new(3),
            ScrollSequenceToken::new(4),
        );
        let mut state = ScrollInteractionState::default();
        state.advance_token_watermark(key);
        state.active.insert(
            key,
            ActiveScrollSession {
                id: ScrollSessionId::new(1),
                disposition: ActiveScrollDisposition::Suppressed(SuppressedScrollSession {
                    reason: ScrollSuppressionReason::ReceiverUnknown,
                    endpoint: None,
                    popup_routing: PopupRoutingRevision::new_for_test(1),
                    policy: PolicyRevision::new(1),
                    config: PresentationConfigRevision::new(1),
                }),
            },
        );

        let retained = state.retention_manifest();
        assert_eq!(retained.active_sessions(), 1);
        assert_eq!(retained.sequence_watermark_guards(), 1);
        assert_eq!(retained.retained_structure_count(), 2);

        assert_eq!(state.retire_stream(stream).len(), 1);
        assert_eq!(
            state.retention_manifest(),
            crate::retention::ScrollRetentionManifest::default()
        );
    }
}
