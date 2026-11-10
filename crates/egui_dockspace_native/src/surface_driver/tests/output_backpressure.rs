//! A real eframe run exercising core output admission, retention and recovery.

#![cfg(target_os = "linux")]

use std::num::NonZeroUsize;
use std::process::Command;
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
    text_mode: bool,
    resize_before_key: bool,
    native_resize_seen: bool,
    native_text_seen: bool,
    text_input_token: Option<NativeOutputToken>,
    text_input_admission: Option<eframe::NativeOutputInputAdmission>,
    release_on_service: bool,
    text_reached_retained_ui: bool,
    expand_roster_on_text: bool,
    ordinary_entry_on_text: bool,
    final_text: Option<String>,
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
        let release = {
            let mut probe = lock(&self.probe);
            std::mem::take(&mut probe.release_on_service)
        };
        if release {
            self.release();
            context.request_repaint();
        }
        self.inner.on_service(context)
    }

    fn before_output_input(
        &self,
        context: &egui::Context,
        token: NativeOutputToken,
        has_input: bool,
    ) -> eframe::NativeOutputInputAdmission {
        let admission = self.inner.before_output_input(context, token, has_input);
        let mut probe = lock(&self.probe);
        if probe.text_mode
            && (probe.holding || probe.ordinary_entry_on_text)
            && probe.native_text_seen
            && has_input
            && if probe.ordinary_entry_on_text {
                !matches!(
                    probe.text_input_admission,
                    Some(eframe::NativeOutputInputAdmission::Proceed)
                )
            } else {
                probe.text_input_token.is_none()
            }
        {
            probe.text_input_token = Some(token);
            probe.text_input_admission = Some(admission);
            // Release only after this real output attempts input admission. A
            // deferred pass has no App callback, so recovery belongs to service.
            probe.release_on_service = !probe.ordinary_entry_on_text
                && (!probe.expand_roster_on_text
                    || matches!(admission, eframe::NativeOutputInputAdmission::Defer(_)));
        }
        admission
    }

    fn on_window_event(
        &self,
        event: eframe::NativeWindowEvent<'_>,
    ) -> eframe::NativeWindowEventDisposition {
        if matches!(
            event.event(),
            winit::event::WindowEvent::Resized(size) if size.width == 720 && size.height == 520
        ) {
            let mut probe = lock(&self.probe);
            if probe.text_mode && probe.holding {
                probe.native_resize_seen = true;
            }
        }
        if let winit::event::WindowEvent::KeyboardInput { event, .. } = event.event()
            && event.state == winit::event::ElementState::Pressed
            && !event.repeat
            && event.text.as_deref() == Some("a")
        {
            let mut probe = lock(&self.probe);
            if probe.text_mode && (probe.holding || probe.ordinary_entry_on_text) {
                probe.native_text_seen = true;
            }
        }
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
        if probe.retained.contains(&terminal.token())
            && !(probe.resize_before_key && probe.native_resize_seen)
        {
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
            let send_expansion_key = !probe.holding && probe.expand_roster_on_text;
            if !probe.holding {
                assert_eq!(terminal.status(), NativeOutputStatus::Presented);
            }
            assert!(
                probe.buffered.len() < if probe.text_mode { 16 } else { 4 },
                "the test retains a bounded suffix"
            );
            probe.holding = true;
            let token = terminal.token();
            probe.buffered.push(terminal);
            if send_expansion_key {
                // Queue the real key before another idle repaint can consume
                // the one output slot left by this held renderer terminal.
                drop(probe);
                let window = u64::from(token.window_id()).to_string();
                let result = Command::new("xdotool")
                    .args(["windowactivate", "--sync", &window, "key", "a"])
                    .output()
                    .expect("the X11 roster expansion regression requires xdotool");
                assert!(result.status.success(), "{result:?}");
                return NativeHostWake::Wait;
            }
            return if probe.release_on_service {
                NativeHostWake::ServiceAfterCurrent
            } else {
                NativeHostWake::RepaintRootAfterCurrent
            };
        }
        drop(probe);
        self.inner.on_output(terminal)
    }
}

struct EditorPanes {
    text: String,
    editor_item: dockspace::model::ItemId,
    target: egui::Id,
    calls: Arc<AtomicUsize>,
}

impl PaneView for EditorPanes {
    fn title(&self, _: dockspace::model::ItemId) -> Option<egui::WidgetText> {
        Some("Backpressured editor".into())
    }

