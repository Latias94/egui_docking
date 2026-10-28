//! Touch arbitration through the public host input and presentation contracts.

use std::time::Duration;

use dockspace::geometry::{LogicalPoint, LogicalRect, LogicalSize};
use dockspace::model::{
    DockspaceActionOutcome, DockspaceAxis, DockspaceContainedLayout, DockspaceLayout,
    DockspaceNode, DockspaceRootLayout, DockspaceSurfaceLayout, FloatingPresentationId, ItemId,
    RootId, SurfaceId,
};
use dockspace::policy::DockPolicy;
use dockspace::runtime::{
    ContainedResizeDirection, DockspaceHostFrame, DockspaceManipulationDelivery,
    DockspaceManipulationPendingReason, DockspaceManipulationStatus, DockspacePaneFocusRequest,
    DockspacePreviewVisual, DockspaceRuntimeErrorKind, DockspaceSession, DockspaceTouchEvent,
    DockspaceVisualKind, HostFrameReport, HostInputOutcome, HostPointerInput, HostPointerRoute,
    PointerDeviceKind, PointerEventMetadata, PresentedDockReceiver,
    SurfacePaintProgramOperation as Operation, SurfacePaintProgramStep as Step,
    SurfacePointerButton, SurfacePointerCapture, SurfacePointerEvent, SurfacePointerId,
    SurfacePointerReceiverFacts, SurfacePresentationResult, SurfaceScrollDelta,
    SurfaceScrollDeviceId, SurfaceScrollEvent, SurfaceScrollModifiers, SurfaceScrollMomentum,
    SurfaceScrollPhase, SurfaceUnavailableReason, UniformSurfaceMetrics,
};
use dockspace_host_harness::{OwnedRecord, RecordingRenderer};

const SURFACE: SurfaceId = SurfaceId::new(1);
const ROOT: RootId = RootId::new(1);
const FIRST: ItemId = ItemId::new(1);
const SECOND: ItemId = ItemId::new(2);
const POINTER: SurfacePointerId = SurfacePointerId::new(1);
const PRESS: SurfacePointerEvent =
    SurfacePointerEvent::ButtonPressed(SurfacePointerButton::Primary);
const END: SurfacePointerEvent = SurfacePointerEvent::ContactEnded(SurfacePointerButton::Primary);

fn session() -> DockspaceSession {
    let layout = DockspaceLayout::new([DockspaceSurfaceLayout::new(
        SURFACE,
        DockspaceRootLayout::new(ROOT, DockspaceNode::central_tabs([FIRST, SECOND])),
    )])
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
) -> (
    RecordingRenderer,
    PresentedDockReceiver,
    Option<DockspacePaneFocusRequest>,
) {
    let mut frame = session.begin_host_frame().unwrap();
    let plan = frame.paint_plan(SURFACE).unwrap().unwrap();
    let descriptor = plan.tab_receiver(SECOND).unwrap();
    let focus = plan.pane_focus_request();
    let (recording, receipt) = RecordingRenderer::execute(plan).unwrap();
    assert!(
        recording
            .records
            .contains(&OwnedRecord::Receiver(descriptor))
    );
    frame.confirm_surface_painted(receipt).unwrap();
    let mut outputs = frame.commit().unwrap().take_painted_outputs();
    assert_eq!(outputs.len(), 1);
    let output = outputs.pop().unwrap();
    assert!(matches!(
        recording.records.last(),
        Some(OwnedRecord::SurfaceEnd(semantic)) if output.matches_semantic_output(*semantic)
    ));
    let artifact = output.presentation_artifact();
    session
        .report_surface_presentation(output, SurfacePresentationResult::Presented)
        .unwrap();
    let settled = settle(session);
    let [ack] = settled.surface_presentation_commits() else {
        panic!("the exact touch-test output must settle once")
    };
    assert!(ack.presents(artifact));
    (
        recording,
        session.bind_presented_receiver(&descriptor).unwrap(),
        focus,
    )
}

fn touch_input<'receiver>(
    session: &DockspaceSession,
    receiver: &'receiver PresentedDockReceiver,
    event: SurfacePointerEvent,
    point: LogicalPoint,
    millis: Option<u64>,
) -> HostPointerInput<'receiver> {
    let route = HostPointerRoute::new(session.presented_surface(SURFACE).unwrap(), point);
    HostPointerInput::new(
        POINTER,
        event,
        route,
        route,
        SurfacePointerCapture::ProviderEndpoint,
        SurfacePointerReceiverFacts::delivery(receiver),
    )
    .with_metadata(millis.map_or_else(
        || PointerEventMetadata::untimed(PointerDeviceKind::Touch),
        |millis| PointerEventMetadata::new(PointerDeviceKind::Touch, Duration::from_millis(millis)),
    ))
}

