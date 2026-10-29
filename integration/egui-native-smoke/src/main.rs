//! One-shot X11/Glow smoke for the real native coordinator.

use std::collections::BTreeMap;
use std::env;
use std::error::Error;
use std::io;
use std::num::NonZeroUsize;
use std::sync::{Arc, Mutex, PoisonError};
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
    NativeActionRequestError, NativeCloseProgress, NativeDockspaceApp, NativeStorageLimit,
    NativeWindowClosePolicy,
};

const ROOT_SURFACE: SurfaceId = SurfaceId::new(1);
const MAIN_ROOT: RootId = RootId::new(1);
const CHILD_ROOT: RootId = RootId::new(2);
const CHILD_FLOATING: FloatingPresentationId = FloatingPresentationId::new(1);
const MAIN_ITEM: ItemId = ItemId::new(1);
const CHILD_ITEM: ItemId = ItemId::new(2);
const CHILD_DETAIL: ItemId = ItemId::new(3);
const SCENARIO_ENV: &str = "EGUI_DOCKSPACE_NATIVE_SMOKE_SCENARIO";
const STORAGE_ENV: &str = "EGUI_DOCKSPACE_NATIVE_SMOKE_STORAGE";
const STORAGE_BYTE_LIMIT: usize = 4 * 1024 * 1024;
const CLOSE_READY_TITLE: &str = "Dockspace native close ready";

fn main() -> Result<(), Box<dyn Error>> {
    let scenario = configured_scenario()?;
    let storage_profile = configured_storage_profile()?;
    eprintln!(
        "native-smoke: scenario {scenario:?}; storage profile {}",
        storage_profile.name()
    );
    let mut policy = DockPolicy::default();
    policy.set_allow_native_surfaces(true);
    let session = DockspaceSession::from_layout(smoke_layout(), policy)?;
    let dockspace = new_dockspace(scenario, storage_profile, session)?;
    let native_host = dockspace.native_host_handler();
    let outcome = Arc::new(Mutex::new(None));
    let reported_outcome = Arc::clone(&outcome);
    let smoke = SmokeApp {
        dockspace,
        scenario,
        storage_profile,
        phase: SmokePhase::AwaitRootPresentation,
        deadline: Instant::now() + Duration::from_secs(30),
        outcome: reported_outcome,
        input_capacity_high_water: 0,
        input_reserved_high_water: 0,
    };
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([720.0, 520.0]),
        native_host: Some(native_host),
        ..Default::default()
    };

    eframe::run_native(
        "egui_dockspace native smoke",
        options,
        Box::new(move |_| Ok(Box::new(smoke))),
    )?;

    match outcome
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .take()
    {
        Some(Ok(())) => Ok(()),
        Some(Err(message)) => Err(io::Error::other(message).into()),
        None => {
            Err(io::Error::other("native smoke exited before reaching a terminal phase").into())
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum SmokeStorageProfile {
    #[default]
    Unlimited,
    Bounded,
}

impl SmokeStorageProfile {
    const fn name(self) -> &'static str {
        match self {
            Self::Unlimited => "unlimited",
            Self::Bounded => "bounded",
        }
    }
}

fn select_storage_profile(environment: Option<&str>) -> Result<SmokeStorageProfile, String> {
    match environment {
        None | Some("unlimited") => Ok(SmokeStorageProfile::Unlimited),
        Some("bounded") => Ok(SmokeStorageProfile::Bounded),
        Some(value) => Err(format!(
            "unknown {STORAGE_ENV} value {value:?}; expected bounded or unlimited"
        )),
    }
}

fn configured_storage_profile() -> Result<SmokeStorageProfile, io::Error> {
    let environment = match env::var(STORAGE_ENV) {
        Ok(value) => Some(value),
        Err(env::VarError::NotPresent) => None,
        Err(env::VarError::NotUnicode(_)) => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("{STORAGE_ENV} must contain Unicode text"),
            ));
        }
    };
    select_storage_profile(environment.as_deref())
        .map_err(|message| io::Error::new(io::ErrorKind::InvalidInput, message))
}