    fn ui(
        &mut self,
        item: dockspace::model::ItemId,
        ui: &mut egui::Ui,
    ) -> egui_dockspace::PaneResponse {
        if item != self.editor_item {
            ui.label("This independent surface shares the same renderer terminal.");
            return egui_dockspace::PaneResponse::default();
        }
        let response = ui.add(egui::TextEdit::singleline(&mut self.text).id(self.target));
        if self.calls.fetch_add(1, Ordering::SeqCst) == 0 {
            response.request_focus();
        }
        egui_dockspace::PaneResponse::default().with_content(&response)
    }

    fn focus_target(&self, item: dockspace::model::ItemId) -> Option<egui::Id> {
        (item == self.editor_item).then_some(self.target)
    }
}

struct RetainedTextApp {
    app: NativeDockspaceApp<EditorPanes>,
    host: Arc<DelayedTerminalHost>,
    calls: Arc<AtomicUsize>,
    key_sent: bool,
    resize_before_key: bool,
    shared: bool,
    started: Instant,
}

impl eframe::App for RetainedTextApp {
    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        assert!(
            self.started.elapsed() < Duration::from_secs(15),
            "the real keyboard input must recover: {:?}",
            lock(&self.host.probe)
        );
        let before = self.calls.load(Ordering::SeqCst);
        if self.shared {
            use dockspace::geometry::{LogicalSize, PhysicalPoint, PhysicalRect, ScaleFactor};
            use dockspace::runtime::NativeSurfacePlacement;

            let scale = f64::from(ui.ctx().pixels_per_point());
            let placements = [1, 2].map(|id| {
                let x = if id == 1 { 0.0 } else { 320.0 };
                NativeSurfacePlacement::new(
                    dockspace::model::SurfaceId::new(id),
                    LogicalSize::new(300.0, 440.0).unwrap(),
                    PhysicalPoint::new(x * scale, 0.0).unwrap(),
                    ScaleFactor::new(scale).unwrap(),
                    PhysicalRect::new(x * scale, 0.0, 300.0 * scale, 440.0 * scale).unwrap(),
                )
            });
            self.app.show_root_surfaces(ui, &placements);
        } else {
            eframe::App::ui(&mut self.app, ui, frame);
        }
        assert!(self.app.error_kind().is_none());
        let after = self.calls.load(Ordering::SeqCst);
        let retained = self.app.current_output_is_retained();
        let token = eframe::current_native_output_token().unwrap();
        let mut probe = lock(&self.host.probe);
        if probe.holding {
            assert!(retained, "the real occupied output slot must retain pixels");
            assert_eq!(after, probe.calls_at_hold);
            if !probe.retained.contains(&token) {
                probe.retained.push(token);
            }
            if ui.input(|input| {
                input
                    .events
                    .iter()
                    .any(|event| matches!(event, egui::Event::Text(text) if text == "a"))
            }) {
                probe.text_reached_retained_ui = true;
            }
            if !self.key_sent {
                self.key_sent = true;
                drop(probe);
                let window = u64::from(token.window_id()).to_string();
                let mut command = Command::new("xdotool");
                command.args(["windowactivate", "--sync", &window]);
                if self.resize_before_key {
                    command.args(["windowsize", "--sync", &window, "720", "520"]);
                }
                let result = command
                    .args(["key", "a"])
                    .output()
                    .expect("the X11 keyboard regression requires xdotool");
                assert!(result.status.success(), "{result:?}");
            }
        } else if probe.released {
            if after > before && !retained && probe.resumed.is_none() {
                probe.resumed = Some(token);
            }
            if probe.resumed_presented {
                probe.done = true;
                ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
            }
        } else if probe.hold_token == Some(token) {
            probe.calls_at_hold = after;
        } else if probe.hold_token.is_none()
            && self.app.is_surface_presented(SURFACE)
            && (!self.shared
                || self
                    .app
                    .is_surface_presented(dockspace::model::SurfaceId::new(2)))
            && after > before
            && !retained
        {
            probe.hold_token = Some(token);
            probe.calls_at_hold = after;
        }
        ui.ctx().request_repaint_after(Duration::from_millis(5));
    }

    fn on_exit(&mut self, _: Option<&eframe::glow::Context>) {
        let text = self.app.with_panes(|panes| panes.text.clone());
        assert_eq!(
            text,
            "a",
            "a real X11 Text event admitted while output is retained must reach the restored TextEdit; probe={:?}",
            lock(&self.host.probe)
        );
    }
}

