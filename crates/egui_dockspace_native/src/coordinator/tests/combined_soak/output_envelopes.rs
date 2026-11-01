//! Real opaque output envelopes in the existing finite lifecycle owner model.
//!
//! Only output identities and terminal callbacks come from the renderer. Pointer,
//! close, create and focus facts remain model inputs; no physical journey or
//! simultaneous GPU frame count is claimed by this fixture.

use std::num::NonZeroUsize;
use std::process::{Child, Command};
use std::sync::{Mutex, mpsc};
use std::time::{Duration, Instant};

use eframe::{NativeHostHandler, NativeHostWake, NativeOutputAdmission, NativeOutputResult};
use winit::platform::x11::EventLoopBuilderExtX11;

use super::*;
use crate::mailbox::{NativeHostBridge, OutputAbandonment};

const OUTPUTS: usize = 256;
const TIMEOUT: Duration = Duration::from_secs(30);
const CHILD_PROCESS: &str = "DOCKSPACE_MIXED_OUTPUT_TEST_CHILD";

struct RendererProcess(Child);

impl Drop for RendererProcess {
    fn drop(&mut self) {
        if !matches!(self.0.try_wait(), Ok(Some(_))) {
            let _ = self.0.kill();
        }
        let _ = self.0.wait();
    }
}

fn run_in_isolated_process() -> bool {
    if std::env::var_os(CHILD_PROCESS).as_deref() == Some(std::ffi::OsStr::new("1")) {
        return false;
    }
    // A renderer-thread/channel failure cannot leave native windows or bridge
    // owners in the process which runs the remaining test suite. The guard also
    // reaps the child on timeout or assertion unwind.
    let module = module_path!().split_once("::").unwrap().1;
    let test = format!(
        "{module}::full_output_capacity_coexists_with_close_create_focus_and_exact_release_retry"
    );
    let mut child = RendererProcess(
        Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                &test,
                "--ignored",
                "--nocapture",
                "--test-threads=1",
            ])
            .env(CHILD_PROCESS, "1")
            .spawn()
            .unwrap(),
    );
    let deadline = Instant::now() + Duration::from_mins(1);
    loop {
        if let Some(status) = child.0.try_wait().unwrap() {
            assert!(status.success(), "isolated renderer test failed: {status}");
            return true;
        }
        assert!(
            Instant::now() < deadline,
            "isolated renderer test exceeded its cleanup deadline"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

struct Envelopes {
    bridge: Option<Arc<NativeHostBridge>>,
    terminals: Vec<NativeOutputResult>,
    shutdown_output: Option<NativeOutputToken>,
    shutdown_terminal: Option<NativeOutputResult>,
}

struct Host {
    state: Arc<Mutex<Envelopes>>,
    window: mpsc::SyncSender<WindowId>,
    bridge: Mutex<mpsc::Receiver<Arc<NativeHostBridge>>>,
}

impl NativeHostHandler for Host {
    fn on_output_begin(
        &self,
        token: NativeOutputToken,
        window: eframe::NativeWindowSnapshot,
        _roster: Option<eframe::NativeViewportRoster<'_>>,
    ) -> NativeOutputAdmission {
        let mut state = self.state.lock().unwrap();
        if state.terminals.len() == OUTPUTS {
            // Close is an input boundary on the following pass. One inline
            // fixture owner admits only that shutdown pass outside the runtime
            // under test, without freezing its input ahead of the model's B.
            assert!(state.shutdown_output.replace(token).is_none());
            return NativeOutputAdmission::Proceed;
        }
        if state.bridge.is_none() {
            self.window.send(token.window_id()).unwrap();
            state.bridge = Some(self.bridge.lock().unwrap().recv_timeout(TIMEOUT).unwrap());
            eprintln!("mixed-output: renderer received the existing model bridge");
        }
        let bridge = state.bridge.as_ref().unwrap();
        assert_eq!(token.viewport_id(), ViewportId::ROOT);
        assert_eq!(
            bridge.on_output_begin(token, window, None),
            NativeOutputAdmission::Proceed
        );
        NativeOutputAdmission::Proceed
    }

    fn on_output_pass_finalized(
        &self,
        finalization: eframe::NativeOutputPassFinalization<'_>,
    ) -> NativeHostWake {
        let (_, token, _, _, _) = finalization.into_parts();
        // Match the existing retained-framebuffer path: keep the exact output
        // reservation until its renderer terminal, with no new affine paint.
        let state = self.state.lock().unwrap();
        if state.shutdown_output == Some(token) {
            return NativeHostWake::Wait;
        }
        assert!(matches!(
            state
                .bridge
                .as_ref()
                .unwrap()
                .settle_output_abandonment(token, false, None),
            OutputAbandonment::NewlyAbandoned
        ));
        NativeHostWake::Wait
    }

    fn on_output(&self, terminal: NativeOutputResult) -> NativeHostWake {
        let mut state = self.state.lock().unwrap();
        if state.shutdown_output == Some(terminal.token()) {
            assert_eq!(terminal.ordinal().get(), OUTPUTS as u64 + 1);
            assert!(state.shutdown_terminal.replace(terminal).is_none());
            return NativeHostWake::Wait;
        }
        assert!(state.terminals.len() < OUTPUTS);
        assert_eq!(
            usize::try_from(terminal.ordinal().get()).unwrap(),
            state.terminals.len() + 1
        );
        state.terminals.push(terminal);
        if state.terminals.len() == 1 || state.terminals.len().is_multiple_of(64) {
            eprintln!(
                "mixed-output: retained original terminals={}",
                state.terminals.len()
            );
        }
        NativeHostWake::Wait
    }
}

struct App(Arc<Mutex<Envelopes>>);

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let state = self.0.lock().unwrap();
        assert!(state.terminals.len() <= OUTPUTS);
        if state.terminals.len() == OUTPUTS {
            assert!(state.shutdown_output.is_some());
        } else if state.terminals.len() + 1 == OUTPUTS {
            // The following fixture-only pass consumes this Close input.
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
        } else {
            ui.ctx().request_repaint();
        }
    }
}

