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
`egui_docking` uses `egui_tiles` types in its public API. Use git dependencies + patch `egui`/`eframe` to the fork (required for now):

```toml
[dependencies]
egui_docking = { git = "https://github.com/Latias94/egui_docking" }
egui_tiles = { package = "egui_tiles_docking", git = "https://github.com/Latias94/egui_tiles_docking", default-features = false }
egui = "0.33"

# If you use eframe:
eframe = { version = "0.33", default-features = false, features = ["default_fonts", "glow", "persistence", "wayland"] }

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

## Example
```sh
cargo run --example multi_viewport_docking
```

## Debugging
- Enable drop target visualization + event log via `DockingMultiViewportOptions { debug_drop_targets: true, ..Default::default() }`.
- For backend/input troubleshooting, run with `RUST_LOG=debug` (the forked `egui` logs when it synthesizes missing mouse-up during drags).

## Tips
- Tear-off: drag a tab/pane and release outside the dock area, or hold `ALT` while releasing to force a new native window.
- Live tear-off (ghost): by default, dragging a tab/pane outside the dock area will immediately spawn a floating "ghost" window that follows the pointer, and can be docked back before release; leaving the native window upgrades it to a new native window (disable via `DockingMultiViewportOptions::ghost_tear_off`).
- Docking: while dragging over a dock, use the overlay targets to choose left/right/top/bottom/center docking; outer edge markers enable dockspace-level splits (dear imgui style outer docking).

## Docs

- `docs/ARCHITECTURE.md`
