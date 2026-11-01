//! Physical docking scenario state machine and its causal barriers.

use std::net::SocketAddr;
use std::sync::PoisonError;
use std::time::{Duration, Instant};

#[cfg(test)]
use std::sync::{Arc, Condvar, Mutex};

use eframe::egui::accesskit::TreeUpdate;
use eframe::egui::{self, ViewportId};
use egui_inspection::{ViewportClass, ViewportDescriptor};
use winit::event::PointerWindowRoute;

#[cfg(test)]
use super::super::app::SharedState;
use super::super::app::{
    CENTER_DROP_GUIDE_LABEL, CHILD_CLOSE_LABEL, CHILD_TAB_LABEL, OverflowOutput,
    PointerPassthroughObservation, ProductObservation, ProductPhase, ROOT_TARGET_LABEL,
    ROOT_WINDOW_TITLE, RedockLayout, Shared, SourceSelection, StationaryMode,
    StationaryOverflowProbe, StationaryPress,
};
use super::super::assertions::{
    ActualGuideHit, DockPreviewEmission, LogicalBounds, NestedPaneCase, OverflowTabGeometry,
    PHYSICAL_TARGET_ENV, PaneCenterCase, PaneCenterError, PaneCenterGeometry, PaneEdgeCase,
    PhysicalBounds, PhysicalPoint, SemanticLookupError, SemanticSelector, SemanticStabilitySample,
    SemanticText, TabHalfCase, TabHalfGeometry, actual_guide_hits, choose_outside_points,
    desktop_to_viewport_logical, dock_preview_emission, native_drag_preview_observation,
    nested_pane_bounds, nested_pane_point, overflow_tab_geometry, overflow_tab_point,
    pane_center_geometry, pane_center_point, pane_edge_point, point_towards,
    pointer_barrier_is_satisfied, pointer_observation_for_barrier, semantic_baseline_after_missing,
    semantic_bounds, semantic_node, semantic_snapshot_is_stable, semantic_tree_summary,
    stationary_overflow_edge_point, stationary_overflow_geometry, tab_drag_point,
    tab_half_geometry, tab_half_point, two_stage_ingress_points, verify_center_preview,
    verify_edge_preview, verify_nested_pane_preview, verify_overflow_tab_preview,
    verify_tab_half_preview, viewport_logical_to_desktop,
};
use super::super::pointer_load::{
    POINTER_LOAD_ENV, POINTER_LOAD_MOVES, begin_pointer_load, finish_pointer_load,
    pointer_load_enabled, verify_pointer_load,
};
use super::inspection::{InspectionClient, ViewportTreeSnapshot, viewport_retirement_is_observed};
use super::x11::{DestroyWatcher, WindowRecord, X11Desktop, x11_window_is_retired};

const TARGET_ENV: &str = PHYSICAL_TARGET_ENV;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RedockTarget {
    GuideControl,
    PaneCenter(PaneCenterCase),
    PaneEdge(PaneEdgeCase),
    TabHalf(TabHalfCase),
    OverflowBefore,
    NestedPane(NestedPaneCase),
}

impl RedockTarget {
    fn from_env() -> Result<Self, String> {
        match std::env::var(TARGET_ENV) {
            Err(std::env::VarError::NotPresent) => Ok(Self::GuideControl),
            Ok(value) => Self::parse(&value),
            Err(error) => Err(format!("{TARGET_ENV}: {error}")),
        }
    }

    fn parse(value: &str) -> Result<Self, String> {
        match value {
            "pane-center-a" => Ok(Self::PaneCenter(PaneCenterCase::A)),
            "pane-center-b" => Ok(Self::PaneCenter(PaneCenterCase::B)),
            "pane-center-c" => Ok(Self::PaneCenter(PaneCenterCase::C)),
            "pane-edge-north" => Ok(Self::PaneEdge(PaneEdgeCase::North)),
            "pane-edge-south" => Ok(Self::PaneEdge(PaneEdgeCase::South)),
            "pane-edge-east" => Ok(Self::PaneEdge(PaneEdgeCase::East)),
            "pane-edge-west" => Ok(Self::PaneEdge(PaneEdgeCase::West)),
            "tab-half-before" => Ok(Self::TabHalf(TabHalfCase::Before)),
            "tab-half-after" => Ok(Self::TabHalf(TabHalfCase::After)),
            "overflow-tab-before" => Ok(Self::OverflowBefore),
            _ => NestedPaneCase::parse(value).map(Self::NestedPane).ok_or_else(|| format!(
                "invalid {TARGET_ENV}={value:?}; unset for guide-control or choose pane-center-a, pane-center-b, pane-center-c, pane-edge-north, pane-edge-south, pane-edge-east, pane-edge-west, tab-half-before, tab-half-after, nested-pane-center, nested-pane-edge"
            )),
        }
    }

    const fn name(self) -> &'static str {
        match self {
            Self::GuideControl => "guide-control",
            Self::PaneCenter(case) => case.name(),
            Self::PaneEdge(case) => case.name(),
            Self::TabHalf(case) => case.name(),
            Self::OverflowBefore => "overflow-tab-before",
            Self::NestedPane(case) => case.name(),
        }
    }

    const fn expected_layout(self) -> RedockLayout {
        match self {
            Self::GuideControl | Self::PaneCenter(_) => RedockLayout::Center,
            Self::PaneEdge(case) => RedockLayout::Edge(case),
            Self::TabHalf(TabHalfCase::Before) => RedockLayout::TabBefore,
            Self::TabHalf(TabHalfCase::After) => RedockLayout::Center,
            Self::OverflowBefore => RedockLayout::OverflowBefore,
            Self::NestedPane(case) => RedockLayout::Nested(case),
        }
    }

    const fn supports_pointer_load(self) -> bool {
        matches!(self, Self::PaneCenter(_))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PaneDockTarget {
    Center(PaneCenterCase),
    Edge(PaneEdgeCase),
    TabHalf(TabHalfCase),
    NestedPane(NestedPaneCase),
}

impl PaneDockTarget {
    const fn name(self) -> &'static str {
        match self {
            Self::Center(case) => case.name(),
            Self::Edge(case) => case.name(),
            Self::TabHalf(case) => case.name(),
            Self::NestedPane(case) => case.name(),
        }
    }

    fn verify_preview(
        self,
        geometry: PaneCenterGeometry,
        tab: TabHalfGeometry,
        preview: DockPreviewEmission,
        pixels_per_point: f32,
    ) -> Result<(), PaneCenterError> {
        match self {
            Self::Center(_) => verify_center_preview(geometry, preview, pixels_per_point),
            Self::Edge(case) => verify_edge_preview(case, geometry, preview, pixels_per_point),
            Self::TabHalf(case) => verify_tab_half_preview(case, tab, preview, pixels_per_point),
            Self::NestedPane(case) => {
                verify_nested_pane_preview(case, geometry, tab, preview, pixels_per_point)
            }
        }
    }
}

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

