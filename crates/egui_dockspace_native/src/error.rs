//! Public failures for the native coordinator boundary.

use dockspace::model::SurfaceId;
use dockspace::runtime::{
    DockspaceRuntimeError, PaintedSurfaceOutput, SurfacePresentationReportError,
};
use egui_dockspace::DockspaceError;
use eframe::{NativeOutputToken, egui::ViewportId};
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
    HostProtocol(NativeHostProtocolError),
    Adapter(Box<DockspaceError>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub(crate) enum NativeHostProtocolError {
    #[error("a native window event must be acknowledged before beginning the next host frame")]
    WindowEventPending,
    #[error("the native window event acknowledgement does not match the journal head")]
    WindowEventAcknowledgementMismatch,
    #[error("a terminal native output is waiting for its affine painted output")]
    OutputAwaitingAttachment,
}

impl NativeRuntimeError {
    /// Returns the stable product-level failure category.
    #[must_use]
    pub const fn kind(&self) -> NativeRuntimeErrorKind {
        match self.source {
            NativeRuntimeErrorSource::Dockspace(_) => NativeRuntimeErrorKind::Dockspace,
            NativeRuntimeErrorSource::Presentation(_) => NativeRuntimeErrorKind::Presentation,
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
    /// The viewport had no exact current native binding when paint completed.
    ViewportUnbound,
    /// The output was emitted for another exact native binding.
    BindingMismatch,
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
