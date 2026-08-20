//! Physical docking scenario state machine and its causal barriers.

use std::net::SocketAddr;
use std::sync::PoisonError;
use std::time::{Duration, Instant};

#[cfg(test)]
use std::sync::{Arc, Condvar, Mutex};

use eframe::egui::accesskit::TreeUpdate;
use eframe::egui::{self, ViewportId};
use egui_inspection::{ViewportClass, ViewportDescriptor};

#[cfg(test)]
use super::super::app::SharedState;
use super::super::app::{
    CENTER_DROP_GUIDE_LABEL, CHILD_CLOSE_LABEL, CHILD_TAB_LABEL, PointerPassthroughObservation,
    ProductObservation, ProductPhase, ROOT_TARGET_LABEL, ROOT_WINDOW_TITLE, Shared,
};
use super::super::assertions::{
    LogicalBounds, PhysicalBounds, PhysicalPoint, SemanticLookupError, SemanticSelector,
    SemanticStabilitySample, SemanticText, choose_outside_point, desktop_to_viewport_logical,
    native_drag_preview_observation, point_towards, pointer_barrier_is_satisfied,
    pointer_observation_for_barrier, semantic_baseline_after_missing, semantic_bounds,
    semantic_node, semantic_snapshot_is_stable, semantic_tree_summary, tab_drag_point,
    two_stage_ingress_points, viewport_logical_to_desktop,
};
use super::inspection::{InspectionClient, ViewportTreeSnapshot, viewport_retirement_is_observed};
use super::x11::{DestroyWatcher, WindowRecord, X11Desktop, x11_window_is_retired};

fn bounded_stage_deadline(overall: Instant, budget: Duration) -> Instant {
    overall.min(Instant::now() + budget)
}

fn wait_for_product(
    shared: &Shared,
    deadline: Instant,
    stage: &str,
    ready: impl Fn(ProductObservation) -> bool,
) -> Result<ProductObservation, String> {
    let (lock, changed) = &**shared;
    let mut guard = lock.lock().unwrap_or_else(PoisonError::into_inner);
    loop {
        if let Some(error) = &guard.fatal_error {
            return Err(format!("{stage}: {error}"));
        }
        if ready(guard.observation) {
            return Ok(guard.observation);
        }
        let now = Instant::now();
        if now >= deadline {
            return Err(format!(
                "{stage}: timed out; last product observation was {:?}",
                guard.observation
            ));
        }
        let (next, timeout) = changed
            .wait_timeout(guard, deadline.saturating_duration_since(now))
            .unwrap_or_else(PoisonError::into_inner);
        guard = next;
        if timeout.timed_out() && !ready(guard.observation) {
            return Err(format!(
                "{stage}: timed out; last product observation was {:?}",
                guard.observation
            ));
        }
    }
}

fn pointer_passthrough_checkpoint(shared: &Shared) -> u64 {
    let (lock, _) = &**shared;
    lock.lock()
        .unwrap_or_else(PoisonError::into_inner)
        .pointer_passthrough_sequence
}

fn wait_for_pointer_passthrough(
    shared: &Shared,
    after_sequence: u64,
    viewport: ViewportId,
    enabled: bool,
    overall_deadline: Instant,
    stage: &str,
) -> Result<PointerPassthroughObservation, String> {
    let deadline = bounded_stage_deadline(overall_deadline, Duration::from_secs(5));
    let (lock, changed) = &**shared;
    let mut guard = lock.lock().unwrap_or_else(PoisonError::into_inner);
    loop {
        if let Some(error) = &guard.fatal_error {
            return Err(format!("{stage}: {error}"));
        }
        if let Some(observation) = guard
            .pointer_passthrough
            .iter()
            .copied()
            .find(|observation| {
                observation.sequence > after_sequence
                    && observation.viewport == viewport
                    && observation.enabled == enabled
            })
        {
            return match observation.status {
                eframe::NativeViewportPointerPassthroughStatus::Applied => {
                    eprintln!(
                        "physical-smoke: {stage}: sequence={} viewport={viewport:?} enabled={enabled}",
                        observation.sequence
                    );
                    Ok(observation)
                }
                eframe::NativeViewportPointerPassthroughStatus::Unsupported
                | eframe::NativeViewportPointerPassthroughStatus::Failed => Err(format!(
                    "{stage}: native pointer pass-through was not applied: {observation:?}"
                )),
            };
        }
        let now = Instant::now();
        if now >= deadline {
            let newer = guard
                .pointer_passthrough
                .iter()
                .copied()
                .filter(|observation| observation.sequence > after_sequence)
                .collect::<Vec<_>>();
            return Err(format!(
                "{stage}: timed out waiting for viewport={viewport:?} enabled={enabled}; after_sequence={after_sequence}, newer_callbacks={newer:?}"
            ));
        }
        let (next, _) = changed
            .wait_timeout(guard, deadline.saturating_duration_since(now))
            .unwrap_or_else(PoisonError::into_inner);
        guard = next;
    }
}

fn selector_bounds(
    snapshot: &ViewportTreeSnapshot,
    selector: SemanticSelector<'_>,
) -> Result<LogicalBounds, String> {
    semantic_bounds(&snapshot.tree, selector).map_err(|error| {
        format!(
            "viewport {:?} step {} missing semantic selector {selector:?}: {error:?}",
            snapshot.viewport, snapshot.step
        )
    })
}

fn optional_selector_bounds(
    snapshot: &ViewportTreeSnapshot,
    selector: SemanticSelector<'_>,
) -> Result<Option<LogicalBounds>, String> {
    match semantic_bounds(&snapshot.tree, selector) {
        Ok(bounds) => Ok(Some(bounds)),
        Err(SemanticLookupError::Missing) => Ok(None),
        Err(error) => Err(format!(
            "viewport {:?} step {} invalid optional selector {selector:?}: {error:?}",
            snapshot.viewport, snapshot.step
        )),
    }
}

