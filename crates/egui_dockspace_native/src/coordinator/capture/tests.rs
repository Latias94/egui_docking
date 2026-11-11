//! Real X11 backend + recording core receivers + simulated eframe receipt transport.
//! No eframe host, GPU presentation, or complete product journey is exercised here.

#![cfg(target_os = "linux")]

use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use dockspace::geometry::{LogicalPoint, LogicalRect, LogicalSize, PhysicalSize, ScaleFactor};
use dockspace::model::{
    DockspaceLayout, DockspaceNode, DockspaceRootLayout, DockspaceSurfaceLayout, ItemId, RootId,
};
use dockspace::policy::DockPolicy;
use dockspace::runtime::{
    DockspaceReceiverDescriptor, NativeHostCapabilities, NativeHostCapability, NativeLocalGeometry,
    NativeMonotonicClock, NativeReceiverPurpose, NativeWindowHit, NativeWindowInputState,
    NativeWindowPresentationState, PointerDeviceKind, SurfacePaintProgramOperation,
    SurfacePaintProgramStep, SurfacePresentationArtifact, UniformSurfaceMetrics,
};
use eframe::{NativeViewportPointerCaptureStatus as Status, NativeViewportPointerObservation};
use winit::application::ApplicationHandler;
use winit::dpi::{PhysicalPosition, PhysicalSize as WindowSize};
use winit::event::{ElementState, MouseButton, PointerWindowRoute, WindowEvent};
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::platform::pump_events::EventLoopExtPumpEvents;
use winit::platform::x11::{
    ActiveEventLoopExtX11, EventLoopBuilderExtX11, WindowAttributesExtX11,
    X11PointerCaptureAcquisition as Acquisition, X11PointerCaptureRelease as Release,
    X11PointerObservation,
};
use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
use winit::window::Window;

use super::*;
use crate::pointer_capture::NativePointerCaptureRecord;
use crate::pointer_observation::{NativePointerObservationKey, NativePointerObservationRecord};

const SURFACE: SurfaceId = SurfaceId::new(1);
const FIRST: ItemId = ItemId::new(1);
const SECOND: ItemId = ItemId::new(2);
const TICK: Duration = Duration::from_millis(5);
const TIMEOUT: Duration = Duration::from_secs(5);

fn inject(arguments: &[&str]) -> Result<(), String> {
    let mut child = Command::new("xdotool")
        .args(arguments)
        .stdout(Stdio::null())
        .spawn()
        .map_err(|error| error.to_string())?;
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        match child.try_wait() {
            Ok(Some(status)) if status.success() => return Ok(()),
            Ok(None) if Instant::now() < deadline => std::thread::sleep(TICK),
            result => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("xdotool {arguments:?}: {result:?}"));
            }
        }
    }
}

#[derive(Clone)]
struct Press {
    device: DeviceId,
    token: PointerPressToken,
}

struct Raw {
    event: WindowEvent,
    geometry: NativeLocalGeometry,
    captured_at: Duration,
}

#[derive(Default)]
struct Backend {
    source: Option<Window>,
    presses: Vec<Press>,
    raw: Vec<Raw>,
    now: Arc<AtomicU64>,
    route: Option<(Arc<NativeHostBridge>, NativeSurfaceBinding)>,
    capture: Option<(NativePointerCaptureKey, Press)>,
    result: Option<(NativePointerCaptureKey, Status)>,
    observation: Option<(NativePointerObservationKey, Press)>,
    observed: bool,
}

fn geometry(window: &Window) -> NativeLocalGeometry {
    let size = window.inner_size();
    let scale = ScaleFactor::new(window.scale_factor()).unwrap();
    NativeLocalGeometry::new(
        PhysicalSize::new(f64::from(size.width), f64::from(size.height)).unwrap(),
        scale,
    )
}

