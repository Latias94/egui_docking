# egui_dockspace

`egui_dockspace` is undergoing a breaking refactor into a renderer-neutral
docking core plus thin UI and platform adapters. There is no compatibility
layer for the former `egui_docking`, `egui_tiles`, or pre-refactor persistence
interfaces.

The current design and implementation sequence live in
[`docs/plans/2026-08-08-001-refactor-dockspace-product-boundary-plan.md`](docs/plans/2026-08-08-001-refactor-dockspace-product-boundary-plan.md).

## Capability matrix

| Layer | Current product capability | Deliberate limit |
| --- | --- | --- |
| `dockspace` | Stable item/root/surface layout, validated atomic actions, core-derived paint and receiver plans, close workflow, optional session-owned persistence, and renderer-neutral native lifecycle semantics | Does not own a renderer, widget tree, animation clock, event loop, or OS window |
| `egui_dockspace` | Interactive official-egui single-surface docking: tab select/close/reorder, center and edge docking, splitter resize, keyboard and AccessKit actions, and contained move/resize | Does not expose native multi-viewport ownership |
| `egui_dockspace_native` | Unpublished fork-backed coordinator with a small interactive two-window example and one bounded CI real-window Glow smoke covering hidden child creation through exact retirement | The example uses an explicit product action for tear-off; broader platform capability coverage and physical cross-window input evidence remain before native multiview is release-ready |
| Open-GPUI | Reference evidence only | No adapter or integration work is in the current plan; any future cutover requires a separate plan |

The repository should therefore be described as a high-correctness docking core
with a usable official-egui single-surface product path and an experimental
native multiview adapter. It is not yet a mature ImGui-level native multiview
crate.

## Architecture

- `dockspace` is the only semantic authority for topology, selection, drop,
  close, persistence identity, and native lifecycle state.
- `egui_dockspace` supplies egui measurements, resources, painting, local
  responses, and accessibility without reconstructing a second dock graph.
- `egui_dockspace_native` supplies event-time platform facts, OS effects,
  renderer settlement, and window lifecycle without owning another docking
  engine.

Default product interfaces are item- and surface-centric. Raw graph nodes,
scene stamps, reducer inputs, provider leases, receipts, persistence candidates,
and backend FSMs are private. Renderers and native hosts integrate through the
narrow `DockspaceSession` runtime facade rather than an unstable raw backend
namespace.

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

The publishable crates use official egui and eframe `0.36.1`. The excluded
native workspace uses the reviewed egui/eframe fork revision
`4f86e8daa16265e6ca0593622969958c2f50b31f`, which contains every admitted native
seam. The event-time Winit fork is pinned at
`6105ef864c93c231cad2d76be94054eaf707375a`. The manifest, lockfile, CI, and
this document must continue to move together when the fork advances.
Remaining fork seams and their removal conditions are documented in
[`docs/knowledge/egui-native-fork-seam-admission.md`](docs/knowledge/egui-native-fork-seam-admission.md).

Dear ImGui and `dear-imgui-rs` are behavioral references for frame phases,
viewport lifecycle, wakeups, geometry/DPI facts, and teardown coverage. Their
global context registries, raw-pointer ownership, timeout focus heuristics, and
renderer-owned docking authority are intentionally not copied.

## Development

The workspace uses Rust 1.95 and edition 2024. Reuse the normal target directory
and run Cargo serially when the machine is busy.

```text
cargo fmt --all -- --check
cargo check --workspace --all-features --all-targets --locked -j1
cargo nextest run --workspace --all-features --all-targets --test-threads=1
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

The ordinary `crates/egui_dockspace/examples/basic.rs` example exercises the
default interactive single-surface facade. To inspect the fork-backed native
path, run the separate example. It starts with the Inspector group as an
egui-styled contained window in the root surface; it does not open a child OS
window automatically:

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
