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
`egui_docking` or `egui_tiles` public APIs and persistence formats. The current
design contract and implementation sequence are recorded in
[`docs/plans/2026-08-08-001-refactor-dockspace-product-boundary-plan.md`](docs/plans/2026-08-08-001-refactor-dockspace-product-boundary-plan.md).

## Status

The new architecture is under active implementation. The renderer-neutral
behavior contract is executable, but the crates.io adapter intentionally does
not expose native multi-viewport support. The former 0.35 hosted-cycle runtime
and its scenario harness were deleted instead of being mechanically ported.
The new 0.36.1 fork exposes only graph-neutral event-time pointer facts and an
opaque renderer `Presented`/`NotPresented` callback. The unpublished native
crate now owns one thin `NativeCoordinator` around `DockspaceSession`; it does
not own a second docking engine, effect ledger, presentation ledger, or input
state machine. It preserves raw cross-window event order, exact viewport
bindings, affine painted-output settlement, typed snapshots, pointer facts,
close facts, and native effect results. A complete eframe application driver,
platform effect executor, and the single real two-window smoke are still U7
work, so native multiview is not yet a runnable product path.

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
upstream cannot prove as `Unknown`. The excluded native workspace pins the
public egui/eframe fork commit
`c9d63ddc22f4756cbf59a7fda0c1347cc2fd8c89` and the event-fact Winit commit
`4176f8aa1663ac804bf63030afbb6c2d0afe4ad7`. The fork remains graph-neutral;
all docking topology, receiver challenges, native lifecycle meaning, and
presentation obligations stay in `dockspace`.

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
native vertical slice. The fork-backed workspace is reproducible from the two
pushed revisions above and can be checked without custom cfg injection or a
separate target directory:

```text
python3 scripts/run_egui_fork_harness.py
```

The former 0.35 harness and E2E scenario runner were removed. U7 will add one
ordinary real-window smoke only after the 0.36 coordinator has a complete app,
effect, and viewport lifecycle path.

The ordinary `crates/egui_dockspace/examples/basic.rs` example deliberately
uses the registry-only paint path and is not an interaction or multiview demo.

## License

`dockspace` is licensed under Apache-2.0. `egui_dockspace` is licensed under MIT OR
Apache-2.0. See `THIRD_PARTY.md` for prior-art and fixture provenance.
