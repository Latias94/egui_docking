//! Public output admission preserves exact presentation and receiver dependencies.

use std::num::NonZeroUsize;

use dockspace::geometry::{LogicalPoint, LogicalRect, LogicalSize};
use dockspace::model::{
    DockspaceContainedLayout, DockspaceLayout, DockspaceNode, DockspaceRootLayout,
    DockspaceSurfaceLayout, FloatingPresentationId, ItemId, RootId, SurfaceId,
};
use dockspace::policy::DockPolicy;
use dockspace::runtime::{
    DockspaceHostFrame, DockspaceManipulationDelivery, DockspaceManipulationStatus,
    DockspaceReceiverDescriptor, DockspaceSession, HostFrameReport, HostPointerInput,
    HostPointerRoute, PaintedSurfaceOutput, PointerEventMetadata, PresentedDockReceiver,
    PresentedDockspaceSurface, SurfacePaintProgramOperation as Operation,
    SurfacePaintProgramStep as Step, SurfacePointerButton as Button, SurfacePointerCapture,
    SurfacePointerEvent as Event, SurfacePointerId, SurfacePointerReceiverFacts,
    SurfacePresentationResult, SurfaceUnavailableReason, SurfaceWork, SurfaceWorkDeferred,
    UniformSurfaceMetrics,
};
use dockspace_host_harness::RecordingRenderer;

const A: SurfaceId = SurfaceId::new(1);
const B: SurfaceId = SurfaceId::new(2);
const C: SurfaceId = SurfaceId::new(3);
const MAIN: RootId = RootId::new(1);
const CONTAINED_ROOT: RootId = RootId::new(2);
const FLOATING: FloatingPresentationId = FloatingPresentationId::new(1);
const FIRST: ItemId = ItemId::new(1);
const SECOND: ItemId = ItemId::new(2);
const TARGET: ItemId = ItemId::new(3);

fn rect(x: f64, y: f64, width: f64, height: f64) -> LogicalRect {
    LogicalRect::new(x, y, width, height).unwrap()
}

fn metrics(width: f64) -> UniformSurfaceMetrics {
    UniformSurfaceMetrics::new(
        rect(0.0, 0.0, width, 600.0),
        LogicalSize::new(64.0, 48.0).unwrap(),
        96.0,
    )
    .unwrap()
}

fn main_surface() -> DockspaceSurfaceLayout {
    DockspaceSurfaceLayout::new(
        A,
        DockspaceRootLayout::new(MAIN, DockspaceNode::central_tabs([FIRST, SECOND])),
    )
}

fn session() -> DockspaceSession {
    DockspaceSession::from_layout(
        DockspaceLayout::new([main_surface()]).unwrap(),
        DockPolicy::default(),
    )
    .unwrap()
}

fn commit(mut frame: DockspaceHostFrame<'_>) -> HostFrameReport {
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    frame.commit().unwrap()
}

fn encode(frame: &mut DockspaceHostFrame<'_>, surface: SurfaceId) {
    let plan = match frame.surface_work(surface).unwrap() {
        SurfaceWork::Ready(plan) => plan,
        SurfaceWork::Deferred(reason) => panic!("{surface:?} unexpectedly deferred: {reason:?}"),
    };
    let (_, receipt) = RecordingRenderer::execute(plan).unwrap();
    frame.confirm_surface_painted(receipt).unwrap();
}

fn only_output(report: &mut HostFrameReport) -> PaintedSurfaceOutput {
    let [output] = report.take_painted_outputs().try_into().unwrap();
    output
}

fn emit_main(
    session: &mut DockspaceSession,
    width: f64,
) -> (
    DockspaceReceiverDescriptor,
    DockspaceReceiverDescriptor,
    PaintedSurfaceOutput,
) {
    let mut frame = session.begin_host_frame().unwrap();
    frame.measure_surface(A, metrics(width)).unwrap();
    let plan = frame.paint_plan(A).unwrap().unwrap();
    let tab = plan.tab_receiver(FIRST).unwrap();
    let mut pane = None;
    plan.visit_surface_program(|step| {
        if let Step::Operation {
            operation: Operation::Pane(record),
            ..
        } = step
            && record.root() == MAIN
        {
            assert!(
                pane.replace(plan.receiver_for_pane(record).unwrap())
                    .is_none()
            );
        }
        Ok::<(), ()>(())
    })
    .unwrap();
    encode(&mut frame, A);
    let output = only_output(&mut commit(frame));
    (tab, pane.unwrap(), output)
}