#[test]
#[ignore = "requires isolated X11/XTest renderer, an EWMH WM, and xdotool"]
fn real_eframe_output_capacity_preserves_text_until_the_editor_resumes() {
    assert_retained_text_delivery_after_backpressure(false, false);
}

#[test]
#[ignore = "requires isolated X11/XTest renderer, an EWMH WM, and xdotool"]
fn real_eframe_output_capacity_preserves_text_after_native_resize() {
    assert_retained_text_delivery_after_backpressure(true, false);
}

#[test]
#[ignore = "requires isolated X11/XTest renderer, an EWMH WM, and xdotool"]
fn real_eframe_shared_output_capacity_defers_text_until_both_surface_slots_are_available() {
    assert_retained_text_delivery_after_backpressure(false, true);
}

fn assert_retained_text_delivery_after_backpressure(resize_before_key: bool, shared: bool) {
    assert!(std::env::var_os("DISPLAY").is_some());
    let mut session = if shared {
        use dockspace::model::{
            DockspaceLayout, DockspaceNode, DockspaceRootLayout, DockspaceSurfaceLayout, ItemId,
            RootId, SurfaceId,
        };

        dockspace::runtime::DockspaceSession::from_layout(
            DockspaceLayout::new([1, 2].map(|id| {
                DockspaceSurfaceLayout::new(
                    SurfaceId::new(id),
                    DockspaceRootLayout::new(
                        RootId::new(id),
                        DockspaceNode::central_tabs([ItemId::new(id)]),
                    ),
                )
            }))
            .unwrap(),
            dockspace::policy::DockPolicy::default(),
        )
        .unwrap()
    } else {
        test_session()
    };
    session
        .configure_output_capacity(NonZeroUsize::new(if shared { 3 } else { 1 }).unwrap())
        .unwrap();
    let calls = Arc::new(AtomicUsize::new(0));
    let app = NativeDockspaceApp::new_with_close_policy(
        egui::Id::unique("real-retained-text"),
        session,
        SURFACE,
        EditorPanes {
            text: String::new(),
            editor_item: dockspace::model::ItemId::new(1),
            target: egui::Id::unique("real-retained-text-editor"),
            calls: Arc::clone(&calls),
        },
        DockStyle::default(),
        NativeWindowClosePolicy::RetainLayout,
    )
    .unwrap();
    let probe = Arc::new(Mutex::new(Probe {
        text_mode: true,
        resize_before_key,
        ..Probe::default()
    }));
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
        "Dockspace retained native text regression",
        options,
        Box::new(move |_| {
            Ok(Box::new(RetainedTextApp {
                app,
                host,
                calls,
                key_sent: false,
                resize_before_key,
                shared,
                started: Instant::now(),
            }))
        }),
    )
    .unwrap();
    let probe = lock(&probe);
    assert!(probe.native_text_seen && probe.text_input_token.is_some());
    assert!(
        !resize_before_key || probe.native_resize_seen,
        "the resized branch must receive the actual native 720x520 resize: {probe:?}"
    );
    assert!(
        matches!(
            probe.text_input_admission,
            Some(eframe::NativeOutputInputAdmission::Defer(_))
        ),
        "backpressure must defer before taking the real Text event: {probe:?}"
    );
    assert!(
        !probe.text_reached_retained_ui,
        "the Text event must stay buffered until pane rendering resumes: {probe:?}"
    );
    assert!(probe.released && probe.resumed_presented && probe.done);
    assert!(probe.buffered.is_empty());
}

struct ExpandingRosterTextApp {
    app: NativeDockspaceApp<EditorPanes>,
    host: Arc<DelayedTerminalHost>,
    calls: Arc<AtomicUsize>,
    expanded: bool,
    recovered_passes: usize,
    started: Instant,
}

