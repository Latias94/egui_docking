use crate::ids::{WorkspaceEpoch, WorkspaceRevision};

/// Version of all workspace and policy state used to derive product actions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct WorkspaceVersion {
    epoch: WorkspaceEpoch,
    revision: WorkspaceRevision,
}

impl WorkspaceVersion {
    /// Compares replacement epochs for versions from the same session.
    ///
    /// Ordinary mutations preserve this relationship; layout/document
    /// replacement breaks it even when stable item identities are reused.
    /// Versions alone cannot identify a session. This comparison grants no
    /// source, receiver or action authority across sessions.
    #[must_use]
    pub const fn is_same_epoch(self, other: Self) -> bool {
        self.epoch.get() == other.epoch.get()
    }

    /// Returns the initial version of a newly constructed workspace.
    #[must_use]
    pub(crate) const fn initial() -> Self {
        Self::new(WorkspaceEpoch::new(0), WorkspaceRevision::new(0))
    }

    /// Creates a version from distinct replacement and mutation counters.
    #[must_use]
    pub(crate) const fn new(epoch: WorkspaceEpoch, revision: WorkspaceRevision) -> Self {
        Self { epoch, revision }
    }

    /// Returns the replacement epoch.
    #[must_use]
    pub(crate) const fn epoch(self) -> WorkspaceEpoch {
        self.epoch
    }

    /// Returns the mutation revision within the current epoch.
    #[must_use]
    pub(crate) const fn revision(self) -> WorkspaceRevision {
        self.revision
    }
}
