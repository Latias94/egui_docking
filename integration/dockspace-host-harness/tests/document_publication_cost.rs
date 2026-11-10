//! Ordinary document publication must not copy append-only identity history.
#![cfg(feature = "serde")]

use std::hint::black_box;

use dockspace::model::{
    DockspaceLayout, DockspaceNode, DockspaceRootLayout, DockspaceSurfaceLayout, ItemId, RootId,
    SurfaceId,
};
use dockspace::policy::DockPolicy;
use dockspace::runtime::{
    DockspaceDocumentBootstrap, DockspaceDocumentId, DockspaceSession, SurfaceUnavailableReason,
};

#[global_allocator]
static ALLOCATOR: dhat::Alloc = dhat::Alloc;

fn session(history: usize) -> DockspaceSession {
    let layout = DockspaceLayout::new([DockspaceSurfaceLayout::new(
        SurfaceId::new(1),
        DockspaceRootLayout::new(RootId::new(1), DockspaceNode::tabs([ItemId::new(1)])),
    )])
    .unwrap();
    if history == 0 {
        return DockspaceSession::from_layout(layout, DockPolicy::default()).unwrap();
    }
    let mut bootstrap =
        DockspaceDocumentBootstrap::new(DockspaceDocumentId::from_bytes([0xA7; 16]));
    bootstrap
        .ensure_items((0..history).map(|index| format!("historical-pane-{index:08}")))
        .unwrap();
    DockspaceSession::from_persistent_layout(layout, DockPolicy::default(), bootstrap).unwrap()
}

fn idle(session: &mut DockspaceSession) {
    let version = session.version();
    let mut frame = session.begin_host_frame().unwrap();
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    black_box(frame.commit().unwrap());
    assert_eq!(session.version(), version);
}

#[test]
fn idle_publication_allocations_do_not_grow_with_identity_history() {
    let mut baseline = None;
    for history in [1, 128, 1_024, 16_384] {
        let mut session = session(history);
        idle(&mut session);
        let profiler = dhat::Profiler::builder()
            .testing()
            .trim_backtraces(Some(4))
            .build();
        let before = dhat::HeapStats::get();
        idle(&mut session);
        let after = dhat::HeapStats::get();
        let allocations = after.total_blocks - before.total_blocks;
        let bytes = after.total_bytes - before.total_bytes;
        drop(profiler);
        let expected = *baseline.get_or_insert((allocations, bytes));
        assert_eq!((allocations, bytes), expected, "historical_keys={history}");
    }
}
