//! Real native passes keep input private through output hooks and publish once.

#![cfg(target_os = "linux")]

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, Weak};
use std::time::{Duration, Instant};

use dockspace::model::{
    DockspaceLayout, DockspaceNode, DockspaceRootLayout, DockspaceSurfaceLayout, ItemId, RootId,
    WorkspaceVersion,
};
use dockspace::runtime::{DockspaceSession, PresentedDockspaceSurface};
use eframe::egui;
use eframe::{
    NativeHostHandler, NativeHostWake, NativeOutputPassFinalization, NativeOutputPassFinalizer,
    NativeOutputResult, NativeOutputStatus, NativeOutputToken,
};
use egui_dockspace::{DockStyle, PaneView};
use winit::platform::x11::EventLoopBuilderExtX11 as _;

use super::super::{NativeRuntimeState, NativeSurfaceUpdate};
use super::SURFACE;
use crate::NativeWindowClosePolicy;

const FIRST: ItemId = ItemId::new(1);
const SECOND: ItemId = ItemId::new(2);

fn lock<T>(value: &Mutex<T>) -> MutexGuard<'_, T> {
    value
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

struct CountingPanes(Arc<AtomicUsize>);

impl PaneView for CountingPanes {
    fn title(&self, _item: ItemId) -> Option<egui::WidgetText> {
        Some("Owned native update".into())
    }

    fn ui(&mut self, _item: ItemId, ui: &mut egui::Ui) -> egui_dockspace::PaneResponse {
        self.0.fetch_add(1, Ordering::SeqCst);
        ui.label("Each actual pass paints this pane exactly once.");
        egui_dockspace::PaneResponse::default()
    }
}

#[derive(Default)]
struct Probe {
    token: Option<NativeOutputToken>,
    version: Option<WorkspaceVersion>,
    reports_before: usize,
    callbacks_before: usize,
    passes: usize,
    hooks: usize,
    published: bool,
    rejected: Option<NativeOutputToken>,
    previous: Option<PresentedDockspaceSurface>,
    rejection_finalized: bool,
    done: bool,
}

type State = NativeRuntimeState<CountingPanes>;

struct OutputHook {
    state: Weak<Mutex<State>>,
    probe: Arc<Mutex<Probe>>,
    reports: Arc<AtomicUsize>,
}

impl egui::plugin::Plugin for OutputHook {
    fn debug_name(&self) -> &'static str {
        "Native owned update publication probe"
    }

    fn output_hook(&mut self, _context: &egui::Context, output: &mut egui::FullOutput) {
        let state = self.state.upgrade().unwrap();
        let state = lock(&state);
        let mut probe = lock(&self.probe);
        if probe.token.is_some()
            && probe.token == eframe::current_native_output_token()
            && !probe.published
        {
            assert_eq!(self.reports.load(Ordering::SeqCst), probe.reports_before);
            assert_eq!(Some(state.coordinator.session().version()), probe.version);
            assert!(
                state
                    .coordinator
                    .session()
                    .view()
                    .item(FIRST)
                    .unwrap()
                    .is_selected()
            );
            assert!(state.owned_update.is_some());
            probe.hooks += 1;
        }
        if probe.rejected.is_some() && probe.rejected == eframe::current_native_output_token() {
            output.shapes.clear();
            output.shapes.push(egui::epaint::ClippedShape {
                transform_after_tessellation: Default::default(),
                clip_rect: egui::Rect::EVERYTHING,
                shape: egui::Shape::rect_filled(
                    egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(640.0, 480.0)),
                    0.0,
                    egui::Color32::RED,
                ),
            });
        }
    }
}

struct Finalizer {
    state: Weak<Mutex<State>>,
    probe: Arc<Mutex<Probe>>,
    reports: Arc<AtomicUsize>,
    calls: Arc<AtomicUsize>,
}

impl NativeOutputPassFinalizer for Finalizer {
    fn on_service(&self, context: &egui::Context) -> NativeHostWake {
        lock(&self.state.upgrade().unwrap())
            .service_native_events(context)
            .unwrap()
    }

    fn before_output_input(
        &self,
        context: &egui::Context,
        token: NativeOutputToken,
        _has_input: bool,
    ) -> eframe::NativeOutputInputAdmission {
        lock(&self.state.upgrade().unwrap())
            .prepare_output_input(context, token)
            .unwrap()
    }

