#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::collections::BTreeMap;
use std::num::NonZeroU32;
use std::sync::Arc;
use std::time::{Duration, Instant};

use egui::{Context, FullOutput, ViewportId};
use egui_dockspace::{
    Dockspace, DockspaceAxis, DockspaceCapability, DockspaceContainedLayout, DockspaceLayout,
    DockspaceNode, DockspaceRootLayout, DockspaceSurfaceLayout, DockspaceUnavailableReason,
    FloatingPresentationId, ItemId, LogicalRect, PaneView, RootId, SurfaceId, run_ui,
};
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy};
use winit::window::{Window, WindowAttributes, WindowId};

const SURFACE: SurfaceId = SurfaceId::new(1);
const MAIN_ROOT: RootId = RootId::new(1);
const FLOATING_ROOT: RootId = RootId::new(2);
const FLOATING: FloatingPresentationId = FloatingPresentationId::new(1);

const EDITOR: ItemId = ItemId::new(1);
const PREVIEW: ItemId = ItemId::new(2);
const OUTLINE: ItemId = ItemId::new(3);
const INSPECTOR: ItemId = ItemId::new(4);

#[derive(Debug)]
enum UserEvent {
    RequestRepaint(Duration),
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let event_loop = EventLoop::<UserEvent>::with_user_event().build()?;
    let mut host = OfficialDemoHost::new(event_loop.create_proxy());
    event_loop.run_app(&mut host)?;
    Ok(())
}

struct OfficialDemoHost {
    proxy: EventLoopProxy<UserEvent>,
    window: Option<Arc<Window>>,
    context: Context,
    egui_winit: Option<egui_winit::State>,
    painter: Option<egui_wgpu::winit::Painter>,
    viewport_info: egui::ViewportInfo,
    dockspace: Dockspace,
    panes: DemoPanes,
    presentation_settlement: DockspaceCapability,
    error: Option<String>,
    next_repaint: Option<Instant>,
}

impl OfficialDemoHost {
    fn new(proxy: EventLoopProxy<UserEvent>) -> Self {
        Self {
            proxy,
            window: None,
            context: Context::default(),
            egui_winit: None,
            painter: None,
            viewport_info: egui::ViewportInfo::default(),
            dockspace: Dockspace::builder("official-caller-owned-demo", demo_layout())
                .build()
                .expect("the static demo layout is valid"),
            panes: DemoPanes::new(),
            presentation_settlement: DockspaceCapability::Unavailable(
                DockspaceUnavailableReason::PresentationSettlementRequired,
            ),
            error: None,
            next_repaint: None,
        }
    }

    fn render(&mut self) {
        let Some(window) = self.window.as_ref() else {
            return;
        };
        let Some(egui_winit) = self.egui_winit.as_mut() else {
            return;
        };
        let Some(painter) = self.painter.as_mut() else {
            return;
        };

        let mut input = egui_winit.take_egui_input(window);
        painter.handle_screenshots(&mut input.events);
        let mut presentation_settlement = self.presentation_settlement;
        let mut error = self.error.take();
        let output = run_ui(&self.context, input, |ui| {
            egui::Panel::top("official-demo-help").show(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.strong("Official crates.io caller-owned run");
                    ui.separator();
                    ui.label(
                        "Drag a tab body into the center or edge of another pane. Drag the dotted Inspector title strip to move the full floating window.",
                    );
                    ui.separator();
                    ui.monospace(format!("Settlement: {presentation_settlement:?}"));
                });
            });
            if let Some(message) = error.as_deref() {
                egui::Panel::bottom("official-demo-error").show(ui, |ui| {
                    ui.colored_label(ui.visuals().error_fg_color, message);
                });
            }
            egui::CentralPanel::default().show(ui, |ui| {
                match self
                    .dockspace
                    .show_single_surface(SURFACE, ui, &mut self.panes)
                {
                    Ok(response) => {
                        presentation_settlement = response.presentation_settlement_capability();
                        error = None;
                    }
                    Err(current) => error = Some(current.to_string()),
                }
            });
        });
        self.presentation_settlement = presentation_settlement;
        self.error = error;

        let FullOutput {
            platform_output,
            mut textures_delta,
            shapes,
            pixels_per_point,
            viewport_output,
        } = output;
        for (_, viewport) in viewport_output {
            let mut actions_requested = Vec::new();
            egui_winit::process_viewport_commands(
                &self.context,
                &mut self.viewport_info,
                viewport.commands,
                window,
                &mut actions_requested,
            );
        }
        egui_winit.handle_platform_output(window, platform_output);
        let clipped_primitives = self.context.tessellate(shapes, pixels_per_point);
        painter.paint_and_update_textures(
            ViewportId::ROOT,
            pixels_per_point,
            [0.055, 0.06, 0.07, 1.0],
            &clipped_primitives,
            &mut textures_delta,
            Vec::new(),
            window,
        );
    }
}

