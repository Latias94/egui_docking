//! Affine native-effect requests exposed by the renderer-neutral runtime.

use std::sync::{Arc, Mutex, PoisonError};

use thiserror::Error;

use super::native::NativeSurfaceBinding;
use crate::effect::{
    CleanupObservationToken, DispatchFailureReason, EffectDispatchResult, EffectId,
    EffectIndeterminateReason, EffectResult, EffectTransition, EffectUnsupportedReason,
    NativeCloseResolution, PlatformEffect, PlatformEffectEmission,
};
use crate::geometry::PhysicalRect;
use crate::ids::WorkspaceEpoch;
use crate::platform_provider::PlatformObservationLease;
use crate::presentation_observation::PresentedNativeStagingPresentation;
use crate::viewport::{PresentationObservationGeneration, ViewportBinding, ViewportRole};

/// Opaque stable identity of one core-emitted native effect.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct NativeEffectHandle(EffectId);

impl NativeEffectHandle {
    pub(super) const fn from_core(effect: EffectId) -> Self {
        Self(effect)
    }
}

/// Public native-window ownership role without exposing viewport internals.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NativeSurfaceRole {
    /// Application-owned root window.
    Root,
    /// Dockspace-owned child window.
    Child,
}

impl From<ViewportRole> for NativeSurfaceRole {
    fn from(role: ViewportRole) -> Self {
        match role {
            ViewportRole::Root => Self::Root,
            ViewportRole::Child => Self::Child,
        }
    }
}

/// Opaque hidden-presentation generation required before a native show.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct NativeHiddenPresentationProof(PresentationObservationGeneration);

/// Opaque first-live staging output proof required before a native show.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct NativePreShowPresentationProof(PresentedNativeStagingPresentation);

/// Exact adapter operation requested by the core.
///
/// The order in [`super::HostFrameReport::take_native_effects`] is normative.
/// Hosts must not deduplicate, merge, or reorder requests.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq)]
pub enum NativeEffectOperation {
    /// Create one hidden native window.
    CreateWindow {
        /// Exact logical/native binding to create.
        surface: NativeSurfaceBinding,
        /// Exact requested outer placement.
        placement: PhysicalRect,
        /// Requested ownership role.
        role: NativeSurfaceRole,
    },
    /// Show a previously staged hidden native window.
    ShowWindow {
        /// Exact logical/native binding to show.
        surface: NativeSurfaceBinding,
        /// Hidden observation which must causally precede the show.
        after_hidden: NativeHiddenPresentationProof,
        /// Exact retained staging output which must have been presented first.
        after_pre_show: NativePreShowPresentationProof,
    },
    /// Close a staging window created by an operation which did not complete.
    CompensatingClose {
        /// Exact logical/native binding to close.
        surface: NativeSurfaceBinding,
        /// Create or replacement effect whose staged lifetime is being compensated.
        compensates: NativeEffectHandle,
    },
    /// Cancel an application-root native close request.
    CancelRootClose {
        /// Exact logical/native binding whose close must be cancelled.
        surface: NativeSurfaceBinding,
    },
    /// Retain ownership of a child window.
    RetainChild {
        /// Exact logical/native child binding.
        surface: NativeSurfaceBinding,
    },
    /// Release and destroy a child window.
    ReleaseChild {
        /// Exact logical/native child binding.
        surface: NativeSurfaceBinding,
    },
    /// Observe a destructive predecessor without executing it again.
    ObserveCleanup {
        /// Exact logical/native binding under observation.
        surface: NativeSurfaceBinding,
        /// Destructive operation whose delayed result is being observed.
        predecessor: NativeEffectHandle,
        /// Previous observation request in this provider lane.
        after: Option<NativeEffectHandle>,
    },
    /// Request closure of an application-root window.
    RequestRootClose {
        /// Exact logical/native root binding.
        surface: NativeSurfaceBinding,
    },
    /// Change pointer pass-through for one exact native binding.
    SetPointerPassthrough {
        /// Exact logical/native binding.
        surface: NativeSurfaceBinding,
        /// Whether the native window must ignore pointer input.
        enabled: bool,
        /// Previous request in this exact property lane.
        after: Option<NativeEffectHandle>,
    },
    /// Request native focus for one exact binding.
    RequestFocus {
        /// Exact logical/native binding.
        surface: NativeSurfaceBinding,
        /// Previous request in the global native-focus lane.
        after: Option<NativeEffectHandle>,
    },
    /// Create a replacement native lifetime at an exact outer placement.
    RequestReplacement {
        /// Exact replacement binding.
        surface: NativeSurfaceBinding,
        /// Exact requested outer placement.
        placement: PhysicalRect,
        /// Requested ownership role.
        role: NativeSurfaceRole,
    },
    /// Resolve one exact native close edge.
    ResolveNativeClose {
        /// Exact provider-bound close edge which authorized this resolution.
        close: super::NativeSurfaceCloseRequest,
        /// Whether the host must accept or cancel the close.
        resolution: NativeCloseDisposition,
        /// Previous resolution request in this native-close lane.
        after: Option<NativeEffectHandle>,
    },
}

