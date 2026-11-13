//! Exact output observations must preserve valid drag feedback and revoke losses.

use crate::geometry::{LogicalPoint, LogicalRect, LogicalSize};
use crate::model::{
    DockspaceContainedLayout, DockspaceLayout, DockspaceNode, DockspaceRootLayout,
    DockspaceSurfaceLayout, FloatingPresentationId, ItemId, RootId, SurfaceId,
};
use crate::policy::DockPolicy;
use crate::runtime::{
    DockspaceManipulationDelivery, DockspaceManipulationStatus, DockspacePreviewVisual,
    DockspaceSession, HostFrameReport, HostPointerInput, HostPointerRoute, PaintedSurfaceOutput,
    SurfacePaintDisposition, SurfacePaintPlan, SurfacePaintProgramOperation,
    SurfacePaintProgramStep, SurfacePointerButton, SurfacePointerCapture, SurfacePointerEvent,
    SurfacePointerId, SurfacePointerReceiverFacts, SurfacePresentationResult,
    SurfaceUnavailableReason, UniformSurfaceMetrics,
};

const SURFACE: SurfaceId = SurfaceId::new(1);
const MAIN_ROOT: RootId = RootId::new(1);
const FLOATING_ROOT: RootId = RootId::new(2);
const FLOATING: FloatingPresentationId = FloatingPresentationId::new(1);
const FIRST: ItemId = ItemId::new(1);
const DRAGGED: ItemId = ItemId::new(2);
const THIRD: ItemId = ItemId::new(3);
const POINTER: SurfacePointerId = SurfacePointerId::new(1);

fn metrics() -> UniformSurfaceMetrics {
    UniformSurfaceMetrics::new(
        LogicalRect::new(0.0, 0.0, 800.0, 600.0).expect("surface bounds validate"),
        LogicalSize::new(64.0, 48.0).expect("pane minimum validates"),
        96.0,
    )
    .expect("surface metrics validate")
}

fn feedback(
    plan: SurfacePaintPlan<'_>,
) -> (Option<DockspacePreviewVisual>, SurfacePaintDisposition) {
    let mut preview = None;
    let mut source_paint = None;
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
                assert!(source_paint.replace(paint).is_none());
            }
            _ => {}
        }
        Ok::<(), ()>(())
    })
    .expect("the canonical surface program is valid");
    (
        preview,
        source_paint.expect("the complete contained source retains its tab in the paint program"),
    )
}

fn inspect_feedback(
    session: &mut DockspaceSession,
) -> (Option<DockspacePreviewVisual>, SurfacePaintDisposition) {
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
    frame
        .measure_surface(SURFACE, metrics())
        .expect("the paint pass measures the current committed layout");
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

    let mut inspection = session
        .begin_host_frame()
        .expect("geometry inspection opens");
    let plan = inspection
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
    inspection
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .expect("geometry inspection retains the presented output");
    inspection.commit().expect("geometry inspection commits");

    session
        .enable_host_pointer()
        .expect("the renderer-neutral host pointer enables");
    let source_hit = session
        .host_pointer_hit(SURFACE, initial)
        .expect("the source has actual presented authority");
    let source_click = source_hit
        .delivery()
        .expect("the source tab has a click receiver");
    let source_drag = source_hit
        .drag_delivery()
        .expect("the source tab has an independent drag receiver");
    let target_hit = session
        .host_pointer_hit(SURFACE, target)
        .expect("the target has actual presented authority");
    let target_surface = target_hit.surface();
    let target_hover = target_hit.hover();
    // An ordinary magnetic basin may have known-empty hover, not an exclusive receiver.
    let target_receivers = match target_hover.as_ref() {
        Some(receiver) => SurfacePointerReceiverFacts::hover(receiver),
        None => SurfacePointerReceiverFacts::no_hover(&target_surface),
    };
    let source_route = HostPointerRoute::new(source_hit.surface(), initial);
    let target_route = HostPointerRoute::new(target_surface, target);
    let mut begin = session.begin_host_frame().expect("drag begin opens");
    begin
        .submit_host_pointer_batch([
            HostPointerInput::new(
                POINTER,
                SurfacePointerEvent::ButtonPressed(SurfacePointerButton::Primary),
                source_route,
                source_route,
                SurfacePointerCapture::ProviderEndpoint,
                SurfacePointerReceiverFacts::delivery(&source_click)
                    .with_drag_delivery(&source_drag),
            ),
            HostPointerInput::new(
                POINTER,
                SurfacePointerEvent::Moved,
                target_route,
                target_route,
                SurfacePointerCapture::ProviderEndpoint,
                target_receivers,
            ),
        ])
        .expect("the presented source press and target move are accepted in order");
    begin
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .expect("the presented source and active target remain unchanged");
    begin.commit().expect("drag begin commits");
    assert!(session.has_active_gesture());
    let (preview, source_paint) = inspect_feedback(&mut session);
    let preview = preview.expect("the drag acquired a magnetic preview");
    assert!(matches!(
        preview,
        DockspacePreviewVisual::Dock {
            surface: SURFACE,
            ..
        }
    ));
    assert_eq!(
        source_paint,
        SurfacePaintDisposition::Visible,
        "the complete contained source tree must remain visible at its projected bounds"
    );
    (session, target, preview)
}

