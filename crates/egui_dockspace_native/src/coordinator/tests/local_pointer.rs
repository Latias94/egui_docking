use dockspace::runtime::{NativeWindowBinding, NativeWindowHit, SurfacePresentationArtifact};
use std::cell::Cell;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use dockspace::geometry::{LogicalPoint, PhysicalPoint, PhysicalSize};
use dockspace::runtime::{
    DockspaceManipulationCancelReason as CancelReason, DockspaceManipulationDelivery as Delivery,
    DockspaceManipulationFeedback, DockspaceManipulationStatus as Status, DockspacePreviewVisual,
    DockspaceReceiverDescriptor, DockspaceSemanticOutput, NativeHostCapability,
    NativeLocalGeometry, NativeLocalPointerFact, NativeLocalPointerLocation, NativeMonotonicClock,
    NativePointerButton, NativePointerEvent, NativePointerId, NativePointerOwner,
    NativeReceiverPurpose, PointerDeviceKind, PointerEventMetadata,
};
use winit::dpi::{PhysicalPosition, PhysicalSize as WinitPhysicalSize};
use winit::event::{
    DeviceId, ElementState, MouseButton, PointerEventFacts, PointerEventGeometry,
    PointerWindowRoute,
};

use super::*;
use crate::event::{CapturedNativePointerLocation, NativePointerRouteSnapshot};

const FIRST: ItemId = ItemId::new(1);
const SECOND: ItemId = ItemId::new(2);
const THIRD: ItemId = ItemId::new(3);
const WINDOW: u64 = 11;
const CAPTURED_AT: Duration = Duration::from_millis(40);

mod drain_resize;
mod random_journey;
mod reentry;

fn geometry() -> NativeLocalGeometry {
    NativeLocalGeometry::new(
        PhysicalSize::new(1200.0, 900.0).unwrap(),
        ScaleFactor::new(2.0).unwrap(),
    )
}

fn finish(mut frame: NativeHostFrame<'_>) -> HostFrameReport {
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    frame.commit().unwrap()
}

fn idle(native: &mut NativeCoordinator) -> HostFrameReport {
    let report = finish(
        native
            .begin_host_frame(|_| NativeReceiverAnswer::Unknown)
            .unwrap(),
    );
    native.settle_host_frame_report(&report).unwrap();
    report
}

struct Scene {
    presentation: Cell<Option<(NativeWindowBinding, SurfacePresentationArtifact)>>,
    tabs: Vec<(ItemId, DockspaceReceiverDescriptor, LogicalRect)>,
    semantic: DockspaceSemanticOutput,
    preview: bool,
}

impl Scene {
    fn point(&self, item: ItemId, fraction: f64) -> LogicalPoint {
        let (_, receiver, visible) = self.tabs.iter().find(|tab| tab.0 == item).unwrap();
        let point = LogicalPoint::new(
            visible.x() + visible.width() * fraction,
            visible.y() + visible.height() * 0.5,
        )
        .unwrap();
        assert!(receiver.bounds().contains(point));
        assert_ne!(point, receiver.center());
        point
    }

    fn answer(
        &self,
        query: impl Into<NativeReceiverQuery>,
        captured_at: Duration,
    ) -> NativeReceiverAnswer {
        let query = match query.into() {
            NativeReceiverQuery::WindowHit(query) => {
                let (window, artifact) = self
                    .presentation
                    .get()
                    .expect("recorded scene retains its exact output");
                return NativeReceiverAnswer::WindowHit(if query.window() == window {
                    NativeWindowHit::Surface(artifact)
                } else {
                    NativeWindowHit::Unknown
                });
            }
            NativeReceiverQuery::Surface(query) => query,
        };
        assert_eq!(query.surface(), FIRST_SURFACE);
        assert_eq!(
            query.metadata(),
            PointerEventMetadata::new(PointerDeviceKind::Mouse, captured_at),
            "delayed raw translation and frame retry must retain event time and device class"
        );
        let point = query
            .point()
            .expect("an exact local sample supplies its logical point");
        let Some((_, receiver, _)) = self.tabs.iter().find(|tab| tab.1.bounds().contains(point))
        else {
            return NativeReceiverAnswer::Unknown;
        };
        if matches!(query.purpose(), NativeReceiverPurpose::HoverHit) {
            query.clear_hover_lane(receiver).map_or(
                NativeReceiverAnswer::Unknown,
                NativeReceiverAnswer::ClearHoverLane,
            )
        } else {
            query
                .bind_receiver(receiver)
                .map_or(NativeReceiverAnswer::Unknown, NativeReceiverAnswer::Dock)
        }
    }
}

fn render(frame: &mut NativeHostFrame<'_>) -> Scene {
    if frame.frame.paint_plan(FIRST_SURFACE).unwrap().is_none() {
        frame
            .frame
            .measure_surface(
                FIRST_SURFACE,
                UniformSurfaceMetrics::new(
                    LogicalRect::new(0.0, 0.0, 800.0, 600.0).unwrap(),
                    LogicalSize::new(32.0, 24.0).unwrap(),
                    96.0,
                )
                .unwrap(),
            )
            .unwrap();
    }
    let plan = frame.frame.paint_plan(FIRST_SURFACE).unwrap().unwrap();
    let mut tabs = Vec::new();
    plan.visit_surface_program(|step| {
        if let SurfacePaintProgramStep::Operation {
            operation: SurfacePaintProgramOperation::Tab(tab),
            ..
        } = step
            && let Some(receiver) = plan.receiver_for_tab_body(tab)
        {
            tabs.push((tab.item(), receiver, tab.visible_bounds()));
        }
        Ok::<(), ()>(())
    })
    .unwrap();
    let mut preview = false;
    let mut semantic = None;
    let receipt = plan
        .execute_surface_program(|step| {
            match step {
                SurfacePaintProgramStep::DragPreview(record) => {
                    assert!(matches!(
                        record.visual(),
                        DockspacePreviewVisual::Dock { surface, .. } if surface == FIRST_SURFACE
                    ));
                    preview = true;
                }
                SurfacePaintProgramStep::SurfaceEnd { semantic_output } => {
                    assert!(semantic.replace(semantic_output).is_none());
                }
                _ => {}
            }
            Ok::<(), ()>(())
        })
        .unwrap();
    frame.frame.confirm_surface_painted(receipt).unwrap();
    Scene {
        presentation: Cell::new(None),
        tabs,
        semantic: semantic.unwrap(),
        preview,
    }
}

fn take_output(
    report: &mut HostFrameReport,
    scene: &Scene,
    binding: NativeSurfaceBinding,
) -> PaintedSurfaceOutput {
    let [output] = report.take_painted_outputs().try_into().unwrap();
    assert!(output.matches_native_binding(binding));
    assert!(output.matches_semantic_output(scene.semantic));
    scene
        .presentation
        .set(Some((binding.window(), output.presentation_artifact())));
    output
}

