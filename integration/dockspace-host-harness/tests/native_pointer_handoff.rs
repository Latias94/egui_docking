//! Explicit native-local to desktop handoff through the public joined host API.

use dockspace::runtime::{NativeWindowBinding, NativeWindowHit, SurfacePresentationArtifact};
use std::cell::Cell;

use std::time::Duration;

use dockspace::geometry::{
    LogicalPoint, LogicalRect, LogicalSize, PhysicalPoint, PhysicalRect, PhysicalSize, ScaleFactor,
};
use dockspace::model::{
    DockspaceLayout, DockspaceNode, DockspaceRootLayout, DockspaceSurfaceLayout, ItemId, RootId,
    SurfaceId,
};
use dockspace::policy::DockPolicy;
use dockspace::runtime::{
    DockspaceHostFrame, DockspaceManipulationDelivery as Delivery, DockspaceManipulationFeedback,
    DockspaceManipulationId, DockspaceManipulationStatus as Status, DockspacePreviewVisual,
    DockspaceReceiverDescriptor, DockspaceReceiverRole, DockspaceSession, HostFrameReport,
    HostWindowToken, NativeDesktopPointerLocation, NativeDesktopPosition, NativeHostCapabilities,
    NativeHostCapability, NativeHostErrorKind, NativeLocalCoordinates, NativeLocalGeometry,
    NativeLocalPointerFact, NativeLocalPointerLocation, NativeLocalPointerSample,
    NativeLocalToDesktopHandoff, NativePointerButton as Button, NativePointerEvent as Event,
    NativePointerHover, NativePointerId, NativePointerInput, NativePointerOwner,
    NativePointerRoster, NativeReceiverAnswer, NativeReceiverPurpose, NativeReceiverQuery,
    NativeSurfaceBinding, NativeWindowFacts, NativeWindowInputState, NativeWindowPresentationState,
    NativeWorkAreaRoster, PaintedSurfaceOutput, PointerDeviceKind, PointerEventMetadata,
    SurfacePaintProgramOperation as Operation, SurfacePaintProgramStep as Step,
    SurfacePresentationResult as Disposition, SurfaceUnavailableReason, SurfaceWork,
    SurfaceWorkDeferred, UniformSurfaceMetrics,
};
use dockspace_host_harness::{OwnedRecord, RecordingRenderer};

const A: SurfaceId = SurfaceId::new(1);
const B: SurfaceId = SurfaceId::new(2);
const FIRST: ItemId = ItemId::new(1);
const SECOND: ItemId = ItemId::new(2);
const THIRD: ItemId = ItemId::new(3);
const OTHER: ItemId = ItemId::new(4);
const FIFTH: ItemId = ItemId::new(5);
const POINTER: NativePointerId = NativePointerId::new(17);
const OTHER_POINTER: NativePointerId = NativePointerId::new(18);

fn geometry() -> NativeLocalGeometry {
    NativeLocalGeometry::new(
        PhysicalSize::new(1200.0, 900.0).unwrap(),
        ScaleFactor::new(2.0).unwrap(),
    )
}

fn bounds(surface: SurfaceId) -> PhysicalRect {
    match surface {
        A => PhysicalRect::new(400.0, -300.0, 1200.0, 900.0).unwrap(),
        B => PhysicalRect::new(1800.0, 250.0, 1200.0, 900.0).unwrap(),
        _ => unreachable!(),
    }
}

fn facts(origin: Option<PhysicalRect>) -> NativeWindowFacts {
    let geometry = geometry();
    let facts = NativeWindowFacts::live()
        .with_content_size(geometry.content_size())
        .with_native_scale_factor(geometry.native_scale_factor())
        .with_presentation_scale_factor(ScaleFactor::new(1.5).unwrap())
        .with_input(NativeWindowInputState::ReceivesInput, None)
        .with_presentation(NativeWindowPresentationState::Visible);
    match origin {
        Some(bounds) => facts.with_content_bounds(bounds),
        None => facts,
    }
}

fn finish(mut frame: DockspaceHostFrame<'_>) -> HostFrameReport {
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    frame.commit().unwrap()
}

fn idle(session: &mut DockspaceSession) -> HostFrameReport {
    finish(
        session
            .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
            .unwrap(),
    )
}

fn snapshot(
    session: &mut DockspaceSession,
    bindings: [NativeSurfaceBinding; 2],
    a_bounds: Option<PhysicalRect>,
) {
    session
        .report_managed_native_snapshot(
            [
                (bindings[0], facts(a_bounds)),
                (bindings[1], facts(Some(bounds(B)))),
            ],
            NativeWorkAreaRoster::Unknown,
        )
        .unwrap();
}

fn measure(frame: &mut DockspaceHostFrame<'_>) {
    for surface in [A, B] {
        frame
            .measure_surface(
                surface,
                UniformSurfaceMetrics::new(
                    LogicalRect::new(0.0, 0.0, 800.0, 600.0).unwrap(),
                    LogicalSize::new(32.0, 24.0).unwrap(),
                    96.0,
                )
                .unwrap(),
            )
            .unwrap();
    }
}

struct Tab {
    item: ItemId,
    receiver: DockspaceReceiverDescriptor,
    visible: LogicalRect,
}

struct Scene {
    presentation: Cell<Option<(NativeWindowBinding, SurfacePresentationArtifact)>>,
    surface: SurfaceId,
    recording: RecordingRenderer,
    tabs: Vec<Tab>,
    dragged_item: Option<ItemId>,
}

impl Scene {
    fn tab(&self, item: ItemId) -> &Tab {
        self.tabs.iter().find(|tab| tab.item == item).unwrap()
    }

    fn insertion(&self, item: ItemId, before: bool) -> LogicalPoint {
        let tab = self.tab(item);
        let point = LogicalPoint::new(
            tab.visible.x() + tab.visible.width() * if before { 0.25 } else { 0.625 },
            tab.visible.y() + tab.visible.height() * 0.5,
        )
        .unwrap();
        assert!(tab.receiver.bounds().contains(point));
        assert_ne!(point, tab.receiver.center());
        point
    }

    fn answer(&self, query: impl Into<NativeReceiverQuery>) -> NativeReceiverAnswer {
        let query = match query.into() {
            NativeReceiverQuery::WindowHit(query) => {
                let (window, artifact) = self
                    .presentation
                    .get()
                    .expect("a receiver scene must retain its exact painted output");
                return NativeReceiverAnswer::WindowHit(if query.window() == window {
                    NativeWindowHit::Surface(artifact)
                } else {
                    NativeWindowHit::Unknown
                });
            }
            NativeReceiverQuery::Surface(query) => query,
        };
        if query.surface() != self.surface {
            return NativeReceiverAnswer::Unknown;
        }
        let Some(point) = query.point() else {
            return NativeReceiverAnswer::Unknown;
        };
        let Some(tab) = self
            .tabs
            .iter()
            .find(|tab| tab.receiver.bounds().contains(point))
        else {
            return NativeReceiverAnswer::Unknown;
        };
        assert!(
            self.recording
                .records
                .contains(&OwnedRecord::Receiver(tab.receiver))
        );
        if matches!(query.purpose(), NativeReceiverPurpose::HoverHit) {
            // Only a real registered tab clears the framework hover lane. Core
            // determines the source-suppressed insertion winner at this point.
            query.clear_hover_lane(&tab.receiver).map_or(
                NativeReceiverAnswer::Unknown,
                NativeReceiverAnswer::ClearHoverLane,
            )
        } else {
            query
                .bind_receiver(&tab.receiver)
                .map_or(NativeReceiverAnswer::Unknown, NativeReceiverAnswer::Dock)
        }
    }

    fn assert_preview(&self, expected: bool) {
        if expected {
            assert!(self.recording.records.iter().any(|record| matches!(
                record,
                OwnedRecord::Preview(DockspacePreviewVisual::Dock { surface, .. })
                    if *surface == self.surface
            )));
        } else {
            assert!(
                self.recording
                    .records
                    .iter()
                    .all(|record| !matches!(record, OwnedRecord::Preview(_)))
            );
        }
    }
}

