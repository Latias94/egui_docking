//! Core-owned platform effect ledger.
//!
//! Dispatch acknowledgement is deliberately absent: an adapter call returning
//! successfully does not prove that a window exists, a flag changed, or a close
//! completed. Only matching authoritative observations can do that.

use std::collections::{BTreeMap, BTreeSet};

use thiserror::Error;

#[cfg(test)]
use crate::backend_ingress::BackendIngressDrainReceipt;
use crate::close_plan::{CloseRequestId, NativeCloseEdge};
use crate::geometry::PhysicalRect;
use crate::ids::WorkspaceEpoch;
use crate::platform_provider::PlatformObservationLease;
#[cfg(test)]
use crate::retention::EffectRetentionManifest;
use crate::viewport::{
    CloseObservationGeneration, InventoryGeneration, ViewportBinding, ViewportRole,
};

/// Monotonic identity of one exact platform side effect.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(transparent)]
pub struct EffectId(u64);

impl EffectId {
    #[must_use]
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }

    #[must_use]
    pub const fn checked_next(self) -> Option<Self> {
        match self.0.checked_add(1) {
            Some(value) => Some(Self(value)),
            None => None,
        }
    }
}

/// Requested resolution of one authoritative native close edge.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NativeCloseResolution {
    /// Accept the native close and allow the exact window incarnation to be destroyed.
    Accept,
    /// Cancel the native close and retain the exact window incarnation.
    Cancel,
}

/// Causal facts frozen when a native close effect is actually emitted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct NativeCloseEmissionFence {
    provider: PlatformObservationLease,
    binding: ViewportBinding,
    observed_through: CloseObservationGeneration,
    received_through: InventoryGeneration,
    after_effect: Option<EffectId>,
}

impl NativeCloseEmissionFence {
    /// Returns the exact platform provider which received this close resolution.
    #[must_use]
    pub const fn provider(self) -> PlatformObservationLease {
        self.provider
    }

    /// Returns the exact native window incarnation whose close edge authorized emission.
    #[must_use]
    pub const fn binding(self) -> ViewportBinding {
        self.binding
    }

    /// Returns the provider-owned close observation generation seen before emission.
    #[must_use]
    pub const fn observed_through(self) -> CloseObservationGeneration {
        self.observed_through
    }

    /// Returns the core inventory generation at which the effect was extracted.
    ///
    /// This is an emission barrier and may be newer than the edge's ingress generation.
    #[must_use]
    pub const fn received_through(self) -> InventoryGeneration {
        self.received_through
    }

    /// Returns the exact predecessor in this native close effect lane.
    #[must_use]
    pub const fn after_effect(self) -> Option<EffectId> {
        self.after_effect
    }
}

/// Immutable proof that one effect request was delivered to one exact provider.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct EffectDelivery {
    provider: PlatformObservationLease,
    inventory_generation: InventoryGeneration,
}

impl EffectDelivery {
    /// Returns the exact provider incarnation which received the request.
    #[must_use]
    pub const fn provider(self) -> PlatformObservationLease {
        self.provider
    }

    /// Returns the inventory generation at which the request was extracted.
    #[must_use]
    pub const fn inventory_generation(self) -> InventoryGeneration {
        self.inventory_generation
    }
}

/// Exact adapter operation requested by the core.
#[derive(Debug, Clone, PartialEq)]
pub enum PlatformEffect {
    /// Create one native window; the provider reports exact Prepared readiness.
    CreateWindow {
        binding: ViewportBinding,
        placement: PhysicalRect,
        role: ViewportRole,
    },
    CompensatingClose {
        binding: ViewportBinding,
        compensates: EffectId,
    },
    #[allow(
        dead_code,
        reason = "the lifecycle reducer retains its complete outcome vocabulary"
    )]
    CancelRootClose {
        binding: ViewportBinding,
    },
    #[allow(
        dead_code,
        reason = "the lifecycle reducer retains its complete outcome vocabulary"
    )]
    RetainChild {
        binding: ViewportBinding,
    },
    ReleaseChild {
        binding: ViewportBinding,
    },
    /// Continue observing one already-emitted destructive cleanup after an authority change.
    ///
    /// This is an observation-only protocol request. The provider must not execute `predecessor`
    /// again. A dispatch result for this request describes only the observation request itself.
    /// A delayed predecessor result must be correlated through the opaque cleanup observation
    /// token carried by this exact emission. The destructive subject may belong to a retired
    /// provider; `after` serializes observation requests inside the current provider's delivery
    /// lane.
    ContinueCleanup {
        binding: ViewportBinding,
        predecessor: EffectId,
        after: Option<EffectId>,
    },
    SetPointerPassthrough {
        binding: ViewportBinding,
        enabled: bool,
        /// Previous effect in this native pointer-input property's causal lane.
        ///
        /// The provider must serialize this request after the predecessor even
        /// when the predecessor's acknowledgement is lost. A definitive
        /// predecessor dispatch failure is terminal and therefore also
        /// releases this request.
        after: Option<EffectId>,
    },
    /// Ask the adapter to focus one exact current window incarnation.
    RequestFocus {
        binding: ViewportBinding,
        /// Previous effect in the global native-window focus causal lane.
        after: Option<EffectId>,
    },
    RequestReplacement {
        binding: ViewportBinding,
        placement: PhysicalRect,
        role: ViewportRole,
    },
    /// Resolve one exact native close edge after it becomes authoritative.
    ResolveNativeClose {
        request: CloseRequestId,
        /// Immutable provider edge the adapter is permitted to resolve.
        ///
        /// The adapter must reject dispatch when this is not its current
        /// pending close edge, even when a later edge shares the same window.
        edge: NativeCloseEdge,
        resolution: NativeCloseResolution,
    },
}

impl PlatformEffect {
    #[must_use]
    pub const fn binding(&self) -> ViewportBinding {
        match self {
            Self::CreateWindow { binding, .. }
            | Self::CompensatingClose { binding, .. }
            | Self::CancelRootClose { binding }
            | Self::RetainChild { binding }
            | Self::ReleaseChild { binding }
            | Self::ContinueCleanup { binding, .. }
            | Self::SetPointerPassthrough { binding, .. }
            | Self::RequestFocus { binding, .. }
            | Self::RequestReplacement { binding, .. } => *binding,
            Self::ResolveNativeClose { edge, .. } => edge.binding(),
        }
    }

    #[must_use]
    #[cfg(test)]
    pub const fn is_non_idempotent(&self) -> bool {
        matches!(
            self,
            Self::CreateWindow { .. }
                | Self::CompensatingClose { .. }
                | Self::ReleaseChild { .. }
                | Self::RequestReplacement { .. }
                | Self::ResolveNativeClose {
                    resolution: NativeCloseResolution::Accept,
                    ..
                }
        )
    }

    /// Returns whether this operation may destroy one exact native lifetime.
    ///
    /// Adapters use this classification to retain a locally produced dispatch result across a
    /// provider handoff without redispatching the destructive operation.
    #[must_use]
    pub const fn is_destructive_cleanup(&self) -> bool {
        matches!(
            self,
            Self::CompensatingClose { .. } | Self::ReleaseChild { .. }
        )
    }
}