fn paint_and_present(native: &mut NativeCoordinator, binding: NativeSurfaceBinding) -> Scene {
    let mut frame = native
        .begin_host_frame(|_| NativeReceiverAnswer::Unknown)
        .unwrap();
    let scene = render(&mut frame);
    let mut report = finish(frame);
    let output = take_output(&mut report, &scene, binding);
    native
        .session
        .report_surface_presentation(output, SurfacePresentationResult::Presented)
        .unwrap();
    assert!(idle(native).manipulation_feedback().is_empty());
    scene
}

fn fixture() -> (
    NativeCoordinator,
    NativeSurfaceBinding,
    Scene,
    Arc<AtomicU64>,
) {
    fixture_with_desktop(false)
}

fn fixture_with_desktop(
    desktop: bool,
) -> (
    NativeCoordinator,
    NativeSurfaceBinding,
    Scene,
    Arc<AtomicU64>,
) {
    let mut capabilities = NativeHostCapabilities::none_supported()
        .with(NativeHostCapability::AuthoritativeInventory)
        .with(NativeHostCapability::AuthoritativeButtonState)
        .with(NativeHostCapability::PointerHitTestObservation);
    if desktop {
        capabilities = capabilities
            .with(NativeHostCapability::DesktopPointerPosition)
            .with(NativeHostCapability::HoveredWindow);
    }
    fixture_with_capabilities(
        desktop,
        capabilities,
        NativePointerRoster::Exact(Vec::new()),
    )
}

fn fixture_with_capabilities(
    desktop: bool,
    capabilities: NativeHostCapabilities,
    initial_pointer_roster: NativePointerRoster,
) -> (
    NativeCoordinator,
    NativeSurfaceBinding,
    Scene,
    Arc<AtomicU64>,
) {
    let layout = DockspaceLayout::new([DockspaceSurfaceLayout::new(
        FIRST_SURFACE,
        DockspaceRootLayout::new(
            RootId::new(1),
            DockspaceNode::central_tabs([FIRST, SECOND, THIRD]),
        ),
    )])
    .unwrap();
    let session = DockspaceSession::from_layout(layout, DockPolicy::default()).unwrap();
    let now = Arc::new(AtomicU64::new(0));
    let clock = Arc::clone(&now);
    let mut native = NativeCoordinator::new_with_storage_allowance(
        session,
        initial_pointer_roster,
        None,
        None,
        None,
        Some(NativeMonotonicClock::new(move || {
            Duration::from_millis(clock.load(Ordering::SeqCst))
        })),
    )
    .unwrap();
    native
        .register_native_root(FIRST_SURFACE, HostWindowToken::new(WINDOW))
        .unwrap();
    let binding = idle(&mut native).native_bindings()[0];
    native
        .bind_viewport(ViewportId::ROOT, WindowId::from(WINDOW), binding)
        .unwrap();
    let geometry = geometry();
    let mut facts = NativeWindowFacts::live()
        .with_content_size(geometry.content_size())
        .with_native_scale_factor(geometry.native_scale_factor())
        .with_presentation_scale_factor(ScaleFactor::new(1.5).unwrap())
        .with_input(NativeWindowInputState::ReceivesInput, None)
        .with_presentation(NativeWindowPresentationState::Visible);
    if desktop {
        facts = facts.with_content_bounds(PhysicalRect::new(400.0, -300.0, 1200.0, 900.0).unwrap());
    }
    native
        .report_snapshot(
            [(binding, facts)],
            NativeWorkAreaRoster::Unknown,
            capabilities,
        )
        .unwrap();
    let mut frame = native
        .begin_host_frame(|_| NativeReceiverAnswer::Unknown)
        .unwrap();
    frame
        .frame
        .measure_surface(
            FIRST_SURFACE,
            UniformSurfaceMetrics::new(
                LogicalRect::new(0.0, 0.0, 800.0, 600.0).unwrap(),
                LogicalSize::new(32.0, 24.0).unwrap(),
                96.0,
            )
            .unwrap(),
        )
        .unwrap();
    let report = finish(frame);
    native.settle_host_frame_report(&report).unwrap();
    let scene = paint_and_present(&mut native, binding);
    let cap = native.session.native_local_coordinates(binding).unwrap();
    assert_eq!(cap.geometry(), geometry);
    assert_eq!(
        native.bridge.with_viewports(|routes| routes
            .local_coordinates_for_event(WindowId::from(WINDOW), Some(ViewportId::ROOT))),
        Some(cap)
    );
    assert!(native.session.view().item(FIRST).unwrap().is_selected());
    assert!(!native.session.has_pending_native_input());
    (native, binding, scene, now)
}

fn order(native: &NativeCoordinator) -> Vec<ItemId> {
    native
        .session
        .view()
        .item(FIRST)
        .unwrap()
        .tabs()
        .items()
        .to_vec()
}

fn raw_event(point: LogicalPoint, state: Option<ElementState>, event_scale: f64) -> WindowEvent {
    let scale = ScaleFactor::new(1.5).unwrap().get();
    let position = PhysicalPosition::new(point.x() * scale, point.y() * scale);
    let facts = PointerEventFacts {
        surface_position: Some(position),
        desktop_position: None,
        geometry: Some(PointerEventGeometry {
            surface_size: WinitPhysicalSize::new(1200, 900),
            scale_factor: event_scale,
        }),
        modifiers: None,
        hover: PointerWindowRoute::Unknown,
        capture: if state == Some(ElementState::Released) {
            PointerWindowRoute::None
        } else {
            PointerWindowRoute::Window(WindowId::from(WINDOW))
        },
    };
    match state {
        Some(state) => WindowEvent::MouseInput {
            primary_press: None,
            device_id: DeviceId::dummy(),
            state,
            button: MouseButton::Left,
            facts,
        },
        None => WindowEvent::CursorMoved {
            device_id: DeviceId::dummy(),
            position,
            facts,
        },
    }
}

fn raw_desktop_event(point: LogicalPoint, state: Option<ElementState>) -> WindowEvent {
    let mut event = raw_event(point, state, 2.0);
    let (WindowEvent::CursorMoved { facts, .. } | WindowEvent::MouseInput { facts, .. }) =
        &mut event
    else {
        unreachable!("the fixture only creates mouse motion and button events");
    };
    let scale = ScaleFactor::new(1.5).unwrap().get();
    facts.desktop_position = Some(PhysicalPosition::new(
        400.0 + point.x() * scale,
        -300.0 + point.y() * scale,
    ));
    facts.hover = PointerWindowRoute::Window(WindowId::from(WINDOW));
    event
}

fn capture(
    native: &NativeCoordinator,
    binding: NativeSurfaceBinding,
    ordinal: u64,
    event: WindowEvent,
) -> NativeWindowEventRecord {
    let cap = native.session.native_local_coordinates(binding).unwrap();
    let record = native.bridge.with_viewports(|routes| {
        NativeWindowEventRecord::for_test_local_capture(
            ordinal,
            WindowId::from(WINDOW),
            Some(ViewportId::ROOT),
            event,
            routes,
            Some(cap),
            Some(geometry()),
        )
    });
    assert_eq!(record.binding(), Some(binding));
    let routes = record.pointer_routes().unwrap();
    assert_eq!(
        routes.delivery(),
        NativePointerRouteSnapshot::Dock(binding.window())
    );
    record
}

