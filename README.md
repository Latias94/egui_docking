# egui_dockspace

`egui_dockspace` is a deterministic docking system for egui, under a breaking
refactor toward a renderer-neutral `dockspace` core.
The repository is split into three layers:

- `dockspace`: a renderer-neutral docking graph, layout solver, transaction engine,
  interaction protocol, and viewport lifecycle coordinator.
- `egui_dockspace`: a crates.io egui renderer for one logical surface plus
  contained floating presentations, built on the headless core.
- `egui_dockspace_native`: an unpublished, fork-backed eframe runtime for
  transactional native viewports and desktop-global input.

The rewrite intentionally has no compatibility layer for the former
`egui_docking` or `egui_tiles` public APIs and persistence formats. The design
contract and implementation sequence are recorded in
[`docs/plans/2026-07-18-001-headless-dockspace-refactor-plan.md`](docs/plans/2026-07-18-001-headless-dockspace-refactor-plan.md).

## Status

The new architecture is under active implementation. The renderer-neutral
behavior contract is executable, but the crates.io adapter intentionally does
not expose native multi-viewport support. The empty `native` feature was removed
rather than presenting test scaffolding as a product capability. The
release-pinned fork now provides a complete-roster, input-before-paint hosted
cycle for both native renderers. The unpublished native runtime connects that
cycle to exact viewport incarnations, the desktop-global pointer journal,
native effects, hidden staging, affine renderer settlement, and the previously
presented immutable receiver graph. The trial example now restores two logical
surfaces into two independently owned OS windows through the same hosted-cycle
path. Docking-owned child close requests now retain their exact event order and
window incarnation, open a core-owned `ClosePlan`, and settle application
allow, veto, or deferred decisions through causally acknowledged `Destroy` or
`CancelClose` effects. Root application shutdown remains a separate host
responsibility. This is a runnable trial path, not a native support claim. The
real-window smoke gate now automates dynamic tear-off and cross-window redock
through the native event loop. Hardware-input coverage, mixed-DPI monitor
transfer, close/focus failure matrices, bounded core ledgers, and an
upstream-reviewable fork patch series are still release blockers.

The ordinary crates.io `show_single_surface` convenience path now treats the
current egui `Response` as local receiver evidence. A Ready single-surface frame
can perform revision-bound local tab actions without inventing a
post-`FullOutput` renderer fact. Output publication remains paint-only in the
renderer-settlement sense; retained/native receiver lookup still requires a
separately accepted snapshot. Pointer-driven tab docking, splitter drag, and
contained move/resize are being migrated to the same local-action boundary and
remain release blockers until their downstream interaction cases pass.

The base adapter keeps the application-facing API at the crate root. Custom
hosts opt into the low-level `backend` feature and use the
`egui_dockspace::backend` protocol; raw `DockEngine` access is not exposed. That
protocol freezes the complete logical surface roster, replaces intermediate
egui multipass drafts, and commits final drafts in one reducer tick only after
the host confirms each matching final `FullOutput`. Each final output is bound
to an opaque renderer-settlement capability. The host
consumes that bound value at its renderer boundary and returns `Presented` or
`Dropped`; results are reduced on the next outer frame behind a contiguous
per-stream causal barrier. This production path now drives tab selection and
close, exact-guide docking, splitter resize, and contained-floating resize
through the same pointer journal and exact egui receiver evidence used by its
official-egui integration tests. The official-egui route is still a low-level
host protocol: no registry-only event loop can provide its missing native facts.
These vertical slices therefore do not constitute native multi-viewport product
support.

Ordinary commands, close decisions, style changes, and paint responses return
product-level mutation and outcome values. They do not expose reducer ticks,
`EngineTransition`, scene stamps, or presentation ledgers. Those diagnostics
remain inside the adapter and core. The explicitly unstable backend protocol
returns only host-actionable receipts: per-surface status, native close edges,
effect acceptance, and presentation-settlement counts.
Facade failures follow the same boundary: callers branch on the six stable
`DockspaceErrorKind` categories, while exact renderer and reducer diagnostics
remain private in the standard error source chain.

