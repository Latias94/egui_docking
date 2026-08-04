//! Atomic persistence documents that bind a workspace to its external item identities.
//!
//! A [`DockspaceDocument`] is the only durable boundary for a workspace and
//! [`ExternalItemKeyMap`]. Separate snapshots are intentionally not sufficient:
//! a valid workspace can otherwise be paired with an unrelated valid key map
//! and silently resolve a numeric [`ItemId`] to the wrong application pane.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::ops::Deref;
use std::sync::atomic::{AtomicU64, Ordering};

use blake3::Hasher;
use serde::de::{Error as _, IgnoredAny, SeqAccess, Visitor};
use serde::ser::SerializeTuple;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use thiserror::Error;

use crate::backend_ingress::{BackendIngressProviderReplacementTicket, BackendIngressRecorder};
use crate::command::WorkspaceCommand;
use crate::engine::{
    CoreHostFramePrelude, CoreHostPresentationFrame, EngineError, PreparedHostFrameCommit,
};
use crate::engine::{DockEngine, EngineInput, ValidatedWorkspaceRestore};
use crate::external_item_key::{
    ExternalItemKeyMap, ExternalItemKeyMapError, ExternalItemKeyReconcileError,
    ExternalItemKeyRestoreError, ExternalItemKeySnapshot, ExternalItemKeySnapshotEnvelope,
};
use crate::graph::Workspace;
use crate::ids::{FloatingPresentationId, ItemId, PresentationIdentityFrontier, RootId, SurfaceId};
use crate::persistence::{
    SnapshotCaptureError, SnapshotEntityKind, SnapshotNode, SnapshotRestoreError,
    WorkspaceSnapshot, WorkspaceSnapshotEnvelope,
};
use crate::pointer_journal::{PointerEdgeSequence, PointerInputLease, PointerProviderScope};
use crate::presentation_observation::{
    HostPresentationStreamId, PresentationHostLease, PresentationStreamQuiescence,
};
use crate::transition::{BackendIngressProviderReplacementStart, EngineTransition, InputOutcome};
use crate::viewport::ViewportRole;
use crate::viewport_persistence::{
    ViewportPlacementPreference, ViewportPlacementPreferences, ViewportPlacementRestoreError,
    ViewportPlacementSnapshot, ViewportPlacementSnapshotEnvelope,
};
use crate::viewport_registry::ViewportAdmission;

/// The document schema version emitted and accepted by this crate release.
pub const DOCKSPACE_DOCUMENT_VERSION: u32 = 1;

const BINDING_HASH_DOMAIN: &[u8] = b"dockspace-document/v1\0";

static NEXT_DOCUMENT_SESSION_WITNESS: AtomicU64 = AtomicU64::new(1);

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
    version: u32,
    document_id: DockspaceDocumentId,
    generation: u64,
    presentation_identity_frontier: PresentationIdentityFrontier,
    workspace: WorkspaceSnapshot,
    external_item_keys: ExternalItemKeySnapshot,
    viewport_placements: ViewportPlacementSnapshot,
    binding_hash: [u8; 32],
}