fn bounded_storage_limit() -> Result<NativeStorageLimit, egui_dockspace_native::NativeRuntimeError>
{
    let slots = |count| NonZeroUsize::new(count).expect("smoke storage bounds are nonzero");
    let input = NativeInputStorageLimit::new(slots(512), 768 * 1024)
        .with_producer_storage_limit(NativeProducerStorageLimit::new(
            slots(16),
            slots(8),
            128 * 1024,
        ))
        .with_report_storage_limit(NativeReportStorageLimit::new(
            slots(64),
            slots(16),
            256 * 1024,
        ));
    NativeStorageLimit::new(input, STORAGE_BYTE_LIMIT, 48, 32, 16)
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum SmokeScenario {
    #[default]
    Redock,
    RecoverClose,
    RetainClose,
    CancelClose,
}

impl SmokeScenario {
    fn parse(value: &str) -> Result<Self, String> {
        match value {
            "redock" => Ok(Self::Redock),
            "recover-close" => Ok(Self::RecoverClose),
            "retain-close" => Ok(Self::RetainClose),
            "cancel-close" => Ok(Self::CancelClose),
            value => Err(format!(
                "unknown native smoke scenario {value:?}; expected redock, recover-close, retain-close, or cancel-close"
            )),
        }
    }

    const fn explicit_close_policy(self) -> Option<NativeWindowClosePolicy> {
        match self {
            Self::RecoverClose => None,
            Self::CancelClose => Some(NativeWindowClosePolicy::Cancel),
            Self::Redock | Self::RetainClose => Some(NativeWindowClosePolicy::RetainLayout),
        }
    }
}

fn configured_scenario() -> Result<SmokeScenario, io::Error> {
    let args = env::args().skip(1).collect::<Vec<_>>();
    let environment = match env::var(SCENARIO_ENV) {
        Ok(value) => Some(value),
        Err(env::VarError::NotPresent) => None,
        Err(env::VarError::NotUnicode(_)) => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("{SCENARIO_ENV} must contain Unicode text"),
            ));
        }
    };
    select_scenario(&args, environment.as_deref())
        .map_err(|message| io::Error::new(io::ErrorKind::InvalidInput, message))
}

fn select_scenario(args: &[String], environment: Option<&str>) -> Result<SmokeScenario, String> {
    let mut selected = None;
    let mut index = 0;
    while let Some(argument) = args.get(index) {
        let value = if let Some(value) = argument.strip_prefix("--scenario=") {
            value
        } else if argument == "--scenario" {
            let value = args
                .get(index + 1)
                .ok_or_else(|| "--scenario requires a value".to_owned())?;
            index += 1;
            value
        } else {
            return Err(format!("unknown native smoke argument {argument:?}"));
        };
        if selected.replace(SmokeScenario::parse(value)?).is_some() {
            return Err("--scenario may be specified only once".to_owned());
        }
        index += 1;
    }
    selected.map_or_else(
        || environment.map_or(Ok(SmokeScenario::default()), SmokeScenario::parse),
        Ok,
    )
}

fn new_dockspace(
    scenario: SmokeScenario,
    storage_profile: SmokeStorageProfile,
    session: DockspaceSession,
) -> Result<NativeDockspaceApp<SmokePanes>, egui_dockspace_native::NativeRuntimeError> {
    let instance = egui::Id::unique("egui-native-smoke");
    let panes = SmokePanes::new();
    let style = DockStyle::default();
    if storage_profile == SmokeStorageProfile::Bounded {
        return NativeDockspaceApp::new_with_storage_limit(
            instance,
            session,
            ROOT_SURFACE,
            panes,
            style,
            scenario.explicit_close_policy().unwrap_or_default(),
            bounded_storage_limit()?,
        );
    }
    match scenario.explicit_close_policy() {
        Some(close_policy) => NativeDockspaceApp::new_with_close_policy(
            instance,
            session,
            ROOT_SURFACE,
            panes,
            style,
            close_policy,
        ),
        None => NativeDockspaceApp::new(instance, session, ROOT_SURFACE, panes, style),
    }
}

