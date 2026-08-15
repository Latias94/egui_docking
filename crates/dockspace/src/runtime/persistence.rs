//! Product persistence owned by the renderer-neutral dockspace session.

use std::fmt;

use thiserror::Error;

use super::DockspaceSession;
use crate::document::{
    BoundDocumentState, DockspaceDocumentCaptureError, DockspaceDocumentDecodeError,
    DockspaceDocumentEnvelope, DockspaceDocumentRestoreError, RuntimeDocumentRestoreError,
};
use crate::external_item_key::{ExternalItemKeyMapError, ExternalItemKeyRestoreError};
use crate::model::{DockspaceLayout, ItemId};
use crate::persistence::SnapshotRestoreError;
use crate::policy::DockPolicy;
use crate::presentation_config::DockPresentationConfig;
use crate::viewport_persistence::ViewportPlacementRestoreError;
use crate::{document, engine::DockEngine};

pub use crate::document::DockspaceDocumentId;

/// One-time allocator for stable application pane identities.
///
/// Allocate every item used by the initial [`DockspaceLayout`] through this
/// value, then consume it with [`DockspaceSession::from_persistent_layout`].
/// The complete mapping is moved into the session and cannot later be replaced
/// independently of the graph.
pub struct DockspaceDocumentBootstrap {
    inner: document::DockspaceDocumentBootstrap,
}

impl DockspaceDocumentBootstrap {
    /// Starts a fresh document lineage whose first saved generation is zero.
    #[must_use]
    pub const fn new(document_id: DockspaceDocumentId) -> Self {
        Self {
            inner: document::DockspaceDocumentBootstrap::new(document_id),
        }
    }

    /// Returns the existing item for one application key or allocates a new one.
    ///
    /// # Errors
    ///
    /// Returns an error for an empty key or exhausted item identity space without
    /// partially changing the bootstrap.
    pub fn ensure_item(
        &mut self,
        external_key: impl Into<String>,
    ) -> Result<ItemId, DockspacePersistenceError> {
        self.inner
            .ensure_external_item_key(external_key)
            .map_err(DockspacePersistenceError::external_item_key)
    }

    /// Atomically allocates every previously unseen key in caller order.
    ///
    /// # Errors
    ///
    /// Returns an error without partial allocation when any key is invalid or
    /// the complete batch cannot fit in the remaining item identity space.
    pub fn ensure_items<I, K>(&mut self, external_keys: I) -> Result<(), DockspacePersistenceError>
    where
        I: IntoIterator<Item = K>,
        K: Into<String>,
    {
        self.inner
            .ensure_external_item_keys(external_keys)
            .map_err(DockspacePersistenceError::external_item_key)
    }

    /// Resolves an item allocated by this bootstrap.
    #[must_use]
    pub fn item_id(&self, external_key: &str) -> Option<ItemId> {
        self.inner.item_id(external_key)
    }
}

impl fmt::Debug for DockspaceDocumentBootstrap {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("DockspaceDocumentBootstrap")
            .finish_non_exhaustive()
    }
}

impl DockspaceSession {
    /// Creates one session and atomically binds its initial layout to a document lineage.
    ///
    /// # Errors
    ///
    /// Returns an error when the layout cannot initialize the core or any layout
    /// item was not allocated through `bootstrap`.
    pub fn from_persistent_layout(
        layout: DockspaceLayout,
        policy: DockPolicy,
        bootstrap: DockspaceDocumentBootstrap,
    ) -> Result<Self, DockspacePersistenceError> {
        Self::from_persistent_layout_with_presentation_config(
            layout,
            policy,
            DockPresentationConfig::default(),
            bootstrap,
        )
    }

    /// Creates one persistent session with explicit renderer-neutral geometry.
    ///
    /// # Errors
    ///
    /// Returns an error when the layout, presentation configuration, or document
    /// identity binding is invalid.
    pub fn from_persistent_layout_with_presentation_config(
        layout: DockspaceLayout,
        policy: DockPolicy,
        presentation_config: DockPresentationConfig,
        bootstrap: DockspaceDocumentBootstrap,
    ) -> Result<Self, DockspacePersistenceError> {
        let workspace = layout.into_workspace();
        let binding = BoundDocumentState::from_bootstrap_workspace(&workspace, bootstrap.inner)
            .map_err(DockspacePersistenceError::capture)?;
        let engine =
            DockEngine::new_with_presentation_config(workspace, policy, presentation_config)
                .map_err(DockspacePersistenceError::engine)?;
        let mut session = Self::from_engine(engine).map_err(DockspacePersistenceError::engine)?;
        session.document = Some(binding);
        Ok(session)
    }