impl eframe::App for ExpandingRosterTextApp {
    fn ui(&mut self, ui: &mut egui::Ui, _: &mut eframe::Frame) {
        use dockspace::geometry::{LogicalSize, PhysicalPoint, PhysicalRect, ScaleFactor};
        use dockspace::model::SurfaceId;
        use dockspace::runtime::NativeSurfacePlacement;

        assert!(
            self.started.elapsed() < Duration::from_secs(15),
            "the expanding roster must recover: {:?}",
            lock(&self.host.probe)
        );
        let token = eframe::current_native_output_token().unwrap();
        let text_input = ui.input(|input| {
            input
                .events
                .iter()
                .any(|event| matches!(event, egui::Event::Text(text) if text == "a"))
        });
        if text_input && !self.expanded {
            let probe = lock(&self.host.probe);
            assert!(probe.native_text_seen && probe.holding && !probe.released);
            assert_eq!(probe.buffered.len(), 1, "only one real output is withheld");
            assert_eq!(probe.text_input_token, Some(token));
            assert!(matches!(
                probe.text_input_admission,
                Some(eframe::NativeOutputInputAdmission::Proceed)
            ));
            assert!(!self.app.is_surface_presented(SurfaceId::new(2)));
            self.expanded = true;
        }
        let before = self.calls.load(Ordering::SeqCst);
        let scale = f64::from(ui.ctx().pixels_per_point());
        let placements = [1, 2].map(|id| {
            let x = if id == 1 { 0.0 } else { 320.0 };
            NativeSurfacePlacement::new(
                SurfaceId::new(id),
                LogicalSize::new(300.0, 440.0).unwrap(),
                PhysicalPoint::new(x * scale, 0.0).unwrap(),
                ScaleFactor::new(scale).unwrap(),
                PhysicalRect::new(x * scale, 0.0, 300.0 * scale, 440.0 * scale).unwrap(),
            )
        });
        let roster = if self.expanded {
            &placements[..]
        } else {
            &placements[..1]
        };
        self.app.show_root_surfaces(ui, roster);
        self.app.with_errors(|primary, cleanup| {
            assert!(
                primary.is_none() && cleanup.is_empty(),
                "roster expansion must not enter shutdown: primary={primary:?}; cleanup={cleanup:?}"
            );
        });
        let after = self.calls.load(Ordering::SeqCst);
        let retained = self.app.current_output_is_retained();
        let mut probe = lock(&self.host.probe);
        if probe.holding {
            if retained && !probe.retained.contains(&token) {
                probe.retained.push(token);
            }
            if text_input {
                probe.text_reached_retained_ui |= retained;
                // The input admission must happen while only A is committed.
                // Release after this same App run requests the larger roster.
                probe.release_on_service = true;
            }
        } else if probe.released {
            if after > before && !retained && probe.resumed.is_none() {
                probe.resumed = Some(token);
            }
            if probe.resumed_presented
                && !retained
                && self.app.is_surface_presented(SURFACE)
                && self.app.is_surface_presented(SurfaceId::new(2))
            {
                self.recovered_passes += 1;
                if self.recovered_passes == 3 {
                    probe.done = true;
                    ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                }
            }
        } else if probe.hold_token.is_none()
            && self.app.is_surface_presented(SURFACE)
            && after > before
            && !retained
            && self
                .app
                .with_panes(|panes| ui.memory(|memory| memory.focused()) == Some(panes.target))
        {
            probe.hold_token = Some(token);
            probe.calls_at_hold = after;
        }
        if probe.hold_token.is_none() || probe.released {
            ui.ctx().request_repaint_after(Duration::from_millis(5));
        }
    }

    fn on_exit(&mut self, _: Option<&eframe::glow::Context>) {
        lock(&self.host.probe).final_text = Some(self.app.with_panes(|panes| panes.text.clone()));
    }
}

#[test]
#[ignore = "requires isolated X11/XTest renderer, an EWMH WM, and xdotool"]
fn real_eframe_output_capacity_preserves_text_when_the_root_roster_expands_after_admission() {
    use dockspace::model::{
        DockspaceLayout, DockspaceNode, DockspaceRootLayout, DockspaceSurfaceLayout, ItemId,
        RootId, SurfaceId,
    };

    assert!(std::env::var_os("DISPLAY").is_some());
    let mut session = dockspace::runtime::DockspaceSession::from_layout(
        DockspaceLayout::new([1, 2].map(|id| {
            DockspaceSurfaceLayout::new(
                SurfaceId::new(id),
                DockspaceRootLayout::new(
                    RootId::new(id),
                    DockspaceNode::central_tabs([ItemId::new(id)]),
                ),
            )
        }))
        .unwrap(),
        dockspace::policy::DockPolicy::default(),
    )
    .unwrap();
    session
        .configure_output_capacity(NonZeroUsize::new(2).unwrap())
        .unwrap();
    let calls = Arc::new(AtomicUsize::new(0));
    let app = NativeDockspaceApp::new_with_close_policy(
        egui::Id::unique("real-expanding-roster-text"),
        session,
        SURFACE,
        EditorPanes {
            text: String::new(),
            editor_item: ItemId::new(1),
            target: egui::Id::unique("real-expanding-roster-editor"),
            calls: Arc::clone(&calls),
        },
        DockStyle::default(),
        NativeWindowClosePolicy::RetainLayout,
    )
    .unwrap();
    let probe = Arc::new(Mutex::new(Probe {
        text_mode: true,
        expand_roster_on_text: true,
        ..Probe::default()
    }));
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
        "Dockspace roster expansion text regression",
        options,
        Box::new(move |_| {
            Ok(Box::new(ExpandingRosterTextApp {
                app,
                host,
                calls,
                expanded: false,
                recovered_passes: 0,
                started: Instant::now(),
            }))
        }),
    )
    .unwrap();
    let probe = lock(&probe);
    assert!(probe.native_text_seen && probe.text_input_token.is_some());
    assert!(matches!(
        probe.text_input_admission,
        Some(eframe::NativeOutputInputAdmission::Proceed)
    ));
    assert!(
        probe.released && probe.resumed_presented && probe.done,
        "{probe:?}"
    );
    assert!(probe.buffered.is_empty());
    assert_eq!(
        probe.final_text.as_deref(),
        Some("a"),
        "the admitted native Text event must survive expansion and reach the editor exactly once: {probe:?}"
    );
}

