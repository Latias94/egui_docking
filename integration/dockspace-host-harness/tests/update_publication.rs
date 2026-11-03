//! A public host measures, encodes, and publishes one private update at a time.

use std::num::NonZeroUsize;
use std::time::Duration;

use dockspace::close::CloseDecision;
use dockspace::geometry::{LogicalPoint, LogicalRect, LogicalSize};
use dockspace::model::{
    DockAnchor, DockEdge, DockFraction, DockPlacement, DockspaceActionOutcome, DockspaceLayout,
    DockspaceNode, DockspaceRootLayout, DockspaceSurfaceLayout, ItemId, RootId, SurfaceId,
};
use dockspace::policy::DockPolicy;
use dockspace::runtime::{
    DockspaceHostFrame, DockspaceManipulationDelivery, DockspaceManipulationStatus,
    DockspaceReceiverDescriptor, DockspaceRuntimeErrorKind, DockspaceSemanticOutput,
    DockspaceSession, DockspaceUpdate, HostFrameReport, HostInputOutcome, HostPointerInput,
    HostPointerRoute, PaintedSurfaceOutput, PointerDeviceKind, PointerEventMetadata,
    SurfacePaintPlan, SurfacePaintProgramOperation as Operation, SurfacePaintProgramStep as Step,
    SurfacePointerButton, SurfacePointerCapture, SurfacePointerEvent, SurfacePointerId,
    SurfacePointerReceiverFacts, SurfacePresentationResult, SurfaceUnavailableReason, SurfaceWork,
    SurfaceWorkDeferred, UniformSurfaceMetrics,
};
use dockspace_host_harness::{OwnedRecord, RecordingRenderer};

const A: SurfaceId = SurfaceId::new(1);
const B: SurfaceId = SurfaceId::new(2);
const ROOT: RootId = RootId::new(1);
const FIRST: ItemId = ItemId::new(1);
const SECOND: ItemId = ItemId::new(2);
const THIRD: ItemId = ItemId::new(3);

fn session() -> DockspaceSession {
    DockspaceSession::from_layout(
        DockspaceLayout::new([DockspaceSurfaceLayout::new(
            A,
            DockspaceRootLayout::new(ROOT, DockspaceNode::central_tabs([FIRST, SECOND])),
        )])
        .unwrap(),
        DockPolicy::default(),
    )
    .unwrap()
}

fn metrics(width: f64, tab_width: f64) -> UniformSurfaceMetrics {
    UniformSurfaceMetrics::new(
        LogicalRect::new(0.0, 0.0, width, 600.0).unwrap(),
        LogicalSize::new(64.0, 48.0).unwrap(),
        tab_width,
    )
    .unwrap()
}

fn ready<'frame>(
    frame: &'frame mut DockspaceHostFrame<'_>,
    surface: SurfaceId,
) -> SurfacePaintPlan<'frame> {
    match frame.surface_work(surface).unwrap() {
        SurfaceWork::Ready(plan) => plan,
        SurfaceWork::Deferred(reason) => panic!("{surface:?} unexpectedly deferred: {reason:?}"),
    }
}

fn encode(frame: &mut DockspaceHostFrame<'_>, surface: SurfaceId) -> RecordingRenderer {
    let (recording, receipt) = RecordingRenderer::execute(ready(frame, surface)).unwrap();
    frame.confirm_surface_painted(receipt).unwrap();
    recording
}

fn semantic(recording: &RecordingRenderer) -> DockspaceSemanticOutput {
    let Some(OwnedRecord::SurfaceEnd(output)) = recording.records.last() else {
        panic!("the recording must include its exact final semantic output");
    };
    *output
}

fn only_output(report: &mut HostFrameReport) -> PaintedSurfaceOutput {
    let [output] = report.take_painted_outputs().try_into().unwrap();
    output
}

fn settle(session: &mut DockspaceSession) -> HostFrameReport {
    let mut frame = session.begin_host_frame().unwrap();
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    frame.commit().unwrap()
}

