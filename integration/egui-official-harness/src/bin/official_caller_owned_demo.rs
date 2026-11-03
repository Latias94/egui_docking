#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use egui::{Context, FullOutput, ViewportId};
use egui_dockspace::{
    Dockspace, DockspaceAxis, DockspaceContainedLayout, DockspaceLayout, DockspaceNode,
    DockspaceRootLayout, DockspaceSurfaceLayout, FloatingPresentationId, ItemId, LogicalRect,
    PaneView, RootId, SurfaceId, SurfacePresentationResult,
};
use egui_wgpu::wgpu;
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

struct OfficialRenderer {
    instance: wgpu::Instance,
    surface: Option<wgpu::Surface<'static>>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    renderer: egui_wgpu::Renderer,
    config: wgpu::SurfaceConfiguration,
    needs_reconfigure: bool,
    pending_textures: egui::TexturesDelta,
}

impl OfficialRenderer {
    async fn new(
        event_loop: &ActiveEventLoop,
        window: Arc<Window>,
    ) -> Result<Self, egui_wgpu::WgpuError> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_with_display_handle(
            Box::new(event_loop.owned_display_handle()),
        ));
        let surface = instance.create_surface(Arc::clone(&window))?;
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                compatible_surface: Some(&surface),
                ..Default::default()
            })
            .await?;
        let limits = if adapter.get_info().backend == wgpu::Backend::Gl {
            wgpu::Limits::downlevel_webgl2_defaults()
        } else {
            wgpu::Limits::default()
        };
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("official demo device"),
                required_limits: limits.using_resolution(adapter.limits()),
                ..Default::default()
            })
            .await?;
        let size = window.inner_size();
        let mut config = surface
            .get_default_config(&adapter, size.width.max(1), size.height.max(1))
            .ok_or(egui_wgpu::WgpuError::NoSurfaceFormatsAvailable)?;
        config.format =
            egui_wgpu::preferred_framebuffer_format(&surface.get_capabilities(&adapter).formats)?;
        config.present_mode = wgpu::PresentMode::AutoVsync;
        let renderer = egui_wgpu::Renderer::new(
            &device,
            config.format,
            egui_wgpu::RendererOptions::default(),
        );
        Ok(Self {
            instance,
            surface: Some(surface),
            device,
            queue,
            renderer,
            config,
            needs_reconfigure: true,
            pending_textures: egui::TexturesDelta::default(),
        })
    }

    fn paint(
        &mut self,
        window: &Arc<Window>,
        pixels_per_point: f32,
        clipped_primitives: &[egui::ClippedPrimitive],
    ) -> SurfacePresentationResult {
        let size = window.inner_size();
        if size.width == 0 || size.height == 0 {
            return SurfacePresentationResult::Dropped;
        }
        if self.surface.is_none() {
            match self.instance.create_surface(Arc::clone(window)) {
                Ok(surface) => self.surface = Some(surface),
                Err(error) => {
                    eprintln!("Failed to recreate the official demo surface: {error}");
                    return SurfacePresentationResult::Dropped;
                }
            }
            self.needs_reconfigure = true;
        }
        let surface = self
            .surface
            .as_ref()
            .expect("the surface was created above");
        if self.needs_reconfigure
            || self.config.width != size.width
            || self.config.height != size.height
        {
            self.config.width = size.width;
            self.config.height = size.height;
            surface.configure(&self.device, &self.config);
            self.needs_reconfigure = false;
        }
        let frame = match surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(frame) => frame,
            wgpu::CurrentSurfaceTexture::Suboptimal(frame) => {
                // Reconfigure only after this frame has been presented and released.
                self.needs_reconfigure = true;
                window.request_redraw();
                frame
            }
            wgpu::CurrentSurfaceTexture::Outdated => {
                self.needs_reconfigure = true;
                window.request_redraw();
                return SurfacePresentationResult::Dropped;
            }
            wgpu::CurrentSurfaceTexture::Lost => {
                // Release the lost surface before creating its replacement next frame.
                self.surface = None;
                window.request_redraw();
                return SurfacePresentationResult::Dropped;
            }
            wgpu::CurrentSurfaceTexture::Timeout => {
                window.request_redraw();
                return SurfacePresentationResult::Dropped;
            }
            wgpu::CurrentSurfaceTexture::Occluded | wgpu::CurrentSurfaceTexture::Validation => {
                return SurfacePresentationResult::Dropped;
            }
        };

        // Acquire before consuming any deltas. Failed acquisitions retain the initial font atlas,
        // subsequent patches, and deferred frees until a later frame can upload and submit them.
        for (id, image_deltas) in self.pending_textures.set.drain() {
            for image_delta in image_deltas {
                self.renderer
                    .update_texture(&self.device, &self.queue, id, &image_delta);
            }
        }
        let screen_descriptor = egui_wgpu::ScreenDescriptor {
            size_in_pixels: [size.width, size.height],
            pixels_per_point,
        };
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("official demo encoder"),
            });
        let user_commands = self.renderer.update_buffers(
            &self.device,
            &self.queue,
            &mut encoder,
            clipped_primitives,
            &screen_descriptor,
        );
        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        {
            let render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("official demo egui pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    depth_slice: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: 0.055,
                            g: 0.06,
                            b: 0.07,
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                ..Default::default()
            });
            self.renderer.render(
                &mut render_pass.forget_lifetime(),
                clipped_primitives,
                &screen_descriptor,
            );
        }
        self.queue
            .submit(user_commands.into_iter().chain([encoder.finish()]));
        // Current-frame draws may still reference these textures until submission.
        for id in self.pending_textures.free.drain() {
            self.renderer.free_texture(&id);
        }
        window.pre_present_notify();
        self.queue.present(frame);
        SurfacePresentationResult::Presented
    }
}

impl Drop for OfficialRenderer {
    fn drop(&mut self) {
        // The renderer and context are shutting down; no future frame will use these deltas.
        self.pending_textures.clear();
    }
}

struct OfficialDemoHost {
    proxy: EventLoopProxy<UserEvent>,
    window: Option<Arc<Window>>,
    context: Context,
    egui_winit: Option<egui_winit::State>,
    painter: Option<OfficialRenderer>,
    viewport_info: egui::ViewportInfo,
    dockspace: Dockspace,
    panes: DemoPanes,
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

        let input = egui_winit.take_egui_input(window);
        let (output, _) =
            match self
                .dockspace
                .run_single_surface(&self.context, input, SURFACE, &mut self.panes)
            {
                Ok(published) => published,
                Err(error) => {
                    window.set_title(&format!("Dockspace update rejected: {error}"));
                    window.request_redraw();
                    return;
                }
            };
        let (output, receipt) = output.into_parts();

        let FullOutput {
            platform_output,
            textures_delta,
            shapes,
            pixels_per_point,
            viewport_output,
        } = output;
        painter.pending_textures.append(textures_delta);
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
        // A panic before paint returns leaves the receipt to report Dropped through RAII.
        let presentation = painter.paint(window, pixels_per_point, &clipped_primitives);
        receipt.report(presentation);
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
        let painter = pollster::block_on(OfficialRenderer::new(event_loop, Arc::clone(&window)))
            .expect("the official demo initializes its wgpu surface");
        egui_winit.set_max_texture_side(painter.device.limits().max_texture_dimension_2d as usize);
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
        if matches!(
            event,
            WindowEvent::Resized(_) | WindowEvent::Occluded(false)
        ) {
            window.request_redraw();
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
                let next = self
                    .next_repaint
                    .map_or(requested, |current| current.min(requested));
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
        self.painter = None;
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
