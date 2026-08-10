use std::collections::BTreeSet;

use crate::geometry::{LogicalRect, LogicalSize};
use crate::ids::{ItemId, RootId, SurfaceId};
use crate::model::{DockspaceLayout, DockspaceNode, DockspaceRootLayout, DockspaceSurfaceLayout};
use crate::policy::{DockPolicy, TabBarVisibility};
use crate::runtime::{
    DockspaceInteractionError, DockspaceSession, DockspaceVisualKind, SurfaceMeasurementAnswer,
    SurfaceMeasurementRequest, TabStripMetrics, UniformSurfaceMetrics,
};

const SURFACE: SurfaceId = SurfaceId::new(1);
const ROOT: RootId = RootId::new(1);
const FIRST: ItemId = ItemId::new(1);
const SECOND: ItemId = ItemId::new(2);

fn session() -> DockspaceSession {
    let layout = DockspaceLayout::new([DockspaceSurfaceLayout::new(
        SURFACE,
        DockspaceRootLayout::new(ROOT, DockspaceNode::central_tabs([FIRST, SECOND])),
    )])
    .expect("measurement test layout validates");
    DockspaceSession::from_layout(layout, DockPolicy::default())
        .expect("measurement test session initializes")
}

fn bounds() -> LogicalRect {
    LogicalRect::new(0.0, 0.0, 800.0, 600.0).expect("test bounds validate")
}

fn minimum() -> LogicalSize {
    LogicalSize::new(64.0, 48.0).expect("test minimum validates")
}

#[test]
fn product_measurement_seam_hides_manifest_keys_and_builds_a_ready_plan() {
    let mut session = session();
    let mut pane_items = BTreeSet::new();
    let mut tab_items = BTreeSet::new();
    let mut tab_strips = 0;
    let mut dock_bounds = 0;

    let mut frame = session
        .begin_host_frame()
        .expect("measurement frame begins");
    frame
        .measure_surface_with(SURFACE, |request| match request {
            SurfaceMeasurementRequest::DockBounds { surface } => {
                assert_eq!(surface, SURFACE);
                dock_bounds += 1;
                SurfaceMeasurementAnswer::Bounds(bounds())
            }
            SurfaceMeasurementRequest::PopupPlaneBounds { surface } => {
                assert_eq!(surface, SURFACE);
                SurfaceMeasurementAnswer::Bounds(bounds())
            }
            SurfaceMeasurementRequest::PaneMinimum { visual, item } => {
                assert_eq!(visual.kind(), DockspaceVisualKind::Pane);
                pane_items.insert(item);
                SurfaceMeasurementAnswer::PaneMinimum(minimum())
            }
            SurfaceMeasurementRequest::TabIntrinsic { visual, item } => {
                assert_eq!(visual.kind(), DockspaceVisualKind::Tab);
                tab_items.insert(item);
                SurfaceMeasurementAnswer::TabIntrinsic(96.0)
            }
            SurfaceMeasurementRequest::TabStrip { visual, visibility } => {
                assert_eq!(visual.kind(), DockspaceVisualKind::TabBar);
                assert_eq!(visibility, TabBarVisibility::Visible);
                tab_strips += 1;
                SurfaceMeasurementAnswer::TabStrip(
                    TabStripMetrics::new(0.0, 0.0).expect("test strip metrics validate"),
                )
            }
        })
        .expect("product measurement answers satisfy the frozen manifest");
    frame.commit().expect("measurement frame commits");

    assert_eq!(dock_bounds, 1);
    assert_eq!(pane_items, BTreeSet::from([Some(FIRST), Some(SECOND)]));
    assert_eq!(tab_items, BTreeSet::from([FIRST, SECOND]));
    assert_eq!(tab_strips, 1);

    let mut paint = session.begin_host_frame().expect("paint frame begins");
    assert!(
        paint
            .paint_plan(SURFACE)
            .expect("ready plan lookup succeeds")
            .is_some()
    );
}

#[test]
fn mismatched_measurement_answer_rolls_back_without_poisoning_the_session() {
    let mut session = session();
    let error = session
        .begin_host_frame()
        .expect("invalid measurement frame begins")
        .measure_surface_with(
            SURFACE,
            |_| SurfaceMeasurementAnswer::PaneMinimum(minimum()),
        )
        .expect_err("a pane-size answer cannot satisfy a bounds request");
    assert_eq!(
        error.interaction_error(),
        Some(&DockspaceInteractionError::MeasurementAnswerMismatch)
    );

    let metrics = UniformSurfaceMetrics::new(bounds(), minimum(), 96.0)
        .expect("uniform recovery metrics validate");
    let mut retry = session.begin_host_frame().expect("retry frame begins");
    retry
        .measure_surface(SURFACE, metrics)
        .expect("the unchanged session accepts a fresh exact contribution");
    retry.commit().expect("retry frame commits");
}
