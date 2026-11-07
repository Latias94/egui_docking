# Official egui compatibility harness

This package verifies that `egui_dockspace` builds and its public single-surface
facade can be consumed with the unmodified crates.io releases of `egui` and
`eframe` 0.37.0.

The manifest is an independent workspace root, and the repository root currently
uses the same official registry coordinates without a local source patch. Do not
add patches, source replacements, or compatibility feature gates here: a compile
failure caused by a fork-only API is the result this harness is intended to
expose. Keeping this package independent also prevents a future development-only
root patch from silently changing the compatibility gate.

The tests keep two official-host tiers distinct:

- A caller that owns `RawInput` uses `Dockspace::run_single_surface` and receives
  an owned complete output, a publication response and an affine renderer
  settlement receipt. Input, measurement and terminal encoding publish once.
- An unmodified `eframe::App::ui` callback keeps rendering and current-pass
  local actions, but reports `PresentationSettlementRequired` for preview and
  pane-focus finality because eframe owns the surrounding input and output.

The composed `egui_dockspace::run_ui` callback wrapper also checks final output,
but each callback's accepted update has already published. Only the owned entry
point keeps the semantic candidate private through late hooks and discard passes.
Passing the callback test is a capability-negative result, not a claim that
ordinary eframe provides full draggable docking.

The wrapper verifies the complete surface shape span after output hooks, then
returns `DockspaceOutput`. Its `into_parts()` transfers the egui output and a
one-use receipt. Headless test owners explicitly accept their frames; the real
window demo owns WGPU acquisition, submission and `queue.present`, and reports
`Presented` only after the latter returns. Failed acquisition reports `Dropped`
while retaining texture deltas for a later successful upload. Receipt drop never
grants authority. Core, official and native adapters share exact output reporting;
there is no public preview-only acknowledgement bypass.

This conservative official path admits synchronous append-only pane drawing on
the supplied Ui layer. Adapter-owned popup chrome also retains exact append-only
spans on registered foreground layers. Cross-layer pane output makes settlement unavailable. Shape
checks include ordinary contents, clips, transforms and order, but cannot inspect
application callback internals. The renderer must preserve the complete output
after taking it; the receipt does not automatically observe later edits or GPU
presentation.

Run the real-window supported tier with official crates.io egui, winit, and
wgpu:

```text
cargo run --manifest-path integration/egui-official-harness/Cargo.toml --features demo --bin official-caller-owned-demo --locked -j1
```

This host owns `RawInput`, the complete `run_ui` call, terminal `FullOutput`, and
GPU submission. Drag tab bodies to magnetic center/edge targets and drag the
dotted contained title strip to move the complete floating window. It is a
portable receipt-path demonstration, not native multiview evidence.

## Continuous editor exercise

The demo contains eight application-owned items: Editor/Preview, Outline,
Inspector, Console/History and Profiler/Assets. Its normal interactive mode
uses editable text, an actual scrolling Console and illustrative profiler
bars. The Inspector is a contained window. No game engine is required.
The application toolbar is composed inside the same atomic owned run. Close
controls allow these sample panes to close while retaining their application
data; **Restore layout** reopens all items without clearing document edits.

Run a bounded exercise through the same real WGPU host, using a new output
path:

```text
cargo run --manifest-path integration/egui-official-harness/Cargo.toml --features demo --profile production --target-dir target --locked -j1 --bin official-caller-owned-demo -- --exercise /tmp/egui-host-frames.csv
```

The optional mode supplies ordinary egui input events at coordinates observed
from the accepted editor output. It keeps the same Dockspace, content and
renderer throughout six consecutive phases: painted idle, hover, text editing,
Console scrolling, splitter dragging and a Preview-to-Outline tab drag.
Each phase has eight warmup frames followed by 64 recorded frames. Setup,
press/release and final validation frames are additional and unrecorded. The
mode exits after success, fails on a rejected update or any dropped output
after setup, and has a 640-frame/90-second bound. An existing CSV is never
overwritten.
The elapsed budget is checked between frames and cannot interrupt a blocked
WGPU or OS call.
Initial surface-acquisition drops are reported as Dropped through the normal
receipt and retried only during setup, under the same total bound. Their count
is logged; eight successful setup frames precede the first workload.

