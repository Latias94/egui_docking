//! Host mapping only: synthetic OS pixels and framebuffer commands, without OS/GPU integration.

use dockspace::geometry::{LogicalPoint, LogicalRect, LogicalSize};
use dockspace::model::{
    DockspaceLayout, DockspaceNode, DockspaceRootLayout, DockspaceSurfaceLayout, ItemId, RootId,
    SurfaceId,
};
use dockspace::policy::DockPolicy;
use dockspace::runtime::{
    DockspaceHostFrame, DockspaceManipulationDelivery as Delivery,
    DockspaceManipulationPendingReason as Pending, DockspaceManipulationStatus as Status,
    DockspacePreviewVisual, DockspaceSession, HostFrameReport, HostPointerInput, HostPointerRoute,
    SurfacePaintDisposition, SurfacePaintProgramStep as Step, SurfacePointerButton,
    SurfacePointerCapture, SurfacePointerEvent, SurfacePointerId, SurfacePointerReceiverFacts,
    SurfacePresentationResult, SurfaceUnavailableReason, UniformSurfaceMetrics,
};
use dockspace_host_harness::{OwnedRecord, RecordingRenderer};

const A: SurfaceId = SurfaceId::new(1);
const B: SurfaceId = SurfaceId::new(2);
const FIRST: ItemId = ItemId::new(1);
const SECOND: ItemId = ItemId::new(2);
const THIRD: ItemId = ItemId::new(3);

#[derive(Clone, Copy, Debug)]
enum Consumption {
    Immediate,
    Deferred,
}

// These are host-owned pixel values, deliberately not core physical geometry.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Pixels(f64, f64);

#[derive(Debug, PartialEq)]
struct FramebufferDraw {
    logical: LogicalRect,
    origin: Pixels,
    size: Pixels,
}

#[derive(Clone, Copy)]
struct HostMapping {
    window_dpi: f64,
    densities: [f64; 2],
    origins: [LogicalPoint; 2],
}

impl HostMapping {
    fn os_pixels(self, surface: usize, local: LogicalPoint) -> Pixels {
        let origin = self.origins[surface];
        Pixels(
            (origin.x() + local.x()) * self.window_dpi,
            (origin.y() + local.y()) * self.window_dpi,
        )
    }

    fn local_from_os(self, surface: usize, physical: Pixels) -> LogicalPoint {
        // OS input uses window DPI, never the render target's framebuffer density.
        let window =
            LogicalPoint::new(physical.0 / self.window_dpi, physical.1 / self.window_dpi).unwrap();
        let origin = self.origins[surface];
        LogicalPoint::new(window.x() - origin.x(), window.y() - origin.y()).unwrap()
    }

    fn framebuffer_point(self, surface: usize, local: LogicalPoint) -> Pixels {
        let density = self.densities[surface];
        Pixels(local.x() * density, local.y() * density)
    }

    fn consume(self, surface: usize, recording: &RecordingRenderer) -> Vec<FramebufferDraw> {
        recording
            .records
            .iter()
            .filter_map(|record| {
                let logical = match record {
                    OwnedRecord::Geometry {
                        bounds,
                        paint: SurfacePaintDisposition::Visible,
                        ..
                    }
                    | OwnedRecord::PaneContent {
                        bounds,
                        paint: SurfacePaintDisposition::Visible,
                        ..
                    } => *bounds,
                    OwnedRecord::Preview(DockspacePreviewVisual::Dock { rect, .. }) => *rect,
                    _ => return None,
                };
                Some(FramebufferDraw {
                    logical,
                    origin: self.framebuffer_point(
                        surface,
                        LogicalPoint::new(logical.x(), logical.y()).unwrap(),
                    ),
                    size: Pixels(
                        logical.width() * self.densities[surface],
                        logical.height() * self.densities[surface],
                    ),
                })
            })
            .collect()
    }
}

fn settle(session: &mut DockspaceSession) -> HostFrameReport {
    let mut frame = session.begin_host_frame().unwrap();
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    frame.commit().unwrap()
}

