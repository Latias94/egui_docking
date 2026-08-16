//! One-shot X11/Glow smoke for the real native coordinator.

use std::collections::BTreeMap;
use std::error::Error;
use std::io;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use dockspace::geometry::{LogicalRect, PhysicalRect};
use dockspace::model::{
    DockAnchor, DockPlacement, DockspaceActionOutcome, DockspaceContainedLayout, DockspaceLayout,
    DockspaceNode, DockspaceRootLayout, DockspaceSurfaceLayout, FloatingPresentationId, ItemId,
    NativeWindowPlacement, RootId, SurfaceId,
};
use dockspace::policy::DockPolicy;
use dockspace::runtime::DockspaceSession;
use eframe::egui::{self, ViewportCommand, ViewportId};
use egui_dockspace::{DockStyle, DockspaceActionStatus, PaneView};
use egui_dockspace_native::{
    NativeActionRequestError, NativeDockspaceApp, NativeWindowClosePolicy,
};

const ROOT_SURFACE: SurfaceId = SurfaceId::new(1);
const MAIN_ROOT: RootId = RootId::new(1);
const CHILD_ROOT: RootId = RootId::new(2);
const CHILD_FLOATING: FloatingPresentationId = FloatingPresentationId::new(1);
const MAIN_ITEM: ItemId = ItemId::new(1);
const CHILD_ITEM: ItemId = ItemId::new(2);
const CHILD_DETAIL: ItemId = ItemId::new(3);

fn main() -> Result<(), Box<dyn Error>> {
    let mut policy = DockPolicy::default();
    policy.set_allow_native_surfaces(true);
    let session = DockspaceSession::from_layout(smoke_layout(), policy)?;
    let dockspace = NativeDockspaceApp::new_with_close_policy(
        egui::Id::new("egui-native-smoke"),
        session,
        ROOT_SURFACE,
        SmokePanes::new(),
        DockStyle::default(),
        NativeWindowClosePolicy::RetainLayout,
    )?;
    let native_host = dockspace.native_host_handler();
    let outcome = Arc::new(Mutex::new(None));
    let reported_outcome = Arc::clone(&outcome);
    let smoke = SmokeApp {
        dockspace,
        phase: SmokePhase::AwaitRootPresentation,
        deadline: Instant::now() + Duration::from_secs(30),
        outcome: reported_outcome,
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

#[derive(Clone, Copy, Debug)]
enum SmokePhase {
    AwaitRootPresentation,
    QueueTearOff,
    AwaitTearOff,
    AwaitFirstLive { target_surface: SurfaceId },
    QueueRedock { target_surface: SurfaceId },
    AwaitRedock { target_surface: SurfaceId },
    AwaitRetirement { target_surface: SurfaceId },
    Done,
}

struct SmokeApp {
    dockspace: NativeDockspaceApp<SmokePanes>,
    phase: SmokePhase,
    deadline: Instant,
    outcome: Arc<Mutex<Option<Result<(), String>>>>,
}

impl eframe::App for SmokeApp {
    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        <NativeDockspaceApp<SmokePanes> as eframe::App>::ui(&mut self.dockspace, ui, frame);
        self.drive(ui.ctx());
        if !matches!(self.phase, SmokePhase::Done) {
            ui.ctx().request_repaint_after(Duration::from_millis(10));
        }
    }
}

impl SmokeApp {
    fn drive(&mut self, context: &egui::Context) {
        if matches!(self.phase, SmokePhase::Done) {
            return;
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
                    self.transition(SmokePhase::QueueTearOff);
                }
            }
            SmokePhase::QueueTearOff => self.queue_tear_off(context),
            SmokePhase::AwaitTearOff => self.await_tear_off(),
            SmokePhase::AwaitFirstLive { target_surface } => self.await_first_live(target_surface),
            SmokePhase::QueueRedock { target_surface } => {
                self.queue_redock(context, target_surface)
            }
            SmokePhase::AwaitRedock { target_surface } => self.await_redock(target_surface),
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
        match self.dockspace.request_tear_off_root(CHILD_ROOT, placement) {
            Ok(()) => self.transition(SmokePhase::AwaitTearOff),
            Err(error) => self.finish(context, Err(action_request_message(error))),
        }
    }

    fn await_tear_off(&mut self) {
        let Some(status) = self.dockspace.take_action_status() else {
            return;
        };
        match status {
            DockspaceActionStatus::Applied(
                DockspaceActionOutcome::NativeRootTearOffRequested {
                    root,
                    source_surface,
                    target_surface,
                    items,
                },
            ) if root == CHILD_ROOT
                && source_surface == ROOT_SURFACE
                && items == [CHILD_ITEM, CHILD_DETAIL] =>
            {
                self.transition(SmokePhase::AwaitFirstLive { target_surface });
            }
            DockspaceActionStatus::Stale { .. } => {
                self.transition(SmokePhase::AwaitRootPresentation);
            }
            status => self.fail_without_context(format!(
                "tear-off returned an unexpected terminal status: {status:?}"
            )),
        }
    }

    fn await_first_live(&mut self, target_surface: SurfaceId) {
        if !self.dockspace.is_surface_presented(target_surface) {
            return;
        }
        let transferred = self.dockspace.with_view(|view| {
            [CHILD_ITEM, CHILD_DETAIL].into_iter().all(|item| {
                view.item(item)
                    .is_some_and(|pane| pane.surface() == target_surface)
            })
        });
        if transferred == Some(true) {
            self.transition(SmokePhase::QueueRedock { target_surface });
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

    fn await_redock(&mut self, target_surface: SurfaceId) {
        let Some(status) = self.dockspace.take_action_status() else {
            return;
        };
        match status {
            DockspaceActionStatus::Applied(DockspaceActionOutcome::RootDocked {
                root,
                target_root,
                items,
                changed: true,
            }) if root == CHILD_ROOT
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
            DockspaceActionStatus::Stale { .. } => {
                self.transition(SmokePhase::QueueRedock { target_surface });
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
        *self.outcome.lock().unwrap_or_else(PoisonError::into_inner) = Some(Err(message));
        self.phase = SmokePhase::Done;
    }

    fn finish(&mut self, context: &egui::Context, outcome: Result<(), String>) {
        match &outcome {
            Ok(()) => eprintln!("native-smoke: complete"),
            Err(message) => eprintln!("native-smoke: failed: {message}"),
        }
        *self.outcome.lock().unwrap_or_else(PoisonError::into_inner) = Some(outcome);
        self.phase = SmokePhase::Done;
        context.send_viewport_cmd_to(ViewportId::ROOT, ViewportCommand::Close);
    }
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