fn wait_for_stable_semantic_bounds(
    shared: &Shared,
    inspection: &mut InspectionClient,
    viewport: ViewportId,
    description: &str,
    deadline: Instant,
    stage: &str,
    mut lookup: impl FnMut(&TreeUpdate) -> Result<LogicalBounds, SemanticLookupError>,
) -> Result<(ViewportTreeSnapshot, LogicalBounds), String> {
    let mut attempts = 0_u32;
    let mut previous = None;
    let mut last_step = None;
    let mut last_roster_epoch = 0_u64;
    let mut last_lookup = "not attempted";
    let mut last_tree = None;
    while Instant::now() < deadline {
        attempts = attempts.saturating_add(1);
        let snapshot = inspection.tree(viewport, deadline)?;
        let roster_epoch = {
            let (lock, _) = &**shared;
            let shared = lock.lock().unwrap_or_else(PoisonError::into_inner);
            shared.roster_epoch
        };
        last_step = Some(snapshot.step);
        last_roster_epoch = roster_epoch;
        match lookup(&snapshot.tree) {
            Ok(bounds) => {
                let sample = SemanticStabilitySample {
                    step: snapshot.step,
                    roster_changes: roster_epoch,
                    bounds,
                };
                if semantic_snapshot_is_stable(previous, sample) {
                    eprintln!(
                        "physical-smoke: {stage}: viewport={viewport:?} step={} attempts={attempts} roster-epoch={roster_epoch} bounds={bounds:?}",
                        snapshot.step
                    );
                    return Ok((snapshot, bounds));
                }
                previous = Some(sample);
                last_lookup = "found bounds, awaiting stable successor";
            }
            Err(SemanticLookupError::Missing) => {
                previous = semantic_baseline_after_missing(previous, roster_epoch);
                last_lookup = "Missing";
            }
            Err(error) => {
                return Err(format!(
                    "{stage}: semantic selector {description} failed closed at viewport={viewport:?} step={}: {error:?}; tree={}",
                    snapshot.step,
                    semantic_tree_summary(&snapshot.tree)
                ));
            }
        }
        last_tree = Some(snapshot.tree);
    }
    let tree_summary = last_tree
        .as_ref()
        .map_or_else(|| "no tree observed".to_owned(), semantic_tree_summary);
    Err(format!(
        "{stage}: semantic selector {description} did not stabilize before the deadline; attempts={attempts}, last_step={last_step:?}, roster_epoch={last_roster_epoch}, last_lookup={last_lookup}, tree={tree_summary}"
    ))
}

fn wait_for_stable_selector_bounds(
    shared: &Shared,
    inspection: &mut InspectionClient,
    viewport: ViewportId,
    selector: SemanticSelector<'_>,
    deadline: Instant,
    stage: &str,
) -> Result<(ViewportTreeSnapshot, LogicalBounds), String> {
    wait_for_stable_semantic_bounds(
        shared,
        inspection,
        viewport,
        &format!("{selector:?}"),
        deadline,
        stage,
        |tree| semantic_bounds(tree, selector),
    )
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ScenarioStage {
    ValidateX11Environment,
    AwaitRootPresentation,
    DiscoverRootDragCoordinates,
    DragOutX11Events,
    AwaitChildFirstLive,
    DiscoverChildRedockCoordinates,
    DragBackX11Events,
    AwaitRedock,
    AwaitRetirementQuiescence,
    VerifyRetiredX11Window,
    VerifyRetiredViewportInventory,
    Complete,
}

impl ScenarioStage {
    const fn name(self) -> &'static str {
        match self {
            Self::ValidateX11Environment => "validate-x11-environment",
            Self::AwaitRootPresentation => "await-root-presentation",
            Self::DiscoverRootDragCoordinates => "discover-root-drag-coordinates",
            Self::DragOutX11Events => "drag-out-x11-events",
            Self::AwaitChildFirstLive => "await-child-first-live",
            Self::DiscoverChildRedockCoordinates => "discover-child-redock-coordinates",
            Self::DragBackX11Events => "drag-back-x11-events",
            Self::AwaitRedock => "await-redock",
            Self::AwaitRetirementQuiescence => "await-retirement-quiescence",
            Self::VerifyRetiredX11Window => "verify-retired-x11-window",
            Self::VerifyRetiredViewportInventory => "verify-retired-viewport-inventory",
            Self::Complete => "complete",
        }
    }
}

struct PhysicalDriver {
    shared: Shared,
    inspection: InspectionClient,
    desktop: X11Desktop,
    deadline: Instant,
    work_area: Option<PhysicalBounds>,
    stage: ScenarioStage,
}

pub(crate) fn run(shared: Shared, inspection_endpoint: SocketAddr) -> Result<(), String> {
    let deadline = Instant::now() + Duration::from_secs(35);
    let mut driver = PhysicalDriver::new(shared, inspection_endpoint, deadline)?;
    driver
        .run_inner()
        .map_err(|error| format!("stage={}: {error}", driver.stage.name()))
}

impl PhysicalDriver {
    fn new(
        shared: Shared,
        inspection_endpoint: SocketAddr,
        deadline: Instant,
    ) -> Result<Self, String> {
        Ok(Self {
            shared,
            inspection: InspectionClient::connect(inspection_endpoint, deadline)?,
            desktop: X11Desktop::connect()?,
            deadline,
            work_area: None,
            stage: ScenarioStage::ValidateX11Environment,
        })
    }

    fn set_stage(&mut self, stage: ScenarioStage) {
        self.stage = stage;
        eprintln!("physical-smoke: stage={}", stage.name());
    }

    fn run_inner(&mut self) -> Result<(), String> {
        self.set_stage(ScenarioStage::ValidateX11Environment);
        self.set_stage(ScenarioStage::AwaitRootPresentation);
        wait_for_product(
            &self.shared,
            self.deadline,
            self.stage.name(),
            |observation| observation.phase != ProductPhase::AwaitingRoot,
        )?;
        self.work_area = Some(wait_for_work_area(&self.shared, self.deadline)?);

        let drag_out = self.perform_drag_out()?;
        eprintln!(
            "physical-smoke: drag-out preview step {} preceded release step {}",
            drag_out.preview.step, drag_out.released.step
        );

        self.set_stage(ScenarioStage::AwaitChildFirstLive);
        wait_for_product(
            &self.shared,
            self.deadline,
            self.stage.name(),
            |observation| {
                matches!(
                    observation.phase,
                    ProductPhase::ChildFirstLive | ProductPhase::Redocked | ProductPhase::Retired
                )
            },
        )?;
        let (retirement, drag_back) = self.perform_drag_back()?;
        eprintln!(
            "physical-smoke: drag-back preview step {} preceded release step {}",
            drag_back.preview.step, drag_back.released.step
        );

        self.set_stage(ScenarioStage::AwaitRedock);
        wait_for_product(
            &self.shared,
            self.deadline,
            self.stage.name(),
            |observation| {
                matches!(
                    observation.phase,
                    ProductPhase::Redocked | ProductPhase::Retired
                )
            },
        )?;
        self.set_stage(ScenarioStage::AwaitRetirementQuiescence);
        wait_for_product(
            &self.shared,
            self.deadline,
            self.stage.name(),
            |observation| observation.phase == ProductPhase::Retired && observation.quiescent,
        )?;
        self.verify_retired_viewport(retirement)?;
        self.set_stage(ScenarioStage::Complete);
        Ok(())
    }
}

