//! Durable document identity, capture and restore.

use super::Dockspace;
use crate::{DockStyle, DockspaceError, DockspaceMutation};
use dockspace::model::{DockspaceLayout, ItemId};
use dockspace::policy::DockPolicy;
use dockspace::runtime::{
    DockspaceDocumentBootstrap, DockspaceDocumentId, DockspaceSession, SurfaceUnavailableReason,
};
use egui::Id;
use std::{fmt::Debug, hash::Hash};

impl Dockspace {
    #[cfg(feature = "serde")]
    pub(crate) fn from_persistent_layout_parts(
        id: Id,
        layout: DockspaceLayout,
        policy: DockPolicy,
        style: DockStyle,
        bootstrap: DockspaceDocumentBootstrap,
    ) -> Result<Self, DockspaceError> {
        let presentation = style
            .presentation_config()
            .map_err(DockspaceError::from_detail)?;
        let session = DockspaceSession::from_persistent_layout_with_presentation_config(
            layout,
            policy,
            presentation,
            bootstrap,
        )
        .map_err(DockspaceError::from_detail)?;
        Ok(Self::from_session(id, session, style))
    }

    /// Strictly restores one complete product document into a new egui facade.
    ///
    /// `recognize_external_item` must recognize every persisted application key,
    /// including closed historical panes. Numeric core identities are restored
    /// from the validated document and cannot be remapped by the application.
    ///
    /// # Errors
    ///
    /// Returns an error for invalid style, malformed or unsupported JSON, rejected
    /// pane identity associations, or a document that cannot initialize the core.
    #[cfg(feature = "serde")]
    pub fn from_document_json(
        id_salt: impl Hash + Debug,
        bytes: &[u8],
        policy: DockPolicy,
        style: DockStyle,
        recognize_external_item: impl Fn(DockspaceDocumentId, &str) -> bool,
    ) -> Result<Self, DockspaceError> {
        let presentation = style
            .presentation_config()
            .map_err(DockspaceError::from_detail)?;
        let session = DockspaceSession::from_document_json_with_presentation_config(
            bytes,
            policy,
            presentation,
            recognize_external_item,
        )
        .map_err(DockspaceError::from_detail)?;
        Ok(Self::from_session(
            Id::unique(("egui_dockspace", id_salt)),
            session,
            style,
        ))
    }

    /// Returns the current durable document lineage, when configured.
    #[cfg(feature = "serde")]
    #[must_use]
    pub fn document_id(&self) -> Option<DockspaceDocumentId> {
        self.session.document_id()
    }

    /// Returns the generation assigned to the next successful save.
    #[cfg(feature = "serde")]
    #[must_use]
    pub fn next_document_generation(&self) -> Option<u64> {
        self.session.next_document_generation()
    }

    /// Resolves one session-owned application pane key.
    #[cfg(feature = "serde")]
    #[must_use]
    pub fn item_id_for_external_key(&self, external_key: &str) -> Option<ItemId> {
        self.session.item_id_for_external_key(external_key)
    }

    /// Resolves one item to its exact session-owned application key.
    #[cfg(feature = "serde")]
    #[must_use]
    pub fn external_key_for_item(&self, item: ItemId) -> Option<&str> {
        self.session.external_key_for_item(item)
    }

    /// Allocates one append-only application pane identity.
    ///
    /// # Errors
    ///
    /// Returns a persistence error when the session is not document-bound, the
    /// key is invalid, or the item identity space is exhausted.
    #[cfg(feature = "serde")]
    pub fn ensure_external_item(
        &mut self,
        external_key: impl Into<String>,
    ) -> Result<ItemId, DockspaceError> {
        self.session
            .ensure_external_item(external_key)
            .map_err(DockspaceError::from_detail)
    }

    /// Captures and encodes the complete session-owned document as JSON bytes.
    ///
    /// # Errors
    ///
    /// Returns a persistence error without advancing the document generation when
    /// strict capture or JSON encoding fails.
    #[cfg(feature = "serde")]
    pub fn save_document_json(&mut self) -> Result<Vec<u8>, DockspaceError> {
        self.session
            .save_document_json()
            .map_err(DockspaceError::from_detail)
    }

    /// Restores a complete document into this session at one host-frame boundary.
    ///
    /// The restored surfaces are initially deferred, so the next egui frame can
    /// measure and paint their exact product plans before exposing interaction.
    ///
    /// # Errors
    ///
    /// Returns a persistence or host-frame error without publishing a partial
    /// document when decoding, identity recognition, or candidate validation
    /// fails.
    #[cfg(feature = "serde")]
    pub fn restore_document_json(
        &mut self,
        bytes: &[u8],
        recognize_external_item: impl Fn(DockspaceDocumentId, &str) -> bool,
    ) -> Result<DockspaceMutation, DockspaceError> {
        let restore = self
            .session
            .prepare_document_restore_json(bytes, recognize_external_item)
            .map_err(DockspaceError::from_detail)?;
        let mut frame = self
            .session
            .begin_document_restore_frame(&restore)
            .map_err(DockspaceError::from_detail)?;
        frame
            .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
            .map_err(DockspaceError::from_detail)?;
        let report = frame.commit().map_err(DockspaceError::from_detail)?;
        self.pending_focus.clear();
        Ok(DockspaceMutation::from_runtime_report(&report))
    }
}
