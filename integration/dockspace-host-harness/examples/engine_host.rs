//! A logical engine host with deferred, owned recording output and no native windows.
//!
//! Run from the repository root:
//! cargo run --manifest-path integration/dockspace-host-harness/Cargo.toml --example engine_host

use std::collections::HashSet;

use dockspace::close::CloseDecision;
use dockspace::geometry::{LogicalRect, LogicalSize};
use dockspace::model::{
    DockspaceActionOutcome, DockspaceLayout, DockspaceNode, DockspaceRootLayout,
    DockspaceSurfaceLayout, ItemId, RootId, SurfaceId,
};
use dockspace::policy::DockPolicy;
use dockspace::runtime::{
    DockspaceCloseOutcome, DockspaceFocusTarget, DockspaceHostFrame, DockspaceManipulationFeedback,
    DockspaceManipulationFeedbackId, DockspaceManipulationStatus, DockspacePaneFocusObservation,
    DockspaceReceiverRole, DockspaceSession, HostFrameReport, HostInputOutcome, HostPointerInput,
    HostPointerRoute, SurfacePointerButton, SurfacePointerCancelReason, SurfacePointerCapture,
    SurfacePointerEvent, SurfacePointerId, SurfacePointerReceiverFacts, SurfacePresentationResult,
    SurfaceUnavailableReason, UniformSurfaceMetrics,
};
use dockspace_host_harness::{OwnedRecord, RecordingRenderer};

const A: SurfaceId = SurfaceId::new(1);
const B: SurfaceId = SurfaceId::new(2);
const ROOT_A: RootId = RootId::new(1);
const FIRST: ItemId = ItemId::new(1);
const SECOND: ItemId = ItemId::new(2);

