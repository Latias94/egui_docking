//! Atomic persistence documents that bind a workspace to its external item identities.
//!
//! A [`DockspaceDocument`] is the only durable boundary for a workspace and
//! [`ExternalItemKeyMap`]. Separate snapshots are intentionally not sufficient:
//! a valid workspace can otherwise be paired with an unrelated valid key map
//! and silently resolve a numeric [`ItemId`] to the wrong application pane.

use std::collections::BTreeSet;
use std::fmt;
use std::sync::Arc;

use blake3::Hasher;
use serde::de::{Error as _, IgnoredAny, SeqAccess, Visitor};
use serde::ser::SerializeTuple;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use thiserror::Error;

use crate::engine::{
    CoreHostPresentationFrame, EngineError, HostFrameView, OwnedPreparedHostFrameCommit,
};
use crate::engine::{DockEngine, EngineInput, ValidatedWorkspaceRestore};
use crate::external_item_key::{
    ExternalItemKeyMap, ExternalItemKeyMapError, ExternalItemKeyReconcileError,
    ExternalItemKeyRestoreError, ExternalItemKeySnapshot, ExternalItemKeySnapshotEnvelope,
};
use crate::graph::Workspace;
use crate::ids::{EngineAuthorityDomainId, ItemId, PresentationIdentityFrontier, SurfaceId};
use crate::persistence::{
    SnapshotCaptureError, SnapshotNode, SnapshotRestoreError, WorkspaceSnapshot,
    WorkspaceSnapshotEnvelope,
};
use crate::transition::{EngineTransition, InputOutcome, WorkspaceVersion};
use crate::viewport::ViewportRole;
use crate::viewport_persistence::{
    NativeSurfaceHostPreference, ViewportPlacementPreference, ViewportPlacementPreferences,
    ViewportPlacementRestoreError, ViewportPlacementSnapshot, ViewportPlacementSnapshotEnvelope,
};
use crate::viewport_registry::ViewportAdmission;

/// The document schema version emitted and accepted by this crate release.
pub const DOCKSPACE_DOCUMENT_VERSION: u32 = 2;

const BINDING_HASH_DOMAIN: &[u8] = b"dockspace-document/v2\0";

/// Stable identity of one persisted document lineage.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Deserialize, Serialize)]
#[serde(transparent)]
pub struct DockspaceDocumentId([u8; 16]);

impl DockspaceDocumentId {
    /// Creates an opaque document identity from application-owned bytes.
    #[must_use]
    pub const fn from_bytes(bytes: [u8; 16]) -> Self {
        Self(bytes)
    }

    /// Returns the exact persisted document identity bytes.
    #[must_use]
    pub const fn as_bytes(self) -> [u8; 16] {
        self.0
    }
}

/// A complete atomic persistence boundary for docking state and pane identity.
///
/// The binding hash detects accidental splice, corruption, and cross-generation
/// mixing. It is an integrity check, not a signature or authentication scheme.
/// Untrusted bytes must be decoded through [`DockspaceDocumentEnvelope`] so
/// nested snapshot-version failures remain typed [`DockspaceDocumentDecodeError`]
/// values instead of being flattened into a serializer error.
#[derive(Clone, Debug, PartialEq)]
pub struct DockspaceDocument {
    document_id: DockspaceDocumentId,
    generation: u64,
    presentation_identity_frontier: PresentationIdentityFrontier,
    workspace: WorkspaceSnapshot,
    external_item_keys: ExternalItemKeySnapshot,
    viewport_placements: ViewportPlacementSnapshot,
    binding_hash: [u8; 32],
}

impl DockspaceDocument {
    pub(crate) fn capture_state(
        document_id: DockspaceDocumentId,
        generation: u64,
        engine: &DockEngine,
        external_item_keys: &ExternalItemKeyMap,
        viewport_placements: &ViewportPlacementPreferences,
    ) -> Result<Self, DockspaceDocumentCaptureError> {
        let workspace = WorkspaceSnapshot::capture(engine.workspace())?;
        let mut presentation_identity_frontier = engine.presentation_identity_frontier();
        observe_snapshot_identities(&workspace, &mut presentation_identity_frontier);
        validate_snapshot_item_bindings(&workspace, external_item_keys)
            .map_err(DockspaceDocumentCaptureError::MissingExternalItemKey)?;
        validate_viewport_placement_surfaces(&workspace, viewport_placements)
            .map_err(DockspaceDocumentCaptureError::UnknownPlacementSurface)?;
        validate_native_host_preferences(&workspace, viewport_placements)?;
        let external_item_keys = ExternalItemKeySnapshot::capture(external_item_keys);
        let viewport_placements = ViewportPlacementSnapshot::capture(viewport_placements);
        let binding_hash = binding_hash(
            document_id,
            generation,
            presentation_identity_frontier,
            &workspace,
            &external_item_keys,
            &viewport_placements,
        )?;

        Ok(Self {
            document_id,
            generation,
            presentation_identity_frontier,
            workspace,
            external_item_keys,
            viewport_placements,
            binding_hash,
        })
    }

    /// Returns this document's stable lineage identity.
    #[must_use]
    pub const fn document_id(&self) -> DockspaceDocumentId {
        self.document_id
    }

    pub(crate) fn restore(
        self,
        recognize_external_item: impl Fn(DockspaceDocumentId, &str) -> bool,
    ) -> Result<RestoredDockspaceDocument, DockspaceDocumentRestoreError> {
        let actual_hash = binding_hash(
            self.document_id,
            self.generation,
            self.presentation_identity_frontier,
            &self.workspace,
            &self.external_item_keys,
            &self.viewport_placements,
        )?;
        if actual_hash != self.binding_hash {
            return Err(DockspaceDocumentRestoreError::BindingHashMismatch);
        }

        let external_item_keys = self.external_item_keys.restore()?;
        let viewport_placements = self.viewport_placements.restore()?;
        let workspace = self.workspace.build_candidate(|_| true)?;
        validate_identity_frontier(&workspace, self.presentation_identity_frontier)?;
        validate_snapshot_item_bindings(&self.workspace, &external_item_keys)
            .map_err(DockspaceDocumentRestoreError::MissingExternalItemKey)?;
        validate_external_item_associations(
            self.document_id,
            &external_item_keys,
            recognize_external_item,
        )?;
        validate_viewport_placement_surfaces(&self.workspace, &viewport_placements)
            .map_err(DockspaceDocumentRestoreError::UnknownPlacementSurface)?;
        validate_native_host_preferences(&self.workspace, &viewport_placements)?;

        Ok(RestoredDockspaceDocument {
            document_id: self.document_id,
            generation: self.generation,
            restore: ValidatedWorkspaceRestore::new(workspace, self.presentation_identity_frontier),
            external_item_keys,
            viewport_placements,
        })
    }
}

#[derive(Serialize)]
struct DockspaceDocumentPayloadRef<'document> {
    document_id: DockspaceDocumentId,
    generation: u64,
    presentation_identity_frontier: PresentationIdentityFrontier,
    workspace: &'document WorkspaceSnapshot,
    external_item_keys: &'document ExternalItemKeySnapshot,
    viewport_placements: &'document ViewportPlacementSnapshot,
    binding_hash: [u8; 32],
}

impl Serialize for DockspaceDocument {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut document = serializer.serialize_tuple(2)?;
        document.serialize_element(&DOCKSPACE_DOCUMENT_VERSION)?;
        document.serialize_element(&DockspaceDocumentPayloadRef {
            document_id: self.document_id,
            generation: self.generation,
            presentation_identity_frontier: self.presentation_identity_frontier,
            workspace: &self.workspace,
            external_item_keys: &self.external_item_keys,
            viewport_placements: &self.viewport_placements,
            binding_hash: self.binding_hash,
        })?;
        document.end()
    }
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
struct DockspaceDocumentV2 {
    document_id: DockspaceDocumentId,
    generation: u64,
    presentation_identity_frontier: PresentationIdentityFrontierWire,
    workspace: WorkspaceSnapshotEnvelope,
    external_item_keys: ExternalItemKeySnapshotEnvelope,
    viewport_placements: ViewportPlacementSnapshotEnvelope,
    binding_hash: [u8; 32],
}

impl DockspaceDocumentV2 {
    fn into_document(self) -> Result<DockspaceDocument, DockspaceDocumentDecodeError> {
        Ok(DockspaceDocument {
            document_id: self.document_id,
            generation: self.generation,
            presentation_identity_frontier: self.presentation_identity_frontier.into(),
            workspace: self.workspace.into_snapshot()?,
            external_item_keys: self.external_item_keys.into_snapshot()?,
            viewport_placements: self.viewport_placements.into_snapshot()?,
            binding_hash: self.binding_hash,
        })
    }
}

/// Version-first envelope for an untrusted dockspace document.
#[derive(Clone, Debug, PartialEq)]
pub struct DockspaceDocumentEnvelope {
    version: u32,
    payload: Option<DockspaceDocumentV2>,
}

impl DockspaceDocumentEnvelope {
    /// Returns the strictly decoded supported document.
    ///
    /// # Errors
    ///
    /// Returns [`DockspaceDocumentDecodeError::UnsupportedVersion`] for every
    /// outer version other than [`DOCKSPACE_DOCUMENT_VERSION`].
    pub fn into_document(self) -> Result<DockspaceDocument, DockspaceDocumentDecodeError> {
        self.payload.map_or_else(
            || {
                Err(DockspaceDocumentDecodeError::UnsupportedVersion {
                    found: self.version,
                    supported: DOCKSPACE_DOCUMENT_VERSION,
                })
            },
            DockspaceDocumentV2::into_document,
        )
    }
}

