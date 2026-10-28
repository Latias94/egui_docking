//! Manual smooth-scroll ownership suppresses stationary edge scrolling.

use std::time::Duration;

use dockspace::geometry::{LogicalPoint, LogicalRect, LogicalSize};
use dockspace::model::{
    DockspaceLayout, DockspaceNode, DockspaceRootLayout, DockspaceSurfaceLayout, ItemId, RootId,
    SurfaceId,
};
use dockspace::policy::DockPolicy;
use dockspace::runtime::{
    DockPresentationConfig, DockspaceHostFrame, DockspacePointerContinuation, DockspaceSession,
    HostFrameReport, HostPointerInput, HostPointerRoute, PointerDeviceKind, PointerEventMetadata,
    PresentedDockReceiver, SurfacePaintProgramOperation as Operation,
    SurfacePaintProgramStep as Step, SurfacePointerButton, SurfacePointerCapture,
    SurfacePointerEvent, SurfacePointerId, SurfacePointerReceiverFacts, SurfacePresentationResult,
    SurfaceScrollDelta, SurfaceScrollDeviceId, SurfaceScrollEvent, SurfaceScrollModifiers,
    SurfaceScrollMomentum, SurfaceScrollPhase, SurfaceScrollSequenceId, SurfaceUnavailableReason,
    UniformSurfaceMetrics,
};
use dockspace_host_harness::{OwnedRecord, RecordingRenderer};

const SOURCE: SurfaceId = SurfaceId::new(1);
const TARGET: SurfaceId = SurfaceId::new(2);
const FIRST: ItemId = ItemId::new(1);
const SECOND: ItemId = ItemId::new(2);
const POINTER: SurfacePointerId = SurfacePointerId::new(1);
const SEQUENCE: SurfaceScrollSequenceId = SurfaceScrollSequenceId::new(1);

fn target_items() -> impl Iterator<Item = ItemId> {
    (10..18).map(ItemId::new)
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct BarState {
    viewport: LogicalRect,
    offset: f64,
    maximum: f64,
}

struct FrameSnapshot {
    bar: BarState,
    source_receiver: dockspace::runtime::DockspaceReceiverDescriptor,
}

fn inspect(frame: &mut DockspaceHostFrame<'_>) -> FrameSnapshot {
    let source = frame.paint_plan(SOURCE).unwrap().unwrap();
    let source_receiver = source.tab_receiver(FIRST).unwrap();
    let target = frame.paint_plan(TARGET).unwrap().unwrap();
    let mut bar = None;
    target
        .visit_surface_program(|step| {
            if let Step::Operation {
                operation: Operation::TabBar(record),
                ..
            } = step
            {
                assert!(bar.is_none(), "the target has one tab strip");
                bar = Some(BarState {
                    viewport: record.viewport(),
                    offset: record.scroll_offset(),
                    maximum: record.maximum_scroll_offset(),
                });
            }
            Ok::<(), ()>(())
        })
        .unwrap();
    FrameSnapshot {
        bar: bar.unwrap(),
        source_receiver,
    }
}

fn finish(mut frame: DockspaceHostFrame<'_>) -> HostFrameReport {
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    frame.commit().unwrap()
}

fn paint(
    mut frame: DockspaceHostFrame<'_>,
) -> (
    FrameSnapshot,
    HostFrameReport,
    Vec<dockspace::runtime::PaintedSurfaceOutput>,
) {
    let snapshot = inspect(&mut frame);
    let recordings = [SOURCE, TARGET].map(|surface| {
        let (recording, receipt) =
            RecordingRenderer::execute(frame.paint_plan(surface).unwrap().unwrap()).unwrap();
        frame.confirm_surface_painted(receipt).unwrap();
        recording
    });
    let mut report = frame.commit().unwrap();
    let outputs = report.take_painted_outputs();
    assert_eq!(outputs.len(), 2);
    for output in &outputs {
        let index = usize::from(output.surface() == TARGET);
        assert!(matches!(
            recordings[index].records.last(),
            Some(OwnedRecord::SurfaceEnd(semantic))
                if output.matches_semantic_output(*semantic)
        ));
    }
    (snapshot, report, outputs)
}

fn present(session: &mut DockspaceSession, outputs: Vec<dockspace::runtime::PaintedSurfaceOutput>) {
    let artifacts = outputs
        .iter()
        .map(|output| output.presentation_artifact())
        .collect::<Vec<_>>();
    for output in outputs {
        session
            .report_surface_presentation(output, SurfacePresentationResult::Presented)
            .unwrap();
    }
    let report = finish(session.begin_host_frame().unwrap());
    for artifact in artifacts {
        assert_eq!(
            report
                .surface_presentation_commits()
                .iter()
                .filter(|ack| ack.presents(artifact))
                .count(),
            1
        );
    }
}

fn timed_input<'receiver>(
    event: SurfacePointerEvent,
    source: dockspace::runtime::PresentedDockspaceSurface,
    moved: LogicalPoint,
    target: dockspace::runtime::PresentedDockspaceSurface,
    point: LogicalPoint,
    facts: SurfacePointerReceiverFacts<'receiver>,
    millis: u64,
) -> HostPointerInput<'receiver> {
    HostPointerInput::new(
        POINTER,
        event,
        HostPointerRoute::new(source, moved),
        HostPointerRoute::new(target, point),
        SurfacePointerCapture::ProviderEndpoint,
        facts,
    )
    .with_metadata(PointerEventMetadata::new(
        PointerDeviceKind::Unknown,
        Duration::from_millis(millis),
    ))
}