/// One immutable request emitted to the adapter exactly once.
#[derive(Debug, Clone, PartialEq)]
pub struct EffectRequest {
    id: EffectId,
    epoch: WorkspaceEpoch,
    effect: PlatformEffect,
    native_close_emission_fence: Option<NativeCloseEmissionFence>,
}

impl EffectRequest {
    #[must_use]
    pub const fn id(&self) -> EffectId {
        self.id
    }

    #[must_use]
    pub const fn epoch(&self) -> WorkspaceEpoch {
        self.epoch
    }

    #[must_use]
    pub const fn effect(&self) -> &PlatformEffect {
        &self.effect
    }

    /// Returns the exact native close edge this request is allowed to resolve.
    ///
    /// Adapters must use this identity when dispatching
    /// [`PlatformEffect::ResolveNativeClose`]. A request must never resolve a
    /// later close edge which happens to share its native window binding.
    #[must_use]
    pub const fn native_close_edge(&self) -> Option<NativeCloseEdge> {
        match &self.effect {
            PlatformEffect::ResolveNativeClose { edge, .. } => Some(*edge),
            _ => None,
        }
    }

    /// Returns the causal fence for a native close effect after adapter extraction.
    ///
    /// Ordinary effects and native close requests which have not yet been emitted have no fence.
    #[must_use]
    pub const fn native_close_emission_fence(&self) -> Option<NativeCloseEmissionFence> {
        self.native_close_emission_fence
    }
}

/// One request emitted to one exact platform provider.
///
/// The wrapper prevents adapters from separating a request from the provider
/// incarnation which received it.
#[derive(Debug, Clone, PartialEq)]
pub struct PlatformEffectEmission {
    request: EffectRequest,
    delivery: EffectDelivery,
}

/// Opaque proof that one exact provider received an observation-only cleanup
/// continuation for one destructive predecessor.
///
/// Adapters may retain this value while waiting for a delayed result owned by
/// the predecessor operation. The token does not authorize redispatching that
/// predecessor; it only lets the receiving provider correlate the late result
/// back through the emitted continuation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CleanupObservationToken {
    continuation: EffectId,
    continuation_epoch: WorkspaceEpoch,
    predecessor: EffectId,
    binding: ViewportBinding,
    delivery: EffectDelivery,
}

impl CleanupObservationToken {
    pub(crate) const fn continuation(self) -> EffectId {
        self.continuation
    }

    pub(crate) const fn predecessor(self) -> EffectId {
        self.predecessor
    }

    pub(crate) const fn binding(self) -> ViewportBinding {
        self.binding
    }
}

impl PlatformEffectEmission {
    /// Returns the immutable effect request.
    #[must_use]
    #[cfg(test)]
    pub const fn request(&self) -> &EffectRequest {
        &self.request
    }

    /// Returns the exact delivery proof for this emission.
    #[must_use]
    #[cfg(test)]
    pub const fn delivery(&self) -> EffectDelivery {
        self.delivery
    }

    /// Returns the exact provider incarnation which received this emission.
    #[must_use]
    pub const fn provider(&self) -> PlatformObservationLease {
        self.delivery.provider()
    }

    #[must_use]
    pub const fn id(&self) -> EffectId {
        self.request.id()
    }

    #[must_use]
    pub const fn epoch(&self) -> WorkspaceEpoch {
        self.request.epoch()
    }

    #[must_use]
    pub const fn effect(&self) -> &PlatformEffect {
        self.request.effect()
    }

    #[must_use]
    pub const fn native_close_edge(&self) -> Option<NativeCloseEdge> {
        self.request.native_close_edge()
    }

    #[must_use]
    pub const fn native_close_emission_fence(&self) -> Option<NativeCloseEmissionFence> {
        self.request.native_close_emission_fence()
    }

    /// Returns the exact delayed-result correlation carried by an emitted
    /// cleanup continuation.
    #[must_use]
    pub const fn cleanup_observation_token(&self) -> Option<CleanupObservationToken> {
        match self.request.effect() {
            PlatformEffect::ContinueCleanup {
                binding,
                predecessor,
                ..
            } => Some(CleanupObservationToken {
                continuation: self.request.id(),
                continuation_epoch: self.request.epoch(),
                predecessor: *predecessor,
                binding: *binding,
                delivery: self.delivery,
            }),
            _ => None,
        }
    }
}

/// Adapter-level reason dispatch did not occur.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DispatchFailureReason {
    AdapterRejected,
    WindowUnavailable,
    ProviderStopped,
}

/// Why an adapter authoritatively cannot execute an effect.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EffectUnsupportedReason {
    BackendUnsupported,
    CapabilityRevoked,
}

/// Why dispatch outcome can no longer be established.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EffectIndeterminateReason {
    AcknowledgementLost,
    ProviderRestarted,
}

/// Why a request was invalidated before it reached the adapter.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EffectInvalidation {
    /// A newer workspace epoch superseded the request.
    WorkspaceReplaced { replacement_epoch: WorkspaceEpoch },
    /// The owning native-create transaction aborted before dispatch.
    NativeCreateAborted,
    /// Initial hosting of existing logical content was explicitly cancelled.
    NativeSurfaceCancelled,
    /// A pre-admission close was authoritatively cleared before its cleanup reached the adapter.
    PreAdmissionCloseCleared,
    /// A close plan withdrew a resolution before it reached the adapter.
    NativeClosePlanChanged { request: CloseRequestId },
    /// The provider which owns this request's causal predecessor was replaced before dispatch.
    PlatformProviderReplaced { provider: PlatformObservationLease },
}

/// Result of invalidating one exact unemitted request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EffectInvalidationTransition {
    Applied,
    AlreadyEmitted,
    AlreadyTerminal,
    UnknownEffect,
}

/// Public phase of one effect ledger record.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EffectPhase {
    Requested,
    DispatchFailed(DispatchFailureReason),
    /// An observation-only continuation was not dispatched.
    ///
    /// Retrying this phase may issue only another observation continuation; it must never
    /// redispatch the destructive predecessor.
    ObservationDispatchFailed(DispatchFailureReason),
    ObservedApplied {
        inventory_generation: InventoryGeneration,
    },
    /// A cleanup continuation observed that its predecessor is still
    /// indeterminate. The continuation remains live for a later definitive
    /// result or provider handoff.
    CleanupObservationIndeterminate {
        predecessor: EffectId,
        reason: EffectIndeterminateReason,
    },
    /// A successor provider observed one delayed dispatch result through the
    /// exact cleanup continuation it received.
    CleanupResultObserved {
        predecessor: EffectId,
    },
    /// A newer cleanup continuation replaced this observation request.
    ///
    /// The destructive predecessor remains authoritative. This terminal phase only closes the
    /// superseded observation identity so retention does not grow with provider or document
    /// handoffs.
    CleanupObservationSuperseded {
        predecessor: EffectId,
        successor: EffectId,
    },
    Unsupported(EffectUnsupportedReason),
    /// An observation-only continuation is unsupported.
    ///
    /// A later provider recovery may retry only the observation continuation.
    ObservationUnsupported(EffectUnsupportedReason),
    Indeterminate(EffectIndeterminateReason),
    Destroyed {
        inventory_generation: InventoryGeneration,
    },
    /// The request was provably never emitted and therefore can no longer execute.
    Invalidated {
        cause: EffectInvalidation,
    },
}

