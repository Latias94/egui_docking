use super::*;
use crate::backend_ingress::BackendIngressDrainReceipt;

impl PointerProviderIncarnation {
    #[allow(
        dead_code,
        reason = "reserved for the independent pointer-ledger integration"
    )]
    fn checked_next(self) -> Option<Self> {
        self.0.checked_add(1).map(Self)
    }
}
impl PointerStreamIncarnation {
    fn checked_next(self) -> Option<Self> {
        self.0.checked_add(1).map(Self)
    }
}
impl PointerInputLease {
    #[allow(
        dead_code,
        reason = "reserved for the host-frame reducer authority boundary"
    )]
    pub(crate) const fn new(
        authority_domain: EngineAuthorityDomainId,
        incarnation: u64,
        scope: PointerProviderScope,
    ) -> Self {
        Self {
            authority_domain,
            incarnation: PointerProviderIncarnation(incarnation),
            scope,
        }
    }

    /// Returns the engine authority domain which minted this lease.
    #[must_use]
    pub const fn authority_domain(self) -> EngineAuthorityDomainId {
        self.authority_domain
    }

    /// Returns the process-local diagnostic representation of this incarnation.
    #[must_use]
    pub const fn incarnation(self) -> u64 {
        self.incarnation.0
    }

    /// Returns the immutable observation lane assigned to this incarnation.
    #[must_use]
    pub const fn scope(self) -> PointerProviderScope {
        self.scope
    }
}
impl PointerStreamId {
    #[allow(
        dead_code,
        reason = "reserved for the host-frame pointer reducer boundary"
    )]
    pub(crate) const fn new(
        lease: PointerInputLease,
        pointer: PointerId,
        incarnation: u64,
    ) -> Self {
        Self {
            lease,
            pointer,
            incarnation: PointerStreamIncarnation(incarnation),
        }
    }

    /// Returns the exact provider incarnation which owns this stream.
    #[must_use]
    pub const fn lease(self) -> PointerInputLease {
        self.lease
    }

    /// Returns the provider-owned pointer identity within that incarnation.
    #[must_use]
    pub const fn pointer(self) -> PointerId {
        self.pointer
    }

    /// Returns the process-local diagnostic representation of this stream
    /// incarnation.
    ///
    /// Incarnations are minted monotonically by the core at journal commit.
    /// They are never provider input and cannot be reused after cancellation.
    #[must_use]
    pub const fn incarnation(self) -> u64 {
        self.incarnation.0
    }
}
impl PointerEdgeTicket {
    #[allow(
        dead_code,
        reason = "reserved for the host-frame reducer acknowledgement boundary"
    )]
    pub(crate) const fn new(lease: PointerInputLease, sequence: PointerEdgeSequence) -> Self {
        Self { lease, sequence }
    }

    /// Returns the exact provider incarnation which owns this edge.
    #[must_use]
    pub const fn lease(self) -> PointerInputLease {
        self.lease
    }

    /// Returns the engine authority domain which minted this ticket's lease.
    #[must_use]
    pub const fn authority_domain(self) -> EngineAuthorityDomainId {
        self.lease.authority_domain()
    }

    /// Returns the accepted provider edge sequence.
    #[must_use]
    pub const fn sequence(self) -> PointerEdgeSequence {
        self.sequence
    }
}
impl JournalPointerAuthority {
    fn unknown(reason: AuthorityUnavailableReason) -> Self {
        Self {
            buttons_complete: false,
            unavailable_reason: reason,
            pressed_buttons: BTreeMap::new(),
            captures: BTreeMap::new(),
        }
    }

    fn apply_checkpoint(&mut self, checkpoint: &PointerAuthorityCheckpoint) {
        self.pressed_buttons.clear();
        self.captures.clear();
        match checkpoint.pointers() {
            Authority::Known(pointers) => {
                self.buttons_complete = true;
                for pointer in pointers {
                    let capture = JournalPointerCapture::new(pointer.capture_owner());
                    for button in pointer.pressed_buttons() {
                        // A complete roster reports held buttons and capture,
                        // not the delivery endpoint of their original press.
                        let _ = self
                            .pressed_buttons
                            .insert((pointer.pointer(), *button), None);
                    }
                    let _ = self.captures.insert(pointer.pointer(), capture);
                }
            }
            Authority::Unknown(reason) => {
                self.buttons_complete = false;
                self.unavailable_reason = *reason;
            }
        }
    }

    fn apply_edge(&mut self, edge: &PointerEdge) -> Result<(), PointerJournalLedgerError> {
        let pointer = edge.pointer();
        self.captures
            .entry(pointer)
            .and_modify(|capture| capture.observe(edge.capture_owner()))
            .or_insert_with(|| JournalPointerCapture::new(edge.capture_owner()));
        match edge.kind() {
            PointerEdgeKind::ButtonPressed(button) => {
                if self.pressed_buttons.contains_key(&(pointer, button)) {
                    return Err(PointerJournalLedgerError::ButtonAlreadyPressed {
                        pointer,
                        button,
                        sequence: edge.sequence(),
                    });
                }
                let origin = match edge.delivery_owner() {
                    Authority::Known(PointerEventDeliveryOwner::Native(binding)) => Some(binding),
                    Authority::Known(_) | Authority::Unknown(_) => None,
                };
                let _ = self.pressed_buttons.insert((pointer, button), origin);
            }
            PointerEdgeKind::ButtonReleased(button) | PointerEdgeKind::ContactEnded(button) => {
                let was_pressed = self.pressed_buttons.remove(&(pointer, button)).is_some();
                if !was_pressed && self.buttons_complete {
                    return Err(PointerJournalLedgerError::ButtonNotPressed {
                        pointer,
                        button,
                        sequence: edge.sequence(),
                    });
                }
            }
            PointerEdgeKind::StreamCancelled(_) => {
                self.pressed_buttons
                    .retain(|(active, _), _| *active != pointer);
                let _ = self.captures.remove(&pointer);
            }
            PointerEdgeKind::Moved
            | PointerEdgeKind::Continuation(_)
            | PointerEdgeKind::CaptureAcquired(_)
            | PointerEdgeKind::CaptureChanged
            | PointerEdgeKind::StreamEnded
            | PointerEdgeKind::Scrolled(_) => {}
        }
        Ok(())
    }

    fn retire_stream(&mut self, pointer: PointerId) {
        self.pressed_buttons
            .retain(|(active, _), _| *active != pointer);
        let _ = self.captures.remove(&pointer);
    }

    fn references_native_binding(&self, pointer: PointerId, binding: ViewportBinding) -> bool {
        self.captures
            .get(&pointer)
            .is_some_and(|capture| capture.retained_native_binding == Some(binding))
            || self
                .pressed_buttons
                .range((pointer, PointerButton::Primary)..)
                .take_while(|((active, _), _)| *active == pointer)
                .any(|(_, origin)| *origin == Some(binding))
    }

    fn edge_authority_after(
        &self,
        pointer: PointerId,
    ) -> (AnyButtonDownAuthority, Authority<PointerCaptureOwner>) {
        let capture = self
            .captures
            .get(&pointer)
            .map(|capture| capture.observed)
            .unwrap_or(Authority::Unknown(
                AuthorityUnavailableReason::ProviderUnavailable,
            ));
        (self.button_authority(), capture)
    }

    fn pointer_has_pressed_buttons(&self, pointer: PointerId) -> bool {
        self.pressed_buttons
            .keys()
            .any(|(active, _)| *active == pointer)
    }

    fn button_authority(&self) -> AnyButtonDownAuthority {
        if !self.pressed_buttons.is_empty() {
            AnyButtonDownAuthority::KnownDown
        } else if self.buttons_complete {
            AnyButtonDownAuthority::KnownAllReleased
        } else {
            AnyButtonDownAuthority::Unknown(self.unavailable_reason)
        }
    }
}

impl JournalPointerCapture {
    fn new(observed: Authority<PointerCaptureOwner>) -> Self {
        let retained_native_binding = match observed {
            Authority::Known(PointerCaptureOwner::Native(binding)) => Some(binding),
            Authority::Known(_) | Authority::Unknown(_) => None,
        };
        Self {
            observed,
            retained_native_binding,
        }
    }

    fn observe(&mut self, observed: Authority<PointerCaptureOwner>) {
        self.observed = observed;
        if let Authority::Known(owner) = observed {
            self.retained_native_binding = match owner {
                PointerCaptureOwner::Native(binding) => Some(binding),
                PointerCaptureOwner::ProviderEndpoint
                | PointerCaptureOwner::Foreign
                | PointerCaptureOwner::None => None,
            };
        }
    }
}
impl PointerJournalLedgerVersion {
    const INITIAL: Self = Self(0);

    fn checked_next(self) -> Option<Self> {
        self.0.checked_add(1).map(Self)
    }
}
#[allow(
    dead_code,
    reason = "reserved for the independent pointer-ledger integration"
)]
impl PreparedPointerJournal {
    /// Returns the exact provider lease that was validated during preparation.
    pub(crate) const fn lease(&self) -> PointerInputLease {
        self.lease
    }

    /// Returns the journal that was validated during preparation.
    pub(crate) const fn journal(&self) -> &PointerEdgeJournal {
        &self.journal
    }
}
#[allow(
    dead_code,
    reason = "reserved for the independent pointer-ledger integration"
)]
impl AcceptedPointerEdge {
    /// Returns the exact core-minted stream affected by this accepted edge.
    pub(crate) const fn stream(&self) -> PointerStreamId {
        self.stream
    }

    /// Returns the exact acknowledgement identity of this accepted edge.
    pub(crate) const fn ticket(&self) -> PointerEdgeTicket {
        self.ticket
    }

    pub(crate) const fn button_authority_after(&self) -> AnyButtonDownAuthority {
        self.button_authority_after
    }

    pub(crate) const fn capture_authority_for_reduction(&self) -> Authority<PointerCaptureOwner> {
        self.capture_authority_for_reduction
    }

    pub(crate) const fn capture_authority_after(&self) -> Authority<PointerCaptureOwner> {
        self.capture_authority_after
    }
}
#[allow(
    dead_code,
    reason = "reserved for the independent pointer-ledger integration"
)]
impl PointerJournalCommit {
    pub(crate) const fn lease(&self) -> PointerInputLease {
        self.lease
    }

    pub(crate) const fn journal(&self) -> &PointerEdgeJournal {
        &self.journal
    }

