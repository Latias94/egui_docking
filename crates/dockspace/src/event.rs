//! Commit-only events emitted by the headless engine.

use crate::command::CommandOutcome;
use crate::ids::InputSequence;
use crate::transition::WorkspaceVersion;

/// Event created after an engine candidate has been published.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceEvent {
    input: InputSequence,
    version: WorkspaceVersion,
    kind: WorkspaceEventKind,
}

impl WorkspaceEvent {
    pub(crate) const fn new(
        input: InputSequence,
        version: WorkspaceVersion,
        kind: WorkspaceEventKind,
    ) -> Self {
        Self {
            input,
            version,
            kind,
        }
    }

    /// Returns the input responsible for this event.
    #[must_use]
    pub const fn input(&self) -> InputSequence {
        self.input
    }

    /// Returns the version after applying the event's change.
    #[must_use]
    pub const fn version(&self) -> WorkspaceVersion {
        self.version
    }

    /// Returns the committed change description.
    #[must_use]
    pub const fn kind(&self) -> &WorkspaceEventKind {
        &self.kind
    }
}

/// Durable or policy change which became observable at a commit boundary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkspaceEventKind {
    /// A complete workspace replacement committed.
    WorkspaceReplaced,
    /// One checked workspace command changed state.
    CommandCommitted(CommandOutcome),
    /// Application docking policy changed.
    PolicyReplaced,
}