/// Result an adapter may report without claiming platform state changed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EffectDispatchResult {
    DispatchFailed(DispatchFailureReason),
    Unsupported(EffectUnsupportedReason),
    Indeterminate(EffectIndeterminateReason),
}

const fn dispatch_result_phase(
    observation_only: bool,
    result: EffectDispatchResult,
) -> EffectPhase {
    match (observation_only, result) {
        (false, EffectDispatchResult::DispatchFailed(reason)) => {
            EffectPhase::DispatchFailed(reason)
        }
        (true, EffectDispatchResult::DispatchFailed(reason)) => {
            EffectPhase::ObservationDispatchFailed(reason)
        }
        (false, EffectDispatchResult::Unsupported(reason)) => EffectPhase::Unsupported(reason),
        (true, EffectDispatchResult::Unsupported(reason)) => {
            EffectPhase::ObservationUnsupported(reason)
        }
        (_, EffectDispatchResult::Indeterminate(reason)) => EffectPhase::Indeterminate(reason),
    }
}

/// Correlated adapter result for one effect request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct EffectResult {
    effect: EffectId,
    /// Epoch in which the effect itself was issued.
    epoch: WorkspaceEpoch,
    /// Epoch of the provider receipt authorizing this report.
    receipt_epoch: WorkspaceEpoch,
    result: EffectDispatchResult,
    cleanup_observation: Option<CleanupObservationToken>,
}

impl EffectResult {
    #[must_use]
    pub const fn new(
        effect: EffectId,
        epoch: WorkspaceEpoch,
        result: EffectDispatchResult,
    ) -> Self {
        Self {
            effect,
            epoch,
            receipt_epoch: epoch,
            result,
            cleanup_observation: None,
        }
    }

    /// Correlates a predecessor result through the exact continuation emitted
    /// to the reporting provider.
    #[must_use]
    pub const fn observed_via_cleanup(
        token: CleanupObservationToken,
        predecessor_epoch: WorkspaceEpoch,
        result: EffectDispatchResult,
    ) -> Self {
        Self {
            effect: token.predecessor,
            epoch: predecessor_epoch,
            receipt_epoch: token.continuation_epoch,
            result,
            cleanup_observation: Some(token),
        }
    }

    #[must_use]
    pub const fn effect(self) -> EffectId {
        self.effect
    }

    #[must_use]
    pub const fn epoch(self) -> WorkspaceEpoch {
        self.epoch
    }

    /// Returns the provider receipt epoch which must match the reducing host frame.
    #[must_use]
    pub const fn receipt_epoch(self) -> WorkspaceEpoch {
        self.receipt_epoch
    }

    #[must_use]
    pub const fn result(self) -> EffectDispatchResult {
        self.result
    }

    /// Reports whether this result is correlated through an emitted cleanup continuation.
    #[must_use]
    pub const fn is_cleanup_observation(self) -> bool {
        self.cleanup_observation.is_some()
    }

    pub(crate) const fn cleanup_observation(self) -> Option<CleanupObservationToken> {
        self.cleanup_observation
    }
}

/// Queryable immutable request and current phase.
#[derive(Debug, Clone, PartialEq)]
pub struct EffectRecord {
    request: EffectRequest,
    phase: EffectPhase,
    delivery: Option<EffectDelivery>,
    binding_publication_provider: Option<PlatformObservationLease>,
    native_close_after_effect: Option<EffectId>,
    terminal_published: bool,
}

impl EffectRecord {
    #[must_use]
    pub const fn request(&self) -> &EffectRequest {
        &self.request
    }

    #[must_use]
    pub const fn phase(&self) -> EffectPhase {
        self.phase
    }

    #[must_use]
    pub const fn was_emitted(&self) -> bool {
        self.delivery.is_some()
    }

    /// Returns the immutable provider-bound delivery, when emitted.
    #[must_use]
    #[cfg(test)]
    pub const fn delivery(&self) -> Option<EffectDelivery> {
        self.delivery
    }

    /// Returns the exact provider which received this effect, when emitted.
    #[must_use]
    pub const fn provider(&self) -> Option<PlatformObservationLease> {
        match self.delivery {
            Some(delivery) => Some(delivery.provider()),
            None => None,
        }
    }

    pub(crate) const fn binding_publication_provider(&self) -> Option<PlatformObservationLease> {
        self.binding_publication_provider
    }

    /// Returns the inventory generation at which the adapter first received this request.
    #[must_use]
    pub const fn emitted_inventory_generation(&self) -> Option<InventoryGeneration> {
        match self.delivery {
            Some(delivery) => Some(delivery.inventory_generation()),
            None => None,
        }
    }
}

/// Current availability of one monotonic effect identity.
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg(test)]
pub enum EffectRecordLookup<'a> {
    /// The complete request and current phase are retained.
    Detailed(&'a EffectRecord),
    /// The effect reached a published terminal state and its detail was compacted.
    RetiredTerminal,
    /// The identity was never allocated by this ledger.
    Unknown,
}

fn effect_delivery_predecessors(record: &EffectRecord) -> [Option<EffectId>; 1] {
    match record.request.effect() {
        PlatformEffect::ContinueCleanup { after, .. } => [*after],
        PlatformEffect::SetPointerPassthrough { after, .. }
        | PlatformEffect::RequestFocus { after, .. } => [*after],
        PlatformEffect::ResolveNativeClose { .. } => [record.native_close_after_effect],
        _ => [None],
    }
}

fn semantic_effect_references(record: &EffectRecord) -> [Option<EffectId>; 2] {
    let semantic_subject = match record.request.effect() {
        PlatformEffect::ContinueCleanup { predecessor, .. } => Some(*predecessor),
        PlatformEffect::CompensatingClose { compensates, .. } => Some(*compensates),
        _ => None,
    };
    let delivery_predecessor = match record.request.effect() {
        // Once the successor itself was emitted, its immutable delivery proof replaces the need
        // to retain every older request in the same provider lane. Semantic subjects such as a
        // destructive cleanup predecessor remain retained independently above.
        PlatformEffect::ContinueCleanup { .. }
        | PlatformEffect::SetPointerPassthrough { .. }
        | PlatformEffect::RequestFocus { .. }
            if record.was_emitted() =>
        {
            None
        }
        _ => effect_delivery_predecessors(record)[0],
    };
    [semantic_subject, delivery_predecessor]
}

