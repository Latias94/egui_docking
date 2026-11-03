//! Joined native input uses committed client geometry without desktop placement.

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
    HostWindowToken, NativeCreateStrategy, NativeDesktopPointerLocation, NativeDesktopPosition,
    NativeHostCapabilities, NativeHostCapability, NativeHostErrorKind, NativeLocalCoordinateError,
    NativeLocalCoordinates, NativeLocalGeometry, NativeLocalPointerFact,
    NativeLocalPointerLocation, NativeLocalPointerSample, NativePointerButton as Button,
    NativePointerEvent as Event, NativePointerHover, NativePointerId, NativePointerInput,
    NativePointerOwner, NativePointerRoster, NativeReceiverAnswer, NativeReceiverPurpose,
    NativeReceiverQuery, NativeSurfaceBinding, NativeWindowFacts, NativeWindowInputState,
    NativeWindowPresentationState, NativeWorkAreaRoster, PaintedSurfaceOutput,
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
const POINTER: NativePointerId = NativePointerId::new(17);

fn size(width: f64, height: f64) -> PhysicalSize {
    PhysicalSize::new(width, height).unwrap()
}

fn scale(value: f64) -> ScaleFactor {
    ScaleFactor::new(value).unwrap()
}

fn geometry() -> NativeLocalGeometry {
    NativeLocalGeometry::new(size(1200.0, 900.0), scale(2.0), scale(1.5))
}

fn changed_geometry() -> NativeLocalGeometry {
    NativeLocalGeometry::new(size(1200.0, 900.0), scale(2.0), scale(2.0))
}

fn facts(geometry: NativeLocalGeometry) -> NativeWindowFacts {
    // Neither an origin, outer bounds, work area nor desktop hover is supplied.
    NativeWindowFacts::live()
        .with_content_size(geometry.content_size())
        .with_native_scale_factor(geometry.native_scale_factor())
        .with_presentation_scale_factor(geometry.presentation_scale_factor())
        .with_input(NativeWindowInputState::ReceivesInput, None)
        .with_presentation(NativeWindowPresentationState::Visible, None)
}

fn finish(mut frame: DockspaceHostFrame<'_>) -> HostFrameReport {
    assert_eq!(frame.native_staging_paints().len(), 0);
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

fn registered() -> (DockspaceSession, [NativeSurfaceBinding; 2]) {
    registered_with_desktop(false)
}

fn registered_with_desktop(desktop: bool) -> (DockspaceSession, [NativeSurfaceBinding; 2]) {
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
            DockspaceRootLayout::new(RootId::new(2), DockspaceNode::central_tabs([OTHER])),
        ),
    ])
    .unwrap();
    let mut policy = DockPolicy::default();
    policy.set_allow_native_surfaces(true);
    let mut session = DockspaceSession::from_layout(layout, policy).unwrap();
    session
        .enable_managed_native_host(
            NativeCreateStrategy::DirectPrepared,
            NativePointerRoster::Exact(Vec::new()),
        )
        .unwrap();
    let mut capabilities = NativeHostCapabilities::none_supported()
        .with(NativeHostCapability::AuthoritativeInventory)
        .with(NativeHostCapability::AuthoritativeButtonState)
        .with(NativeHostCapability::PointerHitTestObservation);
    if desktop {
        capabilities = capabilities
            .with(NativeHostCapability::DesktopPointerPosition)
            .with(NativeHostCapability::HoveredWindow);
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
    (session, bindings)
}

fn snapshot(
    session: &mut DockspaceSession,
    bindings: [NativeSurfaceBinding; 2],
    a_geometry: NativeLocalGeometry,
) {
    session
        .report_managed_native_snapshot(
            [
                (bindings[0], facts(a_geometry)),
                (bindings[1], facts(geometry())),
            ],
            NativeWorkAreaRoster::Unknown,
        )
        .unwrap();
}

fn measure(frame: &mut DockspaceHostFrame<'_>, a_geometry: NativeLocalGeometry) {
    for (surface, geometry) in [(A, a_geometry), (B, geometry())] {
        let size = geometry.content_size();
        let scale = geometry.presentation_scale_factor().get();
        frame
            .measure_surface(
                surface,
                UniformSurfaceMetrics::new(
                    LogicalRect::new(0.0, 0.0, size.width() / scale, size.height() / scale)
                        .unwrap(),
                    LogicalSize::new(32.0, 24.0).unwrap(),
                    96.0,
                )
                .unwrap(),
            )
            .unwrap();
    }
}