impl DockspaceDocument {
    fn capture_state(
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
            version: DOCKSPACE_DOCUMENT_VERSION,
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

    /// Returns the monotonically increasing generation within the lineage.
    #[must_use]
    pub const fn generation(&self) -> u64 {
        self.generation
    }

    /// Returns the durable presentation identity frontier bound into this document.
    #[must_use]
    pub const fn presentation_identity_frontier(&self) -> PresentationIdentityFrontier {
        self.presentation_identity_frontier
    }

    /// Returns the opaque integrity binding for diagnostics and deterministic tests.
    #[must_use]
    pub const fn binding_hash(&self) -> [u8; 32] {
        self.binding_hash
    }

    fn restore(
        self,
        prove_external_item_association: impl Fn(DockspaceDocumentId, ItemId, &str) -> bool,
    ) -> Result<RestoredDockspaceDocument, DockspaceDocumentRestoreError> {
        if self.version != DOCKSPACE_DOCUMENT_VERSION {
            return Err(DockspaceDocumentRestoreError::UnsupportedVersion {
                found: self.version,
                supported: DOCKSPACE_DOCUMENT_VERSION,
            });
        }

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
            prove_external_item_association,
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
        document.serialize_element(&self.version)?;
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
            version: DOCKSPACE_DOCUMENT_VERSION,
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
    /// Returns the outer schema version without validating its payload.
    #[must_use]
    pub const fn version(&self) -> u32 {
        self.version
    }

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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct DocumentSessionWitness(u64);

impl DocumentSessionWitness {
    fn mint() -> Result<Self, DockspaceDocumentSessionError> {
        NEXT_DOCUMENT_SESSION_WITNESS
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |current| {
                current.checked_add(1)
            })
            .map(Self)
            .map_err(|_| DockspaceDocumentSessionError::SessionIdentityExhausted)
    }
}

#[derive(Debug)]
struct BoundDocumentState {
    document_id: DockspaceDocumentId,
    next_generation: Option<u64>,
    external_item_keys: ExternalItemKeyMap,
    viewport_placements: ViewportPlacementPreferences,
}

/// One-time identity bootstrap consumed by a document session before persistence.
///
/// Applications mint every [`ItemId`] from its external key here, then build the
/// workspace with those returned identities. Consuming the bootstrap into the
/// session preserves that exact map owner; no arbitrary pre-paired map enters
/// the normal persistence path.
#[derive(Debug)]
pub struct DockspaceDocumentBootstrap {
    document_id: DockspaceDocumentId,
    next_generation: u64,
    external_item_keys: ExternalItemKeyMap,
    viewport_placements: ViewportPlacementPreferences,
}

impl DockspaceDocumentBootstrap {
    /// Starts an empty persistence identity bootstrap.
    #[must_use]
    pub const fn new(document_id: DockspaceDocumentId, next_generation: u64) -> Self {
        Self {
            document_id,
            next_generation,
            external_item_keys: ExternalItemKeyMap::new(),
            viewport_placements: ViewportPlacementPreferences::new(),
        }
    }

    /// Returns the existing item for one external key or mints a fresh identity.
    ///
    /// # Errors
    ///
    /// Returns a typed allocation error without changing the bootstrap on failure.
    pub fn ensure_external_item_key(
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
    pub fn ensure_external_item_keys<I, K>(
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
    pub fn item_id(&self, external_key: &str) -> Option<ItemId> {
        self.external_item_keys.item_id(external_key)
    }

    /// Adds or replaces one placement component before the workspace is bound.
    pub fn set_viewport_placement(
        &mut self,
        preference: ViewportPlacementPreference,
    ) -> Option<ViewportPlacementPreference> {
        self.viewport_placements.set(preference)
    }
}

/// Single owner of one engine and every durable identity bound to it.
///
/// Raw workspaces, key maps, and placement preferences become persistence-qualified
/// only after they are consumed by [`Self::bind`]. A bound map cannot be replaced;
/// callers may only append identities through this owner. Capture therefore has no
/// map, placement, lineage, or engine parameter that could be accidentally spliced.
#[derive(Debug)]
pub struct DockspaceDocumentSession {
    witness: DocumentSessionWitness,
    engine: DockEngine,
    binding: Option<BoundDocumentState>,
    pending_restore: Option<u64>,
    next_restore_token: u64,
}

impl DockspaceDocumentSession {
    /// Creates an unbound owner for an engine that does not yet support persistence.
    ///
    /// # Errors
    ///
    /// Returns [`DockspaceDocumentSessionError::SessionIdentityExhausted`] if the
    /// process-local affine session identity space is exhausted.
    pub fn unbound(engine: DockEngine) -> Result<Self, DockspaceDocumentSessionError> {
        Ok(Self {
            witness: DocumentSessionWitness::mint()?,
            engine,
            binding: None,
            pending_restore: None,
            next_restore_token: 0,
        })
    }

    /// Creates an owner and consumes its one-time bootstrap state.
    ///
    /// `next_generation` is the generation assigned to the next successful
    /// capture. Every workspace item must already have one exact external key;
    /// every placement must name a surface in the owned engine.
    ///
    /// # Errors
    ///
    /// Returns a typed session or capture validation error without publishing a
    /// partially bound owner.
    pub fn bind(
        engine: DockEngine,
        bootstrap: DockspaceDocumentBootstrap,
    ) -> Result<Self, DockspaceDocumentSessionError> {
        let mut session = Self::unbound(engine)?;
        session.bind_bootstrap(bootstrap)?;
        Ok(session)
    }

    /// Consumes the sole bootstrap identity state for this owner.
    ///
    /// Once bound, the complete map cannot be replaced. New pane identities must
    /// be allocated with [`Self::ensure_external_item_key`] or
    /// [`Self::ensure_external_item_keys`].
    ///
    /// # Errors
    ///
    /// Returns a typed error if this owner is already bound, has a pending restore,
    /// or the bootstrap state is incomplete for the current engine.
    pub fn bind_bootstrap(
        &mut self,
        bootstrap: DockspaceDocumentBootstrap,
    ) -> Result<(), DockspaceDocumentSessionError> {
        self.ensure_idle()?;
        if self.binding.is_some() {
            return Err(DockspaceDocumentSessionError::AlreadyBound);
        }
        let workspace = WorkspaceSnapshot::capture(self.engine.workspace())?;
        validate_snapshot_item_bindings(&workspace, &bootstrap.external_item_keys)
            .map_err(DockspaceDocumentCaptureError::MissingExternalItemKey)?;
        validate_viewport_placement_surfaces(&workspace, &bootstrap.viewport_placements)
            .map_err(DockspaceDocumentCaptureError::UnknownPlacementSurface)?;
        self.binding = Some(BoundDocumentState {
            document_id: bootstrap.document_id,
            next_generation: Some(bootstrap.next_generation),
            external_item_keys: bootstrap.external_item_keys,
            viewport_placements: bootstrap.viewport_placements,
        });
        Ok(())
    }

    /// Explicitly imports an untrusted pre-paired map with per-item proof.
    ///
    /// This migration-only path queries `prove_association` for every assignment
    /// in the complete append-only key history, including panes not currently
    /// present in the workspace. The callback must prove that the exact external
    /// key is the application's identity for that exact numeric item. Returning
    /// `false` rejects the complete import before this session becomes persistent.
    ///
    /// # Errors
    ///
    /// Returns a typed validation, association, or session-state failure without
    /// partially binding this owner.
    pub fn bind_untrusted_import(
        &mut self,
        document_id: DockspaceDocumentId,
        next_generation: u64,
        external_item_keys: ExternalItemKeyMap,
        viewport_placements: ViewportPlacementPreferences,
        prove_association: impl Fn(ItemId, &str) -> bool,
    ) -> Result<(), DockspaceDocumentSessionError> {
        self.ensure_idle()?;
        if self.binding.is_some() {
            return Err(DockspaceDocumentSessionError::AlreadyBound);
        }
        let workspace = WorkspaceSnapshot::capture(self.engine.workspace())?;
        validate_snapshot_item_bindings(&workspace, &external_item_keys)
            .map_err(DockspaceDocumentCaptureError::MissingExternalItemKey)?;
        for (external_key, item) in external_item_keys.iter() {
            if !prove_association(item, external_key) {
                return Err(
                    DockspaceDocumentSessionError::ExternalItemAssociationRejected {
                        item,
                        external_key: external_key.to_owned(),
                    },
                );
            }
        }
        validate_viewport_placement_surfaces(&workspace, &viewport_placements)
            .map_err(DockspaceDocumentCaptureError::UnknownPlacementSurface)?;
        self.binding = Some(BoundDocumentState {
            document_id,
            next_generation: Some(next_generation),
            external_item_keys,
            viewport_placements,
        });
        Ok(())
    }

    /// Returns the exact owned engine.
    #[must_use]
    pub const fn engine(&self) -> &DockEngine {
        &self.engine
    }

    /// Creates a presentation-host lease through this session owner.
    #[doc(hidden)]
    pub fn adapter_create_presentation_host(
        &mut self,
    ) -> Result<PresentationHostLease, EngineError> {
        self.engine.create_presentation_host()
    }

    /// Enrolls the joined backend provider through this session owner.
    #[doc(hidden)]
    pub fn adapter_create_backend_ingress_provider(
        &mut self,
        presentation_host: PresentationHostLease,
        pointer_committed_through: PointerEdgeSequence,
    ) -> Result<BackendIngressRecorder, EngineError> {
        self.engine
            .create_backend_ingress_provider(presentation_host, pointer_committed_through)
    }

    /// Settles one recorder-reclaimed prefix through this session owner.
    ///
    /// # Errors
    ///
    /// Returns the core's exact settlement error without publishing a partial
    /// document-session mutation.
    #[doc(hidden)]
    pub fn adapter_settle_backend_ingress_prefix_retirement(
        &mut self,
        receipt: &mut crate::backend_ingress::BackendIngressPrefixRetirementReceipt,
    ) -> Result<Vec<crate::viewport::ViewportBinding>, EngineError> {
        self.engine
            .settle_backend_ingress_prefix_retirement(receipt)
    }

    /// Starts one joined backend-provider replacement through this session owner.
    #[doc(hidden)]
    pub fn adapter_begin_backend_ingress_provider_replacement(
        &mut self,
        drained: &mut crate::backend_ingress::BackendIngressDrainReceipt,
    ) -> Result<BackendIngressProviderReplacementStart, EngineError> {
        self.engine
            .begin_backend_ingress_provider_replacement(drained)
    }

    /// Finishes one joined backend-provider replacement through this session owner.
    #[doc(hidden)]
    pub fn adapter_finish_backend_ingress_provider_replacement(
        &mut self,
        ticket: &mut BackendIngressProviderReplacementTicket,
        presentation_host: PresentationHostLease,
    ) -> Result<BackendIngressRecorder, EngineError> {
        self.engine
            .finish_backend_ingress_provider_replacement(ticket, presentation_host)
    }

    /// Creates one pointer provider through this session owner.
    #[doc(hidden)]
    pub fn adapter_create_pointer_provider(
        &mut self,
        scope: PointerProviderScope,
        committed_through: PointerEdgeSequence,
    ) -> Result<PointerInputLease, EngineError> {
        self.engine
            .create_pointer_provider(scope, committed_through)
    }

    /// Retires one pointer provider through this session owner.
    #[doc(hidden)]
    pub fn adapter_retire_pointer_provider(
        &mut self,
        provider: PointerInputLease,
    ) -> Result<EngineTransition, EngineError> {
        self.engine.retire_pointer_provider(provider)
    }

    /// Starts a core host frame without exposing general mutable engine access.
    #[doc(hidden)]
    pub fn adapter_begin_host_frame(
        &mut self,
        presentation_host: PresentationHostLease,
    ) -> Result<CoreHostFramePrelude, EngineError> {
        let mut prelude = self.engine.begin_host_frame(presentation_host)?;
        if let Some(binding) = &self.binding {
            prelude.restrict_item_identity_scope(
                binding.external_item_keys.iter().map(|(_, item)| item),
            );
        }
        Ok(prelude)
    }

    /// Prepares a presentation-phase core host frame against this session owner.
    #[doc(hidden)]
    pub fn adapter_prepare_host_presentation_frame(
        &mut self,
        frame: CoreHostPresentationFrame,
    ) -> Result<PreparedHostFrameCommit<'_>, EngineError> {
        if let Some(binding) = &self.binding {
            let expected = binding
                .external_item_keys
                .iter()
                .map(|(_, item)| item)
                .collect::<BTreeSet<_>>();
            if !frame.item_identity_scope_matches(&expected) {
                return Err(EngineError::HostFramePoisoned {
                    source: crate::engine::CoreHostFrameError::ItemIdentityScopeMismatch,
                });
            }
        }
        frame.prepare(&mut self.engine)
    }

    /// Prepares an owned presentation-phase candidate for an enclosing host transaction.
    #[doc(hidden)]
    pub fn adapter_prepare_owned_host_presentation_frame(
        &self,
        frame: CoreHostPresentationFrame,
    ) -> Result<crate::engine::OwnedPreparedHostFrameCommit, EngineError> {
        if let Some(binding) = &self.binding {
            let expected = binding
                .external_item_keys
                .iter()
                .map(|(_, item)| item)
                .collect::<BTreeSet<_>>();
            if !frame.item_identity_scope_matches(&expected) {
                return Err(EngineError::HostFramePoisoned {
                    source: crate::engine::CoreHostFrameError::ItemIdentityScopeMismatch,
                });
            }
        }
        frame.prepare_owned(&self.engine)
    }

    /// Publishes an owned host-frame candidate after its enclosing host sealed.
    #[doc(hidden)]
    pub fn adapter_commit_owned_host_presentation_frame(
        &mut self,
        prepared: crate::engine::OwnedPreparedHostFrameCommit,
    ) -> Result<EngineTransition, EngineError> {
        prepared.commit(&mut self.engine)
    }

    /// Tries to prepare adapter-proven presentation-stream reclamation.
    #[doc(hidden)]
    pub fn adapter_try_prepare_presentation_stream_quiescence(
        &self,
        presentation_host: PresentationHostLease,
        stream: HostPresentationStreamId,
    ) -> Result<Option<PresentationStreamQuiescence>, EngineError> {
        self.engine
            .try_prepare_presentation_stream_quiescence(presentation_host, stream)
    }

    /// Atomically confirms adapter-proven presentation-stream reclamation.
    #[doc(hidden)]
    pub fn adapter_confirm_presentation_stream_quiescence_batch(
        &mut self,
        quiescences: impl IntoIterator<Item = PresentationStreamQuiescence>,
    ) -> Result<(), EngineError> {
        self.engine
            .confirm_presentation_stream_quiescence_batch(quiescences)
    }

    /// Returns this session's durable document lineage, if persistence is bound.
    #[must_use]
    pub fn document_id(&self) -> Option<DockspaceDocumentId> {
        self.binding.as_ref().map(|binding| binding.document_id)
    }

    /// Returns the generation assigned to the next capture, or `None` after exhaustion.
    #[must_use]
    pub fn next_generation(&self) -> Option<u64> {
        self.binding
            .as_ref()
            .and_then(|binding| binding.next_generation)
    }

    /// Resolves one session-owned external pane key.
    #[must_use]
    pub fn item_id(&self, external_key: &str) -> Option<ItemId> {
        self.binding
            .as_ref()
            .and_then(|binding| binding.external_item_keys.item_id(external_key))
    }

    /// Resolves one session-owned item identity.
    #[must_use]
    pub fn external_item_key(&self, item: ItemId) -> Option<&str> {
        self.binding
            .as_ref()
            .and_then(|binding| binding.external_item_keys.external_key(item))
    }

    /// Verifies that every item in a replacement workspace has a session-owned key.
    ///
    /// Unbound sessions may build an initial workspace before bootstrap or
    /// document adoption. Once a session owns a document lineage, a workspace
    /// replacement cannot introduce an item whose external identity was not
    /// allocated through this session.
    ///
    /// # Errors
    ///
    /// Returns a typed error without changing the workspace or identity map.
    pub fn validate_workspace_identity_bindings(
        &self,
        workspace: &Workspace,
    ) -> Result<(), DockspaceDocumentSessionError> {
        self.ensure_idle()?;
        let Some(binding) = self.binding.as_ref() else {
            return Ok(());
        };
        let snapshot = WorkspaceSnapshot::capture(workspace)?;
        validate_snapshot_item_bindings(&snapshot, &binding.external_item_keys)
            .map_err(DockspaceDocumentSessionError::UnknownExternalItemKey)
    }

    /// Verifies every newly opened item in one workspace command before reduction.
    ///
    /// Commands that only move, select, resize, or remove existing content do
    /// not allocate item identity and therefore need no map lookup.
    ///
    /// # Errors
    ///
    /// Returns a typed error without recording the command when a newly opened
    /// item has not been allocated by this session.
    pub fn validate_workspace_command_identity_bindings(
        &self,
        command: &WorkspaceCommand,
    ) -> Result<(), DockspaceDocumentSessionError> {
        self.ensure_idle()?;
        let Some(binding) = self.binding.as_ref() else {
            return Ok(());
        };
        if let Some(item) = command.opened_item()
            && binding.external_item_keys.external_key(item).is_none()
        {
            return Err(DockspaceDocumentSessionError::UnknownExternalItemKey(item));
        }
        Ok(())
    }

    /// Appends one external pane identity to the session-owned map.
    ///
    /// # Errors
    ///
    /// Returns a typed session-state or allocation error. The complete map is
    /// never exposed for replacement.
    pub fn ensure_external_item_key(
        &mut self,
        external_key: impl Into<String>,
    ) -> Result<ItemId, DockspaceDocumentSessionError> {
        self.ensure_idle()?;
        self.binding_mut()?
            .external_item_keys
            .ensure(external_key)
            .map_err(Into::into)
    }

    /// Atomically appends a batch of external pane identities.
    ///
    /// # Errors
    ///
    /// Returns a typed session-state or allocation error without changing the
    /// map when the complete batch cannot be allocated.
    pub fn ensure_external_item_keys<I, K>(
        &mut self,
        external_keys: I,
    ) -> Result<(), DockspaceDocumentSessionError>
    where
        I: IntoIterator<Item = K>,
        K: Into<String>,
    {
        self.ensure_idle()?;
        self.binding_mut()?
            .external_item_keys
            .ensure_all(external_keys)
            .map_err(Into::into)
    }

    /// Returns one session-owned placement preference.
    #[must_use]
    pub fn viewport_placement(
        &self,
        surface: crate::ids::SurfaceId,
    ) -> Option<&ViewportPlacementPreference> {
        self.binding
            .as_ref()
            .and_then(|binding| binding.viewport_placements.get(surface))
    }

    /// Inserts one placement preference after verifying its current surface lineage.
    ///
    /// # Errors
    ///
    /// Returns a typed error if the session is unbound, busy, or the preference
    /// names a surface outside the owned workspace.
    pub fn set_viewport_placement(
        &mut self,
        preference: ViewportPlacementPreference,
    ) -> Result<Option<ViewportPlacementPreference>, DockspaceDocumentSessionError> {
        self.ensure_idle()?;
        if self
            .engine
            .workspace()
            .surface(preference.surface())
            .is_none()
        {
            return Err(DockspaceDocumentSessionError::UnknownPlacementSurface {
                surface: preference.surface(),
            });
        }
        Ok(self.binding_mut()?.viewport_placements.set(preference))
    }

    /// Removes one placement preference from this document lineage.
    ///
    /// # Errors
    ///
    /// Returns a typed error if the session is unbound or a restore is pending.
    pub fn remove_viewport_placement(
        &mut self,
        surface: crate::ids::SurfaceId,
    ) -> Result<Option<ViewportPlacementPreference>, DockspaceDocumentSessionError> {
        self.ensure_idle()?;
        Ok(self.binding_mut()?.viewport_placements.remove(surface))
    }

    /// Reconciles document sidecars against committed workspace and viewport facts.
    ///
    /// Only an admitted, visible docking-owned child viewport with current
    /// coordinate authority may replace its durable rectangle. Application root
    /// windows remain owned by their host runtime (for example eframe's own
    /// window persistence). Temporarily unavailable, hidden, minimized, staging,
    /// and retiring child windows preserve the last confirmed placement.
    #[doc(hidden)]
    pub fn adapter_reconcile_viewport_placements(&mut self) {
        let surfaces = self
            .engine
            .workspace()
            .surfaces()
            .map(|(surface, _)| surface)
            .collect::<BTreeSet<_>>();
        let root_surfaces = surfaces
            .iter()
            .filter_map(|surface| {
                self.engine
                    .viewport()
                    .viewport(*surface)
                    .filter(|record| record.role() == ViewportRole::Root)
                    .map(|_| *surface)
            })
            .collect::<BTreeSet<_>>();
        let authoritative = surfaces
            .iter()
            .filter_map(|surface| {
                let record = self.engine.viewport().viewport(*surface)?;
                if record.role() != ViewportRole::Child
                    || record.admission() != ViewportAdmission::Admitted
                    || !record.has_coordinate_authority()
                {
                    return None;
                }
                let coordinates = record.coordinates()?;
                Some((
                    *surface,
                    coordinates.outer_bounds()?,
                    coordinates.content_bounds().size(),
                    coordinates.native_scale_factor(),
                ))
            })
            .collect::<Vec<_>>();
        if let Some(binding) = &mut self.binding {
            let child_surfaces = surfaces
                .difference(&root_surfaces)
                .copied()
                .collect::<BTreeSet<_>>();
            binding.viewport_placements.retain_surfaces(&child_surfaces);
            for (surface, outer_rect, inner_size, scale_factor) in authoritative {
                let presentation = binding
                    .viewport_placements
                    .get(surface)
                    .and_then(|preference| preference.presentation());
                let Ok(preference) = ViewportPlacementPreference::new(surface, outer_rect) else {
                    continue;
                };
                let Ok(mut preference) = preference.try_with_inner_size(inner_size) else {
                    continue;
                };
                preference = preference.with_scale_factor(scale_factor);
                if let Some(presentation) = presentation {
                    preference = preference.with_presentation(presentation);
                }
                binding.viewport_placements.set(preference);
            }
        }
    }

    /// Captures the next complete document generation from this sole owner.
    ///
    /// The generation advances only after every component validates and hashes.
    /// No engine, key-map, placement, or lineage argument can be substituted.
    ///
    /// # Errors
    ///
    /// Returns a typed session or capture failure without advancing the generation.
    pub fn capture(&mut self) -> Result<DockspaceDocument, DockspaceDocumentSessionError> {
        self.ensure_idle()?;
        self.adapter_reconcile_viewport_placements();
        let binding = self
            .binding
            .as_ref()
            .ok_or(DockspaceDocumentSessionError::Unbound)?;
        let generation = binding
            .next_generation
            .ok_or(DockspaceDocumentCaptureError::GenerationExhausted)?;
        let document = DockspaceDocument::capture_state(
            binding.document_id,
            generation,
            &self.engine,
            &binding.external_item_keys,
            &binding.viewport_placements,
        )?;
        self.binding_mut()?.next_generation = generation.checked_add(1);
        Ok(document)
    }

    /// Validates one complete document and reserves its exact publication transaction.
    ///
    /// The returned affine transaction borrows this session, so no caller can
    /// concurrently mutate its engine or durable sidecars. It exposes only the
    /// exact restore input and the host-frame operations needed to publish that
    /// input. Commit additionally requires the non-forgeable reducer transition
    /// which proves that `RestoreWorkspace`, rather than an ordinary replacement,
    /// reached this exact engine.
    ///
    /// A bound session accepts only its own lineage. An unbound session may adopt
    /// one complete, hash-validated document at this boundary; it never receives
    /// an independently supplied key map.
    ///
    /// # Errors
    ///
    /// Returns a typed validation, lineage, reconciliation, or reservation error
    /// without publishing any engine or durable sidecar state.
    pub fn begin_restore(
        &mut self,
        document: DockspaceDocument,
        prove_external_item_association: impl Fn(DockspaceDocumentId, ItemId, &str) -> bool,
    ) -> Result<DockspaceDocumentRestore<'_>, DockspaceDocumentSessionError> {
        let candidate = self.prepare_restore(document, prove_external_item_association)?;
        Ok(DockspaceDocumentRestore {
            session: self,
            candidate: Some(candidate),
        })
    }

    fn prepare_restore(
        &mut self,
        document: DockspaceDocument,
        prove_external_item_association: impl Fn(DockspaceDocumentId, ItemId, &str) -> bool,
    ) -> Result<PreparedDockspaceDocumentRestore, DockspaceDocumentSessionError> {
        self.ensure_idle()?;
        if let Some(binding) = self.binding.as_ref()
            && document.document_id() != binding.document_id
        {
            return Err(DockspaceDocumentSessionError::WrongLineage {
                expected: binding.document_id,
                found: document.document_id(),
            });
        }
        let mut restored = document.restore(prove_external_item_association)?;
        rebase_restored_presentation_identities(
            &mut restored,
            self.engine.workspace(),
            self.engine.presentation_identity_frontier(),
        )?;
        let external_item_keys = self.binding.as_ref().map_or_else(
            || Ok(restored.external_item_keys.clone()),
            |binding| {
                binding
                    .external_item_keys
                    .reconciled_with(&restored.external_item_keys)
            },
        )?;
        let expected_workspace = WorkspaceSnapshot::capture(restored.restore.workspace())?;
        let original_workspace = WorkspaceSnapshot::capture(self.engine.workspace())?;
        let original_frontier = self.engine.presentation_identity_frontier();
        let original_version = self.engine.version();
        let original_tick = self.engine.last_reducer_tick();
        let mut expected_frontier = original_frontier;
        expected_frontier.merge(restored.restore.presentation_identity_frontier());
        let next_generation = self.binding.as_ref().map_or_else(
            || restored.generation.checked_add(1),
            |binding| {
                merge_next_generation(binding.next_generation, restored.generation.checked_add(1))
            },
        );
        let token = self
            .next_restore_token
            .checked_add(1)
            .ok_or(DockspaceDocumentSessionError::RestoreTokenExhausted)?;
        self.next_restore_token = token;
        self.pending_restore = Some(token);
        Ok(PreparedDockspaceDocumentRestore {
            witness: self.witness,
            token,
            document_id: restored.document_id,
            generation: restored.generation,
            restore: Some(restored.restore),
            external_item_keys,
            viewport_placements: restored.viewport_placements,
            next_generation,
            expected_workspace,
            expected_frontier,
            original_workspace,
            original_frontier,
            original_version,
            original_tick,
        })
    }

    /// Commits sidecars after the candidate's engine input reached this exact engine.
    ///
    /// # Errors
    ///
    /// Returns a typed affine or publication mismatch and leaves the session
    /// fail-closed when the engine did not reach the candidate's exact state.
    fn commit_restore(
        &mut self,
        candidate: PreparedDockspaceDocumentRestore,
        transition: &EngineTransition,
    ) -> Result<DockspaceDocumentPublication, DockspaceDocumentSessionError> {
        self.validate_candidate(&candidate)?;
        if candidate.restore.is_some() {
            return Err(DockspaceDocumentSessionError::RestoreInputNotTaken);
        }
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
                    } if *before == candidate.original_version
                        && *after == self.engine.version()
                        && *frontier == candidate.expected_frontier
                )
            })
            .count();
        if WorkspaceSnapshot::capture(self.engine.workspace())? != candidate.expected_workspace
            || self.engine.presentation_identity_frontier() != candidate.expected_frontier
            || transition.before() != candidate.original_version
            || transition.after() != self.engine.version()
            || transition.tick() != self.engine.last_reducer_tick()
            || matching_restore != 1
        {
            return Err(DockspaceDocumentSessionError::EnginePublicationMismatch);
        }
        if let Some(binding) = self.binding.as_mut() {
            binding.external_item_keys = candidate.external_item_keys;
            binding.viewport_placements = candidate.viewport_placements;
            binding.next_generation = candidate.next_generation;
        } else {
            self.binding = Some(BoundDocumentState {
                document_id: candidate.document_id,
                next_generation: candidate.next_generation,
                external_item_keys: candidate.external_item_keys,
                viewport_placements: candidate.viewport_placements,
            });
        }
        self.pending_restore = None;
        Ok(DockspaceDocumentPublication {
            document_id: candidate.document_id,
            generation: candidate.generation,
        })
    }

    /// Cancels one candidate whose engine input did not publish.
    ///
    /// # Errors
    ///
    /// Returns an affine or rollback mismatch. If the engine has changed, the
    /// session remains pending instead of silently publishing mismatched sidecars.
    fn abort_restore(
        &mut self,
        candidate: &PreparedDockspaceDocumentRestore,
    ) -> Result<(), DockspaceDocumentSessionError> {
        self.validate_candidate(candidate)?;
        if WorkspaceSnapshot::capture(self.engine.workspace())? != candidate.original_workspace
            || self.engine.presentation_identity_frontier() != candidate.original_frontier
            || self.engine.version() != candidate.original_version
            || self.engine.last_reducer_tick() != candidate.original_tick
        {
            return Err(DockspaceDocumentSessionError::EngineRollbackMismatch);
        }
        self.pending_restore = None;
        Ok(())
    }

    fn binding_mut(&mut self) -> Result<&mut BoundDocumentState, DockspaceDocumentSessionError> {
        self.binding
            .as_mut()
            .ok_or(DockspaceDocumentSessionError::Unbound)
    }

    fn ensure_idle(&self) -> Result<(), DockspaceDocumentSessionError> {
        self.pending_restore.map_or(Ok(()), |token| {
            Err(DockspaceDocumentSessionError::RestorePending { token })
        })
    }

    fn validate_candidate(
        &self,
        candidate: &PreparedDockspaceDocumentRestore,
    ) -> Result<(), DockspaceDocumentSessionError> {
        if candidate.witness != self.witness {
            return Err(DockspaceDocumentSessionError::CandidateSessionMismatch);
        }
        if self.pending_restore != Some(candidate.token) {
            return Err(DockspaceDocumentSessionError::CandidateTokenMismatch {
                expected: self.pending_restore,
                found: candidate.token,
            });
        }
        Ok(())
    }
}