    /// Returns the stream and ticket identity of every journal edge in exact
    /// provider order.
    ///
    /// Entry `n` corresponds to journal edge `n`. A `StreamCancelled` entry
    /// still names the stream it terminates; only a later edge for the same
    /// provider pointer receives a successor stream incarnation.
    pub(crate) fn accepted_edges(&self) -> &[AcceptedPointerEdge] {
        &self.accepted_edges
    }
}
/// Cloning a ledger creates a speculative snapshot of the same logical
/// authority, for `DockEngine`'s atomic candidate reduction. It is not an
/// independent provider authority: the clone intentionally shares the ledger
/// identity so a token prepared before candidate cloning can commit on the
/// candidate that will replace the source state.
impl Clone for PointerJournalLedger {
    fn clone(&self) -> Self {
        Self {
            authority_domain: self.authority_domain,
            identity: Arc::clone(&self.identity),
            version: self.version,
            last_incarnation: self.last_incarnation,
            last_stream_incarnation: self.last_stream_incarnation,
            active_streams: self.active_streams.clone(),
            authority: self.authority.clone(),
            authority_checkpoint_boundary: self.authority_checkpoint_boundary,
            last_checkpoint: self.last_checkpoint.clone(),
            active: self.active,
            surface_local_producer: self.surface_local_producer.clone(),
            retired_surface_local_producers: self.retired_surface_local_producers.clone(),
            retired: self.retired.clone(),
            compacted_retired_through: self.compacted_retired_through,
        }
    }
}
impl PartialEq for PointerJournalLedger {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.identity, &other.identity)
            && self.authority_domain == other.authority_domain
            && self.version == other.version
            && self.last_incarnation == other.last_incarnation
            && self.last_stream_incarnation == other.last_stream_incarnation
            && self.active_streams == other.active_streams
            && self.authority == other.authority
            && self.authority_checkpoint_boundary == other.authority_checkpoint_boundary
            && self.last_checkpoint == other.last_checkpoint
            && self.active == other.active
            && self.surface_local_producer == other.surface_local_producer
            && self.retired_surface_local_producers == other.retired_surface_local_producers
            && self.retired == other.retired
            && self.compacted_retired_through == other.compacted_retired_through
    }
}

impl Eq for PointerJournalLedger {}
#[allow(
    dead_code,
    reason = "reserved for the independent pointer-ledger integration"
)]
impl PointerJournalLedger {
    pub(crate) fn new(authority_domain: EngineAuthorityDomainId) -> Self {
        Self {
            authority_domain,
            identity: Arc::new(PointerJournalLedgerIdentity),
            version: PointerJournalLedgerVersion::INITIAL,
            last_incarnation: PointerProviderIncarnation(0),
            last_stream_incarnation: PointerStreamIncarnation(0),
            active_streams: BTreeMap::new(),
            authority: JournalPointerAuthority::unknown(
                AuthorityUnavailableReason::ProviderUnavailable,
            ),
            authority_checkpoint_boundary: None,
            last_checkpoint: None,
            active: None,
            surface_local_producer: None,
            retired_surface_local_producers: BTreeMap::new(),
            retired: BTreeMap::new(),
            compacted_retired_through: PointerProviderIncarnation(0),
        }
    }

    pub(crate) fn retention_manifest(&self) -> crate::retention::PointerRetentionManifest {
        let detailed_in_compacted_range = self
            .retired
            .keys()
            .filter(|lease| lease.incarnation <= self.compacted_retired_through)
            .count() as u64;
        let logical_compacted_leases = self
            .compacted_retired_through
            .0
            .saturating_sub(detailed_in_compacted_range);
        crate::retention::PointerRetentionManifest::new(
            usize::from(self.active.is_some()),
            self.active_streams.len(),
            self.retired.len(),
            usize::from(self.compacted_retired_through.0 > 0),
            logical_compacted_leases,
        )
    }

    /// Returns the sole live provider lease, if this authority currently owns one.
    pub(crate) const fn active_lease(&self) -> Option<PointerInputLease> {
        match self.active {
            Some(active) => Some(active.lease),
            None => None,
        }
    }

    /// Reads device classification only from this exact active stream.
    pub(crate) fn stream_device(&self, stream: PointerStreamId) -> Option<PointerDeviceKind> {
        if self.active_lease() != Some(stream.lease()) {
            return None;
        }
        self.active_streams
            .get(&stream.pointer())
            .filter(|active| active.id == stream)
            .map(|active| active.device)
    }

    /// Checks this exact native stream's observed primary-button continuity.
    ///
    /// A checkpoint's held-button roster has no press origin. Retained capture
    /// dependencies and another pointer's held buttons grant no handoff proof.
    pub(crate) fn native_primary_press_continuity(
        &self,
        stream: PointerStreamId,
        binding: ViewportBinding,
    ) -> bool {
        self.active_lease() == Some(stream.lease())
            && matches!(
                stream.lease().scope(),
                PointerProviderScope::NativeJoined(_)
            )
            && self
                .active_streams
                .get(&stream.pointer())
                .is_some_and(|active| active.id == stream)
            && self
                .authority
                .pressed_buttons
                .get(&(stream.pointer(), PointerButton::Primary))
                == Some(&Some(binding))
            && self
                .authority
                .captures
                .get(&stream.pointer())
                .is_some_and(|capture| {
                    capture.observed == Authority::Known(PointerCaptureOwner::Native(binding))
                })
    }

    pub(crate) fn bind_surface_local_producer(
        &mut self,
        lease: PointerInputLease,
        monitor: SurfaceLocalPointerProducerMonitor,
    ) {
        assert!(
            lease.scope().is_local(),
            "only a local lease can bind an affine producer monitor",
        );
        assert_eq!(
            self.active_lease(),
            Some(lease),
            "only the exact active lease can bind its producer monitor",
        );
        assert!(
            self.surface_local_producer.is_none(),
            "one active pointer provider can retain only one producer monitor",
        );
        self.surface_local_producer = Some((lease, monitor));
    }

    pub(crate) fn abandoned_surface_local_provider(&self) -> Option<PointerInputLease> {
        let (lease, monitor) = self.surface_local_producer.as_ref()?;
        (self.active_lease() == Some(*lease) && monitor.is_abandoned()).then_some(*lease)
    }

    pub(crate) fn abandoned_retired_surface_local_provider(&self) -> Option<PointerInputLease> {
        self.retired_surface_local_producers
            .iter()
            .find_map(|(lease, monitor)| monitor.is_abandoned().then_some(*lease))
    }

    pub(crate) fn retained_committed_through(
        &self,
        lease: PointerInputLease,
    ) -> Result<PointerEdgeSequence, PointerJournalLedgerError> {
        if let Some(active) = self.active
            && active.lease == lease
        {
            return Ok(active.committed_through);
        }
        if let Some(retired) = self.retired.get(&lease) {
            return Ok(retired.committed_through);
        }
        self.require_active(lease)
    }

    pub(crate) fn button_authority(&self) -> AnyButtonDownAuthority {
        self.authority.button_authority()
    }

    /// Creates the sole live provider incarnation with one immutable scope.
    ///
    /// `committed_through` is the provider watermark already known at the
    /// authority handoff. The first journal must begin exactly there.
    pub(crate) fn create_provider(
        &mut self,
        scope: PointerProviderScope,
        committed_through: PointerEdgeSequence,
    ) -> Result<PointerInputLease, PointerJournalLedgerError> {
        if let Some(active) = self.active {
            return Err(PointerJournalLedgerError::ProviderAlreadyActive {
                active: active.lease,
            });
        }
        self.validate_scope(scope)?;

        let incarnation = self
            .last_incarnation
            .checked_next()
            .ok_or(PointerJournalLedgerError::ProviderIncarnationExhausted)?;
        let next_version = self.next_version()?;
        let lease = PointerInputLease::new(self.authority_domain, incarnation.0, scope);
        self.last_incarnation = incarnation;
        self.version = next_version;
        self.active_streams.clear();
        self.authority =
            JournalPointerAuthority::unknown(AuthorityUnavailableReason::ProviderUnavailable);
        self.authority_checkpoint_boundary = Some(PointerAuthorityCheckpointBoundary::Enrollment(
            committed_through,
        ));
        self.last_checkpoint = None;
        self.active = Some(ActivePointerProvider {
            lease,
            committed_through,
            last_observed_at: None,
        });
        self.surface_local_producer = None;
        Ok(lease)
    }

    /// Ends live streams and revokes button/capture facts without retiring the provider.
    pub(crate) fn cancel_live_streams_for_reset(
        &mut self,
        lease: PointerInputLease,
    ) -> Result<(), PointerJournalLedgerError> {
        let committed_through = self.require_active(lease)?;
        let next_version = self.next_version()?;
        self.active_streams.clear();
        self.authority = JournalPointerAuthority::unknown(AuthorityUnavailableReason::NotReported);
        self.authority_checkpoint_boundary = Some(
            PointerAuthorityCheckpointBoundary::RecoveryResync(committed_through),
        );
        self.last_checkpoint = None;
        self.version = next_version;
        Ok(())
    }

    /// Ends only streams with a held-button origin or retained capture at this binding.
    ///
    /// A last idle delivery is not ownership. Unknown capture keeps its preceding
    /// dependency without granting observed capture authority. Complete button
    /// knowledge for unrelated streams survives this narrower terminal fact.
    pub(crate) fn retire_native_binding_input(
        &mut self,
        lease: PointerInputLease,
        binding: ViewportBinding,
    ) -> Result<Vec<PointerStreamId>, PointerJournalLedgerError> {
        self.require_active(lease)?;
        let next_version = self.next_version()?;
        let pointers = self
            .authority
            .captures
            .keys()
            .copied()
            .filter(|pointer| self.authority.references_native_binding(*pointer, binding))
            .collect::<Vec<_>>();
        let mut streams = Vec::with_capacity(pointers.len());
        for pointer in pointers {
            if let Some(stream) = self.active_streams.remove(&pointer) {
                streams.push(stream.id);
            }
            self.authority.retire_stream(pointer);
        }
        if matches!(
            self.authority_checkpoint_boundary,
            Some(PointerAuthorityCheckpointBoundary::Enrollment(_))
        ) {
            self.authority_checkpoint_boundary = None;
        }
        self.last_checkpoint = None;
        self.version = next_version;
        Ok(streams)
    }

    /// Validates a fresh recovery report without changing published authority.
    pub(crate) fn validate_reset_authority_resync(
        &self,
        lease: PointerInputLease,
        observed_through: PointerEdgeSequence,
    ) -> Result<(), PointerJournalLedgerError> {
        let committed_through = self.require_active(lease)?;
        if observed_through < committed_through {
            return Err(PointerJournalLedgerError::JournalReplay {
                lease,
                committed_through,
                submitted_previous: observed_through,
            });
        }
        if observed_through > committed_through {
            return Err(PointerJournalLedgerError::JournalAhead {
                lease,
                committed_through,
                submitted_previous: observed_through,
            });
        }
        if self.authority_checkpoint_boundary
            != Some(PointerAuthorityCheckpointBoundary::RecoveryResync(
                observed_through,
            ))
        {
            return Err(
                PointerJournalLedgerError::AuthorityCheckpointAfterPointerEdges {
                    lease,
                    observed_through,
                },
            );
        }
        Ok(())
    }

    /// Permanently retires one exact live provider incarnation.
    pub(crate) fn retire_provider(
        &mut self,
        lease: PointerInputLease,
    ) -> Result<(), PointerJournalLedgerError> {
        let committed_through = self.require_active(lease)?;
        let next_version = self.next_version()?;
        let tombstone = RetiredPointerInputLease {
            lease,
            committed_through,
        };
        self.active = None;
        if let Some((registered, monitor)) = self.surface_local_producer.take() {
            assert_eq!(
                registered, lease,
                "only the retiring surface-local lease can own its producer monitor",
            );
            let previous = self.retired_surface_local_producers.insert(lease, monitor);
            assert!(
                previous.is_none(),
                "one lease can retain only one retired monitor"
            );
        }
        self.active_streams.clear();
        self.authority =
            JournalPointerAuthority::unknown(AuthorityUnavailableReason::ProviderUnavailable);
        self.authority_checkpoint_boundary = None;
        self.last_checkpoint = None;
        let _ = self.retired.insert(lease, tombstone);
        self.version = next_version;
        Ok(())
    }