fn render(frame: &mut DockspaceHostFrame<'_>, surface: SurfaceId) -> Scene {
    if matches!(
        frame.surface_work(surface).unwrap(),
        SurfaceWork::Deferred(SurfaceWorkDeferred::Unmeasured)
    ) {
        frame
            .measure_surface(
                surface,
                UniformSurfaceMetrics::new(
                    LogicalRect::new(0.0, 0.0, 800.0, 600.0).unwrap(),
                    LogicalSize::new(32.0, 24.0).unwrap(),
                    96.0,
                )
                .unwrap(),
            )
            .unwrap();
    }
    let plan = frame.paint_plan(surface).unwrap().unwrap();
    let mut tabs = Vec::new();
    let mut dragged_item = None;
    plan.visit_surface_program(|step| {
        match step {
            Step::Operation {
                operation: Operation::Tab(tab),
                ..
            } => {
                if let Some(receiver) = plan.receiver_for_tab_body(tab) {
                    assert_eq!(receiver.role(), DockspaceReceiverRole::TabBody);
                    tabs.push(Tab {
                        item: tab.item(),
                        receiver,
                        visible: tab.visible_bounds(),
                    });
                }
            }
            Step::DragDecoration(decoration) => dragged_item = decoration.item(),
            _ => {}
        }
        Ok::<(), ()>(())
    })
    .unwrap();
    let (recording, receipt) = RecordingRenderer::execute(plan).unwrap();
    for tab in &tabs {
        assert!(
            recording
                .records
                .contains(&OwnedRecord::Receiver(tab.receiver))
        );
    }
    frame.confirm_surface_painted(receipt).unwrap();
    Scene {
        presentation: Cell::new(None),
        surface,
        recording,
        tabs,
        dragged_item,
    }
}

fn take_output(
    report: &mut HostFrameReport,
    scene: &Scene,
    binding: NativeSurfaceBinding,
) -> PaintedSurfaceOutput {
    let [output] = report.take_painted_outputs().try_into().unwrap();
    assert!(output.matches_native_binding(binding));
    assert!(matches!(
        scene.recording.records.last(),
        Some(OwnedRecord::SurfaceEnd(semantic)) if output.matches_semantic_output(*semantic)
    ));
    scene
        .presentation
        .set(Some((binding.window(), output.presentation_artifact())));
    output
}

fn paint(
    session: &mut DockspaceSession,
    binding: NativeSurfaceBinding,
) -> (Scene, PaintedSurfaceOutput) {
    let mut frame = session
        .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
        .unwrap();
    let scene = render(&mut frame, binding.surface());
    let mut report = finish(frame);
    let output = take_output(&mut report, &scene, binding);
    (scene, output)
}

fn present(session: &mut DockspaceSession, output: PaintedSurfaceOutput) -> HostFrameReport {
    let artifact = output.presentation_artifact();
    let version = session.version();
    let order = order(session);
    session
        .report_surface_presentation(output, Disposition::Presented)
        .unwrap();
    assert_eq!(session.version(), version);
    assert_eq!(self::order(session), order);
    let report = idle(session);
    assert_eq!(
        report
            .surface_presentation_commits()
            .iter()
            .filter(|commit| commit.settles(artifact))
            .count(),
        1,
    );
    report
}

fn fixture(with_origin: bool) -> (DockspaceSession, [NativeSurfaceBinding; 2], [Scene; 2]) {
    fixture_with_routing(with_origin, false)
}

fn fixture_with_routing(
    with_origin: bool,
    routing: bool,
) -> (DockspaceSession, [NativeSurfaceBinding; 2], [Scene; 2]) {
    fixture_with_destination(with_origin, routing, [OTHER])
}

fn fixture_with_destination(
    with_origin: bool,
    routing: bool,
    destination: impl IntoIterator<Item = ItemId>,
) -> (DockspaceSession, [NativeSurfaceBinding; 2], [Scene; 2]) {
    let layout = DockspaceLayout::new([
        DockspaceSurfaceLayout::new(
            A,
            DockspaceRootLayout::new(
                RootId::new(1),
                DockspaceNode::central_tabs([FIRST, SECOND, THIRD]),
            ),
        ),
        DockspaceSurfaceLayout::new(
            B,
            DockspaceRootLayout::new(RootId::new(2), DockspaceNode::central_tabs(destination)),
        ),
    ])
    .unwrap();
    let mut policy = DockPolicy::default();
    policy.set_allow_native_surfaces(true);
    let mut session = DockspaceSession::from_layout(layout, policy).unwrap();
    session
        .enable_managed_native_host(NativePointerRoster::Exact(Vec::new()))
        .unwrap();
    let mut capabilities = NativeHostCapabilities::none_supported()
        .with(NativeHostCapability::AuthoritativeInventory)
        .with(NativeHostCapability::AuthoritativeButtonState)
        .with(NativeHostCapability::PointerHitTestObservation)
        .with(NativeHostCapability::DesktopPointerPosition)
        .with(NativeHostCapability::HoveredWindow);
    if routing {
        capabilities = capabilities.with(NativeHostCapability::PointerHitTestControl);
    }
    session
        .configure_managed_native_capabilities(capabilities)
        .unwrap();
    for (surface, token) in [(A, 1), (B, 2)] {
        session
            .register_native_root(surface, HostWindowToken::new(token))
            .unwrap();
    }
    let registration = idle(&mut session);
    let bindings = [A, B].map(|surface| {
        *registration
            .native_bindings()
            .iter()
            .find(|binding| binding.surface() == surface)
            .unwrap()
    });
    snapshot(&mut session, bindings, with_origin.then(|| bounds(A)));
    let mut frame = session
        .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
        .unwrap();
    measure(&mut frame);
    assert!(!finish(frame).workspace_changed());
    let scenes = bindings.map(|binding| {
        let (scene, output) = paint(&mut session, binding);
        assert!(!present(&mut session, output).workspace_changed());
        assert!(session.native_local_coordinates(binding).is_some());
        scene
    });
    (session, bindings, scenes)
}

fn order(session: &DockspaceSession) -> Vec<ItemId> {
    session.view().item(FIRST).unwrap().tabs().items().to_vec()
}

fn capture_client(
    cap: NativeLocalCoordinates,
    binding: NativeSurfaceBinding,
    client: PhysicalPoint,
) -> NativeLocalPointerSample {
    cap.capture(
        binding.window(),
        NativeLocalPointerFact::new(
            client,
            geometry().content_size(),
            geometry().native_scale_factor(),
        ),
        geometry(),
    )
    .unwrap()
}

fn sample(
    cap: NativeLocalCoordinates,
    binding: NativeSurfaceBinding,
    point: LogicalPoint,
) -> NativeLocalPointerSample {
    let factor = 1.5;
    let client = PhysicalPoint::new(point.x() * factor, point.y() * factor).unwrap();
    let sample = capture_client(cap, binding, client);
    assert_eq!(sample.client_position(), client);
    assert_eq!(sample_logical(sample), point);
    sample
}

// These fixture surfaces use an explicitly declared whole-client 1.5 mapping.
// This conversion supplies expected test values, never a runtime capability.
fn sample_logical(sample: NativeLocalPointerSample) -> LogicalPoint {
    let client = sample.client_position();
    LogicalPoint::new(client.x() / 1.5, client.y() / 1.5).unwrap()
}

fn desktop_point(surface: SurfaceId, point: LogicalPoint) -> PhysicalPoint {
    let bounds = bounds(surface);
    let factor = 1.5;
    PhysicalPoint::new(
        bounds.x() + point.x() * factor,
        bounds.y() + point.y() * factor,
    )
    .unwrap()
}

fn desktop_location(
    hover: NativeSurfaceBinding,
    point: PhysicalPoint,
) -> NativeDesktopPointerLocation {
    NativeDesktopPointerLocation::new(
        NativeDesktopPosition::Exact(point),
        NativePointerHover::Dock(hover.window()),
        None,
    )
}

