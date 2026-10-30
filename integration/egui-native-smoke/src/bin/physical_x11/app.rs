//! Application and native-host facts for the physical X11 smoke.

use std::collections::BTreeMap;
use std::error::Error;
use std::io;
use std::net::{SocketAddr, ToSocketAddrs as _};
use std::sync::{Arc, Condvar, Mutex, PoisonError};
use std::time::Instant;

use dockspace::model::{
    DockspaceLayout, DockspaceNode, DockspaceRootLayout, DockspaceSurfaceLayout, ItemId, RootId,
    SurfaceId,
};
use dockspace::policy::DockPolicy;
use dockspace::runtime::DockspaceSession;
use eframe::egui::{self, ViewportCommand, ViewportId};
use egui_dockspace::{DockStyle, PaneView};
use egui_dockspace_native::NativeDockspaceApp;

use super::assertions::{POINTER_DIAGNOSTIC_LABEL, PhysicalBounds, ROOT_PANE_CONTENT_LABEL};
use super::driver;
use super::pointer_load::{PointerLoad, PointerSource, load_storage_limit, pointer_load_enabled};

pub(super) const ROOT_SURFACE: SurfaceId = SurfaceId::new(1);
const MAIN_ROOT: RootId = RootId::new(1);
const MAIN_ITEM: ItemId = ItemId::new(1);
pub(super) const CHILD_ITEM: ItemId = ItemId::new(2);
pub(super) const ROOT_WINDOW_TITLE: &str = "egui_dockspace physical X11 smoke";
pub(super) const ROOT_TARGET_LABEL: &str = "Root docking target";
pub(super) const CHILD_TAB_LABEL: &str = "Child";
pub(super) const CHILD_CLOSE_LABEL: &str = "Close Child";
pub(super) const CENTER_DROP_GUIDE_LABEL: &str = "Dockspace center drop guide";

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
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct PointerPassthroughObservation {
    pub(super) sequence: u64,
    pub(super) viewport: ViewportId,
    pub(super) enabled: bool,
    pub(super) status: eframe::NativeViewportPointerPassthroughStatus,
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
}

pub(super) fn run() -> Result<(), Box<dyn Error>> {
    let inspection_endpoint = inspection_endpoint_from_env()?;
    let measure_pointer_load = pointer_load_enabled()?;
    let mut policy = DockPolicy::default();
    policy.set_allow_native_surfaces(true);
    let session = DockspaceSession::from_layout(smoke_layout(), policy)?;
    let id = egui::Id::unique("egui-native-physical-smoke");
    let dockspace = if measure_pointer_load {
        NativeDockspaceApp::new_with_storage_limit(
            id,
            session,
            ROOT_SURFACE,
            SmokePanes::new(),
            DockStyle::default(),
            Default::default(),
            load_storage_limit()?,
        )?
    } else {
        NativeDockspaceApp::new(
            id,
            session,
            ROOT_SURFACE,
            SmokePanes::new(),
            DockStyle::default(),
        )?
    };
    let shared = Arc::new((Mutex::new(SharedState::default()), Condvar::new()));
    let native_host = dockspace.native_host_handler();
    let native_host: Arc<dyn eframe::NativeHostHandler> = Arc::new(DiagnosticNativeHost {
        inner: native_host,
        shared: Arc::clone(&shared),
        measure_pointer_load,
    });
    let app_shared = Arc::clone(&shared);
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([720.0, 520.0]),
        native_host: Some(native_host),
        ..Default::default()
    };

    eframe::run_native(
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
            }))
        }),
    )?;

    let (lock, _) = &*shared;
    match lock
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .driver_result
        .take()
    {
        Some(Ok(())) => Ok(()),
        Some(Err(message)) => Err(io::Error::other(message).into()),
        None => Err(io::Error::other(
            "physical smoke exited before the XTest driver reached a terminal stage",
        )
        .into()),
    }
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
}

impl eframe::NativeHostHandler for DiagnosticNativeHost {
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

    fn on_output_begin(
        &self,
        token: eframe::NativeOutputToken,
        window: eframe::NativeWindowSnapshot,
        root_roster: Option<eframe::NativeViewportRoster<'_>>,
    ) -> eframe::NativeOutputAdmission {
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
        self.inner.on_output_begin(token, window, root_roster)
    }

    fn on_output_pass_finalized(
        &self,
        finalization: eframe::NativeOutputPassFinalization<'_>,
    ) -> eframe::NativeHostWake {
        self.inner.on_output_pass_finalized(finalization)
    }

    fn on_output(&self, result: eframe::NativeOutputResult) -> eframe::NativeHostWake {
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
}

impl eframe::App for PhysicalSmokeApp {
    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        let started = self.measure_pointer_load.then(Instant::now);
        <NativeDockspaceApp<SmokePanes> as eframe::App>::ui(&mut self.dockspace, ui, frame);
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
        self.publish_observation();
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
    fn publish_observation(&mut self) {
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
        let center_merge_verified = redocked
            && self.dockspace.with_view(|view| {
                view.item(MAIN_ITEM)
                    .zip(view.item(CHILD_ITEM))
                    .is_some_and(|(main, child)| {
                        main.surface() == ROOT_SURFACE
                            && child.surface() == ROOT_SURFACE
                            && main.root() == MAIN_ROOT
                            && child.root() == MAIN_ROOT
                            && main.contained().is_none()
                            && child.contained().is_none()
                            && main.tabs().items() == [MAIN_ITEM, CHILD_ITEM]
                            && child.tabs().items() == [MAIN_ITEM, CHILD_ITEM]
                            && main.tab_index() == 0
                            && child.tab_index() == 1
                    })
            }) == Some(true);
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
        };
        let fatal_error = self
            .dockspace
            .error_kind()
            .map(|kind| format!("native runtime entered fatal state: {kind:?}"));
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
        let (lock, changed) = &*self.shared;
        let mut shared = lock.lock().unwrap_or_else(PoisonError::into_inner);
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
                        body: "Root docking surface",
                    },
                ),
                (
                    CHILD_ITEM,
                    SmokePane {
                        title: "Child",
                        body: "Physical drag payload",
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
        if item == MAIN_ITEM {
            publish_root_pane_content_diagnostic(ui);
            publish_root_target_diagnostic(ui);
            ui.label(pane.body);
        } else {
            ui.heading(pane.title);
            ui.label(pane.body);
        }
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
