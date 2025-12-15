# egui_docking Architecture (Frozen)

This document freezes the current design goals, architecture boundaries, interaction rules, and the development roadmap for `egui_docking`, so we can iterate without drifting UX.

For the previous Chinese version, see `docs/ARCHITECTURE.zh-CN.md`.

## Goals

Bridge `egui_tiles` (dock tree model) with `egui` multi-viewport (multiple native OS windows) to enable an editor-grade workflow:

- Tear off a tab into a native viewport (OS window).
- Drag the OS window across monitors.
- Drag the tab/subtree back into any dock (re-dock).
- Move tabs/subtrees between detached windows (detached ↔ detached).
- Converge on Dear ImGui Docking UX (DockSpaceOverViewport mental model), but keep a Rust/egui-friendly implementation.

## Non-goals (for now)

- 1:1 reproduction of ImGui internals.
- Wayland-first support. Primary targets are macOS + Windows; X11 is a bonus.
- Solving all same-viewport occlusion / input isolation limitations in egui (some constraints exist in how `Area` and z-order interact).

## Repos and crates

- `egui_docking` (this repo)
  - Responsibility: multi-viewport bridge, drag/drop state machine, docking overlay and insertion decisions, detached viewport lifecycle.
  - Main entry: `src/multi_viewport/mod.rs`.
- `repo-ref/egui_tiles_docking` (fork of `egui_tiles`)
  - Responsibility: expose the minimal APIs needed by the bridge (subtree extract/insert, dock-zone query, root-tab drag id, and debug hooks).
  - Policy: keep the library name as `egui_tiles` to stay drop-in.
- `repo-ref/egui` (your fork of egui)
  - Responsibility: provide the missing primitives to make the experience as “unified and predictable” as ImGui (see “egui fork plan” below).

## Core UX principles (what makes ImGui feel good)

1. **Single authority per frame**: during a drag, only one system is allowed to decide the preview and the final insertion (no competing highlights).
2. **Preview = outcome**: if the UI highlights “dock left”, the release must result in “dock left”, not a different fallback insertion.
3. **One window concept**: native viewports and contained floating windows must share the same chrome (frame/title/controls) and the same drag semantics.
4. **Debuggability by copy-paste**: every confusing interaction must be explainable via deterministic, copyable logs.

## Stability invariants (must never be violated)

These invariants define the “closed loop” correctness contract:

1. Tree integrity after any mutation: no unreachable tiles; tabs active must be in children.
2. Never insert a moved subtree into a parent inside itself (no self-parent insertion).
3. Internal dock→dock operations stay in `egui_tiles` (the bridge must not re-apply them).
4. A release is handled at most once (single apply per drag session).
5. Empty detached/floating windows are cleaned deterministically and logged.

For a full parity checklist and acceptance checks, see `docs/IMGUI_PARITY.md`.

## Data model and responsibilities

### Viewports and trees

- Root viewport: `ViewportId::ROOT`.
- Root dock tree: `DockingMultiViewport::tree`.
- Detached (native) viewports: `DockingMultiViewport::detached: BTreeMap<ViewportId, DetachedDock<Pane>>`.
- Contained floating windows (same viewport): `DockingMultiViewport::floating` (Area-based, clipped to the dock rect).

### Cross-viewport payload

Cross-viewport drag uses typed payload via `egui::DragAndDrop`:

- `DockPayload { bridge_id, source_viewport, source_floating, tile_id }`
- `bridge_id` isolates multiple dock instances in the same app.

### Mouse-up may be delivered to the source viewport only

Some backends/platforms can keep pointer capture in the source viewport, so “release” may not reach the target viewport. To guarantee cross-viewport drop:

- `pending_drop: Option<PendingDrop { payload, pointer_global }>`
- Collected on release and applied at the end of the root frame, after all trees have produced layout rects.

## Docking authority and preview policy

### Cross-viewport drop

`egui_docking` is authoritative (tiles can’t handle cross-window drop).

### Same-viewport drop

Tiles are authoritative by default (reorder + nearest-zone docking), except when the user explicitly hovers the docking overlay targets.

Policy:

