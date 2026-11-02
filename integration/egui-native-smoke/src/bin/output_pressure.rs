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
    NativeActionStatus, NativeDockspaceApp, NativeRuntimeErrorKind, NativeStorageLimit,
    NativeWindowClosePolicy,
};

#[path = "output_pressure/combined.rs"]
mod combined;
#[path = "output_pressure/host.rs"]
mod host;
#[path = "output_pressure/service.rs"]
mod service;

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
    LateFirstLive,
    Combined,
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
            [value] if value == "late-first-live" => Ok(Self::LateFirstLive),
            [value] if value == "combined" => Ok(Self::Combined),
            _ => Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "expected one output-pressure scenario: ordered, duplicate, reordered, late-staging, late-first-live, combined",
            )),
        }
    }

    fn expects_success(self) -> bool {
        matches!(self, Self::Ordered | Self::Combined)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    Bootstrap,
    AwaitChild,
    AwaitStaging,
    AwaitFirstLive,
    Filling,
    Draining,
    AwaitRedock,
    AwaitRetirement,
    RetireSatellites,
    AwaitFinalBoundary,
    Done,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct SourceOwnership {
    surface: SurfaceId,
    root: RootId,
    floating: FloatingPresentationId,
    rect: LogicalRect,
}

impl SourceOwnership {
    fn read(runtime: &NativeDockspaceApp<Panes>) -> Option<Self> {
        runtime
            .with_view(|view| {
                let item = view.item(CHILD_ITEM)?;
                let floating = item.contained()?;
                let contained = view.contained(floating)?;
                (contained.root()?.id() == item.root()).then_some(Self {
                    surface: item.surface(),
                    root: item.root(),
                    floating,
                    rect: contained.rect(),
                })
            })
            .flatten()
    }
}

struct PressureState {
    scenario: Scenario,
    phase: Phase,
    buffered: VecDeque<eframe::NativeOutputResult>,
    child_viewport: Option<ViewportId>,
    child_surface: Option<SurfaceId>,
    source_ownership: Option<SourceOwnership>,
    child_content_output: Option<eframe::NativeOutputToken>,
    withheld_first_live: Option<eframe::NativeOutputResult>,
    settled_first_live: Option<eframe::NativeOutputResult>,
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
    previous_terminal: Option<eframe::NativeOutputResult>,
    outcome: Option<Result<(), String>>,
    combined: Option<combined::CombinedPressure>,
    service: service::ServiceMeasurements,
    service_repeat_requested: bool,
}

impl PressureState {
    fn new(scenario: Scenario) -> Self {
        Self {
            scenario,
            phase: Phase::Bootstrap,
            buffered: VecDeque::with_capacity(BUFFERED_TERMINALS),
            child_viewport: None,
            child_surface: None,
            source_ownership: None,
            child_content_output: None,
            withheld_first_live: None,
            settled_first_live: None,
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
            previous_terminal: None,
            outcome: None,
            combined: (scenario == Scenario::Combined).then(combined::CombinedPressure::new),
            service: service::ServiceMeasurements::new(),
            service_repeat_requested: false,
        }
    }

    fn finish(&mut self, context: &egui::Context, result: Result<(), String>) {
        if self.outcome.is_none() {
            eprintln!(
                "output-pressure: {:?}: {:?}; ordinary_peak={}, control_peak={}, \
                 resident_peak={}, reserved_peak={}, limit={BYTE_LIMIT}, forwarded={}, \
                 proxy_backing={} bytes, combined_fixture={} bytes",
                self.scenario,
                result,
                self.peak_ordinary,
                self.peak_control,
                self.peak_resident,
                self.peak_reserved,
                self.forwarded,
                self.buffered.capacity() * size_of::<eframe::NativeOutputResult>(),
                if self.combined.is_some() {
                    size_of::<combined::CombinedPressure>()
                } else {
                    0
                },
            );
            self.outcome = Some(result);
        }
        self.phase = Phase::Done;
        context.send_viewport_cmd_to(ViewportId::ROOT, ViewportCommand::Close);
    }

    fn requests_external_repaint(&self) -> bool {
        !(matches!(
            self.scenario,
            Scenario::LateStaging | Scenario::LateFirstLive
        ) && self.late_terminal_after_fatal)
    }

    fn check_source_ownership(&self, runtime: &NativeDockspaceApp<Panes>) -> Result<(), String> {
        let source = self
            .source_ownership
            .ok_or("the source ownership boundary was not captured")?;
        if SourceOwnership::read(runtime) != Some(source) {
            return Err(
                "pending child changed the source surface, root or contained bounds".into(),
            );
        }
        if let Some(child) = self.child_surface
            && (runtime.with_view(|view| view.surface(child).is_none()) != Some(true)
                || runtime.is_surface_presented(child))
        {
            return Err("the withheld child output published ownership or presentation".into());
        }
        Ok(())
    }
}

fn storage_limit(
    scenario: Scenario,
) -> Result<NativeStorageLimit, egui_dockspace_native::NativeRuntimeError> {
    let slots = |count| NonZeroUsize::new(count).unwrap();
    let combined = scenario == Scenario::Combined;
    let input = NativeInputStorageLimit::new(slots(64), 256 * 1024)
        .with_producer_storage_limit(NativeProducerStorageLimit::new(
            slots(if combined { 16 } else { 2 }),
            slots(if combined { 8 } else { 1 }),
            if combined { 128 * 1024 } else { 16 * 1024 },
        ))
        .with_report_storage_limit(NativeReportStorageLimit::new(
            slots(258),
            slots(if combined { 16 } else { 2 }),
            1024 * 1024,
        ));
    if combined {
        NativeStorageLimit::new(input, BYTE_LIMIT, 48, 16)
    } else {
        NativeStorageLimit::new(input, BYTE_LIMIT, 4, 2)
    }
}

fn main() -> Result<(), Box<dyn Error>> {
    let scenario = Scenario::from_args()?;
    let mut surface = DockspaceSurfaceLayout::new(
        ROOT_SURFACE,
        DockspaceRootLayout::new(MAIN_ROOT, DockspaceNode::central_tabs([MAIN_ITEM])),
    )
    .with_contained(DockspaceContainedLayout::new(
        FloatingPresentationId::new(1),
        DockspaceRootLayout::new(CHILD_ROOT, DockspaceNode::central_tabs([CHILD_ITEM])),
        LogicalRect::new(60.0, 70.0, 360.0, 240.0)?,
    ));
    if scenario == Scenario::Combined {
        surface = combined::with_satellites(surface)?;
    }
    let layout = DockspaceLayout::new([surface])?;
    let mut policy = DockPolicy::default();
    policy.set_allow_native_surfaces(true);
    let session = DockspaceSession::from_layout(layout, policy)?;
    let selected = storage_limit(scenario)?;
    eprintln!(
        "output-pressure: {scenario:?}; selected={BYTE_LIMIT}, initial_reserved={}",
        selected.initial_reserved_bytes()
    );
    let shared = Arc::new(Mutex::new(PressureState::new(scenario)));
    let runtime = Arc::new(Mutex::new(NativeDockspaceApp::new_with_storage_limit(
        egui::Id::unique("native-output-pressure"),
        session,
        ROOT_SURFACE,
        Panes(Arc::clone(&shared)),
        DockStyle::default(),
        NativeWindowClosePolicy::default(),
        selected,
    )?));
    let inner = lock(&runtime).native_host_handler();
    let native_host = Arc::new(host::PressureHost {
        inner,
        runtime: Arc::clone(&runtime),
        shared: Arc::clone(&shared),
    });
    let app = PressureApp {
        runtime: Arc::clone(&runtime),
        shared: Arc::clone(&shared),
        deadline: Instant::now() + Duration::from_secs(120),
    };
    let demand_shared = Arc::clone(&shared);
    let demand_runtime = Arc::clone(&runtime);
    let pass_shared = Arc::clone(&shared);
    eframe::run_native(
        "Dockspace real output pressure",
        eframe::NativeOptions {
            viewport: egui::ViewportBuilder::default().with_inner_size([640.0, 480.0]),
            native_host: Some(native_host),
            ..Default::default()
        },
        Box::new(move |creation| {
            let context = creation.egui_ctx.clone();
            if scenario == Scenario::Combined {
                context.options_mut(|options| options.max_passes = 4.try_into().unwrap());
            }
            if scenario == Scenario::LateFirstLive {
                context.on_begin_pass(
                    "output-pressure child content",
                    Arc::new(move |ui| {
                        if ui.ctx().viewport_id() != ViewportId::ROOT {
                            // A discarded pass cannot prove content in its replacement pass.
                            lock(&pass_shared).child_content_output = None;
                        }
                    }),
                );
            }
            std::thread::Builder::new()
                .name("native-output-pressure-demand".to_owned())
                .spawn(move || {
                    let deadline = Instant::now() + Duration::from_secs(120);
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
    if scenario != Scenario::LateFirstLive {
        assert_eq!(shared.peak_ordinary, ORDINARY_OUTPUTS);
        assert_eq!(shared.peak_control, 2);
        assert!(shared.full_root_defer_seen && shared.protected_child_seen);
    }
    assert!(shared.final_terminal_checked);
    shared.service.report();
    if scenario == Scenario::Combined {
        assert!(
            shared.service.has_repeated_output(),
            "combined service measurement must cover a real repeated output pass"
        );
    }
    if scenario == Scenario::LateStaging {
        assert!(shared.late_terminal_after_fatal);
        assert_eq!(shared.forwarded, BUFFERED_TERMINALS);
    }
    if scenario == Scenario::LateFirstLive {
        assert!(shared.late_terminal_after_fatal);
        assert_eq!(shared.forwarded, 1);
        assert!(shared.withheld_first_live.is_some());
        assert_eq!(shared.settled_first_live, shared.withheld_first_live);
    }
    let backlog = lock(&runtime).output_backlog();
    let runtime = lock(&runtime);
    assert_eq!(backlog.ordinary(), 0, "window exit retains ordinary owners");
    assert_eq!(backlog.control(), 0, "window exit retains control owners");
    if matches!(scenario, Scenario::LateStaging | Scenario::LateFirstLive) {
        shared
            .check_source_ownership(&runtime)
            .map_err(io::Error::other)?;
        assert_eq!(backlog.protected_staging(), 0);
        assert_eq!(backlog.awaiting_staging(), 0);
        // The final root terminal checks shutdown quiescence before event-loop
        // exit. Destroying the external root can then enqueue a new OS fact,
        // after the last core boundary and without retaining an output owner.
    }
    if let Some(combined) = &shared.combined {
        combined
            .verify_finished(&runtime)
            .map_err(io::Error::other)?;
    }
    if scenario.expects_success() {
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
        {
            let mut state = lock(&self.shared);
            if state.scenario == Scenario::Combined && !state.service_repeat_requested {
                state.service_repeat_requested = true;
                ui.ctx()
                    .request_discard("native service measurement repeats one real output");
            }
        }
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
        if matches!(
            state.scenario,
            Scenario::LateStaging | Scenario::LateFirstLive
        ) && state.source_ownership.is_some()
        {
            state.check_source_ownership(runtime)?;
        }
        if let Some(error) = runtime.error_kind() {
            let (order_error, cleanup_empty) = runtime.with_errors(|primary, cleanup| {
                (
                    primary.is_some_and(|error| error.is_output_order_violation()),
                    cleanup.is_empty(),
                )
            });
            if state.scenario.expects_success()
                || state.phase != Phase::Draining
                || error != NativeRuntimeErrorKind::HostProtocol
                || !order_error
                || !cleanup_empty
            {
                return Err(runtime.with_errors(|primary, cleanup| {
                    format!("unexpected runtime error {error:?}: primary={primary:?}, cleanup={cleanup:?}")
                }));
            }
            if runtime.output_backlog().ordinary() == 0 && runtime.is_shutdown_drained() {
                if matches!(
                    state.scenario,
                    Scenario::LateStaging | Scenario::LateFirstLive
                ) {
                    if !state.late_terminal_after_fatal {
                        return Err("shutdown drained before the withheld terminal".into());
                    }
                    state.check_source_ownership(runtime)?;
                }
                state.finish(context, Ok(()));
            }
            return Ok(());
        }
        if !state.scenario.expects_success() && state.phase == Phase::Draining {
            return Ok(());
        }
        if let Some(status) = runtime.take_action_status() {
            match status {
                Ok(NativeActionStatus::Dock(DockspaceActionStatus::Applied(ref outcome)))
                    if state
                        .combined
                        .as_mut()
                        .is_some_and(|combined| combined.on_action(outcome)) => {}
                Ok(NativeActionStatus::Dock(DockspaceActionStatus::Applied(
                    DockspaceActionOutcome::NativeRootTearOffRequested {
                        root,
                        target_surface,
                        ..
                    },
                ))) if root == CHILD_ROOT => state.child_surface = Some(target_surface),
                Ok(NativeActionStatus::Dock(DockspaceActionStatus::Applied(
                    DockspaceActionOutcome::RootDocked {
                        root,
                        changed: true,
                        ..
                    },
                ))) if root == CHILD_ROOT && state.phase == Phase::AwaitRedock => {
                    state.phase = Phase::AwaitRetirement;
                }
                status => return Err(format!("unexpected application result {status:?}")),
            }
        }
        match state.phase {
            Phase::Bootstrap if runtime.is_surface_presented(ROOT_SURFACE) => {
                if let Some(combined) = &mut state.combined
                    && !combined.bootstrap(runtime)?
                {
                    return Ok(());
                }
                let source = SourceOwnership::read(runtime)
                    .ok_or("the child source is not a contained root")?;
                if source.surface != ROOT_SURFACE || source.root != CHILD_ROOT {
                    return Err("the child source has unexpected surface or root ownership".into());
                }
                state.source_ownership = Some(source);
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
                        .map_err(|error| format!("redock after output pressure: {error}"))?;
                    state.phase = Phase::AwaitRedock;
                    eprintln!("output-pressure: ordinary output owners drained; redocking");
                }
            }
            Phase::AwaitRetirement if runtime.is_quiescent() => {
                let redocked = runtime.with_view(|view| {
                    view.item(CHILD_ITEM)
                        .is_some_and(|item| item.surface() == ROOT_SURFACE)
                });
                if redocked == Some(true)
                    && state
                        .child_surface
                        .is_some_and(|surface| !runtime.is_surface_presented(surface))
                {
                    if let Some(combined) = &state.combined {
                        if !combined.child_destroyed {
                            return Ok(());
                        }
                        state.phase = Phase::RetireSatellites;
                        return Ok(());
                    }
                    // Let the current semantic terminal enter one last root
                    // settlement pass before closing the native event loop.
                    state.phase = Phase::AwaitFinalBoundary;
                    context.request_repaint_of(ViewportId::ROOT);
                }
            }
            Phase::RetireSatellites => {
                if state.combined.as_mut().unwrap().redock(runtime)? {
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
                if state
                    .combined
                    .as_ref()
                    .is_none_or(|combined| combined.final_roster_checked)
                {
                    state.finish(context, Ok(()));
                }
            }
            _ => {}
        }
        Ok(())
    }
}

struct Panes(Shared);

impl PaneView for Panes {
    fn title(&self, item: ItemId) -> Option<egui::WidgetText> {
        Some(if item == MAIN_ITEM { "Main" } else { "Child" }.into())
    }

    fn ui(&mut self, item: ItemId, ui: &mut egui::Ui) {
        ui.label(format!("Real output pressure pane {}", item.get()));
        if let Some(token) = eframe::current_native_output_token()
            && token.viewport_id() != ViewportId::ROOT
            && let Some(combined) = &mut lock(&self.0).combined
        {
            combined.observe_pane(item, token);
        }
        if item == CHILD_ITEM
            && let Some(token) = eframe::current_native_output_token()
            && token.viewport_id() != ViewportId::ROOT
        {
            let mut state = lock(&self.0);
            if state.scenario == Scenario::LateFirstLive && state.phase == Phase::AwaitFirstLive {
                assert_eq!(Some(token.viewport_id()), state.child_viewport);
                assert_eq!(token.viewport_id(), ui.ctx().viewport_id());
                assert!(ui.is_enabled(), "first-live child content must be enabled");
                state.child_content_output = Some(token);
            }
        }
    }
}