    /// Compacts one exact retired lease after its producer lane has stopped and joined.
    ///
    /// The detailed final watermark is discarded, but the monotonic incarnation frontier
    /// continues to classify every delayed journal from this lease as retired. Callers must own
    /// the typed quiescence proof; ordinary retirement alone is not sufficient.
    pub(crate) fn compact_quiesced_backend(
        &mut self,
        receipt: &BackendIngressDrainReceipt,
    ) -> Result<(), PointerJournalLedgerError> {
        self.compact_retired_provider(receipt.lease().pointer_provider())
    }

    /// Retires and compacts one surface-local provider after its sole producer joined.
    ///
    /// Unlike ordinary retirement, this boundary never materializes a detailed
    /// tombstone. The exact lease and final producer watermark are validated in
    /// the same candidate which advances the compacted incarnation frontier.
    pub(crate) fn retire_quiesced_surface_local(
        &mut self,
        receipt: &SurfaceLocalPointerDrainReceipt,
    ) -> Result<SurfaceLocalPointerQuiescenceDisposition, PointerJournalLedgerError> {
        let lease = receipt.lease();
        if receipt.is_consumed() {
            return Err(PointerJournalLedgerError::SurfaceLocalQuiescenceReceiptConsumed { lease });
        }
        if !lease.scope().is_local() {
            return Err(PointerJournalLedgerError::SurfaceLocalQuiescenceScopeMismatch { lease });
        }
        if lease.authority_domain() != self.authority_domain {
            return Err(PointerJournalLedgerError::ForeignLease {
                expected: self.authority_domain,
                submitted: lease.authority_domain(),
            });
        }
        let (committed_through, disposition) = if let Some(active) = self.active
            && active.lease == lease
        {
            (
                active.committed_through,
                SurfaceLocalPointerQuiescenceDisposition::RetiredActive,
            )
        } else if let Some(retired) = self.retired.get(&lease) {
            (
                retired.committed_through,
                SurfaceLocalPointerQuiescenceDisposition::CompactedPreviouslyRetired,
            )
        } else if lease.incarnation <= self.compacted_retired_through && lease.incarnation.0 != 0 {
            return Err(PointerJournalLedgerError::CompactedLease { lease });
        } else {
            return Err(PointerJournalLedgerError::UnknownLease { lease });
        };
        if receipt.committed_through() != committed_through {
            return Err(
                PointerJournalLedgerError::SurfaceLocalQuiescenceWatermarkMismatch {
                    lease,
                    committed_through,
                    submitted: receipt.committed_through(),
                },
            );
        }
        let next_version = self.next_version()?;
        match disposition {
            SurfaceLocalPointerQuiescenceDisposition::RetiredActive => {
                self.active = None;
                self.surface_local_producer = None;
                self.active_streams.clear();
                self.authority = JournalPointerAuthority::unknown(
                    AuthorityUnavailableReason::ProviderUnavailable,
                );
                self.authority_checkpoint_boundary = None;
                self.last_checkpoint = None;
            }
            SurfaceLocalPointerQuiescenceDisposition::CompactedPreviouslyRetired => {
                let removed = self.retired.remove(&lease);
                debug_assert!(removed.is_some(), "validated retirement remains present");
                let _ = self.retired_surface_local_producers.remove(&lease);
            }
        }
        self.compacted_retired_through = self.compacted_retired_through.max(lease.incarnation);
        self.version = next_version;
        Ok(disposition)
    }

    pub(crate) fn retire_abandoned_active_surface_local(
        &mut self,
        lease: PointerInputLease,
    ) -> Result<(), PointerJournalLedgerError> {
        if !lease.scope().is_local() {
            return Err(PointerJournalLedgerError::SurfaceLocalQuiescenceScopeMismatch { lease });
        }
        if self.abandoned_surface_local_provider() != Some(lease) {
            return Err(PointerJournalLedgerError::SurfaceLocalProducerNotAbandoned { lease });
        }
        let _ = self.require_active(lease)?;
        let next_version = self.next_version()?;
        self.active = None;
        self.surface_local_producer = None;
        self.active_streams.clear();
        self.authority =
            JournalPointerAuthority::unknown(AuthorityUnavailableReason::ProviderUnavailable);
        self.authority_checkpoint_boundary = None;
        self.last_checkpoint = None;
        self.compacted_retired_through = self.compacted_retired_through.max(lease.incarnation);
        self.version = next_version;
        Ok(())
    }

    pub(crate) fn compact_abandoned_retired_surface_local(
        &mut self,
        lease: PointerInputLease,
    ) -> Result<(), PointerJournalLedgerError> {
        let Some(monitor) = self.retired_surface_local_producers.get(&lease) else {
            return Err(PointerJournalLedgerError::UnknownLease { lease });
        };
        if !monitor.is_abandoned() {
            return Err(PointerJournalLedgerError::SurfaceLocalProducerNotAbandoned { lease });
        }
        if !self.retired.contains_key(&lease) {
            return Err(PointerJournalLedgerError::UnknownLease { lease });
        }
        let next_version = self.next_version()?;
        let _ = self.retired.remove(&lease);
        let _ = self.retired_surface_local_producers.remove(&lease);
        self.compacted_retired_through = self.compacted_retired_through.max(lease.incarnation);
        self.version = next_version;
        Ok(())
    }

    fn compact_retired_provider(
        &mut self,
        lease: PointerInputLease,
    ) -> Result<(), PointerJournalLedgerError> {
        if lease.authority_domain() != self.authority_domain {
            return Err(PointerJournalLedgerError::ForeignLease {
                expected: self.authority_domain,
                submitted: lease.authority_domain(),
            });
        }
        if !self.retired.contains_key(&lease) {
            if lease.incarnation <= self.compacted_retired_through && lease.incarnation.0 != 0 {
                return Err(PointerJournalLedgerError::CompactedLease { lease });
            }
            return Err(PointerJournalLedgerError::UnknownLease { lease });
        }
        let next_version = self.next_version()?;
        let removed = self.retired.remove(&lease);
        debug_assert!(removed.is_some(), "validated retirement remains present");
        let _ = self.retired_surface_local_producers.remove(&lease);
        self.compacted_retired_through = self.compacted_retired_through.max(lease.incarnation);
        self.version = next_version;
        Ok(())
    }

    /// Validates and freezes one journal candidate without mutating the ledger.
    ///
    /// The returned token contains no tickets and cannot authorize input on
    /// its own. The host must first complete its exact receipt join, then pass
    /// the token to [`Self::commit_prepared`]. Rejected receipts can therefore
    /// retry preparation without consuming the provider watermark.
    pub(crate) fn prepare_candidate(
        &self,
        lease: PointerInputLease,
        journal: PointerEdgeJournal,
    ) -> Result<PreparedPointerJournal, PointerJournalLedgerError> {
        let committed_through = self.require_active(lease)?;
        journal
            .validate()
            .map_err(PointerJournalLedgerError::InvalidJournal)?;
        if journal.previous() < committed_through {
            return Err(PointerJournalLedgerError::JournalReplay {
                lease,
                committed_through,
                submitted_previous: journal.previous(),
            });
        }
        if journal.previous() > committed_through {
            return Err(PointerJournalLedgerError::JournalAhead {
                lease,
                committed_through,
                submitted_previous: journal.previous(),
            });
        }
        for edge in journal.edges() {
            let submitted = edge.location().lane();
            let matches_scope = matches!(
                (lease.scope(), submitted),
                (
                    PointerProviderScope::DesktopGlobal,
                    PointerEdgeLocationLane::Desktop
                ) | (
                    PointerProviderScope::NativeJoined(_),
                    PointerEdgeLocationLane::Desktop
                        | PointerEdgeLocationLane::NativeLocal
                        | PointerEdgeLocationLane::NativeLocalToDesktop
                ) | (
                    PointerProviderScope::HostLocal(_),
                    PointerEdgeLocationLane::HostLocal
                ) | (
                    PointerProviderScope::SurfaceLocal(_),
                    PointerEdgeLocationLane::SurfaceLocal
                )
            );
            if !matches_scope {
                return Err(PointerJournalLedgerError::LocationScopeMismatch {
                    lease,
                    sequence: edge.sequence(),
                    submitted,
                });
            }
            self.validate_capture_scope(lease, edge)?;
            self.validate_delivery_scope(lease, edge)?;
            self.validate_desktop_route_scope(lease, edge)?;
            self.validate_native_local_route_scope(lease, edge)?;
            self.validate_scroll_scope(lease, edge)?;
        }
        if let Some(checkpoint) = journal.authority_checkpoint()
            && let Authority::Known(pointers) = checkpoint.pointers()
        {
            for pointer in pointers {
                self.validate_capture_owner(
                    lease,
                    checkpoint.observed_through(),
                    pointer.capture_owner(),
                )?;
            }
        }

        Ok(PreparedPointerJournal {
            ledger_identity: Arc::clone(&self.identity),
            ledger_version: self.version,
            lease,
            committed_through,
            journal,
        })
    }

    /// Atomically commits one exact journal prepared by this ledger instance.
    ///
    /// The prepared token is accepted only when the live provider lease,
    /// committed watermark, and ledger mutation version still exactly match
    /// the state observed by `prepare_candidate`. Every successful edge then
    /// receives a ticket and exact stream bound to that lease. A successful
    /// empty interval also advances the ledger version so the token cannot be
    /// replayed.
    pub(crate) fn commit_prepared(
        &mut self,
        prepared: PreparedPointerJournal,
    ) -> Result<PointerJournalCommit, PointerJournalLedgerError> {
        if !Arc::ptr_eq(&self.identity, &prepared.ledger_identity) {
            return Err(PointerJournalLedgerError::ForeignPreparedJournal);
        }

        let active_through = self.require_active(prepared.lease)?;
        if self.version != prepared.ledger_version
            || active_through != prepared.committed_through
            || prepared.journal.previous() != prepared.committed_through
        {
            return Err(PointerJournalLedgerError::PreparedJournalStale {
                lease: prepared.lease,
                prepared_version: prepared.ledger_version.0,
                active_version: self.version.0,
                prepared_committed_through: prepared.committed_through,
                active_committed_through: active_through,
            });
        }

        let next_version = self.next_version()?;
        let (
            last_stream_incarnation,
            active_streams,
            authority,
            authority_checkpoint_boundary,
            last_checkpoint,
            accepted_edges,
            last_observed_at,
        ) = self.plan_accepted_edges(prepared.lease, &prepared.journal)?;

        let Some(active) = &mut self.active else {
            return Err(PointerJournalLedgerError::UnknownLease {
                lease: prepared.lease,
            });
        };
        if active.lease != prepared.lease {
            return Err(PointerJournalLedgerError::UnknownLease {
                lease: prepared.lease,
            });
        }
        active.committed_through = prepared.journal.through();
        active.last_observed_at = last_observed_at;
        self.last_stream_incarnation = last_stream_incarnation;
        self.active_streams = active_streams;
        self.authority = authority;
        self.authority_checkpoint_boundary = authority_checkpoint_boundary;
        self.last_checkpoint = last_checkpoint;
        self.version = next_version;

        Ok(PointerJournalCommit {
            lease: prepared.lease,
            journal: prepared.journal,
            accepted_edges,
        })
    }

