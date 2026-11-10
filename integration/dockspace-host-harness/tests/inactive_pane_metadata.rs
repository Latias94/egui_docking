//! Inactive content still supplies metadata for exact potential docking layouts.

use dockspace::geometry::{LogicalRect, LogicalSize};
use dockspace::model::{
    DockspaceLayout, DockspaceNode, DockspaceRootLayout, DockspaceSurfaceLayout, ItemId, RootId,
    SurfaceId,
};
use dockspace::policy::DockPolicy;
use dockspace::runtime::{
    DockspaceSession, SurfaceMeasurementAnswer as Answer, SurfaceMeasurementRequest as Request,
    SurfaceUnavailableReason, SurfaceWork, TabStripMetrics,
};

const SURFACE: SurfaceId = SurfaceId::new(1);
const FIRST: ItemId = ItemId::new(1);
const SECOND: ItemId = ItemId::new(2);

#[test]
fn missing_inactive_minimum_defers_output_and_never_blocks_accepted_selection() {
    let layout = DockspaceLayout::new([DockspaceSurfaceLayout::new(
        SURFACE,
        DockspaceRootLayout::new(RootId::new(1), DockspaceNode::tabs([FIRST, SECOND])),
    )])
    .unwrap();
    let mut session = DockspaceSession::from_layout(layout, DockPolicy::default()).unwrap();
    for (missing, select_second) in [(true, false), (true, true), (false, false)] {
        let mut frame = session.begin_host_frame().unwrap();
        if select_second {
            frame.select_item_current(SECOND).unwrap();
        }
        let mut requested = Vec::new();
        frame
            .measure_surface_with(SURFACE, |request| match request {
                Request::DockBounds { .. } | Request::PopupPlaneBounds { .. } => {
                    Answer::Bounds(LogicalRect::new(0.0, 0.0, 800.0, 600.0).unwrap())
                }
                Request::PaneMinimum { item, .. } => {
                    requested.push(item);
                    if missing && item == Some(SECOND) {
                        Answer::Unavailable(SurfaceUnavailableReason::ContentUnavailable)
                    } else {
                        Answer::PaneMinimum(LogicalSize::new(64.0, 48.0).unwrap())
                    }
                }
                Request::TabIntrinsic { .. } => Answer::TabIntrinsic(96.0),
                Request::TabStrip { .. } => {
                    Answer::TabStrip(TabStripMetrics::new(0.0, 0.0).unwrap())
                }
            })
            .unwrap();
        assert_eq!(requested, [Some(FIRST), Some(SECOND)]);
        assert_eq!(
            matches!(frame.surface_work(SURFACE).unwrap(), SurfaceWork::Ready(_)),
            !missing
        );
        frame.commit().unwrap();
        assert!(session.view().item(FIRST).is_some());
        assert!(session.view().item(SECOND).is_some());
        if select_second {
            assert!(session.view().item(SECOND).unwrap().is_selected());
        }
    }
}