fn present(session: &mut DockspaceSession, output: PaintedSurfaceOutput) {
    let artifact = output.presentation_artifact();
    session
        .report_surface_presentation(output, SurfacePresentationResult::Presented)
        .unwrap();
    let report = commit(session.begin_host_frame().unwrap());
    let [settled] = report.surface_presentation_commits() else {
        panic!("the single presented output must settle once");
    };
    assert!(settled.presents(artifact));
}

#[test]
fn first_pending_output_after_deferred_selection_admits_only_unchanged_receivers() {
    for change_selection in [false, true] {
        let mut session = session();
        let (tab, pane, first) = emit_main(&mut session, 960.0);
        assert!(session.presented_surface(A).is_none());
        assert!(session.bind_presented_receiver(&tab).is_none());
        assert!(session.bind_presented_receiver(&pane).is_none());

        let selected = if change_selection { SECOND } else { FIRST };
        let mut frame = session.begin_host_frame().unwrap();
        frame.select_item_current(selected).unwrap();
        let mut deferred = commit(frame);
        assert_eq!(deferred.workspace_changed(), change_selection);
        assert!(deferred.take_painted_outputs().is_empty());
        assert!(session.view().item(selected).unwrap().is_selected());
        assert!(session.presented_surface(A).is_none());

        present(&mut session, first);
        let actual = session.presented_surface(A).unwrap();
        let bound_tab = session.bind_presented_receiver(&tab).unwrap();
        assert_eq!(
            session.bind_presented_receiver(&pane).is_some(),
            !change_selection
        );

        // A measured successor cannot lend its changed selection or geometry to A.
        let (next_tab, next_pane, successor) = emit_main(&mut session, 800.0);
        assert_ne!(next_tab, tab);
        assert_ne!(next_pane, pane);
        assert_eq!(session.presented_surface(A), Some(actual));
        assert_eq!(session.bind_presented_receiver(&tab), Some(bound_tab));
        assert!(session.bind_presented_receiver(&next_tab).is_none());
        assert!(session.bind_presented_receiver(&next_pane).is_none());
        assert_eq!(
            session.bind_presented_receiver(&pane).is_some(),
            !change_selection
        );

        present(&mut session, successor);
        assert!(session.bind_presented_receiver(&next_tab).is_some());
        assert!(session.bind_presented_receiver(&next_pane).is_some());
        assert!(session.bind_presented_receiver(&tab).is_none());
        assert!(session.bind_presented_receiver(&pane).is_none());
    }
}