fn reduce_one(native: &mut NativeCoordinator) {
    let recorded = native.session.native_input_recorded_count();
    assert_eq!(
        native
            .reduce_callback_prefix(&egui::Context::default(), &mut test_budget())
            .unwrap()
            .unwrap()
            .get(),
        1
    );
    assert_eq!(native.session.native_input_recorded_count(), recorded + 1);
    assert!(native.bridge.callback_boundary_pending());
}

fn input_boundary(
    native: &mut NativeCoordinator,
    scene: &Scene,
    captured_at: Duration,
) -> HostFrameReport {
    finish(
        native
            .begin_host_frame(|query| scene.answer(query, captured_at))
            .unwrap(),
    )
}

fn feedback(report: &HostFrameReport) -> DockspaceManipulationFeedback {
    let [feedback] = report.manipulation_feedback() else {
        panic!("expected one manipulation publication: {report:?}");
    };
    *feedback
}

#[test]
fn raw_local_reorder_retries_input_and_delivers_before_output_settlement() {
    let (mut native, binding, scene, now) = fixture();
    let source = scene.point(FIRST, 0.25);
    let target = scene.point(THIRD, 0.625);
    let version = native.session.version();
    let presented = native.session.presented_surface(FIRST_SURFACE).unwrap();
    let recorded = native.session.native_input_recorded_count();
    now.store(40, Ordering::SeqCst);
    for (ordinal, point, state) in [
        (1, source, Some(ElementState::Pressed)),
        (2, target, None),
        (3, target, Some(ElementState::Released)),
    ] {
        let record = capture(&native, binding, ordinal, raw_event(point, state, 2.0));
        let Some(CapturedNativePointerLocation::NativeLocal(NativeLocalPointerLocation::Exact(
            sample,
        ))) = record.pointer_location()
        else {
            panic!("matching event/callback/committed geometry must capture a local sample");
        };
        assert_eq!(
            sample.client_position(),
            PhysicalPoint::new(point.x() * 1.5, point.y() * 1.5).unwrap()
        );
        native.bridge.push_record(HostRecord::WindowEvent(record));
    }
    now.store(100, Ordering::SeqCst);
    assert_eq!(
        native.bridge.input_backlog_for_test(),
        (3, Some(Duration::from_millis(60)))
    );
    reduce_one(&mut native);
    let translator = format!("{:?}", native.pointer_translator);
    let mut queries = 0;
    let mut abandoned = native
        .begin_host_frame(|query| {
            let query = match query {
                NativeReceiverQuery::WindowHit(_) => return scene.answer(query, CAPTURED_AT),
                NativeReceiverQuery::Surface(query) => query,
            };

            queries += 1;
            scene.answer(query, CAPTURED_AT)
        })
        .unwrap();
    assert!(!render(&mut abandoned).preview);
    drop(abandoned);
    assert!(queries > 0);
    now.store(200, Ordering::SeqCst);
    assert_eq!(native.session.version(), version);
    assert_eq!(order(&native), [FIRST, SECOND, THIRD]);
    assert_eq!(
        native.session.presented_surface(FIRST_SURFACE),
        Some(presented)
    );
    assert_eq!(native.session.native_input_recorded_count(), recorded + 1);
    assert_eq!(
        native.bridge.input_backlog_for_test(),
        (3, Some(Duration::from_millis(160))),
        "transferred input must retain its original age until commit"
    );
    assert!(
        native
            .reduce_callback_prefix(&egui::Context::default(), &mut test_budget())
            .unwrap()
            .is_none(),
        "a dropped frame retries retained semantic input without translating the raw successor"
    );
    assert_eq!(native.next_window_event().unwrap().unwrap().ordinal(), 2);
    assert_eq!(format!("{:?}", native.pointer_translator), translator);
    assert!(
        input_boundary(&mut native, &scene, CAPTURED_AT)
            .manipulation_feedback()
            .is_empty()
    );

    reduce_one(&mut native);
    assert!(
        input_boundary(&mut native, &scene, CAPTURED_AT)
            .manipulation_feedback()
            .is_empty()
    );
    assert!(native.session.has_active_gesture());
    reduce_one(&mut native);
    let mut frame = native
        .begin_host_frame(|query| scene.answer(query, CAPTURED_AT))
        .unwrap();
    let successor = render(&mut frame);
    assert!(!successor.preview);
    let mut report = finish(frame);
    let terminal = feedback(&report);
    assert_eq!(terminal.status(), Status::Delivered(Delivery::Dock));
    assert!(report.workspace_changed());
    assert_ne!(native.session.version(), version);
    assert_eq!(order(&native), [SECOND, THIRD, FIRST]);
    let delivered_version = native.session.version();
    assert_eq!(native.session.native_input_recorded_count(), recorded + 3);
    assert!(!native.session.has_active_gesture());
    assert!(!native.session.has_pending_native_input());
    assert!(native.next_window_event().unwrap().is_none());
    assert_eq!(native.bridge.input_backlog_for_test(), (0, None));
    let exact = take_output(&mut report, &successor, binding);
    assert!(idle(&mut native).manipulation_feedback().is_empty());
    assert_eq!(order(&native), [SECOND, THIRD, FIRST]);

    let artifact = exact.presentation_artifact();
    native
        .session
        .report_surface_presentation(exact, SurfacePresentationResult::Presented)
        .unwrap();
    drop(
        native
            .begin_host_frame(|_| NativeReceiverAnswer::Unknown)
            .unwrap(),
    );
    let presentation_recorded = native.session.native_input_recorded_count();
    assert_eq!(presentation_recorded, recorded + 4);
    assert_eq!(native.session.version(), delivered_version);
    assert_eq!(order(&native), [SECOND, THIRD, FIRST]);
    let settled = idle(&mut native);
    assert_eq!(
        settled
            .surface_presentation_commits()
            .iter()
            .filter(|commit| commit.settles(artifact))
            .count(),
        1
    );
    assert!(settled.manipulation_feedback().is_empty());
    assert!(!settled.workspace_changed());
    assert_eq!(native.session.version(), delivered_version);
    assert_eq!(order(&native), [SECOND, THIRD, FIRST]);
    assert!(idle(&mut native).manipulation_feedback().is_empty());
    assert_eq!(native.session.version(), delivered_version);
    assert_eq!(
        native.session.native_input_recorded_count(),
        presentation_recorded
    );
    assert!(native.pointer_translator.can_coalesce_idle_cursor_moves());
}