fn capture_for(binding: NativeSurfaceBinding, event: Event) -> NativePointerOwner {
    if matches!(event, Event::ButtonReleased(_)) {
        NativePointerOwner::None
    } else {
        NativePointerOwner::Native(binding.window())
    }
}

fn local(
    binding: NativeSurfaceBinding,
    event: Event,
    sample: NativeLocalPointerSample,
) -> NativePointerInput {
    NativePointerInput::new_local(
        POINTER,
        event,
        NativeLocalPointerLocation::Exact(sample),
        NativePointerOwner::Native(binding.window()),
        capture_for(binding, event),
    )
}

fn desktop(binding: NativeSurfaceBinding, event: Event, point: LogicalPoint) -> NativePointerInput {
    NativePointerInput::new(
        POINTER,
        event,
        desktop_location(binding, desktop_point(binding.surface(), point)),
        NativePointerOwner::Native(binding.window()),
        capture_for(binding, event),
    )
}

fn handoff(
    pointer: NativePointerId,
    binding: NativeSurfaceBinding,
    event: Event,
    local: NativeLocalPointerSample,
    desktop: NativeDesktopPointerLocation,
    capture: NativePointerOwner,
) -> NativePointerInput {
    NativePointerInput::new_handoff(
        pointer,
        event,
        NativeLocalToDesktopHandoff::new(local, desktop),
        NativePointerOwner::Native(binding.window()),
        capture,
    )
}

fn moved_handoff(
    binding: NativeSurfaceBinding,
    local: NativeLocalPointerSample,
) -> NativePointerInput {
    handoff(
        POINTER,
        binding,
        Event::Moved,
        local,
        desktop_location(
            binding,
            desktop_point(binding.surface(), sample_logical(local)),
        ),
        NativePointerOwner::Native(binding.window()),
    )
}

fn reorder_batch(
    session: &DockspaceSession,
    binding: NativeSurfaceBinding,
    scene: &Scene,
) -> [NativePointerInput; 4] {
    let cap = session.native_local_coordinates(binding).unwrap();
    let source = sample(cap, binding, scene.tab(FIRST).receiver.center());
    let target = sample(cap, binding, scene.insertion(THIRD, false));
    [
        local(binding, Event::ButtonPressed(Button::Primary), source),
        local(binding, Event::Moved, target),
        moved_handoff(binding, target),
        desktop(
            binding,
            Event::ButtonReleased(Button::Primary),
            sample_logical(target),
        ),
    ]
}

fn feedback(report: &HostFrameReport) -> DockspaceManipulationFeedback {
    let [feedback] = report.manipulation_feedback() else {
        panic!("expected one manipulation publication: {report:?}");
    };
    *feedback
}

fn assert_delivered(report: &HostFrameReport) -> DockspaceManipulationFeedback {
    assert!(report.workspace_changed());
    let terminal = feedback(report);
    assert_eq!(terminal.status(), Status::Delivered(Delivery::Dock));
    terminal
}

fn assert_no_delivery(report: &HostFrameReport) {
    assert!(!report.workspace_changed());
    assert!(
        report
            .manipulation_feedback()
            .iter()
            .all(|entry| !matches!(entry.status(), Status::Delivered(_) | Status::Pending(_)))
    );
}

fn assert_clean_idle(session: &mut DockspaceSession) {
    let version = session.version();
    let order = order(session);
    let report = idle(session);
    assert!(!report.workspace_changed());
    assert!(report.manipulation_feedback().is_empty());
    assert!(!session.has_active_gesture());
    assert!(!session.has_pending_native_input());
    assert_eq!(session.version(), version);
    assert_eq!(self::order(session), order);
}

fn refresh(session: &mut DockspaceSession, bindings: [NativeSurfaceBinding; 2]) -> Scene {
    snapshot(session, bindings, Some(bounds(A)));
    let mut frame = session
        .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
        .unwrap();
    measure(&mut frame);
    finish(frame);
    let (scene, output) = paint(session, bindings[0]);
    assert!(present(session, output).manipulation_feedback().is_empty());
    scene
}

fn deliver_reorder(
    session: &mut DockspaceSession,
    binding: NativeSurfaceBinding,
    scene: &Scene,
) -> DockspaceManipulationId {
    let version = session.version();
    assert_eq!(order(session), [FIRST, SECOND, THIRD]);
    session
        .record_native_pointer_batch(reorder_batch(session, binding, scene))
        .unwrap();
    let mut frame = session
        .begin_native_host_frame(|query| scene.answer(query))
        .unwrap();
    let successor = render(&mut frame, A);
    successor.assert_preview(false);
    let mut report = finish(frame);
    let terminal = assert_delivered(&report);
    assert_eq!(report.before(), version);
    assert_eq!(report.after(), session.version());
    assert_ne!(session.version(), version);
    assert_eq!(order(session), [SECOND, THIRD, FIRST]);
    let committed = session.version();
    let exact = take_output(&mut report, &successor, binding);
    let settled = present(session, exact);
    assert!(settled.manipulation_feedback().is_empty());
    assert!(!settled.workspace_changed());
    assert_eq!(session.version(), committed);
    assert_eq!(order(session), [SECOND, THIRD, FIRST]);
    assert_clean_idle(session);
    terminal.manipulation_id()
}

fn start_local_drag(
    session: &mut DockspaceSession,
    binding: NativeSurfaceBinding,
    scene: &Scene,
) -> NativeLocalPointerSample {
    let cap = session.native_local_coordinates(binding).unwrap();
    let source = sample(cap, binding, scene.tab(FIRST).receiver.center());
    let target = sample(cap, binding, scene.insertion(THIRD, false));
    session
        .record_native_pointer_batch([
            local(binding, Event::ButtonPressed(Button::Primary), source),
            local(binding, Event::Moved, target),
        ])
        .unwrap();
    let frame = session
        .begin_native_host_frame(|query| scene.answer(query))
        .unwrap();
    let report = finish(frame);
    assert!(!report.workspace_changed());
    assert!(report.manipulation_feedback().is_empty());
    assert!(session.has_active_gesture());
    target
}

#[test]
fn explicit_handoff_retries_the_release_frame_and_settles_without_redelivery() {
    let (mut session, bindings, [scene, _]) = fixture(true);
    let binding = bindings[0];
    let version = session.version();
    let presented = session.presented_surface(A).unwrap();
    let cap = session.native_local_coordinates(binding).unwrap();
    let recorded = session.native_input_recorded_count();
    session
        .record_native_pointer_batch(reorder_batch(&session, binding, &scene))
        .unwrap();
    assert_eq!(session.native_input_recorded_count(), recorded + 4);
    {
        let mut abandoned = session
            .begin_native_host_frame(|query| scene.answer(query))
            .unwrap();
        render(&mut abandoned, A).assert_preview(false);
        assert_eq!(
            abandoned.view().item(FIRST).unwrap().tabs().items(),
            [SECOND, THIRD, FIRST]
        );
    }
    assert_eq!(session.version(), version);
    assert_eq!(order(&session), [FIRST, SECOND, THIRD]);
    assert_eq!(session.presented_surface(A), Some(presented));
    assert_eq!(session.native_local_coordinates(binding), Some(cap));
    assert_eq!(session.native_input_recorded_count(), recorded + 4);
    assert!(session.has_pending_native_input());
    assert!(!session.has_active_gesture());

    let mut frame = session
        .begin_native_host_frame(|query| scene.answer(query))
        .unwrap();
    let successor = render(&mut frame, A);
    successor.assert_preview(false);
    let mut report = finish(frame);
    assert_delivered(&report);
    assert_eq!(report.before(), version);
    assert_eq!(report.after(), session.version());
    assert_ne!(session.version(), version);
    let committed = session.version();
    assert_eq!(order(&session), [SECOND, THIRD, FIRST]);
    assert_eq!(session.native_input_recorded_count(), recorded + 4);
    assert!(!session.has_pending_native_input());
    assert!(!session.has_active_gesture());
    assert!(idle(&mut session).manipulation_feedback().is_empty());

    let exact = take_output(&mut report, &successor, binding);
    let artifact = exact.presentation_artifact();
    session
        .report_surface_presentation(exact, Disposition::Presented)
        .unwrap();
    drop(
        session
            .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
            .unwrap(),
    );
    let presentation_recorded = session.native_input_recorded_count();
    assert!(session.has_pending_native_input());
    assert_eq!(session.version(), committed);
    assert_eq!(order(&session), [SECOND, THIRD, FIRST]);
    let settled = idle(&mut session);
    assert!(
        settled
            .surface_presentation_commits()
            .iter()
            .any(|ack| ack.settles(artifact))
    );
    assert!(settled.manipulation_feedback().is_empty());
    assert!(!settled.workspace_changed());
    assert_eq!(session.version(), committed);
    assert_eq!(session.native_input_recorded_count(), presentation_recorded);
    assert!(
        bindings
            .into_iter()
            .all(|binding| session.is_current_native_binding(binding))
    );
    assert_clean_idle(&mut session);
}

