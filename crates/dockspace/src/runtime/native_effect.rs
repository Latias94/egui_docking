//! Affine native-effect requests exposed by the renderer-neutral runtime.

use std::sync::{Arc, Mutex, PoisonError};

use thiserror::Error;

use super::native::{NativeHostErrorKind, NativeSurfaceBinding};
use crate::effect::{
    CleanupObservationToken, DispatchFailureReason, EffectDispatchResult, EffectId,
    EffectIndeterminateReason, EffectResult, EffectUnsupportedReason, NativeCloseResolution,
    PlatformEffect, PlatformEffectEmission,
};
use crate::geometry::PhysicalRect;
use crate::ids::WorkspaceEpoch;
use crate::platform_provider::PlatformObservationLease;
use crate::viewport::{ViewportBinding, ViewportRole};

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
        binding: NativeSurfaceBinding,
        /// Exact requested outer placement.
        placement: PhysicalRect,
        /// Requested ownership role.
        role: NativeSurfaceRole,
    },
    /// Show a previously staged hidden native window.
    ShowWindow {
        /// Exact logical/native binding to show.
        binding: NativeSurfaceBinding,
    },
    /// Close a staging window created by an operation which did not complete.
    CompensatingClose {
        /// Exact logical/native binding to close.
        binding: NativeSurfaceBinding,
    },
    /// Cancel an application-root native close request.
    CancelRootClose {
        /// Exact logical/native binding whose close must be cancelled.
        binding: NativeSurfaceBinding,
    },
    /// Retain ownership of a child window.
    RetainChild {
        /// Exact logical/native child binding.
        binding: NativeSurfaceBinding,
    },
    /// Release and destroy a child window.
    ReleaseChild {
        /// Exact logical/native child binding.
        binding: NativeSurfaceBinding,
    },
    /// Await one delayed destructive result without executing it again.
    AwaitCleanup {
        /// Exact logical/native binding under observation.
        binding: NativeSurfaceBinding,
    },
    /// Request closure of an application-root window.
    RequestRootClose {
        /// Exact logical/native root binding.
        binding: NativeSurfaceBinding,
    },
    /// Change pointer pass-through for one exact native binding.
    SetPointerPassthrough {
        /// Exact logical/native binding.
        binding: NativeSurfaceBinding,
        /// Whether the native window must ignore pointer input.
        enabled: bool,
    },
    /// Request native focus for one exact binding.
    RequestFocus {
        /// Exact logical/native binding.
        binding: NativeSurfaceBinding,
    },
    /// Create a replacement native lifetime at an exact outer placement.
    RequestReplacement {
        /// Exact replacement binding.
        binding: NativeSurfaceBinding,
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
    },
}

