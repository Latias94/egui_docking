//! Real close/create/age boundary, using the application's production UI and finalizer.
//!
//! Run alone under an X11 WM with XTest, `xdotool`, and Alt-F4 -> WM_DELETE_WINDOW.
//! This slice does not exercise drag/release or claim exact release-output delivery.
//! The test thread runs eframe; one driver thread supplies physical input. No fork
//! tokens, platform snapshots, close callbacks, or renderer terminals are invented.

use std::num::NonZeroUsize;
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use dockspace::geometry::{LogicalRect, PhysicalRect};
use dockspace::model::{
    DockspaceContainedLayout, DockspaceNode, DockspaceRootLayout, DockspaceSurfaceLayout,
    FloatingPresentationId,
};
use dockspace::policy::DockPolicy;
use dockspace::runtime::{
    NativeInputStorageLimit, NativeProducerStorageLimit, NativeReceiverAnswer,
    NativeReportStorageLimit, SurfaceUnavailableReason,
};
use eframe::{
    NativeOutputAdmission, NativeOutputResult, NativeOutputStatus, NativeViewportCloseRequest,
    NativeViewportCreateAdmission, NativeViewportCreateAttempt, NativeWindowEventDisposition,
    NativeWindowSnapshot,
};
use winit::event::WindowEvent;
use winit::platform::x11::EventLoopBuilderExtX11 as _;
use winit::window::WindowId;

use super::*;
use crate::NativeStorageLimit;
use crate::mailbox::{HostRecord, NativeDispatch};

const ROOT: SurfaceId = SurfaceId::new(1);
const A: RootId = RootId::new(2);
const B: RootId = RootId::new(4);
const TABS: [ItemId; 3] = [ItemId::new(3), ItemId::new(5), ItemId::new(6)];
const LIMIT: usize = 4 * 1024 * 1024;
const TIMEOUT: Duration = Duration::from_secs(20);
const TICK: Duration = Duration::from_millis(5);

struct Panes;

impl PaneView for Panes {
    fn title(&self, item: ItemId) -> Option<egui::WidgetText> {
        Some(format!("Item {item:?}").into())
    }

    fn ui(&mut self, _item: ItemId, ui: &mut egui::Ui) {
        ui.label("Real native boundary fixture");
    }
}

type State = Arc<Mutex<NativeRuntimeState<Panes>>>;
type Shared = Arc<Mutex<Facts>>;

#[derive(Default)]
struct Facts {
    root: Option<(NativeOutputToken, NativeWindowSnapshot)>,
    root_presentations: u64,
    root_motion: u64,
    a: Option<ViewportId>,
    q1: Option<NativeViewportCloseRequest>,
    q2: Option<NativeViewportCloseRequest>,
    cancellation: Option<NativeViewportCloseRequest>,
    cancellation_deliveries: usize,
    arm_b: bool,
    b_attempt: Option<NativeViewportCreateAttempt>,
    b_first: Option<NativeOutputToken>,
    b_forwarded: bool,
    b_created: Option<(NativeOutputToken, u64)>,
    release_b: bool,
    pause_at_root_terminal: bool,
    paused: bool,
    service_only: bool,
    deliver_pending: bool,
    drain_outputs: bool,
    stop_outputs: bool,
    terminal: Option<NativeOutputResult>,
    terminal_deliveries: usize,
    last_terminal_ordinal: u64,
    destroyed: [Option<WindowId>; 2],
}

/// Gates only the named delayed facts; ordinary callbacks go to the real host.
struct BoundaryHost {
    inner: Arc<dyn NativeHostHandler>,
    facts: Shared,
    state: State,
}

impl BoundaryHost {
    fn forward_output(&self, result: NativeOutputResult) -> NativeHostWake {
        {
            let mut facts = lock_state(&self.facts);
            assert!(result.ordinal().get() > facts.last_terminal_ordinal);
            facts.last_terminal_ordinal = result.ordinal().get();
            if result.token().viewport_id() == ViewportId::ROOT
                && result.status() == NativeOutputStatus::Presented
            {
                facts.root_presentations += 1;
            }
        }
        let wake = self.inner.on_output(result);
        if lock_state(&self.facts).drain_outputs {
            NativeHostWake::ServiceAfterCurrent
        } else {
            wake
        }
    }

