//! Complete-output single-window hosting with official winit and WGPU.

use std::{
    error::Error,
    sync::Arc,
    time::{Duration, Instant},
};

use egui::{Context, FullOutput, RawInput, ViewportId};
use winit::{
    application::ApplicationHandler,
    event::WindowEvent,
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy},
    window::{Window, WindowAttributes, WindowId},
};

use super::{ActionRequested, WinitIntegration};
use crate::{DockspaceOutput, SurfacePresentationResult};

#[path = "renderer.rs"]
mod renderer;
#[path = "repaint.rs"]
mod repaint;
use renderer::WgpuRenderer;
use repaint::{Repaint, RepaintRequest};

const HIDDEN_REPAINT_INTERVAL: Duration = Duration::from_millis(250);

fn needs_direct_update(window: &Window) -> bool {
    window.is_visible() == Some(false)
        // Runtime minimized-state queries can deadlock on macOS. Windows needs
        // this fallback because hidden/minimized windows may never receive RedrawRequested.
        || (cfg!(target_os = "windows") && window.is_minimized() == Some(true))
}

/// Application failures returned from a native frame callback.
pub type AppResult<T> = Result<T, Box<dyn Error + Send + Sync>>;

/// Startup and execution failure of a complete-output native host.
#[derive(Debug, thiserror::Error)]
pub enum NativeError {
    /// The event loop could not start or continue.
    #[error("native event loop failed: {0}")]
    EventLoop(#[from] winit::error::EventLoopError),
    /// The operating system could not create the window.
    #[error("native window creation failed: {0}")]
    Window(#[from] winit::error::OsError),
    /// WGPU could not initialize its renderer and presentation surface.
    #[error("native renderer initialization failed: {0}")]
    Renderer(#[from] egui_wgpu::WgpuError),
    /// The application's frame callback failed.
    #[error("native application failed: {0}")]
    Application(#[source] Box<dyn Error + Send + Sync>),
    /// A single-window runner cannot create another native viewport.
    #[error("the single-window runner received a separate native viewport")]
    AdditionalViewport,
    /// A screenshot requires renderer readback, which this runner does not provide.
    #[error("the single-window runner does not support screenshot readback")]
    ScreenshotUnsupported,
}

/// Configuration for one application-owned desktop window.
pub struct NativeOptions {
    /// Native title, initial dimensions, decorations, and other winit attributes.
    pub window: WindowAttributes,
    /// Background under the complete egui output.
    pub clear_color: egui::Color32,
}

impl Default for NativeOptions {
    fn default() -> Self {
        Self {
            window: WindowAttributes::default()
                .with_title("Dockspace")
                .with_inner_size(winit::dpi::LogicalSize::new(1100.0, 760.0)),
            clear_color: egui::Color32::from_rgb(24, 26, 30),
        }
    }
}

/// Scheduling decision after one renderer attempt and its exact receipt report.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum FrameControl {
    /// Wait for input or a requested repaint deadline.
    #[default]
    Continue,
    /// Request another application frame immediately.
    Repaint,
    /// Exit the event loop after this frame.
    Exit,
}

/// Read-only observations after the renderer reported this exact output.
///
/// Durations measure CPU work through the return from `queue.present`. They do
/// not measure GPU completion, display scanout, or operating-system input delivery.
pub struct NativeFrameReport<'frame> {
    /// Context used by the application run which produced this output.
    pub context: &'frame Context,
    /// Exact renderer disposition already reported to the docking receipt.
    pub presentation: SurfacePresentationResult,
    /// Renderer diagnostic when the output was dropped, if one is available.
    pub drop_reason: Option<&'static str>,
    /// Accessibility tree update from this complete output, when generated.
    pub accesskit_update: Option<&'frame egui::accesskit::TreeUpdate>,
    /// Tessellated paint primitives retained for this renderer attempt.
    pub primitives: &'frame [egui::ClippedPrimitive],
    /// CPU time spent in [`NativeApp::update`].
    pub update_duration: std::time::Duration,
    /// CPU time spent tessellating the output's shapes.
    pub tessellation_duration: std::time::Duration,
    /// CPU time spent in the renderer's paint and presentation attempt.
    pub paint_duration: std::time::Duration,
    /// Total CPU time from the application update through renderer settlement.
    pub frame_duration: std::time::Duration,
}

/// Application code inside the complete-output runner.
///
/// The application owns its Dockspace and panes. `update` must return the exact
/// output of its owned egui run; the runner does not start a second egui run.
/// [`run_native`] accepts both concrete implementations and borrowed trait
/// objects, including providers which borrow application state for the run.
pub trait NativeApp {
    /// Optionally adjusts input before application timing begins.
    fn prepare_input(&mut self, _input: &mut RawInput) -> AppResult<()> {
        Ok(())
    }

    /// Produces one complete output, including its affine presentation receipt.
    fn update(&mut self, context: &Context, input: RawInput) -> AppResult<DockspaceOutput>;

    /// Applies commands or observes diagnostics after exact renderer settlement.
    fn frame_finished(&mut self, _frame: NativeFrameReport<'_>) -> AppResult<FrameControl> {
        Ok(FrameControl::Continue)
    }
}

/// Runs a desktop application in one OS window using official winit and WGPU.
///
/// Configure fonts and theme on `context` before calling. Panes may float inside
/// the window; separate OS viewports require a different host. Close requests
/// reach the root egui viewport and may be rejected with `ViewportCommand::CancelClose`.
/// The renderer reports Presented only after submitting and presenting the exact
/// complete output. Failed acquisitions report Dropped and retain texture updates.
/// Surface recovery is paced, and hidden-window updates run only when requested.
/// Those updates still process close and visibility commands on platforms which
/// withhold redraw events for hidden windows.
///
/// Applications selected at runtime can be passed as borrowed trait objects:
///
/// ```no_run
/// use egui_dockspace::winit::{NativeApp, NativeError, NativeOptions, run_native};
///
/// fn launch(app: &mut dyn NativeApp) -> Result<(), NativeError> {
///     let context = egui::Context::default();
///     run_native(NativeOptions::default(), context, app)
/// }
/// ```
///
/// # Errors
///
/// Returns startup failures, application callback failures, unsupported output,
/// or an event-loop failure. Call this on the platform's event-loop thread.
pub fn run_native(
    options: NativeOptions,
    context: Context,
    app: &mut (impl NativeApp + ?Sized),
) -> Result<(), NativeError> {
    let event_loop = EventLoop::<UserEvent>::with_user_event().build()?;
    let mut runner = Runner {
        proxy: event_loop.create_proxy(),
        context,
        options,
        app,
        window: None,
        repaint: Repaint::default(),
        failure: None,
    };
    event_loop.run_app(&mut runner)?;
    runner.failure.map_or(Ok(()), Err)
}

enum UserEvent {
    Repaint(RepaintRequest),
    #[cfg(feature = "accesskit")]
    AccessKit(egui_winit::accesskit_winit::Event),
}

#[cfg(feature = "accesskit")]
impl From<egui_winit::accesskit_winit::Event> for UserEvent {
    fn from(event: egui_winit::accesskit_winit::Event) -> Self {
        Self::AccessKit(event)
    }
}

fn stage_output(
    mut output: FullOutput,
    pending_textures: &mut egui::TexturesDelta,
) -> Result<FullOutput, NativeError> {
    // Take texture ownership even when rejecting output. Nonempty deltas must not be dropped.
    pending_textures.append(std::mem::take(&mut output.textures_delta));
    if output
        .viewport_output
        .keys()
        .any(|id| *id != ViewportId::ROOT)
    {
        return Err(NativeError::AdditionalViewport);
    }
    Ok(output)
}

// The renderer releases its surface before the retained OS window is dropped.
struct NativeWindow {
    renderer: WgpuRenderer,
    integration: WinitIntegration,
    window: Arc<Window>,
}

struct Runner<'app, A: ?Sized> {
    proxy: EventLoopProxy<UserEvent>,
    context: Context,
    options: NativeOptions,
    app: &'app mut A,
    window: Option<NativeWindow>,
    repaint: Repaint,
    failure: Option<NativeError>,
}

impl<A: NativeApp + ?Sized> Runner<'_, A> {
    fn open(&mut self, event_loop: &ActiveEventLoop) -> Result<(), NativeError> {
        if self.window.is_some() {
            self.repaint.resume();
            self.repaint.request(Instant::now());
            return Ok(());
        }
        // Accessibility must attach before the window has ever been shown.
        let window =
            Arc::new(event_loop.create_window(self.options.window.clone().with_visible(false))?);
        let renderer = pollster::block_on(WgpuRenderer::new(
            event_loop,
            Arc::clone(&window),
            self.options.clear_color,
        ))?;
        let mut integration = WinitIntegration::new(self.context.clone(), &window, event_loop);
        #[cfg(feature = "accesskit")]
        integration.init_accesskit(event_loop, &window, self.proxy.clone());
        integration
            .set_max_texture_side(renderer.device.limits().max_texture_dimension_2d as usize);
        let proxy = self.proxy.clone();
        self.context.set_request_repaint_callback(move |request| {
            // Capture the deadline when requested, so queueing cannot restart its delay.
            if let Some(request) = RepaintRequest::new(request, Instant::now()) {
                let _ = proxy.send_event(UserEvent::Repaint(request));
            }
        });
        window.set_visible(self.options.window.visible);
        if self.options.window.visible && self.options.window.active {
            window.focus_window();
        }
        self.window = Some(NativeWindow {
            renderer,
            integration,
            window,
        });
        self.repaint.request(Instant::now());
        Ok(())
    }

