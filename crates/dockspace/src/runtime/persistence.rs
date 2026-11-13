//! Product persistence owned by the renderer-neutral dockspace session.

use std::{collections::BTreeSet, fmt};

use thiserror::Error;

use super::DockspaceSession;
use crate::document::{
    BoundDocumentState, DockspaceDocumentCaptureError, DockspaceDocumentDecodeError,
    DockspaceDocumentEnvelope, DockspaceDocumentRestoreError, PreparedRuntimeDocumentRestore,
    RuntimeDocumentRestoreError,
};
use crate::external_item_key::ExternalItemKeyMapError;
use crate::geometry::{PhysicalRect, PhysicalSize, ScaleFactor};
use crate::model::{DockspaceLayout, DockspaceView, ItemId, SurfaceId};
use crate::policy::DockPolicy;
use crate::presentation_config::DockPresentationConfig;
use crate::viewport_persistence::{NativeSurfaceHostPreference, ViewportPlacementPreferences};
use crate::{document, engine::DockEngine};

pub use crate::document::DockspaceDocumentId;
pub use crate::viewport_persistence::{
    NativeSurfaceHostPreference as DockspaceNativeSurfaceHosting,
    WindowPresentationPreference as DockspaceWindowPresentationPreference,
};

/// Durable surface hosting and window-placement preferences from one document.
///
/// The workspace view remains the sole content graph. These declarations are
/// restoration inputs for an application host; they do not create native windows
/// or grant binding, coordinate, recovery or presentation authority. Missing
/// placement remains explicit, and geometry is measured in desktop physical pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DockspaceSurfacePresentationPreference {
    surface: SurfaceId,
    native_hosting: Option<NativeSurfaceHostPreference>,
    outer_rect: Option<PhysicalRect>,
    inner_size: Option<PhysicalSize>,
    scale_factor: Option<ScaleFactor>,
    presentation: Option<DockspaceWindowPresentationPreference>,
}

impl DockspaceSurfacePresentationPreference {
    /// Returns the logical surface described by this declaration.
    #[must_use]
    pub const fn surface(self) -> SurfaceId {
        self.surface
    }

    /// Returns explicit native hosting intent, independently of placement.
    #[must_use]
    pub const fn native_hosting(self) -> Option<DockspaceNativeSurfaceHosting> {
        self.native_hosting
    }

    /// Returns the last-confirmed desktop-physical outer bounds, when saved.
    #[must_use]
    pub const fn outer_rect(self) -> Option<PhysicalRect> {
        self.outer_rect
    }

    /// Returns a native physical client-size hint from the confirming observation.
    #[must_use]
    pub const fn inner_size_hint(self) -> Option<PhysicalSize> {
        self.inner_size
    }

    /// Returns the saved native scale hint, without granting current DPI authority.
    #[must_use]
    pub const fn scale_factor_hint(self) -> Option<ScaleFactor> {
        self.scale_factor
    }

    /// Returns an optional durable presentation preference.
    #[must_use]
    pub const fn window_presentation(self) -> Option<DockspaceWindowPresentationPreference> {
        self.presentation
    }
}

fn surface_presentation_preferences(
    preferences: Option<&ViewportPlacementPreferences>,
) -> impl Iterator<Item = DockspaceSurfacePresentationPreference> + '_ {
    preferences.into_iter().flat_map(|preferences| {
        preferences
            .entries()
            .map(
                |(surface, native_hosting, placement)| DockspaceSurfacePresentationPreference {
                    surface,
                    native_hosting,
                    outer_rect: placement.map(|placement| placement.outer_rect()),
                    inner_size: placement.and_then(|placement| placement.inner_size()),
                    scale_factor: placement.and_then(|placement| placement.scale_factor()),
                    presentation: placement.and_then(|placement| placement.presentation()),
                },
            )
    })
}

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

/// One decoded and application-authorized live document replacement.
///
/// The value remains owned by the caller while a host-frame attempt borrows it.
/// A failed begin, dropped frame, or failed commit therefore leaves the exact
/// validated candidate available for an unchanged retry without decoding JSON
/// or invoking the application recognizer again.
#[must_use = "a prepared document restore should be submitted or intentionally discarded"]
pub struct PreparedDockspaceDocumentRestore {
    inner: PreparedRuntimeDocumentRestore,
}

