# egui_dockspace

`dockspace` is the primary library: a renderer-neutral docking core intended
for custom game-engine editor UIs. Engines own their drawing, pane content,
input delivery, frame scheduling, and platform resources. `egui_dockspace`
and its native integration are interactive reference adapters used to validate
the core's behavior and host contracts. Open-GPUI adapter work is deferred.

The repository is undergoing a breaking refactor. There is no compatibility
layer for the former `egui_docking`, `egui_tiles`, or pre-refactor persistence
interfaces.

The current engine-host and direct-manipulation plan lives in
[`docs/plans/2026-08-21-1357-refactor-dockspace-direct-manipulation-experience-plan.md`](docs/plans/2026-08-21-1357-refactor-dockspace-direct-manipulation-experience-plan.md).
It prioritizes a documented public host API and an independent consumer with
no egui dependency before broader native certification. The recording consumer
now verifies owned output records, ordered presentation settlement, and commands
on two logical surfaces, complete ordered gestures, contained content, close
approval and exact feedback/focus. Native throughput, lifecycle consolidation
and platform certification remain separate unfinished gates.
The earlier product-boundary plan supplies the persistence and ownership context.

## Capability matrix

| Layer | Current product capability | Deliberate limit |
| --- | --- | --- |
| `dockspace` | Stable item/root/surface layout, validated atomic actions, core-derived paint and receiver plans, close workflow, optional session-owned persistence, and renderer-neutral native lifecycle semantics | Does not own a renderer, widget tree, animation clock, event loop, or OS window |
| `egui_dockspace` | Caller-owned official-egui runs support tab select/close/reorder, magnetic center and edge docking, splitter resize, keyboard and AccessKit actions, and contained move/resize | An unmodified `eframe::App::ui` callback cannot prove terminal presentation; preview-qualified drag and pane-focus settlement are typed unavailable there |
| `egui_dockspace_native` | Unpublished fork-backed coordinator with a small interactive two-window example, a bounded programmatic real-window smoke, and an X11/XTest physical drag smoke | Native consolidation and fresh physical qualification remain open. The existing reference lane is X11 on the first X screen with one exact active-CRTC work-area authority; local/headless gates do not certify it |
| Open-GPUI | Reference evidence only | No adapter or integration work is in the current plan; any future cutover requires a separate plan |

Caller-owned official-egui runs also retain explicit local touch identities:
short taps activate on release, pre-arm movement yields to pane content, and a
350ms stationary continuation arms ordinary docking/resize or a semantic context
menu. Explicit Touch resize targets request at least 44 logical points before
surface and protected-control clipping; mouse/pen/unknown-device geometry keeps
its existing profile. Native touch/stationary capture remains unqualified.

Local drags use the same finite continuation for tab hover selection and
overflow-edge scrolling. Each automatic scroll step waits for exact presentation
of its new geometry, and explicit scrolling or menu actions take ownership
through the existing tab-strip state.

The current implementation has a qualified renderer-neutral local/contained
contract, an official-egui caller-owned validation path, and an experimental
native multiview adapter. The fast-gesture and configuration-dependent failures
from the 2026-10-27 audit are repaired and covered by fresh regressions. See
[`docs/VALIDATION.md`](docs/VALIDATION.md). It is not yet a mature ImGui-level
docking and multiview crate.

The experimental native adapter reserves 4,096 raw callback slots and two
reusable complete-roster payloads. It admits at most 256 simultaneously retained
native binding lifetimes, including pending retirement; a route/create pair
shares one commitment. More than 64 work-area records revoke work-area authority
to `Unknown` while preserving the complete window inventory. Snapshot storage
pressure freezes ordinary admission through the existing overload boundary.
The full 4 MiB transfer budget and input-load qualification remain unfinished.

## Architecture

- `dockspace` is the only semantic authority for topology, selection, drop,
  close, persistence identity, and native lifecycle state.
- `egui_dockspace` supplies egui measurements, resources, painting, local
  responses, and accessibility without reconstructing a second dock graph.
- `egui_dockspace_native` supplies event-time platform facts, OS effects,
  renderer settlement, and window lifecycle without owning another docking
  engine.

Default product interfaces are item- and surface-centric. Raw graph nodes,
scene stamps, reducer inputs, provider leases, persistence candidates,
and backend FSMs are private. Renderers and native hosts integrate through the
narrow `DockspaceSession` runtime facade rather than an unstable raw backend
namespace.