    fn validate_capture_scope(
        &self,
        lease: PointerInputLease,
        edge: &PointerEdge,
    ) -> Result<(), PointerJournalLedgerError> {
        self.validate_capture_owner(lease, edge.sequence(), edge.capture_owner())
    }

    fn validate_delivery_scope(
        &self,
        lease: PointerInputLease,
        edge: &PointerEdge,
    ) -> Result<(), PointerJournalLedgerError> {
        let Authority::Known(delivery_owner) = edge.delivery_owner() else {
            return Ok(());
        };

        match (lease.scope(), delivery_owner) {
            (
                PointerProviderScope::DesktopGlobal | PointerProviderScope::NativeJoined(_),
                PointerEventDeliveryOwner::ProviderEndpoint,
            )
            | (PointerProviderScope::HostLocal(_), PointerEventDeliveryOwner::Native(_))
            | (PointerProviderScope::SurfaceLocal(_), PointerEventDeliveryOwner::Native(_)) => {
                Err(PointerJournalLedgerError::DeliveryScopeMismatch {
                    lease,
                    sequence: edge.sequence(),
                    submitted: delivery_owner,
                })
            }
            (
                PointerProviderScope::DesktopGlobal | PointerProviderScope::NativeJoined(_),
                PointerEventDeliveryOwner::Native(binding),
            ) if binding.authority_domain() != self.authority_domain => {
                Err(PointerJournalLedgerError::ForeignDeliveryBinding {
                    lease,
                    sequence: edge.sequence(),
                    binding,
                    expected: self.authority_domain,
                    submitted: binding.authority_domain(),
                })
            }
            _ => Ok(()),
        }
    }

    fn validate_capture_owner(
        &self,
        lease: PointerInputLease,
        sequence: PointerEdgeSequence,
        capture: Authority<PointerCaptureOwner>,
    ) -> Result<(), PointerJournalLedgerError> {
        let Authority::Known(capture_owner) = capture else {
            return Ok(());
        };

        match (lease.scope(), capture_owner) {
            (
                PointerProviderScope::DesktopGlobal | PointerProviderScope::NativeJoined(_),
                PointerCaptureOwner::ProviderEndpoint,
            )
            | (PointerProviderScope::HostLocal(_), PointerCaptureOwner::Native(_))
            | (PointerProviderScope::SurfaceLocal(_), PointerCaptureOwner::Native(_)) => {
                Err(PointerJournalLedgerError::CaptureScopeMismatch {
                    lease,
                    sequence,
                    submitted: capture_owner,
                })
            }
            (
                PointerProviderScope::DesktopGlobal | PointerProviderScope::NativeJoined(_),
                PointerCaptureOwner::Native(binding),
            ) if binding.authority_domain() != self.authority_domain => {
                Err(PointerJournalLedgerError::ForeignCaptureBinding {
                    lease,
                    sequence,
                    binding,
                    expected: self.authority_domain,
                    submitted: binding.authority_domain(),
                })
            }
            _ => Ok(()),
        }
    }

    fn validate_desktop_route_scope(
        &self,
        lease: PointerInputLease,
        edge: &PointerEdge,
    ) -> Result<(), PointerJournalLedgerError> {
        let route = match edge.location() {
            PointerEdgeLocation::Desktop { route }
            | PointerEdgeLocation::NativeLocalToDesktop { desktop: route, .. } => route,
            PointerEdgeLocation::NativeLocal { .. }
            | PointerEdgeLocation::HostLocal { .. }
            | PointerEdgeLocation::SurfaceLocal { .. } => return Ok(()),
        };
        let Authority::Known(DesktopHoveredWindow::Dock(binding)) = route.hovered() else {
            return Ok(());
        };
        if binding.authority_domain() != self.authority_domain {
            return Err(PointerJournalLedgerError::ForeignDesktopRouteBinding {
                lease,
                sequence: edge.sequence(),
                binding,
                expected: self.authority_domain,
                submitted: binding.authority_domain(),
            });
        }
        Ok(())
    }

    fn validate_native_local_route_scope(
        &self,
        lease: PointerInputLease,
        edge: &PointerEdge,
    ) -> Result<(), PointerJournalLedgerError> {
        let route = match edge.location() {
            PointerEdgeLocation::NativeLocal {
                route: Authority::Known(route),
            }
            | PointerEdgeLocation::NativeLocalToDesktop { local: route, .. } => route,
            PointerEdgeLocation::Desktop { .. }
            | PointerEdgeLocation::NativeLocal {
                route: Authority::Unknown(_),
            }
            | PointerEdgeLocation::HostLocal { .. }
            | PointerEdgeLocation::SurfaceLocal { .. } => return Ok(()),
        };
        let binding = route.binding();
        if binding.authority_domain() != self.authority_domain {
            return Err(PointerJournalLedgerError::ForeignNativeLocalRouteBinding {
                lease,
                sequence: edge.sequence(),
                binding,
                expected: self.authority_domain,
                submitted: binding.authority_domain(),
            });
        }
        Ok(())
    }

    fn validate_scroll_scope(
        &self,
        lease: PointerInputLease,
        edge: &PointerEdge,
    ) -> Result<(), PointerJournalLedgerError> {
        let PointerEdgeKind::Scrolled(scroll) = edge.kind() else {
            return Ok(());
        };
        let Authority::Known(endpoint) = scroll.delivery() else {
            return Ok(());
        };
        if endpoint.host().authority_domain() != self.authority_domain {
            return Err(PointerJournalLedgerError::ForeignScrollDeliveryHost {
                lease,
                sequence: edge.sequence(),
                host: endpoint.host(),
            });
        }
        match lease.scope() {
            PointerProviderScope::NativeJoined(host) if endpoint.host() != host => {
                Err(PointerJournalLedgerError::ScrollDeliveryScopeMismatch {
                    lease,
                    sequence: edge.sequence(),
                    endpoint,
                })
            }
            PointerProviderScope::HostLocal(host)
                if endpoint.host() != host || endpoint.binding().is_some() =>
            {
                Err(PointerJournalLedgerError::ScrollDeliveryScopeMismatch {
                    lease,
                    sequence: edge.sequence(),
                    endpoint,
                })
            }
            PointerProviderScope::SurfaceLocal(scope)
                if endpoint.host() != scope.host()
                    || endpoint.surface() != scope.surface()
                    || endpoint.binding() != scope.endpoint().native_binding() =>
            {
                Err(PointerJournalLedgerError::ScrollDeliveryScopeMismatch {
                    lease,
                    sequence: edge.sequence(),
                    endpoint,
                })
            }
            PointerProviderScope::DesktopGlobal | PointerProviderScope::NativeJoined(_)
                if endpoint.binding().is_none() =>
            {
                Err(
                    PointerJournalLedgerError::NativeScrollDeliveryBindingMissing {
                        lease,
                        sequence: edge.sequence(),
                        endpoint,
                    },
                )
            }
            PointerProviderScope::DesktopGlobal
            | PointerProviderScope::NativeJoined(_)
            | PointerProviderScope::HostLocal(_)
            | PointerProviderScope::SurfaceLocal(SurfaceLocalPointerScope { .. }) => {
                if let (Authority::Known(owner), Some(binding)) =
                    (edge.delivery_owner(), endpoint.binding())
                    && owner != PointerEventDeliveryOwner::Native(binding)
                    && owner != PointerEventDeliveryOwner::ProviderEndpoint
                {
                    return Err(PointerJournalLedgerError::ScrollDeliveryOwnerMismatch {
                        lease,
                        sequence: edge.sequence(),
                        endpoint,
                        owner,
                    });
                }
                Ok(())
            }
        }
    }

    fn plan_accepted_edges(
        &self,
        lease: PointerInputLease,
        journal: &PointerEdgeJournal,
    ) -> Result<
        (
            PointerStreamIncarnation,
            BTreeMap<PointerId, ActivePointerStream>,
            JournalPointerAuthority,
            Option<PointerAuthorityCheckpointBoundary>,
            Option<PointerAuthorityCheckpoint>,
            Vec<AcceptedPointerEdge>,
            Option<Duration>,
        ),
        PointerJournalLedgerError,
    > {
        let mut last_stream_incarnation = self.last_stream_incarnation;
        let mut active_streams = self.active_streams.clone();
        let mut authority = self.authority.clone();
        let mut authority_checkpoint_boundary = self.authority_checkpoint_boundary;
        let mut last_checkpoint = self.last_checkpoint.clone();
        let mut accepted_edges = Vec::with_capacity(journal.len());
        let mut last_observed_at = self.active.and_then(|active| active.last_observed_at);

        if let Some(checkpoint) = journal.authority_checkpoint() {
            if authority_checkpoint_boundary.map(PointerAuthorityCheckpointBoundary::watermark)
                != Some(checkpoint.observed_through())
            {
                return Err(
                    PointerJournalLedgerError::AuthorityCheckpointAfterPointerEdges {
                        lease,
                        observed_through: checkpoint.observed_through(),
                    },
                );
            }
            if matches!(
                authority_checkpoint_boundary,
                Some(PointerAuthorityCheckpointBoundary::RecoveryResync(_))
            ) {
                authority.apply_checkpoint(checkpoint);
                if matches!(checkpoint.pointers(), Authority::Known(_)) {
                    authority_checkpoint_boundary = None;
                }
                // Unknown reports keep the recovery opportunity open for fresh facts,
                // including a later complete observation at this same pointer watermark.
                last_checkpoint = None;
            } else if last_checkpoint.as_ref().is_some_and(|retained| {
                retained.observed_through() == checkpoint.observed_through()
            }) {
                if last_checkpoint.as_ref() != Some(checkpoint) {
                    return Err(PointerJournalLedgerError::ConflictingAuthorityCheckpoint {
                        lease,
                        observed_through: checkpoint.observed_through(),
                    });
                }
            } else {
                authority.apply_checkpoint(checkpoint);
                last_checkpoint = Some(checkpoint.clone());
            }
        }

        for edge in journal.edges() {
            if let PointerEdgeKind::Continuation(token) | PointerEdgeKind::CaptureAcquired(token) =
                edge.kind()
                && active_streams.get(&edge.pointer()).map(|stream| stream.id) != Some(token.owner)
            {
                return Err(PointerJournalLedgerError::ContinuationStreamMismatch {
                    sequence: edge.sequence(),
                });
            }
            if let Some(submitted) = edge.metadata().observed_at() {
                if let Some(previous) = last_observed_at
                    && submitted < previous
                {
                    return Err(PointerJournalLedgerError::NonMonotonicTime {
                        sequence: edge.sequence(),
                        previous,
                        submitted,
                    });
                }
                last_observed_at = Some(submitted);
            }
            let metadata = edge.metadata();
            let mut stream = match active_streams.get(&edge.pointer()).copied() {
                Some(stream) => stream,
                None => {
                    last_stream_incarnation = last_stream_incarnation
                        .checked_next()
                        .ok_or(PointerJournalLedgerError::StreamIncarnationExhausted)?;
                    let stream = ActivePointerStream {
                        id: PointerStreamId::new(lease, edge.pointer(), last_stream_incarnation.0),
                        device: metadata.device(),
                    };
                    let _ = active_streams.insert(edge.pointer(), stream);
                    stream
                }
            };
            if metadata.device() != PointerDeviceKind::Unknown {
                if stream.device != PointerDeviceKind::Unknown && stream.device != metadata.device()
                {
                    return Err(PointerJournalLedgerError::DeviceKindChanged {
                        sequence: edge.sequence(),
                        stream: stream.id,
                        expected: stream.device,
                        submitted: metadata.device(),
                    });
                }
                stream.device = metadata.device();
                active_streams.insert(edge.pointer(), stream);
            }
            authority.apply_edge(edge)?;
            let stream_ended = edge.ends_stream();
            let (button_authority_after, capture_authority_for_reduction) =
                authority.edge_authority_after(edge.pointer());
            if matches!(
                edge.kind(),
                PointerEdgeKind::ContactEnded(_) | PointerEdgeKind::StreamEnded
            ) && authority.pointer_has_pressed_buttons(edge.pointer())
            {
                return Err(PointerJournalLedgerError::StreamEndLeavesButtonsPressed {
                    pointer: edge.pointer(),
                    sequence: edge.sequence(),
                });
            }
            if stream_ended {
                authority.retire_stream(edge.pointer());
            }
            let capture_authority_after = authority.edge_authority_after(edge.pointer()).1;
            accepted_edges.push(AcceptedPointerEdge {
                stream: stream.id,
                ticket: PointerEdgeTicket::new(lease, edge.sequence()),
                button_authority_after,
                capture_authority_for_reduction,
                capture_authority_after,
            });

            if stream_ended {
                let _ = active_streams.remove(&edge.pointer());
            }
        }

        if !journal.is_empty() {
            authority_checkpoint_boundary = match authority_checkpoint_boundary {
                Some(PointerAuthorityCheckpointBoundary::RecoveryResync(_)) => Some(
                    PointerAuthorityCheckpointBoundary::RecoveryResync(journal.through()),
                ),
                _ => None,
            };
            last_checkpoint = None;
        }

        Ok((
            last_stream_incarnation,
            active_streams,
            authority,
            authority_checkpoint_boundary,
            last_checkpoint,
            accepted_edges,
            last_observed_at,
        ))
    }

