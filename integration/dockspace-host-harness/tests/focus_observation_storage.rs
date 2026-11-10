//! Retired logical surfaces must not accumulate private focus-provider history.

use dockspace::geometry::{LogicalRect, LogicalSize};
use dockspace::model::{
    DockspaceLayout, DockspaceNode, DockspaceRootLayout, DockspaceSurfaceLayout, ItemId, RootId,
    SurfaceId,
};
use dockspace::policy::DockPolicy;
use dockspace::runtime::{
    DockspaceHostFocus, DockspaceSession, SurfacePresentationResult, UniformSurfaceMetrics,
};

#[global_allocator]
static ALLOCATOR: dhat::Alloc = dhat::Alloc;

const ITEM: ItemId = ItemId::new(1);

fn layout(surface: SurfaceId) -> DockspaceLayout {
    DockspaceLayout::new([DockspaceSurfaceLayout::new(
        surface,
        DockspaceRootLayout::new(RootId::new(1), DockspaceNode::central_tabs([ITEM])),
    )])
    .unwrap()
}

fn retained_bytes(history: u64, observe_focus: bool) -> usize {
    let profiler = dhat::Profiler::builder()
        .testing()
        .trim_backtraces(Some(2))
        .build();
    let mut session =
        DockspaceSession::from_layout(layout(SurfaceId::new(0)), DockPolicy::default()).unwrap();
    let metrics = UniformSurfaceMetrics::new(
        LogicalRect::new(0.0, 0.0, 640.0, 480.0).unwrap(),
        LogicalSize::new(32.0, 24.0).unwrap(),
        80.0,
    )
    .unwrap();
    for identity in 1..=history {
        let surface = SurfaceId::new(identity);
        let reset = session.prepare_layout_reset(layout(surface)).unwrap();
        let mut frame = session.begin_layout_reset_frame(&reset).unwrap();
        frame.measure_surface(surface, metrics).unwrap();
        frame.commit().unwrap();
        let mut frame = session.begin_host_frame().unwrap();
        let plan = frame.paint_plan(surface).unwrap().unwrap();
        let receipt = plan.execute_surface_program(|_| Ok::<(), ()>(())).unwrap();
        frame.confirm_surface_painted(receipt).unwrap();
        let output = frame
            .commit()
            .unwrap()
            .take_painted_outputs()
            .pop()
            .unwrap();
        session
            .report_surface_presentation(output, SurfacePresentationResult::Presented)
            .unwrap();
        let mut frame = session.begin_host_frame().unwrap();
        if observe_focus {
            assert!(
                frame
                    .report_host_focus(surface, DockspaceHostFocus::Pane(ITEM))
                    .unwrap()
            );
        }
        frame.commit().unwrap();
    }
    // Run normal maintenance after the last replacement and terminal result.
    session.begin_host_frame().unwrap().commit().unwrap();
    assert_eq!(session.view().surfaces().count(), 1);
    let bytes = dhat::HeapStats::get().curr_bytes;
    drop(session);
    drop(profiler);
    bytes
}

#[test]
fn focus_provider_storage_stays_bounded_when_logical_surfaces_are_replaced() {
    let mut extra = Vec::new();
    for history in [16, 1_024] {
        let without_focus = retained_bytes(history, false);
        let with_focus = retained_bytes(history, true);
        let focus_bytes = with_focus.saturating_sub(without_focus);
        eprintln!(
            "retired_surfaces={history} focus_provider_and_current_focus_bytes={focus_bytes}"
        );
        extra.push(focus_bytes);
    }
    assert!(
        extra[1] <= extra[0] + 2_048,
        "focus history must remain bounded after exact surface retirement: {extra:?}"
    );
}
