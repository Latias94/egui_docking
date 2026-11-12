# dockspace_winit

Small helpers for a winit-based docking host. This crate depends on `dockspace`
and `winit`; it has no UI framework or GPU dependency and owns no event loop.
GPUI hosts use their own platform integration and do not use this crate.

- `SurfaceCoordinates` maps window-client physical pixels into surface logical
  points using an explicit origin and pixels-per-point scale. Include UI zoom
  in that scale when applicable; a window's DPI alone may not be sufficient.
- `MouseInput` samples one window's mouse events and retains cursor/modifier
  state. Cursor departure forgets position without cancelling a drag. Focus
  loss actively cancels the host stream; window destruction retires its binding.
- `RepaintScheduler` retains the earliest absolute deadline and consumes due
  requests once. Capture the deadline before posting a repaint through a proxy.
- `backend` re-exports the matching winit version.

The host supplies pointer identities, receiver delivery and capture facts, and
reports presentation only after its renderer presents the exact output. Mouse
samples supply none of those proofs and have no original OS timestamp. Touch,
scroll, keyboard shortcuts, native window docking and rendering are outside
this crate's current scope. Keep framework-specific input and accessibility
integration in your UI adapter, and submit each physical input only once.

## Add from source

Until the first release, use path dependencies from this checkout:

```toml
[dependencies]
dockspace = { path = "../dockspace/crates/dockspace" }
dockspace_winit = { path = "../dockspace/crates/dockspace_winit" }
```

Adding this crate opts into winit. The `dockspace` core has no platform
dependency. For egui, use the optional `winit` or `wgpu` feature on
[`egui_dockspace`](../egui_dockspace/README.md) instead of routing input twice.

## Use your event loop

This fragment connects an existing `ApplicationHandler` without introducing a
second event loop. Replace the comments with your host's input routing and
renderer. Here surface logical units follow window logical pixels; a zoomed UI
must provide its actual pixels-per-point scale instead.

```rust,no_run
use std::time::Instant;

use dockspace::geometry::{PhysicalPoint, ScaleFactor};
use dockspace_winit::{MouseInput, RepaintScheduler, SurfaceCoordinates, backend};
use backend::{
    application::ApplicationHandler,
    event::WindowEvent,
    event_loop::{ActiveEventLoop, ControlFlow},
    window::{Window, WindowId},
};

#[derive(Default)]
struct Host {
    window: Option<Window>,
    mouse: MouseInput,
    repaint: RepaintScheduler,
}

impl ApplicationHandler for Host {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_none() {
            let window = event_loop.create_window(Window::default_attributes()).unwrap();
            window.request_redraw();
            self.window = Some(window);
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
        let Some(window) = self.window.as_ref().filter(|window| window.id() == id) else {
            return;
        };
        if let Some(sample) = self.mouse.on_window_event(&event) {
            let coordinates = SurfaceCoordinates::new(
                PhysicalPoint::new(0.0, 0.0).unwrap(),
                ScaleFactor::new(window.scale_factor()).unwrap(),
            );
            let position = sample.logical_position(coordinates).unwrap();
            // Route sample.event and position with your exact presented receiver/capture facts.
            let _ = (sample.event, position, sample.modifiers);
            window.request_redraw();
        }
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::RedrawRequested => {
                self.repaint.frame_started(Instant::now());
                // Run your UI and renderer, then report its exact presentation result.
            }
            WindowEvent::Resized(_) | WindowEvent::ScaleFactorChanged { .. } => {
                window.request_redraw();
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        if self.repaint.take_due(Instant::now()) {
            if let Some(window) = &self.window {
                window.request_redraw();
            }
        }
        event_loop.set_control_flow(
            self.repaint.deadline().map_or(ControlFlow::Wait, ControlFlow::WaitUntil),
        );
    }
}
```

Forward repaint requests to `repaint.request(deadline)` on the event-loop thread.
Starting a frame clears satisfied deadlines while retaining a future request.
