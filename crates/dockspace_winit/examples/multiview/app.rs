//! The application owns scheduling, commands, the session and the renderer.

use std::{
    rc::Rc,
    time::{Duration, Instant},
};

use dockspace::{
    geometry::LogicalRect,
    runtime::{
        DockspaceSession, HostWindowToken, NativeCloseState, NativeReceiverAnswer,
        NativeWindowBinding, SurfacePresentationResult,
    },
};
use dockspace_winit::backend::{
    application::ApplicationHandler,
    dpi::LogicalSize,
    event::{ElementState, WindowEvent},
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop},
    keyboard::{Key, NamedKey},
    window::{Window, WindowId},
};

use super::{
    AppResult,
    host::{Host, WindowState},
    paint,
    smoke::{self, Command},
    workspace,
};

struct App {
    session: DockspaceSession,
    host: Host,
    smoke: Option<smoke::Smoke>,
    root_presented: bool,
    shutdown: Option<Instant>,
    completed: bool,
    error: Option<Box<dyn std::error::Error>>,
}

impl App {
    fn create_root(&mut self, event_loop: &ActiveEventLoop) -> AppResult<()> {
        if !self.host.windows.is_empty() || self.shutdown.is_some() {
            return Ok(());
        }
        let window = event_loop.create_window(
            Window::default_attributes()
                .with_title("Dockspace | N: new window | D: dock back | C: close child | Esc: quit")
                .with_inner_size(LogicalSize::new(960.0, 640.0))
                .with_min_inner_size(LogicalSize::new(480.0, 320.0)),
        )?;
        let id = window.id();
        let state = WindowState::new(event_loop, window, false);
        if let Some(error) = state.preparation_failure() {
            return Err(format!("main renderer preparation failed: {error}").into());
        }
        self.session
            .register_native_root(workspace::MAIN, HostWindowToken::new(1))?;
        let report = workspace::idle(&mut self.session)?;
        let [binding] = report.native_bindings() else {
            return Err("native root registration did not produce one binding".into());
        };
        self.host.windows.insert(*binding, id, state)?;
        self.host.accept(&mut self.session, report)?;
        self.advance(event_loop)?;
        for (_, _, state) in self.host.windows.iter() {
            state.window.request_redraw();
        }
        Ok(())
    }

    fn advance(&mut self, event_loop: &ActiveEventLoop) -> AppResult<()> {
        loop {
            self.host.dispatch(event_loop, &mut self.session)?;
            self.host.settle_observations(&mut self.session)?;
            if !self.host.has_work() {
                break;
            }
        }
        Ok(())
    }

