//! Smooth-scroll ownership uses current popup routing with retained sibling output.

use dockspace::geometry::{LogicalRect, LogicalSize};
use dockspace::model::{
    DockspaceLayout, DockspaceNode, DockspaceRootLayout, DockspaceSurfaceLayout, ItemId, RootId,
    SurfaceId,
};
use dockspace::policy::DockPolicy;
use dockspace::runtime::{
    DockspaceHostFrame, DockspaceReceiverDescriptor, DockspaceSession, HostFrameReport,
    HostPointerInput, HostPointerRoute, PresentedDockReceiver, PresentedDockspaceSurface,
    SurfaceMeasurementAnswer, SurfaceMeasurementRequest, SurfacePaintProgramOperation as Operation,
    SurfacePaintProgramStep as Step, SurfacePointerCapture, SurfacePointerEvent, SurfacePointerId,
    SurfacePointerReceiverFacts, SurfacePresentationResult, SurfaceScrollDelta,
    SurfaceScrollDeviceId, SurfaceScrollEvent, SurfaceScrollModifiers, SurfaceScrollMomentum,
    SurfaceScrollPhase, SurfaceScrollSequenceId, SurfaceUnavailableReason, TabListMenuMetrics,
    TabStripControlKind, TabStripControlMetric, TabStripControlMetrics, TabStripControlPlacement,
    TabStripMetrics,
};
use dockspace_host_harness::RecordingRenderer;

const A: SurfaceId = SurfaceId::new(1);
const B: SurfaceId = SurfaceId::new(2);
const SEQUENCE: SurfaceScrollSequenceId = SurfaceScrollSequenceId::new(1);

#[derive(Debug, Clone, Copy)]
struct StripSnapshot {
    receiver: DockspaceReceiverDescriptor,
    offset: f64,
    maximum: f64,
}

fn measure(frame: &mut DockspaceHostFrame<'_>, surface: SurfaceId) {
    let width = if surface == A { 160.0 } else { 240.0 };
    frame
        .measure_surface_with(surface, |request| match request {
            SurfaceMeasurementRequest::DockBounds { .. }
            | SurfaceMeasurementRequest::PopupPlaneBounds { .. } => {
                SurfaceMeasurementAnswer::Bounds(LogicalRect::new(0.0, 0.0, width, 600.0).unwrap())
            }
            SurfaceMeasurementRequest::PaneMinimum { .. } => {
                SurfaceMeasurementAnswer::PaneMinimum(LogicalSize::new(64.0, 48.0).unwrap())
            }
            SurfaceMeasurementRequest::TabIntrinsic { .. } => {
                SurfaceMeasurementAnswer::TabIntrinsic(96.0)
            }
            SurfaceMeasurementRequest::TabStrip { .. } => {
                let controls = TabStripControlMetrics::new(4.0)
                    .unwrap()
                    .with_scroll_backward(
                        TabStripControlMetric::new(18.0, TabStripControlPlacement::OverlayLeading)
                            .unwrap(),
                    )
                    .with_scroll_forward(
                        TabStripControlMetric::new(18.0, TabStripControlPlacement::OverlayTrailing)
                            .unwrap(),
                    )
                    .with_tab_list_menu(
                        TabStripControlMetric::new(
                            20.0,
                            TabStripControlPlacement::ReservedTrailing,
                        )
                        .unwrap(),
                    );
                SurfaceMeasurementAnswer::TabStrip(
                    TabStripMetrics::new(0.0, 0.0)
                        .unwrap()
                        .with_controls(controls)
                        .with_tab_list_menu(
                            TabListMenuMetrics::new(24.0, 8.0, 8.0, 2.0, 92.0, 10.0).unwrap(),
                        ),
                )
            }
        })
        .unwrap();
}

fn finish(mut frame: DockspaceHostFrame<'_>) -> HostFrameReport {
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    frame.commit().unwrap()
}

fn strip_snapshot(session: &mut DockspaceSession) -> StripSnapshot {
    let mut frame = session.begin_host_frame().unwrap();
    let plan = frame.paint_plan(B).unwrap().unwrap();
    let mut strip = None;
    plan.visit_surface_program(|step| {
        if let Step::Operation {
            operation: Operation::TabBar(bar),
            ..
        } = step
        {
            assert!(strip.is_none(), "B has exactly one overflowing tab strip");
            strip = Some(StripSnapshot {
                receiver: plan.receiver_for_tab_strip_scroll(bar).unwrap(),
                offset: bar.scroll_offset(),
                maximum: bar.maximum_scroll_offset(),
            });
        }
        Ok::<(), ()>(())
    })
    .unwrap();
    finish(frame);
    strip.unwrap()
}

