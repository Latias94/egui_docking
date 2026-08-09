use crate::ids::{WorkspaceEpoch, WorkspaceRevision};

/// Version of all workspace and policy state used to derive product actions.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct WorkspaceVersion {
    epoch: WorkspaceEpoch,
    revision: WorkspaceRevision,
}

impl WorkspaceVersion {
    /// Creates a version from distinct replacement and mutation counters.
    #[must_use]
    pub const fn new(epoch: WorkspaceEpoch, revision: WorkspaceRevision) -> Self {
        Self { epoch, revision }
    }

    /// Returns the replacement epoch.
    #[must_use]
    pub const fn epoch(self) -> WorkspaceEpoch {
        self.epoch
    }

    /// Returns the mutation revision within the current epoch.
    #[must_use]
    pub const fn revision(self) -> WorkspaceRevision {
        self.revision
    }
}