fn paint(
    mut frame: DockspaceHostFrame<'_>,
    mapping: HostMapping,
    mode: Consumption,
) -> (
    HostFrameReport,
    [RecordingRenderer; 2],
    [Vec<FramebufferDraw>; 2],
) {
    let mut commands = [Vec::new(), Vec::new()];
    let records = [A, B].map(|surface| {
        let index = usize::from(surface == B);
        let plan = frame.paint_plan(surface).unwrap().unwrap();
        let (recording, receipt) = RecordingRenderer::execute(plan).unwrap();
        if matches!(mode, Consumption::Immediate) {
            // Consume while the frame still owns its session borrow.
            commands[index] = mapping.consume(index, &recording);
        }
        frame.confirm_surface_painted(receipt).unwrap();
        recording
    });
    let report = frame.commit().unwrap();
    if matches!(mode, Consumption::Deferred) {
        // The frame was consumed: only owned recordings remain here.
        commands = std::array::from_fn(|index| mapping.consume(index, &records[index]));
    }
    for (index, draws) in commands.iter().enumerate() {
        assert!(!draws.is_empty());
        for draw in draws {
            let density = mapping.densities[index];
            assert_eq!(
                draw.origin,
                Pixels(draw.logical.x() * density, draw.logical.y() * density)
            );
            assert_eq!(
                draw.size,
                Pixels(
                    draw.logical.width() * density,
                    draw.logical.height() * density
                )
            );
        }
    }
    (report, records, commands)
}

fn present_exact(
    session: &mut DockspaceSession,
    report: &mut HostFrameReport,
    records: &[RecordingRenderer; 2],
) -> HostFrameReport {
    let release_settlement = report
        .manipulation_feedback()
        .iter()
        .any(|feedback| feedback.status() == Status::Pending(Pending::AwaitingPresentation));
    let outputs = report.take_painted_outputs();
    assert_eq!(outputs.len(), 2);
    let artifacts: Vec<_> = outputs
        .iter()
        .map(|output| output.presentation_artifact())
        .collect();
    for output in outputs {
        let index = usize::from(output.surface() == B);
        assert!(matches!(
            records[index].records.last(),
            Some(OwnedRecord::SurfaceEnd(semantic)) if output.matches_semantic_output(*semantic)
        ));
        session
            .report_surface_presentation(output, SurfacePresentationResult::Presented)
            .unwrap();
    }
    let settled = settle(session);
    assert_eq!(settled.surface_presentation_commits().len(), 2);
    for artifact in artifacts {
        let acknowledgements: Vec<_> = settled
            .surface_presentation_commits()
            .iter()
            .filter(|ack| ack.settles(artifact))
            .collect();
        assert_eq!(acknowledgements.len(), 1);
        if !release_settlement {
            assert!(acknowledgements[0].presents(artifact));
            assert!(!acknowledgements[0].retires(artifact));
        }
        // Delivery may retire its own source geometry after changing topology.
        // The caller checks the exact pending manipulation's Delivered terminal.
    }
    settled
}