    fn validate_scope(&self, scope: PointerProviderScope) -> Result<(), PointerJournalLedgerError> {
        if let PointerProviderScope::NativeJoined(host) = scope {
            let submitted = host.authority_domain();
            return if submitted == self.authority_domain {
                Ok(())
            } else {
                Err(PointerJournalLedgerError::ForeignNativeJoinedHost {
                    host,
                    expected: self.authority_domain,
                    submitted,
                })
            };
        }
        if let PointerProviderScope::HostLocal(host) = scope {
            let submitted = host.authority_domain();
            return if submitted == self.authority_domain {
                Ok(())
            } else {
                Err(PointerJournalLedgerError::ForeignHostLocalHost {
                    host,
                    expected: self.authority_domain,
                    submitted,
                })
            };
        }
        let PointerProviderScope::SurfaceLocal(local) = scope else {
            return Ok(());
        };
        let host_domain = local.host().authority_domain();
        if host_domain != self.authority_domain {
            return Err(PointerJournalLedgerError::ForeignSurfaceLocalHost {
                host: local.host(),
                expected: self.authority_domain,
                submitted: host_domain,
            });
        }
        if let Some(binding) = local.endpoint().native_binding()
            && binding.authority_domain() != self.authority_domain
        {
            return Err(PointerJournalLedgerError::ForeignSurfaceLocalBinding {
                binding,
                expected: self.authority_domain,
                submitted: binding.authority_domain(),
            });
        }
        Ok(())
    }

    fn require_active(
        &self,
        lease: PointerInputLease,
    ) -> Result<PointerEdgeSequence, PointerJournalLedgerError> {
        if lease.authority_domain() != self.authority_domain {
            return Err(PointerJournalLedgerError::ForeignLease {
                expected: self.authority_domain,
                submitted: lease.authority_domain(),
            });
        }
        if let Some(active) = self.active
            && active.lease == lease
        {
            return Ok(active.committed_through);
        }
        if let Some(tombstone) = self.retired.get(&lease) {
            return Err(PointerJournalLedgerError::RetiredLease {
                lease: tombstone.lease,
                committed_through: tombstone.committed_through,
            });
        }
        if lease.incarnation <= self.compacted_retired_through && lease.incarnation.0 != 0 {
            return Err(PointerJournalLedgerError::CompactedLease { lease });
        }
        Err(PointerJournalLedgerError::UnknownLease { lease })
    }

    fn next_version(&self) -> Result<PointerJournalLedgerVersion, PointerJournalLedgerError> {
        self.version
            .checked_next()
            .ok_or(PointerJournalLedgerError::LedgerVersionExhausted)
    }
}

#[cfg(test)]
mod native_joined_tests {
    use super::*;
    use crate::ids::WorkspaceEpoch;
    use crate::presentation_observation::PresentationLedger;
    use crate::viewport::{WindowIncarnation, WindowToken};

    fn setup() -> (PointerJournalLedger, PresentationLedger, PointerInputLease) {
        let domain = EngineAuthorityDomainId::new_for_test(1);
        let mut presentations = PresentationLedger::new(domain);
        let host = presentations.create_host().expect("joined host");
        let mut ledger = PointerJournalLedger::new(domain);
        let lease = ledger
            .create_provider(
                PointerProviderScope::NativeJoined(host),
                PointerEdgeSequence::new(0),
            )
            .expect("native joined provider");
        (ledger, presentations, lease)
    }

    fn binding(domain: EngineAuthorityDomainId, token: u64) -> ViewportBinding {
        ViewportBinding::new(
            domain,
            WorkspaceEpoch::new(1),
            SurfaceId::new(token),
            WindowToken::new(token),
            WindowIncarnation::new(1),
        )
    }

    fn local_location(binding: ViewportBinding) -> PointerEdgeLocation {
        PointerEdgeLocation::NativeLocal {
            route: Authority::Known(NativeLocalPointerRoute::new(
                binding,
                CoordinateGeneration::new(7),
                PhysicalPoint::new(40.0, 20.0).expect("physical point"),
                LogicalPoint::new(20.0, 10.0).expect("logical point"),
            )),
        }
    }

    fn handoff_location(
        local_binding: ViewportBinding,
        hovered_binding: ViewportBinding,
    ) -> PointerEdgeLocation {
        let PointerEdgeLocation::NativeLocal {
            route: Authority::Known(local),
        } = local_location(local_binding)
        else {
            unreachable!("the fixture always supplies exact client coordinates");
        };
        PointerEdgeLocation::NativeLocalToDesktop {
            local,
            desktop: DesktopRouteFact::dock_from_desktop_position(
                hovered_binding,
                Authority::Known(PhysicalPoint::new(440.0, -280.0).expect("desktop point")),
            ),
        }
    }

    fn edge(binding: ViewportBinding, kind: PointerEdgeKind) -> PointerEdge {
        PointerEdge::new_with_delivery(
            PointerEdgeSequence::new(1),
            PointerId::new(1),
            kind,
            local_location(binding),
            Authority::Known(PointerEventDeliveryOwner::Native(binding)),
            Authority::Known(PointerCaptureOwner::None),
        )
    }

    fn journal(edge: PointerEdge) -> PointerEdgeJournal {
        PointerEdgeJournal::new(
            PointerEdgeSequence::new(0),
            PointerEdgeSequence::new(1),
            vec![edge],
        )
        .expect("complete journal")
    }

    fn commit_next_edge(
        ledger: &mut PointerJournalLedger,
        lease: PointerInputLease,
        mut edge: PointerEdge,
    ) -> PointerStreamId {
        let previous = ledger.retained_committed_through(lease).expect("watermark");
        edge.sequence = previous.checked_next().expect("test sequence");
        let journal = PointerEdgeJournal::new(previous, edge.sequence(), vec![edge])
            .expect("one complete next edge");
        let prepared = ledger
            .prepare_candidate(lease, journal)
            .expect("prepare edge");
        ledger
            .commit_prepared(prepared)
            .expect("commit edge")
            .accepted_edges()[0]
            .stream()
    }

    fn scroll(endpoint: ScrollDeliveryEndpoint) -> PointerEdgeKind {
        PointerEdgeKind::Scrolled(
            ScrollEdge::new(
                ScrollDeviceId::new(1),
                None,
                ScrollPhase::Discrete,
                Some(ScrollDelta::LogicalPoints(
                    FiniteScrollVector::new(0.0, 1.0).expect("finite delta"),
                )),
                Authority::Unknown(AuthorityUnavailableReason::NotReported),
                Authority::Known(ScrollModifiers::default()),
                Authority::Known(endpoint),
            )
            .expect("scroll sample"),
        )
    }

    #[test]
    fn native_joined_keeps_native_scope_and_event_time_coordinates() {
        let (_, _, lease) = setup();
        let host = lease.scope().local_host().expect("joined host");
        assert_eq!(lease.scope(), PointerProviderScope::NativeJoined(host));
        assert!(!lease.scope().is_local());
        assert_eq!(lease.scope().host_local(), None);
        assert_eq!(lease.scope().surface_local(), None);
        let binding = binding(lease.authority_domain(), 1);
        let location = local_location(binding);
        assert_eq!(location.desktop_route(), None);
        assert_eq!(location.surface_local_position(), None);
        let PointerEdgeLocation::NativeLocal {
            route: Authority::Known(route),
        } = location
        else {
            panic!("native client route");
        };
        assert_eq!(route.binding(), binding);
        assert_eq!(route.coordinate_generation(), CoordinateGeneration::new(7));
        assert_eq!(
            route.client_position(),
            PhysicalPoint::new(40.0, 20.0).unwrap()
        );
        assert_eq!(
            route.surface_position(),
            LogicalPoint::new(20.0, 10.0).unwrap()
        );
        let implicit = PointerEdge::new(
            PointerEdgeSequence::new(1),
            PointerId::new(1),
            PointerEdgeKind::Moved,
            location,
            Authority::Known(PointerCaptureOwner::None),
        );
        assert_eq!(
            implicit.delivery_owner(),
            Authority::Unknown(AuthorityUnavailableReason::NotReported)
        );
    }

    #[test]
    fn native_joined_foreign_host_enrollment_is_atomic() {
        let domain = EngineAuthorityDomainId::new_for_test(1);
        let mut ledger = PointerJournalLedger::new(domain);
        let mut foreign = PresentationLedger::new(EngineAuthorityDomainId::new_for_test(2));
        let host = foreign.create_host().expect("foreign host");
        let before = ledger.clone();
        assert!(matches!(
            ledger.create_provider(
                PointerProviderScope::NativeJoined(host),
                PointerEdgeSequence::new(0)
            ),
            Err(PointerJournalLedgerError::ForeignNativeJoinedHost { .. })
        ));
        assert_eq!(ledger, before);
    }

