//! Audit-only probe: one live pane, increasing retained external-key history.
//!
//! Copy into integration/dockspace-host-harness/examples/architecture_document_cost.rs
//! and run that example in release mode with --features serde. Remove that copy
//! afterwards. The existing harness supplies dockspace and dhat dependencies.

use std::hint::black_box;
use std::time::Instant;

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

fn main() {
    const BATCHES: usize = 9;
    const FRAMES: usize = 64;
    println!(
        "live_panes,historical_keys,mode,ns_per_frame_p50,allocations_per_frame,requested_bytes_per_frame"
    );
    for history in [0, 1, 128, 1_024, 16_384] {
        let mut session = session(history);
        for _ in 0..8 {
            idle(&mut session);
        }
        // Timing runs without active heap profiling. Construction and key
        // allocation are deliberately excluded from both measured regions.
        let mut samples = [0_u128; BATCHES];
        for sample in &mut samples {
            let start = Instant::now();
            for _ in 0..FRAMES {
                idle(&mut session);
            }
            *sample = start.elapsed().as_nanos() / FRAMES as u128;
        }
        samples.sort_unstable();
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
        println!(
            "1,{history},{},{},{allocations},{bytes}",
            if history == 0 {
                "unbound"
            } else {
                "persistent"
            },
            samples[BATCHES / 2],
        );
    }
}