#[derive(Clone, Copy, Debug)]
enum SmokePhase {
    AwaitRootPresentation,
    AwaitTearOff,
    AwaitFirstLive {
        target_surface: SurfaceId,
    },
    AwaitChildClose {
        target_surface: SurfaceId,
    },
    AwaitCloseSettlement {
        target_surface: SurfaceId,
        child_viewport: ViewportId,
        close_baseline: NativeCloseProgress,
    },
    AwaitRedock {
        target_surface: SurfaceId,
    },
    AwaitRetirement {
        target_surface: SurfaceId,
    },
    Done,
}

struct SmokeApp {
    dockspace: NativeDockspaceApp<SmokePanes>,
    scenario: SmokeScenario,
    storage_profile: SmokeStorageProfile,
    phase: SmokePhase,
    deadline: Instant,
    outcome: Arc<Mutex<Option<Result<(), String>>>>,
    input_capacity_high_water: usize,
    input_reserved_high_water: usize,
}

impl eframe::App for SmokeApp {
    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        <NativeDockspaceApp<SmokePanes> as eframe::App>::ui(&mut self.dockspace, ui, frame);
        // Native callbacks drive progress; this delayed repaint only bounds a missing wake.
        self.drive(ui.ctx());
        if !matches!(self.phase, SmokePhase::Done) {
            ui.ctx()
                .request_repaint_after(self.deadline.saturating_duration_since(Instant::now()));
        }
    }
}

impl SmokeApp {
    fn drive(&mut self, context: &egui::Context) {
        if matches!(self.phase, SmokePhase::Done) {
            return;
        }
        match self.dockspace.input_storage_capacity_bytes() {
            Ok(bytes) => {
                self.input_capacity_high_water = self.input_capacity_high_water.max(bytes);
                match self.storage_profile {
                    SmokeStorageProfile::Bounded => {
                        let Some(reserved) = self.dockspace.input_storage_reserved_bytes() else {
                            self.finish(
                                context,
                                Err("bounded storage profile has no native reservation".to_owned()),
                            );
                            return;
                        };
                        self.input_reserved_high_water =
                            self.input_reserved_high_water.max(reserved);
                        if bytes > reserved || reserved > STORAGE_BYTE_LIMIT {
                            self.finish(context, Err(format!(
                                "bounded storage at a root boundary violated resident <= reserved <= {STORAGE_BYTE_LIMIT}: resident={bytes}, reserved={reserved} bytes"
                            )));
                            return;
                        }
                    }
                    SmokeStorageProfile::Unlimited if bytes > STORAGE_BYTE_LIMIT => {
                        self.finish(context, Err(format!(
                            "unlimited storage root-boundary sample exceeded the 4 MiB sampling threshold: resident={bytes} bytes; no storage limit is selected"
                        )));
                        return;
                    }
                    SmokeStorageProfile::Unlimited => {}
                }
            }
            Err(error) => {
                self.finish(
                    context,
                    Err(format!("native input capacity query failed: {error}")),
                );
                return;
            }
        }
        if let Some(kind) = self.dockspace.error_kind() {
            self.finish(
                context,
                Err(format!("native runtime entered fatal state: {kind:?}")),
            );
            return;
        }
        if Instant::now() >= self.deadline {
            self.finish(
                context,
                Err(format!("native smoke timed out in phase {:?}", self.phase)),
            );
            return;
        }

        match self.phase {
            SmokePhase::AwaitRootPresentation => {
                if self.dockspace.is_surface_presented(ROOT_SURFACE) {
                    self.queue_tear_off(context);
                }
            }
            SmokePhase::AwaitTearOff => self.await_tear_off(context),
            SmokePhase::AwaitFirstLive { target_surface } => {
                self.await_first_live(context, target_surface)
            }
            SmokePhase::AwaitChildClose { target_surface } => {
                self.queue_child_close(context, target_surface)
            }
            SmokePhase::AwaitCloseSettlement {
                target_surface,
                child_viewport,
                close_baseline,
            } => {
                self.await_close_settlement(context, target_surface, child_viewport, close_baseline)
            }
            SmokePhase::AwaitRedock { target_surface } => {
                self.await_redock(context, target_surface)
            }
            SmokePhase::AwaitRetirement { target_surface } => {
                self.await_retirement(context, target_surface)
            }
            SmokePhase::Done => {}
        }
        if matches!(self.phase, SmokePhase::Done) {
            context.send_viewport_cmd_to(ViewportId::ROOT, ViewportCommand::Close);
        }
    }

