//! Public failures for the native coordinator boundary.

use dockspace::model::SurfaceId;
use dockspace::runtime::{
    DockspaceRuntimeError, NativeHostErrorKind, NativeStagingPresentationReportError,
    PaintedNativeStagingOutput, PaintedSurfaceOutput, SurfacePresentationReportError,
};
use eframe::{NativeOutputToken, egui::ViewportId};
use egui_dockspace::DockspaceError;
use thiserror::Error;
use winit::window::WindowId;

/// Stable category for one native coordinator failure.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NativeRuntimeErrorKind {
    /// The renderer-neutral session rejected an exact fact or host frame.
    Dockspace,
    /// A renderer settlement did not belong to the pending presentation stream.
    Presentation,
    /// The host attempted to cross an uncommitted callback-order boundary.
    HostProtocol,
    /// The egui renderer could not represent a core paint or measurement fact.
    Adapter,
}

/// Failure while reducing one native coordinator cycle.
#[derive(Debug)]
pub struct NativeRuntimeError {
    source: NativeRuntimeErrorSource,
}

#[derive(Debug)]
enum NativeRuntimeErrorSource {
    Dockspace(Box<DockspaceRuntimeError>),
    Presentation(Box<SurfacePresentationReportError>),
    StagingPresentation(Box<NativeStagingPresentationReportError>),
    HostProtocol(NativeHostProtocolError),
    Adapter(Box<DockspaceError>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub(crate) enum NativeHostProtocolError {
    #[error("the native app update is not running inside an eframe output callback")]
    OutputTokenUnavailable,
    #[error("native root surface {0} is absent from the product session")]
    RootSurfaceUnavailable(SurfaceId),
    #[error("core rejected native registration of root surface {0}")]
    RootRegistrationRejected(SurfaceId),
    #[error("the root eframe viewport could not bind the registered native surface")]
    RootViewportBindingFailed,
    #[error("a native callback record must be acknowledged before beginning the next host frame")]
    CallbackRecordPending,
    #[error("the native window event acknowledgement does not match the journal head")]
    WindowEventAcknowledgementMismatch,
    #[error("the global native-focus acknowledgement does not match the journal head")]
    GlobalFocusAcknowledgementMismatch,
    #[error("the viewport focus-command acknowledgement does not match the journal head")]
    ViewportFocusAcknowledgementMismatch,
    #[error("the viewport pointer pass-through acknowledgement does not match the journal head")]
    ViewportPointerPassthroughAcknowledgementMismatch,
    #[error("the viewport creation failure acknowledgement does not match the journal head")]
    ViewportCreateFailureAcknowledgementMismatch,
    #[error("the viewport visibility acknowledgement does not match the journal head")]
    ViewportVisibilityAcknowledgementMismatch,
    #[error("the root viewport roster acknowledgement does not match the journal head")]
    ViewportRosterAcknowledgementMismatch,
    #[error("a viewport creation failure has no matching retained native effect")]
    ViewportCreateFailureWithoutEffect,
    #[error("dockspace rejected a retryable native effect result: {0:?}")]
    NativeEffectResultRejected(NativeHostErrorKind),
    #[error("a terminal native output is waiting for its affine painted output")]
    OutputAwaitingAttachment,
    #[error("the final egui pass changed its native output or logical surface")]
    MultipassOutputChanged,
    #[error("discarded egui passes produced conflicting local dockspace actions")]
    MultipassLocalActionConflict,
    #[error("a native surface frame emitted {actual} painted outputs; expected {expected}")]
    PaintedOutputCountMismatch { expected: usize, actual: usize },
    #[error("a native staging frame emitted {actual} painted outputs; expected {expected}")]
    PaintedStagingOutputCountMismatch { expected: usize, actual: usize },
    #[error("the final egui pass could not bind its affine painted output: {0:?}")]
    OutputBindingFailed(NativeOutputBindingErrorKind),
    #[error("native surface {0} did not paint every transient visual required by its core plan")]
    IncompleteTransientPaint(SurfaceId),
    #[error("native output settlements did not preserve one contiguous context-local sequence")]
    OutputOrderViolation,
    #[error("a native window snapshot contains invalid physical geometry or scale")]
    InvalidWindowSnapshot,
    #[error("the native work-area roster contains invalid or duplicate facts")]
    InvalidWorkAreaRoster,
    #[error("the native work-area identity stream is exhausted")]
    WorkAreaIdentityExhausted,
    #[error("a native presentation acknowledgement has no exact presentation state")]
    PresentationAcknowledgementWithoutState,
    #[error("a native viewport effect already has an unobserved presentation acknowledgement")]
    PresentationAcknowledgementAlreadyPending,
    #[error("a deferred viewport effect returned the wrong acknowledgement category")]
    UnexpectedViewportEffectAcknowledgement,
    #[error("a native cleanup observation conflicts with its retained destructive result")]
    CleanupRelayConflict,
    #[error("the first native output callback could not attach its exact viewport route")]
    OutputRouteAttachmentFailed,
    #[error("the admitted native surface {0} no longer owns its exact viewport route")]
    NativeAdmissionRouteChanged(SurfaceId),
    #[error(
        "the retired native viewport route for surface {0} changed before its destruction frame committed"
    )]
    RetiredViewportRouteChanged(SurfaceId),
    #[error("native close callback correlation changed before the causal boundary committed")]
    NativeCloseCorrelationChanged,
    #[error("native focus callback correlation changed before the causal boundary committed")]
    NativeFocusCorrelationChanged,
    #[error(
        "native pointer-input callback correlation changed before the causal boundary committed"
    )]
    NativeInputCorrelationChanged,
    #[error("native pointer pass-through dispatch changed before it was registered")]
    NativeInputDispatchChanged,
    #[error("an exact native close cancellation was not accepted by the core")]
    NativeCloseCancellationRejected,
    #[error("a queued application action committed without one exact product outcome")]
    ApplicationActionOutcomeMissing,
}

