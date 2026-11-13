//! Stable error details for the renderer-neutral product facade.

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
    /// The renderer-neutral session rejected a host-frame operation.
    #[error("dockspace runtime failed: {0}")]
    Runtime(#[from] DockspaceRuntimeError),
    /// An exact renderer report no longer belongs to this output lane.
    #[error("dockspace presentation report failed: {0}")]
    PresentationReport(#[from] dockspace::runtime::SurfacePresentationReportError),
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
    /// The same Dockspace pass was entered through an ambiguous or foreign UI scope.
    #[error("dockspace show conflicts with another renderer pass contribution")]
    PassSettlementConflict,
    /// The owned run did not consume input because earlier output fills its budget.
    #[error(
        "dockspace output is backpressured; settle earlier renderer receipts before retrying input"
    )]
    OutputBackpressure { input: Box<egui::RawInput> },
}

impl DockspaceErrorSource {
    pub(crate) const fn kind(&self) -> DockspaceErrorKind {
        match self {
            Self::Style(_) | Self::InvalidMeasurement { .. } => {
                DockspaceErrorKind::InvalidConfiguration
            }
            Self::SingleSurfaceRequiresOne { .. } | Self::PassSettlementConflict => {
                DockspaceErrorKind::OperationConflict
            }
            Self::OutputBackpressure { .. } => DockspaceErrorKind::OutputBackpressure,
            Self::SurfaceOutsideRoster { .. } | Self::PresentationReport(_) => {
                DockspaceErrorKind::HostProtocol
            }
            #[cfg(feature = "serde")]
            Self::Persistence(_) => DockspaceErrorKind::Persistence,
            Self::Runtime(error) => match error.kind() {
                dockspace::runtime::DockspaceRuntimeErrorKind::InvalidConfiguration => {
                    DockspaceErrorKind::InvalidConfiguration
                }
                dockspace::runtime::DockspaceRuntimeErrorKind::Persistence => {
                    DockspaceErrorKind::Persistence
                }
                dockspace::runtime::DockspaceRuntimeErrorKind::Unsupported => {
                    DockspaceErrorKind::Unsupported
                }
                dockspace::runtime::DockspaceRuntimeErrorKind::OperationConflict
                | dockspace::runtime::DockspaceRuntimeErrorKind::UpdateInProgress => {
                    DockspaceErrorKind::OperationConflict
                }
                dockspace::runtime::DockspaceRuntimeErrorKind::HostProtocol
                | dockspace::runtime::DockspaceRuntimeErrorKind::ForeignUpdate => {
                    DockspaceErrorKind::HostProtocol
                }
                dockspace::runtime::DockspaceRuntimeErrorKind::Internal => {
                    DockspaceErrorKind::Internal
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