struct RecordedTab {
    item: ItemId,
    receiver: DockspaceReceiverDescriptor,
    visible: LogicalRect,
}

struct Scene {
    surface: SurfaceId,
    recording: RecordingRenderer,
    tabs: Vec<RecordedTab>,
    dragged_item: Option<ItemId>,
}

impl Scene {
    fn tab(&self, item: ItemId) -> &RecordedTab {
        self.tabs.iter().find(|tab| tab.item == item).unwrap()
    }

    fn insertion(&self, item: ItemId, before: bool) -> LogicalPoint {
        let tab = self.tab(item);
        // These points are in real tab halves, away from guides and close chrome.
        let point = LogicalPoint::new(
            tab.visible.x() + tab.visible.width() * if before { 0.25 } else { 0.625 },
            tab.visible.y() + tab.visible.height() * 0.5,
        )
        .unwrap();
        assert!(tab.receiver.bounds().contains(point));
        assert_ne!(point, tab.receiver.center());
        point
    }

    fn answer(&self, query: NativeReceiverQuery) -> NativeReceiverAnswer {
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
            // The renderer registered this unobstructed tab. Core resolves the
            // source-suppressed insertion winner, rather than accepting a made-up target.
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
        if !expected {
            assert!(
                self.recording
                    .records
                    .iter()
                    .all(|record| !matches!(record, OwnedRecord::Preview(_)))
            );
            return;
        }
        assert_eq!(
            self.recording.records.iter().any(|record| matches!(
                record,
                OwnedRecord::Preview(DockspacePreviewVisual::Dock { surface, .. })
                    if *surface == self.surface
            )),
            expected,
        );
    }
}

