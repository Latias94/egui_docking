#![cfg(feature = "serde")]

use std::cell::Cell;

use crate::geometry::{LogicalRect, LogicalSize};
use dockspace::model::{
    DockspaceLayout, DockspaceNode, DockspaceRootLayout, DockspaceSurfaceLayout, ItemId, RootId,
    SurfaceId,
};
use dockspace::policy::DockPolicy;
use dockspace::runtime::{
    DockspaceDocumentBootstrap, DockspaceDocumentId, DockspacePersistenceErrorKind,
    DockspaceRuntimeErrorKind, DockspaceSession, HostInputOutcome, NativeHostErrorKind,
    SurfaceUnavailableReason, UniformSurfaceMetrics,
};

const DOCUMENT: DockspaceDocumentId = DockspaceDocumentId::from_bytes([0x73; 16]);
const SURFACE: SurfaceId = SurfaceId::new(1);
const ROOT: RootId = RootId::new(1);

fn layout(items: impl IntoIterator<Item = ItemId>) -> DockspaceLayout {
    DockspaceLayout::new([DockspaceSurfaceLayout::new(
        SURFACE,
        DockspaceRootLayout::new(ROOT, DockspaceNode::central_tabs(items)),
    )])
    .expect("the persistence fixture layout is valid")
}

fn metrics() -> UniformSurfaceMetrics {
    UniformSurfaceMetrics::new(
        LogicalRect::new(0.0, 0.0, 960.0, 640.0).expect("test bounds validate"),
        LogicalSize::new(64.0, 48.0).expect("test minimum validates"),
        96.0,
    )
    .expect("test metrics validate")
}

fn saved_document_with_two_items() -> (Vec<u8>, ItemId, ItemId) {
    let mut bootstrap = DockspaceDocumentBootstrap::new(DOCUMENT);
    let first = bootstrap.ensure_item("pane:first").expect("first item");
    let second = bootstrap.ensure_item("pane:second").expect("second item");
    let mut source = DockspaceSession::from_persistent_layout(
        layout([first, second]),
        DockPolicy::default(),
        bootstrap,
    )
    .expect("the source session builds");
    let bytes = source
        .save_document_json()
        .expect("the source document encodes");
    (bytes, first, second)
}

fn persistent_session_with_first_item() -> (DockspaceSession, ItemId) {
    let mut bootstrap = DockspaceDocumentBootstrap::new(DOCUMENT);
    let first = bootstrap.ensure_item("pane:first").expect("first item");
    let session =
        DockspaceSession::from_persistent_layout(layout([first]), DockPolicy::default(), bootstrap)
            .expect("the target session builds");
    (session, first)
}

#[test]
fn candidate_frame_resolves_session_owned_external_keys() {
    let (mut session, first) = persistent_session_with_first_item();
    let published = session.version();
    let frame = session
        .begin_host_frame()
        .expect("the candidate frame begins");

    assert_eq!(frame.version(), published);
    assert_eq!(frame.external_key_for_item(first), Some("pane:first"));
}

#[test]
fn restore_keeps_document_owned_item_ids_when_the_application_recognizes_keys() {
    let mut bootstrap = DockspaceDocumentBootstrap::new(DOCUMENT);
    let first = bootstrap.ensure_item("pane:first").expect("first item");
    let second = bootstrap.ensure_item("pane:second").expect("second item");
    let historical = bootstrap
        .ensure_item("pane:historical")
        .expect("historical item");
    let mut source = DockspaceSession::from_persistent_layout(
        layout([first, second]),
        DockPolicy::default(),
        bootstrap,
    )
    .expect("the persistent session builds");
    let bytes = source
        .save_document_json()
        .expect("the complete document encodes");

    let restored =
        DockspaceSession::from_document_json(&bytes, DockPolicy::default(), |document, key| {
            document == DOCUMENT && matches!(key, "pane:first" | "pane:second" | "pane:historical")
        })
        .expect("recognized application keys restore");

    assert_eq!(restored.item_id_for_external_key("pane:first"), Some(first));
    assert_eq!(
        restored.item_id_for_external_key("pane:second"),
        Some(second)
    );
    assert_eq!(
        restored.item_id_for_external_key("pane:historical"),
        Some(historical)
    );
}

#[test]
fn restore_rejects_an_unrecognized_application_key() {
    let mut bootstrap = DockspaceDocumentBootstrap::new(DOCUMENT);
    let first = bootstrap.ensure_item("pane:first").expect("first item");
    let mut source =
        DockspaceSession::from_persistent_layout(layout([first]), DockPolicy::default(), bootstrap)
            .expect("the persistent session builds");
    let bytes = source
        .save_document_json()
        .expect("the complete document encodes");

    let error =
        DockspaceSession::from_document_json(&bytes, DockPolicy::default(), |_document, _key| {
            false
        })
        .expect_err("an unrecognized key must reject the complete document");

    assert_eq!(
        error.kind(),
        DockspacePersistenceErrorKind::IdentityConflict
    );
}

