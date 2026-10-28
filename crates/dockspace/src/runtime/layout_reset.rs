//! Application-owned default layouts without document or serialization authority.

use std::fmt;

use crate::engine::{EngineInput, HostFrameView, rebase_workspace_presentation_identities};
use crate::graph::Workspace;
use crate::ids::EngineAuthorityDomainId;
use crate::model::{DockspaceLayout, WorkspaceVersion};

use super::session::HostFrameReplacement;
use super::{
    DockspaceHostFrame, DockspaceRuntimeError, DockspaceSession, NativeReceiverAnswer,
    NativeReceiverQuery,
};

/// Opaque session-bound layout reset, borrowed by one rollbackable host frame.
///
/// The application retains this non-cloneable value after a failed or dropped
/// attempt. A successful replacement changes the workspace version and makes
/// this prepared value stale. Pane identities remain application-owned.
#[must_use = "a prepared reset must be submitted or deliberately discarded"]
pub struct PreparedDockspaceLayoutReset {
    workspace: Workspace,
    authority: EngineAuthorityDomainId,
    version: WorkspaceVersion,
}

impl fmt::Debug for PreparedDockspaceLayoutReset {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PreparedDockspaceLayoutReset")
            .finish_non_exhaustive()
    }
}

impl DockspaceSession {
    /// Prepares an application layout against this exact published session.
    ///
    /// The application owns its immutable default and named presets. This
    /// operation neither requires nor replaces a persistence document binding.
    ///
    /// # Errors
    ///
    /// Returns an error when arrangement editing is locked, or when a bound
    /// document does not already recognize every numeric item in the layout.
    pub fn prepare_layout_reset(
        &self,
        layout: DockspaceLayout,
    ) -> Result<PreparedDockspaceLayoutReset, DockspaceRuntimeError> {
        if self.policy().arrangement_locked() {
            return Err(DockspaceRuntimeError::arrangement_locked());
        }
        let workspace = layout.into_workspace();
        self.ensure_layout_reset_items_known(&workspace)?;
        Ok(PreparedDockspaceLayoutReset {
            workspace,
            authority: self.engine.authority_domain(),
            version: self.version(),
        })
    }

    /// Begins an affine frame whose final semantic mutation resets the layout.
    ///
    /// Retired presentation identities are rebased in the current candidate.
    /// Measurement may follow; its contributions install Ready scenes at commit,
    /// which a later host frame can paint. Later semantic and configuration
    /// input is rejected. Dropping the frame preserves the published state and
    /// leaves `reset` available for an exact retry.
    ///
    /// # Errors
    ///
    /// Returns an error for a stale or foreign prepared value, a locked policy,
    /// unknown document items, a managed host requiring ordered native input,
    /// or a replacement which cannot be prepared.
    pub fn begin_layout_reset_frame(
        &mut self,
        reset: &PreparedDockspaceLayoutReset,
    ) -> Result<DockspaceHostFrame<'_>, DockspaceRuntimeError> {
        self.ensure_standalone_replacement_available()?;
        self.ensure_prepared_layout_reset_current(reset)?;
        self.begin_host_frame_with_replacement(None, Some(HostFrameReplacement::LayoutReset(reset)))
    }

    /// Replays ordered native input before resetting the candidate layout.
    ///
    /// The reset shares the native prefix's rollback and publication boundary.
    /// No old native binding or presentation output gains authority in the new
    /// workspace epoch.
    ///
    /// # Errors
    ///
    /// Returns an error for an unavailable managed host, stale preparation,
    /// locked policy, unknown document items, unavailable receiver facts, or a
    /// replacement which cannot be prepared. Failed attempts retain `reset`.
    pub fn begin_native_layout_reset_frame(
        &mut self,
        reset: &PreparedDockspaceLayoutReset,
        mut resolve: impl FnMut(NativeReceiverQuery) -> NativeReceiverAnswer,
    ) -> Result<DockspaceHostFrame<'_>, DockspaceRuntimeError> {
        self.ensure_managed_native_host_available()?;
        self.ensure_prepared_layout_reset_current(reset)?;
        self.begin_host_frame_with_replacement(
            Some(&mut resolve),
            Some(HostFrameReplacement::LayoutReset(reset)),
        )
    }

    fn ensure_prepared_layout_reset_current(
        &self,
        reset: &PreparedDockspaceLayoutReset,
    ) -> Result<(), DockspaceRuntimeError> {
        if self.policy().arrangement_locked() {
            return Err(DockspaceRuntimeError::arrangement_locked());
        }
        if reset.authority != self.engine.authority_domain() || reset.version != self.version() {
            return Err(DockspaceRuntimeError::prepared_layout_reset_mismatch());
        }
        self.ensure_layout_reset_items_known(&reset.workspace)
    }

    fn ensure_layout_reset_items_known(
        &self,
        workspace: &Workspace,
    ) -> Result<(), DockspaceRuntimeError> {
        #[cfg(feature = "serde")]
        if let Some(document) = self.document.as_ref()
            && let Some(item) = workspace
                .item_multiset()
                .into_keys()
                .find(|item| document.external_item_key(*item).is_none())
        {
            return Err(DockspaceRuntimeError::layout_reset_item_unknown(item));
        }
        #[cfg(not(feature = "serde"))]
        let _ = workspace;
        Ok(())
    }
}

impl PreparedDockspaceLayoutReset {
    pub(super) fn replacement_input(
        &self,
        frame: HostFrameView<'_>,
    ) -> Result<EngineInput, DockspaceRuntimeError> {
        if self.authority != frame.authority_domain() {
            return Err(DockspaceRuntimeError::prepared_layout_reset_mismatch());
        }
        let rebased = rebase_workspace_presentation_identities(
            self.workspace.clone(),
            frame.workspace(),
            frame.presentation_identity_frontier(),
            None,
        )
        .map_err(DockspaceRuntimeError::layout_reset_rebase)?;
        Ok(EngineInput::ReplaceWorkspace(rebased.workspace))
    }
}
