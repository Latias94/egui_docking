use dockspace::geometry::{LogicalPoint, LogicalRect, LogicalSize};
use dockspace::model::{
    DockspaceAxis, DockspaceContainedLayout, DockspaceLayout, DockspaceNode, DockspaceRootLayout,
    DockspaceSurfaceLayout, FloatingPresentationId, ItemId, RootId, SurfaceId, WorkspaceVersion,
};
use dockspace::policy::{CloseCapability, DockPolicy};
use dockspace::runtime::{
    ContainedResizeDirection, DockspaceManipulationDelivery as Delivery,
    DockspaceManipulationFeedback as Feedback, DockspaceManipulationRejection as Rejection,
    DockspaceManipulationStatus as Status, DockspaceRuntimeErrorKind, DockspaceSession,
    HostFrameReport, HostPointerInput, HostPointerRoute, PaintedSurfaceOutput,
    SurfacePaintProgramStep as Step, SurfacePointerButton as Button, SurfacePointerCapture,
    SurfacePointerEvent as Event, SurfacePointerId, SurfacePointerReceiverFacts,
    SurfacePresentationResult, SurfaceUnavailableReason, UniformSurfaceMetrics,
};
use dockspace_host_harness::RecordingRenderer;

const SURFACE: SurfaceId = SurfaceId::new(1);
const FIRST: ItemId = ItemId::new(1);
const SECOND: ItemId = ItemId::new(2);

#[derive(Clone, Copy)]
enum Gesture {
    Dock,
    ContainedResize,
}

fn rect(x: f64, y: f64, width: f64, height: f64) -> LogicalRect {
    LogicalRect::new(x, y, width, height).unwrap()
}

fn settle(session: &mut DockspaceSession) -> HostFrameReport {
    let mut frame = session.begin_host_frame().unwrap();
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    frame.commit().unwrap()
}

fn metrics() -> UniformSurfaceMetrics {
    UniformSurfaceMetrics::new(
        rect(0.0, 0.0, 960.0, 720.0),
        LogicalSize::new(64.0, 48.0).unwrap(),
        96.0,
    )
    .unwrap()
}

fn paint(
    session: &mut DockspaceSession,
) -> (PaintedSurfaceOutput, Option<LogicalPoint>, LogicalPoint) {
    let mut frame = session.begin_host_frame().unwrap();
    if frame.paint_plan(SURFACE).unwrap().is_none() {
        frame.measure_surface(SURFACE, metrics()).unwrap();
    }
    let plan = frame.paint_plan(SURFACE).unwrap().unwrap();
    let tab = plan.tab_receiver(SECOND).map(|receiver| receiver.center());
    let mut resize = None;
    plan.visit_surface_program(|step| {
        if let Step::RootBegin { root, .. } = step
            && let Some(contained) = root.contained()
        {
            let handle = contained
                .resize()
                .find(|handle| handle.direction() == ContainedResizeDirection::SouthEast)
                .unwrap();
            resize = Some(
                plan.receiver_for_contained_resize(contained, handle)
                    .unwrap()
                    .center(),
            );
        }
        Ok::<(), ()>(())
    })
    .unwrap();
    let (_, receipt) = RecordingRenderer::execute(plan).unwrap();
    frame.confirm_surface_painted(receipt).unwrap();
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    let mut report = frame.commit().unwrap();
    assert!(report.manipulation_feedback().is_empty());
    let output = report.take_painted_outputs().pop().unwrap();
    (output, tab, resize.unwrap())
}

fn layout() -> DockspaceLayout {
    let split = DockspaceNode::equal_split(
        DockspaceAxis::Horizontal,
        [DockspaceNode::tabs([FIRST]), DockspaceNode::tabs([SECOND])],
    )
    .unwrap();
    DockspaceLayout::new([DockspaceSurfaceLayout::new(
        SURFACE,
        DockspaceRootLayout::new(RootId::new(1), split),
    )
    .with_contained(DockspaceContainedLayout::new(
        FloatingPresentationId::new(1),
        DockspaceRootLayout::new(RootId::new(2), DockspaceNode::tabs([ItemId::new(3)])),
        rect(520.0, 160.0, 300.0, 260.0),
    ))])
    .unwrap()
}

fn fixture() -> (DockspaceSession, LogicalPoint, LogicalPoint) {
    ready_fixture(DockspaceSession::from_layout(layout(), DockPolicy::default()).unwrap())
}