#[test]
fn raw_local_to_desktop_handoff_retries_without_releasing_source_credit_or_accepting_old_preview() {
    let (mut native, binding, scene, now) = fixture_with_desktop(true);
    let source = scene.point(FIRST, 0.25);
    let target = scene.point(THIRD, 0.625);
    let version = native.session.version();
    let recorded = native.session.native_input_recorded_count();
    let coordinates = native.session.native_local_coordinates(binding).unwrap();
    now.store(40, Ordering::SeqCst);
    let press = capture(
        &native,
        binding,
        1,
        raw_event(source, Some(ElementState::Pressed), 2.0),
    );
    native.bridge.push_record(HostRecord::WindowEvent(press));
    reduce_one(&mut native);
    assert!(
        input_boundary(&mut native, &scene, CAPTURED_AT)
            .manipulation_feedback()
            .is_empty()
    );
    let motion = capture(&native, binding, 2, raw_event(target, None, 2.0));
    native.bridge.push_record(HostRecord::WindowEvent(motion));
    reduce_one(&mut native);
    let mut local_frame = native
        .begin_host_frame(|query| scene.answer(query, CAPTURED_AT))
        .unwrap();
    let local_preview = render(&mut local_frame);
    assert!(local_preview.preview);
    let mut local_report = finish(local_frame);
    assert!(local_report.manipulation_feedback().is_empty());
    let local_output = take_output(&mut local_report, &local_preview, binding);
    let local_artifact = local_output.presentation_artifact();
    assert!(native.session.has_active_gesture());

    let motion = capture(&native, binding, 3, raw_desktop_event(target, None));
    assert_eq!(
        motion.pointer_routes().unwrap().hover(),
        NativePointerRouteSnapshot::Dock(binding.window())
    );
    native.bridge.push_record(HostRecord::WindowEvent(motion));
    reduce_one(&mut native);
    let translator = format!("{:?}", native.pointer_translator);
    let mut abandoned = native
        .begin_host_frame(|query| scene.answer(query, CAPTURED_AT))
        .unwrap();
    assert!(render(&mut abandoned).preview);
    drop(abandoned);
    now.store(80, Ordering::SeqCst);
    assert_eq!(native.session.version(), version);
    assert_eq!(order(&native), [FIRST, SECOND, THIRD]);
    assert_eq!(
        native.session.native_local_coordinates(binding),
        Some(coordinates)
    );
    assert_eq!(native.session.native_input_recorded_count(), recorded + 3);
    assert!(native.session.has_active_gesture());
    assert!(native.bridge.callback_boundary_pending());
    assert_eq!(
        native.bridge.input_backlog_for_test(),
        (1, Some(Duration::from_millis(40)))
    );
    assert!(
        native
            .reduce_callback_prefix(&egui::Context::default(), &mut test_budget())
            .unwrap()
            .is_none()
    );
    assert_eq!(format!("{:?}", native.pointer_translator), translator);
    assert!(
        input_boundary(&mut native, &scene, CAPTURED_AT)
            .manipulation_feedback()
            .is_empty()
    );
    assert_eq!(native.bridge.input_backlog_for_test(), (0, None));
    assert!(native.session.has_active_gesture());

    let release = capture(
        &native,
        binding,
        4,
        raw_desktop_event(target, Some(ElementState::Released)),
    );
    native.bridge.push_record(HostRecord::WindowEvent(release));
    reduce_one(&mut native);
    let mut frame = native
        .begin_host_frame(|query| scene.answer(query, Duration::from_millis(80)))
        .unwrap();
    let successor = render(&mut frame);
    assert!(!successor.preview);
    let mut report = finish(frame);
    assert_eq!(
        feedback(&report).status(),
        Status::Delivered(Delivery::Dock)
    );
    assert!(report.workspace_changed());
    assert_ne!(native.session.version(), version);
    let delivered_version = native.session.version();
    assert_eq!(order(&native), [SECOND, THIRD, FIRST]);
    assert_eq!(native.session.native_input_recorded_count(), recorded + 4);
    assert!(!native.session.has_active_gesture());
    assert!(native.next_window_event().unwrap().is_none());
    assert_eq!(native.bridge.input_backlog_for_test(), (0, None));
    let exact = take_output(&mut report, &successor, binding);
    let artifact = exact.presentation_artifact();
    assert_ne!(artifact, local_artifact);

    native
        .session
        .report_surface_presentation(local_output, SurfacePresentationResult::Presented)
        .unwrap();
    let late = idle(&mut native);
    assert_eq!(
        late.surface_presentation_commits()
            .iter()
            .filter(|commit| commit.settles(local_artifact))
            .count(),
        1
    );
    assert!(late.manipulation_feedback().is_empty());
    assert!(!late.workspace_changed());
    assert_eq!(order(&native), [SECOND, THIRD, FIRST]);
    assert_eq!(native.session.version(), delivered_version);

    native
        .session
        .report_surface_presentation(exact, SurfacePresentationResult::Presented)
        .unwrap();
    let settled = idle(&mut native);
    assert_eq!(
        settled
            .surface_presentation_commits()
            .iter()
            .filter(|commit| commit.settles(artifact))
            .count(),
        1
    );
    assert!(settled.manipulation_feedback().is_empty());
    assert!(!settled.workspace_changed());
    assert_eq!(order(&native), [SECOND, THIRD, FIRST]);
    assert!(!native.session.has_pending_native_input());
    assert!(native.pointer_translator.can_coalesce_idle_cursor_moves());
    assert_eq!(native.session.native_input_recorded_count(), recorded + 6);
    assert!(idle(&mut native).manipulation_feedback().is_empty());
    assert_eq!(native.session.version(), delivered_version);
    assert_eq!(native.session.native_input_recorded_count(), recorded + 6);
}

#[test]
fn an_ordinary_desktop_drag_keeps_its_lane_when_a_callback_adds_local_facts() {
    let (mut native, binding, scene, now) = fixture_with_desktop(true);
    let source = scene.point(FIRST, 0.25);
    let target = scene.point(THIRD, 0.625);
    let version = native.session.version();
    let recorded = native.session.native_input_recorded_count();
    now.store(40, Ordering::SeqCst);
    for (ordinal, point, state) in [(1, source, Some(ElementState::Pressed)), (2, target, None)] {
        let record = capture(&native, binding, ordinal, raw_desktop_event(point, state));
        assert_eq!(
            record.pointer_routes().unwrap().hover(),
            NativePointerRouteSnapshot::Dock(binding.window())
        );
        native.bridge.push_record(HostRecord::WindowEvent(record));
        reduce_one(&mut native);
        assert!(
            input_boundary(&mut native, &scene, CAPTURED_AT)
                .manipulation_feedback()
                .is_empty()
        );
    }
    assert!(native.session.has_active_gesture());
    let release = capture(
        &native,
        binding,
        3,
        raw_desktop_event(target, Some(ElementState::Released)),
    );
    native.bridge.push_record(HostRecord::WindowEvent(release));
    reduce_one(&mut native);
    let mut frame = native
        .begin_host_frame(|query| scene.answer(query, CAPTURED_AT))
        .unwrap();
    let successor = render(&mut frame);
    let mut report = finish(frame);
    assert_ne!(native.session.version(), version);
    assert_eq!(native.session.native_input_recorded_count(), recorded + 3);
    assert_committed_reorder_survives_presentation(&mut native, binding, &successor, &mut report);
}

