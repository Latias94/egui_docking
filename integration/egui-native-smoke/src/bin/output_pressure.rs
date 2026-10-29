//! Real eframe output identities under deliberately delayed terminal delivery.
//!
//! The bounded test proxy owns its own result buffer, outside runtime storage.
//! This does not model 256 simultaneously queued GPU frames.

use std::collections::VecDeque;
use std::error::Error;
use std::io;
use std::num::NonZeroUsize;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use dockspace::geometry::{LogicalRect, PhysicalRect};
use dockspace::model::{
    DockAnchor, DockPlacement, DockspaceActionOutcome, DockspaceContainedLayout, DockspaceLayout,
    DockspaceNode, DockspaceRootLayout, DockspaceSurfaceLayout, FloatingPresentationId, ItemId,
    NativeWindowPlacement, RootId, SurfaceId,
};
use dockspace::policy::DockPolicy;
use dockspace::runtime::{
    DockspaceSession, NativeInputStorageLimit, NativeProducerStorageLimit, NativeReportStorageLimit,
};
use eframe::egui::{self, ViewportCommand, ViewportId};
use egui_dockspace::{DockStyle, DockspaceActionStatus, PaneView};
use egui_dockspace_native::{
    NativeDockspaceApp, NativeRuntimeErrorKind, NativeStorageLimit, NativeWindowClosePolicy,
};

#[path = "output_pressure/host.rs"]
mod host;

const ROOT_SURFACE: SurfaceId = SurfaceId::new(1);
const MAIN_ROOT: RootId = RootId::new(1);
const CHILD_ROOT: RootId = RootId::new(2);
const MAIN_ITEM: ItemId = ItemId::new(1);
const CHILD_ITEM: ItemId = ItemId::new(2);
const ORDINARY_OUTPUTS: usize = 256;
const BUFFERED_TERMINALS: usize = ORDINARY_OUTPUTS + 2;
const BYTE_LIMIT: usize = 4 * 1024 * 1024;

type Runtime = Arc<Mutex<NativeDockspaceApp<Panes>>>;
type Shared = Arc<Mutex<PressureState>>;

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Scenario {
    Ordered,
    Duplicate,
    Reordered,
    LateStaging,
}

impl Scenario {
    fn from_args() -> Result<Self, io::Error> {
        let args: Vec<_> = std::env::args().skip(1).collect();
        match args.as_slice() {
            [] => Ok(Self::Ordered),
            [value] if value == "ordered" => Ok(Self::Ordered),
            [value] if value == "duplicate" => Ok(Self::Duplicate),
            [value] if value == "reordered" => Ok(Self::Reordered),
            [value] if value == "late-staging" => Ok(Self::LateStaging),
            _ => Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "expected one output-pressure scenario: ordered, duplicate, reordered, late-staging",
            )),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    Bootstrap,
    AwaitChild,
    AwaitStaging,
    Filling,
    Draining,
    AwaitRedock,
    AwaitRetirement,
    AwaitFinalBoundary,
    Done,
}

struct PressureState {
    scenario: Scenario,
    phase: Phase,
    buffered: VecDeque<eframe::NativeOutputResult>,
    child_viewport: Option<ViewportId>,
    child_surface: Option<SurfaceId>,
    child_paused: bool,
    root_roster_has_child: bool,
    root_control_seen: bool,
    protected_child_seen: bool,
    root_control: Option<eframe::NativeOutputToken>,
    protected_child: Option<eframe::NativeOutputToken>,
    root_terminal_seen: bool,
    protected_terminal_seen: bool,
    full_root_defer_seen: bool,
    peak_ordinary: usize,
    peak_control: usize,
    peak_resident: usize,
    peak_reserved: usize,
    forwarded: usize,
    final_terminal_checked: bool,
    late_terminal_after_fatal: bool,
    outcome: Option<Result<(), String>>,
}

impl PressureState {
    fn new(scenario: Scenario) -> Self {
        Self {
            scenario,
            phase: Phase::Bootstrap,
            buffered: VecDeque::with_capacity(BUFFERED_TERMINALS),
            child_viewport: None,
            child_surface: None,
            child_paused: false,
            root_roster_has_child: false,
            root_control_seen: false,
            protected_child_seen: false,
            root_control: None,
            protected_child: None,
            root_terminal_seen: false,
            protected_terminal_seen: false,
            full_root_defer_seen: false,
            peak_ordinary: 0,
            peak_control: 0,
            peak_resident: 0,
            peak_reserved: 0,
            forwarded: 0,
            final_terminal_checked: false,
            late_terminal_after_fatal: false,
            outcome: None,
        }
    }

