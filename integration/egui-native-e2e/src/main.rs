use std::collections::BTreeMap;
use std::error::Error;
use std::io;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use dockspace::command::Edge;
use dockspace::drop_target::DropTargetId;
use dockspace::graph::{Axis, Node, RootRecord, SurfacePresentation, Workspace};
use dockspace::ids::{ItemId, RootId, SurfaceId};
use dockspace::interaction::PreviewVisual;
use dockspace::policy::DockPolicy;
use dockspace::viewport::WindowToken;
use eframe::egui;
use eframe::{NativeTestDriver, NativeTestPointerAction, NativeTestPointerEvent};
use egui_dockspace::{Dockspace, PaneView};
use egui_dockspace_native::{
    AllowNativeClose, NativeDockspaceApp, NativeRuntimeStatus, NativeSurfaceSpec,
    NativeViewportRoster,
};

const ROOT_SURFACE: SurfaceId = SurfaceId::new(1);
const ROOT: RootId = RootId::new(1);
const ROOT_ITEM_COUNT: u64 = 3;
const GROUP_ITEM_COUNT: u64 = 2;
const STARTUP_TIMEOUT: Duration = Duration::from_secs(30);
const OUTSIDE_ALL_POINT: egui::Pos2 = egui::pos2(200.0, 800.0);

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
}

struct SmokeApp {
    runtime: NativeDockspaceApp<SmokePanes>,
    outcome: Arc<Mutex<SmokeOutcome>>,
    driver: NativeTestDriver,
    started: Instant,
    phase: SmokePhase,
    phase_cycle: u64,
    redock_point: Option<egui::Pos2>,
    redock_target: Option<DropTargetId>,
    closing: bool,
}

impl SmokeApp {
    fn new(
        outcome: Arc<Mutex<SmokeOutcome>>,
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
            driver,
            started: Instant::now(),
            phase: SmokePhase::AwaitRoot,
            phase_cycle: 0,
            redock_point: None,
            redock_target: None,
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
            let detail = format!(
                "native tear-off/redock timed out in {:?} after {} cycles: {status:?}",
                self.phase, status.committed_cycles,
            );
            *self
                .outcome
                .lock()
                .expect("smoke outcome lock is available") = SmokeOutcome::Failed(detail);
            self.closing = true;
            context.send_viewport_cmd_to(egui::ViewportId::ROOT, egui::ViewportCommand::Close);
        }
    }

    fn advance_interaction(
        &mut self,
        status: NativeRuntimeStatus,
    ) -> Result<Option<NativeRuntimeStatus>, String> {
        let cycle_advanced = status.committed_cycles > self.phase_cycle;
        match self.phase {
            SmokePhase::AwaitRoot => {
                let dockspace = self.runtime.dockspace();
                if status.live_viewports == 1
                    && dockspace.backend_surface_is_interactive(ROOT_SURFACE)
                    && let Some(point) = tab_group_drag_point(dockspace, ROOT_SURFACE)
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
                    .backend_presentation_preview()
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
                let dockspace = self.runtime.dockspace();
                if status.live_viewports == 2
                    && let Some(child) = dynamic_child_surface(dockspace.workspace())
                    && dockspace.backend_surface_is_interactive(child)
                    && let Some(point) = tab_group_drag_point(dockspace, child)
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
                let (point, target) = redock_drop_target(self.runtime.dockspace(), ROOT_SURFACE)
                    .ok_or_else(|| {
                        "root surface published no authoritative left-edge drop target".to_owned()
                    })?;
                self.redock_point = Some(point);
                self.redock_target = Some(target);
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
                let expected_target = self
                    .redock_target
                    .ok_or_else(|| "redock target disappeared before preview".to_owned())?;
                let root_preview = self
                    .runtime
                    .dockspace()
                    .backend_presentation_preview()
                    .is_some_and(|preview| {
                        matches!(
                            preview.visual(),
                            PreviewVisual::Dock {
                                surface: ROOT_SURFACE,
                                target,
                                ..
                            } if *target == expected_target
                        )
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
                let dockspace = self.runtime.dockspace();
                let workspace = dockspace.workspace();
                let surfaces = workspace
                    .surfaces()
                    .map(|(surface, _)| surface)
                    .collect::<Vec<_>>();
                if status.live_viewports == 1
                    && surfaces == [ROOT_SURFACE]
                    && workspace.item_multiset() == expected_item_multiset()
                    && dockspace.backend_surface_is_interactive(ROOT_SURFACE)
                {
                    return Ok(Some(status));
                }
            }
            SmokePhase::RootPressQueued
            | SmokePhase::OutsideMoveQueued
            | SmokePhase::ChildPressQueued
            | SmokePhase::RootMoveQueued => {}
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

fn tab_group_drag_point(dockspace: &Dockspace, surface: SurfaceId) -> Option<egui::Pos2> {
    let plan = dockspace.backend_ready_presentation_plan(surface)?;
    let expected = group_items().collect::<Vec<_>>();
    let bar = plan.tab_bar_records().iter().find(|bar| {
        bar.members()
            .iter()
            .map(|member| member.tab().item)
            .eq(expected.iter().copied())
    })?;
    logical_rect_center(bar.group_drag()?.grip_bounds())
}

fn redock_drop_target(
    dockspace: &Dockspace,
    surface: SurfaceId,
) -> Option<(egui::Pos2, DropTargetId)> {
    let plan = dockspace.backend_ready_presentation_plan(surface)?;
    let target = plan.drop_guide_clusters().iter().find_map(|cluster| {
        cluster
            .target(dockspace::drop_guide::DropGuideSlot::Edge(Edge::Left))
            .map(|target| target.target())
            .filter(|target| target.availability().is_available())
    })?;
    Some((logical_rect_center(target.region().rect())?, target.id()))
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
        self.observe_readiness(context);
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
        outputs: &mut [eframe::HostedViewportOutput<egui::FullOutput>],
    ) -> eframe::HostedViewportAppResult<eframe::HostedViewportCommitDirective> {
        self.runtime.commit_hosted_viewport_cycle(outputs)
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
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("egui_dockspace native E2E root")
            .with_inner_size([420.0, 360.0])
            .with_position([40.0, 60.0]),
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
            Ok(Box::new(SmokeApp::new(shared, driver)?))
        }),
    )?;

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

fn workspace() -> Workspace {
    let mut builder = Workspace::builder();
    let group = builder.insert_node(Node::tabs(group_items()));
    let central = builder.insert_node(Node::tabs([ItemId::new(ROOT_ITEM_COUNT)]));
    let root = builder.insert_node(
        Node::equal_split(Axis::Horizontal, [group, central])
            .expect("two root branches form a valid split"),
    );
    builder.set_root(ROOT, RootRecord::new(root).with_central(central));
    builder.set_surface(ROOT_SURFACE, SurfacePresentation::with_main(ROOT));
    builder.build().expect("the native E2E workspace is valid")
}

fn expected_item_multiset() -> BTreeMap<ItemId, usize> {
    root_items().map(|item| (item, 1)).collect()
}

fn root_items() -> impl Iterator<Item = ItemId> {
    (1..=ROOT_ITEM_COUNT).map(ItemId::new)
}

fn group_items() -> impl Iterator<Item = ItemId> {
    (1..=GROUP_ITEM_COUNT).map(ItemId::new)
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