fn ready_fixture(mut session: DockspaceSession) -> (DockspaceSession, LogicalPoint, LogicalPoint) {
    session.enable_host_pointer().unwrap();
    measure(&mut session);
    let (output, tab, resize) = paint(&mut session);
    session
        .report_surface_presentation(output, SurfacePresentationResult::Presented)
        .unwrap();
    assert!(settle(&mut session).manipulation_feedback().is_empty());
    (session, tab.unwrap(), resize)
}

fn measure(session: &mut DockspaceSession) {
    let mut frame = session.begin_host_frame().unwrap();
    frame
        .measure_surface(
            SURFACE,
            UniformSurfaceMetrics::new(
                rect(0.0, 0.0, 960.0, 720.0),
                LogicalSize::new(64.0, 48.0).unwrap(),
                96.0,
            )
            .unwrap(),
        )
        .unwrap();
    assert!(frame.commit().unwrap().manipulation_feedback().is_empty());
}

fn edge(
    session: &mut DockspaceSession,
    event: Event,
    point: LogicalPoint,
    capture: SurfacePointerCapture,
) -> Result<HostFrameReport, dockspace::runtime::DockspaceRuntimeError> {
    let hit = session.host_pointer_hit(SURFACE, point).unwrap();
    let surface = hit.surface();
    let click = hit.delivery();
    let drag = hit.drag_delivery();
    let hover = hit.hover();
    let facts = match click.as_ref() {
        Some(receiver) => SurfacePointerReceiverFacts::delivery(receiver),
        None => SurfacePointerReceiverFacts::unknown().with_no_delivery(&surface),
    };
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
    frame.submit_host_pointer(HostPointerInput::new(
        SurfacePointerId::new(1),
        event,
        route,
        route,
        capture,
        facts,
    ))?;
    frame.complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)?;
    frame.commit()
}

fn release(
    session: &mut DockspaceSession,
    gesture: Gesture,
    tab: LogicalPoint,
    resize: LogicalPoint,
) -> (Feedback, LogicalPoint) {
    let (press, target) = match gesture {
        Gesture::Dock => (tab, LogicalPoint::new(200.0, 340.0).unwrap()),
        Gesture::ContainedResize => (
            resize,
            LogicalPoint::new(resize.x() + 36.0, resize.y() + 24.0).unwrap(),
        ),
    };
    let press = edge(
        session,
        Event::ButtonPressed(Button::Primary),
        press,
        SurfacePointerCapture::ProviderEndpoint,
    )
    .unwrap();
    assert!(press.manipulation_feedback().is_empty());
    let motion = edge(
        session,
        Event::Moved,
        target,
        SurfacePointerCapture::ProviderEndpoint,
    )
    .unwrap();
    assert!(motion.manipulation_feedback().is_empty());
    let before = session.version();
    let released = edge(
        session,
        Event::ButtonReleased(Button::Primary),
        target,
        SurfacePointerCapture::ProviderEndpoint,
    )
    .unwrap();
    let [feedback] = released.manipulation_feedback() else {
        panic!("one consumed release must publish exactly one terminal: {released:?}");
    };
    assert_eq!(
        feedback.status(),
        Status::Delivered(match gesture {
            Gesture::Dock => Delivery::Dock,
            Gesture::ContainedResize => Delivery::ContainedTransform,
        })
    );
    assert!(released.workspace_changed());
    assert_eq!(released.before(), before);
    assert_eq!(released.after(), session.version());
    assert_ne!(session.version(), before);
    assert!(!session.has_active_gesture());
    assert!(!session.has_pending_presentation());
    match gesture {
        Gesture::Dock => assert_eq!(
            session.view().item(SECOND).unwrap().tabs().items(),
            &[FIRST, SECOND]
        ),
        Gesture::ContainedResize => assert_eq!(
            session
                .view()
                .contained(FloatingPresentationId::new(1))
                .unwrap()
                .rect(),
            rect(520.0, 160.0, 336.0, 284.0)
        ),
    }
    (*feedback, target)
}

fn unrouted_edge(session: &mut DockspaceSession, event: Event) -> HostFrameReport {
    let mut frame = session.begin_host_frame().unwrap();
    frame
        .submit_host_pointer(HostPointerInput::new(
            SurfacePointerId::new(1),
            event,
            HostPointerRoute::Unknown,
            HostPointerRoute::Unknown,
            SurfacePointerCapture::ProviderEndpoint,
            SurfacePointerReceiverFacts::unknown(),
        ))
        .unwrap();
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    frame.commit().unwrap()
}

fn split_weights(session: &DockspaceSession) -> Vec<f32> {
    session
        .view()
        .surface(SURFACE)
        .and_then(|surface| surface.main_root())
        .and_then(|root| root.content())
        .and_then(|content| content.split())
        .unwrap()
        .weights()
        .collect()
}