#[test]
#[ignore = "requires X11 and a renderer to mint original eframe output identities"]
fn full_output_capacity_coexists_with_close_create_focus_and_exact_release_retry() {
    if run_in_isolated_process() {
        return;
    }
    let state = Arc::new(Mutex::new(Envelopes {
        bridge: None,
        terminals: Vec::with_capacity(OUTPUTS),
        shutdown_output: None,
        shutdown_terminal: None,
    }));
    let renderer_state = Arc::clone(&state);
    let (window_tx, window_rx) = mpsc::sync_channel(1);
    let (bridge_tx, bridge_rx) = mpsc::sync_channel(1);
    let (completed_tx, completed_rx) = mpsc::sync_channel(1);
    let renderer = std::thread::spawn(move || {
        let result = eframe::run_native(
            "Dockspace mixed output owners",
            eframe::NativeOptions {
                viewport: egui::ViewportBuilder::default().with_inner_size([160.0, 120.0]),
                native_host: Some(Arc::new(Host {
                    state: Arc::clone(&renderer_state),
                    window: window_tx,
                    bridge: Mutex::new(bridge_rx),
                })),
                event_loop_builder: Some(Box::new(|builder| {
                    builder.with_any_thread(true).with_x11();
                })),
                ..Default::default()
            },
            Box::new(move |creation| {
                creation
                    .egui_ctx
                    .options_mut(|options| options.max_passes = NonZeroUsize::MIN);
                Ok(Box::new(App(renderer_state)))
            }),
        )
        .map_err(|error| error.to_string());
        let _ = completed_tx.send(result);
    });
    let window = window_rx.recv_timeout(TIMEOUT).unwrap();
    eprintln!("mixed-output: exact renderer window={window:?}");
    assert_close_create_and_output_pressure(true, true, Some(window), move |native, peaks| {
        eprintln!("mixed-output: close/create/focus/release owners enrolled; filling O256");
        bridge_tx.send(Arc::clone(&native.bridge)).unwrap();
        completed_rx.recv_timeout(TIMEOUT).unwrap().unwrap();
        renderer.join().unwrap();
        let mut state = state.lock().unwrap();
        assert_eq!(state.terminals.len(), OUTPUTS);
        assert_eq!(
            state.shutdown_terminal.unwrap().token(),
            state.shutdown_output.unwrap()
        );
        assert_eq!(native.output_backlog().ordinary(), OUTPUTS);
        assert_eq!(native.output_backlog().control(), 0);
        peaks.observe(native, 0);
        std::mem::take(&mut state.terminals)
    });
}