    /// Strictly decodes one complete document into a new session authority.
    ///
    /// `recognize_external_item` must recognize every exact persisted application
    /// key, including closed historical panes. Numeric item identities remain
    /// owned by the validated document and cannot be remapped by the caller.
    ///
    /// # Errors
    ///
    /// Returns an error for malformed JSON, unsupported versions, invalid graph
    /// or sidecar state, rejected item identity associations, or core construction.
    pub fn from_document_json(
        bytes: &[u8],
        policy: DockPolicy,
        recognize_external_item: impl Fn(DockspaceDocumentId, &str) -> bool,
    ) -> Result<Self, DockspacePersistenceError> {
        Self::from_document_json_with_presentation_config(
            bytes,
            policy,
            DockPresentationConfig::default(),
            recognize_external_item,
        )
    }

    /// Strictly decodes one complete document with explicit presentation geometry.
    ///
    /// # Errors
    ///
    /// Returns an error without publishing a partial session when decoding,
    /// identity validation, or core construction fails.
    pub fn from_document_json_with_presentation_config(
        bytes: &[u8],
        policy: DockPolicy,
        presentation_config: DockPresentationConfig,
        recognize_external_item: impl Fn(DockspaceDocumentId, &str) -> bool,
    ) -> Result<Self, DockspacePersistenceError> {
        let document = decode_document(bytes)?;
        let restored = document
            .restore(recognize_external_item)
            .map_err(DockspacePersistenceError::restore)?;
        let (restore, binding) = restored.into_runtime_parts();
        let engine = DockEngine::from_validated_restore_with_presentation_config(
            restore,
            policy,
            presentation_config,
        )
        .map_err(DockspacePersistenceError::engine)?;
        let mut session = Self::from_engine(engine).map_err(DockspacePersistenceError::engine)?;
        session.document = Some(binding);
        Ok(session)
    }

    /// Begins one rollbackable frame that replaces the live session from JSON.
    ///
    /// The incoming document must belong to the session's existing lineage.
    /// `recognize_external_item` validates every persisted application key while
    /// numeric [`ItemId`] values remain document-owned. The returned frame exposes
    /// the restored candidate for measurement, but graph state, key history,
    /// viewport placement, generation, and allocator frontiers publish only when
    /// [`super::DockspaceHostFrame::commit`] succeeds.
    /// When a native host is enrolled, restore must instead join that host's
    /// ordered causal frame; this standalone entry point rejects before decoding.
    ///
    /// # Errors
    ///
    /// Returns an error without changing published state when persistence is not
    /// configured, the document is malformed or belongs to another lineage, an
    /// external key is rejected, or the rollbackable host frame cannot begin.
    pub fn begin_document_restore_frame(
        &mut self,
        bytes: &[u8],
        recognize_external_item: impl Fn(DockspaceDocumentId, &str) -> bool,
    ) -> Result<super::DockspaceHostFrame<'_>, super::DockspaceRuntimeError> {
        self.ensure_standalone_document_restore_available()?;
        let document = decode_document(bytes)?;
        let prepared = self
            .document
            .as_ref()
            .ok_or_else(DockspacePersistenceError::not_configured)?
            .prepare_runtime_restore(&self.engine, document, recognize_external_item)
            .map_err(DockspacePersistenceError::session)?;
        self.begin_host_frame_with_document_restore(prepared)
    }

    /// Returns the current document lineage, when persistence is configured.
    #[must_use]
    pub fn document_id(&self) -> Option<DockspaceDocumentId> {
        self.document.as_ref().map(BoundDocumentState::document_id)
    }

    /// Returns the generation assigned to the next successful save.
    #[must_use]
    pub fn next_document_generation(&self) -> Option<u64> {
        self.document
            .as_ref()
            .and_then(BoundDocumentState::next_generation)
    }

    /// Resolves one session-owned application key.
    #[must_use]
    pub fn item_id_for_external_key(&self, external_key: &str) -> Option<ItemId> {
        self.document
            .as_ref()
            .and_then(|document| document.item_id(external_key))
    }

    /// Resolves one item to its exact session-owned application key.
    #[must_use]
    pub fn external_key_for_item(&self, item: ItemId) -> Option<&str> {
        self.document
            .as_ref()
            .and_then(|document| document.external_item_key(item))
    }

    /// Allocates one append-only external item identity through this session.
    ///
    /// # Errors
    ///
    /// Returns an error when persistence is not configured, the key is empty, or
    /// the item identity space is exhausted.
    pub fn ensure_external_item(
        &mut self,
        external_key: impl Into<String>,
    ) -> Result<ItemId, DockspacePersistenceError> {
        self.document
            .as_mut()
            .ok_or_else(DockspacePersistenceError::not_configured)?
            .ensure_external_item_key(external_key)
            .map_err(DockspacePersistenceError::external_item_key)
    }

    /// Atomically allocates a batch of append-only external item identities.
    ///
    /// # Errors
    ///
    /// Returns an error without partial allocation when persistence is absent or
    /// the complete batch is invalid.
    pub fn ensure_external_items<I, K>(
        &mut self,
        external_keys: I,
    ) -> Result<(), DockspacePersistenceError>
    where
        I: IntoIterator<Item = K>,
        K: Into<String>,
    {
        self.document
            .as_mut()
            .ok_or_else(DockspacePersistenceError::not_configured)?
            .ensure_external_item_keys(external_keys)
            .map_err(DockspacePersistenceError::external_item_key)
    }

    /// Captures and encodes the complete session-owned document as UTF-8 JSON bytes.
    ///
    /// The generation advances only after JSON encoding succeeds. A failed save
    /// therefore leaves both the graph and durable sidecars unchanged.
    ///
    /// # Errors
    ///
    /// Returns an error when persistence is not configured, strict capture fails,
    /// the generation is exhausted, or JSON encoding fails.
    pub fn save_document_json(&mut self) -> Result<Vec<u8>, DockspacePersistenceError> {
        let document = self
            .document
            .as_mut()
            .ok_or_else(DockspacePersistenceError::not_configured)?;
        let prepared = document
            .prepare_capture(&self.engine)
            .map_err(DockspacePersistenceError::capture)?;
        let encoded = serde_json::to_vec(prepared.document())
            .map_err(DockspacePersistenceError::encode_json)?;
        let _ = document.commit_capture(prepared);
        Ok(encoded)
    }
}

