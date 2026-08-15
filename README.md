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
The new 0.36.1 fork exposes only graph-neutral event-time pointer facts, a
completed-pass widget hit snapshot, and an opaque renderer
`Presented`/`NotPresented` callback. The unpublished native crate keeps its
callback coordinator private and owns one `DockspaceSession`; it does not own a
second docking engine, effect ledger, presentation ledger, or input state
machine. Its `NativeDockspaceApp` now provides a runnable root-window vertical
slice using the same core-derived product renderer as the ordinary facade. It
preserves local egui actions across discarded passes and qualifies native
receiver authority only with the final presented pass. The slice still rejects
replacement, close, focus, and pointer pass-through effects. It now retains an
exact deferred child create request, renders the pre-show and post-show staging
outputs while the native window is hidden, and applies the correlated show
acknowledgement before core may transfer ownership. Root and admitted child
callbacks now render through one shared `DockspaceSession` and pane registry;
the first semantic child output therefore uses the ordinary affine
presentation result before core can complete first-live admission.
Cross-window scroll authority, destructive child retirement, and the real
two-window smoke are still U7 work, so this lifecycle slice is not yet native
multiview product support.

The ordinary crates.io `show_single_surface` convenience path now owns one
renderer-neutral `DockspaceSession` and treats the current egui `Response` as
local receiver evidence. A Ready single-surface frame can perform
revision-bound tab selection/close and emit current-pass tab, splitter, and
contained gesture actions without inventing a post-`FullOutput` renderer fact.
Tab dragging now paints the complete core-owned center/four-way guide cluster,
including exact active and rejected states, and commits the same target that
was previewed. The product renderer still does not expose native multi-viewport
ownership. Output publication remains paint-only in the renderer-settlement
sense; the missing native lifecycle remains a deliberate release blocker rather
than a heuristic fallback.

The base adapter now has one application-facing path at the crate root. The
former egui backend facade, duplicate projection, presentation ledger, and
test-only engine owner were deleted after the product and native paths moved to
`DockspaceSession`. Renderer and native hosts integrate through the
renderer-neutral `dockspace::runtime` contract instead of constructing another
docking engine inside `egui_dockspace`.

Ordinary commands, close decisions, style changes, and paint responses return
product-level mutation and outcome values. They do not expose reducer ticks,
`EngineTransition`, scene stamps, or presentation ledgers. Those diagnostics
remain inside the adapter and core. Renderer-neutral host reports expose only
host-actionable outcomes: per-surface status, native close edges, affine native
effects, presentation outputs, and repaint requirements.
Facade failures follow the same boundary: callers branch on the six stable
`DockspaceErrorKind` categories, while exact renderer and reducer diagnostics
remain private in the standard error source chain.

The renderer-neutral crate follows the same split. Its default API exposes the
model and `dockspace::runtime` facade; adapter-only reducer, scene, pointer,
effect, recovery, and viewport state machines remain private or are isolated
behind workspace-only core conformance seams. Product adapters do not enable a
second egui backend feature.

The base crate resolves the official egui release and treats receiver facts that
upstream cannot prove as `Unknown`. The excluded native workspace pins the
public egui/eframe fork commit
`bbb96417d63340c2ad92876049e7a3388c8e00a6` and the event-fact Winit commit
`180bfc09743586137fec014ef5543cdde56ce5d0`. The fork remains graph-neutral;
all docking topology, receiver challenges, native lifecycle meaning, and
presentation obligations stay in `dockspace`. The admission reason and removal
condition for every remaining fork seam are recorded in
[`docs/knowledge/egui-native-fork-seam-admission.md`](docs/knowledge/egui-native-fork-seam-admission.md).

## Persistence

The optional `serde` feature adds session-owned product persistence.
Applications allocate stable pane identities
through `DockspaceDocumentBootstrap`, construct a persistent `Dockspace`, and
save or restore one versioned JSON document through the product facade. The
document atomically binds workspace topology, append-only external pane keys,
allocator frontiers, lineage/generation, and viewport placement. Raw workspace
snapshots, mutable restore candidates, and independently replaceable key maps
remain private implementation details.

## Development

The official workspace uses Rust 1.95 and edition 2024.

```text
cargo nextest run --workspace --all-features --all-targets
cargo clippy --workspace --all-features --all-targets -- \
  -A warnings -D clippy::correctness -D clippy::suspicious
cargo fmt --all --check
cargo test --manifest-path integration/egui-product-harness/Cargo.toml -j1
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

The former 0.35 harness and E2E scenario runner were removed. The unpublished
native crate now has one ordinary root-window example:

```text
cargo run --manifest-path crates/egui_dockspace_native/Cargo.toml --example basic
```

It proves the fork-backed app/coordinator/output path without adding a runner,
digest gate, or scenario framework. U7 will add one real two-window smoke only
after deferred child effects and viewport lifecycle are complete.

The ordinary `crates/egui_dockspace/examples/basic.rs` example uses the default
single-surface product facade and is suitable for checking local tab and
contained/splitter feedback, including core-owned center and four-way docking
guides. It is not a native multi-viewport demo.

## License

`dockspace` is licensed under Apache-2.0. `egui_dockspace` is licensed under MIT OR
Apache-2.0. See `THIRD_PARTY.md` for prior-art and fixture provenance.
