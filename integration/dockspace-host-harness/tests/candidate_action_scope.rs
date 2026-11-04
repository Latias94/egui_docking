//! Current-candidate semantic actions track the candidate version they observe.

use dockspace::geometry::{LogicalRect, LogicalSize};
use dockspace::model::{
    DockspaceAxis, DockspaceContainedLayout, DockspaceLayout, DockspaceNode, DockspaceRootLayout,
    DockspaceSurfaceLayout, FloatingPresentationId, ItemId, RootId, SurfaceId,
};
use dockspace::policy::DockPolicy;
use dockspace::runtime::{
    ContainedResizeDirection, DockspaceHostFrame, DockspaceSession, HostInputOutcome,
    SurfaceContainedResizeAdjustment, SurfacePaintPlan, SurfacePaintProgramOperation as Operation,
    SurfacePaintProgramStep as Step, SurfaceSplitterAdjustment, UniformSurfaceMetrics,
};

const SURFACE_A: SurfaceId = SurfaceId::new(1);
const SURFACE_B: SurfaceId = SurfaceId::new(2);
const A1: ItemId = ItemId::new(1);
const A2: ItemId = ItemId::new(2);
const B1: ItemId = ItemId::new(3);
const B2: ItemId = ItemId::new(4);
const B3: ItemId = ItemId::new(5);
const FLOAT: FloatingPresentationId = FloatingPresentationId::new(1);

fn session() -> DockspaceSession {
    let content_b = DockspaceNode::equal_split(
        DockspaceAxis::Horizontal,
        [
            DockspaceNode::central_tabs([B1, B2]),
            DockspaceNode::tabs([B3]),
        ],
    )
    .expect("B split validates");
    let layout = DockspaceLayout::new([
        DockspaceSurfaceLayout::new(
            SURFACE_A,
            DockspaceRootLayout::new(RootId::new(1), DockspaceNode::central_tabs([A1, A2])),
        ),
        DockspaceSurfaceLayout::new(
            SURFACE_B,
            DockspaceRootLayout::new(RootId::new(2), content_b),
        )
        .with_contained(DockspaceContainedLayout::new(
            FLOAT,
            DockspaceRootLayout::new(RootId::new(3), DockspaceNode::tabs([ItemId::new(6)])),
            LogicalRect::new(80.0, 300.0, 360.0, 240.0).unwrap(),
        )),
    ])
    .expect("layout validates");
    let mut session =
        DockspaceSession::from_layout(layout, DockPolicy::default()).expect("session initializes");
    let metrics = UniformSurfaceMetrics::new(
        LogicalRect::new(0.0, 0.0, 800.0, 600.0).expect("valid bounds"),
        LogicalSize::new(32.0, 24.0).expect("valid minimum"),
        80.0,
    )
    .expect("valid metrics");
    let mut frame = session
        .begin_host_frame()
        .expect("measurement frame begins");
    frame
        .measure_surface(SURFACE_A, metrics)
        .expect("A measures");
    frame
        .measure_surface(SURFACE_B, metrics)
        .expect("B measures");
    frame.commit().expect("measurements publish");
    session
}

fn splitter_action(plan: SurfacePaintPlan<'_>) -> dockspace::runtime::PreparedSurfaceAction {
    let mut action = None;
    plan.visit_surface_program(|step| {
        if let Step::Operation {
            operation: Operation::Splitter(splitter),
            ..
        } = step
        {
            action = plan.prepare_splitter_adjustment(
                splitter.visual_id(),
                SurfaceSplitterAdjustment::Increment,
            );
        }
        Ok::<(), ()>(())
    })
    .expect("B surface program validates");
    action.expect("B exposes an operable splitter")
}

fn select_b(frame: &mut DockspaceHostFrame<'_>) -> dockspace::runtime::PreparedSurfaceAction {
    frame
        .paint_plan(SURFACE_B)
        .expect("B lookup succeeds")
        .expect("B remains ready")
        .prepare_tab_select(B2)
        .expect("B2 selection is available")
}

#[test]
fn current_action_prepared_after_independent_selection_uses_current_candidate() {
    let mut session = session();
    assert!(!session.view().item(B2).unwrap().is_selected());
    let mut frame = session.begin_host_frame().expect("input frame begins");
    let initial = frame.version();
    frame.select_item_current(A2).expect("A selection stages");
    assert_ne!(
        initial,
        frame.version(),
        "A selection advances the candidate"
    );

    let action = select_b(&mut frame);
    assert_eq!(action.expected_version(), frame.version());
    frame
        .submit_surface_action(action)
        .expect("current B action stages");
    let report = frame.commit().expect("both selections publish atomically");

    assert!(
        session
            .view()
            .item(A2)
            .expect("A2 remains open")
            .is_selected()
    );
    assert!(
        session
            .view()
            .item(B2)
            .expect("B2 remains open")
            .is_selected()
    );
    assert!(
        !report
            .inputs()
            .iter()
            .any(|outcome| matches!(outcome, HostInputOutcome::StaleRejected { .. })),
        "a current-candidate action must not be compared with the frame's initial version: {:?}",
        report.inputs(),
    );
}