- If overlay has a valid insertion target, overlay is authoritative (and tiles preview is disabled).
- Otherwise, tiles is authoritative (overlay is hidden).

## Interaction state machine

We treat the whole drag as a session, with explicit arbitration on release:

- `DragSession` owns “who handled the release” to avoid double-apply or contradictory actions.
- “Drop wins over ghost finalize”: if a valid drop handler takes the release, the ghost/floating finalization must not also commit changes.

## Window model: native vs contained

We intentionally support two kinds of “windows”, but with one unified UX:

- **Native viewport window**: real OS window via `Context::show_viewport_immediate`. Required for cross-monitor movement and OS-level window management.
- **Contained floating window**: `Area`-based window inside a viewport. Used for ghost tear-off and tool windows that must stay inside the editor viewport.

Contained floating windows will never fully match OS-level isolation; that’s acceptable. What must match is **chrome + drag semantics + docking behavior**.

## egui fork plan (recommended changes)

To eliminate the “split personality” between `egui::Window` and contained floating windows, we want to reuse the same chrome implementation in both places.

Proposed direction in `repo-ref/egui`:

1. **Extract window chrome into a reusable component**
   - Make the title bar layout/interaction (drag region, close/collapse buttons, header background) reusable outside `egui::Window`.
   - Goal: allow `Area`-based windows to render and behave like `egui::Window` without copy-pasting private code.
2. **Public API for “window-like frame + title bar”**
   - Something like a `WindowChrome`/`TitleBar` builder that can be embedded in custom containers.
3. **Keep `egui::Window` as a convenience wrapper**
   - `Window` remains the main high-level API, but internally uses the extracted chrome module.

These changes are intentionally UX-oriented, not “docking-specific”, so they can be upstreamed later if desired.

## egui_tiles fork plan (minimal surface)

The tiles fork should remain small and focused:

- Subtree extraction/insertion APIs needed for cross-tree movement.
- Dock-zone query and dragged-id APIs needed for predictable previews.
- Debug hooks (optional) guarded by options or `debug_assertions`.

Avoid adding “bridge policy” into tiles; the bridge owns multi-viewport policy.

## Debugging and reproducibility

- Provide a Dock Debug window per viewport.
- Add keyboard shortcuts for copy-to-clipboard logs, because dragging prevents clicking.
- Maintain an integrity pass to detect tree inconsistencies (e.g., Tabs active not in children) and make these failures copyable.

## Testing strategy (current + next)

We prefer tests that validate the mutation algebra without relying on GUI automation:

- Pure logic unit tests (fast, deterministic):
  - `src/multi_viewport/drop_sanitize.rs`
  - `src/multi_viewport/drop_policy.rs`
- Next: “model tests” that generate small trees and sequences of extract/insert operations, asserting the invariants above after every step.

## Roadmap

See `docs/ROADMAP.md` for the prioritized execution plan.

## Refactor plan

See `docs/REFACTOR_PLAN.md` for the stability-first refactor phases and invariant protection strategy.

High-level milestones:

1. **Drag reliability across viewports**: stable pointer feed + deterministic release handling (cross-window drop must never be flaky).
2. **One Window Host abstraction**: unify docked/contained/native into a single host model and state machine.
3. **Live native viewport on drag**: make tear-off “become a new OS window while dragging” (ImGui feel).
4. **Unified chrome everywhere**: reuse egui’s extracted chrome for floating + native title bars.
5. **Polish towards ImGui**: snapping thresholds, overlay hotzones, split ratios, and predictable “outer docking” markers.

## Current Ghost behavior (important for ImGui feel)

Ghost tear-off is enabled by default and is intended to converge on ImGui’s “live” behavior:

- Drag a tab/pane outside the dock area beyond `ghost_tear_off_threshold`.
- A ghost window is created immediately.
- By default, the ghost is spawned as a native viewport window as soon as it leaves the dock area (see `ghost_spawn_native_on_leave_dock`).
- Re-dock by hovering a valid overlay target in any dock surface and releasing.

## Status snapshot

See `docs/STATUS.md` for what is implemented today and what gaps remain to match ImGui.
