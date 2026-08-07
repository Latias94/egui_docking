//! Opaque native-window lifecycle capabilities for renderer-neutral hosts.

use std::collections::{BTreeMap, BTreeSet};

use thiserror::Error;

use super::native_effect::{
    NativeCloseEffectAcknowledgement, NativeEffectResult, NativeEffectSubmissionError,
    NativeInputEffectAcknowledgement, NativePresentationEffectAcknowledgement,
};
use super::{DockspaceRuntimeError, DockspaceSession, HostFrameReport};
use crate::PlatformObservationLease;
use crate::backend_ingress::{
    BackendIngressBatch, BackendIngressDrainReceipt, BackendIngressOrdinal,
    BackendIngressPrefixRetirementReceipt, BackendIngressProviderReplacementTicket,
    BackendIngressRecorder,
};
use crate::engine::EngineInput;
use crate::geometry::{PhysicalRect, ScaleFactor};
use crate::ids::{SurfaceId, WorkspaceEpoch};
use crate::intent::{Authority, AuthorityUnavailableReason};
use crate::platform::{
    CapabilityRosterObservation, CloseEffectAcknowledgement, InputEffectAcknowledgement,
    ObservedWindow, PlatformCapabilities, PlatformCapability, PlatformCapabilityReason,
    PlatformRequirement, PlatformSnapshot, PresentationEffectAcknowledgement,
    WindowCloseObservation, WindowCloseState, WindowCoordinateObservation, WindowInputObservation,
    WindowInputState, WindowInventoryObservation, WindowPresentationObservation,
    WindowPresentationState, WorkAreaRosterObservation,
};
use crate::pointer_journal::{PointerEdgeJournal, PointerEdgeSequence};
use crate::viewport::{
    CapabilityObservationGeneration, CloseObservationGeneration, CoordinateObservationGeneration,
    InputObservationGeneration, InventoryObservationGeneration, PlatformSnapshotGeneration,
    PresentationObservationGeneration, ViewportBinding, ViewportRole, WindowToken,
    WorkAreaObservationGeneration,
};
use crate::viewport_focus::{FocusObservationEnvelope, FocusObservationGeneration};

/// Adapter-owned opaque native-window token.
///
/// A token may be reused after exact destruction. It is not sufficient input
/// authority by itself; every observation also carries a core-minted
/// [`NativeSurfaceLease`].
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

/// Opaque exact-incarnation capability for one native docking surface.
///
/// The value is copyable so asynchronous callbacks may retain it. No accessor
/// exposes the core incarnation, workspace epoch, or authority domain.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct NativeSurfaceLease {
    provider: PlatformObservationLease,
    binding: ViewportBinding,
}

impl NativeSurfaceLease {
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

