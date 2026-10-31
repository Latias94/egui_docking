#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::collections::BTreeMap;

use dockspace::geometry::{LogicalRect, PhysicalRect};
use dockspace::model::{
    DockspaceActionOutcome, DockspaceAxis, DockspaceContainedLayout, DockspaceLayout,
    DockspaceNode, DockspaceRootLayout, DockspaceSurfaceLayout, FloatingPresentationId, ItemId,
    NativeWindowPlacement, RootId, SurfaceId,
};
use dockspace::policy::{CloseCapability, DockPolicy};
use dockspace::runtime::DockspaceSession;
use eframe::egui;
use egui_dockspace::{DockStyle, DockspaceActionStatus, PaneView};
use egui_dockspace_native::{NativeActionStatus, NativeDockspaceApp};

const SURFACE: SurfaceId = SurfaceId::new(1);
const MAIN_ROOT: RootId = RootId::new(1);
const INSPECTOR_ROOT: RootId = RootId::new(2);
const INSPECTOR_FLOATING: FloatingPresentationId = FloatingPresentationId::new(1);
const OUTLINE: ItemId = ItemId::new(1);
const EDITOR: ItemId = ItemId::new(2);
const PREVIEW: ItemId = ItemId::new(3);
const INSPECTOR: ItemId = ItemId::new(4);
const CONSOLE: ItemId = ItemId::new(5);
const HISTORY: ItemId = ItemId::new(6);
const PROFILER: ItemId = ItemId::new(7);
const ASSETS: ItemId = ItemId::new(8);

#[path = "basic/layout.rs"]
mod layout;
#[path = "basic/smoke.rs"]
mod smoke;

fn main() -> eframe::Result {
    let mut policy = DockPolicy::default();
    policy.set_allow_native_surfaces(true);
    policy.set_close_capability(CloseCapability::DeferredAllowed);
    let session = DockspaceSession::from_layout(example_layout(), policy)
        .expect("the static native example layout is valid");
    let dockspace = NativeDockspaceApp::new(
        egui::Id::unique("native-dockspace-example"),
        session,
        SURFACE,
        ExamplePanes::new(),
        DockStyle::default(),
    )
    .expect("the native coordinator initializes");
    let native_host = dockspace.native_host_handler();
    let app = NativeExampleApp {
        dockspace,
        action: ExampleActionState::Idle,
        status: None,
        smoke: std::env::args()
            .any(|argument| argument == "--smoke")
            .then(smoke::EditorSmoke::new),
    };
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([1280.0, 820.0]),
        native_host: Some(native_host),
        ..Default::default()
    };
    eframe::run_native(
        "Dockspace native editor",
        options,
        Box::new(move |_| Ok(Box::new(app))),
    )
}

struct NativeExampleApp {
    dockspace: NativeDockspaceApp<ExamplePanes>,
    action: ExampleActionState,
    status: Option<String>,
    smoke: Option<smoke::EditorSmoke>,
}

#[derive(Clone, Copy, Default)]
enum ExampleActionState {
    #[default]
    Idle,
    AwaitingTearOffOutcome,
    AwaitingFirstPresentation(SurfaceId),
    AwaitingDockBackOutcome(RootId),
    AwaitingLayoutOutcome(&'static str),
    AwaitingSmokeOutcome(&'static str),
}

impl ExampleActionState {
    const fn is_pending(self) -> bool {
        !matches!(self, Self::Idle)
    }
}

impl eframe::App for NativeExampleApp {
    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        if matches!(
            self.action,
            ExampleActionState::AwaitingTearOffOutcome
                | ExampleActionState::AwaitingDockBackOutcome(_)
                | ExampleActionState::AwaitingLayoutOutcome(_)
        ) {
            self.collect_action_status();
        }
        egui::Panel::top("native-example-controls").show(ui, |ui| {
            self.show_controls(ui);
        });
        egui::CentralPanel::default().show(ui, |ui| {
            <NativeDockspaceApp<ExamplePanes> as eframe::App>::ui(&mut self.dockspace, ui, frame);
        });
        if let Some(mut smoke) = self.smoke.take() {
            smoke.advance(self, ui.ctx());
            self.smoke = Some(smoke);
        }
    }
}

impl NativeExampleApp {
    fn show_controls(&mut self, ui: &mut egui::Ui) {
        self.finish_first_live_wait();
        let inspector = self
            .dockspace
            .with_view(|view| view.item(INSPECTOR).map(InspectorLocation::from))
            .flatten();
        let root_presented = self.dockspace.is_surface_presented(SURFACE);

        ui.horizontal_wrapped(|ui| {
            ui.strong("Dockspace native editor");
            ui.separator();
            let contained_root = inspector.filter(|location| location.is_contained());
            let enabled = self.controls_available()
                && !self.dockspace.arrangement_locked()
                && root_presented
                && contained_root.is_some();
            if ui
                .add_enabled(enabled, egui::Button::new("Open Inspector Window ↗"))
                .on_disabled_hover_text(
                    "Move the live contained Inspector and Preview group into an OS window.",
                )
                .clicked()
                && let Some(location) = contained_root
            {
                self.request_inspector_window(location.root);
            }
            if let Some(location) = inspector.filter(|location| location.is_native_child()) {
                if ui
                    .add_enabled(
                        self.controls_available() && !self.dockspace.arrangement_locked(),
                        egui::Button::new("Dock Inspector Back"),
                    )
                    .clicked()
                {
                    self.request_inspector_dock_back(location.root);
                }
            } else if !root_presented {
                ui.label("Waiting for the root output to be presented…");
            }
        });
        self.show_layout_controls(ui);
        if self.action.is_pending() {
            self.show_pending_controls(ui);
        }
        self.show_capability_status(ui, inspector, root_presented);
        self.show_presentation_state(ui, inspector, root_presented);
        self.show_walkthrough(ui);
        if let Some(status) = &self.status {
            ui.small(format!("Last action: {status}"));
        }
    }

