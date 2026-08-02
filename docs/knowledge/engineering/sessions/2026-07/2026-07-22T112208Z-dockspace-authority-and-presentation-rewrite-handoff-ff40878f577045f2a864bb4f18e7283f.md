---
type: "Session Handoff"
title: "Dockspace authority and presentation rewrite handoff"
description: "Verified authority work, active acknowledgement migration, and the dependency order for replacing the duplicate egui projection engine."
timestamp: 2026-07-22T11:22:08Z
record_id: "ff40878f577045f2a864bb4f18e7283f"
producer_id: "codex-root"
run_id: "goal-019f752c"
---

# Summary

The refactor keeps the N-ary workspace model while making `dockspace` the sole semantic authority. Rootless surface rosters, policy-at-commit, tick-final vacancy, unique drop winner, canonical drag, native effect ledgers, and multi-surface reducer batches are implemented. The active task separates prepared presentation candidates from prior-host-sequence presentation acknowledgement and exact interaction authority.

# Verified State

- `dockspace_core_protocol` passes 22 typed multi-surface protocol tests and rejects the deleted singular contribution protocol.
- The core suite most recently completed 751 of 752 tests during concurrent edits; the sole observed failure passed on an immediate exact rerun. A stable full rerun is still required after writers stop.
- A stale callback with changed surface measurements can no longer release a drag against old geometry. It terminates with unknown target authority and leaves the workspace unchanged.
- Same-sequence paint cannot acknowledge itself. Delayed acknowledgement is accepted only for an exact retained stamp from a prior host sequence and advances a monotonic observation generation.
- Open GPUI remains the GPUI facade and runtime authority for `DockSurface`, entities, focus wiring, accessibility mapping, motion execution, and native window effects. Its graph, transactions, interaction, routing, and lifecycle semantics are migration targets.

# Open Threads

- Finish the egui acknowledgement test migration and run stable `nextest`, all-target checks, rustdoc warnings, formatting, and diff checks.
- Replace the old presentation records with one `Arc<SurfacePresentationSemantics>` plus a strict one-to-one projection. Hidden, offscreen, and unmeasured entities retain semantic records and only lose measured geometry.
- Move tab scroll, reveal, and overflow state into core; add per-item minimum and optional maximum constraints.
- Replace pair-corner splitters with touching-frontier junctions and a private hit graph.
- Make egui consume core records and exact paint resources, then delete `projection.rs`, `TabStripStateMap`, and all duplicate layout and hit logic.
- Add a sequenced native pointer event journal. Final button snapshots alone lose a release followed by a press between samples.
- Implement persistent `ExternalItemKeyMap` before any production Open GPUI graph cutover.

# Next Action

After the acknowledgement lane is green, review and commit the stable authority/protocol baseline. Then implement per-item measurement keys and the complete semantic roster before changing overflow, splitter junctions, or adapters.

# Citations

- [Execution plan](../../../plans/2026-07-18-001-headless-dockspace-refactor-plan.md)
- `crates/dockspace/src/scene.rs`
- `crates/dockspace/src/engine.rs`
- `crates/egui_dockspace/src/dockspace.rs`
- `crates/dockspace_core_protocol/README.md`
