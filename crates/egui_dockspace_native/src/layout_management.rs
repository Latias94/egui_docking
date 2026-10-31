//! Typed failures for native layout preparation and command admission.

use std::{error::Error, fmt};

use dockspace::model::SurfaceId;
#[cfg(feature = "serde")]
use dockspace::runtime::DockspacePersistenceError;
use dockspace::runtime::DockspaceRuntimeError;

use crate::{NativeActionRequestError, NativeRuntimeErrorKind};

/// Failure to prepare or queue a native layout reset or document restore.
///
/// The original queue, runtime, or persistence error remains available through
/// [`Error::source`] without being reduced to a message.
#[derive(Debug)]
pub struct NativeLayoutRequestError {
    source: NativeLayoutRequestErrorSource,
}

#[derive(Debug)]
enum NativeLayoutRequestErrorSource {
    Queue(NativeActionRequestError),
    Layout(Box<DockspaceRuntimeError>),
    #[cfg(feature = "serde")]
    Persistence(Box<DockspacePersistenceError>),
    #[cfg(feature = "serde")]
    ArrangementLocked,
    HostSurfaceMissing {
        surface: SurfaceId,
    },
    UnhostedSurface {
        surface: SurfaceId,
    },
}

impl NativeLayoutRequestError {
    /// Returns the stable native runtime failure category.
    #[must_use]
    pub fn kind(&self) -> NativeRuntimeErrorKind {
        match &self.source {
            NativeLayoutRequestErrorSource::Queue(_) => NativeRuntimeErrorKind::OperationConflict,
            #[cfg(feature = "serde")]
            NativeLayoutRequestErrorSource::ArrangementLocked => {
                NativeRuntimeErrorKind::OperationConflict
            }
            NativeLayoutRequestErrorSource::Layout(source) => source.kind(),
            #[cfg(feature = "serde")]
            NativeLayoutRequestErrorSource::Persistence(_) => NativeRuntimeErrorKind::Persistence,
            NativeLayoutRequestErrorSource::HostSurfaceMissing { .. } => {
                NativeRuntimeErrorKind::InvalidConfiguration
            }
            NativeLayoutRequestErrorSource::UnhostedSurface { .. } => {
                NativeRuntimeErrorKind::Unsupported
            }
        }
    }

    #[cfg(feature = "serde")]
    pub(crate) const fn new_arrangement_locked() -> Self {
        Self {
            source: NativeLayoutRequestErrorSource::ArrangementLocked,
        }
    }

    pub(crate) const fn new_host_surface_missing(surface: SurfaceId) -> Self {
        Self {
            source: NativeLayoutRequestErrorSource::HostSurfaceMissing { surface },
        }
    }

    pub(crate) const fn new_unhosted_surface(surface: SurfaceId) -> Self {
        Self {
            source: NativeLayoutRequestErrorSource::UnhostedSurface { surface },
        }
    }
}

impl fmt::Display for NativeLayoutRequestError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.source {
            NativeLayoutRequestErrorSource::Queue(source) => {
                write!(
                    formatter,
                    "the native layout request could not be queued: {source}"
                )
            }
            NativeLayoutRequestErrorSource::Layout(source) => {
                write!(
                    formatter,
                    "the native layout could not be prepared: {source}"
                )
            }
            #[cfg(feature = "serde")]
            NativeLayoutRequestErrorSource::Persistence(source) => {
                write!(
                    formatter,
                    "the native layout document could not be prepared: {source}"
                )
            }
            #[cfg(feature = "serde")]
            NativeLayoutRequestErrorSource::ArrangementLocked => formatter.write_str(
                "the native layout document cannot be restored while arrangement editing is locked",
            ),
            NativeLayoutRequestErrorSource::HostSurfaceMissing { surface } => {
                write!(
                    formatter,
                    "the native layout does not contain host surface {surface}"
                )
            }
            NativeLayoutRequestErrorSource::UnhostedSurface { surface } => write!(
                formatter,
                "the replacement surface {surface} requires explicit native window recreation"
            ),
        }
    }
}

impl Error for NativeLayoutRequestError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match &self.source {
            NativeLayoutRequestErrorSource::Queue(source) => Some(source),
            NativeLayoutRequestErrorSource::Layout(source) => Some(source.as_ref()),
            #[cfg(feature = "serde")]
            NativeLayoutRequestErrorSource::Persistence(source) => Some(source.as_ref()),
            #[cfg(feature = "serde")]
            NativeLayoutRequestErrorSource::ArrangementLocked => None,
            NativeLayoutRequestErrorSource::HostSurfaceMissing { .. }
            | NativeLayoutRequestErrorSource::UnhostedSurface { .. } => None,
        }
    }
}

impl From<NativeActionRequestError> for NativeLayoutRequestError {
    fn from(source: NativeActionRequestError) -> Self {
        Self {
            source: NativeLayoutRequestErrorSource::Queue(source),
        }
    }
}

impl From<DockspaceRuntimeError> for NativeLayoutRequestError {
    fn from(source: DockspaceRuntimeError) -> Self {
        Self {
            source: NativeLayoutRequestErrorSource::Layout(Box::new(source)),
        }
    }
}

#[cfg(feature = "serde")]
impl From<DockspacePersistenceError> for NativeLayoutRequestError {
    fn from(source: DockspacePersistenceError) -> Self {
        Self {
            source: NativeLayoutRequestErrorSource::Persistence(Box::new(source)),
        }
    }
}