The renderer-neutral crate follows the same split. Its default API exposes the
model and `dockspace::runtime` facade; adapter-only reducer, scene, pointer,
effect, recovery, and viewport state machines are private. Renderer authors and
workspace-private protocol harnesses explicitly enable `dockspace/backend` and
import those unstable contracts through `dockspace::backend`. This is an
intentional breaking boundary, not a compatibility alias for the former module
paths.

The base crate resolves the official egui release and treats receiver facts that
upstream cannot prove as `Unknown`. The release-pinned fork carries global event
provenance, cross-viewport receiver probing, terminal renderer results, and a
complete-roster input-before-paint hosted-cycle SPI. Native lifecycle ownership
lives in the unpublished `egui_dockspace_native` runtime. The excluded native
workspace pins the public fork commit
`5016206ba71228d11e594ff2c1dd1887da904486` and the event-fact Winit commit
`c4e37f29ca448d0bfb10f5d223479154091c5898`; the single native smoke,
deterministic conformance suites, bounded long-session ledgers, performance
gates, an upstream-reviewable patch series, and the sealed public facade remain
release blockers. Egui no longer computes a second docking geometry plan: it submits intrinsic
measurements and paint resources, then paints the core-owned
`PresentationPlan` directly.

## Persistence

The egui facade persists only an atomic `DockspaceDocument`: one versioned blob
containing the workspace snapshot, stable external-pane key map, viewport
placement preferences, document lineage id, generation, and a BLAKE3 binding
hash. First mint every pane `ItemId` from a one-time
`DockspaceDocumentBootstrap`, build the workspace from those identities, and
consume that bootstrap with `Dockspace::bind_document_persistence`. Then use the parameterless
`Dockspace::save_document_json` and session-owned `load_document_json` while no
joined backend is active. A native/backend host uses `queue_document_json`
instead: the session retains the validated intent across cycle rollback and
provider replacement, records a fresh terminal backend attempt, and publishes
core state plus sidecars in the enclosing outer-frame commit. The resolver
proves every exact `(document id, item id, external key)` association,
including closed-pane history. Restore does not publish the workspace, key map,
placement, document lineage, or generation until every component and the
application identity registry validate. Core state and durable sidecars are then
published through one non-copyable capability bound to the exact session,
restore token, engine authority domain, document generation, and reconciled item
identity scope; an unbound session can adopt one complete validated document at
that same boundary.
Workspace-only and placement-only snapshots are internal interchange components,
not egui restoration APIs.

## Development

The official workspace uses Rust 1.95 and edition 2024.

```text
cargo nextest run --workspace --all-features --all-targets
cargo clippy --workspace --all-features --all-targets -- \
  -A warnings -D clippy::correctness -D clippy::suspicious
cargo fmt --all --check
```

The crate manifests pin the official egui and eframe registry coordinates to
`=0.36.1`, and the root workspace contains no source patch. The independent
`integration/egui-official-harness` workspace verifies that an external consumer
resolves registry egui/eframe and can paint through the public facade:

```text
cargo nextest run --manifest-path integration/egui-official-harness/Cargo.toml --locked -j1
```

Upstream egui `0.36.1` commit
`4c1f2fae95475a40e524884ebb298bcb1714b08e` is the clean baseline for the
next native vertical slice. The current `repo-ref/egui-release` revision
`5016206ba71228d11e594ff2c1dd1887da904486` and Winit revision
`c4e37f29ca448d0bfb10f5d223479154091c5898` are retained only as behavior and
patch references. Their former 0.35 adapter/native build gate is intentionally
suspended because the product crate now targets egui 0.36.1; mixing those two
dependency families would not be a reproducible compatibility claim. U7 will
create one clean 0.36.1 fork/native workspace and one focused real-window smoke
after the minimal seams are proven.

The ordinary `crates/egui_dockspace/examples/basic.rs` example deliberately
uses the registry-only paint path and is not an interaction or multiview demo.

## License

`dockspace` is licensed under Apache-2.0. `egui_dockspace` is licensed under MIT OR
Apache-2.0. See `THIRD_PARTY.md` for prior-art and fixture provenance.