fn measure(frame: &mut DockspaceHostFrame<'_>) {
    // Both render targets use local logical coordinates, independently of any window or DPI.
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

fn settle(session: &mut DockspaceSession) -> HostFrameReport {
    let mut frame = session.begin_host_frame().unwrap();
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    frame.commit().unwrap()
}

fn consume_feedback(
    seen: &mut HashSet<DockspaceManipulationFeedbackId>,
    feedback: &[DockspaceManipulationFeedback],
) {
    for feedback in feedback {
        if !seen.insert(feedback.id()) {
            continue;
        }
        // Status announcements are inert; they never request pane or OS focus.
        let status = match feedback.status() {
            DockspaceManipulationStatus::Pending(reason) => format!("pending: {reason:?}"),
            DockspaceManipulationStatus::Delivered(result) => format!("delivered: {result:?}"),
            DockspaceManipulationStatus::Cancelled(reason) => format!("cancelled: {reason:?}"),
            DockspaceManipulationStatus::Rejected(reason) => format!("rejected: {reason:?}"),
        };
        println!("manipulation {:?}: {status}", feedback.manipulation_id());
    }
}

fn main() {
    let layout = DockspaceLayout::new([
        DockspaceSurfaceLayout::new(
            A,
            DockspaceRootLayout::new(ROOT_A, DockspaceNode::tabs([FIRST, SECOND])),
        ),
        DockspaceSurfaceLayout::new(
            B,
            DockspaceRootLayout::new(
                RootId::new(2),
                DockspaceNode::tabs([ItemId::new(3), ItemId::new(4)]),
            ),
        ),
    ])
    .unwrap();
    let mut session = DockspaceSession::from_layout(layout, DockPolicy::default()).unwrap();
    let mut seen_feedback = HashSet::new();

    // Application commands use stable item identities without pointer registration.
    let select = session.prepare_select_item(SECOND);
    let mut frame = session.begin_host_frame().unwrap();
    let submitted = frame.submit_prepared_action(select).unwrap();
    measure(&mut frame);
    let selected = frame.commit().unwrap();
    match selected.submitted_action_outcome(submitted) {
        Some(HostInputOutcome::ProductActionApplied(DockspaceActionOutcome::Selected {
            item: SECOND,
            changed: true,
        })) => println!("selected item {SECOND:?}"),
        other => panic!("unexpected selection result: {other:?}"),
    }
    consume_feedback(&mut seen_feedback, selected.manipulation_feedback());

    let (recordings, mut painted) = {
        let mut frame = session.begin_host_frame().unwrap();
        let mut recordings = Vec::new();
        for surface in frame.surfaces() {
            let plan = frame.paint_plan(surface).unwrap().unwrap();
            let (recording, receipt) = RecordingRenderer::execute(plan).unwrap();
            frame.confirm_surface_painted(receipt).unwrap();
            recordings.push((surface, recording));
        }
        (recordings, frame.commit().unwrap())
    };

    // The frame borrow is over. An engine can enqueue these owned records unchanged.
    // This sink models output acceptance and disposition, not actual GPU presentation.
    let mut pending = Vec::new();
    for output in painted.take_painted_outputs() {
        let surface = output.surface();
        let (_, recording) = recordings.iter().find(|(id, _)| *id == surface).unwrap();
        assert!(
            matches!(recording.records.last(), Some(OwnedRecord::SurfaceEnd(semantic))
            if output.matches_semantic_output(*semantic))
        );
        for record in &recording.records {
            if let OwnedRecord::PaneContent { item, bounds, .. } = record {
                println!("owned content on {surface:?}: {item:?} at {bounds:?}");
            }
        }
        let receiver = recording
            .records
            .iter()
            .rev()
            .find_map(|record| match record {
                OwnedRecord::Receiver(receiver)
                    if receiver.role() == DockspaceReceiverRole::TabBody =>
                {
                    Some(*receiver)
                }
                _ => None,
            })
            .unwrap();
        let result = if surface == A {
            SurfacePresentationResult::Presented
        } else {
            SurfacePresentationResult::Dropped
        };
        let artifact = output.presentation_artifact();
        assert!(session.bind_presented_receiver(&receiver).is_none());
        session.report_surface_presentation(output, result).unwrap();
        assert!(session.bind_presented_receiver(&receiver).is_none());
        pending.push((surface, artifact, receiver, result));
    }
    assert_eq!(pending.len(), 2);
    consume_feedback(&mut seen_feedback, painted.manipulation_feedback());

    // Reporting a result only queues an observation. Commit settlement before routing input.
    let settled = settle(&mut session);
    assert_eq!(settled.surface_presentation_commits().len(), 2);
    let mut application_focus = None;
    for (surface, artifact, receiver, result) in pending {
        let commit = settled
            .surface_presentation_commits()
            .iter()
            .find(|commit| commit.settles(artifact))
            .unwrap();
        let presented = result == SurfacePresentationResult::Presented;
        assert_eq!(commit.presents(artifact), presented);
        assert_eq!(commit.retires(artifact), !presented);
        assert_eq!(
            session.bind_presented_receiver(&receiver).is_some(),
            presented
        );
        println!("settled {surface:?}: {result:?}");
        if presented {
            let mut frame = session.begin_host_frame().unwrap();
            let plan = frame.paint_plan(surface).unwrap().unwrap();
            if let Some(request) = plan.pane_focus_request() {
                // A real engine updates its own input-focus owner here. This
                // recording host models that state after the exact output settled.
                application_focus = Some(request.target());
                let observation = plan
                    .prepare_pane_focus_observation(request, DockspacePaneFocusObservation::Focused)
                    .unwrap();
                frame.submit_pane_focus_observation(observation).unwrap();
            }
            frame
                .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
                .unwrap();
            frame.commit().unwrap();
            cancel_logical_press(&mut session, receiver.center(), &mut seen_feedback);
        }
    }
    assert_eq!(application_focus, Some(DockspaceFocusTarget::Pane(SECOND)));
    consume_feedback(&mut seen_feedback, settled.manipulation_feedback());

    // A root close is a core-owned transaction requiring a decision for every item.
    let close = session.prepare_close_root(ROOT_A);
    let mut frame = session.begin_host_frame().unwrap();
    frame.submit_prepared_close_request(close).unwrap();
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    let requested = frame.commit().unwrap();
    let [HostInputOutcome::CloseRequested { plan, .. }] = requested.inputs() else {
        panic!("expected an application close plan: {requested:?}")
    };
    assert_eq!(plan.items().len(), 2);
    consume_feedback(&mut seen_feedback, requested.manipulation_feedback());

    let mut frame = session.begin_host_frame().unwrap();
    for item in plan.items() {
        // Real applications decide here after checking their own unsaved content.
        frame
            .resolve_close(plan.request(), item.token(), CloseDecision::Allow)
            .unwrap();
    }
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    let closed = frame.commit().unwrap();
    let mut applied = false;
    for input in closed.inputs() {
        if let HostInputOutcome::CloseDecisionProcessed {
            application: Some(Ok(DockspaceCloseOutcome::RootClosed { root, items })),
            changed: true,
            ..
        } = input
        {
            assert_eq!(*root, ROOT_A);
            assert_eq!(items, &[FIRST, SECOND]);
            applied = true;
            println!("closed root {root:?}: {items:?}");
        }
    }
    assert!(applied);
    assert!(session.view().item(FIRST).is_none());
    assert!(session.view().item(SECOND).is_none());
    consume_feedback(&mut seen_feedback, closed.manipulation_feedback());
}

fn cancel_logical_press(
    session: &mut DockspaceSession,
    point: dockspace::geometry::LogicalPoint,
    seen: &mut HashSet<DockspaceManipulationFeedbackId>,
) {
    // Feed a small logical input batch so the feedback consumer sees a real terminal.
    session.enable_host_pointer().unwrap();
    let hit = session.host_pointer_hit(A, point).unwrap();
    let surface = hit.surface();
    let delivery = hit.delivery().unwrap();
    let drag = hit.drag_delivery();
    let hover = hit.hover();
    let facts = SurfacePointerReceiverFacts::delivery(&delivery);
    let facts = match drag.as_ref() {
        Some(receiver) => facts.with_drag_delivery(receiver),
        None => facts.with_no_drag_delivery(&surface),
    };
    let facts = match hover.as_ref() {
        Some(receiver) => facts.with_hover(receiver),
        None => facts.with_no_hover(&surface),
    };
    let route = HostPointerRoute::new(surface, point);
    let mut frame = session.begin_host_frame().unwrap();
    frame
        .submit_host_pointer_batch(
            [
                SurfacePointerEvent::ButtonPressed(SurfacePointerButton::Primary),
                SurfacePointerEvent::StreamCancelled(SurfacePointerCancelReason::Escape),
            ]
            .into_iter()
            .map(|event| {
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
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    let report = frame.commit().unwrap();
    assert!(matches!(
        report.manipulation_feedback(),
        [feedback] if matches!(feedback.status(), DockspaceManipulationStatus::Cancelled(_))
    ));
    consume_feedback(seen, report.manipulation_feedback());
    // Re-reading the same report must not announce its terminal twice.
    let announced = seen.len();
    consume_feedback(seen, report.manipulation_feedback());
    assert_eq!(seen.len(), announced);
}