fn wait_for_work_area(shared: &Shared, deadline: Instant) -> Result<PhysicalBounds, String> {
    let (lock, changed) = &**shared;
    let mut guard = lock.lock().unwrap_or_else(PoisonError::into_inner);
    loop {
        if let Some(error) = &guard.fatal_error {
            return Err(error.clone());
        }
        if let Some(work_area) = &guard.work_area {
            return work_area.clone();
        }
        let now = Instant::now();
        if now >= deadline {
            return Err("native root roster did not publish an exact X11 work area".to_owned());
        }
        let (next, timeout) = changed
            .wait_timeout(guard, deadline.saturating_duration_since(now))
            .unwrap_or_else(PoisonError::into_inner);
        guard = next;
        if timeout.timed_out() && guard.work_area.is_none() {
            return Err("native root roster did not publish an exact X11 work area".to_owned());
        }
    }
}

impl PhysicalDriver {
    fn perform_drag_out(&mut self) -> Result<DragResult, String> {
        self.set_stage(ScenarioStage::DiscoverRootDragCoordinates);
        let semantic_deadline = self.deadline.min(Instant::now() + Duration::from_secs(5));
        let viewports = self.inspection.viewports(semantic_deadline)?;
        let root = root_viewport(&viewports)?;
        let selector = SemanticSelector {
            role: egui::accesskit::Role::Tab,
            text: SemanticText::Label(CHILD_TAB_LABEL),
        };
        let (root_tree, source_tab) = wait_for_stable_selector_bounds(
            &self.shared,
            &mut self.inspection,
            root.id,
            selector,
            semantic_deadline,
            "await-stable-root-child-tab",
        )?;
        let root_window = self.desktop.window_for_title(ROOT_WINDOW_TITLE)?;
        let source_close = optional_selector_bounds(
            &root_tree,
            SemanticSelector {
                role: egui::accesskit::Role::Button,
                text: SemanticText::Label(CHILD_CLOSE_LABEL),
            },
        )?;
        let source_logical = tab_drag_point(source_tab, source_close).ok_or_else(|| {
            "Child tab has no semantic drag point outside its close control".to_owned()
        })?;
        let source = viewport_logical_to_desktop(
            root_window.bounds,
            root_tree.pixels_per_point,
            source_logical,
        )
        .map_err(|error| format!("translate root Child tab to X11 desktop: {error:?}"))?;
        let work_area = self
            .work_area
            .ok_or_else(|| "exact native work area was not frozen".to_owned())?;
        let process_windows = self.desktop.process_windows()?;
        let outside = choose_outside_point(
            work_area,
            process_windows.iter().map(|window| window.bounds),
        )
        .ok_or_else(|| {
            format!(
                "no free semantic outside-all point exists in work area {work_area:?}; windows={:?}",
                process_windows
                    .iter()
                    .map(|window| (&window.title, window.bounds))
                    .collect::<Vec<_>>()
            )
        })?;
        let threshold_distance = ((source_tab.max_y - source_tab.min_y)
            * f64::from(root_tree.pixels_per_point))
        .max(16.0);
        let threshold = point_towards(source, outside, threshold_distance);
        eprintln!(
            "physical-smoke: root-window={:?} source={source:?} threshold={threshold:?} outside={outside:?} work-area={work_area:?}",
            root_window.bounds
        );

        self.set_stage(ScenarioStage::DragOutX11Events);
        inject_drag(
            &self.shared,
            &mut self.inspection,
            &self.desktop,
            DragRequest {
                label: "drag-out",
                source_viewport: root.id,
                source_step: root_tree.step,
                source_client: root_window.bounds,
                source_pixels_per_point: root_tree.pixels_per_point,
                preview_viewport: root.id,
                preview_step: root_tree.step,
                source,
                threshold,
                target: outside,
                deadline: self.deadline,
            },
        )
    }

    fn perform_drag_back(&mut self) -> Result<(RetirementTarget, DragResult), String> {
        self.set_stage(ScenarioStage::DiscoverChildRedockCoordinates);
        let semantic_deadline = self.deadline.min(Instant::now() + Duration::from_secs(5));
        let viewports = self.inspection.viewports(semantic_deadline)?;
        let (child, child_tree, child_tab) = child_viewport(
            &self.shared,
            &mut self.inspection,
            &viewports,
            semantic_deadline,
        )?;
        let child_title = viewport_window_title(&child)?.to_owned();
        let child_window = self.desktop.window_for_title(&child_title)?;
        let watcher = self.desktop.watch_destroy(child_window.window)?;
        let child_close = optional_selector_bounds(
            &child_tree,
            SemanticSelector {
                role: egui::accesskit::Role::Button,
                text: SemanticText::Label(CHILD_CLOSE_LABEL),
            },
        )?;
        let child_source_logical = tab_drag_point(child_tab, child_close)
            .ok_or_else(|| "native Child tab has no semantic drag point".to_owned())?;
        let child_source = viewport_logical_to_desktop(
            child_window.bounds,
            child_tree.pixels_per_point,
            child_source_logical,
        )
        .map_err(|error| format!("translate native Child tab to X11 desktop: {error:?}"))?;

        let semantic_deadline = self.deadline.min(Instant::now() + Duration::from_secs(5));
        let refreshed = self.inspection.viewports(semantic_deadline)?;
        let refreshed_root = root_viewport(&refreshed)?;
        let (root_tree, root_target) = wait_for_stable_root_target_bounds(
            &self.shared,
            &mut self.inspection,
            refreshed_root.id,
            semantic_deadline,
        )?;
        let refreshed_root_window = self.desktop.window_for_title(ROOT_WINDOW_TITLE)?;
        let (ingress_logical, routed_ingress_logical) = two_stage_ingress_points(root_target);
        let ingress = viewport_logical_to_desktop(
            refreshed_root_window.bounds,
            root_tree.pixels_per_point,
            ingress_logical,
        )
        .map_err(|error| format!("translate root docking marker to X11 desktop: {error:?}"))?;
        let routed_ingress = viewport_logical_to_desktop(
            refreshed_root_window.bounds,
            root_tree.pixels_per_point,
            routed_ingress_logical,
        )
        .map_err(|error| {
            format!("translate routed root docking marker to X11 desktop: {error:?}")
        })?;
        if ingress == routed_ingress {
            return Err(format!(
                "root docking semantic target {root_target:?} projects both route stages to {ingress:?}"
            ));
        }
        let child_threshold_distance = ((child_tab.max_y - child_tab.min_y)
            * f64::from(child_tree.pixels_per_point))
        .max(16.0);
        let child_threshold = point_towards(child_source, ingress, child_threshold_distance);
        eprintln!(
            "physical-smoke: child-window={:?} source={child_source:?} threshold={child_threshold:?} root-ingress={ingress:?} routed-root-ingress={routed_ingress:?}",
            child_window.bounds
        );

        self.set_stage(ScenarioStage::DragBackX11Events);
        let drag_back = inject_drag_to_center_guide(
            &self.shared,
            &mut self.inspection,
            &self.desktop,
            GuideDragRequest {
                label: "drag-back",
                source_viewport: child.id,
                source_step: child_tree.step,
                source_client: child_window.bounds,
                source_pixels_per_point: child_tree.pixels_per_point,
                target_viewport: refreshed_root.id,
                target_window_title: ROOT_WINDOW_TITLE,
                source: child_source,
                threshold: child_threshold,
                ingress,
                routed_ingress,
                deadline: self.deadline,
            },
        )?;
        Ok((
            RetirementTarget {
                viewport: child.id,
                window: child_window,
                watcher,
            },
            drag_back,
        ))
    }