impl NativeRuntimeError {
    /// Returns the stable product-level failure category.
    #[must_use]
    pub const fn kind(&self) -> NativeRuntimeErrorKind {
        match self.source {
            NativeRuntimeErrorSource::Dockspace(_) => NativeRuntimeErrorKind::Dockspace,
            NativeRuntimeErrorSource::Presentation(_)
            | NativeRuntimeErrorSource::StagingPresentation(_) => {
                NativeRuntimeErrorKind::Presentation
            }
            NativeRuntimeErrorSource::HostProtocol(_) => NativeRuntimeErrorKind::HostProtocol,
            NativeRuntimeErrorSource::Adapter(_) => NativeRuntimeErrorKind::Adapter,
        }
    }

    /// Recovers the affine painted output from a presentation failure.
    ///
    /// # Errors
    ///
    /// Returns the unchanged error when it did not originate from renderer
    /// presentation settlement.
    pub fn into_painted_output(self) -> Result<PaintedSurfaceOutput, Self> {
        match self.source {
            NativeRuntimeErrorSource::Presentation(source) => Ok(source.into_output()),
            source => Err(Self { source }),
        }
    }

    /// Recovers the affine staging output from a presentation failure.
    ///
    /// # Errors
    ///
    /// Returns the unchanged error when it did not originate from native
    /// staging presentation settlement.
    pub fn into_painted_staging_output(self) -> Result<PaintedNativeStagingOutput, Self> {
        match self.source {
            NativeRuntimeErrorSource::StagingPresentation(source) => Ok(source.into_output()),
            source => Err(Self { source }),
        }
    }
}

impl std::fmt::Display for NativeRuntimeError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let message = match self.kind() {
            NativeRuntimeErrorKind::Dockspace => {
                "dockspace rejected the native coordinator operation"
            }
            NativeRuntimeErrorKind::Presentation => {
                "dockspace rejected a renderer presentation result"
            }
            NativeRuntimeErrorKind::HostProtocol => {
                "native callback records are not ready for the next host frame"
            }
            NativeRuntimeErrorKind::Adapter => {
                "egui could not represent the core-owned native surface"
            }
        };
        formatter.write_str(message)
    }
}

impl std::error::Error for NativeRuntimeError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(match &self.source {
            NativeRuntimeErrorSource::Dockspace(source) => source.as_ref(),
            NativeRuntimeErrorSource::Presentation(source) => source.as_ref(),
            NativeRuntimeErrorSource::StagingPresentation(source) => source.as_ref(),
            NativeRuntimeErrorSource::HostProtocol(source) => source,
            NativeRuntimeErrorSource::Adapter(source) => source.as_ref(),
        })
    }
}

impl From<DockspaceRuntimeError> for NativeRuntimeError {
    fn from(source: DockspaceRuntimeError) -> Self {
        Self {
            source: NativeRuntimeErrorSource::Dockspace(Box::new(source)),
        }
    }
}

impl From<SurfacePresentationReportError> for NativeRuntimeError {
    fn from(source: SurfacePresentationReportError) -> Self {
        Self {
            source: NativeRuntimeErrorSource::Presentation(Box::new(source)),
        }
    }
}

impl From<NativeStagingPresentationReportError> for NativeRuntimeError {
    fn from(source: NativeStagingPresentationReportError) -> Self {
        Self {
            source: NativeRuntimeErrorSource::StagingPresentation(Box::new(source)),
        }
    }
}