    fn queue_tear_off(&mut self, context: &egui::Context) {
        let placement = NativeWindowPlacement::new(
            PhysicalRect::new(760.0, 100.0, 420.0, 320.0)
                .expect("the smoke child placement is valid"),
        );
        match self
            .dockspace
            .request_move_root_to_new_window(CHILD_ROOT, placement)
        {
            Ok(()) => self.transition(SmokePhase::AwaitTearOff),
            Err(error) => self.finish(context, Err(action_request_message(error))),
        }
    }

    fn await_tear_off(&mut self, context: &egui::Context) {
        let Some(status) = self.dockspace.take_action_status() else {
            return;
        };
        match status {
            Ok(DockspaceActionStatus::Applied(
                DockspaceActionOutcome::NativeRootTearOffRequested {
                    root,
                    source_surface,
                    target_surface,
                    items,
                },
            )) if root == CHILD_ROOT
                && source_surface == ROOT_SURFACE
                && items == [CHILD_ITEM, CHILD_DETAIL] =>
            {
                self.transition(SmokePhase::AwaitFirstLive { target_surface });
            }
            Ok(DockspaceActionStatus::Stale { .. }) => {
                self.queue_tear_off(context);
            }
            status => self.fail_without_context(format!(
                "tear-off returned an unexpected terminal status: {status:?}"
            )),
        }
    }

    fn await_first_live(&mut self, context: &egui::Context, target_surface: SurfaceId) {
        if !self.dockspace.is_surface_presented(ROOT_SURFACE)
            || !self.dockspace.is_surface_presented(target_surface)
        {
            return;
        }
        let transferred = self.dockspace.with_view(|view| {
            let root_is_live = view
                .surface(ROOT_SURFACE)
                .and_then(|surface| surface.main_root())
                .is_some_and(|root| root.id() == MAIN_ROOT);
            let child_is_live = view
                .surface(target_surface)
                .and_then(|surface| surface.main_root())
                .is_some_and(|root| root.id() == CHILD_ROOT);
            root_is_live
                && child_is_live
                && [CHILD_ITEM, CHILD_DETAIL].into_iter().all(|item| {
                    view.item(item)
                        .is_some_and(|pane| pane.surface() == target_surface)
                })
        });
        if transferred == Some(true) {
            match self.scenario {
                SmokeScenario::Redock => self.queue_redock(context, target_surface),
                SmokeScenario::RecoverClose
                | SmokeScenario::RetainClose
                | SmokeScenario::CancelClose => {
                    self.transition(SmokePhase::AwaitChildClose { target_surface });
                    self.queue_child_close(context, target_surface);
                }
            }
        }
    }

    fn queue_child_close(&mut self, context: &egui::Context, target_surface: SurfaceId) {
        let child = context.input(|input| unique_deferred_child(&input.raw.viewports));
        let child_viewport = match child {
            Ok(Some(child_viewport)) => child_viewport,
            Ok(None) => {
                context.request_repaint_once_of(ViewportId::ROOT);
                return;
            }
            Err(message) => {
                self.finish(context, Err(message));
                return;
            }
        };
        let close_baseline = self.dockspace.close_progress();
        // The X11 smoke driver waits for this exact title before sending a real
        // WM close request. `ViewportCommand::Close` would bypass Winit's
        // `CloseRequested` event and therefore could not prove close policy.
        context.send_viewport_cmd_to(
            child_viewport,
            ViewportCommand::Title(CLOSE_READY_TITLE.to_owned()),
        );
        context.request_repaint_once_of(ViewportId::ROOT);
        self.transition(SmokePhase::AwaitCloseSettlement {
            target_surface,
            child_viewport,
            close_baseline,
        });
    }

