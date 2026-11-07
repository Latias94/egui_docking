//! Finite real-renderer regression for blank frames while changing tabs and moving windows.
//! Pass `--single-pass` to exercise retention when egui cannot repeat its measurement pass.

use std::time::{Duration, Instant};

use dockspace::model::{
    DockspaceLayout, DockspaceNode, DockspaceRootLayout, DockspaceSurfaceLayout, ItemId, RootId,
    SurfaceId,
};
use dockspace::policy::DockPolicy;
use dockspace::runtime::DockspaceSession;
use eframe::egui;
use egui_dockspace::{DockStyle, PaneView};
use egui_dockspace_native::NativeDockspaceApp;

const SURFACE: SurfaceId = SurfaceId::new(1);
const FIRST: ItemId = ItemId::new(1);
const SECOND: ItemId = ItemId::new(2);

#[derive(Default)]
struct Panes {
    paints: usize,
    last_bounds: Option<egui::Rect>,
}

impl PaneView for Panes {
    fn title(&self, item: ItemId) -> Option<egui::WidgetText> {
        Some(format!("Pane {}", item.get()).into())
    }

    fn ui(&mut self, item: ItemId, ui: &mut egui::Ui) -> egui_dockspace::PaneResponse {
        self.paints += 1;
        self.last_bounds = Some(ui.max_rect());
        ui.heading(format!("Visible pane {}", item.get()));
        egui_dockspace::PaneResponse::default()
    }
}

struct ContinuityApp {
    dockspace: NativeDockspaceApp<Panes>,
    started: Instant,
    seen_content: bool,
    selections: usize,
    pending: Option<ItemId>,
    moves: usize,
    moving_to: Option<egui::Pos2>,
    layout_changes: usize,
}

impl eframe::App for ContinuityApp {
    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        assert!(self.started.elapsed() < Duration::from_secs(15));
        if self.layout_changes % 2 == 1 {
            ui.add_space(80.0);
        }
        let available = ui.available_rect_before_wrap();
        let before = self.dockspace.with_panes(|panes| panes.paints);
        self.dockspace.ui(ui, frame);
        let painted = self.dockspace.with_panes(|panes| panes.paints) != before;
        assert!(
            !self.seen_content
                || painted
                || self.dockspace.current_output_is_retained()
                || ui.ctx().will_discard(),
            "a visible dockspace must paint its pane content or retain its previous framebuffer; selection {} produced a blank frame",
            self.selections,
        );
        self.seen_content |= painted;
        if painted {
            let bounds = self
                .dockspace
                .with_panes(|panes| panes.last_bounds.unwrap());
            assert!(
                available.contains_rect(bounds),
                "pane geometry must follow the current host Ui: {bounds:?} outside {available:?}",
            );
        }
        assert!(self.dockspace.error_kind().is_none());
        if let Some(target) = self.pending
            && let Some(status) = self.dockspace.take_action_status()
        {
            assert!(status.is_ok(), "selection failed: {status:?}");
            assert_eq!(
                self.dockspace
                    .with_view(|view| view.item(target).unwrap().is_selected()),
                Some(true),
            );
            self.pending = None;
        }
        if let Some(position) = self.moving_to
            && ui
                .input(|input| input.viewport().outer_rect)
                .is_some_and(|rect| rect.min.distance(position) < 1.0)
        {
            self.moving_to = None;
        }
        if self.seen_content
            && self.pending.is_none()
            && self.moving_to.is_none()
            && self.dockspace.is_quiescent()
        {
            if self.selections < 10 {
                let target = if self.selections % 2 == 0 {
                    SECOND
                } else {
                    FIRST
                };
                self.dockspace.request_select_item(target).unwrap();
                self.selections += 1;
                self.pending = Some(target);
            } else if self.moves < 10 {
                let position = if self.moves % 2 == 0 {
                    egui::pos2(80.0, 80.0)
                } else {
                    egui::pos2(120.0, 120.0)
                };
                self.moves += 1;
                self.moving_to = Some(position);
                ui.ctx()
                    .send_viewport_cmd(egui::ViewportCommand::OuterPosition(position));
            } else if self.layout_changes < 4 {
                self.layout_changes += 1;
            } else {
                eprintln!(
                    "presentation-continuity: ten tab selections, ten window moves and four host layout changes without a blank frame or stale pane geometry"
                );
                ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }
        ui.ctx().request_repaint();
    }
}

fn main() -> eframe::Result {
    let single_pass = std::env::args().any(|argument| argument == "--single-pass");
    let layout = DockspaceLayout::new([DockspaceSurfaceLayout::new(
        SURFACE,
        DockspaceRootLayout::new(RootId::new(1), DockspaceNode::central_tabs([FIRST, SECOND])),
    )])
    .unwrap();
    let session = DockspaceSession::from_layout(layout, DockPolicy::default()).unwrap();
    let dockspace = NativeDockspaceApp::new(
        egui::Id::unique("presentation-continuity"),
        session,
        SURFACE,
        Panes::default(),
        DockStyle::default(),
    )
    .unwrap();
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([640.0, 480.0]),
        native_host: Some(dockspace.native_host_handler()),
        ..Default::default()
    };
    eframe::run_native(
        "Dockspace presentation continuity regression",
        options,
        Box::new(move |creation| {
            if single_pass {
                creation
                    .egui_ctx
                    .options_mut(|options| options.max_passes = 1.try_into().unwrap());
            }
            Ok(Box::new(ContinuityApp {
                dockspace,
                started: Instant::now(),
                seen_content: false,
                selections: 0,
                pending: None,
                moves: 0,
                moving_to: None,
                layout_changes: 0,
            }))
        }),
    )
}