#[test]
fn inactive_tab_same_batch_handoff_preserves_the_actual_output_and_source_continuation() {
    let (mut session, bindings, [original, _]) = fixture(true);
    let binding = bindings[0];
    let cap = session.native_local_coordinates(binding).unwrap();
    let presented = session.presented_surface(A).unwrap();
    let source = sample(cap, binding, original.tab(SECOND).receiver.center());
    let target = sample(cap, binding, original.insertion(FIRST, true));
    assert!(session.view().item(FIRST).unwrap().is_selected());
    assert!(!session.view().item(SECOND).unwrap().is_selected());
    session
        .record_native_pointer_batch([
            local(binding, Event::ButtonPressed(Button::Primary), source),
            local(binding, Event::Moved, target),
            moved_handoff(binding, target),
        ])
        .unwrap();
    let mut actual_hover_queries = 0;
    let mut frame = session
        .begin_native_host_frame(|query| {
            let query = match query {
                NativeReceiverQuery::WindowHit(_) => return original.answer(query),
                NativeReceiverQuery::Surface(query) => query,
            };

            if matches!(query.purpose(), NativeReceiverPurpose::HoverHit) {
                actual_hover_queries += 1;
            }
            original.answer(query)
        })
        .unwrap();
    measure(&mut frame);
    let report = finish(frame);
    assert!(report.workspace_changed());
    assert!(report.manipulation_feedback().is_empty());
    assert!(actual_hover_queries > 0);
    assert!(session.has_active_gesture());
    assert!(session.view().item(SECOND).unwrap().is_selected());
    assert_eq!(order(&session), [FIRST, SECOND, THIRD]);
    assert_eq!(session.presented_surface(A), Some(presented));
    assert_eq!(session.native_local_coordinates(binding), Some(cap));
    let (current, fresh) = paint(&mut session, binding);
    assert_eq!(current.dragged_item, Some(SECOND));
    current.assert_preview(true);
    assert!(
        present(&mut session, fresh)
            .manipulation_feedback()
            .is_empty()
    );

    let version = session.version();
    session
        .record_native_pointer_batch([
            desktop(binding, Event::Moved, sample_logical(target)),
            desktop(
                binding,
                Event::ButtonReleased(Button::Primary),
                sample_logical(target),
            ),
        ])
        .unwrap();
    let mut fresh_hover_queries = 0;
    let mut frame = session
        .begin_native_host_frame(|query| {
            let query = match query {
                NativeReceiverQuery::WindowHit(_) => return current.answer(query),
                NativeReceiverQuery::Surface(query) => query,
            };

            if matches!(query.purpose(), NativeReceiverPurpose::HoverHit) {
                fresh_hover_queries += 1;
                assert_eq!(query.point(), Some(sample_logical(target)));
                assert!(
                    query
                        .clear_hover_lane(&original.tab(FIRST).receiver)
                        .is_none()
                );
                assert!(query.bind_receiver(&original.tab(FIRST).receiver).is_none());
            }
            current.answer(query)
        })
        .unwrap();
    let successor = render(&mut frame, A);
    successor.assert_preview(false);
    let mut report = finish(frame);
    assert!(fresh_hover_queries > 0);
    assert_delivered(&report);
    assert_ne!(session.version(), version);
    assert_eq!(order(&session), [SECOND, FIRST, THIRD]);
    let committed = session.version();
    let exact = take_output(&mut report, &successor, binding);
    let settled = present(&mut session, exact);
    assert!(settled.manipulation_feedback().is_empty());
    assert!(!settled.workspace_changed());
    assert_eq!(session.version(), committed);
    assert!(session.view().item(SECOND).unwrap().is_selected());
    assert_clean_idle(&mut session);
}

#[test]
fn dropped_desktop_handoff_output_preserves_delivery_and_the_pointer_can_start_again() {
    let (mut session, bindings, [scene, _]) = fixture(true);
    let binding = bindings[0];
    let version = session.version();
    session
        .record_native_pointer_batch(reorder_batch(&session, binding, &scene))
        .unwrap();
    let mut frame = session
        .begin_native_host_frame(|query| scene.answer(query))
        .unwrap();
    let successor = render(&mut frame, A);
    successor.assert_preview(false);
    let mut report = finish(frame);
    let terminal = assert_delivered(&report);
    assert_ne!(session.version(), version);
    let committed = session.version();
    assert_eq!(order(&session), [SECOND, THIRD, FIRST]);
    let exact = take_output(&mut report, &successor, binding);
    let artifact = exact.presentation_artifact();
    session
        .report_surface_presentation(exact, Disposition::Dropped)
        .unwrap();
    let dropped = idle(&mut session);
    assert!(
        dropped
            .surface_presentation_commits()
            .iter()
            .any(|commit| commit.retires(artifact))
    );
    assert!(dropped.manipulation_feedback().is_empty());
    assert!(!dropped.workspace_changed());
    assert_eq!(session.version(), committed);
    assert_eq!(order(&session), [SECOND, THIRD, FIRST]);
    assert_clean_idle(&mut session);

    let current = refresh(&mut session, bindings);
    let cap = session.native_local_coordinates(binding).unwrap();
    let source = sample(cap, binding, current.tab(FIRST).receiver.center());
    let target = sample(cap, binding, current.insertion(SECOND, true));
    session
        .record_native_pointer_batch([
            local(binding, Event::ButtonPressed(Button::Primary), source),
            local(binding, Event::Moved, target),
            moved_handoff(binding, target),
            desktop(
                binding,
                Event::ButtonReleased(Button::Primary),
                sample_logical(target),
            ),
        ])
        .unwrap();
    let frame = session
        .begin_native_host_frame(|query| current.answer(query))
        .unwrap();
    let next = assert_delivered(&finish(frame));
    assert_ne!(next.manipulation_id(), terminal.manipulation_id());
    assert_eq!(order(&session), [FIRST, SECOND, THIRD]);
    assert_clean_idle(&mut session);
}