    fn deliver_retained(&self) {
        let (terminal, cancellation) = {
            let mut facts = lock_state(&self.facts);
            if !facts.deliver_pending {
                return;
            }
            facts.deliver_pending = false;
            (facts.terminal.take(), facts.cancellation.take())
        };
        if let Some(terminal) = terminal {
            self.forward_output(terminal);
            lock_state(&self.facts).terminal_deliveries += 1;
        }
        if let Some(request) = cancellation {
            self.inner.on_viewport_close_cancelled(request);
            lock_state(&self.facts).cancellation_deliveries += 1;
        }
    }
}

impl NativeHostHandler for BoundaryHost {
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
        self.deliver_retained();
        let paused = {
            let facts = lock_state(&self.facts);
            facts.paused && !facts.service_only
        };
        if paused {
            NativeHostWake::Wait
        } else {
            assert!(eframe::current_native_output_token().is_none());
            let wake = self.inner.on_service(context);
            let state = lock_state(&self.state);
            let mut facts = lock_state(&self.facts);
            let backlog = state.output_backlog();
            if facts.drain_outputs
                && state.is_quiescent()
                && backlog.ordinary() == 0
                && backlog.control() == 0
                && backlog.awaiting_staging() == 0
            {
                // Close ordinary UI admission only after the real service
                // commits the final terminal. The driver cannot race an
                // already admitted next Root output between these checks.
                facts.stop_outputs = true;
                NativeHostWake::Wait
            } else {
                wake
            }
        }
    }

    fn before_output_input(
        &self,
        context: &egui::Context,
        token: NativeOutputToken,
        has_input: bool,
    ) -> eframe::NativeOutputInputAdmission {
        self.inner.before_output_input(context, token, has_input)
    }

    fn begin_deferred_viewport_create(
        &self,
        attempt: NativeViewportCreateAttempt,
    ) -> NativeViewportCreateAdmission {
        let admission = self.inner.begin_deferred_viewport_create(attempt);
        if matches!(admission, NativeViewportCreateAdmission::Proceed { .. }) {
            let mut facts = lock_state(&self.facts);
            if facts.arm_b {
                assert!(
                    facts.b_attempt.is_none(),
                    "B has only one actual OS attempt"
                );
                facts.b_attempt = Some(attempt);
                facts.arm_b = false;
            }
        }
        admission
    }

    fn render_hidden_deferred_viewport(&self, viewport: ViewportId) -> bool {
        self.inner.render_hidden_deferred_viewport(viewport)
    }

    fn on_window_event(
        &self,
        event: eframe::NativeWindowEvent<'_>,
    ) -> NativeWindowEventDisposition {
        let close = event.close_request();
        let viewport = event.viewport_id();
        let window = event.window_id();
        let motion = (viewport == Some(ViewportId::ROOT)
            && matches!(event.event(), WindowEvent::CursorMoved { .. }))
        .then(|| event.ordinal().get());
        let destroyed = matches!(event.event(), WindowEvent::Destroyed);
        let disposition = self.inner.on_window_event(event);
        let mut facts = lock_state(&self.facts);
        if let Some(ordinal) = motion {
            facts.root_motion = ordinal;
        }
        if viewport == facts.a
            && let Some(close) = close
        {
            assert_eq!(disposition, NativeWindowEventDisposition::HoldClose(close));
            match facts.q1 {
                None => facts.q1 = Some(close),
                Some(q1) if q1 == close => {}
                Some(_) => match facts.q2 {
                    None => facts.q2 = Some(close),
                    Some(q2) => assert_eq!(q2, close, "there must be exactly two requests"),
                },
            }
        }
        let target_window = facts.q1.is_some_and(|q1| q1.window_id() == window)
            || facts
                .b_first
                .is_some_and(|token| token.window_id() == window);
        if destroyed && target_window {
            assert!(
                !facts.destroyed.contains(&Some(window)),
                "Destroy is unique"
            );
            let slot = facts.destroyed.iter_mut().find(|slot| slot.is_none());
            *slot.expect("only A and the late-created B are destroyed") = Some(window);
        }
        disposition
    }

    fn on_device_removed(&self, removal: eframe::NativeDeviceRemoval) {
        self.inner.on_device_removed(removal);
    }

    fn on_global_focus(&self, observation: eframe::NativeGlobalFocusObservation) -> NativeHostWake {
        let wake = self.inner.on_global_focus(observation);
        if lock_state(&self.facts).service_only {
            // A real focus callback schedules eframe's real no-UI user event.
            NativeHostWake::ServiceAfterCurrent
        } else {
            wake
        }
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

    fn on_viewport_property_command_retired(
        &self,
        retirement: eframe::NativeViewportPropertyCommandRetirement,
    ) -> NativeHostWake {
        self.inner.on_viewport_property_command_retired(retirement)
    }

    fn on_output_begin(
        &self,
        token: NativeOutputToken,
        window: NativeWindowSnapshot,
        roster: Option<eframe::NativeViewportRoster<'_>>,
    ) -> NativeOutputAdmission {
        self.deliver_retained();
        {
            let mut facts = lock_state(&self.facts);
            if token.create_attempt().is_some() && token.create_attempt() == facts.b_attempt {
                facts.b_first.get_or_insert(token);
                if !facts.release_b {
                    // A rejected candidate owns no output obligation. Only the
                    // future, genuinely admitted callback is forwarded below.
                    return NativeOutputAdmission::Defer;
                }
            }
            if facts.paused || facts.stop_outputs {
                return NativeOutputAdmission::Defer;
            }
        }
        if let Some(roster) = roster {
            assert_eq!(roster.backend(), eframe::NativeWindowingBackend::X11);
        }
        let admission = self.inner.on_output_begin(token, window, roster);
        if admission == NativeOutputAdmission::Proceed {
            let mut facts = lock_state(&self.facts);
            if token.viewport_id() == ViewportId::ROOT {
                facts.root = Some((token, window));
            } else if token.create_attempt().is_some() && token.create_attempt() == facts.b_attempt
            {
                facts.b_forwarded = true;
            }
        }
        admission
    }

    fn on_output_pass_finalized(
        &self,
        finalization: NativeOutputPassFinalization<'_>,
    ) -> NativeHostWake {
        let attempt = lock_state(&self.facts).b_attempt;
        if let Some(attempt) = attempt {
            let created = lock_state(&self.state)
                .boundary_coordinator()
                .dispatch_bridge()
                .with_viewports(|routes| {
                    routes
                        .create_ingress()
                        .find_map(|ingress| match ingress.record() {
                            HostRecord::ViewportCreated(created)
                                if created.token().create_attempt() == Some(attempt) =>
                            {
                                Some((created.token(), ingress.ordinal()))
                            }
                            _ => None,
                        })
                });
            if let Some(created) = created {
                let mut facts = lock_state(&self.facts);
                if let Some(previous) = facts.b_created {
                    assert_eq!(
                        previous, created,
                        "multipass must retain the same Created receipt"
                    );
                } else {
                    facts.b_created = Some(created);
                }
            }
        }
        self.inner.on_output_pass_finalized(finalization)
    }

    fn on_output(&self, result: NativeOutputResult) -> NativeHostWake {
        {
            let mut facts = lock_state(&self.facts);
            if result.token().viewport_id() == ViewportId::ROOT
                && facts.pause_at_root_terminal
                && result.status() == NativeOutputStatus::Presented
            {
                assert!(
                    facts.terminal.is_none(),
                    "one bounded real renderer terminal"
                );
                assert_eq!(result.status(), NativeOutputStatus::Presented);
                facts.terminal = Some(result);
                facts.pause_at_root_terminal = false;
                facts.paused = true;
                return NativeHostWake::Wait;
            }
        }
        self.forward_output(result)
    }

    fn on_viewport_create_failed(
        &self,
        failure: eframe::NativeViewportCreateFailure,
    ) -> NativeHostWake {
        self.inner.on_viewport_create_failed(failure)
    }

    fn on_viewport_visibility(
        &self,
        result: eframe::NativeViewportVisibilityResult,
    ) -> NativeHostWake {
        self.inner.on_viewport_visibility(result)
    }

    fn on_viewport_close_cancelled(&self, request: NativeViewportCloseRequest) -> NativeHostWake {
        let mut facts = lock_state(&self.facts);
        assert_eq!(Some(request), facts.q1, "q2 must be allowed, never vetoed");
        assert!(
            facts.cancellation.is_none(),
            "the canonical callback occurs once"
        );
        assert_eq!(facts.cancellation_deliveries, 0);
        facts.cancellation = Some(request);
        NativeHostWake::Wait
    }
}

