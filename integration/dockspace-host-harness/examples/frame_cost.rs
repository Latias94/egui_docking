//! Opt-in local frame-cost samples; no wall-clock pass/fail threshold.

use std::hint::black_box;
use std::time::Instant;

use dockspace::geometry::{LogicalRect, LogicalSize};
use dockspace::model::{
    DockspaceAxis, DockspaceLayout, DockspaceNode, DockspaceRootLayout, DockspaceSurfaceLayout,
    ItemId, RootId, SurfaceId,
};
use dockspace::policy::DockPolicy;
use dockspace::runtime::{DockspaceSession, SurfaceUnavailableReason, UniformSurfaceMetrics};

fn tree(first: u64, count: usize, axis: DockspaceAxis) -> DockspaceNode {
    if count == 1 {
        return DockspaceNode::tabs([ItemId::new(first * 2), ItemId::new(first * 2 + 1)]);
    }
    let half = count / 2;
    let next = match axis {
        DockspaceAxis::Horizontal => DockspaceAxis::Vertical,
        DockspaceAxis::Vertical => DockspaceAxis::Horizontal,
    };
    DockspaceNode::equal_split(
        axis,
        [
            tree(first, half, next),
            tree(first + u64::try_from(half).unwrap(), count - half, next),
        ],
    )
    .unwrap()
}

fn main() {
    for count in [16, 128, 1_024] {
        let surface = SurfaceId::new(1);
        let layout = DockspaceLayout::new([DockspaceSurfaceLayout::new(
            surface,
            DockspaceRootLayout::new(RootId::new(1), tree(1, count, DockspaceAxis::Horizontal)),
        )])
        .unwrap();
        let mut session = DockspaceSession::from_layout(layout, DockPolicy::default()).unwrap();
        let mut frame = session.begin_host_frame().unwrap();
        frame
            .measure_surface(
                surface,
                UniformSurfaceMetrics::new(
                    LogicalRect::new(0.0, 0.0, 16_384.0, 16_384.0).unwrap(),
                    LogicalSize::new(8.0, 8.0).unwrap(),
                    64.0,
                )
                .unwrap(),
            )
            .unwrap();
        frame.commit().unwrap();
        let initial = session.version();
        let mut idle = Vec::new();
        for _ in 0..5 {
            let start = Instant::now();
            for _ in 0..200 {
                let mut frame = session.begin_host_frame().unwrap();
                frame
                    .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
                    .unwrap();
                black_box(frame.commit().unwrap());
            }
            idle.push(start.elapsed().as_secs_f64() * 1_000_000.0 / 200.0);
        }
        assert_eq!(session.version(), initial);
        let mut touched = Vec::new();
        for _ in 0..5 {
            let start = Instant::now();
            for step in 0..50 {
                let mut frame = session.begin_host_frame().unwrap();
                frame
                    .select_item_current(ItemId::new(if step % 2 == 0 { 3 } else { 2 }))
                    .unwrap();
                frame
                    .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
                    .unwrap();
                assert!(black_box(frame.commit().unwrap()).workspace_changed());
            }
            touched.push(start.elapsed().as_secs_f64() * 1_000_000.0 / 50.0);
        }
        println!("panes={count} idle_frame_us={idle:?} selected_frame_us={touched:?}");
    }
}
