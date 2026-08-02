use std::collections::BTreeMap;
use std::error::Error;
use std::io;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use dockspace::graph::{Node, RootRecord, SurfacePresentation, Workspace};
use dockspace::ids::{ItemId, RootId, SurfaceId};
use dockspace::policy::DockPolicy;
use dockspace::presentation_observation::SurfacePresentationOutputTicket;
use dockspace::scene::SurfaceScene;
use dockspace::viewport::WindowToken;
use eframe::egui;
use eframe::{
    NativeTestDriver, NativeTestPointerAction, NativeTestPointerEvent, NativeTestScrollDelta,
};
use egui_dockspace::{DockStyle, Dockspace, PaneView};
use egui_dockspace_native::{
    AllowNativeClose, NativeDockspaceApp, NativeRuntimeStatus, NativeSurfaceSpec,
    NativeViewportRoster,
};

const ROOT_SURFACE: SurfaceId = SurfaceId::new(1);
const ROOT: RootId = RootId::new(1);
const ROOT_ITEM_COUNT: u64 = 2;
const STARTUP_TIMEOUT: Duration = Duration::from_secs(30);
const OUTSIDE_ALL_POINT: egui::Pos2 = egui::pos2(200.0, 800.0);
const OVERFLOW_TAB_WIDTH: f32 = 1_000.0;
const SCROLL_LINES: f32 = -1.0;
const EXPECTED_SCROLL_OFFSET: f64 = 40.0;

type SmokeError = Box<dyn Error + Send + Sync>;

#[derive(Clone, Debug)]
enum SmokeOutcome {
    Pending,
    Passed(NativeRuntimeStatus),
    Failed(String),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SmokePhase {
    AwaitRoot,
    RootPressQueued,
    OutsideMoveQueued,
    OutsideReleaseQueued,
    ChildPressQueued,
    RootMoveQueued,
    RootReleaseQueued,
    OverflowStyleQueued,
    RootScrollQueued,
}

struct SmokeApp {
    runtime: NativeDockspaceApp<SmokePanes>,
    outcome: Arc<Mutex<SmokeOutcome>>,
    renderer: Arc<RendererCounts>,
    driver: NativeTestDriver,
    started: Instant,
    phase: SmokePhase,
    phase_cycle: u64,
    redock_point: Option<egui::Pos2>,
    overflow_predecessor: Option<SurfacePresentationOutputTicket>,
    scroll_predecessor: Option<SurfacePresentationOutputTicket>,
    closing: bool,
}

impl SmokeApp {
    fn new(
        outcome: Arc<Mutex<SmokeOutcome>>,
        renderer: Arc<RendererCounts>,
        driver: NativeTestDriver,
    ) -> Result<Self, SmokeError> {
        let mut policy = DockPolicy::new();
        policy.set_allow_native_surfaces(true);
        let dockspace = Dockspace::builder("native-e2e", workspace())
            .policy(policy)
            .build()?;
        let roster =
            NativeViewportRoster::new(NativeSurfaceSpec::root(ROOT_SURFACE, WindowToken::new(1)))?;
        let runtime = NativeDockspaceApp::new(dockspace, SmokePanes::new(), roster)?
            .with_close_handler(AllowNativeClose);
        Ok(Self {
            runtime,
            outcome,
            renderer,
            driver,
            started: Instant::now(),
            phase: SmokePhase::AwaitRoot,
            phase_cycle: 0,
            redock_point: None,
            overflow_predecessor: None,
            scroll_predecessor: None,
            closing: false,
        })
    }

