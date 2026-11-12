//! WGPU presentation and texture-delta ownership for the single-window runner.

use std::sync::Arc;

use crate::SurfacePresentationResult;
use egui_wgpu::wgpu;
use winit::event_loop::ActiveEventLoop;
use winit::window::Window;

pub(super) struct WgpuRenderer {
    pub(super) drop_reason: Option<&'static str>,
    clear_color: wgpu::Color,
    instance: wgpu::Instance,
    surface: Option<wgpu::Surface<'static>>,
    pub(super) device: wgpu::Device,
    queue: wgpu::Queue,
    renderer: egui_wgpu::Renderer,
    pub(super) config: wgpu::SurfaceConfiguration,
    needs_reconfigure: bool,
    pub(super) pending_textures: egui::TexturesDelta,
}

impl WgpuRenderer {
    pub(super) async fn new(
        event_loop: &ActiveEventLoop,
        window: Arc<Window>,
        clear_color: egui::Color32,
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
        let adapter_info = adapter.get_info();
        let limits = if adapter_info.backend == wgpu::Backend::Gl {
            wgpu::Limits::downlevel_webgl2_defaults()
        } else {
            wgpu::Limits::default()
        };
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("dockspace device"),
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
        let [r, g, b, a] = egui::Rgba::from(clear_color).to_array();
        Ok(Self {
            clear_color: wgpu::Color {
                r: f64::from(r),
                g: f64::from(g),
                b: f64::from(b),
                a: f64::from(a),
            },
            drop_reason: None,
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

    pub(super) fn paint(
        &mut self,
        window: &Arc<Window>,
        pixels_per_point: f32,
        clipped_primitives: &[egui::ClippedPrimitive],
    ) -> SurfacePresentationResult {
        self.drop_reason = None;
        let size = window.inner_size();
        if size.width == 0 || size.height == 0 {
            self.drop_reason = Some("zero-sized window");
            return SurfacePresentationResult::Dropped;
        }
        if self.surface.is_none() {
            match self.instance.create_surface(Arc::clone(window)) {
                Ok(surface) => self.surface = Some(surface),
                Err(error) => {
                    self.drop_reason = Some("surface recreation failed");
                    eprintln!("Failed to recreate the dockspace surface: {error}");
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
                self.drop_reason = Some("surface outdated");
                self.needs_reconfigure = true;
                window.request_redraw();
                return SurfacePresentationResult::Dropped;
            }
            wgpu::CurrentSurfaceTexture::Lost => {
                self.drop_reason = Some("surface lost");
                // Release the lost surface before creating its replacement next frame.
                self.surface = None;
                window.request_redraw();
                return SurfacePresentationResult::Dropped;
            }
            wgpu::CurrentSurfaceTexture::Timeout => {
                self.drop_reason = Some("surface timeout");
                window.request_redraw();
                return SurfacePresentationResult::Dropped;
            }
            wgpu::CurrentSurfaceTexture::Occluded => {
                self.drop_reason = Some("surface occluded");
                return SurfacePresentationResult::Dropped;
            }
            wgpu::CurrentSurfaceTexture::Validation => {
                self.drop_reason = Some("surface validation failure");
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
                label: Some("dockspace encoder"),
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
                label: Some("dockspace egui pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    depth_slice: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(self.clear_color),
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

impl Drop for WgpuRenderer {
    fn drop(&mut self) {
        // The renderer and context are shutting down; no future frame will use these deltas.
        self.pending_textures.clear();
    }
}