impl<'de> Deserialize<'de> for DockspaceDocumentEnvelope {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_tuple(2, DockspaceDocumentEnvelopeVisitor)
    }
}

struct DockspaceDocumentEnvelopeVisitor;

impl<'de> Visitor<'de> for DockspaceDocumentEnvelopeVisitor {
    type Value = DockspaceDocumentEnvelope;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a [version, payload] dockspace document")
    }

    fn visit_seq<A>(self, mut sequence: A) -> Result<Self::Value, A::Error>
    where
        A: SeqAccess<'de>,
    {
        let version = sequence
            .next_element::<u32>()?
            .ok_or_else(|| A::Error::invalid_length(0, &self))?;
        let payload = if version == DOCKSPACE_DOCUMENT_VERSION {
            let payload = sequence
                .next_element::<DockspaceDocumentV2>()?
                .ok_or_else(|| A::Error::invalid_length(1, &self))?;
            Some(payload)
        } else {
            sequence
                .next_element::<IgnoredAny>()?
                .ok_or_else(|| A::Error::invalid_length(1, &self))?;
            None
        };
        if sequence.next_element::<IgnoredAny>()?.is_some() {
            return Err(A::Error::invalid_length(3, &self));
        }
        Ok(DockspaceDocumentEnvelope { version, payload })
    }
}

#[derive(Clone, PartialEq)]
pub(crate) struct BoundDocumentState {
    document_id: DockspaceDocumentId,
    next_generation: Option<u64>,
    external_item_keys: ExternalItemKeyMap,
    item_identity_scope: Arc<BTreeSet<ItemId>>,
    viewport_placements: ViewportPlacementPreferences,
}

impl fmt::Debug for BoundDocumentState {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("BoundDocumentState")
            .field("document_id", &self.document_id)
            .field("next_generation", &self.next_generation)
            .field("external_item_count", &self.external_item_keys.len())
            .field("viewport_placement_count", &self.viewport_placements.len())
            .finish()
    }
}

impl BoundDocumentState {
    fn from_parts(
        document_id: DockspaceDocumentId,
        next_generation: Option<u64>,
        external_item_keys: ExternalItemKeyMap,
        viewport_placements: ViewportPlacementPreferences,
    ) -> Self {
        let item_identity_scope = collect_item_identity_scope(&external_item_keys);
        Self {
            document_id,
            next_generation,
            external_item_keys,
            item_identity_scope,
            viewport_placements,
        }
    }

    pub(crate) fn from_bootstrap_workspace(
        workspace: &Workspace,
        bootstrap: DockspaceDocumentBootstrap,
    ) -> Result<Self, DockspaceDocumentCaptureError> {
        validate_workspace_item_bindings(workspace, &bootstrap.external_item_keys)
            .map_err(DockspaceDocumentCaptureError::MissingExternalItemKey)?;
        Ok(Self::from_parts(
            bootstrap.document_id,
            Some(0),
            bootstrap.external_item_keys,
            ViewportPlacementPreferences::new(),
        ))
    }

    pub(crate) const fn document_id(&self) -> DockspaceDocumentId {
        self.document_id
    }

    pub(crate) const fn next_generation(&self) -> Option<u64> {
        self.next_generation
    }

    pub(crate) fn viewport_placements(&self) -> &ViewportPlacementPreferences {
        &self.viewport_placements
    }

    pub(crate) fn item_id(&self, external_key: &str) -> Option<ItemId> {
        self.external_item_keys.item_id(external_key)
    }

    pub(crate) fn external_item_key(&self, item: ItemId) -> Option<&str> {
        self.external_item_keys.external_key(item)
    }

    pub(crate) fn ensure_external_item_key(
        &mut self,
        external_key: impl Into<String>,
    ) -> Result<ItemId, ExternalItemKeyMapError> {
        let item = self.external_item_keys.ensure(external_key)?;
        Arc::make_mut(&mut self.item_identity_scope).insert(item);
        Ok(item)
    }

    pub(crate) fn ensure_external_item_keys<I, K>(
        &mut self,
        external_keys: I,
    ) -> Result<(), ExternalItemKeyMapError>
    where
        I: IntoIterator<Item = K>,
        K: Into<String>,
    {
        let previous_len = self.external_item_keys.len();
        self.external_item_keys.ensure_all(external_keys)?;
        if self.external_item_keys.len() != previous_len {
            Arc::make_mut(&mut self.item_identity_scope)
                .extend(self.external_item_keys.iter().map(|(_, item)| item));
        }
        Ok(())
    }

    pub(crate) fn item_identity_scope(&self) -> Arc<BTreeSet<ItemId>> {
        Arc::clone(&self.item_identity_scope)
    }

    pub(crate) fn prepare_publication(
        &self,
        engine: &DockEngine,
    ) -> Result<Self, DockspaceDocumentCaptureError> {
        let viewport_placements = self.reconciled_native_host_preferences(engine)?;
        let mut binding = self.clone();
        binding.viewport_placements = viewport_placements;
        Ok(binding)
    }

    pub(crate) fn prepare_capture(
        &self,
        engine: &DockEngine,
    ) -> Result<PreparedDockspaceDocumentCapture, DockspaceDocumentCaptureError> {
        let viewport_placements = self.reconciled_viewport_placements(engine)?;
        let generation = self
            .next_generation
            .ok_or(DockspaceDocumentCaptureError::GenerationExhausted)?;
        let document = DockspaceDocument::capture_state(
            self.document_id,
            generation,
            engine,
            &self.external_item_keys,
            &viewport_placements,
        )?;
        Ok(PreparedDockspaceDocumentCapture {
            document,
            viewport_placements,
            next_generation: generation.checked_add(1),
        })
    }

    pub(crate) fn prepare_runtime_restore(
        &self,
        engine: &DockEngine,
        document: DockspaceDocument,
        recognize_external_item: impl Fn(DockspaceDocumentId, &str) -> bool,
    ) -> Result<PreparedRuntimeDocumentRestore, RuntimeDocumentRestoreError> {
        if document.document_id() != self.document_id {
            return Err(RuntimeDocumentRestoreError::WrongLineage {
                expected: self.document_id,
                found: document.document_id(),
            });
        }
        let restored = document.restore(recognize_external_item)?;
        let external_item_keys = self
            .external_item_keys
            .reconciled_with(&restored.external_item_keys)?;
        let next_generation =
            merge_next_generation(self.next_generation, restored.generation.checked_add(1));
        let item_identity_scope = collect_item_identity_scope(&external_item_keys);
        Ok(PreparedRuntimeDocumentRestore {
            restored,
            external_item_keys,
            next_generation,
            item_identity_scope,
            frame_start_version: engine.version(),
            source_authority_domain: engine.authority_domain(),
            source_document: self.clone(),
        })
    }

    pub(crate) fn commit_capture(
        &mut self,
        prepared: PreparedDockspaceDocumentCapture,
    ) -> DockspaceDocument {
        let PreparedDockspaceDocumentCapture {
            document,
            viewport_placements,
            next_generation,
        } = prepared;
        self.viewport_placements = viewport_placements;
        self.next_generation = next_generation;
        document
    }

    fn reconciled_native_host_preferences(
        &self,
        engine: &DockEngine,
    ) -> Result<ViewportPlacementPreferences, DockspaceDocumentCaptureError> {
        let mut placements = self.viewport_placements.clone();
        let mut surfaces = BTreeSet::new();
        let mut owned_surfaces = BTreeSet::new();
        for (surface, _) in engine.workspace().surfaces() {
            surfaces.insert(surface);
            let identity = engine
                .viewport()
                .viewport(surface)
                .map(|record| (record.role(), record.binding()))
                .or_else(|| {
                    engine
                        .viewport()
                        .recovery_pending(surface)
                        .map(|pending| (pending.role(), pending.source_binding()))
                });
            let Some((role, binding)) = identity else {
                if engine.root_recovery_anchor(surface).is_some_and(|anchor| {
                    anchor.authority_domain() == engine.authority_domain()
                        && anchor.surface() == surface
                }) {
                    // Root anchors survive physical destruction while their
                    // application-owned logical surface remains in the graph.
                    placements.set_native_host(surface, NativeSurfaceHostPreference::Root);
                }
                continue;
            };
            owned_surfaces.insert(surface);
            let native_host = match role {
                ViewportRole::Root => NativeSurfaceHostPreference::Root,
                ViewportRole::Child => NativeSurfaceHostPreference::Child {
                    recovery_host: engine.native_child_recovery_host(binding).ok_or(
                        DockspaceDocumentCaptureError::MissingChildRecoveryHost { surface },
                    )?,
                },
            };
            placements.set_native_host(surface, native_host);
        }
        // Native-create and cleanup owners can retain bindings before their
        // prospective surfaces acquire durable graph membership.
        if let Some(surface) = engine
            .viewport()
            .registry()
            .records()
            .filter(|(_, record)| !engine.viewport().is_staging_binding(record.binding()))
            .map(|(surface, _)| surface)
            .chain(
                engine
                    .viewport()
                    .pending_recoveries()
                    .map(|(surface, _)| surface),
            )
            .find(|surface| !surfaces.contains(surface))
        {
            return Err(DockspaceDocumentCaptureError::UnknownPlacementSurface(
                surface,
            ));
        }
        placements.retain_surfaces(&surfaces);
        // Inspect the complete pre-clear relation so a recovery chain cannot
        // acquire a different meaning merely because its host sorts first.
        let mut clear_hosting = Vec::new();
        for surface in placements.surface_ids() {
            let Some(error) = native_host_preference_error(surface, &surfaces, &placements) else {
                continue;
            };
            if owned_surfaces.contains(&surface) {
                return Err(error.into());
            }
            clear_hosting.push(surface);
        }
        for surface in clear_hosting {
            placements.clear_native_host(surface);
        }
        Ok(placements)
    }

