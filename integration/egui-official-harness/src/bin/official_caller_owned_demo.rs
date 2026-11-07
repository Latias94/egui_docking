#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use egui::{Context, FullOutput, ViewportId};
use egui_dockspace::{
    CloseDecision, CloseItemDecisionState, Dockspace, DockspaceAxis, DockspaceContainedLayout,
    DockspaceError, DockspaceLayout, DockspaceNode, DockspaceOutput, DockspaceResponse,
    DockspaceRootLayout, DockspaceSurfaceLayout, FloatingPresentationId, ItemId, LogicalRect,
    PaneResponse, PaneView, RootId, SurfaceId, SurfacePresentationResult,
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
const CONSOLE: ItemId = ItemId::new(5);
const HISTORY: ItemId = ItemId::new(6);
const PROFILER: ItemId = ItemId::new(7);
const ASSETS: ItemId = ItemId::new(8);

#[path = "official_caller_owned_demo/exercise.rs"]
mod exercise;

#[derive(Debug)]
enum UserEvent {
    RequestRepaint(Duration),
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let exercise = exercise::Exercise::from_args()?;
    let event_loop = EventLoop::<UserEvent>::with_user_event().build()?;
    let mut host = OfficialDemoHost::new(event_loop.create_proxy(), exercise);
    event_loop.run_app(&mut host)?;
    if let Some(error) = host.failure {
        return Err(error.into());
    }
    if let Some(exercise) = host.exercise {
        exercise.require_complete()?;
    }
    Ok(())
}

struct OfficialRenderer {
    adapter_info: wgpu::AdapterInfo,
    drop_reason: Option<&'static str>,
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
        let adapter_info = adapter.get_info();
        let limits = if adapter_info.backend == wgpu::Backend::Gl {
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
            adapter_info,
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

    fn paint(
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
    exercise: Option<exercise::Exercise>,
    failure: Option<String>,
}

impl OfficialDemoHost {
    fn new(proxy: EventLoopProxy<UserEvent>, exercise: Option<exercise::Exercise>) -> Self {
        let context = Context::default();
        if exercise.is_some() {
            context.enable_accesskit();
        }
        Self {
            proxy,
            window: None,
            context,
            egui_winit: None,
            painter: None,
            viewport_info: egui::ViewportInfo::default(),
            dockspace: Dockspace::builder("official-caller-owned-demo", demo_layout())
                .build()
                .expect("the static demo layout is valid"),
            panes: DemoPanes::new(),
            next_repaint: None,
            exercise,
            failure: None,
        }
    }

    fn render(&mut self) -> Result<bool, String> {
        let Some(window) = self.window.as_ref() else {
            return Ok(false);
        };
        let Some(egui_winit) = self.egui_winit.as_mut() else {
            return Ok(false);
        };
        let Some(painter) = self.painter.as_mut() else {
            return Ok(false);
        };

        let mut input = egui_winit.take_egui_input(window);
        if let Some(exercise) = &mut self.exercise {
            exercise.prepare(&mut input, &self.panes)?;
        }
        let frame_started = Instant::now();
        let mut restore_layout = false;
        let (output, response) = match run_editor(
            &mut self.dockspace,
            &self.context,
            input,
            &mut self.panes,
            &mut restore_layout,
        ) {
            Ok(published) => published,
            Err(error) => {
                if self.exercise.is_some() {
                    return Err(format!("exercise update rejected: {error}"));
                }
                window.set_title(&format!("Dockspace update rejected: {error}"));
                window.request_redraw();
                return Ok(false);
            }
        };
        let update_ns = frame_started.elapsed().as_nanos();
        let (output, receipt) = output.into_parts();

        let FullOutput {
            mut platform_output,
            textures_delta,
            shapes,
            pixels_per_point,
            viewport_output,
        } = output;
        let observations = self
            .exercise
            .as_ref()
            .and_then(|_| platform_output.accesskit_update.take());
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
        let tessellation_started = Instant::now();
        let clipped_primitives = self.context.tessellate(shapes, pixels_per_point);
        let tessellate_ns = tessellation_started.elapsed().as_nanos();
        let paint_started = Instant::now();
        // A panic before paint returns leaves the receipt to report Dropped through RAII.
        let presentation = painter.paint(window, pixels_per_point, &clipped_primitives);
        let paint_present_ns = paint_started.elapsed().as_nanos();
        receipt.report(presentation);
        let ui_to_present_ns = frame_started.elapsed().as_nanos();
        if apply_editor_commands(&mut self.dockspace, response.as_ref(), restore_layout)
            .map_err(|error| format!("editor command failed: {error}"))?
        {
            window.request_redraw();
        }
        if let Some(exercise) = &mut self.exercise {
            exercise.observe(observations.as_ref());
            if presentation == SurfacePresentationResult::Dropped && exercise.first_drop() {
                eprintln!(
                    "egui-host-exercise: initial dropped output: {:?}; visible={:?}; minimized={:?}; focused={}",
                    painter.drop_reason,
                    window.is_visible(),
                    window.is_minimized(),
                    window.has_focus()
                );
            }
            let done = exercise.finish_frame(
                &self.panes,
                &self.dockspace,
                presentation,
                &clipped_primitives,
                exercise::Timing {
                    update_ns,
                    tessellate_ns,
                    paint_present_ns,
                    ui_to_present_ns,
                },
            )?;
            if !done {
                if presentation == SurfacePresentationResult::Dropped {
                    self.context
                        .request_repaint_after(Duration::from_millis(16));
                } else {
                    window.request_redraw();
                }
            }
            return Ok(done);
        }
        Ok(false)
    }
}

fn editor_toolbar(ui: &mut egui::Ui, restore_layout: &mut bool) {
    egui::Panel::top("editor-toolbar").show(ui, |ui| {
        ui.horizontal_wrapped(|ui| {
            ui.strong("Dockspace Editor");
            ui.separator();
            *restore_layout |= ui
                .button("Restore layout")
                .on_hover_text(
                    "Reopen all panes and restore their arrangement. Keep document edits.",
                )
                .clicked();
            ui.separator();
            ui.weak("Drag tabs to arrange · Shift skips magnetic snapping");
        });
    });
}

fn run_editor(
    dockspace: &mut Dockspace,
    context: &Context,
    input: egui::RawInput,
    panes: &mut DemoPanes,
    restore_layout: &mut bool,
) -> Result<(DockspaceOutput, Option<DockspaceResponse>), DockspaceError> {
    if dockspace.view().surfaces().next().is_none() {
        let output = egui_dockspace::run_ui(context, input, |ui| {
            editor_toolbar(ui, restore_layout);
            ui.centered_and_justified(|ui| {
                ui.label(
                    "All panes are closed. Restore layout to reopen them and keep your edits.",
                );
            });
        });
        return Ok((output, None));
    }
    dockspace
        .run_single_surface_with_ui(context, input, SURFACE, panes, |ui| {
            editor_toolbar(ui, restore_layout);
        })
        .map(|(output, response)| (output, Some(response)))
}

fn apply_editor_commands(
    dockspace: &mut Dockspace,
    response: Option<&DockspaceResponse>,
    restore_layout: bool,
) -> Result<bool, DockspaceError> {
    if restore_layout {
        return dockspace
            .reset_layout(demo_layout())
            .map(|mutation| mutation.workspace_changed());
    }
    let mut changed = false;
    // This example allows closing sample panes while retaining their application data.
    // Production applications can veto or defer the same requests for unsaved work.
    for request in response
        .into_iter()
        .flat_map(DockspaceResponse::close_request_events)
    {
        if request.reused() {
            continue;
        }
        let plan = request.plan();
        for item in plan.items() {
            if item.state() == CloseItemDecisionState::Pending {
                changed |= dockspace
                    .resolve_close(plan.request(), item.token(), CloseDecision::Allow)?
                    .mutation()
                    .workspace_changed();
            }
        }
    }
    Ok(changed)
}

impl ApplicationHandler<UserEvent> for OfficialDemoHost {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let attributes = WindowAttributes::default()
            .with_title("egui_dockspace — official caller-owned run")
            .with_inner_size(winit::dpi::LogicalSize::new(1280.0, 820.0));
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
        if self.exercise.is_some() {
            window.set_visible(true);
            window.focus_window();
            eprintln!(
                "egui-host-exercise: adapter={:?}; physical_size={:?}; scale={}; present_mode={:?}; format={:?}",
                painter.adapter_info,
                window.inner_size(),
                window.scale_factor(),
                painter.config.present_mode,
                painter.config.format,
            );
        }
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
        if self.exercise.is_some()
            && matches!(event, WindowEvent::Occluded(_) | WindowEvent::Focused(_))
        {
            eprintln!("egui-host-exercise: window event {event:?}");
        }
        if matches!(event, WindowEvent::CloseRequested | WindowEvent::Destroyed) {
            event_loop.exit();
            return;
        }
        if matches!(event, WindowEvent::RedrawRequested) {
            match self.render() {
                Ok(true) => event_loop.exit(),
                Ok(false) => {}
                Err(error) => {
                    self.failure = Some(error);
                    event_loop.exit();
                }
            }
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
    let top = DockspaceNode::split(
        DockspaceAxis::Horizontal,
        [
            (DockspaceNode::tabs([OUTLINE]), 0.24),
            (DockspaceNode::central_tabs([EDITOR, PREVIEW]), 0.76),
        ],
    )
    .expect("the static split is valid");
    let bottom = DockspaceNode::split(
        DockspaceAxis::Horizontal,
        [
            (DockspaceNode::tabs([CONSOLE, HISTORY]), 0.65),
            (DockspaceNode::tabs([PROFILER, ASSETS]), 0.35),
        ],
    )
    .expect("the bottom split is valid");
    let main = DockspaceNode::split(DockspaceAxis::Vertical, [(top, 0.7), (bottom, 0.3)])
        .expect("the editor rows are valid");
    let contained = DockspaceContainedLayout::new(
        FLOATING,
        DockspaceRootLayout::new(FLOATING_ROOT, DockspaceNode::tabs([INSPECTOR])),
        LogicalRect::new(940.0, 90.0, 300.0, 300.0).expect("the static rect is valid"),
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
    content: Option<egui::Rect>,
    editor: Option<egui::Rect>,
    scroll_offset: f32,
    scroll_bounds: Option<egui::Rect>,
}

struct DemoPanes {
    panes: BTreeMap<ItemId, DemoPane>,
}

impl DemoPanes {
    fn new() -> Self {
        Self {
            panes: [
                (
                    EDITOR,
                    "Editor",
                    "fn main() {\n    println!(\"drag me\");\n}",
                ),
                (
                    PREVIEW,
                    "Preview",
                    "Magnetic center and edge docking preview",
                ),
                (
                    OUTLINE,
                    "Outline",
                    "workspace\n  editor\n  preview\n  inspector",
                ),
                (
                    INSPECTOR,
                    "Inspector",
                    "Drag the dotted title strip to move this whole window.",
                ),
                (CONSOLE, "Console", "Application log"),
                (
                    HISTORY,
                    "History",
                    "Selected object\nMoved object\nSaved document",
                ),
                (
                    PROFILER,
                    "Profiler",
                    "Illustrative values; no engine attached",
                ),
                (
                    ASSETS,
                    "Assets",
                    "scene/main.scene\nmesh/player.mesh\ntexture/grid.png",
                ),
            ]
            .into_iter()
            .map(|(item, title, text)| {
                (
                    item,
                    DemoPane {
                        title,
                        text: text.to_owned(),
                        content: None,
                        editor: None,
                        scroll_offset: 0.0,
                        scroll_bounds: None,
                    },
                )
            })
            .collect(),
        }
    }
}

impl PaneView for DemoPanes {
    fn title(&self, item: ItemId) -> Option<egui::WidgetText> {
        self.panes.get(&item).map(|pane| pane.title.into())
    }

    fn ui(&mut self, item: ItemId, ui: &mut egui::Ui) -> PaneResponse {
        let Some(pane) = self.panes.get_mut(&item) else {
            return PaneResponse::default();
        };
        pane.content = Some(ui.available_rect_before_wrap());
        pane.editor = None;
        if item == CONSOLE {
            let log = egui::ScrollArea::vertical()
                .id_salt("official-editor-console")
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    let mut content = PaneResponse::default().with_content_ids([ui.unique_id()]);
                    for line in 0..128 {
                        content = content
                            .with_content(&ui.monospace(format!("[{line:03}] Scene updated")));
                    }
                    content
                });
            pane.scroll_offset = log.state.clamped_offset().y;
            pane.scroll_bounds = Some(log.inner_rect);
            let controls = [log.id.with("area"), log.id.with(1_usize)]
                .into_iter()
                .filter(|id| {
                    ui.ctx()
                        .viewport(|v| v.this_pass.widgets.get(*id).is_some())
                });
            log.inner.with_content_ids(controls)
        } else if item == PROFILER {
            let mut response = PaneResponse::from(&ui.label(&pane.text));
            for (name, value) in [("Simulation", 0.16), ("Rendering", 0.32), ("UI", 0.05)] {
                response = response.with_content(&ui.add(egui::ProgressBar::new(value).text(name)));
            }
            response
        } else {
            let response = ui.add_sized(
                ui.available_size(),
                egui::TextEdit::multiline(&mut pane.text)
                    .code_editor()
                    .desired_width(f32::INFINITY),
            );
            pane.editor = Some(response.rect);
            PaneResponse::from(&response)
        }
    }
}