fn decode_document(
    bytes: &[u8],
) -> Result<crate::document::DockspaceDocument, DockspacePersistenceError> {
    let envelope: DockspaceDocumentEnvelope =
        serde_json::from_slice(bytes).map_err(DockspacePersistenceError::decode_json)?;
    envelope
        .into_document()
        .map_err(DockspacePersistenceError::decode)
}

/// Stable category for one product persistence failure.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DockspacePersistenceErrorKind {
    /// The operation requires a document-bound session.
    NotConfigured,
    /// Bootstrap data or an application key is invalid.
    InvalidInput,
    /// JSON or a validated document component is malformed or inconsistent.
    InvalidDocument,
    /// The document or one of its embedded snapshots uses an unsupported version.
    UnsupportedVersion,
    /// Persisted application pane identity does not match the application registry.
    IdentityConflict,
    /// An item or document generation identity space is exhausted.
    CapacityExhausted,
    /// A validated core or serializer invariant failed internally.
    Internal,
}

/// Failure at the product persistence boundary.
#[derive(Debug)]
pub struct DockspacePersistenceError {
    source: DockspacePersistenceErrorSource,
}

impl DockspacePersistenceError {
    /// Returns the stable product-level category.
    #[must_use]
    pub fn kind(&self) -> DockspacePersistenceErrorKind {
        match &self.source {
            DockspacePersistenceErrorSource::NotConfigured => {
                DockspacePersistenceErrorKind::NotConfigured
            }
            DockspacePersistenceErrorSource::ExternalItemKey(
                ExternalItemKeyMapError::EmptyExternalKey,
            )
            | DockspacePersistenceErrorSource::Capture(
                DockspaceDocumentCaptureError::MissingExternalItemKey(_)
                | DockspaceDocumentCaptureError::UnknownPlacementSurface(_),
            ) => DockspacePersistenceErrorKind::InvalidInput,
            DockspacePersistenceErrorSource::ExternalItemKey(
                ExternalItemKeyMapError::ItemIdSpaceExhausted { .. },
            )
            | DockspacePersistenceErrorSource::Capture(
                DockspaceDocumentCaptureError::GenerationExhausted,
            ) => DockspacePersistenceErrorKind::CapacityExhausted,
            DockspacePersistenceErrorSource::Decode(
                DockspaceDocumentDecodeError::UnsupportedVersion { .. }
                | DockspaceDocumentDecodeError::Workspace(SnapshotRestoreError::UnsupportedVersion {
                    ..
                })
                | DockspaceDocumentDecodeError::ExternalItemKeys(
                    ExternalItemKeyRestoreError::UnsupportedVersion { .. },
                )
                | DockspaceDocumentDecodeError::ViewportPlacements(
                    ViewportPlacementRestoreError::UnsupportedVersion { .. },
                ),
            ) => DockspacePersistenceErrorKind::UnsupportedVersion,
            DockspacePersistenceErrorSource::Restore(
                DockspaceDocumentRestoreError::ExternalItemAssociationsRejected { .. },
            ) => DockspacePersistenceErrorKind::IdentityConflict,
            DockspacePersistenceErrorSource::DecodeJson(_)
            | DockspacePersistenceErrorSource::Decode(_)
            | DockspacePersistenceErrorSource::Restore(_) => {
                DockspacePersistenceErrorKind::InvalidDocument
            }
            DockspacePersistenceErrorSource::Capture(_)
            | DockspacePersistenceErrorSource::EncodeJson(_)
            | DockspacePersistenceErrorSource::Engine(_) => DockspacePersistenceErrorKind::Internal,
            DockspacePersistenceErrorSource::Session(error) => session_error_kind(error),
        }
    }