    fn reconciled_viewport_placements(
        &self,
        engine: &DockEngine,
    ) -> Result<ViewportPlacementPreferences, DockspaceDocumentCaptureError> {
        let mut placements = self.reconciled_native_host_preferences(engine)?;
        for (surface, _) in engine.workspace().surfaces() {
            let Some(record) = engine.viewport().viewport(surface) else {
                continue;
            };
            if record.admission() != ViewportAdmission::Admitted
                || !record.has_coordinate_authority()
            {
                continue;
            }
            let Some(coordinates) = record.coordinates() else {
                continue;
            };
            let Some(outer_rect) = coordinates.outer_bounds() else {
                continue;
            };
            let presentation = placements
                .get(surface)
                .and_then(|preference| preference.presentation());
            let Ok(preference) = ViewportPlacementPreference::new(surface, outer_rect) else {
                continue;
            };
            let Ok(mut preference) = preference.try_with_inner_size(coordinates.content_size())
            else {
                continue;
            };
            preference = preference.with_scale_factor(coordinates.native_scale_factor());
            if let Some(presentation) = presentation {
                preference = preference.with_presentation(presentation);
            }
            placements.set(preference);
        }
        Ok(placements)
    }
}

pub(crate) struct PreparedDockspaceDocumentCapture {
    document: DockspaceDocument,
    viewport_placements: ViewportPlacementPreferences,
    next_generation: Option<u64>,
}

#[derive(Clone, Debug)]
pub(crate) struct PreparedRuntimeDocumentRestore {
    restored: RestoredDockspaceDocument,
    external_item_keys: ExternalItemKeyMap,
    next_generation: Option<u64>,
    item_identity_scope: Arc<BTreeSet<ItemId>>,
    frame_start_version: WorkspaceVersion,
    source_authority_domain: EngineAuthorityDomainId,
    source_document: BoundDocumentState,
}

#[derive(Debug)]
pub(crate) struct PendingRuntimeDocumentRestore {
    binding: BoundDocumentState,
    source_document: BoundDocumentState,
    expected_workspace: WorkspaceSnapshot,
    expected_frontier: PresentationIdentityFrontier,
    frame_start_version: WorkspaceVersion,
    replacement_base_version: WorkspaceVersion,
    source_authority_domain: EngineAuthorityDomainId,
}

impl PreparedRuntimeDocumentRestore {
    pub(crate) fn workspace(&self) -> &Workspace {
        self.restored.restore.workspace()
    }

    pub(crate) fn viewport_placements(&self) -> &ViewportPlacementPreferences {
        &self.restored.viewport_placements
    }

    pub(crate) fn item_identity_scope(&self) -> Arc<BTreeSet<ItemId>> {
        Arc::clone(&self.item_identity_scope)
    }

    pub(crate) fn is_current_for(
        &self,
        engine: &DockEngine,
        document: &BoundDocumentState,
    ) -> bool {
        self.frame_start_version == engine.version()
            && self.source_authority_domain == engine.authority_domain()
            && self.source_document == *document
    }

    #[cfg(test)]
    pub(crate) fn into_engine_input(
        self,
        frame: HostFrameView<'_>,
    ) -> Result<(EngineInput, PendingRuntimeDocumentRestore), RuntimeDocumentRestoreError> {
        self.into_engine_input_tracking_surface(frame, None)
            .map(|(input, pending, _)| (input, pending))
    }

    pub(crate) fn into_engine_input_tracking_surface(
        self,
        frame: HostFrameView<'_>,
        tracked_surface: Option<SurfaceId>,
    ) -> Result<
        (
            EngineInput,
            PendingRuntimeDocumentRestore,
            Option<SurfaceId>,
        ),
        RuntimeDocumentRestoreError,
    > {
        if frame.authority_domain() != self.source_authority_domain
            || tracked_surface.is_some_and(|surface| self.workspace().surface(surface).is_none())
        {
            return Err(RuntimeDocumentRestoreError::EnginePublicationMismatch);
        }
        let expected_workspace = WorkspaceSnapshot::capture(self.restored.restore.workspace())?;
        let mut expected_frontier = frame.presentation_identity_frontier();
        expected_frontier.merge(self.restored.restore.presentation_identity_frontier());
        let replacement_base_version = frame.version();
        let RestoredDockspaceDocument {
            document_id,
            generation: _,
            restore,
            external_item_keys: _,
            viewport_placements,
        } = self.restored;
        // The replacement outcome and publication proof must carry the same
        // merged allocation history even when restoring an older document.
        let (workspace, _) = restore.into_parts();
        let restore = ValidatedWorkspaceRestore::new(workspace, expected_frontier);
        let pending = PendingRuntimeDocumentRestore {
            binding: BoundDocumentState {
                document_id,
                next_generation: self.next_generation,
                external_item_keys: self.external_item_keys,
                item_identity_scope: self.item_identity_scope,
                viewport_placements,
            },
            source_document: self.source_document,
            expected_workspace,
            expected_frontier,
            frame_start_version: self.frame_start_version,
            replacement_base_version,
            source_authority_domain: self.source_authority_domain,
        };
        Ok((
            EngineInput::RestoreWorkspace(restore),
            pending,
            tracked_surface,
        ))
    }
}

impl PendingRuntimeDocumentRestore {
    pub(crate) fn is_current_for(
        &self,
        engine: &DockEngine,
        document: Option<&BoundDocumentState>,
    ) -> bool {
        self.frame_start_version == engine.version()
            && self.source_authority_domain == engine.authority_domain()
            && document.is_some_and(|document| self.source_document == *document)
    }

    pub(crate) fn viewport_placements(&self) -> &ViewportPlacementPreferences {
        self.binding.viewport_placements()
    }

    pub(crate) fn external_item_key(&self, item: ItemId) -> Option<&str> {
        self.binding.external_item_key(item)
    }

    pub(crate) fn prepare_publication(
        self,
        frame: CoreHostPresentationFrame,
        engine: &DockEngine,
    ) -> Result<PreparedRuntimeDocumentPublication, RuntimeDocumentRestoreError> {
        if engine.authority_domain() != self.source_authority_domain {
            return Err(RuntimeDocumentRestoreError::EnginePublicationMismatch);
        }
        let expected_scope = self.binding.item_identity_scope();
        if !frame.item_identity_scope_matches(expected_scope.as_ref()) {
            return Err(RuntimeDocumentRestoreError::EnginePublicationMismatch);
        }
        let core = frame.prepare_owned(engine)?;
        if WorkspaceSnapshot::capture(core.candidate_workspace())? != self.expected_workspace
            || core.candidate_presentation_identity_frontier() != self.expected_frontier
            || !transition_authorizes_document_restore(
                core.transition(),
                self.source_authority_domain,
                self.frame_start_version,
                self.replacement_base_version,
                self.expected_frontier,
            )
        {
            return Err(RuntimeDocumentRestoreError::EnginePublicationMismatch);
        }
        let binding = self.binding.prepare_publication(core.candidate_engine())?;
        Ok(PreparedRuntimeDocumentPublication { core, binding })
    }
}

#[must_use = "a prepared runtime document publication must be committed or dropped"]
pub(crate) struct PreparedRuntimeDocumentPublication {
    core: OwnedPreparedHostFrameCommit,
    binding: BoundDocumentState,
}

impl PreparedRuntimeDocumentPublication {
    pub(crate) const fn candidate_engine(&self) -> &DockEngine {
        self.core.candidate_engine()
    }

    pub(crate) fn has_new_undelivered_platform_effects(&self, engine: &DockEngine) -> bool {
        self.core.has_new_undelivered_platform_effects(engine)
    }
    pub(crate) const fn transition(&self) -> &EngineTransition {
        self.core.transition()
    }

    pub(crate) fn commit(
        self,
        engine: &mut DockEngine,
    ) -> Result<(EngineTransition, BoundDocumentState), EngineError> {
        let transition = self.core.commit(engine)?;
        Ok((transition, self.binding))
    }
}

impl PreparedDockspaceDocumentCapture {
    pub(crate) const fn document(&self) -> &DockspaceDocument {
        &self.document
    }
}

/// One-time identity bootstrap consumed by the renderer-neutral runtime session.
///
/// Applications mint every [`ItemId`] from its external key here, then build the
/// workspace with those returned identities. Consuming the bootstrap into the
/// runtime preserves that exact map owner; no arbitrary pre-paired map enters
/// the normal persistence path.
#[derive(Debug)]
pub(crate) struct DockspaceDocumentBootstrap {
    document_id: DockspaceDocumentId,
    external_item_keys: ExternalItemKeyMap,
}

impl DockspaceDocumentBootstrap {
    /// Starts an empty persistence identity bootstrap.
    #[must_use]
    pub(crate) const fn new(document_id: DockspaceDocumentId) -> Self {
        Self {
            document_id,
            external_item_keys: ExternalItemKeyMap::new(),
        }
    }

    /// Returns the existing item for one external key or mints a fresh identity.
    ///
    /// # Errors
    ///
    /// Returns a typed allocation error without changing the bootstrap on failure.
    pub(crate) fn ensure_external_item_key(
        &mut self,
        external_key: impl Into<String>,
    ) -> Result<ItemId, ExternalItemKeyMapError> {
        self.external_item_keys.ensure(external_key)
    }

