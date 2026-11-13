# dockspace_winit

Small helpers for a winit-based docking host. This crate depends on `dockspace`
and `winit`; it has no UI framework or GPU dependency and owns no event loop.
GPUI hosts use their own platform integration and do not use this crate.

- `SurfaceCoordinates` maps window-client physical pixels into surface logical
  points using an explicit origin and pixels-per-point scale. Include UI zoom
  in that scale when applicable; a window's DPI alone may not be sufficient.
- `MouseInput` samples one window's mouse events and retains cursor/modifier
  state. Cursor departure, window movement, resizing and DPI changes invalidate
  position with an unknown-position move, preserving an ongoing drag. Focus
  loss reports platform cancellation; destruction reports binding retirement.
- `RepaintScheduler` retains the earliest absolute deadline and consumes due
  requests once. Capture the deadline before posting a repaint through a proxy.
- `backend` re-exports the matching winit version.

The host supplies pointer identities, receiver delivery and capture facts, and
reports presentation only after its renderer presents the exact output. Mouse
samples supply none of those proofs and have no original OS timestamp. Touch,
scroll, keyboard shortcuts, native window docking and rendering are outside
this crate's current scope. Keep framework-specific input and accessibility
integration in your UI adapter, and submit each physical input only once.
The host remains responsible for retiring its pointer provider and window resources.

## Multiple windows

Compose the helpers per window: keep `MouseInput` and `RepaintScheduler` in each
host window's state, and a `SurfaceCoordinates` mapping for each logical surface
placement. Route `WindowEvent` by `WindowId`, while one `DockspaceSession` owns
the shared workspace and the host supplies one total input order across windows.
Use the earliest outstanding window deadline for the event loop's wait.

Native effect execution, core-issued window bindings, capture/hover observations
and exact output settlement remain host responsibilities. Local cursor samples
alone cannot establish cross-window routing or desktop pointer authority. One
OS window may contain several logical surfaces; keep its placement roster separate
from its OS identity and renderer resources.

The current software example demonstrates one window. For a native host, follow
the [engine host guide](../dockspace/docs/guide.md) and check the
[platform matrix](../../docs/PLATFORM_SUPPORT.md). The
[integration skill](../../skills/dockspace-integration/SKILL.md) also covers
host selection and multiview ownership.

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

## Run the software host

```sh
cargo run -p dockspace_winit --example software --features example-app --locked
```

This single-window example draws the core's ordered surface program directly
with softbuffer and a small bitmap font. It uses no UI framework. Drag tabs to
merge or split panes, move the floating Inspector and resize dividers. Hold
**Shift** to float a tab, press **+/-** for UI zoom, **R** to restore the layout,
or **Escape** to cancel a gesture. The window repaints on input and core requests;
it has no animation loop.

The [event loop](examples/software/app.rs), [core wiring](examples/software/editor.rs)
and [software drawing](examples/software/paint.rs) show separate host responsibilities.
`example-app` enables only the example's optional software/font dependencies and
is disabled by default. This is a renderer integration example; touch, scrolling,
accessibility and multiple OS windows need their own host integration.

Add `-- --smoke` for a finite exercise using synthetic winit events with real
software-buffer presentation. It checks core interaction through the public
helpers; it does not drive the physical OS mouse. A desktop session is required.

## Use your event loop

This fragment connects an existing `ApplicationHandler` without introducing a
second event loop. Replace the comments with your host's input routing and
renderer. Here surface logical units follow window logical pixels; a zoomed UI
must provide its actual pixels-per-point scale instead.

```rust,no_run
use std::time::Instant;

use dockspace::runtime::SurfacePointerPosition;
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
    presented_coordinates: Option<SurfaceCoordinates>,
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
        if matches!(&event, WindowEvent::Resized(_) | WindowEvent::ScaleFactorChanged { .. }) {
            self.presented_coordinates = None;
        }
        if let Some(sample) = self.mouse.on_window_event(&event) {
            let position = self.presented_coordinates.map_or(
                SurfacePointerPosition::Unknown,
                |coordinates| sample.logical_position(coordinates).unwrap(),
            );
            // Route sample.event and position with your exact presented receiver/capture facts.
            let _ = (sample.event, position, sample.modifiers);
            window.request_redraw();
        }
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::RedrawRequested => {
                self.repaint.frame_started(Instant::now());
                // Run your UI and renderer, then report its exact presentation result.
                // After it presents, discard cursor state sampled during any mapping gap,
                // submit that invalidation, and store the output's actual coordinates.
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

Submit every position-invalidation sample through the ordinary input lane. Its
unknown position withdraws old hover feedback while keeping the gesture; it does
not assert that the pointer is outside all desktop windows. Repeated invalidations
return `None` until another `CursorMoved` restores a known position, and button
events keep their position unknown in the meantime. Call
`mouse.invalidate_position()` when your host changes its own coordinate mapping
and can no longer qualify the retained point, then submit that returned sample too.

Route input using the mapping of the exact presented output. A new window DPI or
UI zoom is not proof that the corresponding picture has presented. If a resize
or mapping change makes the old output impossible to qualify, keep positions
unknown until the replacement presents; continue forwarding releases and
cancellations. Discard cursor state sampled during that gap before restoring
known-position routing. The software host demonstrates this presentation gate.

`ModifiersChanged` updates the snapshot used by later mouse samples but returns
`None`. Handle that event in the host's original ordered stream when modifiers
control docking: evaluate your activation policy and submit
`SurfacePointerEvent::DockingEnabledChanged` for the existing pointer gesture,
then request a redraw. This updates docking immediately without inventing mouse
motion or creating a pointer for a modifier-only event.
