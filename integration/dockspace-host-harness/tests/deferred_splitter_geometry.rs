//! Deferred discrete splitter actions cannot reuse stale layout geometry.

use dockspace::geometry::{LogicalRect, LogicalSize};
use dockspace::model::{
    DockspaceAxis, DockspaceLayout, DockspaceNode, DockspaceRootLayout, DockspaceSurfaceLayout,
    ItemId, RootId, SurfaceId,
};
use dockspace::policy::DockPolicy;
use dockspace::runtime::{
    DeferredSurfaceAction, DockspaceSession, HostInputOutcome, SurfacePaintPlan,
    SurfacePaintProgramOperation as Operation, SurfacePaintProgramStep as Step,
    SurfaceSplitterAdjustment, UniformSurfaceMetrics,
};

const SURFACE: SurfaceId = SurfaceId::new(1);
const ROOT: RootId = RootId::new(1);

fn measure(session: &mut DockspaceSession, width: f64) {
    let mut frame = session
        .begin_host_frame()
        .expect("measurement frame begins");
    frame
        .measure_surface(
            SURFACE,
            UniformSurfaceMetrics::new(
                LogicalRect::new(0.0, 0.0, width, 600.0).expect("valid bounds"),
                LogicalSize::new(32.0, 24.0).expect("valid minimum"),
                80.0,
            )
            .expect("valid metrics"),
        )
        .expect("surface measures");
    frame.commit().expect("measurement publishes");
}

fn splitter(plan: SurfacePaintPlan<'_>) -> (f64, DeferredSurfaceAction) {
    let mut result = None;
    plan.visit_surface_program(|step| {
        if let Step::Operation {
            operation: Operation::Splitter(record),
            ..
        } = step
        {
            let action = plan
                .prepare_splitter_adjustment(
                    record.visual_id(),
                    SurfaceSplitterAdjustment::Increment,
                )
                .expect("splitter increment is available")
                .into_deferred();
            result = Some((record.draw_bounds().x(), action));
        }
        Ok::<(), ()>(())
    })
    .expect("surface program validates");
    result.expect("one splitter exists")
}

fn session() -> DockspaceSession {
    let content = DockspaceNode::equal_split(
        DockspaceAxis::Horizontal,
        [
            DockspaceNode::central_tabs([ItemId::new(1)]),
            DockspaceNode::tabs([ItemId::new(2)]),
        ],
    )
    .expect("split validates");
    let layout = DockspaceLayout::new([DockspaceSurfaceLayout::new(
        SURFACE,
        DockspaceRootLayout::new(ROOT, content),
    )])
    .expect("layout validates");
    DockspaceSession::from_layout(layout, DockPolicy::default()).expect("session initializes")
}

#[test]
fn deferred_splitter_action_is_rejected_after_its_geometry_basis_changes() {
    let mut session = session();
    measure(&mut session, 800.0);
    let old_action = {
        let mut frame = session.begin_host_frame().expect("capture frame begins");
        splitter(
            frame
                .paint_plan(SURFACE)
                .expect("plan lookup")
                .expect("800px plan is ready"),
        )
        .1
    };

    measure(&mut session, 1600.0);
    let mut frame = session.begin_host_frame().expect("deferred frame begins");
    let (before, _) = splitter(
        frame
            .paint_plan(SURFACE)
            .expect("plan lookup")
            .expect("1600px plan is ready"),
    );
    frame
        .submit_deferred_surface_action(old_action)
        .expect("old action stages for reducer validation");
    let report = frame.commit().expect("deferred frame publishes");

    assert!(
        report
            .inputs()
            .iter()
            .any(|outcome| matches!(outcome, HostInputOutcome::StaleRejected { .. }))
    );
    measure(&mut session, 1600.0);
    let after = {
        let mut frame = session.begin_host_frame().expect("inspection frame begins");
        splitter(
            frame
                .paint_plan(SURFACE)
                .expect("plan lookup")
                .expect("final plan is ready"),
        )
        .0
    };
    assert_eq!(
        after, before,
        "stale discrete intent must not resize the split"
    );
}

#[test]
fn splitter_action_prepared_against_current_geometry_still_applies() {
    let mut session = session();
    measure(&mut session, 1600.0);
    let mut frame = session.begin_host_frame().expect("current frame begins");
    let (before, action) = splitter(
        frame
            .paint_plan(SURFACE)
            .expect("plan lookup")
            .expect("current plan is ready"),
    );
    frame
        .submit_deferred_surface_action(action)
        .expect("current action stages");
    let report = frame.commit().expect("current resize publishes");

    assert!(
        !report
            .inputs()
            .iter()
            .any(|outcome| matches!(outcome, HostInputOutcome::StaleRejected { .. }))
    );
    measure(&mut session, 1600.0);
    let after = {
        let mut frame = session.begin_host_frame().expect("inspection frame begins");
        splitter(
            frame
                .paint_plan(SURFACE)
                .expect("plan lookup")
                .expect("final plan is ready"),
        )
        .0
    };
    assert!(after > before, "a current increment must move the splitter");
}
