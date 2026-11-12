//! Renderer-independent egui input and platform output for one winit window.
//!
//! Feed window events into [`WinitIntegration::on_window_event`], take one input
//! before each egui run, then handle that run's platform output and root viewport
//! commands. The application owns rendering and reports the exact disposition of
//! its [`crate::DockspaceOutputReceipt`] after rendering finishes.

use ::winit::{event::WindowEvent, event_loop::ActiveEventLoop, window::Window};
use egui::{Context, PlatformOutput, RawInput, ViewportCommand, ViewportId, ViewportInfo};

pub use dockspace_winit::backend;
#[cfg(feature = "accesskit")]
pub use egui_winit::accesskit_winit::{
    Event as AccessKitEvent, WindowEvent as AccessKitWindowEvent,
};
pub use egui_winit::{ActionRequested, EventResponse};

/// Platform decisions and renderer work left to the application after one output.
#[derive(Debug)]
#[must_use = "honor the close decision and handle the remaining renderer actions"]
pub struct OutputResponse {
    /// Whether the application may close its root window after this update.
    ///
    /// The request has already appeared in root viewport input, and this output
    /// did not cancel it. This decision is returned once for that request.
    pub close_requested: bool,
    /// Screenshot and hidden-paint requests which require the renderer owner.
    ///
    /// Cut, copy, and paste requests are translated into the next egui input.
    pub actions: Vec<ActionRequested>,
}

/// A single-root-window egui bridge which owns neither a renderer nor an event loop.
///
/// A native close request is delivered once through
/// [`egui::ViewportInfo::close_requested`]. After that input's egui run,
/// [`Self::handle_output`] approves closing unless its commands contain
/// [`ViewportCommand::CancelClose`].
///
/// A programmatic [`ViewportCommand::Close`] starts the same input-and-response
/// cycle on the next run. `CancelClose` in the same output vetoes that request
/// regardless of command order. Approving an already delivered request does not
/// enqueue another one, even if that output also contains `Close`.
pub struct WinitIntegration {
    context: Context,
    state: egui_winit::State,
    #[cfg(feature = "accesskit")]
    window_id: ::winit::window::WindowId,
    viewport_info: ViewportInfo,
    close: CloseState,
}

impl WinitIntegration {
    /// Binds egui input and platform output to an existing root window.
    ///
    /// The caller retains ownership of the window, event loop, and egui context.
    /// Configure the renderer's texture limit with [`Self::set_max_texture_side`]
    /// before the first egui run.
    #[must_use]
    pub fn new(context: Context, window: &Window, event_loop: &ActiveEventLoop) -> Self {
        let state = egui_winit::State::new(
            context.clone(),
            ViewportId::ROOT,
            event_loop,
            Some(window.scale_factor() as f32),
            event_loop.system_theme(),
            None,
        );
        let mut viewport_info = ViewportInfo::default();
        egui_winit::update_viewport_info(&mut viewport_info, &context, window, true);
        Self {
            context,
            state,
            #[cfg(feature = "accesskit")]
            window_id: window.id(),
            viewport_info,
            close: CloseState::default(),
        }
    }

    /// Returns the context shared with the application's egui run.
    #[must_use]
    pub fn context(&self) -> &Context {
        &self.context
    }

    /// Installs the native accessibility adapter for this integration's window.
    ///
    /// Call once with the same window passed to [`Self::new`], and route the
    /// proxy's accessibility events to [`Self::on_accesskit_event`]. Tree
    /// generation activates when the platform requests its initial tree; no
    /// manual call to [`Context::enable_accesskit`] is needed.
    /// Create the window with `WindowAttributes::with_visible(false)`, initialize
    /// accessibility, then show it. Hiding a previously shown window is too late.
    ///
    /// # Panics
    ///
    /// The platform adapter panics if the window is already visible.
    #[cfg(feature = "accesskit")]
    pub fn init_accesskit<T: From<AccessKitEvent> + Send>(
        &mut self,
        event_loop: &ActiveEventLoop,
        window: &Window,
        event_loop_proxy: ::winit::event_loop::EventLoopProxy<T>,
    ) {
        self.state
            .init_accesskit(event_loop, window, event_loop_proxy);
    }