    fn observe_readiness(&mut self, context: &egui::Context) {
        if self.closing {
            return;
        }
        let status = self.runtime.status();
        let advance = self.advance_interaction(status);

        if let Ok(Some(status)) = advance {
            *self
                .outcome
                .lock()
                .expect("smoke outcome lock is available") = SmokeOutcome::Passed(status);
            self.closing = true;
            context.send_viewport_cmd_to(egui::ViewportId::ROOT, egui::ViewportCommand::Close);
        } else if let Err(detail) = advance {
            *self
                .outcome
                .lock()
                .expect("smoke outcome lock is available") = SmokeOutcome::Failed(detail);
            self.closing = true;
            context.send_viewport_cmd_to(egui::ViewportId::ROOT, egui::ViewportCommand::Close);
        } else if self.started.elapsed() >= STARTUP_TIMEOUT {
            let engine = self.runtime.dockspace().engine();
            let surfaces = engine
                .workspace()
                .surfaces()
                .map(|(surface, _)| surface)
                .collect::<Vec<_>>();
            let scenes = surfaces
                .iter()
                .map(|surface| {
                    (
                        *surface,
                        scene_kind(engine.scene().surface(*surface)),
                        engine
                            .viewport()
                            .viewport(*surface)
                            .map(|viewport| (viewport.role(), viewport.lifecycle())),
                    )
                })
                .collect::<Vec<_>>();
            let native_creates = engine
                .viewport()
                .native_create_sagas()
                .map(|(saga, create)| (saga, create.binding(), create.phase()))
                .collect::<Vec<_>>();
            let presentation = engine.presentation_ledger_diagnostics();
            let retention = engine.runtime_retention_manifest();
            let detail = format!(
                "native dynamic tear-off/redock timed out in {:?} after {} cycles: \
                 {status:?}, interaction={:?}, surfaces={surfaces:?}, scenes={scenes:?}, \
                 native_creates={native_creates:?}, preview={:?}, presentation={presentation:?}, \
                 retention={retention:?}, renderer={:?}",
                self.phase,
                status.committed_cycles,
                engine.interaction().status(),
                engine.presentation_preview(),
                self.renderer.snapshot(),
            );
            *self
                .outcome
                .lock()
                .expect("smoke outcome lock is available") = SmokeOutcome::Failed(detail);
            self.closing = true;
            context.send_viewport_cmd_to(egui::ViewportId::ROOT, egui::ViewportCommand::Close);
        } else {
            context.request_repaint_after_for(Duration::from_millis(16), egui::ViewportId::ROOT);
        }
    }

