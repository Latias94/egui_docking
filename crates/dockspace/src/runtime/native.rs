//! Opaque native-window lifecycle facts and bindings for renderer-neutral hosts.

mod budget;
mod compiler;
mod input_batch;
mod local_pointer;
mod pointer;
mod receiver;
mod session;
mod storage;
#[cfg(test)]
mod tests;

use thiserror::Error;

use super::native_effect::{
    NativeCloseEffectAcknowledgement, NativeCreateEffectAcknowledgement, NativeEffectResult,
    NativeEffectSubmissionError, NativeInputEffectAcknowledgement,
    NativePresentationEffectAcknowledgement,
};
use super::{DockspaceRuntimeError, DockspaceSession};
use crate::backend_ingress::{
    BackendIngressError, BackendIngressOrdinal, BackendIngressPrefixRetirementReceipt,
    BackendIngressRecorder,
};
use crate::engine::EngineInput;
use crate::geometry::{PhysicalRect, PhysicalSize, ScaleFactor};
use crate::ids::{SurfaceId, WorkspaceEpoch};
use crate::platform::{
    CloseEffectAcknowledgement, ObservedWindow, WindowCloseObservation, WindowCloseState,
    WindowInputState, WindowPresentationState,
};
pub use crate::platform::{NativeCreateReadiness, NativeCreateStrategy};
use crate::platform_provider::PlatformObservationLease;
use crate::viewport::{
    CloseObservationGeneration, ViewportBinding, ViewportRole, WindowToken, WorkAreaGeneration,
    WorkAreaToken,
};
use crate::viewport_registry::ViewportAdmission;
pub use budget::{NativeInputBudget, NativeMonotonicClock};
use compiler::{
    NativeSnapshotEntry, compile_focus_observation, compile_platform_snapshot,
    compile_unknown_inventory_snapshot, snapshot_descendant_capacity_bytes,
};
pub(super) use input_batch::PendingNativeReplay;
pub use local_pointer::{
    NativeLocalCoordinateError, NativeLocalCoordinates, NativeLocalGeometry,
    NativeLocalPointerFact, NativeLocalPointerLocation, NativeLocalPointerSample,
};
pub use pointer::{
    NativeDesktopPointerLocation, NativeDesktopPosition, NativePointerButton,
    NativePointerCancelReason, NativePointerEvent, NativePointerHover, NativePointerId,
    NativePointerInput, NativePointerLocation, NativePointerOwner, NativePointerRoster,
    NativePointerState, NativeProjectedScrollDelta, NativeReceiverAnswer, NativeReceiverPurpose,
    NativeReceiverQuery, NativeScrollCancelReason, NativeScrollDelta, NativeScrollDeviceId,
    NativeScrollEvent, NativeScrollModifiers, NativeScrollMomentum, NativeScrollPhase,
    NativeScrollReceiverChallenge, NativeScrollSequenceId,
};
pub use storage::{NativeInputStorageLimit, NativeProducerStorageLimit, NativeReportStorageLimit};

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

/// Globally consistent native focus supplied by a managed host.
///
/// `Unknown` revokes prior focus authority. It is not equivalent to proving
/// that no native window is focused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NativeGlobalFocus {
    /// One exact current docking window owns native focus.
    Dock(NativeSurfaceBinding),
    /// A non-docking application or system window owns native focus.
    Foreign,
    /// The platform authoritatively reports that no native window is focused.
    None,
    /// The globally focused native window cannot currently be proven.
    Unknown,
}

/// One native-host operation or observation which a managed adapter can support.
///
/// Capabilities describe the attached backend contract, not the availability
/// of one particular event-time fact. A supported capability may still report
/// an explicit `Unknown` fact at a causal boundary where the backend could not
/// prove a value.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum NativeHostCapability {
    /// Runtime-owned native windows can complete the managed lifecycle.
    NativeWindowLifecycle,
    /// The host publishes one complete live native-window roster.
    AuthoritativeInventory,
    /// Pointer events can identify the native window under the pointer.
    HoveredWindow,
    /// Pointer events can carry an event-time desktop position.
    DesktopPointerPosition,
    /// The host owns an exact button-state checkpoint and ordered updates.
    AuthoritativeButtonState,
    /// Native windows can be created or moved at an explicit desktop rectangle.
    GlobalWindowPlacement,
    /// The host can publish exact usable desktop work areas.
    WorkArea,
    /// Final rendered docking receivers can be observed exactly.
    PointerHitTestObservation,
    /// Native pointer pass-through can be controlled and settled.
    PointerHitTestControl,
    /// Native focus events can identify the focused docking window.
    GlobalFocusObservation,
    /// Native focus can be requested and causally observed.
    WindowActivationControl,
    /// Native close requests can be intercepted and explicitly cancelled.
    CloseCancellation,
}

impl NativeHostCapability {
    const fn bit(self) -> u16 {
        1_u16 << (self as u8)
    }
}