fn scroll_input<'receiver>(
    phase: SurfaceScrollPhase,
    target: dockspace::runtime::PresentedDockspaceSurface,
    point: LogicalPoint,
    receiver: &'receiver PresentedDockReceiver,
    millis: u64,
) -> HostPointerInput<'receiver> {
    let route = HostPointerRoute::new(target, point);
    HostPointerInput::new(
        POINTER,
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
    .with_metadata(PointerEventMetadata::new(
        PointerDeviceKind::Unknown,
        Duration::from_millis(millis),
    ))
}

fn fixture() -> (DockspaceSession, LogicalPoint, LogicalPoint, FrameSnapshot) {
    let config = DockPresentationConfig::builder()
        .hover_selection_delay(None)
        .build()
        .unwrap();
    let layout = DockspaceLayout::new([
        DockspaceSurfaceLayout::new(
            SOURCE,
            DockspaceRootLayout::new(RootId::new(1), DockspaceNode::central_tabs([FIRST, SECOND])),
        ),
        DockspaceSurfaceLayout::new(
            TARGET,
            DockspaceRootLayout::new(RootId::new(2), DockspaceNode::central_tabs(target_items())),
        ),
    ])
    .unwrap();
    let mut session = DockspaceSession::from_layout_with_presentation_config(
        layout,
        DockPolicy::default(),
        config,
    )
    .unwrap();
    session.enable_host_pointer().unwrap();
    let mut frame = session.begin_host_frame().unwrap();
    for (surface, width) in [(SOURCE, 800.0), (TARGET, 240.0)] {
        frame
            .measure_surface(
                surface,
                UniformSurfaceMetrics::new(
                    LogicalRect::new(0.0, 0.0, width, 600.0).unwrap(),
                    LogicalSize::new(64.0, 48.0).unwrap(),
                    96.0,
                )
                .unwrap(),
            )
            .unwrap();
    }
    frame.commit().unwrap();
    let (initial, _, outputs) = paint(session.begin_host_frame().unwrap());
    present(&mut session, outputs);
    assert_eq!(initial.bar.offset, 0.0);
    assert!(initial.bar.maximum > 80.0);
    let a = session.presented_surface(SOURCE).unwrap();
    let b = session.presented_surface(TARGET).unwrap();
    let moved = LogicalPoint::new(700.0, 500.0).unwrap();
    let edge = LogicalPoint::new(
        initial.bar.viewport.max().x() - 1.0,
        initial.bar.viewport.y() + initial.bar.viewport.height() * 0.5,
    )
    .unwrap();
    let source = session
        .bind_presented_receiver(&initial.source_receiver)
        .unwrap();
    let mut frame = session.begin_host_frame().unwrap();
    frame
        .submit_host_pointer_batch([
            timed_input(
                SurfacePointerEvent::ButtonPressed(SurfacePointerButton::Primary),
                a,
                source.center(),
                a,
                source.center(),
                SurfacePointerReceiverFacts::delivery(&source),
                0,
            ),
            timed_input(
                SurfacePointerEvent::Moved,
                a,
                moved,
                b,
                edge,
                SurfacePointerReceiverFacts::unknown().with_clear_hover_lane(&b),
                10,
            ),
        ])
        .unwrap();
    assert!(!finish(frame).workspace_changed());
    assert_eq!(
        session.pointer_continuation().unwrap().deadline(),
        Duration::from_millis(60)
    );
    let (baseline, _, outputs) = paint(session.begin_host_frame().unwrap());
    present(&mut session, outputs);
    (session, moved, edge, baseline)
}