impl Deref for DockspaceDocumentSession {
    type Target = DockEngine;

    fn deref(&self) -> &Self::Target {
        &self.engine
    }
}

/// Affine publication transaction for one validated dockspace document.
///
/// This value exclusively borrows its owning [`DockspaceDocumentSession`]. It
/// deliberately exposes no mutable [`DockEngine`] reference. Adapters may only
/// take the exact restore input, run the normal rollbackable host-frame protocol,
/// and then commit with the resulting [`EngineTransition`] or abort unchanged.
#[must_use = "a document restore transaction must be committed or explicitly aborted"]
#[derive(Debug)]
pub struct DockspaceDocumentRestore<'session> {
    session: &'session mut DockspaceDocumentSession,
    candidate: Option<PreparedDockspaceDocumentRestore>,
}

impl DockspaceDocumentRestore<'_> {
    /// Returns the exact engine being restored without granting mutable access.
    #[must_use]
    pub const fn engine(&self) -> &DockEngine {
        self.session.engine()
    }

    /// Takes the sole validated restore input carried by this transaction.
    ///
    /// # Errors
    ///
    /// Returns a typed affine-state error after the input has already been taken
    /// or when the transaction was previously completed.
    pub fn take_engine_input(&mut self) -> Result<EngineInput, DockspaceDocumentSessionError> {
        self.candidate
            .as_mut()
            .ok_or(DockspaceDocumentSessionError::RestoreTransactionCompleted)?
            .take_engine_input()
    }

    /// Starts the sole strict host-frame publication path for this restore.
    #[doc(hidden)]
    pub fn adapter_begin_host_frame(
        &mut self,
        presentation_host: PresentationHostLease,
    ) -> Result<CoreHostFramePrelude, EngineError> {
        self.session.adapter_begin_host_frame(presentation_host)
    }

    /// Prepares a rollbackable host-frame candidate against the owned engine.
    #[doc(hidden)]
    pub fn adapter_prepare_host_presentation_frame(
        &mut self,
        frame: CoreHostPresentationFrame,
    ) -> Result<PreparedHostFrameCommit<'_>, EngineError> {
        self.session.adapter_prepare_host_presentation_frame(frame)
    }

    /// Publishes durable sidecars after the exact restore transition committed.
    ///
    /// # Errors
    ///
    /// Returns a typed proof or state mismatch and leaves the session fail-closed
    /// when `transition` did not publish this transaction's exact restore input.
    pub fn commit(
        mut self,
        transition: &EngineTransition,
    ) -> Result<DockspaceDocumentPublication, DockspaceDocumentSessionError> {
        let candidate = self
            .candidate
            .take()
            .ok_or(DockspaceDocumentSessionError::RestoreTransactionCompleted)?;
        self.session.commit_restore(candidate, transition)
    }

    /// Releases this restore reservation after a failed rollbackable publication.
    ///
    /// # Errors
    ///
    /// Returns a typed mismatch and keeps the session fail-closed if any engine
    /// boundary committed after this transaction was prepared.
    pub fn abort(mut self) -> Result<(), DockspaceDocumentSessionError> {
        let candidate = self
            .candidate
            .take()
            .ok_or(DockspaceDocumentSessionError::RestoreTransactionCompleted)?;
        self.session.abort_restore(&candidate)
    }
}