The public host contract distinguishes complete surface-program acceptance,
renderer submission, final host presentation, and successful core acceptance of
the presentation report. Opaque execution receipts and output handles bind each
step to its exact emission. Engines can record their own drawing or retained
nodes after the frame borrow ends; a logical surface does not require an OS
window. The independent
[`dockspace-host-harness`](integration/dockspace-host-harness/README.md) demonstrates
the public path without any egui or platform dependency.

Run its complete engine-owned output example:

```text
cargo run --manifest-path integration/dockspace-host-harness/Cargo.toml --example engine_host --locked
```

The example retains its recording after the frame borrow ends, settles two
logical surfaces independently, executes application focus, approves a complete
root close, and deduplicates typed feedback. `Presented`/`Dropped` are recording
host observations; this example does not submit work to a GPU or create windows.

Facade errors follow the same rule. Both the renderer-neutral and egui product
interfaces expose six action-oriented categories: invalid configuration,
persistence, unsupported operation, operation conflict, host protocol, and
internal failure. Exact implementation diagnostics remain in the standard
error source chain.

## Persistence

The optional `serde` feature adds product document persistence. Applications
allocate stable pane identities through `DockspaceDocumentBootstrap`, build a
persistent session, and save or restore one versioned JSON document. The
document atomically binds topology, append-only external pane keys, allocator
frontiers, lineage and generation, and viewport placement.

Raw workspace snapshots, independently replaceable key maps, and mutable
restore candidates are not product interfaces.

## egui and native references

The publishable crates use official egui and eframe `0.37.0`. The excluded
native workspace still pins the last published egui/eframe fork baseline,
`0716beee5b7284389f2e80ffb6b24b5af6ae2910`. Native resource-ownership work now
also requires the local, unpublished fork commit
`1f7c541fde99fd69192622a1a36cb642de58ed29`, qualified through explicit local
Cargo overrides. The tracked native manifests and CI cannot yet reproduce that
new work from their remote pins alone. The event-time Winit fork is pinned at
`f378b50f29cafafec31cd8675c17646b4ef57d70`. The manifest, lockfile, CI, and
this document must continue to move together when the fork advances.
Remaining fork seams and their removal conditions are documented in
[`docs/knowledge/egui-native-fork-seam-admission.md`](docs/knowledge/egui-native-fork-seam-admission.md).

The official adapter and admitted native fork use egui/eframe `0.37.0`.
The fork branch `dockspace-native-seam-0.37` retains graph-neutral seams for
exact native facts and output finalization. Winit remains on stable `0.30.13`,
and egui_tiles remains reference-only.

Hosts that own official crates.io egui's complete run use the Dockspace wrapper
to bind local settlement to that run without application-managed frame counts:

```rust,ignore
let output = egui_dockspace::run_ui(&context, raw_input, |ui| {
    dockspace
        .show_single_surface(surface, ui, panes)
        .expect("the Dockspace frame advances");
});
let (full_output, receipt) = output.into_parts();
let result = renderer.present(full_output); // Presented or Dropped, from the output owner.
receipt.report(result);
```

`egui_dockspace::run_ui` returns `DockspaceOutput`. Split it with
`into_parts()` to obtain `FullOutput` and a one-use `DockspaceOutputReceipt`.
The renderer owner reports `Presented` only after that complete output reaches
presentation, or `Dropped` when it is discarded; dropping the receipt also
retires it. Core accepts exact emissions at the next successful frame boundary,
including delayed or out-of-order reports. Returning from egui never grants
presentation authority by itself.

The adapter checks the complete ordered surface shape span after every output
hook, including ordinary pane/tab drawing, previews, clip and transform data.
It removes only its private markers. This conservative path requires synchronous,
append-only pane drawing on the supplied layer; cross-layer writes and nonempty unenumerated layers are unavailable
for settlement. After `into_parts()`, the host is responsible for preserving the
complete output it reports. Shape checks do not inspect application paint callback
internals or observe GPU submission. Discard, unwind, duplicate enrollment,
foreign contexts/viewports and output mismatch cannot promote partial evidence.

An unmodified `eframe::App::ui` callback may still call
`show_single_surface` directly for rendering, selection, close, menus, and
discrete keyboard or AccessKit actions. Because eframe owns the surrounding
`RawInput` and `FullOutput`, that response reports
`PresentationSettlementRequired`; tab/group/contained pointer drags are not
started instead of showing a preview that can never settle. The fork-backed
native adapter owns the stronger post-plugin finalizer and exact unwind
abandonment required for editor-grade native multiview.

Dear ImGui and `dear-imgui-rs` are behavioral references for frame phases,
viewport lifecycle, wakeups, geometry/DPI facts, and teardown coverage. Their
global context registries, raw-pointer ownership, timeout focus heuristics, and
renderer-owned docking authority are intentionally not copied.