fn render(
    frame: &mut DockspaceHostFrame<'_>,
    surface: SurfaceId,
    geometry: NativeLocalGeometry,
) -> Scene {
    if matches!(
        frame.surface_work(surface).unwrap(),
        SurfaceWork::Deferred(SurfaceWorkDeferred::Unmeasured)
    ) {
        let size = geometry.content_size();
        let scale = geometry.presentation_scale_factor().get();
        frame
            .measure_surface(
                surface,
                UniformSurfaceMetrics::new(
                    LogicalRect::new(0.0, 0.0, size.width() / scale, size.height() / scale)
                        .unwrap(),
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
                    tabs.push(RecordedTab {
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
        surface,
        recording,
        tabs,
        dragged_item,
    }
}

fn output(
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
    output
}

fn paint(
    session: &mut DockspaceSession,
    binding: NativeSurfaceBinding,
    geometry: NativeLocalGeometry,
) -> (Scene, PaintedSurfaceOutput) {
    let mut frame = session
        .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
        .unwrap();
    let scene = render(&mut frame, binding.surface(), geometry);
    let mut report = finish(frame);
    let output = output(&mut report, &scene, binding);
    (scene, output)
}

fn present(
    session: &mut DockspaceSession,
    output: PaintedSurfaceOutput,
    disposition: Disposition,
) -> HostFrameReport {
    let artifact = output.presentation_artifact();
    let version = session.version();
    let order_before = order(session);
    session
        .report_surface_presentation(output, disposition)
        .unwrap();
    assert_eq!(session.version(), version);
    assert_eq!(order(session), order_before);
    let report = idle(session);
    let settled = report
        .surface_presentation_commits()
        .iter()
        .filter(|ack| ack.settles(artifact))
        .collect::<Vec<_>>();
    assert_eq!(settled.len(), 1, "one exact output must settle once");
    if disposition == Disposition::Dropped {
        assert!(settled[0].retires(artifact));
    }
    report
}

fn fixture() -> (DockspaceSession, [NativeSurfaceBinding; 2], Scene) {
    let (mut session, bindings) = registered();
    snapshot(&mut session, bindings, geometry());
    let mut frame = session
        .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
        .unwrap();
    measure(&mut frame, geometry());
    assert!(!finish(frame).workspace_changed());
    let (scene, output) = paint(&mut session, bindings[0], geometry());
    assert!(!present(&mut session, output, Disposition::Presented).workspace_changed());
    let (_, output) = paint(&mut session, bindings[1], geometry());
    assert!(!present(&mut session, output, Disposition::Presented).workspace_changed());
    for binding in bindings {
        assert!(session.native_local_coordinates(binding).is_some());
    }
    (session, bindings, scene)
}

fn order(session: &DockspaceSession) -> Vec<ItemId> {
    session.view().item(FIRST).unwrap().tabs().items().to_vec()
}

fn sample(
    cap: NativeLocalCoordinates,
    binding: NativeSurfaceBinding,
    geometry: NativeLocalGeometry,
    point: LogicalPoint,
) -> NativeLocalPointerSample {
    let factor = geometry.presentation_scale_factor().get();
    let physical = PhysicalPoint::new(point.x() * factor, point.y() * factor).unwrap();
    let sample = cap
        .capture(
            binding,
            NativeLocalPointerFact::new(
                physical,
                geometry.content_size(),
                geometry.native_scale_factor(),
            ),
            geometry,
        )
        .unwrap();
    assert_eq!(sample.client_position(), physical);
    assert_eq!(
        sample.surface_position(),
        point,
        "divide by the presentation scale exactly once"
    );
    sample
}

fn edge(
    binding: NativeSurfaceBinding,
    event: Event,
    location: NativeLocalPointerLocation,
) -> NativePointerInput {
    let capture = if matches!(event, Event::ButtonReleased(_)) {
        NativePointerOwner::None
    } else {
        NativePointerOwner::Native(binding)
    };
    NativePointerInput::new_local(
        POINTER,
        event,
        location,
        NativePointerOwner::Native(binding),
        capture,
    )
}

fn located(
    binding: NativeSurfaceBinding,
    event: Event,
    sample: NativeLocalPointerSample,
) -> NativePointerInput {
    edge(binding, event, NativeLocalPointerLocation::Exact(sample))
}

fn reorder_batch(
    session: &DockspaceSession,
    binding: NativeSurfaceBinding,
    scene: &Scene,
    geometry: NativeLocalGeometry,
) -> [NativePointerInput; 3] {
    let cap = session.native_local_coordinates(binding).unwrap();
    let source = sample(cap, binding, geometry, scene.tab(FIRST).receiver.center());
    let target = sample(cap, binding, geometry, scene.insertion(THIRD, false));
    [
        located(binding, Event::ButtonPressed(Button::Primary), source),
        located(binding, Event::Moved, target),
        located(binding, Event::ButtonReleased(Button::Primary), target),
    ]
}

fn feedback(report: &HostFrameReport) -> DockspaceManipulationFeedback {
    let [feedback] = report.manipulation_feedback() else {
        panic!("expected exactly one manipulation publication: {report:?}");
    };
    *feedback
}

fn assert_clean_idle(session: &mut DockspaceSession) {
    let version = session.version();
    let order_before = order(session);
    let report = idle(session);
    assert!(!report.workspace_changed());
    assert!(report.manipulation_feedback().is_empty());
    assert!(!session.has_active_gesture());
    assert!(!session.has_pending_native_input());
    assert_eq!(session.version(), version);
    assert_eq!(order(session), order_before);
}

fn refresh(
    session: &mut DockspaceSession,
    binding: NativeSurfaceBinding,
    geometry: NativeLocalGeometry,
) -> Scene {
    let mut frame = session
        .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
        .unwrap();
    measure(&mut frame, geometry);
    finish(frame);
    let (scene, output) = paint(session, binding, geometry);
    assert!(
        present(session, output, Disposition::Presented)
            .manipulation_feedback()
            .is_empty()
    );
    scene
}

fn deliver_reorder(
    session: &mut DockspaceSession,
    binding: NativeSurfaceBinding,
    scene: &Scene,
    geometry: NativeLocalGeometry,
) -> DockspaceManipulationId {
    let version = session.version();
    assert_eq!(order(session), [FIRST, SECOND, THIRD]);
    session
        .record_native_pointer_batch(reorder_batch(session, binding, scene, geometry))
        .unwrap();
    let mut frame = session
        .begin_native_host_frame(|query| scene.answer(query))
        .unwrap();
    let successor = render(&mut frame, A, geometry);
    successor.assert_preview(false);
    let mut report = finish(frame);
    let terminal = feedback(&report);
    assert_eq!(terminal.status(), Status::Delivered(Delivery::Dock));
    assert!(report.workspace_changed());
    assert_eq!(report.before(), version);
    assert_eq!(report.after(), session.version());
    assert_eq!(order(session), [SECOND, THIRD, FIRST]);
    assert_ne!(session.version(), version);
    assert!(!session.has_active_gesture());
    let committed = session.version();
    let exact = output(&mut report, &successor, binding);
    let settled = present(session, exact, Disposition::Presented);
    assert!(settled.manipulation_feedback().is_empty());
    assert!(!settled.workspace_changed());
    assert_eq!(session.version(), committed);
    assert_eq!(order(session), [SECOND, THIRD, FIRST]);
    assert_clean_idle(session);
    terminal.manipulation_id()
}

#[test]
fn size_only_reorder_retries_the_release_frame_and_settles_its_output_without_redelivery() {
    let (mut session, bindings, scene) = fixture();
    let binding = bindings[0];
    let version = session.version();
    let presented = session.presented_surface(A).unwrap();
    let captured = reorder_batch(&session, binding, &scene, geometry());
    // New observation provenance with identical facts must keep these samples usable.
    snapshot(&mut session, bindings, geometry());
    let recorded = session.native_input_recorded_count();
    session.record_native_pointer_batch(captured).unwrap();
    assert_eq!(session.native_input_recorded_count(), recorded + 3);
    {
        let mut abandoned = session
            .begin_native_host_frame(|query| scene.answer(query))
            .unwrap();
        render(&mut abandoned, A, geometry()).assert_preview(false);
        assert_eq!(
            abandoned.view().item(FIRST).unwrap().tabs().items(),
            [SECOND, THIRD, FIRST]
        );
    }
    assert_eq!(session.version(), version);
    assert_eq!(order(&session), [FIRST, SECOND, THIRD]);
    assert_eq!(session.presented_surface(A), Some(presented));
    assert_eq!(session.native_input_recorded_count(), recorded + 3);
    assert!(session.has_pending_native_input());
    assert!(!session.has_active_gesture());

    let mut frame = session
        .begin_native_host_frame(|query| scene.answer(query))
        .unwrap();
    let successor = render(&mut frame, A, geometry());
    successor.assert_preview(false);
    let mut report = finish(frame);
    let terminal = feedback(&report);
    assert_eq!(terminal.status(), Status::Delivered(Delivery::Dock));
    assert!(report.workspace_changed());
    assert_eq!(report.before(), version);
    assert_eq!(report.after(), session.version());
    assert_ne!(session.version(), version);
    let committed = session.version();
    assert_eq!(order(&session), [SECOND, THIRD, FIRST]);
    assert_eq!(session.native_input_recorded_count(), recorded + 3);
    assert!(!session.has_pending_native_input());
    assert!(!session.has_active_gesture());
    assert!(idle(&mut session).manipulation_feedback().is_empty());

    let exact = output(&mut report, &successor, binding);
    let artifact = exact.presentation_artifact();
    session
        .report_surface_presentation(exact, Disposition::Presented)
        .unwrap();
    drop(
        session
            .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
            .unwrap(),
    );
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
    assert_eq!(session.native_input_recorded_count(), recorded + 4);
    assert!(session.is_current_native_binding(binding));
    assert_clean_idle(&mut session);
}

#[test]
fn dropped_reorder_output_preserves_delivery_and_the_same_pointer_can_start_again() {
    let (mut session, bindings, scene) = fixture();
    let binding = bindings[0];
    let version = session.version();
    session
        .record_native_pointer_batch(reorder_batch(&session, binding, &scene, geometry()))
        .unwrap();
    let mut frame = session
        .begin_native_host_frame(|query| scene.answer(query))
        .unwrap();
    let successor = render(&mut frame, A, geometry());
    successor.assert_preview(false);
    let mut report = finish(frame);
    let terminal = feedback(&report);
    assert_eq!(terminal.status(), Status::Delivered(Delivery::Dock));
    assert!(report.workspace_changed());
    assert_ne!(session.version(), version);
    let committed = session.version();
    assert_eq!(order(&session), [SECOND, THIRD, FIRST]);
    let exact = output(&mut report, &successor, binding);
    let dropped = present(&mut session, exact, Disposition::Dropped);
    assert!(dropped.manipulation_feedback().is_empty());
    assert!(!dropped.workspace_changed());
    assert_eq!(session.version(), committed);
    assert_eq!(order(&session), [SECOND, THIRD, FIRST]);
    assert_clean_idle(&mut session);

    let current = refresh(&mut session, binding, geometry());
    let cap = session.native_local_coordinates(binding).unwrap();
    let source = sample(
        cap,
        binding,
        geometry(),
        current.tab(FIRST).receiver.center(),
    );
    let target = sample(cap, binding, geometry(), current.insertion(SECOND, true));
    session
        .record_native_pointer_batch([
            located(binding, Event::ButtonPressed(Button::Primary), source),
            located(binding, Event::Moved, target),
            located(binding, Event::ButtonReleased(Button::Primary), target),
        ])
        .unwrap();
    let frame = session
        .begin_native_host_frame(|query| current.answer(query))
        .unwrap();
    let next = feedback(&finish(frame));
    assert_eq!(next.status(), Status::Delivered(Delivery::Dock));
    assert_ne!(next.manipulation_id(), terminal.manipulation_id());
    assert_eq!(order(&session), [FIRST, SECOND, THIRD]);
    assert_clean_idle(&mut session);
}

#[test]
fn capture_requires_event_callback_and_committed_geometry_to_match() {
    let (session, bindings, _) = fixture();
    let binding = bindings[0];
    let cap = session.native_local_coordinates(binding).unwrap();
    let current = geometry();
    let position = PhysicalPoint::new(120.0, 30.0).unwrap();
    for event in [
        NativeLocalPointerFact::new(position, size(1201.0, 900.0), current.native_scale_factor()),
        NativeLocalPointerFact::new(position, current.content_size(), scale(1.0)),
    ] {
        assert_eq!(
            cap.capture(binding, event, current),
            Err(NativeLocalCoordinateError::EventGeometryMismatch)
        );
    }
    for callback in [
        NativeLocalGeometry::new(
            size(1201.0, 900.0),
            current.native_scale_factor(),
            current.presentation_scale_factor(),
        ),
        NativeLocalGeometry::new(
            current.content_size(),
            scale(1.0),
            current.presentation_scale_factor(),
        ),
        NativeLocalGeometry::new(
            current.content_size(),
            current.native_scale_factor(),
            scale(2.0),
        ),
    ] {
        let event = NativeLocalPointerFact::new(
            position,
            callback.content_size(),
            callback.native_scale_factor(),
        );
        assert_eq!(
            cap.capture(binding, event, callback),
            Err(NativeLocalCoordinateError::CallbackGeometryMismatch)
        );
    }
    let captured = sample(
        cap,
        binding,
        current,
        LogicalPoint::new(80.0, 20.0).unwrap(),
    );
    assert_eq!(captured.client_position(), position);
    // Matching capture remains possible after every rejected mismatch.
    assert_eq!(cap.geometry(), current);
}

#[test]
fn identical_geometry_cannot_lend_capabilities_to_another_window_or_session() {
    let (mut session, bindings, scene) = fixture();
    let (other, foreign_bindings, foreign_scene) = fixture();
    let cap = session.native_local_coordinates(bindings[0]).unwrap();
    let sibling = session.native_local_coordinates(bindings[1]).unwrap();
    let foreign = other.native_local_coordinates(foreign_bindings[0]).unwrap();
    assert_eq!(cap.geometry(), sibling.geometry());
    assert_eq!(cap.geometry(), foreign.geometry());
    assert_ne!(cap, sibling);
    assert_ne!(cap, foreign);
    assert!(
        session
            .native_local_coordinates(foreign_bindings[0])
            .is_none()
    );
    let event = NativeLocalPointerFact::new(
        PhysicalPoint::new(120.0, 30.0).unwrap(),
        geometry().content_size(),
        geometry().native_scale_factor(),
    );
    for wrong in [bindings[1], foreign_bindings[0]] {
        assert!(matches!(
            cap.capture(wrong, event, geometry()),
            Err(NativeLocalCoordinateError::BindingMismatch { .. })
        ));
    }

    let own = sample(
        cap,
        bindings[0],
        geometry(),
        scene.tab(FIRST).receiver.center(),
    );
    let stolen = sample(
        foreign,
        foreign_bindings[0],
        geometry(),
        foreign_scene.tab(FIRST).receiver.center(),
    );
    let recorded = session.native_input_recorded_count();
    let error = session
        .record_native_pointer_batch([
            located(bindings[0], Event::Moved, own),
            located(bindings[0], Event::ButtonPressed(Button::Primary), stolen),
        ])
        .unwrap_err();
    assert_eq!(error.native_kind(), Some(NativeHostErrorKind::InvalidFacts));
    assert_eq!(session.native_input_recorded_count(), recorded);
    assert!(!session.has_pending_native_input());
    assert!(!session.has_active_gesture());
    assert_eq!(order(&session), [FIRST, SECOND, THIRD]);

    // Even a valid sample from this provider's other window cannot authorize
    // native delivery or capture owned by A. Supply B's real presented receiver.
    let (sibling_scene, sibling_output) = paint(&mut session, bindings[1], geometry());
    present(&mut session, sibling_output, Disposition::Presented);
    let sibling_sample = sample(
        sibling,
        bindings[1],
        geometry(),
        sibling_scene.tab(OTHER).receiver.center(),
    );
    session
        .record_native_pointer(located(
            bindings[0],
            Event::ButtonPressed(Button::Primary),
            sibling_sample,
        ))
        .unwrap();
    let frame = session
        .begin_native_host_frame(|query| {
            assert!(query.matches_native_binding(bindings[1]));
            sibling_scene.answer(query)
        })
        .unwrap();
    assert!(!finish(frame).workspace_changed());
    assert!(session.view().item(FIRST).unwrap().is_selected());
    assert!(!session.has_active_gesture());
    session
        .record_native_pointer(edge(
            bindings[0],
            Event::ButtonReleased(Button::Primary),
            NativeLocalPointerLocation::Unknown,
        ))
        .unwrap();
    idle(&mut session);
    assert!(!session.has_pending_native_input());
    deliver_reorder(&mut session, bindings[0], &scene, geometry());
}

#[test]
fn local_coordinate_capabilities_only_follow_committed_snapshots_even_after_frame_drop() {
    let (mut session, bindings) = registered();
    let binding = bindings[0];
    let version = session.version();
    assert!(session.native_local_coordinates(binding).is_none());
    snapshot(&mut session, bindings, geometry());
    assert!(session.native_local_coordinates(binding).is_none());
    {
        let mut abandoned = session
            .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
            .unwrap();
        measure(&mut abandoned, geometry());
    }
    assert!(session.native_local_coordinates(binding).is_none());
    assert_eq!(session.version(), version);
    assert!(session.has_pending_native_input());
    let mut frame = session
        .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
        .unwrap();
    measure(&mut frame, geometry());
    assert!(!finish(frame).workspace_changed());
    assert!(session.native_local_coordinates(binding).is_none());
    // Measurement does not establish an active presentation endpoint. A real
    // committed output must first bind this native surface to its host.
    let (_, exact) = paint(&mut session, binding, geometry());
    present(&mut session, exact, Disposition::Presented);
    let original = session.native_local_coordinates(binding).unwrap();
    assert_eq!(original.geometry(), geometry());

    snapshot(&mut session, bindings, geometry());
    idle(&mut session);
    assert_eq!(
        session.native_local_coordinates(binding),
        Some(original),
        "new observation provenance with identical facts must preserve coordinate authority",
    );

    snapshot(&mut session, bindings, changed_geometry());
    assert_eq!(session.native_local_coordinates(binding), Some(original));
    let recorded = session.native_input_recorded_count();
    {
        let mut abandoned = session
            .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
            .unwrap();
        measure(&mut abandoned, changed_geometry());
    }
    assert_eq!(session.native_local_coordinates(binding), Some(original));
    assert_eq!(session.native_input_recorded_count(), recorded);
    assert!(session.has_pending_native_input());
    assert_eq!(session.version(), version);
    let mut frame = session
        .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
        .unwrap();
    measure(&mut frame, changed_geometry());
    assert!(!finish(frame).workspace_changed());
    let current = session.native_local_coordinates(binding).unwrap();
    assert_ne!(current, original);
    assert_eq!(current.geometry(), changed_geometry());
    assert!(!session.has_pending_native_input());
    assert_eq!(session.version(), version);
}

#[test]
fn earlier_snapshot_invalidates_captured_samples_and_still_drains_the_terminal() {
    for already_dragging in [false, true] {
        let (mut session, bindings, scene) = fixture();
        let binding = bindings[0];
        let cap = session.native_local_coordinates(binding).unwrap();
        let source = sample(cap, binding, geometry(), scene.tab(FIRST).receiver.center());
        let target = sample(cap, binding, geometry(), scene.insertion(THIRD, false));
        if already_dragging {
            session
                .record_native_pointer_batch([
                    located(binding, Event::ButtonPressed(Button::Primary), source),
                    located(binding, Event::Moved, target),
                ])
                .unwrap();
            let frame = session
                .begin_native_host_frame(|query| scene.answer(query))
                .unwrap();
            assert!(!finish(frame).workspace_changed());
            assert!(session.has_active_gesture());
        }
        let version = session.version();
        snapshot(&mut session, bindings, changed_geometry());
        let mut inputs = Vec::new();
        if !already_dragging {
            inputs.push(located(
                binding,
                Event::ButtonPressed(Button::Primary),
                source,
            ));
        }
        inputs.extend([
            located(binding, Event::Moved, target),
            located(binding, Event::ButtonReleased(Button::Primary), target),
        ]);
        // Recording retains G1 even though the producer has already seen G2.
        session.record_native_pointer_batch(inputs).unwrap();
        let mut frame = session
            .begin_native_host_frame(|_| {
                panic!("a G1 local sample cannot query a receiver after the preceding G2 snapshot")
            })
            .unwrap();
        measure(&mut frame, changed_geometry());
        let report = finish(frame);
        assert!(!report.workspace_changed());
        assert!(
            report
                .manipulation_feedback()
                .iter()
                .all(|entry| !matches!(entry.status(), Status::Delivered(_) | Status::Pending(_)))
        );
        if already_dragging {
            assert!(matches!(feedback(&report).status(), Status::Cancelled(_)));
        }
        assert_eq!(session.version(), version);
        assert_eq!(order(&session), [FIRST, SECOND, THIRD]);
        assert!(!session.has_active_gesture());
        assert!(!session.has_pending_native_input());
        let fresh = session.native_local_coordinates(binding).unwrap();
        assert_ne!(fresh, cap);
        let scene = refresh(&mut session, binding, changed_geometry());
        deliver_reorder(&mut session, binding, &scene, changed_geometry());
    }
}

#[test]
fn outside_and_unknown_releases_cancel_without_poisoning_the_same_pointer() {
    for unknown in [false, true] {
        let (mut session, bindings, scene) = fixture();
        let binding = bindings[0];
        let cap = session.native_local_coordinates(binding).unwrap();
        let source = sample(cap, binding, geometry(), scene.tab(FIRST).receiver.center());
        let target = sample(cap, binding, geometry(), scene.insertion(THIRD, false));
        let version = session.version();
        session
            .record_native_pointer_sequence([
                located(binding, Event::ButtonPressed(Button::Primary), source),
                located(binding, Event::Moved, target),
            ])
            .unwrap();
        let frame = session
            .begin_native_host_frame(|query| scene.answer(query))
            .unwrap();
        assert!(!finish(frame).workspace_changed());
        assert!(session.has_active_gesture());
        let release = if unknown {
            NativeLocalPointerLocation::Unknown
        } else {
            NativeLocalPointerLocation::Exact(sample(
                cap,
                binding,
                geometry(),
                LogicalPoint::new(-40.0, target.surface_position().y()).unwrap(),
            ))
        };
        session
            .record_native_pointer(edge(
                binding,
                Event::ButtonReleased(Button::Primary),
                release,
            ))
            .unwrap();
        let frame = session
            .begin_native_host_frame(|_| {
                panic!("unknown and out-of-client releases have no local hover authority")
            })
            .unwrap();
        let report = finish(frame);
        let terminal = feedback(&report);
        assert!(matches!(terminal.status(), Status::Cancelled(_)));
        assert!(!report.workspace_changed());
        assert_eq!(session.version(), version);
        assert_eq!(order(&session), [FIRST, SECOND, THIRD]);
        assert_clean_idle(&mut session);
        let scene = refresh(&mut session, binding, geometry());
        let next = deliver_reorder(&mut session, binding, &scene, geometry());
        assert_ne!(next, terminal.manipulation_id());
    }
}

#[test]
fn inactive_tab_selection_preserves_its_actual_output_until_the_successor_is_presented() {
    let (mut session, bindings, original) = fixture();
    let binding = bindings[0];
    let cap = session.native_local_coordinates(binding).unwrap();
    let presented = session.presented_surface(A).unwrap();
    let source = sample(
        cap,
        binding,
        geometry(),
        original.tab(SECOND).receiver.center(),
    );
    let target_point = original.insertion(FIRST, true);
    let target = sample(cap, binding, geometry(), target_point);
    assert!(session.view().item(FIRST).unwrap().is_selected());
    assert!(!session.view().item(SECOND).unwrap().is_selected());
    session
        .record_native_pointer_batch([
            located(binding, Event::ButtonPressed(Button::Primary), source),
            located(binding, Event::Moved, target),
        ])
        .unwrap();
    let mut hover_queries = 0;
    let mut frame = session
        .begin_native_host_frame(|query| {
            if matches!(query.purpose(), NativeReceiverPurpose::HoverHit) {
                hover_queries += 1;
            }
            original.answer(query)
        })
        .unwrap();
    measure(&mut frame, geometry());
    let report = finish(frame);
    assert!(
        report.workspace_changed(),
        "the native mouse press activates its source tab"
    );
    assert!(
        hover_queries > 0,
        "selection preserves the actual output and its semantically current hover target"
    );
    assert!(report.manipulation_feedback().is_empty());
    assert!(session.view().item(SECOND).unwrap().is_selected());
    assert_eq!(order(&session), [FIRST, SECOND, THIRD]);
    assert!(session.has_active_gesture());
    assert_eq!(session.presented_surface(A), Some(presented));
    assert_eq!(session.native_local_coordinates(binding), Some(cap));

    let (current, painted) = paint(&mut session, binding, geometry());
    assert_eq!(
        current.dragged_item,
        Some(SECOND),
        "the exact source continuation must actually cross the drag threshold"
    );
    current.assert_preview(true);
    assert!(
        present(&mut session, painted, Disposition::Presented)
            .manipulation_feedback()
            .is_empty()
    );
    assert!(session.has_active_gesture());
    let version = session.version();
    // Even at identical geometry, only the newly presented descriptors may clear hover.
    session
        .record_native_pointer_batch([
            located(binding, Event::Moved, target),
            located(binding, Event::ButtonReleased(Button::Primary), target),
        ])
        .unwrap();
    let mut rejected_old_hover = 0;
    let mut frame = session
        .begin_native_host_frame(|query| {
            if matches!(query.purpose(), NativeReceiverPurpose::HoverHit) {
                rejected_old_hover += 1;
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
    let successor = render(&mut frame, A, geometry());
    successor.assert_preview(false);
    let mut report = finish(frame);
    assert!(rejected_old_hover > 0);
    let terminal = feedback(&report);
    assert_eq!(terminal.status(), Status::Delivered(Delivery::Dock));
    assert!(report.workspace_changed());
    assert_eq!(order(&session), [SECOND, FIRST, THIRD]);
    assert_ne!(session.version(), version);
    let committed = session.version();
    let exact = output(&mut report, &successor, binding);
    let settled = present(&mut session, exact, Disposition::Presented);
    assert!(settled.manipulation_feedback().is_empty());
    assert!(!settled.workspace_changed());
    assert_eq!(session.version(), committed);
    assert_eq!(order(&session), [SECOND, FIRST, THIRD]);
    assert!(session.view().item(SECOND).unwrap().is_selected());
    assert_clean_idle(&mut session);
}

#[test]
fn the_next_gesture_on_the_same_native_pointer_can_choose_the_desktop_lane() {
    let (mut session, bindings) = registered_with_desktop(true);
    let binding = bindings[0];
    let bounds = PhysicalRect::new(400.0, -300.0, 1200.0, 900.0).unwrap();
    session
        .report_managed_native_snapshot(
            [
                (binding, facts(geometry()).with_content_bounds(bounds)),
                (bindings[1], facts(geometry())),
            ],
            NativeWorkAreaRoster::Unknown,
        )
        .unwrap();
    let mut frame = session
        .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
        .unwrap();
    measure(&mut frame, geometry());
    finish(frame);
    let (scene, exact) = paint(&mut session, binding, geometry());
    present(&mut session, exact, Disposition::Presented);
    let cap = session.native_local_coordinates(binding).unwrap();
    deliver_reorder(&mut session, binding, &scene, geometry());
    assert_eq!(session.native_local_coordinates(binding), Some(cap));

    let current = refresh(&mut session, binding, geometry());
    let point = current.tab(SECOND).receiver.center();
    let desktop = PhysicalPoint::new(
        bounds.x() + point.x() * geometry().presentation_scale_factor().get(),
        bounds.y() + point.y() * geometry().presentation_scale_factor().get(),
    )
    .unwrap();
    let before = session.version();
    let recorded = session.native_input_recorded_count();
    session
        .record_native_pointer_batch(
            [
                (
                    Event::ButtonPressed(Button::Primary),
                    NativePointerOwner::Native(binding),
                ),
                (
                    Event::ButtonReleased(Button::Primary),
                    NativePointerOwner::None,
                ),
            ]
            .map(|(event, capture)| {
                NativePointerInput::new(
                    POINTER,
                    event,
                    NativeDesktopPointerLocation::new(
                        NativeDesktopPosition::Exact(desktop),
                        NativePointerHover::Dock(binding),
                        None,
                    ),
                    NativePointerOwner::Native(binding),
                    capture,
                )
            }),
        )
        .unwrap();
    let frame = session
        .begin_native_host_frame(|query| current.answer(query))
        .unwrap();
    let report = finish(frame);
    assert!(report.workspace_changed());
    assert_ne!(session.version(), before);
    assert!(session.view().item(SECOND).unwrap().is_selected());
    assert_eq!(order(&session), [SECOND, THIRD, FIRST]);
    assert_eq!(session.native_input_recorded_count(), recorded + 2);
    assert_eq!(session.native_local_coordinates(binding), Some(cap));
    assert!(!session.has_active_gesture());
    assert!(!session.has_pending_native_input());
    assert_clean_idle(&mut session);
}
