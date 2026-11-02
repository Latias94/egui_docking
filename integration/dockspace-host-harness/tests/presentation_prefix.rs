//! Presentation prefix retirement preserves the last actually presented output.

use dockspace::geometry::{LogicalRect, LogicalSize};
use dockspace::model::{
    DockspaceLayout, DockspaceNode, DockspaceRootLayout, DockspaceSurfaceLayout, ItemId, RootId,
    SurfaceId,
};
use dockspace::policy::DockPolicy;
use dockspace::runtime::{
    DockspaceReceiverDescriptor, DockspaceSession, HostFrameReport, PaintedSurfaceOutput,
    SurfacePresentationResult, SurfaceUnavailableReason, UniformSurfaceMetrics,
};
use dockspace_host_harness::RecordingRenderer;

const SURFACE: SurfaceId = SurfaceId::new(1);
const FIRST: ItemId = ItemId::new(1);

fn session() -> DockspaceSession {
    let layout = DockspaceLayout::new([DockspaceSurfaceLayout::new(
        SURFACE,
        DockspaceRootLayout::new(RootId::new(1), DockspaceNode::tabs([FIRST, ItemId::new(2)])),
    )])
    .unwrap();
    let mut session = DockspaceSession::from_layout(layout, DockPolicy::default()).unwrap();
    let mut frame = session.begin_host_frame().unwrap();
    frame
        .measure_surface(
            SURFACE,
            UniformSurfaceMetrics::new(
                LogicalRect::new(0.0, 0.0, 800.0, 600.0).unwrap(),
                LogicalSize::new(64.0, 48.0).unwrap(),
                96.0,
            )
            .unwrap(),
        )
        .unwrap();
    frame.commit().unwrap();
    session
}

fn paint(session: &mut DockspaceSession) -> (DockspaceReceiverDescriptor, PaintedSurfaceOutput) {
    let mut frame = session.begin_host_frame().unwrap();
    let plan = frame.paint_plan(SURFACE).unwrap().unwrap();
    let receiver = plan.tab_receiver(FIRST).unwrap();
    let (_, receipt) = RecordingRenderer::execute(plan).unwrap();
    frame.confirm_surface_painted(receipt).unwrap();
    let mut outputs = frame.commit().unwrap().take_painted_outputs();
    assert_eq!(outputs.len(), 1);
    (receiver, outputs.pop().unwrap())
}

fn settle(session: &mut DockspaceSession) -> HostFrameReport {
    let mut frame = session.begin_host_frame().unwrap();
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    frame.commit().unwrap()
}

#[test]
fn later_dropped_output_preserves_the_last_actually_presented_output() {
    let mut session = session();
    let (receiver, presented) = paint(&mut session);
    let (same_receiver, dropped) = paint(&mut session);
    assert_eq!(receiver, same_receiver);
    let artifact = presented.presentation_artifact();
    let dropped_artifact = dropped.presentation_artifact();
    session
        .report_surface_presentation(presented, SurfacePresentationResult::Presented)
        .unwrap();
    session
        .report_surface_presentation(dropped, SurfacePresentationResult::Dropped)
        .unwrap();
    let report = settle(&mut session);
    let [commit] = report.surface_presentation_commits() else {
        panic!("the complete Presented/Dropped prefix must settle once");
    };
    assert!(
        commit.presents(artifact),
        "a later dropped output must preserve the last actual presentation"
    );
    assert!(commit.retires(dropped_artifact));
    assert!(
        session.bind_presented_receiver(&receiver).is_some(),
        "the visible unchanged scene must retain input authority"
    );
}

#[test]
fn separately_settled_dropped_output_preserves_the_last_actually_presented_output() {
    for abandon_settlement in [false, true] {
        let mut session = session();
        let (receiver, presented) = paint(&mut session);
        let (same_receiver, dropped) = paint(&mut session);
        assert_eq!(receiver, same_receiver);
        let artifact = presented.presentation_artifact();
        let dropped_artifact = dropped.presentation_artifact();
        session
            .report_surface_presentation(presented, SurfacePresentationResult::Presented)
            .unwrap();
        let first = settle(&mut session);
        let [first_commit] = first.surface_presentation_commits() else {
            panic!("A must settle before B's result arrives");
        };
        assert!(first_commit.presents(artifact));
        assert!(!first_commit.settles(dropped_artifact));
        let surface = session
            .presented_surface(SURFACE)
            .expect("settling Presented(A) must grant A's surface authority");
        let bound = session
            .bind_presented_receiver(&receiver)
            .expect("settling Presented(A) must grant A's receiver authority");

        session
            .report_surface_presentation(dropped, SurfacePresentationResult::Dropped)
            .unwrap();
        if abandon_settlement {
            let mut abandoned = session.begin_host_frame().unwrap();
            abandoned
                .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
                .unwrap();
            drop(abandoned);
            assert_eq!(session.presented_surface(SURFACE), Some(surface));
            assert_eq!(session.bind_presented_receiver(&receiver), Some(bound));
        }
        let later = settle(&mut session);
        let [later_commit] = later.surface_presentation_commits() else {
            panic!("B's dropped result must remain settleable after an abandoned candidate");
        };
        assert!(
            later_commit.presents(artifact),
            "settling Dropped(B) in a later frame must retain A's artifact; abandoned: {abandon_settlement}"
        );
        assert!(later_commit.retires(dropped_artifact));
        assert_eq!(
            session.presented_surface(SURFACE),
            Some(surface),
            "retiring B must preserve A's exact surface authority"
        );
        assert_eq!(
            session.bind_presented_receiver(&receiver),
            Some(bound),
            "retiring B must preserve A's exact receiver authority"
        );
    }
}

#[test]
fn last_presented_output_is_kept_when_the_earlier_output_was_dropped() {
    let mut session = session();
    let (_, dropped) = paint(&mut session);
    let (receiver, presented) = paint(&mut session);
    let artifact = presented.presentation_artifact();
    session
        .report_surface_presentation(dropped, SurfacePresentationResult::Dropped)
        .unwrap();
    session
        .report_surface_presentation(presented, SurfacePresentationResult::Presented)
        .unwrap();
    let report = settle(&mut session);
    assert!(
        report
            .surface_presentation_commits()
            .iter()
            .any(|commit| commit.presents(artifact))
    );
    assert!(session.bind_presented_receiver(&receiver).is_some());
}