#[test]
fn live_restore_atomically_publishes_document_state_and_stales_prior_actions() {
    let (bytes, first, second) = saved_document_with_two_items();
    let (mut target, target_first) = persistent_session_with_first_item();
    assert_eq!(first, target_first);
    let stale = target.prepare_select_item(first);

    let mut restore = target
        .begin_document_restore_frame(&bytes, |document, key| {
            document == DOCUMENT && matches!(key, "pane:first" | "pane:second")
        })
        .expect("the same-lineage restore frame begins");
    assert!(restore.view().item(second).is_some());
    assert_eq!(restore.external_key_for_item(second), Some("pane:second"));
    restore
        .measure_surface(SURFACE, metrics())
        .expect("the restored surface measures");
    restore.commit().expect("the complete restore publishes");

    assert!(target.view().item(second).is_some());
    assert_eq!(target.item_id_for_external_key("pane:second"), Some(second));
    assert_eq!(target.next_document_generation(), Some(1));

    let mut frame = target.begin_host_frame().expect("the next frame begins");
    frame
        .submit_prepared_action(stale)
        .expect("the same-session stale action is structurally accepted");
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .expect("the restored ready candidate is retained");
    let report = frame.commit().expect("the stale-action frame commits");
    assert!(matches!(
        report.inputs(),
        [HostInputOutcome::StaleRejected { .. }]
    ));
}

#[test]
fn dropping_live_restore_frame_preserves_published_document_state() {
    let (bytes, first, second) = saved_document_with_two_items();
    let (mut target, target_first) = persistent_session_with_first_item();
    assert_eq!(first, target_first);

    {
        let restore = target
            .begin_document_restore_frame(&bytes, |document, key| {
                document == DOCUMENT && matches!(key, "pane:first" | "pane:second")
            })
            .expect("the rollbackable restore frame begins");
        assert!(restore.view().item(second).is_some());
    }

    assert!(target.view().item(first).is_some());
    assert!(target.view().item(second).is_none());
    assert_eq!(target.item_id_for_external_key("pane:second"), None);
    assert_eq!(target.next_document_generation(), Some(0));
}

#[test]
fn live_restore_rejects_conflicting_key_history_atomically() {
    let (bytes, source_first, source_second) = saved_document_with_two_items();
    let mut bootstrap = DockspaceDocumentBootstrap::new(DOCUMENT);
    let target_second = bootstrap.ensure_item("pane:second").expect("second item");
    let target_first = bootstrap.ensure_item("pane:first").expect("first item");
    assert_eq!(source_first, target_second);
    assert_eq!(source_second, target_first);
    let mut target = DockspaceSession::from_persistent_layout(
        layout([target_first]),
        DockPolicy::default(),
        bootstrap,
    )
    .expect("the conflicting target session builds");
    let before = target.version();

    let error = match target.begin_document_restore_frame(&bytes, |document, key| {
        document == DOCUMENT && matches!(key, "pane:first" | "pane:second")
    }) {
        Ok(_) => panic!("conflicting append-only key history must reject"),
        Err(error) => error,
    };

    assert_eq!(error.kind(), DockspaceRuntimeErrorKind::Persistence);
    assert_eq!(
        error.persistence_kind(),
        Some(DockspacePersistenceErrorKind::IdentityConflict)
    );
    assert_eq!(target.version(), before);
    assert!(target.view().item(target_first).is_some());
    assert!(target.view().item(source_first).is_none());
    assert_eq!(target.next_document_generation(), Some(0));
}

#[test]
fn standalone_restore_rejects_before_decode_when_native_host_is_active() {
    let (mut target, first) = persistent_session_with_first_item();
    target
        .enable_observed_native_roots()
        .expect("the native host enrolls");
    let before = target.version();

    let error = match target.begin_document_restore_frame(b"not json", |_document, _key| true) {
        Ok(_) => panic!("native restore must join the native causal frame"),
        Err(error) => error,
    };

    assert_eq!(error.kind(), DockspaceRuntimeErrorKind::OperationConflict);
    assert_eq!(
        error.native_kind(),
        Some(NativeHostErrorKind::OperationConflict)
    );
    assert_eq!(target.version(), before);
    assert!(target.view().item(first).is_some());
}

