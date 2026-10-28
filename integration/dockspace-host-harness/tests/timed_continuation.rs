//! Stationary hover selection through rollbackable public host pointer input.

use std::time::Duration;

use dockspace::geometry::{LogicalPoint, LogicalRect, LogicalSize};
use dockspace::model::{
    DockspaceActionOutcome, DockspaceLayout, DockspaceNode, DockspaceRootLayout,
    DockspaceSurfaceLayout, ItemId, RootId, SurfaceId,
};
use dockspace::policy::DockPolicy;
use dockspace::runtime::{
    DockspaceHostFrame, DockspacePaneFocusRequest, DockspacePreviewVisual,
    DockspaceReceiverDescriptor, DockspaceRuntimeErrorKind, DockspaceSession, HostFrameReport,
    HostInputOutcome, HostPointerInput, HostPointerRoute, PointerDeviceKind, PointerEventMetadata,
    SurfacePointerButton, SurfacePointerCapture, SurfacePointerEvent, SurfacePointerId,
    SurfacePointerReceiverFacts, SurfacePresentationResult, SurfaceUnavailableReason,
    UniformSurfaceMetrics,
};
use dockspace_host_harness::{OwnedRecord, RecordingRenderer};

const A: SurfaceId = SurfaceId::new(1);
const B: SurfaceId = SurfaceId::new(2);
const FIRST: ItemId = ItemId::new(1);
const SECOND: ItemId = ItemId::new(2);
const THIRD: ItemId = ItemId::new(3);
const FOURTH: ItemId = ItemId::new(4);
const POINTER: SurfacePointerId = SurfacePointerId::new(1);

fn session() -> DockspaceSession {
    let layout = DockspaceLayout::new([
        DockspaceSurfaceLayout::new(
            A,
            DockspaceRootLayout::new(RootId::new(1), DockspaceNode::central_tabs([FIRST, SECOND])),
        ),
        DockspaceSurfaceLayout::new(
            B,
            DockspaceRootLayout::new(RootId::new(2), DockspaceNode::central_tabs([THIRD, FOURTH])),
        ),
    ])
    .unwrap();
    let mut session = DockspaceSession::from_layout(layout, DockPolicy::default()).unwrap();
    session.enable_host_pointer().unwrap();
    let mut frame = session.begin_host_frame().unwrap();
    measure(&mut frame);
    frame.commit().unwrap();
    session
}

fn measure(frame: &mut DockspaceHostFrame<'_>) {
    let metrics = UniformSurfaceMetrics::new(
        LogicalRect::new(0.0, 0.0, 800.0, 600.0).unwrap(),
        LogicalSize::new(64.0, 48.0).unwrap(),
        96.0,
    )
    .unwrap();
    for surface in frame.surfaces() {
        frame.measure_surface(surface, metrics).unwrap();
    }
}

fn commit(mut frame: DockspaceHostFrame<'_>) -> HostFrameReport {
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    frame.commit().unwrap()
}

fn settle(session: &mut DockspaceSession) -> HostFrameReport {
    commit(session.begin_host_frame().unwrap())
}

fn paint(
    session: &mut DockspaceSession,
    source_item: ItemId,
) -> (
    [Option<DockspaceReceiverDescriptor>; 2],
    [Option<DockspacePaneFocusRequest>; 2],
) {
    let mut frame = session.begin_host_frame().unwrap();
    let observations = [(A, source_item), (B, FOURTH)].map(|(surface, item)| {
        let plan = frame.paint_plan(surface).unwrap().unwrap();
        let receiver = plan.tab_receiver(item);
        let focus = plan.pane_focus_request();
        let (recording, receipt) = RecordingRenderer::execute(plan).unwrap();
        if let Some(receiver) = receiver {
            assert!(recording.records.contains(&OwnedRecord::Receiver(receiver)));
        }
        frame.confirm_surface_painted(receipt).unwrap();
        (recording, receiver, focus)
    });
    let outputs = frame.commit().unwrap().take_painted_outputs();
    assert_eq!(outputs.len(), 2);
    let mut artifacts = Vec::new();
    for output in outputs {
        let index = [A, B]
            .iter()
            .position(|surface| *surface == output.surface())
            .unwrap();
        assert!(matches!(
            observations[index].0.records.last(),
            Some(OwnedRecord::SurfaceEnd(semantic))
                if output.matches_semantic_output(*semantic)
        ));
        artifacts.push(output.presentation_artifact());
        session
            .report_surface_presentation(output, SurfacePresentationResult::Presented)
            .unwrap();
    }
    let settled = settle(session);
    assert_eq!(settled.surface_presentation_commits().len(), 2);
    for artifact in artifacts {
        assert!(
            settled
                .surface_presentation_commits()
                .iter()
                .any(|ack| ack.presents(artifact))
        );
    }
    (
        observations.each_ref().map(|(_, receiver, _)| *receiver),
        observations.each_ref().map(|(_, _, focus)| *focus),
    )
}