#[test]
fn inactive_tab_tap_waits_within_slop_and_selects_once_on_early_contact_end() {
    let mut session = session();
    let (_, receiver, focus_before) = paint(&mut session);
    let before = session.version();
    let point = receiver.center();
    let input = touch_input(&session, &receiver, PRESS, point, Some(0));
    let mut frame = session.begin_host_frame().unwrap();
    frame.submit_host_pointer(input).unwrap();
    let pressed = commit(frame);
    assert!(pressed.inputs().is_empty());
    assert!(!pressed.workspace_changed());
    assert_eq!(session.version(), before);
    assert!(session.view().item(FIRST).unwrap().is_selected());
    assert!(!session.view().item(SECOND).unwrap().is_selected());
    assert!(session.has_active_gesture());
    let request = session.pointer_continuation().unwrap();
    assert_eq!(request.pointer(), POINTER);
    assert_eq!(request.surface(), SURFACE);
    assert_eq!(request.position(), point);
    assert_eq!(request.deadline(), Duration::from_millis(350));
    let (_, receiver, focus) = paint(&mut session);
    assert_eq!(focus, focus_before);

    let moved = LogicalPoint::new(point.x() + 8.0, point.y()).unwrap();
    assert!(receiver.bounds().contains(moved));
    let input = touch_input(
        &session,
        &receiver,
        SurfacePointerEvent::Moved,
        moved,
        Some(100),
    );
    let mut frame = session.begin_host_frame().unwrap();
    frame.submit_host_pointer(input).unwrap();
    let motion = commit(frame);
    assert!(motion.inputs().is_empty());
    assert_eq!(session.version(), before);
    let pending = session.pointer_continuation().unwrap();
    assert_eq!(pending.deadline(), Duration::from_millis(350));
    assert_eq!(pending.position(), moved);
    assert!(session.has_active_gesture());
    let (_, receiver, focus) = paint(&mut session);
    assert_eq!(focus, focus_before);
    assert!(session.view().item(FIRST).unwrap().is_selected());

    let input = touch_input(&session, &receiver, END, moved, Some(200));
    let mut frame = session.begin_host_frame().unwrap();
    frame.submit_host_pointer(input).unwrap();
    measure(&mut frame);
    let released = commit(frame);
    assert_eq!(
        released.inputs(),
        &[HostInputOutcome::ProductActionApplied(
            DockspaceActionOutcome::Selected {
                item: SECOND,
                changed: true,
            }
        )]
    );
    assert!(released.workspace_changed());
    assert!(session.view().item(SECOND).unwrap().is_selected());
    assert!(!session.has_active_gesture());
    assert!(session.pointer_continuation().is_none());
    let (_, receiver, focus_after) = paint(&mut session);
    let selected = session.version();
    for millis in [200, 400] {
        let input = touch_input(&session, &receiver, END, moved, Some(millis));
        let mut frame = session.begin_host_frame().unwrap();
        frame.submit_host_pointer(input).unwrap();
        let duplicate = commit(frame);
        assert!(duplicate.inputs().is_empty());
        assert!(!duplicate.workspace_changed());
        assert_eq!(session.version(), selected);
        assert!(session.view().item(SECOND).unwrap().is_selected());
        assert!(!session.has_active_gesture());
        assert!(session.pointer_continuation().is_none());
    }
    assert_eq!(paint(&mut session).2, focus_after);
}

