//! Exact output observations must preserve valid drag feedback and revoke losses.

use crate::geometry::{LogicalPoint, LogicalRect, LogicalSize};
use crate::model::{
    DockspaceContainedLayout, DockspaceLayout, DockspaceNode, DockspaceRootLayout,
    DockspaceSurfaceLayout, FloatingPresentationId, ItemId, RootId, SurfaceId,
};
use crate::policy::DockPolicy;
use crate::runtime::{
    DockspacePreviewVisual, DockspaceSession, HostFrameReport, PaintedSurfaceOutput,
    SurfaceGesturePhase, SurfacePaintDisposition, SurfacePaintPlan, SurfacePaintProgramOperation,
    SurfacePaintProgramStep, SurfacePresentationResult, SurfaceUnavailableReason,
    UniformSurfaceMetrics,
};

const SURFACE: SurfaceId = SurfaceId::new(1);
const MAIN_ROOT: RootId = RootId::new(1);
const FLOATING_ROOT: RootId = RootId::new(2);
const FLOATING: FloatingPresentationId = FloatingPresentationId::new(1);
const FIRST: ItemId = ItemId::new(1);
const DRAGGED: ItemId = ItemId::new(2);
const THIRD: ItemId = ItemId::new(3);

fn metrics() -> UniformSurfaceMetrics {
    UniformSurfaceMetrics::new(
        LogicalRect::new(0.0, 0.0, 800.0, 600.0).expect("surface bounds validate"),
        LogicalSize::new(64.0, 48.0).expect("pane minimum validates"),
        96.0,
    )
    .expect("surface metrics validate")
}

fn feedback(plan: SurfacePaintPlan<'_>) -> (Option<DockspacePreviewVisual>, bool) {
    let mut preview = None;
    let mut source_gap = false;
    plan.visit_surface_program(|step| {
        match step {
            SurfacePaintProgramStep::DragPreview(current) => {
                assert!(preview.replace(current.visual()).is_none());
            }
            SurfacePaintProgramStep::Operation {
                operation: SurfacePaintProgramOperation::Tab(tab),
                paint,
                ..
            } if tab.item() == DRAGGED => {
                source_gap = paint == SurfacePaintDisposition::SourceGap;
            }
            _ => {}
        }
        Ok::<(), ()>(())
    })
    .expect("the canonical surface program is valid");
    (preview, source_gap)
}

fn inspect_feedback(session: &mut DockspaceSession) -> (Option<DockspacePreviewVisual>, bool) {
    let mut frame = session.begin_host_frame().expect("inspection begins");
    let plan = frame
        .paint_plan(SURFACE)
        .expect("plan lookup succeeds")
        .expect("the current candidate remains paintable");
    let result = feedback(plan);
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .expect("inspection retains the exact candidate");
    frame.commit().expect("inspection commits");
    result
}

fn paint(session: &mut DockspaceSession) -> PaintedSurfaceOutput {
    let mut frame = session.begin_host_frame().expect("paint begins");
    let receipt = frame
        .paint_plan(SURFACE)
        .expect("plan lookup succeeds")
        .expect("the measured surface is paintable")
        .execute_surface_program(|_| Ok::<(), ()>(()))
        .expect("the accepting test sink consumes the complete surface program");
    frame
        .confirm_surface_painted(receipt)
        .expect("the exact execution receipt is accepted");
    let mut report = frame.commit().expect("paint commits");
    let mut outputs = report.take_painted_outputs();
    assert_eq!(outputs.len(), 1);
    outputs.pop().expect("one exact output was emitted")
}

fn settle(session: &mut DockspaceSession) -> HostFrameReport {
    let mut frame = session.begin_host_frame().expect("observation begins");
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .expect("observation retains the candidate without remeasurement");
    frame
        .commit()
        .expect("the exact output observation commits")
}

