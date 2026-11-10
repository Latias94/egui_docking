//! Owned suspension of the sole rollbackable host-frame candidate.

use std::cell::Cell;
use std::collections::BTreeMap;
use std::fmt;
use std::sync::Arc;

#[cfg(feature = "serde")]
use crate::document::PendingRuntimeDocumentRestore;
use crate::engine::CoreHostFrame;
use crate::ids::SurfaceId;
use crate::presentation_observation::{PresentationHostLease, PresentationStreamAdmission};

use super::presentation::SubmittedPresentationObservation;
use super::{
    DockspaceHostFrame, DockspaceRuntimeError, DockspaceSession, SurfaceProgramExecutionReceipt,
};

/// The sole unpublished update, owned independently of its session.
///
/// Store this value across renderer hooks, then use
/// [`DockspaceSession::resume_update`] to borrow its originating session again.
/// Suspension preserves all input, measurement, and encoding state. It does
/// not start a new encoding pass or authorize publishing staged renderer output.
/// Only [`DockspaceHostFrame::commit`] publishes the candidate. Dropping this
/// value rolls it back and allows another update to begin.
#[must_use = "dropping an update rolls back its uncommitted candidate"]
pub struct DockspaceUpdate {
    presentation_host: PresentationHostLease,
    frame: CoreHostFrame,
    next_source_sequence: u64,
    next_pointer_sequence: Option<u64>,
    pointer_input_submitted: bool,
    native_input_pending: bool,
    submitted_presentation: SubmittedPresentationObservation,
    painted_surfaces: BTreeMap<SurfaceId, SurfaceProgramExecutionReceipt>,
    encoding_surfaces: BTreeMap<SurfaceId, Option<PresentationStreamAdmission>>,
    encoding_started: Cell<bool>,
    encoding_checkpoint: Option<crate::engine::CoreEncodingCheckpoint>,
    encoding_pass: u64,
    replacement_submitted: bool,
    #[cfg(feature = "serde")]
    document_restore: Option<PendingRuntimeDocumentRestore>,
    // Release exclusivity only after the candidate and producer guards drop.
    pub(super) update_lease: Arc<()>,
}

impl fmt::Debug for DockspaceUpdate {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("DockspaceUpdate")
            .finish_non_exhaustive()
    }
}

impl DockspaceUpdate {
    pub(super) fn resume(
        self,
        session: &mut DockspaceSession,
    ) -> Result<DockspaceHostFrame<'_>, DockspaceRuntimeError> {
        if self.presentation_host != session.presentation_host
            || self.frame.view().authority_domain() != session.engine.authority_domain()
            || self.frame.pointer_provider() != session.engine.pointer_provider()
            || self.frame.backend_ingress_provider() != session.engine.backend_ingress_provider()
        {
            return Err(DockspaceRuntimeError::foreign_update());
        }
        #[cfg(feature = "serde")]
        if self.document_restore.as_ref().is_some_and(|restore| {
            !restore.is_current_for(&session.engine, session.document.as_ref())
        }) {
            return Err(DockspaceRuntimeError::prepared_document_restore_mismatch());
        }
        let Self {
            update_lease,
            presentation_host: _,
            frame,
            next_source_sequence,
            next_pointer_sequence,
            pointer_input_submitted,
            native_input_pending,
            submitted_presentation,
            painted_surfaces,
            encoding_surfaces,
            encoding_started,
            encoding_checkpoint,
            encoding_pass,
            replacement_submitted,
            #[cfg(feature = "serde")]
            document_restore,
        } = self;
        Ok(DockspaceHostFrame {
            session,
            update_lease: Some(update_lease),
            frame,
            next_source_sequence,
            next_pointer_sequence,
            pointer_input_submitted,
            native_input_pending,
            submitted_presentation,
            painted_surfaces,
            encoding_surfaces,
            encoding_started,
            encoding_checkpoint,
            encoding_pass,
            replacement_submitted,
            #[cfg(feature = "serde")]
            document_restore,
        })
    }
}

impl DockspaceHostFrame<'_> {
    /// Releases the session borrow while preserving this exact unpublished update.
    ///
    /// The update keeps the sole active lease until it is resumed and committed,
    /// or dropped. Suspending neither publishes nor resets an encoding pass.
    #[must_use]
    pub fn suspend(self) -> DockspaceUpdate {
        let Self {
            session,
            update_lease,
            frame,
            next_source_sequence,
            next_pointer_sequence,
            pointer_input_submitted,
            native_input_pending,
            submitted_presentation,
            painted_surfaces,
            encoding_surfaces,
            encoding_started,
            encoding_checkpoint,
            encoding_pass,
            replacement_submitted,
            #[cfg(feature = "serde")]
            document_restore,
        } = self;
        // Borrowed-only frames rely on `&mut` exclusivity. Acquire a lease only
        // when ownership escapes, so an ended legacy borrow cannot block begin.
        let update_lease = update_lease.unwrap_or_else(|| session.lease_update());
        DockspaceUpdate {
            update_lease,
            presentation_host: session.presentation_host,
            frame,
            next_source_sequence,
            next_pointer_sequence,
            pointer_input_submitted,
            native_input_pending,
            submitted_presentation,
            painted_surfaces,
            encoding_surfaces,
            encoding_started,
            encoding_checkpoint,
            encoding_pass,
            replacement_submitted,
            #[cfg(feature = "serde")]
            document_restore,
        }
    }
}