fn release(session: &mut DockspaceSession, target: LogicalPoint) -> HostFrameReport {
    let hit = session.host_pointer_hit(SURFACE, target);
    let surface = hit.map(|hit| hit.surface());
    let hover = hit.and_then(|hit| hit.hover());
    let route = surface.map_or(HostPointerRoute::Unknown, |surface| {
        HostPointerRoute::new(surface, target)
    });
    let receivers = match (surface.as_ref(), hover.as_ref()) {
        (_, Some(receiver)) => SurfacePointerReceiverFacts::hover(receiver),
        (Some(surface), None) => SurfacePointerReceiverFacts::no_hover(surface),
        (None, None) => SurfacePointerReceiverFacts::unknown(),
    };
    let mut frame = session.begin_host_frame().expect("release begins");
    frame
        .submit_host_pointer_batch([HostPointerInput::new(
            POINTER,
            SurfacePointerEvent::ButtonReleased(SurfacePointerButton::Primary),
            route,
            route,
            SurfacePointerCapture::None,
            receivers,
        )])
        .expect("release consumes only its event-time presentation authority");
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .expect("release does not replace the candidate by remeasurement");
    frame.commit().expect("release commits")
}

#[test]
fn ordinary_release_commits_without_paint_or_presentation() {
    let (mut session, target, _) = dragging_fixture();
    let unrelated = SurfaceId::new(99);
    assert!(session.has_active_gesture());
    assert!(!session.has_pending_presentation());
    assert!(!session.surface_requires_presentation(SURFACE));

    let report = release(&mut session, target);
    let committed = session.version();
    assert!(!session.has_active_gesture());
    assert!(!session.has_pending_presentation());
    assert!(!session.surface_requires_presentation(SURFACE));
    assert!(!session.surface_requires_presentation(unrelated));
    assert_eq!(report.repaint_surfaces(), &[SURFACE]);
    assert!(report.workspace_changed());
    assert_eq!(report.before().epoch(), committed.epoch());
    assert_eq!(
        report.before().revision().get() + 1,
        committed.revision().get()
    );
    assert!(session.view().contained(FLOATING).is_none());
    assert_eq!(session.view().item(DRAGGED).unwrap().root(), MAIN_ROOT);
    assert_eq!(
        report
            .manipulation_feedback()
            .iter()
            .filter(|feedback| feedback.status()
                == DockspaceManipulationStatus::Delivered(DockspaceManipulationDelivery::Dock))
            .count(),
        1
    );

    let mut discarded = session.begin_host_frame().expect("paint retry begins");
    discarded.measure_surface(SURFACE, metrics()).unwrap();
    let receipt = discarded
        .paint_plan(SURFACE)
        .unwrap()
        .unwrap()
        .execute_surface_program(|_| Ok::<(), ()>(()))
        .unwrap();
    discarded.confirm_surface_painted(receipt).unwrap();
    drop(discarded);
    assert!(!session.has_pending_presentation());
    assert_eq!(session.version(), committed);
    let unchanged = settle(&mut session);
    assert!(!unchanged.workspace_changed());
    assert!(unchanged.manipulation_feedback().is_empty());

    let output = paint(&mut session);
    let artifact = output.presentation_artifact();
    let mut inspection = session.begin_host_frame().unwrap();
    let plan = inspection.paint_plan(SURFACE).unwrap().unwrap();
    let tab = plan.tabs().find(|tab| tab.item() == DRAGGED).unwrap();
    let receiver = plan.receiver_for_tab_body(tab).unwrap();
    inspection
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    inspection.commit().unwrap();
    assert!(
        session.bind_presented_receiver(&receiver).is_none(),
        "painting the new layout cannot authorize its unpresented receivers"
    );
    assert!(!session.has_pending_presentation());
    assert!(!session.surface_requires_presentation(SURFACE));
    let in_flight = settle(&mut session);
    assert!(!in_flight.workspace_changed());
    assert!(in_flight.manipulation_feedback().is_empty());
    assert!(
        in_flight
            .surface_presentation_commits()
            .iter()
            .all(|commit| !commit.presents(artifact)),
        "emitting an output cannot manufacture Presented authority"
    );
    session
        .report_surface_presentation(output, SurfacePresentationResult::Presented)
        .unwrap();
    assert!(
        session.bind_presented_receiver(&receiver).is_none(),
        "queued renderer feedback gains authority only at successful publication"
    );
    assert!(!session.has_pending_presentation());
    assert_eq!(session.version(), committed);

    let discarded = session.begin_host_frame().expect("settlement retry begins");
    drop(discarded);
    assert!(!session.has_pending_presentation());
    assert_eq!(session.version(), committed);

    let presented = settle(&mut session);
    assert!(!presented.workspace_changed());
    assert!(presented.manipulation_feedback().is_empty());
    assert!(
        presented
            .surface_presentation_commits()
            .iter()
            .any(|commit| commit.presents(artifact))
    );
    assert!(session.bind_presented_receiver(&receiver).is_some());
    assert_eq!(session.version(), committed);
    assert!(!session.has_pending_presentation());
    assert!(!session.surface_requires_presentation(SURFACE));
    assert!(session.view().contained(FLOATING).is_none());
    assert_eq!(session.view().item(DRAGGED).unwrap().root(), MAIN_ROOT);
}

