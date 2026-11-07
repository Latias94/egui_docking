//! A real eframe run exercising core output admission, retention and recovery.

#![cfg(target_os = "linux")]

use std::num::NonZeroUsize;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use eframe::egui;
use eframe::{
    NativeHostHandler, NativeHostWake, NativeOutputPassFinalizer, NativeOutputResult,
    NativeOutputStatus, NativeOutputToken,
};
use egui_dockspace::{DockStyle, PaneView};
use winit::platform::x11::EventLoopBuilderExtX11 as _;

use super::{SURFACE, test_session};
use crate::{NativeDockspaceApp, NativeWindowClosePolicy};

fn lock<T>(value: &Mutex<T>) -> MutexGuard<'_, T> {
    value
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

struct CountingPanes(Arc<AtomicUsize>);

impl PaneView for CountingPanes {
    fn title(&self, _item: dockspace::model::ItemId) -> Option<egui::WidgetText> {
        Some("Output admission regression".into())
    }

    fn ui(
        &mut self,
        _item: dockspace::model::ItemId,
        ui: &mut egui::Ui,
    ) -> egui_dockspace::PaneResponse {
        self.0.fetch_add(1, Ordering::SeqCst);
        ui.label("A real pane callback must resume after the terminal releases its slot.");
        egui_dockspace::PaneResponse::default()
    }
}

#[derive(Debug, Default)]
struct Probe {
    hold_token: Option<NativeOutputToken>,
    buffered: Vec<NativeOutputResult>,
    holding: bool,
    calls_at_hold: usize,
    retained: Vec<NativeOutputToken>,
    released: bool,
    resumed: Option<NativeOutputToken>,
    resumed_presented: bool,
    observed_ordinal: u64,
    done: bool,
}

/// Delays one real terminal and its ordered suffix; every other callback is forwarded.
struct DelayedTerminalHost {
    inner: Arc<dyn NativeHostHandler>,
    probe: Arc<Mutex<Probe>>,
}

impl DelayedTerminalHost {
    fn release(&self) {
        let buffered = {
            let mut probe = lock(&self.probe);
            assert!(probe.holding && !probe.released);
            assert!(!probe.buffered.is_empty());
            probe.holding = false;
            probe.released = true;
            std::mem::take(&mut probe.buffered)
        };
        for terminal in buffered {
            self.inner.on_output(terminal);
        }
    }
}

impl NativeHostHandler for DelayedTerminalHost {
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
        assert!(terminal.ordinal().get() > probe.observed_ordinal);
        probe.observed_ordinal = terminal.ordinal().get();
        if probe.retained.contains(&terminal.token()) {
            assert_eq!(
                terminal.status(),
                NativeOutputStatus::NotPresented,
                "the backpressured pass must retain the existing framebuffer"
            );
        }
        if probe.resumed == Some(terminal.token()) {
            assert_eq!(terminal.status(), NativeOutputStatus::Presented);
            probe.resumed_presented = true;
        }
        if probe.holding || (probe.hold_token == Some(terminal.token()) && !probe.released) {
            if !probe.holding {
                assert_eq!(terminal.status(), NativeOutputStatus::Presented);
            }
            assert!(
                probe.buffered.len() < 4,
                "the test retains a bounded suffix"
            );
            probe.holding = true;
            probe.buffered.push(terminal);
            return NativeHostWake::RepaintRootAfterCurrent;
        }
        drop(probe);
        self.inner.on_output(terminal)
    }
}

struct BackpressureApp {
    app: NativeDockspaceApp<CountingPanes>,
    host: Arc<DelayedTerminalHost>,
    calls: Arc<AtomicUsize>,
    started: Instant,
}

impl eframe::App for BackpressureApp {
    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        assert!(
            self.started.elapsed() < Duration::from_secs(15),
            "output admission did not recover: {:?}",
            lock(&self.host.probe)
        );
        let before = self.calls.load(Ordering::SeqCst);
        eframe::App::ui(&mut self.app, ui, frame);
        self.app.with_errors(|primary, cleanup| {
            assert!(
                primary.is_none(),
                "backpressure is not a native error: {primary:?}"
            );
            assert!(
                cleanup.is_empty(),
                "no cleanup failure is expected: {cleanup:?}"
            );
        });
        let after = self.calls.load(Ordering::SeqCst);
        let retained = self.app.current_output_is_retained();
        let token = eframe::current_native_output_token().unwrap();
        let mut probe = lock(&self.host.probe);
        if probe.holding {
            assert!(
                retained,
                "a full core output slot must retain the framebuffer"
            );
            assert_eq!(
                after, probe.calls_at_hold,
                "a deferred pass must not run pane callbacks"
            );
            if !probe.retained.contains(&token) {
                probe.retained.push(token);
            }
            if probe.retained.len() == 2 {
                drop(probe);
                self.host.release();
            }
        } else if probe.released {
            if after > before && !retained && probe.resumed.is_none() {
                assert!(after > probe.calls_at_hold);
                probe.resumed = Some(token);
            }
            if probe.resumed_presented {
                probe.done = true;
                ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
            }
        } else if probe.hold_token == Some(token) {
            // Egui may repeat the emitting pass before its one terminal callback.
            probe.calls_at_hold = after;
        } else if probe.hold_token.is_none()
            && self.app.is_surface_presented(SURFACE)
            && after > before
            && !retained
        {
            probe.hold_token = Some(token);
            probe.calls_at_hold = after;
        }
        ui.ctx().request_repaint_after(Duration::from_millis(5));
    }
}

#[test]
fn real_eframe_output_capacity_retains_framebuffer_and_resumes_pane_callbacks() {
    if std::env::var_os("DISPLAY").is_none() {
        eprintln!(
            "SKIP real eframe output backpressure: DISPLAY is unavailable; native qualification requires an X11 run"
        );
        return;
    }
    let mut session = test_session();
    session
        .configure_output_capacity(NonZeroUsize::new(1).unwrap())
        .unwrap();
    let calls = Arc::new(AtomicUsize::new(0));
    let app = NativeDockspaceApp::new_with_close_policy(
        egui::Id::unique("real-output-backpressure"),
        session,
        SURFACE,
        CountingPanes(Arc::clone(&calls)),
        DockStyle::default(),
        NativeWindowClosePolicy::RetainLayout,
    )
    .unwrap();
    let probe = Arc::new(Mutex::new(Probe::default()));
    let host = Arc::new(DelayedTerminalHost {
        inner: app.native_host_handler(),
        probe: Arc::clone(&probe),
    });
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([640.0, 480.0]),
        native_host: Some(host.clone()),
        renderer: eframe::Renderer::Glow,
        event_loop_builder: Some(Box::new(|builder| {
            builder.with_x11().with_any_thread(true);
        })),
        ..Default::default()
    };
    eframe::run_native(
        "Dockspace output backpressure regression",
        options,
        Box::new(move |_| {
            Ok(Box::new(BackpressureApp {
                app,
                host,
                calls,
                started: Instant::now(),
            }))
        }),
    )
    .expect("the real eframe renderer exits cleanly");
    let probe = lock(&probe);
    assert!(
        probe.done && probe.released && probe.resumed_presented,
        "{probe:?}"
    );
    assert_eq!(probe.retained.len(), 2);
    assert!(probe.buffered.is_empty());
}
