//! Documents emitted by the public API must preserve finite fractional geometry.

#![cfg(feature = "serde")]

use dockspace::geometry::LogicalRect;
use dockspace::model::{
    DockspaceContainedLayout, DockspaceLayout, DockspaceNode, DockspaceRootLayout,
    DockspaceSurfaceLayout, FloatingPresentationId, ItemId, RootId, SurfaceId,
};
use dockspace::policy::DockPolicy;
use dockspace::runtime::{
    DockspaceDocumentBootstrap, DockspaceDocumentId, DockspaceSession, SurfaceUnavailableReason,
};

const DOCUMENT: DockspaceDocumentId = DockspaceDocumentId::from_bytes([0xF6; 16]);
const SURFACE: SurfaceId = SurfaceId::new(1);
const ROOT: RootId = RootId::new(1);
const FLOATING: FloatingPresentationId = FloatingPresentationId::new(1);
const ITEM: ItemId = ItemId::new(1);

fn fractional_rect() -> LogicalRect {
    LogicalRect::new(0.0, 0.0, 449.49106478873813, 100.0).unwrap()
}

fn persistent_session() -> DockspaceSession {
    let mut bootstrap = DockspaceDocumentBootstrap::new(DOCUMENT);
    assert_eq!(bootstrap.ensure_item("pane:first").unwrap(), ITEM);
    let layout = DockspaceLayout::new([DockspaceSurfaceLayout::rootless(SURFACE).with_contained(
        DockspaceContainedLayout::new(
            FLOATING,
            DockspaceRootLayout::new(ROOT, DockspaceNode::tabs([ITEM])),
            fractional_rect(),
        ),
    )])
    .unwrap();
    DockspaceSession::from_persistent_layout(layout, DockPolicy::default(), bootstrap).unwrap()
}

fn recognize(document: DockspaceDocumentId, key: &str) -> bool {
    document == DOCUMENT && key == "pane:first"
}

fn assert_restored_geometry(session: &DockspaceSession) {
    assert_eq!(
        session.view().contained(FLOATING).unwrap().rect(),
        fractional_rect(),
        "a saved rectangle must retain its exact finite geometry"
    );
    assert_eq!(session.view().item(ITEM).unwrap().surface(), SURFACE);
    assert_eq!(session.external_key_for_item(ITEM), Some("pane:first"));
}

#[test]
fn saved_document_reloads_with_exact_fractional_geometry() {
    let mut session = persistent_session();
    let bytes = session.save_document_json().unwrap();
    let restored = DockspaceSession::from_document_json(&bytes, DockPolicy::default(), recognize)
        .expect("a successfully saved document must reload from its unchanged bytes");
    assert_restored_geometry(&restored);
}

#[test]
fn saved_document_prepares_and_restores_exact_fractional_geometry() {
    let mut session = persistent_session();
    let bytes = session.save_document_json().unwrap();
    let before = session.version();
    let restore = session
        .prepare_document_restore_json(&bytes, recognize)
        .expect("a successfully saved document must prepare from its unchanged bytes");
    let mut frame = session.begin_document_restore_frame(&restore).unwrap();
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    frame.commit().unwrap();
    assert!(!session.version().is_same_epoch(before));
    assert_restored_geometry(&session);
}