    /// Handles a native accessibility event addressed to this root window.
    ///
    /// Events for other windows return an unconsumed response without changing
    /// egui state. A matching initial-tree request enables tree generation;
    /// honor its repaint response promptly so the platform receives the tree.
    /// Matching actions enter the next [`Self::take_input`] and also request a
    /// repaint. Deactivation disables tree generation without requesting one.
    #[cfg(feature = "accesskit")]
    pub fn on_accesskit_event(&mut self, window: &Window, event: &AccessKitEvent) -> EventResponse {
        if window.id() != self.window_id || event.window_id != self.window_id {
            return EventResponse::default();
        }
        let repaint = match &event.window_event {
            AccessKitWindowEvent::InitialTreeRequested => {
                self.context.enable_accesskit();
                true
            }
            AccessKitWindowEvent::ActionRequested(request) => {
                self.state.on_accesskit_action_request(request.clone());
                true
            }
            AccessKitWindowEvent::AccessibilityDeactivated => {
                self.context.disable_accesskit();
                false
            }
        };
        EventResponse {
            consumed: true,
            repaint,
        }
    }

    /// Accumulates one event for this integration's window.
    ///
    /// Honor the returned repaint request. `CloseRequested` is input for the
    /// application to approve; it must not immediately exit the event loop.
    /// `Destroyed` remains a terminal window-lifecycle event owned by the caller.
    pub fn on_window_event(&mut self, window: &Window, event: &WindowEvent) -> EventResponse {
        match event {
            WindowEvent::CloseRequested => self.close.request(),
            WindowEvent::Occluded(occluded) => self.viewport_info.occluded = Some(*occluded),
            _ => {}
        }
        self.state.on_window_event(window, event)
    }

    /// Takes accumulated input and fresh root-window geometry for one egui run.
    ///
    /// Call [`Self::handle_output`] for that run before taking its successor's
    /// input. Transient viewport events are consumed once, while geometry and
    /// visibility observations remain available to later runs.
    #[must_use]
    pub fn take_input(&mut self, window: &Window) -> RawInput {
        egui_winit::update_viewport_info(&mut self.viewport_info, &self.context, window, false);
        let mut input = self.state.take_egui_input(window);
        let mut viewport = self.viewport_info.take();
        if let Some(state_viewport) = input.viewports.get_mut(&ViewportId::ROOT) {
            viewport.events.append(&mut state_viewport.events);
        }
        if self.close.take_request() && !viewport.close_requested() {
            viewport.events.push(egui::ViewportEvent::Close);
        }
        input.focused = viewport.focused.unwrap_or(input.focused);
        input.viewports.insert(ViewportId::ROOT, viewport);
        input
    }

    /// Sets the maximum supported side length of an egui texture in physical pixels.
    pub fn set_max_texture_side(&mut self, max_texture_side: usize) {
        self.state.set_max_texture_side(max_texture_side);
    }

