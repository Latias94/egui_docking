//! Stable error details for the renderer-neutral product facade.

use dockspace::presentation_config::DockPresentationConfigError;
#[cfg(feature = "serde")]
use dockspace::runtime::DockspacePersistenceError;
use dockspace::runtime::DockspaceRuntimeError;
use thiserror::Error;

use crate::error::{DockspaceError, DockspaceErrorKind};
use crate::style::DockStyleError;

/// Exact product-facade diagnostics kept behind [`DockspaceError`].
#[derive(Debug, Error)]
pub(crate) enum DockspaceErrorSource {
    /// Style validation failed before a session was created.
    #[error("dock style is invalid: {0}")]
    Style(#[from] DockStyleError),
    /// Style geometry could not be represented by the core configuration.
    #[error("dock presentation configuration is invalid: {0}")]
    PresentationConfig(#[from] DockPresentationConfigError),
    /// The renderer-neutral session rejected a host-frame operation.
    #[error("dockspace runtime failed: {0}")]
    Runtime(#[from] DockspaceRuntimeError),
    /// The session-owned document boundary rejected persistence input or output.
    #[cfg(feature = "serde")]
    #[error("dockspace persistence failed: {0}")]
    Persistence(#[from] DockspacePersistenceError),
    /// A product action did not yield its required terminal outcome.
    #[error("dockspace {operation} did not produce its required application outcome")]
    ApplicationOutcomeUnavailable { operation: &'static str },
    /// The adapter could not represent one egui measurement as a core fact.
    #[error("egui measurement for {what} is invalid")]
    InvalidMeasurement { what: &'static str },
    /// The convenience API was used with a surface outside the session roster.
    #[error("surface {surface} is outside the dockspace roster")]
    SurfaceOutsideRoster {
        surface: dockspace::model::SurfaceId,
    },
    /// The convenience API requires exactly one logical surface.
    #[error("show_single_surface requires exactly one surface, found {count}")]
    SingleSurfaceRequiresOne { count: usize },
}

impl DockspaceErrorSource {
    pub(crate) const fn kind(&self) -> DockspaceErrorKind {
        match self {
            Self::Style(_) | Self::PresentationConfig(_) | Self::InvalidMeasurement { .. } => {
                DockspaceErrorKind::InvalidConfiguration
            }
            Self::SingleSurfaceRequiresOne { .. } => DockspaceErrorKind::OperationConflict,
            Self::SurfaceOutsideRoster { .. } => DockspaceErrorKind::HostProtocol,
            #[cfg(feature = "serde")]
            Self::Persistence(_) => DockspaceErrorKind::Persistence,
            Self::Runtime(error) => match error.kind() {
                dockspace::runtime::DockspaceRuntimeErrorKind::ActionAuthority => {
                    DockspaceErrorKind::OperationConflict
                }
                dockspace::runtime::DockspaceRuntimeErrorKind::Interaction
                | dockspace::runtime::DockspaceRuntimeErrorKind::Native
                | dockspace::runtime::DockspaceRuntimeErrorKind::HostFrame
                | dockspace::runtime::DockspaceRuntimeErrorKind::PresentationObservation
                | dockspace::runtime::DockspaceRuntimeErrorKind::SurfaceContributionBegin
                | dockspace::runtime::DockspaceRuntimeErrorKind::SurfaceContributionPrepare => {
                    DockspaceErrorKind::HostProtocol
                }
                dockspace::runtime::DockspaceRuntimeErrorKind::PaintObligationUnavailable
                | dockspace::runtime::DockspaceRuntimeErrorKind::SourceSequenceExhausted
                | dockspace::runtime::DockspaceRuntimeErrorKind::Engine => {
                    DockspaceErrorKind::Internal
                }
                #[cfg(feature = "serde")]
                dockspace::runtime::DockspaceRuntimeErrorKind::Persistence => {
                    DockspaceErrorKind::Persistence
                }
                _ => DockspaceErrorKind::Internal,
            },
            Self::ApplicationOutcomeUnavailable { .. } => DockspaceErrorKind::Internal,
        }
    }
}

impl From<DockspaceRuntimeError> for DockspaceError {
    fn from(error: DockspaceRuntimeError) -> Self {
        DockspaceError::from_source(DockspaceErrorSource::Runtime(error))
    }
}
