#![cfg(feature = "serde")]

use dockspace::model::{
    DockspaceLayout, DockspaceNode, DockspaceRootLayout, DockspaceSurfaceLayout, ItemId, RootId,
    SurfaceId,
};
use dockspace::policy::DockPolicy;
use dockspace::runtime::{
    DockspaceDocumentBootstrap, DockspaceDocumentId, DockspacePersistenceErrorKind,
    DockspaceSession,
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
