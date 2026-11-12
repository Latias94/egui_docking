//! A caller-owned winit loop with immediate software output and exact settlement.

use std::{num::NonZeroU32, rc::Rc, time::Instant};

use backend::{
    application::ApplicationHandler,
    dpi::LogicalSize,
    event::{ElementState, WindowEvent},
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop, OwnedDisplayHandle},
    keyboard::{Key, NamedKey},
    window::{Window, WindowId},
};
use dockspace::{
    geometry::{LogicalRect, PhysicalPoint, ScaleFactor},
    runtime::{DockspaceSession, HostFrameReport, SurfacePresentationResult},
};
use dockspace_winit::{MouseInput, SurfaceCoordinates, backend};

use super::{AppResult, editor, paint, smoke};

struct WindowState {
    // The surface owns the window handle and is dropped with its renderer resources.
    surface: softbuffer::Surface<OwnedDisplayHandle, Rc<Window>>,
    mouse: MouseInput,
    zoom: f64,
    input_scale: Option<f64>,
}

impl WindowState {
    fn scale(&self) -> f64 {
        self.surface.window().scale_factor() * self.zoom
    }

    fn coordinates(&self) -> AppResult<Option<SurfaceCoordinates>> {
        self.input_scale
            .map(|scale| {
                Ok(SurfaceCoordinates::new(
                    PhysicalPoint::new(0.0, 0.0)?,
                    ScaleFactor::new(scale)?,
                ))
            })
            .transpose()
    }

    fn request_repaint(&self, report: &HostFrameReport) {
        if report.repaint_surfaces().contains(&editor::SURFACE) {
            self.surface.window().request_redraw();
        }
    }
}

struct App {
    state: Option<WindowState>,
    session: DockspaceSession,
    smoke: Option<smoke::Smoke>,
    error: Option<Box<dyn std::error::Error>>,
}

impl App {
    fn create_window(&mut self, event_loop: &ActiveEventLoop) -> AppResult<()> {
        if self.state.is_some() {
            return Ok(());
        }
        let window = Rc::new(
            event_loop.create_window(
                Window::default_attributes()
                    .with_title("Dockspace | Software host | Shift: float | +/-: zoom | R: reset")
                    .with_inner_size(LogicalSize::new(960.0, 640.0))
                    .with_min_inner_size(LogicalSize::new(480.0, 360.0)),
            )?,
        );
        let context = softbuffer::Context::new(event_loop.owned_display_handle())?;
        let surface = softbuffer::Surface::new(&context, Rc::clone(&window))?;
        self.state = Some(WindowState {
            surface,
            mouse: MouseInput::default(),
            zoom: 1.0,
            input_scale: None,
        });
        window.request_redraw();
        Ok(())
    }

    fn event(&mut self, event_loop: &ActiveEventLoop, event: &WindowEvent) -> AppResult<()> {
        let Some(state) = self.state.as_mut() else {
            return Ok(());
        };
        if matches!(
            event,
            WindowEvent::Resized(_) | WindowEvent::ScaleFactorChanged { .. }
        ) {
            state.input_scale = None;
        }
        // The finite exercise owns its synthetic cursor. Mixing OS mouse events
        // into that same identity would invalidate the controlled input sequence.
        if self.smoke.is_none() {
            let coordinates = state.coordinates()?;
            if let Some(report) =
                editor::window_input(&mut self.session, &mut state.mouse, coordinates, event)?
            {
                state.request_repaint(&report);
            }
        }
        match event {
            WindowEvent::CloseRequested | WindowEvent::Destroyed => event_loop.exit(),
            WindowEvent::RedrawRequested => self.draw(event_loop)?,
            WindowEvent::Resized(_) | WindowEvent::ScaleFactorChanged { .. } => {
                state.surface.window().request_redraw();
            }
            WindowEvent::KeyboardInput { event, .. }
                if self.smoke.is_none()
                    && event.state == ElementState::Pressed
                    && !event.repeat =>
            {
                self.key(&event.logical_key)?;
            }
            _ => {}
        }
        Ok(())
    }

    fn key(&mut self, key: &Key) -> AppResult<()> {
        let Some(state) = self.state.as_mut() else {
            return Ok(());
        };
        match key {
            Key::Named(NamedKey::Escape) => {
                let report = editor::cancel(&mut self.session)?;
                state.request_repaint(&report);
            }
            Key::Character(text) if text.eq_ignore_ascii_case("r") => {
                let reset = self.session.prepare_layout_reset(editor::layout()?)?;
                let report = self.session.begin_layout_reset_frame(&reset)?.commit()?;
                state.request_repaint(&report);
            }
            Key::Character(text) if matches!(text.as_str(), "+" | "=" | "-") => {
                state.zoom =
                    (state.zoom + if text == "-" { -0.125 } else { 0.125 }).clamp(0.75, 1.75);
                state.input_scale = None;
                // App zoom changes the coordinate domain even when DPI is stable.
                if let Some(sample) = state.mouse.invalidate_position() {
                    editor::input(
                        &mut self.session,
                        sample.event,
                        None,
                        !sample.modifiers.shift_key(),
                    )?;
                }
                state.surface.window().request_redraw();
            }
            _ => {}
        }
        Ok(())
    }