    const fn not_configured() -> Self {
        Self {
            source: DockspacePersistenceErrorSource::NotConfigured,
        }
    }

    fn external_item_key(error: ExternalItemKeyMapError) -> Self {
        Self {
            source: DockspacePersistenceErrorSource::ExternalItemKey(error),
        }
    }

    fn capture(error: DockspaceDocumentCaptureError) -> Self {
        Self {
            source: DockspacePersistenceErrorSource::Capture(error),
        }
    }

    fn encode_json(error: serde_json::Error) -> Self {
        Self {
            source: DockspacePersistenceErrorSource::EncodeJson(error),
        }
    }

    fn decode_json(error: serde_json::Error) -> Self {
        Self {
            source: DockspacePersistenceErrorSource::DecodeJson(error),
        }
    }

    const fn decode(error: DockspaceDocumentDecodeError) -> Self {
        Self {
            source: DockspacePersistenceErrorSource::Decode(error),
        }
    }

    const fn restore(error: DockspaceDocumentRestoreError) -> Self {
        Self {
            source: DockspacePersistenceErrorSource::Restore(error),
        }
    }

    fn engine(error: crate::engine::EngineError) -> Self {
        Self {
            source: DockspacePersistenceErrorSource::Engine(Box::new(error)),
        }
    }

    pub(super) fn session(error: RuntimeDocumentRestoreError) -> Self {
        Self {
            source: DockspacePersistenceErrorSource::Session(Box::new(error)),
        }
    }
}

impl fmt::Display for DockspacePersistenceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.source.fmt(formatter)
    }
}

impl std::error::Error for DockspacePersistenceError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        std::error::Error::source(&self.source)
    }
}

#[derive(Debug, Error)]
enum DockspacePersistenceErrorSource {
    #[error("dockspace persistence is not configured")]
    NotConfigured,
    #[error("external item identity allocation failed: {0}")]
    ExternalItemKey(ExternalItemKeyMapError),
    #[error("dockspace document capture failed: {0}")]
    Capture(DockspaceDocumentCaptureError),
    #[error("dockspace document JSON encoding failed: {0}")]
    EncodeJson(serde_json::Error),
    #[error("dockspace document JSON decoding failed: {0}")]
    DecodeJson(serde_json::Error),
    #[error("dockspace document version decoding failed: {0}")]
    Decode(DockspaceDocumentDecodeError),
    #[error("dockspace document validation failed: {0}")]
    Restore(DockspaceDocumentRestoreError),
    #[error("dockspace engine initialization failed: {0}")]
    Engine(#[source] Box<crate::engine::EngineError>),
    #[error("live dockspace document restore failed: {0}")]
    Session(#[source] Box<RuntimeDocumentRestoreError>),
}

fn session_error_kind(error: &RuntimeDocumentRestoreError) -> DockspacePersistenceErrorKind {
    match error {
        RuntimeDocumentRestoreError::WrongLineage { .. }
        | RuntimeDocumentRestoreError::Reconcile(_) => {
            DockspacePersistenceErrorKind::IdentityConflict
        }
        RuntimeDocumentRestoreError::Restore(
            DockspaceDocumentRestoreError::ExternalItemAssociationsRejected { .. },
        ) => DockspacePersistenceErrorKind::IdentityConflict,
        RuntimeDocumentRestoreError::Restore(_) => DockspacePersistenceErrorKind::InvalidDocument,
        _ => DockspacePersistenceErrorKind::Internal,
    }
}