#[test]
fn stationary_long_press_arms_once_and_requests_one_menu_after_dropped_frame_retry() {
    for drop_due_frame in [false, true] {
        let mut session = session();
        let (initial, receiver, focus_before) = paint(&mut session);
        let before = session.version();
        let point = receiver.center();
        let input = touch_input(&session, &receiver, PRESS, point, Some(0));
        let mut frame = session.begin_host_frame().unwrap();
        frame.submit_host_pointer(input).unwrap();
        assert!(commit(frame).inputs().is_empty());
        let request = session.pointer_continuation().unwrap();
        assert_eq!(request.deadline(), Duration::from_millis(350));

        for millis in [349, 350] {
            let (_, receiver, focus) = paint(&mut session);
            assert_eq!(focus, focus_before);
            let input = touch_input(
                &session,
                &receiver,
                request.event(),
                request.position(),
                Some(millis),
            );
            if drop_due_frame && millis == 350 {
                let mut abandoned = session.begin_host_frame().unwrap();
                abandoned.submit_host_pointer(input).unwrap();
                drop(abandoned);
                assert_eq!(session.version(), before);
                assert!(session.view().item(FIRST).unwrap().is_selected());
                assert!(!session.view().item(SECOND).unwrap().is_selected());
                assert!(session.has_active_gesture());
                assert_eq!(session.pointer_continuation(), Some(request));
            }
            let mut frame = session.begin_host_frame().unwrap();
            frame.submit_host_pointer(input).unwrap();
            let report = commit(frame);
            if millis == 349 {
                assert!(report.inputs().is_empty());
                assert_eq!(session.pointer_continuation(), Some(request));
            } else {
                assert_eq!(
                    report.inputs(),
                    &[HostInputOutcome::Touch(DockspaceTouchEvent::Armed {
                        pointer: POINTER,
                        surface: SURFACE,
                    })]
                );
                assert!(session.pointer_continuation().is_none());
            }
            assert!(!report.workspace_changed());
            assert_eq!(session.version(), before);
            assert!(session.has_active_gesture());
            assert!(session.view().item(FIRST).unwrap().is_selected());
            assert!(!session.view().item(SECOND).unwrap().is_selected());
        }

        let (_, receiver, focus) = paint(&mut session);
        assert_eq!(focus, focus_before);
        let input = touch_input(&session, &receiver, END, point, Some(360));
        let mut frame = session.begin_host_frame().unwrap();
        frame.submit_host_pointer(input).unwrap();
        let released = commit(frame);
        let [HostInputOutcome::Touch(DockspaceTouchEvent::ContextMenuRequested(menu))] =
            released.inputs()
        else {
            panic!(
                "one stationary armed release must request exactly one semantic context menu: {released:?}"
            )
        };
        assert_eq!(menu.version(), session.version());
        assert_eq!(menu.pointer(), POINTER);
        assert_eq!(menu.surface(), SURFACE);
        assert_eq!(menu.root(), ROOT);
        assert_eq!(menu.item(), Some(SECOND));
        assert_eq!(menu.position(), point);
        assert!(!released.workspace_changed());
        assert_eq!(session.version(), before);
        assert!(session.view().item(FIRST).unwrap().is_selected());
        assert!(!session.view().item(SECOND).unwrap().is_selected());
        assert!(!session.has_active_gesture());
        assert!(session.pointer_continuation().is_none());

        let (after, receiver, focus) = paint(&mut session);
        assert_eq!(focus, focus_before);
        let geometry = |recording: RecordingRenderer| {
            recording
                .records
                .into_iter()
                .filter(|record| {
                    matches!(
                        record,
                        OwnedRecord::Geometry { .. } | OwnedRecord::PaneContent { .. }
                    )
                })
                .collect::<Vec<_>>()
        };
        assert_eq!(geometry(after), geometry(initial));
        let input = touch_input(&session, &receiver, request.event(), point, Some(400));
        let mut stale = session.begin_host_frame().unwrap();
        assert_eq!(
            stale.submit_host_pointer(input).unwrap_err().kind(),
            DockspaceRuntimeErrorKind::HostProtocol
        );
        drop(stale);
        assert_eq!(session.version(), before);
        assert!(!session.has_active_gesture());
        assert!(session.pointer_continuation().is_none());
        assert!(settle(&mut session).inputs().is_empty());
    }
}

#[test]
fn touch_movement_yields_to_content_and_untimed_touch_can_still_tap() {
    for timed in [true, false] {
        let mut session = session();
        let (_, receiver, focus_before) = paint(&mut session);
        let point = receiver.center();
        let before = session.version();
        let input = touch_input(&session, &receiver, PRESS, point, timed.then_some(0));
        let mut frame = session.begin_host_frame().unwrap();
        frame.submit_host_pointer(input).unwrap();
        assert!(commit(frame).inputs().is_empty());
        assert_eq!(session.pointer_continuation().is_some(), timed);
        assert_eq!(session.version(), before);

        if !timed {
            let input = touch_input(&session, &receiver, END, point, None);
            let mut frame = session.begin_host_frame().unwrap();
            frame.submit_host_pointer(input).unwrap();
            measure(&mut frame);
            let released = commit(frame);
            assert_eq!(
                released.inputs(),
                &[HostInputOutcome::ProductActionApplied(
                    DockspaceActionOutcome::Selected {
                        item: SECOND,
                        changed: true,
                    }
                )]
            );
            assert!(session.view().item(SECOND).unwrap().is_selected());
            assert!(!session.has_active_gesture());
            assert!(session.pointer_continuation().is_none());
            continue;
        }

        let moved = LogicalPoint::new(point.x() + 9.0, point.y()).unwrap();
        let input = touch_input(
            &session,
            &receiver,
            SurfacePointerEvent::Moved,
            moved,
            Some(10),
        );
        let mut frame = session.begin_host_frame().unwrap();
        frame.submit_host_pointer(input).unwrap();
        let yielded = commit(frame);
        assert_eq!(
            yielded.inputs(),
            &[HostInputOutcome::Touch(
                DockspaceTouchEvent::YieldedToContent {
                    pointer: POINTER,
                    surface: SURFACE,
                }
            )]
        );
        assert!(!yielded.workspace_changed());
        assert_eq!(session.version(), before);
        assert!(session.view().item(FIRST).unwrap().is_selected());
        assert!(!session.view().item(SECOND).unwrap().is_selected());
        assert!(!session.has_active_gesture());
        assert!(session.pointer_continuation().is_none());
        let (_, receiver, focus) = paint(&mut session);
        assert_eq!(focus, focus_before);

        let surface = session.presented_surface(SURFACE).unwrap();
        let route = HostPointerRoute::new(surface, moved);
        let scroll = HostPointerInput::new(
            POINTER,
            SurfacePointerEvent::Scrolled(SurfaceScrollEvent::new(
                SurfaceScrollDeviceId::new(1),
                SurfaceScrollPhase::Discrete {
                    delta: SurfaceScrollDelta::LogicalPoints { x: 0.0, y: 16.0 },
                },
                SurfaceScrollMomentum::Direct,
                SurfaceScrollModifiers::new(false, false, false, false),
            )),
            route,
            route,
            SurfacePointerCapture::ProviderEndpoint,
            SurfacePointerReceiverFacts::unknown(),
        )
        .with_metadata(PointerEventMetadata::new(
            PointerDeviceKind::Touch,
            Duration::from_millis(20),
        ));
        let late_end = touch_input(&session, &receiver, END, moved, Some(400));
        let mut frame = session.begin_host_frame().unwrap();
        frame.submit_host_pointer_batch([scroll, late_end]).unwrap();
        let tail = commit(frame);
        assert!(tail.inputs().is_empty());
        assert!(!tail.workspace_changed());
        assert_eq!(session.version(), before);
        assert!(session.view().item(FIRST).unwrap().is_selected());
        assert!(!session.view().item(SECOND).unwrap().is_selected());
        assert!(!session.has_active_gesture());
        assert!(session.pointer_continuation().is_none());
        assert_eq!(session.view().item(SECOND).unwrap().surface(), SURFACE);
        assert_eq!(
            session.view().item(SECOND).unwrap().tabs().items(),
            &[FIRST, SECOND]
        );
        assert_eq!(paint(&mut session).2, focus_before);
    }
}

