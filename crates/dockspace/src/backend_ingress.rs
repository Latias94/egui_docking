//! Backend-captured total ordering across platform and pointer ingress lanes.
//!
//! This module owns only transport structure. It does not reduce records or
//! advance engine authority. A backend recorder binds one exact platform
//! provider incarnation to one exact desktop-global pointer provider and mints
//! opaque ordinals as facts are captured. Immutable batches can then be retried
//! until a host-frame commit advances the engine-owned watermark.

use thiserror::Error;

use crate::effect::EffectResult;
use crate::engine::EngineInput;
use crate::ids::SurfaceId;
use crate::ids::{EngineAuthorityDomainId, WorkspaceEpoch};
use crate::platform::{PlatformSnapshot, WindowCloseObservation};
use crate::platform_provider::{PlatformObservationLease, PlatformProviderReplacementTicket};
use crate::pointer_journal::{
    PointerEdgeJournal, PointerEdgeSequence, PointerInputLease, PointerProviderScope,
};
use crate::presentation_observation::{HostPresentationObservationEntry, PresentationHostLease};
use crate::surface_recovery::{SurfaceRecoveryBootstrap, SurfaceRecoveryTarget};
use crate::transition::WorkspaceVersion;
use crate::viewport::{ViewportRole, WindowToken};

/// Opaque capture position in one backend ingress provider lifetime.
///
/// Ordinals are allocated only by [`BackendIngressRecorder`]. Callers may
/// retain and compare them, but cannot construct or advance them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(transparent)]
pub struct BackendIngressOrdinal(u64);

impl BackendIngressOrdinal {
    pub(crate) const ORIGIN: Self = Self(0);

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
}

/// Affine authority to finish one exact joined backend-provider handoff.
///
/// This type deliberately does not implement `Clone` or `Copy`, and it never
/// exposes the wrapped platform-only ticket. A joined replacement can therefore
/// activate the platform, desktop-pointer, and backend-order lanes only through
/// `DockEngine::finish_backend_ingress_provider_replacement`.
///
/// ```compile_fail
/// use dockspace::backend_ingress::BackendIngressProviderReplacementTicket;
/// use dockspace::engine::DockEngine;
///
/// fn cannot_finish_only_the_platform_lane(
///     engine: &mut DockEngine,
///     ticket: BackendIngressProviderReplacementTicket,
/// ) {
///     let _ = engine.finish_platform_provider_replacement(ticket);
/// }
/// ```
#[derive(Debug, PartialEq, Eq)]
pub struct BackendIngressProviderReplacementTicket {
    authority: Option<BackendIngressProviderReplacementAuthority>,
}

#[derive(Debug, PartialEq, Eq)]
struct BackendIngressProviderReplacementAuthority {
    platform: PlatformProviderReplacementTicket,
    predecessor: BackendIngressDrainReceipt,
}

impl BackendIngressProviderReplacementTicket {
    pub(crate) const fn new(
        platform: PlatformProviderReplacementTicket,
        predecessor: BackendIngressDrainReceipt,
    ) -> Self {
        Self {
            authority: Some(BackendIngressProviderReplacementAuthority {
                platform,
                predecessor,
            }),
        }
    }

