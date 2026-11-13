//! Typed failures for native style validation and command admission.

use crate::NativeActionRequestError;

/// Failure to validate or queue a native renderer style.
#[non_exhaustive]
#[derive(Debug, thiserror::Error)]
pub enum NativeStyleRequestError {
    /// The existing application command slot is busy or stopped.
    #[error(transparent)]
    Queue(#[from] NativeActionRequestError),
    /// Adapter geometry or visual settings are invalid.
    #[error(transparent)]
    Style(#[from] egui_dockspace::DockStyleError),
}
