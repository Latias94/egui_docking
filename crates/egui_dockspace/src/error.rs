//! Stable failures produced by the egui facade.

pub(crate) use crate::error_detail::DockspaceErrorSource;

/// Stable category for one egui docking facade failure.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DockspaceErrorKind {
    /// Style or presentation configuration was invalid.
    InvalidConfiguration,
    /// Document identity or persistence validation failed.
    Persistence,
    /// The requested operation is unsupported by the selected host path.
    Unsupported,
    /// A valid operation conflicts with another in-flight affine operation.
    OperationConflict,
    /// Earlier renderer outputs must settle before this input can enter egui.
    OutputBackpressure,
    /// A custom host supplied stale, incomplete, or contradictory protocol facts.
    HostProtocol,
    /// An engine, renderer, or adapter invariant failed internally.
    Internal,
}

/// Failure to construct or advance an egui docking frame.
///
/// The stable product API exposes a coarse [`DockspaceErrorKind`]. Exact
/// renderer and reducer diagnostics remain in the standard error source chain
/// without making their state-machine variants part of the product ABI.
#[derive(Debug)]
pub struct DockspaceError {
    source: Box<DockspaceErrorSource>,
}

impl DockspaceError {
    /// Returns the stable facade-level category for this failure.
    #[must_use]
    pub const fn kind(&self) -> DockspaceErrorKind {
        self.source.kind()
    }

    /// Recovers input which an owned run deferred before invoking egui or pane UI.
    ///
    /// Only [`DockspaceErrorKind::OutputBackpressure`] carries unconsumed input.
    /// Settle earlier renderer receipts, then retry this exact batch before later
    /// input. Its event order, observation time and viewport metadata are intact.
    /// Other failures return the original error and cannot be safely replayed.
    ///
    /// # Errors
    ///
    /// Returns `self` when this is not an output-backpressure deferral.
    pub fn into_deferred_input(self) -> Result<egui::RawInput, Self> {
        match *self.source {
            DockspaceErrorSource::OutputBackpressure { input } => Ok(*input),
            source => Err(Self::from_source(source)),
        }
    }

    pub(crate) fn from_source(source: DockspaceErrorSource) -> Self {
        Self {
            source: Box::new(source),
        }
    }

    pub(crate) fn from_detail<T>(source: T) -> Self
    where
        DockspaceErrorSource: From<T>,
    {
        Self::from_source(DockspaceErrorSource::from(source))
    }
}

impl std::fmt::Display for DockspaceError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.source.fmt(formatter)
    }
}

impl std::error::Error for DockspaceError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(self.source.as_ref())
    }
}

impl From<DockspaceErrorSource> for DockspaceError {
    fn from(source: DockspaceErrorSource) -> Self {
        Self::from_source(source)
    }
}