    fn finalize(&self, finalization: NativeOutputPassFinalization<'_>) -> NativeHostWake {
        let (context, token, viewport, disposition, output) = finalization.into_parts();
        assert_eq!(viewport, token.viewport_id());
        let discarded = disposition == egui::OutputPassDisposition::Repeat;
        let state = self.state.upgrade().unwrap();
        let mut state = lock(&state);
        state
            .settle_egui_pass(context, output, token, discarded)
            .unwrap();
        let mut probe = lock(&self.probe);
        if probe.token == Some(token) && !probe.published {
            assert_eq!(
                self.calls.load(Ordering::SeqCst),
                probe.callbacks_before + probe.passes
            );
            assert_eq!(
                probe.hooks, probe.passes,
                "the output hook precedes every finalizer"
            );
            if discarded {
                assert_eq!(self.reports.load(Ordering::SeqCst), probe.reports_before);
                assert!(state.owned_update.is_some());
            } else {
                assert_eq!(probe.passes, 2);
                assert_eq!(
                    self.reports.load(Ordering::SeqCst),
                    probe.reports_before + 1
                );
                assert!(state.owned_update.is_none());
                assert!(
                    state
                        .coordinator
                        .session()
                        .view()
                        .item(SECOND)
                        .unwrap()
                        .is_selected()
                );
                assert!(
                    state
                        .coordinator
                        .dispatch_bridge()
                        .has_pending_output(token)
                );
                assert!(matches!(state.take_action_status(), Some(Ok(_))));
                probe.published = true;
                return NativeHostWake::RepaintRootAfterCurrent;
            }
        }
        if probe.rejected == Some(token) && !discarded {
            assert!(
                output.shapes.is_empty(),
                "the hook's invalid red replacement must be discarded"
            );
            assert!(output.platform_output.accesskit_update.is_none());
            assert!(
                !state
                    .coordinator
                    .dispatch_bridge()
                    .has_pending_output(token)
            );
            assert_eq!(
                state.coordinator.session().presented_surface(SURFACE),
                probe.previous
            );
            probe.rejection_finalized = true;
            return NativeHostWake::RepaintRootAfterCurrent;
        }
        NativeHostWake::Wait
    }
}

struct OutputProbeHost {
    inner: Arc<dyn NativeHostHandler>,
    probe: Arc<Mutex<Probe>>,
}

impl NativeHostHandler for OutputProbeHost {
    fn install_output_pass_finalizer(&self, finalizer: Arc<dyn NativeOutputPassFinalizer>) -> bool {
        self.inner.install_output_pass_finalizer(finalizer)
    }

    fn try_attach(&self, attachment: eframe::NativeHostAttachment) -> bool {
        self.inner.try_attach(attachment)
    }

    fn detach(&self, attachment: eframe::NativeHostAttachment) {
        self.inner.detach(attachment);
    }

    fn on_service(&self, context: &egui::Context) -> NativeHostWake {
        self.inner.on_service(context)
    }

    fn before_output_input(
        &self,
        context: &egui::Context,
        token: NativeOutputToken,
        has_input: bool,
    ) -> eframe::NativeOutputInputAdmission {
        self.inner.before_output_input(context, token, has_input)
    }

    fn on_window_event(
        &self,
        event: eframe::NativeWindowEvent<'_>,
    ) -> eframe::NativeWindowEventDisposition {
        self.inner.on_window_event(event)
    }

    fn on_device_removed(&self, removal: eframe::NativeDeviceRemoval) {
        self.inner.on_device_removed(removal);
    }

    fn on_global_focus(&self, observation: eframe::NativeGlobalFocusObservation) -> NativeHostWake {
        self.inner.on_global_focus(observation)
    }

    fn on_viewport_focus(&self, result: eframe::NativeViewportFocusResult) -> NativeHostWake {
        self.inner.on_viewport_focus(result)
    }

    fn on_viewport_pointer_passthrough(
        &self,
        result: eframe::NativeViewportPointerPassthroughResult,
    ) -> NativeHostWake {
        self.inner.on_viewport_pointer_passthrough(result)
    }

    fn on_viewport_pointer_observation(
        &self,
        result: eframe::NativeViewportPointerObservationResult,
    ) -> NativeHostWake {
        self.inner.on_viewport_pointer_observation(result)
    }

    fn on_viewport_pointer_capture(
        &self,
        result: eframe::NativeViewportPointerCaptureResult,
    ) -> NativeHostWake {
        self.inner.on_viewport_pointer_capture(result)
    }

    fn on_viewport_property_command_retired(
        &self,
        retirement: eframe::NativeViewportPropertyCommandRetirement,
    ) -> NativeHostWake {
        self.inner.on_viewport_property_command_retired(retirement)
    }

    fn on_viewport_close_cancelled(
        &self,
        request: eframe::NativeViewportCloseRequest,
    ) -> NativeHostWake {
        self.inner.on_viewport_close_cancelled(request)
    }

    fn on_output_begin(
        &self,
        token: NativeOutputToken,
        window: eframe::NativeWindowSnapshot,
        roster: Option<eframe::NativeViewportRoster<'_>>,
    ) -> eframe::NativeOutputAdmission {
        self.inner.on_output_begin(token, window, roster)
    }

    fn on_output_pass_finalized(
        &self,
        finalization: eframe::NativeOutputPassFinalization<'_>,
    ) -> NativeHostWake {
        self.inner.on_output_pass_finalized(finalization)
    }

    fn on_output(&self, terminal: NativeOutputResult) -> NativeHostWake {
        let mut probe = lock(&self.probe);
        if probe.rejected == Some(terminal.token()) {
            assert!(probe.rejection_finalized);
            assert_eq!(
                terminal.status(),
                NativeOutputStatus::NotPresented,
                "a hook-invalidated pass must keep the actual previous framebuffer"
            );
            probe.done = true;
        }
        drop(probe);
        self.inner.on_output(terminal)
    }
}