#[test]
fn saving_does_not_stale_a_prepared_product_action() {
    let (mut session, first) = persistent_session_with_first_item();
    let second = session
        .ensure_external_item("pane:second")
        .expect("the second item identity allocates");

    let mut open = session.begin_host_frame().expect("the open frame begins");
    open.open_item_current(second, dockspace::model::DockPlacement::After(first))
        .expect("the second item opens");
    open.complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .expect("the ready surface is retained");
    open.commit().expect("the open frame commits");

    let prepared = session.prepare_select_item(first);
    let version = session.version();
    session
        .save_document_json()
        .expect("saving the session succeeds");
    assert_eq!(session.version(), version);

    let mut frame = session.begin_host_frame().expect("the action frame begins");
    frame
        .submit_prepared_action(prepared)
        .expect("the pre-save action remains structurally valid");
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .expect("the ready surface is retained");
    let report = frame.commit().expect("the action frame commits");

    assert!(matches!(
        report.inputs(),
        [HostInputOutcome::ProductActionApplied(_)]
    ));
    assert!(
        session
            .view()
            .item(first)
            .is_some_and(|item| item.is_selected())
    );
}

#[test]
fn restoring_an_older_generation_preserves_append_only_identity_history() {
    let mut bootstrap = DockspaceDocumentBootstrap::new(DOCUMENT);
    let first = bootstrap.ensure_item("pane:first").expect("first item");
    let mut source =
        DockspaceSession::from_persistent_layout(layout([first]), DockPolicy::default(), bootstrap)
            .expect("the source session builds");
    let older = source
        .save_document_json()
        .expect("the older generation encodes");
    let later = source
        .ensure_external_item("pane:later")
        .expect("the later identity allocates");
    let latest = source
        .save_document_json()
        .expect("the latest generation encodes");

    let mut target =
        DockspaceSession::from_document_json(&latest, DockPolicy::default(), |document, key| {
            document == DOCUMENT && matches!(key, "pane:first" | "pane:later")
        })
        .expect("the latest generation restores");
    assert_eq!(target.next_document_generation(), Some(2));

    let mut restore = target
        .begin_document_restore_frame(&older, |document, key| {
            document == DOCUMENT && key == "pane:first"
        })
        .expect("the older same-lineage generation prepares");
    restore
        .measure_surface(SURFACE, metrics())
        .expect("the restored surface measures");
    restore.commit().expect("the older generation publishes");

    assert_eq!(target.next_document_generation(), Some(2));
    assert_eq!(target.item_id_for_external_key("pane:later"), Some(later));
    let newest = target
        .ensure_external_item("pane:newest")
        .expect("a fresh identity allocates after the restore");
    assert!(newest.get() > later.get());
}

#[test]
fn malformed_unsupported_and_wrong_lineage_documents_reject_atomically() {
    let (mut target, first) = persistent_session_with_first_item();
    let before = target.version();
    let generation = target.next_document_generation();

    for (bytes, expected_kind) in [
        (
            b"{".as_slice(),
            DockspacePersistenceErrorKind::InvalidDocument,
        ),
        (
            br#"[999,{}]"#.as_slice(),
            DockspacePersistenceErrorKind::UnsupportedVersion,
        ),
    ] {
        let error = match target.begin_document_restore_frame(bytes, |_document, _key| true) {
            Ok(_) => panic!("invalid document bytes must reject"),
            Err(error) => error,
        };
        assert_eq!(error.kind(), DockspaceRuntimeErrorKind::Persistence);
        assert_eq!(error.persistence_kind(), Some(expected_kind));
    }

    let foreign_document = DockspaceDocumentId::from_bytes([0x91; 16]);
    let mut bootstrap = DockspaceDocumentBootstrap::new(foreign_document);
    let foreign_item = bootstrap.ensure_item("pane:foreign").expect("foreign item");
    let mut foreign = DockspaceSession::from_persistent_layout(
        layout([foreign_item]),
        DockPolicy::default(),
        bootstrap,
    )
    .expect("the foreign session builds");
    let bytes = foreign
        .save_document_json()
        .expect("the foreign document encodes");
    let resolver_called = Cell::new(false);
    let error = match target.begin_document_restore_frame(&bytes, |_document, _key| {
        resolver_called.set(true);
        true
    }) {
        Ok(_) => panic!("a foreign lineage must reject"),
        Err(error) => error,
    };

    assert_eq!(error.kind(), DockspaceRuntimeErrorKind::Persistence);
    assert_eq!(
        error.persistence_kind(),
        Some(DockspacePersistenceErrorKind::IdentityConflict)
    );
    assert!(!resolver_called.get());
    assert_eq!(target.version(), before);
    assert_eq!(target.next_document_generation(), generation);
    assert!(target.view().item(first).is_some());
}