    fn show_pending_controls(&self, ui: &mut egui::Ui) {
        ui.horizontal_wrapped(|ui| {
            ui.spinner();
            match self.action {
                ExampleActionState::AwaitingDockBackOutcome(_) => {
                    ui.label("Presenting Inspector back in the root dockspace…");
                }
                ExampleActionState::AwaitingLayoutOutcome(operation)
                | ExampleActionState::AwaitingSmokeOutcome(operation) => {
                    ui.label(format!("{operation}: waiting for the committed result…"));
                }
                _ => {
                    ui.label("Creating and presenting the native child window…");
                }
            }
        });
    }

    fn show_capability_status(
        &self,
        ui: &mut egui::Ui,
        inspector: Option<InspectorLocation>,
        root_presented: bool,
    ) {
        let locked = self.dockspace.arrangement_locked();
        let local = if root_presented && locked {
            "selection ready; arrangement locked"
        } else if root_presented {
            "ready"
        } else {
            "waiting for the root surface"
        };
        let contained = if locked {
            "arrangement locked"
        } else if inspector.is_some_and(InspectorLocation::is_contained) {
            "ready"
        } else {
            "not currently presented"
        };
        let native = if self.dockspace.error_kind().is_some() {
            "unavailable (runtime stopped)"
        } else if locked {
            "arrangement locked"
        } else {
            "programmatic window hosting ready; physical input requires backend facts"
        };
        ui.small(format!(
            "Capability status — local select/reorder: {local}; contained move/resize: {contained}; native promotion: {native}."
        ));
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
        egui::CollapsingHeader::new("Editor workflow and platform support")
            .default_open(false)
            .show(ui, |ui| {
                ui.label(
                    "1. Edit the document or Inspector notes, scroll Console, and pan or zoom Preview. \
                     Pane data belongs to this application and survives presentation changes. \
                     2. The Inspector group starts as a contained window. \
                     Drag its title bar to move it, drag an edge to resize it, or open its ⋮ menu and choose Dock Back.",
                );
                ui.label(
                    "3. Explicit native child: open the Inspector title bar's ⋮ menu and choose Move to New Window. \
                     The toolbar buttons use the same typed presentation commands.",
                );
                ui.label(
                    "4. Physical multiview: on a capability-qualified backend, drag a tab or the contained title bar outside an OS window and release. \
                     Drag a tab from the child over document content into a center or edge docking basin; guides are visual hints. \
                     Closing an OS child restores its recovery target and keeps pane data.",
                );
                ui.label(
                    "5. Pane inventory lists all eight application-owned panes. Select an open pane, \
                     request a pane or group close, then explicitly Allow, Veto or Defer each pending decision. \
                     Finish a deferred decision with its continuation controls. Reopen restores the same pane data \
                     beside the current Document, or in the current main root when Document is closed. \
                     Lock arrangement disables layout changes while selection and content editing remain available.",
                );
                ui.small(
                    "Physical promotion is intentionally fail-closed when the host cannot provide exact cross-window pointer facts. \
                     X11 is the physical reference; macOS has programmatic hosting evidence. \
                     Other platforms and mixed DPI require their own qualification. Use the presentation commands when physical facts are unavailable. \
                     Layout presets and document restore are demonstrated separately by the official workspace_persistence example.",
                );
            });
    }

