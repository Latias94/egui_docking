#![cfg(feature = "serde")]

use dockspace::model::{
    DockspaceLayout, DockspaceNode, DockspaceRootLayout, DockspaceSurfaceLayout, ItemId, RootId,
    SurfaceId,
};
use dockspace::policy::DockPolicy;
use dockspace::runtime::{
    DockspaceDocumentBootstrap, DockspaceDocumentId, DockspaceRuntimeErrorKind, DockspaceSession,
    DockspaceUpdate, SurfaceUnavailableReason,
};

const DOCUMENT: DockspaceDocumentId = DockspaceDocumentId::from_bytes([0xD4; 16]);
const ITEM: ItemId = ItemId::new(1);
const SURFACE: SurfaceId = SurfaceId::new(1);
const ROOT: RootId = RootId::new(1);

fn persistent_session() -> DockspaceSession {
    let mut bootstrap = DockspaceDocumentBootstrap::new(DOCUMENT);
    assert_eq!(bootstrap.ensure_item("original").unwrap(), ITEM);
    let layout = DockspaceLayout::new([DockspaceSurfaceLayout::new(
        SURFACE,
        DockspaceRootLayout::new(ROOT, DockspaceNode::central_tabs([ITEM])),
    )])
    .unwrap();
    DockspaceSession::from_persistent_layout(layout, DockPolicy::default(), bootstrap).unwrap()
}

fn suspend_restore(session: &mut DockspaceSession, bytes: &[u8]) -> DockspaceUpdate {
    let restore = session
        .prepare_document_restore_json(bytes, |_, _| true)
        .unwrap();
    let mut frame = session.begin_document_restore_frame(&restore).unwrap();
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    frame.suspend()
}

fn assert_stale_restore_rejected(session: &mut DockspaceSession, update: DockspaceUpdate) {
    match session.resume_update(update) {
        Err(error) => assert_eq!(
            error.kind(),
            DockspaceRuntimeErrorKind::OperationConflict,
            "a changed document sidecar makes the submitted restore stale"
        ),
        Ok(frame) => {
            drop(frame);
            panic!("a restore prepared against older document sidecars must be rejected");
        }
    }
}

fn retry_restore(session: &mut DockspaceSession, bytes: &[u8]) {
    let restore = session
        .prepare_document_restore_json(bytes, |_, _| true)
        .unwrap();
    let mut frame = session.begin_document_restore_frame(&restore).unwrap();
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    frame.commit().unwrap();
}

#[test]
fn suspended_restore_rejects_stale_binding_without_reusing_external_item_ids() {
    let mut session = persistent_session();
    let bytes = session.save_document_json().unwrap();
    let update = suspend_restore(&mut session, &bytes);

    let late = session.ensure_external_item("late").unwrap();
    assert_stale_restore_rejected(&mut session, update);

    assert_eq!(session.item_id_for_external_key("late"), Some(late));
    let different = session.ensure_external_item("different").unwrap();
    assert_ne!(different, late);

    retry_restore(&mut session, &bytes);
    assert_eq!(session.item_id_for_external_key("late"), Some(late));
    assert_ne!(session.ensure_external_item("after-retry").unwrap(), late);
}

#[test]
fn suspended_restore_rejects_stale_binding_without_rewinding_save_generation() {
    let mut session = persistent_session();
    let bytes = session.save_document_json().unwrap();
    let update = suspend_restore(&mut session, &bytes);

    session.save_document_json().unwrap();
    let next_after_save = session.next_document_generation().unwrap();
    assert_stale_restore_rejected(&mut session, update);
    assert_eq!(session.next_document_generation(), Some(next_after_save));

    retry_restore(&mut session, &bytes);
    assert!(
        session
            .next_document_generation()
            .is_some_and(|next| next >= next_after_save)
    );
}