impl NativeEffectOperation {
    /// Returns the exact native binding affected by this operation.
    #[must_use]
    pub const fn binding(&self) -> NativeSurfaceBinding {
        match self {
            Self::CreateWindow { binding, .. }
            | Self::ShowWindow { binding, .. }
            | Self::CompensatingClose { binding, .. }
            | Self::CancelRootClose { binding }
            | Self::RetainChild { binding }
            | Self::ReleaseChild { binding }
            | Self::AwaitCleanup { binding }
            | Self::RequestRootClose { binding }
            | Self::SetPointerPassthrough { binding, .. }
            | Self::RequestFocus { binding, .. }
            | Self::RequestReplacement { binding, .. } => *binding,
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
/// The type deliberately does not implement `Clone` or `Copy`. The request is
/// itself the sole response capability: inspect [`Self::operation`], execute it,
/// then consume the request through one terminal method.
#[must_use = "native effects must be dispatched, rejected, or retained explicitly"]
#[derive(Debug)]
pub struct NativeEffectRequest {
    operation: NativeEffectOperation,
    provider: PlatformObservationLease,
    binding: ViewportBinding,
    effect: EffectId,
    epoch: WorkspaceEpoch,
    correlation: NativeEffectCorrelationKind,
    cleanup: Option<CleanupObservationToken>,
    abandoned: Option<NativeEffectDropQueue>,
}

impl NativeEffectRequest {
    pub(super) fn from_emission(
        emission: &PlatformEffectEmission,
        abandoned: NativeEffectDropQueue,
    ) -> Self {
        let binding = emission.effect().binding();
        let native_binding = NativeSurfaceBinding::from_binding(emission.provider(), binding);
        let (operation, correlation) = match emission.effect() {
            PlatformEffect::CreateWindow {
                placement, role, ..
            } => (
                NativeEffectOperation::CreateWindow {
                    binding: native_binding,
                    placement: *placement,
                    role: (*role).into(),
                },
                // A created window must acknowledge the exact hidden
                // incarnation before the core may advance its bring-up saga.
                NativeEffectCorrelationKind::Presentation,
            ),
            PlatformEffect::ShowWindow { .. } => (
                NativeEffectOperation::ShowWindow {
                    binding: native_binding,
                },
                NativeEffectCorrelationKind::Presentation,
            ),
            PlatformEffect::CompensatingClose { .. } => (
                NativeEffectOperation::CompensatingClose {
                    binding: native_binding,
                },
                NativeEffectCorrelationKind::Close,
            ),
            PlatformEffect::CancelRootClose { .. } => (
                NativeEffectOperation::CancelRootClose {
                    binding: native_binding,
                },
                NativeEffectCorrelationKind::Close,
            ),
            PlatformEffect::RetainChild { .. } => (
                NativeEffectOperation::RetainChild {
                    binding: native_binding,
                },
                NativeEffectCorrelationKind::ExternalFact,
            ),
            PlatformEffect::ReleaseChild { .. } => (
                NativeEffectOperation::ReleaseChild {
                    binding: native_binding,
                },
                NativeEffectCorrelationKind::Close,
            ),
            PlatformEffect::ContinueCleanup { .. } => (
                NativeEffectOperation::AwaitCleanup {
                    binding: native_binding,
                },
                NativeEffectCorrelationKind::Cleanup,
            ),
            PlatformEffect::RequestRootClose { .. } => (
                NativeEffectOperation::RequestRootClose {
                    binding: native_binding,
                },
                NativeEffectCorrelationKind::Close,
            ),
            PlatformEffect::SetPointerPassthrough { enabled, .. } => (
                NativeEffectOperation::SetPointerPassthrough {
                    binding: native_binding,
                    enabled: *enabled,
                },
                NativeEffectCorrelationKind::Input,
            ),
            PlatformEffect::RequestFocus { .. } => (
                NativeEffectOperation::RequestFocus {
                    binding: native_binding,
                },
                NativeEffectCorrelationKind::ExternalFact,
            ),
            PlatformEffect::RequestReplacement {
                placement, role, ..
            } => (
                NativeEffectOperation::RequestReplacement {
                    binding: native_binding,
                    placement: *placement,
                    role: (*role).into(),
                },
                // Replacement has the same first-hidden presentation barrier
                // as an initial create.
                NativeEffectCorrelationKind::Presentation,
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
                    },
                    NativeEffectCorrelationKind::Close,
                )
            }
        };
        Self {
            operation,
            provider: emission.provider(),
            binding,
            effect: emission.id(),
            epoch: emission.epoch(),
            correlation,
            cleanup: emission.cleanup_observation_token(),
            abandoned: Some(abandoned),
        }
    }

    /// Returns the exact operation without exposing core effect-ledger identity.
    #[must_use]
    pub const fn operation(&self) -> &NativeEffectOperation {
        &self.operation
    }

    /// Records successful dispatch and returns any exact later-observation capability.
    ///
    /// `None` means the operation is completed by ordinary inventory, focus, or
    /// lifecycle facts rather than a property acknowledgement.
    #[must_use]
    pub fn accepted(mut self) -> Option<NativeEffectAcknowledgement> {
        self.abandoned = None;
        match self.correlation {
            NativeEffectCorrelationKind::Input => Some(NativeEffectAcknowledgement::Input(
                NativeInputEffectAcknowledgement {
                    provider: self.provider,
                    binding: self.binding,
                    effect: self.effect,
                },
            )),
            NativeEffectCorrelationKind::Presentation => {
                Some(NativeEffectAcknowledgement::Presentation(
                    NativePresentationEffectAcknowledgement {
                        provider: self.provider,
                        binding: self.binding,
                        effect: self.effect,
                    },
                ))
            }
            NativeEffectCorrelationKind::Close => Some(NativeEffectAcknowledgement::Close(
                NativeCloseEffectAcknowledgement {
                    provider: self.provider,
                    binding: self.binding,
                    effect: self.effect,
                },
            )),
            NativeEffectCorrelationKind::Cleanup => Some(NativeEffectAcknowledgement::Cleanup(
                NativeCleanupObservation {
                    provider: self.provider,
                    provider_binding: self.binding,
                    token: self
                        .cleanup
                        .expect("cleanup effects carry an exact observation token"),
                },
            )),
            NativeEffectCorrelationKind::ExternalFact => None,
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

impl PartialEq for NativeEffectRequest {
    fn eq(&self, other: &Self) -> bool {
        self.operation == other.operation
            && self.provider == other.provider
            && self.binding == other.binding
            && self.effect == other.effect
            && self.epoch == other.epoch
            && self.correlation == other.correlation
            && self.cleanup == other.cleanup
    }
}

impl Drop for NativeEffectRequest {
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

/// Exact follow-up capability produced after successful effect dispatch.
///
/// Internal effect identities and causal predecessor chains remain private.
/// The host receives only the later fact it can authoritatively report.
#[non_exhaustive]
#[derive(Debug, PartialEq)]
pub enum NativeEffectAcknowledgement {
    /// Correlate a later pointer-input property observation.
    Input(NativeInputEffectAcknowledgement),
    /// Correlate a later window-presentation property observation.
    Presentation(NativePresentationEffectAcknowledgement),
    /// Correlate a later close or destruction observation.
    Close(NativeCloseEffectAcknowledgement),
    /// Correlate one delayed destructive predecessor result.
    Cleanup(NativeCleanupObservation),
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

    /// Returns the stable product-level failure category without consuming the
    /// result capability.
    #[must_use]
    pub const fn kind(&self) -> NativeHostErrorKind {
        self.error.kind()
    }

    /// Returns the failure category and the unconsumed result for retry or
    /// cleanup correlation.
    #[must_use]
    pub fn into_parts(self) -> (NativeHostErrorKind, NativeEffectResult) {
        (self.error.kind(), self.result)
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::effect::{EffectLedger, PlatformEffect};
    use crate::ids::{EngineAuthorityDomainId, SurfaceId, WorkspaceEpoch};
    use crate::platform_provider::PlatformObservationAuthority;
    use crate::viewport::{InventoryGeneration, WindowIncarnation, WindowToken};

    #[test]
    fn property_effect_request_hides_ledger_links_and_returns_typed_acknowledgement() {
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

        let first_request =
            NativeEffectRequest::from_emission(&first_emission, NativeEffectDropQueue::default());
        assert!(matches!(
            first_request.operation(),
            NativeEffectOperation::SetPointerPassthrough { enabled: true, .. }
        ));
        let Some(NativeEffectAcknowledgement::Input(first_acknowledgement)) =
            first_request.accepted()
        else {
            panic!("pointer pass-through returns one input acknowledgement");
        };
        assert_eq!(first_acknowledgement.provider, provider);
        assert_eq!(first_acknowledgement.binding, binding);
        assert_eq!(first_acknowledgement.effect, first);

        let second_request =
            NativeEffectRequest::from_emission(&second_emission, NativeEffectDropQueue::default());
        assert!(matches!(
            second_request.operation(),
            NativeEffectOperation::SetPointerPassthrough { enabled: false, .. }
        ));
        let Some(NativeEffectAcknowledgement::Input(second_acknowledgement)) =
            second_request.accepted()
        else {
            panic!("the successor returns one input acknowledgement");
        };
        assert_eq!(second_acknowledgement.effect, second);
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
        assert_eq!(results[0].result.effect(), effect);
        assert_eq!(results[0].binding, binding);
        assert!(abandoned.take_for(provider).is_empty());
    }

    #[test]
    fn create_and_replacement_return_exact_presentation_acknowledgements() {
        let domain = EngineAuthorityDomainId::new_for_test(94);
        let mut providers = PlatformObservationAuthority::new(domain);
        let provider = providers.create().expect("the test provider mints");
        let binding = ViewportBinding::new(
            domain,
            WorkspaceEpoch::new(0),
            SurfaceId::new(1),
            WindowToken::new(1),
            WindowIncarnation::new(1),
        );
        let placement =
            PhysicalRect::new(1.0, 2.0, 320.0, 240.0).expect("native placement validates");

        for effect in [
            PlatformEffect::CreateWindow {
                binding,
                placement,
                role: ViewportRole::Child,
            },
            PlatformEffect::RequestReplacement {
                binding,
                placement,
                role: ViewportRole::Child,
            },
        ] {
            let mut ledger = EffectLedger::default();
            let id = ledger.request(effect).expect("the effect allocates");
            let emission = ledger
                .take_new_requests(provider, InventoryGeneration::new(1), |_| false)
                .expect("the effect emits")
                .pop()
                .expect("one emission exists");
            let request =
                NativeEffectRequest::from_emission(&emission, NativeEffectDropQueue::default());
            let Some(NativeEffectAcknowledgement::Presentation(acknowledgement)) =
                request.accepted()
            else {
                panic!("native create and replacement return a presentation acknowledgement");
            };
            assert_eq!(acknowledgement.provider, provider);
            assert_eq!(acknowledgement.binding, binding);
            assert_eq!(acknowledgement.effect, id);
        }
    }

    #[test]
    fn accepting_an_external_fact_effect_disarms_the_drop_failure() {
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
        ledger
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
        let request = NativeEffectRequest::from_emission(&emission, abandoned.clone());
        assert!(matches!(
            request.operation(),
            NativeEffectOperation::RequestFocus { .. }
        ));

        assert_eq!(request.accepted(), None);

        assert!(abandoned.take_for(provider).is_empty());
    }
}
