# GPUI adapter and workbench

A retained Open GPUI adapter for `dockspace`, with a runnable workbench. Applications
provide their session and GPUI views; the adapter places those stable views in the
rectangles produced by the public docking runtime. It does not depend on
`open-gpui`'s existing `gpui_docking` implementation.

## Run

Keep these repositories next to each other:

```text
rust/
  egui_docking/       # This repository (the directory name may differ).
  open-gpui/         # Open GPUI, with its public exact-frame submission API.
```

From this repository:

```sh
cargo run --manifest-path integration/gpui-harness/Cargo.toml --bin gpui-workbench --locked
```

The workbench reports backend warnings and errors to stderr. To inspect native
display-topology or window-creation failures, retain that output; the platform's
specific collection error precedes any higher-level window-open error. Set
`RUST_LOG=open_gpui_macos=debug` for additional macOS backend logging. The library
leaves logging configuration to its embedding application.

The standalone workspace includes its own `Cargo.lock`. If you already build
Open GPUI locally, reuse its target directory with
`CARGO_TARGET_DIR=../open-gpui/target`.

Drag a tab to reorder it, dock it in another pane, or float it inside the window.
Drag a divider to resize panes. The Tools window also has a draggable title and
resize edges. Hold Shift to suspend docking targets while retaining the drag;
press Escape to cancel. Each pane contains a normal GPUI button with its own
counter, so application-widget input can be checked alongside docking input.
Dragging over an inactive target tab selects it after the core's hover delay;
the adapter schedules that deadline without requiring further mouse motion.

## Embed in an application

The independent workspace also exposes the `dockspace_gpui_harness` library.
Create a `GpuiDockspace` once, then include `dock.element()` in your application's
normal `Render` implementation:

```rust
use dockspace_gpui_harness::{DockPane, GpuiDockspace, GpuiDockspaceConfig};

let dock = GpuiDockspace::new(
    session,
    surface,
    [
        DockPane::new(editor_id, "Editor", editor_view),
        DockPane::new(preview_id, preview_title, preview_view),
    ],
    GpuiDockspaceConfig::default(),
    cx,
)?;

// Inside Render, with a sized parent:
div().size_full().child(dock.element())
```

Titles accept owned `SharedString` values and views accept existing entities through
`AnyView`. Tab selection, docking, floating and temporary removal preserve those
entities. `DockPane::with_minimum_size` provides application-specific pane limits.
`GpuiDockspaceConfig` supplies the font, text size, colors and input bindings;
measurement and painting use the same configuration. Core layout metrics remain
part of the session's `DockPresentationConfig`.

This adapter currently owns one logical surface in one application-owned GPUI
window. Use a session whose native-surface and pane-close policies are disabled.
The constructor rejects unsupported sessions and duplicate or missing panes.
Cloning the handle shares its state; mount it once in its original window.
Removing its subtree cancels gestures, timers and unpublished input; reinserting it
in that window establishes a fresh presentation without recreating pane views.
Application startup, window closing and process exit remain application decisions.

## Layout motion

The default `motion` feature enables structural layout transitions. The adapter
uses the existing GPUI host clock and requests the next sample only after the
exact pending output reaches a terminal submission result. Deferred submissions
retain their original packet and coordinates. Settled layouts stop requesting
animation frames; pointer-controlled resizing and floating movement remain direct.

Disable motion at runtime through the constructor configuration:

```rust
use dockspace::motion::{MotionMode, MotionSettings};

let config = GpuiDockspaceConfig {
    motion: MotionSettings {
        mode: MotionMode::Off,
        ..Default::default()
    },
    ..Default::default()
};
```

For an existing host, use `dock.set_motion_settings(settings, window)` with its
owning window. `settings.layout` accepts an instant transition, a tween with a
duration and built-in or custom cubic Bézier curve, or a critical spring.
The adapter currently consumes the layout policy; preview and focus decorations
remain immediate. Pane views are laid out at current presentation rectangles,
so their text and controls retain their size rather than scaling as a texture.

Supply an observed accessibility preference with
`dock.set_motion_preference(MotionPreference::Reduced, window)` to settle geometry
immediately. The initial preference is `Unknown`, which permits motion by default;
the adapter does not claim to read the operating system preference automatically.
Setting `unknown_prefers_reduced_motion` opts into a conservative fallback.

To compile the adapter without motion, disable its default features. The workbench
can also run this static path:

```sh
cargo run --manifest-path integration/gpui-harness/Cargo.toml \
  --no-default-features --bin gpui-workbench --locked
```

## Real-window smoke check

```sh
cargo run --manifest-path integration/gpui-harness/Cargo.toml \
  --bin gpui-workbench --locked -- --smoke
```