impl NativeEffectOperation {
    /// Returns the exact logical/native surface affected by this operation.
    #[must_use]
    pub const fn surface(&self) -> NativeSurfaceBinding {
        match self {
            Self::CreateWindow { surface, .. }
            | Self::ShowWindow { surface, .. }
            | Self::CompensatingClose { surface, .. }
            | Self::CancelRootClose { surface }
            | Self::RetainChild { surface }
            | Self::ReleaseChild { surface }
            | Self::ObserveCleanup { surface, .. }
            | Self::RequestRootClose { surface }
            | Self::SetPointerPassthrough { surface, .. }
            | Self::RequestFocus { surface, .. }
            | Self::RequestReplacement { surface, .. } => *surface,
            Self::ResolveNativeClose { close, .. } => close.binding(),
        }
    }
}

/// Requested native close disposition carried by an emitted effect.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NativeCloseDisposition {
    /// Accept the native close.
    Accept,
    /// Cancel the native close.
    Cancel,
}

impl From<NativeCloseResolution> for NativeCloseDisposition {
    fn from(resolution: NativeCloseResolution) -> Self {
        match resolution {
            NativeCloseResolution::Accept => Self::Accept,
            NativeCloseResolution::Cancel => Self::Cancel,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NativeEffectCorrelationKind {
    Input,
    Presentation,
    Close,
    Cleanup,
    ExternalFact,
}

/// One affine core-emitted request which has not yet been classified by the host.
///
/// Consuming the request yields the operation and the sole dispatch receipt.
/// The type deliberately does not implement `Clone` or `Copy`.
#[must_use = "native effects must be dispatched, rejected, or retained explicitly"]
#[derive(Debug)]
pub struct NativeEffectRequest {
    operation: Option<NativeEffectOperation>,
    receipt: Option<NativeEffectReceipt>,
}

impl NativeEffectRequest {
    pub(super) fn from_emission(
        emission: &PlatformEffectEmission,
        abandoned: NativeEffectDropQueue,
    ) -> Self {
        let binding = emission.effect().binding();
        let surface = NativeSurfaceBinding::from_binding(emission.provider(), binding);
        let (operation, correlation) = match emission.effect() {
            PlatformEffect::CreateWindow {
                placement, role, ..
            } => (
                NativeEffectOperation::CreateWindow {
                    surface,
                    placement: *placement,
                    role: (*role).into(),
                },
                NativeEffectCorrelationKind::ExternalFact,
            ),
            PlatformEffect::ShowWindow {
                after_hidden,
                after_pre_show,
                ..
            } => (
                NativeEffectOperation::ShowWindow {
                    surface,
                    after_hidden: NativeHiddenPresentationProof(*after_hidden),
                    after_pre_show: NativePreShowPresentationProof(*after_pre_show),
                },
                NativeEffectCorrelationKind::Presentation,
            ),
            PlatformEffect::CompensatingClose { compensates, .. } => (
                NativeEffectOperation::CompensatingClose {
                    surface,
                    compensates: NativeEffectHandle::from_core(*compensates),
                },
                NativeEffectCorrelationKind::Close,
            ),
            PlatformEffect::CancelRootClose { .. } => (
                NativeEffectOperation::CancelRootClose { surface },
                NativeEffectCorrelationKind::Close,
            ),
            PlatformEffect::RetainChild { .. } => (
                NativeEffectOperation::RetainChild { surface },
                NativeEffectCorrelationKind::ExternalFact,
            ),
            PlatformEffect::ReleaseChild { .. } => (
                NativeEffectOperation::ReleaseChild { surface },
                NativeEffectCorrelationKind::Close,
            ),
            PlatformEffect::ContinueCleanup {
                predecessor, after, ..
            } => (
                NativeEffectOperation::ObserveCleanup {
                    surface,
                    predecessor: NativeEffectHandle::from_core(*predecessor),
                    after: after.map(NativeEffectHandle::from_core),
                },
                NativeEffectCorrelationKind::Cleanup,
            ),
            PlatformEffect::RequestRootClose { .. } => (
                NativeEffectOperation::RequestRootClose { surface },
                NativeEffectCorrelationKind::Close,
            ),
            PlatformEffect::SetPointerPassthrough { enabled, after, .. } => (
                NativeEffectOperation::SetPointerPassthrough {
                    surface,
                    enabled: *enabled,
                    after: after.map(NativeEffectHandle::from_core),
                },
                NativeEffectCorrelationKind::Input,
            ),
            PlatformEffect::RequestFocus { after, .. } => (
                NativeEffectOperation::RequestFocus {
                    surface,
                    after: after.map(NativeEffectHandle::from_core),
                },
                NativeEffectCorrelationKind::ExternalFact,
            ),
            PlatformEffect::RequestReplacement {
                placement, role, ..
            } => (
                NativeEffectOperation::RequestReplacement {
                    surface,
                    placement: *placement,
                    role: (*role).into(),
                },
                NativeEffectCorrelationKind::ExternalFact,
            ),
            PlatformEffect::ResolveNativeClose {
                edge, resolution, ..
            } => {
                let fence = emission
                    .native_close_emission_fence()
                    .expect("emitted native-close effects carry an exact causal fence");
                debug_assert_eq!(fence.provider(), emission.provider());
                debug_assert_eq!(fence.binding(), binding);
                (
                    NativeEffectOperation::ResolveNativeClose {
                        close: super::NativeSurfaceCloseRequest::from_edge(
                            emission.provider(),
                            *edge,
                        ),
                        resolution: (*resolution).into(),
                        after: fence.after_effect().map(NativeEffectHandle::from_core),
                    },
                    NativeEffectCorrelationKind::Close,
                )
            }
        };
        Self {
            operation: Some(operation),
            receipt: Some(NativeEffectReceipt {
                provider: emission.provider(),
                binding,
                effect: emission.id(),
                epoch: emission.epoch(),
                correlation,
                cleanup: emission.cleanup_observation_token(),
                abandoned: Some(abandoned),
            }),
        }
    }

    /// Consumes the affine request into the operation and its sole dispatch receipt.
    #[must_use]
    pub fn into_parts(mut self) -> (NativeEffectOperation, NativeEffectReceipt) {
        let operation = self
            .operation
            .take()
            .expect("an armed native effect request retains its operation");
        let receipt = self
            .receipt
            .take()
            .expect("an armed native effect request retains its receipt");
        (operation, receipt)
    }
}

impl PartialEq for NativeEffectRequest {
    fn eq(&self, other: &Self) -> bool {
        self.operation == other.operation && self.receipt == other.receipt
    }
}

impl Drop for NativeEffectRequest {
    fn drop(&mut self) {
        drop(self.receipt.take());
    }
}

/// Session-owned sink for affine requests abandoned after core delivery.
///
/// This is intentionally a small private queue rather than a second effect
/// dispatcher. The next joined backend frame records the terminal result in
/// the same ordered ingress stream as ordinary host results.
#[derive(Debug, Clone, Default)]
pub(super) struct NativeEffectDropQueue(Arc<Mutex<Vec<NativeEffectResult>>>);

impl NativeEffectDropQueue {
    fn lock(&self) -> std::sync::MutexGuard<'_, Vec<NativeEffectResult>> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn push(&self, result: NativeEffectResult) {
        self.lock().push(result);
    }

    pub(super) fn take_for(&self, provider: PlatformObservationLease) -> Vec<NativeEffectResult> {
        let pending = std::mem::take(&mut *self.lock());
        pending
            .into_iter()
            .filter(|result| result.provider == provider)
            .collect()
    }
}

/// Sole dispatch receipt for one exact native effect emission.
///
/// Successful dispatch is not proof that platform state changed. Use
/// [`Self::accepted`] and publish the resulting correlation through a later
/// exact platform observation.
#[must_use = "an effect receipt must be accepted or converted into a typed failure"]
#[derive(Debug)]
pub struct NativeEffectReceipt {
    provider: PlatformObservationLease,
    binding: ViewportBinding,
    effect: EffectId,
    epoch: WorkspaceEpoch,
    correlation: NativeEffectCorrelationKind,
    cleanup: Option<CleanupObservationToken>,
    abandoned: Option<NativeEffectDropQueue>,
}

impl NativeEffectReceipt {
    /// Returns the opaque effect identity used to correlate reducer outcomes.
    #[must_use]
    pub const fn handle(&self) -> NativeEffectHandle {
        NativeEffectHandle(self.effect)
    }

    /// Converts a successful dispatch into an opaque later-observation correlation.
    #[must_use]
    pub fn accepted(mut self) -> NativeEffectCorrelation {
        self.abandoned = None;
        NativeEffectCorrelation {
            provider: self.provider,
            binding: self.binding,
            effect: self.effect,
            kind: self.correlation,
            cleanup: self.cleanup,
        }
    }

    /// Reports that the adapter could not dispatch the operation.
    #[must_use]
    pub fn dispatch_failed(self, reason: NativeDispatchFailure) -> NativeEffectResult {
        self.result(EffectDispatchResult::DispatchFailed(reason.into()))
    }

    /// Reports that the backend authoritatively cannot execute the operation.
    #[must_use]
    pub fn unsupported(self, reason: NativeUnsupportedReason) -> NativeEffectResult {
        self.result(EffectDispatchResult::Unsupported(reason.into()))
    }

    /// Reports that the adapter can no longer establish whether dispatch occurred.
    #[must_use]
    pub fn indeterminate(self, reason: NativeIndeterminateReason) -> NativeEffectResult {
        self.result(EffectDispatchResult::Indeterminate(reason.into()))
    }

    fn result(mut self, result: EffectDispatchResult) -> NativeEffectResult {
        self.abandoned = None;
        NativeEffectResult {
            provider: self.provider,
            binding: self.binding,
            result: EffectResult::new(self.effect, self.epoch, result),
        }
    }
}

impl PartialEq for NativeEffectReceipt {
    fn eq(&self, other: &Self) -> bool {
        self.provider == other.provider
            && self.binding == other.binding
            && self.effect == other.effect
            && self.epoch == other.epoch
            && self.correlation == other.correlation
            && self.cleanup == other.cleanup
    }
}

impl Drop for NativeEffectReceipt {
    fn drop(&mut self) {
        let Some(abandoned) = self.abandoned.take() else {
            return;
        };
        abandoned.push(NativeEffectResult {
            provider: self.provider,
            binding: self.binding,
            result: EffectResult::new(
                self.effect,
                self.epoch,
                EffectDispatchResult::DispatchFailed(DispatchFailureReason::ProviderStopped),
            ),
        });
    }
}

/// Opaque proof retained after the host successfully submitted one effect.
///
/// Dispatch success alone never completes the effect. Convert this proof into
/// the matching property acknowledgement, or retain it until an external
/// inventory/focus fact proves the operation.
#[derive(Debug, PartialEq)]
pub struct NativeEffectCorrelation {
    provider: PlatformObservationLease,
    binding: ViewportBinding,
    effect: EffectId,
    kind: NativeEffectCorrelationKind,
    cleanup: Option<CleanupObservationToken>,
}

impl NativeEffectCorrelation {
    /// Converts a pointer-input property effect into an exact acknowledgement.
    pub fn into_input_acknowledgement(self) -> Result<NativeInputEffectAcknowledgement, Self> {
        if self.kind != NativeEffectCorrelationKind::Input {
            return Err(self);
        }
        Ok(NativeInputEffectAcknowledgement {
            provider: self.provider,
            binding: self.binding,
            effect: self.effect,
        })
    }

    /// Converts a window-presentation effect into an exact acknowledgement.
    pub fn into_presentation_acknowledgement(
        self,
    ) -> Result<NativePresentationEffectAcknowledgement, Self> {
        if self.kind != NativeEffectCorrelationKind::Presentation {
            return Err(self);
        }
        Ok(NativePresentationEffectAcknowledgement {
            provider: self.provider,
            binding: self.binding,
            effect: self.effect,
        })
    }

    /// Converts a close-property effect into an exact acknowledgement.
    pub fn into_close_acknowledgement(self) -> Result<NativeCloseEffectAcknowledgement, Self> {
        if self.kind != NativeEffectCorrelationKind::Close {
            return Err(self);
        }
        Ok(NativeCloseEffectAcknowledgement {
            provider: self.provider,
            binding: self.binding,
            effect: self.effect,
        })
    }

    /// Converts an observation-only cleanup continuation into its delayed-result authority.
    pub fn into_cleanup_observation(self) -> Result<NativeCleanupObservation, Self> {
        if self.kind != NativeEffectCorrelationKind::Cleanup {
            return Err(self);
        }
        let Some(token) = self.cleanup else {
            return Err(self);
        };
        Ok(NativeCleanupObservation {
            provider: self.provider,
            provider_binding: self.binding,
            token,
        })
    }
}

/// Exact pointer-input effect acknowledgement for one native observation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct NativeInputEffectAcknowledgement {
    pub(super) provider: PlatformObservationLease,
    pub(super) binding: ViewportBinding,
    pub(super) effect: EffectId,
}

/// Exact presentation effect acknowledgement for one native observation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct NativePresentationEffectAcknowledgement {
    pub(super) provider: PlatformObservationLease,
    pub(super) binding: ViewportBinding,
    pub(super) effect: EffectId,
}