fn assert_committed_reorder_survives_presentation(
    native: &mut NativeCoordinator,
    binding: NativeSurfaceBinding,
    successor: &Scene,
    report: &mut HostFrameReport,
) {
    assert_eq!(feedback(report).status(), Status::Delivered(Delivery::Dock));
    assert!(!successor.preview);
    assert!(report.workspace_changed());
    assert_eq!(order(native), [SECOND, THIRD, FIRST]);
    assert!(!native.session.has_active_gesture());
    assert!(!native.session.has_pending_native_input());
    assert!(native.next_window_event().unwrap().is_none());
    assert_eq!(native.bridge.input_backlog_for_test(), (0, None));
    assert!(native.pointer_translator.can_coalesce_idle_cursor_moves());
    assert!(native.session.is_current_native_binding(binding));
    let version = native.session.version();
    let recorded = native.session.native_input_recorded_count();
    let exact = take_output(report, successor, binding);
    let artifact = exact.presentation_artifact();
    let waiting = idle(native);
    assert!(waiting.manipulation_feedback().is_empty());
    assert!(!waiting.workspace_changed());
    assert_eq!(native.session.version(), version);
    assert_eq!(order(native), [SECOND, THIRD, FIRST]);
    native
        .session
        .report_surface_presentation(exact, SurfacePresentationResult::Presented)
        .unwrap();
    let settled = idle(native);
    assert_eq!(
        settled
            .surface_presentation_commits()
            .iter()
            .filter(|commit| commit.settles(artifact))
            .count(),
        1,
    );
    assert!(settled.manipulation_feedback().is_empty());
    assert!(!settled.workspace_changed());
    assert_eq!(order(native), [SECOND, THIRD, FIRST]);
    assert_eq!(native.session.version(), version);
    assert_eq!(native.session.native_input_recorded_count(), recorded + 1);
    let repeated = idle(native);
    assert!(repeated.manipulation_feedback().is_empty());
    assert!(!repeated.workspace_changed());
    assert_eq!(native.session.version(), version);
    assert_eq!(native.session.native_input_recorded_count(), recorded + 1);
    assert!(native.session.is_current_native_binding(binding));
    assert_eq!(native.bridge.input_backlog_for_test(), (0, None));
}

fn assert_reorder_survives_unknown_capture_release_motion(desktop: bool) {
    let (mut native, binding, scene, now) = fixture_with_desktop(desktop);
    let source = scene.point(FIRST, 0.25);
    let target = scene.point(THIRD, 0.625);
    let version = native.session.version();
    let recorded = native.session.native_input_recorded_count();
    let coordinates = native.session.native_local_coordinates(binding).unwrap();
    let mut local_basis = None;
    now.store(40, Ordering::SeqCst);
    for (ordinal, point, state) in [
        (1, source, Some(ElementState::Pressed)),
        (2, target, None),
        (3, target, None),
        (4, target, Some(ElementState::Released)),
    ] {
        let mut event = if desktop {
            raw_desktop_event(point, state)
        } else {
            raw_event(point, state, 2.0)
        };
        if ordinal == 3 {
            // AppKit mouseUp: emits motion before updating the local button mask.
            let WindowEvent::CursorMoved { facts, .. } = &mut event else {
                unreachable!("the additional release-position event is mouse motion");
            };
            facts.capture = PointerWindowRoute::Unknown;
        }
        assert_eq!(
            native.session.native_local_coordinates(binding),
            Some(coordinates),
        );
        let record = capture(&native, binding, ordinal, event);
        assert_eq!(record.ordinal(), ordinal);
        assert_eq!(record.window_id(), WindowId::from(WINDOW));
        assert_eq!(record.viewport_id(), Some(ViewportId::ROOT));
        let routes = record.pointer_routes().unwrap();
        assert_eq!(
            routes.capture(),
            match ordinal {
                3 => NativePointerRouteSnapshot::Unknown,
                4 => NativePointerRouteSnapshot::None,
                _ => NativePointerRouteSnapshot::Dock(binding.window()),
            },
        );
        assert_eq!(
            routes.hover(),
            if desktop {
                NativePointerRouteSnapshot::Dock(binding.window())
            } else {
                NativePointerRouteSnapshot::Unknown
            },
        );
        let sample = match (desktop, state, record.pointer_location()) {
            (true, Some(_), Some(CapturedNativePointerLocation::Desktop)) => None,
            (true, None, Some(CapturedNativePointerLocation::LocalToDesktop(sample)))
            | (
                false,
                _,
                Some(CapturedNativePointerLocation::NativeLocal(
                    NativeLocalPointerLocation::Exact(sample),
                )),
            ) => Some(sample),
            (_, _, location) => {
                panic!("unexpected coordinate lane for edge {ordinal}: {location:?}")
            }
        };
        if let Some(sample) = sample {
            assert_eq!(
                sample.client_position(),
                PhysicalPoint::new(point.x() * 1.5, point.y() * 1.5).unwrap()
            );
            if let Some(basis) = local_basis {
                assert!(sample.same_coordinates(basis));
            } else {
                local_basis = Some(sample);
            }
        }
        native.bridge.push_record(HostRecord::WindowEvent(record));
        assert_eq!(
            native.next_window_event().unwrap().unwrap().ordinal(),
            ordinal
        );
        reduce_one(&mut native);
        let mut frame = native
            .begin_host_frame(|query| {
                let query = match query {
                    NativeReceiverQuery::WindowHit(_) => return scene.answer(query, CAPTURED_AT),
                    NativeReceiverQuery::Surface(query) => query,
                };

                assert_eq!(query.point(), Some(point));
                scene.answer(query, CAPTURED_AT)
            })
            .unwrap();
        if ordinal == 4 {
            let preview = render(&mut frame);
            let mut report = finish(frame);
            assert_ne!(native.session.version(), version);
            assert_eq!(native.session.native_input_recorded_count(), recorded + 4);
            assert_committed_reorder_survives_presentation(
                &mut native,
                binding,
                &preview,
                &mut report,
            );
        } else {
            let report = finish(frame);
            assert!(report.manipulation_feedback().is_empty());
            assert!(!report.workspace_changed());
            assert!(native.session.has_active_gesture());
            assert_eq!(native.session.version(), version);
            assert_eq!(order(&native), [FIRST, SECOND, THIRD]);
            assert_eq!(
                native.session.native_input_recorded_count(),
                recorded + ordinal
            );
            assert_eq!(
                native.session.native_local_coordinates(binding),
                Some(coordinates),
            );
            assert!(native.next_window_event().unwrap().is_none());
        }
    }
}

#[test]
fn raw_desktop_reorder_survives_unknown_capture_release_motion() {
    assert_reorder_survives_unknown_capture_release_motion(true);
}

#[test]
fn raw_local_reorder_survives_unknown_capture_release_motion() {
    assert_reorder_survives_unknown_capture_release_motion(false);
}