    /// Atomically mints a batch of external item identities.
    ///
    /// # Errors
    ///
    /// Returns a typed allocation error without partial assignment.
    pub(crate) fn ensure_external_item_keys<I, K>(
        &mut self,
        external_keys: I,
    ) -> Result<(), ExternalItemKeyMapError>
    where
        I: IntoIterator<Item = K>,
        K: Into<String>,
    {
        self.external_item_keys.ensure_all(external_keys)
    }

    /// Resolves one identity minted by this bootstrap.
    #[must_use]
    pub(crate) fn item_id(&self, external_key: &str) -> Option<ItemId> {
        self.external_item_keys.item_id(external_key)
    }
}

fn merge_next_generation(active: Option<u64>, incoming: Option<u64>) -> Option<u64> {
    match (active, incoming) {
        (Some(active), Some(incoming)) => Some(active.max(incoming)),
        (None, _) | (_, None) => None,
    }
}

fn collect_item_identity_scope(external_item_keys: &ExternalItemKeyMap) -> Arc<BTreeSet<ItemId>> {
    Arc::new(external_item_keys.iter().map(|(_, item)| item).collect())
}

#[derive(Clone, Debug)]
pub(crate) struct RestoredDockspaceDocument {
    document_id: DockspaceDocumentId,
    generation: u64,
    restore: ValidatedWorkspaceRestore,
    external_item_keys: ExternalItemKeyMap,
    viewport_placements: ViewportPlacementPreferences,
}

impl RestoredDockspaceDocument {
    pub(crate) fn into_runtime_parts(self) -> (ValidatedWorkspaceRestore, BoundDocumentState) {
        let binding = BoundDocumentState::from_parts(
            self.document_id,
            self.generation.checked_add(1),
            self.external_item_keys,
            self.viewport_placements,
        );
        (self.restore, binding)
    }
}

/// Serialized representation used only while decoding an untrusted document.
///
/// `PresentationIdentityFrontier` deliberately has no public deserializer or
/// constructor. The document validator is the only boundary allowed to turn
/// persisted counters back into a core allocator token.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
struct PresentationIdentityFrontierWire {
    last_surface: u64,
    last_root: u64,
    last_floating: u64,
}

impl From<PresentationIdentityFrontierWire> for PresentationIdentityFrontier {
    fn from(value: PresentationIdentityFrontierWire) -> Self {
        Self::from_counters(value.last_surface, value.last_root, value.last_floating)
    }
}