fn target_scroll_receiver(session: &mut DockspaceSession) -> PresentedDockReceiver {
    let mut frame = session.begin_host_frame().unwrap();
    let plan = frame.paint_plan(TARGET).unwrap().unwrap();
    let mut descriptor = None;
    plan.visit_surface_program(|step| {
        if let Step::Operation {
            operation: Operation::TabBar(bar),
            ..
        } = step
        {
            descriptor = plan.receiver_for_tab_strip_scroll(bar);
        }
        Ok::<(), ()>(())
    })
    .unwrap();
    drop(frame);
    session
        .bind_presented_receiver(&descriptor.unwrap())
        .unwrap()
}

#[test]
fn manual_smooth_scroll_owner_cancels_edge_wakes_without_ending_drag() {
    let (mut session, moved, edge, baseline) = fixture();
    let version = session.version();
    let old_wake: DockspacePointerContinuation = session.pointer_continuation().unwrap();
    let b = session.presented_surface(TARGET).unwrap();
    let receiver = target_scroll_receiver(&mut session);

    let mut frame = session.begin_host_frame().unwrap();
    frame
        .submit_host_pointer(scroll_input(
            SurfaceScrollPhase::Begin {
                sequence: SEQUENCE,
                delta: Some(SurfaceScrollDelta::LogicalPoints { x: 0.0, y: 0.0 }),
            },
            b,
            edge,
            &receiver,
            20,
        ))
        .unwrap();
    let begin = finish(frame);
    assert!(!begin.workspace_changed());
    assert_eq!(session.version(), version);
    assert_eq!(session.pointer_continuation(), None);
    assert!(session.has_active_gesture());

    let a = session.presented_surface(SOURCE).unwrap();
    let b = session.presented_surface(TARGET).unwrap();
    let clear = SurfacePointerReceiverFacts::unknown().with_clear_hover_lane(&b);
    let mut frame = session.begin_host_frame().unwrap();
    frame
        .submit_host_pointer(timed_input(
            SurfacePointerEvent::Moved,
            a,
            moved,
            b,
            edge,
            clear,
            30,
        ))
        .unwrap();
    assert!(!finish(frame).workspace_changed());
    assert_eq!(session.pointer_continuation(), None);
    assert!(session.has_active_gesture());

    let a = session.presented_surface(SOURCE).unwrap();
    let b = session.presented_surface(TARGET).unwrap();
    let mut frame = session.begin_host_frame().unwrap();
    frame
        .submit_host_pointer(timed_input(
            old_wake.event(),
            a,
            moved,
            b,
            edge,
            SurfacePointerReceiverFacts::unknown().with_clear_hover_lane(&b),
            60,
        ))
        .unwrap();
    assert!(!finish(frame).workspace_changed());
    assert_eq!(session.pointer_continuation(), None);
    assert!(session.has_active_gesture());

    let b = session.presented_surface(TARGET).unwrap();
    let mut frame = session.begin_host_frame().unwrap();
    frame
        .submit_host_pointer(scroll_input(
            SurfaceScrollPhase::End {
                sequence: SEQUENCE,
                delta: None,
            },
            b,
            edge,
            &receiver,
            70,
        ))
        .unwrap();
    assert!(!finish(frame).workspace_changed());
    assert_eq!(session.pointer_continuation(), None);
    assert!(session.has_active_gesture());

    let a = session.presented_surface(SOURCE).unwrap();
    let b = session.presented_surface(TARGET).unwrap();
    let mut frame = session.begin_host_frame().unwrap();
    frame
        .submit_host_pointer(timed_input(
            SurfacePointerEvent::Moved,
            a,
            moved,
            b,
            edge,
            SurfacePointerReceiverFacts::unknown().with_clear_hover_lane(&b),
            80,
        ))
        .unwrap();
    assert!(!finish(frame).workspace_changed());
    let fresh_wake = session.pointer_continuation().unwrap();
    assert_ne!(fresh_wake.token(), old_wake.token());
    assert_eq!(fresh_wake.deadline(), Duration::from_millis(130));
    assert!(session.has_active_gesture());

    let b = session.presented_surface(TARGET).unwrap();
    let mut frame = session.begin_host_frame().unwrap();
    frame
        .submit_host_pointer(scroll_input(
            SurfaceScrollPhase::Discrete {
                delta: SurfaceScrollDelta::LogicalPoints { x: -40.0, y: 0.0 },
            },
            b,
            edge,
            &receiver,
            90,
        ))
        .unwrap();
    assert!(!finish(frame).workspace_changed());
    let (manual, _, outputs) = paint(session.begin_host_frame().unwrap());
    assert_eq!(manual.bar.offset, 40.0);
    assert_eq!(manual.bar.maximum, baseline.bar.maximum);
    present(&mut session, outputs);
    assert_eq!(session.pointer_continuation(), None);
    assert!(session.has_active_gesture());
    assert_eq!(session.version(), version);

    let a = session.presented_surface(SOURCE).unwrap();
    let b = session.presented_surface(TARGET).unwrap();
    let mut frame = session.begin_host_frame().unwrap();
    frame
        .submit_host_pointer(timed_input(
            fresh_wake.event(),
            a,
            moved,
            b,
            edge,
            SurfacePointerReceiverFacts::unknown().with_clear_hover_lane(&b),
            140,
        ))
        .unwrap();
    let (after_old_wake, report, outputs) = paint(frame);
    assert_eq!(after_old_wake.bar.offset, 40.0);
    assert!(!report.workspace_changed());
    assert_eq!(session.pointer_continuation(), None);
    assert!(session.has_active_gesture());
    assert_eq!(session.version(), version);
    present(&mut session, outputs);
}