    fn advance_interaction(
        &mut self,
        status: NativeRuntimeStatus,
    ) -> Result<Option<NativeRuntimeStatus>, String> {
        let cycle_advanced = status.committed_cycles > self.phase_cycle;
        match self.phase {
            SmokePhase::AwaitRoot => {
                let engine = self.runtime.dockspace().engine();
                if status.live_viewports == 1
                    && engine.interaction_authority(ROOT_SURFACE).is_some()
                    && let Some(point) = tab_drag_point(engine, ROOT_SURFACE)
                {
                    self.queue_pointer(
                        NativeTestPointerEvent::new(
                            egui::ViewportId::ROOT,
                            point,
                            NativeTestPointerAction::PrimaryPressed,
                        ),
                        SmokePhase::RootPressQueued,
                        status.committed_cycles,
                    )?;
                }
            }
            SmokePhase::RootPressQueued if cycle_advanced => {
                self.queue_pointer(
                    NativeTestPointerEvent::outside_all(
                        OUTSIDE_ALL_POINT,
                        NativeTestPointerAction::Move,
                    ),
                    SmokePhase::OutsideMoveQueued,
                    status.committed_cycles,
                )?;
            }
            SmokePhase::OutsideMoveQueued if cycle_advanced => {
                let native_preview = self
                    .runtime
                    .dockspace()
                    .engine()
                    .presentation_preview()
                    .is_some_and(|preview| preview.visual().target_surface().is_some());
                if native_preview {
                    self.queue_pointer(
                        NativeTestPointerEvent::outside_all(
                            OUTSIDE_ALL_POINT,
                            NativeTestPointerAction::PrimaryReleased,
                        ),
                        SmokePhase::OutsideReleaseQueued,
                        status.committed_cycles,
                    )?;
                }
            }
            SmokePhase::OutsideReleaseQueued => {
                let engine = self.runtime.dockspace().engine();
                if status.live_viewports == 2
                    && let Some(child) = dynamic_child_surface(engine.workspace())
                    && engine.interaction_authority(child).is_some()
                    && let Some(point) = tab_drag_point(engine, child)
                {
                    self.queue_pointer(
                        NativeTestPointerEvent::unique_child(
                            point,
                            NativeTestPointerAction::PrimaryPressed,
                        ),
                        SmokePhase::ChildPressQueued,
                        status.committed_cycles,
                    )?;
                }
            }
            SmokePhase::ChildPressQueued if cycle_advanced => {
                let point = redock_drop_point(self.runtime.dockspace().engine(), ROOT_SURFACE)
                    .ok_or_else(|| {
                        "root surface published no authoritative background or center drop target"
                            .to_owned()
                    })?;
                self.redock_point = Some(point);
                self.queue_pointer(
                    NativeTestPointerEvent::new(
                        egui::ViewportId::ROOT,
                        point,
                        NativeTestPointerAction::Move,
                    ),
                    SmokePhase::RootMoveQueued,
                    status.committed_cycles,
                )?;
            }
            SmokePhase::RootMoveQueued if cycle_advanced => {
                let root_preview = self
                    .runtime
                    .dockspace()
                    .engine()
                    .presentation_preview()
                    .is_some_and(|preview| {
                        preview.visual().surface() == ROOT_SURFACE
                            && preview.visual().target_surface().is_none()
                    });
                if root_preview {
                    let point = self
                        .redock_point
                        .ok_or_else(|| "redock point disappeared before release".to_owned())?;
                    self.queue_pointer(
                        NativeTestPointerEvent::new(
                            egui::ViewportId::ROOT,
                            point,
                            NativeTestPointerAction::PrimaryReleased,
                        ),
                        SmokePhase::RootReleaseQueued,
                        status.committed_cycles,
                    )?;
                }
            }
            SmokePhase::RootReleaseQueued => {
                let engine = self.runtime.dockspace().engine();
                let workspace = engine.workspace();
                let surfaces = workspace
                    .surfaces()
                    .map(|(surface, _)| surface)
                    .collect::<Vec<_>>();
                let retained_presentation_streams = engine
                    .runtime_retention_manifest()
                    .presentation_hosts()
                    .retained_stream_states();
                if status.live_viewports == 1
                    && surfaces == [ROOT_SURFACE]
                    && workspace.item_multiset() == expected_item_multiset()
                    && engine.interaction_authority(ROOT_SURFACE).is_some()
                    && retained_presentation_streams == 1
                    && let Some(projection) = engine.interaction_projection(ROOT_SURFACE)
                {
                    self.overflow_predecessor = Some(projection.output_ticket());
                    self.runtime
                        .set_style(overflow_style())
                        .map_err(|error| format!("failed to queue overflow style: {error}"))?;
                    self.phase = SmokePhase::OverflowStyleQueued;
                    self.phase_cycle = status.committed_cycles;
                }
            }
            SmokePhase::OverflowStyleQueued if cycle_advanced => {
                let engine = self.runtime.dockspace().engine();
                if engine.interaction_authority(ROOT_SURFACE).is_some()
                    && let Some((output, point, offset, maximum)) =
                        tab_scroll_state(engine, ROOT_SURFACE)
                    && Some(output) != self.overflow_predecessor
                    && maximum >= EXPECTED_SCROLL_OFFSET
                {
                    if offset != 0.0 {
                        return Err(format!(
                            "overflow tab strip started with a non-zero scroll offset: {offset}"
                        ));
                    }
                    self.scroll_predecessor = Some(output);
                    self.queue_scroll(point, status.committed_cycles)?;
                } else if status.committed_cycles.saturating_sub(self.phase_cycle) >= 8 {
                    let workspace = engine.workspace();
                    let nodes = workspace
                        .nodes()
                        .map(|(id, node)| (id, node.clone()))
                        .collect::<Vec<_>>();
                    let bars = engine
                        .scene()
                        .surface(ROOT_SURFACE)
                        .and_then(SurfaceScene::ready)
                        .map(|ready| {
                            ready
                                .plan()
                                .tab_bar_records()
                                .iter()
                                .map(|bar| {
                                    (
                                        *bar.id(),
                                        bar.members().len(),
                                        bar.viewport().width(),
                                        bar.scroll_offset(),
                                        bar.maximum_scroll_offset(),
                                    )
                                })
                                .collect::<Vec<_>>()
                        });
                    return Err(format!(
                        "overflow style did not produce an authoritative scrollable tab strip: \
                         tab_min_width={}, nodes={nodes:?}, bars={bars:?}",
                        self.runtime.dockspace().style().tab_min_width,
                    ));
                }
            }
            SmokePhase::RootScrollQueued if cycle_advanced => {
                let engine = self.runtime.dockspace().engine();
                if engine.interaction_authority(ROOT_SURFACE).is_some()
                    && let Some((output, _, offset, maximum)) =
                        tab_scroll_state(engine, ROOT_SURFACE)
                    && Some(output) != self.scroll_predecessor
                {
                    if (offset - EXPECTED_SCROLL_OFFSET).abs() > f64::EPSILON {
                        return Err(format!(
                            "native scroll did not reach the core-owned tab-strip state: \
                             offset={offset}, expected={EXPECTED_SCROLL_OFFSET}, maximum={maximum}"
                        ));
                    }
                    return Ok(Some(status));
                }
            }
            SmokePhase::RootPressQueued
            | SmokePhase::OutsideMoveQueued
            | SmokePhase::ChildPressQueued
            | SmokePhase::RootMoveQueued
            | SmokePhase::OverflowStyleQueued
            | SmokePhase::RootScrollQueued => {}
        }
        Ok(None)
    }