#[test]
fn armed_touch_reuses_splitter_terminal_and_contained_presentation_barriers() {
    #[derive(Clone, Copy, Debug)]
    enum Gesture {
        Splitter,
        ContainedMove,
        ContainedResize,
    }

    let floating = FloatingPresentationId::new(1);
    let floating_rect = LogicalRect::new(240.0, 200.0, 320.0, 240.0).unwrap();
    let durable = |session: &DockspaceSession| {
        let weights = session
            .view()
            .root(ROOT)
            .unwrap()
            .content()
            .unwrap()
            .split()
            .unwrap()
            .weights()
            .collect::<Vec<_>>();
        (weights, session.view().contained(floating).unwrap().rect())
    };
    let geometry = |recording: &RecordingRenderer| {
        recording
            .records
            .iter()
            .filter_map(|record| match record {
                OwnedRecord::Geometry { visual, bounds, .. }
                    if !matches!(
                        visual.kind(),
                        DockspaceVisualKind::Contained
                            | DockspaceVisualKind::DragSource
                            | DockspaceVisualKind::DropGuide
                            | DockspaceVisualKind::DropTarget
                    ) =>
                {
                    Some((*visual, *bounds))
                }
                _ => None,
            })
            .collect::<Vec<_>>()
    };

    for gesture in [
        Gesture::ContainedMove,
        Gesture::ContainedResize,
        Gesture::Splitter,
    ] {
        let split = DockspaceNode::equal_split(
            DockspaceAxis::Horizontal,
            [DockspaceNode::tabs([FIRST]), DockspaceNode::tabs([SECOND])],
        )
        .unwrap();
        let layout = DockspaceLayout::new([DockspaceSurfaceLayout::new(
            SURFACE,
            DockspaceRootLayout::new(ROOT, split),
        )
        .with_contained(DockspaceContainedLayout::new(
            floating,
            DockspaceRootLayout::new(RootId::new(2), DockspaceNode::tabs([ItemId::new(3)])),
            floating_rect,
        ))])
        .unwrap();
        let mut session = DockspaceSession::from_layout(layout, DockPolicy::default()).unwrap();
        session.enable_host_pointer().unwrap();
        let mut frame = session.begin_host_frame().unwrap();
        measure(&mut frame);
        frame.commit().unwrap();
        let (initial, _, _) = paint(&mut session);
        let before = session.version();
        let initial_durable = durable(&session);

        let descriptor = {
            let mut inspect = session.begin_host_frame().unwrap();
            let plan = inspect.paint_plan(SURFACE).unwrap().unwrap();
            let mut receiver = None;
            plan.visit_surface_program(|step| {
                match (gesture, step) {
                    (
                        Gesture::Splitter,
                        Step::Operation {
                            operation: Operation::Splitter(splitter),
                            ..
                        },
                    ) => {
                        receiver = plan.receiver_for_splitter(splitter);
                    }
                    (_, Step::RootBegin { root, .. }) => {
                        if let Some(contained) = root.contained() {
                            receiver = match gesture {
                                Gesture::ContainedMove => {
                                    plan.receiver_for_contained_title(contained)
                                }
                                Gesture::ContainedResize => {
                                    let handle = contained
                                        .resize()
                                        .find(|handle| {
                                            handle.direction()
                                                == ContainedResizeDirection::SouthEast
                                        })
                                        .unwrap();
                                    plan.receiver_for_contained_resize(contained, handle)
                                }
                                Gesture::Splitter => receiver,
                            };
                        }
                    }
                    _ => {}
                }
                Ok::<(), ()>(())
            })
            .unwrap();
            receiver.unwrap()
        };
        let receiver = session.bind_presented_receiver(&descriptor).unwrap();
        let press = match gesture {
            // Keep the shared main-root splitter clear of the contained window.
            Gesture::Splitter => LogicalPoint::new(receiver.center().x(), 100.0).unwrap(),
            _ => receiver.center(),
        };
        assert!(receiver.bounds().contains(press));
        let input = touch_input(&session, &receiver, PRESS, press, Some(0));
        let mut frame = session.begin_host_frame().unwrap();
        frame.submit_host_pointer(input).unwrap();
        let pressed = commit(frame);
        assert!(pressed.inputs().is_empty());
        assert!(!pressed.workspace_changed());
        assert_eq!(session.version(), before);
        assert_eq!(durable(&session), initial_durable);
        let request = session.pointer_continuation().unwrap();
        assert_eq!(request.deadline(), Duration::from_millis(350));

        for millis in [349, 350] {
            let input = touch_input(&session, &receiver, request.event(), press, Some(millis));
            let mut frame = session.begin_host_frame().unwrap();
            frame.submit_host_pointer(input).unwrap();
            let report = commit(frame);
            if millis == 349 {
                assert!(report.inputs().is_empty());
                assert_eq!(session.pointer_continuation(), Some(request));
            } else {
                assert_eq!(
                    report.inputs(),
                    &[HostInputOutcome::Touch(DockspaceTouchEvent::Armed {
                        pointer: POINTER,
                        surface: SURFACE,
                    })]
                );
                assert!(session.pointer_continuation().is_none());
            }
            assert!(!report.workspace_changed());
            assert!(session.has_active_gesture());
            assert_eq!(session.version(), before);
            assert_eq!(durable(&session), initial_durable);
        }

        let (dx, dy) = match gesture {
            Gesture::Splitter => (-36.0, 0.0),
            Gesture::ContainedMove => (36.0, 24.0),
            Gesture::ContainedResize => (40.0, 24.0),
        };
        let moved = LogicalPoint::new(press.x() + dx, press.y() + dy).unwrap();
        let expected_rect = match gesture {
            Gesture::Splitter => floating_rect,
            Gesture::ContainedMove => LogicalRect::new(276.0, 224.0, 320.0, 240.0).unwrap(),
            Gesture::ContainedResize => LogicalRect::new(240.0, 200.0, 360.0, 264.0).unwrap(),
        };
        let surface = session.presented_surface(SURFACE).unwrap();
        let route = HostPointerRoute::new(surface, moved);
        let input = HostPointerInput::new(
            POINTER,
            SurfacePointerEvent::Moved,
            route,
            route,
            SurfacePointerCapture::ProviderEndpoint,
            SurfacePointerReceiverFacts::unknown().with_clear_hover_lane(&surface),
        )
        .with_metadata(PointerEventMetadata::new(
            PointerDeviceKind::Touch,
            Duration::from_millis(360),
        ));
        let mut frame = session.begin_host_frame().unwrap();
        frame.submit_host_pointer(input).unwrap();
        if matches!(gesture, Gesture::Splitter) {
            measure(&mut frame);
        }
        let motion = commit(frame);
        assert!(!motion.workspace_changed(), "{gesture:?}");
        assert!(motion.manipulation_feedback().is_empty(), "{gesture:?}");
        assert_eq!(session.version(), before);
        assert_eq!(durable(&session), initial_durable);
        assert!(session.has_active_gesture());
        assert!(session.pointer_continuation().is_none());

        // Inspect the committed motion's proposal without presenting it.
        // Contained delivery waits for its exact output; splitter release uses
        // its already presented source handle and frozen resize constraints.
        let proposal = {
            let mut inspect = session.begin_host_frame().unwrap();
            let plan = inspect.paint_plan(SURFACE).unwrap().unwrap();
            RecordingRenderer::execute(plan).unwrap().0
        };
        assert_ne!(geometry(&proposal), geometry(&initial), "{gesture:?}");
        match gesture {
            Gesture::ContainedMove => assert!(proposal.records.iter().any(|record| matches!(
                record,
                OwnedRecord::Preview(DockspacePreviewVisual::Contained { surface: SURFACE, rect, .. })
                    if *rect == expected_rect
            ))),
            Gesture::ContainedResize => assert!(proposal.records.iter().any(|record| matches!(
                record,
                OwnedRecord::TransformPreview(id, rect) if *id == floating && *rect == expected_rect
            ))),
            Gesture::Splitter => {}
        }
        assert_eq!(session.version(), before);
        assert_eq!(durable(&session), initial_durable);

        let input = HostPointerInput::new(
            POINTER,
            END,
            route,
            route,
            SurfacePointerCapture::ProviderEndpoint,
            SurfacePointerReceiverFacts::unknown().with_clear_hover_lane(&surface),
        )
        .with_metadata(PointerEventMetadata::new(
            PointerDeviceKind::Touch,
            Duration::from_millis(370),
        ));
        let mut frame = session.begin_host_frame().unwrap();
        frame.submit_host_pointer(input).unwrap();
        let released = commit(frame);
        if matches!(gesture, Gesture::Splitter) {
            assert!(released.workspace_changed());
            let [terminal] = released.manipulation_feedback() else {
                panic!("one shared splitter terminal is required: {released:?}")
            };
            assert_eq!(
                terminal.status(),
                DockspaceManipulationStatus::Delivered(
                    DockspaceManipulationDelivery::SplitterResize
                )
            );
            let committed = durable(&session);
            assert!(committed.0[0] < initial_durable.0[0]);
            assert_eq!(committed.1, initial_durable.1);
            assert!(!session.has_active_gesture());
            assert!(session.pointer_continuation().is_none());
            let version = session.version();
            for _ in 0..2 {
                let repeated = settle(&mut session);
                assert!(!repeated.workspace_changed());
                assert!(repeated.manipulation_feedback().is_empty());
                assert_eq!(session.version(), version);
                assert_eq!(durable(&session), committed);
            }
            continue;
        }
        assert!(
            !released.workspace_changed(),
            "{gesture:?}: release must wait for presentation"
        );
        assert_eq!(session.version(), before);
        assert_eq!(durable(&session), initial_durable);
        assert!(!session.has_active_gesture());
        assert!(session.pointer_continuation().is_none());
        let [pending] = released.manipulation_feedback() else {
            panic!("{gesture:?}: exactly one pending release is required: {released:?}")
        };
        assert_eq!(
            pending.status(),
            DockspaceManipulationStatus::Pending(
                DockspaceManipulationPendingReason::AwaitingPresentation
            )
        );
        let pending_id = pending.manipulation_id();
        assert!(!settle(&mut session).workspace_changed());

        let mut frame = session.begin_host_frame().unwrap();
        let plan = frame.paint_plan(SURFACE).unwrap().unwrap();
        let (recording, receipt) = RecordingRenderer::execute(plan).unwrap();
        assert_eq!(geometry(&recording), geometry(&proposal), "{gesture:?}");
        frame.confirm_surface_painted(receipt).unwrap();
        let mut report = frame.commit().unwrap();
        assert!(!report.workspace_changed());
        let mut outputs = report.take_painted_outputs();
        assert_eq!(outputs.len(), 1);
        let output = outputs.pop().unwrap();
        assert!(
            matches!(recording.records.last(), Some(OwnedRecord::SurfaceEnd(semantic))
            if output.matches_semantic_output(*semantic))
        );
        let artifact = output.presentation_artifact();
        assert_eq!(session.version(), before);
        assert_eq!(durable(&session), initial_durable);
        session
            .report_surface_presentation(output, SurfacePresentationResult::Presented)
            .unwrap();
        assert_eq!(session.version(), before);
        assert_eq!(durable(&session), initial_durable);
        let settled = settle(&mut session);
        let [ack] = settled.surface_presentation_commits() else {
            panic!("{gesture:?}: the exact release output must settle once")
        };
        assert!(ack.settles(artifact));
        assert!(settled.workspace_changed());
        let [terminal] = settled.manipulation_feedback() else {
            panic!("{gesture:?}: exactly one terminal is required: {settled:?}")
        };
        assert_eq!(terminal.manipulation_id(), pending_id);
        assert_eq!(
            terminal.status(),
            DockspaceManipulationStatus::Delivered(match gesture {
                Gesture::Splitter => DockspaceManipulationDelivery::SplitterResize,
                Gesture::ContainedMove => DockspaceManipulationDelivery::Contained,
                Gesture::ContainedResize => DockspaceManipulationDelivery::ContainedTransform,
            })
        );
        let committed = durable(&session);
        assert_ne!(session.version(), before);
        assert_eq!(committed.1, expected_rect);
        match gesture {
            Gesture::Splitter => assert!(committed.0[0] < initial_durable.0[0]),
            _ => assert_eq!(committed.0, initial_durable.0),
        }
        assert!(!session.has_active_gesture());
        assert!(session.pointer_continuation().is_none());
        let version = session.version();
        for _ in 0..2 {
            let repeated = settle(&mut session);
            assert!(!repeated.workspace_changed());
            assert!(repeated.manipulation_feedback().is_empty());
            assert_eq!(session.version(), version);
            assert_eq!(durable(&session), committed);
        }
    }
}