    fn verify_retired_viewport(&mut self, retirement: RetirementTarget) -> Result<(), String> {
        self.set_stage(ScenarioStage::VerifyRetiredX11Window);
        retirement.watcher.wait(self.deadline)?;
        let windows = self.desktop.process_windows()?;
        if !x11_window_is_retired(&windows, &retirement.window)? {
            return Err(format!(
                "exact DestroyNotify arrived but retired child remained in the one-shot EWMH inventory: child={:?}, windows={:?}",
                retirement.window.title,
                windows
                    .iter()
                    .map(|window| (window.window, &window.title, window.bounds))
                    .collect::<Vec<_>>()
            ));
        }

        self.set_stage(ScenarioStage::VerifyRetiredViewportInventory);
        let viewports = self.inspection.viewports(self.deadline)?;
        let listed = viewports
            .iter()
            .any(|viewport| viewport.id == retirement.viewport);
        let availability = self
            .inspection
            .tree_availability(retirement.viewport, self.deadline)?;
        if !viewport_retirement_is_observed(listed, availability) {
            return Err(format!(
                "retired child viewport {:?} remained in semantic inspection after its X11 window disappeared; availability={availability:?}, viewports={viewports:?}",
                retirement.viewport
            ));
        }
        eprintln!(
            "physical-smoke: retired semantic viewport barrier viewport={:?} availability={availability:?}",
            retirement.viewport
        );
        let final_root = root_viewport(&viewports)?;
        let final_root_tree = self.inspection.tree(final_root.id, self.deadline)?;
        selector_bounds(
            &final_root_tree,
            SemanticSelector {
                role: egui::accesskit::Role::Tab,
                text: SemanticText::Label(CHILD_TAB_LABEL),
            },
        )?;
        Ok(())
    }
}

struct RetirementTarget {
    viewport: ViewportId,
    window: WindowRecord,
    watcher: DestroyWatcher,
}

#[derive(Clone, Copy)]
struct DragRequest<'a> {
    label: &'a str,
    source_viewport: ViewportId,
    source_step: u64,
    source_client: PhysicalBounds,
    source_pixels_per_point: f32,
    preview_viewport: ViewportId,
    preview_step: u64,
    source: PhysicalPoint,
    threshold: PhysicalPoint,
    target: PhysicalPoint,
    deadline: Instant,
}

struct DragResult {
    preview: ViewportTreeSnapshot,
    released: ViewportTreeSnapshot,
}

#[derive(Clone, Copy)]
struct GuideDragRequest<'a> {
    label: &'a str,
    source_viewport: ViewportId,
    source_step: u64,
    source_client: PhysicalBounds,
    source_pixels_per_point: f32,
    target_viewport: ViewportId,
    target_window_title: &'a str,
    source: PhysicalPoint,
    threshold: PhysicalPoint,
    ingress: PhysicalPoint,
    routed_ingress: PhysicalPoint,
    deadline: Instant,
}