/// Deterministic outcome of a stale, duplicate, or accepted ledger transition.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EffectTransition {
    Applied,
    Duplicate,
    /// An observation did not occur after the request's adapter emission fence.
    CausalityBarrier,
    StaleEpoch,
    UnknownEffect,
    /// The effect reached a published terminal state and its detailed record was compacted.
    RetiredTerminal,
    BindingMismatch,
    /// The result or observation came from a provider which never received this effect.
    ProviderMismatch,
}

/// Core-owned ledger retaining exact effect identity and outcome.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct EffectLedger {
    last_effect: EffectId,
    records: BTreeMap<EffectId, EffectRecord>,
    revoked_providers: BTreeSet<PlatformObservationLease>,
}

impl EffectLedger {
    /// Accounts for every retained effect record and revoked provider guard.
    ///
    /// `ObservedApplied`, definitive `CleanupResultObserved`, `Destroyed`, and `Invalidated`
    /// records may be compacted after their terminal state crosses a publication boundary and no
    /// lifecycle owner or causal successor retains the identity. An indeterminate cleanup
    /// observation remains live for a later definitive result.
    #[cfg(test)]
    pub(crate) fn retention_manifest(&self) -> EffectRetentionManifest {
        let terminal_record_guards = self
            .records
            .values()
            .filter(|record| effect_phase_is_compactable(record.phase))
            .count();
        EffectRetentionManifest::new(
            self.records.len() - terminal_record_guards,
            terminal_record_guards,
            self.revoked_providers.len(),
        )
    }

    /// Returns the greatest effect identity allocated by this ledger.
    pub(crate) const fn latest_id(&self) -> EffectId {
        self.last_effect
    }

    /// Adds one request without publishing it to an adapter yet.
    ///
    /// # Errors
    ///
    /// Returns [`EffectLedgerError::EffectIdExhausted`] rather than wrapping. Native close
    /// resolution must use [`Self::request_native_close`].
    pub(crate) fn request(
        &mut self,
        effect: PlatformEffect,
    ) -> Result<EffectId, EffectLedgerError> {
        self.request_in(effect.binding().epoch(), effect)
    }

    /// Adds a request issued by `issuance_epoch`, which may target an older binding.
    ///
    /// Restore reconciliation uses this to clean up windows which can appear after their
    /// original workspace epoch has been replaced.
    pub(crate) fn request_in(
        &mut self,
        issuance_epoch: WorkspaceEpoch,
        effect: PlatformEffect,
    ) -> Result<EffectId, EffectLedgerError> {
        if matches!(&effect, PlatformEffect::ResolveNativeClose { .. }) {
            return Err(EffectLedgerError::NativeCloseRequiresDedicatedRequest);
        }
        self.insert_request(issuance_epoch, effect, None)
    }

    /// Adds one native close resolution to the dedicated causality-aware lane.
    ///
    /// The request remains unpublished until [`Self::take_new_requests_after`] receives an
    /// authoritative close observation for the exact edge. `after_effect` is an
    /// opaque predecessor identity: the ledger preserves it exactly and never infers causality
    /// from numeric effect identity ordering.
    pub(crate) fn request_native_close(
        &mut self,
        issuance_epoch: WorkspaceEpoch,
        request: CloseRequestId,
        edge: NativeCloseEdge,
        resolution: NativeCloseResolution,
        after_effect: Option<EffectId>,
    ) -> Result<EffectId, EffectLedgerError> {
        if let Some(effect) =
            self.find_native_close_request(issuance_epoch, request, edge, resolution, after_effect)
        {
            return Ok(effect);
        }

        self.insert_request(
            issuance_epoch,
            PlatformEffect::ResolveNativeClose {
                request,
                edge,
                resolution,
            },
            after_effect,
        )
    }

    /// Returns the one ledger identity for an already-requested native close resolution.
    ///
    /// A close request can remain pending across several reducer ticks while the adapter has not
    /// yet supplied an authoritative close observation. Repeating that exact semantic request
    /// must reuse its original effect identity rather than enqueueing another platform command.
    fn find_native_close_request(
        &self,
        issuance_epoch: WorkspaceEpoch,
        request: CloseRequestId,
        edge: NativeCloseEdge,
        resolution: NativeCloseResolution,
        after_effect: Option<EffectId>,
    ) -> Option<EffectId> {
        self.records.iter().find_map(|(effect, record)| {
            let PlatformEffect::ResolveNativeClose {
                request: recorded_request,
                edge: recorded_edge,
                resolution: recorded_resolution,
            } = record.request.effect()
            else {
                return None;
            };

            (record.request.epoch() == issuance_epoch
                && *recorded_request == request
                && *recorded_edge == edge
                && *recorded_resolution == resolution
                && record.native_close_after_effect == after_effect)
                .then_some(*effect)
        })
    }

    fn insert_request(
        &mut self,
        issuance_epoch: WorkspaceEpoch,
        effect: PlatformEffect,
        native_close_after_effect: Option<EffectId>,
    ) -> Result<EffectId, EffectLedgerError> {
        let id = self
            .last_effect
            .checked_next()
            .ok_or(EffectLedgerError::EffectIdExhausted)?;
        let request = EffectRequest {
            id,
            epoch: issuance_epoch,
            effect,
            native_close_emission_fence: None,
        };
        self.records.insert(
            id,
            EffectRecord {
                request,
                phase: EffectPhase::Requested,
                delivery: None,
                binding_publication_provider: None,
                native_close_after_effect,
                terminal_published: false,
            },
        );
        self.last_effect = id;
        Ok(id)
    }

    /// Prevents never-emitted requests from older epochs from reaching an adapter after restore.
    pub(crate) fn invalidate_unemitted_for_workspace_replacement(
        &mut self,
        replacement_epoch: WorkspaceEpoch,
    ) {
        for record in self.records.values_mut() {
            if record.request.epoch < replacement_epoch
                && record.delivery.is_none()
                && matches!(record.phase, EffectPhase::Requested)
            {
                record.phase = EffectPhase::Invalidated {
                    cause: EffectInvalidation::WorkspaceReplaced { replacement_epoch },
                };
            }
        }
    }

    /// Invalidates one exact request only when it provably never reached the adapter.
    pub(crate) fn invalidate_unemitted(
        &mut self,
        effect: EffectId,
        cause: EffectInvalidation,
    ) -> EffectInvalidationTransition {
        let Some(record) = self.records.get_mut(&effect) else {
            return EffectInvalidationTransition::UnknownEffect;
        };
        if record.delivery.is_some() {
            return EffectInvalidationTransition::AlreadyEmitted;
        }
        if !matches!(record.phase, EffectPhase::Requested) {
            return EffectInvalidationTransition::AlreadyTerminal;
        }
        record.phase = EffectPhase::Invalidated { cause };
        EffectInvalidationTransition::Applied
    }