fn paint_and_present(
    session: &mut DockspaceSession,
    surface: SurfaceId,
) -> PresentedDockspaceSurface {
    let mut frame = session.begin_host_frame().unwrap();
    let plan = frame.paint_plan(surface).unwrap().unwrap();
    let (_, receipt) = RecordingRenderer::execute(plan).unwrap();
    frame.confirm_surface_painted(receipt).unwrap();
    let mut outputs = finish(frame).take_painted_outputs();
    assert_eq!(outputs.len(), 1);
    let output = outputs.pop().unwrap();
    assert_eq!(output.surface(), surface);
    let artifact = output.presentation_artifact();
    session
        .report_surface_presentation(output, SurfacePresentationResult::Presented)
        .unwrap();
    let report = finish(session.begin_host_frame().unwrap());
    assert!(
        report
            .surface_presentation_commits()
            .iter()
            .any(|commit| commit.presents(artifact))
    );
    let proof = session.presented_surface(surface).unwrap();
    assert!(proof.matches_presentation_artifact(artifact));
    proof
}

fn fixture() -> (
    DockspaceSession,
    PresentedDockspaceSurface,
    PresentedDockReceiver,
) {
    let layout = DockspaceLayout::new([
        DockspaceSurfaceLayout::new(
            A,
            DockspaceRootLayout::new(
                RootId::new(1),
                DockspaceNode::central_tabs([ItemId::new(1), ItemId::new(2)]),
            ),
        ),
        DockspaceSurfaceLayout::new(
            B,
            DockspaceRootLayout::new(
                RootId::new(2),
                DockspaceNode::central_tabs((10..18).map(ItemId::new)),
            ),
        ),
    ])
    .unwrap();
    let mut session = DockspaceSession::from_layout(layout, DockPolicy::default()).unwrap();
    session.enable_host_pointer().unwrap();
    let mut frame = session.begin_host_frame().unwrap();
    for surface in [A, B] {
        measure(&mut frame, surface);
    }
    finish(frame);
    paint_and_present(&mut session, A);
    let b = paint_and_present(&mut session, B);
    let strip = strip_snapshot(&mut session);
    assert_eq!(strip.offset, 0.0);
    assert!(strip.maximum > 100.0);
    let receiver = session.bind_presented_receiver(&strip.receiver).unwrap();
    (session, b, receiver)
}

fn assert_retained(
    session: &DockspaceSession,
    b: PresentedDockspaceSurface,
    receiver: PresentedDockReceiver,
) {
    assert_eq!(session.presented_surface(B), Some(b));
    assert_eq!(
        session.bind_presented_receiver(&receiver.descriptor()),
        Some(receiver)
    );
}

fn toggle_popup(session: &mut DockspaceSession, open: bool) {
    let mut frame = session.begin_host_frame().unwrap();
    let plan = frame.paint_plan(A).unwrap().unwrap();
    let mut control = None;
    plan.visit_surface_program(|step| {
        if let Step::Operation {
            operation: Operation::TabStripControl(candidate),
            ..
        } = step
            && candidate.kind() == TabStripControlKind::TabListMenu
        {
            assert!(candidate.enabled());
            assert_eq!(candidate.menu_open(), !open);
            assert!(control.replace(candidate).is_none());
        }
        Ok::<(), ()>(())
    })
    .unwrap();
    let action = plan
        .prepare_tab_strip_control_activation(control.unwrap())
        .unwrap();
    finish(frame);

    let mut frame = session.begin_host_frame().unwrap();
    frame.submit_surface_action(action).unwrap();
    measure(&mut frame, A);
    finish(frame);

    let mut frame = session.begin_host_frame().unwrap();
    let plan = frame.paint_plan(A).unwrap().unwrap();
    let mut popup = false;
    plan.visit_surface_program(|step| {
        if let Step::PopupMenu { .. } = step {
            assert!(!popup);
            popup = true;
        }
        Ok::<(), ()>(())
    })
    .unwrap();
    assert_eq!(popup, open);
    finish(frame);
    paint_and_present(session, A);
}

fn scroll_input(
    b: PresentedDockspaceSurface,
    receiver: &PresentedDockReceiver,
    phase: SurfaceScrollPhase,
) -> HostPointerInput<'_> {
    let route = HostPointerRoute::new(b, receiver.center());
    HostPointerInput::new(
        SurfacePointerId::new(1),
        SurfacePointerEvent::Scrolled(SurfaceScrollEvent::new(
            SurfaceScrollDeviceId::new(1),
            phase,
            SurfaceScrollMomentum::Direct,
            SurfaceScrollModifiers::new(false, false, false, false),
        )),
        route,
        route,
        SurfacePointerCapture::ProviderEndpoint,
        SurfacePointerReceiverFacts::delivery(receiver),
    )
}