/// Exact close-property effect acknowledgement for one native observation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct NativeCloseEffectAcknowledgement {
    pub(super) provider: PlatformObservationLease,
    pub(super) binding: ViewportBinding,
    pub(super) effect: EffectId,
}

/// Observation-only authority for one destructive predecessor result.
#[derive(Debug, PartialEq)]
pub struct NativeCleanupObservation {
    provider: PlatformObservationLease,
    provider_binding: ViewportBinding,
    token: CleanupObservationToken,
}

impl NativeCleanupObservation {
    /// Correlates a delayed predecessor result through this exact continuation.
    pub fn correlate(
        self,
        predecessor: NativeEffectResult,
    ) -> Result<NativeEffectResult, NativeCleanupCorrelationFailure> {
        // The predecessor may legitimately belong to a retired provider. The
        // continuation provider is the authority which reports the delayed
        // result; the core effect ledger checks that the predecessor identity
        // was originally delivered to its own provider.
        if predecessor.binding != self.token.binding()
            || predecessor.result.effect() != self.token.predecessor()
            || self.provider_binding != self.token.binding()
        {
            return Err(NativeCleanupCorrelationFailure {
                observation: self,
                predecessor,
            });
        }
        Ok(NativeEffectResult {
            provider: self.provider,
            binding: self.provider_binding,
            result: EffectResult::observed_via_cleanup(
                self.token,
                predecessor.result.epoch(),
                predecessor.result.result(),
            ),
        })
    }
}