fn measure_and_emit(
    session: &mut DockspaceSession,
    width: f64,
    tab_width: f64,
) -> (
    RecordingRenderer,
    DockspaceReceiverDescriptor,
    PaintedSurfaceOutput,
) {
    let mut frame = session.begin_host_frame().unwrap();
    frame.measure_surface(A, metrics(width, tab_width)).unwrap();
    let receiver = ready(&mut frame, A).tab_receiver(FIRST).unwrap();
    let recording = encode(&mut frame, A);
    let mut report = frame.commit().unwrap();
    let output = only_output(&mut report);
    assert!(output.matches_semantic_output(semantic(&recording)));
    (recording, receiver, output)
}

fn unknown_move(millis: u64) -> HostPointerInput<'static> {
    HostPointerInput::new(
        SurfacePointerId::new(1),
        SurfacePointerEvent::Moved,
        HostPointerRoute::Unknown,
        HostPointerRoute::Unknown,
        SurfacePointerCapture::Unknown,
        SurfacePointerReceiverFacts::unknown(),
    )
    .with_metadata(PointerEventMetadata::new(
        PointerDeviceKind::Mouse,
        Duration::from_millis(millis),
    ))
}

#[test]
fn owned_update_suspends_without_publication_and_resumes_the_same_candidate_once() {
    let mut session = session();
    let before = session.version();

    for drop_resumed in [false, true] {
        let update: DockspaceUpdate = session.begin_update().unwrap();
        assert_eq!(session.version(), before);
        let mut frame = session.resume_update(update).unwrap();
        frame.select_item_current(SECOND).unwrap();
        frame.measure_surface(A, metrics(1000.0, 112.0)).unwrap();
        encode(&mut frame, A);
        assert_ne!(frame.version(), before);
        let update = frame.suspend();
        assert_eq!(session.version(), before);
        assert!(session.view().item(FIRST).unwrap().is_selected());
        assert!(session.presented_surface(A).is_none());
        if drop_resumed {
            drop(session.resume_update(update).unwrap());
        } else {
            drop(update);
        }
        assert_eq!(session.version(), before);
        assert!(session.view().item(FIRST).unwrap().is_selected());
    }

    let update = session.begin_update().unwrap();
    let mut frame = session.resume_update(update).unwrap();
    frame.select_item_current(SECOND).unwrap();
    frame.measure_surface(A, metrics(1000.0, 112.0)).unwrap();
    let receiver = ready(&mut frame, A).tab_receiver(SECOND).unwrap();
    let recording = encode(&mut frame, A);
    let candidate_version = frame.version();
    let update = frame.suspend();
    assert_eq!(session.version(), before);
    assert!(session.view().item(FIRST).unwrap().is_selected());
    assert!(session.bind_presented_receiver(&receiver).is_none());

    let mut frame = session.resume_update(update).unwrap();
    assert_eq!(frame.version(), candidate_version);
    assert!(frame.view().item(SECOND).unwrap().is_selected());
    let (retained, _) = RecordingRenderer::execute(ready(&mut frame, A)).unwrap();
    assert_eq!(retained.records, recording.records);
    let update = frame.suspend();
    let frame = session.resume_update(update).unwrap();
    let mut report = frame.commit().unwrap();
    assert_eq!(report.before(), before);
    assert_eq!(report.after(), candidate_version);
    assert!(report.workspace_changed());
    assert!(matches!(
        report.inputs(),
        [HostInputOutcome::ProductActionApplied(
            DockspaceActionOutcome::Selected {
                item: SECOND,
                changed: true,
            }
        )]
    ));
    assert_eq!(report.surface_commits().len(), 1);
    let output = only_output(&mut report);
    assert!(output.matches_semantic_output(semantic(&recording)));
    assert_eq!(session.version(), candidate_version);
    session
        .report_surface_presentation(output, SurfacePresentationResult::Presented)
        .unwrap();
    assert_eq!(settle(&mut session).surface_presentation_commits().len(), 1);
    assert!(session.bind_presented_receiver(&receiver).is_some());
    let mut settled = settle(&mut session);
    assert!(!settled.workspace_changed());
    assert!(settled.inputs().is_empty());
    assert!(settled.take_painted_outputs().is_empty());
    assert!(settled.surface_presentation_commits().is_empty());
}