impl PreparedDockspaceDocumentRestore {
    /// Validates declarations for one retained external root and explicit children.
    ///
    /// This read-only check uses pre-rebase identities and grants no native
    /// authority. Submission validates the final remapped references again.
    ///
    /// # Errors
    ///
    /// Rejects an absent root, a Child declaration for that root, extra roots,
    /// undeclared child surfaces, foreign recovery hosts and absent saved placement.
    pub fn validate_native_surface_hosting(
        &self,
        root_surface: SurfaceId,
    ) -> Result<(), super::DockspaceRuntimeError> {
        native_surface_restore_plan(
            self.view(),
            self.surface_presentation_preferences(),
            [root_surface],
        )?;
        Ok(())
    }

    /// Validates hosting for an explicit set of retained external Root windows.
    ///
    /// Every other surface must declare a Child recovering to one of these
    /// roots and carry a positive saved physical placement. This check uses
    /// document identities and grants no current native authority.
    ///
    /// # Errors
    ///
    /// Rejects empty or duplicate roots, missing Root surfaces, conflicting
    /// declarations, missing Child placement and recovery outside the root set.
    pub fn validate_native_surface_hosting_for_roots(
        &self,
        roots: impl IntoIterator<Item = SurfaceId>,
    ) -> Result<(), super::DockspaceRuntimeError> {
        native_surface_restore_plan(self.view(), self.surface_presentation_preferences(), roots)?;
        Ok(())
    }

    /// Returns a read-only projection of the declared layout before replacement.
    ///
    /// This view grants no presentation authority. The terminal replacement may
    /// rebase [`RootId`](crate::model::RootId), [`SurfaceId`](crate::model::SurfaceId),
    /// and [`FloatingPresentationId`](crate::model::FloatingPresentationId) values
    /// before publication.
    #[must_use]
    pub fn view(&self) -> DockspaceView<'_> {
        DockspaceView::new(self.inner.workspace())
    }

    /// Returns declared surface preferences before presentation identity rebasing.
    ///
    /// Inspect the candidate frame again after submission: surface and recovery
    /// host references may be rebased together at that boundary. These preferences
    /// grant no native window, coordinate or presentation authority.
    pub fn surface_presentation_preferences(
        &self,
    ) -> impl Iterator<Item = DockspaceSurfacePresentationPreference> + '_ {
        surface_presentation_preferences(Some(self.inner.viewport_placements()))
    }
}

/// A document declaration cannot be hosted by its explicit external Root mapping.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum DockspaceNativeDocumentHostingError {
    /// Native restoration requires an explicit nonempty external Root mapping.
    #[error("native document restoration requires at least one external root")]
    RootMappingRequired,
    /// An incoming logical Root can name only one external window.
    #[error("native document repeats root mapping for {surface}")]
    DuplicateRootMapping { surface: SurfaceId },
    /// The incoming layout must preserve its configured logical root.
    #[error("native document omits configured root surface {surface}")]
    RootSurfaceMissing { surface: SurfaceId },
    /// The retained external root cannot be declared as a managed Child.
    #[error("native document declares configured root {surface} as a child")]
    RootDeclaredChild { surface: SurfaceId },
    /// Every additional surface requires one explicit Child declaration.
    #[error("document surface {surface} has no supported child hosting declaration")]
    ChildDeclarationRequired { surface: SurfaceId },
    /// The Child recovery host is outside the supplied external Root set.
    #[error("document child {surface} recovers to unmapped root {root}")]
    RecoveryHostMismatch { surface: SurfaceId, root: SurfaceId },
    /// A saved physical rectangle is required for explicit initial placement.
    #[error("document child {surface} has no positive saved outer placement")]
    ChildPlacementRequired { surface: SurfaceId },
    /// The actual external root cannot continue through this replacement.
    #[error("native document cannot retain the current external root {surface}")]
    RootWindowUnavailable { surface: SurfaceId },
    /// A Root outside the continuation batch still occupies the incoming Child surface.
    #[error("native document child {surface} still owns an external root window")]
    ChildAlreadyHosted { surface: SurfaceId },
}

