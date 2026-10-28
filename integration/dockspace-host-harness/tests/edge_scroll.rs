//! Public stationary edge-scroll regressions, without claiming full overflow arbitration.

use std::time::Duration;

use dockspace::geometry::{LogicalPoint, LogicalRect, LogicalSize};
use dockspace::model::{
    DockspaceLayout, DockspaceNode, DockspaceRootLayout, DockspaceSurfaceLayout, ItemId, RootId,
    SurfaceId, WorkspaceVersion,
};
use dockspace::policy::DockPolicy;
use dockspace::runtime::{
    DockPresentationConfig, DockspaceHostFrame, DockspacePaneFocusRequest,
    DockspacePointerContinuation, DockspaceReceiverDescriptor, DockspaceReceiverRole,
    DockspaceRuntimeErrorKind, DockspaceSession, HostFrameReport, HostPointerInput,
    HostPointerRoute, PaintedSurfaceOutput, PointerDeviceKind, PointerEventMetadata,
    PresentedDockspaceSurface, SurfaceMeasurementAnswer, SurfaceMeasurementRequest,
    SurfacePaintProgramOperation as Operation, SurfacePaintProgramStep as Step,
    SurfacePointerButton, SurfacePointerCapture, SurfacePointerEvent, SurfacePointerId,
    SurfacePointerReceiverFacts, SurfacePresentationResult, SurfaceUnavailableReason,
    TabStripControlMetric, TabStripControlMetrics, TabStripControlPlacement, TabStripMetrics,
    UniformSurfaceMetrics,
};
use dockspace_host_harness::{OwnedRecord, RecordingRenderer};

const SOURCE: SurfaceId = SurfaceId::new(1);
const TARGET: SurfaceId = SurfaceId::new(2);
const FIRST: ItemId = ItemId::new(1);
const SECOND: ItemId = ItemId::new(2);
const POINTER: SurfacePointerId = SurfacePointerId::new(1);

fn target_items() -> impl Iterator<Item = ItemId> {
    (10..18).map(ItemId::new)
}

#[derive(Debug, Clone, Copy)]
struct Bar {
    viewport: LogicalRect,
    offset: f64,
    maximum: f64,
}

struct Snapshot {
    bar: Bar,
    control_hits: Vec<LogicalRect>,
    source: DockspaceReceiverDescriptor,
    focus: [Option<DockspacePaneFocusRequest>; 2],
}

