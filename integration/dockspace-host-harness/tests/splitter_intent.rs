//! A constrained projection does not rewrite persistent weights without resize intent.

use dockspace::geometry::{LogicalRect, LogicalSize};
use dockspace::model::{
    DockspaceAxis, DockspaceLayout, DockspaceNode, DockspaceRootLayout, DockspaceSurfaceLayout,
    ItemId, RootId, SurfaceId,
};
use dockspace::policy::DockPolicy;
use dockspace::runtime::{
    DockspaceSession, HostPointerInput, HostPointerRoute, SurfaceMeasurementAnswer as Answer,
    SurfaceMeasurementRequest as Question, SurfacePaintProgramOperation as Operation,
    SurfacePaintProgramStep as Step, SurfacePointerButton, SurfacePointerCapture,
    SurfacePointerEvent, SurfacePointerId, SurfacePointerReceiverFacts, SurfacePresentationResult,
    SurfaceUnavailableReason, TabStripMetrics,
};
use dockspace_host_harness::RecordingRenderer;

const SURFACE: SurfaceId = SurfaceId::new(1);
const ROOT: RootId = RootId::new(1);
const FIRST: ItemId = ItemId::new(1);
const SECOND: ItemId = ItemId::new(2);

#[test]
fn review_zero_displacement_splitter_click_preserves_durable_intent() {
    let node = DockspaceNode::split(
        DockspaceAxis::Horizontal,
        [
            (DockspaceNode::tabs([FIRST]), 0.25),
            (DockspaceNode::tabs([SECOND]), 0.75),
        ],
    )
    .unwrap();
    let layout = DockspaceLayout::new([DockspaceSurfaceLayout::new(
        SURFACE,
        DockspaceRootLayout::new(ROOT, node),
    )])
    .unwrap();
    let mut session = DockspaceSession::from_layout(layout, DockPolicy::default()).unwrap();
    session.enable_host_pointer().unwrap();
    let gap = session.presentation_config().splitter_thickness();
    let mut measured = session.begin_host_frame().unwrap();
    measured
        .measure_surface_with(SURFACE, |question| match question {
            Question::DockBounds { .. } | Question::PopupPlaneBounds { .. } => {
                Answer::Bounds(LogicalRect::new(0.0, 0.0, 400.0 + gap, 600.0).unwrap())
            }
            Question::PaneMinimum { item, .. } => Answer::PaneMinimum(
                LogicalSize::new(if item == Some(FIRST) { 200.0 } else { 100.0 }, 48.0).unwrap(),
            ),
            Question::TabIntrinsic { .. } => Answer::TabIntrinsic(56.0),
            Question::TabStrip { .. } => Answer::TabStrip(TabStripMetrics::new(0.0, 0.0).unwrap()),
        })
        .unwrap();
    measured.commit().unwrap();
    let (receiver, output) = {
        let mut frame = session.begin_host_frame().unwrap();
        let plan = frame.paint_plan(SURFACE).unwrap().unwrap();
        let mut receiver = None;
        plan.visit_surface_program(|step| {
            if let Step::Operation {
                operation: Operation::Splitter(splitter),
                ..
            } = step
            {
                receiver = plan.receiver_for_splitter(splitter);
            }
            Ok::<(), ()>(())
        })
        .unwrap();
        let (_, receipt) = RecordingRenderer::execute(plan).unwrap();
        frame.confirm_surface_painted(receipt).unwrap();
        let mut outputs = frame.commit().unwrap().take_painted_outputs();
        assert_eq!(outputs.len(), 1);
        (
            receiver.expect("the constrained split must expose its splitter receiver"),
            outputs.pop().unwrap(),
        )
    };
    session
        .report_surface_presentation(output, SurfacePresentationResult::Presented)
        .unwrap();
    let mut settled = session.begin_host_frame().unwrap();
    settled
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    settled.commit().unwrap();
    let before_version = session.version();
    let before_weights = session
        .view()
        .root(ROOT)
        .unwrap()
        .content()
        .unwrap()
        .split()
        .unwrap()
        .weights()
        .collect::<Vec<_>>();
    let bound = session.bind_presented_receiver(&receiver).unwrap();
    let surface = session.presented_surface(SURFACE).unwrap();
    let route = HostPointerRoute::new(surface, receiver.center());
    let facts = SurfacePointerReceiverFacts::delivery(&bound)
        .with_drag_delivery(&bound)
        .with_clear_hover_lane(&surface);
    let mut frame = session.begin_host_frame().unwrap();
    frame
        .submit_host_pointer_batch([
            HostPointerInput::new(
                SurfacePointerId::new(1),
                SurfacePointerEvent::ButtonPressed(SurfacePointerButton::Primary),
                route,
                route,
                SurfacePointerCapture::ProviderEndpoint,
                facts,
            ),
            HostPointerInput::new(
                SurfacePointerId::new(1),
                SurfacePointerEvent::ButtonReleased(SurfacePointerButton::Primary),
                route,
                route,
                SurfacePointerCapture::None,
                facts,
            ),
        ])
        .unwrap();
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    let report = frame.commit().unwrap();
    let after_weights = session
        .view()
        .root(ROOT)
        .unwrap()
        .content()
        .unwrap()
        .split()
        .unwrap()
        .weights()
        .collect::<Vec<_>>();
    assert_eq!(
        after_weights, before_weights,
        "a zero-displacement click must preserve the constrained split's durable weights"
    );
    assert!(!report.workspace_changed());
    assert_eq!(session.version(), before_version);
}