fn assert_root_pointer_passthrough_preserved(
    shared: &Shared,
    after_sequence: u64,
    stage: &str,
) -> Result<(), String> {
    let (lock, _) = &**shared;
    let guard = lock.lock().unwrap_or_else(PoisonError::into_inner);
    if let Some(error) = &guard.fatal_error {
        return Err(format!("{stage}: {error}"));
    }
    if let Some(observation) = guard.pointer_passthrough.iter().find(|observation| {
        observation.sequence > after_sequence
            && observation.viewport == ViewportId::ROOT
            && observation.enabled
    }) {
        return Err(format!(
            "{stage}: External Root must preserve native hit regions; unexpected pointer pass-through enable: {observation:?}"
        ));
    }
    eprintln!(
        "physical-smoke: {stage}: no Root pointer pass-through enable after sequence={after_sequence}"
    );
    Ok(())
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
                | eframe::NativeViewportPointerPassthroughStatus::Failed
                | eframe::NativeViewportPointerPassthroughStatus::Retired => Err(format!(
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
    target: RedockTarget,
}

pub(crate) fn run(shared: Shared, inspection_endpoint: SocketAddr) -> Result<(), String> {
    let deadline = Instant::now() + Duration::from_secs(35);
    let mut driver = PhysicalDriver::new(shared, inspection_endpoint, deadline)?;
    driver
        .run_inner()
        .map_err(|error| format!("stage={}: {error}", driver.stage.name()))
}

impl PhysicalDriver {
    fn click_root_control(
        &mut self,
        role: egui::accesskit::Role,
        label: &'static str,
    ) -> Result<(), String> {
        eprintln!("physical-smoke: overflow preparation click={label}");
        let (snapshot, bounds) = wait_for_stable_selector_bounds(
            &self.shared,
            &mut self.inspection,
            ViewportId::ROOT,
            SemanticSelector {
                role,
                text: SemanticText::Label(label),
            },
            bounded_stage_deadline(self.deadline, Duration::from_secs(3)),
            label,
        )?;
        let close = if role == egui::accesskit::Role::Tab {
            optional_selector_bounds(
                &snapshot,
                SemanticSelector {
                    role: egui::accesskit::Role::Button,
                    text: SemanticText::Label("Close Main"),
                },
            )?
        } else {
            None
        };
        let logical = tab_drag_point(bounds, close)
            .ok_or_else(|| format!("{label} has no safe visible click point"))?;
        let window = self.desktop.window_for_title(ROOT_WINDOW_TITLE)?;
        let point = viewport_logical_to_desktop(window.bounds, snapshot.pixels_per_point, logical)
            .map_err(|error| format!("{label} click coordinates: {error:?}"))?;
        self.desktop.move_pointer(point)?;
        self.desktop.assert_pointer(point, Some(false))?;
        let hovered = input_barrier(
            &self.shared,
            &mut self.inspection,
            ViewportId::ROOT,
            snapshot.step,
            window.bounds,
            snapshot.pixels_per_point,
            point,
            false,
            self.deadline,
            label,
        )?;
        press_primary_with_cleanup(&self.desktop, label)?;
        let pressed = input_barrier(
            &self.shared,
            &mut self.inspection,
            ViewportId::ROOT,
            hovered.step,
            window.bounds,
            snapshot.pixels_per_point,
            point,
            true,
            self.deadline,
            label,
        );
        let release = self.desktop.set_primary_button(false);
        let pressed = pressed?;
        release?;
        input_barrier(
            &self.shared,
            &mut self.inspection,
            ViewportId::ROOT,
            pressed.step,
            window.bounds,
            snapshot.pixels_per_point,
            point,
            false,
            self.deadline,
            label,
        )?;
        Ok(())
    }

    fn prepare_overflow_target(&mut self) -> Result<(), String> {
        // Real clicks establish manual scrolling before the source gesture starts.
        // Selection reveals Main once; a backward step then leaves its right side clipped.
        self.click_root_control(egui::accesskit::Role::Button, "Scroll tabs backward")?;
        self.click_root_control(egui::accesskit::Role::Tab, "Main")?;
        self.click_root_control(egui::accesskit::Role::Button, "Scroll tabs backward")?;
        let (snapshot, geometry, _, _) = wait_for_overflow_scene(
            &self.shared,
            &mut self.inspection,
            ViewportId::ROOT,
            0,
            None,
            self.deadline,
        )?;
        eprintln!(
            "physical-smoke: overflow prepared by actual selection/backward clicks step={} geometry={geometry:?}",
            snapshot.step
        );
        Ok(())
    }

    fn new(
        shared: Shared,
        inspection_endpoint: SocketAddr,
        deadline: Instant,
    ) -> Result<Self, String> {
        let target = RedockTarget::from_env()?;
        if pointer_load_enabled()? && !target.supports_pointer_load() {
            return Err(format!(
                "{POINTER_LOAD_ENV}=1khz requires an explicit {TARGET_ENV}=pane-center-a, pane-center-b, or pane-center-c"
            ));
        }
        Ok(Self {
            shared,
            inspection: InspectionClient::connect(inspection_endpoint, deadline)?,
            desktop: X11Desktop::connect()?,
            deadline,
            work_area: None,
            stage: ScenarioStage::ValidateX11Environment,
            target,
        })
    }

    fn set_stage(&mut self, stage: ScenarioStage) {
        self.stage = stage;
        eprintln!("physical-smoke: stage={}", stage.name());
    }

    fn run_inner(&mut self) -> Result<(), String> {
        eprintln!("physical-smoke: target-case={}", self.target.name());
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
        let final_observation = wait_for_product(
            &self.shared,
            self.deadline,
            self.stage.name(),
            |observation| observation.phase == ProductPhase::Retired && observation.quiescent,
        )?;
        let expected_layout = if self
            .shared
            .0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .stationary
            .is_some()
        {
            RedockLayout::StationaryCenter
        } else {
            self.target.expected_layout()
        };
        if final_observation.redock_layout != Some(expected_layout) {
            return Err(format!(
                "{}: {:?}; expected {:?} in the original root, observed {:?}",
                self.target.name(),
                PaneCenterError::FinalLayoutMismatch,
                expected_layout,
                final_observation.redock_layout
            ));
        }
        self.verify_retired_viewport(retirement)?;
        assert_root_pointer_passthrough_preserved(
            &self.shared,
            0,
            "Root preserved through physical tear-off, redock, and retirement",
        )?;
        if pointer_load_enabled()? {
            verify_pointer_load(&self.shared)?;
        }
        if let Some(probe) = &self
            .shared
            .0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .stationary
        {
            if !probe.complete {
                return Err(format!(
                    "{} full-eframe/GPU barrier was incomplete: {probe:?}",
                    probe.mode.label()
                ));
            }
            match probe.mode {
                StationaryMode::Positive => eprintln!(
                    "physical-smoke: stationary-positive full-eframe/GPU journey; real first-live Child/native press and forwarded capture, Known ROOT hover with original Child delivery/capture, first sample kept Main inactive, actual dwell selected Main while held without motion/button edges; same gesture redocked [Anchor, Main, Child], DestroyNotify/quiescence verified; not the recording fixture"
                ),
                StationaryMode::Negative => eprintln!(
                    "physical-smoke: stationary-negative full-eframe/GPU journey; after real tear-off/first-live, cross-window Observed query denied stationary selection; same held gesture redocked [Anchor, Main, Child], DestroyNotify/quiescence verified; no successful stationary dwell claimed"
                ),
                StationaryMode::Overflow | StationaryMode::OverflowCancel => eprintln!(
                    "physical-smoke: {} full-eframe/GPU journey; exact ROOT Presented overflow outputs and original Child press/capture verified; same held gesture redocked [Anchor, Main, Child], DestroyNotify/quiescence verified",
                    probe.mode.label(),
                ),
                StationaryMode::Disabled => {
                    return Err("a disabled stationary probe cannot qualify".to_owned());
                }
            }
        }
        eprintln!(
            "physical-smoke: target-case={} verified target layout, retirement and quiescence",
            self.target.name()
        );
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
        let (outside, routed_outside) = choose_outside_points(
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
            "physical-smoke: root-window={:?} source={source:?} threshold={threshold:?} outside={outside:?} routed-outside={routed_outside:?} work-area={work_area:?}",
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
                routed_target: routed_outside,
                deadline: self.deadline,
            },
        )
    }

    fn perform_drag_back(&mut self) -> Result<(RetirementTarget, DragResult), String> {
        self.set_stage(ScenarioStage::DiscoverChildRedockCoordinates);
        if self.target == RedockTarget::OverflowBefore {
            self.prepare_overflow_target()?;
        }
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
        let frozen_pane = match self.target {
            RedockTarget::GuideControl => None,
            RedockTarget::PaneCenter(_)
            | RedockTarget::PaneEdge(_)
            | RedockTarget::TabHalf(_)
            | RedockTarget::OverflowBefore
            | RedockTarget::NestedPane(_) => Some(
                pane_center_geometry(&root_tree.tree)
                    .map_err(|error| format!("{} before press: {error:?}", self.target.name()))?,
            ),
        };
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
        let stationary = self
            .shared
            .0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .stationary
            .as_ref()
            .map(|probe| StationaryTarget {
                source_window: child_window.window,
                mode: probe.mode,
            });

        self.set_stage(ScenarioStage::DragBackX11Events);
        let drag_back = inject_redock_drag(
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
                stationary,
                deadline: self.deadline,
            },
            self.target,
            frozen_pane,
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
        wait_for_stable_selector_bounds(
            &self.shared,
            &mut self.inspection,
            final_root.id,
            SemanticSelector {
                role: egui::accesskit::Role::Tab,
                text: SemanticText::Label(CHILD_TAB_LABEL),
            },
            bounded_stage_deadline(self.deadline, Duration::from_secs(3)),
            "retired child item remains in live root",
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
struct StationaryTarget {
    source_window: u32,
    mode: StationaryMode,
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
    routed_target: PhysicalPoint,
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
    stationary: Option<StationaryTarget>,
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
    let route_checkpoint = pointer_passthrough_checkpoint(shared);
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
        let route_confirmed = viewport_barrier(
            inspection,
            ViewportId::ROOT,
            source_at_first_target.step,
            request.deadline,
            &format!("{} drag-route confirmation", request.label),
        )?;
        // This fixture tears content out of an External Root, preserving its hit regions.
        assert_root_pointer_passthrough_preserved(
            shared,
            route_checkpoint,
            &format!(
                "{} Root preserved after first target ingress",
                request.label
            ),
        )?;
        desktop.move_pointer(request.routed_target)?;
        desktop.assert_pointer(request.routed_target, Some(true))?;
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
            request.routed_target,
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
    desktop.assert_pointer(request.routed_target, Some(false))?;
    let released = viewport_barrier(
        inspection,
        request.preview_viewport,
        preview.step,
        request.deadline,
        &format!("{} physical release", request.label),
    )?;
    assert_root_pointer_passthrough_preserved(
        shared,
        route_checkpoint,
        &format!("{} Root preserved through physical release", request.label),
    )?;
    Ok(DragResult { preview, released })
}

fn stationary_main_point(
    snapshot: &ViewportTreeSnapshot,
    client: PhysicalBounds,
) -> Result<PhysicalPoint, String> {
    let main = selector_bounds(
        snapshot,
        SemanticSelector {
            role: egui::accesskit::Role::Tab,
            text: SemanticText::Label("Main"),
        },
    )?;
    let close = selector_bounds(
        snapshot,
        SemanticSelector {
            role: egui::accesskit::Role::Button,
            text: SemanticText::Label("Close Main"),
        },
    )?;
    let logical = tab_drag_point(main, [close])
        .ok_or("stationary: Main has no tab-body point outside Close Main")?;
    let point = viewport_logical_to_desktop(client, snapshot.pixels_per_point, logical)
        .map_err(|error| format!("stationary: Main desktop point: {error:?}"))?;
    let logical = desktop_to_viewport_logical(client, snapshot.pixels_per_point, point)
        .map_err(|error| format!("stationary: rounded Main point: {error:?}"))?;
    if !client.contains(point)
        || logical.x <= main.min_x
        || logical.x >= main.max_x
        || logical.y <= main.min_y
        || logical.y >= main.max_y
        || close.contains(logical)
    {
        return Err("stationary: rounded point is outside the safe Main tab body".to_owned());
    }
    for (_, node) in &snapshot.tree.nodes {
        if node.is_hidden()
            || node.role() != egui::accesskit::Role::Label
            || !matches!(
                node.label(),
                Some(
                    "Dockspace center drop guide"
                        | "Dockspace left drop guide"
                        | "Dockspace right drop guide"
                        | "Dockspace top drop guide"
                        | "Dockspace bottom drop guide"
                )
            )
        {
            continue;
        }
        let bounds = node
            .bounds()
            .ok_or("stationary: visible guide is missing bounds")?;
        if ![bounds.x0, bounds.y0, bounds.x1, bounds.y1]
            .into_iter()
            .all(f64::is_finite)
            || bounds.x0 > bounds.x1
            || bounds.y0 > bounds.y1
            || (logical.x >= bounds.x0
                && logical.x <= bounds.x1
                && logical.y >= bounds.y0
                && logical.y <= bounds.y1)
        {
            return Err(format!(
                "stationary: Main point is not proven outside guide {:?}",
                node.label()
            ));
        }
    }
    Ok(point)
}

#[allow(clippy::too_many_lines)]
fn stationary_while_pressed(
    shared: &Shared,
    inspection: &mut InspectionClient,
    desktop: &X11Desktop,
    request: GuideDragRequest<'_>,
    target: StationaryTarget,
    source_ingress_step: u64,
) -> Result<(ViewportTreeSnapshot, ViewportTreeSnapshot), String> {
    if target.mode.is_overflow() {
        return stationary_overflow_while_pressed(
            shared,
            inspection,
            desktop,
            request,
            target,
            source_ingress_step,
        );
    }
    let deadline = bounded_stage_deadline(request.deadline, Duration::from_secs(5));
    let main_selector = SemanticSelector {
        role: egui::accesskit::Role::Tab,
        text: SemanticText::Label("Main"),
    };
    let (threshold, _) = wait_for_stable_selector_bounds(
        shared,
        inspection,
        request.target_viewport,
        main_selector,
        deadline,
        "stationary ROOT Main after native-child threshold and passthrough",
    )?;
    let root = desktop.window_for_title(request.target_window_title)?;
    let point = stationary_main_point(&threshold, root.bounds)?;
    let press = {
        let mut state = shared.0.lock().unwrap_or_else(PoisonError::into_inner);
        if state.observation.phase != ProductPhase::ChildFirstLive
            || request.source_viewport == request.target_viewport
            || request.target_viewport != ViewportId::ROOT
            || target.source_window == root.window
        {
            return Err(
                "stationary requires a first-live Child source distinct from ROOT".to_owned(),
            );
        }
        let probe = state
            .stationary
            .as_mut()
            .ok_or("stationary probe is absent")?;
        if probe.complete
            || probe.target.is_some()
            || probe.result.is_some()
            || probe.invalidation.is_some()
            || probe.selection != Some(SourceSelection::INACTIVE_MAIN)
        {
            return Err(format!(
                "stationary: invalid before-move model/probe: {probe:?}"
            ));
        }
        let press = probe
            .press
            .clone()
            .ok_or("stationary: no real native primary press")?;
        if press.viewport != request.source_viewport
            || u64::from(press.window) != u64::from(target.source_window)
        {
            return Err(format!(
                "stationary: native press {press:?} does not match child X11 window {}",
                target.source_window
            ));
        }
        probe.target = Some(point);
        // Start pressure before the real motion can mint its continuation.
        // Callback delay cannot make a pre-pressure query qualify this run.
        probe.stress_active = true;
        eprintln!(
            "physical-smoke: stationary before move: native_press={press:?}, public_model_selection={:?}, Main desktop target={point:?}",
            probe.selection
        );
        press
    };
    desktop.move_pointer(point)?;
    desktop.assert_pointer(point, Some(true))?;
    let hovered = input_barrier(
        shared,
        inspection,
        request.source_viewport,
        source_ingress_step,
        request.source_client,
        request.source_pixels_per_point,
        point,
        true,
        deadline,
        "stationary inactive Main hover with primary held",
    )?;
    let (hover_scene, _) = wait_for_stable_selector_bounds(
        shared,
        inspection,
        request.target_viewport,
        main_selector,
        deadline,
        "stationary current Main after hover",
    )?;
    if hover_scene.step <= threshold.step
        || stationary_main_point(&hover_scene, root.bounds)? != point
    {
        return Err("stationary: Main safe point changed after native motion".to_owned());
    }
    {
        let mut state = shared.0.lock().unwrap_or_else(PoisonError::into_inner);
        let probe = state
            .stationary
            .as_mut()
            .ok_or("stationary probe is absent")?;
        if !probe.hover_observed
            || probe.invalidation.is_some()
            || (probe.selection != Some(SourceSelection::INACTIVE_MAIN)
                && !(target.mode == StationaryMode::Positive
                    && probe.selection == Some(SourceSelection::ACTIVE_MAIN)
                    && probe.first_model_after_result.is_some()))
        {
            return Err(format!(
                "stationary: Main must retain its semantic scene under slow/repeat pressure: {probe:?}"
            ));
        }
    }
    // The adapter's deadline wakes the real query. This wait creates no pointer facts.
    let result = {
        let (lock, changed) = &**shared;
        let mut state = lock.lock().unwrap_or_else(PoisonError::into_inner);
        loop {
            if let Some(error) = &state.fatal_error {
                return Err(format!("stationary: {error}"));
            }
            let probe = state
                .stationary
                .as_ref()
                .ok_or("stationary probe is absent")?;
            if let Some(reason) = probe.invalidation {
                return Err(format!("stationary: {reason}; probe={probe:?}"));
            }
            if let Some(result) = probe.result {
                break result;
            }
            let now = Instant::now();
            if now >= deadline {
                return Err(format!(
                    "stationary: timed out waiting for a real forwarded query callback while Main was held; hover_step={}, probe={probe:?}, product={:?}; tree={}",
                    hovered.step,
                    state.observation,
                    semantic_tree_summary(&hovered.tree)
                ));
            }
            let (next, _) = changed
                .wait_timeout(state, deadline.saturating_duration_since(now))
                .unwrap_or_else(PoisonError::into_inner);
            state = next;
        }
    };
    if result.viewport_id() != press.viewport
        || result.expected_window_id() != press.window
        || result.device_id() != press.device
    {
        return Err(format!(
            "stationary: query request identity differs from native press {press:?}: {result:?}"
        ));
    }
    let eframe::NativeViewportPointerObservation::Observed { facts, delivery } =
        result.observation()
    else {
        return Err(format!(
            "stationary requires a real current X11 Observed result; no query/Unavailable/Unsupported cannot qualify: {result:?}"
        ));
    };
    if target.mode == StationaryMode::Positive {
        let source_route = PointerWindowRoute::Window(press.window);
        if !matches!(facts.hover, PointerWindowRoute::Window(window) if u64::from(window) == u64::from(root.window))
            || facts.capture != source_route
            || delivery != source_route
            || !facts.desktop_position.is_some_and(|position| {
                position.x == f64::from(point.x) && position.y == f64::from(point.y)
            })
        {
            return Err(format!(
                "stationary-positive requires raw Known ROOT hover and original Child capture/delivery at Main: {result:?}"
            ));
        }
        let (capture, result_at) = {
            let state = shared.0.lock().unwrap_or_else(PoisonError::into_inner);
            let probe = state
                .stationary
                .as_ref()
                .ok_or("stationary probe is absent")?;
            let capture = probe.capture.ok_or_else(|| {
                format!(
                    "stationary-positive requires an actual forwarded Acquired callback: {probe:?}"
                )
            })?;
            if !probe.capture_pending
                || capture.viewport_id() != press.viewport
                || capture.expected_window_id() != press.window
                || capture.device_id() != press.device
                || capture.action() != eframe::NativeViewportPointerCaptureAction::Acquire
                || capture.status() != eframe::NativeViewportPointerCaptureStatus::Acquired
            {
                return Err(format!(
                    "stationary-positive capture does not match the real press: {probe:?}"
                ));
            }
            (
                capture,
                probe
                    .result_at
                    .ok_or("the first real query has no observation time")?,
            )
        };
        let delay = dockspace::runtime::DockPresentationConfig::default()
            .hover_selection_delay()
            .ok_or("stationary-positive requires the configured finite hover delay")?;
        let (final_result, sample_interval) = {
            let (lock, changed) = &**shared;
            let mut state = lock.lock().unwrap_or_else(PoisonError::into_inner);
            loop {
                if let Some(error) = &state.fatal_error {
                    return Err(format!("stationary-positive: {error}"));
                }
                let probe = state
                    .stationary
                    .as_ref()
                    .ok_or("stationary probe is absent")?;
                if probe.invalidation.is_some() || probe.press.as_ref() != Some(&press) {
                    return Err(format!(
                        "stationary-positive hold/model invalidated: {probe:?}"
                    ));
                }
                if probe.first_model_after_result == Some(result.token())
                    && probe.selection == Some(SourceSelection::ACTIVE_MAIN)
                    && probe
                        .main_selected_at
                        .is_some_and(|at| at.duration_since(result_at) >= delay)
                    && probe.repeated_output.is_some()
                    && probe.repeat_finalizations >= 2
                    && probe.slow_suffixes >= 2
                    && probe
                        .latest_result
                        .is_some_and(|latest| latest.token() != result.token())
                    && probe.model_after_result == probe.latest_result.map(|latest| latest.token())
                    && probe
                        .result_interval
                        .zip(probe.latest_result_interval)
                        .is_some_and(|((_, first_end), (second_begin, _))| {
                            second_begin.saturating_duration_since(first_end) >= delay
                        })
                {
                    let (_, first_end) = probe.result_interval.unwrap();
                    let (second_begin, _) = probe.latest_result_interval.unwrap();
                    break (
                        probe.latest_result.unwrap(),
                        second_begin.saturating_duration_since(first_end),
                    );
                }
                let now = Instant::now();
                if now >= deadline {
                    return Err(format!(
                        "stationary-positive: no fresh dwell/model selection while Primary stayed held: {probe:?}"
                    ));
                }
                let (next, _) = changed
                    .wait_timeout(state, deadline.saturating_duration_since(now))
                    .unwrap_or_else(PoisonError::into_inner);
                state = next;
            }
        };
        let eframe::NativeViewportPointerObservation::Observed { facts, delivery } =
            final_result.observation()
        else {
            return Err(format!(
                "stationary-positive dwell requires a real second Observed query: {final_result:?}"
            ));
        };
        if final_result.viewport_id() != press.viewport
            || final_result.expected_window_id() != press.window
            || final_result.device_id() != press.device
            || !matches!(facts.hover, PointerWindowRoute::Window(window) if u64::from(window) == u64::from(root.window))
            || facts.capture != source_route
            || delivery != source_route
            || !facts.desktop_position.is_some_and(|position| {
                position.x == f64::from(point.x) && position.y == f64::from(point.y)
            })
        {
            return Err(format!(
                "stationary-positive dwell query lost its actual routes: {final_result:?}"
            ));
        }
        let after = input_barrier(
            shared,
            inspection,
            request.source_viewport,
            hovered.step,
            request.source_client,
            request.source_pixels_per_point,
            point,
            true,
            deadline,
            "stationary-positive actual held Child after Main selection",
        )?;
        let (after_scene, _) = wait_for_stable_selector_bounds(
            shared,
            inspection,
            request.target_viewport,
            main_selector,
            deadline,
            "stationary-positive selected Main semantic scene",
        )?;
        let main = semantic_node(&after_scene.tree, main_selector)
            .map_err(|error| format!("stationary-positive selected Main: {error:?}"))?;
        let current_root = desktop.window_for_title(request.target_window_title)?;
        let current_source = desktop
            .process_windows()?
            .into_iter()
            .find(|window| window.window == target.source_window)
            .ok_or("stationary-positive original Child disappeared during the hold")?;
        if main.is_selected() != Some(true)
            || stationary_main_point(&after_scene, root.bounds)? != point
            || current_root.window != root.window
            || current_root.bounds != root.bounds
            || current_source.bounds != request.source_client
            || after.pixels_per_point != request.source_pixels_per_point
            || after_scene.pixels_per_point != threshold.pixels_per_point
        {
            return Err(
                "stationary-positive Main selection/geometry did not survive its semantic barrier"
                    .to_owned(),
            );
        }
        desktop.assert_pointer(point, Some(true))?;
        let mut state = shared.0.lock().unwrap_or_else(PoisonError::into_inner);
        let still_live = state.observation.phase == ProductPhase::ChildFirstLive;
        let probe = state
            .stationary
            .as_mut()
            .ok_or("stationary probe is absent")?;
        if !still_live
            || probe.invalidation.is_some()
            || probe.selection != Some(SourceSelection::ACTIVE_MAIN)
            || probe.press.as_ref() != Some(&press)
        {
            return Err(format!(
                "stationary-positive gesture must remain live and held: {probe:?}"
            ));
        }
        probe.finish();
        eprintln!(
            "physical-smoke: stationary-positive full-eframe/GPU barrier: actual_native_press={press:?}, forwarded_capture={capture:?}, acquisition_sample={result:?}, dwell_sample={final_result:?}, conservative_capture_interval={sample_interval:?}, model_observed_elapsed={:?}, public_model={:?}, held Child AccessKit steps={} -> {}; no motion/button edge during dwell; continue current Center redock",
            probe.main_selected_at.unwrap().duration_since(result_at),
            probe.selection,
            hovered.step,
            after.step
        );
        return Ok((after, after_scene));
    }
    let blocker = shared
        .0
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .stationary
        .as_ref()
        .and_then(|probe| probe.blocker_window)
        .ok_or("stationary-negative requires an actual foreign InputOnly blocker")?;
    if !matches!(
        facts.hover,
        PointerWindowRoute::Unknown | PointerWindowRoute::Foreign
    ) || facts.capture != PointerWindowRoute::Window(press.window)
        || delivery != PointerWindowRoute::Window(press.window)
        || !facts.desktop_position.is_some_and(|position| {
            position.x == f64::from(point.x) && position.y == f64::from(point.y)
        })
    {
        return Err(format!(
            "stationary-negative requires actual blocked hover and independently known source capture/delivery: blocker={blocker}, {result:?}"
        ));
    }
    let after = input_barrier(
        shared,
        inspection,
        request.source_viewport,
        hovered.step,
        request.source_client,
        request.source_pixels_per_point,
        point,
        true,
        deadline,
        "stationary-negative model/hover after the real query result",
    )?;
    let (after_scene, _) = wait_for_stable_selector_bounds(
        shared,
        inspection,
        request.target_viewport,
        main_selector,
        deadline,
        "stationary-negative current Main after result",
    )?;
    let current_root = desktop.window_for_title(request.target_window_title)?;
    let current_source = desktop
        .process_windows()?
        .into_iter()
        .find(|window| window.window == target.source_window)
        .ok_or("stationary-negative: source window disappeared during the hold")?;
    if current_root.window != root.window
        || current_root.bounds != root.bounds
        || current_source.bounds != request.source_client
        || after.pixels_per_point != request.source_pixels_per_point
        || after_scene.pixels_per_point != threshold.pixels_per_point
        || stationary_main_point(&after_scene, root.bounds)? != point
    {
        return Err("stationary-negative: root/Main geometry changed during the hold".to_owned());
    }
    desktop.assert_pointer(point, Some(true))?;
    let (lock, changed) = &**shared;
    let mut state = lock.lock().unwrap_or_else(PoisonError::into_inner);
    loop {
        if let Some(error) = &state.fatal_error {
            return Err(format!("stationary-negative: {error}"));
        }
        let probe = state
            .stationary
            .as_ref()
            .ok_or("stationary probe is absent")?;
        if probe.invalidation.is_some() || probe.selection != Some(SourceSelection::INACTIVE_MAIN) {
            return Err(format!(
                "stationary-negative: hold/model was invalidated: {probe:?}"
            ));
        }
        if probe.model_after_result == Some(result.token())
            && probe.repeated_output.is_some()
            && probe.repeat_finalizations >= 2
            && probe.slow_suffixes >= 2
        {
            break;
        }
        let now = Instant::now();
        if now >= deadline {
            return Err(format!(
                "stationary-negative: no semantic model and real repeat finalizations after exact callback token {:?}: {probe:?}",
                result.token()
            ));
        }
        let (next, _) = changed
            .wait_timeout(state, deadline.saturating_duration_since(now))
            .unwrap_or_else(PoisonError::into_inner);
        state = next;
    }
    let probe = state
        .stationary
        .as_mut()
        .ok_or("stationary probe is absent")?;
    probe.finish();
    eprintln!(
        "physical-smoke: stationary-negative barrier: actual_InputOnly={blocker}, exact_raw_result={result:?}, native_press={press:?}, AccessKit Main-held source steps={} -> {}, public_model_after={:?}, slow_suffixes={}, repeated_output={:?}, real_pass_finalizations={}; known source capture cannot repair blocked hover, Main stayed inactive; continuing the held redock gesture",
        hovered.step,
        after.step,
        probe.selection,
        probe.slow_suffixes,
        probe.repeated_output,
        probe.repeat_finalizations
    );
    Ok((after, after_scene))
}

fn wait_for_stationary_overflow_output(
    shared: &Shared,
    press: &StationaryPress,
    index: usize,
    deadline: Instant,
) -> Result<OverflowOutput, String> {
    let (lock, changed) = &**shared;
    let mut state = lock.lock().unwrap_or_else(PoisonError::into_inner);
    loop {
        if let Some(error) = &state.fatal_error {
            return Err(format!("stationary overflow: {error}"));
        }
        let probe = state
            .stationary
            .as_ref()
            .ok_or("stationary probe is absent")?;
        if probe.invalidation.is_some()
            || probe.press.as_ref() != Some(press)
            || probe.selection != Some(SourceSelection::INACTIVE_MAIN)
        {
            return Err(format!(
                "stationary overflow lost its hold/model: {probe:?}"
            ));
        }
        if let Some(output) = probe
            .overflow
            .as_ref()
            .and_then(|overflow| overflow.outputs[index])
            .filter(|output| output.presented.is_some())
        {
            return Ok(output);
        }
        let now = Instant::now();
        if now >= deadline {
            return Err(format!(
                "stationary overflow: no exact Presented slot={index} before deadline: {probe:?}"
            ));
        }
        let (next, _) = changed
            .wait_timeout(state, deadline.saturating_duration_since(now))
            .unwrap_or_else(PoisonError::into_inner);
        state = next;
    }
}

#[allow(clippy::too_many_lines)]
fn stationary_overflow_while_pressed(
    shared: &Shared,
    inspection: &mut InspectionClient,
    desktop: &X11Desktop,
    request: GuideDragRequest<'_>,
    target: StationaryTarget,
    source_ingress_step: u64,
) -> Result<(ViewportTreeSnapshot, ViewportTreeSnapshot), String> {
    let deadline = bounded_stage_deadline(request.deadline, Duration::from_secs(5));
    let (scene, _) = wait_for_stable_selector_bounds(
        shared,
        inspection,
        ViewportId::ROOT,
        SemanticSelector {
            role: egui::accesskit::Role::Tab,
            text: SemanticText::Label("Main"),
        },
        deadline,
        "stationary overflow inactive Main before motion",
    )?;
    let initial = stationary_overflow_geometry(&scene.tree)
        .map_err(|error| format!("stationary overflow initial geometry: {error:?}"))?;
    let config = dockspace::runtime::DockPresentationConfig::default();
    let step = config.tab_strip_scroll_line_extent();
    if initial.maximum - initial.offset < step * 2.0 {
        return Err(format!(
            "stationary overflow requires two complete steps: initial={initial:?}, step={step}"
        ));
    }
    let root = desktop.window_for_title(request.target_window_title)?;
    let guides = actual_guide_hits(&scene.tree)
        .map_err(|error| format!("stationary overflow guides: {error:?}"))?;
    let (point, logical) = stationary_overflow_edge_point(
        initial,
        root.bounds,
        scene.pixels_per_point,
        config.tab_strip_edge_scroll_band(),
        &guides,
    )
    .map_err(|error| format!("stationary overflow rounded edge point: {error:?}"))?;
    let press = {
        let mut state = shared.0.lock().unwrap_or_else(PoisonError::into_inner);
        if state.observation.phase != ProductPhase::ChildFirstLive
            || request.target_viewport != ViewportId::ROOT
            || request.source_viewport == ViewportId::ROOT
            || root.window == target.source_window
        {
            return Err(
                "stationary overflow requires a distinct first-live Child source".to_owned(),
            );
        }
        let probe = state
            .stationary
            .as_mut()
            .ok_or("stationary probe is absent")?;
        let press = probe
            .press
            .clone()
            .ok_or("stationary overflow has no real press")?;
        if probe.complete
            || probe.target.is_some()
            || probe.result.is_some()
            || probe.invalidation.is_some()
            || probe.selection != Some(SourceSelection::INACTIVE_MAIN)
            || press.viewport != request.source_viewport
            || u64::from(press.window) != u64::from(target.source_window)
        {
            return Err(format!("stationary overflow cannot arm: {probe:?}"));
        }
        probe.target = Some(point);
        probe.overflow = Some(StationaryOverflowProbe::new(initial, root.window));
        probe.stress_active = true;
        press
    };
    desktop.move_pointer(point)?;
    desktop.assert_pointer(point, Some(true))?;
    let hovered = input_barrier(
        shared,
        inspection,
        request.source_viewport,
        source_ingress_step,
        request.source_client,
        request.source_pixels_per_point,
        point,
        true,
        deadline,
        "stationary overflow one real motion into the effective right band",
    )?;
    let first = wait_for_stationary_overflow_output(shared, &press, 1, deadline)?;
    let mut source_step = hovered.step;
    let held_point = if target.mode == StationaryMode::OverflowCancel {
        let exit = request.routed_ingress;
        let logical_exit = desktop_to_viewport_logical(root.bounds, scene.pixels_per_point, exit)
            .map_err(|error| format!("stationary overflow exit point: {error:?}"))?;
        if initial.viewport.contains(logical_exit) {
            return Err("stationary overflow control exit remains in the tab strip".to_owned());
        }
        shared
            .0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .stationary
            .as_mut()
            .unwrap()
            .overflow
            .as_mut()
            .unwrap()
            .exit_target = Some(exit);
        desktop.move_pointer(exit)?;
        desktop.assert_pointer(exit, Some(true))?;
        source_step = input_barrier(
            shared,
            inspection,
            request.source_viewport,
            source_step,
            request.source_client,
            request.source_pixels_per_point,
            exit,
            true,
            deadline,
            "stationary overflow control exit reached the held source UI",
        )?
        .step;
        {
            let mut state = shared.0.lock().unwrap_or_else(PoisonError::into_inner);
            let overflow = state
                .stationary
                .as_mut()
                .unwrap()
                .overflow
                .as_mut()
                .unwrap();
            if overflow.exit_observed_at.is_none() {
                return Err("the source UI exit lacks its original native motion".to_owned());
            }
            // Count the quiet interval only after observing the exact source UI position.
            overflow.stop_after = config
                .tab_strip_edge_scroll_interval()
                .and_then(|interval| Instant::now().checked_add(interval * 2));
        }
        // Inspection requests a normal repaint; it creates no native pointer facts.
        inspection.tree(ViewportId::ROOT, deadline)?;
        exit
    } else {
        point
    };
    let last = wait_for_stationary_overflow_output(shared, &press, 2, deadline)?;
    // Glow executes viewport commands after reporting Presented. Keep the sticky
    // query/capture checks alive through the following fresh source pass.
    let terminal_source = if target.mode == StationaryMode::OverflowCancel {
        Some(input_barrier(
            shared,
            inspection,
            request.source_viewport,
            source_step,
            request.source_client,
            request.source_pixels_per_point,
            held_point,
            true,
            deadline,
            "stationary overflow control crossed terminal commands and fresh source UI",
        )?)
    } else {
        None
    };
    let mut state = shared.0.lock().unwrap_or_else(PoisonError::into_inner);
    let probe = state
        .stationary
        .as_mut()
        .ok_or("stationary probe is absent")?;
    if probe.invalidation.is_some()
        || probe.press.as_ref() != Some(&press)
        || probe.selection != Some(SourceSelection::INACTIVE_MAIN)
    {
        return Err(format!(
            "stationary overflow final hold/model was invalidated: {probe:?}"
        ));
    }
    let capture = probe
        .capture
        .ok_or("stationary overflow has no actual Acquired callback")?;
    if !probe.capture_pending
        || capture.viewport_id() != press.viewport
        || capture.expected_window_id() != press.window
        || capture.device_id() != press.device
        || capture.action() != eframe::NativeViewportPointerCaptureAction::Acquire
        || capture.status() != eframe::NativeViewportPointerCaptureStatus::Acquired
        || probe.repeated_output.is_none()
        || probe.repeat_finalizations < 2
        || probe.slow_suffixes < 2
    {
        return Err(format!(
            "stationary overflow lacks real capture/repeat pressure: {probe:?}"
        ));
    }
    let overflow = probe.overflow.as_ref().unwrap();
    let baseline = overflow.outputs[0].ok_or("stationary overflow has no first-sample baseline")?;
    for output in [baseline, first, last] {
        let eframe::NativeViewportPointerObservation::Observed { facts, delivery } =
            output.sample.observation()
        else {
            return Err(format!(
                "stationary overflow requires actual Observed samples: {output:?}"
            ));
        };
        let source = PointerWindowRoute::Window(press.window);
        if output.sample.viewport_id() != press.viewport
            || output.sample.expected_window_id() != press.window
            || output.sample.device_id() != press.device
            || !matches!(facts.hover, PointerWindowRoute::Window(window) if u64::from(window) == u64::from(root.window))
            || facts.capture != source
            || delivery != source
            || !facts.desktop_position.is_some_and(|position| {
                position.x == f64::from(point.x) && position.y == f64::from(point.y)
            })
        {
            return Err(format!(
                "stationary overflow sample lost its actual route: {output:?}"
            ));
        }
    }
    if target.mode == StationaryMode::OverflowCancel
        && (overflow.exit_observed_at.is_none()
            || overflow
                .stop_after
                .is_none_or(|after| last.presented.unwrap().1 < after)
            || first.sample.token() != last.sample.token()
            || probe.latest_result.map(|latest| latest.token()) != Some(first.sample.token()))
    {
        return Err(format!(
            "stationary overflow control did not stop after accepted exit: {probe:?}"
        ));
    }
    let outputs = [baseline, first, last].map(|row| {
        (
            row.token,
            row.presented.unwrap().0.ordinal().get(),
            row.geometry.offset,
            row.sample.token(),
        )
    });
    let intervals = [
        first
            .sample_interval
            .0
            .saturating_duration_since(baseline.sample_interval.1),
        last.sample_interval
            .0
            .saturating_duration_since(first.sample_interval.1),
    ];
    let quiet = overflow
        .exit_observed_at
        .map(|at| last.presented.unwrap().1.saturating_duration_since(at));
    eprintln!(
        "physical-smoke: {} exact-output barrier: initial_offset={} maximum={} fixed_edge={logical:?}/{point:?} outputs(token,ordinal,offset,sample)={outputs:?} conservative_sample_intervals={intervals:?} quiet_after_native_exit={quiet:?}; real renderer Presented terminals forwarded in order; subsequent steps use the existing core presentation barrier, no fabricated core-commit receipt",
        target.mode.label(),
        initial.offset,
        initial.maximum,
    );
    probe.finish();
    drop(state);
    desktop.assert_pointer(held_point, Some(true))?;
    let after = if let Some(after) = terminal_source {
        after
    } else {
        input_barrier(
            shared,
            inspection,
            request.source_viewport,
            source_step,
            request.source_client,
            request.source_pixels_per_point,
            held_point,
            true,
            deadline,
            "stationary overflow source remains held after exact output steps",
        )?
    };
    Ok((after, scene))
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

fn inject_redock_drag(
    shared: &Shared,
    inspection: &mut InspectionClient,
    desktop: &X11Desktop,
    request: GuideDragRequest<'_>,
    target: RedockTarget,
    frozen_pane: Option<PaneCenterGeometry>,
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
        let (mut source_ingress_step, mut ingress_tree) = reach_target_surface_while_pressed(
            shared,
            inspection,
            desktop,
            request,
            source_at_hover.step,
        )?;
        if let Some(probe) = request.stationary {
            let (source_after_main, _) = stationary_while_pressed(
                shared,
                inspection,
                desktop,
                request,
                probe,
                source_ingress_step,
            )?;
            // Resume the same held gesture with real motion into the docking target.
            // The stationary probe is complete before this new candidate begins.
            desktop.move_pointer(request.routed_ingress)?;
            desktop.assert_pointer(request.routed_ingress, Some(true))?;
            let source_after_resume = input_barrier(
                shared,
                inspection,
                request.source_viewport,
                source_after_main.step,
                request.source_client,
                request.source_pixels_per_point,
                request.routed_ingress,
                true,
                request.deadline,
                "stationary resume held redock with real motion",
            )?;
            let (resumed, _) = wait_for_stable_selector_bounds(
                shared,
                inspection,
                request.target_viewport,
                SemanticSelector {
                    role: egui::accesskit::Role::Label,
                    text: SemanticText::Label(CENTER_DROP_GUIDE_LABEL),
                },
                bounded_stage_deadline(request.deadline, Duration::from_secs(3)),
                "stationary fresh Center guide after real resume",
            )?;
            source_ingress_step = source_after_resume.step;
            ingress_tree = resumed;
        }
        match target {
            RedockTarget::GuideControl => move_to_center_guide_while_pressed(
                shared,
                inspection,
                desktop,
                request,
                source_ingress_step,
                &ingress_tree,
            ),
            RedockTarget::PaneCenter(case) => move_to_pane_target_while_pressed(
                shared,
                inspection,
                desktop,
                request,
                PaneDockTarget::Center(case),
                frozen_pane.ok_or_else(|| {
                    format!("{}: {:?}", case.name(), PaneCenterError::BoundsTooSmall)
                })?,
                source_ingress_step,
                &ingress_tree,
            ),
            RedockTarget::PaneEdge(case) => move_to_pane_target_while_pressed(
                shared,
                inspection,
                desktop,
                request,
                PaneDockTarget::Edge(case),
                frozen_pane.ok_or_else(|| {
                    format!("{}: {:?}", case.name(), PaneCenterError::BoundsTooSmall)
                })?,
                source_ingress_step,
                &ingress_tree,
            ),
            RedockTarget::TabHalf(case) => move_to_pane_target_while_pressed(
                shared,
                inspection,
                desktop,
                request,
                PaneDockTarget::TabHalf(case),
                frozen_pane.ok_or_else(|| {
                    format!("{}: {:?}", case.name(), PaneCenterError::BoundsTooSmall)
                })?,
                source_ingress_step,
                &ingress_tree,
            ),
            RedockTarget::OverflowBefore => move_to_overflow_target_while_pressed(
                shared,
                inspection,
                desktop,
                request,
                source_ingress_step,
                &ingress_tree,
            ),
            RedockTarget::NestedPane(case) => move_to_pane_target_while_pressed(
                shared,
                inspection,
                desktop,
                request,
                PaneDockTarget::NestedPane(case),
                frozen_pane.ok_or_else(|| {
                    format!("{}: {:?}", case.name(), PaneCenterError::BoundsTooSmall)
                })?,
                source_ingress_step,
                &ingress_tree,
            ),
        }
    })();
    let restore_checkpoint = pointer_passthrough_checkpoint(shared);
    let release = desktop.set_primary_button(false);
    let (preview, final_point) = match (pressed, release) {
        (Ok(preview), Ok(())) => preview,
        (Err(error), Ok(())) | (Ok(_), Err(error)) => return Err(error),
        (Err(error), Err(release)) => {
            return Err(format!("{error}; release cleanup also failed: {release}"));
        }
    };
    desktop.assert_pointer(final_point, Some(false))?;
    let released = viewport_barrier(
        inspection,
        request.target_viewport,
        preview.step,
        request.deadline,
        &format!("{} physical release", request.label),
    )?;
    if request.source_viewport != ViewportId::ROOT {
        wait_for_pointer_passthrough(
            shared,
            restore_checkpoint,
            request.source_viewport,
            false,
            request.deadline,
            &format!("{} drag-route restore", request.label),
        )?;
    }
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
    if request.source_viewport != ViewportId::ROOT {
        // The torn-off owned Child contains only the dragged tab in its sole main root,
        // with no contained siblings, so this fixture still requires enable and its ACK.
        wait_for_pointer_passthrough(
            shared,
            route_checkpoint,
            request.source_viewport,
            true,
            request.deadline,
            &format!("{} drag-route enable", request.label),
        )?;
    }
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
    assert_root_pointer_passthrough_preserved(
        shared,
        route_checkpoint,
        &format!(
            "{} Root preserved after target-surface ingress",
            request.label
        ),
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

#[derive(Debug, PartialEq)]
struct PaneCenterScene {
    geometry: PaneCenterGeometry,
    tab: TabHalfGeometry,
    guides: Vec<ActualGuideHit>,
    preview: Option<DockPreviewEmission>,
}

fn wait_for_overflow_scene(
    shared: &Shared,
    inspection: &mut InspectionClient,
    viewport: ViewportId,
    after_step: u64,
    expected: Option<OverflowTabGeometry>,
    overall: Instant,
) -> Result<
    (
        ViewportTreeSnapshot,
        OverflowTabGeometry,
        Vec<ActualGuideHit>,
        Option<DockPreviewEmission>,
    ),
    String,
> {
    let deadline = bounded_stage_deadline(overall, Duration::from_secs(3));
    let mut previous = None;
    let mut last = "no current overflow scene".to_owned();
    while Instant::now() < deadline {
        if let Some(error) = shared_fatal_error(shared) {
            return Err(error);
        }
        let snapshot = inspection.tree(viewport, deadline)?;
        if snapshot.step <= after_step {
            continue;
        }
        let geometry = match overflow_tab_geometry(&snapshot.tree) {
            Ok(geometry) => geometry,
            Err(error) => {
                last = format!("step {}: {error:?}", snapshot.step);
                previous = None;
                continue;
            }
        };
        let guides = match actual_guide_hits(&snapshot.tree) {
            Ok(guides) => guides,
            Err(PaneCenterError::MissingGuide { .. }) if expected.is_none() => Vec::new(),
            Err(error) => {
                last = format!("step {}: {error:?}", snapshot.step);
                previous = None;
                continue;
            }
        };
        let preview = dock_preview_emission(&snapshot.tree)
            .map_err(|error| format!("overflow Dock emission: {error:?}"))?;
        if let Some(expected) = expected {
            if geometry != expected {
                return Err(format!(
                    "overflow target moved: {expected:?} -> {geometry:?}"
                ));
            }
            let Some(preview) = preview else {
                previous = None;
                last = "no Dock preview".to_owned();
                continue;
            };
            if let Err(error) =
                verify_overflow_tab_preview(geometry, preview, snapshot.pixels_per_point)
            {
                last = format!(
                    "step {} transitional preview {preview:?}: {error:?}",
                    snapshot.step
                );
                previous = None;
                continue;
            }
        }
        let epoch = shared
            .0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .roster_epoch;
        let sample = (
            epoch,
            snapshot.pixels_per_point,
            geometry,
            guides.clone(),
            preview,
        );
        if previous
            .as_ref()
            .is_some_and(|(step, prior)| snapshot.step > *step && prior == &sample)
        {
            return Ok((snapshot, geometry, guides, preview));
        }
        previous = Some((snapshot.step, sample));
    }
    Err(format!(
        "overflow scene did not stabilize after step={after_step}: {last}"
    ))
}

fn move_to_overflow_target_while_pressed(
    shared: &Shared,
    inspection: &mut InspectionClient,
    desktop: &X11Desktop,
    request: GuideDragRequest<'_>,
    source_step: u64,
    ingress: &ViewportTreeSnapshot,
) -> Result<(ViewportTreeSnapshot, PhysicalPoint), String> {
    let (before, geometry, guides, _) = wait_for_overflow_scene(
        shared,
        inspection,
        request.target_viewport,
        ingress.step,
        None,
        request.deadline,
    )?;
    if guides.is_empty() {
        return Err("overflow target lacks current actual guide facts".to_owned());
    }
    let window = desktop.window_for_title(request.target_window_title)?;
    let (point, logical) =
        overflow_tab_point(geometry, window.bounds, before.pixels_per_point, &guides)
            .map_err(|error| format!("overflow clipped input point: {error:?}"))?;
    desktop.move_pointer(point)?;
    desktop.assert_pointer(point, Some(true))?;
    let source = input_barrier(
        shared,
        inspection,
        request.source_viewport,
        source_step,
        request.source_client,
        request.source_pixels_per_point,
        point,
        true,
        request.deadline,
        "overflow real motion into clipped Main",
    )?;
    let target = viewport_barrier(
        inspection,
        request.target_viewport,
        before.step,
        request.deadline,
        "overflow target after real motion",
    )?;
    let (preview, final_geometry, guides, emission) = wait_for_overflow_scene(
        shared,
        inspection,
        request.target_viewport,
        target.step,
        Some(geometry),
        request.deadline,
    )?;
    let final_window = desktop.window_for_title(request.target_window_title)?;
    let (final_point, _) = overflow_tab_point(
        final_geometry,
        final_window.bounds,
        preview.pixels_per_point,
        &guides,
    )
    .map_err(|error| format!("overflow before release: {error:?}"))?;
    if point != final_point
        || final_window.bounds != window.bounds
        || before.pixels_per_point != preview.pixels_per_point
    {
        return Err("overflow physical target changed before release".to_owned());
    }
    input_barrier(
        shared,
        inspection,
        request.source_viewport,
        source.step,
        request.source_client,
        request.source_pixels_per_point,
        point,
        true,
        request.deadline,
        "overflow held source before release",
    )?;
    desktop.assert_pointer(point, Some(true))?;
    eprintln!(
        "physical-smoke: overflow clipped-Main input={logical:?}/{point:?} geometry={geometry:?} Dock program emitted={emission:?} step={} after-motion-step={} excluded-guides={} (not a GPU presentation receipt)",
        preview.step,
        target.step,
        guides.len()
    );
    Ok((preview, point))
}

fn wait_for_pane_center_scene(
    shared: &Shared,
    inspection: &mut InspectionClient,
    viewport: ViewportId,
    after_step: u64,
    expected_target: Option<(PaneCenterGeometry, PaneDockTarget)>,
    overall_deadline: Instant,
) -> Result<(ViewportTreeSnapshot, PaneCenterScene), String> {
    let deadline = bounded_stage_deadline(overall_deadline, Duration::from_secs(3));
    let mut previous: Option<(u64, u64, f32, PaneCenterScene)> = None;
    let mut last_pending = "no complete scene observed".to_owned();
    while Instant::now() < deadline {
        if let Some(error) = shared_fatal_error(shared) {
            return Err(error);
        }
        let snapshot = inspection
            .tree(viewport, deadline)
            .map_err(|error| format!("{error}; pane-center last pending: {last_pending}"))?;
        if snapshot.step <= after_step {
            continue;
        }
        let roster_epoch = {
            let (lock, _) = &**shared;
            lock.lock()
                .unwrap_or_else(PoisonError::into_inner)
                .roster_epoch
        };
        let geometry = match pane_center_geometry(&snapshot.tree) {
            Ok(geometry) => geometry,
            Err(PaneCenterError::Semantic {
                error: SemanticLookupError::Missing,
                ..
            }) => {
                last_pending = format!("step {} has no current pane/workspace", snapshot.step);
                previous = previous.filter(|(_, epoch, _, _)| *epoch == roster_epoch);
                continue;
            }
            Err(error) => {
                return Err(format!(
                    "pane-center geometry step={}: {error:?}",
                    snapshot.step
                ));
            }
        };
        let guides = match actual_guide_hits(&snapshot.tree) {
            Ok(guides) => guides,
            Err(error @ PaneCenterError::MissingGuide { .. }) => {
                last_pending = format!("step {}: {error:?}", snapshot.step);
                previous = previous.filter(|(_, epoch, _, _)| *epoch == roster_epoch);
                continue;
            }
            Err(error) => {
                return Err(format!(
                    "pane-center actual guide hits step={}: {error:?}",
                    snapshot.step
                ));
            }
        };
        let preview = dock_preview_emission(&snapshot.tree).map_err(|error| {
            format!(
                "pane-center Dock emission step={}: {error:?}",
                snapshot.step
            )
        })?;
        let tab = tab_half_geometry(&snapshot.tree)
            .map_err(|error| format!("root Main tab step={}: {error:?}", snapshot.step))?;
        if expected_target.is_some() && preview.is_none() {
            last_pending = format!("step {} has no Dock preview emission", snapshot.step);
            previous = None;
            continue;
        }
        if let Some((expected, target)) = expected_target {
            if geometry != expected {
                return Err(format!(
                    "pane-center current geometry: {:?}",
                    PaneCenterError::TargetMoved
                ));
            }
            if let Some(preview) = preview {
                match target.verify_preview(expected, tab, preview, snapshot.pixels_per_point) {
                    Ok(()) => {}
                    Err(PaneCenterError::PreviewBoundsMismatch) => {
                        last_pending = format!(
                            "step {} has transitional preview {preview:?}",
                            snapshot.step
                        );
                        previous = None;
                        continue;
                    }
                    Err(error) => {
                        return Err(format!(
                            "pane-center emitted rectangle step={}: {error:?}; preview={preview:?}, frozen-content={:?}",
                            snapshot.step, expected.content
                        ));
                    }
                }
            }
        }
        let scene = PaneCenterScene {
            geometry,
            tab,
            guides,
            preview,
        };
        if previous
            .as_ref()
            .is_some_and(|(step, epoch, scale, prior)| {
                snapshot.step > *step
                    && roster_epoch == *epoch
                    && snapshot.pixels_per_point == *scale
                    && &scene == prior
            })
        {
            return Ok((snapshot, scene));
        }
        previous = Some((
            snapshot.step,
            roster_epoch,
            snapshot.pixels_per_point,
            scene,
        ));
    }
    Err(format!(
        "pane-center scene did not stabilize after step={after_step}; require-dock-emission={}; last-pending={last_pending}",
        expected_target.is_some()
    ))
}

fn pointer_load_points(
    selected: PhysicalPoint,
    client: PhysicalBounds,
    pixels_per_point: f32,
    scene: &PaneCenterScene,
) -> Result<[PhysicalPoint; 5], String> {
    let config = dockspace::runtime::DockPresentationConfig::default();
    let content = scene.geometry.content;
    let inset = (1.0 - config.magnetic_center_fraction()) * 0.5;
    let center = LogicalBounds {
        min_x: content.min_x + (content.max_x - content.min_x) * inset,
        min_y: content.min_y + (content.max_y - content.min_y) * inset,
        max_x: content.max_x - (content.max_x - content.min_x) * inset,
        max_y: content.max_y - (content.max_y - content.min_y) * inset,
    };
    let workspace = scene.geometry.workspace;
    let band = ((workspace.max_x - workspace.min_x).min(workspace.max_y - workspace.min_y)
        * config.magnetic_outer_band_fraction())
    .clamp(
        config.magnetic_outer_band_minimum(),
        config.magnetic_outer_band_maximum(),
    );
    let margin = 2.0 / f64::from(pixels_per_point);
    let mut points = [selected; 5];
    // Check the fixed square against the target client, not the captured source child.
    for (index, (dx, dy)) in [(-2, -2), (2, -2), (2, 2), (-2, 2), (0, 0)]
        .into_iter()
        .enumerate()
    {
        let point = PhysicalPoint {
            x: selected.x + dx,
            y: selected.y + dy,
        };
        let logical = desktop_to_viewport_logical(client, pixels_per_point, point)
            .map_err(|error| format!("physical load point {point:?}: {error:?}"))?;
        if !client.contains(point) {
            return Err(format!(
                "physical load point {point:?} left target client {client:?}"
            ));
        }
        for (name, bounds, padding) in [
            ("content", content, margin),
            ("center basin", center, margin),
            ("workspace outside root edge band", workspace, band + margin),
        ] {
            if logical.x <= bounds.min_x + padding
                || logical.x >= bounds.max_x - padding
                || logical.y <= bounds.min_y + padding
                || logical.y >= bounds.max_y - padding
            {
                return Err(format!(
                    "physical load point {point:?} left {name}: {bounds:?}"
                ));
            }
        }
        for guide in &scene.guides {
            let bounds = guide.bounds;
            if logical.x >= bounds.min_x - margin
                && logical.x <= bounds.max_x + margin
                && logical.y >= bounds.min_y - margin
                && logical.y <= bounds.max_y + margin
            {
                return Err(format!(
                    "physical load point {point:?} approaches guide {guide:?}"
                ));
            }
        }
        points[index] = point;
    }
    Ok(points)
}

fn inject_pointer_load(
    shared: &Shared,
    desktop: &X11Desktop,
    request: GuideDragRequest<'_>,
    points: [PhysicalPoint; 5],
) -> Result<(), String> {
    const CADENCE: Duration = Duration::from_millis(1);
    if points[0] == request.routed_ingress {
        return Err("physical load first move must advance from routed ingress".to_owned());
    }
    begin_pointer_load(shared, request.source_viewport, points)?;
    let started = Instant::now();
    let mut previous = started;
    let mut maximum_gap = Duration::ZERO;
    let mut injected = 0;
    for step in 1..=POINTER_LOAD_MOVES {
        // Delay the next slot after a late wake; never catch up by emitting a burst.
        let scheduled = previous + CADENCE;
        if scheduled >= request.deadline {
            return Err(
                "physical load cannot fit its fixed schedule before the scenario deadline"
                    .to_owned(),
            );
        }
        loop {
            let now = Instant::now();
            if now >= scheduled {
                break;
            }
            let remaining = scheduled.duration_since(now);
            if remaining > Duration::from_micros(500) {
                std::thread::sleep(remaining - Duration::from_micros(500));
            } else {
                std::hint::spin_loop();
            }
        }
        let now = Instant::now();
        if now >= request.deadline {
            return Err(format!(
                "physical load exceeded its deadline at move {step}"
            ));
        }
        maximum_gap = maximum_gap.max(now.duration_since(previous));
        previous = now;
        let point = if step == POINTER_LOAD_MOVES {
            points[4]
        } else {
            points[((step - 1) % 4) as usize]
        };
        // Each call keeps the existing checked XTest request and physical pointer query.
        desktop.move_pointer(point)?;
        injected += 1;
    }
    let elapsed = started.elapsed();
    eprintln!(
        "physical-load: injector moves={injected} elapsed={elapsed:?} maximum_start_gap={maximum_gap:?} requested_interval={CADENCE:?} catch_up=false"
    );
    finish_pointer_load(shared, injected, elapsed)
}

#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
fn move_to_pane_target_while_pressed(
    shared: &Shared,
    inspection: &mut InspectionClient,
    desktop: &X11Desktop,
    request: GuideDragRequest<'_>,
    target: PaneDockTarget,
    geometry: PaneCenterGeometry,
    source_ingress_step: u64,
    ingress_tree: &ViewportTreeSnapshot,
) -> Result<(ViewportTreeSnapshot, PhysicalPoint), String> {
    let (target_tree, target_scene) = wait_for_pane_center_scene(
        shared,
        inspection,
        request.target_viewport,
        ingress_tree.step,
        None,
        request.deadline,
    )?;
    if target_scene.geometry != geometry {
        return Err(format!(
            "{} before motion: {:?}",
            target.name(),
            PaneCenterError::TargetMoved
        ));
    }
    let window = desktop.window_for_title(request.target_window_title)?;
    let (selected, logical) = match target {
        PaneDockTarget::Center(case) => pane_center_point(
            case,
            geometry,
            window.bounds,
            target_tree.pixels_per_point,
            &target_scene.guides,
        ),
        PaneDockTarget::Edge(case) => pane_edge_point(
            case,
            geometry,
            window.bounds,
            target_tree.pixels_per_point,
            &target_scene.guides,
        ),
        PaneDockTarget::TabHalf(case) => tab_half_point(
            case,
            target_scene.tab,
            window.bounds,
            target_tree.pixels_per_point,
            &target_scene.guides,
        ),
        PaneDockTarget::NestedPane(case) => nested_pane_point(
            case,
            geometry,
            target_scene.tab,
            window.bounds,
            target_tree.pixels_per_point,
            &target_scene.guides,
        ),
    }
    .map_err(|error| format!("{} before motion: {error:?}", target.name()))?;
    if selected == request.routed_ingress {
        return Err(format!(
            "{}: {:?}; non-guide motion must advance from routed ingress",
            target.name(),
            PaneCenterError::BoundsTooSmall
        ));
    }
    eprintln!(
        "physical-smoke: pane-basin case={} logical={logical:?} physical={selected:?} content={:?} excluded-actual-guides={}",
        target.name(),
        geometry.content,
        target_scene.guides.len()
    );
    if matches!(target, PaneDockTarget::NestedPane(_)) {
        let leaf = nested_pane_bounds(geometry, target_scene.tab, target_tree.pixels_per_point)
            .map_err(|error| format!("{} leaf bounds: {error:?}", target.name()))?;
        eprintln!(
            "physical-smoke: nested-target case={} painted-leaf={leaf:?} root-workspace={:?}",
            target.name(),
            geometry.workspace,
        );
    }
    let load_enabled = pointer_load_enabled()?;
    desktop.move_pointer(selected)?;
    desktop.assert_pointer(selected, Some(true))?;
    let source_step = if request.source_viewport == request.target_viewport {
        source_ingress_step.max(target_tree.step)
    } else {
        source_ingress_step
    };
    let source_at_motion = input_barrier(
        shared,
        inspection,
        request.source_viewport,
        source_step,
        request.source_client,
        request.source_pixels_per_point,
        selected,
        true,
        request.deadline,
        &format!("{} non-guide native motion", target.name()),
    )?;
    // Captured raw input is observed on the source viewport. The target independently emits
    // the Dock program; its marker is not a GPU Presented receipt or an input delivery route.
    let target_at_motion = viewport_barrier(
        inspection,
        request.target_viewport,
        target_tree.step,
        request.deadline,
        &format!("{} target step after native motion", target.name()),
    )?;
    let (preview, final_scene) = wait_for_pane_center_scene(
        shared,
        inspection,
        request.target_viewport,
        target_at_motion.step,
        Some((geometry, target)),
        request.deadline,
    )?;
    let final_window = desktop.window_for_title(request.target_window_title)?;
    let (rechecked, _) = match target {
        PaneDockTarget::Center(case) => pane_center_point(
            case,
            geometry,
            final_window.bounds,
            preview.pixels_per_point,
            &final_scene.guides,
        ),
        PaneDockTarget::Edge(case) => pane_edge_point(
            case,
            geometry,
            final_window.bounds,
            preview.pixels_per_point,
            &final_scene.guides,
        ),
        PaneDockTarget::TabHalf(case) => tab_half_point(
            case,
            final_scene.tab,
            final_window.bounds,
            preview.pixels_per_point,
            &final_scene.guides,
        ),
        PaneDockTarget::NestedPane(case) => nested_pane_point(
            case,
            geometry,
            final_scene.tab,
            final_window.bounds,
            preview.pixels_per_point,
            &final_scene.guides,
        ),
    }
    .map_err(|error| format!("{} before release: {error:?}", target.name()))?;
    if final_window.bounds != window.bounds
        || preview.pixels_per_point != target_tree.pixels_per_point
        || rechecked != selected
        || final_scene.tab != target_scene.tab
    {
        return Err(format!(
            "{}: {:?}; target bounds or physical projection changed before release",
            target.name(),
            PaneCenterError::TargetMoved
        ));
    }
    input_barrier(
        shared,
        inspection,
        request.source_viewport,
        source_at_motion.step,
        request.source_client,
        request.source_pixels_per_point,
        selected,
        true,
        request.deadline,
        &format!("{} held source point before release", target.name()),
    )?;
    desktop.assert_pointer(selected, Some(true))?;
    eprintln!(
        "physical-smoke: pane-basin case={} Dock program emitted={:?} step={} after-motion-step={} all-guide-hits-excluded={} (not a GPU presentation receipt)",
        target.name(),
        final_scene.preview,
        preview.step,
        target_at_motion.step,
        final_scene.guides.len()
    );
    if load_enabled {
        let points = pointer_load_points(
            selected,
            final_window.bounds,
            preview.pixels_per_point,
            &final_scene,
        )?;
        // The scene and all guide exclusions are already proven. Return directly after
        // the load so the caller releases without inspection or a drain-to-idle wait.
        inject_pointer_load(shared, desktop, request, points)?;
    }
    Ok((preview, selected))
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
mod target_tests {
    use super::*;

    #[test]
    fn nested_targets_require_distinct_layouts_and_reject_pointer_load() {
        for case in [NestedPaneCase::Center, NestedPaneCase::West] {
            let target = RedockTarget::parse(case.name()).unwrap();
            assert_eq!(target, RedockTarget::NestedPane(case));
            assert_eq!(target.expected_layout(), RedockLayout::Nested(case));
            assert!(!target.supports_pointer_load());
        }
        for case in [PaneCenterCase::A, PaneCenterCase::B, PaneCenterCase::C] {
            assert!(RedockTarget::PaneCenter(case).supports_pointer_load());
        }
        assert!(RedockTarget::parse("nested-pane-edge-east").is_err());
    }
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
                enabled: false,
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
        assert_root_pointer_passthrough_preserved(&shared, 0, "test Root preserve").unwrap();
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
        let child = ViewportId::from_hash_of("physical-smoke-child");
        let mut state = SharedState::default();
        state.pointer_passthrough_sequence = 1;
        state.pointer_passthrough = vec![PointerPassthroughObservation {
            sequence: 1,
            viewport: child,
            enabled: true,
            status: eframe::NativeViewportPointerPassthroughStatus::Failed,
        }];
        let shared = Arc::new((Mutex::new(state), Condvar::new()));

        let error = wait_for_pointer_passthrough(
            &shared,
            0,
            child,
            true,
            Instant::now() + Duration::from_secs(1),
            "test barrier",
        )
        .expect_err("a failed platform callback must fail the drag route");
        assert!(error.contains("was not applied"));
    }

    #[test]
    fn root_preserve_rejects_any_enable_even_after_restore() {
        for status in [
            eframe::NativeViewportPointerPassthroughStatus::Applied,
            eframe::NativeViewportPointerPassthroughStatus::Unsupported,
            eframe::NativeViewportPointerPassthroughStatus::Failed,
            eframe::NativeViewportPointerPassthroughStatus::Retired,
        ] {
            let mut state = SharedState::default();
            state.pointer_passthrough_sequence = 2;
            state.pointer_passthrough = vec![
                PointerPassthroughObservation {
                    sequence: 1,
                    viewport: ViewportId::ROOT,
                    enabled: true,
                    status,
                },
                PointerPassthroughObservation {
                    sequence: 2,
                    viewport: ViewportId::ROOT,
                    enabled: false,
                    status: eframe::NativeViewportPointerPassthroughStatus::Applied,
                },
            ];
            let shared = Arc::new((Mutex::new(state), Condvar::new()));

            let error = assert_root_pointer_passthrough_preserved(&shared, 0, "test Root preserve")
                .expect_err("a later restore must not hide a forbidden Root enable");
            assert!(error.contains("External Root must preserve"));
        }
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
