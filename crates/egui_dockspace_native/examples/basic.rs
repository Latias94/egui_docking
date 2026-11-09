#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use egui_dockspace_native::NativeCommand;
use std::collections::BTreeMap;

use dockspace::geometry::{LogicalRect, PhysicalRect};
use dockspace::model::{
    DockspaceActionOutcome, DockspaceAxis, DockspaceContainedLayout, DockspaceLayout,
    DockspaceNode, DockspaceRootLayout, DockspaceSurfaceLayout, FloatingPresentationId, ItemId,
    NativeWindowPlacement, RootId, SurfaceId,
};
use dockspace::policy::{CloseCapability, DockPolicy};
use dockspace::runtime::DockspaceSession;
#[cfg(feature = "serde")]
use dockspace::runtime::{DockspaceDocumentBootstrap, DockspaceDocumentId};
use eframe::egui;
use egui_dockspace::{DockStyle, DockspaceActionStatus, PaneResponse, PaneView};
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
#[cfg(feature = "serde")]
const DOCUMENT_ID: DockspaceDocumentId = DockspaceDocumentId::from_bytes(*b"native-editor-v1");

#[path = "basic/command_smoke.rs"]
mod command_smoke;
#[path = "basic/docking_smoke.rs"]
mod docking_smoke;
#[path = "basic/layout.rs"]
mod layout;
#[path = "basic/smoke.rs"]
mod smoke;

fn main() -> eframe::Result {
    let mut policy = DockPolicy::default();
    policy.set_allow_native_surfaces(true);
    policy.set_close_capability(CloseCapability::DeferredAllowed);
    let panes = ExamplePanes::new();
    let command_smoke = std::env::args().any(|argument| argument == "--command-smoke");
    let default_layout = if command_smoke {
        command_smoke::layout()
    } else {
        example_layout()
    };
    #[cfg(feature = "serde")]
    let session = {
        let mut bootstrap = DockspaceDocumentBootstrap::new(DOCUMENT_ID);
        // BTreeMap visits the original ItemIds in their allocation order, 1..=8.
        for (&item, pane) in &panes.panes {
            assert_eq!(
                bootstrap.ensure_item(pane.key).expect("the eight keys fit"),
                item,
            );
        }
        DockspaceSession::from_persistent_layout(default_layout.clone(), policy, bootstrap)
            .expect("the static native document is valid")
    };
    #[cfg(not(feature = "serde"))]
    let session = DockspaceSession::from_layout(default_layout.clone(), policy)
        .expect("the static native example layout is valid");
    let dockspace = NativeDockspaceApp::new(
        egui::Id::unique("native-dockspace-example"),
        session,
        SURFACE,
        panes,
        DockStyle::default(),
    )
    .expect("the native coordinator initializes");
    let native_host = dockspace.native_host_handler();
    let app = NativeExampleApp {
        dockspace,
        default_layout,
        #[cfg(feature = "serde")]
        presets: BTreeMap::new(),
        #[cfg(feature = "serde")]
        preset_name: "Working".to_owned(),
        action: ExampleActionState::Idle,
        status: None,
        smoke: std::env::args()
            .any(|argument| argument == "--smoke")
            .then(smoke::EditorSmoke::new),
        docking_smoke: std::env::args()
            .any(|argument| argument == "--docking-smoke")
            .then(docking_smoke::DockingSmoke::new),
        command_smoke: command_smoke.then(command_smoke::CommandSmoke::new),
    };
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([1280.0, 820.0]),
        native_host: Some(native_host),
        ..Default::default()
    };
    eframe::run_native(
        "Dockspace native editor",
        options,
        Box::new(move |creation| {
            if let Some(smoke) = &app.docking_smoke {
                smoke.install(&creation.egui_ctx);
            }
            Ok(Box::new(app))
        }),
    )
}

