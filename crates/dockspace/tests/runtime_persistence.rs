#![cfg(feature = "serde")]

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

    assert_eq!(error.kind(), DockspaceRuntimeErrorKind::Native);
    assert_eq!(
        error.native_kind(),
        Some(NativeHostErrorKind::OperationConflict)
    );
    assert_eq!(target.version(), before);
    assert!(target.view().item(first).is_some());
}
