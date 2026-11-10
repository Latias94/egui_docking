//! Native stationary work through real receiver records and exact output settlement.

use std::time::Duration;

use dockspace::geometry::{
    LogicalPoint, LogicalRect, LogicalSize, PhysicalPoint, PhysicalRect, PhysicalSize, ScaleFactor,
};
use dockspace::model::{
    DockspaceActionOutcome, DockspaceLayout, DockspaceNode, DockspaceRootLayout,
    DockspaceSurfaceLayout, ItemId, RootId, SurfaceId,
};
use dockspace::policy::DockPolicy;
use dockspace::runtime::{
    DockspaceHostFrame, DockspaceManipulationDelivery as Delivery,
    DockspaceManipulationStatus as Status, DockspacePaneFocusRequest, DockspaceReceiverDescriptor,
    DockspaceReceiverRole, DockspaceSession, DockspaceTouchEvent, HostFrameReport,
    HostInputOutcome, HostPointerInput, HostPointerRoute, HostWindowToken,
    NativeDesktopPointerLocation, NativeDesktopPosition, NativeDragSourceInput,
    NativeHostCapabilities, NativeHostCapability, NativeLocalCoordinates, NativeLocalGeometry,
    NativeLocalPointerFact, NativeLocalPointerLocation, NativeLocalPointerSample,
    NativeLocalToDesktopHandoff, NativePointerButton as Button, NativePointerCancelReason,
    NativePointerCaptureRequest, NativePointerContinuation, NativePointerEvent as Event,
    NativePointerHover, NativePointerId, NativePointerInput, NativePointerOwner,
    NativePointerRoster, NativeReceiverAnswer, NativeReceiverPurpose, NativeReceiverQuery,
    NativeScrollDelta, NativeScrollDeviceId, NativeScrollEvent, NativeScrollModifiers,
    NativeScrollMomentum, NativeScrollPhase, NativeSurfaceBinding, NativeWindowFacts,
    NativeWindowInputState, NativeWindowPresentationState, NativeWorkAreaRoster,
    PaintedSurfaceOutput, PointerDeviceKind as Device, PointerEventMetadata,
    SurfacePaintProgramOperation as Operation, SurfacePaintProgramStep as Step,
    SurfacePointerButton, SurfacePointerCapture, SurfacePointerEvent, SurfacePointerId,
    SurfacePointerReceiverFacts, SurfacePresentationResult as Disposition,
    SurfaceUnavailableReason, SurfaceWork, SurfaceWorkDeferred, UniformSurfaceMetrics,
};
use dockspace_host_harness::{OwnedRecord, RecordingRenderer};

const SURFACE: SurfaceId = SurfaceId::new(1);
const ROOT: RootId = RootId::new(1);
const FIRST: ItemId = ItemId::new(1);
const SECOND: ItemId = ItemId::new(2);
const THIRD: ItemId = ItemId::new(3);
const POINTER: NativePointerId = NativePointerId::new(17);
const PRESS: Event = Event::ButtonPressed(Button::Primary);
const RELEASE: Event = Event::ButtonReleased(Button::Primary);
const END: Event = Event::ContactEnded(Button::Primary);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Lane {
    Local,
    Desktop,
}

fn geometry() -> NativeLocalGeometry {
    NativeLocalGeometry::new(
        PhysicalSize::new(1200.0, 900.0).unwrap(),
        ScaleFactor::new(2.0).unwrap(),
        ScaleFactor::new(1.5).unwrap(),
    )
}

fn window_facts(with_origin: bool) -> NativeWindowFacts {
    let geometry = geometry();
    let facts = NativeWindowFacts::live()
        .with_content_size(geometry.content_size())
        .with_native_scale_factor(geometry.native_scale_factor())
        .with_presentation_scale_factor(geometry.presentation_scale_factor())
        .with_input(NativeWindowInputState::ReceivesInput, None)
        .with_presentation(NativeWindowPresentationState::Visible);
    if with_origin {
        facts.with_content_bounds(PhysicalRect::new(400.0, -300.0, 1200.0, 900.0).unwrap())
    } else {
        facts
    }
}

fn measure(frame: &mut DockspaceHostFrame<'_>, width: f64) {
    frame
        .measure_surface(
            SURFACE,
            UniformSurfaceMetrics::new(
                LogicalRect::new(0.0, 0.0, width, 600.0).unwrap(),
                LogicalSize::new(64.0, 48.0).unwrap(),
                96.0,
            )
            .unwrap(),
        )
        .unwrap();
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
            .begin_native_host_frame(|_| panic!("idle frames have no pointer receiver challenge"))
            .unwrap(),
    )
}

struct Tab {
    item: ItemId,
    receiver: DockspaceReceiverDescriptor,
    visible: LogicalRect,
}

struct Scene {
    recording: RecordingRenderer,
    tabs: Vec<Tab>,
    focus: Option<DockspacePaneFocusRequest>,
    dragged_item: Option<ItemId>,
}

impl Scene {
    fn tab(&self, item: ItemId) -> &Tab {
        self.tabs.iter().find(|tab| tab.item == item).unwrap()
    }

    fn point(&self, item: ItemId, fraction: f64) -> LogicalPoint {
        let tab = self.tab(item);
        let point = LogicalPoint::new(
            tab.visible.x() + tab.visible.width() * fraction,
            tab.visible.y() + tab.visible.height() * 0.5,
        )
        .unwrap();
        assert!(tab.receiver.bounds().contains(point));
        point
    }

    fn answer(&self, query: NativeReceiverQuery) -> NativeReceiverAnswer {
        assert_eq!(query.surface(), SURFACE);
        let Some(point) = query.point() else {
            return NativeReceiverAnswer::Unknown;
        };
        let receiver = self
            .tabs
            .iter()
            .find(|tab| tab.receiver.bounds().contains(point))
            .map(|tab| tab.receiver)
            .or_else(|| {
                if !matches!(query.purpose(), NativeReceiverPurpose::HoverHit) {
                    return None;
                }
                self.recording
                    .records
                    .iter()
                    .find_map(|record| match record {
                        OwnedRecord::Receiver(receiver)
                            if receiver.role() == DockspaceReceiverRole::TabStripScroll
                                && receiver.bounds().contains(point) =>
                        {
                            Some(*receiver)
                        }
                        _ => None,
                    })
            });
        let Some(receiver) = receiver else {
            return NativeReceiverAnswer::Unknown;
        };
        assert!(
            self.recording
                .records
                .contains(&OwnedRecord::Receiver(receiver))
        );
        if matches!(query.purpose(), NativeReceiverPurpose::HoverHit) {
            query.clear_hover_lane(&receiver).map_or(
                NativeReceiverAnswer::Unknown,
                NativeReceiverAnswer::ClearHoverLane,
            )
        } else {
            query
                .bind_receiver(&receiver)
                .map_or(NativeReceiverAnswer::Unknown, NativeReceiverAnswer::Dock)
        }
    }

    fn assert_no_preview(&self) {
        assert!(
            self.recording
                .records
                .iter()
                .all(|record| !matches!(record, OwnedRecord::Preview(_)))
        );
    }
}