    fn request_inspector_window(&mut self, root: RootId) {
        let placement = NativeWindowPlacement::new(
            PhysicalRect::new(760.0, 120.0, 440.0, 340.0)
                .expect("the static child placement is valid"),
        );
        match self
            .dockspace
            .request_move_root_to_new_window(root, placement)
        {
            Ok(()) => {
                self.action = ExampleActionState::AwaitingTearOffOutcome;
                self.status = None;
            }
            Err(error) => self.status = Some(format!("Could not open native window: {error}")),
        }
    }

    fn request_inspector_dock_back(&mut self, root: RootId) {
        match self.dockspace.request_dock_root_back(root) {
            Ok(()) => {
                self.action = ExampleActionState::AwaitingDockBackOutcome(root);
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
                ExampleActionState::AwaitingTearOffOutcome,
                Ok(NativeActionStatus::Dock(DockspaceActionStatus::Applied(
                    DockspaceActionOutcome::NativeRootTearOffRequested { target_surface, .. },
                ))),
            ) => {
                self.action = ExampleActionState::AwaitingFirstPresentation(target_surface);
                self.status = Some(
                    "Native child creation accepted; waiting for first live presentation."
                        .to_owned(),
                );
            }
            (
                ExampleActionState::AwaitingDockBackOutcome(expected_root),
                Ok(NativeActionStatus::Dock(DockspaceActionStatus::Applied(
                    DockspaceActionOutcome::RootDocked {
                        root,
                        changed: true,
                        ..
                    },
                ))),
            ) if root == expected_root => {
                self.action = ExampleActionState::Idle;
                self.status =
                    Some("Inspector was presented back in the root dockspace.".to_owned());
            }
            (_, Ok(status)) => {
                self.action = ExampleActionState::Idle;
                self.status = Some(layout::describe_status(&status));
            }
            (_, Err(error)) => {
                self.action = ExampleActionState::Idle;
                self.status = Some(format!("The native action was interrupted: {error}"));
            }
        }
    }