    fn finish(&mut self, context: &egui::Context, result: Result<(), String>) {
        if self.outcome.is_none() {
            eprintln!(
                "output-pressure: {:?}: {:?}; ordinary_peak={}, control_peak={}, \
                 resident_peak={}, reserved_peak={}, limit={BYTE_LIMIT}, forwarded={}, \
                 proxy_backing={} bytes",
                self.scenario,
                result,
                self.peak_ordinary,
                self.peak_control,
                self.peak_resident,
                self.peak_reserved,
                self.forwarded,
                self.buffered.capacity() * size_of::<eframe::NativeOutputResult>(),
            );
            self.outcome = Some(result);
        }
        self.phase = Phase::Done;
        context.send_viewport_cmd_to(ViewportId::ROOT, ViewportCommand::Close);
    }

    fn requests_external_repaint(&self) -> bool {
        !(self.scenario == Scenario::LateStaging && self.late_terminal_after_fatal)
    }
}

fn storage_limit() -> Result<NativeStorageLimit, egui_dockspace_native::NativeRuntimeError> {
    let slots = |count| NonZeroUsize::new(count).unwrap();
    let input = NativeInputStorageLimit::new(slots(64), 256 * 1024)
        .with_producer_storage_limit(NativeProducerStorageLimit::new(
            slots(2),
            slots(1),
            16 * 1024,
        ))
        .with_report_storage_limit(NativeReportStorageLimit::new(
            slots(258),
            slots(2),
            768 * 1024,
        ));
    NativeStorageLimit::new(input, BYTE_LIMIT, 4, 2, 2)
}

fn main() -> Result<(), Box<dyn Error>> {
    let scenario = Scenario::from_args()?;
    let layout = DockspaceLayout::new([DockspaceSurfaceLayout::new(
        ROOT_SURFACE,
        DockspaceRootLayout::new(MAIN_ROOT, DockspaceNode::central_tabs([MAIN_ITEM])),
    )
    .with_contained(DockspaceContainedLayout::new(
        FloatingPresentationId::new(1),
        DockspaceRootLayout::new(CHILD_ROOT, DockspaceNode::central_tabs([CHILD_ITEM])),
        LogicalRect::new(60.0, 70.0, 360.0, 240.0)?,
    ))])?;
    let mut policy = DockPolicy::default();
    policy.set_allow_native_surfaces(true);
    let session = DockspaceSession::from_layout(layout, policy)?;
    let selected = storage_limit()?;
    eprintln!(
        "output-pressure: {scenario:?}; selected={BYTE_LIMIT}, initial_reserved={}",
        selected.initial_reserved_bytes()
    );
    let runtime = Arc::new(Mutex::new(NativeDockspaceApp::new_with_storage_limit(
        egui::Id::unique("native-output-pressure"),
        session,
        ROOT_SURFACE,
        Panes,
        DockStyle::default(),
        NativeWindowClosePolicy::default(),
        selected,
    )?));
    let shared = Arc::new(Mutex::new(PressureState::new(scenario)));
    let inner = lock(&runtime).native_host_handler();
    let native_host = Arc::new(host::PressureHost {
        inner,
        runtime: Arc::clone(&runtime),
        shared: Arc::clone(&shared),
    });
    let app = PressureApp {
        runtime: Arc::clone(&runtime),
        shared: Arc::clone(&shared),
        deadline: Instant::now() + Duration::from_secs(60),
    };
    let demand_shared = Arc::clone(&shared);
    let demand_runtime = Arc::clone(&runtime);
    eframe::run_native(
        "Dockspace real output pressure",
        eframe::NativeOptions {
            viewport: egui::ViewportBuilder::default().with_inner_size([640.0, 480.0]),
            native_host: Some(native_host),
            ..Default::default()
        },
        Box::new(move |creation| {
            let context = creation.egui_ctx.clone();
            std::thread::Builder::new()
                .name("native-output-pressure-demand".to_owned())
                .spawn(move || {
                    let deadline = Instant::now() + Duration::from_secs(60);
                    loop {
                        let request_repaint = {
                            let state = lock(&demand_shared);
                            if state.phase == Phase::Done {
                                break;
                            }
                            if Instant::now() >= deadline {
                                eprintln!(
                                    "output-pressure: deadline in {:?}, buffered={}, backlog={:?}",
                                    state.phase,
                                    state.buffered.len(),
                                    demand_runtime
                                        .try_lock()
                                        .ok()
                                        .map(|runtime| runtime.output_backlog())
                                );
                                std::process::exit(1);
                            }
                            state.requests_external_repaint()
                        };
                        // Repaint demand is the test workload. A separate clock
                        // also bounds cases where Defer prevents another UI pass.
                        if request_repaint {
                            context.request_repaint_of(ViewportId::ROOT);
                        }
                        std::thread::sleep(Duration::from_millis(5));
                    }
                })?;
            Ok(Box::new(app))
        }),
    )?;
    let mut shared = lock(&shared);
    shared
        .outcome
        .take()
        .unwrap_or_else(|| Err(format!("window exited in {:?}", shared.phase)))
        .map_err(io::Error::other)?;
    assert!(shared.buffered.is_empty());
    assert_eq!(shared.peak_ordinary, ORDINARY_OUTPUTS);
    assert_eq!(shared.peak_control, 2);
    assert!(shared.full_root_defer_seen && shared.protected_child_seen);
    assert!(shared.final_terminal_checked);
    if scenario == Scenario::LateStaging {
        assert!(shared.late_terminal_after_fatal);
        assert_eq!(shared.forwarded, BUFFERED_TERMINALS);
    }
    let backlog = lock(&runtime).output_backlog();
    let runtime = lock(&runtime);
    assert_eq!(backlog.ordinary(), 0, "window exit retains ordinary owners");
    assert_eq!(backlog.control(), 0, "window exit retains control owners");
    if scenario == Scenario::Ordered {
        assert_eq!(
            runtime.error_kind(),
            None,
            "the finalizer failed after success"
        );
        runtime.with_errors(|primary, cleanup| {
            assert!(primary.is_none() && cleanup.is_empty());
        });
    } else {
        runtime.with_errors(|primary, cleanup| {
            assert!(primary.is_some_and(|error| error.is_output_order_violation()));
            assert!(
                cleanup.is_empty(),
                "quarantine reported extra failures: {cleanup:?}"
            );
        });
    }
    Ok(())
}