fn render(frame: &mut DockspaceHostFrame<'_>, width: f64) -> Scene {
    if matches!(
        frame.surface_work(SURFACE).unwrap(),
        SurfaceWork::Deferred(SurfaceWorkDeferred::Unmeasured)
    ) {
        measure(frame, width);
    }
    let plan = frame.paint_plan(SURFACE).unwrap().unwrap();
    let focus = plan.pane_focus_request();
    let mut tabs = Vec::new();
    let mut dragged_item = None;
    let mut strip_receivers = Vec::new();
    plan.visit_surface_program(|step| {
        match step {
            Step::Operation {
                operation: Operation::TabBar(bar),
                ..
            } => strip_receivers.extend(plan.receiver_for_tab_strip_scroll(bar)),
            Step::Operation {
                operation: Operation::Tab(tab),
                ..
            } => {
                if let Some(receiver) = plan.receiver_for_tab_body(tab) {
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
    let (mut recording, receipt) = RecordingRenderer::execute(plan).unwrap();
    for receiver in strip_receivers {
        recording
            .records
            .insert(recording.records.len() - 1, OwnedRecord::Receiver(receiver));
    }
    for tab in &tabs {
        assert!(
            recording
                .records
                .contains(&OwnedRecord::Receiver(tab.receiver))
        );
    }
    frame.confirm_surface_painted(receipt).unwrap();
    Scene {
        recording,
        tabs,
        focus,
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
    output
}

fn paint(
    session: &mut DockspaceSession,
    binding: NativeSurfaceBinding,
) -> (Scene, PaintedSurfaceOutput) {
    let mut frame = session
        .begin_native_host_frame(|_| panic!("painting does not sample native input"))
        .unwrap();
    let scene = render(&mut frame, 800.0);
    let mut report = finish(frame);
    let output = take_output(&mut report, &scene, binding);
    (scene, output)
}

fn present(session: &mut DockspaceSession, output: PaintedSurfaceOutput) -> HostFrameReport {
    let artifact = output.presentation_artifact();
    session
        .report_surface_presentation(output, Disposition::Presented)
        .unwrap();
    let report = idle(session);
    assert_eq!(
        report
            .surface_presentation_commits()
            .iter()
            .filter(|commit| commit.settles(artifact))
            .count(),
        1
    );
    report
}

fn fixture(with_origin: bool) -> (DockspaceSession, NativeSurfaceBinding, Scene) {
    let layout = DockspaceLayout::new([DockspaceSurfaceLayout::new(
        SURFACE,
        DockspaceRootLayout::new(ROOT, DockspaceNode::central_tabs([FIRST, SECOND, THIRD])),
    )])
    .unwrap();
    let mut session = DockspaceSession::from_layout(layout, DockPolicy::default()).unwrap();
    session
        .enable_managed_native_host(NativePointerRoster::Exact(Vec::new()))
        .unwrap();
    let mut capabilities = NativeHostCapabilities::none_supported()
        .with(NativeHostCapability::AuthoritativeInventory)
        .with(NativeHostCapability::AuthoritativeButtonState)
        .with(NativeHostCapability::PointerHitTestObservation);
    if with_origin {
        capabilities = capabilities
            .with(NativeHostCapability::DesktopPointerPosition)
            .with(NativeHostCapability::HoveredWindow);
    }
    session
        .configure_managed_native_capabilities(capabilities)
        .unwrap();
    session
        .register_native_root(SURFACE, HostWindowToken::new(1))
        .unwrap();
    let binding = idle(&mut session).native_bindings()[0];
    session
        .report_managed_native_snapshot(
            [(binding, window_facts(with_origin))],
            NativeWorkAreaRoster::Unknown,
        )
        .unwrap();
    let mut frame = session
        .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
        .unwrap();
    measure(&mut frame, 800.0);
    finish(frame);
    let (scene, output) = paint(&mut session, binding);
    assert!(!present(&mut session, output).workspace_changed());
    assert!(session.native_local_coordinates(binding).is_some());
    assert!(session.view().item(FIRST).unwrap().is_selected());
    assert!(!session.has_pending_native_input());
    (session, binding, scene)
}

fn sample(
    coordinates: NativeLocalCoordinates,
    binding: NativeSurfaceBinding,
    point: LogicalPoint,
) -> NativeLocalPointerSample {
    let scale = geometry().presentation_scale_factor().get();
    let sample = coordinates
        .capture(
            binding,
            NativeLocalPointerFact::new(
                PhysicalPoint::new(point.x() * scale, point.y() * scale).unwrap(),
                geometry().content_size(),
                geometry().native_scale_factor(),
            ),
            geometry(),
        )
        .unwrap();
    assert_eq!(sample.surface_position(), point);
    sample
}

fn desktop_location(
    binding: NativeSurfaceBinding,
    point: LogicalPoint,
) -> NativeDesktopPointerLocation {
    let scale = geometry().presentation_scale_factor().get();
    NativeDesktopPointerLocation::new(
        NativeDesktopPosition::Exact(
            PhysicalPoint::new(400.0 + point.x() * scale, -300.0 + point.y() * scale).unwrap(),
        ),
        NativePointerHover::Dock(binding),
        None,
    )
}

fn observed_input(
    coordinates: Option<NativeLocalCoordinates>,
    binding: NativeSurfaceBinding,
    event: Event,
    point: LogicalPoint,
    capture: NativePointerOwner,
    metadata: PointerEventMetadata,
) -> NativePointerInput {
    let input = if let Some(coordinates) = coordinates {
        NativePointerInput::new_local(
            POINTER,
            event,
            NativeLocalPointerLocation::Exact(sample(coordinates, binding, point)),
            NativePointerOwner::Native(binding),
            capture,
        )
    } else {
        NativePointerInput::new(
            POINTER,
            event,
            desktop_location(binding, point),
            NativePointerOwner::Native(binding),
            capture,
        )
    };
    input.with_metadata(metadata)
}

fn input(
    session: &DockspaceSession,
    lane: Lane,
    binding: NativeSurfaceBinding,
    event: Event,
    point: LogicalPoint,
    metadata: PointerEventMetadata,
) -> NativePointerInput {
    observed_input(
        if lane == Lane::Local {
            Some(
                session
                    .native_local_coordinates(binding)
                    .expect("local input requires its actual committed coordinate capability"),
            )
        } else {
            None
        },
        binding,
        event,
        point,
        if matches!(event, Event::ButtonReleased(_) | Event::ContactEnded(_)) {
            NativePointerOwner::None
        } else {
            NativePointerOwner::Native(binding)
        },
        metadata,
    )
}

fn continuation(
    request: NativePointerContinuation,
    at: Duration,
    device: Device,
    capture: NativePointerOwner,
) -> NativePointerInput {
    assert_eq!(request.pointer(), POINTER);
    observed_input(
        request.local_coordinates(),
        request.source_binding(),
        request.event(),
        request.position(),
        capture,
        PointerEventMetadata::new(device, at),
    )
}

fn continuation_with_owners(
    request: NativePointerContinuation,
    at: Duration,
    delivery: NativePointerOwner,
    capture: NativePointerOwner,
) -> NativePointerInput {
    let binding = request.source_binding();
    let input = match request.local_coordinates() {
        Some(coordinates) => NativePointerInput::new_local(
            request.pointer(),
            request.event(),
            NativeLocalPointerLocation::Exact(sample(coordinates, binding, request.position())),
            delivery,
            capture,
        ),
        None => NativePointerInput::new(
            request.pointer(),
            request.event(),
            desktop_location(binding, request.position()),
            delivery,
            capture,
        ),
    };
    input.with_metadata(PointerEventMetadata::new(Device::Mouse, at))
}

fn at(device: Device, millis: u64) -> PointerEventMetadata {
    PointerEventMetadata::new(device, Duration::from_millis(millis))
}

fn reduce(session: &mut DockspaceSession, scene: &Scene) -> HostFrameReport {
    finish(
        session
            .begin_native_host_frame(|query| scene.answer(query))
            .unwrap(),
    )
}

fn order(session: &DockspaceSession) -> Vec<ItemId> {
    session.view().item(FIRST).unwrap().tabs().items().to_vec()
}

fn selections(report: &HostFrameReport) -> Vec<ItemId> {
    report
        .inputs()
        .iter()
        .filter_map(|outcome| match outcome {
            HostInputOutcome::ProductActionApplied(DockspaceActionOutcome::Selected {
                item,
                changed: true,
            }) => Some(*item),
            _ => None,
        })
        .collect()
}

fn start_drag(
    session: &mut DockspaceSession,
    lane: Lane,
    binding: NativeSurfaceBinding,
    scene: &Scene,
    target: LogicalPoint,
) -> NativePointerContinuation {
    session
        .record_native_pointer_batch([
            input(
                session,
                lane,
                binding,
                PRESS,
                scene.point(FIRST, 0.25),
                at(Device::Mouse, 0),
            ),
            input(
                session,
                lane,
                binding,
                Event::Moved,
                target,
                at(Device::Mouse, 10),
            ),
        ])
        .unwrap();
    assert!(!reduce(session, scene).workspace_changed());
    assert!(session.has_active_gesture());
    session
        .native_pointer_continuation()
        .expect("the real target schedules stationary work")
}

#[test]
fn native_hover_dwell_is_read_only_and_retries_the_recorded_due_sample_once() {
    for lane in [Lane::Local, Lane::Desktop] {
        let (mut session, binding, scene) = fixture(lane == Lane::Desktop);
        let target = scene.point(SECOND, 0.625);
        let version = session.version();
        let request = start_drag(&mut session, lane, binding, &scene, target);
        assert_eq!(request.deadline(), Duration::from_millis(160));
        assert_eq!(request.surface(), SURFACE);
        assert_eq!(request.position(), target);
        assert_eq!(request.source_binding(), binding);
        assert_eq!(request.event(), Event::Continuation(request.token()));
        assert_eq!(request.local_coordinates().is_some(), lane == Lane::Local);
        let recorded = session.native_input_recorded_count();
        for _ in 0..3 {
            assert_eq!(session.native_pointer_continuation(), Some(request));
            assert!(session.pointer_continuation().is_none());
            assert!(!session.pointer_continuation_awaits_presentation(SURFACE));
            assert!(!session.native_pointer_continuation_awaits_presentation(SURFACE));
            assert_eq!(session.native_input_recorded_count(), recorded);
            assert_eq!(session.version(), version);
        }
        let (current, output) = paint(&mut session, binding);
        let focus = current.focus;
        assert_eq!(current.dragged_item, Some(FIRST));
        present(&mut session, output);
        assert_eq!(session.native_pointer_continuation(), Some(request));

        session
            .record_native_pointer(continuation(
                request,
                Duration::from_millis(159),
                Device::Mouse,
                NativePointerOwner::Native(binding),
            ))
            .unwrap();
        assert!(session.native_pointer_continuation().is_none());
        let early = reduce(&mut session, &current);
        assert!(!early.workspace_changed());
        assert!(selections(&early).is_empty());
        assert_eq!(session.native_pointer_continuation(), Some(request));

        session
            .record_native_pointer(continuation(
                request,
                request.deadline(),
                Device::Mouse,
                NativePointerOwner::Native(binding),
            ))
            .unwrap();
        let recorded = session.native_input_recorded_count();
        let mut abandoned = session
            .begin_native_host_frame(|query| {
                assert_eq!(query.metadata(), at(Device::Mouse, 160));
                assert!(query.matches_native_binding(binding));
                current.answer(query)
            })
            .unwrap();
        assert!(abandoned.view().item(SECOND).unwrap().is_selected());
        measure(&mut abandoned, 800.0);
        drop(abandoned);
        assert_eq!(session.version(), version);
        assert!(session.view().item(FIRST).unwrap().is_selected());
        assert!(!session.view().item(SECOND).unwrap().is_selected());
        assert!(session.has_pending_native_input());
        assert!(session.native_pointer_continuation().is_none());
        assert_eq!(session.native_input_recorded_count(), recorded);

        let mut retry = session
            .begin_native_host_frame(|query| {
                assert_eq!(query.metadata(), at(Device::Mouse, 160));
                current.answer(query)
            })
            .unwrap();
        measure(&mut retry, 800.0);
        let selected = finish(retry);
        assert_eq!(selections(&selected), [SECOND]);
        assert!(selected.workspace_changed());
        assert_eq!(session.native_input_recorded_count(), recorded);
        assert!(session.has_active_gesture());
        assert!(session.native_pointer_continuation().is_none());
        assert_eq!(order(&session), [FIRST, SECOND, THIRD]);
        let (fresh, output) = paint(&mut session, binding);
        assert!(
            fresh.focus.is_none() || fresh.focus == focus,
            "dwell may retire an old focus request but must not request new pane focus"
        );
        assert_eq!(fresh.dragged_item, Some(FIRST));
        present(&mut session, output);

        let selected_version = session.version();
        session
            .record_native_pointer(continuation(
                request,
                Duration::from_millis(161),
                Device::Mouse,
                NativePointerOwner::Native(binding),
            ))
            .unwrap();
        assert!(idle(&mut session).inputs().is_empty());
        assert_eq!(session.version(), selected_version);
        assert!(session.has_active_gesture());
        session
            .record_native_pointer(input(
                &session,
                lane,
                binding,
                RELEASE,
                target,
                at(Device::Mouse, 162),
            ))
            .unwrap();
        let report = finish(
            session
                .begin_native_host_frame(|query| {
                    NativeReceiverAnswer::Blocked(query.presented_surface())
                })
                .unwrap(),
        );
        assert!(!report.workspace_changed());
        assert!(!session.has_active_gesture());
        assert!(session.native_pointer_continuation().is_none());
    }
}

#[test]
fn a_surface_owned_wake_is_never_projected_as_native_stationary_work() {
    let layout = DockspaceLayout::new([DockspaceSurfaceLayout::new(
        SURFACE,
        DockspaceRootLayout::new(ROOT, DockspaceNode::central_tabs([FIRST, SECOND, THIRD])),
    )])
    .unwrap();
    let mut session = DockspaceSession::from_layout(layout, DockPolicy::default()).unwrap();
    session.enable_host_pointer().unwrap();
    let mut frame = session.begin_host_frame().unwrap();
    measure(&mut frame, 800.0);
    finish(frame);
    let mut frame = session.begin_host_frame().unwrap();
    let scene = render(&mut frame, 800.0);
    let mut report = finish(frame);
    let [output] = report.take_painted_outputs().try_into().unwrap();
    assert!(matches!(
        scene.recording.records.last(),
        Some(OwnedRecord::SurfaceEnd(semantic)) if output.matches_semantic_output(*semantic)
    ));
    session
        .report_surface_presentation(output, Disposition::Presented)
        .unwrap();
    finish(session.begin_host_frame().unwrap());
    let source = session
        .bind_presented_receiver(&scene.tab(FIRST).receiver)
        .unwrap();
    let surface = session.presented_surface(SURFACE).unwrap();
    let press = HostPointerRoute::new(surface, scene.point(FIRST, 0.25));
    let moved = HostPointerRoute::new(surface, scene.point(SECOND, 0.625));
    let mut frame = session.begin_host_frame().unwrap();
    frame
        .submit_host_pointer_batch([
            HostPointerInput::new(
                SurfacePointerId::new(17),
                SurfacePointerEvent::ButtonPressed(SurfacePointerButton::Primary),
                press,
                press,
                SurfacePointerCapture::ProviderEndpoint,
                SurfacePointerReceiverFacts::delivery(&source),
            )
            .with_metadata(at(Device::Mouse, 0)),
            HostPointerInput::new(
                SurfacePointerId::new(17),
                SurfacePointerEvent::Moved,
                moved,
                moved,
                SurfacePointerCapture::ProviderEndpoint,
                SurfacePointerReceiverFacts::unknown().with_clear_hover_lane(&surface),
            )
            .with_metadata(at(Device::Mouse, 10)),
        ])
        .unwrap();
    assert!(!finish(frame).workspace_changed());
    let request = session.pointer_continuation().unwrap();
    assert_eq!(request.deadline(), Duration::from_millis(160));
    let version = session.version();
    for _ in 0..3 {
        assert_eq!(session.pointer_continuation(), Some(request));
        assert!(session.native_pointer_continuation().is_none());
        assert!(!session.native_pointer_continuation_awaits_presentation(SURFACE));
        assert_eq!(session.version(), version);
    }
}

#[test]
fn native_touch_long_press_arms_then_contact_end_delivers_before_its_reorder_output() {
    for lane in [Lane::Local, Lane::Desktop] {
        let (mut session, binding, scene) = fixture(lane == Lane::Desktop);
        let version = session.version();
        let source = scene.point(FIRST, 0.25);
        let target = scene.point(THIRD, 0.625);
        session
            .record_native_pointer(input(
                &session,
                lane,
                binding,
                PRESS,
                source,
                at(Device::Touch, 0),
            ))
            .unwrap();
        let pressed = reduce(&mut session, &scene);
        assert!(pressed.inputs().is_empty());
        assert!(!pressed.workspace_changed());
        let request = session.native_pointer_continuation().unwrap();
        assert_eq!(request.deadline(), Duration::from_millis(350));
        assert!(request.source_input().is_none());
        assert!(request.capture_acquired_event().is_none());
        assert_eq!(request.position(), source);
        assert_eq!(request.local_coordinates().is_some(), lane == Lane::Local);
        session
            .record_native_pointer(continuation(
                request,
                Duration::from_millis(349),
                Device::Touch,
                NativePointerOwner::Native(binding),
            ))
            .unwrap();
        assert!(reduce(&mut session, &scene).inputs().is_empty());
        assert_eq!(session.native_pointer_continuation(), Some(request));
        session
            .record_native_pointer(continuation(
                request,
                request.deadline(),
                Device::Touch,
                NativePointerOwner::Native(binding),
            ))
            .unwrap();
        let recorded = session.native_input_recorded_count();
        drop(
            session
                .begin_native_host_frame(|query| scene.answer(query))
                .unwrap(),
        );
        assert_eq!(session.version(), version);
        assert!(session.has_active_gesture());
        assert!(session.has_pending_native_input());
        assert!(session.native_pointer_continuation().is_none());
        let armed = reduce(&mut session, &scene);
        assert!(matches!(
            armed.inputs(),
            [HostInputOutcome::Touch(DockspaceTouchEvent::Armed { pointer, surface })]
                if *pointer == SurfacePointerId::new(17) && *surface == SURFACE
        ));
        assert_eq!(session.native_input_recorded_count(), recorded);
        assert_eq!(session.version(), version);
        assert!(session.native_pointer_continuation().is_none());
        session
            .record_native_pointer_batch([
                input(
                    &session,
                    lane,
                    binding,
                    Event::Moved,
                    target,
                    at(Device::Touch, 360),
                ),
                input(&session, lane, binding, END, target, at(Device::Touch, 370)),
            ])
            .unwrap();
        let mut frame = session
            .begin_native_host_frame(|query| scene.answer(query))
            .unwrap();
        let successor = render(&mut frame, 800.0);
        successor.assert_no_preview();
        let mut report = finish(frame);
        let [terminal] = report.manipulation_feedback() else {
            panic!("one native touch release must deliver in its update: {report:?}");
        };
        assert_eq!(terminal.status(), Status::Delivered(Delivery::Dock));
        assert!(report.workspace_changed());
        assert_eq!(report.before(), version);
        assert_eq!(report.after(), session.version());
        assert_ne!(session.version(), version);
        assert_eq!(order(&session), [SECOND, THIRD, FIRST]);
        let committed = session.version();
        assert!(!session.has_active_gesture());
        assert!(session.native_pointer_continuation().is_none());
        let output = take_output(&mut report, &successor, binding);
        let settled = present(&mut session, output);
        assert!(settled.manipulation_feedback().is_empty());
        assert!(!settled.workspace_changed());
        assert_eq!(session.version(), committed);
        assert_eq!(order(&session), [SECOND, THIRD, FIRST]);
        assert!(!session.has_pending_native_input());
        assert!(idle(&mut session).inputs().is_empty());
    }
}

#[test]
fn native_touch_taps_inside_slop_and_yields_beyond_it_before_the_real_contact_end() {
    for lane in [Lane::Local, Lane::Desktop] {
        for (timed, distance) in [(true, 8.0), (false, 8.0), (true, 9.0)] {
            let (mut session, binding, scene) = fixture(lane == Lane::Desktop);
            let version = session.version();
            let point = scene.point(SECOND, 0.25);
            let metadata = |millis| {
                if timed {
                    at(Device::Touch, millis)
                } else {
                    PointerEventMetadata::untimed(Device::Touch)
                }
            };
            session
                .record_native_pointer(input(&session, lane, binding, PRESS, point, metadata(0)))
                .unwrap();
            assert!(reduce(&mut session, &scene).inputs().is_empty());
            assert!(session.view().item(FIRST).unwrap().is_selected());
            assert_eq!(session.native_pointer_continuation().is_some(), timed);
            let moved = LogicalPoint::new(point.x() + distance, point.y()).unwrap();
            assert!(scene.tab(SECOND).receiver.bounds().contains(moved));
            session
                .record_native_pointer(input(
                    &session,
                    lane,
                    binding,
                    Event::Moved,
                    moved,
                    metadata(100),
                ))
                .unwrap();
            let motion = reduce(&mut session, &scene);
            let yielded = distance > 8.0;
            if yielded {
                assert!(matches!(
                    motion.inputs(),
                    [HostInputOutcome::Touch(DockspaceTouchEvent::YieldedToContent { pointer, surface })]
                        if *pointer == SurfacePointerId::new(17) && *surface == SURFACE
                ));
                assert!(!session.has_active_gesture());
                assert!(session.native_pointer_continuation().is_none());
            } else if timed {
                let request = session.native_pointer_continuation().unwrap();
                assert_eq!(request.deadline(), Duration::from_millis(350));
                assert_eq!(request.position(), moved);
            }
            assert_eq!(session.version(), version);
            session
                .record_native_pointer(input(&session, lane, binding, END, moved, metadata(200)))
                .unwrap();
            assert!(session.native_pointer_continuation().is_none());
            let mut frame = session
                .begin_native_host_frame(|query| scene.answer(query))
                .unwrap();
            measure(&mut frame, 800.0);
            let terminal = finish(frame);
            assert_eq!(
                selections(&terminal),
                if yielded { vec![] } else { vec![SECOND] }
            );
            assert_eq!(terminal.workspace_changed(), !yielded);
            assert_eq!(session.view().item(SECOND).unwrap().is_selected(), !yielded);
            assert!(!session.has_active_gesture());
            assert!(!session.has_pending_native_input());
            assert!(session.native_pointer_continuation().is_none());
            assert!(idle(&mut session).inputs().is_empty());
        }
    }
}

#[test]
fn native_dwell_unknown_delivery_preserves_the_held_gesture_for_rearm_and_release() {
    for lane in [Lane::Local, Lane::Desktop] {
        for known_capture in [false, true] {
            let (mut session, binding, scene) = fixture(lane == Lane::Desktop);
            let target = scene.point(SECOND, 0.625);
            let version = session.version();
            let request = start_drag(&mut session, lane, binding, &scene, target);
            let capture = if known_capture {
                NativePointerOwner::Native(binding)
            } else {
                NativePointerOwner::Unknown
            };
            session
                .record_native_pointer(continuation_with_owners(
                    request,
                    request.deadline(),
                    NativePointerOwner::Unknown,
                    capture,
                ))
                .unwrap();
            let observed = idle(&mut session);
            assert!(!observed.workspace_changed(), "{lane:?}/{capture:?}");
            assert!(observed.inputs().is_empty());
            assert_eq!(session.version(), version);
            assert!(session.view().item(FIRST).unwrap().is_selected());
            assert!(!session.view().item(SECOND).unwrap().is_selected());
            assert!(session.native_pointer_continuation().is_none());
            assert!(session.has_active_gesture(), "{lane:?}/{capture:?}");
            assert!(!session.has_pending_native_input());

            session
                .record_native_pointer(continuation(
                    request,
                    Duration::from_millis(161),
                    Device::Mouse,
                    NativePointerOwner::Native(binding),
                ))
                .unwrap();
            let duplicate = idle(&mut session);
            assert!(duplicate.inputs().is_empty());
            assert!(!duplicate.workspace_changed());
            assert_eq!(session.version(), version);
            assert!(session.native_pointer_continuation().is_none());
            assert!(session.has_active_gesture());

            session
                .record_native_pointer(input(
                    &session,
                    lane,
                    binding,
                    Event::Moved,
                    target,
                    at(Device::Mouse, 170),
                ))
                .unwrap();
            assert!(!reduce(&mut session, &scene).workspace_changed());
            let rearmed = session.native_pointer_continuation().unwrap();
            assert_ne!(rearmed.token(), request.token());
            assert_eq!(rearmed.position(), target);
            assert!(session.has_active_gesture());
            session
                .record_native_pointer(continuation(
                    rearmed,
                    rearmed.deadline(),
                    Device::Mouse,
                    NativePointerOwner::Native(binding),
                ))
                .unwrap();
            let mut frame = session
                .begin_native_host_frame(|query| scene.answer(query))
                .unwrap();
            measure(&mut frame, 800.0);
            let selected = finish(frame);
            assert_eq!(selections(&selected), [SECOND]);
            assert!(selected.workspace_changed());
            assert!(session.has_active_gesture());
            assert!(session.native_pointer_continuation().is_none());
            let (current, output) = paint(&mut session, binding);
            present(&mut session, output);
            let release_version = session.version();

            session
                .record_native_pointer(input(
                    &session,
                    lane,
                    binding,
                    RELEASE,
                    target,
                    PointerEventMetadata::new(
                        Device::Mouse,
                        rearmed.deadline() + Duration::from_millis(1),
                    ),
                ))
                .unwrap();
            let mut frame = session
                .begin_native_host_frame(|query| current.answer(query))
                .unwrap();
            let successor = render(&mut frame, 800.0);
            successor.assert_no_preview();
            let mut report = finish(frame);
            let [terminal] = report.manipulation_feedback() else {
                panic!("the surviving gesture must deliver in its release update: {report:?}");
            };
            assert_eq!(terminal.status(), Status::Delivered(Delivery::Dock));
            assert!(report.workspace_changed());
            assert_eq!(report.before(), release_version);
            assert_eq!(report.after(), session.version());
            assert_ne!(session.version(), release_version);
            let committed = session.version();
            assert!(!session.has_active_gesture());
            assert_eq!(order(&session), [SECOND, FIRST, THIRD]);
            let output = take_output(&mut report, &successor, binding);
            let settled = present(&mut session, output);
            assert!(settled.manipulation_feedback().is_empty());
            assert!(!settled.workspace_changed());
            assert_eq!(session.version(), committed);
            assert_eq!(order(&session), [SECOND, FIRST, THIRD]);
            assert!(!session.has_pending_native_input());
            assert!(session.native_pointer_continuation().is_none());
            assert!(idle(&mut session).inputs().is_empty());
        }
    }
}

#[test]
fn native_dwell_known_owner_loss_still_cancels_with_the_other_owner_unknown() {
    for lane in [Lane::Local, Lane::Desktop] {
        for lost in [NativePointerOwner::None, NativePointerOwner::Foreign] {
            for delivery_lost in [false, true] {
                let (mut session, binding, scene) = fixture(lane == Lane::Desktop);
                let target = scene.point(SECOND, 0.625);
                let version = session.version();
                let request = start_drag(&mut session, lane, binding, &scene, target);
                let (delivery, capture) = if delivery_lost {
                    (lost, NativePointerOwner::Unknown)
                } else {
                    (NativePointerOwner::Unknown, lost)
                };
                session
                    .record_native_pointer(continuation_with_owners(
                        request,
                        request.deadline(),
                        delivery,
                        capture,
                    ))
                    .unwrap();
                let report = idle(&mut session);
                assert!(!report.workspace_changed());
                assert!(selections(&report).is_empty());
                assert_eq!(session.version(), version);
                assert!(session.view().item(FIRST).unwrap().is_selected());
                assert!(!session.view().item(SECOND).unwrap().is_selected());
                assert!(
                    !session.has_active_gesture(),
                    "{lane:?}/{delivery:?}/{capture:?}"
                );
                assert!(session.native_pointer_continuation().is_none());
                assert!(!session.has_pending_native_input());
            }
        }
    }
}

#[test]
fn native_dwell_revokes_unknown_capture_or_receiver_and_cancels_foreign_capture() {
    for lane in [Lane::Local, Lane::Desktop] {
        for failure in [
            "unknown-capture",
            "foreign-capture",
            "blocked",
            "unknown-receiver",
            "stale-receiver",
        ] {
            let (mut session, binding, scene) = fixture(lane == Lane::Desktop);
            let target = scene.point(SECOND, 0.625);
            let version = session.version();
            let request = start_drag(&mut session, lane, binding, &scene, target);
            let stale = session.presented_surface(SURFACE).unwrap();
            if failure == "stale-receiver" {
                let (_, output) = paint(&mut session, binding);
                present(&mut session, output);
                assert_ne!(session.presented_surface(SURFACE), Some(stale));
            }
            let capture = match failure {
                "unknown-capture" => NativePointerOwner::Unknown,
                "foreign-capture" => NativePointerOwner::Foreign,
                _ => NativePointerOwner::Native(binding),
            };
            session
                .record_native_pointer(continuation(
                    request,
                    request.deadline(),
                    Device::Mouse,
                    capture,
                ))
                .unwrap();
            let report = finish(
                session
                    .begin_native_host_frame(|query| match failure {
                        "unknown-capture" | "foreign-capture" => {
                            panic!("unproven native capture must not generate receiver challenges")
                        }
                        "blocked" => NativeReceiverAnswer::Blocked(query.presented_surface()),
                        "stale-receiver" => NativeReceiverAnswer::ClearHoverLane(stale),
                        _ => NativeReceiverAnswer::Unknown,
                    })
                    .unwrap(),
            );
            assert!(!report.workspace_changed(), "{lane:?}/{failure}");
            assert_eq!(session.version(), version);
            assert!(session.view().item(FIRST).unwrap().is_selected());
            assert!(!session.view().item(SECOND).unwrap().is_selected());
            assert!(session.native_pointer_continuation().is_none());
            assert_eq!(session.has_active_gesture(), failure != "foreign-capture");
            if failure != "foreign-capture" {
                session
                    .record_native_pointer(continuation(
                        request,
                        Duration::from_millis(161),
                        Device::Mouse,
                        NativePointerOwner::Native(binding),
                    ))
                    .unwrap();
                assert!(!idle(&mut session).workspace_changed());
                assert!(session.native_pointer_continuation().is_none());
                assert!(!session.view().item(SECOND).unwrap().is_selected());
            }
            session
                .record_native_pointer(input(
                    &session,
                    lane,
                    binding,
                    RELEASE,
                    target,
                    at(Device::Mouse, 162),
                ))
                .unwrap();
            let report = finish(
                session
                    .begin_native_host_frame(|query| {
                        NativeReceiverAnswer::Blocked(query.presented_surface())
                    })
                    .unwrap(),
            );
            assert!(!report.workspace_changed());
            assert!(!session.has_active_gesture());
            assert!(!session.has_pending_native_input());
        }
    }
}

#[test]
fn wrong_native_receiver_winner_rolls_back_the_due_input_and_retries_with_current_descriptors() {
    for lane in [Lane::Local, Lane::Desktop] {
        let (mut session, binding, scene) = fixture(lane == Lane::Desktop);
        let target = scene.point(SECOND, 0.625);
        let request = start_drag(&mut session, lane, binding, &scene, target);
        let old = session.presented_surface(SURFACE).unwrap();
        let (current, output) = paint(&mut session, binding);
        present(&mut session, output);
        assert_ne!(session.presented_surface(SURFACE), Some(old));
        assert_eq!(session.native_pointer_continuation(), Some(request));
        let version = session.version();
        session
            .record_native_pointer(continuation(
                request,
                request.deadline(),
                Device::Mouse,
                NativePointerOwner::Native(binding),
            ))
            .unwrap();
        let recorded = session.native_input_recorded_count();
        assert!(
            session
                .begin_native_host_frame(|query| {
                    assert_eq!(query.point(), Some(target));
                    NativeReceiverAnswer::NoReceiver(query.presented_surface())
                })
                .is_err()
        );
        assert_eq!(session.version(), version);
        assert_eq!(session.native_input_recorded_count(), recorded);
        assert!(session.has_pending_native_input());
        assert!(session.has_active_gesture());
        assert!(session.view().item(FIRST).unwrap().is_selected());
        let mut retry = session
            .begin_native_host_frame(|query| current.answer(query))
            .unwrap();
        measure(&mut retry, 800.0);
        assert_eq!(selections(&finish(retry)), [SECOND]);
        assert_eq!(session.native_input_recorded_count(), recorded);
        assert!(!session.has_pending_native_input());
        assert!(session.has_active_gesture());
    }
}

#[test]
fn same_native_pointer_scroll_revokes_touch_deadline_and_observes_capture_loss() {
    for lane in [Lane::Local, Lane::Desktop] {
        for capture in [NativePointerOwner::Unknown, NativePointerOwner::Foreign] {
            let (mut session, binding, scene) = fixture(lane == Lane::Desktop);
            let source = scene.point(FIRST, 0.25);
            session
                .record_native_pointer(input(
                    &session,
                    lane,
                    binding,
                    PRESS,
                    source,
                    at(Device::Touch, 0),
                ))
                .unwrap();
            reduce(&mut session, &scene);
            let request = session.native_pointer_continuation().unwrap();
            let scroll = Event::Scrolled(NativeScrollEvent::new(
                NativeScrollDeviceId::new(1),
                NativeScrollPhase::Discrete {
                    delta: NativeScrollDelta::Lines { x: 0.0, y: 1.0 },
                },
                NativeScrollMomentum::Unknown,
                NativeScrollModifiers::Unknown,
            ));
            session
                .record_native_pointer(observed_input(
                    request.local_coordinates(),
                    binding,
                    scroll,
                    source,
                    capture,
                    at(Device::Touch, 20),
                ))
                .unwrap();
            let report = finish(
                session
                    .begin_native_host_frame(|query| {
                        assert!(matches!(
                            query.purpose(),
                            NativeReceiverPurpose::ScrollDelivery(_)
                        ));
                        NativeReceiverAnswer::Unknown
                    })
                    .unwrap(),
            );
            assert!(!report.workspace_changed());
            assert_eq!(
                session.has_active_gesture(),
                capture == NativePointerOwner::Unknown
            );
            assert!(session.native_pointer_continuation().is_none());
            session
                .record_native_pointer(observed_input(
                    request.local_coordinates(),
                    binding,
                    Event::CaptureChanged,
                    source,
                    NativePointerOwner::Native(binding),
                    at(Device::Touch, 30),
                ))
                .unwrap();
            idle(&mut session);
            assert!(session.native_pointer_continuation().is_none());
            session
                .record_native_pointer(continuation(
                    request,
                    request.deadline(),
                    Device::Touch,
                    NativePointerOwner::Native(binding),
                ))
                .unwrap();
            assert!(idle(&mut session).inputs().is_empty());
            session
                .record_native_pointer(input(
                    &session,
                    lane,
                    binding,
                    END,
                    source,
                    at(Device::Touch, 360),
                ))
                .unwrap();
            reduce(&mut session, &scene);
            assert!(!session.has_active_gesture());
            assert!(!session.has_pending_native_input());
        }
    }
}

#[test]
fn non_primary_native_capture_edges_revoke_the_old_wake_and_cancel_known_loss() {
    for lane in [Lane::Local, Lane::Desktop] {
        for capture in [
            NativePointerOwner::Unknown,
            NativePointerOwner::None,
            NativePointerOwner::Foreign,
        ] {
            let (mut session, binding, scene) = fixture(lane == Lane::Desktop);
            let target = scene.point(SECOND, 0.625);
            let request = start_drag(&mut session, lane, binding, &scene, target);
            let version = session.version();
            session
                .record_native_pointer(observed_input(
                    request.local_coordinates(),
                    binding,
                    Event::ButtonPressed(Button::Secondary),
                    target,
                    capture,
                    at(Device::Mouse, 20),
                ))
                .unwrap();
            assert!(!idle(&mut session).workspace_changed());
            assert!(session.native_pointer_continuation().is_none());
            assert_eq!(
                session.has_active_gesture(),
                capture == NativePointerOwner::Unknown
            );
            session
                .record_native_pointer(observed_input(
                    request.local_coordinates(),
                    binding,
                    Event::ButtonReleased(Button::Secondary),
                    target,
                    NativePointerOwner::Native(binding),
                    at(Device::Mouse, 30),
                ))
                .unwrap();
            assert!(!idle(&mut session).workspace_changed());
            assert!(
                session.native_pointer_continuation().is_none(),
                "restoring capture must not resurrect an old challenge"
            );
            session
                .record_native_pointer(continuation(
                    request,
                    request.deadline(),
                    Device::Mouse,
                    NativePointerOwner::Native(binding),
                ))
                .unwrap();
            assert!(!idle(&mut session).workspace_changed());
            assert!(session.view().item(FIRST).unwrap().is_selected());
            assert!(!session.view().item(SECOND).unwrap().is_selected());
            assert_eq!(session.version(), version);
            session
                .record_native_pointer(input(
                    &session,
                    lane,
                    binding,
                    END,
                    target,
                    at(Device::Mouse, 170),
                ))
                .unwrap();
            let report = finish(
                session
                    .begin_native_host_frame(|query| {
                        NativeReceiverAnswer::Blocked(query.presented_surface())
                    })
                    .unwrap(),
            );
            assert!(!report.workspace_changed());
            assert!(!session.has_active_gesture());
            assert!(!session.has_pending_native_input());
            assert!(session.native_pointer_continuation().is_none());
        }
    }
}

#[test]
fn native_continuation_cannot_borrow_another_pointer_or_reused_stream() {
    for lane in [Lane::Local, Lane::Desktop] {
        for reused_stream in [false, true] {
            let (mut session, binding, scene) = fixture(lane == Lane::Desktop);
            let target = scene.point(SECOND, 0.625);
            let request = start_drag(&mut session, lane, binding, &scene, target);
            if reused_stream {
                session
                    .record_native_pointer(input(
                        &session,
                        lane,
                        binding,
                        END,
                        target,
                        at(Device::Mouse, 20),
                    ))
                    .unwrap();
                finish(
                    session
                        .begin_native_host_frame(|query| {
                            NativeReceiverAnswer::Blocked(query.presented_surface())
                        })
                        .unwrap(),
                );
                assert!(!session.has_active_gesture());
                session
                    .record_native_pointer(input(
                        &session,
                        lane,
                        binding,
                        PRESS,
                        scene.point(FIRST, 0.25),
                        at(Device::Mouse, 30),
                    ))
                    .unwrap();
                reduce(&mut session, &scene);
                assert!(session.has_active_gesture());
            }
            let version = session.version();
            let presented = session.presented_surface(SURFACE);
            let before = session.native_input_recorded_count();
            let pointer = if reused_stream {
                POINTER
            } else {
                NativePointerId::new(18)
            };
            let invalid = match request.local_coordinates() {
                Some(coordinates) => NativePointerInput::new_local(
                    pointer,
                    request.event(),
                    NativeLocalPointerLocation::Exact(sample(coordinates, binding, target)),
                    NativePointerOwner::Native(binding),
                    NativePointerOwner::Native(binding),
                ),
                None => NativePointerInput::new(
                    pointer,
                    request.event(),
                    desktop_location(binding, target),
                    NativePointerOwner::Native(binding),
                    NativePointerOwner::Native(binding),
                ),
            }
            .with_metadata(at(Device::Mouse, 160));
            session.record_native_pointer(invalid).unwrap();
            let recorded = session.native_input_recorded_count();
            assert_eq!(recorded, before + 1);
            for _ in 0..2 {
                assert!(
                    session
                        .begin_native_host_frame(|_| panic!(
                            "another stream's token must not query receivers"
                        ))
                        .is_err()
                );
                assert_eq!(session.version(), version);
                assert_eq!(session.presented_surface(SURFACE), presented);
                assert_eq!(session.native_input_recorded_count(), recorded);
                assert!(session.has_pending_native_input());
                assert!(session.has_active_gesture());
                assert_eq!(order(&session), [FIRST, SECOND, THIRD]);
                assert!(!session.view().item(SECOND).unwrap().is_selected());
            }
        }
    }
}

#[test]
fn native_local_coordinate_rebase_revokes_the_wake_and_drains_the_old_sample_terminal() {
    let (mut session, binding, scene) = fixture(false);
    let target = scene.point(SECOND, 0.625);
    let request = start_drag(&mut session, Lane::Local, binding, &scene, target);
    let coordinates = request.local_coordinates().unwrap();
    let version = session.version();
    session
        .report_managed_native_snapshot(
            [(
                binding,
                window_facts(false).with_presentation_scale_factor(ScaleFactor::new(2.0).unwrap()),
            )],
            NativeWorkAreaRoster::Unknown,
        )
        .unwrap();
    assert!(session.native_pointer_continuation().is_none());
    assert!(!idle(&mut session).workspace_changed());
    assert_ne!(session.native_local_coordinates(binding), Some(coordinates));
    assert!(session.native_pointer_continuation().is_none());
    session
        .record_native_pointer(observed_input(
            Some(coordinates),
            binding,
            RELEASE,
            target,
            NativePointerOwner::None,
            at(Device::Mouse, 160),
        ))
        .unwrap();
    let terminal = idle(&mut session);
    assert!(!terminal.workspace_changed());
    assert_eq!(session.version(), version);
    assert!(session.view().item(FIRST).unwrap().is_selected());
    assert_eq!(order(&session), [FIRST, SECOND, THIRD]);
    assert!(!session.has_active_gesture());
    assert!(!session.has_pending_native_input());
    assert!(session.native_pointer_continuation().is_none());
}

#[test]
fn an_obsolete_local_token_after_handoff_cannot_cancel_the_desktop_drag() {
    let (mut session, binding, scene) = fixture(true);
    let target = scene.point(SECOND, 0.625);
    let local = start_drag(&mut session, Lane::Local, binding, &scene, target);
    let packet = NativeLocalToDesktopHandoff::new(
        sample(local.local_coordinates().unwrap(), binding, target),
        desktop_location(binding, target),
    );
    session
        .record_native_pointer(
            NativePointerInput::new_handoff(
                POINTER,
                Event::Moved,
                packet,
                NativePointerOwner::Native(binding),
                NativePointerOwner::Native(binding),
            )
            .with_metadata(at(Device::Mouse, 20)),
        )
        .unwrap();
    assert!(!reduce(&mut session, &scene).workspace_changed());
    let desktop = session.native_pointer_continuation().unwrap();
    assert!(desktop.local_coordinates().is_none());
    assert_ne!(desktop.token(), local.token());
    assert_eq!(desktop.deadline(), Duration::from_millis(170));
    let version = session.version();
    session
        .record_native_pointer(continuation(
            local,
            local.deadline(),
            Device::Mouse,
            NativePointerOwner::Native(binding),
        ))
        .unwrap();
    assert!(idle(&mut session).inputs().is_empty());
    assert_eq!(session.version(), version);
    assert!(session.has_active_gesture());
    assert_eq!(session.native_pointer_continuation(), Some(desktop));
    session
        .record_native_pointer(input(
            &session,
            Lane::Desktop,
            binding,
            RELEASE,
            target,
            at(Device::Mouse, 169),
        ))
        .unwrap();
    assert!(session.native_pointer_continuation().is_none());
    let mut frame = session
        .begin_native_host_frame(|query| scene.answer(query))
        .unwrap();
    let successor = render(&mut frame, 800.0);
    successor.assert_no_preview();
    let mut report = finish(frame);
    let [terminal] = report.manipulation_feedback() else {
        panic!("the surviving desktop owner must deliver in its release update: {report:?}");
    };
    assert_eq!(terminal.status(), Status::Delivered(Delivery::Dock));
    assert!(report.workspace_changed());
    assert_eq!(report.before(), version);
    assert_eq!(report.after(), session.version());
    assert_ne!(session.version(), version);
    assert_eq!(order(&session), [SECOND, FIRST, THIRD]);
    let committed = session.version();
    let output = take_output(&mut report, &successor, binding);
    let settled = present(&mut session, output);
    assert!(settled.manipulation_feedback().is_empty());
    assert!(!settled.workspace_changed());
    assert_eq!(session.version(), committed);
    assert_eq!(order(&session), [SECOND, FIRST, THIRD]);
    assert!(!session.has_active_gesture());
}

#[test]
fn native_pointer_capture_request_precedes_timed_work_and_keeps_exact_drag_identity() {
    let (mut session, binding, scene) = fixture(true);
    let version = session.version();
    assert!(session.native_pointer_capture_request().is_none());
    session
        .record_native_pointer(input(
            &session,
            Lane::Desktop,
            binding,
            PRESS,
            scene.point(FIRST, 0.25),
            at(Device::Mouse, 0),
        ))
        .unwrap();
    assert!(!reduce(&mut session, &scene).workspace_changed());
    assert!(session.has_active_gesture());
    assert!(session.native_pointer_capture_request().is_none());
    session
        .record_native_pointer(input(
            &session,
            Lane::Desktop,
            binding,
            Event::Moved,
            scene.point(FIRST, 0.75),
            at(Device::Mouse, 10),
        ))
        .unwrap();
    assert!(session.native_pointer_capture_request().is_none());
    assert!(!reduce(&mut session, &scene).workspace_changed());
    assert!(session.has_active_gesture());
    assert!(session.native_pointer_continuation().is_none());
    let capture: NativePointerCaptureRequest = session.native_pointer_capture_request().unwrap();
    assert_eq!(capture.pointer(), POINTER);
    assert_eq!(capture.source_binding(), binding);
    assert_eq!(capture.source_input(), NativeDragSourceInput::Preserve);
    let recorded = session.native_input_recorded_count();
    for _ in 0..3 {
        assert_eq!(session.native_pointer_capture_request(), Some(capture));
        assert!(session.native_pointer_continuation().is_none());
        assert_eq!(session.native_input_recorded_count(), recorded);
        assert_eq!(session.version(), version);
    }

    let mut previous = None;
    for (millis, item) in [(40, SECOND), (70, THIRD)] {
        session
            .record_native_pointer(input(
                &session,
                Lane::Desktop,
                binding,
                Event::Moved,
                scene.point(item, 0.625),
                at(Device::Mouse, millis),
            ))
            .unwrap();
        assert!(session.has_pending_native_input());
        assert!(session.native_pointer_capture_request().is_none());
        drop(
            session
                .begin_native_host_frame(|query| scene.answer(query))
                .unwrap(),
        );
        assert!(session.has_pending_native_input());
        assert!(session.native_pointer_capture_request().is_none());
        assert!(!reduce(&mut session, &scene).workspace_changed());
        assert_eq!(session.native_pointer_capture_request(), Some(capture));
        let timed = session.native_pointer_continuation().unwrap();
        assert_ne!(Some(timed.token()), previous);
        previous = Some(timed.token());
    }

    // A later finite action still records the first real acquisition normally.
    let timed = session.native_pointer_continuation().unwrap();
    session
        .record_native_pointer(input(
            &session,
            Lane::Desktop,
            binding,
            timed.capture_acquired_event().unwrap(),
            timed.position(),
            at(Device::Mouse, 240),
        ))
        .unwrap();
    assert!(idle(&mut session).inputs().is_empty());
    let restarted = session.native_pointer_continuation().unwrap();
    assert_ne!(restarted.token(), timed.token());
    assert_eq!(restarted.deadline(), Duration::from_millis(390));
    assert!(restarted.capture_acquired_event().is_none());
    assert_eq!(session.native_pointer_capture_request(), Some(capture));

    session
        .record_native_pointer(input(
            &session,
            Lane::Desktop,
            binding,
            RELEASE,
            timed.position(),
            at(Device::Mouse, 250),
        ))
        .unwrap();
    let released = finish(
        session
            .begin_native_host_frame(|query| {
                NativeReceiverAnswer::Blocked(query.presented_surface())
            })
            .unwrap(),
    );
    assert!(!released.workspace_changed());
    assert!(!session.has_active_gesture());
    assert!(!session.has_pending_native_input());
    assert!(session.native_pointer_capture_request().is_none());

    session
        .record_native_pointer_batch([
            input(
                &session,
                Lane::Desktop,
                binding,
                PRESS,
                scene.point(FIRST, 0.25),
                at(Device::Mouse, 260),
            ),
            input(
                &session,
                Lane::Desktop,
                binding,
                Event::Moved,
                timed.position(),
                at(Device::Mouse, 270),
            ),
        ])
        .unwrap();
    assert!(!reduce(&mut session, &scene).workspace_changed());
    let next = session.native_pointer_capture_request().unwrap();
    assert_eq!(next.pointer(), capture.pointer());
    assert_eq!(next.source_binding(), capture.source_binding());
    assert_eq!(next.source_input(), capture.source_input());
    assert_ne!(
        next, capture,
        "a new drag must differ within the same pointer stream"
    );
}

#[test]
fn native_pointer_capture_request_excludes_local_mouse_and_native_touch_drags() {
    for (lane, device) in [
        (Lane::Local, Device::Mouse),
        (Lane::Local, Device::Touch),
        (Lane::Desktop, Device::Touch),
    ] {
        let (mut session, binding, scene) = fixture(lane == Lane::Desktop);
        let target = scene.point(SECOND, 0.625);
        if device == Device::Touch {
            session
                .record_native_pointer(input(
                    &session,
                    lane,
                    binding,
                    PRESS,
                    scene.point(FIRST, 0.25),
                    at(device, 0),
                ))
                .unwrap();
            assert!(!reduce(&mut session, &scene).workspace_changed());
            assert!(session.native_pointer_capture_request().is_none());
            let pressed = session.native_pointer_continuation().unwrap();
            session
                .record_native_pointer(continuation(
                    pressed,
                    pressed.deadline(),
                    device,
                    NativePointerOwner::Native(binding),
                ))
                .unwrap();
            assert!(!reduce(&mut session, &scene).workspace_changed());
            assert!(session.native_pointer_capture_request().is_none());
            session
                .record_native_pointer(input(
                    &session,
                    lane,
                    binding,
                    Event::Moved,
                    target,
                    at(device, 360),
                ))
                .unwrap();
            assert!(!reduce(&mut session, &scene).workspace_changed());
            assert_eq!(
                session.native_pointer_continuation().unwrap().deadline(),
                Duration::from_millis(510)
            );
        } else {
            start_drag(&mut session, lane, binding, &scene, target);
        }
        assert!(session.has_active_gesture());
        assert!(!session.has_pending_native_input());
        assert!(
            session.native_pointer_capture_request().is_none(),
            "{lane:?}/{device:?}"
        );
    }
}

#[test]
fn native_pointer_capture_request_rejects_foreign_sources_and_ends_on_cancel_or_retirement() {
    let (_, foreign, _) = fixture(true);
    for retire_binding in [false, true] {
        let (mut session, binding, scene) = fixture(true);
        let target = scene.point(SECOND, 0.625);
        start_drag(&mut session, Lane::Desktop, binding, &scene, target);
        let capture = session.native_pointer_capture_request().unwrap();
        assert_ne!(binding, foreign);
        assert!(!session.is_current_native_binding(foreign));
        let recorded = session.native_input_recorded_count();
        assert!(
            session
                .record_native_pointer(
                    NativePointerInput::new(
                        POINTER,
                        Event::CaptureChanged,
                        desktop_location(foreign, target),
                        NativePointerOwner::Native(foreign),
                        NativePointerOwner::Native(foreign),
                    )
                    .with_metadata(at(Device::Mouse, 20)),
                )
                .is_err()
        );
        assert!(
            session
                .report_native_input_binding_retired(foreign)
                .is_err()
        );
        assert_eq!(session.native_input_recorded_count(), recorded);
        assert!(!session.has_pending_native_input());
        assert_eq!(session.native_pointer_capture_request(), Some(capture));

        if retire_binding {
            session
                .report_native_input_binding_retired(binding)
                .unwrap();
        } else {
            session
                .record_native_pointer(input(
                    &session,
                    Lane::Desktop,
                    binding,
                    Event::StreamCancelled(NativePointerCancelReason::ExplicitPlatformCancellation),
                    target,
                    at(Device::Mouse, 20),
                ))
                .unwrap();
        }
        assert!(session.has_pending_native_input());
        assert!(session.native_pointer_capture_request().is_none());
        assert!(!idle(&mut session).workspace_changed());
        assert!(!session.has_pending_native_input());
        assert!(!session.has_active_gesture());
        assert!(session.native_pointer_capture_request().is_none());
    }
}

#[test]
fn native_capture_acquisition_restarts_overdue_dwell_without_executing_it() {
    let (mut session, binding, scene) = fixture(true);
    let target = scene.point(SECOND, 0.625);
    let version = session.version();
    let presented = session.presented_surface(SURFACE);
    let request = start_drag(&mut session, Lane::Desktop, binding, &scene, target);
    assert_eq!(request.deadline(), Duration::from_millis(160));
    assert_eq!(
        request.source_input(),
        Some(NativeDragSourceInput::Preserve)
    );
    let acquired = request.capture_acquired_event().unwrap();
    assert_eq!(acquired, Event::CaptureAcquired(request.token()));
    let recorded = session.native_input_recorded_count();
    for _ in 0..3 {
        assert_eq!(session.native_pointer_continuation(), Some(request));
        assert_eq!(request.capture_acquired_event(), Some(acquired));
        assert_eq!(session.native_input_recorded_count(), recorded);
        assert_eq!(session.version(), version);
    }

    session
        .record_native_pointer(input(
            &session,
            Lane::Desktop,
            binding,
            acquired,
            target,
            at(Device::Mouse, 240),
        ))
        .unwrap();
    assert!(session.native_pointer_continuation().is_none());
    let report = idle(&mut session);
    assert!(report.inputs().is_empty());
    assert!(report.manipulation_feedback().is_empty());
    assert!(!report.workspace_changed());
    assert_eq!(session.version(), version);
    assert_eq!(session.presented_surface(SURFACE), presented);
    assert_eq!(session.native_input_recorded_count(), recorded + 1);
    assert!(session.has_active_gesture());
    assert!(!session.has_pending_native_input());
    assert!(session.view().item(FIRST).unwrap().is_selected());
    assert!(!session.view().item(SECOND).unwrap().is_selected());
    assert_eq!(order(&session), [FIRST, SECOND, THIRD]);

    let restarted = session.native_pointer_continuation().unwrap();
    assert_ne!(restarted.token(), request.token());
    assert_eq!(restarted.deadline(), Duration::from_millis(390));
    assert_eq!(restarted.pointer(), POINTER);
    assert_eq!(restarted.surface(), SURFACE);
    assert_eq!(restarted.source_binding(), binding);
    assert_eq!(restarted.position(), target);
    assert!(restarted.local_coordinates().is_none());
    assert_eq!(restarted.event(), Event::Continuation(restarted.token()));
    assert_eq!(
        restarted.source_input(),
        Some(NativeDragSourceInput::Preserve)
    );
    assert!(restarted.capture_acquired_event().is_none());
    assert!(session.pointer_continuation().is_none());

    session
        .record_native_pointer(continuation(
            request,
            Duration::from_millis(240),
            Device::Mouse,
            NativePointerOwner::Native(binding),
        ))
        .unwrap();
    assert!(idle(&mut session).inputs().is_empty());
    assert_eq!(session.native_pointer_continuation(), Some(restarted));
    for (millis, expected) in [(389, vec![]), (390, vec![SECOND])] {
        session
            .record_native_pointer(continuation(
                restarted,
                Duration::from_millis(millis),
                Device::Mouse,
                NativePointerOwner::Native(binding),
            ))
            .unwrap();
        let report = finish(
            session
                .begin_native_host_frame(|query| {
                    assert_eq!(query.metadata(), at(Device::Mouse, millis));
                    assert_eq!(query.point(), Some(target));
                    assert!(query.matches_native_binding(binding));
                    scene.answer(query)
                })
                .unwrap(),
        );
        assert_eq!(selections(&report), expected);
        assert_eq!(report.workspace_changed(), millis == 390);
        assert!(report.manipulation_feedback().is_empty());
        if millis == 389 {
            assert_eq!(session.native_pointer_continuation(), Some(restarted));
            assert_eq!(session.version(), version);
        }
    }
    assert!(session.view().item(SECOND).unwrap().is_selected());
    assert_eq!(order(&session), [FIRST, SECOND, THIRD]);
    assert!(session.has_active_gesture());
    assert!(session.native_pointer_continuation().is_none());
}

#[test]
fn native_capture_acquisition_from_an_older_challenge_restarts_the_current_target_once() {
    let (mut session, binding, scene) = fixture(true);
    let old_target = scene.point(SECOND, 0.625);
    let target = scene.point(THIRD, 0.625);
    let version = session.version();
    let old = start_drag(&mut session, Lane::Desktop, binding, &scene, old_target);
    let old_acquired = old.capture_acquired_event().unwrap();
    session
        .record_native_pointer(input(
            &session,
            Lane::Desktop,
            binding,
            Event::Moved,
            target,
            at(Device::Mouse, 40),
        ))
        .unwrap();
    assert!(!reduce(&mut session, &scene).workspace_changed());
    let current = session.native_pointer_continuation().unwrap();
    assert_ne!(current.token(), old.token());
    assert_eq!(current.deadline(), Duration::from_millis(190));
    assert_eq!(current.position(), target);
    let current_acquired = current.capture_acquired_event().unwrap();
    session
        .record_native_pointer(input(
            &session,
            Lane::Desktop,
            binding,
            old_acquired,
            target,
            at(Device::Mouse, 240),
        ))
        .unwrap();
    let report = idle(&mut session);
    assert!(report.inputs().is_empty());
    assert!(report.manipulation_feedback().is_empty());
    let restarted = session.native_pointer_continuation().unwrap();
    assert_ne!(restarted.token(), old.token());
    assert_ne!(restarted.token(), current.token());
    assert_eq!(restarted.deadline(), Duration::from_millis(390));
    assert_eq!(restarted.position(), target);
    assert_eq!(restarted.source_binding(), binding);
    assert!(restarted.capture_acquired_event().is_none());

    for (event, metadata) in [
        (old_acquired, at(Device::Mouse, 260)),
        (current_acquired, at(Device::Mouse, 280)),
        (old_acquired, PointerEventMetadata::untimed(Device::Mouse)),
    ] {
        session
            .record_native_pointer(input(
                &session,
                Lane::Desktop,
                binding,
                event,
                target,
                metadata,
            ))
            .unwrap();
        let duplicate = idle(&mut session);
        assert!(duplicate.inputs().is_empty());
        assert!(duplicate.manipulation_feedback().is_empty());
        assert!(!duplicate.workspace_changed());
        assert_eq!(session.version(), version);
        assert!(session.view().item(FIRST).unwrap().is_selected());
        assert_eq!(session.native_pointer_continuation(), Some(restarted));
    }
    session
        .record_native_pointer(continuation(
            restarted,
            restarted.deadline(),
            Device::Mouse,
            NativePointerOwner::Native(binding),
        ))
        .unwrap();
    let selected = reduce(&mut session, &scene);
    assert_eq!(selections(&selected), [THIRD]);
    assert!(selected.workspace_changed());
    assert!(!session.view().item(SECOND).unwrap().is_selected());
    assert!(session.view().item(THIRD).unwrap().is_selected());
    assert_eq!(order(&session), [FIRST, SECOND, THIRD]);
    assert!(session.has_active_gesture());
    assert!(session.native_pointer_continuation().is_none());
}

#[test]
fn native_capture_acquisition_retries_dropped_frames_and_rejects_foreign_or_stale_sources() {
    let (mut session, binding, scene) = fixture(true);
    let target = scene.point(SECOND, 0.625);
    let version = session.version();
    let presented = session.presented_surface(SURFACE);
    let request = start_drag(&mut session, Lane::Desktop, binding, &scene, target);
    let acquired = request.capture_acquired_event().unwrap();
    let (_, foreign, _) = fixture(true);
    let before = session.native_input_recorded_count();
    for delivery_foreign in [false, true] {
        let (delivery, capture) = if delivery_foreign {
            (
                NativePointerOwner::Native(foreign),
                NativePointerOwner::Native(binding),
            )
        } else {
            (
                NativePointerOwner::Native(binding),
                NativePointerOwner::Native(foreign),
            )
        };
        let invalid = NativePointerInput::new(
            POINTER,
            acquired,
            desktop_location(binding, target),
            delivery,
            capture,
        )
        .with_metadata(at(Device::Mouse, 240));
        assert!(session.record_native_pointer(invalid).is_err());
        assert_eq!(session.native_input_recorded_count(), before);
        assert!(!session.has_pending_native_input());
        assert_eq!(session.native_pointer_continuation(), Some(request));
    }

    session
        .record_native_pointer(input(
            &session,
            Lane::Desktop,
            binding,
            acquired,
            target,
            at(Device::Mouse, 240),
        ))
        .unwrap();
    let recorded = session.native_input_recorded_count();
    assert_eq!(recorded, before + 1);
    let abandoned = session
        .begin_native_host_frame(|_| panic!("capture acquisition must not query receivers"))
        .unwrap();
    assert!(abandoned.view().item(FIRST).unwrap().is_selected());
    assert!(!abandoned.view().item(SECOND).unwrap().is_selected());
    drop(abandoned);
    assert_eq!(session.version(), version);
    assert_eq!(session.presented_surface(SURFACE), presented);
    assert!(session.view().item(FIRST).unwrap().is_selected());
    assert!(session.has_active_gesture());
    assert!(session.has_pending_native_input());
    assert_eq!(session.native_input_recorded_count(), recorded);
    assert!(session.native_pointer_continuation().is_none());

    let retried = idle(&mut session);
    assert!(retried.inputs().is_empty());
    assert!(retried.manipulation_feedback().is_empty());
    assert!(!retried.workspace_changed());
    assert_eq!(session.native_input_recorded_count(), recorded);
    assert!(!session.has_pending_native_input());
    let restarted = session.native_pointer_continuation().unwrap();
    assert_ne!(restarted.token(), request.token());
    assert_eq!(restarted.deadline(), Duration::from_millis(390));
    assert!(restarted.capture_acquired_event().is_none());
    session
        .record_native_pointer(input(
            &session,
            Lane::Desktop,
            binding,
            acquired,
            target,
            at(Device::Mouse, 260),
        ))
        .unwrap();
    assert!(idle(&mut session).inputs().is_empty());
    assert_eq!(session.native_pointer_continuation(), Some(restarted));

    session
        .record_native_pointer(input(
            &session,
            Lane::Desktop,
            binding,
            RELEASE,
            target,
            at(Device::Mouse, 270),
        ))
        .unwrap();
    let released = finish(
        session
            .begin_native_host_frame(|query| {
                NativeReceiverAnswer::Blocked(query.presented_surface())
            })
            .unwrap(),
    );
    assert!(!released.workspace_changed());
    assert!(!session.has_active_gesture());
    session
        .record_native_pointer_batch([
            input(
                &session,
                Lane::Desktop,
                binding,
                PRESS,
                scene.point(FIRST, 0.25),
                at(Device::Mouse, 280),
            ),
            input(
                &session,
                Lane::Desktop,
                binding,
                Event::Moved,
                target,
                at(Device::Mouse, 290),
            ),
        ])
        .unwrap();
    assert!(!reduce(&mut session, &scene).workspace_changed());
    let new_gesture = session.native_pointer_continuation().unwrap();
    assert_ne!(new_gesture.token(), restarted.token());
    assert_eq!(new_gesture.deadline(), Duration::from_millis(440));
    assert!(new_gesture.capture_acquired_event().is_some());

    // Primary release preserves the pointer stream, but the drag session is new.
    session
        .record_native_pointer(input(
            &session,
            Lane::Desktop,
            binding,
            acquired,
            target,
            at(Device::Mouse, 300),
        ))
        .unwrap();
    let stale = idle(&mut session);
    assert!(stale.inputs().is_empty());
    assert!(stale.manipulation_feedback().is_empty());
    assert_eq!(session.native_pointer_continuation(), Some(new_gesture));
    assert!(session.has_active_gesture());
    assert_eq!(session.version(), version);
    assert_eq!(order(&session), [FIRST, SECOND, THIRD]);
    session
        .record_native_pointer(input(
            &session,
            Lane::Desktop,
            binding,
            new_gesture.capture_acquired_event().unwrap(),
            target,
            at(Device::Mouse, 310),
        ))
        .unwrap();
    assert!(idle(&mut session).inputs().is_empty());
    let current = session.native_pointer_continuation().unwrap();
    assert_eq!(current.deadline(), Duration::from_millis(460));
    assert_ne!(current.token(), new_gesture.token());
    assert!(current.capture_acquired_event().is_none());
}

#[test]
fn native_capture_acquisition_restarts_edge_scroll_and_waits_for_the_exact_output() {
    fn strip(frame: &mut DockspaceHostFrame<'_>) -> (LogicalRect, f64, f64) {
        let plan = frame.paint_plan(SURFACE).unwrap().unwrap();
        let mut strip = None;
        plan.visit_surface_program(|step| {
            if let Step::Operation {
                operation: Operation::TabBar(bar),
                ..
            } = step
            {
                assert!(strip.is_none());
                strip = Some((
                    bar.viewport(),
                    bar.scroll_offset(),
                    bar.maximum_scroll_offset(),
                ));
            }
            Ok::<(), ()>(())
        })
        .unwrap();
        strip.unwrap()
    }

    let (mut session, binding, _) = fixture(true);
    let version = session.version();
    session
        .report_managed_native_snapshot(
            [(
                binding,
                window_facts(true)
                    .with_content_size(PhysicalSize::new(300.0, 900.0).unwrap())
                    .with_content_bounds(PhysicalRect::new(400.0, -300.0, 300.0, 900.0).unwrap()),
            )],
            NativeWorkAreaRoster::Unknown,
        )
        .unwrap();
    let mut frame = session
        .begin_native_host_frame(|_| panic!("measurement must not query receivers"))
        .unwrap();
    measure(&mut frame, 200.0);
    assert!(!finish(frame).workspace_changed());
    let mut frame = session
        .begin_native_host_frame(|_| panic!("painting the measured strip has no input"))
        .unwrap();
    let (viewport, offset, maximum) = strip(&mut frame);
    assert_eq!(offset, 0.0);
    assert!(
        maximum > 40.0,
        "expected overflow: viewport={viewport:?}, offset={offset}, maximum={maximum}"
    );
    let scene = render(&mut frame, 200.0);
    let mut report = finish(frame);
    let output = take_output(&mut report, &scene, binding);
    present(&mut session, output);
    let edge = LogicalPoint::new(viewport.max().x() - 1.0, viewport.y() + 1.0).unwrap();
    assert!(
        scene.recording.records.iter().any(|record| matches!(record,
            OwnedRecord::Receiver(receiver) if receiver.role() == DockspaceReceiverRole::TabStripScroll
                && receiver.bounds().contains(edge)))
    );
    let request = start_drag(&mut session, Lane::Desktop, binding, &scene, edge);
    assert_eq!(request.deadline(), Duration::from_millis(60));
    assert_eq!(
        request.source_input(),
        Some(NativeDragSourceInput::Preserve)
    );
    session
        .record_native_pointer(input(
            &session,
            Lane::Desktop,
            binding,
            request.capture_acquired_event().unwrap(),
            edge,
            at(Device::Mouse, 240),
        ))
        .unwrap();
    let mut frame = session
        .begin_native_host_frame(|_| panic!("capture acquisition must not query edge receivers"))
        .unwrap();
    assert_eq!(strip(&mut frame), (viewport, 0.0, maximum));
    let acquired = finish(frame);
    assert!(acquired.inputs().is_empty());
    assert!(acquired.manipulation_feedback().is_empty());
    assert!(!acquired.workspace_changed());
    assert!(session.view().item(FIRST).unwrap().is_selected());
    assert_eq!(session.version(), version);
    assert!(!session.native_pointer_continuation_awaits_presentation(SURFACE));
    let restarted = session.native_pointer_continuation().unwrap();
    assert_eq!(restarted.deadline(), Duration::from_millis(290));
    assert_ne!(restarted.token(), request.token());
    assert_eq!(restarted.position(), edge);
    assert_eq!(restarted.source_binding(), binding);
    assert!(restarted.capture_acquired_event().is_none());
    for (wake, millis) in [(request, 240), (restarted, 289)] {
        session
            .record_native_pointer(continuation(
                wake,
                Duration::from_millis(millis),
                Device::Mouse,
                NativePointerOwner::Native(binding),
            ))
            .unwrap();
        let mut frame = session
            .begin_native_host_frame(|query| scene.answer(query))
            .unwrap();
        assert_eq!(strip(&mut frame), (viewport, 0.0, maximum));
        let early = finish(frame);
        assert!(early.inputs().is_empty());
        assert!(!early.workspace_changed());
        assert_eq!(session.native_pointer_continuation(), Some(restarted));
    }

    session
        .record_native_pointer(continuation(
            restarted,
            restarted.deadline(),
            Device::Mouse,
            NativePointerOwner::Native(binding),
        ))
        .unwrap();
    let mut frame = session
        .begin_native_host_frame(|query| {
            assert_eq!(query.metadata(), at(Device::Mouse, 290));
            assert_eq!(query.point(), Some(edge));
            assert!(query.matches_native_binding(binding));
            scene.answer(query)
        })
        .unwrap();
    assert_eq!(strip(&mut frame), (viewport, 40.0, maximum));
    let scrolled = render(&mut frame, 200.0);
    assert_eq!(scrolled.focus, scene.focus);
    let mut report = finish(frame);
    assert!(!report.workspace_changed());
    assert!(selections(&report).is_empty());
    assert!(report.manipulation_feedback().is_empty());
    assert!(session.view().item(FIRST).unwrap().is_selected());
    let exact = take_output(&mut report, &scrolled, binding);
    assert!(session.native_pointer_continuation().is_none());
    assert!(session.native_pointer_continuation_awaits_presentation(SURFACE));
    assert!(!session.pointer_continuation_awaits_presentation(SURFACE));
    assert_eq!(session.version(), version);
    assert_eq!(order(&session), [FIRST, SECOND, THIRD]);
    assert!(session.has_active_gesture());
    assert!(idle(&mut session).inputs().is_empty());
    assert!(session.native_pointer_continuation().is_none());
    assert!(session.native_pointer_continuation_awaits_presentation(SURFACE));

    let artifact = exact.presentation_artifact();
    let presented = present(&mut session, exact);
    assert!(
        presented
            .surface_presentation_commits()
            .iter()
            .any(|commit| commit.presents(artifact))
    );
    let next = session.native_pointer_continuation().unwrap();
    assert_ne!(next.token(), restarted.token());
    assert_eq!(next.deadline(), Duration::from_millis(340));
    assert_eq!(next.position(), edge);
    assert_eq!(next.source_binding(), binding);
    assert_eq!(next.source_input(), Some(NativeDragSourceInput::Preserve));
    assert!(next.capture_acquired_event().is_none());
    assert!(!session.native_pointer_continuation_awaits_presentation(SURFACE));
    assert!(!session.has_pending_native_input());
}

#[test]
fn native_capture_acquisition_after_unknown_capture_does_not_restore_a_cleared_target() {
    let (mut session, binding, scene) = fixture(true);
    let target = scene.point(SECOND, 0.625);
    let version = session.version();
    let request = start_drag(&mut session, Lane::Desktop, binding, &scene, target);
    let acquired = request.capture_acquired_event().unwrap();
    session
        .record_native_pointer(observed_input(
            None,
            binding,
            Event::CaptureChanged,
            target,
            NativePointerOwner::Unknown,
            at(Device::Mouse, 20),
        ))
        .unwrap();
    assert!(idle(&mut session).inputs().is_empty());
    assert!(session.has_active_gesture());
    assert!(session.native_pointer_continuation().is_none());
    session
        .record_native_pointer(input(
            &session,
            Lane::Desktop,
            binding,
            acquired,
            target,
            at(Device::Mouse, 240),
        ))
        .unwrap();
    let report = idle(&mut session);
    assert!(report.inputs().is_empty());
    assert!(report.manipulation_feedback().is_empty());
    assert!(!report.workspace_changed());
    assert_eq!(session.version(), version);
    assert!(session.view().item(FIRST).unwrap().is_selected());
    assert!(session.has_active_gesture());
    assert!(session.native_pointer_continuation().is_none());
    session
        .record_native_pointer(continuation(
            request,
            Duration::from_millis(241),
            Device::Mouse,
            NativePointerOwner::Native(binding),
        ))
        .unwrap();
    assert!(idle(&mut session).inputs().is_empty());
    assert!(session.native_pointer_continuation().is_none());

    session
        .record_native_pointer(input(
            &session,
            Lane::Desktop,
            binding,
            Event::Moved,
            target,
            at(Device::Mouse, 260),
        ))
        .unwrap();
    assert!(!reduce(&mut session, &scene).workspace_changed());
    let rearmed = session.native_pointer_continuation().unwrap();
    assert_ne!(rearmed.token(), request.token());
    assert_eq!(rearmed.deadline(), Duration::from_millis(410));
    assert_eq!(rearmed.position(), target);
    assert_eq!(
        rearmed.source_input(),
        Some(NativeDragSourceInput::Preserve)
    );
    assert!(rearmed.capture_acquired_event().is_none());
    session
        .record_native_pointer(input(
            &session,
            Lane::Desktop,
            binding,
            acquired,
            target,
            at(Device::Mouse, 300),
        ))
        .unwrap();
    assert!(idle(&mut session).inputs().is_empty());
    assert_eq!(session.native_pointer_continuation(), Some(rearmed));
    assert_eq!(session.version(), version);
    assert_eq!(order(&session), [FIRST, SECOND, THIRD]);
}

#[test]
fn native_capture_acquisition_with_missing_facts_only_clears_the_timer() {
    for (unknown_delivery, unknown_capture, metadata) in [
        (true, false, at(Device::Mouse, 240)),
        (false, true, at(Device::Mouse, 240)),
        (true, true, at(Device::Mouse, 240)),
        (false, false, PointerEventMetadata::untimed(Device::Mouse)),
    ] {
        let (mut session, binding, scene) = fixture(true);
        let target = scene.point(SECOND, 0.625);
        let version = session.version();
        let request = start_drag(&mut session, Lane::Desktop, binding, &scene, target);
        let delivery = if unknown_delivery {
            NativePointerOwner::Unknown
        } else {
            NativePointerOwner::Native(binding)
        };
        let capture = if unknown_capture {
            NativePointerOwner::Unknown
        } else {
            NativePointerOwner::Native(binding)
        };
        session
            .record_native_pointer(
                NativePointerInput::new(
                    POINTER,
                    request.capture_acquired_event().unwrap(),
                    desktop_location(binding, target),
                    delivery,
                    capture,
                )
                .with_metadata(metadata),
            )
            .unwrap();
        let unknown = idle(&mut session);
        assert!(unknown.inputs().is_empty());
        assert!(unknown.manipulation_feedback().is_empty());
        assert!(!unknown.workspace_changed());
        assert_eq!(session.version(), version);
        assert!(session.has_active_gesture(), "{delivery:?}/{capture:?}");
        assert!(session.view().item(FIRST).unwrap().is_selected());
        assert!(session.native_pointer_continuation().is_none());
        assert!(!session.has_pending_native_input());

        session
            .record_native_pointer(input(
                &session,
                Lane::Desktop,
                binding,
                Event::Moved,
                target,
                at(Device::Mouse, 260),
            ))
            .unwrap();
        assert!(!reduce(&mut session, &scene).workspace_changed());
        let rearmed = session.native_pointer_continuation().unwrap();
        assert_eq!(rearmed.deadline(), Duration::from_millis(410));
        assert_eq!(rearmed.position(), target);
        assert_ne!(rearmed.token(), request.token());
        let acquired = rearmed.capture_acquired_event().unwrap();
        session
            .record_native_pointer(input(
                &session,
                Lane::Desktop,
                binding,
                acquired,
                target,
                at(Device::Mouse, 300),
            ))
            .unwrap();
        assert!(idle(&mut session).inputs().is_empty());
        let restarted = session.native_pointer_continuation().unwrap();
        assert_eq!(restarted.deadline(), Duration::from_millis(450));
        assert_ne!(restarted.token(), rearmed.token());
        assert!(restarted.capture_acquired_event().is_none());
        assert_eq!(session.version(), version);
        assert_eq!(order(&session), [FIRST, SECOND, THIRD]);
    }
}

#[test]
fn native_capture_acquisition_with_known_owner_loss_cancels_the_gesture() {
    for lost in [NativePointerOwner::None, NativePointerOwner::Foreign] {
        for delivery_lost in [false, true] {
            let (mut session, binding, scene) = fixture(true);
            let target = scene.point(SECOND, 0.625);
            let version = session.version();
            let request = start_drag(&mut session, Lane::Desktop, binding, &scene, target);
            let acquired = request.capture_acquired_event().unwrap();
            let (delivery, capture) = if delivery_lost {
                (lost, NativePointerOwner::Unknown)
            } else {
                (NativePointerOwner::Unknown, lost)
            };
            session
                .record_native_pointer(
                    NativePointerInput::new(
                        POINTER,
                        acquired,
                        desktop_location(binding, target),
                        delivery,
                        capture,
                    )
                    .with_metadata(at(Device::Mouse, 240)),
                )
                .unwrap();
            let cancelled = idle(&mut session);
            assert!(!cancelled.workspace_changed());
            assert!(selections(&cancelled).is_empty());
            let [feedback] = cancelled.manipulation_feedback() else {
                panic!("known owner loss must cancel once: {cancelled:?}");
            };
            assert!(matches!(feedback.status(), Status::Cancelled(_)));
            assert_eq!(session.version(), version);
            assert!(!session.has_active_gesture(), "{delivery:?}/{capture:?}");
            assert!(session.native_pointer_continuation().is_none());
            assert!(!session.has_pending_native_input());
            assert!(session.view().item(FIRST).unwrap().is_selected());
            assert_eq!(order(&session), [FIRST, SECOND, THIRD]);

            session
                .record_native_pointer(input(
                    &session,
                    Lane::Desktop,
                    binding,
                    acquired,
                    target,
                    at(Device::Mouse, 250),
                ))
                .unwrap();
            assert!(idle(&mut session).inputs().is_empty());
            assert!(!session.has_active_gesture());
            assert!(session.native_pointer_continuation().is_none());
            assert_eq!(session.version(), version);
        }
    }
}

#[test]
fn obsolete_capture_acquisition_still_observes_current_capture_loss() {
    for capture in [
        NativePointerOwner::Unknown,
        NativePointerOwner::None,
        NativePointerOwner::Foreign,
    ] {
        let (mut session, binding, scene) = fixture(true);
        let target = scene.point(SECOND, 0.625);
        let version = session.version();
        let old = start_drag(&mut session, Lane::Desktop, binding, &scene, target);
        session
            .record_native_pointer(input(
                &session,
                Lane::Desktop,
                binding,
                RELEASE,
                target,
                at(Device::Mouse, 20),
            ))
            .unwrap();
        let released = finish(
            session
                .begin_native_host_frame(|query| {
                    NativeReceiverAnswer::Blocked(query.presented_surface())
                })
                .unwrap(),
        );
        assert!(!released.workspace_changed());
        assert!(!session.has_active_gesture());
        session
            .record_native_pointer_batch([
                input(
                    &session,
                    Lane::Desktop,
                    binding,
                    PRESS,
                    scene.point(FIRST, 0.25),
                    at(Device::Mouse, 30),
                ),
                input(
                    &session,
                    Lane::Desktop,
                    binding,
                    Event::Moved,
                    target,
                    at(Device::Mouse, 40),
                ),
            ])
            .unwrap();
        assert!(!reduce(&mut session, &scene).workspace_changed());
        let current = session.native_pointer_continuation().unwrap();
        assert_ne!(old.token(), current.token());
        session
            .record_native_pointer(
                NativePointerInput::new(
                    POINTER,
                    old.capture_acquired_event().unwrap(),
                    desktop_location(binding, target),
                    NativePointerOwner::Native(binding),
                    capture,
                )
                .with_metadata(at(Device::Mouse, 50)),
            )
            .unwrap();
        let observed = idle(&mut session);
        assert!(!observed.workspace_changed());
        assert!(selections(&observed).is_empty());
        assert!(session.native_pointer_continuation().is_none());
        assert_eq!(session.version(), version);
        assert_eq!(order(&session), [FIRST, SECOND, THIRD]);
        if capture == NativePointerOwner::Unknown {
            assert!(session.has_active_gesture());
            assert!(observed.manipulation_feedback().is_empty());
            session
                .record_native_pointer(input(
                    &session,
                    Lane::Desktop,
                    binding,
                    Event::Moved,
                    target,
                    at(Device::Mouse, 60),
                ))
                .unwrap();
            assert!(!reduce(&mut session, &scene).workspace_changed());
            let rearmed = session.native_pointer_continuation().unwrap();
            assert_eq!(rearmed.deadline(), Duration::from_millis(210));
            assert!(rearmed.capture_acquired_event().is_some());
        } else {
            assert!(!session.has_active_gesture());
            let [feedback] = observed.manipulation_feedback() else {
                panic!("known capture loss must cancel the current gesture exactly once");
            };
            assert!(matches!(feedback.status(), Status::Cancelled(_)));
        }
    }
}

#[test]
fn a_local_capture_token_cannot_authorize_acquisition_after_desktop_handoff() {
    let (mut session, binding, scene) = fixture(true);
    let target = scene.point(SECOND, 0.625);
    let version = session.version();
    let local = start_drag(&mut session, Lane::Local, binding, &scene, target);
    assert!(local.source_input().is_none());
    assert!(local.capture_acquired_event().is_none());
    let local_acquired = Event::CaptureAcquired(local.token());
    session
        .record_native_pointer(input(
            &session,
            Lane::Local,
            binding,
            local_acquired,
            target,
            at(Device::Mouse, 15),
        ))
        .unwrap();
    assert!(idle(&mut session).inputs().is_empty());
    assert_eq!(session.native_pointer_continuation(), Some(local));
    let packet = NativeLocalToDesktopHandoff::new(
        sample(local.local_coordinates().unwrap(), binding, target),
        desktop_location(binding, target),
    );
    session
        .record_native_pointer(
            NativePointerInput::new_handoff(
                POINTER,
                Event::Moved,
                packet,
                NativePointerOwner::Native(binding),
                NativePointerOwner::Native(binding),
            )
            .with_metadata(at(Device::Mouse, 20)),
        )
        .unwrap();
    assert!(!reduce(&mut session, &scene).workspace_changed());
    let desktop = session.native_pointer_continuation().unwrap();
    assert!(desktop.local_coordinates().is_none());
    assert_eq!(
        desktop.source_input(),
        Some(NativeDragSourceInput::Preserve)
    );
    assert_eq!(desktop.deadline(), Duration::from_millis(170));
    assert_ne!(desktop.token(), local.token());
    let desktop_acquired = desktop.capture_acquired_event().unwrap();
    session
        .record_native_pointer(input(
            &session,
            Lane::Desktop,
            binding,
            local_acquired,
            target,
            at(Device::Mouse, 240),
        ))
        .unwrap();
    let rejected = idle(&mut session);
    assert!(rejected.inputs().is_empty());
    assert!(rejected.manipulation_feedback().is_empty());
    assert!(!rejected.workspace_changed());
    assert_eq!(session.native_pointer_continuation(), Some(desktop));
    assert!(session.has_active_gesture());
    assert_eq!(session.version(), version);
    assert!(session.view().item(FIRST).unwrap().is_selected());
    assert_eq!(order(&session), [FIRST, SECOND, THIRD]);
    session
        .record_native_pointer(input(
            &session,
            Lane::Desktop,
            binding,
            desktop_acquired,
            target,
            at(Device::Mouse, 250),
        ))
        .unwrap();
    assert!(idle(&mut session).inputs().is_empty());
    let restarted = session.native_pointer_continuation().unwrap();
    assert_eq!(restarted.deadline(), Duration::from_millis(400));
    assert_ne!(restarted.token(), desktop.token());
    assert_eq!(restarted.position(), target);
    assert!(restarted.capture_acquired_event().is_none());
    assert_eq!(session.version(), version);
}
