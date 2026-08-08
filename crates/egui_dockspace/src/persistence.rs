//! JSON persistence helpers over the atomic dockspace document schema.

use std::collections::BTreeSet;

use dockspace::backend::ingress::{BackendIngressOrdinal, BackendIngressRecorder};
use dockspace::document::{
    DockspaceDocumentBootstrap, DockspaceDocumentDecodeError, DockspaceDocumentEnvelope,
    DockspaceDocumentId, DockspaceDocumentRestoreTicket, DockspaceDocumentSessionError,
};
use dockspace::external_item_key::ExternalItemKeyMap;
use dockspace::graph::Workspace;
use dockspace::ids::{ItemId, SurfaceId};
use dockspace::transition::EngineTransition;
use dockspace::viewport_persistence::{ViewportPlacementPreference, ViewportPlacementPreferences};
use thiserror::Error;

use crate::{Dockspace, DockspaceError};

/// Failure to capture, encode, decode, validate, or publish an atomic dockspace document.
#[derive(Debug, Error)]
pub enum DockspaceDocumentPersistenceError {
    /// The session-owned identity boundary rejected capture, restore, or publication.
    #[error("dockspace document session failed: {0}")]
    Session(#[from] DockspaceDocumentSessionError),
    /// JSON serialization or syntax decoding failed.
    #[error("dockspace document JSON failed: {0}")]
    Json(#[from] serde_json::Error),
    /// The outer document or one of its embedded versioned snapshots cannot decode.
    #[error("dockspace document decode failed: {0}")]
    Decode(#[from] DockspaceDocumentDecodeError),
    /// The validated candidate could not be committed at its explicit reducer boundary.
    #[error("dockspace document publication failed: {0}")]
    Publish(#[from] DockspaceError),
    /// The reducer failed after changing the engine, so the session stayed fail-closed.
    #[error("dockspace document publication failed: {publisher}; rollback failed: {rollback}")]
    PublishRollback {
        /// Original adapter reducer failure.
        publisher: DockspaceError,
        /// Exact-state rollback verification failure.
        rollback: DockspaceDocumentSessionError,
    },
}

/// A successfully published document together with its restored lineage metadata.
#[derive(Debug)]
pub struct DockspaceDocumentLoad {
    transition: EngineTransition,
    document_id: DockspaceDocumentId,
    generation: u64,
}

impl DockspaceDocumentLoad {
    /// Returns the transition that atomically replaced the live workspace.
    #[must_use]
    pub const fn transition(&self) -> &EngineTransition {
        &self.transition
    }

    /// Returns the restored document lineage identity.
    #[must_use]
    pub const fn document_id(&self) -> DockspaceDocumentId {
        self.document_id
    }

    /// Returns the restored document generation.
    #[must_use]
    pub const fn generation(&self) -> u64 {
        self.generation
    }

    /// Consumes the response and returns the underlying reducer transition.
    #[must_use]
    pub fn into_transition(self) -> EngineTransition {
        self.transition
    }
}

impl Dockspace {
    /// Consumes the sole persistence bootstrap for this dockspace owner.
    ///
    /// After binding, the complete external key map cannot be replaced. Call
    /// [`Self::ensure_external_item_key`] for later pane registration and
    /// [`Self::set_viewport_placement_preference`] for placement updates.
    ///
    /// # Errors
    ///
    /// Returns a typed validation or session-state error without partially binding.
    pub fn bind_document_persistence(
        &mut self,
        bootstrap: DockspaceDocumentBootstrap,
    ) -> Result<(), DockspaceDocumentPersistenceError> {
        self.engine.bind_bootstrap(bootstrap)?;
        Ok(())
    }

    /// Imports a legacy pre-paired identity map after proving every association.
    ///
    /// This is intentionally separate from [`Self::bind_document_persistence`].
    /// New applications should mint item identities through
    /// [`DockspaceDocumentBootstrap`] before constructing their workspace.
    ///
    /// # Errors
    ///
    /// Returns a typed validation or association error without partially binding
    /// this dockspace owner.
    pub fn bind_untrusted_document_persistence(
        &mut self,
        document_id: DockspaceDocumentId,
        next_generation: u64,
        external_item_keys: ExternalItemKeyMap,
        viewport_placements: ViewportPlacementPreferences,
        prove_association: impl Fn(ItemId, &str) -> bool,
    ) -> Result<(), DockspaceDocumentPersistenceError> {
        self.engine.bind_untrusted_import(
            document_id,
            next_generation,
            external_item_keys,
            viewport_placements,
            prove_association,
        )?;
        Ok(())
    }

    /// Returns the bound durable document lineage, if persistence is enabled.
    #[must_use]
    pub fn document_id(&self) -> Option<DockspaceDocumentId> {
        self.engine.document_id()
    }

    /// Returns the generation assigned to the next capture, if one remains.
    #[must_use]
    pub fn next_document_generation(&self) -> Option<u64> {
        self.engine.next_generation()
    }

    /// Resolves an external pane key owned by this dockspace session.
    #[must_use]
    pub fn item_id_for_external_key(&self, external_key: &str) -> Option<ItemId> {
        self.engine.item_id(external_key)
    }

    /// Appends one external pane identity to the owned map.
    ///
    /// # Errors
    ///
    /// Returns a typed session or identity-allocation error.
    pub fn ensure_external_item_key(
        &mut self,
        external_key: impl Into<String>,
    ) -> Result<ItemId, DockspaceDocumentPersistenceError> {
        self.engine
            .ensure_external_item_key(external_key)
            .map_err(Into::into)
    }

    /// Atomically appends a batch of external pane identities.
    ///
    /// # Errors
    ///
    /// Returns a typed session or identity-allocation error.
    pub fn ensure_external_item_keys<I, K>(
        &mut self,
        external_keys: I,
    ) -> Result<(), DockspaceDocumentPersistenceError>
    where
        I: IntoIterator<Item = K>,
        K: Into<String>,
    {
        self.engine
            .ensure_external_item_keys(external_keys)
            .map_err(Into::into)
    }

    /// Returns one placement preference owned by this document lineage.
    #[must_use]
    pub fn viewport_placement_preference(
        &self,
        surface: SurfaceId,
    ) -> Option<&ViewportPlacementPreference> {
        self.engine.viewport_placement(surface)
    }

    /// Inserts one placement preference after checking the current surface lineage.
    ///
    /// # Errors
    ///
    /// Returns a typed session or surface-lineage error.
    pub fn set_viewport_placement_preference(
        &mut self,
        preference: ViewportPlacementPreference,
    ) -> Result<Option<ViewportPlacementPreference>, DockspaceDocumentPersistenceError> {
        self.engine
            .set_viewport_placement(preference)
            .map_err(Into::into)
    }

    /// Removes one placement preference from the owned document lineage.
    ///
    /// # Errors
    ///
    /// Returns a typed session-state error.
    pub fn remove_viewport_placement_preference(
        &mut self,
        surface: SurfaceId,
    ) -> Result<Option<ViewportPlacementPreference>, DockspaceDocumentPersistenceError> {
        self.engine
            .remove_viewport_placement(surface)
            .map_err(Into::into)
    }

    /// Captures and encodes the complete session-owned state as one JSON document.
    ///
    /// # Errors
    ///
    /// Returns [`DockspaceDocumentPersistenceError`] when strict capture or JSON encoding fails.
    pub fn save_document_json(&mut self) -> Result<String, DockspaceDocumentPersistenceError> {
        let document = self.engine.capture()?;
        serde_json::to_string(&document).map_err(Into::into)
    }

    /// Strictly decodes one document and atomically publishes its workspace and key map.
    ///
    /// `prove_external_item_association` is queried for every
    /// `(document, item, key)` assignment in the complete append-only pane
    /// history after all snapshot and binding validation succeeds. The owned map,
    /// placement, and generation change only after the engine accepts the exact
    /// workspace.
    ///
    /// # Errors
    ///
    /// Returns [`DockspaceDocumentPersistenceError`] for malformed JSON,
    /// unsupported schema, a mixed workspace/key map, rejected pane identity
    /// associations, invalid topology, or a rejected reducer boundary.
    pub fn load_document_json(
        &mut self,
        json: &str,
        prove_external_item_association: impl Fn(DockspaceDocumentId, ItemId, &str) -> bool,
    ) -> Result<DockspaceDocumentLoad, DockspaceDocumentPersistenceError> {
        let envelope: DockspaceDocumentEnvelope = serde_json::from_str(json)?;
        let document = envelope.into_document()?;
        self.ensure_native_session_idle()?;
        let presentation_host = self.presentation_host;
        let mut restore = self
            .engine
            .begin_restore(document, prove_external_item_association)?;
        let input = restore.take_engine_input()?;
        let (prepared, sequence) = match Self::submit_document_restore_input(
            &mut restore,
            presentation_host,
            &mut self.pointer_input,
            &mut self.semantic_source_sequence,
            input,
        ) {
            Ok(prepared) => prepared,
            Err(publisher) => {
                return match restore.abort() {
                    Ok(()) => Err(DockspaceDocumentPersistenceError::Publish(publisher)),
                    Err(rollback) => Err(DockspaceDocumentPersistenceError::PublishRollback {
                        publisher,
                        rollback,
                    }),
                };
            }
        };
        let committed = restore
            .commit_publication(prepared)
            .map_err(DockspaceError::from)
            .map_err(DockspaceDocumentPersistenceError::Publish)?;
        let (publication, transition) = committed.into_parts();
        self.accept_document_restore_transition(sequence, &transition);
        self.engine.adapter_reconcile_viewport_placements();
        Ok(DockspaceDocumentLoad {
            transition,
            document_id: publication.document_id(),
            generation: publication.generation(),
        })
    }

    /// Queues one complete document for the next joined backend host frame.
    ///
    /// Unlike [`Self::load_document_json`], this path does not attempt a standalone
    /// reducer boundary. The document remains session-owned across failed native
    /// cycles and provider replacement until one outer commit atomically publishes
    /// its workspace and durable sidecars.
    ///
    /// # Errors
    ///
    /// Returns a strict decode, association, lineage, or session-state failure
    /// without changing the live workspace.
    pub fn queue_document_json(
        &mut self,
        json: &str,
        prove_external_item_association: impl Fn(DockspaceDocumentId, ItemId, &str) -> bool,
    ) -> Result<DockspaceDocumentRestoreTicket, DockspaceDocumentPersistenceError> {
        let envelope: DockspaceDocumentEnvelope = serde_json::from_str(json)?;
        let document = envelope.into_document()?;
        self.ensure_native_session_idle()?;
        self.engine
            .queue_restore(document, prove_external_item_association)
            .map_err(Into::into)
    }

    /// Returns whether a backend-routed document restore awaits publication.
    #[must_use]
    pub const fn has_pending_document_restore(&self) -> bool {
        self.engine.has_queued_restore()
    }

    /// Returns the validated workspace carried by the queued backend restore.
    #[doc(hidden)]
    pub fn pending_document_restore_workspace(&self) -> Option<&Workspace> {
        self.engine.adapter_pending_backend_restore_workspace()
    }

    /// Compares the queued document's logical surfaces with the current workspace.
    #[doc(hidden)]
    pub fn pending_document_restore_matches_current_surface_roster(&self) -> Option<bool> {
        let pending = self.pending_document_restore_workspace()?;
        let pending = pending
            .surfaces()
            .map(|(surface, _)| surface)
            .collect::<BTreeSet<_>>();
        let current = self
            .workspace()
            .surfaces()
            .map(|(surface, _)| surface)
            .collect::<BTreeSet<_>>();
        Some(pending == current)
    }

    /// Cancels a queued restore before it is recorded into a backend batch.
    #[doc(hidden)]
    pub fn cancel_pending_document_restore(
        &mut self,
        ticket: &mut DockspaceDocumentRestoreTicket,
    ) -> Result<(), DockspaceDocumentPersistenceError> {
        self.engine
            .adapter_cancel_queued_restore(ticket)
            .map_err(Into::into)
    }

    /// Records the queued restore as the terminal semantic input of this backend batch.
    #[doc(hidden)]
    pub fn record_pending_backend_document_restore(
        &mut self,
        recorder: &mut BackendIngressRecorder,
    ) -> Result<Option<BackendIngressOrdinal>, DockspaceError> {
        self.engine
            .adapter_record_pending_backend_restore(recorder)
            .map_err(Into::into)
    }
}

#[cfg(test)]
mod tests {
    use dockspace::command::{RootContent, WorkspaceCommand};
    use dockspace::document::{
        DockspaceDocumentDecodeError, DockspaceDocumentRestoreError, DockspaceDocumentSessionError,
    };
    use dockspace::external_item_key::{ExternalItemKeyMap, ExternalItemKeyReconcileError};
    use dockspace::geometry::{LogicalRect, PhysicalRect};
    use dockspace::graph::{ContainedFloating, Node, RootRecord, SurfacePresentation, Workspace};
    use dockspace::ids::{FloatingPresentationId, ItemId, RootId, SourceSequence, SurfaceId};
    use dockspace::viewport_persistence::{
        ViewportPlacementPreference, ViewportPlacementPreferences,
    };

    use super::*;
    use crate::Dockspace;

    const DOCUMENT_ID: DockspaceDocumentId = DockspaceDocumentId::from_bytes(*b"egui-document-id");
    const OTHER_DOCUMENT_ID: DockspaceDocumentId =
        DockspaceDocumentId::from_bytes(*b"egui-other-doc!!");
    const SURFACE: SurfaceId = SurfaceId::new(3);

    fn primary_item_association(document_id: DockspaceDocumentId, item: ItemId, key: &str) -> bool {
        document_id == DOCUMENT_ID && item == ItemId::new(1) && key == "pane/primary"
    }

    fn swapped_external_item_keys() -> ExternalItemKeyMap {
        let mut keys = ExternalItemKeyMap::new();
        keys.ensure_all(["pane/foreign", "pane/primary"])
            .expect("fixture key map must allocate");
        keys
    }

    fn placements() -> ViewportPlacementPreferences {
        let mut placements = ViewportPlacementPreferences::new();
        placements.set(
            ViewportPlacementPreference::new(
                SURFACE,
                PhysicalRect::new(40.0, 80.0, 900.0, 700.0)
                    .expect("fixture rectangle must be valid"),
            )
            .expect("fixture placement must be non-empty"),
        );
        placements
    }

    fn workspace() -> Workspace {
        workspace_with(ItemId::new(1))
    }

    fn workspace_with(item: ItemId) -> Workspace {
        let mut builder = Workspace::builder();
        let tabs = builder.insert_node(Node::tabs([item]));
        builder.set_root(RootId::new(2), RootRecord::new(tabs));
        builder.set_surface(SURFACE, SurfacePresentation::with_main(RootId::new(2)));
        builder.build().expect("fixture workspace must be valid")
    }

    fn rootless_workspace() -> Workspace {
        let item = ItemId::new(1);
        let root = RootId::new(22);
        let surface = SurfaceId::new(23);
        let floating = FloatingPresentationId::new(24);
        let mut builder = Workspace::builder();
        let tabs = builder.insert_node(Node::tabs([item]));
        builder.set_root(root, RootRecord::new(tabs));
        builder.set_surface(surface, SurfacePresentation::rootless());
        builder.set_contained_floating(
            floating,
            ContainedFloating::new(
                root,
                LogicalRect::new(10.0, 12.0, 180.0, 120.0).expect("fixture rectangle is valid"),
            ),
        );
        builder
            .attach_contained(surface, floating)
            .expect("surface exists");
        builder.build().expect("rootless fixture must be valid")
    }

    fn facade_with(
        id: &'static str,
        workspace: Workspace,
        bootstrap: DockspaceDocumentBootstrap,
    ) -> Dockspace {
        let mut dockspace = Dockspace::builder(id, workspace)
            .build()
            .expect("fixture facade must build");
        dockspace
            .bind_document_persistence(bootstrap)
            .expect("fixture persistence must bind");
        dockspace
    }

    fn bootstrap(
        document_id: DockspaceDocumentId,
        next_generation: u64,
        with_placement: bool,
    ) -> DockspaceDocumentBootstrap {
        let mut bootstrap = DockspaceDocumentBootstrap::new(document_id, next_generation);
        assert_eq!(
            bootstrap
                .ensure_external_item_key("pane/primary")
                .expect("fixture identity must allocate"),
            ItemId::new(1)
        );
        if with_placement {
            bootstrap.set_viewport_placement(
                placements()
                    .get(SURFACE)
                    .copied()
                    .expect("fixture placement exists"),
            );
        }
        bootstrap
    }

    fn facade_with_untrusted_map(
        id: &'static str,
        workspace: Workspace,
        keys: ExternalItemKeyMap,
    ) -> Dockspace {
        let mut dockspace = Dockspace::builder(id, workspace)
            .build()
            .expect("fixture facade must build");
        dockspace
            .bind_untrusted_document_persistence(DOCUMENT_ID, 1, keys, placements(), |_, _| true)
            .expect("explicit fixture proof accepts the imported association");
        dockspace
    }

    fn facade(next_generation: u64) -> Dockspace {
        facade_with(
            "persistence-test",
            workspace(),
            bootstrap(DOCUMENT_ID, next_generation, true),
        )
    }

    fn unbound_facade(id: &'static str) -> Dockspace {
        Dockspace::builder(id, workspace())
            .build()
            .expect("fixture facade must build")
    }

    #[test]
    fn document_json_round_trip_publishes_engine_keys_placement_and_lineage() {
        let mut source = facade(7);
        let json = source.save_document_json().expect("document must encode");
        let mut target = facade(20);
        let before = target.engine.version();
        let response = target
            .load_document_json(&json, primary_item_association)
            .expect("document must publish");
        let transition = response.transition();

        assert_eq!(transition.before(), before);
        assert_eq!(transition.reduced_inputs().len(), 1);
        assert!(transition.changed());
        assert_eq!(target.engine.version(), transition.after());
        assert_eq!(response.document_id(), DOCUMENT_ID);
        assert_eq!(response.generation(), 7);
        assert_eq!(
            target.item_id_for_external_key("pane/primary"),
            Some(ItemId::new(1))
        );
        assert!(target.viewport_placement_preference(SURFACE).is_some());
        assert_eq!(
            target.next_document_generation(),
            Some(20),
            "loading an older document must not reuse an issued generation"
        );
    }

    #[test]
    fn unbound_facade_adopts_one_complete_saved_document() {
        let mut source = facade(7);
        let json = source.save_document_json().expect("document must encode");
        let mut target = unbound_facade("unbound-persistence-target");

        let response = target
            .load_document_json(&json, primary_item_association)
            .expect("unbound facade must adopt the validated document");

        assert_eq!(response.document_id(), DOCUMENT_ID);
        assert_eq!(response.generation(), 7);
        assert_eq!(target.document_id(), Some(DOCUMENT_ID));
        assert_eq!(target.next_document_generation(), Some(8));
        assert_eq!(
            target.item_id_for_external_key("pane/primary"),
            Some(ItemId::new(1))
        );
        assert!(target.viewport_placement_preference(SURFACE).is_some());
    }

    #[test]
    fn unbound_facade_rejects_a_self_consistent_swapped_item_key_document() {
        let mut source = facade_with_untrusted_map(
            "unbound-swapped-source",
            workspace(),
            swapped_external_item_keys(),
        );
        let json = source
            .save_document_json()
            .expect("swapped source document is internally valid and freshly hashed");
        let mut target = unbound_facade("unbound-swapped-target");
        let before = target.engine.version();

        assert!(matches!(
            target.load_document_json(&json, primary_item_association),
            Err(DockspaceDocumentPersistenceError::Session(
                DockspaceDocumentSessionError::Restore(
                    DockspaceDocumentRestoreError::ExternalItemAssociationsRejected { rejected }
                )
            )) if rejected.len() == 2
        ));
        assert_eq!(target.engine.version(), before);
        assert_eq!(target.document_id(), None);
    }

    #[test]
    fn binding_is_single_use_and_all_new_keys_are_session_owned() {
        let mut dockspace = facade(0);
        assert!(matches!(
            dockspace.bind_document_persistence(DockspaceDocumentBootstrap::new(DOCUMENT_ID, 0)),
            Err(DockspaceDocumentPersistenceError::Session(
                DockspaceDocumentSessionError::AlreadyBound
            ))
        ));
        assert_eq!(
            dockspace
                .ensure_external_item_key("pane/later")
                .expect("session allocation must succeed"),
            ItemId::new(2)
        );
        assert_eq!(
            dockspace.item_id_for_external_key("pane/primary"),
            Some(ItemId::new(1))
        );
    }

    #[test]
    fn bound_facade_rejects_unmapped_workspace_mutations_before_reduction() {
        let mut dockspace = facade(0);
        let before = dockspace.engine.version();
        let unbound_item = ItemId::new(99);

        assert!(matches!(
            dockspace.replace_workspace(workspace_with(unbound_item)),
            Err(DockspaceError::DocumentSession(
                DockspaceDocumentSessionError::UnknownExternalItemKey(item)
            )) if item == unbound_item
        ));
        assert!(matches!(
            dockspace.submit_command(WorkspaceCommand::CreateSurfaceRoot {
                surface: SurfaceId::new(4),
                root: RootId::new(4),
                content: RootContent::OpenItem(unbound_item),
            }),
            Err(DockspaceError::DocumentSession(
                DockspaceDocumentSessionError::UnknownExternalItemKey(item)
            )) if item == unbound_item
        ));
        assert_eq!(dockspace.engine.version(), before);
    }

    #[test]
    fn workspace_commit_prunes_placement_for_surfaces_absent_from_the_final_roster() {
        let mut dockspace = facade(0);
        assert!(dockspace.viewport_placement_preference(SURFACE).is_some());

        dockspace
            .replace_workspace(Workspace::new())
            .expect("an empty workspace remains a valid document state");

        assert!(dockspace.viewport_placement_preference(SURFACE).is_none());
        dockspace
            .save_document_json()
            .expect("stale placement cannot block the next atomic capture");
    }

    #[test]
    fn swapped_bound_map_is_rejected_without_publication() {
        let mut foreign = facade_with_untrusted_map(
            "foreign-persistence",
            workspace(),
            swapped_external_item_keys(),
        );
        let json = foreign
            .save_document_json()
            .expect("foreign session captures itself");
        let mut target = facade(9);
        let before = target.engine.version();

        assert!(matches!(
            target.load_document_json(&json, |_, _, _| true),
            Err(DockspaceDocumentPersistenceError::Session(
                DockspaceDocumentSessionError::Reconcile(
                    ExternalItemKeyReconcileError::ItemIdConflict {
                        item,
                        active_external_key,
                        incoming_external_key,
                    }
                )
            )) if item == ItemId::new(1)
                && active_external_key == "pane/primary"
                && incoming_external_key == "pane/foreign"
        ));
        assert_eq!(target.engine.version(), before);
        assert_eq!(target.next_document_generation(), Some(9));
        assert_eq!(
            target.item_id_for_external_key("pane/primary"),
            Some(ItemId::new(1))
        );
    }

    #[test]
    fn untrusted_facade_import_rejects_swapped_valid_keys_before_binding() {
        let mut dockspace = Dockspace::builder("untrusted-import", workspace())
            .build()
            .expect("fixture facade must build");

        assert!(matches!(
            dockspace.bind_untrusted_document_persistence(
                DOCUMENT_ID,
                0,
                swapped_external_item_keys(),
                ViewportPlacementPreferences::new(),
                |item, key| item == ItemId::new(1) && key == "pane/primary",
            ),
            Err(DockspaceDocumentPersistenceError::Session(
                DockspaceDocumentSessionError::ExternalItemAssociationRejected {
                    item,
                    external_key,
                }
            )) if item == ItemId::new(1) && external_key == "pane/foreign"
        ));
        assert_eq!(dockspace.document_id(), None);
    }

    #[test]
    fn resolver_rejection_rolls_back_every_owned_component() {
        let mut source = facade(1);
        let json = source
            .save_document_json()
            .expect("source document must encode");
        let mut target = facade(12);
        let before_engine = target.engine.version();
        let before_placement = *target
            .viewport_placement_preference(SURFACE)
            .expect("target placement exists");

        assert!(matches!(
            target.load_document_json(&json, |_, _, _| false),
            Err(DockspaceDocumentPersistenceError::Session(
                DockspaceDocumentSessionError::Restore(
                    DockspaceDocumentRestoreError::ExternalItemAssociationsRejected { .. }
                )
            ))
        ));
        assert_eq!(target.engine.version(), before_engine);
        assert_eq!(
            *target
                .viewport_placement_preference(SURFACE)
                .expect("target placement remains"),
            before_placement
        );
        assert_eq!(target.next_document_generation(), Some(12));
    }

    #[test]
    fn reducer_failure_aborts_candidate_and_keeps_session_usable() {
        let mut source = facade(1);
        let json = source
            .save_document_json()
            .expect("source document must encode");
        let mut target = facade(5);
        let before = target.engine.version();
        target.set_semantic_source_sequence_for_test(SourceSequence::new(u64::MAX));

        assert!(matches!(
            target.load_document_json(&json, primary_item_association),
            Err(DockspaceDocumentPersistenceError::Publish(
                DockspaceError::InputSourceSequenceExhausted { .. }
            ))
        ));
        assert_eq!(target.engine.version(), before);
        assert_eq!(target.next_document_generation(), Some(5));
        assert!(
            target.engine.capture().is_ok(),
            "the failed reducer path must explicitly abort its affine candidate"
        );
    }

    #[test]
    fn wrong_lineage_fails_before_engine_or_generation_change() {
        let mut source = facade_with(
            "other-lineage",
            workspace(),
            bootstrap(OTHER_DOCUMENT_ID, 2, true),
        );
        let json = source
            .save_document_json()
            .expect("foreign lineage must encode");
        let mut target = facade(9);
        let before = target.engine.version();

        assert!(matches!(
            target.load_document_json(&json, |_, _, _| true),
            Err(DockspaceDocumentPersistenceError::Session(
                DockspaceDocumentSessionError::WrongLineage {
                    expected: DOCUMENT_ID,
                    found: OTHER_DOCUMENT_ID,
                }
            ))
        ));
        assert_eq!(target.engine.version(), before);
        assert_eq!(target.next_document_generation(), Some(9));
    }

    #[test]
    fn mixed_document_components_fail_hash_before_resolver_or_publication() {
        let mut first = facade(1);
        let first_json = first
            .save_document_json()
            .expect("first document must encode");
        let mut foreign = facade_with(
            "foreign-components",
            workspace(),
            bootstrap(DOCUMENT_ID, 1, false),
        );
        let foreign_json = foreign
            .save_document_json()
            .expect("foreign document must encode");
        let mut mixed = serde_json::from_str::<serde_json::Value>(&first_json)
            .expect("first document JSON is valid");
        let foreign = serde_json::from_str::<serde_json::Value>(&foreign_json)
            .expect("foreign document JSON is valid");
        mixed[1]["viewport_placements"] = foreign[1]["viewport_placements"].clone();
        let mixed = serde_json::to_string(&mixed).expect("mixed fixture must encode");
        let mut target = facade(9);
        let before = target.engine.version();
        let resolver_called = std::cell::Cell::new(false);

        assert!(matches!(
            target.load_document_json(&mixed, |_, _, _| {
                resolver_called.set(true);
                true
            }),
            Err(DockspaceDocumentPersistenceError::Session(
                DockspaceDocumentSessionError::Restore(
                    DockspaceDocumentRestoreError::BindingHashMismatch
                )
            ))
        ));
        assert!(!resolver_called.get());
        assert_eq!(target.engine.version(), before);
        assert_eq!(target.next_document_generation(), Some(9));
    }

    #[test]
    fn nested_component_versions_are_typed_and_do_not_publish() {
        let mut source = facade(1);
        let json = source
            .save_document_json()
            .expect("source document must encode");
        let mut value =
            serde_json::from_str::<serde_json::Value>(&json).expect("document JSON is valid");
        value[1]["viewport_placements"][0] = serde_json::Value::from(9_u64);
        let json = serde_json::to_string(&value).expect("mutated fixture must encode");
        let mut target = facade(4);
        let before = target.engine.version();

        assert!(matches!(
            target.load_document_json(&json, |_, _, _| true),
            Err(DockspaceDocumentPersistenceError::Decode(
                DockspaceDocumentDecodeError::ViewportPlacements(
                    dockspace::viewport_persistence::ViewportPlacementRestoreError::UnsupportedVersion {
                        found: 9,
                        ..
                    }
                )
            ))
        ));
        assert_eq!(target.engine.version(), before);
        assert_eq!(target.next_document_generation(), Some(4));
    }

    #[test]
    fn rootless_document_round_trip_preserves_adapter_workspace() {
        let mut source = facade_with(
            "rootless-source",
            rootless_workspace(),
            bootstrap(DOCUMENT_ID, 2, false),
        );
        let json = source
            .save_document_json()
            .expect("rootless document must encode");
        let mut target = facade_with(
            "rootless-target",
            rootless_workspace(),
            bootstrap(DOCUMENT_ID, 8, false),
        );
        target
            .load_document_json(&json, primary_item_association)
            .expect("rootless document must publish");
        let restored_surface = target
            .engine
            .workspace()
            .surface(SurfaceId::new(23))
            .expect("restored rootless surface must exist");
        assert_eq!(restored_surface.main_root, None);
        assert_eq!(
            restored_surface.contained.as_slice(),
            &[FloatingPresentationId::new(24)]
        );
    }
}