    /// Reconciles queued resolutions with their original plan after all inputs.
    /// Already delivered requests retain their exact causal predecessor role.
    pub(crate) fn reconcile_unemitted_native_close_resolutions(
        &mut self,
        mut current: impl FnMut(CloseRequestId, NativeCloseEdge, NativeCloseResolution) -> bool,
    ) {
        for record in self.records.values_mut() {
            if record.delivery.is_some() || !matches!(record.phase, EffectPhase::Requested) {
                continue;
            }
            let PlatformEffect::ResolveNativeClose {
                request,
                edge,
                resolution,
            } = *record.request.effect()
            else {
                continue;
            };
            if !current(request, edge, resolution) {
                record.phase = EffectPhase::Invalidated {
                    cause: EffectInvalidation::NativeClosePlanChanged { request },
                };
            }
        }
    }

    /// Invalidates every queued causal successor of effects delivered to `provider`.
    ///
    /// The closure is transitive: a queued successor of another newly invalidated successor is
    /// invalidated in the same atomic pass. Independent queued requests remain dispatchable by a
    /// replacement provider. No request or delivery is rebound to a new provider.
    #[cfg(test)]
    pub(crate) fn invalidate_unemitted_causal_successors_for_provider_replacement(
        &mut self,
        provider: PlatformObservationLease,
    ) -> Vec<EffectId> {
        let mut provider_lane: BTreeSet<_> = self
            .records
            .iter()
            .filter_map(|(effect, record)| (record.provider() == Some(provider)).then_some(*effect))
            .collect();
        let mut invalidated = BTreeSet::new();

        loop {
            let discovered: Vec<_> = self
                .records
                .iter()
                .filter_map(|(effect, record)| {
                    (record.delivery.is_none()
                        && matches!(record.phase, EffectPhase::Requested)
                        && effect_delivery_predecessors(record)
                            .into_iter()
                            .flatten()
                            .any(|predecessor| provider_lane.contains(&predecessor)))
                    .then_some(*effect)
                })
                .filter(|effect| !provider_lane.contains(effect))
                .collect();
            if discovered.is_empty() {
                break;
            }
            for effect in discovered {
                provider_lane.insert(effect);
                invalidated.insert(effect);
            }
        }

        for effect in &invalidated {
            if let Some(record) = self.records.get_mut(effect) {
                record.phase = EffectPhase::Invalidated {
                    cause: EffectInvalidation::PlatformProviderReplaced { provider },
                };
            }
        }
        invalidated.into_iter().collect()
    }

    /// Returns each newly requested effect exactly once and marks it emitted.
    ///
    /// Native close effects remain queued until `close_authority` confirms their exact
    /// authoritative `LiveRequested` edge is still current. Their causal fence is frozen only
    /// when they enter the returned batch.
    ///
    /// # Errors
    ///
    /// Returns a typed error without mutating the ledger when the provider was revoked or any
    /// delivery-lane predecessor was not delivered to this exact provider. An observation-only
    /// cleanup's destructive subject is identity evidence, not a delivery-lane predecessor.
    #[cfg(test)]
    pub(crate) fn take_new_requests<F>(
        &mut self,
        provider: PlatformObservationLease,
        inventory_generation: InventoryGeneration,
        close_authority: F,
    ) -> Result<Vec<PlatformEffectEmission>, EffectLedgerError>
    where
        F: FnMut(NativeCloseEdge) -> bool,
    {
        self.take_new_requests_after(
            EffectId::default(),
            provider,
            inventory_generation,
            close_authority,
        )
    }

    /// Returns newly requested effects allocated strictly after `boundary`.
    ///
    /// Requests which predate an isolated control transaction remain untouched,
    /// even when they become dispatchable while that transaction is reducing.
    ///
    /// # Errors
    ///
    /// Returns a typed error without mutating the ledger when the provider was revoked or any
    /// causal predecessor was not delivered to this exact provider.
    pub(crate) fn take_new_requests_after<F>(
        &mut self,
        boundary: EffectId,
        provider: PlatformObservationLease,
        inventory_generation: InventoryGeneration,
        close_authority: F,
    ) -> Result<Vec<PlatformEffectEmission>, EffectLedgerError>
    where
        F: FnMut(NativeCloseEdge) -> bool,
    {
        self.take_new_requests_after_with_limit(
            boundary,
            provider,
            inventory_generation,
            close_authority,
            None,
        )
    }

    /// Keeps a nondelivered suffix on its original ledger records. Limiting
    /// delivery never acknowledges, cancels or reissues those requests.
    pub(crate) fn take_new_requests_with_limit<F>(
        &mut self,
        provider: PlatformObservationLease,
        inventory_generation: InventoryGeneration,
        close_authority: F,
        limit: Option<usize>,
    ) -> Result<Vec<PlatformEffectEmission>, EffectLedgerError>
    where
        F: FnMut(NativeCloseEdge) -> bool,
    {
        self.take_new_requests_after_with_limit(
            EffectId::default(),
            provider,
            inventory_generation,
            close_authority,
            limit,
        )
    }

    pub(crate) fn has_undelivered_request_after(&self, boundary: EffectId) -> bool {
        self.records
            .range((
                std::ops::Bound::Excluded(boundary),
                std::ops::Bound::Unbounded,
            ))
            .any(|(_, record)| {
                record.delivery.is_none() && matches!(record.phase, EffectPhase::Requested)
            })
    }

    pub(crate) fn is_awaiting_delivery(&self, effect: EffectId) -> bool {
        self.records.get(&effect).is_some_and(|record| {
            record.delivery.is_none() && matches!(record.phase, EffectPhase::Requested)
        })
    }

    fn take_new_requests_after_with_limit<F>(
        &mut self,
        boundary: EffectId,
        provider: PlatformObservationLease,
        inventory_generation: InventoryGeneration,
        mut close_authority: F,
        limit: Option<usize>,
    ) -> Result<Vec<PlatformEffectEmission>, EffectLedgerError>
    where
        F: FnMut(NativeCloseEdge) -> bool,
    {
        if self.revoked_providers.contains(&provider) {
            return Err(EffectLedgerError::ProviderAuthorityRevoked { provider });
        }
        let mut pending = Vec::with_capacity(limit.unwrap_or(0).min(self.records.len()));
        for (id, record) in self.records.range((
            std::ops::Bound::Excluded(boundary),
            std::ops::Bound::Unbounded,
        )) {
            if limit.is_some_and(|limit| pending.len() == limit) {
                break;
            }
            debug_assert_eq!(*id, record.request.id());
            if record.delivery.is_none() && matches!(record.phase, EffectPhase::Requested) {
                if let PlatformEffect::ResolveNativeClose { edge, .. } = &record.request.effect {
                    let edge = *edge;
                    if inventory_generation < edge.received_at() || !close_authority(edge) {
                        continue;
                    }
                }
                pending.push(*id);
            }
        }

        let pending_ids: BTreeSet<_> = pending.iter().copied().collect();
        for effect in &pending {
            let Some(record) = self.records.get(effect) else {
                continue;
            };
            for predecessor in effect_delivery_predecessors(record).into_iter().flatten() {
                let Some(predecessor_record) = self.records.get(&predecessor) else {
                    return Err(EffectLedgerError::CausalPredecessorUnavailable {
                        effect: *effect,
                        predecessor,
                    });
                };
                match predecessor_record.delivery {
                    Some(delivery) if delivery.provider() != provider => {
                        return Err(EffectLedgerError::CausalPredecessorProviderMismatch {
                            effect: *effect,
                            predecessor,
                            requested_provider: provider,
                            delivered_provider: delivery.provider(),
                        });
                    }
                    Some(_) => {}
                    None if pending_ids.contains(&predecessor) && predecessor != *effect => {}
                    None => {
                        return Err(EffectLedgerError::CausalPredecessorUnavailable {
                            effect: *effect,
                            predecessor,
                        });
                    }
                }
            }
        }

        let delivery = EffectDelivery {
            provider,
            inventory_generation,
        };
        let mut emissions = Vec::with_capacity(pending.len());
        for effect in pending {
            let Some(record) = self.records.get_mut(&effect) else {
                continue;
            };
            if let PlatformEffect::ResolveNativeClose { edge, .. } = &record.request.effect {
                let edge = *edge;
                record.request.native_close_emission_fence = Some(NativeCloseEmissionFence {
                    provider,
                    binding: edge.binding(),
                    observed_through: edge.observed_at(),
                    received_through: inventory_generation,
                    after_effect: record.native_close_after_effect,
                });
            }
            record.delivery = Some(delivery);
            emissions.push(PlatformEffectEmission {
                request: record.request.clone(),
                delivery,
            });
        }
        Ok(emissions)
    }