/// Recoverable mismatch while pairing a cleanup continuation with a predecessor result.
#[derive(Debug, PartialEq, Error)]
#[error("cleanup continuation does not authorize the submitted predecessor result")]
pub struct NativeCleanupCorrelationFailure {
    observation: NativeCleanupObservation,
    predecessor: NativeEffectResult,
}

impl NativeCleanupCorrelationFailure {
    /// Returns both unconsumed capabilities for caller recovery.
    #[must_use]
    pub fn into_parts(self) -> (NativeCleanupObservation, NativeEffectResult) {
        (self.observation, self.predecessor)
    }
}

/// Retryable negative or indeterminate effect result for a later host frame.
#[must_use = "native effect results must be reported or retained for cleanup correlation"]
#[derive(Debug, PartialEq)]
pub struct NativeEffectResult {
    pub(super) provider: PlatformObservationLease,
    pub(super) binding: ViewportBinding,
    pub(super) result: EffectResult,
}

/// Recoverable failure while recording one exact effect result in backend order.
#[derive(Debug, Error)]
#[error("native effect result could not be appended: {error}")]
pub struct NativeEffectSubmissionError {
    error: super::native::NativePlatformError,
    result: NativeEffectResult,
}

impl NativeEffectSubmissionError {
    pub(super) fn new(
        error: super::native::NativePlatformError,
        result: NativeEffectResult,
    ) -> Self {
        Self { error, result }
    }

