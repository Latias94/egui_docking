//! Bounded application-owned product action ingress.

use dockspace::model::WorkspaceVersion;
use dockspace::runtime::{HostInputOutcome, PreparedDockAction};
use egui_dockspace::DockspaceActionStatus;

/// Failure to queue one application-owned product action.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NativeActionRequestError {
    /// Another application action is waiting for the next final root pass.
    Busy,
    /// The native runtime has entered terminal shutdown.
    Stopped,
}

impl NativeActionRequestError {
    pub(crate) const fn busy() -> Self {
        Self::Busy
    }

    pub(crate) const fn stopped() -> Self {
        Self::Stopped
    }
}

impl std::fmt::Display for NativeActionRequestError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let message = match self {
            Self::Busy => "a native dockspace application action is already pending",
            Self::Stopped => "the native dockspace runtime is stopped",
        };
        formatter.write_str(message)
    }
}

impl std::error::Error for NativeActionRequestError {}

/// Single-slot queue consumed only by the final root pass.
#[derive(Debug, Default)]
pub(crate) struct NativeApplicationActions {
    pending: Option<PreparedDockAction>,
    awaiting: Option<WorkspaceVersion>,
    result: Option<DockspaceActionStatus>,
}

impl NativeApplicationActions {
    pub(crate) fn enqueue(
        &mut self,
        action: PreparedDockAction,
    ) -> Result<(), NativeActionRequestError> {
        if self.is_occupied() {
            return Err(NativeActionRequestError::busy());
        }
        self.pending = Some(action);
        Ok(())
    }

    pub(crate) const fn is_occupied(&self) -> bool {
        self.pending.is_some() || self.awaiting.is_some() || self.result.is_some()
    }

    pub(crate) const fn has_queued(&self) -> bool {
        self.pending.is_some()
    }

    pub(crate) fn take(&mut self) -> Option<PreparedDockAction> {
        let action = self.pending.take()?;
        self.awaiting = Some(action.expected_version());
        Some(action)
    }

    pub(crate) fn settle(&mut self, inputs: &[HostInputOutcome]) -> Result<bool, ()> {
        let Some(expected) = self.awaiting else {
            return Ok(false);
        };
        let status = inputs.iter().find_map(|input| match input {
            HostInputOutcome::ProductActionApplied(outcome) => {
                Some(DockspaceActionStatus::Applied(outcome.clone()))
            }
            HostInputOutcome::ProductActionRejected(reason) => {
                Some(DockspaceActionStatus::Rejected(*reason))
            }
            HostInputOutcome::StaleRejected {
                expected: stale,
                accepted,
            } if *stale == expected => Some(DockspaceActionStatus::Stale {
                expected: *stale,
                accepted: *accepted,
            }),
            _ => None,
        });
        let Some(status) = status else {
            return Err(());
        };
        self.awaiting = None;
        self.result = Some(status);
        Ok(true)
    }

    pub(crate) fn take_result(&mut self) -> Option<DockspaceActionStatus> {
        self.result.take()
    }

    pub(crate) fn clear(&mut self) {
        self.pending = None;
        self.awaiting = None;
        self.result = None;
    }
}