#[test]
fn manual_scroll_refreshes_the_other_surface_whose_drag_preview_was_cleared() {
    let (mut session, moved, edge, _) = fixture();
    let version = session.version();
    let b = session.presented_surface(TARGET).unwrap();
    let receiver = target_scroll_receiver(&mut session);
    let mut frame = session.begin_host_frame().unwrap();
    frame
        .submit_host_pointer(scroll_input(
            SurfaceScrollPhase::Begin {
                sequence: SEQUENCE,
                delta: None,
            },
            b,
            edge,
            &receiver,
            20,
        ))
        .unwrap();
    finish(frame);
    let a = session.presented_surface(SOURCE).unwrap();
    let source_target = LogicalPoint::new(400.0, 300.0).unwrap();
    let mut frame = session.begin_host_frame().unwrap();
    frame
        .submit_host_pointer(timed_input(
            SurfacePointerEvent::Moved,
            a,
            moved,
            a,
            source_target,
            SurfacePointerReceiverFacts::unknown().with_clear_hover_lane(&a),
            30,
        ))
        .unwrap();
    let plan = frame.paint_plan(SOURCE).unwrap().unwrap();
    assert!(plan.has_manipulation_preview());
    let (recording, receipt) = RecordingRenderer::execute(plan).unwrap();
    assert!(
        recording
            .records
            .iter()
            .any(|record| matches!(record, OwnedRecord::Preview(_)))
    );
    frame.confirm_surface_painted(receipt).unwrap();
    let plan = frame.paint_plan(TARGET).unwrap().unwrap();
    let (_, receipt) = RecordingRenderer::execute(plan).unwrap();
    frame.confirm_surface_painted(receipt).unwrap();
    let outputs = frame.commit().unwrap().take_painted_outputs();
    present(&mut session, outputs);

    let b = session.presented_surface(TARGET).unwrap();
    let receiver = target_scroll_receiver(&mut session);
    let mut frame = session.begin_host_frame().unwrap();
    frame
        .submit_host_pointer(scroll_input(
            SurfaceScrollPhase::Update {
                sequence: SEQUENCE,
                delta: SurfaceScrollDelta::LogicalPoints { x: -40.0, y: 0.0 },
            },
            b,
            edge,
            &receiver,
            40,
        ))
        .unwrap();
    let source_plan = frame.paint_plan(SOURCE).unwrap().unwrap();
    assert!(!source_plan.has_manipulation_preview());
    let (recording, receipt) = RecordingRenderer::execute(source_plan).unwrap();
    assert!(
        !recording
            .records
            .iter()
            .any(|record| matches!(record, OwnedRecord::Preview(_)))
    );
    frame.confirm_surface_painted(receipt).unwrap();
    let target_plan = frame.paint_plan(TARGET).unwrap().unwrap();
    let (_, receipt) = RecordingRenderer::execute(target_plan).unwrap();
    frame.confirm_surface_painted(receipt).unwrap();
    let mut report = frame.commit().unwrap();
    assert!(!report.workspace_changed());
    present(&mut session, report.take_painted_outputs());
    assert_eq!(session.version(), version);
    assert!(session.has_active_gesture());
    assert!(session.pointer_continuation().is_none());
    let mut frame = session.begin_host_frame().unwrap();
    assert_eq!(inspect(&mut frame).bar.offset, 40.0);
    drop(frame);
}