#[test]
fn armed_touch_first_move_and_contact_end_batch_defers_source_activation_until_dock_presented() {
    let third = ItemId::new(3);
    let fourth = ItemId::new(4);
    let fifth = ItemId::new(5);
    let sixth = ItemId::new(6);
    let rear = FloatingPresentationId::new(1);
    let front = FloatingPresentationId::new(2);
    let rear_root = RootId::new(2);
    let rear_rect = LogicalRect::new(40.0, 160.0, 280.0, 220.0).unwrap();
    let front_rect = LogicalRect::new(120.0, 300.0, 250.0, 200.0).unwrap();
    let durable = |session: &DockspaceSession| {
        let items = [FIRST, SECOND, third, fourth, fifth, sixth].map(|item| {
            let item = session.view().item(item).unwrap();
            (
                item.root(),
                item.contained(),
                item.tabs().items().to_vec(),
                item.tabs().selected(),
            )
        });
        let contained = session
            .view()
            .surface(SURFACE)
            .unwrap()
            .contained()
            .map(|view| (view.id(), view.root().unwrap().id(), view.rect()))
            .collect::<Vec<_>>();
        let weights = session
            .view()
            .root(ROOT)
            .unwrap()
            .content()
            .unwrap()
            .split()
            .unwrap()
            .weights()
            .collect::<Vec<_>>();
        (items, contained, weights)
    };
    let geometry = |recording: &RecordingRenderer| {
        recording
            .records
            .iter()
            .filter_map(|record| match record {
                OwnedRecord::Geometry { visual, bounds, .. } => Some((*visual, *bounds)),
                _ => None,
            })
            .collect::<Vec<_>>()
    };

    for rear_title in [false, true] {
        let split = DockspaceNode::equal_split(
            DockspaceAxis::Horizontal,
            [
                DockspaceNode::tabs([FIRST, SECOND]),
                DockspaceNode::tabs([third]),
            ],
        )
        .unwrap();
        let layout = DockspaceLayout::new([DockspaceSurfaceLayout::new(
            SURFACE,
            DockspaceRootLayout::new(ROOT, split),
        )
        .with_contained(DockspaceContainedLayout::new(
            rear,
            DockspaceRootLayout::new(rear_root, DockspaceNode::tabs([fourth, fifth])),
            rear_rect,
        ))
        .with_contained(DockspaceContainedLayout::new(
            front,
            DockspaceRootLayout::new(RootId::new(3), DockspaceNode::tabs([sixth])),
            front_rect,
        ))])
        .unwrap();
        let mut session = DockspaceSession::from_layout(layout, DockPolicy::default()).unwrap();
        session.enable_host_pointer().unwrap();
        let mut frame = session.begin_host_frame().unwrap();
        measure(&mut frame);
        frame.commit().unwrap();
        let (initial, tab, focus_before) = paint(&mut session);
        let (source_descriptor, target) = {
            let mut inspect = session.begin_host_frame().unwrap();
            let plan = inspect.paint_plan(SURFACE).unwrap().unwrap();
            let mut source = tab.descriptor();
            if rear_title {
                plan.visit_surface_program(|step| {
                    if let Step::RootBegin { root, .. } = step
                        && root.root() == rear_root
                    {
                        source = plan
                            .receiver_for_contained_title(root.contained().unwrap())
                            .unwrap();
                    }
                    Ok::<(), ()>(())
                })
                .unwrap();
            }
            (
                source,
                plan.center_drop_receiver_for_item(third).unwrap().center(),
            )
        };
        let before = session.version();
        let initial_durable = durable(&session);
        assert!(!session.view().item(SECOND).unwrap().is_selected());
        assert_eq!(
            initial_durable
                .1
                .iter()
                .map(|(id, _, _)| *id)
                .collect::<Vec<_>>(),
            [rear, front]
        );
        let source = session.bind_presented_receiver(&source_descriptor).unwrap();
        let press = source.center();
        assert_eq!(
            session
                .host_pointer_hit(SURFACE, press)
                .unwrap()
                .drag_delivery(),
            Some(source),
            "the rear title or inactive tab must be exposed at the actual press point"
        );
        assert!(!rear_rect.contains(target));
        assert!(!front_rect.contains(target));
        let input = touch_input(&session, &source, PRESS, press, Some(0));
        let mut frame = session.begin_host_frame().unwrap();
        frame.submit_host_pointer(input).unwrap();
        assert!(commit(frame).inputs().is_empty());
        assert_eq!(session.version(), before);
        assert_eq!(durable(&session), initial_durable);
        let request = session.pointer_continuation().unwrap();
        assert_eq!(request.deadline(), Duration::from_millis(350));
        let (waiting, _, focus) = paint(&mut session);
        assert_eq!(focus, focus_before);
        assert_eq!(geometry(&waiting), geometry(&initial));
        let source = session.bind_presented_receiver(&source_descriptor).unwrap();
        let input = touch_input(&session, &source, request.event(), press, Some(350));
        let mut frame = session.begin_host_frame().unwrap();
        frame.submit_host_pointer(input).unwrap();
        let armed = commit(frame);
        assert_eq!(
            armed.inputs(),
            &[HostInputOutcome::Touch(DockspaceTouchEvent::Armed {
                pointer: POINTER,
                surface: SURFACE,
            })]
        );
        assert_eq!(session.version(), before);
        assert_eq!(durable(&session), initial_durable);
        assert!(session.has_active_gesture());
        assert!(session.pointer_continuation().is_none());
        let (armed_geometry, _, focus) = paint(&mut session);
        assert_eq!(focus, focus_before);
        assert_eq!(geometry(&armed_geometry), geometry(&initial));

        let surface = session.presented_surface(SURFACE).unwrap();
        let route = HostPointerRoute::new(surface, target);
        let mut frame = session.begin_host_frame().unwrap();
        frame
            .submit_host_pointer_batch([(SurfacePointerEvent::Moved, 360), (END, 361)].map(
                |(event, millis)| {
                    HostPointerInput::new(
                        POINTER,
                        event,
                        route,
                        route,
                        SurfacePointerCapture::ProviderEndpoint,
                        SurfacePointerReceiverFacts::unknown().with_clear_hover_lane(&surface),
                    )
                    .with_metadata(PointerEventMetadata::new(
                        PointerDeviceKind::Touch,
                        Duration::from_millis(millis),
                    ))
                },
            ))
            .unwrap();
        let released = commit(frame);
        assert!(
            released.inputs().is_empty(),
            "rear_title={rear_title}: {released:?}"
        );
        assert!(!released.workspace_changed());
        assert_eq!(session.version(), before);
        assert_eq!(
            durable(&session),
            initial_durable,
            "move and contact end must not eagerly select, raise, or change durable geometry"
        );
        assert!(!session.has_active_gesture());
        assert!(session.pointer_continuation().is_none());
        let [pending] = released.manipulation_feedback() else {
            panic!("rear_title={rear_title}: one docking latch is required: {released:?}")
        };
        assert_eq!(
            pending.status(),
            DockspaceManipulationStatus::Pending(
                DockspaceManipulationPendingReason::AwaitingPresentation
            )
        );
        let pending_id = pending.manipulation_id();

        let mut frame = session.begin_host_frame().unwrap();
        let plan = frame.paint_plan(SURFACE).unwrap().unwrap();
        assert_eq!(plan.pane_focus_request(), focus_before);
        let (recording, receipt) = RecordingRenderer::execute(plan).unwrap();
        assert!(
            recording.records.iter().any(|record| matches!(
                record,
                OwnedRecord::Preview(DockspacePreviewVisual::Dock { surface: SURFACE, rect })
                    if rect.contains(target)
            )),
            "rear_title={rear_title}: the unoccluded main pane must own the real target preview"
        );
        frame.confirm_surface_painted(receipt).unwrap();
        let mut report = frame.commit().unwrap();
        assert!(!report.workspace_changed());
        let mut outputs = report.take_painted_outputs();
        assert_eq!(outputs.len(), 1);
        let output = outputs.pop().unwrap();
        assert!(
            matches!(recording.records.last(), Some(OwnedRecord::SurfaceEnd(semantic))
                if output.matches_semantic_output(*semantic))
        );
        let artifact = output.presentation_artifact();
        assert_eq!(durable(&session), initial_durable);
        session
            .report_surface_presentation(output, SurfacePresentationResult::Presented)
            .unwrap();
        assert_eq!(session.version(), before);
        assert_eq!(durable(&session), initial_durable);
        let settled = settle(&mut session);
        let [ack] = settled.surface_presentation_commits() else {
            panic!("the exact burst-release output must settle once")
        };
        assert!(ack.settles(artifact));
        assert!(settled.workspace_changed());
        let [terminal] = settled.manipulation_feedback() else {
            panic!("rear_title={rear_title}: one delivered dock is required: {settled:?}")
        };
        assert_eq!(terminal.manipulation_id(), pending_id);
        assert_eq!(
            terminal.status(),
            DockspaceManipulationStatus::Delivered(DockspaceManipulationDelivery::Dock)
        );
        assert!(session.view().item(FIRST).unwrap().is_selected());
        assert_eq!(session.view().contained(front).unwrap().rect(), front_rect);
        if rear_title {
            assert_eq!(
                session.view().item(third).unwrap().tabs().items(),
                &[third, fourth, fifth]
            );
            for item in [fourth, fifth] {
                assert_eq!(session.view().item(item).unwrap().root(), ROOT);
                assert!(session.view().item(item).unwrap().contained().is_none());
            }
            assert!(session.view().contained(rear).is_none());
            assert_eq!(
                session.view().item(FIRST).unwrap().tabs().items(),
                &[FIRST, SECOND]
            );
            assert!(!session.view().item(SECOND).unwrap().is_selected());
        } else {
            assert_eq!(
                session.view().item(third).unwrap().tabs().items(),
                &[third, SECOND]
            );
            assert_eq!(session.view().item(FIRST).unwrap().tabs().items(), &[FIRST]);
            assert_eq!(session.view().item(SECOND).unwrap().root(), ROOT);
            assert!(session.view().item(SECOND).unwrap().contained().is_none());
            assert_eq!(session.view().contained(rear).unwrap().rect(), rear_rect);
        }
        let committed = durable(&session);
        let version = session.version();
        assert_ne!(version, before);
        assert!(!session.has_active_gesture());
        assert!(session.pointer_continuation().is_none());
        for _ in 0..2 {
            let repeated = settle(&mut session);
            assert!(!repeated.workspace_changed());
            assert!(repeated.manipulation_feedback().is_empty());
            assert_eq!(session.version(), version);
            assert_eq!(durable(&session), committed);
        }
    }
}