#[test]
fn splitter_release_delivers_or_rejects_once_and_new_gestures_keep_distinct_identities() {
    let (mut session, _, _) = fixture();
    let mut terminals: Vec<Feedback> = Vec::new();
    for missing_route in [false, true, false] {
        let splitter = {
            let mut frame = session.begin_host_frame().unwrap();
            let plan = frame.paint_plan(SURFACE).unwrap().unwrap();
            let mut receiver = None;
            plan.visit_surface_program(|step| {
                if let Step::Operation {
                    operation: dockspace::runtime::SurfacePaintProgramOperation::Splitter(splitter),
                    ..
                } = step
                {
                    receiver = Some(plan.receiver_for_splitter(splitter).unwrap().center());
                }
                Ok::<(), ()>(())
            })
            .unwrap();
            receiver.unwrap()
        };
        let point = LogicalPoint::new(splitter.x() - 36.0, splitter.y()).unwrap();
        let before_version = session.version();
        let before_weights = split_weights(&session);
        let pressed = edge(
            &mut session,
            Event::ButtonPressed(Button::Primary),
            splitter,
            SurfacePointerCapture::ProviderEndpoint,
        )
        .unwrap();
        assert!(pressed.manipulation_feedback().is_empty());
        assert!(session.has_active_gesture());

        let motion = if missing_route {
            unrouted_edge(&mut session, Event::Moved)
        } else {
            edge(
                &mut session,
                Event::Moved,
                point,
                SurfacePointerCapture::ProviderEndpoint,
            )
            .unwrap()
        };
        assert!(motion.manipulation_feedback().is_empty());
        assert!(session.has_active_gesture());
        assert_eq!(session.version(), before_version);
        assert_eq!(split_weights(&session), before_weights);

        if !missing_route {
            measure(&mut session);
            let (output, _, _) = paint(&mut session);
            session
                .report_surface_presentation(output, SurfacePresentationResult::Presented)
                .unwrap();
            assert!(settle(&mut session).manipulation_feedback().is_empty());
        }
        let released = if missing_route {
            unrouted_edge(&mut session, Event::ButtonReleased(Button::Primary))
        } else {
            edge(
                &mut session,
                Event::ButtonReleased(Button::Primary),
                point,
                SurfacePointerCapture::ProviderEndpoint,
            )
            .unwrap()
        };
        let [terminal] = released.manipulation_feedback() else {
            panic!("one consumed splitter release must publish exactly one terminal: {released:?}");
        };
        let terminal = *terminal;
        assert_eq!(released.manipulation_feedback(), &[terminal]);
        assert_eq!(
            terminal.status(),
            if missing_route {
                Status::Rejected(Rejection::InputAuthorityUnavailable)
            } else {
                Status::Delivered(Delivery::SplitterResize)
            }
        );
        assert!(!session.has_active_gesture());
        if missing_route {
            assert_eq!(session.version(), before_version);
            assert_eq!(split_weights(&session), before_weights);
        } else {
            assert_ne!(session.version(), before_version);
            assert!(split_weights(&session)[0] < before_weights[0]);
        }
        for previous in &terminals {
            assert_ne!(terminal.id(), previous.id());
            assert_ne!(terminal.manipulation_id(), previous.manipulation_id());
        }
        terminals.push(terminal);

        let settled_version = session.version();
        let settled_weights = split_weights(&session);
        for _ in 0..2 {
            assert!(settle(&mut session).manipulation_feedback().is_empty());
        }
        measure(&mut session);
        let (output, _, _) = paint(&mut session);
        session
            .report_surface_presentation(output, SurfacePresentationResult::Presented)
            .unwrap();
        assert!(settle(&mut session).manipulation_feedback().is_empty());
        assert!(
            edge(
                &mut session,
                Event::Moved,
                point,
                SurfacePointerCapture::ProviderEndpoint,
            )
            .unwrap()
            .manipulation_feedback()
            .is_empty()
        );
        assert!(!session.has_active_gesture());
        assert_eq!(session.version(), settled_version);
        assert_eq!(split_weights(&session), settled_weights);
    }
}

fn durable(session: &DockspaceSession) -> (WorkspaceVersion, [Vec<ItemId>; 3], LogicalRect) {
    (
        session.version(),
        [FIRST, SECOND, ItemId::new(3)]
            .map(|item| session.view().item(item).unwrap().tabs().items().to_vec()),
        session
            .view()
            .contained(FloatingPresentationId::new(1))
            .unwrap()
            .rect(),
    )
}