#[allow(clippy::too_many_lines)]
fn inject_drag(
    shared: &Shared,
    inspection: &mut InspectionClient,
    desktop: &X11Desktop,
    request: DragRequest<'_>,
) -> Result<DragResult, String> {
    desktop.move_pointer(request.source)?;
    let source_at_hover = input_barrier(
        shared,
        inspection,
        request.source_viewport,
        request.source_step,
        request.source_client,
        request.source_pixels_per_point,
        request.source,
        false,
        request.deadline,
        &format!("{} source hover", request.label),
    )?;
    press_primary_with_cleanup(desktop, request.label)?;
    let pressed = (|| {
        let source_at_press = input_barrier(
            shared,
            inspection,
            request.source_viewport,
            source_at_hover.step,
            request.source_client,
            request.source_pixels_per_point,
            request.source,
            true,
            request.deadline,
            &format!("{} source press", request.label),
        )?;
        let route_checkpoint = pointer_passthrough_checkpoint(shared);
        desktop.move_pointer(request.threshold)?;
        desktop.assert_pointer(request.threshold, Some(true))?;
        let source_at_threshold = input_barrier(
            shared,
            inspection,
            request.source_viewport,
            source_at_press.step,
            request.source_client,
            request.source_pixels_per_point,
            request.threshold,
            true,
            request.deadline,
            &format!("{} drag threshold", request.label),
        )?;
        desktop.move_pointer(request.target)?;
        desktop.assert_pointer(request.target, Some(true))?;
        let source_at_first_target = input_barrier(
            shared,
            inspection,
            request.source_viewport,
            source_at_threshold.step,
            request.source_client,
            request.source_pixels_per_point,
            request.target,
            true,
            request.deadline,
            &format!("{} first target ingress", request.label),
        )?;
        wait_for_pointer_passthrough(
            shared,
            route_checkpoint,
            request.source_viewport,
            true,
            request.deadline,
            &format!("{} drag-route enable", request.label),
        )?;
        let route_confirmed = viewport_barrier(
            inspection,
            ViewportId::ROOT,
            source_at_first_target.step,
            request.deadline,
            &format!("{} drag-route confirmation", request.label),
        )?;
        desktop.move_pointer(request.target)?;
        desktop.assert_pointer(request.target, Some(true))?;
        let source_barrier_step = if request.source_viewport == ViewportId::ROOT {
            source_at_first_target.step.max(route_confirmed.step)
        } else {
            source_at_first_target.step
        };
        let source_at_routed_target = input_barrier(
            shared,
            inspection,
            request.source_viewport,
            source_barrier_step,
            request.source_client,
            request.source_pixels_per_point,
            request.target,
            true,
            request.deadline,
            &format!("{} routed target input", request.label),
        )?;
        if request.preview_viewport == request.source_viewport {
            native_drag_preview_barrier(
                shared,
                inspection,
                source_at_routed_target,
                request.deadline,
                &format!("{} production native preview", request.label),
            )
        } else {
            viewport_barrier(
                inspection,
                request.preview_viewport,
                route_confirmed.step.max(request.preview_step),
                request.deadline,
                &format!("{} target preview", request.label),
            )
        }
    })();
    let release = desktop.set_primary_button(false);
    let preview = match (pressed, release) {
        (Ok(preview), Ok(())) => preview,
        (Err(error), Ok(())) | (Ok(_), Err(error)) => return Err(error),
        (Err(error), Err(release)) => {
            return Err(format!("{error}; release cleanup also failed: {release}"));
        }
    };
    desktop.assert_pointer(request.target, Some(false))?;
    let released = viewport_barrier(
        inspection,
        request.preview_viewport,
        preview.step,
        request.deadline,
        &format!("{} physical release", request.label),
    )?;
    Ok(DragResult { preview, released })
}

fn native_drag_preview_barrier(
    shared: &Shared,
    inspection: &mut InspectionClient,
    mut snapshot: ViewportTreeSnapshot,
    overall_deadline: Instant,
    stage: &str,
) -> Result<ViewportTreeSnapshot, String> {
    let deadline = bounded_stage_deadline(overall_deadline, Duration::from_secs(3));
    let mut attempts = 0_u32;
    loop {
        if let Some(error) = shared_fatal_error(shared) {
            return Err(format!("{stage}: {error}"));
        }
        attempts = attempts.saturating_add(1);
        match native_drag_preview_observation(&snapshot.tree) {
            Ok(true) => {
                eprintln!(
                    "physical-smoke: {stage}: viewport={:?} step={} attempts={attempts}",
                    snapshot.viewport, snapshot.step,
                );
                return Ok(snapshot);
            }
            Ok(false) => {}
            Err(error) => {
                return Err(format!(
                    "{stage}: invalid production preview diagnostic at viewport={:?} step={}: {error}; tree={}",
                    snapshot.viewport,
                    snapshot.step,
                    semantic_tree_summary(&snapshot.tree),
                ));
            }
        }
        if Instant::now() >= deadline {
            return Err(format!(
                "{stage}: production renderer did not publish a native preview before release; last_step={} attempts={attempts} tree={}",
                snapshot.step,
                semantic_tree_summary(&snapshot.tree),
            ));
        }
        snapshot = inspection.tree(snapshot.viewport, deadline)?;
    }
}

fn inject_drag_to_center_guide(
    shared: &Shared,
    inspection: &mut InspectionClient,
    desktop: &X11Desktop,
    request: GuideDragRequest<'_>,
) -> Result<DragResult, String> {
    desktop.move_pointer(request.source)?;
    let source_at_hover = input_barrier(
        shared,
        inspection,
        request.source_viewport,
        request.source_step,
        request.source_client,
        request.source_pixels_per_point,
        request.source,
        false,
        request.deadline,
        &format!("{} source hover", request.label),
    )?;
    press_primary_with_cleanup(desktop, request.label)?;
    let pressed = (|| {
        let (source_ingress_step, ingress_tree) = reach_target_surface_while_pressed(
            shared,
            inspection,
            desktop,
            request,
            source_at_hover.step,
        )?;
        move_to_center_guide_while_pressed(
            shared,
            inspection,
            desktop,
            request,
            source_ingress_step,
            &ingress_tree,
        )
    })();
    let release = desktop.set_primary_button(false);
    let (preview, guide) = match (pressed, release) {
        (Ok(preview), Ok(())) => preview,
        (Err(error), Ok(())) | (Ok(_), Err(error)) => return Err(error),
        (Err(error), Err(release)) => {
            return Err(format!("{error}; release cleanup also failed: {release}"));
        }
    };
    desktop.assert_pointer(guide, Some(false))?;
    let released = viewport_barrier(
        inspection,
        request.target_viewport,
        preview.step,
        request.deadline,
        &format!("{} physical release", request.label),
    )?;
    Ok(DragResult { preview, released })
}

fn press_primary_with_cleanup(desktop: &X11Desktop, label: &str) -> Result<(), String> {
    if let Err(press) = desktop.set_primary_button(true) {
        return match desktop.set_primary_button(false) {
            Ok(()) => Err(press),
            Err(release) => Err(format!(
                "{label} primary press failed: {press}; release cleanup also failed: {release}"
            )),
        };
    }
    Ok(())
}

