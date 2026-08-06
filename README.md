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

The ordinary crates.io `show_single_surface` convenience path can paint the
current plan, but upstream egui/eframe does not expose the post-`FullOutput`
presentation fact needed to authorize interaction. That callback-only path
therefore reports presentation authority as unavailable and remains paint-only
in non-test builds.

The base adapter now exposes the low-level `begin_outer_frame` boundary needed
to prove the current official-egui behavior matrix. It freezes the complete
logical surface roster, replaces intermediate egui multipass drafts, and
commits the final drafts in one reducer tick only after the host confirms each
matching final `FullOutput`. Each final output is bound to an opaque
renderer-settlement capability. The host
consumes that bound value at its renderer boundary and returns `Presented` or
`Dropped`; results are reduced on the next outer frame behind a contiguous
per-stream causal barrier. This production path now drives tab selection and
close, exact-guide docking, splitter resize, and contained-floating resize
through the same pointer journal and exact egui receiver evidence used by its
official-egui integration tests. The official-egui route is still a low-level
host protocol: no registry-only event loop can provide its missing native facts.
These vertical slices therefore do not constitute native multi-viewport product
support.

The base crate resolves the official egui release and treats receiver facts that
upstream cannot prove as `Unknown`. The release-pinned fork carries global event
provenance, cross-viewport receiver probing, terminal renderer results, and a
complete-roster input-before-paint hosted-cycle SPI. Native lifecycle ownership
lives in the unpublished `egui_dockspace_native` runtime. The excluded native
workspaces pin the public fork commit
`45e67e3fcddc1f0b38e73ba4f2be7968ee6cbbe7` and the event-fact Winit commit
`ae4e6c5ee0af4b68c4ea1c2a2763b97e3ab7ae4c`; the complete two-window conformance
matrix, bounded long-session ledgers, performance gates, an upstream-reviewable
patch series, and the sealed public facade remain release blockers. Egui no
longer computes a second docking geometry plan: it submits intrinsic
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

The workspace uses Rust 1.92 and edition 2024.

```text
cargo nextest run --workspace --all-features --all-targets
cargo clippy --workspace --all-features --all-targets -- -D warnings
cargo fmt --all --check
```

The crate manifests pin the official egui and eframe registry coordinates to
`=0.35.0`, and the root workspace contains no source patch. The independent
`integration/egui-official-harness` workspace verifies that an external consumer
resolves registry egui/eframe and can paint through the public facade:

```text
cargo test --manifest-path integration/egui-official-harness/Cargo.toml --locked
```

The development fork in `repo-ref/egui-release` starts at the exact upstream
`0.35.0` tag. The excluded fork-backed workspaces pin the complete public patch
revision `45e67e3fcddc1f0b38e73ba4f2be7968ee6cbbe7` and Winit revision
`ae4e6c5ee0af4b68c4ea1c2a2763b97e3ab7ae4c`; their lockfiles make a clean
checkout reproducible without either ignored local repository. The fork-backed
runtime remains `publish = false` until its required seams are available from a
publishable release. Launch the current two-window trial example with:

```text
python3 scripts/run_native_multiview.py
```

A self-driving native smoke gate starts one real root window, tears a two-tab group off into a
dynamically created child, waits for the child's first-live interaction admission, redocks the
complete group into the root, verifies exact ownership, tab order, selection, MRU, and source
vacancy, then scrolls an overflowing tab strip through the production Winit window-event and
egui derivative-claim path before closing itself:

```text
python3 scripts/run_native_e2e.py
```

This gate drives typed pointer events and a positioned `WindowEvent::MouseWheel` through the real
native event loop and uses real OS windows. It does not simulate hardware input, transfer a window
between mixed-DPI monitors, exercise close/focus failure matrices, or prove grab-offset
preservation.

The ordinary `crates/egui_dockspace/examples/basic.rs` example deliberately
uses the registry-only paint path and is not an interaction or multiview demo.

## License

`dockspace` is licensed under Apache-2.0. `egui_dockspace` is licensed under MIT OR
Apache-2.0. See `THIRD_PARTY.md` for prior-art and fixture provenance.
