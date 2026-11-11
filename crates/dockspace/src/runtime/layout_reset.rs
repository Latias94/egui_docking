//! Application-owned default layouts without document or serialization authority.

use std::fmt;

use crate::engine::{EngineInput, HostFrameView};
use crate::graph::Workspace;
use crate::ids::EngineAuthorityDomainId;
use crate::model::{DockspaceLayout, DockspaceView, SurfaceId, WorkspaceVersion};

use super::{
    DockspaceHostFrame, DockspaceRuntimeError, DockspaceSession, NativeReceiverAnswer,
    NativeReceiverQuery, NativeSurfaceBinding,
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
    /// Declared presentation identities are preserved in a new workspace epoch.
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
        let mut frame = self.begin_host_frame()?;
        frame.submit_layout_reset(reset)?;
        Ok(frame)
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
        let mut frame = self.begin_native_host_frame(&mut resolve)?;
        frame.submit_layout_reset(reset)?;
        Ok(frame)
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

impl DockspaceHostFrame<'_> {
    /// Replaces the layout and continues one external Root window on an incoming surface.
    ///
    /// `incoming_root` names the exact surface preserved from the prepared layout.
    /// The exact current `existing_root` must be admitted and available for
    /// continuation. Its old logical surface may disappear from the new layout.
    /// The committed report carries the exact predecessor/successor pair; hosts
    /// must prepay and atomically publish their physical-window route with it.
    /// Fresh observations and output are required before successor interaction.
    ///
    /// # Errors
    ///
    /// Returns ordinary reset errors, a missing incoming surface, a stale or
    /// unavailable external Root, or a conflicting target window. Dropping a
    /// failed frame preserves the graph, bindings and prepared reset.
    pub fn submit_native_layout_reset(
        &mut self,
        reset: &PreparedDockspaceLayoutReset,
        existing_root: NativeSurfaceBinding,
        incoming_root: SurfaceId,
    ) -> Result<(), DockspaceRuntimeError> {
        self.submit_native_layout_reset_with_roots(reset, [(existing_root, incoming_root)])
    }

    /// Resets the layout with an atomic set of external Root window continuations.
    ///
    /// Each pair is `(existing_root_binding, incoming_root_surface)`. Sources
    /// and targets must be unique. Participating windows may exchange their
    /// logical surfaces; another window may not occupy a target. Proposed
    /// routes become installable only with the matching committed report.
    ///
    /// # Errors
    ///
    /// Returns reset errors, missing targets, duplicate mappings, stale or
    /// foreign bindings and conflicting target ownership. No part of a failed
    /// or dropped replacement publishes.
    pub fn submit_native_layout_reset_with_roots(
        &mut self,
        reset: &PreparedDockspaceLayoutReset,
        mappings: impl IntoIterator<Item = (NativeSurfaceBinding, SurfaceId)>,
    ) -> Result<(), DockspaceRuntimeError> {
        self.ensure_replacement_input_allowed()?;
        self.session.ensure_managed_native_host_available()?;
        self.session.ensure_prepared_layout_reset_current(reset)?;
        self.complete_pointer_input()?;
        reset.ensure_authority(self.frame.view())?;
        let mappings = mappings.into_iter().collect::<Vec<_>>();
        for &(_, target) in &mappings {
            if reset.workspace.surface(target).is_none() {
                return Err(DockspaceRuntimeError::native_root_surface_missing(target));
            }
        }
        let continuations = self.prepare_native_root_continuations(&mappings, &reset.workspace)?;
        self.append(EngineInput::ReplaceWorkspaceContinuingRoots {
            workspace: reset.workspace.clone(),
            continuations,
        })?;
        self.replacement_submitted = true;
        Ok(())
    }

    pub(super) fn prepare_native_root_continuations(
        &self,
        mappings: &[(NativeSurfaceBinding, SurfaceId)],
        workspace: &Workspace,
    ) -> Result<Vec<crate::frame::PreparedNativeRootContinuation>, DockspaceRuntimeError> {
        let viewport = self.frame.view().viewport();
        let predecessors = mappings
            .iter()
            .map(|&(existing, target)| {
                if !self
                    .session
                    .native
                    .as_ref()
                    .is_some_and(|native| native.recognizes_binding(existing))
                {
                    return Err(DockspaceRuntimeError::native_root_unavailable(
                        existing.surface(),
                    ));
                }
                let predecessor = viewport
                    .viewport(existing.surface())
                    .map(|record| record.binding())
                    .filter(|binding| existing.matches_viewport_binding(*binding))
                    .ok_or_else(|| {
                        DockspaceRuntimeError::native_root_unavailable(existing.surface())
                    })?;
                Ok((predecessor, target))
            })
            .collect::<Result<Vec<_>, _>>()?;
        let surfaces = workspace.surfaces().map(|(surface, _)| surface).collect();
        viewport
            .prepare_native_root_continuations(&predecessors, &surfaces)
            .map_err(DockspaceRuntimeError::native_root_continuation)
    }

    /// Resets the layout after this frame's complete ordered input prefix.
    ///
    /// Buffered native hosts may first finish [`DockspaceSession::poll_native_host_frame`]
    /// under their existing budget and submit here when
    /// [`Self::native_input_pending`] is false. This operation keeps that exact
    /// candidate and its already reduced input; it starts no second replay.
    ///
    /// The preparation remains bound to the published session, and its declared
    /// presentation identities survive replacement in a new epoch. Earlier product
    /// outcomes are superseded by the report's single `LayoutReset` result.
    /// Measurement may follow, but further semantic and configuration input may
    /// not. Dropping the frame leaves `reset` available for an exact retry.
    ///
    /// # Errors
    ///
    /// Returns an error for incomplete native input, stale or foreign preparation,
    /// arrangement lock, unknown document items, a previously submitted replacement,
    /// or a frame which has already entered configuration or presentation.
    pub fn submit_layout_reset(
        &mut self,
        reset: &PreparedDockspaceLayoutReset,
    ) -> Result<(), DockspaceRuntimeError> {
        self.ensure_replacement_input_allowed()?;
        self.session.ensure_prepared_layout_reset_current(reset)?;
        self.complete_pointer_input()?;
        let input = reset.replacement_input(self.frame.view())?;
        self.append(input)?;
        self.replacement_submitted = true;
        Ok(())
    }
}

impl PreparedDockspaceLayoutReset {
    /// Returns a read-only projection of the declared layout before replacement.
    ///
    /// This view grants no presentation authority. The terminal replacement preserves
    /// [`RootId`](crate::model::RootId), [`SurfaceId`](crate::model::SurfaceId),
    /// and [`FloatingPresentationId`](crate::model::FloatingPresentationId) values
    /// in a new workspace epoch.
    #[must_use]
    pub fn view(&self) -> DockspaceView<'_> {
        DockspaceView::new(&self.workspace)
    }

    pub(super) fn replacement_input(
        &self,
        frame: HostFrameView<'_>,
    ) -> Result<EngineInput, DockspaceRuntimeError> {
        self.ensure_authority(frame)?;
        Ok(EngineInput::ReplaceWorkspace(self.workspace.clone()))
    }

    fn ensure_authority(&self, frame: HostFrameView<'_>) -> Result<(), DockspaceRuntimeError> {
        if self.authority != frame.authority_domain() {
            return Err(DockspaceRuntimeError::prepared_layout_reset_mismatch());
        }
        Ok(())
    }
}
