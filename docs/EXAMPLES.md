# Examples

Run commands from the repository root with Rust 1.95 or later. Windowed examples
need a desktop session and a working graphics backend.

## Choose a starting point

| Goal | Example | What it demonstrates |
| --- | --- | --- |
| Learn the renderer-neutral core | [Layout commands](../crates/dockspace/examples/layout_commands.rs) | Create a layout, select/open/move panes and inspect committed results; no window |
| Integrate a custom renderer | [Engine host](../integration/dockspace-host-harness/examples/engine_host.rs) | Measure, record and settle output for two logical surfaces; recording renderer |
| Try full docking with official egui | [Workbench](../crates/egui_dockspace/examples/workbench.rs) | Drag docking, splitters, contained Inspector, themes and retained document data |
| Start from an eframe callback | [Basic](../crates/egui_dockspace/examples/basic.rs) | Pane rendering, close decisions and theme/font/density controls; callback capabilities |
| Save before closing a tab | [Background save](../crates/egui_dockspace/examples/background_save.rs) | Background worker, exact deferred token, failure recovery and stale completion |
| Save and restore editor layouts | [Workspace persistence](../crates/egui_dockspace/examples/workspace_persistence.rs) | Presets, closed-pane inventory, missing content and layout locking |
| Explore native OS windows | [Native editor](../crates/egui_dockspace_native/examples/basic.rs) | Experimental multiview, window recovery, policy/style commands and optional presets |

Start with the **workbench** for draggable egui docking. The three
packaged eframe examples use an ordinary `App::ui` callback: selection, close,
menus and keyboard resizing work, while drag docking and pane-focus confirmation
are unavailable because eframe owns the surrounding output. Their purpose is to
teach those application workflows; the complete-output editor demonstrates the
full drag path.

## Renderer-neutral core

```sh
cargo run -p dockspace --example layout_commands --locked
cargo run --manifest-path integration/dockspace-host-harness/Cargo.toml --example engine_host --locked
```

`layout_commands` is the smallest introduction. The independent `engine_host`
adds output ownership and input integration. It uses public core interfaces and
models presentation results without creating windows or submitting GPU work.

## Official egui

Full docking, with official crates.io dependencies:

```sh
cargo run -p egui_dockspace --example workbench --features example-wgpu --locked
```

Try editing text, dragging a tab into another group, resizing a split, moving the
contained Inspector, then closing and restoring a pane. The host integrates
`run_single_surface_with_ui` with winit/WGPU and reports its actual renderer
presentation result. This is a single OS-window example. Its application code
lives in [editor.rs](../crates/egui_dockspace/examples/workbench/editor.rs), separate
from the window loop, renderer and opt-in measurement exercise. The independent
official harness compiles the same example against its own locked dependencies.
Enable **Custom tabs** to try per-tab icons and rounded backgrounds, fixed group
counts and **Tint** buttons. The [tab callbacks](../crates/egui_dockspace/examples/workbench/tabs.rs)
show how to customize chrome while retaining docking input and accessibility.

Smaller eframe examples:

```sh
cargo run -p egui_dockspace --example basic --features example-app --locked
cargo run -p egui_dockspace --example background_save --features example-app --locked
cargo run -p egui_dockspace --example workspace_persistence --features example-app,serde --locked
```

In **background_save**, edit the document and close its tab. The application
freezes editing, saves a new copy in the system temporary directory, then allows
the original close. Failure keeps the pane open. The status strip shows the
destination; the example does not overwrite an existing file.

In **workspace_persistence**, save a named layout, close/reopen panes, lock the
arrangement, and restore the default. Presets live in application memory; pane
content is stored separately from layout documents.

## Experimental native windows

```sh
cargo run --manifest-path integration/egui-fork-workspace/Cargo.toml -p egui_dockspace_native --example basic --locked
```

Add `--features serde` for named JSON presets. The editor starts in one Root OS
window. Use **Move to New Window** to create a native child and **Dock Back** to
return it; the default Child-close policy recovers its content. Programmatic
window commands and physical drag-out have different platform requirements.
Check [platform support](PLATFORM_SUPPORT.md) before testing cross-window input.

This workspace pins eframe/winit forks and remains separate from the official
egui adapter. It does not require changes to the official workspace's dependency
sources.

## Validation tools

These are maintained checks and measurements rather than starter applications:

- [Presentation continuity](../crates/egui_dockspace_native/examples/presentation_continuity.rs): real-renderer tab and geometry continuity.
- [Native qualification](../integration/egui-native-smoke/README.md): isolated Linux X11 input and window-lifecycle drivers.
- [Frame cost](../integration/dockspace-host-harness/examples/frame_cost.rs) and [product comparison](../integration/product-comparison/README.md): measured workloads with explicit scope.

Build/test commands are in [Contributing](../CONTRIBUTING.md). Detailed integration
contracts are indexed in [Documentation](README.md).
