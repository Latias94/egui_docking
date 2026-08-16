//! Backend-captured total ordering across platform and pointer ingress lanes.
//!
//! This module owns only transport structure. It does not reduce records or
//! advance engine authority. A backend recorder binds one exact platform
//! provider incarnation to one exact desktop-global pointer provider and mints
//! opaque ordinals as facts are captured. Immutable batches can then be retried
//! until a host-frame commit advances the engine-owned watermark.

use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{Arc, Mutex, Weak};

use thiserror::Error;

use crate::effect::EffectResult;
use crate::engine::EngineInput;
use crate::ids::SurfaceId;
use crate::ids::{EngineAuthorityDomainId, WorkspaceEpoch};
use crate::platform::{PlatformSnapshot, WindowCloseObservation};
use crate::platform_provider::{PlatformObservationLease, PlatformProviderReservation};
use crate::pointer_journal::{
    PointerEdgeJournal, PointerEdgeSequence, PointerInputLease, PointerProviderScope,
};
use crate::presentation_observation::{HostPresentationObservationEntry, PresentationHostLease};
use crate::surface_recovery::{SurfaceRecoveryBootstrap, SurfaceRecoveryTarget};
use crate::transition::WorkspaceVersion;
use crate::viewport::{ViewportBinding, ViewportRole, WindowToken};

/// Opaque capture position in one backend ingress provider lifetime.
///
/// Ordinals are allocated only by [`BackendIngressRecorder`]. Callers may
/// retain and compare them, but cannot construct or advance them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(transparent)]
pub struct BackendIngressOrdinal(u64);

impl BackendIngressOrdinal {
    pub(crate) const ORIGIN: Self = Self(0);

    pub(crate) const fn from_committed_source_sequence(value: u64) -> Self {
        Self(value)
    }

    const fn checked_next(self) -> Option<Self> {
        match self.0.checked_add(1) {
            Some(value) => Some(Self(value)),
            None => None,
        }
    }

    /// Returns the diagnostic protocol representation.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}

/// Non-reused identity of one recorder append, independent of rollback ordinals.
///
/// Rollback may reuse a public capture position, but it must never make a
/// different payload indistinguishable from the immutable batch which core
/// already committed at that position.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct BackendIngressRecordIdentity(u64);

impl BackendIngressRecordIdentity {
    const ORIGIN: Self = Self(0);

