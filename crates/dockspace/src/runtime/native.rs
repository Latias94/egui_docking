//! Opaque native-window lifecycle facts and bindings for renderer-neutral hosts.

mod compiler;
mod pointer;
mod receiver;
mod session;
#[cfg(test)]
mod tests;

use std::collections::{BTreeMap, BTreeSet};

use thiserror::Error;

use super::native_effect::{
    NativeCloseEffectAcknowledgement, NativeEffectResult, NativeEffectSubmissionError,
    NativeInputEffectAcknowledgement, NativePresentationEffectAcknowledgement,
};
use super::{DockspaceRuntimeError, DockspaceSession};
use crate::backend_ingress::{
    BackendIngressBatch, BackendIngressPrefixRetirementReceipt, BackendIngressRecorder,
};
use crate::engine::EngineInput;
use crate::geometry::{PhysicalRect, ScaleFactor};
use crate::ids::{SurfaceId, WorkspaceEpoch};
use crate::platform::{
    CloseEffectAcknowledgement, ObservedWindow, WindowCloseObservation, WindowCloseState,
    WindowInputState, WindowPresentationState,
};
use crate::platform_provider::PlatformObservationLease;
use crate::viewport::{
    CloseObservationGeneration, ViewportBinding, ViewportRole, WindowToken, WorkAreaGeneration,
    WorkAreaToken,
};
use compiler::{compile_platform_snapshot, compile_unknown_inventory_snapshot};
pub use pointer::{
    NativeDesktopPointerLocation, NativeDesktopPosition, NativePointerButton, NativePointerEvent,
    NativePointerHover, NativePointerId, NativePointerInput, NativePointerOwner,
    NativePointerRoster, NativePointerState, NativeProjectedScrollDelta, NativeReceiverAnswer,
    NativeReceiverPurpose, NativeReceiverQuery, NativeScrollCancelReason, NativeScrollDelta,
    NativeScrollDeviceId, NativeScrollEvent, NativeScrollModifiers, NativeScrollMomentum,
    NativeScrollPhase, NativeScrollReceiverChallenge, NativeScrollSequenceId,
};
pub(in crate::runtime) use receiver::resolve_receiver_observation;

/// Adapter-owned opaque native-window token.
///
/// A token may be reused after exact destruction. It is not sufficient input
/// authority by itself; every observation also carries a core-minted
/// [`NativeSurfaceBinding`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(transparent)]
pub struct HostWindowToken(u64);

impl HostWindowToken {
    /// Creates a stable adapter token without exposing an operating-system handle.
    #[must_use]
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    const fn into_core(self) -> WindowToken {
        WindowToken::new(self.0)
    }
}

/// Opaque exact-incarnation binding for one native docking surface.
///
/// The value is copyable so asynchronous callbacks may retain it. No accessor
/// exposes the core incarnation, workspace epoch, or authority domain.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct NativeSurfaceBinding {
    provider: PlatformObservationLease,
    binding: ViewportBinding,
}

/// Adapter-owned identity for one platform work area.
///
/// The token is not a geometry proof by itself. Hosts must obtain the binding
/// from the current committed native snapshot before using it in an outside-all
/// pointer route.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(transparent)]
pub struct HostWorkAreaToken(u64);

impl HostWorkAreaToken {
    /// Creates a stable adapter token without exposing a platform handle.
    #[must_use]
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    const fn into_core(self) -> WorkAreaToken {
        WorkAreaToken::new(self.0)
    }
}

/// Exact work-area facts supplied by a managed native host.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NativeWorkAreaFacts {
    token: HostWorkAreaToken,
    bounds: PhysicalRect,
    scale_factor: ScaleFactor,
}

/// Complete work-area authority supplied with one managed native snapshot.
///
/// Absence is not a fact. A host must explicitly report either the complete
/// exact roster or that the roster is currently unavailable.
#[derive(Debug, Clone, PartialEq)]
pub enum NativeWorkAreaRoster {
    /// Complete current platform work-area roster.
    Exact(Vec<NativeWorkAreaFacts>),
    /// Work-area authority is unavailable at this causal boundary.
    Unknown,
}