fn fixture() -> NativeDockspaceApp<Panes> {
    let layout = DockspaceLayout::new([DockspaceSurfaceLayout::new(
        ROOT,
        DockspaceRootLayout::new(RootId::new(1), DockspaceNode::central_tabs(TABS)),
    )
    .with_contained(DockspaceContainedLayout::new(
        FloatingPresentationId::new(2),
        DockspaceRootLayout::new(A, DockspaceNode::tabs([ItemId::new(2)])),
        LogicalRect::new(20.0, 150.0, 280.0, 180.0).unwrap(),
    ))
    .with_contained(DockspaceContainedLayout::new(
        FloatingPresentationId::new(4),
        DockspaceRootLayout::new(B, DockspaceNode::tabs([ItemId::new(4)])),
        LogicalRect::new(340.0, 150.0, 280.0, 180.0).unwrap(),
    ))])
    .unwrap();
    let mut policy = DockPolicy::default();
    policy.set_allow_native_surfaces(true);
    let slots = |n| NonZeroUsize::new(n).unwrap();
    let input = NativeInputStorageLimit::new(slots(512), 768 * 1024)
        .with_producer_storage_limit(NativeProducerStorageLimit::new(
            slots(4),
            slots(1),
            128 * 1024,
        ))
        .with_report_storage_limit(NativeReportStorageLimit::new(
            slots(64),
            slots(2),
            256 * 1024,
        ));
    let storage = NativeStorageLimit::new(input, LIMIT, 4, 2, 2).unwrap();
    assert!(storage.initial_reserved_bytes() <= LIMIT);
    NativeDockspaceApp::new_with_storage_limit(
        Id::unique("native-boundary-x11"),
        DockspaceSession::from_layout(layout, policy).unwrap(),
        ROOT,
        Panes,
        DockStyle::default(),
        NativeWindowClosePolicy::Cancel,
        storage,
    )
    .unwrap()
}