    fn command(&mut self, event_loop: &ActiveEventLoop, command: Command) -> AppResult<()> {
        match command {
            Command::Detach if self.host.child().is_none() && self.shutdown.is_none() => {
                let placement = self.host.child_placement()?;
                let report = workspace::detach(&mut self.session, placement)?;
                self.host.accept(&mut self.session, report)?;
            }
            Command::DockBack if self.host.child().is_some() => {
                let report = workspace::dock_back(&mut self.session)?;
                self.host.accept(&mut self.session, report)?;
            }
            Command::CloseChild => {
                if let Some(child) = self.host.child() {
                    self.session
                        .publish_native_close(child, NativeCloseState::Requested, None)?;
                }
            }
            Command::Quit => {
                self.host.begin_shutdown();
                self.shutdown
                    .get_or_insert_with(|| Instant::now() + Duration::from_secs(10));
                if self.session.has_pending_native_create() {
                    let mut frame = self
                        .session
                        .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)?;
                    frame.cancel_next_native_create()?;
                    let report = frame.commit()?;
                    self.host.accept(&mut self.session, report)?;
                } else if let Some(child) = self.host.child() {
                    self.session
                        .publish_native_close(child, NativeCloseState::Requested, None)?;
                }
            }
            Command::MinimizeMain => {
                let child = self.host.child();
                for (binding, _, state) in self.host.windows.iter() {
                    if Some(binding) != child {
                        state.window.set_minimized(true);
                    }
                }
            }
            _ => {}
        }
        self.advance(event_loop)
    }

    fn draw(
        &mut self,
        event_loop: &ActiveEventLoop,
        binding: NativeWindowBinding,
    ) -> AppResult<()> {
        if let Some(state) = self.host.windows.get_mut(binding) {
            state.apply_initial_placement()?;
        }
        self.advance(event_loop)?;
        let child = self.host.child() == Some(binding);
        let Some(state) = self.host.windows.get_mut(binding) else {
            return Ok(());
        };
        let size = state.window.inner_size();
        if size.width == 0 || size.height == 0 {
            return Ok(());
        }
        let scale = state.window.scale_factor();
        let bounds = LogicalRect::new(
            0.0,
            0.0,
            f64::from(size.width) / scale,
            f64::from(size.height) / scale,
        )?;
        let surfaces: Vec<_> = self
            .session
            .native_surface_bindings_for_window(binding)
            .collect();
        let [surface] = surfaces.as_slice() else {
            return Err("this example renders one logical surface per native window".into());
        };
        let mut frame = self
            .session
            .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)?;
        frame.begin_encoding_pass()?;
        paint::measure(&mut frame, surface.surface(), bounds)?;
        let Some(plan) = frame.paint_plan(surface.surface())? else {
            let report = frame.commit()?;
            self.host.accept(&mut self.session, report)?;
            return Ok(());
        };
        let window = Rc::clone(&state.window);
        let renderer = state
            .surface
            .as_mut()
            .ok_or("paint requested before renderer preparation")?;
        let mut buffer = renderer.buffer_mut()?;
        let receipt = paint::render(plan, &mut buffer, [size.width, size.height], scale)?;
        frame.confirm_surface_painted(receipt)?;
        let mut report = frame.commit()?;
        let outputs = report.take_painted_outputs();
        // Keep the exact outputs paired with this buffer, including the pending
        // child's first-live candidate. Buffer encoding does not move content.
        window.pre_present_notify();
        let presented = buffer.present();
        drop(window);
        let result = if presented.is_ok() {
            SurfacePresentationResult::Presented
        } else {
            SurfacePresentationResult::Dropped
        };
        for output in outputs {
            self.session.report_surface_presentation(output, result)?;
        }
        self.host.accept(&mut self.session, report)?;
        presented?;
        self.root_presented |= !child;
        self.advance(event_loop)
    }

    fn event(
        &mut self,
        event_loop: &ActiveEventLoop,
        id: WindowId,
        event: WindowEvent,
    ) -> AppResult<()> {
        if matches!(event, WindowEvent::Destroyed) {
            if let Some(binding) = self.host.windows.binding(id) {
                self.host.retire(&self.session, binding, None)?;
            }
            if self.host.destroyed(id) {
                self.advance(event_loop)?;
            }
            return Ok(());
        }
        let Some(binding) = self.host.windows.binding(id) else {
            return Ok(());
        };
        match event {
            WindowEvent::RedrawRequested => self.draw(event_loop, binding)?,
            WindowEvent::CloseRequested => {
                let command = if self.host.child() == Some(binding) {
                    Command::CloseChild
                } else {
                    Command::Quit
                };
                self.command(event_loop, command)?;
            }
            WindowEvent::KeyboardInput { event, .. }
                if self.smoke.is_none()
                    && event.state == ElementState::Pressed
                    && !event.repeat =>
            {
                let command = match event.logical_key {
                    Key::Named(NamedKey::Escape) => Some(Command::Quit),
                    Key::Character(text) => match text.to_lowercase().as_str() {
                        "n" => Some(Command::Detach),
                        "d" => Some(Command::DockBack),
                        "c" => Some(Command::CloseChild),
                        _ => None,
                    },
                    _ => None,
                };
                if let Some(command) = command {
                    self.command(event_loop, command)?;
                }
            }
            WindowEvent::Resized(_)
            | WindowEvent::ScaleFactorChanged { .. }
            | WindowEvent::Moved(_)
            | WindowEvent::Occluded(_) => {
                self.advance(event_loop)?;
                if let Some(state) = self.host.windows.get(binding) {
                    state.window.request_redraw();
                }
            }
            _ => {}
        }
        Ok(())
    }

    fn wait(&mut self, event_loop: &ActiveEventLoop) -> AppResult<()> {
        if event_loop.exiting() {
            return Ok(());
        }
        if let Some(smoke) = self.smoke.as_mut()
            && let Some(command) = smoke.next(&self.session, &self.host, self.root_presented)?
        {
            self.command(event_loop, command)?;
        }
        if self.shutdown.is_some()
            && self.host.child().is_none()
            && self.host.idle()
            && !self.session.has_pending_native_create()
        {
            let root = self
                .host
                .windows
                .iter()
                .next()
                .map(|(binding, _, _)| binding);
            if let Some(binding) = root {
                self.host.retire(&self.session, binding, None)?;
            } else {
                // Commit the permanent quiescence records before the session is
                // dropped. Every real window has delivered its final OS event.
                self.advance(event_loop)?;
                if self.smoke.as_ref().is_some_and(|smoke| !smoke.finished()) {
                    return Err("multiview smoke exited before its final stage".into());
                }
                self.completed = true;
                if self.smoke.is_some() {
                    if self.host.creations != 4 || self.host.destructions != 5 {
                        return Err(
                            "multiview smoke did not observe every child and root destruction"
                                .into(),
                        );
                    }
                    println!(
                        "multiview smoke passed: failed preparation cleanup, three live children, exact presentation, Dock Back, close recovery and minimized-root shutdown"
                    );
                }
                event_loop.exit();
            }
        }
        let deadline = self
            .smoke
            .as_ref()
            .map(smoke::Smoke::deadline)
            .into_iter()
            .chain(self.shutdown)
            .min();
        if deadline.is_some_and(|deadline| Instant::now() >= deadline) {
            return Err(format!("multiview lifecycle timed out: windows={}, creations={}, destructions={}, idle={}, pending_create={}, inspector_surface={:?}", self.host.windows.len(), self.host.creations, self.host.destructions, self.host.idle(), self.session.has_pending_native_create(), self.session.view().item(workspace::INSPECTOR).map(|item| item.surface())).into());
        }
        let wake = if self.smoke.is_some() {
            // Poll only the finite OS exercise, including asynchronous minimize
            // completion. Ordinary interactive use has no polling loop.
            deadline
                .into_iter()
                .chain([Instant::now() + Duration::from_millis(50)])
                .min()
        } else {
            deadline
        };
        event_loop.set_control_flow(wake.map_or(ControlFlow::Wait, ControlFlow::WaitUntil));
        Ok(())
    }

    fn finish_event(&mut self, event_loop: &ActiveEventLoop, result: AppResult<()>) {
        if let Err(error) = result {
            if self.error.is_none() {
                self.error = Some(error);
            }
            event_loop.exit();
        }
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        let result = self.create_root(event_loop);
        self.finish_event(event_loop, result);
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
        if event_loop.exiting() {
            return;
        }
        let result = self.event(event_loop, id, event);
        self.finish_event(event_loop, result);
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        let result = self.wait(event_loop);
        self.finish_event(event_loop, result);
    }
}