    /// Returns the structural producer error without consuming the result capability.
    #[must_use]
    pub const fn error(&self) -> &super::native::NativePlatformError {
        &self.error
    }

    /// Returns both the error and the unconsumed result for retry or cleanup correlation.
    #[must_use]
    pub fn into_parts(self) -> (super::native::NativePlatformError, NativeEffectResult) {
        (self.error, self.result)
    }
}

impl NativeEffectResult {
    /// Returns the opaque effect identity used to correlate reducer outcomes.
    #[must_use]
    pub const fn handle(&self) -> NativeEffectHandle {
        NativeEffectHandle(self.result.effect())
    }
}

/// Adapter-level reason an effect was not dispatched.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NativeDispatchFailure {
    /// The adapter rejected the request.
    AdapterRejected,
    /// The exact target window was unavailable.
    WindowUnavailable,
    /// The provider stopped before dispatch.
    ProviderStopped,
}

impl From<NativeDispatchFailure> for DispatchFailureReason {
    fn from(reason: NativeDispatchFailure) -> Self {
        match reason {
            NativeDispatchFailure::AdapterRejected => Self::AdapterRejected,
            NativeDispatchFailure::WindowUnavailable => Self::WindowUnavailable,
            NativeDispatchFailure::ProviderStopped => Self::ProviderStopped,
        }
    }
}