    fn queue_pointer(
        &mut self,
        event: NativeTestPointerEvent,
        next: SmokePhase,
        committed_cycles: u64,
    ) -> Result<(), String> {
        self.driver
            .send_pointer(event)
            .map_err(|error| format!("native test event loop closed: {error}"))?;
        self.phase = next;
        self.phase_cycle = committed_cycles;
        Ok(())
    }

    fn queue_scroll(&mut self, position: egui::Pos2, committed_cycles: u64) -> Result<(), String> {
        self.driver
            .send_pointer(NativeTestPointerEvent::new(
                egui::ViewportId::ROOT,
                position,
                NativeTestPointerAction::Scroll(NativeTestScrollDelta::lines(0.0, SCROLL_LINES)),
            ))
            .map_err(|error| format!("native test event loop closed: {error}"))?;
        self.phase = SmokePhase::RootScrollQueued;
        self.phase_cycle = committed_cycles;
        Ok(())
    }
}

fn scene_kind(scene: Option<&SurfaceScene>) -> &'static str {
    match scene {
        Some(SurfaceScene::Ready(_)) => "ready",
        Some(SurfaceScene::Stale(_)) => "stale",
        Some(SurfaceScene::Bootstrap(_)) => "bootstrap",
        None => "absent",
    }
}

fn dynamic_child_surface(workspace: &Workspace) -> Option<SurfaceId> {
    let children = workspace
        .surfaces()
        .map(|(surface, _)| surface)
        .filter(|surface| *surface != ROOT_SURFACE)
        .collect::<Vec<_>>();
    let [child] = children.as_slice() else {
        return None;
    };
    Some(*child)
}

fn tab_drag_point(
    engine: &dockspace::engine::DockEngine,
    surface: SurfaceId,
) -> Option<egui::Pos2> {
    let ready = engine.scene().surface(surface)?.ready()?;
    logical_rect_center(ready.plan().tab_records().first()?.drag_hit().rect())
}

fn redock_drop_point(
    engine: &dockspace::engine::DockEngine,
    surface: SurfaceId,
) -> Option<egui::Pos2> {
    let ready = engine.scene().surface(surface)?.ready()?;
    let plan = ready.plan();
    let region = plan
        .drop_targets()
        .iter()
        .find(|target| {
            target.availability().is_available()
                && target.id().kind() == dockspace::drop_target::DropTargetKind::Center
        })
        .map(|target| target.region())
        .or_else(|| {
            plan.drop_guide_clusters().iter().find_map(|cluster| {
                cluster
                    .target(dockspace::drop_guide::DropGuideSlot::Center)
                    .map(|target| target.target())
                    .filter(|target| target.availability().is_available())
                    .map(|target| target.region())
            })
        })
        .or_else(|| plan.surface_background().map(|target| target.region()))?;
    logical_rect_center(region.rect())
}

