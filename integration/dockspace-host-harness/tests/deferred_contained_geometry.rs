//! Deferred contained resizes retain only their exact geometry authority.

use dockspace::geometry::{LogicalRect, LogicalSize};
use dockspace::model::{
    DockspaceContainedLayout, DockspaceLayout, DockspaceNode, DockspaceRootLayout,
    DockspaceSurfaceLayout, FloatingPresentationId, ItemId, RootId, SurfaceId,
};
use dockspace::policy::DockPolicy;
use dockspace::runtime::{
    ContainedResizeDirection, DeferredSurfaceAction, DockPresentationConfig, DockspaceSession,
    HostFrameReport, HostInputOutcome, SurfaceContainedResizeAdjustment,
    SurfaceMeasurementAnswer as Answer, SurfaceMeasurementRequest as Question, SurfacePaintPlan,
    SurfacePaintProgramStep as Step, TabStripMetrics,
};

const SURFACE: SurfaceId = SurfaceId::new(1);
const MAIN_ROOT: RootId = RootId::new(1);
const FLOAT_ROOT: RootId = RootId::new(2);
const FLOAT: FloatingPresentationId = FloatingPresentationId::new(1);
const MAIN_ITEM: ItemId = ItemId::new(1);
const FLOAT_ITEM: ItemId = ItemId::new(2);
const STEP: f64 = 16.0;

fn rect(x: f64, y: f64, width: f64, height: f64) -> LogicalRect {
    LogicalRect::new(x, y, width, height).unwrap()
}

fn session() -> DockspaceSession {
    let layout = DockspaceLayout::new([DockspaceSurfaceLayout::new(
        SURFACE,
        DockspaceRootLayout::new(MAIN_ROOT, DockspaceNode::central_tabs([MAIN_ITEM])),
    )
    .with_contained(DockspaceContainedLayout::new(
        FLOAT,
        DockspaceRootLayout::new(FLOAT_ROOT, DockspaceNode::tabs([FLOAT_ITEM])),
        rect(80.0, 100.0, 360.0, 260.0),
    ))])
    .unwrap();
    DockspaceSession::from_layout_with_presentation_config(
        layout,
        DockPolicy::default(),
        DockPresentationConfig::builder()
            .splitter_keyboard_step(STEP)
            .build()
            .unwrap(),
    )
    .unwrap()
}

fn measure_frame(
    frame: &mut dockspace::runtime::DockspaceHostFrame<'_>,
    surface_width: f64,
    floating_minimum_width: f64,
    background_minimum_width: f64,
) {
    frame
        .measure_surface_with(SURFACE, |question| match question {
            Question::DockBounds { .. } | Question::PopupPlaneBounds { .. } => {
                Answer::Bounds(rect(0.0, 0.0, surface_width, 650.0))
            }
            Question::PaneMinimum { item, .. } => Answer::PaneMinimum(
                LogicalSize::new(
                    if item == Some(FLOAT_ITEM) {
                        floating_minimum_width
                    } else {
                        background_minimum_width
                    },
                    48.0,
                )
                .unwrap(),
            ),
            Question::TabIntrinsic { .. } => Answer::TabIntrinsic(96.0),
            Question::TabStrip { .. } => Answer::TabStrip(TabStripMetrics::new(0.0, 0.0).unwrap()),
        })
        .unwrap();
}

fn measure(
    session: &mut DockspaceSession,
    surface_width: f64,
    floating_minimum_width: f64,
    background_minimum_width: f64,
) {
    let mut frame = session.begin_host_frame().unwrap();
    measure_frame(
        &mut frame,
        surface_width,
        floating_minimum_width,
        background_minimum_width,
    );
    let report = frame.commit().unwrap();
    assert!(!report.workspace_changed());
}

fn resize(plan: SurfacePaintPlan<'_>) -> (LogicalSize, DeferredSurfaceAction) {
    let mut minimum = None;
    plan.visit_surface_program(|step| {
        if let Step::RootBegin { root, .. } = step
            && root.root() == FLOAT_ROOT
        {
            minimum = Some(root.contained().unwrap().minimum_size());
        }
        Ok::<(), ()>(())
    })
    .unwrap();
    let action = plan
        .prepare_contained_resize_adjustment(
            FLOAT,
            ContainedResizeDirection::East,
            SurfaceContainedResizeAdjustment::Decrement,
        )
        .expect("the east resize edge remains operable")
        .into_deferred();
    (minimum.unwrap(), action)
}

