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
            pressed_buttons: BTreeSet::new(),
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
                    for button in pointer.pressed_buttons() {
                        let _ = self.pressed_buttons.insert((pointer.pointer(), *button));
                    }
                    let _ = self
                        .captures
                        .insert(pointer.pointer(), pointer.capture_owner());
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
        let _ = self.captures.insert(pointer, edge.capture_owner());
        match edge.kind() {
            PointerEdgeKind::ButtonPressed(button) => {
                if self.pressed_buttons.contains(&(pointer, button)) {
                    return Err(PointerJournalLedgerError::ButtonAlreadyPressed {
                        pointer,
                        button,
                        sequence: edge.sequence(),
                    });
                }
                let _ = self.pressed_buttons.insert((pointer, button));
            }
            PointerEdgeKind::ButtonReleased(button) | PointerEdgeKind::ContactEnded(button) => {
                let was_pressed = self.pressed_buttons.remove(&(pointer, button));
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
                    .retain(|(active, _)| *active != pointer);
                let _ = self.captures.remove(&pointer);
            }
            PointerEdgeKind::Moved
            | PointerEdgeKind::CaptureChanged
            | PointerEdgeKind::StreamEnded
            | PointerEdgeKind::Scrolled(_) => {}
        }
        Ok(())
    }

    fn retire_stream(&mut self, pointer: PointerId) {
        self.pressed_buttons
            .retain(|(active, _)| *active != pointer);
        let _ = self.captures.remove(&pointer);
    }

    fn edge_authority_after(
        &self,
        pointer: PointerId,
    ) -> (AnyButtonDownAuthority, Authority<PointerCaptureOwner>) {
        let capture = self
            .captures
            .get(&pointer)
            .copied()
            .unwrap_or(Authority::Unknown(
                AuthorityUnavailableReason::ProviderUnavailable,
            ));
        (self.button_authority(), capture)
    }

    fn pointer_has_pressed_buttons(&self, pointer: PointerId) -> bool {
        self.pressed_buttons
            .iter()
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

    pub(crate) fn bind_surface_local_producer(
        &mut self,
        lease: PointerInputLease,
        monitor: SurfaceLocalPointerProducerMonitor,
    ) {
        assert!(
            lease.scope().surface_local().is_some(),
            "only a surface-local lease can bind an affine producer monitor",
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
        self.authority_checkpoint_boundary = Some(committed_through);
        self.last_checkpoint = None;
        self.active = Some(ActivePointerProvider {
            lease,
            committed_through,
        });
        self.surface_local_producer = None;
        Ok(lease)
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
        if lease.scope().surface_local().is_none() {
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
        if lease.scope().surface_local().is_none() {
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
            (PointerProviderScope::DesktopGlobal, PointerEventDeliveryOwner::ProviderEndpoint)
            | (PointerProviderScope::SurfaceLocal(_), PointerEventDeliveryOwner::Native(_)) => {
                Err(PointerJournalLedgerError::DeliveryScopeMismatch {
                    lease,
                    sequence: edge.sequence(),
                    submitted: delivery_owner,
                })
            }
            (PointerProviderScope::DesktopGlobal, PointerEventDeliveryOwner::Native(binding))
                if binding.authority_domain() != self.authority_domain =>
            {
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
            (PointerProviderScope::DesktopGlobal, PointerCaptureOwner::ProviderEndpoint)
            | (PointerProviderScope::SurfaceLocal(_), PointerCaptureOwner::Native(_)) => {
                Err(PointerJournalLedgerError::CaptureScopeMismatch {
                    lease,
                    sequence,
                    submitted: capture_owner,
                })
            }
            (PointerProviderScope::DesktopGlobal, PointerCaptureOwner::Native(binding))
                if binding.authority_domain() != self.authority_domain =>
            {
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
        let Some(binding) = edge
            .desktop_route()
            .and_then(|route| match route.hovered() {
                Authority::Known(DesktopHoveredWindow::Dock(binding)) => Some(binding),
                Authority::Known(DesktopHoveredWindow::Foreign | DesktopHoveredWindow::None)
                | Authority::Unknown(_) => None,
            })
        else {
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
            PointerProviderScope::DesktopGlobal if endpoint.binding().is_none() => Err(
                PointerJournalLedgerError::DesktopScrollDeliveryBindingMissing {
                    lease,
                    sequence: edge.sequence(),
                    endpoint,
                },
            ),
            PointerProviderScope::DesktopGlobal
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
            BTreeMap<PointerId, PointerStreamId>,
            JournalPointerAuthority,
            Option<PointerEdgeSequence>,
            Option<PointerAuthorityCheckpoint>,
            Vec<AcceptedPointerEdge>,
        ),
        PointerJournalLedgerError,
    > {
        let mut last_stream_incarnation = self.last_stream_incarnation;
        let mut active_streams = self.active_streams.clone();
        let mut authority = self.authority.clone();
        let mut authority_checkpoint_boundary = self.authority_checkpoint_boundary;
        let mut last_checkpoint = self.last_checkpoint.clone();
        let mut accepted_edges = Vec::with_capacity(journal.len());

        if let Some(checkpoint) = journal.authority_checkpoint() {
            if authority_checkpoint_boundary != Some(checkpoint.observed_through()) {
                return Err(
                    PointerJournalLedgerError::AuthorityCheckpointAfterPointerEdges {
                        lease,
                        observed_through: checkpoint.observed_through(),
                    },
                );
            }
            if last_checkpoint.as_ref().is_some_and(|retained| {
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
            let stream = match active_streams.get(&edge.pointer()).copied() {
                Some(stream) => stream,
                None => {
                    last_stream_incarnation = last_stream_incarnation
                        .checked_next()
                        .ok_or(PointerJournalLedgerError::StreamIncarnationExhausted)?;
                    let stream =
                        PointerStreamId::new(lease, edge.pointer(), last_stream_incarnation.0);
                    let _ = active_streams.insert(edge.pointer(), stream);
                    stream
                }
            };
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
                stream,
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
            authority_checkpoint_boundary = None;
            last_checkpoint = None;
        }

        Ok((
            last_stream_incarnation,
            active_streams,
            authority,
            authority_checkpoint_boundary,
            last_checkpoint,
            accepted_edges,
        ))
    }

    fn validate_scope(&self, scope: PointerProviderScope) -> Result<(), PointerJournalLedgerError> {
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