#[cfg(feature = "serde")]
#[test]
fn document_restore_replaces_delivered_layout_without_cancelling_its_identity() {
    use dockspace::runtime::{DockspaceDocumentBootstrap, DockspaceDocumentId, HostInputOutcome};

    const DOCUMENT: DockspaceDocumentId = DockspaceDocumentId::from_bytes([0x76; 16]);
    for gesture in [Gesture::Dock, Gesture::ContainedResize] {
        let mut bootstrap = DockspaceDocumentBootstrap::new(DOCUMENT);
        assert_eq!(bootstrap.ensure_item("pane:first").unwrap(), FIRST);
        assert_eq!(bootstrap.ensure_item("pane:second").unwrap(), SECOND);
        assert_eq!(
            bootstrap.ensure_item("pane:contained").unwrap(),
            ItemId::new(3)
        );
        let persistent =
            DockspaceSession::from_persistent_layout(layout(), DockPolicy::default(), bootstrap)
                .unwrap();
        let (mut session, tab, resize) = ready_fixture(persistent);
        let initial = durable(&session);
        let saved = session.save_document_json().unwrap();
        let (delivered, _) = release(&mut session, gesture, tab, resize);
        let (late_output, _, _) = paint(&mut session);
        let prepared = session
            .prepare_document_restore_json(&saved, |document, key| {
                document == DOCUMENT
                    && matches!(key, "pane:first" | "pane:second" | "pane:contained")
            })
            .unwrap();
        let previous = session.version();
        let mut restore = session.begin_document_restore_frame(&prepared).unwrap();
        restore
            .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
            .unwrap();
        let report = restore.commit().unwrap();
        assert_eq!(
            report.inputs(),
            &[HostInputOutcome::DocumentRestored {
                previous,
                current: session.version()
            }]
        );
        assert_ne!(session.version(), previous);
        assert!(
            report.manipulation_feedback().is_empty(),
            "restore cannot cancel an already delivered manipulation: {delivered:?}"
        );
        assert!(!session.has_active_gesture());
        let restored = durable(&session);
        assert_eq!(restored.1, initial.1);
        assert_eq!(restored.2, initial.2);
        session
            .report_surface_presentation(late_output, SurfacePresentationResult::Presented)
            .unwrap();
        for _ in 0..2 {
            assert!(settle(&mut session).manipulation_feedback().is_empty());
            assert_eq!(durable(&session), restored);
        }
        let (output, _, _) = paint(&mut session);
        session
            .report_surface_presentation(output, SurfacePresentationResult::Presented)
            .unwrap();
        assert!(settle(&mut session).manipulation_feedback().is_empty());
        assert_eq!(durable(&session), restored);
    }
}

#[test]
fn delivered_terminal_feedback_never_replays_after_presented_or_dropped_outputs() {
    for gesture in [Gesture::Dock, Gesture::ContainedResize] {
        for disposition in [
            SurfacePresentationResult::Presented,
            SurfacePresentationResult::Dropped,
        ] {
            let (mut session, tab, resize) = fixture();
            let (terminal, point) = release(&mut session, gesture, tab, resize);
            let committed = durable(&session);
            let (output, _, _) = paint(&mut session);
            session
                .report_surface_presentation(output, disposition)
                .unwrap();
            let report = settle(&mut session);
            assert!(!report.workspace_changed());
            assert!(
                report.manipulation_feedback().is_empty(),
                "the release already delivered once: {terminal:?}"
            );
            assert_eq!(durable(&session), committed);
            for _ in 0..2 {
                assert!(settle(&mut session).manipulation_feedback().is_empty());
                assert_eq!(durable(&session), committed);
            }
            let (later_output, _, _) = paint(&mut session);
            session
                .report_surface_presentation(later_output, SurfacePresentationResult::Presented)
                .unwrap();
            assert!(settle(&mut session).manipulation_feedback().is_empty());
            match edge(
                &mut session,
                Event::ButtonReleased(Button::Primary),
                point,
                SurfacePointerCapture::ProviderEndpoint,
            ) {
                Ok(report) => assert!(report.manipulation_feedback().is_empty()),
                Err(error) => assert_eq!(error.kind(), DockspaceRuntimeErrorKind::HostProtocol),
            }
            assert_eq!(durable(&session), committed);
        }
    }
}

