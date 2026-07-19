//! JSON persistence helpers over the renderer-neutral snapshot schema.

use dockspace::ids::{InputSequence, ItemId};
use dockspace::persistence::{
    SnapshotCaptureError, SnapshotReplacementError, SnapshotRestoreError, WorkspaceSnapshot,
    WorkspaceSnapshotEnvelope,
};
use thiserror::Error;

#[cfg(test)]
use dockspace::transition::WorkspaceVersion;

use crate::facade::Dockspace;

/// Failure to capture, encode, decode, validate, or queue a workspace snapshot.
#[derive(Debug, Error)]
pub enum DockspacePersistenceError {
    /// The live renderer-neutral workspace could not be captured.
    #[error("workspace snapshot capture failed: {0}")]
    Capture(#[from] SnapshotCaptureError),
    /// JSON serialization or syntax decoding failed.
    #[error("workspace snapshot JSON failed: {0}")]
    Json(#[from] serde_json::Error),
    /// The version-first envelope rejected an unsupported snapshot version.
    #[error("workspace snapshot envelope failed: {0}")]
    Envelope(#[from] SnapshotRestoreError),
    /// The complete candidate or engine replacement queue rejected the snapshot.
    #[error("workspace snapshot replacement failed: {0}")]
    Replacement(#[from] SnapshotReplacementError),
}

impl Dockspace {
    /// Captures and encodes the current renderer-neutral workspace as JSON.
    ///
    /// # Errors
    ///
    /// Returns [`DockspacePersistenceError`] when strict capture or JSON encoding fails.
    pub fn save_json(&self) -> Result<String, DockspacePersistenceError> {
        let snapshot = WorkspaceSnapshot::capture(self.engine.workspace())?;
        serde_json::to_string(&snapshot).map_err(Into::into)
    }

    /// Strictly decodes JSON and queues an epoch-advancing workspace replacement.
    ///
    /// `contains_item` is queried for every distinct persisted item before any
    /// live state changes. The replacement is published at the next facade
    /// frame boundary.
    ///
    /// # Errors
    ///
    /// Returns [`DockspacePersistenceError`] for malformed JSON, unsupported
    /// schema, unknown panes, invalid topology, or exhausted engine input IDs.
    pub fn load_json(
        &mut self,
        json: &str,
        contains_item: impl Fn(ItemId) -> bool,
    ) -> Result<InputSequence, DockspacePersistenceError> {
        let envelope: WorkspaceSnapshotEnvelope = serde_json::from_str(json)?;
        let snapshot = envelope.into_snapshot()?;
        self.engine
            .enqueue_snapshot_replacement(&snapshot, contains_item)
            .map_err(Into::into)
    }
}

#[cfg(test)]
mod tests {
    use dockspace::graph::{Node, RootRecord, SurfacePresentation, Workspace};
    use dockspace::ids::{ItemId, RootId, SurfaceId};

    use super::*;
    use crate::Dockspace;

    fn facade() -> Dockspace {
        let item = ItemId::new(11);
        let mut builder = Workspace::builder();
        let tabs = builder.insert_node(Node::tabs([item]));
        builder.set_root(RootId::new(2), RootRecord::new(tabs));
        builder.set_surface(SurfaceId::new(3), SurfacePresentation::new(RootId::new(2)));
        let workspace = builder.build().expect("fixture workspace must be valid");
        Dockspace::builder("persistence-test", workspace)
            .build()
            .expect("fixture facade must build")
    }

    #[test]
    fn json_round_trip_queues_an_atomic_replacement() {
        let mut dockspace = facade();
        let json = dockspace.save_json().expect("snapshot must encode");
        dockspace
            .load_json(&json, |item| item == ItemId::new(11))
            .expect("snapshot must queue");

        assert_eq!(dockspace.engine.pending_inputs().len(), 1);
        assert_eq!(dockspace.engine.version(), WorkspaceVersion::default());
    }

    #[test]
    fn unknown_items_leave_the_engine_queue_unchanged() {
        let mut dockspace = facade();
        let json = dockspace.save_json().expect("snapshot must encode");
        let error = dockspace
            .load_json(&json, |_| false)
            .expect_err("unknown pane must fail");

        assert!(matches!(error, DockspacePersistenceError::Replacement(_)));
        assert!(dockspace.engine.pending_inputs().is_empty());
    }
}
