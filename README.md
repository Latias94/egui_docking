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
| `egui_dockspace_native` | Unpublished fork-backed coordinator with one bounded real-window Glow smoke covering hidden child creation through exact retirement | The smoke uses product actions for tear-off and redock; release still requires a remotely reproducible fork pin, CI admission, platform capability coverage, and physical cross-window input evidence |
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
native workspace pins the graph-neutral egui/eframe fork revision
`a3c5ee57f4158ef717b5a956bf0d5c4aeda186ed` and the event-time Winit revision
`180bfc09743586137fec014ef5543cdde56ce5d0`. Remaining fork seams and their
removal conditions are documented in
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
cargo nextest run --manifest-path integration/egui-product-harness/Cargo.toml --all-features --test-threads=1
cargo nextest run --manifest-path integration/egui-official-harness/Cargo.toml --all-features --test-threads=1
```

The fork-backed native workspace uses one thin Cargo launcher:

```text
python3 scripts/run_egui_fork_harness.py
```

The ordinary `crates/egui_dockspace/examples/basic.rs` example exercises the
default interactive single-surface facade. The unpublished native example is a
development fixture, not a multiview product demo.

## Packaging and release order

`dockspace` must be packaged and published before `egui_dockspace`, because the
adapter's packaged manifest resolves the exact core version from the registry.
Before the first core release, CI fully verifies the core package and checks the
adapter's package file list; the local product and official-egui downstream
workspaces provide the adapter build and behavior evidence. After the core
version exists in the registry, rerun the full
`cargo package -p egui_dockspace` verification.

## License

`dockspace` is licensed under Apache-2.0. `egui_dockspace` is licensed under MIT
OR Apache-2.0. See [`THIRD_PARTY.md`](THIRD_PARTY.md) for prior-art and fixture
provenance.