#[test]
fn rejected_handoffs_drain_real_terminals_and_leave_the_same_native_pointer_reusable() {
    #[derive(Clone, Copy, Debug)]
    enum MissingProof {
        SamePoint,
        Origin,
        CaptureUnknown,
        CaptureNone,
        CaptureForeign,
        SamePointer,
        ExplicitPacket,
    }

    for missing in [
        MissingProof::SamePoint,
        MissingProof::Origin,
        MissingProof::CaptureUnknown,
        MissingProof::CaptureNone,
        MissingProof::CaptureForeign,
        MissingProof::SamePointer,
        MissingProof::ExplicitPacket,
    ] {
        let (mut session, bindings, [scene, _]) = fixture(!matches!(missing, MissingProof::Origin));
        let binding = bindings[0];
        let target = start_local_drag(&mut session, binding, &scene);
        let version = session.version();
        let recorded = session.native_input_recorded_count();
        let mut point = desktop_point(A, sample_logical(target));
        if matches!(missing, MissingProof::SamePoint) {
            point = PhysicalPoint::new(point.x() + 3.0, point.y()).unwrap();
        }
        let capture = match missing {
            MissingProof::CaptureUnknown => NativePointerOwner::Unknown,
            MissingProof::CaptureNone => NativePointerOwner::None,
            MissingProof::CaptureForeign => NativePointerOwner::Foreign,
            _ => NativePointerOwner::Native(binding.window()),
        };
        let input = if matches!(missing, MissingProof::ExplicitPacket) {
            desktop(binding, Event::Moved, sample_logical(target))
        } else {
            handoff(
                if matches!(missing, MissingProof::SamePointer) {
                    OTHER_POINTER
                } else {
                    POINTER
                },
                binding,
                Event::Moved,
                target,
                desktop_location(binding, point),
                capture,
            )
        };
        session.record_native_pointer(input).unwrap();
        let frame = session
            .begin_native_host_frame(|query| scene.answer(query))
            .unwrap();
        assert_no_delivery(&finish(frame));
        assert_eq!(session.version(), version, "{missing:?}");
        assert_eq!(order(&session), [FIRST, SECOND, THIRD], "{missing:?}");
        assert_eq!(session.native_input_recorded_count(), recorded + 1);

        if matches!(missing, MissingProof::SamePointer) {
            assert!(
                session.has_active_gesture(),
                "another pointer cannot replace the owner"
            );
            // This pointer never pressed a button. End its real buttonless
            // stream instead of inventing an unmatched release or checkpoint.
            session
                .record_native_pointer(handoff(
                    OTHER_POINTER,
                    binding,
                    Event::StreamEnded,
                    target,
                    desktop_location(binding, point),
                    NativePointerOwner::None,
                ))
                .unwrap();
            let frame = session
                .begin_native_host_frame(|query| scene.answer(query))
                .unwrap();
            assert_no_delivery(&finish(frame));
            assert!(session.has_active_gesture());
        }

        // An ordinary desktop release succeeds only after a real upgrade.
        // Rejected packets must consume this actual terminal and cancel the
        // original local gesture rather than silently changing its lane.
        session
            .record_native_pointer(desktop(
                binding,
                Event::ButtonReleased(Button::Primary),
                sample_logical(target),
            ))
            .unwrap();
        let frame = session
            .begin_native_host_frame(|query| scene.answer(query))
            .unwrap();
        assert_no_delivery(&finish(frame));
        assert_eq!(session.version(), version, "{missing:?}");
        assert_eq!(order(&session), [FIRST, SECOND, THIRD], "{missing:?}");
        assert_clean_idle(&mut session);
        assert!(session.is_current_native_binding(binding));

        let current = refresh(&mut session, bindings);
        deliver_reorder(&mut session, binding, &current);
    }
}

#[test]
fn rebased_and_foreign_local_samples_cannot_upgrade_or_publish_a_valid_prefix() {
    for adding_origin in [false, true] {
        let (mut session, bindings, [scene, _]) = fixture(!adding_origin);
        let binding = bindings[0];
        let old_cap = session.native_local_coordinates(binding).unwrap();
        let target = start_local_drag(&mut session, binding, &scene);
        let version = session.version();
        let new_bounds = if adding_origin {
            bounds(A)
        } else {
            PhysicalRect::new(420.0, -300.0, 1200.0, 900.0).unwrap()
        };
        snapshot(&mut session, bindings, Some(new_bounds));
        assert_eq!(session.native_local_coordinates(binding), Some(old_cap));
        let point = PhysicalPoint::new(
            new_bounds.x() + target.client_position().x(),
            new_bounds.y() + target.client_position().y(),
        )
        .unwrap();
        session
            .record_native_pointer_batch([
                handoff(
                    POINTER,
                    binding,
                    Event::Moved,
                    target,
                    desktop_location(binding, point),
                    NativePointerOwner::Native(binding.window()),
                ),
                NativePointerInput::new(
                    POINTER,
                    Event::ButtonReleased(Button::Primary),
                    desktop_location(binding, point),
                    NativePointerOwner::Native(binding.window()),
                    NativePointerOwner::None,
                ),
            ])
            .unwrap();
        let mut frame = session
            .begin_native_host_frame(|_| {
                panic!("a preceding coordinate rebase cannot lend G2 authority to a G1 handoff")
            })
            .unwrap();
        measure(&mut frame);
        assert_no_delivery(&finish(frame));
        assert_eq!(session.version(), version);
        assert_eq!(order(&session), [FIRST, SECOND, THIRD]);
        let current = session.native_local_coordinates(binding).unwrap();
        assert_ne!(current, old_cap);
        assert_eq!(current.geometry(), old_cap.geometry());
        assert_clean_idle(&mut session);
        let current = refresh(&mut session, bindings);
        deliver_reorder(&mut session, binding, &current);
    }

    let (mut session, bindings, [scene, _]) = fixture(true);
    let (foreign_session, foreign_bindings, [foreign_scene, _]) = fixture(true);
    let binding = bindings[0];
    let target = start_local_drag(&mut session, binding, &scene);
    let foreign_cap = foreign_session
        .native_local_coordinates(foreign_bindings[0])
        .unwrap();
    let foreign = sample(
        foreign_cap,
        foreign_bindings[0],
        foreign_scene.insertion(THIRD, false),
    );
    let version = session.version();
    let presented = session.presented_surface(A).unwrap();
    let recorded = session.native_input_recorded_count();
    let error = session
        .record_native_pointer_batch([
            moved_handoff(binding, target),
            handoff(
                POINTER,
                binding,
                Event::Moved,
                foreign,
                desktop_location(binding, desktop_point(A, sample_logical(target))),
                NativePointerOwner::Native(binding.window()),
            ),
        ])
        .unwrap_err();
    assert_eq!(error.native_kind(), Some(NativeHostErrorKind::InvalidFacts));
    assert_eq!(session.native_input_recorded_count(), recorded);
    assert_eq!(session.version(), version);
    assert_eq!(order(&session), [FIRST, SECOND, THIRD]);
    assert_eq!(session.presented_surface(A), Some(presented));
    assert!(!session.has_pending_native_input());
    assert!(session.has_active_gesture());
    // The valid first packet was not appended. The still-local owner must
    // reject an implicit upgrade by this ordinary desktop terminal.
    session
        .record_native_pointer(desktop(
            binding,
            Event::ButtonReleased(Button::Primary),
            sample_logical(target),
        ))
        .unwrap();
    let frame = session
        .begin_native_host_frame(|query| scene.answer(query))
        .unwrap();
    assert_no_delivery(&finish(frame));
    assert_clean_idle(&mut session);
    let current = refresh(&mut session, bindings);
    deliver_reorder(&mut session, binding, &current);
}

#[test]
fn late_local_preview_after_handoff_cannot_replace_or_redeliver_the_accepted_release() {
    for same_target in [false, true] {
        let (mut session, bindings, [scene, _]) = fixture(true);
        let binding = bindings[0];
        let cap = session.native_local_coordinates(binding).unwrap();
        let source = sample(cap, binding, scene.tab(FIRST).receiver.center());
        let local_target = sample(cap, binding, scene.insertion(THIRD, !same_target));
        let desktop_target = sample(cap, binding, scene.insertion(THIRD, false));
        assert_eq!(
            sample_logical(local_target) == sample_logical(desktop_target),
            same_target
        );
        session
            .record_native_pointer_batch([
                local(binding, Event::ButtonPressed(Button::Primary), source),
                local(binding, Event::Moved, local_target),
            ])
            .unwrap();
        let mut frame = session
            .begin_native_host_frame(|query| scene.answer(query))
            .unwrap();
        let local_preview = render(&mut frame, A);
        local_preview.assert_preview(true);
        let mut report = finish(frame);
        assert!(report.manipulation_feedback().is_empty());
        let old_output = take_output(&mut report, &local_preview, binding);
        let old_artifact = old_output.presentation_artifact();
        assert!(session.has_active_gesture());
        let version = session.version();

        session
            .record_native_pointer_batch([
                moved_handoff(binding, desktop_target),
                // A packet on the already-desktop owner is an ordinary desktop
                // observation; it neither upgrades twice nor returns to local.
                moved_handoff(binding, desktop_target),
                desktop(
                    binding,
                    Event::ButtonReleased(Button::Primary),
                    sample_logical(desktop_target),
                ),
            ])
            .unwrap();
        let mut frame = session
            .begin_native_host_frame(|query| scene.answer(query))
            .unwrap();
        let successor = render(&mut frame, A);
        successor.assert_preview(false);
        let mut report = finish(frame);
        assert_delivered(&report);
        assert_ne!(session.version(), version);
        let committed = session.version();
        let exact = take_output(&mut report, &successor, binding);
        let exact_artifact = exact.presentation_artifact();
        assert_ne!(old_artifact, exact_artifact);
        assert_eq!(session.version(), committed);
        assert_eq!(order(&session), [SECOND, THIRD, FIRST]);

        let late = present(&mut session, old_output);
        assert!(
            late.surface_presentation_commits()
                .iter()
                .any(|ack| ack.settles(old_artifact))
        );
        assert!(
            !late
                .surface_presentation_commits()
                .iter()
                .any(|ack| ack.settles(exact_artifact))
        );
        assert!(late.manipulation_feedback().is_empty());
        assert!(!late.workspace_changed());
        assert_eq!(session.version(), committed);
        assert_eq!(order(&session), [SECOND, THIRD, FIRST]);

        let settled = present(&mut session, exact);
        assert!(
            settled
                .surface_presentation_commits()
                .iter()
                .any(|ack| ack.settles(exact_artifact))
        );
        assert!(settled.manipulation_feedback().is_empty());
        assert!(!settled.workspace_changed());
        assert_eq!(session.version(), committed);
        assert_eq!(order(&session), [SECOND, THIRD, FIRST]);
        assert_clean_idle(&mut session);
    }
}