impl Drop for DockspaceDocumentRestore<'_> {
    fn drop(&mut self) {
        if let Some(candidate) = self.candidate.take() {
            let _ = self.session.abort_restore(&candidate);
        }
    }
}

/// Affine candidate coupling one validated engine input to private durable sidecars.
#[must_use = "a prepared document restore must be committed or explicitly aborted"]
#[derive(Debug)]
struct PreparedDockspaceDocumentRestore {
    witness: DocumentSessionWitness,
    token: u64,
    document_id: DockspaceDocumentId,
    generation: u64,
    restore: Option<ValidatedWorkspaceRestore>,
    external_item_keys: ExternalItemKeyMap,
    viewport_placements: ViewportPlacementPreferences,
    next_generation: Option<u64>,
    expected_workspace: WorkspaceSnapshot,
    expected_frontier: PresentationIdentityFrontier,
    original_workspace: WorkspaceSnapshot,
    original_frontier: PresentationIdentityFrontier,
    original_version: crate::transition::WorkspaceVersion,
    original_tick: crate::ids::ReducerTickId,
}

impl PreparedDockspaceDocumentRestore {
    /// Takes the sole engine input carried by this candidate.
    ///
    /// # Errors
    ///
    /// Returns [`DockspaceDocumentSessionError::RestoreInputAlreadyTaken`] after
    /// the affine input has already been consumed.
    fn take_engine_input(&mut self) -> Result<EngineInput, DockspaceDocumentSessionError> {
        self.restore
            .take()
            .map(EngineInput::RestoreWorkspace)
            .ok_or(DockspaceDocumentSessionError::RestoreInputAlreadyTaken)
    }
}

