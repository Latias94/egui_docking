//! Application and native-host facts for the physical X11 smoke.

use std::collections::BTreeMap;
use std::error::Error;
use std::io;
use std::net::{SocketAddr, ToSocketAddrs as _};
use std::sync::{Arc, Condvar, Mutex, PoisonError};
use std::time::Instant;

use dockspace::model::{
    DockspaceAxis, DockspaceLayout, DockspaceNode, DockspaceRootLayout, DockspaceSurfaceLayout,
    DockspaceView, ItemId, RootId, SurfaceId,
};
use dockspace::policy::DockPolicy;
use dockspace::runtime::DockspaceSession;
use eframe::egui::{self, ViewportCommand, ViewportId};
use egui_dockspace::{DockStyle, PaneView};
use egui_dockspace_native::NativeDockspaceApp;
use winit::event::{DeviceId, ElementState, MouseButton, WindowEvent};
use winit::window::WindowId;

use super::assertions::{
    NestedPaneCase, PHYSICAL_TARGET_ENV, POINTER_DIAGNOSTIC_LABEL, PaneEdgeCase, PhysicalBounds,
    PhysicalPoint, ROOT_PANE_CONTENT_LABEL,
};
use super::driver;
use super::pointer_load::{
    PointerLoad, PointerSource, active_service_start, load_storage_limit, pointer_load_enabled,
};

#[path = "nested.rs"]
mod nested;

pub(super) const ROOT_SURFACE: SurfaceId = SurfaceId::new(1);
const MAIN_ROOT: RootId = RootId::new(1);
const MAIN_ITEM: ItemId = ItemId::new(1);
pub(super) const CHILD_ITEM: ItemId = ItemId::new(2);
const ANCHOR_ITEM: ItemId = ItemId::new(4);
pub(super) const ROOT_WINDOW_TITLE: &str = "egui_dockspace physical X11 smoke";
pub(super) const ROOT_TARGET_LABEL: &str = "Root docking target";
pub(super) const CHILD_TAB_LABEL: &str = "Child";
pub(super) const CHILD_CLOSE_LABEL: &str = "Close Child";
pub(super) const CENTER_DROP_GUIDE_LABEL: &str = "Dockspace center drop guide";
const STATIONARY_ENV: &str = "EGUI_DOCKSPACE_PHYSICAL_STATIONARY";