fn tab_scroll_state(
    engine: &dockspace::engine::DockEngine,
    surface: SurfaceId,
) -> Option<(SurfacePresentationOutputTicket, egui::Pos2, f64, f64)> {
    let projection = engine.interaction_projection(surface)?;
    let bar = projection.plan().tab_bar_records().first()?;
    Some((
        projection.output_ticket(),
        logical_rect_center(bar.viewport())?,
        bar.scroll_offset(),
        bar.maximum_scroll_offset(),
    ))
}

fn overflow_style() -> DockStyle {
    DockStyle {
        tab_min_width: OVERFLOW_TAB_WIDTH,
        tab_max_width: OVERFLOW_TAB_WIDTH,
        ..DockStyle::default()
    }
}

fn logical_rect_center(rect: dockspace::geometry::LogicalRect) -> Option<egui::Pos2> {
    let x = (rect.min().x() + rect.max().x()) * 0.5;
    let y = (rect.min().y() + rect.max().y()) * 0.5;
    if !x.is_finite()
        || !y.is_finite()
        || !(f64::from(f32::MIN)..=f64::from(f32::MAX)).contains(&x)
        || !(f64::from(f32::MIN)..=f64::from(f32::MAX)).contains(&y)
    {
        return None;
    }
    let point = egui::pos2(x as f32, y as f32);
    point.is_finite().then_some(point)
}

impl eframe::App for SmokeApp {
    fn hosted_viewport_mode(&self) -> eframe::HostedViewportMode {
        self.runtime.hosted_viewport_mode()
    }

    fn begin_hosted_viewport_cycle(
        &mut self,
        context: &egui::Context,
        cycle: &eframe::HostedViewportCycle,
        frame: &mut eframe::Frame,
    ) -> eframe::HostedViewportAppResult<()> {
        self.runtime
            .begin_hosted_viewport_cycle(context, cycle, frame)
    }

    fn hosted_viewport_ui(
        &mut self,
        viewport: egui::ViewportId,
        ui: &mut egui::Ui,
        frame: &mut eframe::Frame,
    ) -> eframe::HostedViewportAppResult<eframe::HostedViewportUiDisposition> {
        self.runtime.hosted_viewport_ui(viewport, ui, frame)
    }

    fn end_hosted_viewport_cycle(
        &mut self,
        context: &egui::Context,
        outputs: &mut [eframe::HostedViewportOutput<egui::FullOutput>],
        frame: &mut eframe::Frame,
    ) -> eframe::HostedViewportAppResult<()> {
        self.runtime
            .end_hosted_viewport_cycle(context, outputs, frame)
    }

    fn commit_hosted_viewport_cycle(
        &mut self,
        context: &egui::Context,
        outputs: &mut [eframe::HostedViewportOutput<egui::FullOutput>],
        frame: &mut eframe::Frame,
    ) -> eframe::HostedViewportAppResult<()> {
        self.runtime
            .commit_hosted_viewport_cycle(context, outputs, frame)?;
        self.observe_readiness(context);
        Ok(())
    }

    fn abort_hosted_viewport_cycle(&mut self, context: &egui::Context, frame: &mut eframe::Frame) {
        self.runtime.abort_hosted_viewport_cycle(context, frame);
    }

    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        self.runtime.ui(ui, frame);
    }
}