struct SwitchingRootEntryTextApp {
    app: NativeDockspaceApp<EditorPanes>,
    host: Arc<DelayedTerminalHost>,
    calls: Arc<AtomicUsize>,
    key_sent: bool,
    switched: bool,
    recovered_passes: usize,
    started: Instant,
}

impl eframe::App for SwitchingRootEntryTextApp {
    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        use dockspace::geometry::{LogicalSize, PhysicalPoint, PhysicalRect, ScaleFactor};
        use dockspace::model::SurfaceId;
        use dockspace::runtime::NativeSurfacePlacement;

        assert!(
            self.started.elapsed() < Duration::from_secs(15),
            "the ordinary Root entry must preserve admitted sibling input: {:?}",
            lock(&self.host.probe)
        );
        let token = eframe::current_native_output_token().unwrap();
        let text_input = ui.input(|input| {
            input
                .events
                .iter()
                .any(|event| matches!(event, egui::Event::Text(text) if text == "a"))
        });
        if text_input && !self.switched {
            let probe = lock(&self.host.probe);
            assert!(self.key_sent && probe.native_text_seen);
            assert!(!probe.holding && probe.buffered.is_empty());
            assert_eq!(probe.text_input_token, Some(token));
            assert!(matches!(
                probe.text_input_admission,
                Some(eframe::NativeOutputInputAdmission::Proceed)
            ));
            self.switched = true;
        }
        let before = self.calls.load(Ordering::SeqCst);
        if self.switched {
            eframe::App::ui(&mut self.app, ui, frame);
        } else {
            let scale = f64::from(ui.ctx().pixels_per_point());
            let placements = [1, 2].map(|id| {
                let x = if id == 1 { 0.0 } else { 320.0 };
                NativeSurfacePlacement::new(
                    SurfaceId::new(id),
                    LogicalSize::new(300.0, 440.0).unwrap(),
                    PhysicalPoint::new(x * scale, 0.0).unwrap(),
                    ScaleFactor::new(scale).unwrap(),
                    PhysicalRect::new(x * scale, 0.0, 300.0 * scale, 440.0 * scale).unwrap(),
                )
            });
            self.app.show_root_surfaces(ui, &placements);
        }
        self.app.with_errors(|primary, cleanup| {
            assert!(
                primary.is_none() && cleanup.is_empty(),
                "switching the Root entry must not enter shutdown: primary={primary:?}; cleanup={cleanup:?}"
            );
        });
        let after = self.calls.load(Ordering::SeqCst);
        let retained = self.app.current_output_is_retained();
        let focused = ui.memory(|memory| memory.focused());
        let editor_focused = self.app.with_panes(|panes| focused == Some(panes.target));
        let mut probe = lock(&self.host.probe);
        if self.switched {
            if text_input && probe.resumed.is_none() {
                assert!(!retained, "the fully prepaid roster can paint this input");
                probe.resumed = Some(token);
            }
            if probe.resumed_presented && !retained && self.app.is_surface_presented(SURFACE) {
                self.recovered_passes += 1;
                if self.recovered_passes == 3 {
                    probe.done = true;
                    ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                }
            }
        } else if !self.key_sent
            && editor_focused
            && after > before
            && !retained
            && self.app.is_surface_presented(SURFACE)
            && self.app.is_surface_presented(SurfaceId::new(2))
        {
            self.key_sent = true;
            drop(probe);
            let window = u64::from(token.window_id()).to_string();
            let result = Command::new("xdotool")
                .args(["windowactivate", "--sync", &window, "key", "a"])
                .output()
                .expect("the X11 ordinary Root entry regression requires xdotool");
            assert!(result.status.success(), "{result:?}");
        }
        ui.ctx().request_repaint_after(Duration::from_millis(5));
    }

    fn on_exit(&mut self, _: Option<&eframe::glow::Context>) {
        lock(&self.host.probe).final_text = Some(self.app.with_panes(|panes| panes.text.clone()));
    }
}