#[test]
fn popup_cycle_preserves_new_sibling_smooth_scroll_without_remeasurement() {
    let (mut session, b, receiver) = fixture();
    let version = session.version();
    toggle_popup(&mut session, true);
    assert_retained(&session, b, receiver);
    toggle_popup(&mut session, false);
    assert_retained(&session, b, receiver);

    let mut frame = session.begin_host_frame().unwrap();
    frame
        .submit_host_pointer(scroll_input(
            b,
            &receiver,
            SurfaceScrollPhase::Begin {
                sequence: SEQUENCE,
                delta: None,
            },
        ))
        .unwrap();
    assert!(!finish(frame).workspace_changed());
    assert_retained(&session, b, receiver);

    let mut frame = session.begin_host_frame().unwrap();
    frame
        .submit_host_pointer_batch(
            [
                SurfaceScrollPhase::Update {
                    sequence: SEQUENCE,
                    delta: SurfaceScrollDelta::LogicalPoints { x: -40.0, y: 0.0 },
                },
                SurfaceScrollPhase::End {
                    sequence: SEQUENCE,
                    delta: Some(SurfaceScrollDelta::LogicalPoints { x: -20.0, y: 0.0 }),
                },
            ]
            .map(|phase| scroll_input(b, &receiver, phase)),
        )
        .unwrap();
    // B is measured only after the entire sequence consumed its original output proof.
    measure(&mut frame, B);
    assert!(!finish(frame).workspace_changed());
    assert_eq!(
        strip_snapshot(&mut session).offset,
        60.0,
        "a new smooth-scroll owner must use current popup routing with B's retained output"
    );
    assert!(
        session
            .bind_presented_receiver(&receiver.descriptor())
            .is_none(),
        "the changed strip must require a fresh output proof"
    );
    assert_eq!(session.version(), version);
}

#[test]
fn a_real_popup_cycle_terminates_the_previous_sibling_scroll_owner() {
    for change_popup in [false, true] {
        let (mut session, b, receiver) = fixture();
        let version = session.version();
        let mut frame = session.begin_host_frame().unwrap();
        frame
            .submit_host_pointer(scroll_input(
                b,
                &receiver,
                SurfaceScrollPhase::Begin {
                    sequence: SEQUENCE,
                    delta: Some(SurfaceScrollDelta::LogicalPoints { x: -20.0, y: 0.0 }),
                },
            ))
            .unwrap();
        measure(&mut frame, B);
        assert!(!finish(frame).workspace_changed());
        assert_eq!(strip_snapshot(&mut session).offset, 20.0);
        let b = paint_and_present(&mut session, B);
        let strip = strip_snapshot(&mut session);
        let receiver = session.bind_presented_receiver(&strip.receiver).unwrap();

        if change_popup {
            toggle_popup(&mut session, true);
            assert_retained(&session, b, receiver);
            let mut frame = session.begin_host_frame().unwrap();
            frame
                .submit_host_pointer(scroll_input(
                    b,
                    &receiver,
                    SurfaceScrollPhase::Update {
                        sequence: SEQUENCE,
                        delta: SurfaceScrollDelta::LogicalPoints { x: -40.0, y: 0.0 },
                    },
                ))
                .unwrap();
            assert!(!finish(frame).workspace_changed());
            assert_retained(&session, b, receiver);
            toggle_popup(&mut session, false);
            assert_retained(&session, b, receiver);
        }

        let mut frame = session.begin_host_frame().unwrap();
        frame
            .submit_host_pointer_batch(
                [
                    SurfaceScrollPhase::Update {
                        sequence: SEQUENCE,
                        delta: SurfaceScrollDelta::LogicalPoints { x: -40.0, y: 0.0 },
                    },
                    SurfaceScrollPhase::End {
                        sequence: SEQUENCE,
                        delta: Some(SurfaceScrollDelta::LogicalPoints { x: -20.0, y: 0.0 }),
                    },
                ]
                .map(|phase| scroll_input(b, &receiver, phase)),
            )
            .unwrap();
        measure(&mut frame, B);
        assert!(!finish(frame).workspace_changed());
        assert_eq!(
            strip_snapshot(&mut session).offset,
            if change_popup { 20.0 } else { 80.0 },
            "a real popup change must terminate the old owner without reviving it on close"
        );
        assert_eq!(session.version(), version);
    }
}