fn native_surface_restore_plan(
    view: DockspaceView<'_>,
    preferences: impl Iterator<Item = DockspaceSurfacePresentationPreference>,
    roots: impl IntoIterator<Item = SurfaceId>,
) -> Result<crate::frame::NativeSurfaceRestorePlan, DockspaceNativeDocumentHostingError> {
    let mut root_set = BTreeSet::new();
    for root in roots {
        if !root_set.insert(root) {
            return Err(DockspaceNativeDocumentHostingError::DuplicateRootMapping {
                surface: root,
            });
        }
        if view.surface(root).is_none() {
            return Err(DockspaceNativeDocumentHostingError::RootSurfaceMissing { surface: root });
        }
    }
    if root_set.is_empty() {
        return Err(DockspaceNativeDocumentHostingError::RootMappingRequired);
    }
    let preferences = preferences
        .map(|preference| (preference.surface(), preference))
        .collect::<std::collections::BTreeMap<_, _>>();
    for &root in &root_set {
        if matches!(
            preferences
                .get(&root)
                .and_then(|preference| preference.native_hosting()),
            Some(NativeSurfaceHostPreference::Child { .. })
        ) {
            return Err(DockspaceNativeDocumentHostingError::RootDeclaredChild { surface: root });
        }
    }
    let mut children = Vec::new();
    for surface in view
        .surfaces()
        .map(|surface| surface.id())
        .filter(|surface| !root_set.contains(surface))
    {
        let preference = preferences.get(&surface);
        let Some(NativeSurfaceHostPreference::Child { recovery_host }) =
            preference.and_then(|preference| preference.native_hosting())
        else {
            return Err(DockspaceNativeDocumentHostingError::ChildDeclarationRequired { surface });
        };
        if !root_set.contains(&recovery_host) {
            return Err(DockspaceNativeDocumentHostingError::RecoveryHostMismatch {
                surface,
                root: recovery_host,
            });
        }
        let Some(placement) = preference
            .and_then(|preference| preference.outer_rect())
            .filter(|rect| rect.width() > 0.0 && rect.height() > 0.0)
        else {
            return Err(DockspaceNativeDocumentHostingError::ChildPlacementRequired { surface });
        };
        children.push(crate::frame::NativeSurfaceInitialRequest {
            surface,
            recovery_host,
            placement,
        });
    }
    Ok(crate::frame::NativeSurfaceRestorePlan {
        roots: root_set,
        children,
    })
}

impl fmt::Debug for PreparedDockspaceDocumentRestore {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PreparedDockspaceDocumentRestore")
            .finish_non_exhaustive()
    }
}

