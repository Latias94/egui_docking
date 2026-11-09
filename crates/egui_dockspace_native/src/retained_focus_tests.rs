//! Real output tokens exercise the retention entry used by deferred viewports.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use dockspace::model::{
    DockspaceLayout, DockspaceNode, DockspaceRootLayout, DockspaceSurfaceLayout, ItemId, RootId,
};
use dockspace::runtime::DockspaceSession;
use egui_dockspace::{DockStyle, PaneResponse};
use winit::platform::x11::EventLoopBuilderExtX11 as _;

use super::*;

const SURFACE: SurfaceId = SurfaceId::new(1);
const EDITOR: ItemId = ItemId::new(1);

struct Editor {
    target: egui::Id,
    text: String,
    calls: Arc<AtomicUsize>,
}

impl PaneView for Editor {
    fn title(&self, _: ItemId) -> Option<egui::WidgetText> {
        Some("Retained editor".into())
    }

    fn ui(&mut self, _: ItemId, ui: &mut egui::Ui) -> PaneResponse {
        let response = ui.add(egui::TextEdit::singleline(&mut self.text).id(self.target));
        if self.calls.fetch_add(1, Ordering::SeqCst) == 0 {
            response.request_focus();
        }
        PaneResponse::default().with_content(&response)
    }

    fn focus_target(&self, _: ItemId) -> Option<egui::Id> {
        Some(self.target)
    }
}

struct RetainApp {
    app: NativeDockspaceApp<Editor>,
    target: egui::Id,
    calls: Arc<AtomicUsize>,
    skipped: Arc<AtomicUsize>,
    started: Instant,
}

impl eframe::App for RetainApp {
    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        assert!(self.started.elapsed() < Duration::from_secs(15));
        let skipped = self.skipped.load(Ordering::SeqCst);
        if skipped > 0 {
            assert!(
                ui.memory(|memory| memory.has_focus(self.target)),
                "a completed retained pass must keep the existing editor focus"
            );
        }
        if self.app.is_surface_presented(SURFACE) && skipped < 2 {
            let calls = self.calls.load(Ordering::SeqCst);
            let token = eframe::current_native_output_token().unwrap();
            let mut state = lock_state(&self.app.state);
            state
                .stage_retain_previous(ui.ctx(), token, SURFACE)
                .unwrap();
            assert_eq!(self.calls.load(Ordering::SeqCst), calls);
            assert!(
                ui.ctx()
                    .viewport(|viewport| viewport.this_pass.widgets.get(self.target).is_none())
            );
            self.skipped.fetch_add(1, Ordering::SeqCst);
        } else {
            let before = self.calls.load(Ordering::SeqCst);
            eframe::App::ui(&mut self.app, ui, frame);
            if skipped == 2 && self.calls.load(Ordering::SeqCst) > before {
                assert!(ui.memory(|memory| memory.has_focus(self.target)));
                self.skipped.fetch_add(1, Ordering::SeqCst);
                ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }
        ui.ctx().request_repaint_after(Duration::from_millis(5));
    }
}

#[test]
fn real_eframe_retained_output_keeps_editor_focus_without_pane_callbacks() {
    if std::env::var_os("DISPLAY").is_none() {
        eprintln!(
            "SKIP retained editor focus: DISPLAY is unavailable; run the X11 qualification gate"
        );
        return;
    }
    let layout = DockspaceLayout::new([DockspaceSurfaceLayout::new(
        SURFACE,
        DockspaceRootLayout::new(RootId::new(1), DockspaceNode::central_tabs([EDITOR])),
    )])
    .unwrap();
    let target = egui::Id::unique("real-retained-editor");
    let calls = Arc::new(AtomicUsize::new(0));
    let skipped = Arc::new(AtomicUsize::new(0));
    let app = NativeDockspaceApp::new_with_close_policy(
        egui::Id::unique("real-retained-focus"),
        DockspaceSession::from_layout(layout, Default::default()).unwrap(),
        SURFACE,
        Editor {
            target,
            text: "retained".into(),
            calls: Arc::clone(&calls),
        },
        DockStyle::default(),
        NativeWindowClosePolicy::RetainLayout,
    )
    .unwrap();
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([640.0, 480.0]),
        native_host: Some(app.native_host_handler()),
        renderer: eframe::Renderer::Glow,
        event_loop_builder: Some(Box::new(|builder| {
            builder.with_x11().with_any_thread(true);
        })),
        ..Default::default()
    };
    let completed = Arc::clone(&skipped);
    eframe::run_native(
        "Retained editor focus regression",
        options,
        Box::new(move |_| {
            Ok(Box::new(RetainApp {
                app,
                target,
                calls,
                skipped,
                started: Instant::now(),
            }))
        }),
    )
    .unwrap();
    assert_eq!(completed.load(Ordering::SeqCst), 3);
}