struct DriverApp {
    state: Arc<Mutex<State>>,
    probe: Arc<Mutex<Probe>>,
    reports: Arc<AtomicUsize>,
    calls: Arc<AtomicUsize>,
    started: Instant,
}

impl eframe::App for DriverApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        assert!(
            self.started.elapsed() < Duration::from_secs(15),
            "the native owned run must finish"
        );
        let token = eframe::current_native_output_token().unwrap();
        let mut state = lock(&self.state);
        let mut probe = lock(&self.probe);
        if probe.token.is_none() && state.is_surface_presented(SURFACE) {
            state.request_select_item(SECOND).unwrap();
            probe.token = Some(token);
            probe.version = Some(state.coordinator.session().version());
            probe.reports_before = self.reports.load(Ordering::SeqCst);
            probe.callbacks_before = self.calls.load(Ordering::SeqCst);
        }
        let update = state.update_surface(ui, SURFACE).unwrap();
        if probe.token == Some(token) && !probe.published {
            assert_eq!(update, NativeSurfaceUpdate::Semantic);
            probe.passes += 1;
            assert_eq!(Some(state.coordinator.session().version()), probe.version);
            assert_eq!(self.reports.load(Ordering::SeqCst), probe.reports_before);
            assert!(
                state
                    .coordinator
                    .session()
                    .view()
                    .item(FIRST)
                    .unwrap()
                    .is_selected()
            );
            assert_eq!(
                state.service_native_events(ui.ctx()).unwrap(),
                NativeHostWake::ServiceAfterCurrent
            );
            assert!(state.owned_update.is_some());
            if probe.passes == 1 {
                ui.ctx()
                    .request_discard("exercise the same native candidate across actual passes");
            }
        }
        if probe.published
            && probe.rejected.is_none()
            && update == NativeSurfaceUpdate::Semantic
            && state.is_surface_presented(SURFACE)
        {
            probe.rejected = Some(token);
            probe.previous = state.coordinator.session().presented_surface(SURFACE);
        }
        if probe.done {
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
        }
        ui.ctx().request_repaint_after(Duration::from_millis(5));
    }
}

#[test]
fn real_eframe_actual_passes_publish_the_owned_update_once_after_output_hooks() {
    if std::env::var_os("DISPLAY").is_none() {
        eprintln!("SKIP real native E3: DISPLAY is unavailable; run the X11 qualification gate");
        return;
    }
    let layout = DockspaceLayout::new([DockspaceSurfaceLayout::new(
        SURFACE,
        DockspaceRootLayout::new(RootId::new(1), DockspaceNode::central_tabs([FIRST, SECOND])),
    )])
    .unwrap();
    let session =
        DockspaceSession::from_layout(layout, dockspace::policy::DockPolicy::default()).unwrap();
    let calls = Arc::new(AtomicUsize::new(0));
    let reports = Arc::new(AtomicUsize::new(0));
    let mut state = NativeRuntimeState::new(
        egui::Id::unique("native-owned-update-regression"),
        session,
        SURFACE,
        CountingPanes(Arc::clone(&calls)),
        DockStyle::default(),
        NativeWindowClosePolicy::RetainLayout,
    )
    .unwrap();
    state.coordinator.observe_reports_for_test({
        let reports = Arc::clone(&reports);
        move |_| {
            reports.fetch_add(1, Ordering::SeqCst);
        }
    });
    let host = state.native_host_handler();
    let state = Arc::new(Mutex::new(state));
    let probe = Arc::new(Mutex::new(Probe::default()));
    assert!(host.install_output_pass_finalizer(Arc::new(Finalizer {
        state: Arc::downgrade(&state),
        probe: Arc::clone(&probe),
        reports: Arc::clone(&reports),
        calls: Arc::clone(&calls),
    })));
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([640.0, 480.0]),
        native_host: Some(Arc::new(OutputProbeHost {
            inner: host,
            probe: Arc::clone(&probe),
        })),
        renderer: eframe::Renderer::Glow,
        event_loop_builder: Some(Box::new(|builder| {
            builder.with_x11().with_any_thread(true);
        })),
        ..Default::default()
    };
    let completed = Arc::clone(&probe);
    eframe::run_native(
        "Native owned update regression",
        options,
        Box::new(move |creation| {
            creation.egui_ctx.options_mut(|options| {
                options.max_passes = std::num::NonZeroUsize::new(2).unwrap()
            });
            creation.egui_ctx.add_plugin(OutputHook {
                state: Arc::downgrade(&state),
                probe: Arc::clone(&probe),
                reports: Arc::clone(&reports),
            });
            Ok(Box::new(DriverApp {
                state,
                probe,
                reports,
                calls,
                started: Instant::now(),
            }))
        }),
    )
    .unwrap();
    assert!(lock(&completed).done);
}
