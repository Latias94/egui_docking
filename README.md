# Dockspace

Docking layouts for Rust editor UIs, independent of the UI framework.

Dockspace manages tabs, split layouts, floating groups, docking interactions and
close decisions. Your application owns pane content; your host owns rendering,
input and OS windows. Use the renderer-neutral core with a custom engine, or
start with the egui adapter.

**Pre-release:** the crates are not published yet, and the API may change.
Rust 1.95 or later is required. The egui adapter targets egui 0.37.

## Try it

Run the interactive editor using official egui, winit and WGPU:

```sh
cargo run --manifest-path integration/egui-official-harness/Cargo.toml --features demo --bin official-caller-owned-demo --locked
```

Drag tabs to merge or split panes, resize splitters, and move the floating
Inspector. Edit a document, close it, then use **Restore layout** to reopen it
with its application data intact. This example uses one OS window and needs no
engine or dependency fork.

For a small terminal-only introduction to the core:

```sh
cargo run -p dockspace --example layout_commands --locked
```

[All examples and run commands](docs/EXAMPLES.md)

## Choose an integration

| You want to… | Start here | Host requirements |
| --- | --- | --- |
| Add docking to a custom engine or UI framework | [`dockspace`](crates/dockspace) | Supply measurements, drawing, input and presentation results |
| Use egui in one OS window | [`egui_dockspace`](crates/egui_dockspace) | Own the complete egui run and final output for drag docking |
| Experiment with multiple native windows | [`egui_dockspace_native`](crates/egui_dockspace_native) | Use the pinned eframe/winit forks; platform support varies |

An ordinary `eframe::App::ui` callback supports rendering, selection, close,
menus and keyboard resizing. Drag docking and pane-focus confirmation need the
complete-output host shown above. [Host choices](crates/egui_dockspace#host-choices)
explain the difference before you integrate.

## What you can build

- Editor layouts with tabs, nested splits, splitter junctions and central panes.
- Drag docking with center/edge guides, tab reordering and overflow menus.
- Floating groups within a surface, with move, resize and Dock Back actions.
- Application-controlled close decisions, including background saving.
- Saved layouts with stable pane keys, restored independently of pane data.
- Custom pane drawing, minimum sizes, styling, focus and docking policies.

The experimental native adapter adds OS-window creation, recovery and shared
Root-window layouts. Physical cross-window docking is qualified on a finite
Linux X11 profile; macOS/Windows physical drag-out and mixed-monitor DPI need
further validation. See [platform support](docs/PLATFORM_SUPPORT.md) before
choosing a native integration.

## Use from source

Clone this repository and use path dependencies until the first release. Adjust
the paths to your checkout:

```toml
[dependencies]
dockspace = { path = "../dockspace/crates/dockspace" }
# For egui applications:
egui_dockspace = { path = "../dockspace/crates/egui_dockspace" }
```

Enable `serde` on the crate you use for layout persistence. Pane data and file
storage stay in your application.

## Learn more

- [Examples](docs/EXAMPLES.md): choose a runnable starting point.
- [Core overview](crates/dockspace/README.md) and [engine host guide](crates/dockspace/docs/guide.md).
- [egui overview](crates/egui_dockspace/README.md) and [integration guide](crates/egui_dockspace/docs/guide.md).
- [Documentation index](docs/README.md): platform limits, adapter testing and architecture decisions.
- [Contributing](CONTRIBUTING.md): repository layout and development checks.

## License

`dockspace` uses Apache-2.0. `egui_dockspace` and the experimental native adapter
use MIT OR Apache-2.0. [Third-party sources](THIRD_PARTY.md) records prior art
and fixture provenance.