/// Immutable capability roster for one attached managed native backend.
///
/// The roster is configured once after the real windowing backend is known.
/// Omitting a capability means that the backend contract does not support it;
/// transient missing facts must instead be reported through the corresponding
/// `Unknown` observation value.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct NativeHostCapabilities {
    supported: u16,
}

impl NativeHostCapabilities {
    /// Creates an exact roster with no optional managed capability enabled.
    #[must_use]
    pub const fn none_supported() -> Self {
        Self { supported: 0 }
    }

    /// Returns a new roster with one capability enabled.
    #[must_use]
    pub const fn with(mut self, capability: NativeHostCapability) -> Self {
        self.supported |= capability.bit();
        self
    }

    /// Returns whether the attached backend supports one capability.
    #[must_use]
    pub const fn supports(self, capability: NativeHostCapability) -> bool {
        self.supported & capability.bit() != 0
    }
}

impl core::fmt::Debug for NativeHostCapabilities {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter
            .debug_struct("NativeHostCapabilities")
            .field("supported", &self.supported)
            .finish()
    }
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

    /// Reports whether two provider-bound handles name the same native lifetime.
    ///
    /// A provider handoff may mint a new observation lease while retaining the
    /// exact workspace, surface, window token, and incarnation. This comparison
    /// keeps those opaque lifetime details private while allowing a native host
    /// to rendezvous delayed cleanup results with the successor provider.
    #[must_use]
    pub fn same_window_lifetime(self, other: Self) -> bool {
        self.binding == other.binding
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
    edge: crate::close_plan::NativeCloseEdge,
}

impl NativeSurfaceCloseRequest {
    pub(super) const fn from_edge(
        provider: PlatformObservationLease,
        edge: crate::close_plan::NativeCloseEdge,
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

/// Product action used to resolve one exact native surface-close edge.
///
/// Presentation recovery uses the exact core-owned recovery obligation. Hosts
/// never derive a destination from their current window roster.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NativeSurfaceCloseAction {
    /// Recover the complete presentation through its validated recovery anchor.
    RecoverPresentation,
    /// Destroy the native binding while retaining its logical surface roster.
    RetainLayout,
    /// Close every content item owned by the logical surface.
    CloseContent,
}

impl NativeSurfaceCloseAction {
    fn into_request(self) -> crate::close_plan::SurfaceCloseRequest {
        match self {
            Self::RecoverPresentation => {
                crate::close_plan::SurfaceCloseRequest::RecoverPresentation
            }
            Self::RetainLayout => crate::close_plan::SurfaceCloseRequest::RetainLayout,
            Self::CloseContent => crate::close_plan::SurfaceCloseRequest::CloseContent,
        }
    }
}

impl Default for NativeSurfaceCloseAction {
    fn default() -> Self {
        Self::RecoverPresentation
    }
}

/// Stable product reason why a native surface-close action was rejected.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum NativeSurfaceCloseRejection {
    /// The exact close edge is stale or no longer current.
    #[error("the native close edge is no longer current")]
    StaleEdge,
    /// The window is still inside its native create or replacement barrier.
    #[error("the native surface is still staging")]
    Staging,
    /// The logical source surface disappeared before the action was prepared.
    #[error("native surface {surface} is unavailable")]
    SurfaceUnavailable {
        /// Stable logical source surface.
        surface: SurfaceId,
    },
    /// The platform cannot explicitly cancel a rejected close request.
    #[error("the native host cannot cancel this close request")]
    CancellationUnsupported,
    /// Current docking policy rejected the action.
    #[error("current docking policy rejects this native close action")]
    PolicyDenied,
    /// One pane on the source surface is not closeable.
    #[error("pane {item} cannot be closed")]
    PaneCloseDisabled {
        /// Stable application item which rejected closure.
        item: crate::ids::ItemId,
    },
    /// The source surface has no complete closeable content roster.
    #[error("the native surface has no complete closeable content roster")]
    ContentUnavailable,
    /// Another non-terminal close plan already owns the exact binding.
    #[error("another close plan already owns this native surface")]
    OperationConflict,
    /// The requested operation is outside the stable native close facade.
    #[error("the requested native close operation is unsupported")]
    Unsupported,
}