impl ApplicationHandler<UserEvent> for OfficialDemoHost {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let attributes = WindowAttributes::default()
            .with_title("egui_dockspace — official caller-owned run")
            .with_inner_size(winit::dpi::LogicalSize::new(1100.0, 720.0));
        let window = Arc::new(
            event_loop
                .create_window(attributes)
                .expect("the official demo creates one window"),
        );
        let mut egui_winit = egui_winit::State::new(
            self.context.clone(),
            ViewportId::ROOT,
            event_loop,
            Some(window.scale_factor() as f32),
            event_loop.system_theme(),
            None,
        );
        let mut painter = pollster::block_on(egui_wgpu::winit::Painter::new(
            self.context.clone(),
            egui_wgpu::WgpuConfiguration::default(),
            false,
            egui_wgpu::RendererOptions::default(),
        ));
        pollster::block_on(painter.set_window(ViewportId::ROOT, Some(Arc::clone(&window))))
            .expect("the official demo initializes its wgpu surface");
        if let Some(max_texture_side) = painter.max_texture_side() {
            egui_winit.set_max_texture_side(max_texture_side);
        }
        let proxy = self.proxy.clone();
        self.context.set_request_repaint_callback(move |info| {
            let _ = proxy.send_event(UserEvent::RequestRepaint(info.delay));
        });
        window.request_redraw();
        self.window = Some(window);
        self.egui_winit = Some(egui_winit);
        self.painter = Some(painter);
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        window_id: WindowId,
        event: WindowEvent,
    ) {
        let Some(window) = self.window.as_ref() else {
            return;
        };
        if window.id() != window_id {
            return;
        }
        if matches!(event, WindowEvent::CloseRequested | WindowEvent::Destroyed) {
            event_loop.exit();
            return;
        }
        if matches!(event, WindowEvent::RedrawRequested) {
            self.render();
            return;
        }
        if let WindowEvent::Resized(size) = event
            && let (Some(width), Some(height)) =
                (NonZeroU32::new(size.width), NonZeroU32::new(size.height))
            && let Some(painter) = self.painter.as_mut()
        {
            painter.on_window_resized(ViewportId::ROOT, width, height);
        }
        if let Some(egui_winit) = self.egui_winit.as_mut()
            && egui_winit.on_window_event(window, &event).repaint
        {
            window.request_redraw();
        }
    }

    fn user_event(&mut self, event_loop: &ActiveEventLoop, event: UserEvent) {
        let Some(window) = self.window.as_ref() else {
            return;
        };
        match event {
            UserEvent::RequestRepaint(delay) if delay.is_zero() => window.request_redraw(),
            UserEvent::RequestRepaint(delay) => {
                let requested = Instant::now() + delay;
                let next = self.next_repaint.map_or(requested, |current| current.min(requested));
                self.next_repaint = Some(next);
                event_loop.set_control_flow(ControlFlow::WaitUntil(next));
            }
        }
    }

    fn new_events(&mut self, event_loop: &ActiveEventLoop, cause: winit::event::StartCause) {
        if matches!(cause, winit::event::StartCause::ResumeTimeReached { .. })
            && let Some(window) = self.window.as_ref()
        {
            self.next_repaint = None;
            event_loop.set_control_flow(ControlFlow::Wait);
            window.request_redraw();
        }
    }

    fn exiting(&mut self, _event_loop: &ActiveEventLoop) {
        if let Some(painter) = self.painter.as_mut() {
            painter.destroy();
        }
    }
}

fn demo_layout() -> DockspaceLayout {
    let main = DockspaceNode::split(
        DockspaceAxis::Horizontal,
        [
            (DockspaceNode::tabs([OUTLINE]), 0.24),
            (DockspaceNode::central_tabs([EDITOR, PREVIEW]), 0.76),
        ],
    )
    .expect("the static split is valid");
    let contained = DockspaceContainedLayout::new(
        FLOATING,
        DockspaceRootLayout::new(FLOATING_ROOT, DockspaceNode::tabs([INSPECTOR])),
        LogicalRect::new(700.0, 100.0, 300.0, 300.0).expect("the static rect is valid"),
    );
    DockspaceLayout::new([DockspaceSurfaceLayout::new(
        SURFACE,
        DockspaceRootLayout::new(MAIN_ROOT, main),
    )
    .with_contained(contained)])
    .expect("the demo layout is valid")
}

struct DemoPane {
    title: &'static str,
    text: String,
}

struct DemoPanes {
    panes: BTreeMap<ItemId, DemoPane>,
}

impl DemoPanes {
    fn new() -> Self {
        Self {
            panes: BTreeMap::from([
                (
                    EDITOR,
                    DemoPane {
                        title: "Editor",
                        text: "fn main() {\n    println!(\"drag me\");\n}".to_owned(),
                    },
                ),
                (
                    PREVIEW,
                    DemoPane {
                        title: "Preview",
                        text: "Magnetic center and edge docking preview".to_owned(),
                    },
                ),
                (
                    OUTLINE,
                    DemoPane {
                        title: "Outline",
                        text: "workspace\n  editor\n  preview\n  inspector".to_owned(),
                    },
                ),
                (
                    INSPECTOR,
                    DemoPane {
                        title: "Inspector",
                        text: "Drag the dotted title strip to move this whole window.".to_owned(),
                    },
                ),
            ]),
        }
    }
}

impl PaneView for DemoPanes {
    fn title(&self, item: ItemId) -> Option<egui::WidgetText> {
        self.panes.get(&item).map(|pane| pane.title.into())
    }

    fn ui(&mut self, item: ItemId, ui: &mut egui::Ui) {
        let Some(pane) = self.panes.get_mut(&item) else {
            return;
        };
        ui.add(
            egui::TextEdit::multiline(&mut pane.text)
                .code_editor()
                .desired_width(f32::INFINITY)
                .desired_rows(16),
        );
    }
}