    fn render(&mut self, event_loop: &ActiveEventLoop) -> Result<FrameControl, NativeError> {
        let Some(state) = &mut self.window else {
            return Ok(FrameControl::Continue);
        };
        self.repaint.frame_started(Instant::now());
        let mut input = state.integration.take_input(&state.window);
        self.app
            .prepare_input(&mut input)
            .map_err(NativeError::Application)?;
        let started = Instant::now();
        let output = self
            .app
            .update(&self.context, input)
            .map_err(NativeError::Application)?;
        let update_duration = started.elapsed();
        let (output, receipt) = output.into_parts();
        let FullOutput {
            platform_output,
            textures_delta: _,
            shapes,
            pixels_per_point,
            mut viewport_output,
        } = stage_output(output, &mut state.renderer.pending_textures)?;
        // Preserve the platform delivery while retaining read-only diagnostics for the app.
        let accesskit_update = platform_output.accesskit_update.clone();
        let commands = viewport_output
            .remove(&ViewportId::ROOT)
            .map(|output| output.commands);
        let response = state.integration.handle_output(
            &state.window,
            event_loop,
            platform_output,
            commands.into_iter().flatten(),
        );
        if response
            .actions
            .iter()
            .any(|action| matches!(action, ActionRequested::Screenshot(_)))
        {
            return Err(NativeError::ScreenshotUnsupported);
        }
        // Every redraw runs UI and attempts painting, satisfying PaintWhileHidden as well.
        let tessellation_started = Instant::now();
        let primitives = self.context.tessellate(shapes, pixels_per_point);
        let tessellation_duration = tessellation_started.elapsed();
        let paint_started = Instant::now();
        let presentation = state
            .renderer
            .paint(&state.window, pixels_per_point, &primitives);
        let paint_duration = paint_started.elapsed();
        let now = Instant::now();
        if let Some(reason) = state.renderer.drop_reason {
            self.repaint.defer_until(now + reason.retry_delay());
            if reason.needs_retry() {
                self.repaint.request(now);
            }
        } else {
            self.repaint.resume();
        }
        if state.renderer.needs_another_frame() {
            self.repaint.request(now);
        }
        receipt.report(presentation);
        let frame_duration = started.elapsed();
        let control = self
            .app
            .frame_finished(NativeFrameReport {
                context: &self.context,
                presentation,
                drop_reason: state
                    .renderer
                    .drop_reason
                    .map(renderer::DropReason::description),
                accesskit_update: accesskit_update.as_ref(),
                primitives: &primitives,
                update_duration,
                tessellation_duration,
                paint_duration,
                frame_duration,
            })
            .map_err(NativeError::Application)?;
        if response.close_requested {
            Ok(FrameControl::Exit)
        } else {
            Ok(control)
        }
    }