#[test]
#[ignore = "requires isolated X11/XTest renderer, an EWMH WM, and xdotool"]
fn real_eframe_output_capacity_preserves_text_when_shared_root_switches_to_the_ordinary_entry() {
    use dockspace::model::{
        DockspaceLayout, DockspaceNode, DockspaceRootLayout, DockspaceSurfaceLayout, ItemId,
        RootId, SurfaceId,
    };

    assert!(std::env::var_os("DISPLAY").is_some());
    let mut session = dockspace::runtime::DockspaceSession::from_layout(
        DockspaceLayout::new([1, 2].map(|id| {
            DockspaceSurfaceLayout::new(
                SurfaceId::new(id),
                DockspaceRootLayout::new(
                    RootId::new(id),
                    DockspaceNode::central_tabs([ItemId::new(id)]),
                ),
            )
        }))
        .unwrap(),
        dockspace::policy::DockPolicy::default(),
    )
    .unwrap();
    session
        .configure_output_capacity(NonZeroUsize::new(3).unwrap())
        .unwrap();
    let calls = Arc::new(AtomicUsize::new(0));
    let app = NativeDockspaceApp::new_with_close_policy(
        egui::Id::unique("real-ordinary-root-text"),
        session,
        SURFACE,
        EditorPanes {
            text: String::new(),
            editor_item: ItemId::new(2),
            target: egui::Id::unique("real-ordinary-root-sibling-editor"),
            calls: Arc::clone(&calls),
        },
        DockStyle::default(),
        NativeWindowClosePolicy::RetainLayout,
    )
    .unwrap();
    let probe = Arc::new(Mutex::new(Probe {
        text_mode: true,
        ordinary_entry_on_text: true,
        ..Probe::default()
    }));
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
        "Dockspace ordinary Root entry text regression",
        options,
        Box::new(move |_| {
            Ok(Box::new(SwitchingRootEntryTextApp {
                app,
                host,
                calls,
                key_sent: false,
                switched: false,
                recovered_passes: 0,
                started: Instant::now(),
            }))
        }),
    )
    .unwrap();
    let probe = lock(&probe);
    assert!(probe.native_text_seen && probe.text_input_token.is_some());
    assert!(matches!(
        probe.text_input_admission,
        Some(eframe::NativeOutputInputAdmission::Proceed)
    ));
    assert!(probe.resumed_presented && probe.done, "{probe:?}");
    assert!(!probe.holding && probe.buffered.is_empty());
    assert_eq!(
        probe.final_text.as_deref(),
        Some("a"),
        "the ordinary Root entry must deliver the admitted sibling Text event exactly once: {probe:?}"
    );
}

struct BackpressureApp {
    app: NativeDockspaceApp<CountingPanes>,
    host: Arc<DelayedTerminalHost>,
    calls: Arc<AtomicUsize>,
    started: Instant,
    style_queued: bool,
    style_applied: bool,
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
            if !self.style_queued {
                self.app
                    .request_style(DockStyle {
                        tab_bar_height: 44.0,
                        ..DockStyle::default()
                    })
                    .unwrap();
                self.style_queued = true;
            }
            assert!(
                self.app.take_action_status().is_none(),
                "retained output cannot publish the queued style"
            );
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
            if let Some(result) = self.app.take_action_status() {
                assert_eq!(
                    result.unwrap(),
                    crate::NativeActionStatus::StyleChanged {
                        changed: true,
                        geometry_changed: true
                    }
                );
                self.style_applied = true;
            }
            if after > before && !retained && probe.resumed.is_none() {
                assert!(after > probe.calls_at_hold);
                probe.resumed = Some(token);
            }
            if probe.resumed_presented
                && self.style_applied
                && self.app.is_surface_presented(SURFACE)
            {
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

    fn on_exit(&mut self, _: Option<&eframe::glow::Context>) {
        assert!(self.style_queued && self.style_applied);
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
                style_queued: false,
                style_applied: false,
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