#[test]
fn contained_resize_uses_the_version_of_the_candidate_it_observed() {
    for stale in [false, true] {
        let mut session = session();
        let before = session.view().contained(FLOAT).unwrap().rect();
        let mut frame = session.begin_host_frame().unwrap();
        let prepare = |frame: &mut DockspaceHostFrame<'_>| {
            frame
                .paint_plan(SURFACE_B)
                .unwrap()
                .unwrap()
                .prepare_contained_resize_adjustment(
                    FLOAT,
                    ContainedResizeDirection::East,
                    SurfaceContainedResizeAdjustment::Increment,
                )
                .unwrap()
        };
        let old_action = stale.then(|| prepare(&mut frame));
        frame.select_item_current(A2).unwrap();
        let action = old_action.unwrap_or_else(|| prepare(&mut frame));
        frame.submit_surface_action(action).unwrap();
        let report = frame.commit().unwrap();
        assert_eq!(
            report
                .inputs()
                .iter()
                .any(|outcome| matches!(outcome, HostInputOutcome::StaleRejected { .. })),
            stale
        );
        let after = session.view().contained(FLOAT).unwrap().rect();
        if stale {
            assert_eq!(after, before);
        } else {
            assert!(after.width() > before.width());
        }
        assert!(session.view().item(A2).unwrap().is_selected());
    }
}

#[test]
fn action_captured_before_candidate_advance_remains_stale() {
    let mut session = session();
    let mut frame = session.begin_host_frame().expect("input frame begins");
    let action = select_b(&mut frame);
    frame.select_item_current(A2).expect("A selection stages");
    frame
        .submit_surface_action(action)
        .expect("stale action stages for reducer validation");
    let report = frame.commit().expect("candidate publishes");

    assert!(
        session
            .view()
            .item(A2)
            .expect("A2 remains open")
            .is_selected()
    );
    assert!(
        !session
            .view()
            .item(B2)
            .expect("B2 remains open")
            .is_selected()
    );
    assert!(
        report
            .inputs()
            .iter()
            .any(|outcome| matches!(outcome, HostInputOutcome::StaleRejected { .. }))
    );
}

#[test]
fn action_from_another_session_is_rejected_at_submission() {
    let mut source = session();
    let action = {
        let mut frame = source.begin_host_frame().expect("source frame begins");
        let action = select_b(&mut frame);
        frame.commit().expect("source frame commits");
        action
    };
    let mut destination = session();
    let mut frame = destination
        .begin_host_frame()
        .expect("destination frame begins");

    assert!(frame.submit_surface_action(action).is_err());
    frame
        .commit()
        .expect("rejected submission leaves frame usable");
    assert!(
        !destination
            .view()
            .item(B2)
            .expect("B2 remains open")
            .is_selected()
    );
}

#[test]
fn current_splitter_action_prepared_after_independent_selection_uses_current_candidate() {
    let mut session = session();
    let weights_before = session
        .view()
        .root(RootId::new(2))
        .expect("B root exists")
        .content()
        .expect("B root has content")
        .split()
        .expect("B content is split")
        .weights()
        .collect::<Vec<_>>();
    let mut frame = session.begin_host_frame().expect("input frame begins");
    frame.select_item_current(A2).expect("A selection stages");
    let action = splitter_action(
        frame
            .paint_plan(SURFACE_B)
            .expect("B lookup succeeds")
            .expect("unchanged B remains ready"),
    );
    assert_eq!(action.expected_version(), frame.version());
    frame
        .submit_surface_action(action)
        .expect("current B splitter action stages");
    let report = frame.commit().expect("both inputs publish atomically");
    let weights_after = session
        .view()
        .root(RootId::new(2))
        .expect("B root remains")
        .content()
        .expect("B root retains content")
        .split()
        .expect("B remains split")
        .weights()
        .collect::<Vec<_>>();

    assert_ne!(
        weights_after, weights_before,
        "the current action adjusts B"
    );
    assert!(
        !report
            .inputs()
            .iter()
            .any(|outcome| matches!(outcome, HostInputOutcome::StaleRejected { .. }))
    );
}