fn capture(session: &mut DockspaceSession) -> (LogicalSize, DeferredSurfaceAction) {
    let mut frame = session.begin_host_frame().unwrap();
    resize(frame.paint_plan(SURFACE).unwrap().unwrap())
}

fn submit(session: &mut DockspaceSession, action: DeferredSurfaceAction) -> HostFrameReport {
    let mut frame = session.begin_host_frame().unwrap();
    frame.submit_deferred_surface_action(action).unwrap();
    frame.commit().unwrap()
}

#[test]
fn deferred_contained_resize_rejects_a_superseded_minimum_without_a_workspace_change() {
    let mut session = session();
    measure(&mut session, 900.0, 64.0, 64.0);
    let (_, old_action) = capture(&mut session);
    let version = session.version();
    let before = session.view().contained(FLOAT).unwrap().rect();

    measure(&mut session, 900.0, 350.0, 64.0);
    assert_eq!(session.version(), version);
    assert_eq!(session.view().contained(FLOAT).unwrap().rect(), before);
    let (minimum, current_action) = capture(&mut session);
    assert!(minimum.width() > before.width() - STEP);
    assert!(minimum.width() < before.width());

    let report = submit(&mut session, old_action);
    assert!(matches!(
        report.inputs(),
        [HostInputOutcome::StaleRejected { .. }]
    ));
    assert!(!report.workspace_changed());
    assert_eq!(session.version(), version);
    assert_eq!(session.view().contained(FLOAT).unwrap().rect(), before);

    assert!(submit(&mut session, current_action).workspace_changed());
    let after = session.view().contained(FLOAT).unwrap().rect();
    assert_eq!(after.width(), minimum.width());
    assert_eq!(after.x(), before.x());
    assert_eq!(after.y(), before.y());
    assert_eq!(after.height(), before.height());
}

#[test]
fn deferred_contained_resize_rejects_superseded_surface_bounds_without_a_workspace_change() {
    let mut session = session();
    measure(&mut session, 900.0, 64.0, 64.0);
    let (_, old_action) = capture(&mut session);
    let version = session.version();
    let before = session.view().contained(FLOAT).unwrap().rect();

    let surface_width = 420.0;
    measure(&mut session, surface_width, 64.0, 64.0);
    assert_eq!(session.version(), version);
    assert_eq!(session.view().contained(FLOAT).unwrap().rect(), before);
    let (_, current_action) = capture(&mut session);

    let report = submit(&mut session, old_action);
    assert!(matches!(
        report.inputs(),
        [HostInputOutcome::StaleRejected { .. }]
    ));
    assert!(!report.workspace_changed());
    assert_eq!(session.version(), version);
    assert_eq!(session.view().contained(FLOAT).unwrap().rect(), before);

    assert!(submit(&mut session, current_action).workspace_changed());
    let after = session.view().contained(FLOAT).unwrap().rect();
    assert_eq!(after.max().x(), surface_width);
    assert_eq!(after.x(), before.x());
    assert_eq!(after.y(), before.y());
    assert_eq!(after.height(), before.height());
}

#[test]
fn discarded_first_pass_can_supply_a_deferred_contained_resize_witness() {
    let mut session = session();
    let version = session.version();
    let before = session.view().contained(FLOAT).unwrap().rect();
    let action = {
        let mut discarded = session.begin_host_frame().unwrap();
        measure_frame(&mut discarded, 900.0, 64.0, 64.0);
        resize(discarded.paint_plan(SURFACE).unwrap().unwrap()).1
    };
    assert_eq!(session.version(), version);
    assert_eq!(session.view().contained(FLOAT).unwrap().rect(), before);

    assert!(submit(&mut session, action).workspace_changed());
    assert_eq!(
        session.view().contained(FLOAT).unwrap().rect().width(),
        before.width() - STEP
    );
}

#[test]
fn unrelated_background_minimum_does_not_expire_a_deferred_contained_resize() {
    let mut session = session();
    measure(&mut session, 900.0, 64.0, 64.0);
    let (minimum, action) = capture(&mut session);
    let version = session.version();
    let before = session.view().contained(FLOAT).unwrap().rect();

    measure(&mut session, 900.0, 64.0, 200.0);
    assert_eq!(session.version(), version);
    assert_eq!(session.view().contained(FLOAT).unwrap().rect(), before);
    assert_eq!(capture(&mut session).0, minimum);

    assert!(submit(&mut session, action).workspace_changed());
    assert_eq!(
        session.view().contained(FLOAT).unwrap().rect().width(),
        before.width() - STEP
    );
}
