//! Direct prepared cancellation retains the identity of its captured gesture.

use dockspace::geometry::{LogicalRect, LogicalSize};
use dockspace::model::{
    DockspaceLayout, DockspaceNode, DockspaceRootLayout, DockspaceSurfaceLayout, ItemId, RootId,
    SurfaceId,
};
use dockspace::policy::DockPolicy;
use dockspace::runtime::{
    DockspaceHostFrame, DockspaceSession, HostFrameReport, HostInputOutcome, HostPointerInput,
    HostPointerRoute, PresentedDockReceiver, PresentedDockspaceSurface,
    SurfacePaintProgramOperation as Operation, SurfacePaintProgramStep as Step,
    SurfacePointerButton as Button, SurfacePointerCancelReason, SurfacePointerCapture,
    SurfacePointerEvent as Event, SurfacePointerId, SurfacePointerReceiverFacts,
    SurfacePresentationResult, SurfaceUnavailableReason, UniformSurfaceMetrics,
};
use dockspace_host_harness::RecordingRenderer;

const SURFACE: SurfaceId = SurfaceId::new(1);
const FIRST: ItemId = ItemId::new(1);
const SECOND: ItemId = ItemId::new(2);

fn commit(mut frame: DockspaceHostFrame<'_>) -> HostFrameReport {
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    frame.commit().unwrap()
}

fn pointer_edge(
    session: &mut DockspaceSession,
    surface: PresentedDockspaceSurface,
    receiver: &PresentedDockReceiver,
    pointer: u64,
    event: Event,
) {
    let route = HostPointerRoute::new(surface, receiver.center());
    let mut frame = session.begin_host_frame().unwrap();
    frame
        .submit_host_pointer_batch([HostPointerInput::new(
            SurfacePointerId::new(pointer),
            event,
            route,
            route,
            SurfacePointerCapture::ProviderEndpoint,
            SurfacePointerReceiverFacts::delivery(receiver).with_clear_hover_lane(&surface),
        )])
        .unwrap();
    assert!(!commit(frame).workspace_changed());
}

#[test]
fn direct_prepared_cancel_cannot_cancel_a_successor_gesture_at_the_same_version() {
    let layout = DockspaceLayout::new([DockspaceSurfaceLayout::new(
        SURFACE,
        DockspaceRootLayout::new(RootId::new(1), DockspaceNode::central_tabs([FIRST, SECOND])),
    )])
    .unwrap();
    let mut session = DockspaceSession::from_layout(layout, DockPolicy::default()).unwrap();
    session.enable_host_pointer().unwrap();

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
    let plan = frame.paint_plan(SURFACE).unwrap().unwrap();
    let mut descriptor = None;
    plan.visit_surface_program(|step| {
        if let Step::Operation {
            operation: Operation::Tab(tab),
            ..
        } = step
            && tab.item() == FIRST
        {
            descriptor = plan.receiver_for_tab_close(tab);
        }
        Ok::<(), ()>(())
    })
    .unwrap();
    let descriptor = descriptor.expect("the selected tab exposes a close receiver");
    let (_, receipt) = RecordingRenderer::execute(plan).unwrap();
    frame.confirm_surface_painted(receipt).unwrap();
    let mut outputs = commit(frame).take_painted_outputs();
    assert_eq!(outputs.len(), 1);
    session
        .report_surface_presentation(outputs.pop().unwrap(), SurfacePresentationResult::Presented)
        .unwrap();
    commit(session.begin_host_frame().unwrap());

    let surface = session.presented_surface(SURFACE).unwrap();
    let receiver = session.bind_presented_receiver(&descriptor).unwrap();
    let version = session.version();
    pointer_edge(
        &mut session,
        surface,
        &receiver,
        1,
        Event::ButtonPressed(Button::Primary),
    );
    assert!(session.has_active_gesture());
    let stale_cancel = {
        let mut frame = session.begin_host_frame().unwrap();
        frame
            .paint_plan(SURFACE)
            .unwrap()
            .unwrap()
            .prepare_gesture_cancel()
            .unwrap()
    };
    assert_eq!(stale_cancel.expected_version(), version);

    pointer_edge(
        &mut session,
        surface,
        &receiver,
        1,
        Event::StreamCancelled(SurfacePointerCancelReason::ExplicitPlatformCancellation),
    );
    assert!(!session.has_active_gesture());
    pointer_edge(
        &mut session,
        surface,
        &receiver,
        2,
        Event::ButtonPressed(Button::Primary),
    );
    assert!(session.has_active_gesture());
    assert_eq!(session.version(), version);

    let mut frame = session.begin_host_frame().unwrap();
    frame.submit_surface_action(stale_cancel).unwrap();
    let report = commit(frame);
    assert!(
        session.has_active_gesture(),
        "cancellation captured from the previous gesture must leave its same-version successor active: {report:?}"
    );
    assert_eq!(session.version(), version);
    assert!(!report.workspace_changed());
    assert!(matches!(
        report.inputs(),
        [HostInputOutcome::StaleRejected { .. }]
    ));

    let current_cancel = {
        let mut frame = session.begin_host_frame().unwrap();
        frame
            .paint_plan(SURFACE)
            .unwrap()
            .unwrap()
            .prepare_gesture_cancel()
            .unwrap()
    };
    let mut frame = session.begin_host_frame().unwrap();
    frame.submit_surface_action(current_cancel).unwrap();
    assert!(!commit(frame).workspace_changed());
    assert!(
        !session.has_active_gesture(),
        "a fresh prepared action must still cancel the exact current gesture"
    );
}
