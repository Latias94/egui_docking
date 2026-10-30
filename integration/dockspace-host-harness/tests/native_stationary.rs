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
    DockspaceManipulationPendingReason as PendingReason, DockspaceManipulationStatus as Status,
    DockspacePaneFocusRequest, DockspacePreviewVisual, DockspaceReceiverDescriptor,
    DockspaceSession, DockspaceTouchEvent, HostFrameReport, HostInputOutcome, HostPointerInput,
    HostPointerRoute, HostWindowToken, NativeCreateStrategy, NativeDesktopPointerLocation,
    NativeDesktopPosition, NativeHostCapabilities, NativeHostCapability, NativeLocalCoordinates,
    NativeLocalGeometry, NativeLocalPointerFact, NativeLocalPointerLocation,
    NativeLocalPointerSample, NativeLocalToDesktopHandoff, NativePointerButton as Button,
    NativePointerContinuation, NativePointerEvent as Event, NativePointerHover, NativePointerId,
    NativePointerInput, NativePointerOwner, NativePointerRoster, NativeReceiverAnswer,
    NativeReceiverPurpose, NativeReceiverQuery, NativeScrollDelta, NativeScrollDeviceId,
    NativeScrollEvent, NativeScrollModifiers, NativeScrollMomentum, NativeScrollPhase,
    NativeSurfaceBinding, NativeWindowFacts, NativeWindowInputState, NativeWindowPresentationState,
    NativeWorkAreaRoster, PaintedSurfaceOutput, PointerDeviceKind as Device, PointerEventMetadata,
    SurfacePaintProgramOperation as Operation, SurfacePaintProgramStep as Step,
    SurfacePointerButton, SurfacePointerCapture, SurfacePointerEvent, SurfacePointerId,
    SurfacePointerReceiverFacts, SurfacePresentationResult as Disposition,
    SurfaceUnavailableReason, UniformSurfaceMetrics,
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
        .with_presentation(NativeWindowPresentationState::Visible, None);
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
    assert_eq!(frame.native_staging_paints().len(), 0);
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

    fn assert_preview(&self) {
        assert!(self.recording.records.iter().any(|record| matches!(
            record,
            OwnedRecord::Preview(DockspacePreviewVisual::Dock {
                surface: SURFACE,
                ..
            })
        )));
    }
}

fn render(frame: &mut DockspaceHostFrame<'_>) -> Scene {
    let plan = frame.paint_plan(SURFACE).unwrap().unwrap();
    let focus = plan.pane_focus_request();
    let mut tabs = Vec::new();
    let mut dragged_item = None;
    plan.visit_surface_program(|step| {
        match step {
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
    let scene = render(&mut frame);
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
        .enable_managed_native_host(
            NativeCreateStrategy::DirectPrepared,
            NativePointerRoster::Exact(Vec::new()),
        )
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
    let scene = render(&mut frame);
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
fn native_touch_long_press_arms_then_contact_end_waits_for_the_exact_reorder_output() {
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
        let preview = render(&mut frame);
        preview.assert_preview();
        let mut report = finish(frame);
        let [pending] = report.manipulation_feedback() else {
            panic!("one native touch release must wait for presentation: {report:?}");
        };
        let pending_id = pending.manipulation_id();
        assert_eq!(
            pending.status(),
            Status::Pending(PendingReason::AwaitingPresentation)
        );
        assert_eq!(session.version(), version);
        assert_eq!(order(&session), [FIRST, SECOND, THIRD]);
        assert!(!session.has_active_gesture());
        assert!(session.native_pointer_continuation().is_none());
        let output = take_output(&mut report, &preview, binding);
        let delivered = present(&mut session, output);
        let [terminal] = delivered.manipulation_feedback() else {
            panic!("the exact native touch output must deliver once: {delivered:?}");
        };
        assert_eq!(terminal.manipulation_id(), pending_id);
        assert_eq!(terminal.status(), Status::Delivered(Delivery::Dock));
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
    let preview = render(&mut frame);
    preview.assert_preview();
    let mut report = finish(frame);
    let [pending] = report.manipulation_feedback() else {
        panic!("the surviving desktop owner must release normally: {report:?}");
    };
    let pending_id = pending.manipulation_id();
    assert_eq!(
        pending.status(),
        Status::Pending(PendingReason::AwaitingPresentation)
    );
    let output = take_output(&mut report, &preview, binding);
    let delivered = present(&mut session, output);
    let [terminal] = delivered.manipulation_feedback() else {
        panic!("the handoff must preserve its terminal identity: {delivered:?}");
    };
    assert_eq!(terminal.manipulation_id(), pending_id);
    assert_eq!(terminal.status(), Status::Delivered(Delivery::Dock));
    assert_eq!(order(&session), [SECOND, FIRST, THIRD]);
    assert!(!session.has_active_gesture());
}