#[test]
fn dropped_or_abandoned_output_preserves_the_committed_release_without_replay() {
    for explicit_terminal in [false, true] {
        let (mut session, target, _) = dragging_fixture();
        let delivered = release(&mut session, target);
        assert!(delivered.workspace_changed());
        let committed = session.version();
        let output = paint(&mut session);
        let artifact = output.presentation_artifact();
        assert!(!session.has_pending_presentation());
        if explicit_terminal {
            session
                .report_surface_presentation(output, SurfacePresentationResult::Dropped)
                .unwrap();
        } else {
            drop(output);
        }
        assert!(!session.has_pending_presentation());
        let dropped = settle(&mut session);
        assert!(!dropped.workspace_changed());
        assert!(dropped.manipulation_feedback().is_empty());
        assert!(
            dropped
                .surface_presentation_commits()
                .iter()
                .any(|commit| commit.retires(artifact))
        );
        assert!(
            dropped
                .surface_presentation_commits()
                .iter()
                .all(|commit| !commit.presents(artifact))
        );
        assert_eq!(session.version(), committed);
        assert!(!session.has_pending_presentation());
        assert!(!session.surface_requires_presentation(SURFACE));
        assert_eq!(session.view().item(DRAGGED).unwrap().root(), MAIN_ROOT);
        assert!(session.view().contained(FLOATING).is_none());

        let later = paint(&mut session);
        session
            .report_surface_presentation(later, SurfacePresentationResult::Presented)
            .unwrap();
        let presented = settle(&mut session);
        assert!(!presented.workspace_changed());
        assert!(presented.manipulation_feedback().is_empty());
        assert_eq!(session.version(), committed);
        assert!(!session.has_pending_presentation());
        assert!(!session.surface_requires_presentation(SURFACE));
        assert_eq!(session.view().item(DRAGGED).unwrap().root(), MAIN_ROOT);
    }
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
        (Some(preview), SurfacePaintDisposition::Visible),
        "positive promotion must preserve the exact preview and its visible source projection"
    );
    assert!(session.host_pointer_hit(SURFACE, target).is_some());

    // The first preview promotion changes interaction semantics. A subsequent
    // equivalent presentation must not keep a Presented -> repaint cycle alive.
    let equivalent = paint(&mut session);
    session
        .report_surface_presentation(equivalent, SurfacePresentationResult::Presented)
        .unwrap();
    assert!(
        settle(&mut session).repaint_surfaces().is_empty(),
        "equivalent Presented provenance must not create a repaint loop"
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
fn dropped_unpresented_preview_keeps_the_target_and_release_commits_without_a_terminal_output() {
    let (mut session, target, preview) = dragging_fixture();
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
    assert_eq!(inspect_feedback(&mut session).0, Some(preview));
    assert!(session.host_pointer_hit(SURFACE, target).is_some());

    let release = release(&mut session, target);
    let committed = session.version();
    assert!(release.workspace_changed());
    assert!(!session.has_active_gesture());
    assert!(!session.has_pending_presentation());
    assert!(session.view().contained(FLOATING).is_none());
    assert_eq!(
        session.view().item(DRAGGED).unwrap().root(),
        MAIN_ROOT,
        "the still-visible target authorizes ordinary release independently of preview paint"
    );

    let replacement = paint(&mut session);
    let replacement_artifact = replacement.presentation_artifact();
    session
        .report_surface_presentation(replacement, SurfacePresentationResult::Presented)
        .unwrap();
    let report = settle(&mut session);
    assert!(
        report
            .surface_presentation_commits()
            .iter()
            .any(|commit| commit.settles(replacement_artifact))
    );
    assert!(!report.workspace_changed());
    assert!(report.manipulation_feedback().is_empty());
    assert_eq!(session.version(), committed);
    assert!(!session.has_pending_presentation());
    assert!(session.view().contained(FLOATING).is_none());
    assert_eq!(
        session.view().item(DRAGGED).unwrap().root(),
        MAIN_ROOT,
        "later presentation acknowledges output without replaying the delivered edit"
    );
    assert!(!settle(&mut session).workspace_changed());
}

#[test]
fn a_later_dropped_output_preserves_an_already_presented_preview() {
    let (mut session, target, preview) = dragging_fixture();
    let presented = paint(&mut session);
    let presented_artifact = presented.presentation_artifact();
    session
        .report_surface_presentation(presented, SurfacePresentationResult::Presented)
        .unwrap();
    settle(&mut session);
    assert_eq!(
        inspect_feedback(&mut session),
        (Some(preview), SurfacePaintDisposition::Visible)
    );

    let dropped = paint(&mut session);
    let dropped_artifact = dropped.presentation_artifact();
    session
        .report_surface_presentation(dropped, SurfacePresentationResult::Dropped)
        .unwrap();
    let report = settle(&mut session);
    let commit = report
        .surface_presentation_commits()
        .iter()
        .find(|commit| commit.settles(dropped_artifact))
        .unwrap();
    assert!(commit.retires(dropped_artifact));
    assert!(commit.presents(presented_artifact));
    assert_eq!(
        inspect_feedback(&mut session),
        (Some(preview), SurfacePaintDisposition::Visible)
    );
    assert!(session.host_pointer_hit(SURFACE, target).is_some());
    release(&mut session, target);
    assert!(session.view().contained(FLOATING).is_none());
    assert_eq!(session.view().item(DRAGGED).unwrap().root(), MAIN_ROOT);
    assert!(!session.has_active_gesture());
    assert!(!session.has_pending_presentation());
    assert!(!settle(&mut session).workspace_changed());
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