    /// Applies one run's platform output and root-window viewport commands.
    ///
    /// Pass only commands for [`ViewportId::ROOT`]. Clipboard, cursor, IME and URL
    /// output go through egui-winit. Accessibility delivery requires the
    /// `accesskit` feature and an initialized adapter. The active event loop lets
    /// egui-winit register custom cursor images as well as standard cursor icons.
    /// Cut, copy, and paste commands enqueue ordinary egui input and request
    /// another redraw; screenshot and hidden-paint actions are returned to the
    /// renderer owner.
    ///
    /// Closing is approved only after a request was delivered by
    /// [`Self::take_input`]. A new programmatic `Close` requests a redraw for its
    /// next input delivery; any `CancelClose` in this output vetoes closing.
    pub fn handle_output(
        &mut self,
        window: &Window,
        event_loop: &ActiveEventLoop,
        platform_output: PlatformOutput,
        root_commands: impl IntoIterator<Item = ViewportCommand>,
    ) -> OutputResponse {
        let mut requested = false;
        let mut cancelled = false;
        let commands = root_commands.into_iter().filter(|command| match command {
            ViewportCommand::Close => {
                requested = true;
                false
            }
            ViewportCommand::CancelClose => {
                cancelled = true;
                false
            }
            _ => true,
        });
        let mut requested_actions = Vec::new();
        egui_winit::process_viewport_commands(
            &self.context,
            &mut self.viewport_info,
            commands,
            window,
            &mut requested_actions,
        );
        let close_requested = self.close.finish_output(requested, cancelled);
        self.state
            .handle_platform_output_with_event_loop(window, event_loop, platform_output);

        let mut queued_input = false;
        let mut actions = Vec::new();
        for action in requested_actions {
            match action {
                ActionRequested::Cut => {
                    self.state.egui_input_mut().events.push(egui::Event::Cut);
                    queued_input = true;
                }
                ActionRequested::Copy => {
                    self.state.egui_input_mut().events.push(egui::Event::Copy);
                    queued_input = true;
                }
                ActionRequested::Paste => {
                    if let Some(text) = self.state.clipboard_text() {
                        let text = text.replace("\r\n", "\n");
                        if !text.is_empty() {
                            self.state
                                .egui_input_mut()
                                .events
                                .push(egui::Event::Paste(text));
                            queued_input = true;
                        }
                    } else if let Some(image) = self.state.clipboard_image() {
                        self.state
                            .egui_input_mut()
                            .events
                            .push(egui::Event::PasteImage(std::sync::Arc::new(image)));
                        queued_input = true;
                    }
                }
                action @ (ActionRequested::Screenshot(_) | ActionRequested::PaintWhileHidden) => {
                    actions.push(action);
                }
            }
        }
        if queued_input || self.close.is_pending() {
            window.request_redraw();
        }
        OutputResponse {
            close_requested,
            actions,
        }
    }
}

#[derive(Default)]
enum CloseState {
    #[default]
    Idle,
    Pending,
    Delivered,
}

impl CloseState {
    fn request(&mut self) {
        if matches!(self, Self::Idle) {
            *self = Self::Pending;
        }
    }

    fn take_request(&mut self) -> bool {
        if matches!(self, Self::Pending) {
            *self = Self::Delivered;
            true
        } else {
            false
        }
    }

    fn finish_output(&mut self, requested: bool, cancelled: bool) -> bool {
        if cancelled {
            *self = Self::Idle;
            false
        } else if matches!(self, Self::Delivered) {
            *self = Self::Idle;
            true
        } else {
            if requested {
                self.request();
            }
            false
        }
    }

    fn is_pending(&self) -> bool {
        matches!(self, Self::Pending)
    }
}

#[cfg(test)]
mod tests {
    use super::CloseState;

    #[test]
    fn native_close_waits_for_input_and_approves_exactly_once() {
        let mut close = CloseState::default();
        close.request();
        close.request();
        assert!(!close.finish_output(false, false));
        assert!(close.take_request());
        assert!(!close.take_request());
        assert!(close.finish_output(false, false));
        assert!(!close.finish_output(false, false));
        assert!(!close.take_request());
    }

    #[test]
    fn cancelled_native_close_does_not_reappear_and_allows_a_later_request() {
        let mut close = CloseState::default();
        close.request();
        assert!(close.take_request());
        assert!(!close.finish_output(false, true));
        assert!(!close.take_request());
        assert!(!close.finish_output(false, false));

        close.request();
        assert!(close.take_request());
        assert!(close.finish_output(false, false));
    }

    #[test]
    fn programmatic_close_is_delivered_next_run_without_requeueing_approval() {
        let mut close = CloseState::default();
        assert!(!close.finish_output(true, false));
        assert!(close.is_pending());
        assert!(close.take_request());
        assert!(close.finish_output(true, false));
        assert!(!close.is_pending());
        assert!(!close.take_request());
        assert!(!close.finish_output(false, false));
    }

    #[test]
    fn cancellation_vetoes_both_new_and_delivered_programmatic_close() {
        let mut close = CloseState::default();
        assert!(!close.finish_output(true, true));
        assert!(!close.take_request());

        assert!(!close.finish_output(true, false));
        assert!(close.take_request());
        assert!(!close.finish_output(true, true));
        assert!(!close.take_request());
        assert!(!close.finish_output(false, false));
    }
}

#[cfg(feature = "wgpu")]
#[path = "winit/native.rs"]
mod native;
#[cfg(feature = "wgpu")]
pub use native::{
    AppResult, FrameControl, NativeApp, NativeError, NativeFrameReport, NativeOptions, run_native,
};