#[test]
fn window_dpi_and_framebuffer_density_are_independent_for_cross_surface_batches() {
    let mut baseline = None;
    for mode in [Consumption::Immediate, Consumption::Deferred] {
        for window_dpi in [1.25, 2.0] {
            for densities in [[2.0, 0.75], [0.75, 2.0]] {
                let mapping = HostMapping {
                    window_dpi,
                    densities,
                    origins: [
                        LogicalPoint::new(40.0, 60.0).unwrap(),
                        LogicalPoint::new(1000.0, 140.0).unwrap(),
                    ],
                };
                let layout = DockspaceLayout::new([
                    DockspaceSurfaceLayout::new(
                        A,
                        DockspaceRootLayout::new(
                            RootId::new(1),
                            DockspaceNode::central_tabs([FIRST, SECOND]),
                        ),
                    ),
                    DockspaceSurfaceLayout::new(
                        B,
                        DockspaceRootLayout::new(
                            RootId::new(2),
                            DockspaceNode::central_tabs([THIRD]),
                        ),
                    ),
                ])
                .unwrap();
                let mut session =
                    DockspaceSession::from_layout(layout, DockPolicy::default()).unwrap();
                session.enable_host_pointer().unwrap();
                let initial_version = session.version();
                let bounds = LogicalRect::new(0.0, 0.0, 800.0, 600.0).unwrap();
                let mut measured = session.begin_host_frame().unwrap();
                for surface in [A, B] {
                    measured
                        .measure_surface(
                            surface,
                            UniformSurfaceMetrics::new(
                                bounds,
                                LogicalSize::new(64.0, 48.0).unwrap(),
                                96.0,
                            )
                            .unwrap(),
                        )
                        .unwrap();
                }
                assert!(!measured.commit().unwrap().workspace_changed());

                let mut initial = session.begin_host_frame().unwrap();
                let source_descriptor = initial
                    .paint_plan(A)
                    .unwrap()
                    .unwrap()
                    .tab_receiver(FIRST)
                    .unwrap();
                let target_descriptor = initial
                    .paint_plan(B)
                    .unwrap()
                    .unwrap()
                    .center_drop_receiver_for_item(THIRD)
                    .unwrap();
                let (mut report, records, _) = paint(initial, mapping, mode);
                assert!(
                    records[0]
                        .records
                        .contains(&OwnedRecord::Receiver(source_descriptor))
                );
                assert!(
                    session
                        .bind_presented_receiver(&source_descriptor)
                        .is_none()
                );
                assert!(
                    session
                        .bind_presented_receiver(&target_descriptor)
                        .is_none()
                );
                assert!(
                    present_exact(&mut session, &mut report, &records)
                        .manipulation_feedback()
                        .is_empty()
                );
                let source = session.bind_presented_receiver(&source_descriptor).unwrap();
                assert!(
                    session
                        .bind_presented_receiver(&target_descriptor)
                        .is_some()
                );
                let pane = records[1]
                    .records
                    .iter()
                    .find_map(|record| match record {
                        OwnedRecord::PaneContent {
                            item: Some(THIRD),
                            bounds,
                            ..
                        } => Some(*bounds),
                        _ => None,
                    })
                    .unwrap();
                // A real pane interior point, away from center and edge guide buttons.
                let hover = LogicalPoint::new(
                    pane.x() + pane.width() * 0.375,
                    pane.y() + pane.height() * 0.375,
                )
                .unwrap();
                assert!(pane.contains(hover));
                let mut inspect = session.begin_host_frame().unwrap();
                let plan = inspect.paint_plan(B).unwrap().unwrap();
                let mut guide_buttons = 0;
                plan.visit_surface_program(|step| {
                    if let Step::DropGuide {
                        structural,
                        affordance,
                    } = step
                    {
                        for button in structural.targets() {
                            guide_buttons += 1;
                            assert!(!button.hit_bounds().contains(hover));
                        }
                        if let Some(cluster) = affordance {
                            for button in cluster.targets() {
                                if let Some(receiver) = plan.receiver_for_drop_target(button) {
                                    assert!(!receiver.bounds().contains(hover));
                                }
                            }
                        }
                    }
                    Ok::<(), ()>(())
                })
                .unwrap();
                assert!(guide_buttons > 0);
                drop(inspect);
                assert!(session.host_pointer_hit(B, hover).is_some());

                let press_os = mapping.os_pixels(0, source.center());
                let drop_os = mapping.os_pixels(1, hover);
                let press = mapping.local_from_os(0, press_os);
                let manipulation = mapping.local_from_os(0, drop_os);
                let mapped_hover = mapping.local_from_os(1, drop_os);
                assert_eq!(press, source.center());
                assert_eq!(mapped_hover, hover);
                assert!(!bounds.contains(manipulation));
                assert_ne!(manipulation, mapped_hover);
                assert_eq!(
                    manipulation.x() - mapped_hover.x(),
                    mapping.origins[1].x() - mapping.origins[0].x()
                );
                assert_eq!(
                    manipulation.y() - mapped_hover.y(),
                    mapping.origins[1].y() - mapping.origins[0].y()
                );
                for (index, local, os) in [
                    (0, press, press_os),
                    (0, manipulation, drop_os),
                    (1, mapped_hover, drop_os),
                ] {
                    assert_ne!(mapping.framebuffer_point(index, local), os);
                    let wrong = LogicalPoint::new(
                        os.0 / densities[index] - mapping.origins[index].x(),
                        os.1 / densities[index] - mapping.origins[index].y(),
                    )
                    .unwrap();
                    if densities[index] != window_dpi {
                        assert_ne!(wrong, local, "render density must not decode OS input");
                    }
                }
                let a = session.presented_surface(A).unwrap();
                let b = session.presented_surface(B).unwrap();
                let press_route = HostPointerRoute::new(a, press);
                let edge = |event| {
                    HostPointerInput::new(
                        SurfacePointerId::new(1),
                        event,
                        HostPointerRoute::new(a, manipulation),
                        HostPointerRoute::new(b, mapped_hover),
                        SurfacePointerCapture::ProviderEndpoint,
                        // This host draws bare pane content and proves its lane
                        // clear; core chooses the magnetic target at this point.
                        SurfacePointerReceiverFacts::delivery(&source).with_clear_hover_lane(&b),
                    )
                };
                let mut frame = session.begin_host_frame().unwrap();
                frame
                    .submit_host_pointer_batch([
                        HostPointerInput::new(
                            SurfacePointerId::new(1),
                            SurfacePointerEvent::ButtonPressed(SurfacePointerButton::Primary),
                            press_route,
                            press_route,
                            SurfacePointerCapture::ProviderEndpoint,
                            SurfacePointerReceiverFacts::delivery(&source),
                        ),
                        edge(SurfacePointerEvent::Moved),
                        edge(SurfacePointerEvent::ButtonReleased(
                            SurfacePointerButton::Primary,
                        )),
                    ])
                    .unwrap();
                let (mut released, previews, commands) = paint(frame, mapping, mode);
                let preview_rect = previews[1]
                    .records
                    .iter()
                    .find_map(|record| match record {
                        OwnedRecord::Preview(DockspacePreviewVisual::Dock { surface: B, rect }) => {
                            Some(*rect)
                        }
                        _ => None,
                    })
                    .expect("the target owns the exact release preview");
                assert!(commands[1].iter().any(|draw| draw.logical == preview_rect));
                assert!(!released.workspace_changed());
                assert_eq!(session.version(), initial_version);
                assert_eq!(session.view().item(FIRST).unwrap().surface(), A);
                assert!(!session.has_active_gesture());
                let [pending] = released.manipulation_feedback() else {
                    panic!("one pending release")
                };
                assert_eq!(
                    pending.status(),
                    Status::Pending(Pending::AwaitingPresentation)
                );
                let pending_id = pending.manipulation_id();
                let pending_feedback_id = pending.id();
                let waiting = settle(&mut session);
                assert!(!waiting.workspace_changed());
                assert!(waiting.manipulation_feedback().is_empty());
                assert_eq!(session.version(), initial_version);

                // Dropped-output denial is covered by public_contract.rs's same-batch negative.
                let delivered = present_exact(&mut session, &mut released, &previews);
                assert!(delivered.workspace_changed());
                assert_eq!(delivered.before(), initial_version);
                assert_eq!(delivered.after(), session.version());
                // The public version is opaque. This consumer verifies one changed
                // publication and delivery; core regressions own exact revision counts.
                assert_ne!(session.version(), initial_version);
                let [terminal] = delivered.manipulation_feedback() else {
                    panic!("one typed delivery")
                };
                assert_eq!(terminal.status(), Status::Delivered(Delivery::Dock));
                assert_eq!(terminal.manipulation_id(), pending_id);
                assert_ne!(terminal.id(), pending_feedback_id);
                assert_eq!(session.view().surfaces().count(), 2);
                let topology: Vec<_> = [FIRST, SECOND, THIRD]
                    .map(|item| {
                        let view = session.view().item(item).unwrap();
                        assert!(view.contained().is_none());
                        (
                            item,
                            view.surface(),
                            view.root(),
                            view.tabs().items().to_vec(),
                        )
                    })
                    .into_iter()
                    .collect();
                assert_eq!(
                    topology,
                    vec![
                        (FIRST, B, RootId::new(2), vec![THIRD, FIRST]),
                        (SECOND, A, RootId::new(1), vec![SECOND]),
                        (THIRD, B, RootId::new(2), vec![THIRD, FIRST]),
                    ]
                );
                if let Some(expected) = &baseline {
                    assert_eq!(&topology, expected);
                } else {
                    baseline = Some(topology);
                }
                let delivered_version = session.version();
                for _ in 0..2 {
                    let quiet = settle(&mut session);
                    assert!(!quiet.workspace_changed());
                    assert_eq!(quiet.before(), delivered_version);
                    assert_eq!(quiet.after(), delivered_version);
                    assert!(quiet.manipulation_feedback().is_empty());
                    assert!(quiet.surface_presentation_commits().is_empty());
                    assert!(quiet.presentation_transitions().is_empty());
                }
                assert!(!session.has_active_gesture());
            }
        }
    }
}