    const fn checked_next(self) -> Option<Self> {
        match self.0.checked_add(1) {
            Some(value) => Some(Self(value)),
            None => None,
        }
    }
}

/// Single-writer synchronization shared by one recorder branch and every
/// frozen batch derived from it. The lock spans the final engine swap so a
/// concurrent rollback cannot revoke a record between validation and publish.
#[derive(Clone, Debug)]
struct BackendIngressCoordination(Arc<Mutex<()>>);

impl BackendIngressCoordination {
    fn new() -> Self {
        Self(Arc::new(Mutex::new(())))
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, ()> {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

impl PartialEq for BackendIngressCoordination {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

impl Eq for BackendIngressCoordination {}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
enum BackendIngressRecordState {
    Live = 0,
    Publishing = 1,
    Committed = 2,
    Revoked = 3,
}

impl BackendIngressRecordState {
    fn load(state: &AtomicU8) -> Self {
        match state.load(Ordering::Acquire) {
            0 => Self::Live,
            1 => Self::Publishing,
            2 => Self::Committed,
            3 => Self::Revoked,
            value => unreachable!("invalid backend ingress record state {value}"),
        }
    }
}

/// Shared publication state for one exact recorder append.
///
/// Immutable batches and document receipts retain the same allocation. The
/// recorder and final host publication therefore agree on one linearized
/// `Live -> Publishing -> Committed` or `Live -> Revoked` transition even when
/// public ordinals are later reused by another append branch.
#[derive(Clone, Debug)]
struct BackendIngressRecordLiveness(Arc<AtomicU8>);

impl BackendIngressRecordLiveness {
    fn new() -> Self {
        Self(Arc::new(AtomicU8::new(
            BackendIngressRecordState::Live as u8,
        )))
    }

    fn state(&self) -> BackendIngressRecordState {
        BackendIngressRecordState::load(&self.0)
    }

    fn is_live(&self) -> bool {
        self.state() == BackendIngressRecordState::Live
    }

    fn begin_publication(&self) {
        debug_assert_eq!(self.state(), BackendIngressRecordState::Live);
        self.0.store(
            BackendIngressRecordState::Publishing as u8,
            Ordering::Release,
        );
    }

    fn commit_publication(&self) {
        debug_assert_eq!(self.state(), BackendIngressRecordState::Publishing);
        self.0.store(
            BackendIngressRecordState::Committed as u8,
            Ordering::Release,
        );
    }

    fn abort_publication(&self) {
        debug_assert_eq!(self.state(), BackendIngressRecordState::Publishing);
        self.0
            .store(BackendIngressRecordState::Live as u8, Ordering::Release);
    }

    fn revoke_live(&self) {
        debug_assert_eq!(self.state(), BackendIngressRecordState::Live);
        self.0
            .store(BackendIngressRecordState::Revoked as u8, Ordering::Release);
    }
}

impl PartialEq for BackendIngressRecordLiveness {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

impl Eq for BackendIngressRecordLiveness {}

/// Test proof of one exact recorder append across savepoint rollback.
#[cfg(test)]
#[derive(Debug, Clone)]
pub(crate) struct BackendIngressRecordReceipt {
    lease: BackendIngressLease,
    ordinal: BackendIngressOrdinal,
    identity: BackendIngressRecordIdentity,
    liveness: BackendIngressRecordLiveness,
}

#[cfg(test)]
impl BackendIngressRecordReceipt {
    pub(crate) const fn ordinal(&self) -> BackendIngressOrdinal {
        self.ordinal
    }

    pub(crate) fn is_live(&self) -> bool {
        self.liveness.is_live()
    }
}

#[cfg(test)]
impl PartialEq for BackendIngressRecordReceipt {
    fn eq(&self, other: &Self) -> bool {
        self.lease == other.lease
            && self.ordinal == other.ordinal
            && self.identity == other.identity
            && self.liveness == other.liveness
    }
}

#[cfg(test)]
impl Eq for BackendIngressRecordReceipt {}

/// Exact backend lifetime joining platform and desktop-global pointer ingress.
///
/// The fields and constructor are private. A runtime may retain this lease but
/// cannot rebind either provider incarnation or move it to another engine
/// authority domain.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct BackendIngressLease {
    platform: PlatformObservationLease,
    pointer: PointerInputLease,
    presentation_host: PresentationHostLease,
}

/// Affine proof that one backend producer has stopped minting ingress records.
///
/// The receipt is created only by consuming the sole non-cloneable
/// [`BackendIngressRecorder`]. Records captured after the core's committed prefix are explicitly
/// abandoned at this boundary; immutable batches already handed out remain harmless because the
/// provider replacement revokes their exact lease before a successor is activated.
#[derive(Debug, PartialEq, Eq)]
#[must_use = "a drained backend producer must be retired or replaced"]
pub struct BackendIngressDrainReceipt {
    lease: BackendIngressLease,
    recorded_through: BackendIngressOrdinal,
    pointer_through: PointerEdgeSequence,
    consumed: bool,
}

impl BackendIngressDrainReceipt {
    /// Returns the exact joined provider lifetime whose producer stopped.
    #[must_use]
    pub const fn lease(&self) -> BackendIngressLease {
        self.lease
    }

    /// Returns the final backend ordinal minted before producer shutdown.
    #[must_use]
    pub const fn recorded_through(&self) -> BackendIngressOrdinal {
        self.recorded_through
    }

    /// Returns the final physical pointer sequence observed by the predecessor producer.
    #[must_use]
    pub const fn pointer_through(&self) -> PointerEdgeSequence {
        self.pointer_through
    }

    /// Returns whether this affine proof was transferred into a replacement ticket.
    #[must_use]
    pub const fn is_consumed(&self) -> bool {
        self.consumed
    }

    pub(crate) const fn validate_active(&self) -> Result<(), BackendIngressError> {
        if self.consumed {
            Err(BackendIngressError::DrainReceiptConsumed)
        } else {
            Ok(())
        }
    }

    pub(crate) fn transfer(&mut self) -> Result<Self, BackendIngressError> {
        self.validate_active()?;
        self.consumed = true;
        Ok(Self {
            lease: self.lease,
            recorded_through: self.recorded_through,
            pointer_through: self.pointer_through,
            consumed: false,
        })
    }

    /// Reconstructs the core-owned copy of a proof after the original adapter
    /// ticket was dropped. This is safe only for metadata already persisted in
    /// [`BackendIngressAuthority::pending_replacement`]; callers cannot supply
    /// this constructor data through the public API.
    fn from_replacement_state(
        lease: BackendIngressLease,
        recorded_through: BackendIngressOrdinal,
        pointer_through: PointerEdgeSequence,
    ) -> Self {
        Self {
            lease,
            recorded_through,
            pointer_through,
            consumed: false,
        }
    }
}

/// Affine proof that one recorder prefix has been reclaimed after core commit.
///
/// Binding quiescence facts inside this prefix are safe to apply only while the
/// engine remains at the receipt's opaque commit boundary. The receipt remains reusable
/// after a failed settlement and becomes consumed only after the engine publishes
/// every associated retention update atomically. Prefixes without binding
/// quiescence remain allocation-free and do not require engine settlement.
#[derive(Debug, PartialEq, Eq)]
#[must_use = "a reclaimed backend prefix must be settled with its core authority"]
pub struct BackendIngressPrefixRetirementReceipt {
    authority: Option<BackendIngressPrefixRetirementAuthority>,
}

#[derive(Debug, PartialEq, Eq)]
struct BackendIngressPrefixRetirementAuthority {
    lease: BackendIngressLease,
    through: BackendIngressOrdinal,
    binding_quiescences: Vec<ViewportBinding>,
}

impl BackendIngressPrefixRetirementReceipt {
    fn new(
        lease: BackendIngressLease,
        through: BackendIngressOrdinal,
        binding_quiescences: Vec<ViewportBinding>,
    ) -> Self {
        Self {
            authority: Some(BackendIngressPrefixRetirementAuthority {
                lease,
                through,
                binding_quiescences,
            }),
        }
    }

    pub(crate) const fn lease(&self) -> Option<BackendIngressLease> {
        match &self.authority {
            Some(authority) => Some(authority.lease),
            None => None,
        }
    }

    #[cfg(test)]
    pub(crate) const fn through(&self) -> Option<BackendIngressOrdinal> {
        match &self.authority {
            Some(authority) => Some(authority.through),
            None => None,
        }
    }

    pub(crate) fn binding_quiescences(&self) -> &[ViewportBinding] {
        self.authority
            .as_ref()
            .map_or(&[], |authority| &authority.binding_quiescences)
    }

    fn authority(&self) -> Result<&BackendIngressPrefixRetirementAuthority, BackendIngressError> {
        self.authority
            .as_ref()
            .ok_or(BackendIngressError::PrefixRetirementReceiptConsumed)
    }

    pub(crate) fn consume(&mut self) -> Result<(), BackendIngressError> {
        self.authority
            .take()
            .map(|_| ())
            .ok_or(BackendIngressError::PrefixRetirementReceiptConsumed)
    }
}

/// Affine authority to finish one exact joined backend-provider handoff.
///
/// This type deliberately does not implement `Clone` or `Copy`, and it never
/// exposes the core-owned platform reservation. A joined replacement can therefore
/// activate the platform, desktop-pointer, and backend-order lanes only through
/// the engine-owned joined replacement path.
#[derive(Debug, PartialEq, Eq)]
pub struct BackendIngressProviderReplacementTicket {
    authority: Option<BackendIngressProviderReplacementAuthority>,
}

#[derive(Debug)]
struct BackendIngressProviderReplacementAuthority {
    authority_domain: EngineAuthorityDomainId,
    handoff: BackendIngressReplacementId,
    generation: u64,
    _monitor: Arc<()>,
}

impl PartialEq for BackendIngressProviderReplacementAuthority {
    fn eq(&self, other: &Self) -> bool {
        self.authority_domain == other.authority_domain
            && self.handoff == other.handoff
            && self.generation == other.generation
            && Arc::ptr_eq(&self._monitor, &other._monitor)
    }
}

impl Eq for BackendIngressProviderReplacementAuthority {}

/// Core-owned identity of one pending joined-provider handoff.
///
/// The identity is deliberately separate from the platform ticket. A dropped
/// adapter ticket does not discard the quiescence proof retained by core, and a
/// later reissued ticket still has to name this exact pending handoff.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct BackendIngressReplacementId(u64);

impl BackendIngressReplacementId {
    const ORIGIN: Self = Self(0);

    const fn checked_next(self) -> Option<Self> {
        match self.0.checked_add(1) {
            Some(value) => Some(Self(value)),
            None => None,
        }
    }
}

impl BackendIngressProviderReplacementTicket {
    pub(crate) const fn new(
        authority_domain: EngineAuthorityDomainId,
        handoff: BackendIngressReplacementId,
        generation: u64,
        monitor: Arc<()>,
    ) -> Self {
        Self {
            authority: Some(BackendIngressProviderReplacementAuthority {
                authority_domain,
                handoff,
                generation,
                _monitor: monitor,
            }),
        }
    }

    pub(crate) fn authority(
        &self,
    ) -> Option<(
        EngineAuthorityDomainId,
        BackendIngressReplacementId,
        u64,
        &Arc<()>,
    )> {
        match &self.authority {
            Some(authority) => Some((
                authority.authority_domain,
                authority.handoff,
                authority.generation,
                &authority._monitor,
            )),
            None => None,
        }
    }

    pub(crate) fn consume(&mut self) -> bool {
        self.authority.take().is_some()
    }

    /// Returns whether this handoff authority was already committed.
    #[must_use]
    pub const fn is_consumed(&self) -> bool {
        self.authority.is_none()
    }
}

/// Core-minted proof that one exact joined provider committed an ingress prefix.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BackendIngressCommitWatermark {
    lease: BackendIngressLease,
    through: BackendIngressOrdinal,
    record_identity: BackendIngressRecordIdentity,
}

impl BackendIngressCommitWatermark {
    const fn new(
        lease: BackendIngressLease,
        through: BackendIngressOrdinal,
        record_identity: BackendIngressRecordIdentity,
    ) -> Self {
        Self {
            lease,
            through,
            record_identity,
        }
    }

    /// Returns the exact provider pair which committed this prefix.
    #[must_use]
    pub const fn lease(self) -> BackendIngressLease {
        self.lease
    }

    /// Returns the inclusive committed ingress position.
    #[must_use]
    pub const fn through(self) -> BackendIngressOrdinal {
        self.through
    }
}

impl BackendIngressLease {
    pub(crate) fn new(
        platform: PlatformObservationLease,
        pointer: PointerInputLease,
        presentation_host: PresentationHostLease,
    ) -> Result<Self, BackendIngressError> {
        let platform_domain = platform.authority_domain();
        let pointer_domain = pointer.authority_domain();
        if platform_domain != pointer_domain {
            return Err(BackendIngressError::AuthorityDomainMismatch {
                platform: platform_domain,
                pointer: pointer_domain,
            });
        }
        if pointer.scope() != PointerProviderScope::DesktopGlobal {
            return Err(BackendIngressError::PointerProviderNotDesktopGlobal { provider: pointer });
        }
        let presentation_domain = presentation_host.authority_domain();
        if presentation_domain != platform_domain {
            return Err(BackendIngressError::PresentationAuthorityDomainMismatch {
                backend: platform_domain,
                presentation: presentation_domain,
            });
        }
        Ok(Self {
            platform,
            pointer,
            presentation_host,
        })
    }

    /// Returns the engine authority domain shared by both provider incarnations.
    #[must_use]
    pub const fn authority_domain(self) -> EngineAuthorityDomainId {
        self.platform.authority_domain()
    }

    /// Returns the platform-provider incarnation for diagnostics.
    #[must_use]
    pub const fn platform_incarnation(self) -> u64 {
        self.platform.incarnation()
    }

    /// Returns the pointer-provider incarnation for diagnostics.
    #[must_use]
    pub const fn pointer_incarnation(self) -> u64 {
        self.pointer.incarnation()
    }

    /// Returns the exact presentation host bound to this backend lifetime.
    #[must_use]
    pub const fn presentation_host(self) -> PresentationHostLease {
        self.presentation_host
    }

    pub(crate) const fn platform_provider(self) -> PlatformObservationLease {
        self.platform
    }

    pub(crate) const fn pointer_provider(self) -> PointerInputLease {
        self.pointer
    }
}

/// Payload captured at one exact backend ingress ordinal.
///
/// Lane-local generations remain part of each payload. The enclosing ingress
/// ordinal adds only the total order between those independent lanes.
#[derive(Debug, Clone, PartialEq)]
pub enum BackendIngressPayload {
    /// One complete atomic platform observation batch.
    PlatformSnapshot {
        /// Workspace epoch whose binding roster the backend observed.
        expected_epoch: WorkspaceEpoch,
        /// Complete platform fact envelope captured at this ingress position.
        snapshot: PlatformSnapshot,
    },
    /// The producer has permanently closed every ingress lane for one exact binding.
    ///
    /// This record does not itself release core retention. Reclamation becomes
    /// authoritative only when the recorder retires a core-committed prefix and
    /// returns a [`BackendIngressPrefixRetirementReceipt`].
    PlatformBindingQuiesced {
        /// Exact core-minted binding which no future producer fact may name.
        binding: ViewportBinding,
    },
    /// One exact binding-scoped native-close observation.
    NativeCloseObservation {
        /// Workspace epoch whose binding incarnation the backend observed.
        expected_epoch: WorkspaceEpoch,
        /// Exact close fact captured at this ingress position.
        observation: WindowCloseObservation,
    },
    /// One globally consistent native-focus observation.
    GlobalFocusObservation {
        /// Workspace epoch whose binding incarnation the backend observed.
        expected_epoch: WorkspaceEpoch,
        /// Provider-owned focus fact captured at this ingress position.
        observation: crate::viewport_focus::FocusObservationEnvelope,
    },
    /// One non-observational platform-effect dispatch result.
    PlatformEffectResult(EffectResult),
    /// One narrow non-pointer semantic action captured by the backend event loop.
    SemanticInput(EngineInput),
    /// One presentation-stream fact captured at this exact backend position.
    PresentationObservation {
        /// Core-minted host bound when this backend provider was enrolled.
        host: PresentationHostLease,
        /// One stream observation; a no-update entry still consumes this ordinal.
        entry: HostPresentationObservationEntry,
    },
    /// One edgewise pointer journal segment, including an optional authority checkpoint.
    PointerSegment(PointerEdgeJournal),
}

/// One immutable provider-bound record in a backend ingress stream.
#[derive(Debug, Clone, PartialEq)]
pub struct BackendIngressRecord {
    lease: BackendIngressLease,
    ordinal: BackendIngressOrdinal,
    identity: BackendIngressRecordIdentity,
    payload: BackendIngressPayload,
    liveness: BackendIngressRecordLiveness,
}

impl BackendIngressRecord {
    /// Returns this record's opaque capture position.
    #[must_use]
    pub const fn ordinal(&self) -> BackendIngressOrdinal {
        self.ordinal
    }

    /// Returns the typed fact captured at this position.
    #[must_use]
    pub const fn payload(&self) -> &BackendIngressPayload {
        &self.payload
    }
}

/// Commit-time proof that every record reduced by one prepared frame still
/// belongs to the recorder branch which produced it.
#[derive(Debug)]
pub(crate) struct BackendIngressCommitGuard {
    coordination: BackendIngressCoordination,
    records: Vec<(BackendIngressOrdinal, BackendIngressRecordLiveness)>,
}

impl BackendIngressCommitGuard {
    pub(crate) fn validate(&self) -> Result<(), BackendIngressError> {
        let _coordination = self.coordination.lock();
        self.validate_locked()
    }

    pub(crate) fn publish_with<T, E>(
        self,
        publish: impl FnOnce() -> Result<T, E>,
    ) -> Result<Result<T, E>, BackendIngressError> {
        let _coordination = self.coordination.lock();
        self.validate_locked()?;
        for (_, liveness) in &self.records {
            liveness.begin_publication();
        }
        match publish() {
            Ok(value) => {
                for (_, liveness) in &self.records {
                    liveness.commit_publication();
                }
                Ok(Ok(value))
            }
            Err(error) => {
                for (_, liveness) in &self.records {
                    liveness.abort_publication();
                }
                Ok(Err(error))
            }
        }
    }

    fn validate_locked(&self) -> Result<(), BackendIngressError> {
        for (ordinal, liveness) in &self.records {
            match liveness.state() {
                BackendIngressRecordState::Live => {}
                BackendIngressRecordState::Publishing => {
                    return Err(BackendIngressError::RecordPublicationInProgress {
                        ordinal: *ordinal,
                    });
                }
                BackendIngressRecordState::Committed => {
                    return Err(BackendIngressError::RecordAlreadyCommitted { ordinal: *ordinal });
                }
                BackendIngressRecordState::Revoked => {
                    return Err(BackendIngressError::RecordRevoked { ordinal: *ordinal });
                }
            }
        }
        Ok(())
    }
}

/// Immutable complete interval of one backend ingress stream.
///
/// Construction is private and validates exact lease equality plus a contiguous
/// `(previous, through]` ordinal interval. The value is cloneable and contains
/// no consumptive cursor, so a failed host-frame attempt may retry the same batch.
#[derive(Debug, Clone, PartialEq)]
pub struct BackendIngressBatch {
    coordination: BackendIngressCoordination,
    lease: BackendIngressLease,
    previous: BackendIngressOrdinal,
    previous_record_identity: BackendIngressRecordIdentity,
    through: BackendIngressOrdinal,
    records: Vec<BackendIngressRecord>,
}

impl BackendIngressBatch {
    fn from_records(
        coordination: BackendIngressCoordination,
        lease: BackendIngressLease,
        previous: BackendIngressOrdinal,
        previous_record_identity: BackendIngressRecordIdentity,
        through: BackendIngressOrdinal,
        records: Vec<BackendIngressRecord>,
    ) -> Result<Self, BackendIngressError> {
        let batch = Self {
            coordination,
            lease,
            previous,
            previous_record_identity,
            through,
            records,
        };
        batch.validate_shape()?;
        Ok(batch)
    }

    /// Returns the exact joined provider lifetime which captured this batch.
    #[must_use]
    pub const fn lease(&self) -> BackendIngressLease {
        self.lease
    }

    /// Returns the exclusive lower ingress watermark.
    #[must_use]
    pub const fn previous(&self) -> BackendIngressOrdinal {
        self.previous
    }

    /// Returns the inclusive upper ingress watermark.
    #[must_use]
    pub const fn through(&self) -> BackendIngressOrdinal {
        self.through
    }

    /// Returns records in immutable backend capture order.
    #[must_use]
    pub fn records(&self) -> &[BackendIngressRecord] {
        &self.records
    }

    /// Returns the number of captured records in this interval.
    #[must_use]
    pub fn len(&self) -> usize {
        self.records.len()
    }

    /// Returns whether this interval advances no ingress facts.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }

    /// Validates this batch against the engine-owned provider and committed watermark.
    fn validate_against(
        &self,
        expected_lease: BackendIngressLease,
        committed_through: BackendIngressOrdinal,
        committed_record_identity: BackendIngressRecordIdentity,
    ) -> Result<(), BackendIngressError> {
        if self.lease != expected_lease {
            return Err(BackendIngressError::BatchLeaseMismatch {
                expected: expected_lease,
                submitted: self.lease,
            });
        }
        if self.previous != committed_through {
            return Err(BackendIngressError::BatchPreviousMismatch {
                expected: committed_through,
                submitted: self.previous,
            });
        }
        if self.previous_record_identity != committed_record_identity {
            return Err(BackendIngressError::BatchPreviousIdentityMismatch {
                ordinal: committed_through,
            });
        }
        self.validate_shape()?;
        self.validate_publishable()
    }

    fn validate_shape(&self) -> Result<(), BackendIngressError> {
        let mut cursor = self.previous;
        for record in &self.records {
            if record.lease != self.lease {
                return Err(BackendIngressError::RecordLeaseMismatch {
                    ordinal: record.ordinal,
                    expected: self.lease,
                    submitted: record.lease,
                });
            }
            let expected = cursor
                .checked_next()
                .ok_or(BackendIngressError::OrdinalExhausted)?;
            if record.ordinal != expected {
                return Err(BackendIngressError::RecordOrdinalGap {
                    expected,
                    submitted: record.ordinal,
                });
            }
            cursor = record.ordinal;
        }
        if cursor != self.through {
            return Err(BackendIngressError::BatchThroughMismatch {
                expected: cursor,
                submitted: self.through,
            });
        }
        Ok(())
    }

    fn validate_publishable(&self) -> Result<(), BackendIngressError> {
        for record in &self.records {
            match record.liveness.state() {
                BackendIngressRecordState::Live => {}
                BackendIngressRecordState::Publishing => {
                    return Err(BackendIngressError::RecordPublicationInProgress {
                        ordinal: record.ordinal,
                    });
                }
                BackendIngressRecordState::Committed => {
                    return Err(BackendIngressError::RecordAlreadyCommitted {
                        ordinal: record.ordinal,
                    });
                }
                BackendIngressRecordState::Revoked => {
                    return Err(BackendIngressError::RecordRevoked {
                        ordinal: record.ordinal,
                    });
                }
            }
        }
        Ok(())
    }

    pub(crate) fn commit_guard(&self) -> BackendIngressCommitGuard {
        BackendIngressCommitGuard {
            coordination: self.coordination.clone(),
            records: self
                .records
                .iter()
                .map(|record| (record.ordinal, record.liveness.clone()))
                .collect(),
        }
    }
}

/// Single-writer backend capture recorder.
///
/// The recorder is intentionally not cloneable. Calling its record methods in
/// the native event-loop callback order is the sole way to mint new ingress
/// ordinals. Producing a batch is non-destructive, which keeps every captured
/// interval retryable until the engine publishes its own commit watermark.
#[derive(Debug)]
pub struct BackendIngressRecorder {
    coordination: BackendIngressCoordination,
    lease: BackendIngressLease,
    retired_through: BackendIngressOrdinal,
    retired_record_identity: BackendIngressRecordIdentity,
    last_ordinal: BackendIngressOrdinal,
    /// Monotonic append authority which deliberately does not rewind on rollback.
    last_record_identity: BackendIngressRecordIdentity,
    pointer_through: PointerEdgeSequence,
    records: Vec<BackendIngressRecord>,
}

/// Affine rollback boundary for one recorder-owned append transaction.
///
/// The fields are private so an adapter cannot forge an earlier ingress or
/// pointer watermark. Consuming this value through
/// [`BackendIngressRecorder::rollback_to`] restores only records appended by
/// the same recorder after the savepoint was minted.
#[derive(Debug)]
pub struct BackendIngressSavepoint {
    lease: BackendIngressLease,
    retired_through: BackendIngressOrdinal,
    recorded_through: BackendIngressOrdinal,
    boundary_record_identity: BackendIngressRecordIdentity,
    pointer_through: PointerEdgeSequence,
    record_count: usize,
}

impl BackendIngressSavepoint {
    /// Returns the exact joined provider lifetime owning this boundary.
    #[must_use]
    pub const fn lease(&self) -> BackendIngressLease {
        self.lease
    }

    /// Returns the latest record which existed before the transaction began.
    #[must_use]
    pub const fn recorded_through(&self) -> BackendIngressOrdinal {
        self.recorded_through
    }
}

impl BackendIngressRecorder {
    /// Creates the recorder for an already core-minted provider pair.
    ///
    /// The constructor remains crate-private because enrolling or replacing
    /// providers is engine authority, not an adapter decision.
    pub(crate) fn new(
        platform: PlatformObservationLease,
        pointer: PointerInputLease,
        presentation_host: PresentationHostLease,
        pointer_committed_through: PointerEdgeSequence,
    ) -> Result<Self, BackendIngressError> {
        Ok(Self {
            coordination: BackendIngressCoordination::new(),
            lease: BackendIngressLease::new(platform, pointer, presentation_host)?,
            retired_through: BackendIngressOrdinal::ORIGIN,
            retired_record_identity: BackendIngressRecordIdentity::ORIGIN,
            last_ordinal: BackendIngressOrdinal::ORIGIN,
            last_record_identity: BackendIngressRecordIdentity::ORIGIN,
            pointer_through: pointer_committed_through,
            records: Vec::new(),
        })
    }

    /// Returns the exact provider pair owned by this recorder.
    #[must_use]
    pub const fn lease(&self) -> BackendIngressLease {
        self.lease
    }

    /// Returns the latest ordinal allocated by this recorder.
    #[must_use]
    pub const fn recorded_through(&self) -> BackendIngressOrdinal {
        self.last_ordinal
    }

    /// Returns the latest pointer edge sequence captured by this recorder.
    #[must_use]
    pub(crate) const fn pointer_through(&self) -> PointerEdgeSequence {
        self.pointer_through
    }

    /// Mints an affine rollback boundary for a fallible adapter append batch.
    #[must_use]
    pub fn savepoint(&self) -> BackendIngressSavepoint {
        BackendIngressSavepoint {
            lease: self.lease,
            retired_through: self.retired_through,
            recorded_through: self.last_ordinal,
            boundary_record_identity: self
                .records
                .last()
                .map_or(self.retired_record_identity, |record| record.identity),
            pointer_through: self.pointer_through,
            record_count: self.records.len(),
        }
    }

    /// Removes every record appended after an exact recorder savepoint.
    ///
    /// # Errors
    ///
    /// Returns an error when the savepoint belongs to another provider, when
    /// its prefix was already reclaimed, or when recorder state no longer
    /// contains the captured prefix.
    pub fn rollback_to(
        &mut self,
        savepoint: BackendIngressSavepoint,
    ) -> Result<(), BackendIngressError> {
        if savepoint.lease != self.lease {
            return Err(BackendIngressError::SavepointLeaseMismatch {
                expected: self.lease,
                submitted: savepoint.lease,
            });
        }
        if savepoint.retired_through != self.retired_through {
            return Err(BackendIngressError::SavepointPrefixReclaimed {
                saved: savepoint.retired_through,
                current: self.retired_through,
            });
        }
        if savepoint.record_count > self.records.len()
            || savepoint.recorded_through > self.last_ordinal
        {
            return Err(BackendIngressError::SavepointAhead {
                saved: savepoint.recorded_through,
                current: self.last_ordinal,
            });
        }
        let current_boundary = self
            .record_count_boundary(savepoint.record_count)
            .map(|record| (record.ordinal, record.identity))
            .unwrap_or((self.retired_through, self.retired_record_identity));
        if current_boundary
            != (
                savepoint.recorded_through,
                savepoint.boundary_record_identity,
            )
        {
            return Err(BackendIngressError::SavepointPrefixRewritten {
                boundary: savepoint.recorded_through,
            });
        }
        let _coordination = self.coordination.lock();
        for record in &self.records[savepoint.record_count..] {
            match record.liveness.state() {
                BackendIngressRecordState::Live => {}
                BackendIngressRecordState::Publishing | BackendIngressRecordState::Committed => {
                    return Err(BackendIngressError::RollbackCrossesPublishedRecord {
                        ordinal: record.ordinal,
                    });
                }
                BackendIngressRecordState::Revoked => {
                    return Err(BackendIngressError::RecordRevoked {
                        ordinal: record.ordinal,
                    });
                }
            }
        }
        for record in &self.records[savepoint.record_count..] {
            record.liveness.revoke_live();
        }
        self.records.truncate(savepoint.record_count);
        self.last_ordinal = savepoint.recorded_through;
        self.pointer_through = savepoint.pointer_through;
        Ok(())
    }

    /// Stops this producer and returns its non-forgeable replacement proof.
    ///
    /// Consuming the recorder is the type-level join boundary: no caller can mint another record
    /// for this backend lifetime afterward. Captured suffix records which were not committed by
    /// the core are intentionally abandoned and cannot cross the subsequent lease replacement.
    #[must_use]
    pub fn drain(self) -> BackendIngressDrainReceipt {
        let _coordination = self.coordination.lock();
        for record in &self.records {
            if record.liveness.is_live() {
                record.liveness.revoke_live();
            }
        }
        BackendIngressDrainReceipt {
            lease: self.lease,
            recorded_through: self.last_ordinal,
            pointer_through: self.pointer_through,
            consumed: false,
        }
    }

    /// Records one complete platform snapshot at the next backend position.
    pub fn record_platform_snapshot(
        &mut self,
        expected_epoch: WorkspaceEpoch,
        snapshot: PlatformSnapshot,
    ) -> Result<BackendIngressOrdinal, BackendIngressError> {
        self.push(BackendIngressPayload::PlatformSnapshot {
            expected_epoch,
            snapshot,
        })
    }

    /// Records permanent producer quiescence for one exact platform binding.
    ///
    /// The caller must first remove every route, callback, and sidecar capable of
    /// generating a later fact for this binding. A future fact which violates that
    /// promise is rejected fail-closed by the core after the guard is reclaimed.
    ///
    /// # Errors
    ///
    /// Returns an error when the binding belongs to another engine authority
    /// domain, the retained prefix already closes the same binding lane, or the
    /// backend ordinal cannot advance without wrapping.
    pub fn record_platform_binding_quiescence(
        &mut self,
        binding: ViewportBinding,
    ) -> Result<BackendIngressOrdinal, BackendIngressError> {
        if binding.authority_domain() != self.lease.authority_domain() {
            return Err(BackendIngressError::BindingQuiescenceAuthorityMismatch {
                expected: self.lease.authority_domain(),
                submitted: binding.authority_domain(),
            });
        }
        if self.records.iter().any(|record| {
            matches!(
                &record.payload,
                BackendIngressPayload::PlatformBindingQuiesced {
                    binding: recorded
                } if *recorded == binding
            )
        }) {
            return Err(BackendIngressError::DuplicateBindingQuiescence { binding });
        }
        self.push(BackendIngressPayload::PlatformBindingQuiesced { binding })
    }

    /// Records one exact binding-scoped native-close observation.
    pub fn record_native_close_observation(
        &mut self,
        expected_epoch: WorkspaceEpoch,
        observation: WindowCloseObservation,
    ) -> Result<BackendIngressOrdinal, BackendIngressError> {
        self.push(BackendIngressPayload::NativeCloseObservation {
            expected_epoch,
            observation,
        })
    }

    /// Records one globally consistent native-focus observation.
    pub fn record_global_focus_observation(
        &mut self,
        expected_epoch: WorkspaceEpoch,
        observation: crate::viewport_focus::FocusObservationEnvelope,
    ) -> Result<BackendIngressOrdinal, BackendIngressError> {
        self.push(BackendIngressPayload::GlobalFocusObservation {
            expected_epoch,
            observation,
        })
    }

    /// Records one platform-effect result at the next backend position.
    pub fn record_platform_effect_result(
        &mut self,
        result: EffectResult,
    ) -> Result<BackendIngressOrdinal, BackendIngressError> {
        self.push(BackendIngressPayload::PlatformEffectResult(result))
    }

    /// Records one non-pointer semantic action at the next backend position.
    ///
    /// Platform facts must use their typed lanes and configuration remains a
    /// terminal host-frame phase, so neither can be smuggled through this path.
    pub fn record_semantic_input(
        &mut self,
        input: EngineInput,
    ) -> Result<BackendIngressOrdinal, BackendIngressError> {
        if input.is_backend_ingress_fact() {
            return Err(BackendIngressError::SemanticInputIsBackendFact);
        }
        if input.is_configuration_commit() {
            return Err(BackendIngressError::SemanticInputIsConfiguration);
        }
        self.push(BackendIngressPayload::SemanticInput(input))
    }

    #[cfg(test)]
    pub(crate) fn record_semantic_input_receipt(
        &mut self,
        input: EngineInput,
    ) -> Result<BackendIngressRecordReceipt, BackendIngressError> {
        let ordinal = self.record_semantic_input(input)?;
        let record = self
            .records
            .last()
            .expect("a successful append retains its record");
        Ok(BackendIngressRecordReceipt {
            lease: record.lease,
            ordinal,
            identity: record.identity,
            liveness: record.liveness.clone(),
        })
    }

    pub(crate) fn record_presentation_observation(
        &mut self,
        entry: HostPresentationObservationEntry,
    ) -> Result<BackendIngressOrdinal, BackendIngressError> {
        self.push(BackendIngressPayload::PresentationObservation {
            host: self.lease.presentation_host,
            entry,
        })
    }

    /// Records one viewport registration through this recorder's exact platform provider.
    ///
    /// The provider identity is deliberately not supplied by the caller. This keeps a native
    /// runtime from accidentally pairing a current registration with a superseded platform
    /// provider while still allowing registration to share the backend's total input order.
    pub fn record_viewport_registration(
        &mut self,
        expected: WorkspaceVersion,
        surface: SurfaceId,
        token: WindowToken,
        role: ViewportRole,
        recovery_target: Option<SurfaceRecoveryTarget>,
    ) -> Result<BackendIngressOrdinal, BackendIngressError> {
        self.record_semantic_input(EngineInput::RegisterViewport {
            provider: self.lease.platform_provider(),
            expected,
            surface,
            token,
            role,
            recovery_target,
        })
    }

    /// Records the first registration of an existing docking-owned child viewport.
    ///
    /// The core resolves the current recovery anchor and mints any presentation identity; this
    /// recorder carries only adapter-observed bootstrap facts.
    pub fn record_child_viewport_bootstrap(
        &mut self,
        expected: WorkspaceVersion,
        surface: SurfaceId,
        token: WindowToken,
        recovery: SurfaceRecoveryBootstrap,
    ) -> Result<BackendIngressOrdinal, BackendIngressError> {
        self.record_semantic_input(EngineInput::BootstrapChildViewport {
            provider: self.lease.platform_provider(),
            expected,
            surface,
            token,
            recovery,
        })
    }

    /// Records one pointer segment at the next backend position.
    ///
    /// Segments remain edgewise because receiver evidence is requested after
    /// each edge. The pointer watermark must continue exactly from the preceding
    /// segment even when platform records occur between pointer records.
    pub fn record_pointer_segment(
        &mut self,
        segment: PointerEdgeJournal,
    ) -> Result<BackendIngressOrdinal, BackendIngressError> {
        if segment.len() > 1 {
            return Err(BackendIngressError::PointerSegmentMustBeEdgewise {
                edges: segment.len(),
            });
        }
        if segment.previous() != self.pointer_through {
            return Err(BackendIngressError::PointerSegmentPreviousMismatch {
                expected: self.pointer_through,
                submitted: segment.previous(),
            });
        }
        let through = segment.through();
        let ordinal = self.push(BackendIngressPayload::PointerSegment(segment))?;
        self.pointer_through = through;
        Ok(ordinal)
    }

    /// Returns the complete immutable stream captured so far.
    pub fn pending_batch(&self) -> Result<BackendIngressBatch, BackendIngressError> {
        BackendIngressBatch::from_records(
            self.coordination.clone(),
            self.lease,
            self.retired_through,
            self.retired_record_identity,
            self.last_ordinal,
            self.records.clone(),
        )
    }

    /// Returns the immutable suffix after an engine-committed watermark.
    ///
    /// This operation does not consume records. Repeating it before more facts
    /// are recorded returns an exactly equal batch suitable for atomic retry.
    pub fn batch_after(
        &self,
        committed_through: BackendIngressOrdinal,
    ) -> Result<BackendIngressBatch, BackendIngressError> {
        if committed_through < self.retired_through {
            return Err(BackendIngressError::CommittedWatermarkRetired {
                committed: committed_through,
                retired: self.retired_through,
            });
        }
        if committed_through > self.last_ordinal {
            return Err(BackendIngressError::CommittedWatermarkAhead {
                committed: committed_through,
                recorded: self.last_ordinal,
            });
        }
        let previous_record_identity = if committed_through == self.retired_through {
            self.retired_record_identity
        } else {
            self.records
                .iter()
                .find(|record| record.ordinal == committed_through)
                .map(|record| record.identity)
                .ok_or(BackendIngressError::CommitWatermarkContentMismatch {
                    ordinal: committed_through,
                })?
        };
        let records = self
            .records
            .iter()
            .filter(|record| record.ordinal > committed_through)
            .cloned()
            .collect();
        BackendIngressBatch::from_records(
            self.coordination.clone(),
            self.lease,
            committed_through,
            previous_record_identity,
            self.last_ordinal,
            records,
        )
    }

    /// Reclaims the exact prefix proven committed by the core.
    ///
    /// # Errors
    ///
    /// Returns an error when the watermark belongs to another backend lifetime,
    /// regresses behind the reclaimed prefix, or advances beyond captured input.
    pub fn retire_committed_prefix(
        &mut self,
        committed: BackendIngressCommitWatermark,
    ) -> Result<Option<BackendIngressPrefixRetirementReceipt>, BackendIngressError> {
        if committed.lease != self.lease {
            return Err(BackendIngressError::CommitWatermarkLeaseMismatch {
                expected: self.lease,
                submitted: committed.lease,
            });
        }
        if committed.through < self.retired_through {
            return Err(BackendIngressError::RetirementWatermarkRegressed {
                retired: self.retired_through,
                submitted: committed.through,
            });
        }
        if committed.through > self.last_ordinal {
            return Err(BackendIngressError::CommittedWatermarkAhead {
                committed: committed.through,
                recorded: self.last_ordinal,
            });
        }
        if committed.through == self.retired_through {
            return Ok(None);
        }
        let retained = self
            .records
            .partition_point(|record| record.ordinal <= committed.through);
        let committed_record = self.records.get(retained.saturating_sub(1)).ok_or(
            BackendIngressError::CommitWatermarkContentMismatch {
                ordinal: committed.through,
            },
        )?;
        if committed_record.ordinal != committed.through
            || committed_record.identity != committed.record_identity
        {
            return Err(BackendIngressError::CommitWatermarkContentMismatch {
                ordinal: committed.through,
            });
        }
        let mut binding_quiescences = self.records[..retained]
            .iter()
            .filter_map(|record| match &record.payload {
                BackendIngressPayload::PlatformBindingQuiesced { binding } => Some(*binding),
                BackendIngressPayload::PlatformSnapshot { .. }
                | BackendIngressPayload::NativeCloseObservation { .. }
                | BackendIngressPayload::GlobalFocusObservation { .. }
                | BackendIngressPayload::PlatformEffectResult(_)
                | BackendIngressPayload::SemanticInput(_)
                | BackendIngressPayload::PresentationObservation { .. }
                | BackendIngressPayload::PointerSegment(_) => None,
            })
            .collect::<Vec<_>>();
        binding_quiescences.sort_unstable();
        binding_quiescences.dedup();
        self.records.drain(..retained);
        self.retired_through = committed.through;
        self.retired_record_identity = committed.record_identity;
        if binding_quiescences.is_empty() {
            Ok(None)
        } else {
            Ok(Some(BackendIngressPrefixRetirementReceipt::new(
                self.lease,
                committed.through,
                binding_quiescences,
            )))
        }
    }

    /// Returns the number of captured records not yet reclaimed.
    #[doc(hidden)]
    #[must_use]
    pub const fn retained_record_count(&self) -> usize {
        self.records.len()
    }

    fn push(
        &mut self,
        payload: BackendIngressPayload,
    ) -> Result<BackendIngressOrdinal, BackendIngressError> {
        let ordinal = self
            .last_ordinal
            .checked_next()
            .ok_or(BackendIngressError::OrdinalExhausted)?;
        let identity = self
            .last_record_identity
            .checked_next()
            .ok_or(BackendIngressError::RecordIdentityExhausted)?;
        let liveness = BackendIngressRecordLiveness::new();
        self.records.push(BackendIngressRecord {
            lease: self.lease,
            ordinal,
            identity,
            payload,
            liveness,
        });
        self.last_ordinal = ordinal;
        self.last_record_identity = identity;
        Ok(ordinal)
    }

    fn record_count_boundary(&self, record_count: usize) -> Option<&BackendIngressRecord> {
        record_count
            .checked_sub(1)
            .and_then(|index| self.records.get(index))
    }
}

/// Engine-owned publication state for the sole joined backend ingress lane.
///
/// Provider-local capture lives in [`BackendIngressRecorder`]. This ledger is
/// cloned into a host-frame candidate, so accepting a batch advances its
/// watermark only when the complete host frame commits.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct BackendIngressAuthority {
    authority_domain: EngineAuthorityDomainId,
    active: Option<BackendIngressLease>,
    committed_through: BackendIngressOrdinal,
    committed_record_identity: BackendIngressRecordIdentity,
    next_replacement_id: BackendIngressReplacementId,
    pending_replacement: Option<BackendIngressReplacementState>,
}

#[derive(Debug, Clone)]
pub(crate) struct BackendIngressReplacementState {
    handoff: BackendIngressReplacementId,
    ticket_generation: u64,
    ticket_monitor: Weak<()>,
    platform: PlatformProviderReservation,
    predecessor: BackendIngressLease,
    recorded_through: BackendIngressOrdinal,
    pointer_through: PointerEdgeSequence,
}

impl PartialEq for BackendIngressReplacementState {
    fn eq(&self, other: &Self) -> bool {
        self.handoff == other.handoff
            && self.ticket_generation == other.ticket_generation
            && Weak::ptr_eq(&self.ticket_monitor, &other.ticket_monitor)
            && self.platform == other.platform
            && self.predecessor == other.predecessor
            && self.recorded_through == other.recorded_through
            && self.pointer_through == other.pointer_through
    }
}

impl Eq for BackendIngressReplacementState {}

impl BackendIngressReplacementState {
    pub(crate) const fn handoff(&self) -> BackendIngressReplacementId {
        self.handoff
    }

    pub(crate) const fn platform(&self) -> PlatformProviderReservation {
        self.platform
    }

    pub(crate) const fn predecessor(&self) -> BackendIngressLease {
        self.predecessor
    }

    pub(crate) const fn pointer_through(&self) -> PointerEdgeSequence {
        self.pointer_through
    }

    pub(crate) fn drain_receipt(&self) -> BackendIngressDrainReceipt {
        BackendIngressDrainReceipt::from_replacement_state(
            self.predecessor,
            self.recorded_through,
            self.pointer_through,
        )
    }
}

impl BackendIngressAuthority {
    pub(crate) const fn new(authority_domain: EngineAuthorityDomainId) -> Self {
        Self {
            authority_domain,
            active: None,
            committed_through: BackendIngressOrdinal::ORIGIN,
            committed_record_identity: BackendIngressRecordIdentity::ORIGIN,
            next_replacement_id: BackendIngressReplacementId::ORIGIN,
            pending_replacement: None,
        }
    }

    pub(crate) const fn active(&self) -> Option<BackendIngressLease> {
        self.active
    }

    pub(crate) const fn committed_through(&self) -> BackendIngressOrdinal {
        self.committed_through
    }

    pub(crate) fn reserve_replacement(
        &mut self,
        platform: PlatformProviderReservation,
        predecessor: &mut BackendIngressDrainReceipt,
    ) -> Result<BackendIngressProviderReplacementTicket, BackendIngressError> {
        if let Some(active) = self.active {
            return Err(BackendIngressError::ProviderAlreadyActive { active });
        }
        if self.pending_replacement.is_some() {
            return Err(BackendIngressError::ProviderReplacementPending);
        }
        predecessor.validate_active()?;
        if predecessor.lease.authority_domain() != self.authority_domain {
            return Err(BackendIngressError::ForeignAuthorityDomain {
                expected: self.authority_domain,
                submitted: predecessor.lease.authority_domain(),
            });
        }
        let handoff = self
            .next_replacement_id
            .checked_next()
            .ok_or(BackendIngressError::ReplacementIdentityExhausted)?;
        let predecessor = predecessor.transfer()?;
        self.next_replacement_id = handoff;
        self.pending_replacement = Some(BackendIngressReplacementState {
            handoff,
            ticket_generation: 1,
            ticket_monitor: Weak::new(),
            platform,
            predecessor: predecessor.lease,
            recorded_through: predecessor.recorded_through,
            pointer_through: predecessor.pointer_through,
        });
        let state = self
            .pending_replacement
            .as_mut()
            .expect("replacement was inserted immediately above");
        Ok(Self::issue_replacement_ticket(self.authority_domain, state))
    }

    pub(crate) fn reissue_replacement_ticket(
        &mut self,
    ) -> Result<BackendIngressProviderReplacementTicket, BackendIngressError> {
        let state = self
            .pending_replacement
            .as_mut()
            .ok_or(BackendIngressError::ProviderReplacementNotPending)?;
        state.ticket_generation = state
            .ticket_generation
            .checked_add(1)
            .ok_or(BackendIngressError::ReplacementTicketGenerationExhausted)?;
        Ok(Self::issue_replacement_ticket(self.authority_domain, state))
    }

    fn issue_replacement_ticket(
        authority_domain: EngineAuthorityDomainId,
        state: &mut BackendIngressReplacementState,
    ) -> BackendIngressProviderReplacementTicket {
        let monitor = Arc::new(());
        state.ticket_monitor = Arc::downgrade(&monitor);
        BackendIngressProviderReplacementTicket::new(
            authority_domain,
            state.handoff,
            state.ticket_generation,
            monitor,
        )
    }

    pub(crate) fn replacement_state(
        &self,
        authority_domain: EngineAuthorityDomainId,
        handoff: BackendIngressReplacementId,
        generation: u64,
        monitor: &Arc<()>,
    ) -> Result<BackendIngressReplacementState, BackendIngressError> {
        if authority_domain != self.authority_domain {
            return Err(BackendIngressError::ForeignAuthorityDomain {
                expected: self.authority_domain,
                submitted: authority_domain,
            });
        }
        let state = self
            .pending_replacement
            .as_ref()
            .ok_or(BackendIngressError::ProviderReplacementNotPending)?;
        if state.handoff != handoff || state.ticket_generation != generation {
            return Err(BackendIngressError::UnknownReplacementTicket);
        }
        let Some(current_monitor) = state.ticket_monitor.upgrade() else {
            return Err(BackendIngressError::UnknownReplacementTicket);
        };
        if !Arc::ptr_eq(&current_monitor, monitor) {
            return Err(BackendIngressError::UnknownReplacementTicket);
        }
        Ok(state.clone())
    }

    pub(crate) fn replacement_ticket_abandoned(&self) -> bool {
        self.pending_replacement
            .as_ref()
            .is_some_and(|state| state.ticket_monitor.upgrade().is_none())
    }

    pub(crate) fn pending_replacement_state(
        &self,
    ) -> Result<BackendIngressReplacementState, BackendIngressError> {
        self.pending_replacement
            .clone()
            .ok_or(BackendIngressError::ProviderReplacementNotPending)
    }

    pub(crate) fn complete_replacement(
        &mut self,
        authority_domain: EngineAuthorityDomainId,
        handoff: BackendIngressReplacementId,
        generation: u64,
        monitor: &Arc<()>,
    ) -> Result<(), BackendIngressError> {
        let _ = self.replacement_state(authority_domain, handoff, generation, monitor)?;
        self.pending_replacement = None;
        Ok(())
    }

    pub(crate) fn abort_replacement(
        &mut self,
        handoff: BackendIngressReplacementId,
    ) -> Result<(), BackendIngressError> {
        let state = self
            .pending_replacement
            .as_ref()
            .ok_or(BackendIngressError::ProviderReplacementNotPending)?;
        if state.handoff != handoff {
            return Err(BackendIngressError::UnknownReplacementTicket);
        }
        self.pending_replacement = None;
        Ok(())
    }

    pub(crate) fn enroll(
        &mut self,
        platform: PlatformObservationLease,
        pointer: PointerInputLease,
        presentation_host: PresentationHostLease,
        pointer_committed_through: PointerEdgeSequence,
    ) -> Result<BackendIngressRecorder, BackendIngressError> {
        if let Some(active) = self.active {
            return Err(BackendIngressError::ProviderAlreadyActive { active });
        }
        let recorder = BackendIngressRecorder::new(
            platform,
            pointer,
            presentation_host,
            pointer_committed_through,
        )?;
        let lease = recorder.lease();
        if lease.authority_domain() != self.authority_domain {
            return Err(BackendIngressError::ForeignAuthorityDomain {
                expected: self.authority_domain,
                submitted: lease.authority_domain(),
            });
        }
        self.active = Some(lease);
        self.committed_through = BackendIngressOrdinal::ORIGIN;
        self.committed_record_identity = BackendIngressRecordIdentity::ORIGIN;
        Ok(recorder)
    }

    pub(crate) fn revoke(&mut self, lease: BackendIngressLease) -> Result<(), BackendIngressError> {
        let Some(active) = self.active else {
            return Err(BackendIngressError::ProviderUnavailable);
        };
        if active != lease {
            return Err(BackendIngressError::ProviderLeaseMismatch {
                expected: active,
                submitted: lease,
            });
        }
        self.active = None;
        self.committed_through = BackendIngressOrdinal::ORIGIN;
        self.committed_record_identity = BackendIngressRecordIdentity::ORIGIN;
        Ok(())
    }

    pub(crate) fn validate_batch(
        &self,
        batch: &BackendIngressBatch,
    ) -> Result<(), BackendIngressError> {
        let active = self
            .active
            .ok_or(BackendIngressError::ProviderUnavailable)?;
        batch.validate_against(
            active,
            self.committed_through,
            self.committed_record_identity,
        )
    }

    pub(crate) fn commit_batch(
        &mut self,
        batch: &BackendIngressBatch,
    ) -> Result<(), BackendIngressError> {
        self.validate_batch(batch)?;
        if let Some(record) = batch.records.last() {
            self.committed_record_identity = record.identity;
        }
        self.committed_through = batch.through();
        Ok(())
    }

    pub(crate) fn commit_watermark(&self) -> Option<BackendIngressCommitWatermark> {
        self.active.map(|lease| {
            BackendIngressCommitWatermark::new(
                lease,
                self.committed_through,
                self.committed_record_identity,
            )
        })
    }

    pub(crate) fn commit_prefix_retirement(
        &mut self,
        receipt: &BackendIngressPrefixRetirementReceipt,
    ) -> Result<(), BackendIngressError> {
        let authority = receipt.authority()?;
        let active = self
            .active
            .ok_or(BackendIngressError::ProviderUnavailable)?;
        if authority.lease != active {
            return Err(BackendIngressError::PrefixRetirementLeaseMismatch {
                expected: active,
                submitted: authority.lease,
            });
        }
        if authority.through != self.committed_through {
            return Err(BackendIngressError::PrefixRetirementWatermarkMismatch {
                committed: self.committed_through,
                submitted: authority.through,
            });
        }
        Ok(())
    }
}

/// Structural rejection while capturing or validating backend ingress.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum BackendIngressError {
    /// A joined replacement attempted to reuse an already transferred drain proof.
    #[error("backend ingress drain receipt was already consumed")]
    DrainReceiptConsumed,
    /// One affine prefix-retirement proof was submitted after successful settlement.
    #[error("backend ingress prefix-retirement receipt was already consumed")]
    PrefixRetirementReceiptConsumed,
    /// A successfully committed provider handoff ticket was submitted again.
    #[error("backend ingress provider replacement ticket was already consumed")]
    ProviderReplacementTicketConsumed,
    /// A joined provider replacement is already pending and must be finished,
    /// reissued, or explicitly reaped before another handoff can begin.
    #[error("backend ingress provider replacement is already pending")]
    ProviderReplacementPending,
    /// No joined provider replacement is currently pending.
    #[error("backend ingress provider replacement is not pending")]
    ProviderReplacementNotPending,
    /// A replacement ticket does not name the exact core-owned handoff.
    #[error("backend ingress provider replacement ticket is not the pending handoff")]
    UnknownReplacementTicket,
    /// The core-owned replacement identity counter cannot advance.
    #[error("backend ingress provider replacement identity counter is exhausted")]
    ReplacementIdentityExhausted,
    /// The ticket generation for one pending handoff cannot advance.
    #[error("backend ingress provider replacement ticket generation is exhausted")]
    ReplacementTicketGenerationExhausted,
    /// Another joined backend provider is already active.
    #[error("backend ingress provider {active:?} is already active")]
    ProviderAlreadyActive {
        /// Exact currently active provider pair.
        active: BackendIngressLease,
    },
    /// No joined backend provider currently owns ingress authority.
    #[error("backend ingress provider is unavailable")]
    ProviderUnavailable,
    /// A binding-quiescence record belongs to another engine authority domain.
    #[error("binding quiescence belongs to authority domain {submitted:?}, expected {expected:?}")]
    BindingQuiescenceAuthorityMismatch {
        /// Engine authority owned by the backend recorder.
        expected: EngineAuthorityDomainId,
        /// Engine authority carried by the binding.
        submitted: EngineAuthorityDomainId,
    },
    /// One unretired recorder prefix repeated a binding-quiescence assertion.
    #[error("backend ingress already recorded quiescence for binding {binding:?}")]
    DuplicateBindingQuiescence {
        /// Exact binding repeated by the producer.
        binding: ViewportBinding,
    },
    /// An operation named a provider other than the exact active pair.
    #[error("backend ingress provider {submitted:?} does not match active provider {expected:?}")]
    ProviderLeaseMismatch {
        /// Exact currently active provider pair.
        expected: BackendIngressLease,
        /// Provider pair supplied by the caller.
        submitted: BackendIngressLease,
    },
    /// A recorder rollback boundary belongs to another joined provider.
    #[error("backend ingress savepoint lease {submitted:?} does not match {expected:?}")]
    SavepointLeaseMismatch {
        /// Recorder-owned exact provider pair.
        expected: BackendIngressLease,
        /// Provider pair carried by the savepoint.
        submitted: BackendIngressLease,
    },
    /// A recorder prefix was reclaimed after the rollback boundary was minted.
    #[error("backend ingress savepoint prefix {saved:?} was reclaimed through {current:?}")]
    SavepointPrefixReclaimed {
        /// Reclaimed watermark captured by the savepoint.
        saved: BackendIngressOrdinal,
        /// Recorder's current reclaimed watermark.
        current: BackendIngressOrdinal,
    },
    /// Recorder state no longer contains the savepoint's complete prefix.
    #[error("backend ingress savepoint {saved:?} is ahead of current recorder state {current:?}")]
    SavepointAhead {
        /// Last ordinal captured by the savepoint.
        saved: BackendIngressOrdinal,
        /// Recorder's current last ordinal.
        current: BackendIngressOrdinal,
    },
    /// Public ordinals were reused after an outer rollback, so this savepoint no longer
    /// authenticates the exact recorder prefix it originally observed.
    #[error("backend ingress savepoint prefix through {boundary:?} was rewritten")]
    SavepointPrefixRewritten {
        /// Public ordinal at the stale savepoint boundary.
        boundary: BackendIngressOrdinal,
    },
    /// A presentation record named a host other than the one bound to this backend lifetime.
    #[error("backend presentation host {submitted:?} does not match bound host {expected:?}")]
    PresentationHostLeaseMismatch {
        /// Host frozen when the joined backend provider was enrolled.
        expected: PresentationHostLease,
        /// Host carried by the rejected record.
        submitted: PresentationHostLease,
    },
    /// The joined provider pair belongs to another engine authority domain.
    #[error("backend ingress belongs to authority domain {submitted:?}, expected {expected:?}")]
    ForeignAuthorityDomain {
        /// Engine authority which owns this ledger.
        expected: EngineAuthorityDomainId,
        /// Authority embedded in the submitted pair.
        submitted: EngineAuthorityDomainId,
    },
    /// The paired providers belong to different engine authority domains.
    #[error("platform ingress belongs to {platform:?}, but pointer ingress belongs to {pointer:?}")]
    AuthorityDomainMismatch {
        /// Platform provider authority domain.
        platform: EngineAuthorityDomainId,
        /// Pointer provider authority domain.
        pointer: EngineAuthorityDomainId,
    },
    /// The bound presentation host belongs to another engine authority domain.
    #[error(
        "presentation host belongs to {presentation:?}, but backend ingress belongs to {backend:?}"
    )]
    PresentationAuthorityDomainMismatch {
        /// Authority domain carried by the joined platform and pointer providers.
        backend: EngineAuthorityDomainId,
        /// Authority domain carried by the presentation host.
        presentation: EngineAuthorityDomainId,
    },
    /// Unified backend ingress requires a desktop-global pointer provider.
    #[error("backend ingress pointer provider {provider:?} is not desktop-global")]
    PointerProviderNotDesktopGlobal {
        /// Rejected exact pointer-provider incarnation.
        provider: PointerInputLease,
    },
    /// The backend capture ordinal cannot advance without wrapping.
    #[error("backend ingress ordinal is exhausted")]
    OrdinalExhausted,
    /// The non-reused append identity cannot advance without wrapping.
    #[error("backend ingress record identity is exhausted")]
    RecordIdentityExhausted,
    /// Recorder rollback or drain revoked a record retained by an older batch.
    #[error("backend ingress record {ordinal:?} was revoked before core publication")]
    RecordRevoked {
        /// Exact provider-local position whose append branch was abandoned.
        ordinal: BackendIngressOrdinal,
    },
    /// Another publication currently owns the exact record under the recorder lock.
    #[error("backend ingress record {ordinal:?} is already being published")]
    RecordPublicationInProgress {
        /// Exact provider-local position whose publication has started.
        ordinal: BackendIngressOrdinal,
    },
    /// The record was already published by a successful host-frame commit.
    #[error("backend ingress record {ordinal:?} was already committed")]
    RecordAlreadyCommitted {
        /// Exact provider-local position already owned by the committed engine.
        ordinal: BackendIngressOrdinal,
    },
    /// Recorder rollback cannot erase input already published to the engine.
    #[error("backend rollback would cross published record {ordinal:?}")]
    RollbackCrossesPublishedRecord {
        /// First suffix record whose publication started or completed.
        ordinal: BackendIngressOrdinal,
    },
    /// A pointer segment contained more than one receiver-bearing edge.
    #[error("backend ingress pointer segment contains {edges} edges; at most one is allowed")]
    PointerSegmentMustBeEdgewise {
        /// Number of edges in the rejected segment.
        edges: usize,
    },
    /// A pointer segment did not continue from the recorder's exact lane watermark.
    #[error(
        "backend ingress pointer segment starts at {submitted}, expected committed watermark {expected}"
    )]
    PointerSegmentPreviousMismatch {
        /// Recorder-owned pointer watermark.
        expected: PointerEdgeSequence,
        /// Segment's exclusive lower pointer watermark.
        submitted: PointerEdgeSequence,
    },
    /// A platform snapshot or effect result was submitted through the semantic lane.
    #[error("backend platform facts must use their typed ingress lanes")]
    SemanticInputIsBackendFact,
    /// A terminal configuration update was submitted inside the causal input prefix.
    #[error("backend configuration input belongs to the terminal configuration phase")]
    SemanticInputIsConfiguration,
    /// A requested suffix starts after the latest captured record.
    #[error(
        "backend ingress committed watermark {committed:?} is ahead of recorded watermark {recorded:?}"
    )]
    CommittedWatermarkAhead {
        /// Watermark supplied by the caller.
        committed: BackendIngressOrdinal,
        /// Latest recorder-owned ordinal.
        recorded: BackendIngressOrdinal,
    },
    /// A requested retry prefix has already been reclaimed from this recorder.
    #[error(
        "backend ingress committed watermark {committed:?} predates reclaimed watermark {retired:?}"
    )]
    CommittedWatermarkRetired {
        /// Requested historical lower bound.
        committed: BackendIngressOrdinal,
        /// Oldest lower bound still retained by the recorder.
        retired: BackendIngressOrdinal,
    },
    /// A commit proof belongs to another joined provider incarnation.
    #[error("backend commit watermark lease {submitted:?} does not match {expected:?}")]
    CommitWatermarkLeaseMismatch {
        /// Recorder-owned exact provider pair.
        expected: BackendIngressLease,
        /// Provider pair carried by the core proof.
        submitted: BackendIngressLease,
    },
    /// Recorder contents at a committed ordinal differ from the immutable batch core accepted.
    #[error("backend commit watermark content no longer matches record {ordinal:?}")]
    CommitWatermarkContentMismatch {
        /// Public capture position whose non-reused identity changed after rollback.
        ordinal: BackendIngressOrdinal,
    },
    /// Prefix reclamation attempted to move behind an already reclaimed position.
    #[error("backend retirement watermark {submitted:?} predates reclaimed watermark {retired:?}")]
    RetirementWatermarkRegressed {
        /// Current reclaimed lower bound.
        retired: BackendIngressOrdinal,
        /// Regressive proof position.
        submitted: BackendIngressOrdinal,
    },
    /// A prefix-retirement proof belongs to another active joined provider.
    #[error("prefix-retirement lease {submitted:?} does not match active lease {expected:?}")]
    PrefixRetirementLeaseMismatch {
        /// Current core-owned backend lease.
        expected: BackendIngressLease,
        /// Recorder lease carried by the proof.
        submitted: BackendIngressLease,
    },
    /// A prefix-retirement proof does not end at the current core commit boundary.
    ///
    /// Exact equality is the causal barrier between permanent producer
    /// quiescence and guard reclamation. It prevents a later committed fact from
    /// crossing an older closure proof before that proof is settled.
    #[error(
        "prefix-retirement proof reaches {submitted:?}, but core is committed through {committed:?}"
    )]
    PrefixRetirementWatermarkMismatch {
        /// Last backend ordinal committed by a complete host frame.
        committed: BackendIngressOrdinal,
        /// Inclusive upper watermark carried by the proof.
        submitted: BackendIngressOrdinal,
    },
    /// The batch belongs to a different exact provider pair.
    #[error("backend ingress batch lease {submitted:?} does not match {expected:?}")]
    BatchLeaseMismatch {
        /// Engine-owned current lease.
        expected: BackendIngressLease,
        /// Lease carried by the batch.
        submitted: BackendIngressLease,
    },
    /// The batch does not begin at the engine's committed ingress watermark.
    #[error(
        "backend ingress batch starts at {submitted:?}, expected committed watermark {expected:?}"
    )]
    BatchPreviousMismatch {
        /// Engine-owned committed watermark.
        expected: BackendIngressOrdinal,
        /// Batch's exclusive lower watermark.
        submitted: BackendIngressOrdinal,
    },
    /// The batch follows a rewritten record at the public committed ordinal.
    #[error(
        "backend ingress batch predecessor identity does not match committed record {ordinal:?}"
    )]
    BatchPreviousIdentityMismatch {
        /// Public capture position whose predecessor identity diverged.
        ordinal: BackendIngressOrdinal,
    },
    /// One record was spliced from another provider pair.
    #[error(
        "backend ingress record {ordinal:?} carries lease {submitted:?}, expected {expected:?}"
    )]
    RecordLeaseMismatch {
        /// Position of the rejected record.
        ordinal: BackendIngressOrdinal,
        /// Batch-owned exact provider pair.
        expected: BackendIngressLease,
        /// Record-owned exact provider pair.
        submitted: BackendIngressLease,
    },
    /// Records are duplicated, missing, or reordered.
    #[error("backend ingress record ordinal {submitted:?} is not contiguous after {expected:?}")]
    RecordOrdinalGap {
        /// Sole acceptable next ordinal.
        expected: BackendIngressOrdinal,
        /// Rejected record ordinal.
        submitted: BackendIngressOrdinal,
    },
    /// The batch header does not name its last contiguous record.
    #[error("backend ingress batch through {submitted:?} does not match final record {expected:?}")]
    BatchThroughMismatch {
        /// Last contiguous record, or the lower watermark for an empty batch.
        expected: BackendIngressOrdinal,
        /// Inclusive upper watermark supplied by the batch header.
        submitted: BackendIngressOrdinal,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::effect::{DispatchFailureReason, EffectDispatchResult, EffectId};
    use crate::intent::{Authority, AuthorityUnavailableReason, PointerId};
    use crate::platform::{
        CapabilityRosterObservation, CloseEffectAcknowledgement, WindowCloseObservation,
        WindowCloseState, WindowInventoryObservation, WorkAreaRosterObservation,
    };
    use crate::platform_provider::PlatformObservationAuthority;
    use crate::pointer_journal::{
        DesktopRouteFact, PointerCaptureOwner, PointerEdge, PointerEdgeKind, PointerEdgeLocation,
        SurfaceLocalPointerEndpoint, SurfaceLocalPointerScope,
    };
    use crate::presentation_observation::PresentationLedger;
    use crate::viewport::{
        CapabilityObservationGeneration, CloseObservationGeneration,
        InventoryObservationGeneration, PlatformSnapshotGeneration, WindowIncarnation, WindowToken,
        WorkAreaObservationGeneration,
    };
    use crate::viewport_focus::{FocusObservationEnvelope, FocusObservationGeneration};
    use crate::{ids::SurfaceId, ids::WorkspaceEpoch};

    fn platform_provider(domain: EngineAuthorityDomainId) -> PlatformObservationLease {
        let mut authority = PlatformObservationAuthority::new(domain);
        authority
            .create()
            .expect("test platform provider must mint")
    }

    fn desktop_pointer_provider(
        domain: EngineAuthorityDomainId,
        incarnation: u64,
    ) -> PointerInputLease {
        PointerInputLease::new(domain, incarnation, PointerProviderScope::DesktopGlobal)
    }

    fn presentation_host(domain: EngineAuthorityDomainId) -> PresentationHostLease {
        let mut ledger = PresentationLedger::new(domain);
        ledger
            .create_host()
            .expect("test presentation host must mint")
    }

    fn recorder(domain: u64) -> BackendIngressRecorder {
        let domain = EngineAuthorityDomainId::new_for_test(domain);
        BackendIngressRecorder::new(
            platform_provider(domain),
            desktop_pointer_provider(domain, 1),
            presentation_host(domain),
            PointerEdgeSequence::new(0),
        )
        .expect("matching desktop providers must create a recorder")
    }

    fn binding(domain: u64, token: u64) -> ViewportBinding {
        ViewportBinding::new(
            EngineAuthorityDomainId::new_for_test(domain),
            WorkspaceEpoch::new(0),
            SurfaceId::new(token),
            WindowToken::new(token),
            WindowIncarnation::new(1),
        )
    }

    fn authority_and_recorder(domain: u64) -> (BackendIngressAuthority, BackendIngressRecorder) {
        let domain = EngineAuthorityDomainId::new_for_test(domain);
        let mut authority = BackendIngressAuthority::new(domain);
        let recorder = authority
            .enroll(
                platform_provider(domain),
                desktop_pointer_provider(domain, 1),
                presentation_host(domain),
                PointerEdgeSequence::new(0),
            )
            .expect("matching backend authority must enroll");
        (authority, recorder)
    }

    fn snapshot(generation: u64) -> PlatformSnapshot {
        let reason = AuthorityUnavailableReason::NotReported;
        PlatformSnapshot::new(
            PlatformSnapshotGeneration::new(generation),
            CapabilityRosterObservation::unknown(
                CapabilityObservationGeneration::new(generation),
                reason,
            ),
            FocusObservationEnvelope::new(
                FocusObservationGeneration::new(generation),
                Authority::Unknown(reason),
                Authority::Unknown(reason),
            ),
            WindowInventoryObservation::unknown(
                InventoryObservationGeneration::new(generation),
                reason,
            ),
            Vec::new(),
            Vec::new(),
            WorkAreaRosterObservation::unknown(
                WorkAreaObservationGeneration::new(generation),
                reason,
            ),
        )
        .expect("unknown platform facts form a canonical snapshot")
    }

    fn effect_result(effect: u64) -> EffectResult {
        EffectResult::new(
            EffectId::new(effect),
            WorkspaceEpoch::new(0),
            EffectDispatchResult::DispatchFailed(DispatchFailureReason::AdapterRejected),
        )
    }

    fn pointer_segment(previous: u64) -> PointerEdgeJournal {
        let previous = PointerEdgeSequence::new(previous);
        let through = previous
            .checked_next()
            .expect("test pointer sequence must advance");
        let reason = AuthorityUnavailableReason::NotReported;
        PointerEdgeJournal::new(
            previous,
            through,
            vec![PointerEdge::new(
                through,
                PointerId::new(1),
                PointerEdgeKind::Moved,
                PointerEdgeLocation::Desktop {
                    route: DesktopRouteFact::unknown(Authority::Unknown(reason), reason),
                },
                Authority::Known(PointerCaptureOwner::None),
            )],
        )
        .expect("test pointer segment must be contiguous")
    }

    #[test]
    fn recorder_binds_exact_matching_desktop_provider_incarnations() {
        let domain = EngineAuthorityDomainId::new_for_test(1);
        let recorder = BackendIngressRecorder::new(
            platform_provider(domain),
            desktop_pointer_provider(domain, 7),
            presentation_host(domain),
            PointerEdgeSequence::new(3),
        )
        .expect("matching providers must be admitted");

        assert_eq!(recorder.lease().authority_domain(), domain);
        assert_eq!(recorder.lease().platform_incarnation(), 1);
        assert_eq!(recorder.lease().pointer_incarnation(), 7);
    }

    #[test]
    fn recorder_rejects_foreign_or_surface_local_pointer_provider() {
        let first = EngineAuthorityDomainId::new_for_test(1);
        let second = EngineAuthorityDomainId::new_for_test(2);
        let platform = platform_provider(first);
        let foreign = desktop_pointer_provider(second, 1);
        assert_eq!(
            BackendIngressRecorder::new(
                platform,
                foreign,
                presentation_host(first),
                PointerEdgeSequence::new(0),
            )
            .expect_err("foreign pointer authority must fail"),
            BackendIngressError::AuthorityDomainMismatch {
                platform: first,
                pointer: second,
            }
        );

        let mut presentations = PresentationLedger::new(first);
        let host = presentations
            .create_host()
            .expect("test presentation host must mint");
        let local = PointerInputLease::new(
            first,
            2,
            PointerProviderScope::SurfaceLocal(SurfaceLocalPointerScope::new(
                host,
                SurfaceLocalPointerEndpoint::Logical(SurfaceId::new(1)),
            )),
        );
        assert_eq!(
            BackendIngressRecorder::new(platform, local, host, PointerEdgeSequence::new(0),)
                .expect_err("surface-local pointer authority must fail"),
            BackendIngressError::PointerProviderNotDesktopGlobal { provider: local }
        );
    }

    #[test]
    fn recorder_mints_one_contiguous_order_across_all_payload_lanes() {
        let mut recorder = recorder(1);
        let platform = recorder
            .record_platform_snapshot(WorkspaceEpoch::new(0), snapshot(1))
            .expect("platform record must fit");
        let effect = recorder
            .record_platform_effect_result(effect_result(1))
            .expect("effect record must fit");
        let close = recorder
            .record_native_close_observation(
                WorkspaceEpoch::new(0),
                WindowCloseObservation::new(
                    crate::viewport::ViewportBinding::new(
                        EngineAuthorityDomainId::new_for_test(1),
                        WorkspaceEpoch::new(0),
                        SurfaceId::new(1),
                        WindowToken::new(1),
                        WindowIncarnation::new(1),
                    ),
                    CloseObservationGeneration::new(1),
                    Authority::Known(WindowCloseState::LiveRequested),
                    CloseEffectAcknowledgement::known(None),
                ),
            )
            .expect("close record must fit");
        let semantic = recorder
            .record_semantic_input(EngineInput::ValidateWorkspace)
            .expect("semantic record must fit");
        let pointer = recorder
            .record_pointer_segment(pointer_segment(0))
            .expect("pointer record must fit");

        assert_eq!(
            [
                platform.get(),
                effect.get(),
                close.get(),
                semantic.get(),
                pointer.get(),
            ],
            [1, 2, 3, 4, 5],
        );
        let batch = recorder
            .pending_batch()
            .expect("recorder-owned batch must validate");
        assert_eq!(batch.previous().get(), 0);
        assert_eq!(batch.through().get(), 5);
        assert_eq!(
            batch
                .records()
                .iter()
                .map(|record| record.ordinal().get())
                .collect::<Vec<_>>(),
            vec![1, 2, 3, 4, 5]
        );
        assert!(matches!(
            batch.records()[0].payload(),
            BackendIngressPayload::PlatformSnapshot { .. }
        ));
        assert!(matches!(
            batch.records()[1].payload(),
            BackendIngressPayload::PlatformEffectResult(_)
        ));
        assert!(matches!(
            batch.records()[2].payload(),
            BackendIngressPayload::NativeCloseObservation { .. }
        ));
        assert!(matches!(
            batch.records()[3].payload(),
            BackendIngressPayload::SemanticInput(EngineInput::ValidateWorkspace)
        ));
        assert!(matches!(
            batch.records()[4].payload(),
            BackendIngressPayload::PointerSegment(_)
        ));
    }

    #[test]
    fn draining_the_single_writer_freezes_its_final_backend_and_pointer_frontiers() {
        let mut recorder = recorder(1);
        let lease = recorder.lease();
        recorder
            .record_semantic_input(EngineInput::ValidateWorkspace)
            .expect("semantic record must fit");
        recorder
            .record_pointer_segment(pointer_segment(0))
            .expect("pointer record must fit");

        let receipt = recorder.drain();

        assert_eq!(receipt.lease(), lease);
        assert_eq!(receipt.recorded_through(), BackendIngressOrdinal(2));
        assert_eq!(receipt.pointer_through(), PointerEdgeSequence::new(1));
    }

    #[test]
    fn rejected_pointer_gap_does_not_consume_ingress_or_pointer_watermarks() {
        let mut recorder = recorder(1);
        assert_eq!(
            recorder
                .record_pointer_segment(pointer_segment(1))
                .expect_err("pointer gap must fail"),
            BackendIngressError::PointerSegmentPreviousMismatch {
                expected: PointerEdgeSequence::new(0),
                submitted: PointerEdgeSequence::new(1),
            }
        );
        assert_eq!(recorder.recorded_through().get(), 0);
        assert!(
            recorder
                .pending_batch()
                .expect("empty recorder-owned batch must validate")
                .is_empty()
        );

        assert_eq!(
            recorder
                .record_pointer_segment(pointer_segment(0))
                .expect("retry from the retained pointer watermark must work")
                .get(),
            1
        );
    }

    #[test]
    fn savepoint_rolls_back_records_and_the_pointer_lane_atomically() {
        let mut recorder = recorder(1);
        recorder
            .record_semantic_input(EngineInput::ValidateWorkspace)
            .expect("the prefix must fit");
        let savepoint = recorder.savepoint();
        recorder
            .record_pointer_segment(pointer_segment(0))
            .expect("the transaction pointer edge must fit");
        recorder
            .record_platform_effect_result(effect_result(1))
            .expect("the transaction effect must fit");

        recorder
            .rollback_to(savepoint)
            .expect("the unchanged recorder prefix is rollback-safe");

        let batch = recorder
            .pending_batch()
            .expect("the restored prefix remains a valid batch");
        assert_eq!(batch.through().get(), 1);
        assert_eq!(batch.len(), 1);
        assert_eq!(
            recorder
                .record_pointer_segment(pointer_segment(0))
                .expect("the pointer watermark must also roll back")
                .get(),
            2
        );
    }

    #[test]
    fn stale_nested_savepoint_cannot_accept_a_rewritten_public_ordinal() {
        let mut recorder = recorder(1);
        let outer = recorder.savepoint();
        let abandoned = recorder
            .record_semantic_input_receipt(EngineInput::ValidateWorkspace)
            .expect("the first branch record must fit");
        let stale_nested = recorder.savepoint();

        recorder
            .rollback_to(outer)
            .expect("the outer boundary must abandon the first branch");
        assert!(!abandoned.is_live());
        let replacement = recorder
            .record_semantic_input_receipt(EngineInput::ValidateWorkspace)
            .expect("the replacement may reuse only the public ordinal");
        assert_eq!(abandoned.ordinal(), replacement.ordinal());

        assert_eq!(
            recorder
                .rollback_to(stale_nested)
                .expect_err("a stale nested boundary cannot authenticate the rewritten branch"),
            BackendIngressError::SavepointPrefixRewritten {
                boundary: BackendIngressOrdinal(1),
            }
        );
        assert!(replacement.is_live());
        assert_eq!(recorder.retained_record_count(), 1);
    }

    #[test]
    fn immutable_batch_is_exactly_retryable_and_validates_commit_authority() {
        let mut recorder = recorder(1);
        recorder
            .record_platform_snapshot(WorkspaceEpoch::new(0), snapshot(1))
            .expect("platform record must fit");
        recorder
            .record_pointer_segment(pointer_segment(0))
            .expect("pointer record must fit");

        let first = recorder
            .batch_after(BackendIngressOrdinal::ORIGIN)
            .expect("origin suffix must exist");
        let retry = recorder
            .batch_after(BackendIngressOrdinal::ORIGIN)
            .expect("retry must preserve the same batch");
        assert_eq!(retry, first);
        assert_eq!(
            first.validate_against(
                recorder.lease(),
                BackendIngressOrdinal::ORIGIN,
                BackendIngressRecordIdentity::ORIGIN,
            ),
            Ok(())
        );
        assert_eq!(
            first
                .validate_against(
                    recorder.lease(),
                    BackendIngressOrdinal(1),
                    BackendIngressRecordIdentity::ORIGIN,
                )
                .expect_err("a different engine watermark must fail"),
            BackendIngressError::BatchPreviousMismatch {
                expected: BackendIngressOrdinal(1),
                submitted: BackendIngressOrdinal::ORIGIN,
            }
        );
    }

    #[test]
    fn batch_validation_rejects_reordering_and_cross_lease_splicing() {
        let mut first_recorder = recorder(1);
        first_recorder
            .record_platform_snapshot(WorkspaceEpoch::new(0), snapshot(1))
            .expect("first record must fit");
        first_recorder
            .record_platform_effect_result(effect_result(1))
            .expect("second record must fit");
        let first = first_recorder
            .pending_batch()
            .expect("first recorder-owned batch must validate");

        let mut reordered = first.records.clone();
        reordered.swap(0, 1);
        assert_eq!(
            BackendIngressBatch::from_records(
                first.coordination.clone(),
                first.lease,
                first.previous,
                first.previous_record_identity,
                first.through,
                reordered,
            )
            .expect_err("record reordering must fail"),
            BackendIngressError::RecordOrdinalGap {
                expected: BackendIngressOrdinal(1),
                submitted: BackendIngressOrdinal(2),
            }
        );

        let mut second_recorder = recorder(2);
        second_recorder
            .record_platform_snapshot(WorkspaceEpoch::new(0), snapshot(1))
            .expect("foreign record must fit its own recorder");
        let mut spliced = first.records.clone();
        spliced[0] = second_recorder
            .pending_batch()
            .expect("second recorder-owned batch must validate")
            .records[0]
            .clone();
        assert!(matches!(
            BackendIngressBatch::from_records(
                first.coordination.clone(),
                first.lease,
                first.previous,
                first.previous_record_identity,
                first.through,
                spliced,
            ),
            Err(BackendIngressError::RecordLeaseMismatch { ordinal, .. })
                if ordinal == BackendIngressOrdinal(1)
        ));
    }

    #[test]
    fn committed_prefix_retirement_mints_one_affine_binding_quiescence_proof() {
        let (mut authority, mut recorder) = authority_and_recorder(1);
        let retired_binding = binding(1, 7);
        recorder
            .record_semantic_input(EngineInput::ValidateWorkspace)
            .expect("prefix semantic input must fit");
        recorder
            .record_platform_binding_quiescence(retired_binding)
            .expect("binding quiescence must fit");
        let before_reclaim = recorder.savepoint();
        let batch = recorder.pending_batch().expect("prefix must freeze");
        authority
            .commit_batch(&batch)
            .expect("core authority must commit the exact prefix");
        let watermark = authority
            .commit_watermark()
            .expect("the committed authority must expose its exact watermark");
        let mut receipt = recorder
            .retire_committed_prefix(watermark)
            .expect("committed prefix must be reclaimable")
            .expect("an advancing prefix must mint a receipt");

        assert_eq!(receipt.through(), Some(BackendIngressOrdinal(2)));
        assert_eq!(receipt.binding_quiescences(), &[retired_binding]);
        assert_eq!(
            recorder
                .rollback_to(before_reclaim)
                .expect_err("a reclaimed prefix invalidates older savepoints"),
            BackendIngressError::SavepointPrefixReclaimed {
                saved: BackendIngressOrdinal::ORIGIN,
                current: BackendIngressOrdinal(2),
            }
        );

        authority
            .commit_prefix_retirement(&receipt)
            .expect("the same core authority must accept its reclaimed prefix");
        receipt.consume().expect("the proof is affine");
        assert_eq!(
            authority
                .commit_prefix_retirement(&receipt)
                .expect_err("a consumed proof cannot replay"),
            BackendIngressError::PrefixRetirementReceiptConsumed,
        );
    }

    #[test]
    fn prefix_retirement_receipt_cannot_cross_a_later_committed_fact() {
        let (mut authority, mut recorder) = authority_and_recorder(1);
        recorder
            .record_platform_binding_quiescence(binding(1, 1))
            .expect("first quiescence must fit");
        let first_batch = recorder.pending_batch().expect("first prefix must freeze");
        authority
            .commit_batch(&first_batch)
            .expect("first prefix must commit");
        let first_watermark = authority
            .commit_watermark()
            .expect("the first commit must expose its exact watermark");
        let first = recorder
            .retire_committed_prefix(first_watermark)
            .expect("first prefix must reclaim")
            .expect("first prefix must mint a receipt");

        recorder
            .record_platform_binding_quiescence(binding(1, 2))
            .expect("second quiescence must fit");
        let second_batch = recorder.pending_batch().expect("second prefix must freeze");
        authority
            .commit_batch(&second_batch)
            .expect("second prefix must commit");
        let second_watermark = authority
            .commit_watermark()
            .expect("the second commit must expose its exact watermark");
        let second = recorder
            .retire_committed_prefix(second_watermark)
            .expect("second prefix must reclaim")
            .expect("second prefix must mint a receipt");

        assert_eq!(
            authority
                .commit_prefix_retirement(&first)
                .expect_err("a later committed fact must invalidate the older barrier"),
            BackendIngressError::PrefixRetirementWatermarkMismatch {
                committed: BackendIngressOrdinal(2),
                submitted: BackendIngressOrdinal(1),
            },
        );
        authority
            .commit_prefix_retirement(&second)
            .expect("the proof ending at the exact current boundary remains valid");
    }

    #[test]
    fn committed_watermark_rejects_rollback_rewrite_at_the_same_ordinal() {
        let (mut authority, mut recorder) = authority_and_recorder(1);
        let savepoint = recorder.savepoint();
        recorder
            .record_semantic_input(EngineInput::ValidateWorkspace)
            .expect("the original record must fit");
        let committed = recorder
            .pending_batch()
            .expect("the original immutable batch must freeze");
        authority
            .commit_batch(&committed)
            .expect("core commits the original record identity");
        let watermark = authority
            .commit_watermark()
            .expect("the original commit must expose its exact identity");

        recorder
            .rollback_to(savepoint)
            .expect("the recorder may abandon its unretired branch");
        recorder
            .record_platform_binding_quiescence(binding(1, 9))
            .expect("rollback may reuse the public ordinal with a new record identity");

        assert_eq!(
            recorder
                .retire_committed_prefix(watermark)
                .expect_err("an old commit cannot authenticate rewritten quiescence"),
            BackendIngressError::CommitWatermarkContentMismatch {
                ordinal: BackendIngressOrdinal(1),
            },
        );
        assert_eq!(recorder.retained_record_count(), 1);
    }

    #[test]
    fn recorder_rollback_revokes_every_previously_frozen_batch() {
        let (authority, mut recorder) = authority_and_recorder(1);
        let savepoint = recorder.savepoint();
        recorder
            .record_semantic_input(EngineInput::ValidateWorkspace)
            .expect("the original record must fit");
        let frozen = recorder
            .pending_batch()
            .expect("the original immutable batch must freeze");

        recorder
            .rollback_to(savepoint)
            .expect("recorder rollback must revoke the abandoned append branch");

        assert_eq!(
            authority
                .validate_batch(&frozen)
                .expect_err("a frozen batch cannot outlive recorder rollback"),
            BackendIngressError::RecordRevoked {
                ordinal: BackendIngressOrdinal::from_committed_source_sequence(1),
            },
        );
    }

    #[test]
    fn rollback_preserves_a_frozen_prefix_and_revokes_its_frozen_suffix() {
        let (mut authority, mut recorder) = authority_and_recorder(1);
        recorder
            .record_semantic_input(EngineInput::ValidateWorkspace)
            .expect("the retained prefix record must fit");
        let prefix = recorder
            .pending_batch()
            .expect("the retained prefix must freeze");
        let savepoint = recorder.savepoint();
        recorder
            .record_semantic_input(EngineInput::ValidateWorkspace)
            .expect("the abandoned suffix record must fit");
        let full = recorder
            .pending_batch()
            .expect("the complete branch must freeze before rollback");

        recorder
            .rollback_to(savepoint)
            .expect("rollback must preserve the exact frozen prefix");

        authority
            .validate_batch(&prefix)
            .expect("the frozen retained prefix must remain publishable");
        assert_eq!(
            authority
                .validate_batch(&full)
                .expect_err("the frozen full batch must retain revoked suffix identity"),
            BackendIngressError::RecordRevoked {
                ordinal: BackendIngressOrdinal::from_committed_source_sequence(2),
            }
        );
        authority
            .commit_batch(&prefix)
            .expect("the retained prefix must still commit exactly once");
    }

    #[test]
    fn commit_guard_linearizes_publication_before_rollback() {
        let mut recorder = recorder(1);
        let savepoint = recorder.savepoint();
        recorder
            .record_semantic_input(EngineInput::ValidateWorkspace)
            .expect("the record must fit");
        let batch = recorder
            .pending_batch()
            .expect("the batch must freeze before publication");
        let coordination = batch.coordination.clone();

        let published = batch
            .commit_guard()
            .publish_with(|| {
                assert!(matches!(
                    coordination.0.try_lock(),
                    Err(std::sync::TryLockError::WouldBlock)
                ));
                Ok::<_, ()>(())
            })
            .expect("the guard must publish while holding coordination");
        assert_eq!(published, Ok(()));
        assert!(coordination.0.try_lock().is_ok());

        assert_eq!(
            recorder
                .rollback_to(savepoint)
                .expect_err("rollback must not erase a published record"),
            BackendIngressError::RollbackCrossesPublishedRecord {
                ordinal: BackendIngressOrdinal::from_committed_source_sequence(1),
            }
        );
        assert_eq!(recorder.retained_record_count(), 1);

        let committed_prefix = recorder.savepoint();
        recorder
            .record_semantic_input(EngineInput::ValidateWorkspace)
            .expect("a live suffix may follow the committed prefix");
        recorder
            .rollback_to(committed_prefix)
            .expect("rollback may still remove only the later live suffix");
        assert_eq!(recorder.retained_record_count(), 1);
    }

    #[test]
    fn failed_publication_restores_record_rollback_authority() {
        let mut recorder = recorder(1);
        let savepoint = recorder.savepoint();
        recorder
            .record_semantic_input(EngineInput::ValidateWorkspace)
            .expect("the record must fit");
        let batch = recorder
            .pending_batch()
            .expect("the batch must freeze before publication");

        let published = batch
            .commit_guard()
            .publish_with(|| Err::<(), _>("rejected"))
            .expect("the guard itself must remain valid");
        assert_eq!(published, Err("rejected"));
        recorder
            .rollback_to(savepoint)
            .expect("failed publication must restore rollback authority");
        assert_eq!(recorder.retained_record_count(), 0);
    }

    #[test]
    fn committed_predecessor_identity_rejects_a_rewritten_prefix_hidden_by_a_later_record() {
        let (mut authority, mut recorder) = authority_and_recorder(1);
        let savepoint = recorder.savepoint();
        recorder
            .record_semantic_input(EngineInput::ValidateWorkspace)
            .expect("the original record must fit");
        let original = recorder
            .pending_batch()
            .expect("the original immutable batch must freeze");
        let original_identity = original.records[0].identity;
        authority
            .commit_batch(&original)
            .expect("core commits the original predecessor identity");

        recorder
            .rollback_to(savepoint)
            .expect("the recorder may abandon its unretired branch");
        recorder
            .record_platform_binding_quiescence(binding(1, 9))
            .expect("the rewritten predecessor must fit");
        recorder
            .record_semantic_input(EngineInput::ValidateWorkspace)
            .expect("a later record must fit after the rewritten predecessor");
        let rewritten_suffix = recorder
            .batch_after(BackendIngressOrdinal(1))
            .expect("the recorder can expose the rewritten ordinal suffix");

        assert_ne!(rewritten_suffix.previous_record_identity, original_identity);
        assert_eq!(
            authority
                .validate_batch(&rewritten_suffix)
                .expect_err("the later record cannot hide a rewritten committed predecessor"),
            BackendIngressError::BatchPreviousIdentityMismatch {
                ordinal: BackendIngressOrdinal(1),
            },
        );
        let watermark = authority
            .commit_watermark()
            .expect("the rejected suffix must leave the original watermark intact");
        assert_eq!(watermark.through(), BackendIngressOrdinal(1));
        assert_eq!(
            recorder
                .retire_committed_prefix(watermark)
                .expect_err("rewritten quiescence must not mint a retirement receipt"),
            BackendIngressError::CommitWatermarkContentMismatch {
                ordinal: BackendIngressOrdinal(1),
            },
        );
        assert_eq!(recorder.retained_record_count(), 2);
    }

    #[test]
    fn ordinary_prefix_reclamation_needs_no_engine_retirement_receipt() {
        let (mut authority, mut recorder) = authority_and_recorder(1);
        recorder
            .record_semantic_input(EngineInput::ValidateWorkspace)
            .expect("ordinary ingress must fit");
        let batch = recorder
            .pending_batch()
            .expect("ordinary prefix must freeze");
        authority
            .commit_batch(&batch)
            .expect("ordinary prefix must commit");
        let watermark = authority
            .commit_watermark()
            .expect("the ordinary commit must expose its exact watermark");

        assert!(
            recorder
                .retire_committed_prefix(watermark)
                .expect("ordinary prefix must reclaim")
                .is_none(),
            "prefixes without binding closure must not force an engine candidate clone",
        );
        assert_eq!(recorder.retained_record_count(), 0);
    }

    #[test]
    fn binding_quiescence_rejects_foreign_authority_and_unretired_duplicates() {
        let mut recorder = recorder(1);
        assert_eq!(
            recorder
                .record_platform_binding_quiescence(binding(2, 1))
                .expect_err("foreign binding authority must fail"),
            BackendIngressError::BindingQuiescenceAuthorityMismatch {
                expected: EngineAuthorityDomainId::new_for_test(1),
                submitted: EngineAuthorityDomainId::new_for_test(2),
            }
        );
        let local = binding(1, 1);
        recorder
            .record_platform_binding_quiescence(local)
            .expect("local binding quiescence must fit");
        assert_eq!(
            recorder
                .record_platform_binding_quiescence(local)
                .expect_err("one retained prefix cannot repeat a lane closure"),
            BackendIngressError::DuplicateBindingQuiescence { binding: local },
        );
    }
}