/// Authoritative reason the backend cannot execute an effect.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NativeUnsupportedReason {
    /// The backend does not implement this operation.
    BackendUnsupported,
    /// Capability was revoked after the effect was emitted.
    CapabilityRevoked,
}

impl From<NativeUnsupportedReason> for EffectUnsupportedReason {
    fn from(reason: NativeUnsupportedReason) -> Self {
        match reason {
            NativeUnsupportedReason::BackendUnsupported => Self::BackendUnsupported,
            NativeUnsupportedReason::CapabilityRevoked => Self::CapabilityRevoked,
        }
    }
}

/// Reason a dispatch outcome can no longer be established.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NativeIndeterminateReason {
    /// The provider lost the platform acknowledgement.
    AcknowledgementLost,
    /// The provider restarted after the effect was submitted.
    ProviderRestarted,
}

impl From<NativeIndeterminateReason> for EffectIndeterminateReason {
    fn from(reason: NativeIndeterminateReason) -> Self {
        match reason {
            NativeIndeterminateReason::AcknowledgementLost => Self::AcknowledgementLost,
            NativeIndeterminateReason::ProviderRestarted => Self::ProviderRestarted,
        }
    }
}

/// Stable facade classification of one reported effect result.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NativeEffectReportOutcome {
    /// The result changed the live effect ledger.
    Applied,
    /// The same exact result was already accepted.
    Duplicate,
    /// The observation did not occur after the request delivery fence.
    CausalityBarrier,
    /// The result belongs to an older workspace epoch.
    StaleEpoch,
    /// The effect identity is unknown.
    UnknownEffect,
    /// The effect reached terminal state and its detailed record was compacted.
    RetiredTerminal,
    /// The result names a different native binding.
    BindingMismatch,
    /// The reporting provider never received the effect.
    ProviderMismatch,
}