fn dragging_fixture() -> (DockspaceSession, LogicalPoint, DockspacePreviewVisual) {
    let contained_bounds =
        LogicalRect::new(160.0, 120.0, 320.0, 240.0).expect("contained bounds validate");
    let layout = DockspaceLayout::new([DockspaceSurfaceLayout::new(
        SURFACE,
        DockspaceRootLayout::new(MAIN_ROOT, DockspaceNode::central_tabs([FIRST, THIRD])),
    )
    .with_contained(DockspaceContainedLayout::new(
        FLOATING,
        DockspaceRootLayout::new(FLOATING_ROOT, DockspaceNode::tabs([DRAGGED])),
        contained_bounds,
    ))])
    .expect("the magnetic redock layout validates");
    let mut session = DockspaceSession::from_layout(layout, DockPolicy::default())
        .expect("the session initializes");
    let mut measure = session.begin_host_frame().expect("measurement begins");
    measure
        .measure_surface(SURFACE, metrics())
        .expect("the initial surface measures");
    measure.commit().expect("the initial candidate commits");
    let initial_output = paint(&mut session);
    session
        .report_surface_presentation(initial_output, SurfacePresentationResult::Presented)
        .expect("the initial output is reported");
    settle(&mut session);

    let mut begin = session.begin_host_frame().expect("drag begin opens");
    let plan = begin
        .paint_plan(SURFACE)
        .expect("plan lookup succeeds")
        .expect("the contained source is paintable");
    let tab = plan
        .tabs()
        .find(|tab| tab.item() == DRAGGED)
        .expect("the contained tab is visible");
    let initial = plan
        .receiver_for_tab_body(tab)
        .expect("the contained tab has an exact drag receiver")
        .center();
    let pane = plan
        .panes()
        .find(|pane| pane.root() == MAIN_ROOT)
        .expect("the main pane provides a magnetic target");
    let target = LogicalPoint::new(
        pane.content_bounds().x() + pane.content_bounds().width() * 0.75,
        pane.content_bounds().y() + pane.content_bounds().height() * 0.75,
    )
    .expect("the magnetic target is finite");
    assert!(!contained_bounds.contains(target));
    assert!(
        plan.drop_guides()
            .flat_map(|cluster| cluster.targets())
            .all(|guide| !guide.hit_bounds().contains(target)),
        "the fixture targets the ordinary pane basin, not an exact guide button"
    );
    let action = plan
        .prepare_tab_gesture(
            DRAGGED,
            SurfaceGesturePhase::Begin {
                initial,
                current: target,
            },
        )
        .expect("the exact source prepares a local drag");
    begin
        .submit_surface_action(action)
        .expect("drag begin is accepted");
    begin
        .measure_surface(SURFACE, metrics())
        .expect("the source-gap projection measures");
    begin.commit().expect("drag begin commits");
    assert!(session.has_active_gesture());
    let (preview, source_gap) = inspect_feedback(&mut session);
    let preview = preview.expect("the drag acquired a magnetic preview");
    assert!(matches!(
        preview,
        DockspacePreviewVisual::Dock {
            surface: SURFACE,
            ..
        }
    ));
    assert!(source_gap, "the source tab must already be a source gap");
    (session, target, preview)
}

fn release(session: &mut DockspaceSession, target: LogicalPoint) {
    let mut frame = session.begin_host_frame().expect("release begins");
    let action = frame
        .paint_plan(SURFACE)
        .expect("plan lookup succeeds")
        .expect("the retained candidate remains paintable")
        .prepare_tab_gesture(DRAGGED, SurfaceGesturePhase::Release { current: target })
        .expect("the exact source prepares release");
    frame
        .submit_surface_action(action)
        .expect("release is accepted");
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .expect("release does not replace the candidate by remeasurement");
    frame.commit().expect("release commits");
}