fn inspect(frame: &mut DockspaceHostFrame<'_>) -> Snapshot {
    let source = frame.paint_plan(SOURCE).unwrap().unwrap();
    let source_receiver = source.tab_receiver(FIRST).unwrap();
    let source_focus = source.pane_focus_request();
    let target = frame.paint_plan(TARGET).unwrap().unwrap();
    let target_focus = target.pane_focus_request();
    let mut bar = None;
    let mut control_hits = Vec::new();
    target
        .visit_surface_program(|step| {
            match step {
                Step::Operation {
                    operation: Operation::TabBar(record),
                    ..
                } => {
                    assert!(bar.is_none(), "the target has exactly one tab strip");
                    bar = Some(Bar {
                        viewport: record.viewport(),
                        offset: record.scroll_offset(),
                        maximum: record.maximum_scroll_offset(),
                    });
                }
                Step::Operation {
                    operation: Operation::TabStripControl(control),
                    ..
                } => control_hits.push(control.hit_bounds()),
                _ => {}
            }
            Ok::<(), ()>(())
        })
        .unwrap();
    Snapshot {
        bar: bar.unwrap(),
        control_hits,
        source: source_receiver,
        focus: [source_focus, target_focus],
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
) -> (Snapshot, HostFrameReport, Vec<PaintedSurfaceOutput>) {
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

fn present(session: &mut DockspaceSession, outputs: Vec<PaintedSurfaceOutput>) {
    let artifacts = outputs
        .iter()
        .map(PaintedSurfaceOutput::presentation_artifact)
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
    source: PresentedDockspaceSurface,
    moved: LogicalPoint,
    target: PresentedDockspaceSurface,
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

fn fixture() -> (DockspaceSession, LogicalPoint, LogicalPoint, Snapshot) {
    fixture_with_controls(None)
}

fn fixture_with_controls(
    placement: Option<TabStripControlPlacement>,
) -> (DockspaceSession, LogicalPoint, LogicalPoint, Snapshot) {
    let config = DockPresentationConfig::builder()
        .hover_selection_delay(None)
        .build()
        .unwrap();
    assert_eq!(config.tab_strip_edge_scroll_band(), 24.0);
    assert_eq!(
        config.tab_strip_edge_scroll_interval(),
        Some(Duration::from_millis(50))
    );
    assert_eq!(config.tab_strip_scroll_line_extent(), 40.0);
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
        if surface == TARGET
            && let Some(placement) = placement
        {
            let metric = TabStripControlMetric::new(60.0, placement).unwrap();
            let controls = TabStripControlMetrics::new(0.0)
                .unwrap()
                .with_scroll_backward(metric)
                .with_scroll_forward(metric)
                .with_tab_list_menu(metric);
            let strip = TabStripMetrics::new(0.0, 0.0)
                .unwrap()
                .with_controls(controls);
            frame
                .measure_surface_with(surface, |request| match request {
                    SurfaceMeasurementRequest::DockBounds { .. }
                    | SurfaceMeasurementRequest::PopupPlaneBounds { .. } => {
                        SurfaceMeasurementAnswer::Bounds(
                            LogicalRect::new(0.0, 0.0, width, 600.0).unwrap(),
                        )
                    }
                    SurfaceMeasurementRequest::PaneMinimum { .. } => {
                        SurfaceMeasurementAnswer::PaneMinimum(LogicalSize::new(64.0, 48.0).unwrap())
                    }
                    SurfaceMeasurementRequest::TabIntrinsic { .. } => {
                        SurfaceMeasurementAnswer::TabIntrinsic(96.0)
                    }
                    SurfaceMeasurementRequest::TabStrip { .. } => {
                        SurfaceMeasurementAnswer::TabStrip(strip)
                    }
                })
                .unwrap();
            continue;
        }
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
    let source = session.bind_presented_receiver(&initial.source).unwrap();
    let a = session.presented_surface(SOURCE).unwrap();
    let b = session.presented_surface(TARGET).unwrap();
    let moved = LogicalPoint::new(700.0, 500.0).unwrap();
    let content_right = if placement == Some(TabStripControlPlacement::OverlayTrailing) {
        initial
            .control_hits
            .iter()
            .map(|hit| hit.x())
            .fold(initial.bar.viewport.max().x(), f64::min)
    } else {
        initial.bar.viewport.max().x()
    };
    let edge = LogicalPoint::new(
        content_right - 1.0,
        if placement.is_some() {
            // Keep the content probe clear of the tab's centered close button.
            initial.bar.viewport.y() + 1.0
        } else {
            initial.bar.viewport.y() + initial.bar.viewport.height() * 0.5
        },
    )
    .unwrap();
    if placement.is_some() {
        let hit = session.host_pointer_hit(TARGET, edge).unwrap();
        for receiver in [hit.delivery(), hit.drag_delivery()] {
            assert!(
                receiver.is_none_or(|receiver| matches!(
                    receiver.descriptor().role(),
                    DockspaceReceiverRole::TabBody
                        | DockspaceReceiverRole::TabGroupGrip
                        | DockspaceReceiverRole::TabGroupTrailingEmpty
                )),
                "the content edge must be unclaimed or tab chrome, away from close/control receivers: {placement:?}, {edge:?}, {receiver:?}"
            );
        }
    }
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
    let request = session.pointer_continuation().unwrap();
    assert_eq!(request.deadline(), Duration::from_millis(60));
    assert_eq!(request.surface(), TARGET);
    assert_eq!(request.position(), edge);
    let (baseline, _, outputs) = paint(session.begin_host_frame().unwrap());
    present(&mut session, outputs);
    assert_eq!(session.pointer_continuation(), Some(request));
    (session, moved, edge, baseline)
}

fn assert_durable_unchanged(session: &DockspaceSession, version: WorkspaceVersion) {
    assert_eq!(session.version(), version);
    assert!(session.has_active_gesture());
    for (item, surface, selected) in [(FIRST, SOURCE, true), (SECOND, SOURCE, false)]
        .into_iter()
        .chain(target_items().map(|item| (item, TARGET, item == ItemId::new(10))))
    {
        let view = session.view();
        let item = view.item(item).unwrap();
        assert_eq!(item.surface(), surface);
        assert_eq!(item.is_selected(), selected);
    }
    assert_eq!(
        session.view().item(FIRST).unwrap().tabs().items(),
        &[FIRST, SECOND]
    );
    assert_eq!(
        session.view().item(ItemId::new(10)).unwrap().tabs().items(),
        target_items().collect::<Vec<_>>()
    );
}

#[test]
fn same_side_overlay_controls_crossing_the_midpoint_preserve_stationary_edge_scrolling() {
    for placement in [
        TabStripControlPlacement::OverlayLeading,
        TabStripControlPlacement::OverlayTrailing,
    ] {
        let (mut session, moved, edge, baseline) = fixture_with_controls(Some(placement));
        let version = session.version();
        let raw = baseline.bar.viewport;
        assert_eq!(baseline.control_hits.len(), 3, "{placement:?}");
        let total_width: f64 = baseline.control_hits.iter().map(|hit| hit.width()).sum();
        assert_eq!(total_width, 180.0, "{placement:?}");
        assert!(total_width > raw.width() * 0.5, "{placement:?}");
        assert!(total_width < raw.width(), "{placement:?}");
        let midpoint = raw.x() + raw.width() * 0.5;
        let (content_left, content_right) = match placement {
            TabStripControlPlacement::OverlayLeading => {
                assert!(
                    baseline
                        .control_hits
                        .iter()
                        .any(|hit| { hit.x() + hit.width() * 0.5 > midpoint })
                );
                (
                    baseline
                        .control_hits
                        .iter()
                        .map(|hit| hit.max().x())
                        .fold(raw.x(), f64::max),
                    raw.max().x(),
                )
            }
            TabStripControlPlacement::OverlayTrailing => {
                assert!(
                    baseline
                        .control_hits
                        .iter()
                        .any(|hit| { hit.x() + hit.width() * 0.5 < midpoint })
                );
                (
                    raw.x(),
                    baseline
                        .control_hits
                        .iter()
                        .map(|hit| hit.x())
                        .fold(raw.max().x(), f64::min),
                )
            }
            _ => unreachable!(),
        };
        assert!(content_right > content_left, "{placement:?}");
        assert_eq!(edge.x(), content_right - 1.0);
        assert!(edge.x() > content_left, "{placement:?}");
        assert!(raw.contains(edge));
        assert!(!baseline.control_hits.iter().any(|hit| hit.contains(edge)));
        assert!(baseline.bar.maximum > 0.0);

        let wake = session.pointer_continuation().unwrap();
        assert_eq!(wake.deadline(), Duration::from_millis(60));
        assert_eq!(wake.position(), edge);
        let a = session.presented_surface(SOURCE).unwrap();
        let b = session.presented_surface(TARGET).unwrap();
        let mut frame = session.begin_host_frame().unwrap();
        frame
            .submit_host_pointer(timed_input(
                wake.event(),
                a,
                moved,
                b,
                edge,
                SurfacePointerReceiverFacts::unknown().with_clear_hover_lane(&b),
                60,
            ))
            .unwrap();
        let (scrolled, report, outputs) = paint(frame);
        assert_eq!(scrolled.bar.offset, 40.0, "{placement:?}");
        assert_eq!(scrolled.focus, baseline.focus);
        assert!(!report.workspace_changed());
        assert_eq!(report.before(), report.after());
        assert_durable_unchanged(&session, version);
        assert!(session.pointer_continuation().is_none());
        present(&mut session, outputs);
        assert_durable_unchanged(&session, version);
        assert_eq!(
            session.pointer_continuation().unwrap().deadline(),
            Duration::from_millis(110)
        );
    }
}

#[test]
fn stationary_edge_scroll_retries_after_drop_and_waits_for_each_exact_presented_output() {
    let (mut session, moved, edge, baseline) = fixture();
    let version = session.version();
    let request = session.pointer_continuation().unwrap();
    let a = session.presented_surface(SOURCE).unwrap();
    let old_b = session.presented_surface(TARGET).unwrap();
    let clear = SurfacePointerReceiverFacts::unknown().with_clear_hover_lane(&old_b);
    let mut frame = session.begin_host_frame().unwrap();
    frame
        .submit_host_pointer(timed_input(
            request.event(),
            a,
            moved,
            old_b,
            edge,
            clear,
            59,
        ))
        .unwrap();
    assert_eq!(inspect(&mut frame).bar.offset, 0.0);
    assert!(!finish(frame).workspace_changed());
    assert_eq!(session.pointer_continuation(), Some(request));

    let input = timed_input(request.event(), a, moved, old_b, edge, clear, 60);
    let mut abandoned = session.begin_host_frame().unwrap();
    abandoned.submit_host_pointer(input).unwrap();
    let candidate = inspect(&mut abandoned);
    assert_eq!(candidate.bar.offset, 40.0);
    assert_eq!(candidate.focus, baseline.focus);
    drop(abandoned);
    assert_durable_unchanged(&session, version);
    assert_eq!(session.pointer_continuation(), Some(request));
    assert_eq!(
        inspect(&mut session.begin_host_frame().unwrap()).bar.offset,
        0.0
    );

    let mut frame = session.begin_host_frame().unwrap();
    frame.submit_host_pointer(input).unwrap();
    let (first, report, mut outputs) = paint(frame);
    assert_eq!(first.bar.offset, 40.0);
    assert_eq!(first.focus, baseline.focus);
    assert!(!report.workspace_changed());
    assert_eq!(report.before(), report.after());
    assert_durable_unchanged(&session, version);
    assert!(session.pointer_continuation().is_none());
    let target_output = outputs.remove(
        outputs
            .iter()
            .position(|output| output.surface() == TARGET)
            .unwrap(),
    );
    present(&mut session, outputs);
    assert!(
        session.pointer_continuation().is_none(),
        "presenting only the source cannot authorize the next target-strip step"
    );
    present(&mut session, vec![target_output]);
    let next = session.pointer_continuation().unwrap();
    assert_eq!(next.deadline(), Duration::from_millis(110));
    assert_ne!(next.token(), request.token());
    assert_eq!(next.position(), edge);

    // A new token cannot borrow receiver facts from the predecessor emission.
    let current_a = session.presented_surface(SOURCE).unwrap();
    let current_b = session.presented_surface(TARGET).unwrap();
    let mut stale = session.begin_host_frame().unwrap();
    match stale.submit_host_pointer(timed_input(
        next.event(),
        current_a,
        moved,
        current_b,
        edge,
        clear,
        110,
    )) {
        Ok(()) => assert_eq!(inspect(&mut stale).bar.offset, 40.0),
        Err(error) => assert_eq!(error.kind(), DockspaceRuntimeErrorKind::HostProtocol),
    }
    drop(stale);
    assert_eq!(session.pointer_continuation(), Some(next));
    assert_durable_unchanged(&session, version);

    let mut offset = first.bar.offset;
    for _ in 0..32 {
        let Some(wake) = session.pointer_continuation() else {
            break;
        };
        let a = session.presented_surface(SOURCE).unwrap();
        let b = session.presented_surface(TARGET).unwrap();
        assert_eq!(wake.position(), edge, "stationary work needs no Moved edge");
        let mut frame = session.begin_host_frame().unwrap();
        frame
            .submit_host_pointer(timed_input(
                wake.event(),
                a,
                moved,
                b,
                edge,
                SurfacePointerReceiverFacts::unknown().with_clear_hover_lane(&b),
                u64::try_from(wake.deadline().as_millis()).unwrap(),
            ))
            .unwrap();
        let (snapshot, report, outputs) = paint(frame);
        let expected = (offset + 40.0).min(baseline.bar.maximum);
        assert_eq!(snapshot.bar.offset, expected);
        assert_eq!(snapshot.bar.maximum, baseline.bar.maximum);
        assert_eq!(snapshot.focus, baseline.focus);
        assert!(!report.workspace_changed());
        assert_eq!(report.before(), report.after());
        assert_durable_unchanged(&session, version);
        assert!(session.pointer_continuation().is_none());
        present(&mut session, outputs);
        offset = expected;
    }
    assert_eq!(offset, baseline.bar.maximum);
    assert!(session.pointer_continuation().is_none());
    for _ in 0..2 {
        let (snapshot, _, outputs) = paint(session.begin_host_frame().unwrap());
        assert_eq!(snapshot.bar.offset, offset);
        assert_eq!(snapshot.focus, baseline.focus);
        present(&mut session, outputs);
        assert!(session.pointer_continuation().is_none());
        assert_durable_unchanged(&session, version);
    }
}

#[test]
fn blocked_or_left_edge_clears_the_wake_and_raw_release_wins_before_a_due_continuation() {
    for scenario in ["blocked", "leave", "release"] {
        let (mut session, moved, edge, baseline) = fixture();
        let version = session.version();
        let wake: DockspacePointerContinuation = session.pointer_continuation().unwrap();
        let a = session.presented_surface(SOURCE).unwrap();
        let b = session.presented_surface(TARGET).unwrap();
        let clear = SurfacePointerReceiverFacts::unknown().with_clear_hover_lane(&b);
        let mut frame = session.begin_host_frame().unwrap();
        match scenario {
            "blocked" => frame
                .submit_host_pointer(timed_input(
                    wake.event(),
                    a,
                    moved,
                    b,
                    edge,
                    SurfacePointerReceiverFacts::unknown().with_blocked_hover(&b),
                    60,
                ))
                .unwrap(),
            "leave" => frame
                .submit_host_pointer_batch([
                    timed_input(
                        SurfacePointerEvent::Moved,
                        a,
                        moved,
                        b,
                        LogicalPoint::new(
                            baseline.bar.viewport.x() + baseline.bar.viewport.width() * 0.5,
                            baseline.bar.viewport.y() + baseline.bar.viewport.height() * 0.5,
                        )
                        .unwrap(),
                        clear,
                        59,
                    ),
                    timed_input(wake.event(), a, moved, b, edge, clear, 60),
                ])
                .unwrap(),
            "release" => frame
                .submit_host_pointer_batch([
                    // Block the release target to isolate terminal ordering
                    // from the independent docking-delivery decision.
                    timed_input(
                        SurfacePointerEvent::ButtonReleased(SurfacePointerButton::Primary),
                        a,
                        moved,
                        b,
                        edge,
                        SurfacePointerReceiverFacts::unknown().with_blocked_hover(&b),
                        59,
                    ),
                    timed_input(wake.event(), a, moved, b, edge, clear, 60),
                ])
                .unwrap(),
            _ => unreachable!(),
        }
        let (snapshot, report, outputs) = paint(frame);
        assert_eq!(snapshot.bar.offset, 0.0, "{scenario}");
        assert_eq!(snapshot.focus, baseline.focus, "{scenario}");
        assert!(!report.workspace_changed(), "{scenario}");
        assert_eq!(session.version(), version);
        assert!(session.pointer_continuation().is_none(), "{scenario}");
        assert_eq!(session.has_active_gesture(), scenario != "release");
        present(&mut session, outputs);
        assert!(session.pointer_continuation().is_none(), "{scenario}");
        for _ in 0..2 {
            let (idle, _, outputs) = paint(session.begin_host_frame().unwrap());
            assert_eq!(idle.bar.offset, 0.0, "{scenario}");
            assert_eq!(idle.focus, baseline.focus, "{scenario}");
            present(&mut session, outputs);
            assert!(session.pointer_continuation().is_none(), "{scenario}");
        }
        if scenario != "release" {
            let a = session.presented_surface(SOURCE).unwrap();
            let b = session.presented_surface(TARGET).unwrap();
            let mut frame = session.begin_host_frame().unwrap();
            frame
                .submit_host_pointer(timed_input(
                    wake.event(),
                    a,
                    moved,
                    b,
                    edge,
                    SurfacePointerReceiverFacts::unknown().with_clear_hover_lane(&b),
                    61,
                ))
                .unwrap();
            assert_eq!(inspect(&mut frame).bar.offset, 0.0, "{scenario}");
            assert!(!finish(frame).workspace_changed());
            assert!(session.pointer_continuation().is_none(), "{scenario}");
            assert_durable_unchanged(&session, version);
        }
    }
}