#[test]
fn provider_retirement_preserves_delivered_edits_without_another_terminal() {
    for gesture in [Gesture::Dock, Gesture::ContainedResize] {
        let (mut session, tab, resize) = fixture();
        let (terminal, _) = release(&mut session, gesture, tab, resize);
        let committed = durable(&session);
        let (late_output, _, _) = paint(&mut session);
        let retired = session.disable_host_pointer().unwrap().unwrap();
        assert!(
            retired.manipulation_feedback().is_empty(),
            "no pending obligation remains after {terminal:?}"
        );
        assert_eq!(durable(&session), committed);
        assert!(session.disable_host_pointer().unwrap().is_none());
        session
            .report_surface_presentation(late_output, SurfacePresentationResult::Presented)
            .unwrap();
        assert!(settle(&mut session).manipulation_feedback().is_empty());
        assert_eq!(durable(&session), committed);
    }
}

#[test]
fn policy_changes_preserve_delivered_edits_and_late_output_is_inert() {
    for gesture in [Gesture::Dock, Gesture::ContainedResize] {
        let (mut session, tab, resize) = fixture();
        let (terminal, _) = release(&mut session, gesture, tab, resize);
        let committed = durable(&session);
        let (late_output, _, _) = paint(&mut session);
        let mut policy = session.policy().clone();
        policy.set_close_capability(CloseCapability::Disabled);
        let mut frame = session.begin_host_frame().unwrap();
        frame.replace_policy(policy).unwrap();
        frame
            .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
            .unwrap();
        let report = frame.commit().unwrap();
        assert!(
            report.manipulation_feedback().is_empty(),
            "policy cannot cancel {terminal:?}"
        );
        assert_eq!(report.before(), committed.0);
        assert_eq!(report.after(), session.version());
        assert_ne!(
            report.after(),
            committed.0,
            "the policy replacement owns its own workspace revision"
        );
        let after_policy = durable(&session);
        assert_eq!(
            after_policy.1, committed.1,
            "policy changes cannot replay the delivered dock"
        );
        assert_eq!(
            after_policy.2, committed.2,
            "policy changes cannot undo the delivered resize"
        );
        session
            .report_surface_presentation(late_output, SurfacePresentationResult::Presented)
            .unwrap();
        let settled = settle(&mut session);
        assert!(!settled.workspace_changed());
        assert!(settled.manipulation_feedback().is_empty());
        assert_eq!(durable(&session), after_policy);
    }
}

#[test]
fn a_new_gesture_gets_a_distinct_identity_without_replacing_a_delivered_edit() {
    let (mut session, tab, resize) = fixture();
    let (first, _) = release(&mut session, Gesture::ContainedResize, tab, resize);
    let committed = durable(&session);
    let (dropped, _, _) = paint(&mut session);
    session
        .report_surface_presentation(dropped, SurfacePresentationResult::Dropped)
        .unwrap();
    assert!(settle(&mut session).manipulation_feedback().is_empty());
    assert_eq!(durable(&session), committed);
    let (output, _, resize) = paint(&mut session);
    session
        .report_surface_presentation(output, SurfacePresentationResult::Presented)
        .unwrap();
    assert!(settle(&mut session).manipulation_feedback().is_empty());
    let replacement = edge(
        &mut session,
        Event::ButtonPressed(Button::Primary),
        resize,
        SurfacePointerCapture::ProviderEndpoint,
    )
    .unwrap();
    assert!(replacement.manipulation_feedback().is_empty());
    assert_eq!(durable(&session), committed);
    assert!(session.has_active_gesture());
    let point = LogicalPoint::new(resize.x() + 24.0, resize.y() + 16.0).unwrap();
    assert!(
        edge(
            &mut session,
            Event::Moved,
            point,
            SurfacePointerCapture::ProviderEndpoint
        )
        .unwrap()
        .manipulation_feedback()
        .is_empty()
    );
    let report = edge(
        &mut session,
        Event::ButtonReleased(Button::Primary),
        point,
        SurfacePointerCapture::ProviderEndpoint,
    )
    .unwrap();
    let [next] = report.manipulation_feedback() else {
        panic!("the new gesture delivers its own terminal once: {report:?}")
    };
    assert_ne!(first.manipulation_id(), next.manipulation_id());
    assert_ne!(first.id(), next.id());
    assert_eq!(
        next.status(),
        Status::Delivered(Delivery::ContainedTransform)
    );
    assert!(report.workspace_changed());
    assert_eq!(
        session
            .view()
            .contained(FloatingPresentationId::new(1))
            .unwrap()
            .rect(),
        rect(520.0, 160.0, 360.0, 300.0)
    );
    let after = durable(&session);
    assert!(settle(&mut session).manipulation_feedback().is_empty());
    assert_eq!(durable(&session), after);
}