    fn fail(&mut self, event_loop: &ActiveEventLoop, error: NativeError) {
        self.failure = Some(error);
        event_loop.exit();
    }

    fn redraw(&mut self, event_loop: &ActiveEventLoop) {
        match self.render(event_loop) {
            Ok(FrameControl::Exit) => event_loop.exit(),
            Ok(FrameControl::Repaint) => self.repaint.request(Instant::now()),
            Ok(FrameControl::Continue) => {}
            Err(error) => self.fail(event_loop, error),
        }
        if self
            .window
            .as_ref()
            .is_some_and(|state| needs_direct_update(&state.window))
        {
            // An application can still close or show a hidden window. Throttle
            // only requested work; an idle window needs no background polling.
            self.repaint
                .defer_until(Instant::now() + HIDDEN_REPAINT_INTERVAL);
        }
    }

    fn schedule(&mut self, event_loop: &ActiveEventLoop) {
        if event_loop.exiting() {
            return;
        }
        if self.repaint.take_due(Instant::now()) {
            if let Some(state) = &self.window {
                if needs_direct_update(&state.window) {
                    self.redraw(event_loop);
                } else {
                    state.window.request_redraw();
                }
            }
        }
        event_loop.set_control_flow(
            self.repaint
                .deadline()
                .map_or(ControlFlow::Wait, ControlFlow::WaitUntil),
        );
    }
}