impl From<NativeHostProtocolError> for NativeRuntimeError {
    fn from(source: NativeHostProtocolError) -> Self {
        Self {
            source: NativeRuntimeErrorSource::HostProtocol(source),
        }
    }
}

impl From<DockspaceError> for NativeRuntimeError {
    fn from(source: DockspaceError) -> Self {
        Self {
            source: NativeRuntimeErrorSource::Adapter(Box::new(source)),
        }
    }
}

/// Why a viewport could not be associated with one logical native surface.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum NativeViewportBindingError {
    /// The binding is foreign, retired, or no longer current in this session.
    #[error("native viewport {viewport:?} names a non-current binding for surface {surface}")]
    BindingNotCurrent {
        /// Eframe viewport being associated.
        viewport: ViewportId,
        /// Logical surface named by the rejected binding.
        surface: SurfaceId,
    },
    /// The viewport has no current exact binding.
    #[error("native viewport {viewport:?} is not bound")]
    ViewportUnbound {
        /// Missing eframe viewport.
        viewport: ViewportId,
    },
    /// The viewport is already assigned to another surface.
    #[error("native viewport {viewport:?} is already assigned to surface {existing}")]
    ViewportAlreadyBound {
        /// Reused eframe viewport.
        viewport: ViewportId,
        /// Surface currently assigned to the viewport.
        existing: SurfaceId,
    },
    /// The viewport already has a different native window attached.
    #[error("native viewport {viewport:?} is already attached to window {existing:?}")]
    ViewportWindowAlreadyAttached {
        /// Eframe viewport whose reserved binding already has a window.
        viewport: ViewportId,
        /// Existing native window.
        existing: WindowId,
    },
    /// The surface is already assigned to another viewport.
    #[error("native surface {surface} is already assigned to viewport {existing:?}")]
    SurfaceAlreadyBound {
        /// Reused logical surface.
        surface: SurfaceId,
        /// Viewport currently assigned to the surface.
        existing: ViewportId,
    },
    /// The native window is already assigned to another viewport.
    #[error("native window {window:?} is already assigned to viewport {existing:?}")]
    WindowAlreadyBound {
        /// Reused native window.
        window: WindowId,
        /// Viewport currently assigned to the window.
        existing: ViewportId,
    },
    /// A stale predecessor attempted to replace or remove a newer binding.
    #[error(
        "native viewport {viewport:?} binding changed (expected surface {expected}, current surface {current})"
    )]
    BindingMismatch {
        /// Viewport named by the stale operation.
        viewport: ViewportId,
        /// Logical surface named by the stale exact binding.
        expected: SurfaceId,
        /// Logical surface currently assigned to the viewport.
        current: SurfaceId,
    },
    /// Replacement attempted to change the logical surface implicitly.
    #[error("native viewport {viewport:?} replacement changed surface {expected} to {successor}")]
    ReplacementSurfaceMismatch {
        /// Viewport being replaced.
        viewport: ViewportId,
        /// Surface owned by the predecessor binding.
        expected: SurfaceId,
        /// Surface named by the successor binding.
        successor: SurfaceId,
    },
}

/// Stable reason why an affine painted output could not be bound to an eframe token.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NativeOutputBindingErrorKind {
    /// The token was not reserved during its viewport callback.
    TokenNotReserved,
    /// The viewport/window pair had no exact current native binding when paint completed.
    RouteUnavailable,
    /// The output was emitted for another exact native binding.
    BindingMismatch,
    /// Paint-time receiver bindings did not belong to this exact output pass.
    ReceiverMismatch,
    /// The token already owns another pending painted output.
    OutputAlreadyBound,
}

/// Failed token-to-output binding which retains the affine output for recovery.
#[derive(Debug)]
pub struct NativeOutputBindingError {
    kind: NativeOutputBindingErrorKind,
    token: NativeOutputToken,
    output: Box<PaintedSurfaceOutput>,
}

impl NativeOutputBindingError {
    pub(crate) fn new(
        kind: NativeOutputBindingErrorKind,
        token: NativeOutputToken,
        output: PaintedSurfaceOutput,
    ) -> Self {
        Self {
            kind,
            token,
            output: Box::new(output),
        }
    }

    /// Returns the stable failure category.
    #[must_use]
    pub const fn kind(&self) -> NativeOutputBindingErrorKind {
        self.kind
    }

    /// Returns the rejected eframe output token.
    #[must_use]
    pub const fn token(&self) -> NativeOutputToken {
        self.token
    }

    /// Recovers the unconsumed affine output.
    pub fn into_output(self) -> PaintedSurfaceOutput {
        *self.output
    }
}

impl std::fmt::Display for NativeOutputBindingError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "cannot bind native output token {:?}: {:?}",
            self.token, self.kind
        )
    }
}

impl std::error::Error for NativeOutputBindingError {}
