# egui_docking Architecture

This document captures the current design goals, architecture boundaries, and interaction rules for `egui_docking`.
It aims to keep UX decisions stable while the implementation evolves toward Dear ImGui-like “Docking + Viewports”.

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
- `egui_tiles_docking` (fork of `egui_tiles`, https://github.com/Latias94/egui_tiles_docking)
  - Responsibility: expose the minimal APIs needed by the bridge (subtree extract/insert, dock-zone query, root-tab drag id, and debug hooks).
  - Policy: keep the library name as `egui_tiles` to stay drop-in.
- `egui` fork (https://github.com/Latias94/egui)
  - Responsibility: provide a few missing primitives to make the experience as “unified and predictable” as ImGui.

This project is experimental and currently intended to be consumed via git dependencies (no crates.io release planned short-term).

Build setup note: keep `egui_docking`, `egui_tiles_docking`, and `egui`/`eframe` on the same fork to avoid duplicate `egui` versions.
If your workspace mixes crates.io and git sources, apply a top-level `[patch.crates-io]` override.

- The `egui` fork is currently required because `egui_docking` uses `egui::containers::window_chrome` (not public in upstream `egui 0.33`).
- The bridge *runtime* is designed to degrade gracefully when fork-only backend hints are absent, but the ImGui-like
  “cross-window drag reliability” still benefits from backend cooperation (see below).

## Core UX principles (what makes ImGui feel good)

1. **Single authority per frame**: during a drag, only one system is allowed to decide the preview and the final insertion (no competing highlights).
2. **Preview = outcome**: if the UI highlights “dock left”, the release must result in “dock left”, not a different fallback insertion.
3. **One window concept**: native viewports and contained floating windows must share the same chrome (frame/title/controls) and the same drag semantics.
4. **Debuggability by copy-paste**: every confusing interaction must be explainable via deterministic, copyable logs.

## Fixed constraints & trade-offs (frozen)

To keep the design closed-loop and avoid drifting UX, we freeze these constraints:

1. **`egui::Window` is not the docking unit**
   - `egui::Window` remains a floating/overlay container (dialogs, popups, inspectors), not something that becomes a docked tab.
   - Docking units are modeled as tiles/panes (and window hosts) managed by `egui_tiles` + `egui_docking`.
2. **Dockable things use a single abstraction**
   - “Dockable/Tool windows” are represented as panes/trees that can be hosted as: docked, contained-floating, or native viewport.
   - We do not try to embed an `egui::Window` as a docked tab; that would create two window systems with conflicting input/z-order semantics.
3. **Client-side decorations (CSD) are a first-class path**
   - Detached native viewports may be undecorated (`detached_viewport_decorations=false`) for an ImGui-like unified feel.
   - When borderless, we prefer **one chrome**: the dock-node tab bar acts as the title bar (move + controls) whenever the detached root is a `Tabs` container.
   - OS decorations remain a supported fallback for platforms/backends where CSD is undesirable or incomplete.
4. **Discoverability wins by default**
   - Single-tab dock nodes keep a visible header/tab bar by default (ImGui baseline).
   - Optional “auto-hide tab bar for single tab” is allowed as a non-default preference.

## Stability invariants (must never be violated)

These invariants define the “closed loop” correctness contract:

1. Tree integrity after any mutation: no unreachable tiles; tabs active must be in children.
2. Never insert a moved subtree into a parent inside itself (no self-parent insertion).
3. Internal dock→dock operations stay in `egui_tiles` (the bridge must not re-apply them).
4. A release is handled at most once (single apply per drag session).
5. Empty detached/floating windows are cleaned deterministically and logged.

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

To make this reliable across overlapping windows (and during OS-native window moves), we also benefit from backend-provided hints:

- **Hovered native viewport**: `egui-winit::mouse_hovered_viewport_id` (ImGui analog: `io.MouseHoveredViewport`).
- **Global pointer position**: `egui-winit::pointer_global_points` (ImGui analog: `io.MousePos`), used when window-local cursor
  positions are stale or never delivered to the target window.

When these hints are missing, we fall back to geometry-only inference (`inner_rect + local pointer`) which is necessarily less robust.

## Docking authority and preview policy

### Cross-viewport drop

`egui_docking` is authoritative (tiles can’t handle cross-window drop).

### Same-viewport drop

Tiles are authoritative by default (reorder + nearest-zone docking), except when the user explicitly hovers the docking overlay targets.

Policy:

- If overlay has a valid insertion target, overlay is authoritative (and tiles preview is disabled).
- Otherwise, tiles is authoritative (overlay is hidden).

### Preview/hit-test rules (to avoid “double highlights”)

- Internal drags (within the same tiles tree): `egui_tiles` paints the default preview. `egui_docking` only paints an overlay when it is going to take over (explicit target hit).
- External drags (cross-host/cross-viewport): `egui_docking` paints the preview.
  - Subtree moves: explicit overlay targets win; otherwise fall back to `dock_zone_at` (default heuristic).
  - Window moves (`payload.tile_id == None`): docking is **explicit-only** (no docking unless a target is hit).
    - ImGui parity: also respects SHIFT gating (`DockingMultiViewportOptions::config_docking_with_shift`):
      - default `false`: holding SHIFT disables docking while moving
      - `true`: holding SHIFT enables docking while moving
    - ImGui explicit target rect: for window-move drags, “tab docking” is only allowed when hovering the target:
      - Tabs tab bar (preferred, supports insertion index), or
      - the top “title band” of a non-Tabs tile (height = `Behavior::tab_bar_height`).
    - Release without a hovered target must be a no-op (keep the window floating); it must not mutate any dock tree.

### Geometry cache (hit-testing must not depend on draw order)

Some targets (contained floating windows) are not part of `egui_tiles` layout and require our own rect tracking.

Rule: every viewport rebuilds its “floating rect cache” *before* any overlay decision / drop resolution runs for that frame.
This keeps hit-testing and preview stable regardless of whether floating windows are drawn before or after the dock tree.

## Interaction state machine

We treat the whole drag as a session, with explicit arbitration on release:

- `DragSession` owns “who handled the release” to avoid double-apply or contradictory actions.
- “Drop wins over ghost finalize”: if a valid drop handler takes the release, the ghost/floating finalization must not also commit changes.

## Window model: native vs contained

We intentionally support two kinds of “windows”, but with one unified UX:

- **Native viewport window**: real OS window via `Context::show_viewport_immediate`. Required for cross-monitor movement and OS-level window management.
- **Contained floating window**: `Area`-based window inside a viewport. Used for ghost tear-off and tool windows that must stay inside the editor viewport.

Contained floating windows will never fully match OS-level isolation; that’s acceptable. What must match is **chrome + drag semantics + docking behavior**.

### Detached viewport drag handle (ImGui-like)

Detached native viewports should not show an extra, second “title label” widget. Instead, we treat the `egui_tiles` tab bar as the drag handle:

- Drag the **tab-bar background** of a detached window to move the native viewport and perform window-move docking (moves the whole dock node, like ImGui when a floating dock node has multiple tabs).
- Drag individual tabs/panes to move/tear-off those panes (subtree drag), as usual.

This keeps the UI consistent: a single, discoverable handle per dock node.

Implementation note: for native window moves we prefer `ViewportCommand::StartDrag` (OS-native window dragging) over driving `ViewportCommand::OuterPosition` each frame.
`OuterPosition` remains useful for "live tear-off" ghost behavior where we must keep a newly spawned window under the cursor immediately.

## DockableWindow / ToolWindow model (recommended)

We treat the editor as a collection of “dockable windows” that can be hosted in three ways:

- **Docked**: rendered as an `egui_tiles` pane within a dock tree (tab/split managed by tiles).
- **Contained floating**: rendered as an `Area`-based floating window inside a viewport (same chrome, clipped).
- **Native viewport**: rendered in a real OS window via `show_viewport_immediate` (OS decorations optional; CSD is supported for ImGui-like unity).

`egui::Window` remains available inside any viewport (including detached ones), but only for overlay UI
that should not participate in docking (dialogs/pickers/context UI).

## Tabs & tab-bar UX (tiles-level, ImGui parity)

Important decision: **tab appearance and interaction lives in `egui_tiles` (`egui_tiles_docking` fork)**, not in `egui_docking`.

Rationale:
- `egui` does not have a first-class “TabBar/TabItem” widget like Dear ImGui.
- `egui_tiles` already owns tabs rendering and exposes customization hooks via `Behavior`.
- If `egui_docking` draws its own tab visuals, we will end up with a third styling system and a fragmented UX.

What we will do (frozen plan):
- Keep `egui_docking` responsible for multi-viewport bridging and drop policy only (overlay decision, host moves, tear-off).
- Make ImGui-like tab behavior achievable by configuring `egui_tiles::Behavior`:
  - title formatting (icon + text)
  - close button visibility + placement (`Behavior::is_tab_closable`, `on_tab_close`, sizes)
  - tab active/hover feedback and “drag-over selects tab” semantics
  - tab bar background as a draggable handle for the parent container (already supported in `egui_tiles`)
  - tab bar padding/spacing/color to match the editor theme
- Default policy (ImGui baseline): every dock node shows a header/tab bar even when it contains a single tab/pane. This keeps the drag handle discoverable and makes window-move docking targets reliable.
- Optional polish (non-default): an “auto-hide tab bar when single tab” toggle (ImGui has `AutoHideTabBar`-like behavior) for users who prefer a Unity-like cleaner layout.
- If we need additional controls (e.g. left-side buttons, dock-node menu), we add them as Behavior hooks in `egui_tiles_docking` (default methods only, no breaking API).

Acceptance criteria (ImGui-like baseline):
- While dragging across windows, the hovered tab target is visually obvious.
- Releasing over a dock node without explicit split target docks as a tab/center by default.
- Split docking only happens when explicitly hovering the split targets (directional intent is unambiguous).

## egui fork (what we rely on today)

The forked `egui` (https://github.com/Latias94/egui) currently provides a few UX-oriented primitives used by `egui_docking`:

- **Reusable window chrome**: `egui::containers::window_chrome` is exposed so contained floating windows and borderless
  detached windows can share the same title-bar layout/controls as `egui::Window`.
- **Backend hover authority**: `egui-winit` stores `mouse_hovered_viewport_id` in `Context::data` on cursor enter/move/leave.
- **Backend global pointer + release fallbacks** (eframe integrations): during active drags, device events maintain a best-effort
  global pointer position, synthesize pointer moves into the best viewport, and force-release pointer buttons when the OS swallows
  mouse-up outside all windows.

These changes are intentionally UX-oriented (not “docking-specific”) and are good candidates for upstreaming once APIs settle.

## Backend hints contract (ImGui-style Platform/IO bridge)

To reach Dear ImGui-like reliability for cross-viewport docking, the backend must provide a few “platform hints” each frame.
`egui_docking` reads these from `Context::data` (temp storage):

- `egui-winit::mouse_hovered_viewport_id` → `ViewportId` (or `Option<ViewportId>`)
  - Equivalent mental model: ImGui `io.MouseHoveredViewport`.
  - Used to decide which viewport should receive the docking preview/overlay during cross-window drags.
- `egui-winit::pointer_global_points` → `Pos2` (or `Option<Pos2>`)
  - Best-effort global pointer position in **points** (desktop/global coordinates).
  - Used as a fallback when some viewports stop receiving `CursorMoved` (e.g. during OS-native window moves).
- `egui-winit::monitors_outer_rects_points` → `Vec<Rect>` (or `Option<Vec<Rect>>`)
  - A list of monitor rectangles in global coordinates, in **points**.
  - Used for best-effort clamping when restoring/saving native viewport window positions.
  - Backend note: for `eframe`/winit, this can be refreshed on each redraw using `ActiveEventLoop::available_monitors()`.

If these hints are absent, `egui_docking` degrades gracefully (it can still dock within a single window), but the “editor-grade”
cross-window experience will be less reliable.

### Monitor work area (known limitation)

Dear ImGui exposes both “main area” and “work area” (`MainPos/MainSize` vs `WorkPos/WorkSize`).
In winit, true work-area insets (taskbar/menu bar) are not consistently available cross-platform, so backends often fall back to
using the full monitor bounds as the work area. This is acceptable for now because our current usage is “window restore clamping”,
not precise taskbar-aware placement.

## CSD notes (ImGui-like unity)

When `detached_viewport_decorations=false`, we aim for “one chrome” per detached native viewport:

- If the detached root is a `Tabs` container, we inject close/min/max controls into the root tab bar (so tab scrolling/layout accounts for the reserved width).
- If the detached root is not `Tabs` (e.g. split-root layouts), we show a small custom title bar above the dock surface with the same controls.
- Double-clicking the detached window’s tab-bar background toggles maximize (best-effort; excludes clicks on tabs and window buttons).

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

## Layout persistence (ImGui .ini-like)

`egui_docking` provides an optional `persistence` feature that saves/loads the *layout* as RON:

- Scope: root dock tree + detached native viewports + contained floating windows (geometry + z-order + collapsed state).
- Important: we **do not** serialize `Pane` values. Instead, the user provides a stable `PaneId` mapping via `PaneRegistry`:
  - `PaneRegistry::pane_id(&Pane) -> PaneId`
  - `PaneRegistry::pane_from_id(PaneId) -> Pane`
- Goal: keep the persistence format easy to diff and hand-edit while we iterate, similar to ImGui’s `.ini`.

Practical note: if your app removes panes over time, you can implement `PaneRegistry::try_pane_from_id` and return `None` for missing ids;
the loader will drop those panes and keep the remaining layout.

This persistence format is versioned and intentionally unstable while the project is experimental.

## Milestones (high-level)

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