#[test]
fn wrong_winner_suffix_rolls_back_handoff_and_retries_the_original_batch_once() {
    let (mut session, bindings, [scene, _]) = fixture(true);
    let binding = bindings[0];
    let target = start_local_drag(&mut session, binding, &scene);
    let version = session.version();
    let cap = session.native_local_coordinates(binding).unwrap();
    let presented = session.presented_surface(A).unwrap();
    let recorded = session.native_input_recorded_count();
    session
        .record_native_pointer_batch([
            moved_handoff(binding, target),
            desktop(
                binding,
                Event::ButtonReleased(Button::Primary),
                sample_logical(target),
            ),
        ])
        .unwrap();
    let accepted = session.native_input_recorded_count();
    assert_eq!(accepted, recorded + 2);
    let mut hover_queries = 0;
    assert!(
        session
            .begin_native_host_frame(|query| {
                let query = match query {
                    NativeReceiverQuery::WindowHit(_) => return scene.answer(query),
                    NativeReceiverQuery::Surface(query) => query,
                };

                if matches!(query.purpose(), NativeReceiverPurpose::HoverHit) {
                    hover_queries += 1;
                    assert_eq!(query.point(), Some(sample_logical(target)));
                    assert!(query.matches_native_binding(binding));
                    if hover_queries == 2 {
                        // This is the correct exact output, but its real tab
                        // receiver contradicts a claim that there is no winner.
                        return NativeReceiverAnswer::NoReceiver(query.presented_surface());
                    }
                }
                scene.answer(query)
            })
            .is_err(),
    );
    assert_eq!(
        hover_queries, 2,
        "the valid handoff prefix must precede the invalid release"
    );
    assert_eq!(session.version(), version);
    assert_eq!(order(&session), [FIRST, SECOND, THIRD]);
    assert!(session.view().item(FIRST).unwrap().is_selected());
    assert_eq!(session.native_local_coordinates(binding), Some(cap));
    assert_eq!(session.presented_surface(A), Some(presented));
    assert_eq!(session.native_input_recorded_count(), accepted);
    assert!(session.has_pending_native_input());
    assert!(session.has_active_gesture());

    let mut retry_hover_queries = 0;
    let mut frame = session
        .begin_native_host_frame(|query| {
            let query = match query {
                NativeReceiverQuery::WindowHit(_) => return scene.answer(query),
                NativeReceiverQuery::Surface(query) => query,
            };

            if matches!(query.purpose(), NativeReceiverPurpose::HoverHit) {
                retry_hover_queries += 1;
                assert_eq!(query.point(), Some(sample_logical(target)));
            }
            scene.answer(query)
        })
        .unwrap();
    let successor = render(&mut frame, A);
    successor.assert_preview(false);
    let mut report = finish(frame);
    assert_eq!(retry_hover_queries, 2);
    assert_delivered(&report);
    assert_ne!(session.version(), version);
    let committed = session.version();
    assert_eq!(order(&session), [SECOND, THIRD, FIRST]);
    assert_eq!(session.native_input_recorded_count(), accepted);
    assert!(!session.has_pending_native_input());
    assert!(!session.has_active_gesture());
    let exact = take_output(&mut report, &successor, binding);
    let settled = present(&mut session, exact);
    assert!(settled.manipulation_feedback().is_empty());
    assert!(!settled.workspace_changed());
    assert_eq!(session.version(), committed);
    assert_eq!(session.native_input_recorded_count(), accepted + 1);
    assert_eq!(order(&session), [SECOND, THIRD, FIRST]);
    assert_clean_idle(&mut session);
}

#[test]
fn handoff_queries_b_at_its_desktop_hover_point_but_missing_hit_observation_cannot_authorize_drop()
{
    let (mut session, bindings, [source_scene, target_scene]) = fixture_with_routing(true, true);
    let source = bindings[0];
    let destination = bindings[1];
    start_local_drag(&mut session, source, &source_scene);
    let version = session.version();
    let hover_point = target_scene.insertion(OTHER, false);
    let desktop = desktop_point(B, hover_point);
    let source_client =
        PhysicalPoint::new(desktop.x() - bounds(A).x(), desktop.y() - bounds(A).y()).unwrap();
    let manipulation = capture_client(
        session.native_local_coordinates(source).unwrap(),
        source,
        source_client,
    );
    assert_ne!(sample_logical(manipulation), hover_point);
    assert!(manipulation.client_position().x() > geometry().content_size().width());
    session
        .record_native_pointer(handoff(
            POINTER,
            source,
            Event::Moved,
            manipulation,
            desktop_location(destination, desktop),
            NativePointerOwner::Native(source.window()),
        ))
        .unwrap();
    let mut b_hover_queries = 0;
    let mut frame = session
        .begin_native_host_frame(|query| {
            let query = match query {
                NativeReceiverQuery::WindowHit(_) => return target_scene.answer(query),
                NativeReceiverQuery::Surface(query) => query,
            };

            assert!(matches!(query.purpose(), NativeReceiverPurpose::HoverHit));
            assert_eq!(query.surface(), B);
            assert!(query.matches_native_binding(destination));
            assert_eq!(query.point(), Some(hover_point));
            assert_ne!(query.point(), Some(sample_logical(manipulation)));
            b_hover_queries += 1;
            // Exact native hover and hit-test control cannot supply the missing
            // independent observation of the rendered receiver at this edge.
            NativeReceiverAnswer::Unknown
        })
        .unwrap();
    let target = render(&mut frame, B);
    target.assert_preview(false);
    let mut report = finish(frame);
    assert!(b_hover_queries > 0);
    assert_no_delivery(&report);
    assert!(report.take_native_effects().is_empty());
    assert!(session.has_active_gesture());
    assert_eq!(session.version(), version);
    let target_output = take_output(&mut report, &target, destination);
    assert!(
        present(&mut session, target_output)
            .manipulation_feedback()
            .is_empty()
    );

    // Presenting a real output cannot supply an unobserved release-time hit.
    session
        .record_native_pointer(NativePointerInput::new(
            POINTER,
            Event::ButtonReleased(Button::Primary),
            desktop_location(destination, desktop),
            NativePointerOwner::Native(source.window()),
            NativePointerOwner::None,
        ))
        .unwrap();
    let frame = session
        .begin_native_host_frame(|query| {
            let query = match query {
                NativeReceiverQuery::WindowHit(_) => return target.answer(query),
                NativeReceiverQuery::Surface(query) => query,
            };

            assert_eq!(query.surface(), B);
            assert!(query.matches_native_binding(destination));
            assert_eq!(query.point(), Some(hover_point));
            NativeReceiverAnswer::Unknown
        })
        .unwrap();
    let mut report = finish(frame);
    assert_no_delivery(&report);
    assert!(report.take_native_effects().is_empty());
    assert_eq!(session.version(), version);
    assert_eq!(order(&session), [FIRST, SECOND, THIRD]);
    assert_eq!(session.view().item(FIRST).unwrap().surface(), A);
    assert_eq!(session.view().item(OTHER).unwrap().surface(), B);
    assert_clean_idle(&mut session);
    let current = refresh(&mut session, bindings);
    deliver_reorder(&mut session, source, &current);
}