#[test]
fn output_admission_counts_all_new_streams_before_encoding_and_keeps_input_publishable() {
    for capacity in [2, 3] {
        let mut session = session();
        session
            .configure_output_capacity(NonZeroUsize::new(capacity).unwrap())
            .unwrap();
        let (_, _, first) = emit_main(&mut session, 960.0);
        present(&mut session, first);
        let actual = session.presented_surface(A).unwrap();

        let mut frame = session.begin_host_frame().unwrap();
        frame
            .attach_empty_surface_current(B, RootId::new(2))
            .unwrap();
        frame
            .attach_empty_surface_current(C, RootId::new(3))
            .unwrap();
        let mut attached = commit(frame);
        assert!(attached.workspace_changed());
        assert!(attached.take_painted_outputs().is_empty());
        assert!(session.view().surface(B).is_some());
        assert!(session.view().surface(C).is_some());
        assert_eq!(session.presented_surface(A), Some(actual));

        let mut frame = session.begin_host_frame().unwrap();
        frame.select_item_current(SECOND).unwrap();
        frame.measure_surface(B, metrics(960.0)).unwrap();
        frame.measure_surface(C, metrics(960.0)).unwrap();
        encode(&mut frame, B);
        if capacity == 2 {
            assert!(matches!(
                frame.surface_work(C).unwrap(),
                SurfaceWork::Deferred(SurfaceWorkDeferred::OutputCapacity)
            ));
        } else {
            encode(&mut frame, C);
        }
        let mut report = commit(frame);
        assert!(report.workspace_changed());
        assert!(session.view().item(SECOND).unwrap().is_selected());
        let outputs = report.take_painted_outputs();
        assert_eq!(outputs.len(), capacity - 1);
        assert!(outputs.iter().any(|output| output.surface() == B));
        assert_eq!(
            outputs.iter().any(|output| output.surface() == C),
            capacity == 3
        );

        // A full stream roster must still permit a semantic update with no output.
        let mut frame = session.begin_host_frame().unwrap();
        frame.select_item_current(FIRST).unwrap();
        if capacity == 2 {
            assert!(matches!(
                frame.surface_work(C).unwrap(),
                SurfaceWork::Deferred(SurfaceWorkDeferred::OutputCapacity)
            ));
        }
        let mut input_only = commit(frame);
        assert!(input_only.workspace_changed());
        assert!(input_only.take_painted_outputs().is_empty());
        assert!(session.view().item(FIRST).unwrap().is_selected());

        drop(outputs);
        let retired = commit(session.begin_host_frame().unwrap());
        assert_eq!(retired.surface_presentation_commits().len(), capacity - 1);
        assert_eq!(session.presented_surface(A), Some(actual));

        if capacity == 2 {
            let mut frame = session.begin_host_frame().unwrap();
            frame.retire_empty_surface_current(B).unwrap();
            assert!(commit(frame).workspace_changed());
            let mut frame = session.begin_host_frame().unwrap();
            encode(&mut frame, C);
            let output = only_output(&mut commit(frame));
            assert_eq!(output.surface(), C);
            present(&mut session, output);
        }
    }
}

fn emit_contained(
    session: &mut DockspaceSession,
) -> (
    DockspaceReceiverDescriptor,
    DockspaceReceiverDescriptor,
    PaintedSurfaceOutput,
) {
    let mut frame = session.begin_host_frame().unwrap();
    frame.measure_surface(A, metrics(960.0)).unwrap();
    let plan = frame.paint_plan(A).unwrap().unwrap();
    let source = plan.tab_receiver(FIRST).unwrap();
    let target = plan.center_drop_receiver_for_item(TARGET).unwrap();
    encode(&mut frame, A);
    (source, target, only_output(&mut commit(frame)))
}

fn drag_onto(
    session: &mut DockspaceSession,
    surface: PresentedDockspaceSurface,
    source: &PresentedDockReceiver,
    target: &PresentedDockReceiver,
) -> HostFrameReport {
    let start = source.center();
    let moved = LogicalPoint::new(start.x() + 32.0, start.y()).unwrap();
    let press = HostPointerInput::new(
        SurfacePointerId::new(1),
        Event::ButtonPressed(Button::Primary),
        HostPointerRoute::new(surface, start),
        HostPointerRoute::new(surface, start),
        SurfacePointerCapture::ProviderEndpoint,
        SurfacePointerReceiverFacts::delivery(source),
    );
    let target_edge = |event| {
        HostPointerInput::new(
            SurfacePointerId::new(1),
            event,
            HostPointerRoute::new(surface, moved),
            HostPointerRoute::new(surface, target.center()),
            SurfacePointerCapture::ProviderEndpoint,
            SurfacePointerReceiverFacts::delivery(source).with_hover(target),
        )
        .with_metadata(PointerEventMetadata::unknown().with_magnetic_targeting(false))
    };
    let mut frame = session.begin_host_frame().unwrap();
    frame
        .submit_host_pointer_batch([
            press,
            target_edge(Event::Moved),
            target_edge(Event::ButtonReleased(Button::Primary)),
        ])
        .unwrap();
    commit(frame)
}

fn assert_docked(session: &DockspaceSession, report: &HostFrameReport) {
    assert!(report.workspace_changed());
    let [feedback] = report.manipulation_feedback() else {
        panic!("one ordinary release must produce one terminal delivery");
    };
    assert_eq!(
        feedback.status(),
        DockspaceManipulationStatus::Delivered(DockspaceManipulationDelivery::Dock)
    );
    assert_eq!(session.view().item(FIRST).unwrap().root(), CONTAINED_ROOT);
    assert_eq!(
        session.view().item(FIRST).unwrap().tabs().items(),
        session.view().item(TARGET).unwrap().tabs().items()
    );
    assert!(!session.has_active_gesture());
    assert!(!session.has_pending_presentation());
}