#[test]
fn macos_capabilities_and_unknown_initial_roster_allow_same_window_reorder() {
    let capabilities = capabilities_for_backend(eframe::NativeWindowingBackend::MacOs);
    assert!(!capabilities.supports(NativeHostCapability::AuthoritativeButtonState));
    let (mut native, binding, scene, now) =
        fixture_with_capabilities(true, capabilities, NativePointerRoster::Unknown);
    let source = scene.point(FIRST, 0.25);
    let target = scene.point(THIRD, 0.625);
    let version = native.session.version();
    let recorded = native.session.native_input_recorded_count();
    let coordinates = native.session.native_local_coordinates(binding).unwrap();
    now.store(40, Ordering::SeqCst);
    for (ordinal, point, state) in [(1, source, Some(ElementState::Pressed)), (2, target, None)] {
        let record = capture(&native, binding, ordinal, raw_desktop_event(point, state));
        assert_eq!(record.ordinal(), ordinal);
        assert_eq!(
            record.pointer_routes().unwrap().capture(),
            NativePointerRouteSnapshot::Dock(binding.window()),
        );
        native.bridge.push_record(HostRecord::WindowEvent(record));
        reduce_one(&mut native);
        let report = input_boundary(&mut native, &scene, CAPTURED_AT);
        assert!(report.manipulation_feedback().is_empty());
        assert!(native.session.has_active_gesture());
        assert_eq!(native.session.version(), version);
        assert_eq!(
            native.session.native_local_coordinates(binding),
            Some(coordinates),
        );
    }
    let release = capture(
        &native,
        binding,
        3,
        raw_desktop_event(target, Some(ElementState::Released)),
    );
    assert_eq!(release.ordinal(), 3);
    assert_eq!(
        release.pointer_routes().unwrap().capture(),
        NativePointerRouteSnapshot::None,
    );
    native.bridge.push_record(HostRecord::WindowEvent(release));
    reduce_one(&mut native);
    let mut frame = native
        .begin_host_frame(|query| {
            let query = match query {
                NativeReceiverQuery::WindowHit(_) => return scene.answer(query, CAPTURED_AT),
                NativeReceiverQuery::Surface(query) => query,
            };

            assert_eq!(query.point(), Some(target));
            scene.answer(query, CAPTURED_AT)
        })
        .unwrap();
    let preview = render(&mut frame);
    let mut report = finish(frame);
    assert_ne!(native.session.version(), version);
    assert_eq!(native.session.native_input_recorded_count(), recorded + 3);
    assert_committed_reorder_survives_presentation(&mut native, binding, &preview, &mut report);
}

#[test]
fn raw_foreign_capture_motion_cancels_reorder_before_none_capture_release() {
    for desktop in [false, true] {
        let (mut native, binding, scene, now) = fixture_with_desktop(desktop);
        let source = scene.point(FIRST, 0.25);
        let target = scene.point(THIRD, 0.625);
        let version = native.session.version();
        let recorded = native.session.native_input_recorded_count();
        let coordinates = native.session.native_local_coordinates(binding).unwrap();
        now.store(40, Ordering::SeqCst);
        for (ordinal, point, state) in [(1, source, Some(ElementState::Pressed)), (2, target, None)]
        {
            let event = if desktop {
                raw_desktop_event(point, state)
            } else {
                raw_event(point, state, 2.0)
            };
            let record = capture(&native, binding, ordinal, event);
            assert_eq!(record.ordinal(), ordinal);
            native.bridge.push_record(HostRecord::WindowEvent(record));
            reduce_one(&mut native);
            let report = input_boundary(&mut native, &scene, CAPTURED_AT);
            assert!(report.manipulation_feedback().is_empty());
            assert!(native.session.has_active_gesture());
            assert_eq!(native.session.version(), version);
        }
        let mut foreign_motion = if desktop {
            raw_desktop_event(target, None)
        } else {
            raw_event(target, None, 2.0)
        };
        let WindowEvent::CursorMoved { facts, .. } = &mut foreign_motion else {
            unreachable!("capture loss is reported on a real mouse motion");
        };
        facts.capture = PointerWindowRoute::Foreign;
        let record = capture(&native, binding, 3, foreign_motion);
        assert_eq!(record.ordinal(), 3);
        assert_eq!(
            record.pointer_routes().unwrap().capture(),
            NativePointerRouteSnapshot::Foreign,
        );
        native.bridge.push_record(HostRecord::WindowEvent(record));
        reduce_one(&mut native);
        let mut frame = native
            .begin_host_frame(|query| scene.answer(query, CAPTURED_AT))
            .unwrap();
        let cancelled_scene = render(&mut frame);
        let mut cancelled = finish(frame);
        assert_eq!(
            feedback(&cancelled).status(),
            Status::Cancelled(CancelReason::CaptureLost),
        );
        assert!(!cancelled_scene.preview);
        assert!(!cancelled.workspace_changed());
        assert!(!native.session.has_active_gesture());
        assert_eq!(native.session.version(), version);
        assert_eq!(order(&native), [FIRST, SECOND, THIRD]);
        assert_eq!(native.session.native_input_recorded_count(), recorded + 3);
        let output = take_output(&mut cancelled, &cancelled_scene, binding);
        let artifact = output.presentation_artifact();

        let release = if desktop {
            raw_desktop_event(target, Some(ElementState::Released))
        } else {
            raw_event(target, Some(ElementState::Released), 2.0)
        };
        let record = capture(&native, binding, 4, release);
        assert_eq!(record.ordinal(), 4);
        assert_eq!(
            record.pointer_routes().unwrap().capture(),
            NativePointerRouteSnapshot::None,
        );
        native.bridge.push_record(HostRecord::WindowEvent(record));
        reduce_one(&mut native);
        let released = input_boundary(&mut native, &scene, CAPTURED_AT);
        assert!(released.manipulation_feedback().is_empty());
        assert!(!released.workspace_changed());
        assert!(!native.session.has_active_gesture());
        assert!(!native.session.has_pending_native_input());
        assert_eq!(native.session.native_input_recorded_count(), recorded + 4);
        assert_eq!(
            native.session.native_local_coordinates(binding),
            Some(coordinates),
        );
        assert!(native.next_window_event().unwrap().is_none());
        assert!(native.pointer_translator.can_coalesce_idle_cursor_moves());
        native
            .session
            .report_surface_presentation(output, SurfacePresentationResult::Presented)
            .unwrap();
        let settled = idle(&mut native);
        assert_eq!(
            settled
                .surface_presentation_commits()
                .iter()
                .filter(|commit| commit.settles(artifact))
                .count(),
            1,
        );
        assert!(settled.manipulation_feedback().is_empty());
        assert!(!settled.workspace_changed());
        assert_eq!(native.session.version(), version);
        assert_eq!(order(&native), [FIRST, SECOND, THIRD]);
        assert_eq!(native.bridge.input_backlog_for_test(), (0, None));
    }
}