impl ApplicationHandler for Backend {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.source.is_none() {
            assert!(event_loop.is_x11());
            self.source = Some(
                event_loop
                    .create_window(
                        Window::default_attributes()
                            .with_title("dockspace native capture receipt test")
                            .with_inner_size(WindowSize::new(640, 480))
                            .with_position(PhysicalPosition::new(32, 64))
                            .with_decorations(false)
                            .with_override_redirect(true),
                    )
                    .unwrap(),
            );
        }
    }

    fn window_event(&mut self, _: &ActiveEventLoop, window: WindowId, event: WindowEvent) {
        let Some(source) = self.source.as_ref().filter(|source| source.id() == window) else {
            return;
        };
        if let WindowEvent::MouseInput {
            device_id,
            state: ElementState::Pressed,
            button: MouseButton::Left,
            primary_press,
            ..
        } = &event
        {
            self.presses.push(Press {
                device: *device_id,
                token: primary_press
                    .clone()
                    .expect("a physical Winit original press"),
            });
        }
        if matches!(
            event,
            WindowEvent::CursorMoved { .. } | WindowEvent::MouseInput { .. }
        ) {
            self.raw.push(Raw {
                event,
                geometry: geometry(source),
                captured_at: Duration::from_millis(self.now.load(Ordering::SeqCst)),
            });
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        if let Some((key, press)) = self.capture.take() {
            let status = match key.action {
                Action::Acquire => {
                    match event_loop.acquire_pointer_capture(key.window, press.device, &press.token)
                    {
                        Acquisition::Pending => Status::Pending,
                        Acquisition::Acquired => Status::Acquired,
                        Acquisition::Unavailable => Status::Unavailable,
                        Acquisition::Unsupported => Status::Unsupported,
                    }
                }
                Action::Release => {
                    match event_loop.release_pointer_capture(key.window, press.device, &press.token)
                    {
                        Release::Pending => Status::Pending,
                        Release::Released => Status::Released,
                        Release::Unavailable => Status::Unavailable,
                        Release::Unsupported => Status::Unsupported,
                    }
                }
            };
            let (bridge, binding) = self.route.as_ref().unwrap();
            assert!(
                bridge.record_pointer_capture_for_test(NativePointerCaptureRecord::for_test(
                    key, *binding, status,
                ))
            );
            self.result = Some((key, status));
        }
        if let Some((key, press)) = self.observation.as_ref() {
            match event_loop.observe_pointer(key.window, press.device, Some(&press.token)) {
                X11PointerObservation::Unavailable => {}
                X11PointerObservation::Unsupported => panic!("the real XTest master is supported"),
                X11PointerObservation::Observed { facts, delivery } => {
                    // Acquisition status alone cannot substitute for these fresh routes.
                    assert_eq!(facts.capture, PointerWindowRoute::Window(key.window));
                    assert_eq!(delivery, PointerWindowRoute::Window(key.window));
                    assert_eq!(facts.hover, PointerWindowRoute::Window(key.window));
                    assert!(facts.desktop_position.is_some());
                    let (bridge, binding) = self.route.as_ref().unwrap();
                    let record = bridge.with_viewports(|routes| {
                        NativePointerObservationRecord::for_test(
                            *key,
                            *binding,
                            NativeViewportPointerObservation::Observed { facts, delivery },
                            routes,
                        )
                    });
                    assert!(bridge.record_pointer_observation_for_test(record));
                    self.observed = true;
                    self.observation = None;
                }
            }
        }
    }
}

struct Run {
    backend: Backend,
    event_loop: EventLoop<()>,
}

// Keep the single owning loop and source alive through timeout and panic cleanup.
impl Drop for Run {
    fn drop(&mut self) {
        if let Err(error) = inject(&["mouseup", "1", "getmouselocation"]) {
            eprintln!("capture test button cleanup: {error}");
        }
        struct Cleanup(WindowId, Vec<Press>);
        impl ApplicationHandler for Cleanup {
            fn resumed(&mut self, _: &ActiveEventLoop) {}
            fn window_event(&mut self, _: &ActiveEventLoop, _: WindowId, _: WindowEvent) {}
            fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
                self.1.retain(|press| {
                    matches!(
                        event_loop.release_pointer_capture(self.0, press.device, &press.token),
                        Release::Pending,
                    )
                });
            }
        }
        if let Some(source) = &self.backend.source {
            let mut cleanup = Cleanup(source.id(), self.backend.presses.clone());
            let deadline = Instant::now() + TIMEOUT;
            loop {
                self.event_loop.pump_app_events(Some(TICK), &mut cleanup);
                if cleanup.1.is_empty() {
                    break;
                }
                if Instant::now() >= deadline {
                    eprintln!(
                        "capture test cleanup timed out: {} releases",
                        cleanup.1.len()
                    );
                    break;
                }
            }
        }
    }
}