    fn await_close_settlement(
        &mut self,
        context: &egui::Context,
        target_surface: SurfaceId,
        child_viewport: ViewportId,
        close_baseline: NativeCloseProgress,
    ) {
        if self.scenario == SmokeScenario::CancelClose {
            let child_window_exists =
                context.input(|input| input.raw.viewports.contains_key(&child_viewport));
            let layout_matches = self.dockspace.with_view(|view| {
                view.surface(ROOT_SURFACE)
                    .and_then(|surface| surface.main_root())
                    .is_some_and(|root| root.id() == MAIN_ROOT)
                    && view
                        .surface(target_surface)
                        .and_then(|surface| surface.main_root())
                        .is_some_and(|root| root.id() == CHILD_ROOT)
                    && [CHILD_ITEM, CHILD_DETAIL].into_iter().all(|item| {
                        view.item(item).is_some_and(|pane| {
                            pane.surface() == target_surface && pane.root() == CHILD_ROOT
                        })
                    })
            });
            if cancel_close_settled(
                close_baseline.cancellations_committed(),
                self.dockspace.close_progress().cancellations_committed(),
                child_window_exists,
                self.dockspace.is_surface_presented(ROOT_SURFACE),
                self.dockspace.is_surface_presented(target_surface),
                layout_matches == Some(true),
                self.dockspace.is_quiescent(),
            ) {
                self.finish(context, Ok(()));
            }
            return;
        }
        let child_window_destroyed =
            context.input(|input| !input.raw.viewports.contains_key(&child_viewport));
        if !child_window_destroyed
            || !self.dockspace.is_surface_presented(ROOT_SURFACE)
            || self.dockspace.is_surface_presented(target_surface)
            || !self.dockspace.is_quiescent()
        {
            return;
        }

        let layout_matches = self.dockspace.with_view(|view| {
            let root_is_live = view
                .surface(ROOT_SURFACE)
                .and_then(|surface| surface.main_root())
                .is_some_and(|root| root.id() == MAIN_ROOT);
            if !root_is_live {
                return false;
            }
            match self.scenario {
                SmokeScenario::RecoverClose => {
                    [CHILD_ITEM, CHILD_DETAIL].into_iter().all(|item| {
                        view.item(item)
                            .is_some_and(|pane| pane.surface() == ROOT_SURFACE)
                    }) && view
                        .surface(target_surface)
                        .is_none_or(|surface| surface.is_rootless())
                }
                SmokeScenario::RetainClose => {
                    [CHILD_ITEM, CHILD_DETAIL].into_iter().all(|item| {
                        view.item(item)
                            .is_some_and(|pane| pane.surface() == target_surface)
                    }) && view
                        .surface(target_surface)
                        .and_then(|surface| surface.main_root())
                        .is_some_and(|root| root.id() == CHILD_ROOT)
                }
                SmokeScenario::Redock | SmokeScenario::CancelClose => false,
            }
        });
        match layout_matches {
            Some(true) => self.finish(context, Ok(())),
            Some(false) => {
                let layout = self.dockspace.with_view(|view| {
                    let root = view
                        .surface(ROOT_SURFACE)
                        .and_then(|surface| surface.main_root())
                        .map(|root| root.id());
                    let target = view
                        .surface(target_surface)
                        .and_then(|surface| surface.main_root())
                        .map(|root| root.id());
                    let items = [CHILD_ITEM, CHILD_DETAIL].map(|item| {
                        view.item(item).map(|pane| {
                            (
                                item,
                                pane.surface(),
                                pane.root(),
                                pane.contained(),
                                pane.is_selected(),
                            )
                        })
                    });
                    format!("root={root:?}, target={target:?}, items={items:?}")
                });
                self.finish(
                    context,
                    Err(format!(
                        "{:?} reached retired and quiescent state with an unexpected layout: {}",
                        self.scenario,
                        layout.unwrap_or_else(|| "published view unavailable".to_owned())
                    )),
                );
            }
            None => self.finish(
                context,
                Err("native runtime lost its published view after child close".to_owned()),
            ),
        }
    }