## Development

The workspace uses Rust 1.95 and edition 2024. Reuse the normal target directory
and run Cargo serially when the machine is busy.

```text
cargo fmt --all -- --check
python3 scripts/check_core_dependencies.py
cargo check --workspace --all-features --all-targets --locked -j1
cargo nextest run --workspace --all-features --all-targets --test-threads=1
cargo nextest run --manifest-path integration/dockspace-host-harness/Cargo.toml --locked --test-threads=1
cargo nextest run --manifest-path integration/dockspace-host-harness/Cargo.toml --features serde --locked --test-threads=1
cargo nextest run --manifest-path integration/egui-product-harness/Cargo.toml --test-threads=1
cargo nextest run --manifest-path integration/egui-product-harness/Cargo.toml --features serde --test persistence --test-threads=1
cargo nextest run --manifest-path integration/egui-official-harness/Cargo.toml --all-features --test-threads=1
cargo package --package dockspace --all-features --locked -j1
cargo package --package egui_dockspace --all-features --locked --list
```

The fork-backed native workspace uses one thin Cargo launcher:

```text
python3 scripts/run_egui_fork_harness.py
```

To run the supported official-crates.io tier in a real window, use the small
caller-owned `egui-winit`/`egui-wgpu` host. It calls
`egui_dockspace::run_ui` around the complete frame, so tab drag, magnetic dock,
and contained-window movement have terminal-output settlement without any fork:

```text
cargo run --manifest-path integration/egui-official-harness/Cargo.toml --features demo --bin official-caller-owned-demo --locked -j1
```

The ordinary `crates/egui_dockspace/examples/basic.rs` example demonstrates the
honest unmodified-eframe boundary: rendering, selection, close, menus, and
keyboard adjustments remain available while the status strip explains why
preview-qualified drag is disabled:

```text
cargo run --manifest-path crates/egui_dockspace/Cargo.toml --features example-app --example basic --locked -j1
```

To exercise draggable tabs, magnetic docking, contained movement, and the
fork-backed exact-output path, run the separate native example. It starts with
the Inspector group as
an egui-styled contained window in the root surface; it does not open a child
OS window automatically:

```text
cargo run --manifest-path integration/egui-fork-workspace/Cargo.toml --package egui_dockspace_native --example basic --locked -j1
```

The example exposes all three presentation paths without conflating their
evidence. Use the contained title bar to move or resize the same-surface window,
or open its **⋮** menu for **Dock Back** and **Move to New Window**. The toolbar
duplicates the native move and dock-back commands as explicitly labelled
programmatic fallbacks. On a capability-qualified backend, drag a tab or the
contained title bar outside an OS window to request a physical native child,
then drag a child tab onto a root-window docking guide to return it. The default
child close policy recovers the previous presentation. Physical dragging
remains fail-closed on backends without complete cross-window pointer facts, so
the programmatic lifecycle demo is not described as physical-input evidence.
The pinned macOS Winit facts are tested for same-window routing, but macOS does
not advertise cross-surface physical promotion until an OS-input lane proves
the complete button/capture contract. Windows likewise keeps hover and desktop
pointer authority unavailable until equivalent platform-fact tests land. X11
is the current certified physical reference backend.

CI keeps those claims separate. The programmatic Glow smoke proves hidden
creation, staging, first-live transfer, redock, and retirement through product
commands. The X11/XTest smoke locates only read-only semantic receivers, injects
real OS pointer events through Winit, and proves drag-out, first-live, the exact
Center guide, drag-back, OS-window destruction, and retirement quiescence. It
does not expose core geometry or provide a reusable GUI test engine.

## Packaging and release order

`dockspace` must still be published before `egui_dockspace`, because the
adapter's packaged manifest resolves its declared core dependency from the
registry. Before that publication, a clean checkout cannot make Cargo verify the
adapter tarball: package verification intentionally resolves the normalized
manifest through the registry rather than through the workspace path dependency.

The local and CI gate therefore verifies the `dockspace` tarball, checks the
complete `egui_dockspace` package file list, and compiles the adapter through the
independent product and official-egui harnesses. After the matching `dockspace`
version is available in the registry, run `cargo package --package
egui_dockspace --all-features --locked` as the release-side adapter verification.

## License

`dockspace` is licensed under Apache-2.0. `egui_dockspace` is licensed under MIT
OR Apache-2.0. See [`THIRD_PARTY.md`](THIRD_PARTY.md) for prior-art and fixture
provenance.
