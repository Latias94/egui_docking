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

- A caller that owns `Context::run_ui` uses `egui_dockspace::run_ui` and
  receives an owned complete output and an affine renderer settlement receipt.
- An unmodified `eframe::App::ui` callback keeps rendering and current-pass
  local actions, but reports `PresentationSettlementRequired` for preview and
  pane-focus finality because eframe owns the surrounding input and output.

Passing the second test is a capability-negative result, not a claim that
ordinary eframe provides full draggable docking.

The wrapper verifies the complete surface shape span after output hooks, then
returns `DockspaceOutput`. Its `into_parts()` transfers the egui output and a
one-use receipt. Headless test owners explicitly accept their frames; the real
window demo owns WGPU acquisition, submission and `queue.present`, and reports
`Presented` only after the latter returns. Failed acquisition reports `Dropped`
while retaining texture deltas for a later successful upload. Receipt drop never
grants authority. Core, official and native adapters share exact output reporting;
there is no public preview-only acknowledgement bypass.

This conservative official path admits synchronous append-only drawing on the
supplied Ui layer. Cross-layer pane output makes settlement unavailable. Shape
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
