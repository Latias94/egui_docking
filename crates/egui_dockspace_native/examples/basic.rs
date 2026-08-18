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
    action: InspectorActionState,
    status: Option<String>,
}

#[derive(Clone, Copy, Default)]
enum InspectorActionState {
    #[default]
    Idle,
    AwaitingTearOffOutcome,
    AwaitingFirstPresentation(SurfaceId),
    AwaitingDockBackOutcome(RootId),
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
                | InspectorActionState::AwaitingDockBackOutcome(_)
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
        let inspector = self
            .dockspace
            .with_view(|view| view.item(INSPECTOR).map(InspectorLocation::from))
            .flatten();
        let root_presented = self.dockspace.is_surface_presented(SURFACE);

        ui.horizontal_wrapped(|ui| {
            ui.strong("Native multiview demo");
            ui.separator();
            let contained_root = inspector.filter(|location| location.is_contained());
            let enabled = root_presented && contained_root.is_some();
            if ui
                .add_enabled(
                    enabled,
                    egui::Button::new("Fallback: Move Inspector Group to Native Window ↗"),
                )
                .on_disabled_hover_text(
                    "This fallback is available while the Inspector group is a live contained window.",
                )
                .clicked()
                && let Some(location) = contained_root
            {
                self.request_inspector_window(location.root);
            }
            if let Some(location) = inspector.filter(|location| location.is_native_child()) {
                if ui.button("Fallback: Dock Inspector Root Back").clicked() {
                    self.request_inspector_dock_back(location.root);
                }
            } else if !root_presented {
                ui.label("Waiting for the root output to be presented…");
            }
        });
        self.show_presentation_state(ui, inspector, root_presented);
        self.show_walkthrough(ui);
        if let Some(status) = &self.status {
            ui.small(format!("Last fallback action: {status}"));
        }
    }

    fn show_pending_controls(&self, ui: &mut egui::Ui) {
        ui.horizontal_wrapped(|ui| {
            ui.strong("Native multiview demo");
            ui.separator();
            ui.spinner();
            match self.action {
                InspectorActionState::AwaitingDockBackOutcome(_) => {
                    ui.label("Presenting Inspector back in the root dockspace…");
                }
                _ => {
                    ui.label("Creating and presenting the native child window…");
                }
            }
        });
        if let Some(status) = &self.status {
            ui.small(format!("Last fallback action: {status}"));
        }
    }

    fn show_presentation_state(
        &self,
        ui: &mut egui::Ui,
        inspector: Option<InspectorLocation>,
        root_presented: bool,
    ) {
        let state = match inspector {
            Some(location) if location.is_contained() && root_presented => {
                "Inspector presentation: contained floating window in the root egui surface."
            }
            Some(location) if location.is_contained() => {
                "Inspector presentation: contained, waiting for the root surface output."
            }
            Some(location) if location.is_native_child() => {
                if self.dockspace.is_surface_presented(location.surface) {
                    "Inspector presentation: live native child OS window; closing it recovers the previous presentation."
                } else {
                    "Inspector presentation: native child transition is not live yet."
                }
            }
            Some(_) if root_presented => "Inspector presentation: docked in the root surface.",
            Some(_) => "Inspector presentation: root surface output is not live yet.",
            None => "Inspector presentation: pane is closed.",
        };
        ui.small(state);
    }

    fn show_walkthrough(&self, ui: &mut egui::Ui) {
        egui::CollapsingHeader::new("How to try contained floating and native multiview")
            .default_open(true)
            .show(ui, |ui| {
                ui.label(
                    "1. Contained floating: the Inspector group starts as an egui-styled window. \
                     Drag its title bar to move it, drag an edge to resize it, or open its ⋮ menu and choose Dock Back.",
                );
                ui.label(
                    "2. Explicit native child: open the Inspector title bar's ⋮ menu and choose Move to New Window. \
                     The toolbar button performs the same programmatic action only as a fallback.",
                );
                ui.label(
                    "3. Physical multiview: on a capability-qualified backend, drag a tab or the contained title bar outside an OS window and release. \
                     Drag a tab from the child back over a center or edge docking guide to dock it; releasing inside the target surface away from a guide creates a contained window.",
                );
                ui.small(
                    "Physical promotion is intentionally fail-closed when the host cannot provide exact cross-window pointer facts. \
                     In that case, use the ⋮ presentation menu or the explicit fallback controls above.",
                );
            });
    }

    fn request_inspector_window(&mut self, root: RootId) {
        let placement = NativeWindowPlacement::new(
            PhysicalRect::new(760.0, 120.0, 440.0, 340.0)
                .expect("the static child placement is valid"),
        );
        match self.dockspace.request_tear_off_root(root, placement) {
            Ok(()) => {
                self.action = InspectorActionState::AwaitingTearOffOutcome;
                self.status = None;
            }
            Err(error) => self.status = Some(format!("Could not open native window: {error}")),
        }
    }

    fn request_inspector_dock_back(&mut self, root: RootId) {
        match self.dockspace.request_dock_root_back(root) {
            Ok(()) => {
                self.action = InspectorActionState::AwaitingDockBackOutcome(root);
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
                InspectorActionState::AwaitingDockBackOutcome(expected_root),
                DockspaceActionStatus::Applied(DockspaceActionOutcome::RootDocked {
                    root,
                    changed: true,
                    ..
                }),
            ) if root == expected_root => {
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

#[derive(Clone, Copy)]
struct InspectorLocation {
    root: RootId,
    surface: SurfaceId,
    contained: bool,
}

impl InspectorLocation {
    fn is_contained(self) -> bool {
        self.surface == SURFACE && self.contained
    }

    fn is_native_child(self) -> bool {
        self.surface != SURFACE
    }
}

impl From<dockspace::model::DockspaceItemView<'_>> for InspectorLocation {
    fn from(item: dockspace::model::DockspaceItemView<'_>) -> Self {
        Self {
            root: item.root(),
            surface: item.surface(),
            contained: item.contained().is_some(),
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
                        text: "This tab shares the initial contained Inspector group. Use the title bar's ⋮ menu for explicit presentation commands, or drag a tab to exercise the physical path."
                            .to_owned(),
                    },
                ),
                (
                    INSPECTOR,
                    ExamplePane {
                        title: "Inspector",
                        text: "Start here: move or resize this contained window, choose Move to New Window from the ⋮ menu, then drag a child tab back onto a docking guide. The toolbar action is only a programmatic fallback."
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