    fn finish_first_live_wait(&mut self) {
        let ExampleActionState::AwaitingFirstPresentation(surface) = self.action else {
            return;
        };
        if self.dockspace.is_surface_presented(surface) {
            self.action = ExampleActionState::Idle;
            self.status = Some("Native child reached its first live presentation.".to_owned());
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
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
    let center = DockspaceNode::split(
        DockspaceAxis::Vertical,
        [
            (
                DockspaceNode::central_tabs([EDITOR, HISTORY, PROFILER]),
                0.74,
            ),
            (DockspaceNode::tabs([CONSOLE]), 0.26),
        ],
    )
    .expect("the editor and console split is valid");
    let root = DockspaceNode::split(
        DockspaceAxis::Horizontal,
        [
            (DockspaceNode::tabs([OUTLINE, ASSETS]), 0.20),
            (center, 0.80),
        ],
    )
    .expect("the static split is valid");
    let inspector = DockspaceContainedLayout::new(
        INSPECTOR_FLOATING,
        DockspaceRootLayout::new(
            INSPECTOR_ROOT,
            DockspaceNode::central_tabs_with_selection([PREVIEW, INSPECTOR], Some(INSPECTOR))
                .expect("the Inspector starts selected"),
        ),
        LogicalRect::new(850.0, 80.0, 360.0, 410.0).expect("the static contained rect is valid"),
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
    widgets: Vec<egui::Id>,
    zoom: f32,
    pan: egui::Vec2,
    input: Option<(egui::ViewportId, egui::Rect, bool, f32)>,
}

struct ExamplePanes {
    panes: BTreeMap<ItemId, ExamplePane>,
}

impl ExamplePanes {
    fn new() -> Self {
        Self {
            panes: [
                (OUTLINE, "Outline", "Scene\n  Camera\n  Directional light\n  Player\n    Mesh\n    Controller"),
                (EDITOR, "Document", "// Application-owned document\nfn update(scene: &mut Scene) {\n    scene.player.translate([1.0, 0.0, 0.0]);\n}\n\nEdit this document, then move or dock its tab.\n"),
                (PREVIEW, "Preview", "Application-drawn scene"),
                (INSPECTOR, "Inspector", "Object: Player\nPosition: [0, 0, 0]\nMesh: player.mesh\n\nThese editable notes survive moving this group into a native window and docking it back.\n"),
                (CONSOLE, "Console", "Ready. Application log; scroll to inspect older messages."),
                (HISTORY, "History", "Selected Player\nMoved Player\nChanged light intensity\nSaved scene"),
                (PROFILER, "Profiler", "Illustrative frame samples — no live engine attached"),
                (ASSETS, "Assets", "scenes/main.scene\nmeshes/player.mesh\nmaterials/default.mat\ntextures/grid.png"),
            ]
            .into_iter()
            .map(|(item, title, text)| (item, ExamplePane {
                title,
                text: text.to_owned(),
                widgets: Vec::with_capacity(3),
                zoom: 1.0,
                pan: egui::Vec2::ZERO,
                input: None,
            }))
            .collect(),
        }
    }
}

fn pane_focus_id(item: ItemId) -> egui::Id {
    egui::Id::unique(("native-editor-content", item.get()))
}

fn paint_canvas(pane: &mut ExamplePane, ui: &mut egui::Ui) {
    ui.horizontal(|ui| {
        let reset = ui.button("Reset view");
        pane.widgets.push(reset.id);
        if reset.clicked() {
            pane.pan = egui::Vec2::ZERO;
            pane.zoom = 1.0;
        }
        let zoom = ui.add(egui::Slider::new(&mut pane.zoom, 0.5..=2.0).text("Zoom"));
        pane.widgets.push(zoom.id);
    });
    let (rect, _) = ui.allocate_exact_size(ui.available_size(), egui::Sense::hover());
    let response = ui.interact(rect, pane_focus_id(PREVIEW), egui::Sense::click_and_drag());
    pane.widgets.push(response.id);
    if response.dragged() {
        pane.pan += response.drag_delta();
    }
    let painter = ui.painter().with_clip_rect(rect.intersect(ui.clip_rect()));
    painter.rect_filled(rect, 0.0, ui.visuals().extreme_bg_color);
    let spacing = 32.0 * pane.zoom;
    let origin = rect.center() + pane.pan;
    let stroke = ui.visuals().widgets.noninteractive.bg_stroke;
    for index in -32..=32 {
        let offset = index as f32 * spacing;
        painter.line_segment(
            [
                egui::pos2(origin.x + offset, rect.top()),
                egui::pos2(origin.x + offset, rect.bottom()),
            ],
            stroke,
        );
        painter.line_segment(
            [
                egui::pos2(rect.left(), origin.y + offset),
                egui::pos2(rect.right(), origin.y + offset),
            ],
            stroke,
        );
    }
    painter.circle_filled(origin, 30.0 * pane.zoom, ui.visuals().selection.bg_fill);
    painter.text(
        rect.center_bottom() - egui::vec2(0.0, 20.0),
        egui::Align2::CENTER_BOTTOM,
        "Application canvas · drag to pan",
        egui::FontId::proportional(14.0),
        ui.visuals().text_color(),
    );
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
        // Only widgets actually registered in this content callback qualify.
        pane.widgets.clear();
        pane.input = None;
        ui.heading(pane.title);
        if item == PREVIEW {
            paint_canvas(pane, ui);
        } else if item == CONSOLE {
            ui.label(&pane.text);
            egui::ScrollArea::vertical()
                .id_salt(("native-editor-log", item.get()))
                .show(ui, |ui| {
                    for line in 0..128 {
                        ui.monospace(format!("[{line:03}] Scene update completed"));
                    }
                });
        } else if item == PROFILER {
            ui.label(&pane.text);
            for (stage, milliseconds) in [("Simulation", 2.6), ("Rendering", 5.4), ("UI", 0.8)] {
                ui.label(format!("{stage}: {milliseconds:.1} ms"));
                ui.add(egui::ProgressBar::new(milliseconds / 16.67).desired_width(f32::INFINITY));
            }
        } else {
            let response = ui.add_sized(
                ui.available_size(),
                egui::TextEdit::multiline(&mut pane.text)
                    .id(pane_focus_id(item))
                    .desired_width(f32::INFINITY)
                    .font(egui::TextStyle::Monospace),
            );
            pane.widgets.push(response.id);
            pane.input = Some((
                ui.ctx().viewport_id(),
                response.rect,
                response.has_focus(),
                ui.ctx().pixels_per_point(),
            ));
        }
    }

    fn docking_content_widgets(&self, item: ItemId) -> &[egui::Id] {
        self.panes
            .get(&item)
            .map_or(&[], |pane| pane.widgets.as_slice())
    }

    fn focus_target(&self, item: ItemId) -> Option<egui::Id> {
        self.panes
            .contains_key(&item)
            .then_some(item)
            .filter(|item| ![PREVIEW, CONSOLE, PROFILER].contains(item))
            .map(pane_focus_id)
    }

    fn minimum_size(&self, item: ItemId) -> egui::Vec2 {
        if item == PREVIEW {
            egui::vec2(220.0, 160.0)
        } else {
            egui::vec2(120.0, 72.0)
        }
    }
}