    /// Revokes one exact provider without transferring any delivery to its successor.
    ///
    /// Requests which were never emitted remain queued. Outstanding requests delivered to the
    /// revoked provider become indeterminate. A non-terminal cleanup observation is superseded:
    /// its successor must mint fresh correlation authority from the replacement provider.
    #[cfg(test)]
    pub(crate) fn revoke_provider_authority(&mut self, provider: PlatformObservationLease) {
        self.revoked_providers.insert(provider);
        for record in self.records.values_mut() {
            if record.provider() != Some(provider) {
                continue;
            }
            match record.phase {
                EffectPhase::Requested | EffectPhase::Indeterminate(_) => {
                    record.phase =
                        EffectPhase::Indeterminate(EffectIndeterminateReason::ProviderRestarted);
                }
                EffectPhase::CleanupObservationIndeterminate { .. } => {
                    record.phase = EffectPhase::Invalidated {
                        cause: EffectInvalidation::PlatformProviderReplaced { provider },
                    };
                }
                EffectPhase::DispatchFailed(_)
                | EffectPhase::ObservationDispatchFailed(_)
                | EffectPhase::ObservedApplied { .. }
                | EffectPhase::CleanupResultObserved { .. }
                | EffectPhase::CleanupObservationSuperseded { .. }
                | EffectPhase::Unsupported(_)
                | EffectPhase::ObservationUnsupported(_)
                | EffectPhase::Destroyed { .. }
                | EffectPhase::Invalidated { .. } => {}
            }
        }
    }

    /// Closes one observation-only cleanup request after a typed successor replaces it.
    ///
    /// This never changes the destructive predecessor. The successor remains the only live
    /// observation authority, while the superseded request becomes compactable after publication.
    pub(crate) fn supersede_cleanup_observation(
        &mut self,
        effect: EffectId,
        successor: EffectId,
        predecessor: EffectId,
        binding: ViewportBinding,
    ) -> EffectTransition {
        if effect == successor || effect == predecessor {
            return EffectTransition::CausalityBarrier;
        }
        let Some(successor_record) = self.records.get(&successor) else {
            return self.missing_transition(successor);
        };
        if !matches!(
            successor_record.request.effect(),
            PlatformEffect::ContinueCleanup {
                binding: exact_binding,
                predecessor: exact_predecessor,
                ..
            } if *exact_binding == binding && *exact_predecessor == predecessor
        ) {
            return EffectTransition::CausalityBarrier;
        }

        let missing = self.missing_transition(effect);
        let Some(record) = self.records.get_mut(&effect) else {
            return missing;
        };
        if !matches!(
            record.request.effect(),
            PlatformEffect::ContinueCleanup {
                binding: exact_binding,
                predecessor: exact_predecessor,
                ..
            } if *exact_binding == binding && *exact_predecessor == predecessor
        ) {
            return EffectTransition::BindingMismatch;
        }
        let superseded = EffectPhase::CleanupObservationSuperseded {
            predecessor,
            successor,
        };
        if record.phase == superseded {
            return EffectTransition::Duplicate;
        }
        if !matches!(
            record.phase,
            EffectPhase::Requested
                | EffectPhase::ObservationDispatchFailed(_)
                | EffectPhase::ObservationUnsupported(_)
                | EffectPhase::Indeterminate(_)
                | EffectPhase::CleanupObservationIndeterminate { .. }
                | EffectPhase::Invalidated { .. }
        ) {
            return EffectTransition::CausalityBarrier;
        }
        record.phase = superseded;
        EffectTransition::Applied
    }

    /// Releases one revoked-provider guard after its sole backend producer has quiesced.
    ///
    /// Effect records remain intact: lifecycle owners, causal successors, and terminal observers
    /// require independent proofs before any record-level compaction is valid.
    #[cfg(test)]
    pub(crate) fn compact_quiesced_backend_provider(
        &mut self,
        receipt: &BackendIngressDrainReceipt,
    ) -> bool {
        self.revoked_providers
            .remove(&receipt.lease().platform_provider())
    }

    /// Applies a result which cannot claim the platform state changed.
    pub(crate) fn report(
        &mut self,
        provider: PlatformObservationLease,
        current_epoch: WorkspaceEpoch,
        result: EffectResult,
    ) -> EffectTransition {
        if result.epoch != current_epoch {
            return EffectTransition::StaleEpoch;
        }
        self.report_exact(provider, result)
    }

    /// Applies a result against its exact immutable request epoch.
    ///
    /// Cross-epoch cleanup results use [`Self::report_cleanup_observation`]. The coordinator
    /// may also validate a definitive failure of an original unobserved create still retained
    /// for compensation. This path only accepts the provider which received the effect itself.
    pub(crate) fn report_exact(
        &mut self,
        provider: PlatformObservationLease,
        result: EffectResult,
    ) -> EffectTransition {
        if self.revoked_providers.contains(&provider) {
            return EffectTransition::ProviderMismatch;
        }
        let missing = self.missing_transition(result.effect);
        let Some(record) = self.records.get_mut(&result.effect) else {
            return missing;
        };
        if record.request.epoch != result.epoch {
            return EffectTransition::StaleEpoch;
        }
        let Some(delivery) = record.delivery else {
            return EffectTransition::CausalityBarrier;
        };
        if delivery.provider() != provider {
            return EffectTransition::ProviderMismatch;
        }
        let observation_only = matches!(
            record.request.effect,
            PlatformEffect::ContinueCleanup { .. }
        );
        let phase = dispatch_result_phase(observation_only, result.result);
        if record.phase == phase {
            return EffectTransition::Duplicate;
        }
        let transition_allowed = matches!(record.phase, EffectPhase::Requested)
            || matches!(record.phase, EffectPhase::Indeterminate(_))
                && matches!(
                    phase,
                    EffectPhase::DispatchFailed(_)
                        | EffectPhase::ObservationDispatchFailed(_)
                        | EffectPhase::Unsupported(_)
                        | EffectPhase::ObservationUnsupported(_)
                );
        if !transition_allowed {
            return EffectTransition::Duplicate;
        }
        record.phase = phase;
        EffectTransition::Applied
    }