impl From<EffectTransition> for NativeEffectReportOutcome {
    fn from(transition: EffectTransition) -> Self {
        match transition {
            EffectTransition::Applied => Self::Applied,
            EffectTransition::Duplicate => Self::Duplicate,
            EffectTransition::CausalityBarrier => Self::CausalityBarrier,
            EffectTransition::StaleEpoch => Self::StaleEpoch,
            EffectTransition::UnknownEffect => Self::UnknownEffect,
            EffectTransition::RetiredTerminal => Self::RetiredTerminal,
            EffectTransition::BindingMismatch => Self::BindingMismatch,
            EffectTransition::ProviderMismatch => Self::ProviderMismatch,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::effect::{EffectLedger, PlatformEffect};
    use crate::ids::{EngineAuthorityDomainId, SurfaceId, WorkspaceEpoch};
    use crate::platform_provider::PlatformObservationAuthority;
    use crate::viewport::{InventoryGeneration, WindowIncarnation, WindowToken};

    #[test]
    fn property_effect_operation_retains_its_causal_predecessor() {
        let domain = EngineAuthorityDomainId::new_for_test(91);
        let mut providers = PlatformObservationAuthority::new(domain);
        let provider = providers.create().expect("the test provider mints");
        let binding = ViewportBinding::new(
            domain,
            WorkspaceEpoch::new(0),
            SurfaceId::new(1),
            WindowToken::new(1),
            WindowIncarnation::new(1),
        );
        let mut ledger = EffectLedger::default();
        let first = ledger
            .request(PlatformEffect::SetPointerPassthrough {
                binding,
                enabled: true,
                after: None,
            })
            .expect("the first property effect allocates");
        let first_emission = ledger
            .take_new_requests(provider, InventoryGeneration::new(1), |_| false)
            .expect("the first property effect emits")
            .pop()
            .expect("one first emission exists");
        let second = ledger
            .request(PlatformEffect::SetPointerPassthrough {
                binding,
                enabled: false,
                after: Some(first),
            })
            .expect("the successor property effect allocates");
        let second_emission = ledger
            .take_new_requests(provider, InventoryGeneration::new(2), |_| false)
            .expect("the successor property effect emits")
            .pop()
            .expect("one successor emission exists");

        let (first_operation, first_receipt) =
            NativeEffectRequest::from_emission(&first_emission, NativeEffectDropQueue::default())
                .into_parts();
        assert!(matches!(
            first_operation,
            NativeEffectOperation::SetPointerPassthrough {
                enabled: true,
                after: None,
                ..
            }
        ));
        assert_eq!(first_receipt.handle(), NativeEffectHandle::from_core(first));

        let (second_operation, second_receipt) =
            NativeEffectRequest::from_emission(&second_emission, NativeEffectDropQueue::default())
                .into_parts();
        assert!(matches!(
            second_operation,
            NativeEffectOperation::SetPointerPassthrough {
                enabled: false,
                after: Some(predecessor),
                ..
            } if predecessor == NativeEffectHandle::from_core(first)
        ));
        assert_eq!(
            second_receipt.handle(),
            NativeEffectHandle::from_core(second)
        );
    }

    #[test]
    fn dropped_request_records_a_terminal_dispatch_failure() {
        let domain = EngineAuthorityDomainId::new_for_test(92);
        let mut providers = PlatformObservationAuthority::new(domain);
        let provider = providers.create().expect("the test provider mints");
        let binding = ViewportBinding::new(
            domain,
            WorkspaceEpoch::new(0),
            SurfaceId::new(1),
            WindowToken::new(1),
            WindowIncarnation::new(1),
        );
        let mut ledger = EffectLedger::default();
        let effect = ledger
            .request(PlatformEffect::SetPointerPassthrough {
                binding,
                enabled: true,
                after: None,
            })
            .expect("the effect allocates");
        let emission = ledger
            .take_new_requests(provider, InventoryGeneration::new(1), |_| false)
            .expect("the effect emits")
            .pop()
            .expect("one emission exists");
        let abandoned = NativeEffectDropQueue::default();

        drop(NativeEffectRequest::from_emission(
            &emission,
            abandoned.clone(),
        ));

        let results = abandoned.take_for(provider);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].handle(), NativeEffectHandle::from_core(effect));
        assert_eq!(results[0].binding, binding);
        assert!(abandoned.take_for(provider).is_empty());
    }

    #[test]
    fn dropped_dispatch_receipt_records_the_same_terminal_failure() {
        let domain = EngineAuthorityDomainId::new_for_test(93);
        let mut providers = PlatformObservationAuthority::new(domain);
        let provider = providers.create().expect("the test provider mints");
        let binding = ViewportBinding::new(
            domain,
            WorkspaceEpoch::new(0),
            SurfaceId::new(1),
            WindowToken::new(1),
            WindowIncarnation::new(1),
        );
        let mut ledger = EffectLedger::default();
        let effect = ledger
            .request(PlatformEffect::RequestFocus {
                binding,
                after: None,
            })
            .expect("the focus effect allocates");
        let emission = ledger
            .take_new_requests(provider, InventoryGeneration::new(1), |_| false)
            .expect("the focus effect emits")
            .pop()
            .expect("one emission exists");
        let abandoned = NativeEffectDropQueue::default();
        let (_operation, receipt) =
            NativeEffectRequest::from_emission(&emission, abandoned.clone()).into_parts();

        drop(receipt);

        let results = abandoned.take_for(provider);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].handle(), NativeEffectHandle::from_core(effect));
    }
}