impl<A: NativeApp + ?Sized> ApplicationHandler<UserEvent> for Runner<'_, A> {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if let Err(error) = self.open(event_loop) {
            self.fail(event_loop, error);
        }
        self.schedule(event_loop);
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
        let Some(state) = &mut self.window else {
            return;
        };
        if state.window.id() != id {
            return;
        }
        if matches!(event, WindowEvent::Destroyed) {
            event_loop.exit();
            return;
        }
        if matches!(event, WindowEvent::RedrawRequested) {
            self.redraw(event_loop);
            self.schedule(event_loop);
            return;
        }
        let response = state.integration.on_window_event(&state.window, &event);
        if response.repaint
            || matches!(
                event,
                WindowEvent::Resized(_) | WindowEvent::Occluded(false)
            )
        {
            self.repaint.resume();
            self.repaint.request(Instant::now());
        }
        self.schedule(event_loop);
    }

    fn user_event(&mut self, event_loop: &ActiveEventLoop, event: UserEvent) {
        match event {
            UserEvent::Repaint(request) => {
                if request.is_current(&self.context) {
                    self.repaint.request(request.deadline);
                }
            }
            #[cfg(feature = "accesskit")]
            UserEvent::AccessKit(event) => {
                if let Some(state) = &mut self.window {
                    if state
                        .integration
                        .on_accesskit_event(&state.window, &event)
                        .repaint
                    {
                        self.repaint.resume();
                        self.repaint.request(Instant::now());
                    }
                }
            }
        }
        self.schedule(event_loop);
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        self.schedule(event_loop);
    }

    fn exiting(&mut self, _: &ActiveEventLoop) {
        self.window = None;
    }
}

#[cfg(test)]
mod tests {
    use super::{NativeError, stage_output};

    #[test]
    fn unsupported_viewport_retains_font_textures_without_panicking() {
        let context = egui::Context::default();
        context.set_embed_viewports(false);
        let output = context.run_ui(egui::RawInput::default(), |ui| {
            ui.label("This first output contains a font texture.");
            ui.ctx().show_viewport_deferred(
                egui::ViewportId::from_hash_of("unsupported-window"),
                egui::ViewportBuilder::default(),
                |_, _| {},
            );
        });
        assert!(!output.textures_delta.is_empty());
        let mut pending = egui::TexturesDelta::default();
        let result = stage_output(output, &mut pending);
        let retained = !pending.is_empty();
        pending.clear();
        assert!(matches!(result, Err(NativeError::AdditionalViewport)));
        assert!(
            retained,
            "shutdown must own and retire the pending textures"
        );
    }
}