    #[test]
    fn native_joined_mixed_lanes_and_unlocated_release_keep_the_stream() {
        let (mut ledger, _, lease) = setup();
        let binding = binding(lease.authority_domain(), 1);
        let press = edge(
            binding,
            PointerEdgeKind::ButtonPressed(PointerButton::Primary),
        );
        let mut moved = edge(binding, PointerEdgeKind::Moved);
        moved.sequence = PointerEdgeSequence::new(2);
        moved.location = PointerEdgeLocation::Desktop {
            route: DesktopRouteFact::unknown(
                Authority::Unknown(AuthorityUnavailableReason::NotReported),
                AuthorityUnavailableReason::NotReported,
            ),
        };
        let mut release = edge(
            binding,
            PointerEdgeKind::ButtonReleased(PointerButton::Primary),
        );
        release.sequence = PointerEdgeSequence::new(3);
        release.location = PointerEdgeLocation::NativeLocal {
            route: Authority::Unknown(AuthorityUnavailableReason::NotReported),
        };
        let journal = PointerEdgeJournal::new(
            PointerEdgeSequence::new(0),
            PointerEdgeSequence::new(3),
            vec![press, moved, release],
        )
        .expect("complete mixed-lane journal");
        let prepared = ledger
            .prepare_candidate(lease, journal)
            .expect("native lanes");
        let commit = ledger
            .commit_prepared(prepared)
            .expect("commit native lanes");
        let accepted = commit.accepted_edges();
        assert_eq!(accepted.len(), 3);
        assert!(
            accepted
                .iter()
                .all(|edge| edge.stream() == accepted[0].stream())
        );
        assert_eq!(
            ledger.retained_committed_through(lease).expect("watermark"),
            PointerEdgeSequence::new(3)
        );
    }

    #[test]
    fn native_handoff_facts_keep_identity_without_authorizing_desktop() {
        let (mut ledger, _, lease) = setup();
        let native = binding(lease.authority_domain(), 1);
        let mut moved = edge(native, PointerEdgeKind::Moved).with_metadata(
            PointerEventMetadata::new(PointerDeviceKind::Mouse, Duration::from_millis(12)),
        );
        moved.location = handoff_location(native, binding(lease.authority_domain(), 2));
        assert_eq!(moved.desktop_route(), None);
        assert_eq!(moved.location().surface_local_position(), None);
        let PointerEdgeLocation::NativeLocalToDesktop { desktop, .. } = moved.location() else {
            panic!("explicit handoff facts");
        };
        let desktop_view = moved.with_location(PointerEdgeLocation::Desktop { route: desktop });
        let mut expected = moved.clone();
        expected.location = PointerEdgeLocation::Desktop { route: desktop };
        assert_eq!(desktop_view, expected);
        assert_eq!(moved.desktop_route(), None);
        assert_eq!(desktop_view.desktop_route(), Some(desktop));

        let implicit = PointerEdge::new(
            moved.sequence(),
            moved.pointer(),
            moved.kind(),
            moved.location(),
            moved.capture_owner(),
        );
        assert_eq!(
            implicit.delivery_owner(),
            Authority::Unknown(AuthorityUnavailableReason::NotReported)
        );
        let stream = commit_next_edge(&mut ledger, lease, moved);
        assert!(!ledger.native_primary_press_continuity(stream, native));
        let press = edge(
            native,
            PointerEdgeKind::ButtonPressed(PointerButton::Primary),
        );
        assert_eq!(commit_next_edge(&mut ledger, lease, press), stream);
        let mut release = edge(
            native,
            PointerEdgeKind::ButtonReleased(PointerButton::Primary),
        );
        release.location = PointerEdgeLocation::NativeLocal {
            route: Authority::Unknown(AuthorityUnavailableReason::NotReported),
        };
        assert_eq!(commit_next_edge(&mut ledger, lease, release), stream);
        assert_eq!(
            ledger.retained_committed_through(lease).expect("watermark"),
            PointerEdgeSequence::new(3)
        );
    }

    #[test]
    fn native_handoff_lane_is_exclusive_to_joined_providers() {
        let (_, mut presentations, lease) = setup();
        let host = presentations.create_host().expect("another host");
        let native = binding(lease.authority_domain(), 1);
        for scope in [
            PointerProviderScope::DesktopGlobal,
            PointerProviderScope::HostLocal(host),
            PointerProviderScope::SurfaceLocal(SurfaceLocalPointerScope::new(
                host,
                SurfaceLocalPointerEndpoint::Logical(native.surface()),
            )),
            PointerProviderScope::SurfaceLocal(SurfaceLocalPointerScope::new_native(
                host,
                native,
                CoordinateGeneration::new(7),
            )),
        ] {
            let mut ledger = PointerJournalLedger::new(lease.authority_domain());
            let provider = ledger
                .create_provider(scope, PointerEdgeSequence::new(0))
                .unwrap();
            let mut moved = edge(native, PointerEdgeKind::Moved);
            moved.location = handoff_location(native, native);
            let before = ledger.clone();
            assert!(matches!(
                ledger.prepare_candidate(provider, journal(moved)),
                Err(PointerJournalLedgerError::LocationScopeMismatch {
                    submitted: PointerEdgeLocationLane::NativeLocalToDesktop,
                    ..
                })
            ));
            assert_eq!(ledger, before);
        }
    }

    #[test]
    fn native_handoff_facts_retain_native_scope_and_checkpoint_guards() {
        let (ledger, _, lease) = setup();
        let native = binding(lease.authority_domain(), 1);
        let foreign = binding(EngineAuthorityDomainId::new_for_test(2), 1);
        let mut moved = edge(native, PointerEdgeKind::Moved);
        moved.location = handoff_location(foreign, native);
        let before = ledger.clone();
        assert!(matches!(
            ledger.prepare_candidate(lease, journal(moved)),
            Err(PointerJournalLedgerError::ForeignNativeLocalRouteBinding { .. })
        ));
        let mut moved = edge(native, PointerEdgeKind::Moved);
        moved.location = handoff_location(native, foreign);
        assert_eq!(moved.desktop_route(), None);
        assert!(matches!(
            ledger.prepare_candidate(lease, journal(moved)),
            Err(PointerJournalLedgerError::ForeignDesktopRouteBinding { .. })
        ));
        for owner in [
            PointerCaptureOwner::ProviderEndpoint,
            PointerCaptureOwner::Native(foreign),
        ] {
            let mut moved = edge(native, PointerEdgeKind::Moved);
            moved.location = handoff_location(native, native);
            moved.capture_owner = Authority::Known(owner);
            assert!(matches!(
                ledger.prepare_candidate(lease, journal(moved)),
                Err(PointerJournalLedgerError::CaptureScopeMismatch { .. }
                    | PointerJournalLedgerError::ForeignCaptureBinding { .. })
            ));
            let mut moved = edge(native, PointerEdgeKind::Moved);
            moved.location = handoff_location(native, native);
            let checkpoint = PointerAuthorityCheckpoint::known(
                PointerEdgeSequence::new(0),
                vec![
                    PointerStateObservation::new(
                        PointerId::new(1),
                        Vec::new(),
                        Authority::Known(owner),
                    )
                    .unwrap(),
                ],
            )
            .unwrap();
            let journal = journal(moved)
                .with_authority_checkpoint(checkpoint)
                .unwrap();
            assert!(matches!(
                ledger.prepare_candidate(lease, journal),
                Err(PointerJournalLedgerError::CaptureScopeMismatch { .. }
                    | PointerJournalLedgerError::ForeignCaptureBinding { .. })
            ));
        }
        for owner in [
            PointerEventDeliveryOwner::ProviderEndpoint,
            PointerEventDeliveryOwner::Native(foreign),
        ] {
            let mut moved = edge(native, PointerEdgeKind::Moved);
            moved.location = handoff_location(native, native);
            moved.delivery_owner = Authority::Known(owner);
            assert!(matches!(
                ledger.prepare_candidate(lease, journal(moved)),
                Err(PointerJournalLedgerError::DeliveryScopeMismatch { .. }
                    | PointerJournalLedgerError::ForeignDeliveryBinding { .. })
            ));
        }
        assert_eq!(ledger, before);
    }

    #[test]
    fn native_primary_press_continuity_is_exact_and_read_only() {
        let (mut ledger, _, lease) = setup();
        let native = binding(lease.authority_domain(), 1);
        let mut press = edge(
            native,
            PointerEdgeKind::ButtonPressed(PointerButton::Primary),
        );
        press.capture_owner = Authority::Known(PointerCaptureOwner::Native(native));
        let stream = commit_next_edge(&mut ledger, lease, press);
        let mut other_pointer = edge(native, PointerEdgeKind::Moved);
        other_pointer.pointer = PointerId::new(2);
        other_pointer.capture_owner = Authority::Known(PointerCaptureOwner::Native(native));
        let other_stream = commit_next_edge(&mut ledger, lease, other_pointer);
        let mut secondary = edge(
            native,
            PointerEdgeKind::ButtonPressed(PointerButton::Secondary),
        );
        secondary.pointer = PointerId::new(3);
        secondary.capture_owner = Authority::Known(PointerCaptureOwner::Native(native));
        let secondary_stream = commit_next_edge(&mut ledger, lease, secondary);
        let before = ledger.clone();
        assert!(ledger.native_primary_press_continuity(stream, native));
        assert_eq!(ledger.button_authority(), AnyButtonDownAuthority::KnownDown);
        assert!(!ledger.native_primary_press_continuity(other_stream, native));
        assert!(!ledger.native_primary_press_continuity(secondary_stream, native));
        assert!(
            !ledger.native_primary_press_continuity(stream, binding(lease.authority_domain(), 2))
        );
        assert!(!ledger.native_primary_press_continuity(
            PointerStreamId::new(lease, stream.pointer(), stream.incarnation() + 1),
            native,
        ));
        assert!(!ledger.native_primary_press_continuity(
            PointerStreamId::new(lease, PointerId::new(4), stream.incarnation()),
            native,
        ));
        let foreign_lease = PointerInputLease::new(
            EngineAuthorityDomainId::new_for_test(2),
            lease.incarnation(),
            lease.scope(),
        );
        assert!(!ledger.native_primary_press_continuity(
            PointerStreamId::new(foreign_lease, stream.pointer(), stream.incarnation()),
            native,
        ));
        assert_eq!(ledger, before);
    }

    #[test]
    fn native_primary_press_continuity_requires_current_observed_capture() {
        for capture in [
            Authority::Unknown(AuthorityUnavailableReason::NotReported),
            Authority::Known(PointerCaptureOwner::None),
            Authority::Known(PointerCaptureOwner::Foreign),
        ] {
            let (mut ledger, _, lease) = setup();
            let native = binding(lease.authority_domain(), 1);
            let mut press = edge(
                native,
                PointerEdgeKind::ButtonPressed(PointerButton::Primary),
            );
            press.capture_owner = Authority::Known(PointerCaptureOwner::Native(native));
            let stream = commit_next_edge(&mut ledger, lease, press);
            assert!(ledger.native_primary_press_continuity(stream, native));
            let mut changed = edge(native, PointerEdgeKind::CaptureChanged);
            changed.capture_owner = capture;
            assert_eq!(commit_next_edge(&mut ledger, lease, changed), stream);
            if matches!(capture, Authority::Unknown(_)) {
                assert_eq!(
                    ledger.authority.captures[&stream.pointer()].retained_native_binding,
                    Some(native)
                );
            }
            let before = ledger.clone();
            assert!(!ledger.native_primary_press_continuity(stream, native));
            assert_eq!(ledger, before);
            let mut changed = edge(native, PointerEdgeKind::CaptureChanged);
            changed.capture_owner = Authority::Known(PointerCaptureOwner::Native(binding(
                lease.authority_domain(),
                2,
            )));
            commit_next_edge(&mut ledger, lease, changed);
            assert!(!ledger.native_primary_press_continuity(stream, native));
            let mut changed = edge(native, PointerEdgeKind::CaptureChanged);
            changed.capture_owner = Authority::Known(PointerCaptureOwner::Native(native));
            commit_next_edge(&mut ledger, lease, changed);
            assert!(ledger.native_primary_press_continuity(stream, native));
            let mut release = edge(
                native,
                PointerEdgeKind::ButtonReleased(PointerButton::Primary),
            );
            release.capture_owner = Authority::Known(PointerCaptureOwner::Native(native));
            commit_next_edge(&mut ledger, lease, release);
            assert!(!ledger.native_primary_press_continuity(stream, native));
        }
    }