impl NativeWorkAreaFacts {
    /// Creates one exact physical work-area observation.
    #[must_use]
    pub const fn new(
        token: HostWorkAreaToken,
        bounds: PhysicalRect,
        scale_factor: ScaleFactor,
    ) -> Self {
        Self {
            token,
            bounds,
            scale_factor,
        }
    }

    pub(super) const fn token(self) -> WorkAreaToken {
        self.token.into_core()
    }

    pub(super) const fn bounds(self) -> PhysicalRect {
        self.bounds
    }

    pub(super) const fn scale_factor(self) -> ScaleFactor {
        self.scale_factor
    }
}

/// Exact current work-area binding used by an outside-all pointer edge.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct NativeWorkAreaBinding {
    provider: PlatformObservationLease,
    generation: WorkAreaGeneration,
    token: WorkAreaToken,
}

impl NativeWorkAreaBinding {
    pub(super) const fn new(
        provider: PlatformObservationLease,
        generation: WorkAreaGeneration,
        token: WorkAreaToken,
    ) -> Self {
        Self {
            provider,
            generation,
            token,
        }
    }

    /// Returns the stable adapter-owned work-area token.
    #[must_use]
    pub const fn token(self) -> HostWorkAreaToken {
        HostWorkAreaToken(self.token.get())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum NativeHostProfile {
    ObservedRoots,
    ManagedDesktop,
}

impl NativeSurfaceBinding {
    pub(super) const fn from_binding(
        provider: PlatformObservationLease,
        binding: ViewportBinding,
    ) -> Self {
        Self { provider, binding }
    }

    /// Returns the stable logical surface.
    #[must_use]
    pub const fn surface(self) -> SurfaceId {
        self.binding.surface()
    }

    /// Returns the reusable adapter-owned window token.
    #[must_use]
    pub const fn window_token(self) -> HostWindowToken {
        HostWindowToken(self.binding.token().get())
    }

    pub(super) fn matches_viewport_binding(self, binding: ViewportBinding) -> bool {
        self.binding == binding
    }
}

/// Opaque exact native close edge returned by a committed platform observation.
///
/// The copyable value is bound to the provider and binding incarnation which
/// observed it. Replays remain fail-closed in the core close protocol.
#[must_use = "native close requests must be resolved, cancelled, or retained explicitly"]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct NativeSurfaceCloseRequest {
    provider: PlatformObservationLease,
    edge: crate::NativeCloseEdge,
}

impl NativeSurfaceCloseRequest {
    pub(super) const fn from_edge(
        provider: PlatformObservationLease,
        edge: crate::NativeCloseEdge,
    ) -> Self {
        Self { provider, edge }
    }

    /// Returns the stable logical surface which requested closure.
    #[must_use]
    pub const fn surface(&self) -> SurfaceId {
        self.edge.binding().surface()
    }

    /// Returns the exact provider-bound native surface binding.
    #[must_use]
    pub const fn binding(&self) -> NativeSurfaceBinding {
        NativeSurfaceBinding::from_binding(self.provider, self.edge.binding())
    }
}

/// Native close fact accepted by the narrow live-binding lane.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NativeCloseState {
    /// The exact binding has no pending native close request.
    Clear,
    /// The exact binding has a pending native close request.
    Requested,
}

/// Exact pointer-input state reported for one native window.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NativeWindowInputState {
    /// The native window receives pointer input.
    ReceivesInput,
    /// Pointer input passes through the native window.
    PassThrough,
}

impl From<NativeWindowInputState> for WindowInputState {
    fn from(state: NativeWindowInputState) -> Self {
        match state {
            NativeWindowInputState::ReceivesInput => Self::ReceivesInput,
            NativeWindowInputState::PassThrough => Self::PassThrough,
        }
    }
}

/// Exact presentation state reported for one native window.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NativeWindowPresentationState {
    /// The native window is visible.
    Visible,
    /// The native window is hidden.
    Hidden,
    /// The native window is minimized.
    Minimized,
}

impl From<NativeWindowPresentationState> for WindowPresentationState {
    fn from(state: NativeWindowPresentationState) -> Self {
        match state {
            NativeWindowPresentationState::Visible => Self::Visible,
            NativeWindowPresentationState::Hidden => Self::Hidden,
            NativeWindowPresentationState::Minimized => Self::Minimized,
        }
    }
}