#[test]
fn none_capture_release_rejects_unknown_or_foreign_delivery_after_unknown_motion() {
    for (delivery, reason) in [
        (
            NativePointerOwner::Unknown,
            CancelReason::DeliveryAuthorityUnavailable,
        ),
        (NativePointerOwner::Foreign, CancelReason::DeliveryOwnerLost),
    ] {
        let (mut native, binding, scene, _) = fixture();
        let source = scene.point(FIRST, 0.25);
        let target = scene.point(THIRD, 0.625);
        let version = native.session.version();
        let recorded = native.session.native_input_recorded_count();
        let coordinates = native.session.native_local_coordinates(binding).unwrap();
        let pointer = NativePointerId::new(17);
        for (offset, point, event, capture_owner) in [
            (
                1,
                source,
                NativePointerEvent::ButtonPressed(NativePointerButton::Primary),
                NativePointerOwner::Native(binding.window()),
            ),
            (
                2,
                target,
                NativePointerEvent::Moved,
                NativePointerOwner::Native(binding.window()),
            ),
            (
                3,
                target,
                NativePointerEvent::Moved,
                NativePointerOwner::Unknown,
            ),
            (
                4,
                target,
                NativePointerEvent::ButtonReleased(NativePointerButton::Primary),
                NativePointerOwner::None,
            ),
        ] {
            let geometry = coordinates.geometry();
            let scale = ScaleFactor::new(1.5).unwrap().get();
            let sample = coordinates
                .capture(
                    binding.window(),
                    NativeLocalPointerFact::new(
                        PhysicalPoint::new(point.x() * scale, point.y() * scale).unwrap(),
                        geometry.content_size(),
                        geometry.native_scale_factor(),
                    ),
                    geometry,
                )
                .unwrap();
            assert_eq!(
                sample.client_position(),
                PhysicalPoint::new(point.x() * 1.5, point.y() * 1.5).unwrap()
            );
            native
                .record_pointer(
                    NativePointerInput::new_local(
                        pointer,
                        event,
                        NativeLocalPointerLocation::Exact(sample),
                        if offset == 4 {
                            delivery
                        } else {
                            NativePointerOwner::Native(binding.window())
                        },
                        capture_owner,
                    )
                    .with_metadata(PointerEventMetadata::new(
                        PointerDeviceKind::Mouse,
                        CAPTURED_AT,
                    )),
                )
                .unwrap();
            let report = input_boundary(&mut native, &scene, CAPTURED_AT);
            if offset == 4 {
                assert_eq!(feedback(&report).status(), Status::Cancelled(reason));
                assert!(!native.session.has_active_gesture());
            } else {
                assert!(report.manipulation_feedback().is_empty());
                assert!(native.session.has_active_gesture());
            }
            assert!(!report.workspace_changed());
            assert_eq!(native.session.version(), version);
            assert_eq!(order(&native), [FIRST, SECOND, THIRD]);
            assert_eq!(
                native.session.native_input_recorded_count(),
                recorded + offset
            );
            assert_eq!(
                native.session.native_local_coordinates(binding),
                Some(coordinates),
            );
        }
        assert!(!native.session.has_pending_native_input());
        assert!(idle(&mut native).manipulation_feedback().is_empty());
        let refreshed = paint_and_present(&mut native, binding);
        assert!(!refreshed.preview);
        assert_eq!(native.session.version(), version);
        assert_eq!(order(&native), [FIRST, SECOND, THIRD]);
        assert!(native.session.is_current_native_binding(binding));
    }
}

#[test]
fn queued_old_scale_release_cancels_through_mailbox_and_the_same_pointer_can_reorder() {
    let (mut native, binding, scene, now) = fixture();
    let source = scene.point(FIRST, 0.25);
    let target = scene.point(THIRD, 0.625);
    let version = native.session.version();
    let recorded = native.session.native_input_recorded_count();
    now.store(40, Ordering::SeqCst);
    let mut pressed_translator = None;
    for (ordinal, point, state) in [(1, source, Some(ElementState::Pressed)), (2, target, None)] {
        let record = capture(&native, binding, ordinal, raw_event(point, state, 2.0));
        native.bridge.push_record(HostRecord::WindowEvent(record));
        reduce_one(&mut native);
        if ordinal == 1 {
            pressed_translator = Some(format!("{:?}", native.pointer_translator));
        }
        assert!(
            input_boundary(&mut native, &scene, CAPTURED_AT)
                .manipulation_feedback()
                .is_empty()
        );
    }
    assert!(native.session.has_active_gesture());
    // The OS generated this event under scale 1; callback and committed cap are scale 2.
    let queued_release = raw_event(target, Some(ElementState::Released), 1.0);
    let release = capture(&native, binding, 3, queued_release);
    assert_eq!(
        release.pointer_location(),
        Some(CapturedNativePointerLocation::NativeLocal(
            NativeLocalPointerLocation::Unknown
        ))
    );
    assert_eq!(
        release.pointer_routes().unwrap().capture(),
        NativePointerRouteSnapshot::None
    );
    native.bridge.push_record(HostRecord::WindowEvent(release));
    now.store(80, Ordering::SeqCst);
    assert_eq!(
        native.bridge.input_backlog_for_test(),
        (1, Some(Duration::from_millis(40)))
    );
    reduce_one(&mut native);
    let cancelled = finish(
        native
            .begin_host_frame(|_| panic!("an unknown local terminal cannot query a receiver"))
            .unwrap(),
    );
    let cancellation = feedback(&cancelled);
    assert!(matches!(cancellation.status(), Status::Cancelled(_)));
    assert!(!cancelled.workspace_changed());
    assert_eq!(native.session.version(), version);
    assert_eq!(order(&native), [FIRST, SECOND, THIRD]);
    assert!(!native.session.has_active_gesture());
    assert!(!native.session.has_pending_native_input());
    assert!(native.next_window_event().unwrap().is_none());
    assert_eq!(native.bridge.input_backlog_for_test(), (0, None));
    assert!(native.pointer_translator.can_coalesce_idle_cursor_moves());
    assert_eq!(native.session.native_input_recorded_count(), recorded + 3);
    assert!(idle(&mut native).manipulation_feedback().is_empty());

    let scene = paint_and_present(&mut native, binding);
    let recorded_after_refresh = native.session.native_input_recorded_count();
    let captured_at = Duration::from_millis(80);
    for (ordinal, point, state) in [
        (4, scene.point(FIRST, 0.25), Some(ElementState::Pressed)),
        (5, scene.point(THIRD, 0.625), None),
    ] {
        let record = capture(&native, binding, ordinal, raw_event(point, state, 2.0));
        native.bridge.push_record(HostRecord::WindowEvent(record));
        reduce_one(&mut native);
        if ordinal == 4 {
            assert_eq!(
                Some(format!("{:?}", native.pointer_translator)),
                pressed_translator,
                "the same raw device must reuse its translator identity without a checkpoint"
            );
        }
        assert!(
            input_boundary(&mut native, &scene, captured_at)
                .manipulation_feedback()
                .is_empty()
        );
    }
    let release = capture(
        &native,
        binding,
        6,
        raw_event(scene.point(THIRD, 0.625), Some(ElementState::Released), 2.0),
    );
    native.bridge.push_record(HostRecord::WindowEvent(release));
    reduce_one(&mut native);
    let mut frame = native
        .begin_host_frame(|query| scene.answer(query, captured_at))
        .unwrap();
    let successor = render(&mut frame);
    let mut report = finish(frame);
    let terminal = feedback(&report);
    assert_eq!(terminal.status(), Status::Delivered(Delivery::Dock));
    assert_ne!(terminal.manipulation_id(), cancellation.manipulation_id());
    assert_ne!(native.session.version(), version);
    assert_eq!(
        native.session.native_input_recorded_count(),
        recorded_after_refresh + 3
    );
    assert_committed_reorder_survives_presentation(&mut native, binding, &successor, &mut report);
    assert_eq!(
        native.session.native_input_recorded_count(),
        recorded_after_refresh + 4
    );
}