#[test]
fn owned_update_rejects_competing_foreign_and_stale_session_authority_without_locking() {
    let mut origin = session();
    let mut foreign = session();
    let before = origin.version();

    // A legacy borrow may end before its wrapper's lexical scope does.
    let legacy = origin.begin_host_frame().unwrap();
    assert_eq!(legacy.version(), before);
    let update = origin.begin_update().unwrap();
    assert_eq!(
        origin.begin_update().unwrap_err().kind(),
        DockspaceRuntimeErrorKind::UpdateInProgress
    );
    assert_eq!(
        origin.begin_host_frame().err().unwrap().kind(),
        DockspaceRuntimeErrorKind::UpdateInProgress
    );

    let foreign_update = foreign.begin_update().unwrap();
    assert_eq!(
        foreign.resume_update(update).err().unwrap().kind(),
        DockspaceRuntimeErrorKind::ForeignUpdate
    );
    assert_eq!(origin.version(), before);
    assert!(origin.view().item(FIRST).unwrap().is_selected());
    drop(origin.begin_update().unwrap());
    drop(origin.begin_host_frame().unwrap());
    assert_eq!(
        foreign.begin_update().unwrap_err().kind(),
        DockspaceRuntimeErrorKind::UpdateInProgress
    );
    drop(foreign.resume_update(foreign_update).unwrap());
    drop(foreign.begin_update().unwrap());

    for native in [false, true] {
        let mut stale = session();
        let before = stale.version();
        let update = stale.begin_update().unwrap();
        let mut frame = stale.resume_update(update).unwrap();
        frame.select_item_current(SECOND).unwrap();
        let update = frame.suspend();
        if native {
            stale
                .enable_managed_native_host(
                    dockspace::runtime::NativeCreateStrategy::Staged,
                    dockspace::runtime::NativePointerRoster::Exact(Vec::new()),
                )
                .unwrap();
        } else {
            stale.enable_host_pointer().unwrap();
        }
        assert_eq!(
            stale.resume_update(update).err().unwrap().kind(),
            DockspaceRuntimeErrorKind::ForeignUpdate
        );
        assert_eq!(stale.version(), before);
        assert!(stale.view().item(FIRST).unwrap().is_selected());
        drop(stale.begin_update().unwrap());
    }
}

#[test]
fn repeated_encoding_keeps_one_input_candidate_and_rejects_discarded_pass_receipts() {
    let mut session = session();
    session
        .configure_output_capacity(NonZeroUsize::MIN)
        .unwrap();
    let before = session.version();
    let mut frame = session.begin_host_frame().unwrap();
    frame.select_item_current(SECOND).unwrap();
    frame.begin_encoding_pass().unwrap();
    frame.measure_surface(A, metrics(800.0, 96.0)).unwrap();
    let (_, discarded_receipt) = RecordingRenderer::execute(ready(&mut frame, A)).unwrap();
    let update = frame.suspend();
    assert_eq!(session.version(), before);
    assert!(session.view().item(FIRST).unwrap().is_selected());

    let mut frame = session.resume_update(update).unwrap();
    frame.restart_encoding_pass().unwrap();
    // Identical scene geometry must not make an execution from a discarded pass valid.
    frame.measure_surface(A, metrics(800.0, 96.0)).unwrap();
    assert_eq!(
        frame
            .confirm_surface_painted(discarded_receipt)
            .unwrap_err()
            .kind(),
        DockspaceRuntimeErrorKind::HostProtocol
    );
    encode(&mut frame, A);
    frame.restart_encoding_pass().unwrap();
    frame.measure_surface(A, metrics(1000.0, 112.0)).unwrap();
    let receiver = ready(&mut frame, A).tab_receiver(SECOND).unwrap();
    let recording = encode(&mut frame, A);
    let mut report = frame.commit().unwrap();
    assert_eq!(report.before(), before);
    assert!(matches!(
        report.inputs(),
        [HostInputOutcome::ProductActionApplied(
            DockspaceActionOutcome::Selected {
                item: SECOND,
                changed: true
            }
        )]
    ));
    assert_eq!(report.surface_commits().len(), 1);
    let output = only_output(&mut report);
    assert!(output.matches_semantic_output(semantic(&recording)));
    session
        .report_surface_presentation(output, SurfacePresentationResult::Presented)
        .unwrap();
    assert_eq!(settle(&mut session).surface_presentation_commits().len(), 1);
    assert!(session.bind_presented_receiver(&receiver).is_some());
}

