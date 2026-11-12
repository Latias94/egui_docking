# egui_dockspace_native

Experimental native-window hosting for `egui_dockspace`.

This unpublished adapter connects Dockspace to pinned eframe/winit forks. It
adds native window creation and recovery, per-window output reporting and
multiview input integration. The core remains the sole docking-state owner;
your application owns pane content.

For one OS window using official egui, start with
[`egui_dockspace`](../egui_dockspace) and its complete-output editor instead.

## Run the editor

From the repository root:

```sh
cargo run --manifest-path integration/egui-fork-workspace/Cargo.toml -p egui_dockspace_native --example basic --locked
```

Use **Move to New Window** and **Dock Back** to explore the native lifecycle.
Add `--features serde` for application-owned named layout presets. These presets
live in the example process; pane data stays separate from layout documents.

The [platform matrix](../../docs/PLATFORM_SUPPORT.md) distinguishes programmatic
window operations from physical cross-window dragging. Linux X11 has a finite
qualified physical-input profile. macOS/Windows physical drag-out and arbitrary
mixed-monitor DPI are not qualified by that profile.

## Integrate

Construct `NativeDockspaceApp` with a `DockspaceSession`, Root surface, pane
provider and matching `DockStyle`. Install its native host handler in the fork's
eframe options. The [basic example](examples/basic.rs) shows the complete setup.

Pane providers implement `PaneView + Send + 'static` because deferred viewport
callbacks retain them. `Box<dyn PaneView + Send>` is supported. Registry/view
callbacks hold the runtime lock: keep them short, perform I/O outside them, and
submit commands after they return.

Submit `NativeCommand` through `request_command`. Acceptance queues one command;
consume its exact committed result with `take_action_status` before submitting
another. Use the returned close tokens for deferred saving. Apply completed pane
data through `with_panes_mut` and request repaint on the affected egui viewport.

This crate uses the separate [fork workspace](../../integration/egui-fork-workspace/Cargo.toml).
Do not patch the official adapter workspace to run it. Current dependency pins
are maintained in that manifest and lockfile.

Use `NativeDockspaceApp::with_input` with the same `egui_dockspace::DockInputConfig`
as the official adapter. It controls hold-to-enable/disable docking and the
gesture-cancel shortcut; default bindings are Shift and Escape. Queue runtime
changes through `request_input` and consume their result through
`take_action_status`, like other application commands.

`with_child_window_options` and `set_child_window_options` configure managed
Child window decorations, Windows taskbar presence, stacking level and icon.
New windows and replacements share these settings. Placement and visibility
remain coordinated by the native lifecycle; configure the external Root window
through eframe's `NativeOptions::viewport`. Child titles follow selected panes
without adding a library-brand suffix.

[Examples](../../docs/EXAMPLES.md) · [Development](../../CONTRIBUTING.md)
· [Native qualification](../../integration/egui-native-smoke/README.md)

## License

MIT OR Apache-2.0. See [third-party sources](../../THIRD_PARTY.md).
