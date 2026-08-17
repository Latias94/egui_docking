#![cfg(feature = "serde")]

use dockspace::model::{
    DockspaceLayout, DockspaceNode, DockspaceRootLayout, DockspaceSurfaceLayout, ItemId, RootId,
    SurfaceId,
};
use dockspace::policy::DockPolicy;
use egui_dockspace::{
    DockStyle, Dockspace, DockspaceActionStatus, DockspaceDocumentBootstrap, DockspaceDocumentId,
    DockspaceErrorKind,
};

const DOCUMENT: DockspaceDocumentId = DockspaceDocumentId::from_bytes([0x42; 16]);
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
fn document_round_trip_preserves_complete_append_only_item_history() {
    let mut bootstrap = DockspaceDocumentBootstrap::new(DOCUMENT);
    let first = bootstrap.ensure_item("pane:first").expect("first item");
    let second = bootstrap.ensure_item("pane:second").expect("second item");
    let closed = bootstrap
        .ensure_item("pane:closed")
        .expect("historical item");
    let mut dockspace = Dockspace::builder("persistent-source", layout([first, second]))
        .persistence(bootstrap)
        .build()
        .expect("the persistent facade builds");

    let later = dockspace
        .ensure_external_item("pane:later")
        .expect("later identity allocation succeeds");
    assert_eq!(dockspace.next_document_generation(), Some(0));
    let encoded = dockspace
        .save_document_json()
        .expect("the complete document encodes");
    assert_eq!(dockspace.next_document_generation(), Some(1));

    let restored = Dockspace::from_document_json(
        "persistent-restored",
        &encoded,
        DockPolicy::default(),
        DockStyle::default(),
        |document, key| {
            assert_eq!(document, DOCUMENT);
            matches!(
                key,
                "pane:first" | "pane:second" | "pane:closed" | "pane:later"
            )
        },
    )
    .expect("the complete document restores");

    assert_eq!(restored.document_id(), Some(DOCUMENT));
    assert_eq!(restored.next_document_generation(), Some(1));
    assert_eq!(
        restored.item_id_for_external_key("pane:closed"),
        Some(closed)
    );
    assert_eq!(restored.item_id_for_external_key("pane:later"), Some(later));
    assert_eq!(restored.external_key_for_item(first), Some("pane:first"));
    assert!(restored.view().item(first).is_some());
    assert!(restored.view().item(second).is_some());
    assert!(restored.view().item(closed).is_none());
    assert!(restored.view().item(later).is_none());
}

#[test]
fn live_restore_publishes_atomically_and_stales_pre_restore_actions() {
    let mut bootstrap = DockspaceDocumentBootstrap::new(DOCUMENT);
    let first = bootstrap.ensure_item("pane:first").expect("first item");
    let second = bootstrap.ensure_item("pane:second").expect("second item");
    let mut dockspace = Dockspace::builder("live-restore", layout([first, second]))
        .persistence(bootstrap)
        .build()
        .expect("the persistent facade builds");
    let encoded = dockspace
        .save_document_json()
        .expect("the source document encodes");
    let prepared = dockspace.prepare_select_item(second);
    let before_restore = dockspace.version();

    let mutation = dockspace
        .restore_document_json(&encoded, |document, key| {
            document == DOCUMENT && matches!(key, "pane:first" | "pane:second")
        })
        .expect("the complete document restores into the live session");

    assert_eq!(mutation.before(), before_restore);
    assert_ne!(mutation.after(), before_restore);
    let stale = dockspace
        .submit_prepared_action(prepared)
        .expect("the pre-restore action is consumed as stale");
    assert!(matches!(
        stale.status(),
        DockspaceActionStatus::Stale { .. }
    ));
}

#[test]
fn bootstrap_rejects_layout_items_without_session_owned_keys() {
    let mut bootstrap = DockspaceDocumentBootstrap::new(DOCUMENT);
    let first = bootstrap.ensure_item("pane:first").expect("first item");
    let foreign = ItemId::new(first.get() + 1);

    let error = match Dockspace::builder("incomplete-bootstrap", layout([first, foreign]))
        .persistence(bootstrap)
        .build()
    {
        Ok(_) => panic!("an unbound layout item must be rejected"),
        Err(error) => error,
    };

    assert_eq!(error.kind(), DockspaceErrorKind::Persistence);
}

#[test]
fn restore_rejects_an_unrecognized_application_key() {
    let mut bootstrap = DockspaceDocumentBootstrap::new(DOCUMENT);
    let first = bootstrap.ensure_item("pane:first").expect("first item");
    let mut dockspace = Dockspace::builder("identity-source", layout([first]))
        .persistence(bootstrap)
        .build()
        .expect("the persistent facade builds");
    let encoded = dockspace
        .save_document_json()
        .expect("the source document encodes");

    let error = match Dockspace::from_document_json(
        "identity-target",
        &encoded,
        DockPolicy::default(),
        DockStyle::default(),
        |_document, _key| false,
    ) {
        Ok(_) => panic!("an unrecognized application key must reject the complete document"),
        Err(error) => error,
    };

    assert_eq!(error.kind(), DockspaceErrorKind::Persistence);
}