#[test]
fn desktop_synthetic_focus_move_preserves_the_gesture_until_release_delivery() {
    let (mut native, binding, scene, now) = fixture_with_desktop(true);
    let source = scene.point(FIRST, 0.25);
    let target = scene.point(THIRD, 0.625);
    let version = native.session.version();
    let recorded = native.session.native_input_recorded_count();
    let scale = ScaleFactor::new(1.5).unwrap().get();
    let desktop_event = |point: LogicalPoint, state| {
        let mut event = raw_event(point, state, 2.0);
        let facts = match &mut event {
            WindowEvent::CursorMoved { facts, .. } | WindowEvent::MouseInput { facts, .. } => facts,
            _ => unreachable!("the fixture only creates mouse motion and button events"),
        };
        facts.geometry = None;
        facts.desktop_position = Some(PhysicalPosition::new(
            400.0 + point.x() * scale,
            -300.0 + point.y() * scale,
        ));
        facts.hover = PointerWindowRoute::Window(WindowId::from(WINDOW));
        event
    };
    let surface_position = PhysicalPosition::new(target.x() * scale, target.y() * scale);
    // X11 xinput2_focused reports a client point without geometry, desktop, hover, or capture.
    let synthetic_focus_move = WindowEvent::CursorMoved {
        device_id: DeviceId::dummy(),
        position: surface_position,
        facts: PointerEventFacts {
            surface_position: Some(surface_position),
            ..PointerEventFacts::default()
        },
    };
    now.store(40, Ordering::SeqCst);
    let mut synthetic_location = None;
    for (ordinal, event) in [
        (1, desktop_event(source, Some(ElementState::Pressed))),
        (2, synthetic_focus_move),
        (3, desktop_event(target, None)),
        (4, desktop_event(target, Some(ElementState::Released))),
    ] {
        let record = native.bridge.with_viewports(|routes| {
            NativeWindowEventRecord::for_test_local_capture(
                ordinal,
                WindowId::from(WINDOW),
                Some(ViewportId::ROOT),
                event,
                routes,
                native.session.native_local_coordinates(binding),
                Some(geometry()),
            )
        });
        assert_eq!(record.binding(), Some(binding));
        let routes = record.pointer_routes().unwrap();
        assert_eq!(
            routes.delivery(),
            NativePointerRouteSnapshot::Dock(binding.window())
        );
        if ordinal == 2 {
            assert_eq!(routes.hover(), NativePointerRouteSnapshot::Unknown);
            assert_eq!(routes.capture(), NativePointerRouteSnapshot::Unknown);
            synthetic_location = record.pointer_location();
        } else {
            assert_eq!(
                record.pointer_location(),
                Some(CapturedNativePointerLocation::Desktop)
            );
            assert_eq!(
                routes.hover(),
                NativePointerRouteSnapshot::Dock(binding.window())
            );
            assert_eq!(
                routes.capture(),
                if ordinal == 4 {
                    NativePointerRouteSnapshot::None
                } else {
                    NativePointerRouteSnapshot::Dock(binding.window())
                }
            );
        }
        native.bridge.push_record(HostRecord::WindowEvent(record));
    }
    now.store(100, Ordering::SeqCst);
    assert_eq!(
        native.bridge.input_backlog_for_test(),
        (4, Some(Duration::from_millis(60)))
    );

    reduce_one(&mut native);
    let pressed = finish(
        native
            .begin_host_frame(|query| {
                let query = match query {
                    NativeReceiverQuery::WindowHit(_) => return scene.answer(query, CAPTURED_AT),
                    NativeReceiverQuery::Surface(query) => query,
                };

                assert_eq!(query.point(), Some(source));
                scene.answer(query, CAPTURED_AT)
            })
            .unwrap(),
    );
    assert!(pressed.manipulation_feedback().is_empty());
    assert!(native.session.has_active_gesture());
    assert!(!native.pointer_translator.can_coalesce_idle_cursor_moves());
    let pressed_translator = format!("{:?}", native.pointer_translator);

    reduce_one(&mut native);
    let focused = finish(
        native
            .begin_host_frame(|_| {
                panic!("a synthetic focus move without coordinates cannot query a receiver")
            })
            .unwrap(),
    );
    assert!(
        native.session.has_active_gesture(),
        "missing desktop coordinates must not switch the armed Desktop gesture to NativeLocal"
    );
    assert!(focused.manipulation_feedback().is_empty());
    assert!(!focused.workspace_changed());
    assert_eq!(
        synthetic_location,
        Some(CapturedNativePointerLocation::Desktop)
    );
    assert_eq!(
        format!("{:?}", native.pointer_translator),
        pressed_translator
    );
    assert_eq!(native.session.native_input_recorded_count(), recorded + 2);
    assert_eq!(native.session.version(), version);
    assert_eq!(order(&native), [FIRST, SECOND, THIRD]);

    reduce_one(&mut native);
    let moved = finish(
        native
            .begin_host_frame(|query| {
                let query = match query {
                    NativeReceiverQuery::WindowHit(_) => return scene.answer(query, CAPTURED_AT),
                    NativeReceiverQuery::Surface(query) => query,
                };

                assert_eq!(query.point(), Some(target));
                scene.answer(query, CAPTURED_AT)
            })
            .unwrap(),
    );
    assert!(moved.manipulation_feedback().is_empty());
    assert!(native.session.has_active_gesture());
    assert_eq!(
        format!("{:?}", native.pointer_translator),
        pressed_translator,
        "the focus notification and real move must retain the original mouse identity and capture"
    );

    reduce_one(&mut native);
    let mut frame = native
        .begin_host_frame(|query| {
            let query = match query {
                NativeReceiverQuery::WindowHit(_) => return scene.answer(query, CAPTURED_AT),
                NativeReceiverQuery::Surface(query) => query,
            };

            assert_eq!(query.point(), Some(target));
            scene.answer(query, CAPTURED_AT)
        })
        .unwrap();
    let successor = render(&mut frame);
    let mut report = finish(frame);
    assert_ne!(native.session.version(), version);
    assert_eq!(native.session.native_input_recorded_count(), recorded + 4);
    assert_committed_reorder_survives_presentation(&mut native, binding, &successor, &mut report);
    assert_eq!(native.session.native_input_recorded_count(), recorded + 5);
}