Completion checks real text insertion with all original text retained, actual
scroll offset, changed splitter geometry and a committed version, exact tab
membership/selection, and survival of all eight application items. Idle,
hover and content-only phases must preserve the docking version. Every presented
exercise frame must produce actual mesh geometry. The CSV is written after
successful final validation; it contains all 384 recorded frames without timing
thresholds.

| CSV field | Measurement boundary |
| --- | --- |
| `update_ns` | Complete composed editor update through `run_single_surface_with_ui`, including the toolbar, input, measurement, encoding and publication |
| `tessellate_ns` | All output shapes converted to CPU meshes |
| `paint_present_ns` | Existing WGPU surface acquisition, texture/buffer upload, encoding, submission and `queue.present` CPU calls |
| `ui_to_present_ns` | Public update through presentation-receipt reporting, including platform output and host work between the component windows; excludes OS event delivery, input extraction and driver preparation |
| `vertices`, `indices` | Counts from the actual complete meshes, inspected after timing |

Surface acquisition or presentation may block. These are CPU-side call
durations, not GPU execution time, scanout, or OS-input-to-photon latency.
The exercise injects egui events, not physical OS input. It enables AccessKit
to observe semantic tab/splitter coordinates; driver state checks, coordinate
inspection and CSV writing stay outside timing. Painted idle explicitly asks
for a frame and does not measure the normal idle scheduler's ability to sleep.
Use a fixed build/window/DPI and preserve every raw row when comparing runs;
report p50/p95/maximum independently rather than summing phase medians.

The same journey has a headless semantic test, with a test-owned presentation
acknowledgement and zero timing placeholders. The demo tests also exercise
close approval, retained document edits, and recovery through the actual
Restore button after both roots close. They are not GPU/OS evidence:

```text
cargo nextest run --manifest-path integration/egui-official-harness/Cargo.toml --features demo --bin official-caller-owned-demo --target-dir target --locked -j1
```

The existing native eight-pane editor and its X11 drivers remain the separate
physical-input and native-lifecycle gates. This exercise adds continuous host
behavior and frame-cost evidence without changing those platform claims.
The [recorded host run](../../docs/performance/2026-11-08-egui-host-exercise.md)
includes every raw row, environment details and failed attempts.

## Docking Hover And Composed UI

The caller-owned tier qualifies docking input against the exact accepted output
and its completed egui pass. Applications explicitly declare ordinary pane
widgets through the `PaneResponse` returned by `PaneView::ui`; this does not make those
widgets docking drag handles or replace their normal input handling. Undeclared
interactive widgets remain blockers. Composite widgets may require more than
one registration; a visual rectangle or a container ID alone is not proof that
all of its interactive children are ordinary pane content.

`tests/hover_blockers.rs` constructs real official-egui popups, modals, foreground
areas and same-layer explicit blockers. It checks their actual bounds, rejects
hover and release through them, and rejects an old preview if a blocker appears
before release. A completed pass that did not render Dockspace cannot lend new
widget facts to its older accepted output. These are headless composed-UI and
output-contract tests, not physical input or GPU presentation certification.

The portable adapter does not infer arbitrary widget ancestry, popup ownership,
or transformed-layer coordinates. Missing exact facts remain unavailable or
blocked; it never falls back to pane geometry to authorize a different target.
The fork-backed final-stack query has a separate native qualification gate.

Run the gate from the repository root:

```text
cargo nextest run --manifest-path integration/egui-official-harness/Cargo.toml --locked -j1
```

Inspect the resolved dependency sources when diagnosing a failure:

```text
cargo tree --manifest-path integration/egui-official-harness/Cargo.toml --locked
```

Both `egui v0.37.0` and `eframe v0.37.0` in that tree must come from the
registry. The only intentional path dependencies are `egui_dockspace` and its
local `dockspace` dependency.