#[test]
fn cross_window_handoff_preserves_root_input_and_delivers_before_the_target_output() {
    for with_hit_test_control in [false, true] {
        let (mut session, bindings, [source_scene, target_scene]) =
            fixture_with_routing(true, with_hit_test_control);
        let [source, destination] = bindings;
        start_local_drag(&mut session, source, &source_scene);
        let version = session.version();
        let cap = session.native_local_coordinates(source).unwrap();
        let point = target_scene.insertion(OTHER, true);
        let desktop_point = desktop_point(B, point);
        let source_client = PhysicalPoint::new(
            desktop_point.x() - bounds(A).x(),
            desktop_point.y() - bounds(A).y(),
        )
        .unwrap();
        let local = capture_client(cap, source, source_client);
        assert_ne!(sample_logical(local), point);
        session
            .record_native_pointer(handoff(
                POINTER,
                source,
                Event::Moved,
                local,
                desktop_location(destination, desktop_point),
                NativePointerOwner::Native(source.window()),
            ))
            .unwrap();
        let mut frame = session
            .begin_native_host_frame(|query| {
                let query = match query {
                    NativeReceiverQuery::WindowHit(_) => return target_scene.answer(query),
                    NativeReceiverQuery::Surface(query) => query,
                };

                assert_eq!(query.surface(), B);
                assert!(query.matches_native_binding(destination));
                assert_eq!(query.point(), Some(point));
                target_scene.answer(query)
            })
            .unwrap();
        let target = render(&mut frame, B);
        target.assert_preview(true);
        let mut report = finish(frame);
        assert_no_delivery(&report);
        assert!(report.take_native_effects().is_empty());
        assert!(session.has_active_gesture());
        let target_output = take_output(&mut report, &target, destination);
        let mut presented = present(&mut session, target_output);
        assert_no_delivery(&presented);
        assert!(presented.take_native_effects().is_empty());
        assert_eq!(session.version(), version);
        assert_eq!(session.native_local_coordinates(source), Some(cap));

        // Keep a real pre-release output in flight. Its late settlement must
        // remain separate from the successor and cannot undo the accepted edit.
        let (old_preview, old_output) = paint(&mut session, destination);
        old_preview.assert_preview(true);
        let old_artifact = old_output.presentation_artifact();
        let release_point = target.insertion(OTHER, false);
        let release_desktop = self::desktop_point(B, release_point);
        assert_ne!(release_point, point);
        session
            .record_native_pointer_batch(
                [Event::Moved, Event::ButtonReleased(Button::Primary)].map(|event| {
                    NativePointerInput::new(
                        POINTER,
                        event,
                        desktop_location(destination, release_desktop),
                        NativePointerOwner::Native(source.window()),
                        capture_for(source, event),
                    )
                }),
            )
            .unwrap();
        let mut frame = session
            .begin_native_host_frame(|query| {
                let query = match query {
                    NativeReceiverQuery::WindowHit(_) => return target.answer(query),
                    NativeReceiverQuery::Surface(query) => query,
                };

                assert_eq!(query.surface(), B);
                assert!(query.matches_native_binding(destination));
                assert_eq!(query.point(), Some(release_point));
                assert_ne!(query.point(), Some(sample_logical(local)));
                target.answer(query)
            })
            .unwrap();
        let successor = render(&mut frame, B);
        successor.assert_preview(false);
        let mut report = finish(frame);
        assert_delivered(&report);
        assert_eq!(report.before(), version);
        assert_eq!(report.after(), session.version());
        assert_ne!(session.version(), version);
        let committed = session.version();
        assert!(report.take_native_effects().is_empty());
        assert!(!session.has_active_gesture());
        assert!(!session.has_pending_native_input());
        let exact = take_output(&mut report, &successor, destination);
        let exact_artifact = exact.presentation_artifact();
        assert_ne!(old_artifact, exact_artifact);
        assert_no_delivery(&idle(&mut session));
        assert_eq!(session.view().item(FIRST).unwrap().surface(), B);
        assert_eq!(session.version(), committed);

        let mut late = present(&mut session, old_output);
        assert!(
            late.surface_presentation_commits()
                .iter()
                .any(|ack| ack.settles(old_artifact))
        );
        assert!(
            !late
                .surface_presentation_commits()
                .iter()
                .any(|ack| ack.settles(exact_artifact))
        );
        assert_no_delivery(&late);
        assert!(late.manipulation_feedback().is_empty());
        assert!(late.take_native_effects().is_empty());
        assert_eq!(session.version(), committed);
        assert_eq!(order(&session), [OTHER, FIRST]);
        assert_eq!(session.view().item(FIRST).unwrap().surface(), B);
        assert_eq!(session.view().item(OTHER).unwrap().surface(), B);

        let mut settled = present(&mut session, exact);
        assert!(settled.manipulation_feedback().is_empty());
        assert!(!settled.workspace_changed());
        assert_eq!(session.version(), committed);
        assert!(settled.take_native_effects().is_empty());
        assert_eq!(session.view().item(FIRST).unwrap().surface(), B);
        assert_eq!(
            session.view().item(OTHER).unwrap().tabs().items(),
            [OTHER, FIRST]
        );
        assert_eq!(
            session.view().item(SECOND).unwrap().tabs().items(),
            [SECOND, THIRD]
        );
        assert!(
            bindings
                .into_iter()
                .all(|binding| session.is_current_native_binding(binding))
        );
        assert_clean_idle(&mut session);
    }
}

