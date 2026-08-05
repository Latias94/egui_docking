//! Core-owned platform effect ledger.
//!
//! Dispatch acknowledgement is deliberately absent: an adapter call returning
//! successfully does not prove that a window exists, a flag changed, or a close
//! completed. Only matching authoritative observations can do that.

use std::collections::{BTreeMap, BTreeSet};

use thiserror::Error;

use crate::backend_ingress::BackendIngressDrainReceipt;
use crate::close_plan::{CloseRequestId, NativeCloseEdge};
use crate::geometry::PhysicalRect;
use crate::ids::WorkspaceEpoch;
use crate::platform_provider::PlatformObservationLease;
use crate::presentation_observation::PresentedNativeStagingPresentation;
use crate::retention::EffectRetentionManifest;
use crate::viewport::{
    CloseObservationGeneration, InventoryGeneration, PresentationObservationGeneration,
    ViewportBinding, ViewportRole,
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
    /// Create one hidden native window at the exact requested placement.
    ///
    /// The adapter must not expose it for input until the core later emits
    /// [`PlatformEffect::ShowWindow`].
    CreateWindow {
        binding: ViewportBinding,
        placement: PhysicalRect,
        role: ViewportRole,
    },
    /// Show a previously created hidden window after its exact retained staging output.
    ShowWindow {
        binding: ViewportBinding,
        /// Exact hidden observation which must causally precede this show.
        after_hidden: PresentationObservationGeneration,
        /// Exact pre-show output which proved the retained staging resource was presented.
        after_pre_show: PresentedNativeStagingPresentation,
    },
    CompensatingClose {
        binding: ViewportBinding,
        compensates: EffectId,
    },
    CancelRootClose {
        binding: ViewportBinding,
    },
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
    RequestRootClose {
        binding: ViewportBinding,
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
            | Self::ShowWindow { binding, .. }
            | Self::CompensatingClose { binding, .. }
            | Self::CancelRootClose { binding }
            | Self::RetainChild { binding }
            | Self::ReleaseChild { binding }
            | Self::ContinueCleanup { binding, .. }
            | Self::RequestRootClose { binding }
            | Self::SetPointerPassthrough { binding, .. }
            | Self::RequestFocus { binding, .. }
            | Self::RequestReplacement { binding, .. } => *binding,
            Self::ResolveNativeClose { edge, .. } => edge.binding(),
        }
    }

    #[must_use]
    pub const fn is_non_idempotent(&self) -> bool {
        matches!(
            self,
            Self::CreateWindow { .. }
                | Self::CompensatingClose { .. }
                | Self::ReleaseChild { .. }
                | Self::RequestRootClose { .. }
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
            Self::CompensatingClose { .. }
                | Self::ReleaseChild { .. }
                | Self::RequestRootClose { .. }
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
    pub const fn request(&self) -> &EffectRequest {
        &self.request
    }

    /// Returns the exact delivery proof for this emission.
    #[must_use]
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
    /// A staging close was authoritatively cleared before its private cleanup reached the adapter.
    StagingCloseCleared,
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
        // to retain every older observation request in the same provider lane.
        PlatformEffect::ContinueCleanup { .. } if record.was_emitted() => None,
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
    /// The request remains unpublished until [`Self::take_new_requests`] receives an
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

    /// Invalidates every queued causal successor of effects delivered to `provider`.
    ///
    /// The closure is transitive: a queued successor of another newly invalidated successor is
    /// invalidated in the same atomic pass. Independent queued requests remain dispatchable by a
    /// replacement provider. No request or delivery is rebound to a new provider.
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
        mut close_authority: F,
    ) -> Result<Vec<PlatformEffectEmission>, EffectLedgerError>
    where
        F: FnMut(NativeCloseEdge) -> bool,
    {
        if self.revoked_providers.contains(&provider) {
            return Err(EffectLedgerError::ProviderAuthorityRevoked { provider });
        }
        let mut pending = Vec::new();
        for (id, record) in self.records.range((
            std::ops::Bound::Excluded(boundary),
            std::ops::Bound::Unbounded,
        )) {
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
    /// Cross-epoch cleanup results must use [`Self::report_cleanup_observation`]; this path only
    /// accepts the provider which received the effect itself.
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
        if record.request.effect.binding() != binding {
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
            record.request.effect.binding() == binding
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
        if record.request.effect.binding() != binding {
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
mod tests {
    use super::*;
    use crate::close_plan::{
        CloseAuthority, CloseCoordinator, NativeCloseEdge, SurfaceCloseRequest,
    };
    use crate::ids::{EngineAuthorityDomainId, SurfaceId, WorkspaceRevision};
    use crate::platform_provider::PlatformObservationAuthority;
    use crate::policy::PolicyRevision;
    use crate::transition::WorkspaceVersion;
    use crate::viewport::{WindowIncarnation, WindowToken};

    fn binding(epoch: u64, incarnation: u64) -> ViewportBinding {
        ViewportBinding::new(
            EngineAuthorityDomainId::new_for_test(17),
            WorkspaceEpoch::new(epoch),
            SurfaceId::new(2),
            WindowToken::new(3),
            WindowIncarnation::new(incarnation),
        )
    }

    fn create_effect(binding: ViewportBinding) -> PlatformEffect {
        PlatformEffect::CreateWindow {
            binding,
            placement: PhysicalRect::new(0.0, 0.0, 100.0, 80.0)
                .expect("test placement must be valid"),
            role: ViewportRole::Child,
        }
    }

    fn test_provider() -> PlatformObservationLease {
        let mut authority =
            PlatformObservationAuthority::new(EngineAuthorityDomainId::new_for_test(17));
        authority.create().expect("test provider must be available")
    }

    fn test_provider_replacement() -> (PlatformObservationLease, PlatformObservationLease) {
        let mut authority =
            PlatformObservationAuthority::new(EngineAuthorityDomainId::new_for_test(17));
        let first = authority
            .create()
            .expect("first test provider must be available");
        let ticket = authority
            .begin_replacement(first)
            .expect("replacement test provider must be reserved");
        let second = authority
            .finish_replacement(ticket)
            .expect("replacement test provider must be activated");
        (first, second)
    }

    #[test]
    fn published_terminal_effects_compact_to_the_monotonic_identity_frontier() {
        let binding = binding(0, 1);
        let mut ledger = EffectLedger::default();
        let mut last = EffectId::default();

        for _ in 0..10_000 {
            let effect = ledger
                .request(create_effect(binding))
                .expect("effect identity must remain available");
            assert_eq!(
                ledger.invalidate_unemitted(effect, EffectInvalidation::NativeCreateAborted),
                EffectInvalidationTransition::Applied,
            );
            last = effect;
        }

        ledger.mark_boundary_published();
        let retention = ledger.retention_manifest();
        assert_eq!(retention.unsettled_records(), 0);
        assert_eq!(retention.terminal_record_guards(), 10_000);
        assert_eq!(retention.revoked_provider_guards(), 0);
        assert_eq!(retention.retained_structure_count(), 10_000);
        assert_eq!(retention.provider_release_barrier(), None,);

        assert_eq!(
            ledger.compact_published_terminal(&BTreeSet::from([last])),
            9_999,
        );
        assert_eq!(ledger.retention_manifest().terminal_record_guards(), 1);
        assert_eq!(ledger.compact_published_terminal(&BTreeSet::new()), 1,);
        assert_eq!(ledger.retention_manifest().retained_structure_count(), 0);
        assert_eq!(
            ledger.mark_destroyed(last, binding, InventoryGeneration::new(1)),
            EffectTransition::RetiredTerminal,
        );
    }

    #[test]
    fn live_owner_retains_the_complete_terminal_effect_predecessor_chain() {
        let binding = binding(0, 1);
        let mut ledger = EffectLedger::default();
        let first = ledger
            .request(PlatformEffect::SetPointerPassthrough {
                binding,
                enabled: true,
                after: None,
            })
            .expect("first effect identity must be available");
        let second = ledger
            .request(PlatformEffect::SetPointerPassthrough {
                binding,
                enabled: false,
                after: Some(first),
            })
            .expect("second effect identity must be available");
        assert_eq!(
            take_ordinary_requests(&mut ledger, InventoryGeneration::new(1)).len(),
            2,
        );
        for effect in [first, second] {
            assert_eq!(
                ledger.mark_observed_applied(
                    test_provider(),
                    effect,
                    binding,
                    InventoryGeneration::new(2),
                ),
                EffectTransition::Applied,
            );
        }
        ledger.mark_boundary_published();

        assert_eq!(
            ledger.compact_published_terminal(&BTreeSet::from([second])),
            0,
        );
        assert_eq!(ledger.records().count(), 2);
        assert_eq!(ledger.compact_published_terminal(&BTreeSet::new()), 2);
        assert_eq!(ledger.records().count(), 0);
    }

    #[test]
    fn semantic_cleanup_subjects_survive_until_their_live_owner_is_terminal() {
        let binding = binding(0, 1);
        let mut ledger = EffectLedger::default();
        let compensated = ledger
            .request(create_effect(binding))
            .expect("compensated effect identity must be available");
        let observed = ledger
            .request(create_effect(binding))
            .expect("observed cleanup identity must be available");
        for effect in [compensated, observed] {
            assert_eq!(
                ledger.invalidate_unemitted(effect, EffectInvalidation::NativeCreateAborted),
                EffectInvalidationTransition::Applied,
            );
        }
        let compensation = ledger
            .request(PlatformEffect::CompensatingClose {
                binding,
                compensates: compensated,
            })
            .expect("compensation identity must be available");
        let continuation = ledger
            .request(PlatformEffect::ContinueCleanup {
                binding,
                predecessor: observed,
                after: None,
            })
            .expect("continuation identity must be available");
        ledger.mark_boundary_published();

        assert_eq!(ledger.compact_published_terminal(&BTreeSet::new()), 0);
        assert_eq!(ledger.records().count(), 4);
        for effect in [compensation, continuation] {
            assert_eq!(
                ledger.invalidate_unemitted(effect, EffectInvalidation::NativeCreateAborted),
                EffectInvalidationTransition::Applied,
            );
        }
        ledger.mark_boundary_published();
        assert_eq!(ledger.compact_published_terminal(&BTreeSet::new()), 4);
        assert_eq!(ledger.records().count(), 0);
    }

    #[test]
    fn retention_accounts_for_revoked_provider_tombstones() {
        let provider = test_provider();
        let mut ledger = EffectLedger::default();
        ledger.revoke_provider_authority(provider);

        let retention = ledger.retention_manifest();
        assert_eq!(retention.unsettled_records(), 0);
        assert_eq!(retention.terminal_record_guards(), 0);
        assert_eq!(retention.revoked_provider_guards(), 1);
        assert_eq!(retention.retained_structure_count(), 1);
        assert_eq!(
            retention.provider_release_barrier(),
            Some(crate::retention::RuntimeRetentionReleaseBarrier::EffectIngressQuiesced),
        );
    }

    fn take_ordinary_requests(
        ledger: &mut EffectLedger,
        inventory_generation: InventoryGeneration,
    ) -> Vec<PlatformEffectEmission> {
        ledger
            .take_new_requests(test_provider(), inventory_generation, |_| false)
            .expect("ordinary test effects must have valid causal predecessors")
    }

    fn native_close_edge(
        binding: ViewportBinding,
        observed_at: u64,
        received_at: u64,
    ) -> NativeCloseEdge {
        NativeCloseEdge::from_authoritative_requested(
            EngineAuthorityDomainId::new_for_test(17),
            binding,
            CloseObservationGeneration::new(observed_at),
            InventoryGeneration::new(received_at),
        )
    }

    fn close_request(edge: NativeCloseEdge) -> CloseRequestId {
        let domain = edge.domain();
        let authority = CloseAuthority::new(
            domain,
            WorkspaceVersion::new(edge.binding().epoch(), WorkspaceRevision::new(1)),
            PolicyRevision::new(1),
        );
        let mut coordinator = CloseCoordinator::new(domain);
        coordinator
            .open_surface(authority, edge, SurfaceCloseRequest::RetainLayout, [], ())
            .expect("test close plan must open")
            .request()
    }

    #[test]
    fn native_close_fence_is_frozen_only_when_the_request_is_extracted() {
        let mut ledger = EffectLedger::default();
        let binding = binding(1, 1);
        let edge = native_close_edge(binding, 7, 11);
        let effect = ledger
            .request_native_close(
                WorkspaceEpoch::new(1),
                close_request(edge),
                edge,
                NativeCloseResolution::Accept,
                None,
            )
            .expect("native close effect must allocate");

        let queued = ledger.record(effect).expect("request must be recorded");
        assert!(!queued.was_emitted());
        assert_eq!(queued.request().native_close_emission_fence(), None);

        assert_eq!(queued.request().native_close_edge(), Some(edge));

        let provider = test_provider();
        let emitted = ledger
            .take_new_requests(provider, InventoryGeneration::new(12), |actual| {
                assert_eq!(actual, edge);
                true
            })
            .expect("native close effect must be causally valid");
        assert_eq!(emitted.len(), 1);
        assert_eq!(
            emitted[0].native_close_emission_fence(),
            Some(NativeCloseEmissionFence {
                provider,
                binding,
                observed_through: CloseObservationGeneration::new(7),
                received_through: InventoryGeneration::new(12),
                after_effect: None,
            })
        );
    }

    #[test]
    fn native_close_without_authoritative_edge_remains_requested_and_unemitted() {
        let mut ledger = EffectLedger::default();
        let binding = binding(1, 1);
        let edge = native_close_edge(binding, 8, 3);
        let effect = ledger
            .request_native_close(
                WorkspaceEpoch::new(1),
                close_request(edge),
                edge,
                NativeCloseResolution::Cancel,
                None,
            )
            .expect("native close effect must allocate");

        assert!(
            ledger
                .take_new_requests(test_provider(), InventoryGeneration::new(3), |_| false)
                .expect("queued request validation must succeed")
                .is_empty()
        );
        let queued = ledger.record(effect).expect("request must remain recorded");
        assert_eq!(queued.phase(), EffectPhase::Requested);
        assert!(!queued.was_emitted());
        assert_eq!(queued.request().native_close_emission_fence(), None);

        assert_eq!(
            ledger
                .take_new_requests(
                    test_provider(),
                    InventoryGeneration::new(4),
                    |actual| actual == edge,
                )
                .expect("native close effect must be causally valid")
                .len(),
            1
        );
    }

    #[test]
    fn native_close_effect_never_emits_against_a_reissued_edge() {
        let mut ledger = EffectLedger::default();
        let binding = binding(1, 1);
        let expected = native_close_edge(binding, 5, 5);
        let replacement = native_close_edge(binding, 7, 7);
        let same_provider_generation_with_new_ingress = native_close_edge(binding, 5, 6);
        let effect = ledger
            .request_native_close(
                WorkspaceEpoch::new(1),
                close_request(expected),
                expected,
                NativeCloseResolution::Cancel,
                None,
            )
            .expect("native close effect must allocate");

        assert!(
            ledger
                .take_new_requests(
                    test_provider(),
                    InventoryGeneration::new(7),
                    |current| current == replacement,
                )
                .expect("stale native edge must remain queued")
                .is_empty()
        );
        assert!(
            ledger
                .take_new_requests(
                    test_provider(),
                    InventoryGeneration::new(7),
                    |current| current == same_provider_generation_with_new_ingress,
                )
                .expect("reissued native edge must remain queued")
                .is_empty()
        );
        assert!(
            !ledger
                .record(effect)
                .expect("stale request remains queryable")
                .was_emitted()
        );

        let emitted = ledger
            .take_new_requests(test_provider(), InventoryGeneration::new(7), |current| {
                current == expected
            })
            .expect("exact native edge must emit");
        assert_eq!(emitted.len(), 1);
        assert_eq!(emitted[0].id(), effect);
        assert_eq!(emitted[0].native_close_edge(), Some(expected));
    }

    #[test]
    fn native_close_deduplication_includes_the_exact_edge() {
        let mut ledger = EffectLedger::default();
        let binding = binding(1, 1);
        let first_edge = native_close_edge(binding, 5, 5);
        let second_edge = native_close_edge(binding, 7, 7);
        let request = close_request(first_edge);

        let first = ledger
            .request_native_close(
                WorkspaceEpoch::new(1),
                request,
                first_edge,
                NativeCloseResolution::Accept,
                None,
            )
            .expect("first native close effect must allocate");
        let second = ledger
            .request_native_close(
                WorkspaceEpoch::new(1),
                request,
                second_edge,
                NativeCloseResolution::Accept,
                None,
            )
            .expect("reissued edge must allocate a distinct effect");

        assert_ne!(first, second);
        assert_eq!(ledger.records().count(), 2);
    }

    #[test]
    fn ordinary_effects_never_receive_a_native_close_fence() {
        let mut ledger = EffectLedger::default();
        let effect = ledger
            .request(create_effect(binding(1, 1)))
            .expect("ordinary effect must allocate");

        let emitted = take_ordinary_requests(&mut ledger, InventoryGeneration::new(1));
        assert_eq!(emitted.len(), 1);
        assert_eq!(emitted[0].id(), effect);
        assert_eq!(emitted[0].native_close_emission_fence(), None);
    }

    #[test]
    fn native_close_predecessor_is_preserved_for_the_same_provider() {
        let mut ledger = EffectLedger::default();
        let binding = binding(1, 1);
        let edge = native_close_edge(binding, 6, 4);
        let provider = test_provider();
        let predecessor = ledger
            .request(PlatformEffect::CancelRootClose { binding })
            .expect("predecessor must allocate");
        ledger
            .take_new_requests(provider, InventoryGeneration::new(4), |_| false)
            .expect("predecessor must emit");
        let effect = ledger
            .request_native_close(
                WorkspaceEpoch::new(1),
                close_request(edge),
                edge,
                NativeCloseResolution::Cancel,
                Some(predecessor),
            )
            .expect("native close effect must allocate");

        let emitted = ledger
            .take_new_requests(provider, InventoryGeneration::new(5), |actual| {
                actual == edge
            })
            .expect("same-provider predecessor must be valid");
        assert_eq!(emitted[0].id(), effect);
        assert_eq!(
            emitted[0]
                .native_close_emission_fence()
                .expect("native request must carry a fence")
                .after_effect(),
            Some(predecessor)
        );
    }

    #[test]
    fn emitted_native_close_is_never_extracted_twice() {
        let mut ledger = EffectLedger::default();
        let binding = binding(1, 1);
        let edge = native_close_edge(binding, 3, 2);
        ledger
            .request_native_close(
                WorkspaceEpoch::new(1),
                close_request(edge),
                edge,
                NativeCloseResolution::Accept,
                None,
            )
            .expect("native close effect must allocate");

        assert_eq!(
            ledger
                .take_new_requests(
                    test_provider(),
                    InventoryGeneration::new(2),
                    |actual| actual == edge,
                )
                .expect("first extraction must succeed")
                .len(),
            1
        );
        assert!(
            ledger
                .take_new_requests(
                    test_provider(),
                    InventoryGeneration::new(9),
                    |actual| actual == edge,
                )
                .expect("duplicate extraction check must succeed")
                .is_empty()
        );
    }

    #[test]
    fn bounded_extraction_never_emits_an_older_queued_request() {
        let mut ledger = EffectLedger::default();
        let older = ledger
            .request(create_effect(binding(1, 1)))
            .expect("older effect must allocate");
        let boundary = ledger.latest_id();
        let newer = ledger
            .request(create_effect(binding(2, 1)))
            .expect("newer effect must allocate");

        let isolated = ledger
            .take_new_requests_after(
                boundary,
                test_provider(),
                InventoryGeneration::new(1),
                |_| false,
            )
            .expect("bounded extraction must succeed");
        assert_eq!(isolated.len(), 1);
        assert_eq!(isolated[0].id(), newer);
        assert!(
            !ledger
                .record(older)
                .expect("older record exists")
                .was_emitted()
        );
        assert!(
            ledger
                .record(newer)
                .expect("newer record exists")
                .was_emitted()
        );

        let remaining = take_ordinary_requests(&mut ledger, InventoryGeneration::new(1));
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].id(), older);
    }

    #[test]
    fn repeated_native_close_resolution_reuses_the_original_effect_across_ticks() {
        let mut ledger = EffectLedger::default();
        let binding = binding(1, 1);
        let edge = native_close_edge(binding, 3, 2);
        let request = close_request(edge);

        let first = ledger
            .request_native_close(
                WorkspaceEpoch::new(1),
                request,
                edge,
                NativeCloseResolution::Accept,
                None,
            )
            .expect("native close effect must allocate");
        let repeated_before_emission = ledger
            .request_native_close(
                WorkspaceEpoch::new(1),
                request,
                edge,
                NativeCloseResolution::Accept,
                None,
            )
            .expect("the same native close resolution must be idempotent");
        assert_eq!(repeated_before_emission, first);
        assert_eq!(ledger.records().count(), 1);

        assert_eq!(
            ledger
                .take_new_requests(
                    test_provider(),
                    InventoryGeneration::new(2),
                    |actual| actual == edge,
                )
                .expect("native close extraction must succeed")
                .len(),
            1
        );

        let repeated_after_emission = ledger
            .request_native_close(
                WorkspaceEpoch::new(1),
                request,
                edge,
                NativeCloseResolution::Accept,
                None,
            )
            .expect("the emitted native close resolution must remain idempotent");
        assert_eq!(repeated_after_emission, first);
        assert_eq!(ledger.records().count(), 1);
        assert!(
            ledger
                .take_new_requests(
                    test_provider(),
                    InventoryGeneration::new(4),
                    |actual| actual == edge,
                )
                .expect("duplicate extraction check must succeed")
                .is_empty()
        );
    }

    #[test]
    fn ordinary_request_entry_points_reject_native_close_effects() {
        let mut ledger = EffectLedger::default();
        let binding = binding(1, 1);
        let edge = native_close_edge(binding, 1, 1);
        let native = PlatformEffect::ResolveNativeClose {
            request: close_request(edge),
            edge,
            resolution: NativeCloseResolution::Accept,
        };
        assert_eq!(
            ledger.request(native.clone()),
            Err(EffectLedgerError::NativeCloseRequiresDedicatedRequest)
        );
        assert_eq!(
            ledger.request_in(WorkspaceEpoch::new(1), native),
            Err(EffectLedgerError::NativeCloseRequiresDedicatedRequest)
        );
        assert!(ledger.records().next().is_none());
    }

    #[test]
    fn only_accepted_native_close_is_non_idempotent() {
        let binding = binding(1, 1);
        let edge = native_close_edge(binding, 1, 1);
        let request = close_request(edge);
        assert!(
            PlatformEffect::ResolveNativeClose {
                request,
                edge,
                resolution: NativeCloseResolution::Accept,
            }
            .is_non_idempotent()
        );
        assert!(
            !PlatformEffect::ResolveNativeClose {
                request,
                edge,
                resolution: NativeCloseResolution::Cancel,
            }
            .is_non_idempotent()
        );
    }

    #[test]
    fn request_is_emitted_once_and_indeterminate_never_redispatches() {
        let mut ledger = EffectLedger::default();
        let binding = binding(1, 1);
        let effect = ledger
            .request(create_effect(binding))
            .expect("effect identity must be available");
        assert_eq!(
            take_ordinary_requests(&mut ledger, InventoryGeneration::new(1)).len(),
            1
        );
        assert!(take_ordinary_requests(&mut ledger, InventoryGeneration::new(1)).is_empty());

        assert_eq!(
            ledger.report(
                test_provider(),
                WorkspaceEpoch::new(1),
                EffectResult::new(
                    effect,
                    WorkspaceEpoch::new(1),
                    EffectDispatchResult::Indeterminate(
                        EffectIndeterminateReason::AcknowledgementLost,
                    ),
                ),
            ),
            EffectTransition::Applied
        );
        assert!(take_ordinary_requests(&mut ledger, InventoryGeneration::new(1)).is_empty());
    }

    #[test]
    fn definitive_failure_may_refine_an_indeterminate_dispatch() {
        let mut ledger = EffectLedger::default();
        let effect = ledger
            .request(create_effect(binding(1, 1)))
            .expect("effect identity must be available");
        let requests = take_ordinary_requests(&mut ledger, InventoryGeneration::new(1));
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].id(), effect);
        assert_eq!(
            ledger.report(
                test_provider(),
                WorkspaceEpoch::new(1),
                EffectResult::new(
                    effect,
                    WorkspaceEpoch::new(1),
                    EffectDispatchResult::Indeterminate(
                        EffectIndeterminateReason::AcknowledgementLost,
                    ),
                ),
            ),
            EffectTransition::Applied
        );
        assert_eq!(
            ledger.report(
                test_provider(),
                WorkspaceEpoch::new(1),
                EffectResult::new(
                    effect,
                    WorkspaceEpoch::new(1),
                    EffectDispatchResult::DispatchFailed(DispatchFailureReason::ProviderStopped),
                ),
            ),
            EffectTransition::Applied
        );
        assert_eq!(
            ledger.record(effect).map(EffectRecord::phase),
            Some(EffectPhase::DispatchFailed(
                DispatchFailureReason::ProviderStopped
            ))
        );
    }

    #[test]
    fn cleanup_continuation_dispatch_results_do_not_mutate_the_destructive_predecessor() {
        for (result, expected_phase) in [
            (
                EffectDispatchResult::DispatchFailed(DispatchFailureReason::ProviderStopped),
                EffectPhase::ObservationDispatchFailed(DispatchFailureReason::ProviderStopped),
            ),
            (
                EffectDispatchResult::Unsupported(EffectUnsupportedReason::BackendUnsupported),
                EffectPhase::ObservationUnsupported(EffectUnsupportedReason::BackendUnsupported),
            ),
        ] {
            let mut ledger = EffectLedger::default();
            let binding = binding(1, 1);
            let predecessor = ledger
                .request(PlatformEffect::ReleaseChild { binding })
                .expect("destructive cleanup must allocate");
            let _ = take_ordinary_requests(&mut ledger, InventoryGeneration::new(1));
            let continuation = ledger
                .request_in(
                    WorkspaceEpoch::new(2),
                    PlatformEffect::ContinueCleanup {
                        binding,
                        predecessor,
                        after: None,
                    },
                )
                .expect("observation continuation must allocate");
            let _ = take_ordinary_requests(&mut ledger, InventoryGeneration::new(2));

            assert_eq!(
                ledger.report(
                    test_provider(),
                    WorkspaceEpoch::new(2),
                    EffectResult::new(continuation, WorkspaceEpoch::new(2), result),
                ),
                EffectTransition::Applied
            );
            assert_eq!(
                ledger.record(continuation).map(EffectRecord::phase),
                Some(expected_phase)
            );
            assert_eq!(
                ledger.record(predecessor).map(EffectRecord::phase),
                Some(EffectPhase::Requested)
            );
        }
    }

    #[test]
    fn cleanup_observation_token_is_the_only_cross_epoch_predecessor_result_authority() {
        let mut ledger = EffectLedger::default();
        let (predecessor_provider, observation_provider) = test_provider_replacement();
        let target_binding = binding(1, 1);
        let predecessor = ledger
            .request(PlatformEffect::ReleaseChild {
                binding: target_binding,
            })
            .expect("destructive cleanup must allocate");
        ledger
            .take_new_requests(predecessor_provider, InventoryGeneration::new(1), |_| false)
            .expect("destructive cleanup must emit");
        let continuation = ledger
            .request_in(
                WorkspaceEpoch::new(2),
                PlatformEffect::ContinueCleanup {
                    binding: target_binding,
                    predecessor,
                    after: None,
                },
            )
            .expect("cleanup continuation must allocate");
        let emission = ledger
            .take_new_requests(observation_provider, InventoryGeneration::new(2), |_| false)
            .expect("cleanup continuation must emit")
            .pop()
            .expect("one cleanup continuation must be emitted");
        let token = emission
            .cleanup_observation_token()
            .expect("cleanup continuation must mint observation authority");
        let failure = EffectDispatchResult::DispatchFailed(DispatchFailureReason::ProviderStopped);

        assert_eq!(
            ledger.report(
                observation_provider,
                WorkspaceEpoch::new(2),
                EffectResult::new(predecessor, WorkspaceEpoch::new(1), failure),
            ),
            EffectTransition::StaleEpoch
        );
        let correlated = EffectResult::observed_via_cleanup(token, WorkspaceEpoch::new(1), failure);
        assert_eq!(correlated.receipt_epoch(), WorkspaceEpoch::new(2));
        assert_eq!(
            ledger.report_cleanup_observation(
                observation_provider,
                WorkspaceEpoch::new(2),
                correlated,
            ),
            EffectTransition::Applied
        );
        assert_eq!(
            ledger.record(predecessor).map(EffectRecord::phase),
            Some(EffectPhase::DispatchFailed(
                DispatchFailureReason::ProviderStopped,
            ))
        );
        assert_eq!(
            ledger.record(continuation).map(EffectRecord::phase),
            Some(EffectPhase::CleanupResultObserved { predecessor })
        );
        assert_eq!(
            ledger.report_cleanup_observation(
                observation_provider,
                WorkspaceEpoch::new(2),
                correlated,
            ),
            EffectTransition::Duplicate
        );
        assert_eq!(
            ledger.report_cleanup_observation(
                observation_provider,
                WorkspaceEpoch::new(2),
                EffectResult::observed_via_cleanup(
                    token,
                    WorkspaceEpoch::new(1),
                    EffectDispatchResult::Unsupported(EffectUnsupportedReason::BackendUnsupported,),
                ),
            ),
            EffectTransition::CausalityBarrier
        );
    }

    #[test]
    fn cleanup_observation_rejects_foreign_provider_and_spliced_binding_atomically() {
        let mut ledger = EffectLedger::default();
        let (predecessor_provider, observation_provider) = test_provider_replacement();
        let target_binding = binding(1, 1);
        let predecessor = ledger
            .request(PlatformEffect::ReleaseChild {
                binding: target_binding,
            })
            .expect("destructive cleanup must allocate");
        ledger
            .take_new_requests(predecessor_provider, InventoryGeneration::new(1), |_| false)
            .expect("destructive cleanup must emit");
        let continuation = ledger
            .request_in(
                WorkspaceEpoch::new(2),
                PlatformEffect::ContinueCleanup {
                    binding: target_binding,
                    predecessor,
                    after: None,
                },
            )
            .expect("cleanup continuation must allocate");
        let emission = ledger
            .take_new_requests(observation_provider, InventoryGeneration::new(2), |_| false)
            .expect("cleanup continuation must emit")
            .pop()
            .expect("one cleanup continuation must be emitted");
        let token = emission
            .cleanup_observation_token()
            .expect("cleanup continuation must mint observation authority");
        let failure = EffectDispatchResult::DispatchFailed(DispatchFailureReason::ProviderStopped);

        assert_eq!(
            ledger.report_cleanup_observation(
                predecessor_provider,
                WorkspaceEpoch::new(2),
                EffectResult::observed_via_cleanup(token, WorkspaceEpoch::new(1), failure),
            ),
            EffectTransition::ProviderMismatch
        );
        assert_eq!(
            ledger.record(predecessor).map(EffectRecord::phase),
            Some(EffectPhase::Requested)
        );
        assert_eq!(
            ledger.record(continuation).map(EffectRecord::phase),
            Some(EffectPhase::Requested)
        );

        let spliced = CleanupObservationToken {
            binding: binding(1, 2),
            ..token
        };
        assert_eq!(
            ledger.report_cleanup_observation(
                observation_provider,
                WorkspaceEpoch::new(2),
                EffectResult::observed_via_cleanup(spliced, WorkspaceEpoch::new(1), failure),
            ),
            EffectTransition::BindingMismatch
        );
        assert_eq!(
            ledger.record(predecessor).map(EffectRecord::phase),
            Some(EffectPhase::Requested)
        );
        assert_eq!(
            ledger.record(continuation).map(EffectRecord::phase),
            Some(EffectPhase::Requested)
        );
    }

    #[test]
    fn indeterminate_cleanup_observation_remains_live_until_a_definitive_result() {
        let mut ledger = EffectLedger::default();
        let (predecessor_provider, observation_provider) = test_provider_replacement();
        let binding = binding(1, 1);
        let predecessor = ledger
            .request(PlatformEffect::ReleaseChild { binding })
            .expect("destructive cleanup must allocate");
        ledger
            .take_new_requests(predecessor_provider, InventoryGeneration::new(1), |_| false)
            .expect("destructive cleanup must emit");
        ledger.revoke_provider_authority(predecessor_provider);
        let continuation = ledger
            .request_in(
                WorkspaceEpoch::new(2),
                PlatformEffect::ContinueCleanup {
                    binding,
                    predecessor,
                    after: None,
                },
            )
            .expect("cleanup continuation must allocate");
        let emission = ledger
            .take_new_requests(observation_provider, InventoryGeneration::new(2), |_| false)
            .expect("cleanup continuation must emit")
            .pop()
            .expect("one cleanup continuation must be emitted");
        let token = emission
            .cleanup_observation_token()
            .expect("cleanup continuation must mint observation authority");

        let indeterminate = EffectResult::observed_via_cleanup(
            token,
            WorkspaceEpoch::new(1),
            EffectDispatchResult::Indeterminate(EffectIndeterminateReason::AcknowledgementLost),
        );
        assert_eq!(
            ledger.report_cleanup_observation(
                observation_provider,
                WorkspaceEpoch::new(2),
                indeterminate,
            ),
            EffectTransition::Applied
        );
        assert_eq!(
            ledger.record(continuation).map(EffectRecord::phase),
            Some(EffectPhase::CleanupObservationIndeterminate {
                predecessor,
                reason: EffectIndeterminateReason::AcknowledgementLost,
            })
        );
        assert_eq!(
            ledger.record(predecessor).map(EffectRecord::phase),
            Some(EffectPhase::Indeterminate(
                EffectIndeterminateReason::ProviderRestarted,
            )),
            "the observer owns its acknowledgement-loss reason without erasing provider revocation",
        );
        ledger.mark_boundary_published();
        assert_eq!(ledger.compact_published_terminal(&BTreeSet::new()), 0);

        let definitive = EffectResult::observed_via_cleanup(
            token,
            WorkspaceEpoch::new(1),
            EffectDispatchResult::Unsupported(EffectUnsupportedReason::BackendUnsupported),
        );
        assert_eq!(
            ledger.report_cleanup_observation(
                observation_provider,
                WorkspaceEpoch::new(2),
                definitive,
            ),
            EffectTransition::Applied
        );
        assert_eq!(
            ledger.record(continuation).map(EffectRecord::phase),
            Some(EffectPhase::CleanupResultObserved { predecessor })
        );
        assert_eq!(
            ledger.record(predecessor).map(EffectRecord::phase),
            Some(EffectPhase::Unsupported(
                EffectUnsupportedReason::BackendUnsupported,
            ))
        );
        ledger.mark_boundary_published();
        assert_eq!(ledger.compact_published_terminal(&BTreeSet::new()), 1);
        assert!(ledger.record(continuation).is_none());
    }

    #[test]
    fn provider_replacement_supersedes_an_indeterminate_cleanup_observer() {
        let mut ledger = EffectLedger::default();
        let (predecessor_provider, observation_provider) = test_provider_replacement();
        let binding = binding(1, 1);
        let predecessor = ledger
            .request(PlatformEffect::ReleaseChild { binding })
            .expect("destructive cleanup must allocate");
        ledger
            .take_new_requests(predecessor_provider, InventoryGeneration::new(1), |_| false)
            .expect("destructive cleanup must emit");
        let continuation = ledger
            .request_in(
                WorkspaceEpoch::new(2),
                PlatformEffect::ContinueCleanup {
                    binding,
                    predecessor,
                    after: None,
                },
            )
            .expect("cleanup continuation must allocate");
        let token = ledger
            .take_new_requests(observation_provider, InventoryGeneration::new(2), |_| false)
            .expect("cleanup continuation must emit")
            .pop()
            .and_then(|emission| emission.cleanup_observation_token())
            .expect("cleanup continuation must mint observation authority");
        assert_eq!(
            ledger.report_cleanup_observation(
                observation_provider,
                WorkspaceEpoch::new(2),
                EffectResult::observed_via_cleanup(
                    token,
                    WorkspaceEpoch::new(1),
                    EffectDispatchResult::Indeterminate(
                        EffectIndeterminateReason::AcknowledgementLost,
                    ),
                ),
            ),
            EffectTransition::Applied
        );

        ledger.revoke_provider_authority(observation_provider);
        assert_eq!(
            ledger.record(continuation).map(EffectRecord::phase),
            Some(EffectPhase::Invalidated {
                cause: EffectInvalidation::PlatformProviderReplaced {
                    provider: observation_provider,
                },
            })
        );
        ledger.mark_boundary_published();
        assert_eq!(ledger.compact_published_terminal(&BTreeSet::new()), 1);
        assert!(ledger.record(continuation).is_none());
        assert!(ledger.record(predecessor).is_some());
    }

    #[test]
    fn conflicting_indeterminate_cleanup_observation_is_rejected_atomically() {
        let mut ledger = EffectLedger::default();
        let (predecessor_provider, observation_provider) = test_provider_replacement();
        let binding = binding(1, 1);
        let predecessor = ledger
            .request(PlatformEffect::ReleaseChild { binding })
            .expect("destructive cleanup must allocate");
        ledger
            .take_new_requests(predecessor_provider, InventoryGeneration::new(1), |_| false)
            .expect("destructive cleanup must emit");
        let continuation = ledger
            .request_in(
                WorkspaceEpoch::new(2),
                PlatformEffect::ContinueCleanup {
                    binding,
                    predecessor,
                    after: None,
                },
            )
            .expect("cleanup continuation must allocate");
        let token = ledger
            .take_new_requests(observation_provider, InventoryGeneration::new(2), |_| false)
            .expect("cleanup continuation must emit")
            .pop()
            .and_then(|emission| emission.cleanup_observation_token())
            .expect("cleanup continuation must mint observation authority");
        let first = EffectResult::observed_via_cleanup(
            token,
            WorkspaceEpoch::new(1),
            EffectDispatchResult::Indeterminate(EffectIndeterminateReason::AcknowledgementLost),
        );
        assert_eq!(
            ledger.report_cleanup_observation(observation_provider, WorkspaceEpoch::new(2), first,),
            EffectTransition::Applied
        );
        let before = ledger.clone();
        assert_eq!(
            ledger.report_cleanup_observation(
                observation_provider,
                WorkspaceEpoch::new(2),
                EffectResult::observed_via_cleanup(
                    token,
                    WorkspaceEpoch::new(1),
                    EffectDispatchResult::Indeterminate(
                        EffectIndeterminateReason::ProviderRestarted,
                    ),
                ),
            ),
            EffectTransition::CausalityBarrier
        );
        assert_eq!(ledger.records, before.records);
        assert_eq!(
            ledger.report_cleanup_observation(observation_provider, WorkspaceEpoch::new(2), first,),
            EffectTransition::Duplicate
        );
        assert_eq!(
            ledger.record(continuation).map(EffectRecord::phase),
            Some(EffectPhase::CleanupObservationIndeterminate {
                predecessor,
                reason: EffectIndeterminateReason::AcknowledgementLost,
            })
        );
    }

    #[test]
    fn cleanup_observation_churn_retains_only_subject_and_current_observer() {
        let mut ledger = EffectLedger::default();
        let provider = test_provider();
        let binding = binding(1, 1);
        let predecessor = ledger
            .request(PlatformEffect::ReleaseChild { binding })
            .expect("destructive cleanup must allocate");
        ledger
            .take_new_requests(provider, InventoryGeneration::new(1), |_| false)
            .expect("destructive cleanup must emit");
        let mut current = ledger
            .request_in(
                WorkspaceEpoch::new(1),
                PlatformEffect::ContinueCleanup {
                    binding,
                    predecessor,
                    after: None,
                },
            )
            .expect("first cleanup observer must allocate");
        ledger
            .take_new_requests(provider, InventoryGeneration::new(2), |_| false)
            .expect("first cleanup observer must emit");

        for generation in 3..=10_002 {
            let successor = ledger
                .request_in(
                    WorkspaceEpoch::new(1),
                    PlatformEffect::ContinueCleanup {
                        binding,
                        predecessor,
                        after: Some(current),
                    },
                )
                .expect("successor cleanup observer must allocate");
            assert_eq!(
                ledger.supersede_cleanup_observation(current, successor, predecessor, binding,),
                EffectTransition::Applied
            );
            ledger
                .take_new_requests(provider, InventoryGeneration::new(generation), |_| false)
                .expect("successor cleanup observer must emit");
            ledger.mark_boundary_published();
            assert_eq!(ledger.compact_published_terminal(&BTreeSet::new()), 1);
            assert_eq!(ledger.records.len(), 2);
            current = successor;
        }

        assert!(ledger.record(predecessor).is_some());
        assert!(ledger.record(current).is_some());
    }

    #[test]
    fn stale_duplicate_and_binding_mismatch_results_are_harmless() {
        let mut ledger = EffectLedger::default();
        let current_binding = binding(4, 1);
        let effect = ledger
            .request(create_effect(current_binding))
            .expect("effect identity must be available");
        let result = EffectResult::new(
            effect,
            WorkspaceEpoch::new(4),
            EffectDispatchResult::DispatchFailed(DispatchFailureReason::AdapterRejected),
        );
        assert_eq!(
            ledger.report(test_provider(), WorkspaceEpoch::new(5), result),
            EffectTransition::StaleEpoch
        );
        let requests = take_ordinary_requests(&mut ledger, InventoryGeneration::new(1));
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].id(), effect);
        assert_eq!(
            ledger.report(test_provider(), WorkspaceEpoch::new(4), result),
            EffectTransition::Applied
        );
        assert_eq!(
            ledger.report(test_provider(), WorkspaceEpoch::new(4), result),
            EffectTransition::Duplicate
        );
        assert_eq!(
            ledger.mark_observed_applied(
                test_provider(),
                effect,
                binding(4, 2),
                InventoryGeneration::new(1),
            ),
            EffectTransition::BindingMismatch
        );
        assert_eq!(
            ledger.mark_observed_applied(
                test_provider(),
                effect,
                current_binding,
                InventoryGeneration::new(2),
            ),
            EffectTransition::Applied
        );
    }

    #[test]
    fn effect_identity_exhaustion_does_not_mutate_the_ledger() {
        let mut ledger = EffectLedger::default();
        ledger.exhaust_effect_ids();
        let before = ledger.clone();
        assert_eq!(
            ledger.request(create_effect(binding(0, 1))),
            Err(EffectLedgerError::EffectIdExhausted)
        );
        assert_eq!(ledger, before);
    }

    #[test]
    fn restore_can_issue_cleanup_for_an_exact_older_binding() {
        let mut ledger = EffectLedger::default();
        let old_binding = binding(3, 1);
        let effect = ledger
            .request_in(
                WorkspaceEpoch::new(4),
                PlatformEffect::CompensatingClose {
                    binding: old_binding,
                    compensates: EffectId::new(7),
                },
            )
            .expect("cleanup identity must be available");
        let request = ledger
            .take_new_requests(test_provider(), InventoryGeneration::new(1), |_| false)
            .expect("cleanup extraction must succeed")
            .pop()
            .expect("cleanup must be emitted");
        assert_eq!(request.epoch(), WorkspaceEpoch::new(4));
        assert_eq!(request.effect().binding(), old_binding);
        assert_eq!(
            ledger.mark_destroyed(effect, old_binding, InventoryGeneration::new(9)),
            EffectTransition::Applied
        );
    }

    #[test]
    fn restore_invalidates_only_never_emitted_older_requests() {
        let mut ledger = EffectLedger::default();
        let first_old_emitted = ledger
            .request(create_effect(binding(1, 1)))
            .expect("old request must allocate");
        let old_emitted = ledger
            .request(create_effect(binding(2, 2)))
            .expect("second request must allocate");
        let _ = take_ordinary_requests(&mut ledger, InventoryGeneration::new(1));
        let current = ledger
            .request(create_effect(binding(3, 3)))
            .expect("current request must allocate");

        // Create a distinct never-emitted old request after draining the earlier records.
        let late_old = ledger
            .request_in(WorkspaceEpoch::new(1), create_effect(binding(1, 4)))
            .expect("late old request must allocate");
        ledger.invalidate_unemitted_for_workspace_replacement(WorkspaceEpoch::new(3));

        assert!(
            ledger
                .record(first_old_emitted)
                .is_some_and(EffectRecord::was_emitted)
        );
        assert!(
            ledger
                .record(old_emitted)
                .is_some_and(EffectRecord::was_emitted)
        );
        assert_eq!(
            ledger.record(late_old).map(EffectRecord::phase),
            Some(EffectPhase::Invalidated {
                cause: EffectInvalidation::WorkspaceReplaced {
                    replacement_epoch: WorkspaceEpoch::new(3),
                },
            })
        );
        let expected_current = ledger
            .record(current)
            .expect("current record must remain")
            .request()
            .clone();
        let emissions = take_ordinary_requests(&mut ledger, InventoryGeneration::new(2));
        assert_eq!(emissions.len(), 1);
        assert_eq!(emissions[0].request(), &expected_current);
    }

    #[test]
    fn exact_unemitted_invalidation_prevents_same_tick_extraction() {
        let mut ledger = EffectLedger::default();
        let effect = ledger
            .request(create_effect(binding(1, 1)))
            .expect("create request must allocate");

        assert_eq!(
            ledger.invalidate_unemitted(effect, EffectInvalidation::NativeCreateAborted),
            EffectInvalidationTransition::Applied
        );
        assert!(
            take_ordinary_requests(&mut ledger, InventoryGeneration::new(1)).is_empty(),
            "an invalidated create must never reach the adapter"
        );
        assert_eq!(
            ledger.record(effect).map(EffectRecord::phase),
            Some(EffectPhase::Invalidated {
                cause: EffectInvalidation::NativeCreateAborted,
            })
        );
    }

    #[test]
    fn emission_and_observation_are_bound_to_the_exact_provider() {
        let mut ledger = EffectLedger::default();
        let (first, replacement) = test_provider_replacement();
        let binding = binding(1, 1);
        let effect = ledger
            .request(create_effect(binding))
            .expect("effect must allocate");

        let emissions = ledger
            .take_new_requests(first, InventoryGeneration::new(1), |_| false)
            .expect("effect must emit");
        assert_eq!(emissions.len(), 1);
        assert_eq!(emissions[0].provider(), first);
        assert_eq!(
            emissions[0].delivery().inventory_generation(),
            InventoryGeneration::new(1)
        );
        assert_eq!(
            ledger.record(effect).and_then(EffectRecord::provider),
            Some(first)
        );

        let result = EffectResult::new(
            effect,
            WorkspaceEpoch::new(1),
            EffectDispatchResult::DispatchFailed(DispatchFailureReason::ProviderStopped),
        );
        assert_eq!(
            ledger.report(replacement, WorkspaceEpoch::new(1), result),
            EffectTransition::ProviderMismatch
        );
        assert_eq!(
            ledger
                .mark_observed_applied(replacement, effect, binding, InventoryGeneration::new(2),),
            EffectTransition::ProviderMismatch
        );
        assert_eq!(
            ledger.record(effect).map(EffectRecord::phase),
            Some(EffectPhase::Requested)
        );
        assert_eq!(
            ledger.mark_observed_applied(first, effect, binding, InventoryGeneration::new(2)),
            EffectTransition::Applied
        );
    }

    #[test]
    fn same_batch_causal_predecessor_is_delivered_to_one_provider() {
        let mut ledger = EffectLedger::default();
        let provider = test_provider();
        let binding = binding(1, 1);
        let predecessor = ledger
            .request(PlatformEffect::SetPointerPassthrough {
                binding,
                enabled: true,
                after: None,
            })
            .expect("predecessor must allocate");
        let successor = ledger
            .request(PlatformEffect::SetPointerPassthrough {
                binding,
                enabled: false,
                after: Some(predecessor),
            })
            .expect("successor must allocate");

        let emissions = ledger
            .take_new_requests(provider, InventoryGeneration::new(1), |_| false)
            .expect("same-batch causal chain must emit atomically");
        assert_eq!(
            emissions
                .iter()
                .map(PlatformEffectEmission::id)
                .collect::<Vec<_>>(),
            vec![predecessor, successor]
        );
        assert!(
            emissions
                .iter()
                .all(|emission| emission.provider() == provider)
        );
    }

    #[test]
    fn cross_provider_after_dependency_rejects_the_complete_batch() {
        let mut ledger = EffectLedger::default();
        let (first, replacement) = test_provider_replacement();
        let binding = binding(1, 1);
        let predecessor = ledger
            .request(PlatformEffect::SetPointerPassthrough {
                binding,
                enabled: true,
                after: None,
            })
            .expect("predecessor must allocate");
        ledger
            .take_new_requests(first, InventoryGeneration::new(1), |_| false)
            .expect("predecessor must emit");
        let unrelated = ledger
            .request(create_effect(binding))
            .expect("unrelated effect must allocate");
        let successor = ledger
            .request(PlatformEffect::SetPointerPassthrough {
                binding,
                enabled: false,
                after: Some(predecessor),
            })
            .expect("successor must allocate");
        let before = ledger.clone();

        assert_eq!(
            ledger.take_new_requests(replacement, InventoryGeneration::new(2), |_| false),
            Err(EffectLedgerError::CausalPredecessorProviderMismatch {
                effect: successor,
                predecessor,
                requested_provider: replacement,
                delivered_provider: first,
            })
        );
        assert_eq!(ledger, before, "failed extraction must be atomic");
        assert_eq!(
            ledger.record(unrelated).and_then(EffectRecord::delivery),
            None
        );
        assert_eq!(
            ledger.record(successor).and_then(EffectRecord::delivery),
            None
        );
    }

    #[test]
    fn cross_provider_cleanup_subject_is_observable_by_a_replacement_provider() {
        let mut ledger = EffectLedger::default();
        let (first, replacement) = test_provider_replacement();
        let binding = binding(1, 1);
        let predecessor = ledger
            .request(PlatformEffect::ReleaseChild { binding })
            .expect("predecessor must allocate");
        ledger
            .take_new_requests(first, InventoryGeneration::new(1), |_| false)
            .expect("predecessor must emit");
        let continuation = ledger
            .request_in(
                WorkspaceEpoch::new(2),
                PlatformEffect::ContinueCleanup {
                    binding,
                    predecessor,
                    after: None,
                },
            )
            .expect("continuation must allocate");

        let emissions = ledger
            .take_new_requests(replacement, InventoryGeneration::new(2), |_| false)
            .expect("the destructive subject may predate the observation provider");
        assert_eq!(emissions.len(), 1);
        assert_eq!(emissions[0].id(), continuation);
        assert_eq!(emissions[0].delivery().provider(), replacement);
    }

    #[test]
    fn cross_provider_cleanup_observation_lane_is_rejected() {
        let mut ledger = EffectLedger::default();
        let (first, replacement) = test_provider_replacement();
        let binding = binding(1, 1);
        let predecessor = ledger
            .request(PlatformEffect::ReleaseChild { binding })
            .expect("destructive subject must allocate");
        ledger
            .take_new_requests(first, InventoryGeneration::new(1), |_| false)
            .expect("destructive subject must emit");
        let old_observer = ledger
            .request_in(
                WorkspaceEpoch::new(2),
                PlatformEffect::ContinueCleanup {
                    binding,
                    predecessor,
                    after: None,
                },
            )
            .expect("old observer must allocate");
        ledger
            .take_new_requests(first, InventoryGeneration::new(2), |_| false)
            .expect("old observer must emit");
        let successor = ledger
            .request_in(
                WorkspaceEpoch::new(2),
                PlatformEffect::ContinueCleanup {
                    binding,
                    predecessor,
                    after: Some(old_observer),
                },
            )
            .expect("successor observer must allocate");

        assert_eq!(
            ledger.take_new_requests(replacement, InventoryGeneration::new(3), |_| false),
            Err(EffectLedgerError::CausalPredecessorProviderMismatch {
                effect: successor,
                predecessor: old_observer,
                requested_provider: replacement,
                delivered_provider: first,
            })
        );
    }

    #[test]
    fn provider_cutover_invalidates_only_transitive_queued_causal_successors() {
        let mut ledger = EffectLedger::default();
        let (first, replacement) = test_provider_replacement();
        let binding = binding(1, 1);
        let predecessor = ledger
            .request(PlatformEffect::SetPointerPassthrough {
                binding,
                enabled: true,
                after: None,
            })
            .expect("predecessor must allocate");
        ledger
            .take_new_requests(first, InventoryGeneration::new(1), |_| false)
            .expect("predecessor must emit");
        let direct = ledger
            .request(PlatformEffect::SetPointerPassthrough {
                binding,
                enabled: false,
                after: Some(predecessor),
            })
            .expect("direct successor must allocate");
        let transitive = ledger
            .request(PlatformEffect::SetPointerPassthrough {
                binding,
                enabled: true,
                after: Some(direct),
            })
            .expect("transitive successor must allocate");
        let independent = ledger
            .request(create_effect(binding))
            .expect("independent effect must allocate");

        assert_eq!(
            ledger.invalidate_unemitted_causal_successors_for_provider_replacement(first),
            vec![direct, transitive]
        );
        for effect in [direct, transitive] {
            assert_eq!(
                ledger.record(effect).map(EffectRecord::phase),
                Some(EffectPhase::Invalidated {
                    cause: EffectInvalidation::PlatformProviderReplaced { provider: first },
                })
            );
            assert_eq!(ledger.record(effect).and_then(EffectRecord::delivery), None);
        }
        assert_eq!(
            ledger.record(independent).map(EffectRecord::phase),
            Some(EffectPhase::Requested)
        );

        let emissions = ledger
            .take_new_requests(replacement, InventoryGeneration::new(2), |_| false)
            .expect("independent request may use replacement provider");
        assert_eq!(emissions.len(), 1);
        assert_eq!(emissions[0].id(), independent);
        assert_eq!(emissions[0].provider(), replacement);
    }

    #[test]
    fn unavailable_predecessor_rejects_extraction_without_mutation() {
        let mut ledger = EffectLedger::default();
        let missing = EffectId::new(900);
        let successor = ledger
            .request(PlatformEffect::RequestFocus {
                binding: binding(1, 1),
                after: Some(missing),
            })
            .expect("successor must allocate");
        let before = ledger.clone();

        assert_eq!(
            ledger.take_new_requests(test_provider(), InventoryGeneration::new(1), |_| false),
            Err(EffectLedgerError::CausalPredecessorUnavailable {
                effect: successor,
                predecessor: missing,
            })
        );
        assert_eq!(ledger, before);
    }

    #[test]
    fn provider_revocation_preserves_delivery_and_terminal_history() {
        let mut ledger = EffectLedger::default();
        let (first, replacement) = test_provider_replacement();
        let binding = binding(1, 1);
        let outstanding = ledger
            .request(create_effect(binding))
            .expect("outstanding effect must allocate");
        let indeterminate = ledger
            .request(create_effect(binding))
            .expect("indeterminate effect must allocate");
        let terminal = ledger
            .request(create_effect(binding))
            .expect("terminal effect must allocate");
        ledger
            .take_new_requests(first, InventoryGeneration::new(1), |_| false)
            .expect("initial effects must emit");
        assert_eq!(
            ledger.report(
                first,
                WorkspaceEpoch::new(1),
                EffectResult::new(
                    indeterminate,
                    WorkspaceEpoch::new(1),
                    EffectDispatchResult::Indeterminate(
                        EffectIndeterminateReason::AcknowledgementLost,
                    ),
                ),
            ),
            EffectTransition::Applied
        );
        assert_eq!(
            ledger.report(
                first,
                WorkspaceEpoch::new(1),
                EffectResult::new(
                    terminal,
                    WorkspaceEpoch::new(1),
                    EffectDispatchResult::DispatchFailed(DispatchFailureReason::ProviderStopped,),
                ),
            ),
            EffectTransition::Applied
        );
        let queued = ledger
            .request(create_effect(binding))
            .expect("queued effect must allocate");

        ledger.revoke_provider_authority(first);

        for effect in [outstanding, indeterminate] {
            let record = ledger.record(effect).expect("emitted record must remain");
            assert_eq!(
                record.phase(),
                EffectPhase::Indeterminate(EffectIndeterminateReason::ProviderRestarted)
            );
            assert_eq!(record.provider(), Some(first));
        }
        assert_eq!(
            ledger.record(terminal).map(EffectRecord::phase),
            Some(EffectPhase::DispatchFailed(
                DispatchFailureReason::ProviderStopped,
            ))
        );
        let queued_record = ledger.record(queued).expect("queued record must remain");
        assert_eq!(queued_record.phase(), EffectPhase::Requested);
        assert_eq!(queued_record.delivery(), None);
        assert_eq!(
            ledger.report(
                replacement,
                WorkspaceEpoch::new(1),
                EffectResult::new(
                    outstanding,
                    WorkspaceEpoch::new(1),
                    EffectDispatchResult::DispatchFailed(DispatchFailureReason::ProviderStopped,),
                ),
            ),
            EffectTransition::ProviderMismatch
        );
        assert_eq!(
            ledger.report(
                first,
                WorkspaceEpoch::new(1),
                EffectResult::new(
                    outstanding,
                    WorkspaceEpoch::new(1),
                    EffectDispatchResult::DispatchFailed(DispatchFailureReason::ProviderStopped,),
                ),
            ),
            EffectTransition::ProviderMismatch,
            "a delayed result from the revoked provider must remain inert"
        );
        assert_eq!(
            ledger.take_new_requests(first, InventoryGeneration::new(2), |_| false),
            Err(EffectLedgerError::ProviderAuthorityRevoked { provider: first })
        );

        let replacement_emissions = ledger
            .take_new_requests(replacement, InventoryGeneration::new(2), |_| false)
            .expect("never-emitted request may move to the replacement provider");
        assert_eq!(replacement_emissions.len(), 1);
        assert_eq!(replacement_emissions[0].id(), queued);
        assert_eq!(replacement_emissions[0].provider(), replacement);
        assert_eq!(
            ledger.record(outstanding).and_then(EffectRecord::provider),
            Some(first)
        );
    }

    #[test]
    fn destruction_is_exact_state_and_never_fabricates_delivery() {
        let mut ledger = EffectLedger::default();
        let binding = binding(1, 1);
        let effect = ledger
            .request(create_effect(binding))
            .expect("effect must allocate");

        assert_eq!(
            ledger.mark_destroyed(effect, binding, InventoryGeneration::new(9)),
            EffectTransition::Applied
        );
        let record = ledger.record(effect).expect("destroyed record must remain");
        assert_eq!(record.delivery(), None);
        assert_eq!(
            record.phase(),
            EffectPhase::Destroyed {
                inventory_generation: InventoryGeneration::new(9),
            }
        );
        assert!(
            ledger
                .take_new_requests(test_provider(), InventoryGeneration::new(10), |_| false)
                .expect("destroyed state must not corrupt extraction")
                .is_empty()
        );
    }
}