    /// Returns the exact provider-bound native surface capability.
    #[must_use]
    pub const fn native_surface(&self) -> NativeSurfaceLease {
        NativeSurfaceLease::from_binding(self.provider, self.edge.binding())
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

/// Structural capability mode owned by the native runtime facade.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NativePlatformMode {
    /// Observe an exact roster of externally owned root windows.
    ///
    /// The facade will not claim native create, replacement, or close-cancellation
    /// support in this mode.
    ObservedRoots,
    /// Dispatch the complete native lifecycle effect protocol.
    ManagedWindows,
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

/// Retryable complete native snapshot minted by one session sidecar.
#[derive(Debug, Clone, PartialEq)]
pub struct NativePlatformSnapshot {
    provider: PlatformObservationLease,
    expected_epoch: WorkspaceEpoch,
    generation: u64,
    bindings: Vec<ViewportBinding>,
    snapshot: PlatformSnapshot,
}

/// Native lifecycle failure at the stable facade boundary.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum NativePlatformError {
    /// No native provider was enrolled for this session.
    #[error("no facade-owned native platform provider is active")]
    ProviderUnavailable,
    /// The submitted snapshot belongs to a superseded provider.
    #[error("native snapshot belongs to a superseded platform provider")]
    ProviderSuperseded,
    /// The provider is already enrolled under another structural mode.
    #[error("native platform provider is already active under a different mode")]
    ProviderModeConflict,
    /// A joined provider handoff is pending and must be finished, retried, or aborted.
    #[error("native platform provider replacement is pending")]
    ProviderReplacementPending,
    /// No joined provider handoff is pending.
    #[error("native platform provider replacement is not pending")]
    ProviderReplacementUnavailable,
    /// A callback named an older binding incarnation for this logical surface.
    #[error("native surface {surface} lease is stale")]
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
    /// A snapshot was captured against an older workspace epoch.
    #[error("native snapshot belongs to an older workspace epoch")]
    StaleWorkspace,
    /// Provider-owned observation generations cannot advance without wrapping.
    #[error("native observation generation is exhausted")]
    GenerationExhausted,
    /// Snapshot generation is no longer the next provider-owned generation.
    #[error("native platform snapshot generation is stale")]
    SnapshotGenerationStale,
    /// The live binding roster changed after this snapshot was captured.
    #[error("native platform snapshot was captured for an older binding roster")]
    SnapshotRosterStale,
    /// Destroyed inventory facts also carried live-window properties.
    #[error("destroyed native surface {surface} also reported live-window properties")]
    DestroyedSurfaceHasLiveFacts {
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
    /// A private joined recorder contains a desktop pointer segment, but the
    /// renderer-neutral façade has no receiver-proof API for it yet.
    #[error("native desktop pointer input requires a receiver-proof adapter")]
    DesktopPointerInputUnsupported,
}

#[derive(Debug)]
pub(super) struct RuntimeNativeState {
    recorder: BackendIngressRecorder,
    pending_prefix_retirement: Option<BackendIngressPrefixRetirementReceipt>,
    pending_pointer_checkpoint: Option<BackendIngressOrdinal>,
    mode: NativePlatformMode,
    snapshot_generation: u64,
    bindings: BTreeMap<SurfaceId, NativeSurfaceLease>,
    retired_bindings: BTreeSet<ViewportBinding>,
    close_generations: BTreeMap<ViewportBinding, u64>,
}

#[derive(Debug)]
pub(super) enum RuntimeNativeHandoff {
    Drained {
        mode: NativePlatformMode,
        receipt: BackendIngressDrainReceipt,
    },
    Replacing {
        mode: NativePlatformMode,
        ticket: BackendIngressProviderReplacementTicket,
    },
}

#[derive(Debug)]
struct CompiledNativeWindow {
    is_live: bool,
    window: Option<ObservedWindow>,
    close: WindowCloseObservation,
}

impl RuntimeNativeState {
    fn new(recorder: BackendIngressRecorder, mode: NativePlatformMode) -> Self {
        Self {
            recorder,
            pending_prefix_retirement: None,
            pending_pointer_checkpoint: None,
            mode,
            snapshot_generation: 0,
            bindings: BTreeMap::new(),
            retired_bindings: BTreeSet::new(),
            close_generations: BTreeMap::new(),
        }
    }

    pub(super) const fn provider(&self) -> PlatformObservationLease {
        self.recorder.lease().platform_provider()
    }

    pub(super) fn recorder_mut(&mut self) -> &mut BackendIngressRecorder {
        &mut self.recorder
    }

    fn validate_replacement(
        &self,
        engine: &crate::engine::DockEngine,
    ) -> Result<(), DockspaceRuntimeError> {
        engine
            .validate_backend_ingress_provider_replacement(&self.recorder.drain_preview())
            .map_err(DockspaceRuntimeError::from)
    }

    pub(super) fn prepare_batch(
        &mut self,
        engine: &crate::engine::DockEngine,
    ) -> Result<BackendIngressBatch, NativePlatformError> {
        let committed = engine.backend_ingress_committed_through();
        if self
            .pending_pointer_checkpoint
            .is_some_and(|checkpoint| checkpoint <= committed)
        {
            self.pending_pointer_checkpoint = None;
        }
        if self.pending_pointer_checkpoint.is_none() {
            let checkpoint = PointerEdgeJournal::new(
                self.recorder.pointer_through(),
                self.recorder.pointer_through(),
                Vec::new(),
            )
            .map_err(|_| NativePlatformError::ProtocolInvariant)?;
            let ordinal = self
                .recorder
                .record_pointer_segment(checkpoint)
                .map_err(|_| NativePlatformError::ProtocolInvariant)?;
            self.pending_pointer_checkpoint = Some(ordinal);
        }
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

    fn record_snapshot(
        &mut self,
        snapshot: NativePlatformSnapshot,
    ) -> Result<(), NativePlatformError> {
        self.recorder
            .record_platform_snapshot(snapshot.expected_epoch, snapshot.snapshot)
            .map_err(|_| NativePlatformError::ProtocolInvariant)?;
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
        lease: NativeSurfaceLease,
    ) -> Result<(), NativePlatformError> {
        if lease.provider != self.provider() {
            return Err(NativePlatformError::ProviderSuperseded);
        }
        if self.bindings.get(&lease.surface()) == Some(&lease) {
            return Err(NativePlatformError::BindingStillLive {
                surface: lease.surface(),
            });
        }
        if !self.retired_bindings.remove(&lease.binding) {
            return Err(NativePlatformError::BindingNotRetired {
                surface: lease.surface(),
            });
        }
        if self
            .recorder
            .record_platform_binding_quiescence(lease.binding)
            .is_err()
        {
            self.retired_bindings.insert(lease.binding);
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
        observations: impl IntoIterator<Item = (NativeSurfaceLease, NativeWindowFacts)>,
    ) -> Result<BTreeMap<ViewportBinding, NativeWindowFacts>, NativePlatformError> {
        let mut supplied = BTreeMap::new();
        for (lease, facts) in observations {
            if self.bindings.get(&lease.surface()) != Some(&lease) {
                return Err(NativePlatformError::StaleSurface {
                    surface: lease.surface(),
                });
            }
            if supplied.insert(lease.binding, facts).is_some() {
                return Err(NativePlatformError::DuplicateSurface {
                    surface: lease.surface(),
                });
            }
        }
        if supplied.len() != self.bindings.len() {
            return Err(NativePlatformError::IncompleteRoster);
        }
        Ok(supplied)
    }

    fn capture_snapshot(
        &self,
        expected_epoch: WorkspaceEpoch,
        observations: impl IntoIterator<Item = (NativeSurfaceLease, NativeWindowFacts)>,
    ) -> Result<NativePlatformSnapshot, NativePlatformError> {
        let generation = self.next_snapshot_generation()?;
        let supplied = self.validate_snapshot_roster(observations)?;
        let snapshot =
            compile_platform_snapshot(self.provider(), self.mode, generation, &supplied)?;
        Ok(NativePlatformSnapshot {
            provider: self.provider(),
            expected_epoch,
            generation,
            bindings: supplied.into_keys().collect(),
            snapshot,
        })
    }

    fn capture_unknown_inventory_snapshot(
        &self,
        expected_epoch: WorkspaceEpoch,
    ) -> Result<NativePlatformSnapshot, NativePlatformError> {
        let generation = self.next_snapshot_generation()?;
        let snapshot = compile_unknown_inventory_snapshot(self.mode, generation)?;
        Ok(NativePlatformSnapshot {
            provider: self.provider(),
            expected_epoch,
            generation,
            bindings: self.bindings.values().map(|lease| lease.binding).collect(),
            snapshot,
        })
    }

    pub(super) fn commit(&mut self, engine: &crate::engine::DockEngine) {
        let next_bindings: BTreeMap<SurfaceId, NativeSurfaceLease> = engine
            .viewport()
            .registry()
            .records()
            .map(|(surface, record)| {
                (
                    surface,
                    NativeSurfaceLease::from_binding(self.provider(), record.binding()),
                )
            })
            .collect();
        for lease in self.bindings.values() {
            if next_bindings.get(&lease.surface()) != Some(lease) {
                self.retired_bindings.insert(lease.binding);
            }
        }
        self.bindings = next_bindings;
        self.close_generations.retain(|binding, _| {
            self.bindings
                .get(&binding.surface())
                .is_some_and(|lease| lease.binding == *binding)
        });
    }

    fn into_drain(self) -> RuntimeNativeHandoff {
        debug_assert!(self.pending_prefix_retirement.is_none());
        RuntimeNativeHandoff::Drained {
            mode: self.mode,
            receipt: self.recorder.drain(),
        }
    }
}

fn compile_unknown_inventory_snapshot(
    mode: NativePlatformMode,
    generation: u64,
) -> Result<PlatformSnapshot, NativePlatformError> {
    let reason = AuthorityUnavailableReason::NotReported;
    PlatformSnapshot::new(
        PlatformSnapshotGeneration::new(generation),
        CapabilityRosterObservation::new(
            CapabilityObservationGeneration::new(generation),
            Authority::Known(native_capabilities(mode)),
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
        WorkAreaRosterObservation::unknown(WorkAreaObservationGeneration::new(generation), reason),
    )
    .map_err(|_| NativePlatformError::ProtocolInvariant)
}

fn compile_platform_snapshot(
    provider: PlatformObservationLease,
    mode: NativePlatformMode,
    generation: u64,
    supplied: &BTreeMap<ViewportBinding, NativeWindowFacts>,
) -> Result<PlatformSnapshot, NativePlatformError> {
    let reason = AuthorityUnavailableReason::NotReported;
    let mut live_bindings = Vec::new();
    let mut windows = Vec::new();
    let mut close_observations = Vec::new();
    for (&binding, &facts) in supplied {
        let compiled = compile_window_fact(provider, binding, facts, generation)?;
        if compiled.is_live {
            live_bindings.push(binding);
        }
        if let Some(window) = compiled.window {
            windows.push(window);
        }
        close_observations.push(compiled.close);
    }

    let capabilities = native_capabilities(mode);
    PlatformSnapshot::new(
        PlatformSnapshotGeneration::new(generation),
        CapabilityRosterObservation::new(
            CapabilityObservationGeneration::new(generation),
            Authority::Known(capabilities),
        ),
        FocusObservationEnvelope::new(
            FocusObservationGeneration::new(generation),
            Authority::Unknown(reason),
            Authority::Unknown(reason),
        ),
        WindowInventoryObservation::new(
            InventoryObservationGeneration::new(generation),
            Authority::Known(live_bindings),
        )
        .map_err(|_| NativePlatformError::ProtocolInvariant)?,
        windows,
        close_observations,
        WorkAreaRosterObservation::unknown(WorkAreaObservationGeneration::new(generation), reason),
    )
    .map_err(|_| NativePlatformError::ProtocolInvariant)
}

fn native_capabilities(mode: NativePlatformMode) -> PlatformCapabilities {
    let unsupported = |requirement| {
        PlatformCapability::unsupported(requirement, PlatformCapabilityReason::BackendUnsupported)
    };
    let mut capabilities = PlatformCapabilities::default();
    capabilities.set_native_window_lifecycle(match mode {
        NativePlatformMode::ObservedRoots => {
            unsupported(PlatformRequirement::NativeWindowLifecycle)
        }
        NativePlatformMode::ManagedWindows => PlatformCapability::Supported,
    });
    capabilities.set_authoritative_inventory(PlatformCapability::Supported);
    capabilities.set_hovered_window(unsupported(PlatformRequirement::HoveredWindow));
    capabilities
        .set_desktop_pointer_position(unsupported(PlatformRequirement::DesktopPointerPosition));
    capabilities
        .set_authoritative_button_state(unsupported(PlatformRequirement::AuthoritativeButtonState));
    capabilities
        .set_global_window_placement(unsupported(PlatformRequirement::GlobalWindowPlacement));
    capabilities.set_work_area(unsupported(PlatformRequirement::WorkArea));
    capabilities.set_pointer_hit_test_observation(unsupported(
        PlatformRequirement::PointerHitTestObservation,
    ));
    capabilities
        .set_pointer_hit_test_control(unsupported(PlatformRequirement::PointerHitTestControl));
    capabilities
        .set_global_focus_observation(unsupported(PlatformRequirement::GlobalFocusObservation));
    capabilities
        .set_window_activation_control(unsupported(PlatformRequirement::WindowActivationControl));
    capabilities.set_close_cancellation(match mode {
        NativePlatformMode::ObservedRoots => unsupported(PlatformRequirement::CloseCancellation),
        NativePlatformMode::ManagedWindows => PlatformCapability::Supported,
    });
    capabilities
}

fn compile_window_fact(
    provider: PlatformObservationLease,
    binding: ViewportBinding,
    facts: NativeWindowFacts,
    generation: u64,
) -> Result<CompiledNativeWindow, NativePlatformError> {
    let coordinate_generation = CoordinateObservationGeneration::new(generation);
    let input_generation = InputObservationGeneration::new(generation);
    let presentation_generation = PresentationObservationGeneration::new(generation);
    let close_generation = CloseObservationGeneration::new(generation);
    let reason = AuthorityUnavailableReason::NotReported;

    match facts.lifecycle {
        NativeWindowLifecycleFact::Live => Ok(CompiledNativeWindow {
            is_live: true,
            window: Some(
                ObservedWindow::new(binding)
                    .with_coordinate_observation(WindowCoordinateObservation::new(
                        binding,
                        coordinate_generation,
                        fact_authority(facts.content_bounds, reason),
                        fact_authority(facts.outer_bounds, reason),
                        fact_authority(facts.native_scale_factor, reason),
                        fact_authority(facts.presentation_scale_factor, reason),
                    ))
                    .with_input_observation(WindowInputObservation::new(
                        binding,
                        input_generation,
                        facts.input.map_or(Authority::Unknown(reason), |input| {
                            Authority::Known(input.state.into())
                        }),
                        input_acknowledgement(provider, binding, facts.input, reason)?,
                    ))
                    .with_presentation_observation(WindowPresentationObservation::new(
                        binding,
                        presentation_generation,
                        facts
                            .presentation
                            .map_or(Authority::Unknown(reason), |presentation| {
                                Authority::Known(presentation.state.into())
                            }),
                        presentation_acknowledgement(
                            provider,
                            binding,
                            facts.presentation,
                            reason,
                        )?,
                    )),
            ),
            close: WindowCloseObservation::new(
                binding,
                close_generation,
                facts.close.map_or(Authority::Unknown(reason), |close| {
                    Authority::Known(match close.state {
                        NativeCloseState::Clear => WindowCloseState::LiveClear,
                        NativeCloseState::Requested => WindowCloseState::LiveRequested,
                    })
                }),
                close_acknowledgement(provider, binding, facts.close, reason)?,
            ),
        }),
        NativeWindowLifecycleFact::Destroyed { acknowledgement } => {
            if facts.content_bounds.is_some()
                || facts.outer_bounds.is_some()
                || facts.native_scale_factor.is_some()
                || facts.presentation_scale_factor.is_some()
                || facts.input.is_some()
                || facts.presentation.is_some()
                || facts.close.is_some()
            {
                return Err(NativePlatformError::DestroyedSurfaceHasLiveFacts {
                    surface: binding.surface(),
                });
            }
            Ok(CompiledNativeWindow {
                is_live: false,
                window: None,
                close: WindowCloseObservation::new(
                    binding,
                    close_generation,
                    Authority::Known(WindowCloseState::Destroyed),
                    known_close_acknowledgement(provider, binding, acknowledgement)?,
                ),
            })
        }
    }
}

fn fact_authority<T: Copy>(value: Option<T>, reason: AuthorityUnavailableReason) -> Authority<T> {
    value.map_or(Authority::Unknown(reason), Authority::Known)
}

fn input_acknowledgement(
    provider: PlatformObservationLease,
    binding: ViewportBinding,
    fact: Option<NativeInputFact>,
    reason: AuthorityUnavailableReason,
) -> Result<InputEffectAcknowledgement, NativePlatformError> {
    let Some(fact) = fact else {
        return Ok(InputEffectAcknowledgement::unknown(reason));
    };
    match fact.acknowledgement {
        Some(acknowledgement) if acknowledgement.provider != provider => {
            Err(NativePlatformError::EffectAcknowledgementProviderMismatch {
                surface: binding.surface(),
            })
        }
        Some(acknowledgement) if acknowledgement.binding != binding => {
            Err(NativePlatformError::EffectAcknowledgementBindingMismatch {
                surface: binding.surface(),
            })
        }
        Some(acknowledgement) => Ok(InputEffectAcknowledgement::known(Some(
            acknowledgement.effect,
        ))),
        None => Ok(InputEffectAcknowledgement::known(None)),
    }
}

fn presentation_acknowledgement(
    provider: PlatformObservationLease,
    binding: ViewportBinding,
    fact: Option<NativePresentationFact>,
    reason: AuthorityUnavailableReason,
) -> Result<PresentationEffectAcknowledgement, NativePlatformError> {
    let Some(fact) = fact else {
        return Ok(PresentationEffectAcknowledgement::unknown(reason));
    };
    match fact.acknowledgement {
        Some(acknowledgement) if acknowledgement.provider != provider => {
            Err(NativePlatformError::EffectAcknowledgementProviderMismatch {
                surface: binding.surface(),
            })
        }
        Some(acknowledgement) if acknowledgement.binding != binding => {
            Err(NativePlatformError::EffectAcknowledgementBindingMismatch {
                surface: binding.surface(),
            })
        }
        Some(acknowledgement) => Ok(PresentationEffectAcknowledgement::known(Some(
            acknowledgement.effect,
        ))),
        None => Ok(PresentationEffectAcknowledgement::known(None)),
    }
}

fn close_acknowledgement(
    provider: PlatformObservationLease,
    binding: ViewportBinding,
    fact: Option<NativeCloseFact>,
    reason: AuthorityUnavailableReason,
) -> Result<CloseEffectAcknowledgement, NativePlatformError> {
    let Some(fact) = fact else {
        return Ok(CloseEffectAcknowledgement::unknown(reason));
    };
    known_close_acknowledgement(provider, binding, fact.acknowledgement)
}

fn known_close_acknowledgement(
    provider: PlatformObservationLease,
    binding: ViewportBinding,
    acknowledgement: Option<NativeCloseEffectAcknowledgement>,
) -> Result<CloseEffectAcknowledgement, NativePlatformError> {
    match acknowledgement {
        Some(acknowledgement) if acknowledgement.provider != provider => {
            Err(NativePlatformError::EffectAcknowledgementProviderMismatch {
                surface: binding.surface(),
            })
        }
        Some(acknowledgement) if acknowledgement.binding != binding => {
            Err(NativePlatformError::EffectAcknowledgementBindingMismatch {
                surface: binding.surface(),
            })
        }
        Some(acknowledgement) => Ok(CloseEffectAcknowledgement::known(Some(
            acknowledgement.effect,
        ))),
        None => Ok(CloseEffectAcknowledgement::known(None)),
    }
}

impl DockspaceSession {
    fn native_state(&self) -> Result<&RuntimeNativeState, NativePlatformError> {
        if self.native_handoff.is_some() {
            return Err(NativePlatformError::ProviderReplacementPending);
        }
        self.native
            .as_ref()
            .ok_or(NativePlatformError::ProviderUnavailable)
    }

    fn native_state_mut(&mut self) -> Result<&mut RuntimeNativeState, NativePlatformError> {
        if self.native_handoff.is_some() {
            return Err(NativePlatformError::ProviderReplacementPending);
        }
        self.native
            .as_mut()
            .ok_or(NativePlatformError::ProviderUnavailable)
    }

    /// Enrolls the session-owned native platform observation source.
    ///
    /// Repeating this call is inert. Transport replacement is available through
    /// the joined begin/finish/abort methods without exposing provider tickets.
    ///
    /// # Errors
    ///
    /// Returns an error when the core cannot mint platform authority.
    pub fn enable_native_platform(
        &mut self,
        mode: NativePlatformMode,
    ) -> Result<(), DockspaceRuntimeError> {
        if self.native_handoff.is_some() {
            return Err(NativePlatformError::ProviderReplacementPending.into());
        }
        if let Some(native) = &self.native {
            return if native.mode == mode {
                Ok(())
            } else {
                Err(NativePlatformError::ProviderModeConflict.into())
            };
        }
        let recorder = self
            .engine
            .create_backend_ingress_provider(self.presentation_host, PointerEdgeSequence::new(0))?;
        let mut native = RuntimeNativeState::new(recorder, mode);
        native.commit(&self.engine);
        self.native = Some(native);
        Ok(())
    }

    /// Revokes the current joined provider and starts one atomic successor handoff.
    ///
    /// Calling this method again after a transient start failure retries the same
    /// core-owned drain proof. No platform, pointer, or ordering ticket is
    /// exposed through the public façade.
    ///
    /// # Errors
    ///
    /// Returns an error when no native provider is active or the core cannot
    /// reserve the joined successor. A failed reservation retains the exact
    /// drained proof for a later retry.
    pub fn begin_native_provider_replacement(
        &mut self,
    ) -> Result<HostFrameReport, DockspaceRuntimeError> {
        if self.native_handoff.is_none() {
            let Some(mut native) = self.native.take() else {
                return Err(NativePlatformError::ProviderUnavailable.into());
            };
            if let Err(error) = native.record_abandoned_effects(&self.abandoned_native_effects) {
                self.native = Some(native);
                return Err(error.into());
            }
            if let Err(error) = native.reclaim_committed_prefix(&mut self.engine) {
                self.native = Some(native);
                return Err(error);
            }
            if let Err(error) = native.validate_replacement(&self.engine) {
                self.native = Some(native);
                return Err(error);
            }
            self.native_handoff = Some(native.into_drain());
            self.presentation.discard_uncommitted_backend_records();
        }
        self.start_drained_native_handoff()
    }

    /// Activates the reserved joined provider successor.
    ///
    /// Failure leaves the affine ticket inside the session so the exact same
    /// handoff can be retried without reissuing or reconstructing authority.
    ///
    /// # Errors
    ///
    /// Returns an error when no reserved replacement exists or the core rejects
    /// successor activation.
    pub fn finish_native_provider_replacement(&mut self) -> Result<(), DockspaceRuntimeError> {
        let presentation_host = self.presentation_host;
        let mode = match self.native_handoff.as_mut() {
            Some(RuntimeNativeHandoff::Replacing { mode, ticket, .. }) => {
                let recorder = self
                    .engine
                    .finish_backend_ingress_provider_replacement(ticket, presentation_host)?;
                let mode = *mode;
                let mut native = RuntimeNativeState::new(recorder, mode);
                native.commit(&self.engine);
                self.native = Some(native);
                mode
            }
            Some(RuntimeNativeHandoff::Drained { .. }) => {
                return Err(NativePlatformError::ProviderReplacementPending.into());
            }
            None => return Err(NativePlatformError::ProviderReplacementUnavailable.into()),
        };
        debug_assert!(
            self.native
                .as_ref()
                .is_some_and(|native| native.mode == mode)
        );
        self.native_handoff = None;
        Ok(())
    }

    /// Abandons one reserved joined handoff and leaves the session unenrolled.
    ///
    /// The predecessor remains permanently revoked. The returned report carries
    /// any effects or repaint requirements emitted by the core-owned abort.
    ///
    /// # Errors
    ///
    /// Returns an error when no reserved replacement exists. A merely drained
    /// start must first be retried through [`Self::begin_native_provider_replacement`].
    pub fn abort_native_provider_replacement(
        &mut self,
    ) -> Result<HostFrameReport, DockspaceRuntimeError> {
        match self.native_handoff.as_ref() {
            Some(RuntimeNativeHandoff::Replacing { .. }) => {}
            Some(RuntimeNativeHandoff::Drained { .. }) => {
                return Err(NativePlatformError::ProviderReplacementPending.into());
            }
            None => return Err(NativePlatformError::ProviderReplacementUnavailable.into()),
        }
        let transition = self.engine.abort_backend_ingress_provider_replacement()?;
        debug_assert!(transition.reduced_inputs().is_empty());
        debug_assert!(transition.platform_effects().is_empty());
        self.native_handoff = None;
        Ok(HostFrameReport::from_transition(
            &transition,
            Vec::new(),
            None,
            self.abandoned_native_effects.clone(),
        ))
    }

    fn start_drained_native_handoff(&mut self) -> Result<HostFrameReport, DockspaceRuntimeError> {
        let Some(handoff) = self.native_handoff.take() else {
            return Err(NativePlatformError::ProviderReplacementUnavailable.into());
        };
        let RuntimeNativeHandoff::Drained { mode, mut receipt } = handoff else {
            self.native_handoff = Some(handoff);
            return Err(NativePlatformError::ProviderReplacementPending.into());
        };
        match self
            .engine
            .begin_backend_ingress_provider_replacement(&mut receipt)
        {
            Ok(start) => {
                let (ticket, transition) = start.into_parts();
                debug_assert!(transition.reduced_inputs().is_empty());
                debug_assert!(transition.platform_effects().is_empty());
                self.native_handoff = Some(RuntimeNativeHandoff::Replacing { mode, ticket });
                Ok(HostFrameReport::from_transition(
                    &transition,
                    Vec::new(),
                    None,
                    self.abandoned_native_effects.clone(),
                ))
            }
            Err(error) => {
                self.native_handoff = Some(RuntimeNativeHandoff::Drained { mode, receipt });
                Err(error.into())
            }
        }
    }

    /// Returns the current exact native binding for one logical surface.
    #[must_use]
    pub fn native_surface(&self, surface: SurfaceId) -> Option<NativeSurfaceLease> {
        self.native.as_ref()?.bindings.get(&surface).copied()
    }

    /// Captures one retryable exact-set native platform snapshot.
    ///
    /// Every currently registered native surface must appear exactly once.
    /// The returned value carries no core authority until it is recorded through
    /// [`Self::publish_native_snapshot`].
    ///
    /// # Errors
    ///
    /// Returns an error for an unavailable provider, stale or duplicate leases,
    /// an incomplete roster, or generation exhaustion.
    pub fn capture_native_snapshot(
        &self,
        observations: impl IntoIterator<Item = (NativeSurfaceLease, NativeWindowFacts)>,
    ) -> Result<NativePlatformSnapshot, NativePlatformError> {
        self.native_state()?
            .capture_snapshot(self.version().epoch(), observations)
    }

    /// Captures an explicit Unknown native-window inventory tombstone.
    ///
    /// This revokes retained inventory authority without guessing that any
    /// surface was destroyed. The capture remains bound to the current provider,
    /// workspace epoch, and logical binding roster until it is durably recorded
    /// through [`Self::publish_native_snapshot`].
    ///
    /// # Errors
    ///
    /// Returns an error when native authority is unavailable or the provider
    /// generation cannot advance.
    pub fn capture_native_inventory_unknown(
        &self,
    ) -> Result<NativePlatformSnapshot, NativePlatformError> {
        self.native_state()?
            .capture_unknown_inventory_snapshot(self.version().epoch())
    }

    /// Records one complete native platform snapshot in the joined backend order.
    ///
    /// Recording is durable within the facade-owned producer: dropping or
    /// rejecting the next host frame does not lose the physical fact. The exact
    /// immutable record is replayed until a host-frame commit advances the core
    /// watermark.
    ///
    /// # Errors
    ///
    /// Returns an error for a superseded provider, stale workspace or binding
    /// roster, or a non-contiguous provider generation.
    pub fn publish_native_snapshot(
        &mut self,
        snapshot: NativePlatformSnapshot,
    ) -> Result<(), DockspaceRuntimeError> {
        let current_epoch = self.version().epoch();
        let native = self.native_state_mut()?;
        if native.provider() != snapshot.provider {
            return Err(NativePlatformError::ProviderSuperseded.into());
        }
        if snapshot.expected_epoch != current_epoch {
            return Err(NativePlatformError::StaleWorkspace.into());
        }
        let expected_generation = native.next_snapshot_generation()?;
        if expected_generation != snapshot.generation {
            return Err(NativePlatformError::SnapshotGenerationStale.into());
        }
        if snapshot.bindings.len() != native.bindings.len()
            || snapshot.bindings.iter().any(|binding| {
                native
                    .bindings
                    .get(&binding.surface())
                    .is_none_or(|lease| lease.binding != *binding)
            })
        {
            return Err(NativePlatformError::SnapshotRosterStale.into());
        }

        let generation = snapshot.generation;
        let bindings = snapshot.bindings.clone();
        native.record_snapshot(snapshot)?;
        native.snapshot_generation = generation;
        for binding in bindings {
            native.close_generations.insert(binding, generation);
        }
        Ok(())
    }

    /// Records one exact live-binding native close observation in backend order.
    ///
    /// Stale callback capabilities are rejected before they can be redirected
    /// to a replacement binding which reused the same host token.
    ///
    /// # Errors
    ///
    /// Returns a typed stale-surface, acknowledgement, or generation error.
    pub fn publish_native_close(
        &mut self,
        lease: NativeSurfaceLease,
        state: NativeCloseState,
        acknowledgement: Option<NativeCloseEffectAcknowledgement>,
    ) -> Result<(), DockspaceRuntimeError> {
        let expected_epoch = self.version().epoch();
        let native = self.native_state_mut()?;
        if native.bindings.get(&lease.surface()) != Some(&lease) {
            return Err(NativePlatformError::StaleSurface {
                surface: lease.surface(),
            }
            .into());
        }
        let generation = native
            .close_generations
            .get(&lease.binding)
            .copied()
            .unwrap_or(0)
            .checked_add(1)
            .ok_or(NativePlatformError::GenerationExhausted)?;
        let state = match state {
            NativeCloseState::Clear => WindowCloseState::LiveClear,
            NativeCloseState::Requested => WindowCloseState::LiveRequested,
        };
        let acknowledgement = match acknowledgement {
            Some(acknowledgement) if acknowledgement.provider != native.provider() => {
                return Err(NativePlatformError::EffectAcknowledgementProviderMismatch {
                    surface: lease.surface(),
                }
                .into());
            }
            Some(acknowledgement) if acknowledgement.binding != lease.binding => {
                return Err(NativePlatformError::EffectAcknowledgementBindingMismatch {
                    surface: lease.surface(),
                }
                .into());
            }
            Some(acknowledgement) => {
                CloseEffectAcknowledgement::known(Some(acknowledgement.effect))
            }
            None => CloseEffectAcknowledgement::known(None),
        };
        native.record_close(
            expected_epoch,
            WindowCloseObservation::new(
                lease.binding,
                CloseObservationGeneration::new(generation),
                Authority::Known(state),
                acknowledgement,
            ),
        )?;
        native.close_generations.insert(lease.binding, generation);
        Ok(())
    }

    /// Records one negative or indeterminate result for an emitted native effect.
    ///
    /// # Errors
    ///
    /// Returns a recoverable error containing the original affine result when
    /// its provider or binding was superseded, or the producer rejects it.
    pub fn report_native_effect_result(
        &mut self,
        result: NativeEffectResult,
    ) -> Result<(), NativeEffectSubmissionError> {
        if self.native_handoff.is_some() {
            return Err(NativeEffectSubmissionError::new(
                NativePlatformError::ProviderReplacementPending,
                result,
            ));
        }
        let Some(native) = self.native.as_mut() else {
            return Err(NativeEffectSubmissionError::new(
                NativePlatformError::ProviderUnavailable,
                result,
            ));
        };
        if native.provider() != result.provider {
            return Err(NativeEffectSubmissionError::new(
                NativePlatformError::ProviderSuperseded,
                result,
            ));
        }
        let surface = result.binding.surface();
        if native
            .bindings
            .get(&surface)
            .is_none_or(|lease| lease.binding != result.binding)
        {
            return Err(NativeEffectSubmissionError::new(
                NativePlatformError::StaleSurface { surface },
                result,
            ));
        }
        native
            .record_effect_result(result.result)
            .map_err(|error| NativeEffectSubmissionError::new(error, result))
    }

    /// Records permanent host quiescence for one retired exact native binding.
    ///
    /// Call this only after every route, callback, renderer sidecar, and queued
    /// platform fact capable of naming the binding has been retired. The record
    /// lets core compact binding-scoped effect, presentation, and tombstone
    /// history without inferring quiescence from callback absence.
    ///
    /// # Errors
    ///
    /// Returns an error when the lease belongs to another provider, remains
    /// live, was not retired by this provider, or the joined recorder rejects
    /// the permanent boundary.
    pub fn report_native_binding_quiescence(
        &mut self,
        lease: NativeSurfaceLease,
    ) -> Result<(), DockspaceRuntimeError> {
        self.native_state_mut()?
            .record_binding_quiescence(lease)
            .map_err(Into::into)
    }

    /// Records registration of one externally owned native root window.
    ///
    /// # Errors
    ///
    /// Returns an error when native authority is unavailable or the backend
    /// producer rejects the semantic record.
    pub fn register_native_root(
        &mut self,
        surface: SurfaceId,
        token: HostWindowToken,
    ) -> Result<(), DockspaceRuntimeError> {
        let expected = self.version();
        self.native_state_mut()?
            .recorder_mut()
            .record_viewport_registration(
                expected,
                surface,
                token.into_core(),
                ViewportRole::Root,
                None,
            )
            .map_err(|_| NativePlatformError::ProtocolInvariant)?;
        Ok(())
    }

    /// Records one explicit core-owned close-plan request for an exact native edge.
    ///
    /// # Errors
    ///
    /// Returns an error when the edge belongs to a superseded provider or the
    /// backend producer rejects the semantic record.
    pub fn request_native_surface_close(
        &mut self,
        close: NativeSurfaceCloseRequest,
        request: crate::SurfaceCloseRequest,
    ) -> Result<(), DockspaceRuntimeError> {
        let expected = self.version();
        let native = self.native_state_mut()?;
        validate_close_request(native, close)?;
        native
            .recorder_mut()
            .record_semantic_input(EngineInput::RequestSurfaceClose {
                expected,
                edge: close.edge,
                request,
            })
            .map_err(|_| NativePlatformError::ProtocolInvariant)?;
        Ok(())
    }

    /// Records exact platform cancellation for one unresolved native close edge.
    ///
    /// # Errors
    ///
    /// Returns an error when the edge belongs to a superseded provider or the
    /// backend producer rejects the semantic record.
    pub fn cancel_native_surface_close(
        &mut self,
        close: NativeSurfaceCloseRequest,
    ) -> Result<(), DockspaceRuntimeError> {
        let expected = self.version();
        let native = self.native_state_mut()?;
        validate_close_request(native, close)?;
        native
            .recorder_mut()
            .record_semantic_input(EngineInput::CancelSurfaceClose {
                expected,
                edge: close.edge,
            })
            .map_err(|_| NativePlatformError::ProtocolInvariant)?;
        Ok(())
    }
}

fn validate_close_request(
    native: &RuntimeNativeState,
    close: NativeSurfaceCloseRequest,
) -> Result<(), NativePlatformError> {
    if native.provider() != close.provider {
        return Err(NativePlatformError::ProviderSuperseded);
    }
    let binding = close.edge.binding();
    if native
        .bindings
        .get(&binding.surface())
        .is_none_or(|lease| lease.binding != binding)
    {
        return Err(NativePlatformError::StaleSurface {
            surface: binding.surface(),
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::effect::EffectId;
    use crate::graph::{Node, RootRecord, SurfacePresentation, Workspace};
    use crate::ids::{ItemId, RootId};
    use crate::policy::DockPolicy;
    use crate::scene_manifest::MeasurementUnavailableReason;

    const SURFACE: SurfaceId = SurfaceId::new(1);
    const ROOT: RootId = RootId::new(1);
    const ITEM: ItemId = ItemId::new(1);
    const WINDOW: HostWindowToken = HostWindowToken::new(41);

    fn native_root_session() -> (DockspaceSession, NativeSurfaceLease) {
        let mut builder = Workspace::builder();
        let tabs = builder.insert_node(Node::tabs([ITEM]));
        builder.set_root(ROOT, RootRecord::new(tabs));
        builder.set_surface(SURFACE, SurfacePresentation::with_main(ROOT));
        let mut session = DockspaceSession::new(
            builder
                .build()
                .expect("the native test workspace validates"),
            DockPolicy::default(),
        )
        .expect("the native test session initializes");
        session
            .enable_native_platform(NativePlatformMode::ObservedRoots)
            .expect("the test provider enrolls");
        session
            .register_native_root(SURFACE, WINDOW)
            .expect("the root registration records");
        let mut frame = session
            .begin_host_frame()
            .expect("the registration frame begins");
        frame
            .complete_unpainted_surfaces(MeasurementUnavailableReason::Deferred)
            .expect("the registration frame settles every surface");
        let report = frame.commit().expect("the registration frame commits");
        let lease = match report.inputs() {
            [super::super::HostInputOutcome::NativeSurfaceRegistered { lease }] => *lease,
            outcomes => panic!("expected one native registration, got {outcomes:?}"),
        };
        (session, lease)
    }

    #[test]
    fn live_window_facts_leave_independent_authority_unknown() {
        let (_session, lease) = native_root_session();
        let compiled =
            compile_window_fact(lease.provider, lease.binding, NativeWindowFacts::live(), 1)
                .expect("inventory-only facts compile");
        let window = compiled
            .window
            .expect("a live binding remains in inventory");
        let coordinates = window
            .coordinate_observation()
            .expect("the provider advances an explicit coordinate tombstone");
        let input = window
            .input_observation()
            .expect("the provider advances an explicit input tombstone");
        let presentation = window
            .presentation_observation()
            .expect("the provider advances an explicit presentation tombstone");

        assert!(compiled.is_live);
        assert!(!coordinates.has_authority());
        assert!(matches!(
            coordinates.content_bounds(),
            Authority::Unknown(AuthorityUnavailableReason::NotReported)
        ));
        assert!(matches!(
            coordinates.outer_bounds(),
            Authority::Unknown(AuthorityUnavailableReason::NotReported)
        ));
        assert!(matches!(
            coordinates.native_scale_factor(),
            Authority::Unknown(AuthorityUnavailableReason::NotReported)
        ));
        assert!(matches!(
            coordinates.presentation_scale_factor(),
            Authority::Unknown(AuthorityUnavailableReason::NotReported)
        ));
        assert!(matches!(
            window.input_state(),
            Authority::Unknown(AuthorityUnavailableReason::NotReported)
        ));
        assert!(matches!(
            input.state(),
            Authority::Unknown(AuthorityUnavailableReason::NotReported)
        ));
        assert!(matches!(
            presentation.state(),
            Authority::Unknown(AuthorityUnavailableReason::NotReported)
        ));
        assert!(matches!(
            window.close_requested(),
            Authority::Unknown(AuthorityUnavailableReason::NotReported)
        ));
        assert!(matches!(
            compiled.close.state(),
            Authority::Unknown(AuthorityUnavailableReason::NotReported)
        ));
    }

    #[test]
    fn unknown_inventory_capture_does_not_infer_destruction() {
        let (session, lease) = native_root_session();

        let snapshot = session
            .capture_native_inventory_unknown()
            .expect("the active provider captures an inventory tombstone");

        assert_eq!(snapshot.provider, lease.provider);
        assert_eq!(snapshot.bindings, vec![lease.binding]);
        assert!(matches!(
            snapshot.snapshot.inventory_observation().roster(),
            Authority::Unknown(AuthorityUnavailableReason::NotReported)
        ));
        assert!(snapshot.snapshot.window_observations().is_empty());
        assert!(snapshot.snapshot.close_observations().is_empty());
    }

    #[test]
    fn live_binding_cannot_be_declared_quiescent() {
        let (mut session, lease) = native_root_session();

        assert!(matches!(
            session.report_native_binding_quiescence(lease),
            Err(DockspaceRuntimeError::Native(
                NativePlatformError::BindingStillLive { surface: SURFACE }
            ))
        ));
    }

    #[test]
    fn joined_provider_replacement_rotates_surface_capabilities() {
        let (mut session, predecessor) = native_root_session();

        session
            .begin_native_provider_replacement()
            .expect("the predecessor drains into one joined reservation");
        assert_eq!(session.native_surface(SURFACE), None);
        assert!(matches!(
            session.capture_native_snapshot([(predecessor, NativeWindowFacts::live())]),
            Err(NativePlatformError::ProviderReplacementPending)
        ));
        assert!(matches!(
            session.register_native_root(SURFACE, WINDOW),
            Err(DockspaceRuntimeError::Native(
                NativePlatformError::ProviderReplacementPending
            ))
        ));

        session
            .finish_native_provider_replacement()
            .expect("the joined successor activates");
        let successor = session
            .native_surface(SURFACE)
            .expect("the successor rebuilds the exact binding roster");
        assert_ne!(successor, predecessor);
        assert_eq!(successor.surface(), predecessor.surface());
        assert_eq!(successor.window_token(), predecessor.window_token());
        assert!(matches!(
            session.capture_native_snapshot([(predecessor, NativeWindowFacts::live())]),
            Err(NativePlatformError::StaleSurface { surface: SURFACE })
        ));
        session
            .capture_native_snapshot([(successor, NativeWindowFacts::live())])
            .expect("the successor capability captures the current roster");
    }

    #[test]
    fn replacement_abort_leaves_the_session_unenrolled_until_explicit_reenable() {
        let (mut session, predecessor) = native_root_session();
        session
            .begin_native_provider_replacement()
            .expect("the predecessor drains into one joined reservation");
        session
            .abort_native_provider_replacement()
            .expect("the reserved successor aborts");

        assert_eq!(session.native_surface(SURFACE), None);
        assert!(matches!(
            session.capture_native_snapshot(std::iter::empty()),
            Err(NativePlatformError::ProviderUnavailable)
        ));

        session
            .enable_native_platform(NativePlatformMode::ObservedRoots)
            .expect("the host explicitly enrolls a fresh provider");
        let successor = session
            .native_surface(SURFACE)
            .expect("reenrollment reconstructs the current binding roster");
        assert_ne!(successor, predecessor);
    }

    #[test]
    fn predecessor_effect_acknowledgement_cannot_authorize_the_successor() {
        let (mut session, predecessor) = native_root_session();
        let stale_acknowledgement = NativeCloseEffectAcknowledgement {
            provider: predecessor.provider,
            binding: predecessor.binding,
            effect: EffectId::new(1),
        };
        session
            .begin_native_provider_replacement()
            .expect("the predecessor drains into one joined reservation");
        session
            .finish_native_provider_replacement()
            .expect("the joined successor activates");
        let successor = session
            .native_surface(SURFACE)
            .expect("the successor rebuilds the exact binding roster");

        assert!(matches!(
            session.publish_native_close(
                successor,
                NativeCloseState::Clear,
                Some(stale_acknowledgement),
            ),
            Err(DockspaceRuntimeError::Native(
                NativePlatformError::EffectAcknowledgementProviderMismatch { surface: SURFACE }
            ))
        ));
    }

    #[test]
    fn destroyed_fact_can_acknowledge_the_exact_destructive_effect() {
        let (_session, lease) = native_root_session();
        let effect = EffectId::new(7);
        let acknowledgement = NativeCloseEffectAcknowledgement {
            provider: lease.provider,
            binding: lease.binding,
            effect,
        };
        let compiled = compile_window_fact(
            lease.provider,
            lease.binding,
            NativeWindowFacts::destroyed_after(acknowledgement),
            1,
        )
        .expect("an exact destructive acknowledgement compiles");

        assert!(!compiled.is_live);
        assert_eq!(compiled.window, None);
        assert_eq!(
            compiled.close.known_state(),
            Some(WindowCloseState::Destroyed)
        );
        assert!(compiled.close.acknowledges(effect));
    }
}
