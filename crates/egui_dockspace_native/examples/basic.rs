#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::collections::BTreeMap;

use dockspace::geometry::{LogicalRect, PhysicalRect};
use dockspace::model::{
    DockspaceActionOutcome, DockspaceAxis, DockspaceContainedLayout, DockspaceLayout,
    DockspaceNode, DockspaceRootLayout, DockspaceSurfaceLayout, FloatingPresentationId, ItemId,
    NativeWindowPlacement, RootId, SurfaceId,
};
use dockspace::policy::DockPolicy;
use dockspace::runtime::DockspaceSession;
use eframe::egui;
use egui_dockspace::{DockStyle, DockspaceActionStatus, PaneView};
use egui_dockspace_native::NativeDockspaceApp;

const SURFACE: SurfaceId = SurfaceId::new(1);
const MAIN_ROOT: RootId = RootId::new(1);
const INSPECTOR_ROOT: RootId = RootId::new(2);
const INSPECTOR_FLOATING: FloatingPresentationId = FloatingPresentationId::new(1);
const OUTLINE: ItemId = ItemId::new(1);
const EDITOR: ItemId = ItemId::new(2);
const PREVIEW: ItemId = ItemId::new(3);
const INSPECTOR: ItemId = ItemId::new(4);

fn main() -> eframe::Result {
    let mut policy = DockPolicy::default();
    policy.set_allow_native_surfaces(true);
    let session = DockspaceSession::from_layout(example_layout(), policy)
        .expect("the static native example layout is valid");
    let dockspace = NativeDockspaceApp::new(
        egui::Id::new("native-dockspace-example"),
        session,
        SURFACE,
        ExamplePanes::new(),
        DockStyle::default(),
    )
    .expect("the native coordinator initializes");
    let native_host = dockspace.native_host_handler();
    let app = NativeExampleApp {
        dockspace,
        auto_open: true,
        action: InspectorActionState::Idle,
        status: None,
    };
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([1100.0, 720.0]),
        native_host: Some(native_host),
        ..Default::default()
    };
    eframe::run_native(
        "egui_dockspace native",
        options,
        Box::new(move |_| Ok(Box::new(app))),
    )
}

struct NativeExampleApp {
    dockspace: NativeDockspaceApp<ExamplePanes>,
    auto_open: bool,
    action: InspectorActionState,
    status: Option<String>,
}

#[derive(Clone, Copy, Default)]
enum InspectorActionState {
    #[default]
    Idle,
    AwaitingTearOffOutcome,
    AwaitingFirstPresentation(SurfaceId),
    AwaitingDockBackOutcome,
}

impl InspectorActionState {
    const fn is_pending(self) -> bool {
        !matches!(self, Self::Idle)
    }
}

impl eframe::App for NativeExampleApp {
    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        if matches!(
            self.action,
            InspectorActionState::AwaitingTearOffOutcome
                | InspectorActionState::AwaitingDockBackOutcome
        ) {
            self.collect_action_status();
        }
        egui::Panel::top("native-example-controls").show(ui, |ui| {
            self.show_controls(ui);
        });
        egui::CentralPanel::default().show(ui, |ui| {
            <NativeDockspaceApp<ExamplePanes> as eframe::App>::ui(&mut self.dockspace, ui, frame);
        });
    }
}

impl NativeExampleApp {
    fn show_controls(&mut self, ui: &mut egui::Ui) {
        self.finish_first_live_wait();
        if self.action.is_pending() {
            self.show_pending_controls(ui);
            return;
        }
        let (inspector_is_contained, inspector_is_detached) = self
            .dockspace
            .with_view(|view| {
                let location = view.item(INSPECTOR);
                (
                    location.is_some_and(|item| {
                        item.surface() == SURFACE && item.contained().is_some()
                    }),
                    location.is_some_and(|item| item.surface() != SURFACE),
                )
            })
            .unwrap_or_default();
        let root_presented = !inspector_is_detached && self.dockspace.is_surface_presented(SURFACE);
        if self.auto_open && root_presented && inspector_is_contained {
            self.auto_open = false;
            self.request_inspector_window();
        }
        if self.action.is_pending() {
            self.show_pending_controls(ui);
            return;
        }

        ui.horizontal_wrapped(|ui| {
            ui.strong("Native multiview demo");
            ui.separator();
            let enabled = root_presented && inspector_is_contained;
            if ui
                .add_enabled(enabled, egui::Button::new("Open Inspector in New Window ↗"))
                .clicked()
            {
                self.request_inspector_window();
            }
            if inspector_is_detached {
                if ui.button("Dock Inspector Back").clicked() {
                    self.request_inspector_dock_back();
                }
                ui.label("Inspector is live in a second OS window; closing it also docks it back.");
            } else if !root_presented {
                ui.label("Waiting for the root output to be presented…");
            } else {
                ui.label(
                    "The demo opens one native child automatically; the button is the manual path.",
                );
            }
        });
        if let Some(status) = &self.status {
            ui.small(status);
        }
    }

    fn show_pending_controls(&self, ui: &mut egui::Ui) {
        ui.horizontal_wrapped(|ui| {
            ui.strong("Native multiview demo");
            ui.separator();
            ui.spinner();
            match self.action {
                InspectorActionState::AwaitingDockBackOutcome => {
                    ui.label("Presenting Inspector back in the root dockspace…");
                }
                _ => {
                    ui.label("Creating and presenting the native child window…");
                }
            }
        });
        if let Some(status) = &self.status {
            ui.small(status);
        }
    }