pub(super) type Shared = Arc<(Mutex<SharedState>, Condvar)>;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum ProductPhase {
    #[default]
    AwaitingRoot,
    RootPresented,
    ChildFirstLive,
    Redocked,
    Retired,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct ProductObservation {
    pub(super) phase: ProductPhase,
    pub(super) quiescent: bool,
    pub(super) center_merge_verified: bool,
    pub(super) redock_layout: Option<RedockLayout>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum RedockLayout {
    Center,
    StationaryCenter,
    TabBefore,
    OverflowBefore,
    Edge(PaneEdgeCase),
    Nested(NestedPaneCase),
}

fn observed_redock_layout(view: DockspaceView<'_>) -> Option<RedockLayout> {
    let main = view.item(MAIN_ITEM)?;
    let child = view.item(CHILD_ITEM)?;
    if [main, child].iter().any(|item| {
        item.surface() != ROOT_SURFACE || item.root() != MAIN_ROOT || item.contained().is_some()
    }) {
        return None;
    }
    let content = view.surface(ROOT_SURFACE)?.main_root()?.content()?;
    if let Some(tabs) = content.tabs() {
        return match tabs.items() {
            [MAIN_ITEM, CHILD_ITEM] => Some(RedockLayout::Center),
            [CHILD_ITEM, MAIN_ITEM] => Some(RedockLayout::TabBefore),
            _ => None,
        };
    }
    let split = content.split()?;
    if split.child_count() != 2
        || split
            .weights()
            .any(|weight| (weight - 0.5).abs() > f32::EPSILON)
    {
        return None;
    }
    let mut children = split.children();
    let first = children.next()?.tabs()?.items();
    let second = children.next()?.tabs()?.items();
    let child_before = match (first, second) {
        ([CHILD_ITEM], [MAIN_ITEM]) => true,
        ([MAIN_ITEM], [CHILD_ITEM]) => false,
        _ => return None,
    };
    let edge = match (split.axis(), child_before) {
        (DockspaceAxis::Horizontal, true) => PaneEdgeCase::West,
        (DockspaceAxis::Horizontal, false) => PaneEdgeCase::East,
        (DockspaceAxis::Vertical, true) => PaneEdgeCase::North,
        (DockspaceAxis::Vertical, false) => PaneEdgeCase::South,
    };
    Some(RedockLayout::Edge(edge))
}

fn observed_stationary_redock_layout(view: DockspaceView<'_>) -> Option<RedockLayout> {
    for id in [MAIN_ITEM, ANCHOR_ITEM, CHILD_ITEM] {
        let item = view.item(id)?;
        if item.surface() != ROOT_SURFACE || item.root() != MAIN_ROOT || item.contained().is_some()
        {
            return None;
        }
    }
    let surface = view.surface(ROOT_SURFACE)?;
    if view.surfaces().count() != 1 || surface.contained_count() != 0 {
        return None;
    }
    let root = surface.main_root()?;
    let tabs = root.content()?.tabs()?;
    (root.id() == MAIN_ROOT
        && tabs.is_central()
        && tabs.items() == [ANCHOR_ITEM, MAIN_ITEM, CHILD_ITEM]
        && tabs.selected() == Some(CHILD_ITEM))
    .then_some(RedockLayout::StationaryCenter)
}

fn observed_overflow_redock_layout(view: DockspaceView<'_>) -> Option<RedockLayout> {
    let root = view.surface(ROOT_SURFACE)?.main_root()?;
    let tabs = root.content()?.tabs()?;
    (view.surfaces().count() == 1
        && view.surface(ROOT_SURFACE)?.contained_count() == 0
        && root.id() == MAIN_ROOT
        && tabs.is_central()
        && tabs.items() == [ANCHOR_ITEM, CHILD_ITEM, MAIN_ITEM]
        && tabs.selected() == Some(CHILD_ITEM)
        && [ANCHOR_ITEM, CHILD_ITEM, MAIN_ITEM].iter().all(|item| {
            view.item(*item)
                .is_some_and(|item| item.root() == MAIN_ROOT && item.contained().is_none())
        }))
    .then_some(RedockLayout::OverflowBefore)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct PointerPassthroughObservation {
    pub(super) sequence: u64,
    pub(super) viewport: ViewportId,
    pub(super) enabled: bool,
    pub(super) status: eframe::NativeViewportPointerPassthroughStatus,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct StationaryPress {
    pub(super) viewport: ViewportId,
    pub(super) window: WindowId,
    pub(super) device: DeviceId,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct SourceSelection {
    pub(super) main_selected: bool,
    pub(super) anchor_selected: bool,
    pub(super) child_selected: bool,
}

impl SourceSelection {
    pub(super) const INACTIVE_MAIN: Self = Self {
        main_selected: false,
        anchor_selected: true,
        child_selected: true,
    };
}

/// One bounded diagnostic of the real query; it owns no input or continuation.
#[derive(Debug, Default)]
pub(super) struct StationaryProbe {
    pub(super) press: Option<StationaryPress>,
    pub(super) target: Option<PhysicalPoint>,
    pub(super) hover_observed: bool,
    pub(super) selection: Option<SourceSelection>,
    pub(super) result: Option<eframe::NativeViewportPointerObservationResult>,
    pub(super) model_after_result: Option<eframe::NativeViewportPointerObservationCommandToken>,
    pub(super) invalidation: Option<&'static str>,
    pub(super) stress_active: bool,
    pub(super) repeated_output: Option<eframe::NativeOutputToken>,
    pub(super) repeat_finalizations: usize,
    pub(super) slow_suffixes: usize,
    pub(super) complete: bool,
}

impl StationaryProbe {
    fn observe_event(&mut self, event: eframe::NativeWindowEvent<'_>) {
        if self.complete {
            return;
        }
        match event.event() {
            WindowEvent::MouseInput {
                device_id,
                button: MouseButton::Left,
                state,
                ..
            } => {
                if self.target.is_some() {
                    self.invalidation
                        .get_or_insert("a primary-button edge interrupted the stationary hold");
                } else {
                    self.press = (*state == ElementState::Pressed)
                        .then(|| {
                            event.viewport_id().map(|viewport| StationaryPress {
                                viewport,
                                window: event.window_id(),
                                device: *device_id,
                            })
                        })
                        .flatten();
                }
            }
            WindowEvent::CursorMoved {
                device_id, facts, ..
            } => {
                if let Some(target) = self.target {
                    if self.press.is_some_and(|press| {
                        event.viewport_id() == Some(press.viewport)
                            && event.window_id() == press.window
                            && *device_id == press.device
                    }) && facts.desktop_position.is_some_and(|point| {
                        point.x == f64::from(target.x) && point.y == f64::from(target.y)
                    }) {
                        self.hover_observed = true;
                    } else {
                        self.invalidation
                            .get_or_insert("native motion left the exact stationary target/stream");
                    }
                }
            }
            WindowEvent::Destroyed if self.target.is_some() => {
                self.invalidation
                    .get_or_insert("a native window was destroyed during the stationary hold");
            }
            _ => {}
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
enum FrozenWorkAreas {
    Exact(Vec<eframe::NativeWorkAreaRecord>),
    Unknown,
}

#[derive(Clone, Debug, PartialEq)]
struct RootRosterKey {
    backend: eframe::NativeWindowingBackend,
    records: Vec<eframe::NativeViewportRecord>,
    work_areas: FrozenWorkAreas,
}

impl RootRosterKey {
    fn capture(roster: eframe::NativeViewportRoster<'_>) -> Self {
        let work_areas = match roster.work_areas() {
            eframe::NativeWorkAreaRoster::Exact(records) => {
                FrozenWorkAreas::Exact(records.to_vec())
            }
            eframe::NativeWorkAreaRoster::Unknown => FrozenWorkAreas::Unknown,
        };
        Self {
            backend: roster.backend(),
            records: roster.records().to_vec(),
            work_areas,
        }
    }
}

#[derive(Debug, Default)]
pub(super) struct SharedState {
    pub(super) observation: ProductObservation,
    pub(super) pointer_passthrough_sequence: u64,
    pub(super) pointer_passthrough: Vec<PointerPassthroughObservation>,
    pub(super) roster_epoch: u64,
    pub(super) work_area: Option<Result<PhysicalBounds, String>>,
    root_roster: Option<RootRosterKey>,
    pub(super) fatal_error: Option<String>,
    pub(super) driver_result: Option<Result<(), String>>,
    pub(super) last_primary_press: Option<PointerSource>,
    pub(super) pointer_load: Option<PointerLoad>,
    pub(super) stationary: Option<StationaryProbe>,
}

pub(super) fn run() -> Result<(), Box<dyn Error>> {
    let measure_pointer_load = pointer_load_enabled()?;
    let stationary_negative = stationary_negative_from_env(measure_pointer_load)?;
    let inspection_endpoint = inspection_endpoint_from_env()?;
    let nested_target = std::env::var(PHYSICAL_TARGET_ENV)
        .ok()
        .and_then(|value| NestedPaneCase::parse(&value));
    let overflow_target =
        std::env::var(PHYSICAL_TARGET_ENV).as_deref() == Ok("overflow-tab-before");
    if measure_pointer_load && nested_target.is_some() {
        return Err(io::Error::other(
            "the 1 kHz physical load requires an original pane-center target",
        )
        .into());
    }
    let mut policy = DockPolicy::default();
    policy.set_allow_native_surfaces(true);
    let layout = if nested_target.is_some() {
        nested::smoke_layout()
    } else if stationary_negative || overflow_target {
        stationary_smoke_layout()
    } else {
        smoke_layout()
    };
    let mut style = DockStyle::default();
    if overflow_target {
        style.tab_min_width = 400.0;
        style.tab_max_width = 400.0;
    }
    let presentation = if overflow_target {
        dockspace::runtime::DockPresentationConfig::builder()
            .tab_min_width(400.0)
            .tab_max_width(400.0)
            .build()?
    } else {
        dockspace::runtime::DockPresentationConfig::default()
    };
    let session =
        DockspaceSession::from_layout_with_presentation_config(layout, policy, presentation)?;
    let id = egui::Id::unique("egui-native-physical-smoke");
    let dockspace = if measure_pointer_load {
        NativeDockspaceApp::new_with_storage_limit(
            id,
            session,
            ROOT_SURFACE,
            SmokePanes::new(),
            style,
            Default::default(),
            load_storage_limit()?,
        )?
    } else {
        NativeDockspaceApp::new(id, session, ROOT_SURFACE, SmokePanes::new(), style)?
    };
    let shared = Arc::new((
        Mutex::new(SharedState {
            stationary: stationary_negative.then(StationaryProbe::default),
            ..SharedState::default()
        }),
        Condvar::new(),
    ));
    let native_host = dockspace.native_host_handler();
    let native_host: Arc<dyn eframe::NativeHostHandler> = Arc::new(DiagnosticNativeHost {
        inner: native_host,
        shared: Arc::clone(&shared),
        measure_pointer_load,
        stationary_negative,
    });
    let app_shared = Arc::clone(&shared);
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([720.0, 520.0]),
        native_host: Some(native_host),
        ..Default::default()
    };

    let native_result = eframe::run_native(
        ROOT_WINDOW_TITLE,
        options,
        Box::new(move |creation| {
            creation
                .egui_ctx
                .add_plugin(egui_inspection::InspectionPlugin::new(Some(
                    ROOT_WINDOW_TITLE.to_owned(),
                )));
            egui_inspection::serve(&creation.egui_ctx, &inspection_endpoint.to_string())?;
            creation
                .egui_ctx
                .add_plugin(PhysicalSemanticProbe::default());
            let driver_context = creation.egui_ctx.clone();
            let driver_shared = Arc::clone(&app_shared);
            std::thread::Builder::new()
                .name("dockspace-x11-physical-driver".to_owned())
                .spawn(move || {
                    let result = driver::run(Arc::clone(&driver_shared), inspection_endpoint);
                    let (lock, changed) = &*driver_shared;
                    lock.lock()
                        .unwrap_or_else(PoisonError::into_inner)
                        .driver_result = Some(result);
                    changed.notify_all();
                    driver_context.request_repaint_of(ViewportId::ROOT);
                })?;
            Ok(Box::new(PhysicalSmokeApp {
                dockspace,
                child_surface: None,
                shared: Arc::clone(&app_shared),
                measure_pointer_load,
                nested_target: nested_target.is_some(),
                stationary_negative,
                overflow_target,
            }))
        }),
    );

    let (lock, _) = &*shared;
    let mut state = lock.lock().unwrap_or_else(PoisonError::into_inner);
    if let Some(load) = &state.pointer_load {
        // Preserve service diagnostics even when injection or another existing gate fails.
        load.service.report();
    }
    native_result?;
    match state.driver_result.take() {
        Some(Ok(())) => Ok(()),
        Some(Err(message)) => Err(io::Error::other(message).into()),
        None => Err(io::Error::other(
            "physical smoke exited before the XTest driver reached a terminal stage",
        )
        .into()),
    }
}

fn stationary_negative_from_env(measure_pointer_load: bool) -> Result<bool, String> {
    match std::env::var(STATIONARY_ENV) {
        Err(std::env::VarError::NotPresent) => return Ok(false),
        Ok(value) if value == "negative" => {}
        value => {
            return Err(format!(
                "{STATIONARY_ENV}: expected negative or an unset variable, found {value:?}"
            ));
        }
    }
    if measure_pointer_load || std::env::var_os(PHYSICAL_TARGET_ENV).is_some() {
        return Err(format!(
            "{STATIONARY_ENV}=negative requires its fixed anchor layout and default guide-control target ({PHYSICAL_TARGET_ENV} unset), without the 1 kHz load"
        ));
    }
    Ok(true)
}

fn inspection_endpoint_from_env() -> io::Result<SocketAddr> {
    let configured = egui_inspection::bind_addr_from_env().ok_or_else(|| {
        io::Error::other(
            "EGUI_INSPECTION must name a loopback endpoint for semantic viewport diagnostics",
        )
    })?;
    let mut endpoints = configured
        .to_socket_addrs()?
        .filter(SocketAddr::is_ipv4)
        .collect::<Vec<_>>();
    endpoints.sort_unstable();
    endpoints.dedup();
    let [endpoint] = endpoints.as_slice() else {
        return Err(io::Error::other(format!(
            "inspection endpoint {configured:?} must resolve to exactly one IPv4 address"
        )));
    };
    if !endpoint.ip().is_loopback() {
        return Err(io::Error::other(format!(
            "physical smoke refuses non-loopback inspection endpoint {endpoint}"
        )));
    }
    Ok(*endpoint)
}

#[derive(Default)]
struct PhysicalSemanticProbe {
    diagnostic_enabled: bool,
}

impl egui::Plugin for PhysicalSemanticProbe {
    fn debug_name(&self) -> &'static str {
        "egui_native_smoke::physical_semantic_probe"
    }

    fn on_begin_pass(&mut self, ui: &mut egui::Ui) {
        if !self.diagnostic_enabled {
            // This is a private smoke-only handshake with the renderer. It is
            // intentionally not exposed by the product facade as a supported
            // application API.
            ui.ctx().data_mut(|data| {
                data.insert_temp(
                    egui::Id::unique("egui-dockspace-native-drag-preview-diagnostic"),
                    true,
                );
            });
            self.diagnostic_enabled = true;
        }
        publish_pointer_diagnostic(ui);
    }
}

struct DiagnosticNativeHost {
    inner: Arc<dyn eframe::NativeHostHandler>,
    shared: Shared,
    measure_pointer_load: bool,
    stationary_negative: bool,
}

impl eframe::NativeHostHandler for DiagnosticNativeHost {
    fn on_service(&self, context: &egui::Context) -> eframe::NativeHostWake {
        let started = active_service_start(&self.shared, self.measure_pointer_load);
        let wake = self.inner.on_service(context);
        if let Some(started) = started {
            let elapsed = started.elapsed();
            if let Some(load) = &mut self
                .shared
                .0
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .pointer_load
            {
                load.service.host_event(elapsed);
            }
        }
        wake
    }

    fn before_output_input(
        &self,
        context: &egui::Context,
        token: eframe::NativeOutputToken,
        has_input: bool,
    ) -> eframe::NativeOutputInputAdmission {
        let admission = self.inner.before_output_input(context, token, has_input);
        if self.measure_pointer_load
            && matches!(admission, eframe::NativeOutputInputAdmission::Defer(_))
        {
            let ended = Instant::now();
            if let Some(load) = &mut self
                .shared
                .0
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .pointer_load
            {
                load.service.input_deferred(token, ended);
            }
        }
        admission
    }

    fn install_output_pass_finalizer(
        &self,
        finalizer: Arc<dyn eframe::NativeOutputPassFinalizer>,
    ) -> bool {
        self.inner.install_output_pass_finalizer(finalizer)
    }

    fn try_attach(&self, attachment: eframe::NativeHostAttachment) -> bool {
        self.inner.try_attach(attachment)
    }

    fn detach(&self, attachment: eframe::NativeHostAttachment) {
        self.inner.detach(attachment);
    }

    fn begin_deferred_viewport_create(
        &self,
        attempt: eframe::NativeViewportCreateAttempt,
    ) -> eframe::NativeViewportCreateAdmission {
        self.inner.begin_deferred_viewport_create(attempt)
    }

    fn render_hidden_deferred_viewport(&self, viewport_id: ViewportId) -> bool {
        self.inner.render_hidden_deferred_viewport(viewport_id)
    }

    fn on_window_event(
        &self,
        event: eframe::NativeWindowEvent<'_>,
    ) -> eframe::NativeWindowEventDisposition {
        if self.measure_pointer_load {
            let observed_at = Instant::now();
            self.shared
                .0
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .observe_pointer_event(
                    event.viewport_id(),
                    event.window_id(),
                    event.event(),
                    observed_at,
                );
        }
        if self.stationary_negative {
            let (lock, changed) = &*self.shared;
            if let Some(probe) = &mut lock
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .stationary
            {
                probe.observe_event(event);
            }
            changed.notify_all();
        }
        self.inner.on_window_event(event)
    }

    fn on_device_removed(&self, removal: eframe::NativeDeviceRemoval) {
        if self.measure_pointer_load {
            self.shared
                .0
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .observe_device_removal();
        }
        if self.stationary_negative {
            let (lock, changed) = &*self.shared;
            if let Some(probe) = &mut lock
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .stationary
                && !probe.complete
            {
                probe.press = None;
                probe
                    .invalidation
                    .get_or_insert("a device was removed before the stationary barrier");
            }
            changed.notify_all();
        }
        self.inner.on_device_removed(removal);
    }

    fn on_global_focus(
        &self,
        observation: eframe::NativeGlobalFocusObservation,
    ) -> eframe::NativeHostWake {
        self.inner.on_global_focus(observation)
    }

    fn on_viewport_focus(
        &self,
        result: eframe::NativeViewportFocusResult,
    ) -> eframe::NativeHostWake {
        self.inner.on_viewport_focus(result)
    }

    fn on_viewport_pointer_passthrough(
        &self,
        result: eframe::NativeViewportPointerPassthroughResult,
    ) -> eframe::NativeHostWake {
        let wake = self.inner.on_viewport_pointer_passthrough(result);
        let (lock, changed) = &*self.shared;
        let mut shared = lock.lock().unwrap_or_else(PoisonError::into_inner);
        shared.pointer_passthrough_sequence = shared.pointer_passthrough_sequence.saturating_add(1);
        let observation = PointerPassthroughObservation {
            sequence: shared.pointer_passthrough_sequence,
            viewport: result.viewport_id(),
            enabled: result.enabled(),
            status: result.status(),
        };
        if let Some(previous) = shared.pointer_passthrough.iter_mut().find(|previous| {
            previous.viewport == observation.viewport && previous.enabled == observation.enabled
        }) {
            *previous = observation;
        } else {
            // Retain one latest fact per route key, never an unbounded callback history.
            shared.pointer_passthrough.push(observation);
        }
        changed.notify_all();
        wake
    }

    fn on_viewport_property_command_retired(
        &self,
        retirement: eframe::NativeViewportPropertyCommandRetirement,
    ) -> eframe::NativeHostWake {
        self.inner.on_viewport_property_command_retired(retirement)
    }

    fn on_viewport_pointer_observation(
        &self,
        result: eframe::NativeViewportPointerObservationResult,
    ) -> eframe::NativeHostWake {
        let wake = self.inner.on_viewport_pointer_observation(result);
        if self.stationary_negative {
            eprintln!(
                "physical-smoke: stationary-negative forwarded raw_result={result:?} host_wake={wake:?}"
            );
            let (lock, changed) = &*self.shared;
            if let Some(probe) = &mut lock
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .stationary
                && probe.hover_observed
                && !probe.complete
            {
                probe.result.get_or_insert(result);
                if wake != eframe::NativeHostWake::RepaintRootAfterCurrent {
                    probe
                        .invalidation
                        .get_or_insert("the real query result was not admitted by the native host");
                }
            }
            changed.notify_all();
        }
        wake
    }

    fn on_output_begin(
        &self,
        token: eframe::NativeOutputToken,
        window: eframe::NativeWindowSnapshot,
        root_roster: Option<eframe::NativeViewportRoster<'_>>,
    ) -> eframe::NativeOutputAdmission {
        // Begin before admission, so the later input prelude and every Repeat
        // pass share this one outer synchronous service interval.
        let started = active_service_start(&self.shared, self.measure_pointer_load);
        if let Some(roster) = root_roster {
            let key = RootRosterKey::capture(roster);
            let work_area = validated_x11_work_area(roster);
            let (lock, changed) = &*self.shared;
            let mut shared = lock.lock().unwrap_or_else(PoisonError::into_inner);
            if shared.root_roster.as_ref() != Some(&key) {
                shared.roster_epoch = shared.roster_epoch.saturating_add(1);
                shared.work_area = Some(work_area);
                shared.root_roster = Some(key);
                eprintln!(
                    "physical-smoke: native root roster epoch={} work-area={:?}",
                    shared.roster_epoch, shared.work_area
                );
                changed.notify_all();
            }
        }
        let admission = self.inner.on_output_begin(token, window, root_roster);
        if let Some(started) = started {
            let ended = Instant::now();
            if let Some(load) = &mut self
                .shared
                .0
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .pointer_load
            {
                load.service.admitted(token, started, ended, admission);
            }
        }
        admission
    }

    fn on_output_pass_finalized(
        &self,
        finalization: eframe::NativeOutputPassFinalization<'_>,
    ) -> eframe::NativeHostWake {
        let token = eframe::current_native_output_token();
        let disposition = finalization.disposition();
        let wake = self.inner.on_output_pass_finalized(finalization);
        if self.measure_pointer_load {
            let ended = Instant::now();
            if let Some(load) = &mut self
                .shared
                .0
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .pointer_load
            {
                load.service.finalized(
                    token.expect("a real host finalizer has its active output token"),
                    ended,
                    disposition,
                );
            }
        }
        if self.stationary_negative {
            let (lock, changed) = &*self.shared;
            if let Some(probe) = &mut lock
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .stationary
                && token.is_some()
                && probe.repeated_output == token
                && !probe.complete
            {
                // Two real pass finalizations for one outer output prove that
                // its requested discard actually repeated before terminal UI.
                probe.repeat_finalizations += 1;
                changed.notify_all();
            }
        }
        wake
    }

    fn on_output(&self, result: eframe::NativeOutputResult) -> eframe::NativeHostWake {
        if self.measure_pointer_load
            && let Some(load) = &mut self
                .shared
                .0
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .pointer_load
        {
            load.service.terminal(result.token());
        }
        self.inner.on_output(result)
    }

    fn on_viewport_create_failed(
        &self,
        failure: eframe::NativeViewportCreateFailure,
    ) -> eframe::NativeHostWake {
        self.inner.on_viewport_create_failed(failure)
    }

    fn on_viewport_visibility(
        &self,
        result: eframe::NativeViewportVisibilityResult,
    ) -> eframe::NativeHostWake {
        self.inner.on_viewport_visibility(result)
    }

    fn on_viewport_close_cancelled(
        &self,
        request: eframe::NativeViewportCloseRequest,
    ) -> eframe::NativeHostWake {
        self.inner.on_viewport_close_cancelled(request)
    }
}

fn validated_x11_work_area(
    roster: eframe::NativeViewportRoster<'_>,
) -> Result<PhysicalBounds, String> {
    if roster.backend() != eframe::NativeWindowingBackend::X11 {
        return Err(format!(
            "physical smoke requires an X11 roster, observed {:?}",
            roster.backend()
        ));
    }
    if !roster
        .records()
        .iter()
        .any(|record| record.viewport_id() == ViewportId::ROOT)
    {
        return Err("native roster does not contain the root viewport".to_owned());
    }
    let eframe::NativeWorkAreaRoster::Exact(records) = roster.work_areas() else {
        return Err("X11 native roster did not provide exact work-area authority".to_owned());
    };
    let [record] = records else {
        return Err(format!(
            "X11 exact work-area roster must contain one proven display, found {}",
            records.len()
        ));
    };
    let display = physical_bounds(record.display_bounds(), "display bounds")?;
    let work_area = physical_bounds(record.work_area_bounds(), "work-area bounds")?;
    validate_work_area_bounds(display, work_area, record.scale_factor())?;
    Ok(work_area)
}

fn physical_bounds(
    rect: eframe::NativePhysicalRect,
    description: &str,
) -> Result<PhysicalBounds, String> {
    let width = i32::try_from(rect.width())
        .map_err(|_| format!("{description} width {} exceeds i32", rect.width()))?;
    let height = i32::try_from(rect.height())
        .map_err(|_| format!("{description} height {} exceeds i32", rect.height()))?;
    if width <= 0 || height <= 0 {
        return Err(format!("{description} is empty: {rect:?}"));
    }
    let max_x = rect
        .x()
        .checked_add(width)
        .ok_or_else(|| format!("{description} horizontal extent overflows"))?;
    let max_y = rect
        .y()
        .checked_add(height)
        .ok_or_else(|| format!("{description} vertical extent overflows"))?;
    Ok(PhysicalBounds {
        min_x: rect.x(),
        min_y: rect.y(),
        max_x,
        max_y,
    })
}

fn validate_work_area_bounds(
    display: PhysicalBounds,
    work_area: PhysicalBounds,
    scale_factor: f64,
) -> Result<(), String> {
    if !scale_factor.is_finite() || scale_factor <= 0.0 {
        return Err(format!(
            "native work-area scale factor must be finite and positive, found {scale_factor}"
        ));
    }
    if work_area.min_x < display.min_x
        || work_area.min_y < display.min_y
        || work_area.max_x > display.max_x
        || work_area.max_y > display.max_y
    {
        return Err(format!(
            "native work area {work_area:?} is not contained by display {display:?}"
        ));
    }
    Ok(())
}

struct PhysicalSmokeApp {
    dockspace: NativeDockspaceApp<SmokePanes>,
    child_surface: Option<SurfaceId>,
    shared: Shared,
    measure_pointer_load: bool,
    nested_target: bool,
    stationary_negative: bool,
    overflow_target: bool,
}

impl eframe::App for PhysicalSmokeApp {
    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        // Only a result forwarded before this UI can label its later model observation.
        let stationary_result = self.stationary_negative.then(|| {
            self.shared
                .0
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .stationary
                .as_ref()
                .and_then(|probe| probe.result)
        });
        let started = active_service_start(&self.shared, self.measure_pointer_load);
        <NativeDockspaceApp<SmokePanes> as eframe::App>::ui(&mut self.dockspace, ui, frame);
        if self.stationary_negative {
            let slow_suffix = {
                let mut shared = self.shared.0.lock().unwrap_or_else(PoisonError::into_inner);
                shared.stationary.as_mut().is_some_and(|probe| {
                    if !probe.stress_active || probe.complete {
                        return false;
                    }
                    if let Some(token) = eframe::current_native_output_token()
                        && probe.repeated_output.is_none()
                    {
                        probe.repeated_output = Some(token);
                        ui.ctx()
                            .request_discard("stationary query survives a repeated slow UI");
                    }
                    probe.slow_suffixes += 1;
                    true
                })
            };
            if slow_suffix {
                // Model ordinary wrapper work after the dockspace UI. Query
                // preparation must already have paid the shared 2 ms budget.
                std::thread::sleep(std::time::Duration::from_millis(3));
            }
        }
        if let Some(started) = started {
            if let Some(load) = self
                .shared
                .0
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .pointer_load
                .as_mut()
            {
                load.max_root_update = load.max_root_update.max(started.elapsed());
            }
        }
        self.publish_observation(stationary_result.flatten().map(|result| result.token()));
        let driver_finished = {
            let (lock, _) = &*self.shared;
            lock.lock()
                .unwrap_or_else(PoisonError::into_inner)
                .driver_result
                .is_some()
        };
        if driver_finished {
            ui.ctx()
                .send_viewport_cmd_to(ViewportId::ROOT, ViewportCommand::Close);
        }
    }
}

impl PhysicalSmokeApp {
    fn publish_observation(
        &mut self,
        stationary_result: Option<eframe::NativeViewportPointerObservationCommandToken>,
    ) {
        let root_presented = self.dockspace.is_surface_presented(ROOT_SURFACE);
        let child_location = self
            .dockspace
            .with_view(|view| {
                view.item(CHILD_ITEM)
                    .map(dockspace::model::DockspaceItemView::surface)
            })
            .flatten();
        if self.child_surface.is_none()
            && let Some(surface) = child_location
            && surface != ROOT_SURFACE
        {
            self.child_surface = Some(surface);
        }
        let first_live = self.child_surface.is_some_and(|surface| {
            child_location == Some(surface) && self.dockspace.is_surface_presented(surface)
        });
        let redocked = self.child_surface.is_some() && child_location == Some(ROOT_SURFACE);
        let redock_layout = redocked
            .then(|| {
                self.dockspace
                    .with_view(|view| {
                        if self.nested_target {
                            nested::observed_redock_layout(view)
                        } else if self.stationary_negative {
                            observed_stationary_redock_layout(view)
                        } else if self.overflow_target {
                            observed_overflow_redock_layout(view)
                        } else {
                            observed_redock_layout(view)
                        }
                    })
                    .flatten()
            })
            .flatten();
        let center_merge_verified = matches!(
            redock_layout,
            Some(RedockLayout::Center | RedockLayout::StationaryCenter)
        );
        let retired = self
            .child_surface
            .is_some_and(|surface| redocked && !self.dockspace.is_surface_presented(surface));
        let phase = if retired {
            ProductPhase::Retired
        } else if redocked {
            ProductPhase::Redocked
        } else if first_live {
            ProductPhase::ChildFirstLive
        } else if root_presented {
            ProductPhase::RootPresented
        } else {
            ProductPhase::AwaitingRoot
        };
        let observation = ProductObservation {
            phase,
            quiescent: self.dockspace.is_quiescent(),
            center_merge_verified,
            redock_layout,
        };
        let fatal_error = self.dockspace.with_errors(|primary, cleanup| {
            primary.map(|error| {
                format!("native runtime entered fatal state: {error:?}; cleanup={cleanup:?}")
            })
        });
        let load_active = self.measure_pointer_load
            && self
                .shared
                .0
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .pointer_load
                .is_some();
        let load_storage = if load_active {
            Some((
                self.dockspace.input_storage_capacity_bytes(),
                self.dockspace.input_storage_reserved_bytes(),
            ))
        } else {
            None
        };
        let source_selection =
            self.stationary_negative
                .then(|| {
                    self.dockspace
                        .with_view(|view| {
                            let main = view.item(MAIN_ITEM)?;
                            let anchor = view.item(ANCHOR_ITEM)?;
                            let child = view.item(CHILD_ITEM)?;
                            if [main, anchor].iter().any(|item| {
                                item.surface() != ROOT_SURFACE || item.root() != MAIN_ROOT
                            }) || self.child_surface != Some(child.surface())
                                || child.surface() == ROOT_SURFACE
                            {
                                return None;
                            }
                            Some(SourceSelection {
                                main_selected: main.is_selected(),
                                anchor_selected: anchor.is_selected(),
                                child_selected: child.is_selected(),
                            })
                        })
                        .flatten()
                })
                .flatten();
        let stationary_semantic =
            self.stationary_negative && !self.dockspace.current_output_is_retained();
        let (lock, changed) = &*self.shared;
        let mut shared = lock.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(probe) = &mut shared.stationary
            && !probe.complete
        {
            probe.selection = source_selection;
            if probe.target.is_some() && source_selection != Some(SourceSelection::INACTIVE_MAIN) {
                probe
                    .invalidation
                    .get_or_insert("the public model changed the inactive Main/source selection");
            }
            if stationary_semantic {
                probe.model_after_result = stationary_result;
            }
            changed.notify_all();
        }
        if let Some(load) = &mut shared.pointer_load {
            load.observe_model(center_merge_verified, Instant::now());
            if let Some((owned, reserved)) = &load_storage {
                load.observe_storage(owned.as_ref().ok().copied(), *reserved);
            }
        }
        let observation_changed = shared.observation != observation;
        let fatal_changed = shared.fatal_error != fatal_error;
        if observation_changed {
            shared.observation = observation;
            eprintln!("physical-smoke: product observation {observation:?}");
        }
        if fatal_changed {
            shared.fatal_error = fatal_error;
        }
        if observation_changed || fatal_changed {
            changed.notify_all();
        }
    }
}

fn smoke_layout() -> DockspaceLayout {
    let root = DockspaceRootLayout::new(
        MAIN_ROOT,
        DockspaceNode::central_tabs_with_selection([MAIN_ITEM, CHILD_ITEM], Some(CHILD_ITEM))
            .expect("the physical drag payload is a valid initial selection"),
    );
    DockspaceLayout::new([DockspaceSurfaceLayout::new(ROOT_SURFACE, root)])
        .expect("the physical smoke layout validates")
}

fn stationary_smoke_layout() -> DockspaceLayout {
    let root = DockspaceRootLayout::new(
        MAIN_ROOT,
        DockspaceNode::central_tabs_with_selection(
            [ANCHOR_ITEM, MAIN_ITEM, CHILD_ITEM],
            Some(CHILD_ITEM),
        )
        .expect("the stationary payload starts selected beside an inactive Main"),
    );
    DockspaceLayout::new([DockspaceSurfaceLayout::new(ROOT_SURFACE, root)])
        .expect("the fixed stationary smoke layout validates")
}

struct SmokePane {
    title: &'static str,
    text: String,
    widgets: Vec<egui::Id>,
}

struct SmokePanes {
    panes: BTreeMap<ItemId, SmokePane>,
}

impl SmokePanes {
    fn new() -> Self {
        Self {
            panes: [
                (MAIN_ITEM, "Main", "Root docking surface"),
                (CHILD_ITEM, "Child", "Physical drag payload"),
                (
                    ANCHOR_ITEM,
                    "Anchor",
                    "Keep Main inactive after native tear-off",
                ),
                (nested::SIBLING_ITEM, "Sibling", "Unchanged nested sibling"),
            ]
            .into_iter()
            .map(|(item, title, text)| {
                (
                    item,
                    SmokePane {
                        title,
                        text: text.to_owned(),
                        widgets: Vec::with_capacity(1),
                    },
                )
            })
            .collect(),
        }
    }
}

impl PaneView for SmokePanes {
    fn title(&self, item: ItemId) -> Option<egui::WidgetText> {
        self.panes.get(&item).map(|pane| pane.title.into())
    }

    fn ui(&mut self, item: ItemId, ui: &mut egui::Ui) {
        let Some(pane) = self.panes.get_mut(&item) else {
            ui.label("Missing smoke pane");
            return;
        };
        if item == MAIN_ITEM || item == ANCHOR_ITEM {
            publish_root_pane_content_diagnostic(ui);
            publish_root_target_diagnostic(ui);
        } else {
            ui.heading(pane.title);
        }
        let response = ui.add_sized(
            ui.available_size(),
            egui::TextEdit::multiline(&mut pane.text)
                .id(egui::Id::unique(("physical-smoke-content", item.get())))
                .desired_width(f32::INFINITY),
        );
        pane.widgets.clear();
        pane.widgets.push(response.id);
    }

    fn docking_content_widgets(&self, item: ItemId) -> &[egui::Id] {
        self.panes
            .get(&item)
            .map_or(&[], |pane| pane.widgets.as_slice())
    }
}

fn publish_root_pane_content_diagnostic(ui: &egui::Ui) {
    let pane = ui.max_rect();
    let id = ui.make_persistent_id(("physical-smoke-pane-content", ui.ctx().viewport_id()));
    ui.ctx().accesskit_node_builder(id, |node| {
        node.set_role(egui::accesskit::Role::Label);
        node.set_value(ROOT_PANE_CONTENT_LABEL);
        node.set_bounds(egui::accesskit::Rect::new(
            f64::from(pane.min.x),
            f64::from(pane.min.y),
            f64::from(pane.max.x),
            f64::from(pane.max.y),
        ));
    });
}

fn publish_root_target_diagnostic(ui: &egui::Ui) {
    let pane = ui.max_rect();
    let ingress = egui::Rect::from_min_max(
        egui::pos2(
            egui::lerp(pane.left()..=pane.right(), 0.1),
            egui::lerp(pane.top()..=pane.bottom(), 0.1),
        ),
        egui::pos2(
            egui::lerp(pane.left()..=pane.right(), 0.2),
            egui::lerp(pane.top()..=pane.bottom(), 0.2),
        ),
    );
    let id = ui.make_persistent_id(("physical-smoke-root-target", ui.ctx().viewport_id()));
    ui.ctx().accesskit_node_builder(id, |node| {
        node.set_role(egui::accesskit::Role::Label);
        node.set_value(ROOT_TARGET_LABEL);
        node.set_bounds(egui::accesskit::Rect::new(
            f64::from(ingress.min.x),
            f64::from(ingress.min.y),
            f64::from(ingress.max.x),
            f64::from(ingress.max.y),
        ));
    });
}

fn publish_pointer_diagnostic(ui: &egui::Ui) {
    let Some((position, primary_down)) = ui.input(|input| {
        input
            .pointer
            .latest_pos()
            .map(|position| (position, input.pointer.primary_down()))
    }) else {
        return;
    };
    let id = ui.make_persistent_id(("physical-smoke-pointer-diagnostic", ui.ctx().viewport_id()));
    ui.ctx().accesskit_node_builder(id, |node| {
        node.set_role(egui::accesskit::Role::Label);
        node.set_label(POINTER_DIAGNOSTIC_LABEL);
        node.set_value(if primary_down {
            "primary-down"
        } else {
            "primary-up"
        });
        node.set_bounds(egui::accesskit::Rect::new(
            f64::from(position.x) - 0.5,
            f64::from(position.y) - 0.5,
            f64::from(position.x) + 0.5,
            f64::from(position.y) + 0.5,
        ));
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn physical_drag_payload_starts_selected() {
        let session = DockspaceSession::from_layout(smoke_layout(), DockPolicy::default())
            .expect("the physical smoke layout should create a session");

        assert!(
            session
                .view()
                .item(CHILD_ITEM)
                .is_some_and(|item| item.is_selected())
        );
    }

    #[test]
    fn redock_observation_distinguishes_center_and_ordered_edge_splits() {
        assert_eq!(
            observed_redock_layout(smoke_layout().view()),
            Some(RedockLayout::Center)
        );
        for (axis, first, second, expected) in [
            (
                DockspaceAxis::Horizontal,
                CHILD_ITEM,
                MAIN_ITEM,
                PaneEdgeCase::West,
            ),
            (
                DockspaceAxis::Horizontal,
                MAIN_ITEM,
                CHILD_ITEM,
                PaneEdgeCase::East,
            ),
            (
                DockspaceAxis::Vertical,
                CHILD_ITEM,
                MAIN_ITEM,
                PaneEdgeCase::North,
            ),
            (
                DockspaceAxis::Vertical,
                MAIN_ITEM,
                CHILD_ITEM,
                PaneEdgeCase::South,
            ),
        ] {
            let split = DockspaceNode::equal_split(
                axis,
                [DockspaceNode::tabs([first]), DockspaceNode::tabs([second])],
            )
            .unwrap();
            let layout = DockspaceLayout::new([DockspaceSurfaceLayout::new(
                ROOT_SURFACE,
                DockspaceRootLayout::new(MAIN_ROOT, split),
            )])
            .unwrap();
            assert_eq!(
                observed_redock_layout(layout.view()),
                Some(RedockLayout::Edge(expected))
            );
        }
        let reversed_center = DockspaceLayout::new([DockspaceSurfaceLayout::new(
            ROOT_SURFACE,
            DockspaceRootLayout::new(MAIN_ROOT, DockspaceNode::tabs([CHILD_ITEM, MAIN_ITEM])),
        )])
        .unwrap();
        assert_eq!(
            observed_redock_layout(reversed_center.view()),
            Some(RedockLayout::TabBefore)
        );
        let uneven = DockspaceLayout::new([DockspaceSurfaceLayout::new(
            ROOT_SURFACE,
            DockspaceRootLayout::new(
                MAIN_ROOT,
                DockspaceNode::split(
                    DockspaceAxis::Vertical,
                    [
                        (DockspaceNode::tabs([CHILD_ITEM]), 0.4),
                        (DockspaceNode::tabs([MAIN_ITEM]), 0.6),
                    ],
                )
                .unwrap(),
            ),
        )])
        .unwrap();
        assert_eq!(observed_redock_layout(uneven.view()), None);
    }

    #[test]
    fn exact_work_area_must_be_contained_and_scaled() {
        let display = PhysicalBounds {
            min_x: 0,
            min_y: 0,
            max_x: 1280,
            max_y: 800,
        };
        let work_area = PhysicalBounds {
            min_x: 0,
            min_y: 24,
            max_x: 1280,
            max_y: 800,
        };

        assert_eq!(validate_work_area_bounds(display, work_area, 1.0), Ok(()));
        assert!(
            validate_work_area_bounds(
                display,
                PhysicalBounds {
                    max_x: 1281,
                    ..work_area
                },
                1.0,
            )
            .is_err()
        );
        assert!(validate_work_area_bounds(display, work_area, 0.0).is_err());
    }
}