fn wait_until(
    deadline: Instant,
    label: &str,
    mut ready: impl FnMut() -> bool,
) -> Result<(), String> {
    while Instant::now() < deadline {
        if ready() {
            return Ok(());
        }
        thread::sleep(TICK);
    }
    Err(format!("timed out waiting for {label}"))
}

/// No shell and no unbounded `--sync` subprocess; reap even a wedged X server client.
fn xdotool(args: &[&str]) -> Result<String, String> {
    let mut child = Command::new("xdotool")
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("xdotool {args:?}: {error}"))?;
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if Instant::now() < deadline => thread::sleep(TICK),
            result => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("xdotool {args:?} did not finish: {result:?}"));
            }
        }
    }
    let output = child
        .wait_with_output()
        .map_err(|error| error.to_string())?;
    if !output.status.success() {
        return Err(format!(
            "xdotool {args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    String::from_utf8(output.stdout).map_err(|error| error.to_string())
}

fn wm_delete(window: WindowId) -> Result<(), String> {
    // Do not use `windowkill` or a synthetic winit CloseRequested. The isolated
    // WM receives physical XTest keys and sends the client's WM_DELETE_WINDOW.
    xdotool(&[
        "windowactivate",
        "--sync",
        &u64::from(window).to_string(),
        "key",
        "--clearmodifiers",
        "alt+F4",
    ])?;
    Ok(())
}

fn move_root(state: &State, context: &egui::Context, root: RootId, y: f64) {
    lock_state(state)
        .request_move_root_to_new_window(
            root,
            NativeWindowPlacement::new(PhysicalRect::new(880.0, y, 320.0, 240.0).unwrap()),
        )
        .unwrap();
    context.request_repaint_of(ViewportId::ROOT);
}

fn assert_capacity(state: &State) {
    let state = lock_state(state);
    state.with_errors(|error, cleanup| {
        assert!(
            error.is_none(),
            "native runtime: {error:?}; cleanup: {cleanup:?}"
        );
        assert!(cleanup.is_empty(), "cleanup errors: {cleanup:?}");
    });
    let resident = state.owned_input_capacity_bytes().unwrap();
    let reserved = state.input_storage_reserved_bytes().unwrap();
    assert!(
        resident <= reserved && reserved <= LIMIT,
        "resident={resident}, reserved={reserved}"
    );
}

fn source(state: &State) -> (SurfaceId, RootId, FloatingPresentationId, LogicalRect) {
    lock_state(state)
        .with_view(|view| {
            let item = view.item(ItemId::new(4)).unwrap();
            let floating = item.contained().unwrap();
            (
                item.surface(),
                item.root(),
                floating,
                view.contained(floating).unwrap().rect(),
            )
        })
        .unwrap()
}

fn parked(state: &State, request: NativeViewportCloseRequest) -> Option<(u64, Duration)> {
    lock_state(state)
        .boundary_coordinator()
        .dispatch_bridge()
        .with_close_control(|control| {
            control.parked_ingress().find_map(|ingress| {
                (ingress.window_event()?.close_request() == Some(request))
                    .then_some((ingress.ordinal(), ingress.captured_at()))
            })
        })
}

fn drive(state: &State, context: &egui::Context, host: &BoundaryHost) -> Result<(), String> {
    let deadline = Instant::now() + TIMEOUT;
    wait_until(deadline, "presented, quiescent Root", || {
        lock_state(state).is_quiescent()
    })?;
    assert_capacity(state);
    let initial_source = source(state);
    move_root(state, context, A, 100.0);
    let mut a_status = None;
    wait_until(deadline, "A's exact first-live admission", || {
        let mut state = lock_state(state);
        if a_status.is_none() {
            a_status = state.take_action_status();
        }
        let native = state.boundary_coordinator();
        a_status.is_some()
            && native.lifecycle_progress().first_live_admissions() == 1
            && native
                .session()
                .view()
                .item(ItemId::new(2))
                .unwrap()
                .surface()
                != ROOT
            && state.is_quiescent()
    })?;
    let (a_binding, a_window) = {
        let mut state = lock_state(state);
        assert!(a_status.unwrap().is_ok());
        let native = state.boundary_coordinator();
        let surface = native
            .session()
            .view()
            .item(ItemId::new(2))
            .unwrap()
            .surface();
        let viewport = native.surface_viewport(surface).unwrap();
        let binding = native.viewport_binding(viewport).unwrap();
        let window = native
            .dispatch_bridge()
            .with_viewports(|routes| routes.window(viewport))
            .unwrap();
        lock_state(&host.facts).a = Some(viewport);
        (binding, window)
    };
    eprintln!("native-boundary: A first-live; starting the admitted, delayed B create");
    lock_state(&host.facts).arm_b = true;
    move_root(state, context, B, 380.0);
    wait_until(
        deadline,
        "B's real create attempt and deferred first output",
        || lock_state(&host.facts).b_first.is_some(),
    )?;
    let (b_attempt, b_first) = {
        let facts = lock_state(&host.facts);
        (facts.b_attempt.unwrap(), facts.b_first.unwrap())
    };
    let bridge = lock_state(state).boundary_coordinator().dispatch_bridge();
    let b_binding = bridge.create_binding(b_attempt.viewport_id()).unwrap();
    assert_eq!(b_first.create_attempt(), Some(b_attempt));
    assert!(bridge.with_viewports(|routes| {
        routes.has_admitted_create_attempt(b_attempt.viewport_id(), b_binding)
            && routes.binding_for_create_attempt(b_attempt) == Some(b_binding)
    }));
    {
        let mut state = lock_state(state);
        let native = state.boundary_coordinator();
        let mut frame = native
            .begin_host_frame(|_| NativeReceiverAnswer::Unknown)
            .unwrap();
        assert!(frame.frame.cancel_next_native_create().unwrap());
        frame
            .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
            .unwrap();
        let mut report = frame.commit().unwrap();
        native.settle_host_frame_report(&report).unwrap();
        native
            .accept_native_effects(context, report.take_native_effects())
            .unwrap();
        assert!(!native.session().has_pending_native_create());
    }
    assert_eq!(source(state), initial_source);
    assert!(bridge.with_viewports(|routes| {
        routes.has_admitted_create_attempt(b_attempt.viewport_id(), b_binding)
            && routes.binding_for_create_attempt(b_attempt) == Some(b_binding)
    }));
    let root_before = lock_state(&host.facts).root_presentations;
    context.request_repaint_of(ViewportId::ROOT);
    wait_until(deadline, "fresh Root after semantic B cancellation", || {
        lock_state(&host.facts).root_presentations > root_before
            && lock_state(state).is_surface_presented(ROOT)
    })?;
    assert_capacity(state);

    wm_delete(a_window)?;
    wait_until(deadline, "q1's real canonical Veto callback", || {
        lock_state(&host.facts).cancellation.is_some()
    })?;
    let q1 = lock_state(&host.facts).q1.unwrap();
    assert_eq!(lock_state(&host.facts).cancellation, Some(q1));
    wm_delete(a_window)?;
    wait_until(deadline, "a distinct held q2", || {
        lock_state(&host.facts).q2.is_some()
    })?;
    let q2 = lock_state(&host.facts).q2.unwrap();
    assert_ne!(q1, q2);
    assert!(q1.event() < q2.event());
    let q2_capture = parked(state, q2).expect("q2 must be parked behind q1's genuine callback");
    assert_eq!(lock_state(state).close_progress().requests_retained(), 2);
    assert_eq!(
        lock_state(state).close_progress().cancellations_committed(),
        0
    );
    eprintln!("native-boundary: distinct q1/q2, q1 callback withheld, q2 parked at {q2_capture:?}");

    lock_state(&host.facts).pause_at_root_terminal = true;
    context.request_repaint_of(ViewportId::ROOT);
    wait_until(deadline, "one real retained Root renderer terminal", || {
        lock_state(&host.facts).paused
    })?;
    let terminal = lock_state(&host.facts).terminal.unwrap();
    assert!(bridge.has_pending_output(terminal.token()));
    let (root_token, root_window) = lock_state(&host.facts).root.unwrap();
    let rect = root_window
        .inner_rect()
        .expect("X11 supplies actual client geometry");
    // Two nearby physical motions supply ingress even after a repeated run
    // leaves the pointer at the previous target. No
    // window bounds or pointer event is synthesized for the native host.
    let captured = bridge.captured_through_for_test();
    let motion_before = lock_state(&host.facts).root_motion;
    assert!(
        bridge.permits_unscheduled_input(),
        "ordinary ingress must start unfrozen"
    );
    xdotool(&[
        "mousemove",
        &(rect.x() + 9).to_string(),
        &(rect.y() + 100).to_string(),
        "mousemove",
        &(rect.x() + 10).to_string(),
        &(rect.y() + 100).to_string(),
    ])?;
    wait_until(
        deadline,
        "XTest motion captured by the real Root host",
        || {
            lock_state(&host.facts).root_motion > motion_before
                && bridge.captured_through_for_test() > captured
        },
    )?;
    assert_eq!(root_token.viewport_id(), ViewportId::ROOT);
    let age_deadline = Instant::now() + Duration::from_millis(275);
    wait_until(
        deadline,
        "the existing ingress naturally reaching 250 ms",
        || Instant::now() >= age_deadline,
    )?;
    let (_, age) = bridge.input_backlog_for_test();
    assert!(age.unwrap() >= Duration::from_millis(250));
    let accepted_before_freeze = bridge.captured_through_for_test();
    // Freezing first requires a committed core/prelude boundary. Replay and
    // cancellation follow that barrier through the normal host service.
    assert_eq!(
        bridge.admission_dispatch(),
        NativeDispatch::ContinueBoundary
    );
    assert!(q2_capture.0 <= accepted_before_freeze);
    assert!(!bridge.permits_unscheduled_input());
    assert_eq!(parked(state, q2), Some(q2_capture));
    assert_capacity(state);

    // Exercise the real no-UI finalizer while all three independent owners
    // remain outstanding. `permits_unscheduled_input` changes only after the
    // exact overload cancellation commits, not merely after a poll or timeout.
    lock_state(&host.facts).service_only = true;
    // At least one real focus transition occurs even with focus-follows-mouse.
    // All host callbacks, including delayed delivery, stay on the GUI thread.
    xdotool(&[
        "windowactivate",
        "--sync",
        &u64::from(a_window).to_string(),
        "windowactivate",
        "--sync",
        &u64::from(root_token.window_id()).to_string(),
    ])?;
    wait_until(
        deadline,
        "overload cancellation independent of parked owners",
        || bridge.permits_unscheduled_input(),
    )?;
    assert_eq!(parked(state, q2), Some(q2_capture));
    assert!(bridge.has_pending_output(terminal.token()));
    assert!(bridge.with_viewports(|routes| {
        routes.has_admitted_create_attempt(b_attempt.viewport_id(), b_binding)
            && routes.binding_for_create_attempt(b_attempt) == Some(b_binding)
    }));
    assert!(!lock_state(&host.facts).b_forwarded);
    assert_eq!(lock_state(&host.facts).cancellation_deliveries, 0);
    assert_eq!(source(state), initial_source);
    eprintln!(
        "native-boundary: natural age freeze includes prefix through {accepted_before_freeze}; real service cancelled overload with owners still parked"
    );

    *lock_state(state).boundary_close_policy() = NativeWindowClosePolicy::RetainLayout;
    {
        let mut facts = lock_state(&host.facts);
        facts.deliver_pending = true;
        facts.service_only = false;
        facts.release_b = true;
        facts.paused = false;
    }
    context.request_repaint_of(b_attempt.viewport_id());
    context.request_repaint_of(ViewportId::ROOT);
    wait_until(
        deadline,
        "q2 Allow/Destroyed, B Created/cleanup, and quiescence",
        || {
            // Consume the cancelled application's exact terminal result, rather
            // than leaving its single action-result owner occupied forever.
            let mut state = lock_state(state);
            if let Some(status) = state.take_action_status() {
                assert!(status.is_ok(), "B action terminal: {status:?}");
            }
            state.is_quiescent()
                && lock_state(&host.facts)
                    .destroyed
                    .iter()
                    .all(Option::is_some)
        },
    )?;
    lock_state(&host.facts).drain_outputs = true;
    context.request_repaint_of(ViewportId::ROOT);
    let drained = wait_until(deadline, "zero native output owners", || {
        lock_state(&host.facts).stop_outputs
    });
    if let Err(error) = drained {
        let state = lock_state(state);
        let facts = lock_state(&host.facts);
        return Err(format!(
            "{error}: backlog={:?}, quiescent={}, root_presentations={}, created={:?}",
            state.output_backlog(),
            state.is_quiescent(),
            facts.root_presentations,
            facts.b_created,
        ));
    }
    let backlog = lock_state(state).output_backlog();
    assert_eq!(backlog.ordinary(), 0);
    assert_eq!(backlog.control(), 0);
    assert_eq!(backlog.awaiting_staging(), 0);
    assert_capacity(state);
    assert_eq!(source(state), initial_source);
    {
        let mut state = lock_state(state);
        let native = state.boundary_coordinator();
        assert!(!native.session().recognizes_native_binding(a_binding));
        assert!(!native.session().recognizes_native_binding(b_binding));
        assert_eq!(native.lifecycle_progress().first_live_admissions(), 1);
        assert_eq!(native.lifecycle_progress().destroyed_observations(), 2);
        assert_eq!(native.lifecycle_progress().quiescent_retirements(), 2);
        assert_eq!(native.close_progress().cancellations_committed(), 1);
        assert_eq!(
            native
                .session()
                .view()
                .item(TABS[0])
                .unwrap()
                .tabs()
                .items(),
            TABS
        );
        assert!(state.is_quiescent());
    }
    let facts = lock_state(&host.facts);
    assert!(
        facts.b_forwarded,
        "B's real first output must eventually reach the host"
    );
    let (created, _) = facts
        .b_created
        .expect("production UI must capture B's real Created receipt");
    assert_eq!(created.create_attempt(), Some(b_attempt));
    assert_eq!(created.window_id(), b_first.window_id());
    assert!(
        facts.destroyed.contains(&Some(a_window)),
        "q2's exact window was destroyed"
    );
    assert!(
        facts.destroyed.contains(&Some(b_first.window_id())),
        "B's actual window was cleaned up"
    );
    assert_eq!(facts.cancellation_deliveries, 1);
    assert_eq!(facts.terminal_deliveries, 1);
    eprintln!(
        "native-boundary: exact A/B Destroyed, source unchanged, quiescent, output zero, <=4 MiB"
    );
    Ok(())
}

#[test]
#[ignore = "requires isolated X11/XTest renderer, an EWMH WM with Alt-F4, and xdotool"]
fn real_native_close_create_age_boundary() {
    assert!(
        std::env::var_os("DISPLAY").is_some(),
        "requires an isolated X11 display"
    );
    xdotool(&["version"]).expect("requires xdotool with XTest");
    let app = fixture();
    let state = Arc::clone(&app.state);
    let facts = Arc::new(Mutex::new(Facts::default()));
    let host = Arc::new(BoundaryHost {
        inner: app.native_host_handler(),
        facts,
        state: Arc::clone(&state),
    });
    let driver = Arc::new(Mutex::new(None));
    let driver_slot = Arc::clone(&driver);
    let driver_host = Arc::clone(&host);
    let driver_state = Arc::clone(&state);
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([800.0, 600.0])
            .with_position([40.0, 40.0]),
        native_host: Some(host),
        renderer: eframe::Renderer::Glow,
        event_loop_builder: Some(Box::new(|builder| {
            builder.with_x11().with_any_thread(true);
        })),
        ..Default::default()
    };
    let result = eframe::run_native(
        "Dockspace real native boundary",
        options,
        Box::new(move |creation| {
            let context = creation.egui_ctx.clone();
            *lock_state(&driver_slot) = Some(
                thread::Builder::new()
                    .name("native-boundary-x11-driver".into())
                    .spawn(move || {
                        let outcome =
                            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                                drive(&driver_state, &context, &driver_host)
                            }));
                        // Every success/error/panic opens the gates, returns real owned
                        // callbacks, and lets the production host clean up before Root
                        // closes. A failed assertion is preserved in `outcome` below.
                        {
                            let mut facts = lock_state(&driver_host.facts);
                            facts.paused = false;
                            facts.service_only = false;
                            facts.deliver_pending = true;
                            facts.drain_outputs = false;
                            facts.stop_outputs = false;
                            facts.pause_at_root_terminal = false;
                            facts.release_b = true;
                        }
                        *lock_state(&driver_state).boundary_close_policy() =
                            NativeWindowClosePolicy::RetainLayout;
                        context.request_repaint_of(ViewportId::ROOT);
                        let cleanup = wait_until(
                            Instant::now() + Duration::from_secs(3),
                            "cleanup before Root exit",
                            || {
                                let mut state = lock_state(&driver_state);
                                let _ = state.take_action_status();
                                state.is_quiescent() || state.is_shutdown_drained()
                            },
                        );
                        context
                            .send_viewport_cmd_to(ViewportId::ROOT, egui::ViewportCommand::Close);
                        context.request_repaint_of(ViewportId::ROOT);
                        (outcome, cleanup)
                    })
                    .unwrap(),
            );
            Ok(Box::new(app))
        }),
    );
    let handle = lock_state(&driver)
        .take()
        .expect("eframe must start the driver");
    let (outcome, cleanup) = handle.join().expect("the driver cleanup must not panic");
    result.expect("the real eframe renderer must exit cleanly");
    if let Err(error) = &cleanup {
        eprintln!("native-boundary cleanup: {error}");
    }
    match outcome {
        Ok(result) => {
            result.unwrap_or_else(|error| panic!("{error}"));
            cleanup.unwrap_or_else(|error| panic!("{error}"));
        }
        Err(payload) => std::panic::resume_unwind(payload),
    }
}