struct NativeExampleApp {
    dockspace: NativeDockspaceApp<ExamplePanes>,
    default_layout: DockspaceLayout,
    #[cfg(feature = "serde")]
    presets: BTreeMap<String, layout::NamedPreset>,
    #[cfg(feature = "serde")]
    preset_name: String,
    action: ExampleActionState,
    status: Option<String>,
    smoke: Option<smoke::EditorSmoke>,
    docking_smoke: Option<docking_smoke::DockingSmoke>,
    command_smoke: Option<command_smoke::CommandSmoke>,
}

#[derive(Clone, Copy, Default)]
enum ExampleActionState {
    #[default]
    Idle,
    AwaitingTearOffOutcome,
    AwaitingFirstPresentation(SurfaceId),
    AwaitingDockBackOutcome(RootId),
    AwaitingLayoutOutcome(&'static str),
    AwaitingFreshPresentation(&'static str),
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
        <NativeDockspaceApp<ExamplePanes> as eframe::App>::ui(&mut self.dockspace, ui, frame);
        if let Some(mut smoke) = self.smoke.take() {
            smoke.advance(self, ui.ctx());
            self.smoke = Some(smoke);
        }
        if let Some(mut smoke) = self.docking_smoke.take() {
            smoke.advance(self, ui.ctx());
            self.docking_smoke = Some(smoke);
        }
        if let Some(mut smoke) = self.command_smoke.take() {
            smoke.advance(&self.dockspace, ui.ctx());
            self.command_smoke = Some(smoke);
        }
    }
}

impl NativeExampleApp {
    fn show_controls(&mut self, ui: &mut egui::Ui) {
        self.finish_presentation_wait();
        let root_surface = self.dockspace.root_surface();
        let inspector = self.dockspace.with_view(|view| {
            view.item(INSPECTOR)
                .map(|item| InspectorLocation::new(item, root_surface))
        });
        let root_presented = self.dockspace.is_surface_presented(root_surface);

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
                    self.request_root_dock_back(location.root);
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
                    ui.label("Returning the window group to the main dockspace…");
                }
                ExampleActionState::AwaitingLayoutOutcome(operation)
                | ExampleActionState::AwaitingSmokeOutcome(operation) => {
                    ui.label(format!("{operation}: waiting for the committed result…"));
                }
                ExampleActionState::AwaitingFreshPresentation(operation) => {
                    ui.label(format!(
                        "{operation}: layout restored; waiting for the main window and each child window to be ready for editing…"
                    ));
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
                    "5. Pane inventory lists all eight application-owned panes. Open a pane's menu to select or move it, \
                     request a pane or group close, then choose Close, Keep open or Decide later for each pane. \
                     Postponed decisions use the same Close and Keep open controls. A group closes together; keeping any pane open cancels the request. Reopen restores the same pane data \
                     beside the current Document, or in the current main root when Document is closed. \
                     Lock arrangement disables layout changes while selection and content editing remain available.",
                );
                ui.label(
                    "6. Default layout restores the application's immutable layout while retaining all eight pane identities and their data. \
                     Inventory can temporarily unload content; its recoverable placeholder keeps the title, key and data for Reload content.",
                );
                #[cfg(feature = "serde")]
                ui.label(
                    "7. Save and restore up to eight named JSON presets in application memory, including the main window, contained windows and native child windows. \
                     Restore applies the saved layout first, then replaces the child windows. Wait until every window has displayed its restored panes before editing.",
                );
                ui.small(
                    "Physical promotion is intentionally fail-closed when the host cannot provide exact cross-window pointer facts. \
                     X11 is the physical reference; macOS has programmatic hosting evidence. \
                     Other platforms and mixed DPI require their own qualification. Use the presentation commands when physical facts are unavailable.",
                );
            });
    }

    fn request_inspector_window(&mut self, root: RootId) {
        self.request_native_window(
            root,
            PhysicalRect::new(760.0, 120.0, 440.0, 340.0)
                .expect("the static child placement is valid"),
        );
    }

    fn request_native_window(&mut self, root: RootId, bounds: PhysicalRect) {
        let placement = NativeWindowPlacement::new(bounds);
        match self
            .dockspace
            .request_command(NativeCommand::MoveRootToNewWindow { root, placement })
        {
            Ok(()) => {
                self.action = ExampleActionState::AwaitingTearOffOutcome;
                self.status = None;
            }
            Err(error) => self.status = Some(format!("Could not open native window: {error}")),
        }
    }

    fn request_root_dock_back(&mut self, root: RootId) {
        match self
            .dockspace
            .request_command(NativeCommand::DockRootBack { root })
        {
            Ok(()) => {
                self.action = ExampleActionState::AwaitingDockBackOutcome(root);
                self.status = None;
            }
            Err(error) => {
                self.status = Some(format!("Could not dock the window group back: {error}"))
            }
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
                    Some("The window group was presented back in the main dockspace.".to_owned());
            }
            (_, Ok(status)) => {
                self.record_action_status(&status);
            }
            (_, Err(error)) => {
                self.action = ExampleActionState::Idle;
                self.status = Some(format!("The native action was interrupted: {error}"));
            }
        }
    }

    fn record_action_status(&mut self, status: &NativeActionStatus) {
        self.action = match status {
            NativeActionStatus::LayoutReset { .. } => {
                ExampleActionState::AwaitingFreshPresentation("Default layout")
            }
            #[cfg(feature = "serde")]
            NativeActionStatus::DocumentRestored { .. } => {
                ExampleActionState::AwaitingFreshPresentation("Document restore")
            }
            _ => ExampleActionState::Idle,
        };
        self.status = Some(
            self.dockspace
                .with_panes(|panes| layout::describe_status(status, panes)),
        );
    }

    fn finish_presentation_wait(&mut self) {
        match self.action {
            ExampleActionState::AwaitingFirstPresentation(surface)
                if self.dockspace.is_surface_presented(surface) =>
            {
                self.action = ExampleActionState::Idle;
                self.status = Some("Native child reached its first live presentation.".to_owned());
            }
            ExampleActionState::AwaitingFirstPresentation(surface)
                if self.dockspace.is_quiescent()
                    && self
                        .dockspace
                        .with_view(|view| view.surface(surface).is_none()) =>
            {
                self.action = ExampleActionState::Idle;
                self.status = Some(
                    "Native child is no longer available; its panes remain in the dockspace."
                        .to_owned(),
                );
            }
            ExampleActionState::AwaitingFreshPresentation(operation)
                if self.all_surfaces_presented() && self.dockspace.is_quiescent() =>
            {
                self.action = ExampleActionState::Idle;
                self.status = Some(format!(
                    "{operation}: the main window and every child window have displayed their panes and are ready for editing."
                ));
            }
            _ => {}
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct InspectorLocation {
    root: RootId,
    surface: SurfaceId,
    host_surface: SurfaceId,
    contained: bool,
}

impl InspectorLocation {
    fn new(item: dockspace::model::DockspaceItemView<'_>, host_surface: SurfaceId) -> Self {
        Self {
            root: item.root(),
            surface: item.surface(),
            host_surface,
            contained: item.contained().is_some(),
        }
    }

    fn is_contained(self) -> bool {
        self.surface == self.host_surface && self.contained
    }

    fn is_native_child(self) -> bool {
        self.surface != self.host_surface
    }
}

fn example_layout() -> DockspaceLayout {
    editor_layout(false)
}

fn editor_layout(separate_assets: bool) -> DockspaceLayout {
    editor_layout_on_surface(separate_assets, SURFACE)
}

fn editor_layout_on_surface(separate_assets: bool, host_surface: SurfaceId) -> DockspaceLayout {
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
            (
                DockspaceNode::tabs(if separate_assets {
                    vec![OUTLINE]
                } else {
                    vec![OUTLINE, ASSETS]
                }),
                0.20,
            ),
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
    let mut surface =
        DockspaceSurfaceLayout::new(host_surface, DockspaceRootLayout::new(MAIN_ROOT, root))
            .with_contained(inspector);
    if separate_assets {
        surface = surface.with_contained(DockspaceContainedLayout::new(
            FloatingPresentationId::new(2),
            DockspaceRootLayout::new(RootId::new(3), DockspaceNode::tabs([ASSETS])),
            LogicalRect::new(120.0, 100.0, 360.0, 300.0)
                .expect("the static Assets placement is valid"),
        ));
    }
    DockspaceLayout::new([surface]).expect("the static surface is valid")
}

struct ExamplePane {
    key: &'static str,
    title: &'static str,
    text: String,
    loaded: bool,
    missing_painted: bool,
    zoom: f32,
    pan: egui::Vec2,
    input: Option<(egui::ViewportId, egui::Rect, bool, f32)>,
    content: Option<(egui::Rect, f32)>,
    window_id: Option<u64>,
}

impl ExamplePane {
    fn set_loaded(&mut self, loaded: bool) {
        self.loaded = loaded;
        self.missing_painted = false;
        self.input = None;
        self.content = None;
        self.window_id = None;
    }
}

struct ExamplePanes {
    panes: BTreeMap<ItemId, ExamplePane>,
}

impl ExamplePanes {
    fn new() -> Self {
        Self {
            panes: [
                (OUTLINE, "pane/outline", "Outline", "Scene\n  Camera\n  Directional light\n  Player\n    Mesh\n    Controller"),
                (EDITOR, "pane/document", "Document", "// Application-owned document\nfn update(scene: &mut Scene) {\n    scene.player.translate([1.0, 0.0, 0.0]);\n}\n\nEdit this document, then move or dock its tab.\n"),
                (PREVIEW, "pane/preview", "Preview", "Application-drawn scene"),
                (INSPECTOR, "pane/inspector", "Inspector", "Object: Player\nPosition: [0, 0, 0]\nMesh: player.mesh\n\nThese editable notes survive moving this group into a native window and docking it back.\n"),
                (CONSOLE, "pane/console", "Console", "Ready. Application log; scroll to inspect older messages."),
                (HISTORY, "pane/history", "History", "Selected Player\nMoved Player\nChanged light intensity\nSaved scene"),
                (PROFILER, "pane/profiler", "Profiler", "Illustrative frame samples — no live engine attached"),
                (ASSETS, "pane/assets", "Assets", "scenes/main.scene\nmeshes/player.mesh\nmaterials/default.mat\ntextures/grid.png"),
            ]
            .into_iter()
            .map(|(item, key, title, text)| (item, ExamplePane {
                key,
                title,
                text: text.to_owned(),
                loaded: true,
                missing_painted: false,
                zoom: 1.0,
                pan: egui::Vec2::ZERO,
                input: None,
                content: None,
                window_id: None,
            }))
            .collect(),
        }
    }
}

fn pane_focus_id(item: ItemId) -> egui::Id {
    egui::Id::unique(("native-editor-content", item.get()))
}

fn paint_canvas(pane: &mut ExamplePane, ui: &mut egui::Ui, content: PaneResponse) -> PaneResponse {
    let controls = ui.horizontal(|ui| {
        let reset = ui.button("Reset view");
        if reset.clicked() {
            pane.pan = egui::Vec2::ZERO;
            pane.zoom = 1.0;
        }
        let zoom = ui.add(egui::Slider::new(&mut pane.zoom, 0.5..=2.0).text("Zoom"));
        (reset, zoom)
    });
    let (rect, background) = ui.allocate_exact_size(ui.available_size(), egui::Sense::hover());
    let response = ui.interact(rect, pane_focus_id(PREVIEW), egui::Sense::click_and_drag());
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
    content
        .with_content(&controls.inner.0)
        .with_content(&controls.inner.1)
        .with_content(&controls.response)
        .with_content(&background)
        .with_content(&response)
}

impl PaneView for ExamplePanes {
    fn title(&self, item: ItemId) -> Option<egui::WidgetText> {
        self.panes.get(&item).map(|pane| pane.title.into())
    }

    fn ui(&mut self, item: ItemId, ui: &mut egui::Ui) -> PaneResponse {
        let Some(pane) = self.panes.get_mut(&item) else {
            ui.label("Missing pane");
            return PaneResponse::default();
        };
        pane.input = None;
        pane.content = Some((ui.max_rect(), ui.ctx().pixels_per_point()));
        pane.window_id = None;
        pane.missing_painted = false;
        let heading = ui.heading(pane.title);
        let mut content = PaneResponse::from(&heading);
        if !pane.loaded {
            let placeholder = ui.label("Recoverable missing content — temporarily unloaded.");
            pane.missing_painted = ui.is_rect_visible(placeholder.rect);
            let retained = ui.small(format!(
                "{} · title and application data retained",
                pane.key
            ));
            let reload = ui.button("Reload content");
            if reload.clicked() {
                pane.set_loaded(true);
                if let Some(target) = self.focus_target(item) {
                    ui.memory_mut(|memory| memory.request_focus(target));
                }
                ui.ctx().request_repaint();
            }
            return content
                .with_content(&placeholder)
                .with_content(&retained)
                .with_content(&reload);
        }
        if item == PREVIEW {
            content = paint_canvas(pane, ui, content);
        } else if item == CONSOLE {
            let description = ui.label(&pane.text);
            content = content.with_content(&description);
            let log = egui::ScrollArea::vertical()
                .id_salt(("native-editor-log", item.get()))
                .show(ui, |ui| {
                    let mut content = content.with_content_ids([ui.unique_id()]);
                    for line in 0..128 {
                        let entry = ui.monospace(format!("[{line:03}] Scene update completed"));
                        content = content.with_content(&entry);
                    }
                    content
                });
            content = log.inner;
            // Only declare scroll controls actually registered in this pass.
            for id in [log.id.with("area"), log.id.with(1_usize)] {
                if ui
                    .ctx()
                    .viewport(|viewport| viewport.this_pass.widgets.get(id).is_some())
                {
                    content = content.with_content_ids([id]);
                }
            }
        } else if item == PROFILER {
            let description = ui.label(&pane.text);
            content = content.with_content(&description);
            for (stage, milliseconds) in [("Simulation", 2.6), ("Rendering", 5.4), ("UI", 0.8)] {
                let label = ui.label(format!("{stage}: {milliseconds:.1} ms"));
                content = content.with_content(&label);
                let progress = ui
                    .add(egui::ProgressBar::new(milliseconds / 16.67).desired_width(f32::INFINITY));
                content = content.with_content(&progress);
            }
        } else {
            let response = ui.add_sized(
                ui.available_size(),
                egui::TextEdit::multiline(&mut pane.text)
                    .id(pane_focus_id(item))
                    .desired_width(f32::INFINITY)
                    .font(egui::TextStyle::Monospace),
            );
            content = content.with_content(&response);
            pane.input = Some((
                ui.ctx().viewport_id(),
                response.rect,
                response.has_focus(),
                ui.ctx().pixels_per_point(),
            ));
            pane.window_id =
                eframe::current_native_output_token().map(|token| u64::from(token.window_id()));
        }
        content
    }

    fn focus_target(&self, item: ItemId) -> Option<egui::Id> {
        self.panes
            .get(&item)
            .filter(|pane| pane.loaded && ![PREVIEW, CONSOLE, PROFILER].contains(&item))
            .map(|_| pane_focus_id(item))
    }

    fn minimum_size(&self, item: ItemId) -> egui::Vec2 {
        if item == PREVIEW {
            egui::vec2(220.0, 160.0)
        } else {
            egui::vec2(120.0, 72.0)
        }
    }
}