    fn request_inspector_window(&mut self) {
        let placement = NativeWindowPlacement::new(
            PhysicalRect::new(760.0, 120.0, 440.0, 340.0)
                .expect("the static child placement is valid"),
        );
        match self
            .dockspace
            .request_tear_off_root(INSPECTOR_ROOT, placement)
        {
            Ok(()) => {
                self.action = InspectorActionState::AwaitingTearOffOutcome;
                self.status = None;
            }
            Err(error) => self.status = Some(format!("Could not open native window: {error}")),
        }
    }

    fn request_inspector_dock_back(&mut self) {
        match self.dockspace.request_dock_root_back(INSPECTOR_ROOT) {
            Ok(()) => {
                self.action = InspectorActionState::AwaitingDockBackOutcome;
                self.status = None;
            }
            Err(error) => self.status = Some(format!("Could not dock Inspector back: {error}")),
        }
    }

    fn collect_action_status(&mut self) {
        let Some(status) = self.dockspace.take_action_status() else {
            return;
        };
        match (self.action, status) {
            (
                InspectorActionState::AwaitingTearOffOutcome,
                DockspaceActionStatus::Applied(
                    DockspaceActionOutcome::NativeRootTearOffRequested { target_surface, .. },
                ),
            ) => {
                self.action = InspectorActionState::AwaitingFirstPresentation(target_surface);
                self.status = Some(
                    "Native child creation accepted; waiting for first live presentation."
                        .to_owned(),
                );
            }
            (
                InspectorActionState::AwaitingDockBackOutcome,
                DockspaceActionStatus::Applied(DockspaceActionOutcome::RootDocked {
                    root: INSPECTOR_ROOT,
                    changed: true,
                    ..
                }),
            ) => {
                self.action = InspectorActionState::Idle;
                self.status =
                    Some("Inspector was presented back in the root dockspace.".to_owned());
            }
            (_, DockspaceActionStatus::Applied(outcome)) => {
                self.action = InspectorActionState::Idle;
                self.status = Some(format!(
                    "Unexpected native demo action outcome: {outcome:?}"
                ));
            }
            (_, DockspaceActionStatus::Rejected(reason)) => {
                self.action = InspectorActionState::Idle;
                self.status = Some(format!("The presentation action was rejected: {reason:?}"));
            }
            (_, DockspaceActionStatus::Stale { .. }) => {
                self.action = InspectorActionState::Idle;
                self.status =
                    Some("The layout changed before the request committed; try again.".to_owned());
            }
            (_, DockspaceActionStatus::PresentationFailed { reason, .. }) => {
                self.action = InspectorActionState::Idle;
                self.status = Some(format!(
                    "The target presentation did not complete: {reason:?}"
                ));
            }
        }
    }

    fn finish_first_live_wait(&mut self) {
        let InspectorActionState::AwaitingFirstPresentation(surface) = self.action else {
            return;
        };
        if self.dockspace.is_surface_presented(surface) {
            self.action = InspectorActionState::Idle;
            self.status = Some("Native child reached its first live presentation.".to_owned());
        }
    }
}

fn example_layout() -> DockspaceLayout {
    let root = DockspaceNode::split(
        DockspaceAxis::Horizontal,
        [
            (DockspaceNode::tabs([OUTLINE]), 0.24),
            (DockspaceNode::central_tabs([EDITOR]), 0.76),
        ],
    )
    .expect("the static split is valid");
    let inspector = DockspaceContainedLayout::new(
        INSPECTOR_FLOATING,
        DockspaceRootLayout::new(
            INSPECTOR_ROOT,
            DockspaceNode::central_tabs([PREVIEW, INSPECTOR]),
        ),
        LogicalRect::new(690.0, 90.0, 330.0, 310.0).expect("the static contained rect is valid"),
    );
    DockspaceLayout::new([DockspaceSurfaceLayout::new(
        SURFACE,
        DockspaceRootLayout::new(MAIN_ROOT, root),
    )
    .with_contained(inspector)])
    .expect("the static surface is valid")
}

struct ExamplePane {
    title: &'static str,
    text: String,
}

struct ExamplePanes {
    panes: BTreeMap<ItemId, ExamplePane>,
}

impl ExamplePanes {
    fn new() -> Self {
        Self {
            panes: BTreeMap::from([
                (
                    OUTLINE,
                    ExamplePane {
                        title: "Outline",
                        text: "src\n  main.rs\n  docking.rs".to_owned(),
                    },
                ),
                (
                    EDITOR,
                    ExamplePane {
                        title: "Editor",
                        text: "fn main() {\n    println!(\"dockspace\");\n}".to_owned(),
                    },
                ),
                (
                    PREVIEW,
                    ExamplePane {
                        title: "Preview",
                        text: "This contained root can become a native child window.".to_owned(),
                    },
                ),
                (
                    INSPECTOR,
                    ExamplePane {
                        title: "Inspector",
                        text: "This root opens automatically in a second OS window; the toolbar is the manual fallback."
                            .to_owned(),
                    },
                ),
            ]),
        }
    }
}

impl PaneView for ExamplePanes {
    fn title(&self, item: ItemId) -> Option<egui::WidgetText> {
        self.panes.get(&item).map(|pane| pane.title.into())
    }

    fn ui(&mut self, item: ItemId, ui: &mut egui::Ui) {
        let Some(pane) = self.panes.get_mut(&item) else {
            ui.label("Missing pane");
            return;
        };
        ui.heading(pane.title);
        ui.add(
            egui::TextEdit::multiline(&mut pane.text)
                .desired_width(f32::INFINITY)
                .desired_rows(16),
        );
    }
}