    pub(crate) const fn authority(
        &self,
    ) -> Option<(
        PlatformProviderReplacementTicket,
        &BackendIngressDrainReceipt,
    )> {
        match &self.authority {
            Some(authority) => Some((authority.platform, &authority.predecessor)),
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
}

impl BackendIngressCommitWatermark {
    pub(crate) const fn new(lease: BackendIngressLease, through: BackendIngressOrdinal) -> Self {
        Self { lease, through }
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
    /// One exact binding-scoped native-close observation.
    NativeCloseObservation {
        /// Workspace epoch whose binding incarnation the backend observed.
        expected_epoch: WorkspaceEpoch,
        /// Exact close fact captured at this ingress position.
        observation: WindowCloseObservation,
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
    payload: BackendIngressPayload,
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

/// Immutable complete interval of one backend ingress stream.
///
/// Construction is private and validates exact lease equality plus a contiguous
/// `(previous, through]` ordinal interval. The value is cloneable and contains
/// no consumptive cursor, so a failed host-frame attempt may retry the same batch.
#[derive(Debug, Clone, PartialEq)]
pub struct BackendIngressBatch {
    lease: BackendIngressLease,
    previous: BackendIngressOrdinal,
    through: BackendIngressOrdinal,
    records: Vec<BackendIngressRecord>,
}

impl BackendIngressBatch {
    fn from_records(
        lease: BackendIngressLease,
        previous: BackendIngressOrdinal,
        through: BackendIngressOrdinal,
        records: Vec<BackendIngressRecord>,
    ) -> Result<Self, BackendIngressError> {
        let batch = Self {
            lease,
            previous,
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
    pub(crate) fn validate_against(
        &self,
        expected_lease: BackendIngressLease,
        committed_through: BackendIngressOrdinal,
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
        self.validate_shape()
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
}

/// Single-writer backend capture recorder.
///
/// The recorder is intentionally not cloneable. Calling its record methods in
/// the native event-loop callback order is the sole way to mint new ingress
/// ordinals. Producing a batch is non-destructive, which keeps every captured
/// interval retryable until the engine publishes its own commit watermark.
#[derive(Debug)]
pub struct BackendIngressRecorder {
    lease: BackendIngressLease,
    retired_through: BackendIngressOrdinal,
    last_ordinal: BackendIngressOrdinal,
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
            lease: BackendIngressLease::new(platform, pointer, presentation_host)?,
            retired_through: BackendIngressOrdinal::ORIGIN,
            last_ordinal: BackendIngressOrdinal::ORIGIN,
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

    /// Mints an affine rollback boundary for a fallible adapter append batch.
    #[must_use]
    pub fn savepoint(&self) -> BackendIngressSavepoint {
        BackendIngressSavepoint {
            lease: self.lease,
            retired_through: self.retired_through,
            recorded_through: self.last_ordinal,
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
        BackendIngressDrainReceipt {
            lease: self.lease,
            recorded_through: self.last_ordinal,
            pointer_through: self.pointer_through,
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
            self.lease,
            self.retired_through,
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
        let records = self
            .records
            .iter()
            .filter(|record| record.ordinal > committed_through)
            .cloned()
            .collect();
        BackendIngressBatch::from_records(self.lease, committed_through, self.last_ordinal, records)
    }

    /// Reclaims the exact prefix proven committed by the core.
    pub fn retire_committed_prefix(
        &mut self,
        committed: BackendIngressCommitWatermark,
    ) -> Result<(), BackendIngressError> {
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
        let retained = self
            .records
            .partition_point(|record| record.ordinal <= committed.through);
        self.records.drain(..retained);
        self.retired_through = committed.through;
        Ok(())
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
        self.records.push(BackendIngressRecord {
            lease: self.lease,
            ordinal,
            payload,
        });
        self.last_ordinal = ordinal;
        Ok(ordinal)
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
}

impl BackendIngressAuthority {
    pub(crate) const fn new(authority_domain: EngineAuthorityDomainId) -> Self {
        Self {
            authority_domain,
            active: None,
            committed_through: BackendIngressOrdinal::ORIGIN,
        }
    }

    pub(crate) const fn active(&self) -> Option<BackendIngressLease> {
        self.active
    }

    pub(crate) const fn committed_through(&self) -> BackendIngressOrdinal {
        self.committed_through
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
        Ok(())
    }

    pub(crate) fn validate_batch(
        &self,
        batch: &BackendIngressBatch,
    ) -> Result<(), BackendIngressError> {
        let active = self
            .active
            .ok_or(BackendIngressError::ProviderUnavailable)?;
        batch.validate_against(active, self.committed_through)
    }

    pub(crate) fn commit_batch(
        &mut self,
        batch: &BackendIngressBatch,
    ) -> Result<(), BackendIngressError> {
        self.validate_batch(batch)?;
        self.committed_through = batch.through();
        Ok(())
    }
}

/// Structural rejection while capturing or validating backend ingress.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum BackendIngressError {
    /// A successfully committed provider handoff ticket was submitted again.
    #[error("backend ingress provider replacement ticket was already consumed")]
    ProviderReplacementTicketConsumed,
    /// Another joined backend provider is already active.
    #[error("backend ingress provider {active:?} is already active")]
    ProviderAlreadyActive {
        /// Exact currently active provider pair.
        active: BackendIngressLease,
    },
    /// No joined backend provider currently owns ingress authority.
    #[error("backend ingress provider is unavailable")]
    ProviderUnavailable,
    /// An operation named a provider other than the exact active pair.
    #[error("backend ingress provider {submitted:?} does not match active provider {expected:?}")]
    ProviderLeaseMismatch {
        /// Exact currently active provider pair.
        expected: BackendIngressLease,
        /// Provider pair supplied by the caller.
        submitted: BackendIngressLease,
    },
    /// Platform-only replacement cannot revoke one lane of a joined backend provider.
    #[error(
        "platform-only replacement cannot revoke joined backend provider {active:?}; drain the backend recorder first"
    )]
    PlatformReplacementRequiresDrain {
        /// Exact joined provider whose affine producer must be drained.
        active: BackendIngressLease,
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
    /// Prefix reclamation attempted to move behind an already reclaimed position.
    #[error("backend retirement watermark {submitted:?} predates reclaimed watermark {retired:?}")]
    RetirementWatermarkRegressed {
        /// Current reclaimed lower bound.
        retired: BackendIngressOrdinal,
        /// Regressive proof position.
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
            first.validate_against(recorder.lease(), BackendIngressOrdinal::ORIGIN),
            Ok(())
        );
        assert_eq!(
            first
                .validate_against(recorder.lease(), BackendIngressOrdinal(1))
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
                first.lease,
                first.previous,
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
                first.lease,
                first.previous,
                first.through,
                spliced,
            ),
            Err(BackendIngressError::RecordLeaseMismatch { ordinal, .. })
                if ordinal == BackendIngressOrdinal(1)
        ));
    }
}