#[test]
fn stationary_hover_after_handoff_keeps_source_binding_and_requires_confirmed_cross_window_route() {
    for with_hit_test_control in [false, true] {
        let (mut session, bindings, [source_scene, target_scene]) =
            fixture_with_destination(true, with_hit_test_control, [OTHER, FIFTH]);
        let [source, destination] = bindings;
        start_local_drag(&mut session, source, &source_scene);
        let version = session.version();
        let hover = target_scene.insertion(FIFTH, false);
        let desktop = desktop_point(B, hover);
        let manipulation = capture_client(
            session.native_local_coordinates(source).unwrap(),
            source,
            PhysicalPoint::new(desktop.x() - bounds(A).x(), desktop.y() - bounds(A).y()).unwrap(),
        );
        assert_ne!(sample_logical(manipulation), hover);
        let metadata = |millis| {
            PointerEventMetadata::new(PointerDeviceKind::Mouse, Duration::from_millis(millis))
        };
        session
            .record_native_pointer(
                handoff(
                    POINTER,
                    source,
                    Event::Moved,
                    manipulation,
                    desktop_location(destination, desktop),
                    NativePointerOwner::Native(source.window()),
                )
                .with_metadata(metadata(10)),
            )
            .unwrap();
        let mut report = finish(
            session
                .begin_native_host_frame(|query| {
                    let query = match query {
                        NativeReceiverQuery::WindowHit(_) => return target_scene.answer(query),
                        NativeReceiverQuery::Surface(query) => query,
                    };

                    assert_eq!(query.surface(), B);
                    assert_eq!(query.point(), Some(hover));
                    target_scene.answer(query)
                })
                .unwrap(),
        );
        assert!(!report.workspace_changed());
        let initial_request = session
            .native_pointer_continuation()
            .expect("the preserving source route is current with either control roster");
        assert!(report.take_native_effects().is_empty());
        assert_eq!(initial_request.pointer(), POINTER);
        assert_eq!(initial_request.source_binding(), source);
        assert_eq!(initial_request.surface(), B);
        assert_eq!(initial_request.position(), hover);
        assert!(initial_request.local_coordinates().is_none());
        assert_eq!(initial_request.deadline(), Duration::from_millis(160));
        let observed = |event, millis| {
            NativePointerInput::new(
                POINTER,
                event,
                desktop_location(destination, desktop),
                NativePointerOwner::Native(source.window()),
                NativePointerOwner::Native(source.window()),
            )
            .with_metadata(metadata(millis))
        };
        session
            .record_native_pointer(observed(Event::Moved, 20))
            .unwrap();
        let mut report = finish(
            session
                .begin_native_host_frame(|query| {
                    let query = match query {
                        NativeReceiverQuery::WindowHit(_) => return target_scene.answer(query),
                        NativeReceiverQuery::Surface(query) => query,
                    };

                    assert_eq!(query.surface(), B);
                    assert_eq!(query.point(), Some(hover));
                    target_scene.answer(query)
                })
                .unwrap(),
        );
        assert!(!report.workspace_changed());
        assert!(report.take_native_effects().is_empty());
        assert_eq!(session.version(), version);
        let request = session
            .native_pointer_continuation()
            .expect("confirmed preserving routing must schedule the destination dwell");
        assert_eq!(request.pointer(), POINTER);
        assert_eq!(request.source_binding(), source);
        assert_eq!(request.surface(), B);
        assert_eq!(request.position(), hover);
        assert!(request.local_coordinates().is_none());
        assert_eq!(request.deadline(), initial_request.deadline());
        assert_ne!(request.token(), initial_request.token());
        session
            .record_native_pointer(observed(request.event(), 160))
            .unwrap();
        let mut frame = session
            .begin_native_host_frame(|query| {
                let query = match query {
                    NativeReceiverQuery::WindowHit(_) => return target_scene.answer(query),
                    NativeReceiverQuery::Surface(query) => query,
                };

                assert!(query.matches_native_binding(destination));
                assert_eq!(query.point(), Some(hover));
                assert_ne!(query.point(), Some(sample_logical(manipulation)));
                target_scene.answer(query)
            })
            .unwrap();
        measure(&mut frame);
        let mut report = finish(frame);
        assert!(report.workspace_changed());
        assert_eq!(report.before(), version);
        assert_eq!(report.after(), session.version());
        assert_ne!(session.version(), version);
        assert!(report.take_native_effects().is_empty());
        assert!(session.view().item(FIFTH).unwrap().is_selected());
        assert!(!session.view().item(OTHER).unwrap().is_selected());
        assert_eq!(session.view().item(FIRST).unwrap().surface(), A);
        assert!(session.has_active_gesture());
        assert!(session.native_pointer_continuation().is_none());
        let (fresh, output) = paint(&mut session, destination);
        present(&mut session, output);
        session
            .record_native_pointer(
                NativePointerInput::new(
                    POINTER,
                    Event::ContactEnded(Button::Primary),
                    desktop_location(destination, desktop),
                    NativePointerOwner::Native(source.window()),
                    NativePointerOwner::None,
                )
                .with_metadata(metadata(180)),
            )
            .unwrap();
        let mut report = finish(
            session
                .begin_native_host_frame(|query| {
                    let query = match query {
                        NativeReceiverQuery::WindowHit(_) => return fresh.answer(query),
                        NativeReceiverQuery::Surface(query) => query,
                    };

                    assert!(
                        query
                            .bind_receiver(&target_scene.tab(FIFTH).receiver)
                            .is_none()
                    );
                    NativeReceiverAnswer::Blocked(query.presented_surface())
                })
                .unwrap(),
        );
        assert!(!report.workspace_changed());
        assert!(!session.has_active_gesture());
        assert!(report.take_native_effects().is_empty());
        idle(&mut session);
        assert_eq!(fresh.surface, B);
        assert!(
            bindings
                .into_iter()
                .all(|binding| session.is_current_native_binding(binding))
        );
        assert_clean_idle(&mut session);
    }
}

#[test]
fn rejected_mapping_preserves_the_real_local_move_and_release() {
    let (mut session, bindings, [scene, _]) = fixture(true);
    let binding = bindings[0];
    let target = start_local_drag(&mut session, binding, &scene);
    let version = session.version();
    let mismatch = desktop_point(A, sample_logical(target));
    let mismatch = PhysicalPoint::new(mismatch.x() + 1.0, mismatch.y()).unwrap();
    session
        .record_native_pointer_batch([
            handoff(
                POINTER,
                binding,
                Event::Moved,
                target,
                desktop_location(binding, mismatch),
                NativePointerOwner::Native(binding.window()),
            ),
            local(binding, Event::ButtonReleased(Button::Primary), target),
        ])
        .unwrap();
    let mut frame = session
        .begin_native_host_frame(|query| {
            let query = match query {
                NativeReceiverQuery::WindowHit(_) => return scene.answer(query),
                NativeReceiverQuery::Surface(query) => query,
            };

            assert_eq!(query.point(), Some(sample_logical(target)));
            scene.answer(query)
        })
        .unwrap();
    let successor = render(&mut frame, A);
    successor.assert_preview(false);
    let mut report = finish(frame);
    assert_delivered(&report);
    assert_ne!(session.version(), version);
    assert_eq!(order(&session), [SECOND, THIRD, FIRST]);
    let committed = session.version();
    let exact = take_output(&mut report, &successor, binding);
    let settled = present(&mut session, exact);
    assert!(settled.manipulation_feedback().is_empty());
    assert!(!settled.workspace_changed());
    assert_eq!(session.version(), committed);
    assert_eq!(order(&session), [SECOND, THIRD, FIRST]);
    assert_clean_idle(&mut session);
}

#[test]
fn armed_and_terminal_handoff_packets_preserve_the_original_local_event() {
    for terminal_packet in [false, true] {
        let (mut session, bindings, [scene, _]) = fixture(true);
        let binding = bindings[0];
        let cap = session.native_local_coordinates(binding).unwrap();
        let source = sample(cap, binding, scene.tab(FIRST).receiver.center());
        let target = sample(cap, binding, scene.insertion(THIRD, false));
        let unrelated_desktop = NativeDesktopPointerLocation::new(
            NativeDesktopPosition::Unknown,
            NativePointerHover::Unknown,
            None,
        );
        let mut inputs = vec![local(
            binding,
            Event::ButtonPressed(Button::Primary),
            source,
        )];
        if !terminal_packet {
            inputs.push(handoff(
                POINTER,
                binding,
                Event::Moved,
                source,
                unrelated_desktop,
                NativePointerOwner::Native(binding.window()),
            ));
        }
        inputs.push(local(binding, Event::Moved, target));
        inputs.push(if terminal_packet {
            handoff(
                POINTER,
                binding,
                Event::ButtonReleased(Button::Primary),
                target,
                unrelated_desktop,
                NativePointerOwner::None,
            )
        } else {
            local(binding, Event::ButtonReleased(Button::Primary), target)
        });
        session.record_native_pointer_batch(inputs).unwrap();
        let mut frame = session
            .begin_native_host_frame(|query| {
                let query = match query {
                    NativeReceiverQuery::WindowHit(_) => return scene.answer(query),
                    NativeReceiverQuery::Surface(query) => query,
                };

                assert_eq!(query.surface(), A);
                scene.answer(query)
            })
            .unwrap();
        let successor = render(&mut frame, A);
        successor.assert_preview(false);
        let mut report = finish(frame);
        assert_delivered(&report);
        assert_eq!(order(&session), [SECOND, THIRD, FIRST]);
        assert!(!session.has_active_gesture());
        assert!(!session.has_pending_native_input());
        let committed = session.version();
        let exact = take_output(&mut report, &successor, binding);
        let settled = present(&mut session, exact);
        assert!(settled.manipulation_feedback().is_empty());
        assert!(!settled.workspace_changed());
        assert_eq!(session.version(), committed);
        assert_eq!(order(&session), [SECOND, THIRD, FIRST]);
        assert_clean_idle(&mut session);
    }
}
