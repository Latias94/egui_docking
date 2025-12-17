# `egui_docking`

Multi-viewport docking for [`egui`](https://github.com/emilk/egui): bridges `egui_tiles` (dock tree/model) with the native multi-viewport API, enabling a complete workflow:

- Tear-off a tab into a native window
- Move across monitors
- Drag tabs (or whole tab-groups) between windows
- Drag back into the main dock

## Status
Experimental / WIP. Targeted at editor-like workflows. No crates.io release planned short-term.

## Forks (required for now)

This project is developed and tested against these forks:

- `egui_tiles_docking`: https://github.com/Latias94/egui_tiles_docking
- `egui` (incl. `egui-winit` / `eframe`): https://github.com/Latias94/egui

The `egui` fork is currently required because `egui_docking` uses `egui::containers::window_chrome` (not public in upstream `egui 0.33`),
and because editor-grade cross-viewport docking benefits from backend-provided input hints/fallbacks.

## Usage
`egui_docking` uses `egui_tiles` types in its public API. Prefer git dependencies for all related crates to keep a single `egui` source:

```toml
[dependencies]
egui_docking = { git = "https://github.com/Latias94/egui_docking" }
egui_tiles = { package = "egui_tiles_docking", git = "https://github.com/Latias94/egui_tiles_docking", default-features = false }
egui = { git = "https://github.com/Latias94/egui", default-features = false }

# If you use eframe:
eframe = { git = "https://github.com/Latias94/egui", default-features = false, features = ["default_fonts", "glow", "persistence", "wayland"] }
```

If your workspace also pulls crates.io `egui`/`eframe` elsewhere, add a top-level `[patch.crates-io]` override to avoid duplicate `egui` versions.

```toml
[patch.crates-io]
egui = { git = "https://github.com/Latias94/egui" }
eframe = { git = "https://github.com/Latias94/egui" }
```

Then, in your app update loop:

```rust
// `docking: egui_docking::DockingMultiViewport<Pane>`
// `behavior: impl egui_tiles::Behavior<Pane>`
docking.ui(ctx, &mut behavior);
```

## Optional: layout persistence (RON)

Enable the `persistence` feature to save/load the docking layout (ImGui `.ini`-like), serialized as RON.

- This does **not** serialize your `Pane` state.
- You provide a `PaneId` mapping (recommended via `PaneRegistry` / `SimplePaneRegistry`).
- If some panes are removed over time, implement `PaneRegistry::try_pane_from_id` and return `None` to drop missing panes on load.
- Snapshot format is experimental and versioned; breaking changes may require regenerating your `.ron`.

```toml
egui_docking = { git = "https://github.com/Latias94/egui_docking", features = ["persistence"] }
```

## Example
```sh
cargo run --example multi_viewport_docking
```

With layout Save/Load buttons:

```sh
cargo run --example multi_viewport_docking --features persistence
```

## Debugging
- Enable drop target visualization + event log via `DockingMultiViewportOptions { debug_drop_targets: true, ..Default::default() }`.
- For backend/input troubleshooting, run with `RUST_LOG=debug` (the forked `egui` logs when it synthesizes missing mouse-up during drags).

## Tips
- Tear-off: drag a tab/pane and release outside the dock area, or hold `ALT` while releasing to force a new native window.
- Live tear-off (ghost): by default, dragging a tab/pane outside the dock area will immediately spawn a floating "ghost" window that follows the pointer, and can be docked back before release; leaving the native window upgrades it to a new native window (disable via `DockingMultiViewportOptions::ghost_tear_off`).
- Docking: while dragging over a dock, use the overlay targets to choose left/right/top/bottom/center docking; outer edge markers enable dockspace-level splits (dear imgui style outer docking).
- CSD (ImGui-like): set `DockingMultiViewportOptions::detached_viewport_decorations = false` and keep `detached_csd_window_controls = true` for client-side close/min/max on detached native windows.

## Docs

- `docs/ARCHITECTURE.md`