#[test]
fn stationary_hover_selects_once_at_deadline_and_dropped_frame_keeps_token_retryable() {
    for (source_item, drop_due_frame, release_at_deadline) in [
        (FIRST, false, false),
        (FIRST, true, false),
        (SECOND, false, false),
        (SECOND, true, false),
        (FIRST, false, true),
    ] {
        let remaining_item = if source_item == FIRST { SECOND } else { FIRST };
        let mut session = session();
        assert!(session.view().item(FIRST).unwrap().is_selected());
        assert!(!session.view().item(SECOND).unwrap().is_selected());
        assert!(session.view().item(THIRD).unwrap().is_selected());
        assert!(!session.view().item(FOURTH).unwrap().is_selected());
        let (receivers, _) = paint(&mut session, source_item);
        let source = session
            .bind_presented_receiver(&receivers[0].unwrap())
            .unwrap();
        let target = session
            .bind_presented_receiver(&receivers[1].unwrap())
            .unwrap();
        let a = session.presented_surface(A).unwrap();
        let b = session.presented_surface(B).unwrap();
        let press = HostPointerRoute::new(a, source.center());
        let moved = LogicalPoint::new(700.0, 500.0).unwrap();
        let mut frame = session.begin_host_frame().unwrap();
        frame
            .submit_host_pointer_batch([
                HostPointerInput::new(
                    POINTER,
                    SurfacePointerEvent::ButtonPressed(SurfacePointerButton::Primary),
                    press,
                    press,
                    SurfacePointerCapture::ProviderEndpoint,
                    SurfacePointerReceiverFacts::delivery(&source),
                )
                .with_metadata(PointerEventMetadata::new(
                    PointerDeviceKind::Mouse,
                    Duration::ZERO,
                )),
                HostPointerInput::new(
                    POINTER,
                    SurfacePointerEvent::Moved,
                    HostPointerRoute::new(a, moved),
                    HostPointerRoute::new(b, target.center()),
                    SurfacePointerCapture::ProviderEndpoint,
                    SurfacePointerReceiverFacts::delivery(&source).with_clear_hover_lane(&b),
                )
                .with_metadata(PointerEventMetadata::new(
                    PointerDeviceKind::Mouse,
                    Duration::from_millis(10),
                )),
            ])
            .unwrap();
        if source_item == SECOND {
            // Pressing an unselected source changes A's pane requirement.
            measure(&mut frame);
        }
        let pressed = commit(frame);
        assert_eq!(pressed.workspace_changed(), source_item == SECOND);
        assert!(session.view().item(source_item).unwrap().is_selected());
        assert!(!session.view().item(remaining_item).unwrap().is_selected());
        assert!(session.has_active_gesture());

        let request = session
            .pointer_continuation()
            .expect("hovering FOURTH during the drag must schedule a finite wake");
        assert_eq!(request.pointer(), POINTER);
        assert_eq!(request.deadline(), Duration::from_millis(160));
        assert_eq!(request.surface(), B);
        assert_eq!(request.position(), target.center());
        assert_eq!(
            request.event(),
            SurfacePointerEvent::Continuation(request.token())
        );
        let (_, focus_before) = paint(&mut session, source_item);
        assert_eq!(session.pointer_continuation(), Some(request));

        let hover_samples: &[(u64, bool)] = if release_at_deadline {
            &[]
        } else {
            &[(159, false), (160, true), (161, false)]
        };
        for &(millis, selects) in hover_samples {
            // Each callback queries the current presented output, including
            // the newly painted B output after a successful selection.
            let a = session.presented_surface(A).unwrap();
            let b = session.presented_surface(request.surface()).unwrap();
            let continuation = |pointer| {
                HostPointerInput::new(
                    pointer,
                    request.event(),
                    HostPointerRoute::new(a, moved),
                    HostPointerRoute::new(b, request.position()),
                    SurfacePointerCapture::ProviderEndpoint,
                    SurfacePointerReceiverFacts::unknown().with_clear_hover_lane(&b),
                )
                .with_metadata(PointerEventMetadata::new(
                    PointerDeviceKind::Mouse,
                    Duration::from_millis(millis),
                ))
            };
            let input = continuation(request.pointer());

            if selects && !drop_due_frame {
                let before = session.version();
                let gesture_before = session.has_active_gesture();
                let wake_before = session.pointer_continuation();
                let mut foreign = session.begin_host_frame().unwrap();
                let error = foreign
                    .submit_host_pointer(continuation(SurfacePointerId::new(2)))
                    .unwrap_err();
                assert_eq!(error.kind(), DockspaceRuntimeErrorKind::HostProtocol);
                drop(foreign);
                assert_eq!(session.version(), before);
                assert_eq!(session.has_active_gesture(), gesture_before);
                assert_eq!(session.pointer_continuation(), wake_before);
            }

            if drop_due_frame && selects {
                let before = session.version();
                let mut abandoned = session.begin_host_frame().unwrap();
                abandoned.submit_host_pointer(input).unwrap();
                measure(&mut abandoned);
                assert!(abandoned.view().item(FOURTH).unwrap().is_selected());
                assert!(!abandoned.view().item(THIRD).unwrap().is_selected());
                drop(abandoned);
                assert_eq!(session.version(), before);
                assert!(session.view().item(THIRD).unwrap().is_selected());
                assert!(!session.view().item(FOURTH).unwrap().is_selected());
                assert!(session.has_active_gesture());
                assert_eq!(session.pointer_continuation(), Some(request));
            }

            let mut frame = session.begin_host_frame().unwrap();
            frame.submit_host_pointer(input).unwrap();
            if selects {
                // Selection changes B's pane measurement requirement.
                measure(&mut frame);
            }
            let report = commit(frame);
            let selections: Vec<_> = report
                .inputs()
                .iter()
                .filter_map(|outcome| match outcome {
                    HostInputOutcome::ProductActionApplied(DockspaceActionOutcome::Selected {
                        item,
                        changed,
                    }) => Some((*item, *changed)),
                    _ => None,
                })
                .collect();
            assert_eq!(
                selections,
                if selects {
                    vec![(FOURTH, true)]
                } else {
                    vec![]
                },
                "source_item={source_item:?}, drop_due_frame={drop_due_frame}, millis={millis}"
            );
            assert_eq!(report.workspace_changed(), selects);
            assert_eq!(report.before() == report.after(), !selects);
            assert_eq!(
                session.view().item(FOURTH).unwrap().is_selected(),
                millis >= 160
            );
            assert_eq!(
                session.view().item(THIRD).unwrap().is_selected(),
                millis < 160
            );
            assert!(session.has_active_gesture());
            assert_eq!(session.view().item(source_item).unwrap().surface(), A);
            assert_eq!(
                session.view().item(source_item).unwrap().tabs().items(),
                &[FIRST, SECOND]
            );
            assert_eq!(session.view().item(FOURTH).unwrap().surface(), B);
            assert_eq!(
                session.view().item(FOURTH).unwrap().tabs().items(),
                &[THIRD, FOURTH]
            );

            let (_, focus_after) = paint(&mut session, source_item);
            assert_eq!(
                focus_after, focus_before,
                "hover selection must not add pane focus"
            );
            assert!(session.has_active_gesture());
            assert_eq!(
                session.pointer_continuation(),
                if millis < 160 { Some(request) } else { None }
            );
        }

        if drop_due_frame {
            let third_point = {
                let mut inspect = session.begin_host_frame().unwrap();
                inspect.paint_plan(B).unwrap().unwrap().tab_receiver(THIRD).unwrap().center()
            };
            for (index, failure) in ["blocked", "unknown", "capture"].into_iter().enumerate() {
                let now = 200 + u64::try_from(index).unwrap() * 400;
                let a = session.presented_surface(A).unwrap();
                let b = session.presented_surface(B).unwrap();
                let mut frame = session.begin_host_frame().unwrap();
                frame.submit_host_pointer(HostPointerInput::new(
                    POINTER, SurfacePointerEvent::Moved,
                    HostPointerRoute::new(a, moved), HostPointerRoute::new(b, third_point),
                    SurfacePointerCapture::ProviderEndpoint,
                    SurfacePointerReceiverFacts::unknown().with_clear_hover_lane(&b),
                ).with_metadata(PointerEventMetadata::new(PointerDeviceKind::Mouse, Duration::from_millis(now)))).unwrap();
                assert!(!commit(frame).workspace_changed());
                let wake = session.pointer_continuation().unwrap();
                assert_eq!(wake.deadline(), Duration::from_millis(now + 150));
                let facts = match failure {
                    "blocked" => SurfacePointerReceiverFacts::unknown().with_blocked_hover(&b),
                    "unknown" => SurfacePointerReceiverFacts::unknown(),
                    "capture" => SurfacePointerReceiverFacts::unknown().with_clear_hover_lane(&b),
                    _ => unreachable!(),
                };
                let mut frame = session.begin_host_frame().unwrap();
                frame.submit_host_pointer(HostPointerInput::new(
                    wake.pointer(), wake.event(),
                    HostPointerRoute::new(a, moved), HostPointerRoute::new(b, third_point),
                    if failure == "capture" { SurfacePointerCapture::Foreign } else { SurfacePointerCapture::ProviderEndpoint },
                    facts,
                ).with_metadata(PointerEventMetadata::new(PointerDeviceKind::Mouse, wake.deadline()))).unwrap();
                assert!(!commit(frame).workspace_changed());
                assert!(!session.view().item(THIRD).unwrap().is_selected());
                assert!(session.view().item(FOURTH).unwrap().is_selected());
                assert!(session.pointer_continuation().is_none());
                assert_eq!(session.has_active_gesture(), failure != "capture");
                if failure != "capture" {
                    // Once invalidated, this token cannot borrow a later clear
                    // receiver to complete the discarded dwell interval.
                    let mut frame = session.begin_host_frame().unwrap();
                    frame.submit_host_pointer(HostPointerInput::new(
                        wake.pointer(), wake.event(),
                        HostPointerRoute::new(a, moved), HostPointerRoute::new(b, third_point),
                        SurfacePointerCapture::ProviderEndpoint,
                        SurfacePointerReceiverFacts::unknown().with_clear_hover_lane(&b),
                    ).with_metadata(PointerEventMetadata::new(PointerDeviceKind::Mouse, Duration::from_millis(now + 151)))).unwrap();
                    assert!(!commit(frame).workspace_changed());
                    assert!(!session.view().item(THIRD).unwrap().is_selected());
                    assert!(session.pointer_continuation().is_none());
                }
            }
        }

        if !drop_due_frame {
            // The hover selection has been measured, painted, and presented.
            // Finish the original drag against that new scene and fingerprint.
            let content = if release_at_deadline {
                request.position()
            } else {
                let mut inspect = session.begin_host_frame().unwrap();
                inspect
                    .paint_plan(B)
                    .unwrap()
                    .unwrap()
                    .center_drop_receiver_for_item(FOURTH)
                    .expect("the selected FOURTH pane must expose a center drop receiver")
                    .center()
            };
            let a = session.presented_surface(A).unwrap();
            let b = session.presented_surface(B).unwrap();
            let mut frame = session.begin_host_frame().unwrap();
            frame
                .submit_host_pointer_batch(
                    [
                        (SurfacePointerEvent::Moved, if release_at_deadline { 160 } else { 162 }),
                        (
                            SurfacePointerEvent::ButtonReleased(SurfacePointerButton::Primary),
                            if release_at_deadline { 160 } else { 163 },
                        ),
                    ]
                    .map(|(event, millis)| {
                        HostPointerInput::new(
                            request.pointer(),
                            event,
                            HostPointerRoute::new(a, moved),
                            HostPointerRoute::new(b, content),
                            SurfacePointerCapture::ProviderEndpoint,
                            SurfacePointerReceiverFacts::unknown().with_clear_hover_lane(&b),
                        )
                        .with_metadata(PointerEventMetadata::new(
                            PointerDeviceKind::Mouse,
                            Duration::from_millis(millis),
                        ))
                    }),
                )
                .unwrap();
            if release_at_deadline {
                // The matching preview was already presented before this
                // batch. The physical terminal can therefore deliver now;
                // a due hover task must not mutate its target scene first.
                assert_eq!(frame.view().item(source_item).unwrap().surface(), B);
                assert!(commit(frame).workspace_changed());
                assert_eq!(session.view().item(source_item).unwrap().surface(), B);
                assert_eq!(session.view().item(remaining_item).unwrap().surface(), A);
                assert!(!session.has_active_gesture());
                assert!(session.pointer_continuation().is_none());
                continue;
            }
            let recordings = [A, B].map(|surface| {
                let plan = frame.paint_plan(surface).unwrap().unwrap();
                let (recording, receipt) = RecordingRenderer::execute(plan).unwrap();
                frame.confirm_surface_painted(receipt).unwrap();
                recording
            });
            assert!(recordings[1].records.iter().any(|record| matches!(
                record,
                OwnedRecord::Preview(DockspacePreviewVisual::Dock { surface: B, .. })
            )));
            let mut released = frame.commit().unwrap();
            assert!(!released.workspace_changed());
            assert_eq!(session.view().item(source_item).unwrap().surface(), A);
            assert!(!session.has_active_gesture());

            let outputs = released.take_painted_outputs();
            assert_eq!(outputs.len(), 2);
            let mut artifacts = Vec::new();
            for output in outputs {
                let index = [A, B]
                    .iter()
                    .position(|surface| *surface == output.surface())
                    .unwrap();
                assert!(matches!(
                    recordings[index].records.last(),
                    Some(OwnedRecord::SurfaceEnd(semantic))
                        if output.matches_semantic_output(*semantic)
                ));
                artifacts.push(output.presentation_artifact());
                session
                    .report_surface_presentation(output, SurfacePresentationResult::Presented)
                    .unwrap();
            }
            assert_eq!(session.view().item(source_item).unwrap().surface(), A);
            let settled = settle(&mut session);
            for artifact in artifacts {
                assert_eq!(
                    settled
                        .surface_presentation_commits()
                        .iter()
                        .filter(|ack| ack.settles(artifact))
                        .count(),
                    1,
                    "each exact release output must settle once"
                );
            }
            assert!(settled.workspace_changed());
            assert_eq!(session.view().item(source_item).unwrap().surface(), B);
            assert_eq!(
                session.view().item(source_item).unwrap().tabs().items(),
                &[THIRD, FOURTH, source_item]
            );
            assert_eq!(session.view().item(remaining_item).unwrap().surface(), A);
            assert_eq!(
                session.view().item(remaining_item).unwrap().tabs().items(),
                &[remaining_item]
            );
            assert_eq!(session.view().item(THIRD).unwrap().surface(), B);
            assert_eq!(session.view().item(FOURTH).unwrap().surface(), B);
            assert!(!session.has_active_gesture());
            assert!(session.pointer_continuation().is_none());
        }
    }
}