impl From<&crate::transition::SurfaceCloseRequestRejection> for NativeSurfaceCloseRejection {
    fn from(reason: &crate::transition::SurfaceCloseRequestRejection) -> Self {
        use crate::transition::SurfaceCloseRequestRejection;

        match reason {
            SurfaceCloseRequestRejection::EdgeUnavailable { .. } => Self::StaleEdge,
            SurfaceCloseRequestRejection::StagingBinding { .. } => Self::Staging,
            SurfaceCloseRequestRejection::SurfaceUnavailable { surface } => {
                Self::SurfaceUnavailable { surface: *surface }
            }
            SurfaceCloseRequestRejection::CancellationUnsupported => Self::CancellationUnsupported,
            SurfaceCloseRequestRejection::PolicyRejected(_)
            | SurfaceCloseRequestRejection::RehomePolicyRejected(_) => Self::PolicyDenied,
            SurfaceCloseRequestRejection::RehomeProgramUnavailable => Self::Unsupported,
            SurfaceCloseRequestRejection::PaneCloseDisabled { item } => {
                Self::PaneCloseDisabled { item: *item }
            }
            SurfaceCloseRequestRejection::CloseContentUnavailable => Self::ContentUnavailable,
            SurfaceCloseRequestRejection::ActivePlan { .. } => Self::OperationConflict,
        }
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
    content_size: Option<PhysicalSize>,
    content_bounds: Option<PhysicalRect>,
    outer_bounds: Option<PhysicalRect>,
    native_scale_factor: Option<ScaleFactor>,
    presentation_scale_factor: Option<ScaleFactor>,
    input: Option<NativeInputFact>,
    presentation: Option<NativePresentationFact>,
    create_readiness: Option<NativeCreateReadinessFact>,
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
struct NativeCreateReadinessFact {
    state: NativeCreateReadiness,
    acknowledgement: Option<NativeCreateEffectAcknowledgement>,
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
            content_size: None,
            content_bounds: None,
            outer_bounds: None,
            native_scale_factor: None,
            presentation_scale_factor: None,
            input: None,
            presentation: None,
            create_readiness: None,
            close: None,
        }
    }

    /// Adds exact physical content bounds without inferring outer bounds.
    #[must_use]
    pub const fn with_content_bounds(mut self, bounds: PhysicalRect) -> Self {
        self.content_bounds = Some(bounds);
        self
    }