impl Run {
    fn until(&mut self, description: &str, ready: impl Fn(&Backend) -> bool) {
        let deadline = Instant::now() + TIMEOUT;
        while !ready(&self.backend) {
            assert!(
                Instant::now() < deadline,
                "timed out waiting for {description}"
            );
            self.event_loop
                .pump_app_events(Some(TICK), &mut self.backend);
        }
    }

    fn take(&mut self, predicate: impl Fn(&WindowEvent) -> bool) -> Raw {
        self.until("the exact physical callback", |backend| {
            backend.raw.iter().any(|raw| predicate(&raw.event))
        });
        let index = self
            .backend
            .raw
            .iter()
            .position(|raw| predicate(&raw.event))
            .unwrap();
        self.backend.raw.remove(index)
    }
}

struct Scene(
    Vec<(ItemId, DockspaceReceiverDescriptor, LogicalRect)>,
    NativeSurfaceBinding,
    SurfacePresentationArtifact,
);

impl Scene {
    fn point(&self, item: ItemId) -> LogicalPoint {
        let (_, receiver, bounds) = self.0.iter().find(|tab| tab.0 == item).unwrap();
        let point = LogicalPoint::new(
            bounds.x() + bounds.width() * 0.625,
            bounds.y() + bounds.height() * 0.5,
        )
        .unwrap();
        assert!(receiver.bounds().contains(point));
        assert_ne!(point, receiver.center());
        point
    }