To run the same interaction journey and close the window before quitting:

```sh
cargo run --manifest-path integration/gpui-harness/Cargo.toml \
  --bin gpui-workbench --locked -- --smoke-close
```

This opens the ordinary workbench on the real platform and waits for the visible
native window, the composed Editor, Inspector, and Tools GPUI views, a real exact
Submitted result, and the matching core artifact. It then dispatches framework
mouse events to:

1. Click the Editor button and verify its counter advances from zero to one.
2. Select Preview, click its button, then return to Editor and verify both
   counters and their original GPUI entities survive the tab switches.
3. Press, move, and release the splitter, waiting for a submitted artifact
   between those edges, and verify the resulting pane size.
4. Drag the Tools window with 24 consecutive motion samples without waiting for
   each preceding submission, then verify its submitted preview follows the last
   position and the gesture ends with the original view.

Button coordinates come from paint-time bounds enabled only for this check.
Tab and splitter coordinates come from the exact submitted core packet. Each
assertion waits for a newer real renderer submission; Accepted and Deferred alone
never satisfy it. The check counts motion samples sent while a submission is pending,
prints the last input-to-observed-submission interval and `SMOKE OK`
before requesting native quit. Opening
failure, closing or quitting early, an adapter error, a failed assertion, and a
20-second interaction timeout all produce a nonzero exit status. An independent
25-second process watchdog also detects a stalled native quit; success requires
the process to actually exit with status zero.
The close variant removes the application window through GPUI's normal retirement
path before its close observer requests application exit. It covers window cleanup
separately from an application-level quit request.

The smoke check requires a working native display and renderer. Its input enters
through GPUI's public `Window::dispatch_event` path rather than the operating
system's mouse-delivery queue. It verifies native backend rendering and framework
interaction, without claiming OS mouse delivery, physical display scanout, or
native multiview qualification. The tests below provide deterministic CPU
coverage of the same retained adapter.

## Integration boundaries

- `src/element.rs` measures title text, executes the complete ordered surface
  program, positions GPUI views at core-owned rectangles, and paints chrome.
- `src/input.rs` saves event-time receiver, clearance, and capture observations.
  An application widget in front of docking chrome blocks docking delivery.
  A later layout never replaces the observations saved for an earlier event.
- `src/continuation.rs` schedules the core's current stationary request once,
  then refreshes exact presentation, capture and framework clearance before
  submitting its non-motion event. Cancellation and new input revoke old wakes.
- `src/presentation.rs` holds the suspended public update until GPUI accepts its
  frame. It reports an output as presented only after the matching exact-frame
  submission callback succeeds. Rejected candidates can retry their original
  input prefix; failed or abandoned outputs are retired as dropped.

The presentation boundary is GPUI's renderer submission to the platform's
presentation mechanism. It does not claim to observe physical display scanout.
GPUI frame acceptance alone does not grant the core input authority. Input
also checks that the framework packet matches the exact artifact acknowledged
by the core. An already captured gesture can retain its own source coordinates
while a replacement awaits submission; that capability grants neither new-press
nor hover authority.

This first slice supports one OS window and contained floating roots. Pane close
requests are disabled because the example has no close-approval flow. Touch,
tab-strip scrolling, keyboard navigation, persistence UI, and native tear-off
windows are not implemented here. The adapter does not infer desktop hit stacks,
physical placement, DPI transitions, or cross-window capture from logical pane
coordinates; those native observations remain unknown.

The manifest is an independent Cargo workspace. It copies the two consumer
dependency patches required by Open GPUI without adding its platform dependency
graph to the root workspace.

## Automated interaction checks

```sh
cargo nextest run --manifest-path integration/gpui-harness/Cargo.toml --locked
```

The retained tests run the actual workbench Element and GPUI event dispatch.
They click application buttons, switch tabs while retaining entity state, drag a
tab into another pane, resize a split, and move a contained floating root. A
deferred renderer case checks that an accepted replacement packet cannot use the
previous packet's presentation authority to start a gesture.
An outer prepaint transaction rollback verifies that publication loss releases a
newer suspended update. A rejected candidate followed by platform capture loss
verifies that cancellation follows the retained input prefix during retry.
External consumer tests embed the public host with different identities, dynamic
titles and application-owned views, and verify removal and reinsertion.

These tests use GPUI's CPU test platform and its explicit Submitted/Deferred
renderer outcomes. They verify adapter contracts and interaction behavior,
including exact frame correlation. They do not exercise Metal, a window server,
desktop hit testing, or physical display presentation. The normal workbench
command above remains the real OS-window demonstration.