    /// Adds the client-area size without asserting a desktop origin or outer bounds.
    ///
    /// When content bounds are also reported, their size must match this fact.
    #[must_use]
    pub const fn with_content_size(mut self, size: PhysicalSize) -> Self {
        self.content_size = Some(size);
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

    /// Reports exact creation or renderer preparation using the create request's capability.
    ///
    /// Omission explicitly leaves preparation Unknown in this complete snapshot.
    /// Prepared may be the first observation; Created is not a mandatory pass.
    #[must_use]
    pub const fn with_create_readiness(
        mut self,
        state: NativeCreateReadiness,
        acknowledgement: Option<NativeCreateEffectAcknowledgement>,
    ) -> Self {
        self.create_readiness = Some(NativeCreateReadinessFact {
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
            content_size: None,
            content_bounds: None,
            outer_bounds: None,
            native_scale_factor: None,
            presentation_scale_factor: None,
            input: None,
            presentation: None,
            create_readiness: None,
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
            content_size: None,
            content_bounds: None,
            outer_bounds: None,
            native_scale_factor: None,
            presentation_scale_factor: None,
            input: None,
            presentation: None,
            create_readiness: None,
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
    /// Native input transport cannot currently retain the submitted input.
    InputCapacity,
    /// The enrolled native host cannot perform the requested operation.
    Unsupported,
    /// The host crossed an internal protocol boundary unexpectedly.
    Internal,
}

impl NativeHostErrorKind {
    /// Maps this native-host failure to the stable renderer-neutral category.
    #[must_use]
    pub const fn runtime_error_kind(self) -> super::DockspaceRuntimeErrorKind {
        match self {
            Self::NotEnabled | Self::Unsupported => super::DockspaceRuntimeErrorKind::Unsupported,
            Self::AlreadyEnabled | Self::OperationConflict | Self::InputCapacity => {
                super::DockspaceRuntimeErrorKind::OperationConflict
            }
            Self::StaleBinding | Self::InvalidFacts => {
                super::DockspaceRuntimeErrorKind::HostProtocol
            }
            Self::Internal => super::DockspaceRuntimeErrorKind::Internal,
        }
    }
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
    /// Independently reported client size disagrees with desktop content bounds.
    #[error("native surface {surface} content size differs from its content bounds")]
    ContentSizeBoundsMismatch { surface: SurfaceId },
    /// A preceding roster mutation has not reached a committed host boundary.
    #[error("native binding roster is not yet settled")]
    BindingRosterUnsettled,
    /// Earlier accepted native facts have not yet reached the publication prefix.
    #[error("earlier native input must commit before application input")]
    InputPending,
    /// Native input or future terminal-report storage could not fit its allowance.
    #[error("native input or terminal-report capacity is unavailable")]
    InputCapacity,
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
    /// The attached managed backend changed its immutable capability roster.
    #[error("managed native backend capability roster changed after attachment")]
    CapabilityRosterChanged,
    /// A managed snapshot arrived before the attached backend was identified.
    #[error("managed native backend capabilities have not been configured")]
    CapabilityRosterUnavailable,
    /// Focus cannot become authoritative before the managed capability roster exists.
    #[error("native focus authority requires an initial managed platform snapshot")]
    FocusAuthorityUnavailable,
    /// Standalone replacement cannot bypass an enrolled native causal stream.
    #[error("workspace replacement requires the enrolled native host causal frame")]
    ReplacementRequiresNativeFrame,
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
    pub(super) const fn from_ingress(error: BackendIngressError) -> Self {
        match error {
            BackendIngressError::NativeTransportCapacityExceeded { .. }
            | BackendIngressError::CapacityAllocationFailed => Self::InputCapacity,
            _ => Self::ProtocolInvariant,
        }
    }

    pub(super) const fn kind(&self) -> NativeHostErrorKind {
        match self {
            Self::ProviderUnavailable => NativeHostErrorKind::NotEnabled,
            Self::ProviderAlreadyEnabled => NativeHostErrorKind::AlreadyEnabled,
            Self::InputCapacity => NativeHostErrorKind::InputCapacity,
            Self::ProviderSuperseded | Self::StaleSurface { .. } => {
                NativeHostErrorKind::StaleBinding
            }
            Self::DuplicateSurface { .. }
            | Self::IncompleteRoster
            | Self::ContentSizeBoundsMismatch { .. }
            | Self::InvalidWorkAreaRoster
            | Self::InvalidPointerFacts
            | Self::CapabilityRosterChanged
            | Self::DestroyedSurfaceHasLiveFacts { .. }
            | Self::RetiredSurfaceHasLiveFacts { .. }
            | Self::EffectAcknowledgementBindingMismatch { .. }
            | Self::EffectAcknowledgementProviderMismatch { .. } => {
                NativeHostErrorKind::InvalidFacts
            }
            Self::PointerProviderAlreadyEnabled
            | Self::BindingRosterUnsettled
            | Self::InputPending
            | Self::BindingStillLive { .. }
            | Self::BindingNotRetired { .. }
            | Self::FocusAuthorityUnavailable
            | Self::ReplacementRequiresNativeFrame
            | Self::CapabilityRosterUnavailable => NativeHostErrorKind::OperationConflict,
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
    managed_capabilities: Option<NativeHostCapabilities>,
    recorder: BackendIngressRecorder,
    pending_prefix_retirement: Option<BackendIngressPrefixRetirementReceipt>,
    pending_replay: Option<PendingNativeReplay>,
    snapshot_generation: u64,
    binding_roster_unsettled_through: Option<BackendIngressOrdinal>,
    producer: NativeProducerState,
    producer_storage_limit: Option<NativeProducerStorageLimit>,
    focus_generation: u64,
    focus: NativeGlobalFocus,
}

/// One exact incarnation remains allocated until its quiescence append succeeds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct NativeBindingState {
    binding: ViewportBinding,
    admission: Option<ViewportAdmission>,
    window_generation: u64,
    close_generation: u64,
}

#[derive(Debug, PartialEq, Eq)]
struct NativeProducerState {
    bindings: Vec<NativeBindingState>,
    work_areas: Vec<WorkAreaToken>,
    work_area_generation: WorkAreaGeneration,
}

impl NativeProducerState {
    fn binding(&self, binding: ViewportBinding) -> Option<&NativeBindingState> {
        self.bindings
            .binary_search_by_key(&binding, |state| state.binding)
            .ok()
            .and_then(|index| self.bindings.get(index))
    }

    fn binding_mut(&mut self, binding: ViewportBinding) -> Option<&mut NativeBindingState> {
        self.bindings
            .binary_search_by_key(&binding, |state| state.binding)
            .ok()
            .and_then(|index| self.bindings.get_mut(index))
    }

    fn prepare(
        engine: &crate::engine::DockEngine,
        previous: Option<&Self>,
        limit: Option<NativeProducerStorageLimit>,
    ) -> Result<Self, NativePlatformError> {
        if let Some(limit) = limit {
            limit.validate()?;
        }
        let registry = engine.viewport().registry();
        let mut binding_count = previous.map_or(0, |producer| producer.bindings.len());
        for (_, record) in registry.records() {
            if previous.is_none_or(|producer| producer.binding(record.binding()).is_none()) {
                binding_count = binding_count
                    .checked_add(1)
                    .ok_or(NativePlatformError::InputCapacity)?;
            }
            if limit.is_some_and(|limit| binding_count > limit.binding_slots().get()) {
                return Err(NativePlatformError::InputCapacity);
            }
        }
        let work_area_count = engine.viewport().work_areas().count();
        if limit.is_some_and(|limit| {
            binding_count > limit.binding_slots().get()
                || work_area_count > limit.work_area_slots().get()
        }) {
            return Err(NativePlatformError::InputCapacity);
        }
        let mut bindings = storage::producer_buffer(
            limit.map_or(binding_count, |limit| limit.binding_slots().get()),
        )?;
        let mut work_areas = storage::producer_buffer(
            limit.map_or(work_area_count, |limit| limit.work_area_slots().get()),
        )?;
        if let Some(previous) = previous {
            for state in &previous.bindings {
                let mut next = *state;
                next.admission = registry
                    .record(state.binding.surface())
                    .filter(|record| record.binding() == state.binding)
                    .map(|record| record.admission());
                if next.admission.is_none() {
                    // Retired windows retain close tombstone generations, but
                    // can no longer publish live coordinate/input observations.
                    next.window_generation = 0;
                }
                bindings.push(next);
            }
        }
        for (_, record) in registry.records() {
            if previous.is_none_or(|producer| producer.binding(record.binding()).is_none()) {
                bindings.push(NativeBindingState {
                    binding: record.binding(),
                    admission: Some(record.admission()),
                    window_generation: 0,
                    close_generation: 0,
                });
            }
        }
        bindings.sort_unstable_by_key(|state| state.binding);
        work_areas.extend(engine.viewport().work_areas().map(|(token, _)| token));
        work_areas.sort_unstable();
        Ok(Self {
            bindings,
            work_areas,
            work_area_generation: engine.viewport().work_area_generation(),
        })
    }

    fn owned_capacity_bytes(&self) -> usize {
        self.bindings
            .capacity()
            .saturating_mul(size_of::<NativeBindingState>())
            .saturating_add(
                self.work_areas
                    .capacity()
                    .saturating_mul(size_of::<WorkAreaToken>()),
            )
    }
}

#[derive(Debug)]
struct CompiledNativeWindow {
    is_live: bool,
    window: Option<ObservedWindow>,
    close: WindowCloseObservation,
}

impl RuntimeNativeState {
    fn new(
        recorder: BackendIngressRecorder,
        profile: NativeHostProfile,
        producer: NativeProducerState,
        producer_storage_limit: Option<NativeProducerStorageLimit>,
    ) -> Self {
        Self {
            profile,
            managed_capabilities: None,
            recorder,
            pending_prefix_retirement: None,
            pending_replay: None,
            snapshot_generation: 0,
            binding_roster_unsettled_through: None,
            producer,
            producer_storage_limit,
            focus_generation: 0,
            focus: NativeGlobalFocus::Unknown,
        }
    }

    fn configure_managed_capabilities(
        &mut self,
        capabilities: NativeHostCapabilities,
    ) -> Result<(), NativePlatformError> {
        if self.profile != NativeHostProfile::ManagedDesktop {
            return Err(NativePlatformError::HostProfileMismatch);
        }
        match self.managed_capabilities {
            None => self.managed_capabilities = Some(capabilities),
            Some(current) if current == capabilities => {}
            Some(_) => return Err(NativePlatformError::CapabilityRosterChanged),
        }
        Ok(())
    }

    pub(super) const fn provider(&self) -> PlatformObservationLease {
        self.recorder.lease().platform_provider()
    }

    pub(super) fn binding_allocation_budget(&self) -> Option<usize> {
        self.producer_storage_limit.map(|limit| {
            limit
                .binding_slots()
                .get()
                .saturating_sub(self.producer.bindings.len())
        })
    }

    pub(super) fn contains_binding(&self, binding: NativeSurfaceBinding) -> bool {
        binding.provider == self.provider() && self.binding_is_live(binding)
    }

    pub(super) fn recognizes_binding(&self, binding: NativeSurfaceBinding) -> bool {
        binding.provider == self.provider() && self.producer.binding(binding.binding).is_some()
    }

    pub(super) fn binding_is_live(&self, binding: NativeSurfaceBinding) -> bool {
        self.producer
            .binding(binding.binding)
            .is_some_and(|state| state.admission.is_some())
    }

    fn current_binding(&self, surface: SurfaceId) -> Option<NativeSurfaceBinding> {
        self.producer
            .bindings
            .iter()
            .find(|state| state.binding.surface() == surface && state.admission.is_some())
            .map(|state| NativeSurfaceBinding::from_binding(self.provider(), state.binding))
    }

    fn current_bindings(&self) -> impl Iterator<Item = NativeSurfaceBinding> + '_ {
        self.producer
            .bindings
            .iter()
            .filter(|state| state.admission.is_some())
            .map(|state| NativeSurfaceBinding::from_binding(self.provider(), state.binding))
    }

    fn work_area_binding(&self, token: WorkAreaToken) -> Option<NativeWorkAreaBinding> {
        self.producer
            .work_areas
            .binary_search(&token)
            .ok()
            .map(|_| {
                NativeWorkAreaBinding::new(
                    self.provider(),
                    self.producer.work_area_generation,
                    token,
                )
            })
    }

    pub(super) fn recorder_mut(&mut self) -> &mut BackendIngressRecorder {
        &mut self.recorder
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
            .map_err(NativePlatformError::from_ingress)?;
        Ok(())
    }

    fn record_effect_result(
        &mut self,
        result: crate::effect::EffectResult,
    ) -> Result<(), NativePlatformError> {
        self.recorder
            .record_platform_effect_result(result)
            .map_err(NativePlatformError::from_ingress)?;
        Ok(())
    }

    fn record_binding_quiescence(
        &mut self,
        binding: NativeSurfaceBinding,
    ) -> Result<(), NativePlatformError> {
        if binding.provider != self.provider() {
            return Err(NativePlatformError::ProviderSuperseded);
        }
        if self.binding_is_live(binding) {
            return Err(NativePlatformError::BindingStillLive {
                surface: binding.surface(),
            });
        }
        let index = self
            .producer
            .bindings
            .binary_search_by_key(&binding.binding, |state| state.binding)
            .map_err(|_| NativePlatformError::BindingNotRetired {
                surface: binding.surface(),
            })?;
        self.recorder
            .record_platform_binding_quiescence(binding.binding)
            .map_err(NativePlatformError::from_ingress)?;
        self.producer.bindings.remove(index);
        Ok(())
    }

    pub(super) fn record_abandoned_effects(
        &mut self,
        abandoned: &super::native_effect::NativeEffectDropQueue,
    ) -> Result<(), NativePlatformError> {
        abandoned.record_for(self.provider(), |result| {
            self.record_effect_result(result.result)
        })
    }

    fn next_snapshot_generation(&self) -> Result<u64, NativePlatformError> {
        self.snapshot_generation
            .checked_add(1)
            .ok_or(NativePlatformError::GenerationExhausted)
    }

    fn next_focus_generation(&self) -> Result<u64, NativePlatformError> {
        self.focus_generation
            .checked_add(1)
            .ok_or(NativePlatformError::GenerationExhausted)
    }

    fn current_focus_observation(
        &self,
    ) -> Result<crate::viewport_focus::FocusObservationEnvelope, NativePlatformError> {
        compile_focus_observation(self.provider(), self.focus_generation, self.focus)
    }

    fn focus_observation_for_bindings(
        &self,
        bindings: impl IntoIterator<Item = ViewportBinding>,
    ) -> Result<
        (
            crate::viewport_focus::FocusObservationEnvelope,
            Option<(u64, NativeGlobalFocus)>,
        ),
        NativePlatformError,
    > {
        if let NativeGlobalFocus::Dock(binding) = self.focus
            && !bindings.into_iter().any(|live| live == binding.binding)
        {
            let generation = self.next_focus_generation()?;
            return Ok((
                compile_focus_observation(self.provider(), generation, NativeGlobalFocus::Unknown)?,
                Some((generation, NativeGlobalFocus::Unknown)),
            ));
        }
        Ok((self.current_focus_observation()?, None))
    }

    fn validate_snapshot_roster(
        &self,
        observations: impl IntoIterator<Item = (NativeSurfaceBinding, NativeWindowFacts)>,
    ) -> Result<Vec<NativeSnapshotEntry>, NativePlatformError> {
        if self.binding_roster_unsettled_through.is_some() {
            return Err(NativePlatformError::BindingRosterUnsettled);
        }
        let mut supplied: Vec<NativeSnapshotEntry> = Vec::new();
        for (binding, facts) in observations {
            if binding.provider != self.provider() {
                return Err(NativePlatformError::ProviderSuperseded);
            }
            let state = self.producer.binding(binding.binding).ok_or(
                NativePlatformError::StaleSurface {
                    surface: binding.surface(),
                },
            )?;
            if state.admission.is_none()
                && !matches!(facts.lifecycle, NativeWindowLifecycleFact::Destroyed { .. })
            {
                return Err(NativePlatformError::RetiredSurfaceHasLiveFacts {
                    surface: binding.surface(),
                });
            }
            if supplied
                .iter()
                .any(|entry| entry.binding == binding.binding)
            {
                return Err(NativePlatformError::DuplicateSurface {
                    surface: binding.surface(),
                });
            }
            let close_generation = state
                .close_generation
                .checked_add(1)
                .ok_or(NativePlatformError::GenerationExhausted)?;
            let window_generation = if matches!(facts.lifecycle, NativeWindowLifecycleFact::Live) {
                state
                    .window_generation
                    .checked_add(1)
                    .ok_or(NativePlatformError::GenerationExhausted)?
            } else {
                0
            };
            // Iterator hints never determine allocation or admission. Validate
            // each capability and its slot before growing owned scratch.
            if supplied.len() == supplied.capacity() {
                if let Some(limit) = self.producer_storage_limit {
                    if supplied.len() >= limit.binding_slots().get() {
                        return Err(NativePlatformError::InputCapacity);
                    }
                    supplied = storage::producer_buffer(limit.binding_slots().get())?;
                } else {
                    supplied
                        .try_reserve(1)
                        .map_err(|_| NativePlatformError::InputCapacity)?;
                }
            }
            supplied.push(NativeSnapshotEntry {
                binding: binding.binding,
                facts,
                window_generation,
                close_generation,
            });
        }
        supplied.sort_unstable_by_key(|entry| entry.binding);
        if self.producer.bindings.iter().any(|state| {
            state.admission.is_some()
                && supplied
                    .binary_search_by_key(&state.binding, |entry| entry.binding)
                    .is_err()
        }) {
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
        if let (Some(limit), NativeWorkAreaRoster::Exact(areas)) =
            (self.producer_storage_limit, &work_areas)
            && areas.len() > limit.work_area_slots().get()
        {
            return Err(NativePlatformError::InputCapacity);
        }
        let generation = self.next_snapshot_generation()?;
        let supplied = self.validate_snapshot_roster(observations)?;
        let descendants = snapshot_descendant_capacity_bytes(&supplied, &work_areas)?;
        self.recorder
            .preflight_native_transport_append(1, descendants)
            .map_err(NativePlatformError::from_ingress)?;
        let (focus, focus_update) =
            self.focus_observation_for_bindings(supplied.iter().filter_map(|entry| {
                matches!(entry.facts.lifecycle, NativeWindowLifecycleFact::Live)
                    .then_some(entry.binding)
            }))?;
        let snapshot = compile_platform_snapshot(
            self.profile,
            self.managed_capabilities,
            self.provider(),
            generation,
            focus,
            &supplied,
            &work_areas,
        )?;
        let ordinal = self
            .recorder
            .record_platform_snapshot(expected_epoch, snapshot)
            .map_err(NativePlatformError::from_ingress)?;
        self.binding_roster_unsettled_through = supplied
            .iter()
            .any(|entry| {
                matches!(
                    entry.facts.lifecycle,
                    NativeWindowLifecycleFact::Destroyed { .. }
                )
            })
            .then_some(ordinal);
        self.snapshot_generation = generation;
        // All generation candidates remain private until the final recorder
        // append succeeds. Installing them only updates existing lifetime slots.
        for state in &mut self.producer.bindings {
            if let Ok(index) = supplied.binary_search_by_key(&state.binding, |entry| entry.binding)
                && let Some(entry) = supplied.get(index)
            {
                if matches!(entry.facts.lifecycle, NativeWindowLifecycleFact::Live) {
                    state.window_generation = entry.window_generation;
                }
                state.close_generation = entry.close_generation;
            }
        }
        if let Some((generation, focus)) = focus_update {
            self.focus_generation = generation;
            self.focus = focus;
        }
        Ok(())
    }

    fn record_unknown_inventory(
        &mut self,
        expected_epoch: WorkspaceEpoch,
    ) -> Result<(), NativePlatformError> {
        let generation = self.next_snapshot_generation()?;
        let (focus, focus_update) = self.focus_observation_for_bindings([])?;
        let snapshot = compile_unknown_inventory_snapshot(
            self.profile,
            self.managed_capabilities,
            generation,
            focus,
        )?;
        self.recorder
            .record_platform_snapshot(expected_epoch, snapshot)
            .map_err(NativePlatformError::from_ingress)?;
        self.snapshot_generation = generation;
        if let Some((generation, focus)) = focus_update {
            self.focus_generation = generation;
            self.focus = focus;
        }
        Ok(())
    }

    fn record_global_focus(
        &mut self,
        expected_epoch: WorkspaceEpoch,
        focus: NativeGlobalFocus,
    ) -> Result<(), NativePlatformError> {
        if self.profile != NativeHostProfile::ManagedDesktop {
            return Err(NativePlatformError::HostProfileMismatch);
        }
        if let NativeGlobalFocus::Dock(binding) = focus {
            if binding.provider != self.provider() {
                return Err(NativePlatformError::ProviderSuperseded);
            }
            if !self.binding_is_live(binding) {
                return Err(NativePlatformError::StaleSurface {
                    surface: binding.surface(),
                });
            }
        }
        if self.snapshot_generation == 0 {
            return Err(NativePlatformError::FocusAuthorityUnavailable);
        }
        let generation = self.next_focus_generation()?;
        let observation = compile_focus_observation(self.provider(), generation, focus)?;
        self.recorder
            .record_global_focus_observation(expected_epoch, observation)
            .map_err(NativePlatformError::from_ingress)?;
        self.focus_generation = generation;
        self.focus = focus;
        Ok(())
    }

    fn record_root_registration(
        &mut self,
        expected: crate::model::WorkspaceVersion,
        committed: BackendIngressOrdinal,
        surface: SurfaceId,
        token: WindowToken,
    ) -> Result<(), NativePlatformError> {
        self.preflight_registration(committed, surface)?;
        let ordinal = self
            .recorder
            .record_viewport_registration(expected, surface, token, ViewportRole::Root, None)
            .map_err(NativePlatformError::from_ingress)?;
        self.binding_roster_unsettled_through = Some(ordinal);
        Ok(())
    }

    fn record_child_bootstrap(
        &mut self,
        expected: crate::model::WorkspaceVersion,
        committed: BackendIngressOrdinal,
        surface: SurfaceId,
        token: WindowToken,
        recovery: crate::surface_recovery::SurfaceRecoveryBootstrap,
    ) -> Result<(), NativePlatformError> {
        self.preflight_registration(committed, surface)?;
        let ordinal = self
            .recorder
            .record_child_viewport_bootstrap(expected, surface, token, recovery)
            .map_err(NativePlatformError::from_ingress)?;
        self.binding_roster_unsettled_through = Some(ordinal);
        Ok(())
    }

    fn preflight_registration(
        &self,
        committed: BackendIngressOrdinal,
        surface: SurfaceId,
    ) -> Result<(), NativePlatformError> {
        let Some(limit) = self.producer_storage_limit else {
            return Ok(());
        };
        // A repeated current/queued surface cannot create another incarnation.
        // Preserve its existing semantic rejection instead of reserving a slot.
        if self.current_binding(surface).is_some() {
            return Ok(());
        }
        let records = self.recorder.retained_records();
        let mut required = self.producer.bindings.len();
        let mut already_queued = false;
        for (index, record) in records.iter().enumerate() {
            if record.ordinal() <= committed {
                continue;
            }
            let Some(queued) = registration_surface(record) else {
                continue;
            };
            if queued == surface {
                already_queued = true;
            }
            if self.current_binding(queued).is_some()
                || records[..index].iter().any(|previous| {
                    previous.ordinal() > committed && registration_surface(previous) == Some(queued)
                })
            {
                continue;
            }
            required = required
                .checked_add(1)
                .ok_or(NativePlatformError::InputCapacity)?;
        }
        if !already_queued {
            required = required
                .checked_add(1)
                .ok_or(NativePlatformError::InputCapacity)?;
        }
        if required > limit.binding_slots().get() {
            return Err(NativePlatformError::InputCapacity);
        }
        Ok(())
    }

    pub(super) fn prepare_native_commit(
        &self,
        engine: &crate::engine::DockEngine,
    ) -> Result<PreparedNativeHostCommit, NativePlatformError> {
        let next_producer = NativeProducerState::prepare(
            engine,
            Some(&self.producer),
            self.producer_storage_limit,
        )?;
        let current_count = next_producer
            .bindings
            .iter()
            .filter(|state| state.admission.is_some())
            .count();
        let mut bindings = storage::producer_buffer(current_count)?;
        let mut admissions = storage::producer_buffer(current_count)?;
        for state in &next_producer.bindings {
            if let Some(admission) = state.admission {
                let binding = NativeSurfaceBinding::from_binding(self.provider(), state.binding);
                bindings.push(binding);
                if admission == ViewportAdmission::Admitted
                    && self
                        .producer
                        .binding(state.binding)
                        .is_some_and(|previous| {
                            previous.admission == Some(ViewportAdmission::Pending)
                        })
                {
                    admissions.push(binding);
                }
            }
        }
        // Preserve the public roster and first-live ordering of the old maps.
        bindings.sort_unstable_by_key(|binding| binding.surface());
        admissions.sort_unstable_by_key(|binding| binding.binding);
        let committed = engine.backend_ingress_committed_through();
        let binding_roster_unsettled_through = self
            .binding_roster_unsettled_through
            .filter(|through| *through > committed);
        Ok(PreparedNativeHostCommit {
            next_producer,
            binding_roster_unsettled_through,
            report: NativeHostCommit {
                provider: self.provider(),
                admissions,
                bindings,
                input_pending: self.recorder.recorded_through() > committed,
            },
        })
    }
}

/// The complete producer replacement and report are admitted before core publish.
pub(super) struct PreparedNativeHostCommit {
    next_producer: NativeProducerState,
    binding_roster_unsettled_through: Option<BackendIngressOrdinal>,
    report: NativeHostCommit,
}

impl PreparedNativeHostCommit {
    pub(super) fn commit(self, native: &mut RuntimeNativeState) -> NativeHostCommit {
        native.producer = self.next_producer;
        native.binding_roster_unsettled_through = self.binding_roster_unsettled_through;
        self.report
    }
}

pub(super) struct NativeHostCommit {
    pub(super) provider: PlatformObservationLease,
    pub(super) admissions: Vec<NativeSurfaceBinding>,
    pub(super) bindings: Vec<NativeSurfaceBinding>,
    pub(super) input_pending: bool,
}

fn registration_surface(
    record: &crate::backend_ingress::BackendIngressRecord,
) -> Option<SurfaceId> {
    let crate::backend_ingress::BackendIngressPayload::SemanticInput(input) = record.payload()
    else {
        return None;
    };
    match input.as_ref() {
        EngineInput::RegisterViewport { surface, .. }
        | EngineInput::BootstrapChildViewport { surface, .. } => Some(*surface),
        _ => None,
    }
}