    fn queue_redock(&mut self, context: &egui::Context, target_surface: SurfaceId) {
        match self.dockspace.request_dock_root(
            CHILD_ROOT,
            DockPlacement::Center(DockAnchor::Item(MAIN_ITEM)),
        ) {
            Ok(()) => self.transition(SmokePhase::AwaitRedock { target_surface }),
            Err(error) => self.finish(context, Err(action_request_message(error))),
        }
    }

    fn await_redock(&mut self, context: &egui::Context, target_surface: SurfaceId) {
        let Some(status) = self.dockspace.take_action_status() else {
            return;
        };
        match status {
            Ok(DockspaceActionStatus::Applied(DockspaceActionOutcome::RootDocked {
                root,
                target_root,
                items,
                changed: true,
            })) if root == CHILD_ROOT
                && target_root == MAIN_ROOT
                && items == [CHILD_ITEM, CHILD_DETAIL] =>
            {
                let left_child = self.dockspace.with_view(|view| {
                    [CHILD_ITEM, CHILD_DETAIL].into_iter().all(|item| {
                        view.item(item)
                            .is_some_and(|pane| pane.surface() == ROOT_SURFACE)
                    }) && view
                        .surface(target_surface)
                        .is_none_or(|surface| surface.is_rootless())
                });
                if left_child == Some(true) {
                    self.transition(SmokePhase::AwaitRetirement { target_surface });
                } else {
                    self.fail_without_context(
                        "redock outcome did not match the published item ownership".to_owned(),
                    );
                }
            }
            Ok(DockspaceActionStatus::Stale { .. }) => {
                self.queue_redock(context, target_surface);
            }
            status => self.fail_without_context(format!(
                "redock returned an unexpected terminal status: {status:?}"
            )),
        }
    }

    fn await_retirement(&mut self, context: &egui::Context, target_surface: SurfaceId) {
        if self.dockspace.is_surface_presented(target_surface) || !self.dockspace.is_quiescent() {
            return;
        }
        let restored = self.dockspace.with_view(|view| {
            [MAIN_ITEM, CHILD_ITEM, CHILD_DETAIL]
                .into_iter()
                .all(|item| {
                    view.item(item)
                        .is_some_and(|pane| pane.surface() == ROOT_SURFACE)
                })
        });
        if restored == Some(true) {
            self.finish(context, Ok(()));
        }
    }

    fn transition(&mut self, phase: SmokePhase) {
        eprintln!("native-smoke: {phase:?}");
        self.phase = phase;
    }

    fn fail_without_context(&mut self, message: String) {
        self.report_storage_peaks();
        *self.outcome.lock().unwrap_or_else(PoisonError::into_inner) = Some(Err(message));
        self.phase = SmokePhase::Done;
    }

    fn report_storage_peaks(&self) {
        match self.storage_profile {
            SmokeStorageProfile::Bounded => eprintln!(
                "native-smoke: storage profile bounded; root-boundary peaks resident={} bytes, reserved={} bytes; selected limit={STORAGE_BYTE_LIMIT} bytes",
                self.input_capacity_high_water, self.input_reserved_high_water
            ),
            SmokeStorageProfile::Unlimited => eprintln!(
                "native-smoke: storage profile unlimited; peak resident sample at root boundaries={} bytes; no storage reservation selected",
                self.input_capacity_high_water
            ),
        }
    }

    fn finish(&mut self, context: &egui::Context, outcome: Result<(), String>) {
        self.report_storage_peaks();
        match &outcome {
            Ok(()) => eprintln!("native-smoke: complete"),
            Err(message) => eprintln!("native-smoke: failed: {message}"),
        }
        *self.outcome.lock().unwrap_or_else(PoisonError::into_inner) = Some(outcome);
        self.phase = SmokePhase::Done;
        context.send_viewport_cmd_to(ViewportId::ROOT, ViewportCommand::Close);
    }
}