    fn answer(&self, query: NativeReceiverQuery, captured_at: Duration) -> NativeReceiverAnswer {
        let query = match query {
            NativeReceiverQuery::WindowHit(query) => {
                assert_eq!(query.window(), self.1.window());
                return NativeReceiverAnswer::WindowHit(NativeWindowHit::Surface(self.2));
            }
            NativeReceiverQuery::Surface(query) => query,
        };
        assert_eq!(query.surface(), SURFACE);
        assert_eq!(query.metadata().device(), PointerDeviceKind::Mouse);
        assert_eq!(query.metadata().observed_at(), Some(captured_at));
        let Some((_, receiver, _)) = self.0.iter().find(|tab| {
            query
                .point()
                .is_some_and(|point| tab.1.bounds().contains(point))
        }) else {
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

fn finish(mut frame: NativeHostFrame<'_>) -> HostFrameReport {
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    frame.commit().unwrap()
}

fn measure_surfaces(
    frame: &mut NativeHostFrame<'_>,
    binding: NativeSurfaceBinding,
    geometry: NativeLocalGeometry,
) {
    let size = geometry.content_size();
    let scale = geometry.native_scale_factor().get();
    for surface in frame.frame.surfaces() {
        let logical_size = frame
            .frame
            .native_surface_bindings_for_window(binding.window())
            .find(|binding| binding.surface() == surface)
            .and_then(|binding| frame.frame.native_surface_placement(binding))
            .map(|placement| placement.size())
            .unwrap_or_else(|| {
                LogicalSize::new(size.width() / scale, size.height() / scale).unwrap()
            });
        frame
            .frame
            .measure_surface(
                surface,
                UniformSurfaceMetrics::new(
                    LogicalRect::new(0.0, 0.0, logical_size.width(), logical_size.height())
                        .unwrap(),
                    LogicalSize::new(32.0, 24.0).unwrap(),
                    96.0,
                )
                .unwrap(),
            )
            .unwrap();
    }
}

fn present(
    native: &mut NativeCoordinator,
    binding: NativeSurfaceBinding,
    geometry: NativeLocalGeometry,
) -> Scene {
    let mut frame = native
        .begin_host_frame(|_| NativeReceiverAnswer::Unknown)
        .unwrap();
    measure_surfaces(&mut frame, binding, geometry);
    let mut tabs = Vec::new();
    for surface in frame.frame.surfaces() {
        let plan = frame.frame.paint_plan(surface).unwrap().unwrap();
        if surface == SURFACE {
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
        }
        let receipt = plan.execute_surface_program(|_| Ok::<(), ()>(())).unwrap();
        frame.frame.confirm_surface_painted(receipt).unwrap();
    }
    let mut report = finish(frame);
    let outputs = report.take_painted_outputs();
    let output = outputs
        .iter()
        .find(|output| output.surface() == SURFACE)
        .unwrap();
    assert!(output.matches_native_binding(binding));
    let artifact = output.presentation_artifact();
    native
        .session
        .report_surface_presentations(outputs, SurfacePresentationResult::Presented)
        .unwrap();
    let report = finish(
        native
            .begin_host_frame(|_| NativeReceiverAnswer::Unknown)
            .unwrap(),
    );
    native.settle_host_frame_report(&report).unwrap();
    Scene(tabs, binding, artifact)
}

struct Core {
    native: NativeCoordinator,
    binding: NativeSurfaceBinding,
    scene: Scene,
    context: eframe::egui::Context,
    clock: NativeMonotonicClock,
    now: Arc<AtomicU64>,
}

impl Core {
    fn new(window: &Window, now: Arc<AtomicU64>, placement: SourcePlacement) -> Self {
        use dockspace::geometry::PhysicalPoint;
        use dockspace::runtime::NativeSurfacePlacement;

        let mut surfaces = vec![DockspaceSurfaceLayout::new(
            SURFACE,
            DockspaceRootLayout::new(RootId::new(1), DockspaceNode::central_tabs([FIRST, SECOND])),
        )];
        let representative = if matches!(placement, SourcePlacement::SharedSibling) {
            surfaces.push(DockspaceSurfaceLayout::new(
                SurfaceId::new(2),
                DockspaceRootLayout::new(
                    RootId::new(2),
                    DockspaceNode::central_tabs([ItemId::new(3)]),
                ),
            ));
            SurfaceId::new(2)
        } else {
            SURFACE
        };
        let layout = DockspaceLayout::new(surfaces).unwrap();
        let ticks = Arc::clone(&now);
        let clock =
            NativeMonotonicClock::new(move || Duration::from_millis(ticks.load(Ordering::SeqCst)));
        let mut native = NativeCoordinator::new_with_storage_allowance(
            DockspaceSession::from_layout(layout, DockPolicy::default()).unwrap(),
            NativePointerRoster::Exact(Vec::new()),
            None,
            None,
            None,
            Some(clock.clone()),
        )
        .unwrap();
        native
            .register_native_root(representative, HostWindowToken::new(u64::from(window.id())))
            .unwrap();
        let report = finish(
            native
                .begin_host_frame(|_| NativeReceiverAnswer::Unknown)
                .unwrap(),
        );
        native.settle_host_frame_report(&report).unwrap();
        let binding = report.native_bindings()[0];
        native
            .bind_viewport(ViewportId::ROOT, window.id(), binding)
            .unwrap();
        let geometry = geometry(window);
        let origin = window.inner_position().unwrap();
        let size = geometry.content_size();
        let facts = NativeWindowFacts::live()
            .with_content_bounds(
                PhysicalRect::new(
                    f64::from(origin.x),
                    f64::from(origin.y),
                    size.width(),
                    size.height(),
                )
                .unwrap(),
            )
            .with_native_scale_factor(geometry.native_scale_factor())
            .with_presentation_scale_factor(geometry.native_scale_factor())
            .with_input(NativeWindowInputState::ReceivesInput, None)
            .with_presentation(NativeWindowPresentationState::Visible);
        native
            .report_snapshot(
                [(binding, facts)],
                NativeWorkAreaRoster::Unknown,
                NativeHostCapabilities::none_supported()
                    .with(NativeHostCapability::AuthoritativeInventory)
                    .with(NativeHostCapability::AuthoritativeButtonState)
                    .with(NativeHostCapability::PointerHitTestObservation)
                    .with(NativeHostCapability::DesktopPointerPosition)
                    .with(NativeHostCapability::HoveredWindow),
            )
            .unwrap();
        let mut frame = native
            .begin_host_frame(|_| NativeReceiverAnswer::Unknown)
            .unwrap();
        let scale = geometry.native_scale_factor().get();
        if !matches!(placement, SourcePlacement::Implicit) {
            // Identical declarations are a no-op. Rebase actual geometry to
            // exercise a newer placement generation on the same source surface.
            let source_width = if matches!(placement, SourcePlacement::Rebased) {
                size.width() - scale
            } else {
                size.width()
            };
            let mut placements = vec![NativeSurfacePlacement::new(
                SURFACE,
                LogicalSize::new(source_width / scale, size.height() / scale).unwrap(),
                PhysicalPoint::new(0.0, 0.0).unwrap(),
                geometry.native_scale_factor(),
                PhysicalRect::new(0.0, 0.0, source_width, size.height()).unwrap(),
            )];
            if representative != SURFACE {
                placements.push(NativeSurfacePlacement::new(
                    representative,
                    LogicalSize::new(20.0, 20.0).unwrap(),
                    PhysicalPoint::new(size.width() - 20.0 * scale, size.height() - 20.0 * scale)
                        .unwrap(),
                    geometry.native_scale_factor(),
                    PhysicalRect::new(
                        size.width() - 20.0 * scale,
                        size.height() - 20.0 * scale,
                        20.0 * scale,
                        20.0 * scale,
                    )
                    .unwrap(),
                ));
            }
            frame
                .frame
                .set_native_surface_placements(binding.window(), &placements)
                .unwrap();
        }
        measure_surfaces(&mut frame, binding, geometry);
        let report = finish(frame);
        native.settle_host_frame_report(&report).unwrap();
        let binding = native
            .session
            .native_surface_bindings_for_window(binding.window())
            .find(|binding| binding.surface() == SURFACE)
            .unwrap();
        if !matches!(placement, SourcePlacement::Implicit) {
            assert_ne!(
                native
                    .bridge
                    .with_viewports(|routes| routes.binding(ViewportId::ROOT)),
                Some(binding),
                "the source witness must differ from the callback route's historical surface tag"
            );
        }
        let scene = present(&mut native, binding, geometry);
        for (_, receiver, _) in &scene.0 {
            assert!(native.session.bind_presented_receiver(receiver).is_some());
        }
        native.bridge.enable_observation_attachment_for_test();
        Self {
            native,
            binding,
            scene,
            context: eframe::egui::Context::default(),
            clock,
            now,
        }
    }

    fn budget(&self) -> NativeInputBudget {
        NativeInputBudget::new(&self.clock)
    }

    fn commit(&mut self, captured_at: Duration) -> HostFrameReport {
        let scene = &self.scene;
        let report = finish(
            self.native
                .begin_host_frame(|query| scene.answer(query, captured_at))
                .unwrap(),
        );
        self.native.settle_host_frame_report(&report).unwrap();
        report
    }

    fn reduce(&mut self) {
        let mut budget = self.budget();
        assert_eq!(
            self.native
                .reduce_callback_prefix(&self.context, &mut budget)
                .unwrap()
                .unwrap()
                .get(),
            1
        );
    }

    fn input(&mut self, window: WindowId, raw: Raw) {
        assert_eq!(self.clock.now(), raw.captured_at);
        let count_before = self.native.session.native_input_recorded_count();
        let coordinates = self.native.session.native_local_coordinates(self.binding);
        let ordinal = self.native.bridge.captured_through_for_test() + 1;
        let record = self.native.bridge.with_viewports(|routes| {
            NativeWindowEventRecord::for_test_local_capture(
                ordinal,
                window,
                Some(ViewportId::ROOT),
                raw.event,
                routes,
                coordinates,
                Some(raw.geometry),
            )
        });
        self.native
            .bridge
            .push_record(HostRecord::WindowEvent(record));
        assert_eq!(
            self.native.bridge.front_captured_event().unwrap().1,
            raw.captured_at
        );
        self.reduce();
        assert_eq!(
            self.native.session.native_input_recorded_count(),
            count_before + 1
        );
        let version = self.native.session.version();
        let count = self.native.session.native_input_recorded_count();
        let scene = &self.scene;
        drop(
            self.native
                .begin_host_frame(|query| scene.answer(query, raw.captured_at))
                .unwrap(),
        );
        assert_eq!(self.native.session.version(), version);
        assert_eq!(self.native.session.native_input_recorded_count(), count);
        self.commit(raw.captured_at);
    }
}

#[derive(Clone, Copy)]
enum SourcePlacement {
    Implicit,
    Rebased,
    SharedSibling,
}

#[test]
#[ignore = "requires DOCKSPACE_X11_CAPTURE_TEST=1, isolated X11, XTest and xdotool"]
fn real_x11_original_press_capture_receipts_and_cleanup() {
    assert_eq!(
        std::env::var("DOCKSPACE_X11_CAPTURE_TEST").as_deref(),
        Ok("1")
    );
    let mut run = Run {
        backend: Backend::default(),
        event_loop: EventLoop::builder()
            .with_x11()
            .with_any_thread(true)
            .build()
            .unwrap(),
    };
    run.until("a viewable direct-root source", |backend| {
        backend
            .source
            .as_ref()
            .is_some_and(|window| window.is_visible() == Some(true))
    });
    for placement in [
        SourcePlacement::Implicit,
        SourcePlacement::Rebased,
        SourcePlacement::SharedSibling,
    ] {
        for delayed_ordinary in [false, true] {
            capture_receipt_journey(&mut run, delayed_ordinary, placement);
        }
    }
}

fn capture_receipt_journey(run: &mut Run, delayed_ordinary: bool, placement: SourcePlacement) {
    run.backend.raw.clear();
    run.backend.observed = false;
    run.backend.now.store(1_000, Ordering::SeqCst);
    let previous_presses = run.backend.presses.len();
    let window = run.backend.source.as_ref().unwrap();
    let source = window.id();
    let xid = match window.window_handle().unwrap().as_raw() {
        RawWindowHandle::Xlib(handle) => handle.window.to_string(),
        RawWindowHandle::Xcb(handle) => handle.window.get().to_string(),
        _ => panic!("the test requires X11"),
    };
    let mut core = Core::new(window, Arc::clone(&run.backend.now), placement);
    run.backend.route = Some((Arc::clone(&core.native.bridge), core.binding));
    let scale = window.scale_factor();
    let first = core.scene.point(FIRST);
    let target = core.scene.point(SECOND);
    let move_to = |point: LogicalPoint| {
        vec![
            "mousemove".to_owned(),
            "--window".to_owned(),
            xid.clone(),
            (point.x() * scale).round().to_string(),
            (point.y() * scale).round().to_string(),
        ]
    };
    let mut arguments = move_to(first);
    arguments.extend(["mousedown".into(), "1".into(), "getmouselocation".into()]);
    core.now.store(1_000, Ordering::SeqCst);
    inject(&arguments.iter().map(String::as_str).collect::<Vec<_>>()).unwrap();
    let press_raw = run.take(|event| {
        matches!(
            event,
            WindowEvent::MouseInput {
                state: ElementState::Pressed,
                button: MouseButton::Left,
                ..
            }
        )
    });
    core.input(source, press_raw);
    assert_eq!(run.backend.presses.len(), previous_presses + 1);
    let press = run.backend.presses.last().unwrap().clone();
    run.backend.raw.clear();
    core.now.store(1_010, Ordering::SeqCst);
    let mut arguments = move_to(target);
    arguments.push("getmouselocation".into());
    inject(&arguments.iter().map(String::as_str).collect::<Vec<_>>()).unwrap();
    let motion = run.take(|event| matches!(event, WindowEvent::CursorMoved { facts, .. } if facts.surface_position.is_some_and(|point| (point.x - target.x() * scale).abs() <= 1.0)));
    core.input(source, motion);
    assert!(core.native.session.has_active_gesture());
    let request = core
        .native
        .session
        .native_pointer_capture_request()
        .expect("a real Desktop drag requests capture");
    assert_eq!(request.source_binding(), core.binding);
    assert_eq!(
        core.native
            .pointer_translator
            .mouse_press_for_pointer(request.pointer()),
        Some((press.device, Some(&press.token)))
    );
    let continuation = core.native.session.native_pointer_continuation().unwrap();
    assert!(continuation.local_coordinates().is_none());
    assert!(continuation.capture_acquired_event().is_some());
    let recorded = core.native.session.native_input_recorded_count();
    let ordinary_key = delayed_ordinary.then(|| {
        core.now.store(
            u64::try_from(continuation.deadline().as_millis()).unwrap() + 1,
            Ordering::SeqCst,
        );
        // This ordinary query is prepared before capture dispatch, then its
        // actual backend sample runs only after the acquire receipt settles.
        let mut budget = core.budget();
        let prepared = core
            .native
            .prepare_stationary_observation(&core.context, &mut budget)
            .expect("an earlier ordinary query can already own its receipt");
        let command = core.native.admit_stationary_observation(prepared).unwrap();
        let key = core.native.pending_stationary_key().unwrap();
        assert_eq!(key.token, command.token());
        command.enqueue(&core.context);
        key
    });
    let mut statuses = Vec::new();
    let mut checked_pending_deadline = false;
    let deadline = Instant::now() + TIMEOUT;
    loop {
        assert!(Instant::now() < deadline, "capture acquisition timed out");
        let mut budget = core.budget();
        assert!(
            core.native
                .drive_pointer_capture(&core.context, &mut budget, true)
                .unwrap()
        );
        let duty = core.native.stationary.capture.as_ref().unwrap();
        let key = duty.pending.unwrap();
        assert_eq!(
            (key.window, key.device, key.action),
            (source, press.device, Action::Acquire)
        );
        assert_eq!(duty.press, press.token);
        let next = eframe::prepare_native_viewport_pointer_capture(
            key.viewport,
            source,
            press.device,
            press.token.clone(),
            Action::Acquire,
        );
        let foreign = NativePointerCaptureKey {
            token: next.token(),
            ..key
        };
        assert_ne!(foreign.token, key.token);
        assert!(
            !core
                .native
                .bridge
                .reserve_pointer_capture(foreign, core.binding)
        );
        run.backend.result = None;
        run.backend.capture = Some((key, press.clone()));
        run.until("one actual backend capture result", |backend| {
            backend.result.is_some()
        });
        let (_, status) = run.backend.result.unwrap();
        statuses.push(status);
        let (record, captured_at) = core.native.bridge.front_pointer_capture().unwrap();
        assert_eq!(record.key(), key);
        assert_eq!(record.status(), status);
        assert_eq!(captured_at, core.clock.now());
        assert!(!core.native.bridge.record_pointer_capture_for_test(
            NativePointerCaptureRecord::for_test(key, core.binding, status)
        ));
        assert!(!core.native.bridge.record_pointer_capture_for_test(
            NativePointerCaptureRecord::for_test(foreign, core.binding, status)
        ));
        let (retained, time) = core.native.bridge.front_pointer_capture().unwrap();
        assert_eq!(
            (retained.key(), retained.status(), time),
            (key, status, captured_at)
        );
        core.reduce();
        let version = core.native.session.version();
        drop(
            core.native
                .begin_host_frame(|_| NativeReceiverAnswer::Unknown)
                .unwrap(),
        );
        assert!(core.native.bridge.pointer_capture_pending(key));
        assert!(
            !core
                .native
                .bridge
                .reserve_pointer_capture(foreign, core.binding)
        );
        assert!(
            !core
                .native
                .drive_pointer_capture(&core.context, &mut budget, true)
                .unwrap()
        );
        assert_eq!(core.native.session.version(), version);
        assert_eq!(core.native.session.native_input_recorded_count(), recorded);
        core.commit(captured_at);
        assert!(!core.native.bridge.pointer_capture_pending(key));
        assert_eq!(
            core.native.session.native_pointer_continuation(),
            Some(continuation)
        );
        if status == Status::Acquired {
            break;
        }
        assert_eq!(status, Status::Pending);
        if !checked_pending_deadline && !delayed_ordinary {
            core.now.store(
                u64::try_from(continuation.deadline().as_millis()).unwrap() + 1,
                Ordering::SeqCst,
            );
            let mut budget = core.budget();
            assert!(
                core.native
                    .prepare_stationary_observation(&core.context, &mut budget)
                    .is_none(),
                "an unresolved acquisition must not consume the old dwell deadline"
            );
            assert_eq!(
                core.native.session.native_pointer_continuation(),
                Some(continuation)
            );
            checked_pending_deadline = true;
        }
    }
    assert!(
        statuses.contains(&Status::Pending),
        "exercise the real FIFO Pending receipt"
    );
    assert_eq!(core.native.session.native_input_recorded_count(), recorded);
    assert!(core.native.capture_is_acquired_for(continuation));

    // Only fresh press-qualified observation can produce CaptureAcquired in core.
    core.now.fetch_add(10, Ordering::SeqCst);
    let acquisition_time = core.clock.now();
    assert!(acquisition_time > continuation.deadline());
    let key = ordinary_key.unwrap_or_else(|| {
        let mut budget = core.budget();
        let prepared = core
            .native
            .prepare_stationary_observation(&core.context, &mut budget)
            .expect("fresh acquisition restarts an already due dwell interval");
        let command = core.native.admit_stationary_observation(prepared).unwrap();
        let key = core.native.pending_stationary_key().unwrap();
        assert_eq!(key.token, command.token());
        command.enqueue(&core.context);
        key
    });
    run.backend.observation = Some((key, press.clone()));
    run.until("fresh capture and delivery observation", |backend| {
        backend.observed
    });
    let captured_at = core.native.bridge.front_pointer_observation().unwrap().1;
    assert_eq!(captured_at, core.clock.now());
    core.reduce();
    assert_eq!(
        core.native.session.native_input_recorded_count(),
        recorded + 1
    );
    let version = core.native.session.version();
    drop(
        core.native
            .begin_host_frame(|query| core.scene.answer(query, captured_at))
            .unwrap(),
    );
    assert_eq!(core.native.session.version(), version);
    assert_eq!(
        core.native.session.native_input_recorded_count(),
        recorded + 1
    );
    assert!(core.native.session.has_pending_native_input());
    assert!(core.native.session.native_pointer_continuation().is_none());
    assert!(
        core.native
            .session
            .native_pointer_capture_request()
            .is_none()
    );
    assert!(core.native.stationary.capture.is_some());
    core.commit(captured_at);
    core.native.finish_stationary_observation();
    assert!(
        core.native
            .session
            .view()
            .item(FIRST)
            .unwrap()
            .is_selected(),
        "first fresh acquisition must restart the interval before selecting a target"
    );
    let acquired = core.native.session.native_pointer_continuation().unwrap();
    assert_ne!(acquired.token(), continuation.token());
    assert_eq!(
        acquired.deadline() - continuation.deadline(),
        acquisition_time - Duration::from_millis(1_010)
    );
    assert!(acquired.capture_acquired_event().is_none());
    assert!(
        !core
            .native
            .session
            .view()
            .item(SECOND)
            .unwrap()
            .is_selected()
    );
    let mut budget = core.budget();
    assert!(
        core.native
            .prepare_stationary_observation(&core.context, &mut budget)
            .is_none()
    );
    assert_eq!(
        core.native.session.native_pointer_continuation(),
        Some(acquired)
    );

    run.backend.raw.clear();
    core.now.fetch_add(10, Ordering::SeqCst);
    inject(&["mouseup", "1", "getmouselocation"]).unwrap();
    let release = run.take(|event| matches!(event, WindowEvent::MouseInput { state: ElementState::Released, button: MouseButton::Left, device_id, .. } if *device_id == press.device));
    core.input(source, release);
    assert!(
        core.native
            .session
            .native_pointer_capture_request()
            .is_none()
    );
    let deadline = Instant::now() + TIMEOUT;
    while core.native.stationary.capture.is_some() {
        assert!(Instant::now() < deadline, "exact native release timed out");
        let mut budget = core.budget();
        assert!(
            core.native
                .drive_pointer_capture(&core.context, &mut budget, true)
                .unwrap()
        );
        let duty = core.native.stationary.capture.as_ref().unwrap();
        let key = duty.pending.unwrap();
        assert_eq!(
            (key.window, key.device, key.action),
            (source, press.device, Action::Release)
        );
        assert_eq!(duty.press, press.token);
        run.backend.result = None;
        run.backend.capture = Some((key, press.clone()));
        run.until("the original source release result", |backend| {
            backend.result.is_some()
        });
        let status = run.backend.result.unwrap().1;
        assert!(matches!(
            status,
            Status::Pending | Status::Released | Status::Unavailable
        ));
        core.reduce();
        core.commit(core.clock.now());
        assert!(!core.native.bridge.pointer_capture_pending(key));
    }
    // Remeasure and present the layout committed by release using a recording receipt.
    // This is simulated presentation transport, with no renderer or GPU claim.
    let _ = present(
        &mut core.native,
        core.binding,
        geometry(run.backend.source.as_ref().unwrap()),
    );
    assert!(!core.native.session.has_active_gesture());
    assert!(!core.native.session.has_pending_native_input());
    assert!(!core.native.stationary.has_pending_work());
    assert!(
        !core.native.bridge.has_pending_coordinator_work(),
        "capture must release its window resource pin"
    );
    assert!(core.native.is_quiescent());
}
