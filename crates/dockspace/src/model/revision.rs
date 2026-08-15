use crate::ids::{WorkspaceEpoch, WorkspaceRevision};

/// Version of all workspace and policy state used to derive product actions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct WorkspaceVersion {
    epoch: WorkspaceEpoch,
    revision: WorkspaceRevision,
}

impl WorkspaceVersion {
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