pub(super) fn run() -> AppResult<()> {
    let mut smoke = false;
    for argument in std::env::args().skip(1) {
        match argument.as_str() {
            "--smoke" => smoke = true,
            "--help" | "-h" => {
                println!(
                    "Native winit windows with software output.\nN: detach Inspector | D: dock back | C: close child and recover | Escape: quit\n--smoke: finite programmatic lifecycle using real windows and presentation\nThis example does not implement physical cross-window pointer dragging."
                );
                return Ok(());
            }
            _ => return Err(format!("unknown argument: {argument}").into()),
        }
    }
    let mut builder = EventLoop::builder();
    #[cfg(all(target_os = "linux", feature = "x11"))]
    {
        // Exact global placement needs X11. Wayland applications may still use
        // the library helpers with their actual, more limited capabilities.
        use dockspace_winit::backend::platform::x11::EventLoopBuilderExtX11;
        builder.with_x11();
    }
    #[cfg(all(target_os = "linux", not(feature = "x11")))]
    return Err("this absolute-placement example requires the x11 feature on Linux".into());
    let event_loop = builder.build()?;
    let mut host = Host::new();
    host.fail_next_preparation = smoke;
    let mut app = App {
        session: workspace::session()?,
        host,
        smoke: smoke.then(smoke::Smoke::new),
        root_presented: false,
        shutdown: None,
        completed: false,
        error: None,
    };
    event_loop.run_app(&mut app)?;
    if let Some(error) = app.error {
        return Err(error);
    }
    if smoke && !app.completed {
        return Err("multiview smoke closed before its lifecycle completed".into());
    }
    Ok(())
}