    #[test]
    fn native_primary_press_continuity_requires_primary_native_press_origin() {
        let (ledger, _, lease) = setup();
        let native = binding(lease.authority_domain(), 1);
        for delivery in [
            Authority::Unknown(AuthorityUnavailableReason::NotReported),
            Authority::Known(PointerEventDeliveryOwner::None),
            Authority::Known(PointerEventDeliveryOwner::Foreign),
            Authority::Known(PointerEventDeliveryOwner::Native(binding(
                lease.authority_domain(),
                2,
            ))),
        ] {
            let mut ledger = ledger.clone();
            let mut press = edge(
                native,
                PointerEdgeKind::ButtonPressed(PointerButton::Primary),
            );
            press.delivery_owner = delivery;
            press.capture_owner = Authority::Known(PointerCaptureOwner::Native(native));
            let stream = commit_next_edge(&mut ledger, lease, press);
            assert!(!ledger.native_primary_press_continuity(stream, native));
        }

        let mut ledger = ledger;
        let checkpoint = PointerAuthorityCheckpoint::known(
            PointerEdgeSequence::new(0),
            vec![
                PointerStateObservation::new(
                    PointerId::new(1),
                    vec![PointerButton::Primary],
                    Authority::Known(PointerCaptureOwner::Native(native)),
                )
                .unwrap(),
            ],
        )
        .unwrap();
        let mut moved = edge(native, PointerEdgeKind::Moved);
        moved.location = handoff_location(native, native);
        moved.capture_owner = Authority::Known(PointerCaptureOwner::Native(native));
        let journal = journal(moved)
            .with_authority_checkpoint(checkpoint)
            .unwrap();
        let prepared = ledger.prepare_candidate(lease, journal).unwrap();
        let stream = ledger.commit_prepared(prepared).unwrap().accepted_edges()[0].stream();
        assert_eq!(ledger.button_authority(), AnyButtonDownAuthority::KnownDown);
        assert!(!ledger.native_primary_press_continuity(stream, native));
    }

    #[test]
    fn native_primary_press_continuity_does_not_survive_stream_or_provider_retirement() {
        let (mut ledger, _, lease) = setup();
        let native = binding(lease.authority_domain(), 1);
        let mut press = edge(
            native,
            PointerEdgeKind::ButtonPressed(PointerButton::Primary),
        );
        press.capture_owner = Authority::Known(PointerCaptureOwner::Native(native));
        let stream = commit_next_edge(&mut ledger, lease, press.clone());
        assert!(ledger.native_primary_press_continuity(stream, native));
        let cancelled = edge(
            native,
            PointerEdgeKind::StreamCancelled(
                PointerStreamCancelReason::ExplicitPlatformCancellation,
            ),
        );
        assert_eq!(commit_next_edge(&mut ledger, lease, cancelled), stream);
        assert!(!ledger.native_primary_press_continuity(stream, native));
        let successor = commit_next_edge(&mut ledger, lease, press.clone());
        assert_ne!(successor, stream);
        assert!(ledger.native_primary_press_continuity(successor, native));
        assert!(!ledger.native_primary_press_continuity(stream, native));
        ledger.retire_provider(lease).unwrap();
        assert!(!ledger.native_primary_press_continuity(successor, native));
        let replacement = ledger
            .create_provider(lease.scope(), PointerEdgeSequence::new(0))
            .unwrap();
        let current = commit_next_edge(&mut ledger, replacement, press);
        assert!(ledger.native_primary_press_continuity(current, native));
        assert!(!ledger.native_primary_press_continuity(successor, native));
    }

    #[test]
    fn native_local_lane_cannot_bypass_other_provider_scopes() {
        let (_, mut presentations, lease) = setup();
        let host = presentations.create_host().expect("another host");
        let binding = binding(lease.authority_domain(), 1);
        for scope in [
            PointerProviderScope::DesktopGlobal,
            PointerProviderScope::HostLocal(host),
            PointerProviderScope::SurfaceLocal(SurfaceLocalPointerScope::new(
                host,
                SurfaceLocalPointerEndpoint::Logical(binding.surface()),
            )),
        ] {
            let mut ledger = PointerJournalLedger::new(lease.authority_domain());
            let provider = ledger
                .create_provider(scope, PointerEdgeSequence::new(0))
                .expect("provider");
            let before = ledger.clone();
            assert!(matches!(
                ledger.prepare_candidate(provider, journal(edge(binding, PointerEdgeKind::Moved))),
                Err(PointerJournalLedgerError::LocationScopeMismatch {
                    submitted: PointerEdgeLocationLane::NativeLocal,
                    ..
                })
            ));
            assert_eq!(ledger, before);
        }
        let (ledger, _, lease) = setup();
        for location in [
            PointerEdgeLocation::HostLocal {
                manipulation: Authority::Unknown(AuthorityUnavailableReason::NotReported),
                hover: Authority::Unknown(AuthorityUnavailableReason::NotReported),
            },
            PointerEdgeLocation::SurfaceLocal {
                position: Authority::Unknown(AuthorityUnavailableReason::NotReported),
            },
        ] {
            let mut edge = edge(binding, PointerEdgeKind::Moved);
            edge.location = location;
            assert!(matches!(
                ledger.prepare_candidate(lease, journal(edge)),
                Err(PointerJournalLedgerError::LocationScopeMismatch { .. })
            ));
        }
    }

    #[test]
    fn native_joined_rejects_provider_endpoints_and_foreign_native_bindings() {
        let (ledger, _, lease) = setup();
        let native = binding(lease.authority_domain(), 1);
        let foreign = binding(EngineAuthorityDomainId::new_for_test(2), 1);
        let before = ledger.clone();
        let mut sample = edge(native, PointerEdgeKind::Moved);
        sample.delivery_owner = Authority::Known(PointerEventDeliveryOwner::ProviderEndpoint);
        assert!(matches!(
            ledger.prepare_candidate(lease, journal(sample)),
            Err(PointerJournalLedgerError::DeliveryScopeMismatch { .. })
        ));
        let mut sample = edge(native, PointerEdgeKind::Moved);
        sample.capture_owner = Authority::Known(PointerCaptureOwner::ProviderEndpoint);
        assert!(matches!(
            ledger.prepare_candidate(lease, journal(sample)),
            Err(PointerJournalLedgerError::CaptureScopeMismatch { .. })
        ));
        let mut sample = edge(native, PointerEdgeKind::Moved);
        sample.delivery_owner = Authority::Known(PointerEventDeliveryOwner::Native(foreign));
        assert!(matches!(
            ledger.prepare_candidate(lease, journal(sample)),
            Err(PointerJournalLedgerError::ForeignDeliveryBinding { .. })
        ));
        let mut sample = edge(native, PointerEdgeKind::Moved);
        sample.capture_owner = Authority::Known(PointerCaptureOwner::Native(foreign));
        assert!(matches!(
            ledger.prepare_candidate(lease, journal(sample)),
            Err(PointerJournalLedgerError::ForeignCaptureBinding { .. })
        ));
        let mut sample = edge(native, PointerEdgeKind::Moved);
        sample.location = local_location(foreign);
        assert!(matches!(
            ledger.prepare_candidate(lease, journal(sample)),
            Err(PointerJournalLedgerError::ForeignNativeLocalRouteBinding { .. })
        ));
        for capture in [
            PointerCaptureOwner::ProviderEndpoint,
            PointerCaptureOwner::Native(foreign),
        ] {
            let checkpoint = PointerAuthorityCheckpoint::known(
                PointerEdgeSequence::new(0),
                vec![
                    PointerStateObservation::new(
                        PointerId::new(1),
                        Vec::new(),
                        Authority::Known(capture),
                    )
                    .expect("pointer state"),
                ],
            )
            .expect("checkpoint");
            let journal = journal(edge(native, PointerEdgeKind::Moved))
                .with_authority_checkpoint(checkpoint)
                .expect("checkpoint boundary");
            assert!(matches!(
                ledger.prepare_candidate(lease, journal),
                Err(PointerJournalLedgerError::CaptureScopeMismatch { .. }
                    | PointerJournalLedgerError::ForeignCaptureBinding { .. })
            ));
        }
        assert_eq!(ledger, before);
    }

    #[test]
    fn native_joined_scroll_requires_the_exact_host_and_native_delivery() {
        let (ledger, mut presentations, lease) = setup();
        let host = lease.scope().local_host().expect("joined host");
        let other_host = presentations.create_host().expect("another host");
        let native = binding(lease.authority_domain(), 1);
        let other_native = binding(lease.authority_domain(), 2);
        let endpoint = ScrollDeliveryEndpoint::new(
            host,
            native.surface(),
            Some(native),
            CoordinateGeneration::new(7),
        )
        .expect("native endpoint");
        assert!(
            ledger
                .prepare_candidate(lease, journal(edge(native, scroll(endpoint))))
                .is_ok()
        );
        let endpoint = ScrollDeliveryEndpoint::new(
            other_host,
            native.surface(),
            Some(native),
            CoordinateGeneration::new(7),
        )
        .expect("other host endpoint");
        assert!(matches!(
            ledger.prepare_candidate(lease, journal(edge(native, scroll(endpoint)))),
            Err(PointerJournalLedgerError::ScrollDeliveryScopeMismatch { .. })
        ));
        let endpoint =
            ScrollDeliveryEndpoint::new(host, native.surface(), None, CoordinateGeneration::new(7))
                .expect("headless endpoint");
        assert!(matches!(
            ledger.prepare_candidate(lease, journal(edge(native, scroll(endpoint)))),
            Err(PointerJournalLedgerError::NativeScrollDeliveryBindingMissing { .. })
        ));
        let endpoint = ScrollDeliveryEndpoint::new(
            host,
            other_native.surface(),
            Some(other_native),
            CoordinateGeneration::new(7),
        )
        .expect("other native endpoint");
        assert!(matches!(
            ledger.prepare_candidate(lease, journal(edge(native, scroll(endpoint)))),
            Err(PointerJournalLedgerError::ScrollDeliveryOwnerMismatch { .. })
        ));
        let mut foreign = PresentationLedger::new(EngineAuthorityDomainId::new_for_test(2));
        let foreign_host = foreign.create_host().expect("foreign host");
        let foreign_binding = binding(foreign_host.authority_domain(), 1);
        let endpoint = ScrollDeliveryEndpoint::new(
            foreign_host,
            foreign_binding.surface(),
            Some(foreign_binding),
            CoordinateGeneration::new(7),
        )
        .expect("foreign endpoint");
        assert!(matches!(
            ledger.prepare_candidate(lease, journal(edge(native, scroll(endpoint)))),
            Err(PointerJournalLedgerError::ForeignScrollDeliveryHost { .. })
        ));
    }