impl DockspaceSession {
    /// Returns surface preferences from the committed document binding.
    ///
    /// Reading this projection neither captures new OS facts nor advances the
    /// save generation. Hosting declarations follow successful publication and
    /// never retain a removed recovery host. Saving a native snapshot captures
    /// its current observed placement; an unbound session yields no preferences.
    pub fn surface_presentation_preferences(
        &self,
    ) -> impl Iterator<Item = DockspaceSurfacePresentationPreference> + '_ {
        surface_presentation_preferences(
            self.document
                .as_ref()
                .map(BoundDocumentState::viewport_placements),
        )
    }

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

    /// Decodes and authorizes one complete live document replacement.
    ///
    /// The incoming document must belong to the session's existing lineage.
    /// `recognize_external_item` validates every persisted application key while
    /// numeric [`ItemId`] values remain document-owned. The returned opaque value
    /// is bound to the current session authority, workspace version, and document
    /// sidecars. It can be retried unchanged after a recoverable host-frame error.
    /// Recognizing an identity does not require its application content to be loaded.
    /// Hosts may retain a known unavailable item and draw a recoverable placeholder;
    /// returning `false` rejects the complete document rather than closing that item.
    ///
    /// # Errors
    ///
    /// Returns an error when persistence is not configured, the document is
    /// malformed or belongs to another lineage, or an external key is rejected.
    pub fn prepare_document_restore_json(
        &self,
        bytes: &[u8],
        recognize_external_item: impl Fn(DockspaceDocumentId, &str) -> bool,
    ) -> Result<PreparedDockspaceDocumentRestore, DockspacePersistenceError> {
        let document = decode_document(bytes)?;
        let inner = self
            .document
            .as_ref()
            .ok_or_else(DockspacePersistenceError::not_configured)?
            .prepare_runtime_restore(&self.engine, document, recognize_external_item)
            .map_err(DockspacePersistenceError::session)?;
        Ok(PreparedDockspaceDocumentRestore { inner })
    }

    /// Begins one rollbackable frame that replaces the live session.
    ///
    /// The returned frame exposes the restored candidate for measurement, but
    /// graph state, key history, viewport placement, generation, and allocator
    /// frontiers publish only when [`super::DockspaceHostFrame::commit`] succeeds.
    /// An observed-roots host joins this frame without a pointer resolver. A
    /// managed-desktop host must use [`Self::begin_native_document_restore_frame`]
    /// so pointer receiver questions remain in the same ordered causal frame.
    ///
    /// # Sequencing
    ///
    /// Document replacement is the final semantic mutation in this frame.
    /// Product actions, close operations, configuration changes, and non-empty
    /// pointer input must be submitted in an earlier prefix or a later frame.
    /// Attempts after the replacement return `OperationConflict`.
    ///
    /// # Errors
    ///
    /// Returns an error without changing published state when the prepared value
    /// is stale or belongs to another session, arrangement editing is locked,
    /// or the rollbackable frame cannot begin. If the frame is suspended and its
    /// document sidecars change before resume, resuming the submitted restore
    /// returns `OperationConflict`; prepare it again against the current session.
    /// The caller retains `restore` for an exact retry when no intervening state changed.
    pub fn begin_document_restore_frame(
        &mut self,
        restore: &PreparedDockspaceDocumentRestore,
    ) -> Result<super::DockspaceHostFrame<'_>, super::DockspaceRuntimeError> {
        self.ensure_standalone_replacement_available()?;
        self.ensure_prepared_document_restore_current(restore)?;
        let mut frame = self.begin_host_frame()?;
        frame.submit_document_restore(restore)?;
        Ok(frame)
    }

    /// Begins one native host frame that atomically replaces the live document.
    ///
    /// Native platform facts, pointer input, and the document replacement reduce
    /// through the same ordered host frame. The restored candidate remains
    /// unpublished if receiver resolution, measurement, presentation, or final
    /// commit fails. This entry point is only for a managed-desktop host;
    /// observed-roots hosts use [`Self::begin_document_restore_frame`].
    ///
    /// # Sequencing
    ///
    /// Document replacement is the final semantic mutation in this frame.
    /// Product actions, close operations, configuration changes, and non-empty
    /// pointer input must be submitted in the native prefix or a later frame.
    /// Attempts after the replacement return `OperationConflict`.
    ///
    /// # Errors
    ///
    /// Returns an error without changing published state when no managed native
    /// host is active, the prepared value is stale or belongs to another session,
    /// arrangement editing is locked, receiver facts are unavailable, or the rollbackable frame cannot begin.
    /// The caller retains `restore` for an exact retry.
    pub fn begin_native_document_restore_frame(
        &mut self,
        restore: &PreparedDockspaceDocumentRestore,
        mut resolve: impl FnMut(super::NativeReceiverQuery) -> super::NativeReceiverAnswer,
    ) -> Result<super::DockspaceHostFrame<'_>, super::DockspaceRuntimeError> {
        self.ensure_managed_native_host_available()?;
        self.ensure_prepared_document_restore_current(restore)?;
        let mut frame = self.begin_native_host_frame(&mut resolve)?;
        frame.submit_document_restore(restore)?;
        Ok(frame)
    }

    fn ensure_prepared_document_restore_current(
        &self,
        restore: &PreparedDockspaceDocumentRestore,
    ) -> Result<(), super::DockspaceRuntimeError> {
        if self.policy().arrangement_locked() {
            return Err(super::DockspaceRuntimeError::arrangement_locked());
        }
        let document = self
            .document
            .as_ref()
            .ok_or_else(DockspacePersistenceError::not_configured)?;
        if !restore.inner.is_current_for(&self.engine, document) {
            return Err(super::DockspaceRuntimeError::prepared_document_restore_mismatch());
        }
        Ok(())
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

impl super::DockspaceHostFrame<'_> {
    /// Returns the candidate document's preferences with rebased surface references.
    ///
    /// A submitted document restore uses its pending atomic binding. Ordinary
    /// frames read the committed binding. Declared preferences grant no authority
    /// and publish only with this frame's successful document replacement.
    pub fn surface_presentation_preferences(
        &self,
    ) -> impl Iterator<Item = DockspaceSurfacePresentationPreference> + '_ {
        let preferences = self
            .document_restore
            .as_ref()
            .map(|restore| restore.viewport_placements())
            .or_else(|| {
                self.session
                    .document
                    .as_ref()
                    .map(BoundDocumentState::viewport_placements)
            });
        surface_presentation_preferences(preferences)
    }

    /// Restores a prepared document after this frame's complete ordered input.
    ///
    /// A buffered native host can finish its existing budgeted poll and submit
    /// here without restarting replay. Earlier input uses the published
    /// document's item identities. Only this final replacement admits new keys
    /// from the already validated and reconciled restore; they cannot authorize
    /// an earlier input.
    ///
    /// The restored workspace, key history, placement preferences and generation
    /// publish together at commit. Earlier product outcomes are superseded by
    /// the report's `DocumentRestored` result. Measurement may follow, while
    /// further semantic and configuration input is rejected. Dropping or failing
    /// the frame retains the preparation for an exact retry.
    ///
    /// # Errors
    ///
    /// Returns an error for incomplete native input, a stale or foreign
    /// preparation, changed document sidecars, arrangement lock, a previous
    /// replacement, or a frame already in configuration or presentation.
    pub fn submit_document_restore(
        &mut self,
        restore: &PreparedDockspaceDocumentRestore,
    ) -> Result<(), super::DockspaceRuntimeError> {
        self.submit_document_restore_with_native_roots(restore, None, &[])
    }

    /// Atomically restores a document and queues its explicit native Child surfaces.
    ///
    /// The configured external Root must survive as a retained native successor.
    /// Child intents use final rebased identities and saved physical placement.
    /// This terminal input creates no windows; later budgeted frames advance the
    /// same lifecycle after fresh facts and exact predecessor quiescence. Each
    /// child still requires renderer preparation and semantic first-live output.
    ///
    /// # Errors
    ///
    /// Returns ordinary restore errors, invalid native hosting declarations,
    /// an unavailable managed host or an external Root that cannot be retained.
    pub fn submit_native_document_restore(
        &mut self,
        restore: &PreparedDockspaceDocumentRestore,
        root_surface: SurfaceId,
    ) -> Result<(), super::DockspaceRuntimeError> {
        self.session.ensure_managed_native_host_available()?;
        restore.validate_native_surface_hosting(root_surface)?;
        self.submit_document_restore_with_native_roots(restore, Some(&[root_surface]), &[])
    }

    /// Restores native surfaces while continuing the external Root on a new surface.
    ///
    /// `incoming_root` names the prepared document's surface before rebasing.
    /// The selected Root, Child recovery references and saved placement are
    /// resolved through the same identity remap. The old logical Root surface
    /// may disappear. A committed exact continuation preserves its OS window,
    /// while fresh observations and output establish successor interaction.
    ///
    /// # Errors
    ///
    /// Returns ordinary native document restore errors, an unavailable exact
    /// external Root or a conflicting target window. Graph, document sidecars
    /// and binding continuation publish at the same rollbackable boundary.
    pub fn submit_native_document_restore_rebinding_root(
        &mut self,
        restore: &PreparedDockspaceDocumentRestore,
        existing_root: super::NativeSurfaceBinding,
        incoming_root: SurfaceId,
    ) -> Result<(), super::DockspaceRuntimeError> {
        self.submit_native_document_restore_with_roots(restore, [(existing_root, incoming_root)])
    }

    /// Restores native hosting through explicit existing-window to incoming-Root mappings.
    ///
    /// Each pair is `(existing_root_binding, incoming_root_surface)`. Supply
    /// every external Root required by the document. Sources and targets must
    /// be unique; participating windows may exchange surface identities. All
    /// continuations, document state and Child creation intents publish in one
    /// transaction. A saved Child recovers only to its declared mapped Root.
    ///
    /// Hosts may prepare physical routes from the proposed continuations, but
    /// install them only after the matching commit. Fresh native facts and
    /// presented output are required before successor input becomes available.
    ///
    /// # Errors
    ///
    /// Rejects ordinary restore failures, incomplete hosting declarations,
    /// duplicate mappings, stale or foreign predecessors, unavailable Roots
    /// and targets occupied by a window outside this mapping. A failed or
    /// dropped frame preserves the document, bindings and preparation.
    pub fn submit_native_document_restore_with_roots(
        &mut self,
        restore: &PreparedDockspaceDocumentRestore,
        mappings: impl IntoIterator<Item = (super::NativeSurfaceBinding, SurfaceId)>,
    ) -> Result<(), super::DockspaceRuntimeError> {
        self.session.ensure_managed_native_host_available()?;
        let mappings = mappings.into_iter().collect::<Vec<_>>();
        let roots = mappings
            .iter()
            .map(|(_, surface)| *surface)
            .collect::<Vec<_>>();
        restore.validate_native_surface_hosting_for_roots(roots.iter().copied())?;
        self.submit_document_restore_with_native_roots(restore, Some(&roots), &mappings)
    }

    fn submit_document_restore_with_native_roots(
        &mut self,
        restore: &PreparedDockspaceDocumentRestore,
        native_roots: Option<&[SurfaceId]>,
        mappings: &[(super::NativeSurfaceBinding, SurfaceId)],
    ) -> Result<(), super::DockspaceRuntimeError> {
        self.ensure_replacement_input_allowed()?;
        self.session
            .ensure_prepared_document_restore_current(restore)?;
        self.complete_pointer_input()?;
        let source_scope = self
            .session
            .document
            .as_ref()
            .ok_or_else(DockspacePersistenceError::not_configured)?
            .item_identity_scope();
        let target_scope = restore.inner.item_identity_scope();
        let (mut input, pending) = restore
            .inner
            .clone()
            .into_engine_input(self.frame.view())
            .map_err(DockspacePersistenceError::session)?;
        if let Some(roots) = native_roots {
            let crate::engine::EngineInput::RestoreWorkspace(restored) = input else {
                unreachable!("validated document preparation produces a restore input");
            };
            let plan = native_surface_restore_plan(
                DockspaceView::new(restored.workspace()),
                surface_presentation_preferences(Some(pending.viewport_placements())),
                roots.iter().copied(),
            )?;
            let continuations =
                self.prepare_native_root_continuations(mappings, restored.workspace())?;
            let viewport = self.frame.view().viewport();
            for &root in &plan.roots {
                if mappings.is_empty()
                    && !viewport.viewport(root).is_some_and(|record| {
                        record.role() == crate::viewport::ViewportRole::Root
                            && record.ownership()
                                == crate::viewport_registry::ViewportOwnership::External
                            && record.admission()
                                == crate::viewport_registry::ViewportAdmission::Admitted
                            && !matches!(
                                record.lifecycle(),
                                crate::viewport_registry::ViewportLifecycle::CloseRequested
                                    | crate::viewport_registry::ViewportLifecycle::AwaitingDestroyed
                                    | crate::viewport_registry::ViewportLifecycle::Missing
                                    | crate::viewport_registry::ViewportLifecycle::Destroyed
                            )
                    })
                {
                    return Err(DockspaceNativeDocumentHostingError::RootWindowUnavailable {
                        surface: root,
                    }
                    .into());
                }
            }
            for child in &plan.children {
                if viewport.viewport(child.surface).is_some_and(|record| {
                    record.role() == crate::viewport::ViewportRole::Root
                        && !mappings.iter().any(|(existing, _)| {
                            existing.matches_viewport_binding(record.binding())
                        })
                }) {
                    return Err(DockspaceNativeDocumentHostingError::ChildAlreadyHosted {
                        surface: child.surface,
                    }
                    .into());
                }
            }
            let restored = restored
                .with_native_surfaces(plan)
                .with_root_continuations(continuations);
            input = crate::engine::EngineInput::RestoreWorkspace(restored);
        }
        let sequence = self.next_application_source_sequence()?;
        self.frame.append_document_replacement(
            super::host_frame::APPLICATION_INPUT_SOURCE,
            sequence,
            input,
            source_scope.as_ref(),
            target_scope,
        )?;
        self.document_restore = Some(pending);
        self.replacement_submitted = true;
        Ok(())
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
    /// The outer document schema uses an unsupported version.
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
                DockspaceDocumentDecodeError::UnsupportedVersion { .. },
            ) => DockspacePersistenceErrorKind::UnsupportedVersion,
            DockspacePersistenceErrorSource::Restore(
                DockspaceDocumentRestoreError::ExternalItemAssociationsRejected { .. },
            ) => DockspacePersistenceErrorKind::IdentityConflict,
            DockspacePersistenceErrorSource::DecodeJson(_)
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

    pub(super) fn capture(error: DockspaceDocumentCaptureError) -> Self {
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
    ExternalItemKey(#[source] ExternalItemKeyMapError),
    #[error("dockspace document capture failed: {0}")]
    Capture(#[source] DockspaceDocumentCaptureError),
    #[error("dockspace document JSON encoding failed: {0}")]
    EncodeJson(#[source] serde_json::Error),
    #[error("dockspace document JSON decoding failed: {0}")]
    DecodeJson(#[source] serde_json::Error),
    #[error("dockspace document version decoding failed: {0}")]
    Decode(#[source] DockspaceDocumentDecodeError),
    #[error("dockspace document validation failed: {0}")]
    Restore(#[source] DockspaceDocumentRestoreError),
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
