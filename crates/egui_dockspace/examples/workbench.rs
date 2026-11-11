//! Complete single-window docking with application-owned panes and presentation.
//!
//! Start with workbench/editor.rs for the layout and PaneView implementation.
//! This file supplies the winit loop; workbench/renderer.rs owns WGPU output.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::sync::Arc;
use std::time::{Duration, Instant};

use egui::{Context, FullOutput, ViewportId};
use egui_dockspace::{Dockspace, SurfacePresentationResult};
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy};
use winit::window::{Window, WindowAttributes, WindowId};

#[path = "workbench/editor.rs"]
mod editor;
#[path = "workbench/exercise.rs"]
mod exercise;
#[path = "workbench/renderer.rs"]
mod renderer;

use editor::{DemoPanes, apply_editor_commands, demo_layout, run_editor};
use renderer::OfficialRenderer;

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
        let context = editor::new_context();
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
            dockspace: Dockspace::builder("workbench", demo_layout())
                .style(editor::dock_style())
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

impl ApplicationHandler<UserEvent> for OfficialDemoHost {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let attributes = WindowAttributes::default()
            .with_title("Dockspace Workbench")
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