/// Complete facts supplied for one binding in a native platform snapshot.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NativeWindowFacts {
    lifecycle: NativeWindowLifecycleFact,
    content_bounds: Option<PhysicalRect>,
    outer_bounds: Option<PhysicalRect>,
    native_scale_factor: Option<ScaleFactor>,
    presentation_scale_factor: Option<ScaleFactor>,
    input: Option<NativeInputFact>,
    presentation: Option<NativePresentationFact>,
    close: Option<NativeCloseFact>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NativeWindowLifecycleFact {
    Live,
    Destroyed {
        acknowledgement: Option<NativeCloseEffectAcknowledgement>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct NativeInputFact {
    state: NativeWindowInputState,
    acknowledgement: Option<NativeInputEffectAcknowledgement>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct NativePresentationFact {
    state: NativeWindowPresentationState,
    acknowledgement: Option<NativePresentationEffectAcknowledgement>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct NativeCloseFact {
    state: NativeCloseState,
    acknowledgement: Option<NativeCloseEffectAcknowledgement>,
}

impl NativeWindowFacts {
    /// Reports exact inventory presence while leaving every independent property Unknown.
    #[must_use]
    pub const fn live() -> Self {
        Self {
            lifecycle: NativeWindowLifecycleFact::Live,
            content_bounds: None,
            outer_bounds: None,
            native_scale_factor: None,
            presentation_scale_factor: None,
            input: None,
            presentation: None,
            close: None,
        }
    }

    /// Adds exact physical content bounds without inferring outer bounds.
    #[must_use]
    pub const fn with_content_bounds(mut self, bounds: PhysicalRect) -> Self {
        self.content_bounds = Some(bounds);
        self
    }

    /// Adds exact physical outer bounds without inferring content bounds.
    #[must_use]
    pub const fn with_outer_bounds(mut self, bounds: PhysicalRect) -> Self {
        self.outer_bounds = Some(bounds);
        self
    }

    /// Adds the exact operating-system scale factor.
    #[must_use]
    pub const fn with_native_scale_factor(mut self, scale_factor: ScaleFactor) -> Self {
        self.native_scale_factor = Some(scale_factor);
        self
    }

    /// Adds the exact renderer presentation scale factor.
    #[must_use]
    pub const fn with_presentation_scale_factor(mut self, scale_factor: ScaleFactor) -> Self {
        self.presentation_scale_factor = Some(scale_factor);
        self
    }

    /// Adds one exact pointer-input property observation.
    #[must_use]
    pub const fn with_input(
        mut self,
        state: NativeWindowInputState,
        acknowledgement: Option<NativeInputEffectAcknowledgement>,
    ) -> Self {
        self.input = Some(NativeInputFact {
            state,
            acknowledgement,
        });
        self
    }

    /// Adds one exact presentation property observation.
    #[must_use]
    pub const fn with_presentation(
        mut self,
        state: NativeWindowPresentationState,
        acknowledgement: Option<NativePresentationEffectAcknowledgement>,
    ) -> Self {
        self.presentation = Some(NativePresentationFact {
            state,
            acknowledgement,
        });
        self
    }

    /// Adds one exact native-close property observation.
    #[must_use]
    pub const fn with_close(
        mut self,
        state: NativeCloseState,
        acknowledgement: Option<NativeCloseEffectAcknowledgement>,
    ) -> Self {
        self.close = Some(NativeCloseFact {
            state,
            acknowledgement,
        });
        self
    }

    /// Reports exact destruction and removes the binding from the live inventory.
    ///
    /// This form explicitly states that destruction was not correlated with a
    /// core-emitted close effect.
    #[must_use]
    pub const fn destroyed() -> Self {
        Self {
            lifecycle: NativeWindowLifecycleFact::Destroyed {
                acknowledgement: None,
            },
            content_bounds: None,
            outer_bounds: None,
            native_scale_factor: None,
            presentation_scale_factor: None,
            input: None,
            presentation: None,
            close: None,
        }
    }

    /// Reports exact destruction caused by one accepted core close effect.
    #[must_use]
    pub const fn destroyed_after(acknowledgement: NativeCloseEffectAcknowledgement) -> Self {
        Self {
            lifecycle: NativeWindowLifecycleFact::Destroyed {
                acknowledgement: Some(acknowledgement),
            },
            content_bounds: None,
            outer_bounds: None,
            native_scale_factor: None,
            presentation_scale_factor: None,
            input: None,
            presentation: None,
            close: None,
        }
    }
}

/// Stable product-level category for a native host failure.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NativeHostErrorKind {
    /// No native host is enrolled for the session.
    NotEnabled,
    /// A native host is already enrolled.
    AlreadyEnabled,
    /// A callback or acknowledgement names an older binding incarnation.
    StaleBinding,
    /// The host supplied facts that are malformed or incomplete.
    InvalidFacts,
    /// The requested operation conflicts with the current host lifecycle.
    OperationConflict,
    /// The enrolled native host cannot perform the requested operation.
    Unsupported,
    /// The host crossed an internal protocol boundary unexpectedly.
    Internal,
}

/// Exact native lifecycle failure retained inside the runtime implementation.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub(super) enum NativePlatformError {
    /// No native provider was enrolled for this session.
    #[error("no facade-owned native platform provider is active")]
    ProviderUnavailable,
    /// The submitted snapshot belongs to a superseded provider.
    #[error("native snapshot belongs to a superseded platform provider")]
    ProviderSuperseded,
    /// A native provider is already enrolled for this session.
    #[error("native platform provider is already active")]
    ProviderAlreadyEnabled,
    /// A surface-local pointer provider prevents desktop-global enrollment.
    #[error("surface-local pointer provider is already active")]
    PointerProviderAlreadyEnabled,
    /// A callback named an older binding incarnation for this logical surface.
    #[error("native surface {surface} binding is stale")]
    StaleSurface {
        /// Stable logical surface named by the stale capability.
        surface: SurfaceId,
    },
    /// A complete snapshot repeated one binding capability.
    #[error("native snapshot repeats surface {surface}")]
    DuplicateSurface {
        /// Duplicated logical surface.
        surface: SurfaceId,
    },
    /// A complete snapshot did not exactly cover the current binding roster.
    #[error("native snapshot roster is incomplete or contains a foreign surface")]
    IncompleteRoster,
    /// A preceding roster mutation has not reached a committed host boundary.
    #[error("native binding roster is not yet settled")]
    BindingRosterUnsettled,
    /// A binding is still live and therefore cannot be declared permanently quiescent.
    #[error("native surface {surface} is still live and cannot be quiesced")]
    BindingStillLive {
        /// Stable logical surface which still owns the binding.
        surface: SurfaceId,
    },
    /// The binding was not retired by this active provider.
    #[error("native surface {surface} has no retired binding awaiting quiescence")]
    BindingNotRetired {
        /// Stable logical surface named by the invalid quiescence capability.
        surface: SurfaceId,
    },
    /// Provider-owned observation generations cannot advance without wrapping.
    #[error("native observation generation is exhausted")]
    GenerationExhausted,
    /// Destroyed inventory facts also carried live-window properties.
    #[error("destroyed native surface {surface} also reported live-window properties")]
    DestroyedSurfaceHasLiveFacts {
        /// Stable logical surface carrying contradictory facts.
        surface: SurfaceId,
    },
    /// A retired binding was reported as a live window again.
    #[error("retired native surface {surface} reported live-window facts")]
    RetiredSurfaceHasLiveFacts {
        /// Stable logical surface carrying contradictory facts.
        surface: SurfaceId,
    },
    /// An effect acknowledgement belongs to another binding incarnation.
    #[error("native effect acknowledgement for surface {surface} belongs to another binding")]
    EffectAcknowledgementBindingMismatch {
        /// Stable logical surface whose fact contained the stale acknowledgement.
        surface: SurfaceId,
    },
    /// An effect acknowledgement belongs to another provider incarnation.
    #[error("native effect acknowledgement for surface {surface} belongs to another provider")]
    EffectAcknowledgementProviderMismatch {
        /// Stable logical surface whose fact contained the stale acknowledgement.
        surface: SurfaceId,
    },
    /// Facade-owned typed facts violated an internal platform invariant.
    #[error("facade-owned native platform data violated an internal invariant")]
    ProtocolInvariant,
    /// An operation was submitted through the wrong native host profile.
    #[error("native operation is unavailable for the enrolled host profile")]
    HostProfileMismatch,
    /// A managed snapshot supplied an empty, duplicate, or invalid work-area roster.
    #[error("managed native work-area roster is invalid")]
    InvalidWorkAreaRoster,
    /// Native pointer facts are contradictory or name a stale capability.
    #[error("managed native pointer facts are invalid or stale")]
    InvalidPointerFacts,
    /// The desktop pointer sequence cannot advance without wrapping.
    #[error("managed native pointer sequence is exhausted")]
    PointerSequenceExhausted,
    /// A desktop pointer segment needs exact receiver facts, but the host did
    /// not provide its synchronous resolver for this frame.
    #[error("native desktop pointer input requires a receiver resolver")]
    ReceiverResolverRequired,
}

impl NativePlatformError {
    pub(super) const fn kind(&self) -> NativeHostErrorKind {
        match self {
            Self::ProviderUnavailable => NativeHostErrorKind::NotEnabled,
            Self::ProviderAlreadyEnabled => NativeHostErrorKind::AlreadyEnabled,
            Self::ProviderSuperseded | Self::StaleSurface { .. } => {
                NativeHostErrorKind::StaleBinding
            }
            Self::DuplicateSurface { .. }
            | Self::IncompleteRoster
            | Self::InvalidWorkAreaRoster
            | Self::InvalidPointerFacts
            | Self::DestroyedSurfaceHasLiveFacts { .. }
            | Self::RetiredSurfaceHasLiveFacts { .. }
            | Self::EffectAcknowledgementBindingMismatch { .. }
            | Self::EffectAcknowledgementProviderMismatch { .. } => {
                NativeHostErrorKind::InvalidFacts
            }
            Self::PointerProviderAlreadyEnabled
            | Self::BindingRosterUnsettled
            | Self::BindingStillLive { .. }
            | Self::BindingNotRetired { .. } => NativeHostErrorKind::OperationConflict,
            Self::ReceiverResolverRequired | Self::HostProfileMismatch => {
                NativeHostErrorKind::Unsupported
            }
            Self::GenerationExhausted
            | Self::PointerSequenceExhausted
            | Self::ProtocolInvariant => NativeHostErrorKind::Internal,
        }
    }
}

#[derive(Debug)]
pub(super) struct RuntimeNativeState {
    pub(super) profile: NativeHostProfile,
    recorder: BackendIngressRecorder,
    pending_prefix_retirement: Option<BackendIngressPrefixRetirementReceipt>,
    snapshot_generation: u64,
    binding_roster_unsettled: bool,
    bindings: BTreeMap<SurfaceId, NativeSurfaceBinding>,
    retired_bindings: BTreeSet<ViewportBinding>,
    work_areas: BTreeMap<WorkAreaToken, NativeWorkAreaBinding>,
    close_generations: BTreeMap<ViewportBinding, u64>,
}

#[derive(Debug)]
struct CompiledNativeWindow {
    is_live: bool,
    window: Option<ObservedWindow>,
    close: WindowCloseObservation,
}

impl RuntimeNativeState {
    fn new(recorder: BackendIngressRecorder, profile: NativeHostProfile) -> Self {
        Self {
            profile,
            recorder,
            pending_prefix_retirement: None,
            snapshot_generation: 0,
            binding_roster_unsettled: false,
            bindings: BTreeMap::new(),
            retired_bindings: BTreeSet::new(),
            work_areas: BTreeMap::new(),
            close_generations: BTreeMap::new(),
        }
    }

    pub(super) const fn provider(&self) -> PlatformObservationLease {
        self.recorder.lease().platform_provider()
    }

    pub(super) fn contains_binding(&self, binding: NativeSurfaceBinding) -> bool {
        binding.provider == self.provider()
            && self.bindings.get(&binding.surface()) == Some(&binding)
    }

    pub(super) fn recognizes_binding(&self, binding: NativeSurfaceBinding) -> bool {
        binding.provider == self.provider()
            && (self.bindings.get(&binding.surface()) == Some(&binding)
                || self.retired_bindings.contains(&binding.binding))
    }

    pub(super) fn recorder_mut(&mut self) -> &mut BackendIngressRecorder {
        &mut self.recorder
    }

    pub(super) fn prepare_batch(
        &mut self,
        engine: &crate::engine::DockEngine,
    ) -> Result<BackendIngressBatch, NativePlatformError> {
        let committed = engine.backend_ingress_committed_through();
        self.recorder
            .batch_after(committed)
            .map_err(|_| NativePlatformError::ProtocolInvariant)
    }

    pub(super) fn reclaim_committed_prefix(
        &mut self,
        engine: &mut crate::engine::DockEngine,
    ) -> Result<(), DockspaceRuntimeError> {
        self.settle_pending_prefix_retirement(engine)?;
        if let Some(watermark) = engine.backend_ingress_commit_watermark() {
            self.pending_prefix_retirement = self
                .recorder
                .retire_committed_prefix(watermark)
                .map_err(|_| NativePlatformError::ProtocolInvariant)?;
            self.settle_pending_prefix_retirement(engine)?;
        }
        Ok(())
    }

    fn settle_pending_prefix_retirement(
        &mut self,
        engine: &mut crate::engine::DockEngine,
    ) -> Result<(), DockspaceRuntimeError> {
        if let Some(receipt) = self.pending_prefix_retirement.as_mut() {
            let _ = engine.settle_backend_ingress_prefix_retirement(receipt)?;
            self.pending_prefix_retirement = None;
        }
        Ok(())
    }

    fn record_close(
        &mut self,
        expected_epoch: WorkspaceEpoch,
        observation: WindowCloseObservation,
    ) -> Result<(), NativePlatformError> {
        self.recorder
            .record_native_close_observation(expected_epoch, observation)
            .map_err(|_| NativePlatformError::ProtocolInvariant)?;
        Ok(())
    }

    fn record_effect_result(
        &mut self,
        result: crate::effect::EffectResult,
    ) -> Result<(), NativePlatformError> {
        self.recorder
            .record_platform_effect_result(result)
            .map_err(|_| NativePlatformError::ProtocolInvariant)?;
        Ok(())
    }

    fn record_binding_quiescence(
        &mut self,
        binding: NativeSurfaceBinding,
    ) -> Result<(), NativePlatformError> {
        if binding.provider != self.provider() {
            return Err(NativePlatformError::ProviderSuperseded);
        }
        if self.bindings.get(&binding.surface()) == Some(&binding) {
            return Err(NativePlatformError::BindingStillLive {
                surface: binding.surface(),
            });
        }
        if !self.retired_bindings.remove(&binding.binding) {
            return Err(NativePlatformError::BindingNotRetired {
                surface: binding.surface(),
            });
        }
        if self
            .recorder
            .record_platform_binding_quiescence(binding.binding)
            .is_err()
        {
            self.retired_bindings.insert(binding.binding);
            return Err(NativePlatformError::ProtocolInvariant);
        }
        Ok(())
    }

    pub(super) fn record_abandoned_effects(
        &mut self,
        abandoned: &super::native_effect::NativeEffectDropQueue,
    ) -> Result<(), NativePlatformError> {
        for result in abandoned.take_for(self.provider()) {
            self.record_effect_result(result.result)?;
        }
        Ok(())
    }

    fn next_snapshot_generation(&self) -> Result<u64, NativePlatformError> {
        self.close_generations
            .values()
            .copied()
            .max()
            .unwrap_or(0)
            .max(self.snapshot_generation)
            .checked_add(1)
            .ok_or(NativePlatformError::GenerationExhausted)
    }

    fn validate_snapshot_roster(
        &self,
        observations: impl IntoIterator<Item = (NativeSurfaceBinding, NativeWindowFacts)>,
    ) -> Result<BTreeMap<ViewportBinding, NativeWindowFacts>, NativePlatformError> {
        if self.binding_roster_unsettled {
            return Err(NativePlatformError::BindingRosterUnsettled);
        }
        let mut supplied = BTreeMap::new();
        for (binding, facts) in observations {
            if binding.provider != self.provider() {
                return Err(NativePlatformError::ProviderSuperseded);
            }
            let current = self.bindings.get(&binding.surface()) == Some(&binding);
            let retired = self.retired_bindings.contains(&binding.binding);
            if !current && !retired {
                return Err(NativePlatformError::StaleSurface {
                    surface: binding.surface(),
                });
            }
            if retired && !matches!(facts.lifecycle, NativeWindowLifecycleFact::Destroyed { .. }) {
                return Err(NativePlatformError::RetiredSurfaceHasLiveFacts {
                    surface: binding.surface(),
                });
            }
            if supplied.insert(binding.binding, facts).is_some() {
                return Err(NativePlatformError::DuplicateSurface {
                    surface: binding.surface(),
                });
            }
        }
        if self
            .bindings
            .values()
            .any(|binding| !supplied.contains_key(&binding.binding))
        {
            return Err(NativePlatformError::IncompleteRoster);
        }
        Ok(supplied)
    }

    fn record_snapshot_facts(
        &mut self,
        expected_epoch: WorkspaceEpoch,
        observations: impl IntoIterator<Item = (NativeSurfaceBinding, NativeWindowFacts)>,
        work_areas: NativeWorkAreaRoster,
    ) -> Result<(), NativePlatformError> {
        if self.profile == NativeHostProfile::ObservedRoots
            && !matches!(work_areas, NativeWorkAreaRoster::Unknown)
        {
            return Err(NativePlatformError::HostProfileMismatch);
        }
        let generation = self.next_snapshot_generation()?;
        let supplied = self.validate_snapshot_roster(observations)?;
        let snapshot = compile_platform_snapshot(
            self.profile,
            self.provider(),
            generation,
            &supplied,
            &work_areas,
        )?;
        self.recorder
            .record_platform_snapshot(expected_epoch, snapshot)
            .map_err(|_| NativePlatformError::ProtocolInvariant)?;
        self.binding_roster_unsettled = supplied
            .values()
            .any(|facts| matches!(facts.lifecycle, NativeWindowLifecycleFact::Destroyed { .. }));
        self.snapshot_generation = generation;
        for binding in supplied.into_keys() {
            self.close_generations.insert(binding, generation);
        }
        Ok(())
    }

    fn record_unknown_inventory(
        &mut self,
        expected_epoch: WorkspaceEpoch,
    ) -> Result<(), NativePlatformError> {
        let generation = self.next_snapshot_generation()?;
        let snapshot = compile_unknown_inventory_snapshot(self.profile, generation)?;
        self.recorder
            .record_platform_snapshot(expected_epoch, snapshot)
            .map_err(|_| NativePlatformError::ProtocolInvariant)?;
        self.snapshot_generation = generation;
        for binding in self.bindings.values() {
            self.close_generations.insert(binding.binding, generation);
        }
        Ok(())
    }

    fn record_root_registration(
        &mut self,
        expected: crate::model::WorkspaceVersion,
        surface: SurfaceId,
        token: WindowToken,
    ) -> Result<(), NativePlatformError> {
        self.recorder
            .record_viewport_registration(expected, surface, token, ViewportRole::Root, None)
            .map_err(|_| NativePlatformError::ProtocolInvariant)?;
        self.binding_roster_unsettled = true;
        Ok(())
    }

    pub(super) fn commit(&mut self, engine: &crate::engine::DockEngine) {
        let next_bindings: BTreeMap<SurfaceId, NativeSurfaceBinding> = engine
            .viewport()
            .registry()
            .records()
            .map(|(surface, record)| {
                (
                    surface,
                    NativeSurfaceBinding::from_binding(self.provider(), record.binding()),
                )
            })
            .collect();
        for binding in self.bindings.values() {
            if next_bindings.get(&binding.surface()) != Some(binding) {
                self.retired_bindings.insert(binding.binding);
            }
        }
        self.bindings = next_bindings;
        self.work_areas = engine
            .viewport()
            .work_areas()
            .map(|(token, _)| {
                (
                    token,
                    NativeWorkAreaBinding::new(
                        self.provider(),
                        engine.viewport().work_area_generation(),
                        token,
                    ),
                )
            })
            .collect();
        self.close_generations.retain(|binding, _| {
            self.bindings
                .get(&binding.surface())
                .is_some_and(|current| current.binding == *binding)
        });
        self.binding_roster_unsettled = false;
    }
}