#[test]
fn exact_presented_preview_survives_promotion_and_releases_without_another_paint() {
    let (mut session, target, preview) = dragging_fixture();
    let output = paint(&mut session);
    let artifact = output.presentation_artifact();
    session
        .report_surface_presentation(output, SurfacePresentationResult::Presented)
        .expect("the complete preview output reaches presentation");
    let report = settle(&mut session);
    assert!(
        report
            .surface_presentation_commits()
            .iter()
            .any(|commit| commit.presents(artifact))
    );
    assert!(session.has_active_gesture());
    assert_eq!(
        inspect_feedback(&mut session),
        (Some(preview), true),
        "positive promotion must preserve the preview and its source-gap projection"
    );

    release(&mut session, target);
    assert!(
        session.view().contained(FLOATING).is_none(),
        "an already presented preview must finish redock without a replacement output"
    );
    assert_eq!(
        session
            .view()
            .item(DRAGGED)
            .expect("the item survives")
            .root(),
        MAIN_ROOT
    );
    assert!(!session.has_active_gesture());
}

#[test]
fn dropped_preview_clears_feedback_and_cannot_redock_on_release() {
    let (mut session, target, _) = dragging_fixture();
    let output = paint(&mut session);
    let artifact = output.presentation_artifact();
    session
        .report_surface_presentation(output, SurfacePresentationResult::Dropped)
        .expect("the dropped preview output is reported");
    let report = settle(&mut session);
    assert!(
        report
            .surface_presentation_commits()
            .iter()
            .any(|commit| commit.retires(artifact))
    );
    assert_eq!(
        inspect_feedback(&mut session).0,
        None,
        "retiring the output must clear its unavailable target feedback"
    );

    release(&mut session, target);
    assert!(session.view().contained(FLOATING).is_some());
    assert_eq!(
        session
            .view()
            .item(DRAGGED)
            .expect("the item survives")
            .root(),
        FLOATING_ROOT,
        "a dropped output cannot authorize the docking mutation"
    );
}

#[test]
fn a_later_dropped_output_revokes_an_already_painted_preview() {
    let (mut session, target, preview) = dragging_fixture();
    let presented = paint(&mut session);
    session
        .report_surface_presentation(presented, SurfacePresentationResult::Presented)
        .expect("the first preview output reaches presentation");
    settle(&mut session);
    assert_eq!(inspect_feedback(&mut session), (Some(preview), true));

    let dropped = paint(&mut session);
    let artifact = dropped.presentation_artifact();
    session
        .report_surface_presentation(dropped, SurfacePresentationResult::Dropped)
        .expect("the successor output is dropped");
    let report = settle(&mut session);
    assert!(
        report
            .surface_presentation_commits()
            .iter()
            .any(|commit| commit.retires(artifact))
    );
    assert_eq!(
        inspect_feedback(&mut session).0,
        None,
        "an old painted flag must not preserve feedback after authority is revoked"
    );
    release(&mut session, target);
    assert!(session.view().contained(FLOATING).is_some());
}

#[test]
fn identical_measurement_preserves_an_unreported_output_candidate() {
    let layout = DockspaceLayout::new([DockspaceSurfaceLayout::new(
        SURFACE,
        DockspaceRootLayout::new(MAIN_ROOT, DockspaceNode::central_tabs([FIRST, THIRD])),
    )])
    .unwrap();
    let mut session = DockspaceSession::from_layout(layout, DockPolicy::default()).unwrap();
    let mut frame = session.begin_host_frame().unwrap();
    frame.measure_surface(SURFACE, metrics()).unwrap();
    frame.commit().unwrap();
    let output = paint(&mut session);
    let artifact = output.presentation_artifact();

    let mut frame = session.begin_host_frame().unwrap();
    frame.measure_surface(SURFACE, metrics()).unwrap();
    frame.commit().unwrap();
    assert!(session.presented_surface(SURFACE).is_none());
    session
        .report_surface_presentation(output, SurfacePresentationResult::Presented)
        .unwrap();
    assert!(
        settle(&mut session)
            .surface_presentation_commits()
            .iter()
            .any(|commit| commit.presents(artifact)),
        "identical measurement cannot discard an otherwise valid delayed output"
    );
}
