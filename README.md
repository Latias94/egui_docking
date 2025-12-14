# `egui_docking`

Multi-viewport docking for [`egui`](https://github.com/emilk/egui): bridges `egui_tiles` (dock tree/model) with the native multi-viewport API, enabling a complete workflow:

- Tear-off a tab into a native window
- Move across monitors
- Drag tabs (or whole tab-groups) between windows
- Drag back into the main dock

## Status
Experimental / WIP. Targeted at editor-like workflows.

## Usage
`egui_docking` uses `egui_tiles` types in its public API. If you want the multi-viewport bridge features, use the forked tiles crate:

```toml
[dependencies]
egui = "0.33"
egui_docking = "0.1"
egui_tiles = { package = "egui_tiles_docking", version = "0.14" }
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