#[test]
fn discarded_encoding_or_update_cannot_lend_its_receiver_to_different_geometry() {
    for restart in [true, false] {
        let mut session = session();
        let before = session.version();
        let mut frame = session.begin_host_frame().unwrap();
        frame.begin_encoding_pass().unwrap();
        frame.measure_surface(A, metrics(800.0, 200.0)).unwrap();
        let discarded_receiver = ready(&mut frame, A).tab_receiver(SECOND).unwrap();
        let (discarded_recording, _) = RecordingRenderer::execute(ready(&mut frame, A)).unwrap();
        let update = frame.suspend();
        let mut frame = if restart {
            let mut frame = session.resume_update(update).unwrap();
            frame.restart_encoding_pass().unwrap();
            frame
        } else {
            drop(update);
            assert_eq!(session.version(), before);
            let mut frame = session.begin_host_frame().unwrap();
            frame.begin_encoding_pass().unwrap();
            frame
        };
        frame.measure_surface(A, metrics(1000.0, 80.0)).unwrap();
        let receiver = ready(&mut frame, A).tab_receiver(SECOND).unwrap();
        assert_ne!(discarded_receiver.bounds(), receiver.bounds());
        let recording = encode(&mut frame, A);
        let mut report = frame.commit().unwrap();
        let output = only_output(&mut report);
        assert!(output.matches_semantic_output(semantic(&recording)));
        assert!(
            !output.matches_semantic_output(semantic(&discarded_recording)),
            "discarded geometry cannot reuse the final output identity (restart={restart})"
        );
        session
            .report_surface_presentation(output, SurfacePresentationResult::Presented)
            .unwrap();
        settle(&mut session);
        assert!(session.bind_presented_receiver(&receiver).is_some());
        assert!(
            session
                .bind_presented_receiver(&discarded_receiver)
                .is_none()
        );
    }
}

#[test]
fn first_measurement_is_ready_and_emitted_in_the_same_update() {
    let mut session = session();
    let before = session.version();
    let mut frame = session.begin_host_frame().unwrap();
    assert!(matches!(
        frame.surface_work(A).unwrap(),
        SurfaceWork::Deferred(SurfaceWorkDeferred::Unmeasured)
    ));
    frame.measure_surface(A, metrics(800.0, 96.0)).unwrap();
    let receiver = ready(&mut frame, A).tab_receiver(FIRST).unwrap();
    let recording = encode(&mut frame, A);
    let mut report = frame.commit().unwrap();
    let output = only_output(&mut report);
    assert!(output.matches_semantic_output(semantic(&recording)));
    assert_eq!(report.before(), before);
    assert_eq!(report.after(), before);
    assert!(!report.workspace_changed());
    assert_eq!(report.surface_commits().len(), 1);
    assert!(session.presented_surface(A).is_none());
    assert!(session.bind_presented_receiver(&receiver).is_none());

    let artifact = output.presentation_artifact();
    session
        .report_surface_presentation(output, SurfacePresentationResult::Presented)
        .unwrap();
    let settled = settle(&mut session);
    let [commit] = settled.surface_presentation_commits() else {
        panic!("the single-update output must settle once");
    };
    assert!(commit.presents(artifact));
    assert!(session.bind_presented_receiver(&receiver).is_some());
}