/// Metadata from one fully committed document publication.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DockspaceDocumentPublication {
    document_id: DockspaceDocumentId,
    generation: u64,
}

impl DockspaceDocumentPublication {
    /// Returns the committed lineage identity.
    #[must_use]
    pub const fn document_id(self) -> DockspaceDocumentId {
        self.document_id
    }

    /// Returns the committed document generation.
    #[must_use]
    pub const fn generation(self) -> u64 {
        self.generation
    }
}

fn merge_next_generation(active: Option<u64>, incoming: Option<u64>) -> Option<u64> {
    match (active, incoming) {
        (Some(active), Some(incoming)) => Some(active.max(incoming)),
        (None, _) | (_, None) => None,
    }
}

#[derive(Debug)]
struct RestoredDockspaceDocument {
    document_id: DockspaceDocumentId,
    generation: u64,
    restore: ValidatedWorkspaceRestore,
    external_item_keys: ExternalItemKeyMap,
    viewport_placements: ViewportPlacementPreferences,
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
    /// The in-memory document version is not supported.
    #[error("unsupported dockspace document version {found}; supported version is {supported}")]
    UnsupportedVersion {
        /// Version carried by the document.
        found: u32,
        /// Version implemented by this crate release.
        supported: u32,
    },
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

/// Failure at the session-owned document identity boundary.
#[derive(Debug, Error)]
pub enum DockspaceDocumentSessionError {
    /// The process-local affine session identity space is exhausted.
    #[error("dockspace document session identity space is exhausted")]
    SessionIdentityExhausted,
    /// This engine owner already consumed its one permitted bootstrap binding.
    #[error("dockspace document session is already bound")]
    AlreadyBound,
    /// This operation requires a persistent document binding, but the owner has not
    /// consumed a bootstrap or adopted one complete restored document.
    #[error("dockspace document session is not bound")]
    Unbound,
    /// An affine restore candidate must finish before another identity operation.
    #[error("dockspace document restore {token} is still pending")]
    RestorePending {
        /// Process-local pending restore token.
        token: u64,
    },
    /// The decoded document belongs to another durable lineage.
    #[error("dockspace document lineage mismatch: expected {expected:?}, found {found:?}")]
    WrongLineage {
        /// Lineage permanently owned by this session.
        expected: DockspaceDocumentId,
        /// Lineage declared by the incoming document.
        found: DockspaceDocumentId,
    },
    /// A placement preference names a surface outside the owned workspace.
    #[error("viewport placement names unknown workspace surface {surface}")]
    UnknownPlacementSurface {
        /// Unknown stable logical surface.
        surface: crate::ids::SurfaceId,
    },
    /// An explicit legacy import could not prove one numeric item/key association.
    #[error("external item association was rejected for item {item}: {external_key}")]
    ExternalItemAssociationRejected {
        /// Numeric item identity used by the imported workspace.
        item: ItemId,
        /// External application identity paired with that item by the imported map.
        external_key: String,
    },
    /// A live workspace operation references an item not allocated by this session.
    #[error("workspace operation references item {0} without a session-owned external key")]
    UnknownExternalItemKey(ItemId),
    /// The process-local restore token space is exhausted.
    #[error("dockspace document restore token space is exhausted")]
    RestoreTokenExhausted,
    /// A prepared candidate was presented to a different session owner.
    #[error("prepared dockspace document restore belongs to another session")]
    CandidateSessionMismatch,
    /// The candidate does not match the session's sole pending token.
    #[error(
        "prepared dockspace document restore token mismatch: expected {expected:?}, found {found}"
    )]
    CandidateTokenMismatch {
        /// Current pending token, if any.
        expected: Option<u64>,
        /// Token carried by the candidate.
        found: u64,
    },
    /// The candidate's engine input has not been consumed.
    #[error("prepared dockspace document restore input has not been taken")]
    RestoreInputNotTaken,
    /// The candidate's affine engine input was already consumed.
    #[error("prepared dockspace document restore input was already taken")]
    RestoreInputAlreadyTaken,
    /// The affine restore transaction was already committed or aborted.
    #[error("dockspace document restore transaction is already completed")]
    RestoreTransactionCompleted,
    /// The engine did not publish the candidate's exact workspace and allocator frontier.
    #[error("engine state does not match the prepared dockspace document restore")]
    EnginePublicationMismatch,
    /// An abort was requested after the owned engine changed.
    #[error("engine state changed before the prepared dockspace document restore was aborted")]
    EngineRollbackMismatch,
    /// Strict workspace capture failed.
    #[error("workspace snapshot capture failed: {0}")]
    Workspace(#[from] SnapshotCaptureError),
    /// Complete document capture failed.
    #[error("dockspace document capture failed: {0}")]
    Capture(#[from] DockspaceDocumentCaptureError),
    /// Complete document restore failed.
    #[error("dockspace document restore failed: {0}")]
    Restore(#[from] DockspaceDocumentRestoreError),
    /// Active and restored append-only item histories conflict.
    #[error("external item identity reconciliation failed: {0}")]
    Reconcile(#[from] ExternalItemKeyReconcileError),
    /// One session-owned external key allocation failed.
    #[error("external item identity allocation failed: {0}")]
    ExternalItemKey(#[from] ExternalItemKeyMapError),
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
    prove_association: impl Fn(DockspaceDocumentId, ItemId, &str) -> bool,
) -> Result<(), DockspaceDocumentRestoreError> {
    let mut rejected = Vec::new();
    for (external_key, item) in external_item_keys.iter() {
        if !prove_association(document_id, item, external_key) {
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
            document.restore(|_, _, _| true),
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
            document.restore(|_, _, _| true),
            Err(
                DockspaceDocumentRestoreError::SurfaceIdentityFrontierBehind {
                    frontier: 0,
                    live: 1,
                }
            )
        ));
    }
}