fn reach_target_surface_while_pressed(
    shared: &Shared,
    inspection: &mut InspectionClient,
    desktop: &X11Desktop,
    request: GuideDragRequest<'_>,
    source_hover_step: u64,
) -> Result<(u64, ViewportTreeSnapshot), String> {
    let source_at_press = input_barrier(
        shared,
        inspection,
        request.source_viewport,
        source_hover_step,
        request.source_client,
        request.source_pixels_per_point,
        request.source,
        true,
        request.deadline,
        &format!("{} source press", request.label),
    )?;
    let root_before_route = root_step_before_drag_route(inspection, request, source_at_press.step)?;
    let route_checkpoint = pointer_passthrough_checkpoint(shared);
    desktop.move_pointer(request.threshold)?;
    desktop.assert_pointer(request.threshold, Some(true))?;
    let source_at_threshold = input_barrier(
        shared,
        inspection,
        request.source_viewport,
        source_at_press.step,
        request.source_client,
        request.source_pixels_per_point,
        request.threshold,
        true,
        request.deadline,
        &format!("{} drag threshold", request.label),
    )?;
    desktop.move_pointer(request.ingress)?;
    desktop.assert_pointer(request.ingress, Some(true))?;
    let source_at_first_ingress = input_barrier(
        shared,
        inspection,
        request.source_viewport,
        source_at_threshold.step,
        request.source_client,
        request.source_pixels_per_point,
        request.ingress,
        true,
        request.deadline,
        &format!("{} first target-surface ingress", request.label),
    )?;
    wait_for_pointer_passthrough(
        shared,
        route_checkpoint,
        request.source_viewport,
        true,
        request.deadline,
        &format!("{} drag-route enable", request.label),
    )?;
    let root_barrier_step = if request.source_viewport == ViewportId::ROOT {
        source_at_first_ingress.step
    } else {
        root_before_route
    };
    let route_confirmed = viewport_barrier(
        inspection,
        ViewportId::ROOT,
        root_barrier_step,
        request.deadline,
        &format!("{} drag-route confirmation", request.label),
    )?;

    desktop.move_pointer(request.routed_ingress)?;
    desktop.assert_pointer(request.routed_ingress, Some(true))?;
    let source_barrier_step = if request.source_viewport == ViewportId::ROOT {
        source_at_first_ingress.step.max(route_confirmed.step)
    } else {
        source_at_first_ingress.step
    };
    let source_at_routed_ingress = input_barrier(
        shared,
        inspection,
        request.source_viewport,
        source_barrier_step,
        request.source_client,
        request.source_pixels_per_point,
        request.routed_ingress,
        true,
        request.deadline,
        &format!("{} routed target-surface ingress input", request.label),
    )?;
    let guide_deadline = bounded_stage_deadline(request.deadline, Duration::from_secs(3));
    let (ingress_tree, _) = wait_for_stable_selector_bounds(
        shared,
        inspection,
        request.target_viewport,
        SemanticSelector {
            role: egui::accesskit::Role::Label,
            text: SemanticText::Label(CENTER_DROP_GUIDE_LABEL),
        },
        guide_deadline,
        &format!("{} stable Center guide after target ingress", request.label),
    )?;
    center_guide_bounds(&ingress_tree)?;
    Ok((source_at_routed_ingress.step, ingress_tree))
}

fn root_step_before_drag_route(
    inspection: &mut InspectionClient,
    request: GuideDragRequest<'_>,
    source_step: u64,
) -> Result<u64, String> {
    if request.source_viewport == ViewportId::ROOT {
        return Ok(source_step);
    }
    inspection
        .tree(
            ViewportId::ROOT,
            bounded_stage_deadline(request.deadline, Duration::from_secs(3)),
        )
        .map(|snapshot| snapshot.step)
}

fn move_to_center_guide_while_pressed(
    shared: &Shared,
    inspection: &mut InspectionClient,
    desktop: &X11Desktop,
    request: GuideDragRequest<'_>,
    source_ingress_step: u64,
    ingress_tree: &ViewportTreeSnapshot,
) -> Result<(ViewportTreeSnapshot, PhysicalPoint), String> {
    let guide_bounds = center_guide_bounds(ingress_tree)?;
    let target_window = desktop.window_for_title(request.target_window_title)?;
    let ingress_logical = desktop_to_viewport_logical(
        target_window.bounds,
        ingress_tree.pixels_per_point,
        request.routed_ingress,
    )
    .map_err(|error| {
        format!(
            "{} translate target-surface ingress into target viewport: {error:?}",
            request.label
        )
    })?;
    if guide_bounds.contains(ingress_logical) {
        return Err(format!(
            "{} semantic ingress {ingress_logical:?} already falls inside exact Center guide {guide_bounds:?}; two-stage drag proof is not independent",
            request.label
        ));
    }
    let guide = viewport_logical_to_desktop(
        target_window.bounds,
        ingress_tree.pixels_per_point,
        guide_bounds.center(),
    )
    .map_err(|error| {
        format!(
            "{} translate exact Center guide to X11 desktop: {error:?}",
            request.label
        )
    })?;
    if !target_window.bounds.contains(guide) {
        return Err(format!(
            "{} exact Center guide point {guide:?} falls outside target client {:?}",
            request.label, target_window.bounds
        ));
    }
    eprintln!(
        "physical-smoke: exact Center guide logical={guide_bounds:?} physical={guide:?} target-window={:?}",
        target_window.bounds
    );

    desktop.move_pointer(guide)?;
    desktop.assert_pointer(guide, Some(true))?;
    let _source_at_guide = input_barrier(
        shared,
        inspection,
        request.source_viewport,
        source_ingress_step,
        request.source_client,
        request.source_pixels_per_point,
        guide,
        true,
        request.deadline,
        &format!("{} exact Center guide input", request.label),
    )?;
    let guide_deadline = bounded_stage_deadline(request.deadline, Duration::from_secs(3));
    let (preview, _) = wait_for_stable_selector_bounds(
        shared,
        inspection,
        request.target_viewport,
        SemanticSelector {
            role: egui::accesskit::Role::Label,
            text: SemanticText::Label(CENTER_DROP_GUIDE_LABEL),
        },
        guide_deadline,
        &format!("{} stable exact Center guide after input", request.label),
    )?;
    if preview.step <= ingress_tree.step {
        return Err(format!(
            "{} exact Center guide did not cross the target tree barrier; before={}, after={}",
            request.label, ingress_tree.step, preview.step
        ));
    }
    verify_final_center_guide(desktop, request, &preview, guide)?;
    Ok((preview, guide))
}