#[test]
fn selection_or_topology_and_resize_are_encoded_before_their_single_publication() {
    for split in [false, true] {
        let mut session = session();
        let (_, _, first) = measure_and_emit(&mut session, 800.0, 96.0);
        session
            .report_surface_presentation(first, SurfacePresentationResult::Presented)
            .unwrap();
        settle(&mut session);
        let before = session.version();

        let mut frame = session.begin_host_frame().unwrap();
        if split {
            frame
                .open_item_current(
                    THIRD,
                    DockPlacement::InnerEdge {
                        anchor: DockAnchor::Item(FIRST),
                        edge: DockEdge::Right,
                        fraction: DockFraction::new(0.4).unwrap(),
                    },
                )
                .unwrap();
        } else {
            frame.select_item_current(SECOND).unwrap();
        }
        frame.measure_surface(A, metrics(1000.0, 112.0)).unwrap();
        let receiver = ready(&mut frame, A)
            .tab_receiver(if split { THIRD } else { SECOND })
            .unwrap();
        let recording = encode(&mut frame, A);
        let selected = recording
            .records
            .iter()
            .filter_map(|record| match record {
                OwnedRecord::PaneContent { item, .. } => Some(*item),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(selected.len(), if split { 2 } else { 1 });
        assert!(selected.contains(&Some(if split { THIRD } else { SECOND })));
        let pane_right = recording
            .records
            .iter()
            .filter_map(|record| match record {
                OwnedRecord::PaneContent { bounds, .. } => Some(bounds.x() + bounds.width()),
                _ => None,
            })
            .fold(0.0_f64, f64::max);
        assert_eq!(pane_right, 1000.0);
        let candidate_version = frame.version();
        let mut report = frame.commit().unwrap();
        assert_eq!(report.before(), before);
        assert_eq!(report.after(), candidate_version);
        assert_ne!(report.after(), before);
        assert!(report.workspace_changed());
        assert_eq!(report.inputs().len(), 1);
        assert_eq!(report.surface_commits().len(), 1);
        let output = only_output(&mut report);
        assert!(output.matches_semantic_output(semantic(&recording)));
        assert_eq!(session.version(), candidate_version);
        assert!(session.bind_presented_receiver(&receiver).is_none());

        session
            .report_surface_presentation(output, SurfacePresentationResult::Presented)
            .unwrap();
        assert!(!settle(&mut session).workspace_changed());
        assert!(session.bind_presented_receiver(&receiver).is_some());
        assert_eq!(session.version(), candidate_version);
    }
}

#[test]
fn abandoned_and_failed_encoded_updates_preserve_scene_version_and_producer_watermark() {
    for fail in [false, true] {
        let mut session = session();
        session.enable_host_pointer().unwrap();
        session
            .configure_output_capacity(NonZeroUsize::new(1).unwrap())
            .unwrap();
        let (original, receiver, first) = measure_and_emit(&mut session, 800.0, 96.0);
        session
            .report_surface_presentation(first, SurfacePresentationResult::Presented)
            .unwrap();
        settle(&mut session);
        let before = session.version();
        let surface = session.presented_surface(A).unwrap();
        let bound = session.bind_presented_receiver(&receiver).unwrap();

        let mut candidate = session.begin_host_frame().unwrap();
        candidate.submit_host_pointer(unknown_move(30)).unwrap();
        candidate.select_item_current(SECOND).unwrap();
        candidate
            .measure_surface(A, metrics(1000.0, 112.0))
            .unwrap();
        let abandoned = encode(&mut candidate, A);
        assert_ne!(semantic(&abandoned), semantic(&original));
        assert_ne!(candidate.version(), before);
        assert!(candidate.view().item(SECOND).unwrap().is_selected());
        if fail {
            assert!(candidate.select_item_current(FIRST).is_err());
            assert!(
                candidate
                    .defer_surface(A, SurfaceUnavailableReason::SurfaceUnavailable)
                    .is_err()
            );
            assert!(candidate.commit().is_err());
        } else {
            drop(candidate);
        }

        assert_eq!(session.version(), before);
        assert!(session.view().item(FIRST).unwrap().is_selected());
        assert_eq!(session.presented_surface(A), Some(surface));
        assert_eq!(session.bind_presented_receiver(&receiver), Some(bound));
        assert!(!session.has_active_gesture());
        let mut inspect = session.begin_host_frame().unwrap();
        let (retained, _) = RecordingRenderer::execute(ready(&mut inspect, A)).unwrap();
        assert_eq!(retained.records, original.records);
        drop(inspect);

        // Public event-time validation also proves the rejected producer prefix
        // did not advance: an event earlier than the discarded 30ms still enters.
        let mut retry = session.begin_host_frame().unwrap();
        retry.submit_host_pointer(unknown_move(20)).unwrap();
        retry.select_item_current(SECOND).unwrap();
        retry.measure_surface(A, metrics(1000.0, 112.0)).unwrap();
        let recording = encode(&mut retry, A);
        let mut report = retry.commit().unwrap();
        assert!(report.workspace_changed());
        assert!(matches!(
            report.inputs(),
            [HostInputOutcome::ProductActionApplied(
                DockspaceActionOutcome::Selected {
                    item: SECOND,
                    changed: true,
                }
            )]
        ));
        assert!(only_output(&mut report).matches_semantic_output(semantic(&recording)));
        assert!(session.view().item(SECOND).unwrap().is_selected());
        let mut old_time = session.begin_host_frame().unwrap();
        assert!(old_time.submit_host_pointer(unknown_move(19)).is_err());
    }
}

#[test]
fn full_output_capacity_defers_encoding_but_allows_zero_output_selection_and_close() {
    let mut session = session();
    session
        .configure_output_capacity(NonZeroUsize::new(1).unwrap())
        .unwrap();
    let (_, _, inflight) = measure_and_emit(&mut session, 800.0, 96.0);

    let mut frame = session.begin_host_frame().unwrap();
    frame.select_item_current(SECOND).unwrap();
    frame.measure_surface(A, metrics(800.0, 96.0)).unwrap();
    assert!(matches!(
        frame.surface_work(A).unwrap(),
        SurfaceWork::Deferred(SurfaceWorkDeferred::OutputCapacity)
    ));
    assert!(frame.paint_plan(A).unwrap().is_none());
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    let mut selected = frame.commit().unwrap();
    assert!(selected.workspace_changed());
    assert!(selected.take_painted_outputs().is_empty());
    assert!(session.view().item(SECOND).unwrap().is_selected());

    let mut close = session.begin_host_frame().unwrap();
    close.request_close_item_current(FIRST).unwrap();
    close
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    let mut requested = close.commit().unwrap();
    assert!(requested.take_painted_outputs().is_empty());
    let [HostInputOutcome::CloseRequested { plan, .. }] = requested.inputs() else {
        panic!("output backpressure must not prevent an application close request");
    };
    let mut decision = session.begin_host_frame().unwrap();
    decision
        .resolve_close(
            plan.request(),
            plan.items()[0].token(),
            CloseDecision::Allow,
        )
        .unwrap();
    decision
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    let mut closed = decision.commit().unwrap();
    assert!(closed.workspace_changed());
    assert!(closed.take_painted_outputs().is_empty());
    assert!(session.view().item(FIRST).is_none());
    assert!(session.view().item(SECOND).unwrap().is_selected());
    drop(inflight);
}

#[test]
fn one_slot_is_reclaimed_by_reporting_or_dropping_inflight_output_in_the_next_update() {
    for disposition in [
        Some(SurfacePresentationResult::Presented),
        Some(SurfacePresentationResult::Dropped),
        None,
    ] {
        let mut session = session();
        session
            .configure_output_capacity(NonZeroUsize::new(1).unwrap())
            .unwrap();
        let (_, receiver, first) = measure_and_emit(&mut session, 800.0, 96.0);
        let first_artifact = first.presentation_artifact();
        match disposition {
            Some(result) => session.report_surface_presentation(first, result).unwrap(),
            None => drop(first),
        }

        let mut abandoned = session.begin_host_frame().unwrap();
        encode(&mut abandoned, A);
        drop(abandoned);
        assert!(session.presented_surface(A).is_none());
        assert!(session.bind_presented_receiver(&receiver).is_none());

        let mut replacement = session.begin_host_frame().unwrap();
        let recording = encode(&mut replacement, A);
        let mut report = replacement.commit().unwrap();
        let [commit] = report.surface_presentation_commits() else {
            panic!("the previous terminal and the successor must publish together");
        };
        assert!(commit.settles(first_artifact));
        assert_eq!(
            commit.presents(first_artifact),
            disposition == Some(SurfacePresentationResult::Presented)
        );
        assert_eq!(
            session.bind_presented_receiver(&receiver).is_some(),
            disposition == Some(SurfacePresentationResult::Presented)
        );
        let output = only_output(&mut report);
        assert_ne!(output.presentation_artifact(), first_artifact);
        assert!(output.matches_semantic_output(semantic(&recording)));

        let mut full = session.begin_host_frame().unwrap();
        assert!(matches!(
            full.surface_work(A).unwrap(),
            SurfaceWork::Deferred(SurfaceWorkDeferred::OutputCapacity)
        ));
        drop(full);
        drop(output);
    }
}

#[test]
fn ordinary_splitter_terminal_commits_while_an_output_occupies_the_only_slot() {
    let mut session = session();
    session.enable_host_pointer().unwrap();
    session
        .configure_output_capacity(NonZeroUsize::new(1).unwrap())
        .unwrap();
    let mut initial = session.begin_host_frame().unwrap();
    initial
        .dock_item_current(
            SECOND,
            DockPlacement::InnerEdge {
                anchor: DockAnchor::Item(FIRST),
                edge: DockEdge::Right,
                fraction: DockFraction::new(0.5).unwrap(),
            },
        )
        .unwrap();
    initial.measure_surface(A, metrics(800.0, 96.0)).unwrap();
    let plan = ready(&mut initial, A);
    let mut splitter = None;
    plan.visit_surface_program(|step| {
        if let Step::Operation {
            operation: Operation::Splitter(record),
            ..
        } = step
        {
            assert!(
                splitter
                    .replace(plan.receiver_for_splitter(record).unwrap())
                    .is_none()
            );
        }
        Ok::<(), ()>(())
    })
    .unwrap();
    encode(&mut initial, A);
    let first = only_output(&mut initial.commit().unwrap());
    session
        .report_surface_presentation(first, SurfacePresentationResult::Presented)
        .unwrap();
    settle(&mut session);
    let source = session.bind_presented_receiver(&splitter.unwrap()).unwrap();
    let surface = session.presented_surface(A).unwrap();

    let mut painted = session.begin_host_frame().unwrap();
    encode(&mut painted, A);
    let inflight = only_output(&mut painted.commit().unwrap());
    let point = source.center();
    let moved = LogicalPoint::new(point.x() + 30.0, point.y()).unwrap();
    let facts = SurfacePointerReceiverFacts::delivery(&source)
        .with_drag_delivery(&source)
        .with_clear_hover_lane(&surface);
    let mut release = session.begin_host_frame().unwrap();
    release
        .submit_host_pointer_batch(
            [
                (
                    SurfacePointerEvent::ButtonPressed(SurfacePointerButton::Primary),
                    point,
                ),
                (SurfacePointerEvent::Moved, moved),
            ]
            .map(|(event, point)| {
                let route = HostPointerRoute::new(surface, point);
                HostPointerInput::new(
                    SurfacePointerId::new(1),
                    event,
                    route,
                    route,
                    SurfacePointerCapture::ProviderEndpoint,
                    facts,
                )
            }),
        )
        .unwrap();
    let source_only = release
        .host_pointer_manipulation_surface(SurfacePointerId::new(1))
        .unwrap();
    assert_eq!(source_only, surface);
    assert!(
        release
            .host_pointer_manipulation_surface(SurfacePointerId::new(2))
            .is_none()
    );
    release
        .submit_host_pointer(HostPointerInput::new(
            SurfacePointerId::new(1),
            SurfacePointerEvent::ButtonReleased(SurfacePointerButton::Primary),
            HostPointerRoute::new(source_only, moved),
            HostPointerRoute::Unknown,
            SurfacePointerCapture::ProviderEndpoint,
            SurfacePointerReceiverFacts::unknown(),
        ))
        .unwrap();
    assert!(
        release
            .host_pointer_manipulation_surface(SurfacePointerId::new(1))
            .is_none()
    );
    release.measure_surface(A, metrics(800.0, 96.0)).unwrap();
    assert!(matches!(
        release.surface_work(A).unwrap(),
        SurfaceWork::Deferred(SurfaceWorkDeferred::OutputCapacity)
    ));
    release
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    let mut report = release.commit().unwrap();
    assert!(report.workspace_changed());
    assert!(report.take_painted_outputs().is_empty());
    let [feedback] = report.manipulation_feedback() else {
        panic!("the released splitter must produce one terminal without painting");
    };
    assert_eq!(
        feedback.status(),
        DockspaceManipulationStatus::Delivered(DockspaceManipulationDelivery::SplitterResize)
    );
    assert!(!session.has_active_gesture());
    assert!(
        session
            .view()
            .root(ROOT)
            .unwrap()
            .content()
            .unwrap()
            .split()
            .unwrap()
            .weights()
            .next()
            .unwrap()
            > 0.5
    );
    drop(inflight);
    let terminal = settle(&mut session);
    assert!(!terminal.workspace_changed());
    assert!(terminal.manipulation_feedback().is_empty());
}

#[test]
fn three_single_update_measurements_keep_exact_scenes_through_reverse_results() {
    for last_result in [
        SurfacePresentationResult::Presented,
        SurfacePresentationResult::Dropped,
    ] {
        let mut session = session();
        let (_, a_receiver, first) = measure_and_emit(&mut session, 800.0, 96.0);
        let (_, b_receiver, second) = measure_and_emit(&mut session, 800.0, 104.0);
        let (_, c_receiver, third) = measure_and_emit(&mut session, 800.0, 112.0);
        assert_ne!(a_receiver, b_receiver);
        assert_ne!(b_receiver, c_receiver);
        let a_artifact = first.presentation_artifact();
        let b_artifact = second.presentation_artifact();
        let c_artifact = third.presentation_artifact();
        session
            .report_surface_presentation(third, last_result)
            .unwrap();
        assert!(
            settle(&mut session)
                .surface_presentation_commits()
                .is_empty()
        );
        session
            .report_surface_presentation(second, SurfacePresentationResult::Presented)
            .unwrap();
        assert!(
            settle(&mut session)
                .surface_presentation_commits()
                .is_empty()
        );
        assert!(session.presented_surface(A).is_none());

        session
            .report_surface_presentation(first, SurfacePresentationResult::Presented)
            .unwrap();
        let report = settle(&mut session);
        let [commit] = report.surface_presentation_commits() else {
            panic!("only the complete reverse-reported prefix may settle");
        };
        let (artifact, receiver, retired_receiver) =
            if last_result == SurfacePresentationResult::Presented {
                (c_artifact, c_receiver, b_receiver)
            } else {
                (b_artifact, b_receiver, c_receiver)
            };
        assert!(commit.presents(artifact));
        assert!(commit.retires(a_artifact));
        assert!(commit.settles(b_artifact));
        assert!(commit.settles(c_artifact));
        assert!(
            session
                .presented_surface(A)
                .unwrap()
                .matches_presentation_artifact(artifact)
        );
        assert!(session.bind_presented_receiver(&receiver).is_some());
        assert!(session.bind_presented_receiver(&a_receiver).is_none());
        assert!(session.bind_presented_receiver(&retired_receiver).is_none());
        assert!(
            settle(&mut session)
                .surface_presentation_commits()
                .is_empty()
        );
        assert!(session.bind_presented_receiver(&receiver).is_some());
    }
}

#[test]
fn measuring_b_after_sealing_a_preserves_the_exact_encoded_scene_for_a() {
    let mut session = DockspaceSession::from_layout(
        DockspaceLayout::new([
            DockspaceSurfaceLayout::new(
                A,
                DockspaceRootLayout::new(ROOT, DockspaceNode::central_tabs([FIRST, SECOND])),
            ),
            DockspaceSurfaceLayout::new(
                B,
                DockspaceRootLayout::new(RootId::new(2), DockspaceNode::central_tabs([THIRD])),
            ),
        ])
        .unwrap(),
        DockPolicy::default(),
    )
    .unwrap();
    let mut frame = session.begin_host_frame().unwrap();
    frame.measure_surface(A, metrics(800.0, 96.0)).unwrap();
    let a_receiver = ready(&mut frame, A).tab_receiver(FIRST).unwrap();
    let a = encode(&mut frame, A);
    frame.measure_surface(B, metrics(500.0, 128.0)).unwrap();
    let b_receiver = ready(&mut frame, B).tab_receiver(THIRD).unwrap();
    let b = encode(&mut frame, B);
    let (a_after_b, _) = RecordingRenderer::execute(ready(&mut frame, A)).unwrap();
    assert_eq!(a_after_b.records, a.records);
    let mut report = frame.commit().unwrap();
    assert_eq!(report.surface_commits().len(), 2);
    let outputs = report.take_painted_outputs();
    assert_eq!(outputs.len(), 2);
    for output in outputs {
        let recording = if output.surface() == A { &a } else { &b };
        assert!(output.matches_semantic_output(semantic(recording)));
        session
            .report_surface_presentation(output, SurfacePresentationResult::Presented)
            .unwrap();
    }
    assert_eq!(settle(&mut session).surface_presentation_commits().len(), 2);
    assert!(session.bind_presented_receiver(&a_receiver).is_some());
    assert!(session.bind_presented_receiver(&b_receiver).is_some());
}
