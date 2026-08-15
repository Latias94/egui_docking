//! Atomic persistence documents that bind a workspace to its external item identities.
//!
//! A [`DockspaceDocument`] is the only durable boundary for a workspace and
//! [`ExternalItemKeyMap`]. Separate snapshots are intentionally not sufficient:
//! a valid workspace can otherwise be paired with an unrelated valid key map
//! and silently resolve a numeric [`ItemId`] to the wrong application pane.

use std::collections::{BTreeMap, BTreeSet};
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
use crate::ids::{
    EngineAuthorityDomainId, FloatingPresentationId, ItemId, PresentationIdentityFrontier, RootId,
    SurfaceId,
};
use crate::persistence::{
    SnapshotCaptureError, SnapshotEntityKind, SnapshotNode, SnapshotRestoreError,
    WorkspaceSnapshot, WorkspaceSnapshotEnvelope,
};
use crate::transition::{EngineTransition, InputOutcome, WorkspaceVersion};
use crate::viewport::ViewportRole;
use crate::viewport_persistence::{
    ViewportPlacementPreference, ViewportPlacementPreferences, ViewportPlacementRestoreError,
    ViewportPlacementSnapshot, ViewportPlacementSnapshotEnvelope,
};
use crate::viewport_registry::ViewportAdmission;

/// The document schema version emitted and accepted by this crate release.
pub const DOCKSPACE_DOCUMENT_VERSION: u32 = 1;

const BINDING_HASH_DOMAIN: &[u8] = b"dockspace-document/v1\0";

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
struct DockspaceDocumentV1 {
    document_id: DockspaceDocumentId,
    generation: u64,
    presentation_identity_frontier: PresentationIdentityFrontierWire,
    workspace: WorkspaceSnapshotEnvelope,
    external_item_keys: ExternalItemKeySnapshotEnvelope,
    viewport_placements: ViewportPlacementSnapshotEnvelope,
    binding_hash: [u8; 32],
}

impl DockspaceDocumentV1 {
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
    payload: Option<DockspaceDocumentV1>,
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
            DockspaceDocumentV1::into_document,
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
                .next_element::<DockspaceDocumentV1>()?
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