    /// Applies one delayed predecessor result through the exact observation
    /// continuation emitted to the current provider.
    pub(crate) fn report_cleanup_observation(
        &mut self,
        provider: PlatformObservationLease,
        current_epoch: WorkspaceEpoch,
        result: EffectResult,
    ) -> EffectTransition {
        let Some(token) = result.cleanup_observation else {
            return EffectTransition::CausalityBarrier;
        };
        if self.revoked_providers.contains(&provider) {
            return EffectTransition::ProviderMismatch;
        }

        let Some(continuation) = self.records.get(&token.continuation) else {
            return self.missing_transition(token.continuation);
        };
        if token.continuation_epoch != current_epoch
            || continuation.request.epoch != token.continuation_epoch
            || continuation.delivery != Some(token.delivery)
            || token.delivery.provider() != provider
        {
            return EffectTransition::ProviderMismatch;
        }
        let PlatformEffect::ContinueCleanup {
            binding,
            predecessor,
            ..
        } = continuation.request.effect()
        else {
            return EffectTransition::CausalityBarrier;
        };
        if *binding != token.binding || *predecessor != token.predecessor {
            return EffectTransition::BindingMismatch;
        }

        let Some(predecessor_record) = self.records.get(&token.predecessor) else {
            return self.missing_transition(token.predecessor);
        };
        if result.effect != token.predecessor
            || result.epoch != predecessor_record.request.epoch
            || predecessor_record.request.effect.binding() != token.binding
            || !predecessor_record.was_emitted()
            || !predecessor_record.request.effect.is_destructive_cleanup()
        {
            return EffectTransition::CausalityBarrier;
        }

        let reported_predecessor_phase = dispatch_result_phase(false, result.result);
        // An observation-only continuation owns its own indeterminate reason. If provider
        // revocation already made the destructive predecessor indeterminate, a later cleanup
        // observation must not erase that causal fact merely because it also cannot prove a
        // terminal result. A definitive result may still refine the predecessor below.
        let predecessor_phase = match (predecessor_record.phase, reported_predecessor_phase) {
            (existing @ EffectPhase::Indeterminate(_), EffectPhase::Indeterminate(_)) => existing,
            (_, reported) => reported,
        };
        let continuation_phase = match result.result {
            EffectDispatchResult::Indeterminate(reason) => {
                EffectPhase::CleanupObservationIndeterminate {
                    predecessor: token.predecessor,
                    reason,
                }
            }
            EffectDispatchResult::DispatchFailed(_) | EffectDispatchResult::Unsupported(_) => {
                EffectPhase::CleanupResultObserved {
                    predecessor: token.predecessor,
                }
            }
        };
        if continuation.phase == continuation_phase && predecessor_record.phase == predecessor_phase
        {
            return EffectTransition::Duplicate;
        }
        let continuation_open = matches!(
            continuation.phase,
            EffectPhase::Requested | EffectPhase::Indeterminate(_)
        ) || matches!(
            continuation.phase,
            EffectPhase::CleanupObservationIndeterminate { predecessor, .. }
                if predecessor == token.predecessor
        );
        if !continuation_open {
            return if matches!(
                continuation.phase,
                EffectPhase::CleanupResultObserved {
                    predecessor: exact,
                } if exact == token.predecessor
            ) && predecessor_record.phase != predecessor_phase
            {
                EffectTransition::CausalityBarrier
            } else {
                EffectTransition::Duplicate
            };
        }
        if let (
            EffectPhase::CleanupObservationIndeterminate {
                predecessor: existing_predecessor,
                reason: existing,
            },
            EffectPhase::CleanupObservationIndeterminate {
                predecessor: reported_predecessor,
                reason: reported,
            },
        ) = (continuation.phase, continuation_phase)
            && existing_predecessor == reported_predecessor
            && existing != reported
        {
            return EffectTransition::CausalityBarrier;
        }
        let predecessor_transition_allowed =
            matches!(
                predecessor_record.phase,
                EffectPhase::Requested | EffectPhase::Indeterminate(_)
            ) && (matches!(predecessor_record.phase, EffectPhase::Requested)
                || matches!(
                    predecessor_phase,
                    EffectPhase::DispatchFailed(_)
                        | EffectPhase::Unsupported(_)
                        | EffectPhase::Indeterminate(_)
                ));
        if !predecessor_transition_allowed {
            return EffectTransition::Duplicate;
        }

        self.records
            .get_mut(&token.predecessor)
            .expect("validated predecessor remains present")
            .phase = predecessor_phase;
        self.records
            .get_mut(&token.continuation)
            .expect("validated continuation remains present")
            .phase = continuation_phase;
        EffectTransition::Applied
    }

    pub(crate) fn mark_observed_applied(
        &mut self,
        provider: PlatformObservationLease,
        effect: EffectId,
        binding: ViewportBinding,
        inventory_generation: InventoryGeneration,
    ) -> EffectTransition {
        if self.revoked_providers.contains(&provider) {
            return EffectTransition::ProviderMismatch;
        }
        let missing = self.missing_transition(effect);
        let Some(record) = self.records.get_mut(&effect) else {
            return missing;
        };
        if record.request.effect.binding().window() != binding.window() {
            return EffectTransition::BindingMismatch;
        }
        let Some(delivery) = record.delivery else {
            return EffectTransition::CausalityBarrier;
        };
        if delivery.provider() != provider {
            return EffectTransition::ProviderMismatch;
        }
        if inventory_generation <= delivery.inventory_generation() {
            return EffectTransition::CausalityBarrier;
        }
        apply_observation_phase(
            record,
            EffectPhase::ObservedApplied {
                inventory_generation,
            },
        )
    }

    pub(crate) fn observation_matches_delivery(
        &self,
        provider: PlatformObservationLease,
        effect: EffectId,
        binding: ViewportBinding,
        inventory_generation: InventoryGeneration,
    ) -> bool {
        if self.revoked_providers.contains(&provider) {
            return false;
        }
        self.records.get(&effect).is_some_and(|record| {
            record.request.effect.binding().window() == binding.window()
                && record.delivery.is_some_and(|delivery| {
                    delivery.provider() == provider
                        && inventory_generation > delivery.inventory_generation()
                })
                && matches!(
                    record.phase,
                    EffectPhase::Requested
                        | EffectPhase::DispatchFailed(_)
                        | EffectPhase::ObservationDispatchFailed(_)
                        | EffectPhase::Unsupported(_)
                        | EffectPhase::ObservationUnsupported(_)
                        | EffectPhase::Indeterminate(_)
                        | EffectPhase::ObservedApplied { .. }
                )
        })
    }