fn cancel_close_settled(
    cancellations_before: u64,
    cancellations_now: u64,
    child_window_exists: bool,
    root_presented: bool,
    child_presented: bool,
    layout_matches: bool,
    quiescent: bool,
) -> bool {
    cancellations_before.checked_add(1) == Some(cancellations_now)
        && child_window_exists
        && root_presented
        && child_presented
        && layout_matches
        && quiescent
}

/// Selects the one non-root viewport owned by this bounded smoke.
///
/// `RawInput` does not expose the viewport class and does not guarantee that a
/// deferred callback reports its logical parent. The one-child layout makes
/// uniqueness sufficient without inspecting adapter bindings.
fn unique_deferred_child(
    viewports: &egui::ViewportIdMap<egui::ViewportInfo>,
) -> Result<Option<ViewportId>, String> {
    let mut children = viewports
        .keys()
        .filter(|viewport| **viewport != ViewportId::ROOT);
    let Some(viewport) = children.next() else {
        return Ok(None);
    };
    if children.next().is_some() {
        return Err(format!(
            "expected at most one deferred child viewport, found multiple: {:?}",
            viewports.keys().collect::<Vec<_>>()
        ));
    }
    Ok(Some(*viewport))
}

fn action_request_message(error: NativeActionRequestError) -> String {
    format!("native action request failed: {error}")
}

fn smoke_layout() -> DockspaceLayout {
    let main = DockspaceRootLayout::new(MAIN_ROOT, DockspaceNode::central_tabs([MAIN_ITEM]));
    let child = DockspaceRootLayout::new(
        CHILD_ROOT,
        DockspaceNode::central_tabs([CHILD_ITEM, CHILD_DETAIL]),
    );
    let contained = DockspaceContainedLayout::new(
        CHILD_FLOATING,
        child,
        LogicalRect::new(80.0, 90.0, 380.0, 280.0).expect("smoke contained bounds validate"),
    );
    DockspaceLayout::new(
        [DockspaceSurfaceLayout::new(ROOT_SURFACE, main).with_contained(contained)],
    )
    .expect("the native smoke layout validates")
}

struct SmokePane {
    title: &'static str,
    body: &'static str,
}

struct SmokePanes {
    panes: BTreeMap<ItemId, SmokePane>,
}

impl SmokePanes {
    fn new() -> Self {
        Self {
            panes: BTreeMap::from([
                (
                    MAIN_ITEM,
                    SmokePane {
                        title: "Main",
                        body: "Native smoke recovery host",
                    },
                ),
                (
                    CHILD_ITEM,
                    SmokePane {
                        title: "Child",
                        body: "Tear-off payload",
                    },
                ),
                (
                    CHILD_DETAIL,
                    SmokePane {
                        title: "Detail",
                        body: "Second item preserves whole-root identity",
                    },
                ),
            ]),
        }
    }
}

impl PaneView for SmokePanes {
    fn title(&self, item: ItemId) -> Option<egui::WidgetText> {
        self.panes.get(&item).map(|pane| pane.title.into())
    }