struct PressureApp {
    runtime: Runtime,
    shared: Shared,
    deadline: Instant,
}

impl eframe::App for PressureApp {
    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        let mut runtime = lock(&self.runtime);
        <NativeDockspaceApp<Panes> as eframe::App>::ui(&mut runtime, ui, frame);
        let mut state = lock(&self.shared);
        if state.phase == Phase::Done {
            return;
        }
        if Instant::now() >= self.deadline {
            let message = format!(
                "timed out in {:?}, buffered={}, backlog={:?}",
                state.phase,
                state.buffered.len(),
                runtime.output_backlog(),
            );
            state.finish(ui.ctx(), Err(message));
            return;
        }
        if let Err(error) = self.drive(&runtime, &mut state, ui.ctx()) {
            state.finish(ui.ctx(), Err(error));
        }
        // Test demand produces actual native outputs; it does not mint identities.
        if state.phase != Phase::Done && state.requests_external_repaint() {
            ui.ctx().request_repaint_after(Duration::from_millis(1));
        }
    }
}

impl PressureApp {
    fn drive(
        &self,
        runtime: &NativeDockspaceApp<Panes>,
        state: &mut PressureState,
        context: &egui::Context,
    ) -> Result<(), String> {
        let resident = runtime
            .input_storage_capacity_bytes()
            .map_err(|error| error.to_string())?;
        let reserved = runtime
            .input_storage_reserved_bytes()
            .ok_or("the bounded runtime lost its reservation")?;
        state.peak_resident = state.peak_resident.max(resident);
        state.peak_reserved = state.peak_reserved.max(reserved);
        if resident > reserved || reserved > BYTE_LIMIT {
            return Err(format!(
                "resident={resident}, reserved={reserved}, limit={BYTE_LIMIT}"
            ));
        }
        if let Some(error) = runtime.error_kind() {
            let (order_error, cleanup_empty) = runtime.with_errors(|primary, cleanup| {
                (
                    primary.is_some_and(|error| error.is_output_order_violation()),
                    cleanup.is_empty(),
                )
            });
            if state.scenario == Scenario::Ordered
                || state.phase != Phase::Draining
                || error != NativeRuntimeErrorKind::HostProtocol
                || !order_error
                || !cleanup_empty
            {
                return Err(format!("unexpected runtime error {error:?}"));
            }
            if runtime.output_backlog().ordinary() == 0 && runtime.is_shutdown_drained() {
                if state.scenario == Scenario::LateStaging {
                    if !state.late_terminal_after_fatal {
                        return Err("shutdown drained before the withheld staging terminal".into());
                    }
                    let source_preserved = runtime.with_view(|view| {
                        view.item(CHILD_ITEM)
                            .is_some_and(|item| item.surface() == ROOT_SURFACE)
                    });
                    if source_preserved != Some(true)
                        || state
                            .child_surface
                            .is_some_and(|surface| runtime.is_surface_presented(surface))
                    {
                        return Err("late staging terminal transferred the preserved source".into());
                    }
                }
                state.finish(context, Ok(()));
            }
            return Ok(());
        }
        if state.scenario != Scenario::Ordered && state.phase == Phase::Draining {
            return Ok(());
        }
        if let Some(status) = runtime.take_action_status() {
            match status {
                Ok(DockspaceActionStatus::Applied(
                    DockspaceActionOutcome::NativeRootTearOffRequested {
                        root,
                        target_surface,
                        ..
                    },
                )) if root == CHILD_ROOT => state.child_surface = Some(target_surface),
                Ok(DockspaceActionStatus::Applied(DockspaceActionOutcome::RootDocked {
                    root,
                    changed: true,
                    ..
                })) if root == CHILD_ROOT && state.phase == Phase::AwaitRedock => {
                    state.phase = Phase::AwaitRetirement;
                }
                status => return Err(format!("unexpected application result {status:?}")),
            }
        }
        match state.phase {
            Phase::Bootstrap if runtime.is_surface_presented(ROOT_SURFACE) => {
                runtime
                    .request_move_root_to_new_window(
                        CHILD_ROOT,
                        NativeWindowPlacement::new(
                            PhysicalRect::new(720.0, 100.0, 380.0, 280.0).unwrap(),
                        ),
                    )
                    .map_err(|error| format!("tear-off request: {error}"))?;
                state.phase = Phase::AwaitChild;
            }
            Phase::Filling if state.root_control_seen => {
                if let Some(child) = state.child_viewport {
                    context.request_repaint_of(child);
                }
            }
            Phase::Draining => {
                let backlog = runtime.output_backlog();
                if let Some(child) = state.child_surface
                    && runtime.is_surface_presented(child)
                    && runtime.is_quiescent()
                    && backlog.ordinary() > 0
                    && backlog.control() == 0
                {
                    runtime
                        .request_dock_root(
                            CHILD_ROOT,
                            DockPlacement::Center(DockAnchor::Item(MAIN_ITEM)),
                        )
                        .map_err(|error| format!("redock after recovered admission: {error}"))?;
                    state.phase = Phase::AwaitRedock;
                    eprintln!("output-pressure: ordinary admission recovered; redocking");
                }
            }
            Phase::AwaitRetirement if runtime.is_quiescent() => {
                let restored = runtime.with_view(|view| {
                    view.item(CHILD_ITEM)
                        .is_some_and(|item| item.surface() == ROOT_SURFACE)
                });
                if restored == Some(true)
                    && state
                        .child_surface
                        .is_some_and(|surface| !runtime.is_surface_presented(surface))
                {
                    // Let the current semantic terminal enter one last root
                    // settlement pass before closing the native event loop.
                    state.phase = Phase::AwaitFinalBoundary;
                    context.request_repaint_of(ViewportId::ROOT);
                }
            }
            Phase::AwaitFinalBoundary
                if runtime.is_quiescent()
                    && runtime.current_output_is_retained()
                    && runtime.output_backlog().ordinary() == 1
                    && runtime.output_backlog().control() == 0 =>
            {
                state.finish(context, Ok(()));
            }
            _ => {}
        }
        Ok(())
    }
}

struct Panes;

impl PaneView for Panes {
    fn title(&self, item: ItemId) -> Option<egui::WidgetText> {
        Some(if item == MAIN_ITEM { "Main" } else { "Child" }.into())
    }

    fn ui(&mut self, item: ItemId, ui: &mut egui::Ui) {
        ui.label(format!("Real output pressure pane {}", item.get()));
    }
}