fn main() -> Result<(), Box<dyn Error>> {
    let outcome = Arc::new(Mutex::new(SmokeOutcome::Pending));
    let shared = Arc::clone(&outcome);
    let renderer = Arc::new(RendererCounts::default());
    let app_renderer = Arc::clone(&renderer);
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("egui_dockspace native E2E root")
            .with_inner_size([420.0, 360.0])
            .with_position([40.0, 60.0]),
        presentation_result_hook: Some(Arc::new({
            let renderer = Arc::clone(&renderer);
            move |result| renderer.record(result.outcome())
        })),
        persist_window: false,
        ..Default::default()
    };
    eframe::run_native(
        "egui_dockspace native E2E",
        options,
        Box::new(move |creation_context| {
            let driver = creation_context.native_test_driver.clone().ok_or_else(|| {
                io::Error::other("eframe native-test-support driver is unavailable")
            })?;
            Ok(Box::new(SmokeApp::new(shared, app_renderer, driver)?))
        }),
    )?;

    renderer.validate_native_outcomes()?;

    match outcome
        .lock()
        .expect("smoke outcome lock is available")
        .clone()
    {
        SmokeOutcome::Passed(status) => {
            println!("native-e2e passed: {status:?}");
            Ok(())
        }
        SmokeOutcome::Failed(detail) => Err(io::Error::other(detail).into()),
        SmokeOutcome::Pending => Err(io::Error::other(
            "native root closed before the first-live two-window barrier completed",
        )
        .into()),
    }
}

#[derive(Default)]
struct RendererCounts {
    submitted: AtomicU64,
    swapped: AtomicU64,
    skipped: AtomicU64,
    failed: AtomicU64,
    browser_canvas: AtomicU64,
}

impl RendererCounts {
    fn record(&self, outcome: &egui::PaintOutcome) {
        let counter = match outcome {
            egui::PaintOutcome::SubmittedToBrowserCanvas => &self.browser_canvas,
            egui::PaintOutcome::SubmittedToSwapchain => &self.submitted,
            egui::PaintOutcome::Swapped => &self.swapped,
            egui::PaintOutcome::Skipped(_) => &self.skipped,
            egui::PaintOutcome::Failed(_) => &self.failed,
        };
        counter.fetch_add(1, Ordering::Relaxed);
    }

    fn snapshot(&self) -> [u64; 5] {
        [
            self.submitted.load(Ordering::Relaxed),
            self.swapped.load(Ordering::Relaxed),
            self.skipped.load(Ordering::Relaxed),
            self.failed.load(Ordering::Relaxed),
            self.browser_canvas.load(Ordering::Relaxed),
        ]
    }

    fn validate_native_outcomes(&self) -> io::Result<()> {
        let failed = self.failed.load(Ordering::Relaxed);
        let browser_canvas = self.browser_canvas.load(Ordering::Relaxed);
        if failed == 0 && browser_canvas == 0 {
            return Ok(());
        }
        Err(io::Error::other(format!(
            "native renderer reported {failed} failed and {browser_canvas} browser-canvas outcomes"
        )))
    }
}

fn workspace() -> Workspace {
    let mut builder = Workspace::builder();
    let root_tabs = builder.insert_node(Node::tabs(root_items()));
    builder.set_root(ROOT, RootRecord::new(root_tabs).with_central(root_tabs));
    builder.set_surface(ROOT_SURFACE, SurfacePresentation::with_main(ROOT));
    builder.build().expect("the native E2E workspace is valid")
}

fn expected_item_multiset() -> BTreeMap<ItemId, usize> {
    root_items().map(|item| (item, 1)).collect()
}

fn root_items() -> impl Iterator<Item = ItemId> {
    (1..=ROOT_ITEM_COUNT).map(ItemId::new)
}

struct SmokePanes;

impl SmokePanes {
    fn new() -> Self {
        Self
    }
}

impl PaneView for SmokePanes {
    fn title(&self, item: ItemId) -> Option<egui::WidgetText> {
        Some(format!("Pane {}", item.get()).into())
    }

    fn ui(&mut self, item: ItemId, ui: &mut egui::Ui) {
        ui.label(format!("Pane {}", item.get()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_renderer_gate_rejects_terminal_failure() {
        let renderer = RendererCounts::default();
        renderer.record(&egui::PaintOutcome::SubmittedToSwapchain);
        renderer.record(&egui::PaintOutcome::Swapped);
        assert!(renderer.validate_native_outcomes().is_ok());

        renderer.record(&egui::PaintOutcome::Failed(
            egui::PaintFailure::RendererUnavailable,
        ));
        assert!(renderer.validate_native_outcomes().is_err());
    }
}