    fn ui(&mut self, item: ItemId, ui: &mut egui::Ui) {
        let Some(pane) = self.panes.get(&item) else {
            ui.label("Missing smoke pane");
            return;
        };
        ui.heading(pane.title);
        ui.label(pane.body);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn storage_environment_defaults_to_unlimited_and_accepts_explicit_profiles() {
        assert_eq!(
            select_storage_profile(None).unwrap(),
            SmokeStorageProfile::Unlimited
        );
        assert_eq!(
            select_storage_profile(Some("unlimited")).unwrap(),
            SmokeStorageProfile::Unlimited
        );
        assert_eq!(
            select_storage_profile(Some("bounded")).unwrap(),
            SmokeStorageProfile::Bounded
        );
    }

    #[test]
    fn storage_environment_rejects_unknown_empty_and_padded_values() {
        for value in ["", "Bounded", "bounded ", " unlimited", "finite"] {
            let error = select_storage_profile(Some(value)).unwrap_err();
            assert!(error.contains(STORAGE_ENV));
            assert!(error.contains("expected bounded or unlimited"));
        }
    }

    #[test]
    fn scenario_selection_defaults_to_redock_and_cli_overrides_the_environment() {
        assert_eq!(
            select_scenario(&[], None).expect("the default scenario is valid"),
            SmokeScenario::Redock
        );
        assert_eq!(
            select_scenario(&[], Some("recover-close")).expect("the environment is valid"),
            SmokeScenario::RecoverClose
        );
        assert_eq!(
            select_scenario(
                &["--scenario=retain-close".to_owned()],
                Some("recover-close")
            )
            .expect("the CLI selection is valid"),
            SmokeScenario::RetainClose
        );
    }

    #[test]
    fn recover_close_uses_the_default_policy_and_retain_close_is_explicit() {
        assert_eq!(SmokeScenario::RecoverClose.explicit_close_policy(), None);
        assert_eq!(
            SmokeScenario::RetainClose.explicit_close_policy(),
            Some(NativeWindowClosePolicy::RetainLayout)
        );
    }

    #[test]
    fn cancel_close_selects_cancel_policy_from_environment_or_cli() {
        assert_eq!(
            SmokeScenario::CancelClose.explicit_close_policy(),
            Some(NativeWindowClosePolicy::Cancel)
        );
        assert_eq!(
            select_scenario(&[], Some("cancel-close")).expect("cancel-close is valid"),
            SmokeScenario::CancelClose
        );
        for args in [
            vec!["--scenario=cancel-close".to_owned()],
            vec!["--scenario".to_owned(), "cancel-close".to_owned()],
        ] {
            assert_eq!(
                select_scenario(&args, Some("recover-close"))
                    .expect("the CLI overrides the environment"),
                SmokeScenario::CancelClose
            );
        }
        assert!(SmokeScenario::parse("cancel").is_err());
    }

    #[test]
    fn cancel_close_requires_exactly_one_new_committed_cancellation() {
        assert!(cancel_close_settled(7, 8, true, true, true, true, true));
        for (before, now) in [(7, 7), (7, 9), (7, 6), (u64::MAX, u64::MAX)] {
            assert!(!cancel_close_settled(
                before, now, true, true, true, true, true
            ));
        }
    }

    #[test]
    fn cancel_close_requires_live_windows_original_layout_and_quiescence() {
        for missing in 0..5 {
            let mut facts = [true; 5];
            facts[missing] = false;
            assert!(!cancel_close_settled(
                7, 8, facts[0], facts[1], facts[2], facts[3], facts[4]
            ));
        }
        assert!(!cancel_close_settled(7, 8, false, true, false, true, true));
    }

    #[test]
    fn raw_input_child_selection_requires_one_unique_deferred_child() {
        let mut viewports = egui::ViewportIdMap::default();
        viewports.insert(ViewportId::ROOT, egui::ViewportInfo::default());
        assert_eq!(
            unique_deferred_child(&viewports).expect("root-only input is valid"),
            None
        );

        let child = ViewportId::from_hash_of("smoke-child");
        viewports.insert(
            child,
            egui::ViewportInfo {
                parent: Some(ViewportId::ROOT),
                ..Default::default()
            },
        );
        assert_eq!(
            unique_deferred_child(&viewports).expect("one child is unambiguous"),
            Some(child)
        );

        viewports
            .get_mut(&child)
            .expect("the unique child remains present")
            .parent = None;
        assert_eq!(
            unique_deferred_child(&viewports)
                .expect("a deferred callback need not repeat logical parentage"),
            Some(child)
        );

        viewports.insert(
            ViewportId::from_hash_of("unexpected-second-child"),
            egui::ViewportInfo {
                parent: Some(ViewportId::ROOT),
                ..Default::default()
            },
        );
        assert!(unique_deferred_child(&viewports).is_err());
    }
}