#[test]
fn contained_drop_requires_the_presented_targets_current_durable_rectangle() {
    for move_rect in [false, true] {
        let original = rect(640.0, 140.0, 400.0, 260.0);
        let layout = DockspaceLayout::new([main_surface().with_contained(
            DockspaceContainedLayout::new(
                FLOATING,
                DockspaceRootLayout::new(CONTAINED_ROOT, DockspaceNode::tabs([TARGET])),
                original,
            ),
        )])
        .unwrap();
        let mut session = DockspaceSession::from_layout(layout, DockPolicy::default()).unwrap();
        session.enable_host_pointer().unwrap();
        let (source_descriptor, target_descriptor, output) = emit_contained(&mut session);
        present(&mut session, output);
        let actual = session.presented_surface(A).unwrap();
        let source = session.bind_presented_receiver(&source_descriptor).unwrap();
        let target = session.bind_presented_receiver(&target_descriptor).unwrap();

        if move_rect {
            let action = session.prepare_bring_contained_into_view(TARGET);
            let mut frame = session.begin_host_frame().unwrap();
            frame.submit_prepared_action(action).unwrap();
            let mut moved = commit(frame);
            assert!(moved.workspace_changed());
            assert!(moved.take_painted_outputs().is_empty());
            assert_eq!(
                session.view().contained(FLOATING).unwrap().rect(),
                rect(560.0, 140.0, 400.0, 260.0)
            );
            assert_eq!(session.presented_surface(A), Some(actual));
            assert_eq!(
                session.bind_presented_receiver(&source_descriptor),
                Some(source)
            );
            assert!(
                session
                    .bind_presented_receiver(&target_descriptor)
                    .is_none(),
                "the moved target must reject a new bind before testing its old bound receiver; \
                 target={target:?}; workspace={:#?}",
                session.view()
            );
        }

        // Submit the old bound target as well: rejecting new binds alone is insufficient.
        let report = drag_onto(&mut session, actual, &source, &target);
        if !move_rect {
            assert_docked(&session, &report);
            continue;
        }
        assert!(
            report.manipulation_feedback().iter().all(|feedback| {
                !matches!(
                    feedback.status(),
                    DockspaceManipulationStatus::Delivered(DockspaceManipulationDelivery::Dock)
                )
            }),
            "the old bound target bypassed current rectangle admission and delivered Dock; \
             manipulation={:?}; hover={:?}; report={report:#?}; workspace={:#?}",
            LogicalPoint::new(source.center().x() + 32.0, source.center().y()).unwrap(),
            target.center(),
            session.view()
        );
        assert!(
            !report.workspace_changed(),
            "rejecting the old contained target must not fall through to a contained placement; \
             report={report:#?}; workspace={:#?}",
            session.view()
        );
        assert_eq!(session.view().item(FIRST).unwrap().root(), MAIN);
        assert_eq!(session.view().item(TARGET).unwrap().root(), CONTAINED_ROOT);
        assert!(
            session
                .bind_presented_receiver(&target_descriptor)
                .is_none()
        );
        assert!(!session.has_active_gesture());
        assert!(!session.has_pending_presentation());

        let (current_source, current_target, successor) = emit_contained(&mut session);
        assert!(session.bind_presented_receiver(&current_source).is_none());
        assert!(session.bind_presented_receiver(&current_target).is_none());
        assert!(
            session
                .bind_presented_receiver(&target_descriptor)
                .is_none()
        );
        assert_eq!(session.presented_surface(A), Some(actual));

        present(&mut session, successor);
        let surface = session.presented_surface(A).unwrap();
        let source = session.bind_presented_receiver(&current_source).unwrap();
        let target = session.bind_presented_receiver(&current_target).unwrap();
        let report = drag_onto(&mut session, surface, &source, &target);
        assert_docked(&session, &report);
    }
}