    /// Records authoritative destruction of the exact binding independently of effect delivery.
    ///
    /// Destruction is platform state, not an acknowledgement that this effect was dispatched or
    /// applied. It therefore requires neither a provider lease nor an emission fence and never
    /// changes the immutable delivery recorded for the effect.
    pub(crate) fn mark_destroyed(
        &mut self,
        effect: EffectId,
        binding: ViewportBinding,
        inventory_generation: InventoryGeneration,
    ) -> EffectTransition {
        let missing = self.missing_transition(effect);
        let Some(record) = self.records.get_mut(&effect) else {
            return missing;
        };
        if record.request.effect.binding().window() != binding.window() {
            return EffectTransition::BindingMismatch;
        }
        apply_observation_phase(
            record,
            EffectPhase::Destroyed {
                inventory_generation,
            },
        )
    }

    #[must_use]
    pub fn record(&self, effect: EffectId) -> Option<&EffectRecord> {
        self.records.get(&effect)
    }

    /// Classifies an identity without requiring permanent retention of terminal records.
    #[must_use]
    #[cfg(test)]
    pub fn lookup(&self, effect: EffectId) -> EffectRecordLookup<'_> {
        match self.record(effect) {
            Some(record) => EffectRecordLookup::Detailed(record),
            None if self.effect_was_retired_terminal(effect) => EffectRecordLookup::RetiredTerminal,
            None => EffectRecordLookup::Unknown,
        }
    }

    pub fn records(&self) -> impl Iterator<Item = (EffectId, &EffectRecord)> {
        self.records.iter().map(|(id, record)| (*id, record))
    }

    /// Marks compactable records which were visible at this successful publication boundary.
    pub(crate) fn mark_boundary_published(&mut self) {
        for record in self.records.values_mut() {
            if effect_phase_is_compactable(record.phase) {
                record.terminal_published = true;
            }
        }
    }

    pub(crate) fn mark_creation_bindings_published(
        &mut self,
        provider: PlatformObservationLease,
        is_current_binding: impl Fn(ViewportBinding) -> bool,
    ) {
        for record in self.records.values_mut() {
            if let PlatformEffect::CreateWindow { binding, .. }
            | PlatformEffect::RequestReplacement { binding, .. } = record.request.effect()
                && is_current_binding(*binding)
            {
                record.binding_publication_provider = Some(provider);
            }
        }
    }

    /// Removes terminal detail no longer named by a lifecycle owner or causal successor.
    pub(crate) fn compact_published_terminal(
        &mut self,
        retained_by_owner: &BTreeSet<EffectId>,
    ) -> usize {
        let mut retained = retained_by_owner.clone();
        retained.extend(self.records.iter().filter_map(|(effect, record)| {
            (!record.terminal_published || !effect_phase_is_compactable(record.phase))
                .then_some(*effect)
        }));
        let mut pending = retained.iter().copied().collect::<Vec<_>>();
        while let Some(effect) = pending.pop() {
            let Some(record) = self.records.get(&effect) else {
                continue;
            };
            for predecessor in semantic_effect_references(record).into_iter().flatten() {
                if retained.insert(predecessor) {
                    pending.push(predecessor);
                }
            }
        }

        let before = self.records.len();
        self.records.retain(|effect, record| {
            !record.terminal_published
                || !effect_phase_is_compactable(record.phase)
                || retained.contains(effect)
        });
        before - self.records.len()
    }

    fn missing_transition(&self, effect: EffectId) -> EffectTransition {
        if self.effect_was_retired_terminal(effect) {
            EffectTransition::RetiredTerminal
        } else {
            EffectTransition::UnknownEffect
        }
    }

    fn effect_was_retired_terminal(&self, effect: EffectId) -> bool {
        effect != EffectId::default()
            && effect <= self.last_effect
            && !self.records.contains_key(&effect)
    }

    #[cfg(test)]
    pub(crate) fn exhaust_effect_ids(&mut self) {
        self.last_effect = EffectId::new(u64::MAX);
    }
}

const fn effect_phase_is_compactable(phase: EffectPhase) -> bool {
    matches!(
        phase,
        EffectPhase::ObservedApplied { .. }
            | EffectPhase::CleanupResultObserved { .. }
            | EffectPhase::CleanupObservationSuperseded { .. }
            | EffectPhase::Destroyed { .. }
            | EffectPhase::Invalidated { .. }
    )
}

fn apply_observation_phase(record: &mut EffectRecord, phase: EffectPhase) -> EffectTransition {
    if record.phase == phase {
        return EffectTransition::Duplicate;
    }
    let transition_allowed = match phase {
        EffectPhase::ObservedApplied { .. } => matches!(
            record.phase,
            EffectPhase::Requested
                | EffectPhase::DispatchFailed(_)
                | EffectPhase::ObservationDispatchFailed(_)
                | EffectPhase::Unsupported(_)
                | EffectPhase::ObservationUnsupported(_)
                | EffectPhase::Indeterminate(_)
        ),
        EffectPhase::Destroyed { .. } => !matches!(
            record.phase,
            EffectPhase::Destroyed { .. } | EffectPhase::Invalidated { .. }
        ),
        EffectPhase::Requested
        | EffectPhase::DispatchFailed(_)
        | EffectPhase::ObservationDispatchFailed(_)
        | EffectPhase::CleanupObservationIndeterminate { .. }
        | EffectPhase::CleanupResultObserved { .. }
        | EffectPhase::CleanupObservationSuperseded { .. }
        | EffectPhase::Unsupported(_)
        | EffectPhase::ObservationUnsupported(_)
        | EffectPhase::Indeterminate(_)
        | EffectPhase::Invalidated { .. } => false,
    };
    if !transition_allowed {
        return EffectTransition::Duplicate;
    }
    record.phase = phase;
    EffectTransition::Applied
}

/// Typed failure while allocating or delivering an effect.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum EffectLedgerError {
    #[error("platform effect identity is exhausted")]
    EffectIdExhausted,
    #[error("native close effects must use the dedicated request_native_close entry point")]
    NativeCloseRequiresDedicatedRequest,
    #[error("platform effect provider {provider:?} has been revoked")]
    ProviderAuthorityRevoked { provider: PlatformObservationLease },
    #[error(
        "effect {effect:?} causal predecessor {predecessor:?} has not been delivered in this batch"
    )]
    CausalPredecessorUnavailable {
        effect: EffectId,
        predecessor: EffectId,
    },
    #[error(
        "effect {effect:?} cannot be delivered to provider {requested_provider:?}: causal predecessor {predecessor:?} was delivered to {delivered_provider:?}"
    )]
    CausalPredecessorProviderMismatch {
        effect: EffectId,
        predecessor: EffectId,
        requested_provider: PlatformObservationLease,
        delivered_provider: PlatformObservationLease,
    },
}

#[cfg(test)]
mod tests;