    #[test]
    fn native_local_button_and_capture_keep_their_real_binding_until_retirement() {
        for capture_only in [false, true] {
            let (mut ledger, _, lease) = setup();
            let native = binding(lease.authority_domain(), 1);
            let mut initial = if capture_only {
                let mut sample = edge(native, PointerEdgeKind::Moved);
                sample.capture_owner = Authority::Known(PointerCaptureOwner::Native(native));
                sample
            } else {
                edge(
                    native,
                    PointerEdgeKind::ButtonPressed(PointerButton::Primary),
                )
            };
            // Capture must remain an independent retirement dependency even
            // when the event was delivered through another native endpoint.
            if capture_only {
                initial.delivery_owner = Authority::Known(PointerEventDeliveryOwner::Native(
                    binding(lease.authority_domain(), 2),
                ));
                initial.location = local_location(binding(lease.authority_domain(), 2));
            }
            let prepared = ledger
                .prepare_candidate(lease, journal(initial))
                .expect("native input");
            let commit = ledger
                .commit_prepared(prepared)
                .expect("commit native input");
            let stream = commit.accepted_edges()[0].stream();
            let mut moved = edge(binding(lease.authority_domain(), 2), PointerEdgeKind::Moved);
            moved.sequence = PointerEdgeSequence::new(2);
            moved.capture_owner = Authority::Unknown(AuthorityUnavailableReason::NotReported);
            moved.location = PointerEdgeLocation::NativeLocal {
                route: Authority::Unknown(AuthorityUnavailableReason::NotReported),
            };
            let journal = PointerEdgeJournal::new(
                PointerEdgeSequence::new(1),
                PointerEdgeSequence::new(2),
                vec![moved],
            )
            .expect("continuation journal");
            let prepared = ledger
                .prepare_candidate(lease, journal)
                .expect("continuation");
            ledger
                .commit_prepared(prepared)
                .expect("commit continuation");
            assert_eq!(
                ledger
                    .retire_native_binding_input(lease, native)
                    .expect("binding retirement"),
                vec![stream]
            );
            assert_eq!(ledger.active_lease(), Some(lease));
            assert!(ledger.active_streams.is_empty());
            assert_eq!(
                ledger.retained_committed_through(lease).expect("watermark"),
                PointerEdgeSequence::new(2)
            );
        }
    }
}

#[cfg(test)]
mod host_local_tests {
    use super::*;
    use crate::ids::WorkspaceEpoch;
    use crate::presentation_observation::PresentationLedger;
    use crate::viewport::{WindowIncarnation, WindowToken};

    fn setup() -> (PointerJournalLedger, PresentationLedger, PointerInputLease) {
        let domain = EngineAuthorityDomainId::new_for_test(1);
        let mut presentation = PresentationLedger::new(domain);
        let host = presentation.create_host().expect("host lease");
        let mut ledger = PointerJournalLedger::new(domain);
        let lease = ledger
            .create_provider(
                PointerProviderScope::HostLocal(host),
                PointerEdgeSequence::new(0),
            )
            .expect("host-local provider");
        (ledger, presentation, lease)
    }

    fn local_edge(kind: PointerEdgeKind) -> PointerEdge {
        PointerEdge::new(
            PointerEdgeSequence::new(1),
            PointerId::new(1),
            kind,
            PointerEdgeLocation::HostLocal {
                manipulation: Authority::Unknown(AuthorityUnavailableReason::NotReported),
                hover: Authority::Unknown(AuthorityUnavailableReason::NotReported),
            },
            Authority::Known(PointerCaptureOwner::ProviderEndpoint),
        )
    }

    fn journal(edge: PointerEdge) -> PointerEdgeJournal {
        PointerEdgeJournal::new(
            PointerEdgeSequence::new(0),
            PointerEdgeSequence::new(1),
            vec![edge],
        )
        .expect("complete journal")
    }

    fn native_binding(lease: PointerInputLease) -> ViewportBinding {
        ViewportBinding::new(
            lease.authority_domain(),
            WorkspaceEpoch::new(1),
            SurfaceId::new(1),
            WindowToken::new(1),
            WindowIncarnation::new(1),
        )
    }

    #[test]
    fn foreign_host_enrollment_preserves_the_ledger() {
        let (mut ledger, _, _) = setup();
        let mut foreign = PresentationLedger::new(EngineAuthorityDomainId::new_for_test(2));
        let host = foreign.create_host().expect("foreign host");
        let mut empty = PointerJournalLedger::new(ledger.authority_domain);
        let before = empty.clone();
        assert!(matches!(
            empty.create_provider(
                PointerProviderScope::HostLocal(host),
                PointerEdgeSequence::new(0)
            ),
            Err(PointerJournalLedgerError::ForeignHostLocalHost { .. })
        ));
        assert_eq!(empty, before);
        // An active provider remains the first enrollment barrier.
        assert!(matches!(
            ledger.create_provider(
                PointerProviderScope::HostLocal(host),
                PointerEdgeSequence::new(0)
            ),
            Err(PointerJournalLedgerError::ProviderAlreadyActive { .. })
        ));
    }

    #[test]
    fn host_local_journals_commit_without_desktop_or_surface_local_facts() {
        let (mut ledger, _, lease) = setup();
        let edge = local_edge(PointerEdgeKind::Moved);
        assert_eq!(edge.desktop_route(), None);
        assert_eq!(edge.location().surface_local_position(), None);
        assert_eq!(
            edge.delivery_owner(),
            Authority::Known(PointerEventDeliveryOwner::ProviderEndpoint)
        );
        let journal = journal(edge);
        let prepared = ledger
            .prepare_candidate(lease, journal.clone())
            .expect("local journal");
        ledger.commit_prepared(prepared).expect("local commit");
        assert!(matches!(
            ledger.prepare_candidate(lease, journal),
            Err(PointerJournalLedgerError::JournalReplay { .. })
        ));
    }

    #[test]
    fn host_local_rejects_surface_lane_and_native_capture_or_delivery() {
        let (ledger, _, lease) = setup();
        let before = ledger.clone();
        let mut edge = local_edge(PointerEdgeKind::Moved);
        edge.location = PointerEdgeLocation::SurfaceLocal {
            position: Authority::Unknown(AuthorityUnavailableReason::NotReported),
        };
        assert!(matches!(
            ledger.prepare_candidate(lease, journal(edge)),
            Err(PointerJournalLedgerError::LocationScopeMismatch { .. })
        ));
        let mut edge = local_edge(PointerEdgeKind::Moved);
        edge.capture_owner = Authority::Known(PointerCaptureOwner::Native(native_binding(lease)));
        assert!(matches!(
            ledger.prepare_candidate(lease, journal(edge)),
            Err(PointerJournalLedgerError::CaptureScopeMismatch { .. })
        ));
        let mut edge = local_edge(PointerEdgeKind::Moved);
        edge.delivery_owner =
            Authority::Known(PointerEventDeliveryOwner::Native(native_binding(lease)));
        assert!(matches!(
            ledger.prepare_candidate(lease, journal(edge)),
            Err(PointerJournalLedgerError::DeliveryScopeMismatch { .. })
        ));
        assert_eq!(ledger, before);
    }

    #[test]
    fn host_local_checkpoint_rejects_native_capture() {
        let (ledger, _, lease) = setup();
        let checkpoint = PointerAuthorityCheckpoint::known(
            PointerEdgeSequence::new(0),
            vec![
                PointerStateObservation::new(
                    PointerId::new(1),
                    Vec::new(),
                    Authority::Known(PointerCaptureOwner::Native(native_binding(lease))),
                )
                .expect("pointer state"),
            ],
        )
        .expect("checkpoint");
        let journal = journal(local_edge(PointerEdgeKind::Moved))
            .with_authority_checkpoint(checkpoint)
            .expect("checkpoint boundary");
        assert!(matches!(
            ledger.prepare_candidate(lease, journal),
            Err(PointerJournalLedgerError::CaptureScopeMismatch { .. })
        ));
    }

    #[test]
    fn host_local_scroll_accepts_only_same_host_headless_endpoints() {
        let (ledger, mut presentation, lease) = setup();
        let host = lease.scope().host_local().expect("host-local lease");
        let other_host = presentation.create_host().expect("other host");
        for (delivery_host, surface, binding, accepted) in [
            (host, SurfaceId::new(1), None, true),
            (host, SurfaceId::new(2), None, true),
            (other_host, SurfaceId::new(1), None, false),
            (host, SurfaceId::new(1), Some(native_binding(lease)), false),
        ] {
            let delivery = ScrollDeliveryEndpoint::new(
                delivery_host,
                surface,
                binding,
                CoordinateGeneration::new(1),
            )
            .expect("delivery endpoint");
            let scroll = ScrollEdge::new(
                ScrollDeviceId::new(1),
                None,
                ScrollPhase::Discrete,
                Some(ScrollDelta::LogicalPoints(
                    FiniteScrollVector::new(0.0, 1.0).expect("finite delta"),
                )),
                Authority::Unknown(AuthorityUnavailableReason::NotReported),
                Authority::Known(ScrollModifiers::default()),
                Authority::Known(delivery),
            )
            .expect("scroll sample");
            let result = ledger.prepare_candidate(
                lease,
                journal(local_edge(PointerEdgeKind::Scrolled(scroll))),
            );
            if accepted {
                assert!(result.is_ok());
            } else {
                assert!(matches!(
                    result,
                    Err(PointerJournalLedgerError::ScrollDeliveryScopeMismatch { .. })
                ));
            }
        }
    }

    #[test]
    fn host_local_producer_drain_compacts_and_abandonment_retires() {
        let (mut ledger, _, lease) = setup();
        let provider = SurfaceLocalPointerProvider::new(lease, PointerEdgeSequence::new(0));
        ledger.bind_surface_local_producer(lease, provider.monitor());
        let receipt = provider.drain().expect("producer drains");
        assert_eq!(ledger.abandoned_surface_local_provider(), None);
        assert_eq!(
            ledger
                .retire_quiesced_surface_local(&receipt)
                .expect("host-local retirement"),
            SurfaceLocalPointerQuiescenceDisposition::RetiredActive,
        );
        assert!(matches!(
            ledger.prepare_candidate(lease, journal(local_edge(PointerEdgeKind::Moved))),
            Err(PointerJournalLedgerError::CompactedLease { .. })
        ));
        let lease = ledger
            .create_provider(lease.scope(), PointerEdgeSequence::new(0))
            .expect("successor");
        let provider = SurfaceLocalPointerProvider::new(lease, PointerEdgeSequence::new(0));
        ledger.bind_surface_local_producer(lease, provider.monitor());
        drop(provider);
        assert_eq!(ledger.abandoned_surface_local_provider(), Some(lease));
        ledger
            .retire_abandoned_active_surface_local(lease)
            .expect("abandoned host-local retirement");
        assert_eq!(ledger.active_lease(), None);
    }
}