fn verify_final_center_guide(
    desktop: &X11Desktop,
    request: GuideDragRequest<'_>,
    preview: &ViewportTreeSnapshot,
    selected_guide: PhysicalPoint,
) -> Result<(), String> {
    let final_guide_bounds = center_guide_bounds(preview)?;
    let target_window = desktop.window_for_title(request.target_window_title)?;
    let final_guide = viewport_logical_to_desktop(
        target_window.bounds,
        preview.pixels_per_point,
        final_guide_bounds.center(),
    )
    .map_err(|error| {
        format!(
            "{} translate final Center guide to X11 desktop: {error:?}",
            request.label
        )
    })?;
    if final_guide != selected_guide {
        return Err(format!(
            "{} Center guide moved before release: selected={selected_guide:?}, final={final_guide:?}",
            request.label
        ));
    }
    let observed = desktop_to_viewport_logical(
        target_window.bounds,
        preview.pixels_per_point,
        selected_guide,
    )
    .map_err(|error| {
        format!(
            "{} translate final XTest pointer into target viewport: {error:?}",
            request.label
        )
    })?;
    if !final_guide_bounds.contains(observed) {
        return Err(format!(
            "{} final XTest pointer {observed:?} is outside exact Center guide {final_guide_bounds:?}",
            request.label
        ));
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn input_barrier(
    shared: &Shared,
    inspection: &mut InspectionClient,
    viewport: ViewportId,
    previous_step: u64,
    client: PhysicalBounds,
    pixels_per_point: f32,
    expected_physical: PhysicalPoint,
    expected_primary_down: bool,
    overall_deadline: Instant,
    stage: &str,
) -> Result<ViewportTreeSnapshot, String> {
    let expected = desktop_to_viewport_logical(client, pixels_per_point, expected_physical)
        .map_err(|error| format!("{stage}: derive expected semantic pointer: {error:?}"))?;
    let deadline = bounded_stage_deadline(overall_deadline, Duration::from_secs(3));
    let mut attempts = 0_u32;
    let mut last_step = previous_step;
    let mut last_observation = None;
    let mut last_lookup = "not attempted";
    let mut last_tree = None;
    while Instant::now() < deadline {
        if let Some(error) = shared_fatal_error(shared) {
            return Err(format!("{stage}: {error}"));
        }
        let snapshot = inspection.tree(viewport, deadline)?;
        attempts = attempts.saturating_add(1);
        last_step = snapshot.step;
        if snapshot.step <= previous_step {
            last_lookup = "tree step did not advance";
            last_tree = Some(snapshot.tree);
            continue;
        }
        match pointer_observation_for_barrier(&snapshot.tree) {
            Ok(Some(observed)) => {
                if pointer_barrier_is_satisfied(
                    snapshot.step,
                    previous_step,
                    observed,
                    expected,
                    expected_primary_down,
                    pixels_per_point,
                ) {
                    eprintln!(
                        "physical-smoke: {stage}: viewport={viewport:?} step={} attempts={attempts} observation={observed:?}",
                        snapshot.step,
                    );
                    return Ok(snapshot);
                }
                last_observation = Some(observed);
                last_lookup = "pointer diagnostic did not match";
            }
            Ok(None) => last_lookup = "Missing",
            Err(error) => {
                return Err(format!(
                    "{stage}: invalid AccessKit pointer diagnostic at viewport={viewport:?} step={}: {error}; tree={}",
                    snapshot.step,
                    semantic_tree_summary(&snapshot.tree)
                ));
            }
        }
        last_tree = Some(snapshot.tree);
    }
    let tree_summary = last_tree
        .as_ref()
        .map_or_else(|| "no tree observed".to_owned(), semantic_tree_summary);
    Err(format!(
        "{stage}: AccessKit pointer diagnostic did not cross the OS-event/tree barrier; previous_step={previous_step}, last_step={last_step}, expected_position={expected:?}, expected_primary_down={expected_primary_down}, last_observation={last_observation:?}, last_lookup={last_lookup}, attempts={attempts}, tree={tree_summary}"
    ))
}

fn shared_fatal_error(shared: &Shared) -> Option<String> {
    let (lock, _) = &**shared;
    lock.lock()
        .unwrap_or_else(PoisonError::into_inner)
        .fatal_error
        .clone()
}

fn viewport_barrier(
    inspection: &mut InspectionClient,
    viewport: ViewportId,
    previous_step: u64,
    overall_deadline: Instant,
    stage: &str,
) -> Result<ViewportTreeSnapshot, String> {
    let deadline = bounded_stage_deadline(overall_deadline, Duration::from_secs(3));
    let mut last = None;
    while Instant::now() < deadline {
        let snapshot = inspection.tree(viewport, deadline)?;
        if snapshot.step > previous_step {
            eprintln!(
                "physical-smoke: {stage}: viewport={viewport:?} step={}",
                snapshot.step
            );
            return Ok(snapshot);
        }
        last = Some(snapshot);
    }
    let current = last
        .as_ref()
        .map_or(previous_step, |snapshot| snapshot.step);
    let summary = last.as_ref().map_or_else(
        || "no tree observed".to_owned(),
        |snapshot| semantic_tree_summary(&snapshot.tree),
    );
    Err(format!(
        "{stage}: viewport {viewport:?} did not advance after OS input; previous={previous_step}, current={current}, tree={summary}"
    ))
}

fn root_viewport(viewports: &[ViewportDescriptor]) -> Result<ViewportDescriptor, String> {
    let mut roots = viewports
        .iter()
        .filter(|viewport| viewport.id == ViewportId::ROOT && viewport.class == ViewportClass::Root)
        .cloned()
        .collect::<Vec<_>>();
    match roots.len() {
        1 => Ok(roots.remove(0)),
        count => Err(format!(
            "semantic viewport inventory must contain exactly one root, found {count}: {viewports:?}"
        )),
    }
}

fn child_viewport(
    shared: &Shared,
    inspection: &mut InspectionClient,
    viewports: &[ViewportDescriptor],
    deadline: Instant,
) -> Result<(ViewportDescriptor, ViewportTreeSnapshot, LogicalBounds), String> {
    let mut deferred = viewports
        .iter()
        .filter(|viewport| {
            viewport.id != ViewportId::ROOT && viewport.class == ViewportClass::Deferred
        })
        .cloned()
        .collect::<Vec<_>>();
    let child = match deferred.len() {
        1 => deferred.remove(0),
        count => Err(format!(
            "expected one deferred child viewport before semantic selection, found {count}: {viewports:?}"
        ))?,
    };
    let (snapshot, bounds) = wait_for_stable_selector_bounds(
        shared,
        inspection,
        child.id,
        SemanticSelector {
            role: egui::accesskit::Role::Tab,
            text: SemanticText::Label(CHILD_TAB_LABEL),
        },
        deadline,
        "await-stable-child-tab",
    )?;
    Ok((child, snapshot, bounds))
}

fn viewport_window_title(viewport: &ViewportDescriptor) -> Result<&str, String> {
    if let Some(title) = viewport.title.as_deref().filter(|title| !title.is_empty()) {
        return Ok(title);
    }
    Err(format!(
        "semantic viewport {:?} has no native title",
        viewport.id
    ))
}

fn root_target_semantic_bounds(tree: &TreeUpdate) -> Result<LogicalBounds, SemanticLookupError> {
    semantic_bounds(
        tree,
        SemanticSelector {
            role: egui::accesskit::Role::Label,
            text: SemanticText::Value(ROOT_TARGET_LABEL),
        },
    )
}

fn wait_for_stable_root_target_bounds(
    shared: &Shared,
    inspection: &mut InspectionClient,
    viewport: ViewportId,
    deadline: Instant,
) -> Result<(ViewportTreeSnapshot, LogicalBounds), String> {
    wait_for_stable_semantic_bounds(
        shared,
        inspection,
        viewport,
        ROOT_TARGET_LABEL,
        deadline,
        "await-stable-root-target",
        root_target_semantic_bounds,
    )
}

fn center_guide_bounds(snapshot: &ViewportTreeSnapshot) -> Result<LogicalBounds, String> {
    let selector = SemanticSelector {
        role: egui::accesskit::Role::Label,
        text: SemanticText::Label(CENTER_DROP_GUIDE_LABEL),
    };
    let node = semantic_node(&snapshot.tree, selector).map_err(|error| {
        format!(
            "viewport {:?} step {} missing exact Center guide semantic receiver: {error:?}",
            snapshot.viewport, snapshot.step
        )
    })?;
    if node.supports_action(egui::accesskit::Action::Click)
        || node.supports_action(egui::accesskit::Action::Focus)
    {
        return Err(format!(
            "viewport {:?} step {} Center guide diagnostic unexpectedly exposes an action",
            snapshot.viewport, snapshot.step
        ));
    }
    selector_bounds(snapshot, selector)
}

#[cfg(test)]
mod pointer_passthrough_tests {
    use super::*;

    #[test]
    fn barrier_selects_the_exact_new_callback() {
        let child = ViewportId::from_hash_of("physical-smoke-child");
        let mut state = SharedState::default();
        state.pointer_passthrough_sequence = 2;
        state.pointer_passthrough = vec![
            PointerPassthroughObservation {
                sequence: 1,
                viewport: ViewportId::ROOT,
                enabled: true,
                status: eframe::NativeViewportPointerPassthroughStatus::Applied,
            },
            PointerPassthroughObservation {
                sequence: 2,
                viewport: child,
                enabled: true,
                status: eframe::NativeViewportPointerPassthroughStatus::Applied,
            },
        ];
        let shared = Arc::new((Mutex::new(state), Condvar::new()));

        assert_eq!(pointer_passthrough_checkpoint(&shared), 2);
        assert_eq!(
            wait_for_pointer_passthrough(
                &shared,
                1,
                child,
                true,
                Instant::now() + Duration::from_secs(1),
                "test barrier",
            ),
            Ok(PointerPassthroughObservation {
                sequence: 2,
                viewport: child,
                enabled: true,
                status: eframe::NativeViewportPointerPassthroughStatus::Applied,
            })
        );
    }

    #[test]
    fn barrier_rejects_a_failed_platform_result() {
        let mut state = SharedState::default();
        state.pointer_passthrough_sequence = 1;
        state.pointer_passthrough = vec![PointerPassthroughObservation {
            sequence: 1,
            viewport: ViewportId::ROOT,
            enabled: true,
            status: eframe::NativeViewportPointerPassthroughStatus::Failed,
        }];
        let shared = Arc::new((Mutex::new(state), Condvar::new()));

        let error = wait_for_pointer_passthrough(
            &shared,
            0,
            ViewportId::ROOT,
            true,
            Instant::now() + Duration::from_secs(1),
            "test barrier",
        )
        .expect_err("a failed platform callback must fail the drag route");
        assert!(error.contains("was not applied"));
    }
}

#[cfg(test)]
mod guide_semantic_tests {
    use egui::accesskit::{Action, Node, NodeId, Rect, Tree, TreeId};

    use super::*;

    fn guide_snapshot(node: Node) -> ViewportTreeSnapshot {
        let root = NodeId::from(1_u64);
        ViewportTreeSnapshot {
            viewport: ViewportId::ROOT,
            step: 7,
            pixels_per_point: 1.5,
            tree: TreeUpdate {
                nodes: vec![(root, node)],
                tree: Some(Tree::new(root)),
                tree_id: TreeId::ROOT,
                focus: root,
            },
        }
    }

    #[test]
    fn center_guide_selector_accepts_only_a_passive_exact_receiver() {
        let mut guide = Node::new(egui::accesskit::Role::Label);
        guide.set_label(CENTER_DROP_GUIDE_LABEL);
        guide.set_bounds(Rect::new(310.0, 210.0, 390.0, 290.0));
        assert_eq!(
            center_guide_bounds(&guide_snapshot(guide)),
            Ok(LogicalBounds {
                min_x: 310.0,
                min_y: 210.0,
                max_x: 390.0,
                max_y: 290.0,
            })
        );

        let mut actionable = Node::new(egui::accesskit::Role::Label);
        actionable.set_label(CENTER_DROP_GUIDE_LABEL);
        actionable.set_bounds(Rect::new(310.0, 210.0, 390.0, 290.0));
        actionable.add_action(Action::Click);
        assert!(center_guide_bounds(&guide_snapshot(actionable)).is_err());
    }
}

#[cfg(test)]
mod viewport_title_tests {
    use eframe::egui::ViewportId;
    use egui_inspection::{ViewportClass, ViewportDescriptor};

    use super::viewport_window_title;

    fn descriptor(id: ViewportId, title: Option<&str>) -> ViewportDescriptor {
        ViewportDescriptor {
            id,
            parent: ViewportId::ROOT,
            class: if id == ViewportId::ROOT {
                ViewportClass::Root
            } else {
                ViewportClass::Deferred
            },
            title: title.map(str::to_owned),
            tree_step: Some(7),
        }
    }

    #[test]
    fn root_window_requires_its_own_viewport_title() {
        let root = descriptor(ViewportId::ROOT, None);

        assert!(viewport_window_title(&root).is_err());
    }

    #[test]
    fn deferred_window_requires_its_own_viewport_title() {
        let child = descriptor(ViewportId::from_hash_of("child"), None);

        assert!(viewport_window_title(&child).is_err());
    }
}
