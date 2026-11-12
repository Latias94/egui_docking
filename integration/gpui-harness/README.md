# GPUI workbench

A standalone Open GPUI consumer of `dockspace`. It opens a real OS window and
hosts stable GPUI views in the rectangles produced by the public docking runtime.
It does not depend on `open-gpui`'s existing `gpui_docking` implementation.

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

The standalone workspace includes its own `Cargo.lock`. If you already build
Open GPUI locally, reuse its target directory with
`CARGO_TARGET_DIR=../open-gpui/target`.

Drag a tab to reorder it, dock it in another pane, or float it inside the window.
Drag a divider to resize panes. The Tools window also has a draggable title and
resize edges. Hold Shift to suspend docking targets while retaining the drag;
press Escape to cancel. Each pane contains a normal GPUI button with its own
counter, so application-widget input can be checked alongside docking input.

## Real-window smoke check

```sh
cargo run --manifest-path integration/gpui-harness/Cargo.toml \
  --bin gpui-workbench --locked -- --smoke
```

This opens the ordinary workbench on the real platform and exits successfully
only after the window is visible, the Editor, Inspector, and Tools GPUI views
have been composed, and the corresponding exact submission reaches Submitted
and its artifact is accepted by the core. It prints `SMOKE OK` on success.
Accepted and Deferred alone do not complete the check. Opening failure, closing
or quitting early, an adapter error, and a 12-second presentation timeout all
produce a nonzero exit status.

The smoke check requires a working native display and renderer. It does not
inject input, verify physical display scanout, or qualify native multiview.
Use the retained tests below for deterministic input coverage.

## Integration boundaries

- `src/element.rs` measures title text, executes the complete ordered surface
  program, positions GPUI views at core-owned rectangles, and paints chrome.
- `src/input.rs` saves event-time receiver, clearance, and capture observations.
  An application widget in front of docking chrome blocks docking delivery.
  A later layout never replaces the observations saved for an earlier event.
- `src/presentation.rs` holds the suspended public update until GPUI accepts its
  frame. It reports an output as presented only after the matching exact-frame
  submission callback succeeds. Rejected candidates can retry their original
  input prefix; failed or abandoned outputs are retired as dropped.

The presentation boundary is GPUI's renderer submission to the platform's
presentation mechanism. It does not claim to observe physical display scanout.
GPUI frame acceptance alone does not grant the core input authority. Input
also checks that the framework packet matches the exact artifact acknowledged
by the core.

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
cargo nextest run --manifest-path integration/gpui-harness/Cargo.toml --lib --locked
```

The retained tests run the actual workbench Element and GPUI event dispatch.
They click application buttons, switch tabs while retaining entity state, drag a
tab into another pane, resize a split, and move a contained floating root. A
deferred renderer case checks that an accepted replacement packet cannot use the
previous packet's presentation authority to start a gesture.
An outer prepaint transaction rollback verifies that publication loss releases a
newer suspended update. A rejected candidate followed by platform capture loss
verifies that cancellation follows the retained input prefix during retry.

These tests use GPUI's CPU test platform and its explicit Submitted/Deferred
renderer outcomes. They verify adapter contracts and interaction behavior,
including exact frame correlation. They do not exercise Metal, a window server,
desktop hit testing, or physical display presentation. The normal workbench
command above remains the real OS-window demonstration.