    pub(crate) fn prepare_capture(
        &self,
        engine: &DockEngine,
    ) -> Result<PreparedDockspaceDocumentCapture, DockspaceDocumentCaptureError> {
        let viewport_placements = self.reconciled_viewport_placements(engine);
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

    fn reconciled_viewport_placements(&self, engine: &DockEngine) -> ViewportPlacementPreferences {
        let mut placements = self.viewport_placements.clone();
        let mut child_surfaces = BTreeSet::new();
        for (surface, _) in engine.workspace().surfaces() {
            let record = engine.viewport().viewport(surface);
            if record.is_some_and(|record| record.role() == ViewportRole::Root) {
                continue;
            }
            child_surfaces.insert(surface);
            let Some(record) = record else {
                continue;
            };
            if record.role() != ViewportRole::Child
                || record.admission() != ViewportAdmission::Admitted
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
            let Ok(mut preference) =
                preference.try_with_inner_size(coordinates.content_bounds().size())
            else {
                continue;
            };
            preference = preference.with_scale_factor(coordinates.native_scale_factor());
            if let Some(presentation) = presentation {
                preference = preference.with_presentation(presentation);
            }
            placements.set(preference);
        }
        placements.retain_surfaces(&child_surfaces);
        placements
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
    expected_workspace: WorkspaceSnapshot,
    expected_frontier: PresentationIdentityFrontier,
    frame_start_version: WorkspaceVersion,
    replacement_base_version: WorkspaceVersion,
    source_authority_domain: EngineAuthorityDomainId,
}

impl PreparedRuntimeDocumentRestore {
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

    pub(crate) fn into_engine_input(
        mut self,
        frame: HostFrameView<'_>,
    ) -> Result<(EngineInput, PendingRuntimeDocumentRestore), RuntimeDocumentRestoreError> {
        if frame.authority_domain() != self.source_authority_domain {
            return Err(RuntimeDocumentRestoreError::EnginePublicationMismatch);
        }
        rebase_restored_presentation_identities(
            &mut self.restored,
            frame.workspace(),
            frame.presentation_identity_frontier(),
        )?;
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
        let pending = PendingRuntimeDocumentRestore {
            binding: BoundDocumentState {
                document_id,
                next_generation: self.next_generation,
                external_item_keys: self.external_item_keys,
                item_identity_scope: self.item_identity_scope,
                viewport_placements,
            },
            expected_workspace,
            expected_frontier,
            frame_start_version: self.frame_start_version,
            replacement_base_version,
            source_authority_domain: self.source_authority_domain,
        };
        Ok((EngineInput::RestoreWorkspace(restore), pending))
    }
}

impl PendingRuntimeDocumentRestore {
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
        Ok(PreparedRuntimeDocumentPublication {
            core,
            binding: self.binding,
        })
    }
}

#[must_use = "a prepared runtime document publication must be committed or dropped"]
pub(crate) struct PreparedRuntimeDocumentPublication {
    core: OwnedPreparedHostFrameCommit,
    binding: BoundDocumentState,
}

impl PreparedRuntimeDocumentPublication {
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

fn rebase_restored_presentation_identities(
    restored: &mut RestoredDockspaceDocument,
    active_workspace: &Workspace,
    active_frontier: PresentationIdentityFrontier,
) -> Result<(), DockspaceDocumentRestoreError> {
    let mut snapshot = WorkspaceSnapshot::capture(restored.restore.workspace())?;
    let mut frontier = active_frontier;
    frontier.merge(restored.restore.presentation_identity_frontier());

    let mut surfaces = BTreeMap::new();
    for record in &snapshot.surfaces {
        let persisted = SurfaceId::new(record.id);
        let runtime = if active_workspace.surface(persisted).is_some()
            || persisted.get() > active_frontier.last_surface()
        {
            persisted
        } else {
            frontier.reserve_surface().ok_or(
                DockspaceDocumentRestoreError::PresentationIdentitySpaceExhausted {
                    kind: SnapshotEntityKind::Surface,
                },
            )?
        };
        surfaces.insert(persisted, runtime);
    }

    let mut roots = BTreeMap::new();
    for record in &snapshot.roots {
        let persisted = RootId::new(record.id);
        let runtime = if active_workspace.root(persisted).is_some()
            || persisted.get() > active_frontier.last_root()
        {
            persisted
        } else {
            frontier.reserve_root().ok_or(
                DockspaceDocumentRestoreError::PresentationIdentitySpaceExhausted {
                    kind: SnapshotEntityKind::Root,
                },
            )?
        };
        roots.insert(persisted, runtime);
    }

    let mut floatings = BTreeMap::new();
    for record in &snapshot.contained_floatings {
        let persisted = FloatingPresentationId::new(record.id);
        let runtime = if active_workspace.contained_floating(persisted).is_some()
            || persisted.get() > active_frontier.last_floating()
        {
            persisted
        } else {
            frontier.reserve_floating().ok_or(
                DockspaceDocumentRestoreError::PresentationIdentitySpaceExhausted {
                    kind: SnapshotEntityKind::ContainedFloating,
                },
            )?
        };
        floatings.insert(persisted, runtime);
    }

    for record in &mut snapshot.roots {
        let persisted = record.id;
        record.id = roots
            .get(&RootId::new(persisted))
            .copied()
            .ok_or(
                DockspaceDocumentRestoreError::PresentationIdentityRemapInvariant {
                    kind: SnapshotEntityKind::Root,
                    id: persisted,
                },
            )?
            .get();
    }
    for record in &mut snapshot.surfaces {
        let persisted = record.id;
        record.id = surfaces
            .get(&SurfaceId::new(persisted))
            .copied()
            .ok_or(
                DockspaceDocumentRestoreError::PresentationIdentityRemapInvariant {
                    kind: SnapshotEntityKind::Surface,
                    id: persisted,
                },
            )?
            .get();
        record.main_root = record
            .main_root
            .map(|root| {
                roots
                    .get(&RootId::new(root))
                    .copied()
                    .map(RootId::get)
                    .ok_or(
                        DockspaceDocumentRestoreError::PresentationIdentityRemapInvariant {
                            kind: SnapshotEntityKind::Root,
                            id: root,
                        },
                    )
            })
            .transpose()?;
        for floating in &mut record.contained {
            let persisted = *floating;
            *floating = floatings
                .get(&FloatingPresentationId::new(persisted))
                .copied()
                .ok_or(
                    DockspaceDocumentRestoreError::PresentationIdentityRemapInvariant {
                        kind: SnapshotEntityKind::ContainedFloating,
                        id: persisted,
                    },
                )?
                .get();
        }
    }
    for record in &mut snapshot.contained_floatings {
        let persisted_floating = record.id;
        record.id = floatings
            .get(&FloatingPresentationId::new(persisted_floating))
            .copied()
            .ok_or(
                DockspaceDocumentRestoreError::PresentationIdentityRemapInvariant {
                    kind: SnapshotEntityKind::ContainedFloating,
                    id: persisted_floating,
                },
            )?
            .get();
        let persisted_root = record.root;
        record.root = roots
            .get(&RootId::new(persisted_root))
            .copied()
            .ok_or(
                DockspaceDocumentRestoreError::PresentationIdentityRemapInvariant {
                    kind: SnapshotEntityKind::Root,
                    id: persisted_root,
                },
            )?
            .get();
    }

    let workspace = snapshot.build_candidate(|_| true)?;
    restored.viewport_placements.remap_surfaces(&surfaces);
    restored.restore = ValidatedWorkspaceRestore::new(workspace, frontier);
    Ok(())
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
    /// A placement preference names a surface outside the captured workspace.
    #[error("viewport placement names unknown workspace surface {0}")]
    UnknownPlacementSurface(crate::ids::SurfaceId),
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
    /// A validated runtime workspace could not be recaptured for identity rebasing.
    #[error("restored workspace recapture failed: {0}")]
    WorkspaceCapture(#[from] SnapshotCaptureError),
    /// Reintroducing a retired presentation identity exhausted its numeric domain.
    #[error("restored {kind:?} identity space is exhausted")]
    PresentationIdentitySpaceExhausted {
        /// Exhausted presentation identity class.
        kind: SnapshotEntityKind,
    },
    /// A validated presentation reference was absent from the complete remap.
    #[error("restored {kind:?} identity {id} was absent from the complete remap")]
    PresentationIdentityRemapInvariant {
        /// Missing presentation identity class.
        kind: SnapshotEntityKind,
        /// Missing persisted identity.
        id: u64,
    },
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
    /// A placement preference names a surface outside the restored workspace.
    #[error("viewport placement names unknown workspace surface {0}")]
    UnknownPlacementSurface(crate::ids::SurfaceId),
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
        .iter()
        .map(|placement| placement.surface())
        .find(|surface| !surfaces.contains(surface))
        .map_or(Ok(()), Err)
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
    use crate::engine::DockEngine;
    use crate::external_item_key::{
        ExternalItemKeyMap, ExternalItemKeyRestoreError, SnapshotExternalItemKeyEntry,
    };
    use crate::graph::{Node, RootRecord, SurfacePresentation, Workspace};
    use crate::ids::{ItemId, PresentationIdentityFrontier, RootId, SurfaceId};
    use crate::policy::DockPolicy;
    use crate::viewport_persistence::ViewportPlacementPreferences;

    use super::{
        DockspaceDocument, DockspaceDocumentId, DockspaceDocumentRestoreError, binding_hash,
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