/// Failure while capturing a dockspace document.
#[derive(Debug, Error)]
pub enum DockspaceDocumentCaptureError {
    /// The live workspace cannot produce a strict renderer-neutral snapshot.
    #[error("workspace snapshot capture failed: {0}")]
    Workspace(#[from] SnapshotCaptureError),
    /// A workspace-referenced item has no stable external key assignment.
    #[error("workspace item {0} has no external key assignment")]
    MissingExternalItemKey(ItemId),
    /// A viewport preference row names a surface outside the captured workspace.
    #[error("viewport preference names unknown workspace surface {0}")]
    UnknownPlacementSurface(crate::ids::SurfaceId),
    /// A native hosting preference has an invalid recovery-host reference.
    #[error("native hosting preference is invalid: {0}")]
    NativeHostPreference(#[from] NativeSurfaceHostPreferenceError),
    /// A current child binding has no exact core-owned recovery destination.
    #[error("native child surface {surface} has no exact recovery host")]
    MissingChildRecoveryHost {
        /// Current child surface whose recovery ownership could not be verified.
        surface: crate::ids::SurfaceId,
    },
    /// The document generation cannot advance further.
    #[error("dockspace document generation is exhausted")]
    GenerationExhausted,
    /// Canonical component encoding for the binding hash failed.
    #[error("dockspace document binding hash encoding failed: {0}")]
    Hash(#[from] DockspaceDocumentHashError),
}

/// Failure while decoding an untrusted dockspace document envelope.
#[derive(Debug, Error)]
pub enum DockspaceDocumentDecodeError {
    /// The outer document schema version is not implemented by this crate release.
    #[error("unsupported dockspace document version {found}; supported version is {supported}")]
    UnsupportedVersion {
        /// Version read from the outer document envelope.
        found: u32,
        /// Version implemented by this crate release.
        supported: u32,
    },
    /// The embedded workspace snapshot version is not supported.
    #[error("embedded workspace snapshot cannot decode: {0}")]
    Workspace(#[from] SnapshotRestoreError),
    /// The embedded external item-key snapshot version is not supported.
    #[error("embedded external item-key snapshot cannot decode: {0}")]
    ExternalItemKeys(#[from] ExternalItemKeyRestoreError),
    /// The embedded viewport-placement snapshot version is not supported.
    #[error("embedded viewport placement snapshot cannot decode: {0}")]
    ViewportPlacements(#[from] ViewportPlacementRestoreError),
}

/// Failure while validating and restoring a dockspace document.
#[derive(Debug, Error)]
pub enum DockspaceDocumentRestoreError {
    /// The document's components do not match its declared binding hash.
    #[error("dockspace document binding hash does not match its content")]
    BindingHashMismatch,
    /// Canonical component encoding for the binding hash failed.
    #[error("dockspace document binding hash encoding failed: {0}")]
    Hash(#[from] DockspaceDocumentHashError),
    /// The embedded external-item-key mapping is invalid.
    #[error("external item-key snapshot restore failed: {0}")]
    ExternalItemKeys(#[from] ExternalItemKeyRestoreError),
    /// The embedded viewport-placement component is invalid.
    #[error("viewport placement snapshot restore failed: {0}")]
    ViewportPlacements(#[from] ViewportPlacementRestoreError),
    /// The embedded workspace snapshot is invalid.
    #[error("workspace snapshot restore failed: {0}")]
    Workspace(#[from] SnapshotRestoreError),
    /// The durable root allocator counter is behind a live root identity.
    #[error("document root identity frontier {frontier} is behind live root {live}")]
    RootIdentityFrontierBehind {
        /// Persisted greatest observed or retired root identity.
        frontier: u64,
        /// Greatest live root identity.
        live: u64,
    },
    /// The durable surface allocator counter is behind a live surface identity.
    #[error("document surface identity frontier {frontier} is behind live surface {live}")]
    SurfaceIdentityFrontierBehind {
        /// Persisted greatest observed or retired surface identity.
        frontier: u64,
        /// Greatest live surface identity.
        live: u64,
    },
    /// The durable floating allocator counter is behind a live presentation identity.
    #[error("document floating identity frontier {frontier} is behind live floating {live}")]
    FloatingIdentityFrontierBehind {
        /// Persisted greatest observed or retired floating identity.
        frontier: u64,
        /// Greatest live floating identity.
        live: u64,
    },
    /// A workspace-referenced item has no stable external key assignment.
    #[error("workspace item {0} has no external key assignment")]
    MissingExternalItemKey(ItemId),
    /// A viewport preference row names a surface outside the restored workspace.
    #[error("viewport preference names unknown workspace surface {0}")]
    UnknownPlacementSurface(crate::ids::SurfaceId),
    /// A native hosting preference has an invalid recovery-host reference.
    #[error("native hosting preference is invalid: {0}")]
    NativeHostPreference(#[from] NativeSurfaceHostPreferenceError),
    /// The application rejected one or more persisted item/key associations.
    #[error("the application registry rejected {rejected_count} external item association(s)", rejected_count = .rejected.len())]
    ExternalItemAssociationsRejected {
        /// Every rejected assignment in canonical external-key order.
        rejected: Vec<RejectedExternalItemAssociation>,
    },
}

/// Failure while preparing or publishing one session-owned runtime restore.
#[derive(Debug, Error)]
pub(crate) enum RuntimeDocumentRestoreError {
    #[error("dockspace document lineage mismatch: expected {expected:?}, found {found:?}")]
    WrongLineage {
        expected: DockspaceDocumentId,
        found: DockspaceDocumentId,
    },
    #[error("engine state does not match the prepared dockspace document restore")]
    EnginePublicationMismatch,
    #[error("document restore host publication failed: {0}")]
    Engine(#[from] EngineError),
    #[error("workspace snapshot capture failed: {0}")]
    Workspace(#[from] SnapshotCaptureError),
    #[error("dockspace document restore failed: {0}")]
    Restore(#[from] DockspaceDocumentRestoreError),
    #[error("external item identity reconciliation failed: {0}")]
    Reconcile(#[from] ExternalItemKeyReconcileError),
    #[error("document preferences cannot publish: {0}")]
    Publication(#[from] DockspaceDocumentCaptureError),
}

fn transition_authorizes_document_restore(
    transition: &EngineTransition,
    authority_domain: EngineAuthorityDomainId,
    frame_start_version: WorkspaceVersion,
    replacement_base_version: WorkspaceVersion,
    expected_frontier: PresentationIdentityFrontier,
) -> bool {
    let matching_restore = transition
        .reduced_inputs()
        .iter()
        .filter(|reduced| {
            matches!(
                reduced.outcome(),
                InputOutcome::WorkspaceReplaced {
                    before,
                    after,
                    restored_identity_frontier: Some(frontier),
                    ..
                } if *before == replacement_base_version
                    && *after == transition.after()
                    && *frontier == expected_frontier
            )
        })
        .count();
    transition.authority_domain() == authority_domain
        && transition.before() == frame_start_version
        && transition_document_restore_count(transition) == 1
        && matching_restore == 1
}

fn transition_document_restore_count(transition: &EngineTransition) -> usize {
    transition
        .reduced_inputs()
        .iter()
        .filter(|reduced| {
            matches!(
                reduced.outcome(),
                InputOutcome::WorkspaceReplaced {
                    restored_identity_frontier: Some(_),
                    ..
                }
            )
        })
        .count()
}

/// One persisted item/key association rejected by the application registry.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RejectedExternalItemAssociation {
    /// Stable numeric identity persisted by dockspace.
    pub item: ItemId,
    /// Exact application-owned key paired with that identity.
    pub external_key: String,
}

/// Failure while producing the canonical bytes covered by the binding hash.
#[derive(Debug, Error)]
pub enum DockspaceDocumentHashError {
    /// A strict snapshot component could not be encoded into canonical JSON bytes.
    #[error("canonical JSON encoding failed: {0}")]
    Encode(#[from] serde_json::Error),
    /// The current platform cannot represent a component byte length in the wire format.
    #[error("canonical component length {length} does not fit u64")]
    ComponentTooLarge {
        /// Component byte length reported by the host platform.
        length: usize,
    },
}

fn binding_hash(
    document_id: DockspaceDocumentId,
    generation: u64,
    presentation_identity_frontier: PresentationIdentityFrontier,
    workspace: &WorkspaceSnapshot,
    external_item_keys: &ExternalItemKeySnapshot,
    viewport_placements: &ViewportPlacementSnapshot,
) -> Result<[u8; 32], DockspaceDocumentHashError> {
    let presentation_identity_frontier = serde_json::to_vec(&presentation_identity_frontier)?;
    let workspace = serde_json::to_vec(workspace)?;
    let external_item_keys = serde_json::to_vec(external_item_keys)?;
    let viewport_placements = serde_json::to_vec(viewport_placements)?;
    let mut hasher = Hasher::new();
    hasher.update(BINDING_HASH_DOMAIN);
    hasher.update(&document_id.as_bytes());
    hasher.update(&generation.to_le_bytes());
    hash_component(&mut hasher, &presentation_identity_frontier)?;
    hash_component(&mut hasher, &workspace)?;
    hash_component(&mut hasher, &external_item_keys)?;
    hash_component(&mut hasher, &viewport_placements)?;
    Ok(*hasher.finalize().as_bytes())
}

fn observe_snapshot_identities(
    snapshot: &WorkspaceSnapshot,
    frontier: &mut PresentationIdentityFrontier,
) {
    for surface in &snapshot.surfaces {
        frontier.observe_surface(crate::ids::SurfaceId::new(surface.id));
    }
    for root in &snapshot.roots {
        frontier.observe_root(crate::ids::RootId::new(root.id));
    }
    for floating in &snapshot.contained_floatings {
        frontier.observe_floating(crate::ids::FloatingPresentationId::new(floating.id));
    }
}

fn validate_identity_frontier(
    workspace: &Workspace,
    frontier: PresentationIdentityFrontier,
) -> Result<(), DockspaceDocumentRestoreError> {
    let live_surface = workspace
        .surfaces()
        .map(|(surface, _)| surface.get())
        .max()
        .unwrap_or(0);
    if frontier.last_surface() < live_surface {
        return Err(
            DockspaceDocumentRestoreError::SurfaceIdentityFrontierBehind {
                frontier: frontier.last_surface(),
                live: live_surface,
            },
        );
    }
    let live_root = workspace
        .roots()
        .map(|(root, _)| root.get())
        .max()
        .unwrap_or(0);
    if frontier.last_root() < live_root {
        return Err(DockspaceDocumentRestoreError::RootIdentityFrontierBehind {
            frontier: frontier.last_root(),
            live: live_root,
        });
    }
    let live_floating = workspace
        .contained_floatings()
        .map(|(floating, _)| floating.get())
        .max()
        .unwrap_or(0);
    if frontier.last_floating() < live_floating {
        return Err(
            DockspaceDocumentRestoreError::FloatingIdentityFrontierBehind {
                frontier: frontier.last_floating(),
                live: live_floating,
            },
        );
    }
    Ok(())
}

fn hash_component(hasher: &mut Hasher, bytes: &[u8]) -> Result<(), DockspaceDocumentHashError> {
    let length =
        u64::try_from(bytes.len()).map_err(|_| DockspaceDocumentHashError::ComponentTooLarge {
            length: bytes.len(),
        })?;
    hasher.update(&length.to_le_bytes());
    hasher.update(bytes);
    Ok(())
}

fn snapshot_item_ids(snapshot: &WorkspaceSnapshot) -> BTreeSet<ItemId> {
    snapshot
        .nodes
        .iter()
        .filter_map(|record| match &record.node {
            SnapshotNode::Tabs {
                items,
                selected,
                mru,
            } => Some(
                items
                    .iter()
                    .copied()
                    .chain(selected.iter().copied())
                    .chain(mru.iter().copied()),
            ),
            SnapshotNode::Split { .. } => None,
        })
        .flatten()
        .map(ItemId::new)
        .collect()
}

fn validate_snapshot_item_bindings(
    snapshot: &WorkspaceSnapshot,
    external_item_keys: &ExternalItemKeyMap,
) -> Result<(), ItemId> {
    snapshot_item_ids(snapshot)
        .into_iter()
        .find(|item| external_item_keys.external_key(*item).is_none())
        .map_or(Ok(()), Err)
}

fn validate_workspace_item_bindings(
    workspace: &Workspace,
    external_item_keys: &ExternalItemKeyMap,
) -> Result<(), ItemId> {
    workspace
        .item_multiset()
        .into_keys()
        .find(|item| external_item_keys.external_key(*item).is_none())
        .map_or(Ok(()), Err)
}

fn validate_viewport_placement_surfaces(
    snapshot: &WorkspaceSnapshot,
    viewport_placements: &ViewportPlacementPreferences,
) -> Result<(), crate::ids::SurfaceId> {
    let surfaces = snapshot
        .surfaces
        .iter()
        .map(|surface| crate::ids::SurfaceId::new(surface.id))
        .collect::<BTreeSet<_>>();
    viewport_placements
        .surface_ids()
        .find(|surface| !surfaces.contains(surface))
        .map_or(Ok(()), Err)
}

/// Invalid cross-surface reference in a document-owned native hosting preference.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Error)]
pub enum NativeSurfaceHostPreferenceError {
    /// A child's recovery host is absent from the document's workspace.
    #[error("native child surface {surface} names unknown recovery host {recovery_host}")]
    UnknownRecoveryHost {
        /// Child surface whose recovery reference is invalid.
        surface: crate::ids::SurfaceId,
        /// Recovery destination absent from the workspace.
        recovery_host: crate::ids::SurfaceId,
    },
    /// A child cannot recover into itself.
    #[error("native child surface {surface} names itself as its recovery host")]
    SelfRecoveryHost {
        /// Child surface incorrectly used as its own recovery destination.
        surface: crate::ids::SurfaceId,
    },
    /// A child cannot recover into another declared child surface.
    #[error(
        "native child surface {surface} names child surface {recovery_host} as its recovery host"
    )]
    ChildRecoveryHost {
        /// Child surface whose recovery reference is invalid.
        surface: crate::ids::SurfaceId,
        /// Recovery destination declared as another child.
        recovery_host: crate::ids::SurfaceId,
    },
}

fn validate_native_host_preferences(
    snapshot: &WorkspaceSnapshot,
    viewport_placements: &ViewportPlacementPreferences,
) -> Result<(), NativeSurfaceHostPreferenceError> {
    let surfaces = snapshot
        .surfaces
        .iter()
        .map(|surface| crate::ids::SurfaceId::new(surface.id))
        .collect::<BTreeSet<_>>();
    for surface in viewport_placements.surface_ids() {
        if let Some(error) = native_host_preference_error(surface, &surfaces, viewport_placements) {
            return Err(error);
        }
    }
    Ok(())
}

fn native_host_preference_error(
    surface: SurfaceId,
    surfaces: &BTreeSet<SurfaceId>,
    viewport_placements: &ViewportPlacementPreferences,
) -> Option<NativeSurfaceHostPreferenceError> {
    let NativeSurfaceHostPreference::Child { recovery_host } =
        viewport_placements.native_host(surface)?
    else {
        return None;
    };
    if !surfaces.contains(&recovery_host) {
        return Some(NativeSurfaceHostPreferenceError::UnknownRecoveryHost {
            surface,
            recovery_host,
        });
    }
    if recovery_host == surface {
        return Some(NativeSurfaceHostPreferenceError::SelfRecoveryHost { surface });
    }
    matches!(
        viewport_placements.native_host(recovery_host),
        Some(NativeSurfaceHostPreference::Child { .. })
    )
    .then_some(NativeSurfaceHostPreferenceError::ChildRecoveryHost {
        surface,
        recovery_host,
    })
}

fn validate_external_item_associations(
    document_id: DockspaceDocumentId,
    external_item_keys: &ExternalItemKeyMap,
    recognize_external_item: impl Fn(DockspaceDocumentId, &str) -> bool,
) -> Result<(), DockspaceDocumentRestoreError> {
    let mut rejected = Vec::new();
    for (external_key, item) in external_item_keys.iter() {
        if !recognize_external_item(document_id, external_key) {
            rejected.push(RejectedExternalItemAssociation {
                item,
                external_key: external_key.to_owned(),
            });
        }
    }
    if rejected.is_empty() {
        Ok(())
    } else {
        Err(DockspaceDocumentRestoreError::ExternalItemAssociationsRejected { rejected })
    }
}

#[cfg(test)]
mod tests {
    use crate::behavior_tests::support::{self, TestPresentationHost};
    use crate::engine::{DockEngine, EngineInput};
    use crate::external_item_key::{
        ExternalItemKeyMap, ExternalItemKeyRestoreError, SnapshotExternalItemKeyEntry,
    };
    use crate::geometry::{PhysicalRect, ScaleFactor};
    use crate::graph::{Node, RootRecord, SurfacePresentation, Workspace};
    use crate::ids::{
        ItemId, PresentationIdentityFrontier, RootId, StableInputSourceId, SurfaceId,
    };
    use crate::intent::{Authority, AuthorityUnavailableReason};
    use crate::model::DockspaceView;
    use crate::platform::{
        ObservedWindow, PlatformCapabilities, PlatformCapability, PlatformSnapshot,
        PresentationEffectAcknowledgement, WindowCoordinateObservation,
        WindowPresentationObservation, WindowPresentationState,
    };
    use crate::policy::DockPolicy;
    use crate::surface_recovery::SurfaceRecoveryBootstrap;
    use crate::transition::InputOutcome;
    use crate::viewport::{
        CoordinateObservationGeneration, PlatformSnapshotGeneration,
        PresentationObservationGeneration, ViewportRole, WindowToken,
    };
    use crate::viewport_focus::{FocusObservationGeneration, unknown_focus_observation};
    use crate::viewport_persistence::{
        NativeSurfaceHostPreference, ViewportPlacementPreference, ViewportPlacementPreferences,
        ViewportPlacementSnapshot,
    };

    use super::{
        BoundDocumentState, DOCKSPACE_DOCUMENT_VERSION, DockspaceDocument,
        DockspaceDocumentCaptureError, DockspaceDocumentDecodeError, DockspaceDocumentEnvelope,
        DockspaceDocumentId, DockspaceDocumentRestoreError, NativeSurfaceHostPreferenceError,
        binding_hash,
    };

    const DOCUMENT_ID: DockspaceDocumentId = DockspaceDocumentId::from_bytes(*b"malformed-map-id");

    fn workspace() -> Workspace {
        let mut builder = Workspace::builder();
        let tabs = builder.insert_node(Node::tabs([ItemId::new(1)]));
        builder.set_root(RootId::new(1), RootRecord::new(tabs));
        builder.set_surface(
            SurfaceId::new(1),
            SurfacePresentation::with_main(RootId::new(1)),
        );
        builder.build().expect("fixture workspace must be valid")
    }

    fn item_keys() -> ExternalItemKeyMap {
        let mut keys = ExternalItemKeyMap::new();
        keys.ensure("pane/one")
            .expect("fixture key assignment must fit");
        keys
    }

    fn engine() -> DockEngine {
        DockEngine::new(workspace(), DockPolicy::default())
            .expect("fixture workspace must initialize an engine")
    }

    fn profile_engine(surfaces: &[u64]) -> DockEngine {
        let mut builder = Workspace::builder();
        for (index, &surface) in surfaces.iter().enumerate() {
            let tabs = builder.insert_node(Node::tabs([ItemId::new(index as u64 + 1)]));
            builder.set_root(RootId::new(surface), RootRecord::new(tabs));
            builder.set_surface(
                SurfaceId::new(surface),
                SurfacePresentation::with_main(RootId::new(surface)),
            );
        }
        DockEngine::new(
            builder.build().expect("profile workspace must be valid"),
            DockPolicy::default(),
        )
        .expect("profile engine must initialize")
    }

    fn profile_item_keys() -> ExternalItemKeyMap {
        let mut keys = ExternalItemKeyMap::new();
        keys.ensure_all(["pane/one", "pane/two", "pane/three"])
            .expect("profile item keys must allocate");
        keys
    }

    fn profile_preferences() -> ViewportPlacementPreferences {
        let mut preferences = ViewportPlacementPreferences::new();
        preferences.set_native_host(SurfaceId::new(1), NativeSurfaceHostPreference::Root);
        preferences.set_native_host(
            SurfaceId::new(2),
            NativeSurfaceHostPreference::Child {
                recovery_host: SurfaceId::new(1),
            },
        );
        preferences.set_native_host(SurfaceId::new(3), NativeSurfaceHostPreference::Root);
        preferences
    }

    fn profile_document(preferences: &ViewportPlacementPreferences) -> DockspaceDocument {
        DockspaceDocument::capture_state(
            DOCUMENT_ID,
            1,
            &profile_engine(&[1, 2, 3]),
            &profile_item_keys(),
            preferences,
        )
        .expect("profile document must capture")
    }

    fn rebind_hash(document: &mut DockspaceDocument) {
        document.binding_hash = binding_hash(
            document.document_id,
            document.generation,
            document.presentation_identity_frontier,
            &document.workspace,
            &document.external_item_keys,
            &document.viewport_placements,
        )
        .expect("mutated profile must remain canonically encodable");
    }

    #[test]
    fn v2_round_trip_preserves_hosting_without_placement_and_rejects_v1() {
        let preferences = profile_preferences();
        let document = profile_document(&preferences);
        let bytes = serde_json::to_vec(&document).expect("V2 document must encode");
        let decoded = serde_json::from_slice::<DockspaceDocumentEnvelope>(&bytes)
            .expect("V2 envelope must decode")
            .into_document()
            .expect("V2 document must be supported");
        let restored = decoded
            .restore(|_, _| true)
            .expect("V2 profile must restore");
        assert_eq!(restored.viewport_placements, preferences);
        assert_eq!(restored.viewport_placements.len(), 3);
        assert_eq!(restored.viewport_placements.iter().count(), 0);
        assert_eq!(
            crate::persistence::WorkspaceSnapshot::capture(restored.restore.workspace())
                .expect("restored graph must capture"),
            document.workspace
        );
        let legacy = serde_json::from_str::<DockspaceDocumentEnvelope>("[1, {}]")
            .expect("unsupported outer payload must be skipped");
        assert!(matches!(
            legacy.into_document(),
            Err(DockspaceDocumentDecodeError::UnsupportedVersion {
                found: 1,
                supported: DOCKSPACE_DOCUMENT_VERSION,
            })
        ));
    }

    #[test]
    fn metadata_only_rows_must_name_existing_workspace_surfaces() {
        let mut preferences = profile_preferences();
        preferences.set_native_host(SurfaceId::new(99), NativeSurfaceHostPreference::Root);
        assert!(matches!(
            DockspaceDocument::capture_state(
                DOCUMENT_ID,
                1,
                &profile_engine(&[1, 2, 3]),
                &profile_item_keys(),
                &preferences,
            ),
            Err(DockspaceDocumentCaptureError::UnknownPlacementSurface(surface))
                if surface == SurfaceId::new(99)
        ));
        let mut document = profile_document(&profile_preferences());
        document.viewport_placements = ViewportPlacementSnapshot::capture(&preferences);
        rebind_hash(&mut document);
        assert!(matches!(
            document.restore(|_, _| true),
            Err(DockspaceDocumentRestoreError::UnknownPlacementSurface(surface))
                if surface == SurfaceId::new(99)
        ));
    }

    #[test]
    fn child_host_references_are_validated_during_capture_and_restore() {
        let child = SurfaceId::new(2);
        let cases = [
            (
                SurfaceId::new(99),
                false,
                NativeSurfaceHostPreferenceError::UnknownRecoveryHost {
                    surface: child,
                    recovery_host: SurfaceId::new(99),
                },
            ),
            (
                child,
                false,
                NativeSurfaceHostPreferenceError::SelfRecoveryHost { surface: child },
            ),
            (
                SurfaceId::new(1),
                true,
                NativeSurfaceHostPreferenceError::ChildRecoveryHost {
                    surface: child,
                    recovery_host: SurfaceId::new(1),
                },
            ),
        ];
        for (recovery_host, host_is_child, expected) in cases {
            let mut preferences = profile_preferences();
            preferences
                .set_native_host(child, NativeSurfaceHostPreference::Child { recovery_host });
            if host_is_child {
                preferences.set_native_host(
                    recovery_host,
                    NativeSurfaceHostPreference::Child {
                        recovery_host: SurfaceId::new(3),
                    },
                );
            }
            assert!(matches!(
                DockspaceDocument::capture_state(
                    DOCUMENT_ID,
                    1,
                    &profile_engine(&[1, 2, 3]),
                    &profile_item_keys(),
                    &preferences,
                ),
                Err(DockspaceDocumentCaptureError::NativeHostPreference(actual))
                    if actual == expected
            ));
            let mut document = profile_document(&profile_preferences());
            document.viewport_placements = ViewportPlacementSnapshot::capture(&preferences);
            rebind_hash(&mut document);
            assert!(matches!(
                document.restore(|_, _| true),
                Err(DockspaceDocumentRestoreError::NativeHostPreference(actual))
                    if actual == expected
            ));
        }
    }

    #[test]
    fn publication_clears_a_removed_recovery_host_but_keeps_independent_placement() {
        let child = SurfaceId::new(2);
        let mut preferences = profile_preferences();
        let placement = ViewportPlacementPreference::new(
            child,
            PhysicalRect::new(-120.0, 80.0, 640.0, 480.0).unwrap(),
        )
        .unwrap()
        .with_scale_factor(ScaleFactor::new(1.5).unwrap())
        .with_presentation(crate::viewport_persistence::WindowPresentationPreference::Maximized);
        preferences.set(placement);
        let bound = BoundDocumentState::from_parts(
            DOCUMENT_ID,
            Some(7),
            profile_item_keys(),
            preferences.clone(),
        );
        let engine = profile_engine(&[2, 3]);
        let candidate = bound.prepare_publication(&engine).unwrap();
        assert_eq!(bound.viewport_placements(), &preferences);
        assert_eq!(candidate.next_generation(), Some(7));
        assert_eq!(candidate.document_id(), DOCUMENT_ID);
        assert_eq!(candidate.viewport_placements().native_host(child), None);
        assert_eq!(candidate.viewport_placements().get(child), Some(&placement));
        assert_eq!(
            candidate
                .viewport_placements()
                .surface_ids()
                .collect::<Vec<_>>(),
            [child, SurfaceId::new(3)]
        );
        DockspaceDocument::capture_state(
            DOCUMENT_ID,
            7,
            &engine,
            &candidate.external_item_keys,
            candidate.viewport_placements(),
        )
        .unwrap()
        .restore(|_, _| true)
        .unwrap();
    }

    #[test]
    fn publication_clears_invalid_unbound_chains_and_self_references_without_order_dependence() {
        let first = SurfaceId::new(1);
        let second = SurfaceId::new(2);
        let third = SurfaceId::new(3);
        for (source, intermediate, host) in [(first, second, third), (third, second, first)] {
            let mut preferences = ViewportPlacementPreferences::new();
            preferences.set_native_host(
                source,
                NativeSurfaceHostPreference::Child {
                    recovery_host: intermediate,
                },
            );
            preferences.set_native_host(
                intermediate,
                NativeSurfaceHostPreference::Child {
                    recovery_host: host,
                },
            );
            preferences.set_native_host(host, NativeSurfaceHostPreference::Root);
            let bound = BoundDocumentState::from_parts(
                DOCUMENT_ID,
                Some(0),
                profile_item_keys(),
                preferences.clone(),
            );
            let candidate = bound
                .prepare_publication(&profile_engine(&[1, 2, 3]))
                .unwrap();
            assert_eq!(candidate.viewport_placements().native_host(source), None);
            assert_eq!(
                candidate.viewport_placements().native_host(intermediate),
                Some(NativeSurfaceHostPreference::Child {
                    recovery_host: host
                })
            );
            assert_eq!(bound.viewport_placements(), &preferences);

            let without_host = bound
                .prepare_publication(&profile_engine(&[source.get(), intermediate.get()]))
                .unwrap();
            assert!(without_host.viewport_placements().is_empty());
        }
        let mut preferences = profile_preferences();
        preferences.set_native_host(
            second,
            NativeSurfaceHostPreference::Child {
                recovery_host: second,
            },
        );
        let bound =
            BoundDocumentState::from_parts(DOCUMENT_ID, Some(0), profile_item_keys(), preferences);
        let candidate = bound
            .prepare_publication(&profile_engine(&[1, 2, 3]))
            .unwrap();
        assert_eq!(candidate.viewport_placements().native_host(second), None);
        assert_eq!(candidate.viewport_placements().len(), 2);
    }

    #[test]
    fn publication_removes_a_retired_child_row_and_its_hints() {
        let child = SurfaceId::new(2);
        let mut preferences = profile_preferences();
        preferences.set(
            ViewportPlacementPreference::new(
                child,
                PhysicalRect::new(10.0, 20.0, 640.0, 480.0).unwrap(),
            )
            .unwrap(),
        );
        let bound =
            BoundDocumentState::from_parts(DOCUMENT_ID, Some(0), profile_item_keys(), preferences);
        let candidate = bound.prepare_publication(&profile_engine(&[1, 3])).unwrap();
        assert_eq!(candidate.viewport_placements().native_host(child), None);
        assert!(candidate.viewport_placements().get(child).is_none());
        assert_eq!(candidate.viewport_placements().len(), 2);
    }

    #[test]
    fn binding_hash_rejects_a_valid_native_host_profile_splice() {
        let mut document = profile_document(&profile_preferences());
        let mut other_preferences = profile_preferences();
        other_preferences.set_native_host(
            SurfaceId::new(2),
            NativeSurfaceHostPreference::Child {
                recovery_host: SurfaceId::new(3),
            },
        );
        let other = profile_document(&other_preferences);
        assert_ne!(document.binding_hash, other.binding_hash);
        document.viewport_placements = other.viewport_placements;
        assert!(matches!(
            document
                .restore(|_, _| panic!("spliced profile must reject before application lookup")),
            Err(DockspaceDocumentRestoreError::BindingHashMismatch)
        ));
    }

    #[test]
    fn prepared_restore_preserves_profile_surface_and_recovery_host_identities() {
        let mut preferences = profile_preferences();
        let rect = PhysicalRect::new(-40.0, 30.0, 800.0, 600.0).expect("placement must validate");
        preferences.set(
            ViewportPlacementPreference::new(SurfaceId::new(2), rect)
                .expect("nonempty child placement must validate"),
        );
        let mut active = profile_engine(&[10]);
        let bound = BoundDocumentState::from_parts(
            DOCUMENT_ID,
            Some(0),
            profile_item_keys(),
            ViewportPlacementPreferences::new(),
        );
        let prepared = bound
            .prepare_runtime_restore(&active, profile_document(&preferences), |_, _| true)
            .expect("same-lineage restore must prepare");
        assert_eq!(prepared.viewport_placements(), &preferences);
        let mut host = TestPresentationHost::new(&mut active);
        let frame = host.begin(&active);
        let (input, pending) = prepared
            .clone()
            .into_engine_input(frame.view())
            .expect("saved profile identities must survive preparation");
        let EngineInput::RestoreWorkspace(restore) = input else {
            panic!("prepared restore must produce one workspace replacement");
        };
        let view = DockspaceView::new(restore.workspace());
        let child = view
            .item(ItemId::new(2))
            .expect("child item must survive")
            .surface();
        let root = view
            .item(ItemId::new(1))
            .expect("host item must survive")
            .surface();
        assert_eq!(child, SurfaceId::new(2));
        assert_eq!(root, SurfaceId::new(1));
        assert_eq!(
            restore.presentation_identity_frontier(),
            active.presentation_identity_frontier()
        );
        assert_eq!(pending.viewport_placements(), &preferences);
        assert_eq!(
            pending.viewport_placements().native_host(child),
            Some(NativeSurfaceHostPreference::Child {
                recovery_host: root
            })
        );
        assert_eq!(
            pending.viewport_placements().native_host(root),
            Some(NativeSurfaceHostPreference::Root)
        );
        let placement = pending
            .viewport_placements()
            .get(child)
            .expect("placement must follow source");
        assert_eq!(placement.surface(), child);
        assert_eq!(placement.outer_rect(), rect);
        assert_eq!(prepared.viewport_placements(), &preferences);
        assert!(bound.viewport_placements().is_empty());
        assert!(active.workspace().surface(SurfaceId::new(10)).is_some());
    }

    #[test]
    fn public_document_restore_merges_higher_live_frontiers_before_publication() {
        use crate::geometry::{LogicalRect, LogicalSize};
        use crate::model::{
            DockspaceContainedLayout, DockspaceLayout, DockspaceNode, DockspaceRootLayout,
            DockspaceSurfaceLayout, FloatingPresentationId,
        };
        use crate::runtime::{
            DockspaceDocumentBootstrap, DockspaceSession, HostInputOutcome,
            SurfaceUnavailableReason, UniformSurfaceMetrics,
        };

        let bootstrap = || {
            let mut bootstrap = DockspaceDocumentBootstrap::new(DOCUMENT_ID);
            bootstrap
                .ensure_items(["pane/one", "pane/two", "pane/three"])
                .unwrap();
            bootstrap
        };
        let layout = |surface, main, contained, floating| {
            DockspaceLayout::new([DockspaceSurfaceLayout::new(
                SurfaceId::new(surface),
                DockspaceRootLayout::new(
                    RootId::new(main),
                    DockspaceNode::central_tabs([ItemId::new(1), ItemId::new(2)]),
                ),
            )
            .with_contained(DockspaceContainedLayout::new(
                FloatingPresentationId::new(floating),
                DockspaceRootLayout::new(
                    RootId::new(contained),
                    DockspaceNode::tabs([ItemId::new(3)]),
                ),
                LogicalRect::new(100.0, 80.0, 320.0, 240.0).unwrap(),
            ))])
            .unwrap()
        };
        let mut source = DockspaceSession::from_persistent_layout(
            layout(1, 1, 2, 1),
            DockPolicy::default(),
            bootstrap(),
        )
        .unwrap();
        let older = source.save_document_json().unwrap();
        let mut target = DockspaceSession::from_persistent_layout(
            layout(100, 200, 201, 300),
            DockPolicy::default(),
            bootstrap(),
        )
        .unwrap();
        let before = target.version();
        let prepared = target
            .prepare_document_restore_json(&older, |document, key| {
                document == DOCUMENT_ID && matches!(key, "pane/one" | "pane/two" | "pane/three")
            })
            .unwrap();

        {
            let frame = target.begin_document_restore_frame(&prepared).unwrap();
            assert_eq!(
                frame.view().item(ItemId::new(1)).unwrap().root(),
                RootId::new(1)
            );
        }
        assert_eq!(target.version(), before);
        assert!(target.view().surface(SurfaceId::new(100)).is_some());

        let mut frame = target.begin_document_restore_frame(&prepared).unwrap();
        frame
            .measure_surface(
                SurfaceId::new(1),
                UniformSurfaceMetrics::new(
                    LogicalRect::new(0.0, 0.0, 900.0, 650.0).unwrap(),
                    LogicalSize::new(48.0, 32.0).unwrap(),
                    96.0,
                )
                .unwrap(),
            )
            .unwrap();
        let report = frame
            .commit()
            .expect("the merged frontier must satisfy publication proof");
        assert!(matches!(
            report.inputs(),
            [HostInputOutcome::DocumentRestored { previous, current }]
                if *previous == before && *current == target.version()
        ));
        assert_eq!(target.version().epoch().get(), before.epoch().get() + 1);
        assert_eq!(
            target.view().item(ItemId::new(1)).unwrap().root(),
            RootId::new(1)
        );
        let contained = target.view().item(ItemId::new(3)).unwrap();
        assert_eq!(contained.surface(), SurfaceId::new(1));
        assert_eq!(contained.root(), RootId::new(2));
        assert_eq!(contained.contained(), Some(FloatingPresentationId::new(1)));

        let saved = target.save_document_json().unwrap();
        let document = serde_json::from_slice::<DockspaceDocumentEnvelope>(&saved)
            .unwrap()
            .into_document()
            .unwrap();
        assert_eq!(
            document.presentation_identity_frontier,
            PresentationIdentityFrontier::from_counters(100, 201, 300)
        );

        let mut frame = target.begin_host_frame().unwrap();
        frame
            .float_item_current(ItemId::new(2), SurfaceId::new(1))
            .unwrap();
        let floated = frame.view().item(ItemId::new(2)).unwrap();
        assert_eq!(floated.root(), RootId::new(202));
        assert_eq!(floated.contained(), Some(FloatingPresentationId::new(301)));
        frame
            .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
            .unwrap();
        frame.commit().unwrap();
    }

    #[test]
    fn capture_uses_explicit_roles_retains_unbound_rows_and_prunes_removed_surfaces() {
        let mut engine = profile_engine(&[1, 2, 3]);
        let mut host = TestPresentationHost::new(&mut engine);
        let provider = host.platform_provider();
        let source = StableInputSourceId::new(0xd0c5_0000_0000_0301);
        let expected = engine.version();
        let root = support::submit_input(
            &mut engine,
            &mut host,
            source,
            EngineInput::RegisterViewport {
                provider,
                expected,
                surface: SurfaceId::new(1),
                token: WindowToken::new(41),
                role: ViewportRole::Root,
                recovery_target: None,
            },
        )
        .expect("native root registration must reduce");
        assert!(matches!(
            root.reduced_inputs()[0].outcome(),
            InputOutcome::ViewportRegistered { .. }
        ));
        let expected = engine.version();
        let child = support::submit_input(
            &mut engine,
            &mut host,
            source,
            EngineInput::BootstrapChildViewport {
                provider,
                expected,
                surface: SurfaceId::new(2),
                token: WindowToken::new(42),
                recovery: SurfaceRecoveryBootstrap::new(SurfaceId::new(1)),
            },
        )
        .expect("native child bootstrap must reduce");
        assert!(matches!(
            child.reduced_inputs()[0].outcome(),
            InputOutcome::ViewportRegistered { .. }
        ));
        let mut previous = profile_preferences();
        previous.set_native_host(
            SurfaceId::new(1),
            NativeSurfaceHostPreference::Child {
                recovery_host: SurfaceId::new(3),
            },
        );
        previous.set_native_host(
            SurfaceId::new(2),
            NativeSurfaceHostPreference::Child {
                recovery_host: SurfaceId::new(3),
            },
        );
        previous.set_native_host(SurfaceId::new(99), NativeSurfaceHostPreference::Root);
        let unbound_placement = ViewportPlacementPreference::new(
            SurfaceId::new(3),
            PhysicalRect::new(-900.0, 60.0, 640.0, 480.0).expect("unbound placement must validate"),
        )
        .expect("nonempty unbound placement must validate");
        previous.set(unbound_placement);
        let bound =
            BoundDocumentState::from_parts(DOCUMENT_ID, Some(0), profile_item_keys(), previous);
        let captured = bound
            .prepare_capture(&engine)
            .expect("explicit roles must capture");
        let restored = captured
            .document
            .restore(|_, _| true)
            .expect("captured profile must restore");
        let mut expected_preferences = profile_preferences();
        expected_preferences.set(unbound_placement);
        assert_eq!(restored.viewport_placements, expected_preferences);
        assert!(
            restored
                .viewport_placements
                .get(SurfaceId::new(1))
                .is_none()
        );
        assert!(
            restored
                .viewport_placements
                .get(SurfaceId::new(2))
                .is_none()
        );
        assert!(engine.viewport().viewport(SurfaceId::new(3)).is_none());

        let InputOutcome::ViewportRegistered { binding: root } = root.reduced_inputs()[0].outcome()
        else {
            unreachable!("root registration was checked above");
        };
        let InputOutcome::ViewportRegistered { binding: child } =
            child.reduced_inputs()[0].outcome()
        else {
            unreachable!("child registration was checked above");
        };
        let outer =
            PhysicalRect::new(80.0, 60.0, 960.0, 720.0).expect("root placement must validate");
        let content =
            PhysicalRect::new(88.0, 90.0, 944.0, 682.0).expect("content bounds must validate");
        let scale = ScaleFactor::new(1.5).expect("native scale must validate");
        let windows = vec![
            ObservedWindow::new(*root)
                .with_coordinate_observation(WindowCoordinateObservation::new(
                    *root,
                    CoordinateObservationGeneration::new(1),
                    Authority::Known(content),
                    Authority::Known(outer),
                    Authority::Known(scale),
                    Authority::Known(scale),
                ))
                .with_presentation_observation(WindowPresentationObservation::new(
                    *root,
                    PresentationObservationGeneration::new(1),
                    Authority::Known(WindowPresentationState::Visible),
                    PresentationEffectAcknowledgement::known(None),
                )),
            ObservedWindow::new(*child),
        ];
        let mut capabilities = PlatformCapabilities::default();
        capabilities.set_native_window_lifecycle(PlatformCapability::Supported);
        capabilities.set_authoritative_inventory(PlatformCapability::Supported);
        capabilities.set_global_window_placement(PlatformCapability::Supported);
        let snapshot = PlatformSnapshot::new(
            PlatformSnapshotGeneration::new(1),
            support::known_capability_observation(1, capabilities),
            unknown_focus_observation(
                FocusObservationGeneration::new(1),
                AuthorityUnavailableReason::NotReported,
            ),
            support::known_inventory_observation(1, &windows),
            windows,
            Vec::new(),
            support::unknown_work_area_observation(1),
        )
        .expect("root observation must validate");
        let expected_epoch = engine.version().epoch();
        support::submit_input(
            &mut engine,
            &mut host,
            source,
            EngineInput::PublishPlatformSnapshot {
                provider,
                expected_epoch,
                snapshot,
            },
        )
        .expect("root geometry must publish");
        let captured = bound
            .prepare_capture(&engine)
            .expect("observed root placement must capture");
        let restored = captured
            .document
            .restore(|_, _| true)
            .expect("root placement must restore");
        let placement = restored
            .viewport_placements
            .get(SurfaceId::new(1))
            .expect("root placement must persist");
        assert_eq!(placement.outer_rect(), outer);
        assert_eq!(placement.inner_size(), Some(content.size()));
        assert_eq!(placement.scale_factor(), Some(scale));
        assert_eq!(
            restored.viewport_placements.get(SurfaceId::new(3)),
            Some(&unbound_placement)
        );
        assert_eq!(
            restored.viewport_placements.native_host(SurfaceId::new(2)),
            Some(NativeSurfaceHostPreference::Child {
                recovery_host: SurfaceId::new(1)
            })
        );
    }

    #[test]
    fn binding_valid_document_still_rejects_a_malformed_embedded_key_map() {
        let mut document = DockspaceDocument::capture_state(
            DOCUMENT_ID,
            1,
            &engine(),
            &item_keys(),
            &ViewportPlacementPreferences::new(),
        )
        .expect("fixture document must capture");
        document
            .external_item_keys
            .entries
            .push(SnapshotExternalItemKeyEntry {
                external_key: "pane/duplicate-id".to_owned(),
                item_id: 1,
            });
        document.binding_hash = binding_hash(
            document.document_id,
            document.generation,
            document.presentation_identity_frontier,
            &document.workspace,
            &document.external_item_keys,
            &document.viewport_placements,
        )
        .expect("malformed fixture remains canonically encodable");

        assert!(matches!(
            document.restore(|_, _| true),
            Err(DockspaceDocumentRestoreError::ExternalItemKeys(
                ExternalItemKeyRestoreError::DuplicateItemId { item_id: 1, .. }
            ))
        ));
    }

    #[test]
    fn binding_valid_document_rejects_a_surface_frontier_behind_live_state() {
        let mut document = DockspaceDocument::capture_state(
            DOCUMENT_ID,
            1,
            &engine(),
            &item_keys(),
            &ViewportPlacementPreferences::new(),
        )
        .expect("fixture document must capture");
        document.presentation_identity_frontier =
            PresentationIdentityFrontier::from_counters(0, 1, 0);
        document.binding_hash = binding_hash(
            document.document_id,
            document.generation,
            document.presentation_identity_frontier,
            &document.workspace,
            &document.external_item_keys,
            &document.viewport_placements,
        )
        .expect("invalid frontier fixture remains canonically encodable");

        assert!(matches!(
            document.restore(|_, _| true),
            Err(
                DockspaceDocumentRestoreError::SurfaceIdentityFrontierBehind {
                    frontier: 0,
                    live: 1,
                }
            )
        ));
    }
}