    fn draw(&mut self, event_loop: &ActiveEventLoop) -> AppResult<()> {
        let Some(state) = self.state.as_mut() else {
            return Ok(());
        };
        let size = state.surface.window().inner_size();
        let (Some(width), Some(height)) =
            (NonZeroU32::new(size.width), NonZeroU32::new(size.height))
        else {
            return Ok(());
        };
        let scale = state.scale();
        let bounds = LogicalRect::new(
            0.0,
            0.0,
            f64::from(size.width) / scale,
            f64::from(size.height) / scale,
        )?;
        let window = Rc::clone(state.surface.window());
        state.surface.resize(width, height)?;
        let mut buffer = state.surface.buffer_mut()?;
        let mut frame = self.session.begin_host_frame()?;
        frame.begin_encoding_pass()?;
        editor::measure(&mut frame, bounds)?;
        let Some(plan) = frame.paint_plan(editor::SURFACE)? else {
            let report = frame.commit()?;
            drop(buffer);
            state.request_repaint(&report);
            return Ok(());
        };
        let snapshot = self
            .smoke
            .as_ref()
            .map(|_| smoke::inspect(plan))
            .transpose()?;
        let receipt = paint::render(plan, &mut buffer, [size.width, size.height], scale)?;
        frame.confirm_surface_painted(receipt)?;
        let mut report = frame.commit()?;
        // Encoding a buffer is not presentation. Settle only the exact output
        // accepted by softbuffer, and retire it explicitly on present failure.
        window.pre_present_notify();
        let presented = buffer.present();
        let result = if presented.is_ok() {
            SurfacePresentationResult::Presented
        } else {
            SurfacePresentationResult::Dropped
        };
        for output in report.take_painted_outputs() {
            self.session.report_surface_presentation(output, result)?;
        }
        let settled = self.session.begin_host_frame()?.commit()?;
        state.request_repaint(&report);
        state.request_repaint(&settled);
        presented?;
        if state.input_scale.is_none()
            && self.smoke.is_none()
            && let Some(sample) = state.mouse.invalidate_position()
        {
            // Motion delivered while the mapping was unavailable cannot become
            // a known position merely because a new output has now presented.
            let report = editor::input(
                &mut self.session,
                sample.event,
                None,
                !sample.modifiers.shift_key(),
            )?;
            state.request_repaint(&report);
        }
        state.input_scale = Some(scale);

        if let (Some(smoke), Some(snapshot)) = (self.smoke.as_mut(), snapshot) {
            let coordinates = state.coordinates()?.ok_or("presented mapping is missing")?;
            if smoke.step(
                &mut self.session,
                &mut state.mouse,
                coordinates,
                scale,
                &snapshot,
            )? {
                println!(
                    "software smoke passed: synthetic winit events and real software presentation"
                );
                self.smoke = None;
                event_loop.exit();
            } else {
                state.surface.window().request_redraw();
            }
        }
        Ok(())
    }

    fn finish_event(&mut self, event_loop: &ActiveEventLoop, result: AppResult<()>) {
        if let Err(error) = result {
            self.error = Some(error);
            event_loop.exit();
        }
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        let result = self.create_window(event_loop);
        self.finish_event(event_loop, result);
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
        if !self
            .state
            .as_ref()
            .is_some_and(|state| state.surface.window().id() == id)
        {
            return;
        }
        let result = self.event(event_loop, &event);
        self.finish_event(event_loop, result);
    }

    fn suspended(&mut self, event_loop: &ActiveEventLoop) {
        let result = editor::input(
            &mut self.session,
            dockspace::runtime::SurfacePointerEvent::StreamCancelled(
                dockspace::runtime::SurfacePointerCancelReason::ExplicitPlatformCancellation,
            ),
            None,
            true,
        )
        .map(|_| ());
        self.state = None;
        self.finish_event(event_loop, result);
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        if let Some(smoke) = self.smoke.as_ref() {
            if Instant::now() >= smoke.deadline() {
                self.finish_event(
                    event_loop,
                    Err("software smoke timed out before completion".into()),
                );
            } else {
                event_loop.set_control_flow(ControlFlow::WaitUntil(smoke.deadline()));
            }
        }
    }

    fn exiting(&mut self, _: &ActiveEventLoop) {
        self.state = None;
    }
}

pub(super) fn run() -> AppResult<()> {
    let mut smoke = false;
    for argument in std::env::args().skip(1) {
        match argument.as_str() {
            "--smoke" => smoke = true,
            "--help" | "-h" => {
                println!(
                    "Software docking host. Drag tabs and dividers.\nShift: float a tab | +/-: UI zoom | R: reset | Escape: cancel\n--smoke: finite synthetic-input exercise with real software output"
                );
                return Ok(());
            }
            _ => return Err(format!("unknown argument: {argument}").into()),
        }
    }
    let mut app = App {
        state: None,
        session: editor::session()?,
        smoke: smoke.then(smoke::Smoke::new),
        error: None,
    };
    EventLoop::new()?.run_app(&mut app)?;
    if let Some(error) = app.error {
        return Err(error);
    }
    if smoke && app.smoke.is_some() {
        return Err("software smoke closed before completion".into());
    }
    Ok(())
}
